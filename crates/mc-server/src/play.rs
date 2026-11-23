//! Play 脚手架：Login 队列 → "ready to join" + 初始视距 + 每秒 KeepAlive 计数
//! 包 ID 取 `packets::PLAY_*` 现有常量，已按P4A对照表核对777。
//! P5b 只做视距：spawn 中心 radius=4（81 块），每 tick 只发 1 块（限流防 tick lag）。
use bevy_ecs::prelude::*;
use std::collections::VecDeque;
use std::net::SocketAddr;
use tracing::info;

use crate::TickCounter;

/// 初始视距半径（切比雪夫）：`(2*4+1)^2=81` 块，只兼容 26.3 脚手架值。
pub const VIEW_RADIUS: i32 = 4;
/// `VIEW_RADIUS=4` 对应的总块数。
pub const VIEW_CHUNK_COUNT: usize = 81;
/// 每 tick 最多下发块数：限流防 tick lag（81 块需 81 tick ≈ 4s 发完）。
pub const CHUNKS_PER_TICK: usize = 1;

/// Play 脚手架状态：net 线程交来的登录排队，P3 接真实 Player 实体。
/// P3后半 TODO：`NewConnection` 目前只带 addr（profile/socket 句柄留在 net 线程，
/// 真下发走 `mc_network::login_route::enter_play`）；要把 socket 从 net 线程交到 ECS，
/// 需扩展 channel 信封携带 `PlayerProfile` + socket 句柄（或 tokio mpsc 发包通道），
/// 届时本队列升级为 `Vec<PlayerProfile>`，真下发从 net 侧移到 ECS 侧。
#[derive(Resource, Default, Debug)]
pub struct PlayScaffold {
    /// channel 收到的 NewConnection 按到达顺序排队（替代 P2 的 u64 计数器）。
    pub pending_login: Vec<SocketAddr>,
    /// 已 log "ready to join" 的数量。
    pub joined: u64,
    /// KeepAlive 自增序号（S2C payload）。
    pub keepalive_seq: i64,
    /// 视距待发队列（P5b）：`login_join_system` 按 spawn 中心 radius=4 入队 81 块，
    /// 每 tick 只 pop 1 块发 ChunkData。共享队列脚手架；P5c 按 Player 实体拆分。
    pub pending_chunks: VecDeque<mc_core::ChunkPos>,
    /// 已组包（log 占位）的 ChunkData 数量。
    pub sent_chunks: u64,
    /// 当前视距中心（spawn 区块）；多玩家同中心时去重参考，P5c 改 per-player。
    pub view_center: Option<mc_core::ChunkPos>,
}

/// 出生点所在区块（视距中心）：`SPAWN_POS(8,80,8)` → chunk `(0,0)`。
pub fn spawn_center_chunk() -> mc_core::ChunkPos {
    let spawn = mc_world::SPAWN_POS;
    mc_core::ChunkPos(spawn.0.div_euclid(16), spawn.2.div_euclid(16))
}

