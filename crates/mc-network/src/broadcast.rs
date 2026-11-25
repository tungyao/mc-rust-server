//! broadcast: per-player 写通道 hub（P21 多人同服可见），只兼容 26.3/777。
//!
//! 设计（HANDOFF §5.1 的 per-player 写通道方向）：socket 仍留在 net 线程、每个
//! `play_loop` 独占自己的 `TcpStream`、ECS 依然不写 socket。新增一个全服共享的
//! `BroadcastHub`（`conn_id → 发送端 + 位置/视距快照`）：
//!
//! - A 的权威判定线程（仍是 A 自己的 `play_loop`）把 `BlockUpdate` / 掉落
//!   `Spawn`+`Data` / `Remove` / `TakeItem` 按原始包体投递给**视距内其它连接**的
//!   队列；各连接的 `play_loop` 在 tick 间隙排空自己队列写入自己 socket。
//! - `Ack` 只回操作者本人，绝不广播（客户端序列预测是 per-connection 的）。
//! - 视距用**接收方**的 `last_pos` + `radius`（切比雪夫，含 `ChunkView` 同口径）；
//!   跨区块移动后 `update_pos` 即时更新，广播集自然跟随，无需等节流 flush。
//! - 锁粒度：单把 `RwLock<HashMap>` 只做注册/注销/位置更新与接收集快照；
//!   广播先在读锁内拷贝目标发送端，丢锁后再 `try_send`（无界队列，永不阻塞
//!   判定线程）；发送失败（对端已走）只跳过，真正的清理在 `enter_play_with_options`
//!   退出路径与 `Disconnect` 唯一删除点汇合（`unregister`），不留幽灵通道。
//!
//! 不同步（MVP 外，留 TODO）：挖掘动画 `Set Block Destroy Stage 0x05`；玩家实体
//! 互相可见（已有 `PlayerConnId` 实体但无位置广播，保持不动）。

use mc_core::ChunkPos;
use std::collections::HashMap;
use std::sync::RwLock;

/// 排队给其它连接 `play_loop` 代写的原始 S2C 包（包 ID + 未压缩包体；
///
/// 接收方用自己的 `threshold` 经 `write_packet_any` 写出）。
#[derive(Debug, Clone)]
pub struct QueuedPacket {
    pub id: i32,
    pub body: Vec<u8>,
}

struct HubConn {
    tx: crossbeam_channel::Sender<QueuedPacket>,
    radius: i32,
    pos: (f64, f64, f64),
}

/// 全服共享的广播 hub（`Arc` 进 `NetConfig`/`PlayOptions`）。
pub struct BroadcastHub {
    conns: RwLock<HashMap<u64, HubConn>>,
}

impl std::fmt::Debug for BroadcastHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BroadcastHub").field("len", &self.len()).finish()
    }
}

impl BroadcastHub {
    pub fn new() -> Self {
        Self { conns: RwLock::new(HashMap::new()) }
    }