/// Login 成功队列排空 + 初始视距入队 + 每 tick 限流 1 块。
/// 调用链（main.rs）：`handle_new_connections`（收 NewConnection 入 `pending_login`）
/// → 本系统（join + 视距入队 + 发 1 块）。包 ID 已按P4A对照表核对777。
/// TODO(P5c)：socket 句柄交接进 ECS 后，在此真写 Join Game / Set Center Chunk /
/// Chunk Data / Player Position Sync；视距队列按 Player 实体拆分，本处共享队列仅脚手架。
pub fn login_join_system(mut play: ResMut<PlayScaffold>) {
    // 1) 新登录：逐个触发一次 Join 下发（当前为 log，socket 真发包在 net 线程已做），
    //    并按 spawn 中心 radius=4 触发初始视距（81 块入队）。
    if !play.pending_login.is_empty() {
        let batch = std::mem::take(&mut play.pending_login);
        for addr in batch {
            play.joined += 1;
            let center = spawn_center_chunk();
            // SetCenter 编码自检（log 占位，真写待 socket 进 ECS）。
            let mut center_buf = Vec::new();
            mc_protocol::packets::encode_set_center_chunk_packet(
                &mc_protocol::packets::SetCenterChunk {
                    chunk_x: center.0,
                    chunk_z: center.1,
                },
                &mut center_buf,
            );
            // 初始视距：spawn 中心 radius=4，共 81 块（中心优先排序，见 world::view_chunks）。
            let view = mc_world::view_chunks(center, VIEW_RADIUS);
            debug_assert_eq!(view.len(), VIEW_CHUNK_COUNT);
            play.view_center = Some(center);
            play.pending_chunks.extend(view);
            let spawn = mc_world::SPAWN_POS;
            info!(
                "ready to join #{} addr={} spawn=({},{},{}) center=({},{}) view_enqueued={} center_bytes={} (join已由net线程enter_play真下发; TODO socket句柄进ECS后在此真写 Join 0x32 + Chunk 0x2E + Position Sync 0x49)",
                play.joined,
                addr,
                spawn.0,
                spawn.1,
                spawn.2,
                center.0,
                center.1,
                play.pending_chunks.len(),
                center_buf.len()
            );
        }
    }
    // 2) 限流下发：每 tick 只发 1 块（防 tick lag），用 chunk_network_bytes 占位组 ChunkData。
    for _ in 0..CHUNKS_PER_TICK {
        let Some(pos) = play.pending_chunks.pop_front() else {
            break;
        };
        let chunk = mc_world::chunk_for(pos);
        let section_bytes = mc_world::chunk_network_bytes(&chunk);
        let data = mc_protocol::packets::PlayChunkData {
            chunk_x: pos.0,
            chunk_z: pos.1,
            heightmaps_nbt: mc_protocol::packets::minimal_empty_nbt_bytes(),
            section_bytes,
        };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_play_chunk_data_packet(&data, &mut buf);
        play.sent_chunks += 1;
        info!(
            "chunk data ({},{}) bytes={} sent={} pending={} (TODO: 接socket真写, ChunkData 0x2E 已按P4A对照表核对777)",
            pos.0,
            pos.1,
            buf.len(),
            play.sent_chunks,
            play.pending_chunks.len()
        );
    }
}