    /// 当前在线连接数（测试/诊断用）。
    pub fn len(&self) -> usize {
        self.conns.read().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub fn contains(&self, conn_id: u64) -> bool {
        self.conns.read().unwrap_or_else(|p| p.into_inner()).contains_key(&conn_id)
    }

    /// 注册连接的写队列（Join 序列完成后、进 `play_loop` 前调用，保证接收方
    /// 已持有完整视距区块，广播包不会排在区块包前面）。
    pub fn register(
        &self,
        conn_id: u64,
        tx: crossbeam_channel::Sender<QueuedPacket>,
        radius: i32,
        pos: (f64, f64, f64),
    ) {
        self.conns
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(conn_id, HubConn { tx, radius, pos });
    }

    /// 注销连接（`enter_play_with_options` 退出路径，与 `Disconnect` 投递同点，
    /// 覆盖所有退出分支；重复注销无害）。
    pub fn unregister(&self, conn_id: u64) {
        self.conns.write().unwrap_or_else(|p| p.into_inner()).remove(&conn_id);
    }

    /// 更新接收视距快照（每个移动包后调用；只改本连接条目，写锁极短）。
    pub fn update_pos(&self, conn_id: u64, pos: (f64, f64, f64)) {
        if let Some(c) = self.conns.write().unwrap_or_else(|p| p.into_inner()).get_mut(&conn_id) {
            c.pos = pos;
        }
    }

    /// 事件所在区块视距内的其它连接（快照；调用方再逐个 `unicast` 并维护
    /// `DropState::known`，避免本文件依赖掉落状态）。
    pub fn recipients_in_view(&self, except: u64, chunk: ChunkPos) -> Vec<u64> {
        self.conns
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|(id, c)| {
                **id != except && chunk_in_view(chunk_of_pos(c.pos), c.radius, chunk)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// 向单个连接排队（对端已注销/队列异常只返回 `false`，调用方跳过）。
    pub fn unicast(&self, conn_id: u64, id: i32, body: Vec<u8>) -> bool {
        let tx = self
            .conns
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&conn_id)
            .map(|c| c.tx.clone());
        match tx {
            Some(tx) => tx.try_send(QueuedPacket { id, body }).is_ok(),
            None => false,
        }
    }

    /// 向事件区块视距内的其它连接各排队一份（`except` 本人不收；`Ack` 不要走这里）。
    pub fn broadcast(&self, except: u64, chunk: ChunkPos, id: i32, body: &[u8]) {
        // 读锁内只拷贝发送端，丢锁后再发送：判定线程永不阻塞，锁粒度最小。
        let targets: Vec<crossbeam_channel::Sender<QueuedPacket>> = {
            let guard = self.conns.read().unwrap_or_else(|p| p.into_inner());
            guard
                .iter()
                .filter(|(cid, c)| **cid != except && chunk_in_view(chunk_of_pos(c.pos), c.radius, chunk))
                .map(|(_, c)| c.tx.clone())
                .collect()
        };
        for tx in targets {
            let _ = tx.try_send(QueuedPacket { id, body: body.to_vec() });
        }
    }
}

fn chunk_of_pos(pos: (f64, f64, f64)) -> ChunkPos {
    // 与 `ChunkView::chunk_of_block` 同口径（floor + div_euclid），非有限坐标钳制。
    let bx = pos.0.floor().clamp(-30_000_000.0, 30_000_000.0) as i32;
    let bz = pos.2.floor().clamp(-30_000_000.0, 30_000_000.0) as i32;
    ChunkPos(bx.div_euclid(16), bz.div_euclid(16))
}

fn chunk_in_view(center: ChunkPos, radius: i32, chunk: ChunkPos) -> bool {
    (chunk.0 - center.0).abs() <= radius && (chunk.1 - center.1).abs() <= radius
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub_with(a_pos: (f64, f64, f64), b_pos: (f64, f64, f64)) -> (BroadcastHub, crossbeam_channel::Receiver<QueuedPacket>, crossbeam_channel::Receiver<QueuedPacket>) {
        let hub = BroadcastHub::new();
        let (atx, arx) = crossbeam_channel::unbounded();
        let (btx, brx) = crossbeam_channel::unbounded();
        hub.register(1, atx, 2, a_pos);
        hub.register(2, btx, 2, b_pos);
        (hub, arx, brx)
    }

    #[test]
    fn broadcast_reaches_viewer_in_range_but_not_far_connection() {
        // A(1) 与 B(2) 同在出生区块；事件也在出生区块。
        let (hub, arx, brx) = hub_with((369.5, 73.0, 381.5), (373.5, 73.0, 381.5));
        // 远端 C 在 100 格外（半径 2 视距外）。
        let (ctx, crx) = crossbeam_channel::unbounded();
        hub.register(3, ctx, 2, (600.5, 73.0, 700.5));
        assert_eq!(hub.len(), 3);
        let chunk = ChunkPos(23, 23);
        hub.broadcast(1, chunk, 0x08, &[1, 2, 3]);
        // A（操作者）不收；B 收到；C 收不到。
        assert!(arx.try_recv().is_err());
        let got = brx.try_recv().unwrap();
        assert_eq!((got.id, got.body), (0x08, vec![1, 2, 3]));
        assert!(crx.try_recv().is_err());
    }

    #[test]
    fn update_pos_moves_broadcast_set_and_unregister_clears() {
        let (hub, _arx, brx) = hub_with((369.5, 73.0, 381.5), (373.5, 73.0, 381.5));
        let chunk = ChunkPos(23, 23);
        // B 走远（同 C 位置）后不再收到出生区块广播。
        hub.update_pos(2, (600.5, 73.0, 700.5));
        assert_eq!(hub.recipients_in_view(1, chunk), Vec::<u64>::new());
        // B 走回后恢复。
        hub.update_pos(2, (373.5, 73.0, 381.5));
        assert_eq!(hub.recipients_in_view(1, chunk), vec![2]);
        hub.broadcast(1, chunk, 0x08, &[]);
        assert!(brx.try_recv().is_ok());
        // 注销后幽灵通道不留存：unicast 失败，recipients 为空。
        hub.unregister(2);
        assert!(!hub.contains(2));
        assert!(hub.contains(1));
        assert_eq!(hub.len(), 1);
        assert!(!hub.unicast(2, 0x08, vec![]));
        hub.unregister(2);
    }
}