/// 每秒（20 tick）一次 KeepAlive 计数：只 log 不真发，留 TODO 接 socket 写。
/// 用 TickCounter 判断整秒，避免在 tick 里做 IO。
pub fn keepalive_system(tick: Res<TickCounter>, mut play: ResMut<PlayScaffold>) {
    if tick.0 == 0 || tick.0 % 20 != 0 {
        return;
    }
    play.keepalive_seq += 1;
    // 脚手架 encode 自检：确保包能编码，但不发送。
    let mut buf = Vec::new();
    mc_protocol::packets::encode_keepalive_s2c_packet(play.keepalive_seq, &mut buf);
    info!(
        "keepalive seq={} tick={} bytes={} (TODO: 接socket写, s2c 0x2D/c2s 0x1C 已按P4A对照表核对777)",
        play.keepalive_seq,
        tick.0,
        buf.len()
    );
    // TODO(P3): 遍历在线 Player 的 socket 写 KeepAlive S2C，并处理 C2S 超时踢出。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_login_is_a_real_queue() {
        // 真实队列：两个不同 addr 入队 → 一次 drain 触发两次 join，不丢身份。
        let play = PlayScaffold {
            pending_login: vec![
                "127.0.0.1:10001".parse().unwrap(),
                "127.0.0.1:10002".parse().unwrap(),
            ],
            ..Default::default()
        };
        let mut world = World::new();
        world.insert_resource(play);
        let mut sched = Schedule::default();
        sched.add_systems(login_join_system);
        sched.run(&mut world);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        // 2 个 join 入队 2*81=162 块，本 tick 限流只发 1 块
        assert_eq!(play.joined, 2);
        assert!(play.pending_login.is_empty());
        assert_eq!(play.sent_chunks, 1);
        assert_eq!(play.pending_chunks.len(), 2 * VIEW_CHUNK_COUNT - 1);
        // 空队列再跑一次：joined 不变，但限流继续发 1 块
        sched.run(&mut world);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.joined, 2);
        assert_eq!(play.sent_chunks, 2);
        assert_eq!(play.pending_chunks.len(), 2 * VIEW_CHUNK_COUNT - 2);
    }

    #[test]
    fn initial_view_is_81_around_spawn_one_per_tick() {
        // 单玩家：spawn 中心 radius=4 共 81 块，每 tick 只发 1 块
        let center = spawn_center_chunk();
        assert_eq!(center, mc_core::ChunkPos(0, 0));
        let play = PlayScaffold {
            pending_login: vec!["127.0.0.1:20001".parse().unwrap()],
            ..Default::default()
        };
        let mut world = World::new();
        world.insert_resource(play);
        let mut sched = Schedule::default();
        sched.add_systems(login_join_system);
        sched.run(&mut world);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.joined, 1);
        assert_eq!(play.view_center, Some(center));
        assert_eq!(play.sent_chunks, 1);
        assert_eq!(play.pending_chunks.len(), VIEW_CHUNK_COUNT - 1);
        // 后续 tick 无新登录也继续发，每次恰好 1 块
        for expect_sent in 2..=5 {
            sched.run(&mut world);
            let play = world.get_resource::<PlayScaffold>().unwrap();
            assert_eq!(play.sent_chunks, expect_sent);
            assert_eq!(play.pending_chunks.len(), VIEW_CHUNK_COUNT - expect_sent as usize);
        }
    }

    #[test]
    fn empty_queue_sends_nothing() {
        let mut world = World::new();
        world.insert_resource(PlayScaffold::default());
        let mut sched = Schedule::default();
        sched.add_systems(login_join_system);
        sched.run(&mut world);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.joined, 0);
        assert_eq!(play.sent_chunks, 0);
        assert!(play.pending_chunks.is_empty());
    }
    #[test]
    fn scaffold_encode_does_not_panic() {
        // Join Game + 中心 chunk + 空 chunk + 出生同步 + KeepAlive 全走一遍 encode
        let login = mc_protocol::packets::LoginPlay::default();
        let mut buf = Vec::new();
        mc_protocol::packets::encode_login_play_packet(&login, &mut buf);
        assert!(!buf.is_empty());

        let center = mc_protocol::packets::SetCenterChunk { chunk_x: 0, chunk_z: 0 };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_set_center_chunk_packet(&center, &mut buf);
        assert!(!buf.is_empty());

        let chunk = mc_world::chunk_for(mc_core::ChunkPos(0, 0));
        let section_bytes = mc_world::chunk_network_bytes(&chunk);
        let data = mc_protocol::packets::PlayChunkData {
            chunk_x: 0,
            chunk_z: 0,
            heightmaps_nbt: mc_protocol::packets::minimal_empty_nbt_bytes(),
            section_bytes,
        };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_play_chunk_data_packet(&data, &mut buf);
        assert!(!buf.is_empty());

        // Unload 编码自检（0x26 forget_level_chunk，不是 KeepAlive）
        let mut buf = Vec::new();
        mc_protocol::packets::encode_unload_chunk_packet(
            &mc_protocol::packets::UnloadChunk { chunk_x: 0, chunk_z: 0 },
            &mut buf,
        );
        assert!(!buf.is_empty());

        let spawn = mc_world::SPAWN_POS;
        let sync = mc_protocol::packets::PlayerPositionSync {
            x: spawn.0 as f64 + 0.5,
            y: spawn.1 as f64,
            z: spawn.2 as f64 + 0.5,
            yaw: 0.0,
            pitch: 0.0,
            flags: 0,
            teleport_id: 1,
        };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_player_position_sync_packet(&sync, &mut buf);
        assert!(!buf.is_empty());

        let mut buf = Vec::new();
        mc_protocol::packets::encode_keepalive_s2c_packet(1, &mut buf);
        assert!(!buf.is_empty());
    }
}
