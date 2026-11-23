//! play_events: net 线程 → ECS 的 Play 期事件（移动/KeepAlive/断线），只兼容 26.3/777。
//! 设计：socket 仍留在 net 线程（读包/写包/超时都在 tokio 任务里），只把 ECS 需要的
//! 语义事件经 crossbeam channel 投递，避免把可运行链路拆成半截。
//! 实体键用 accept 时生成的 `conn_id`（单调 u64），`addr` 只用于日志：同一源 addr
//! 快速重连时，旧连接的迟到 Disconnect 不会误删新实体。
use mc_protocol::packets::C2SMovement;
use std::net::SocketAddr;

/// Play 期从 net 线程投递给 ECS 的事件。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayEvent {
    /// C2S KeepAlive 回显：ECS 只更新 `PlayerKeepAlive.last_seen`，不做在线数增减。
    KeepAlive { addr: SocketAddr, conn_id: u64 },
    /// C2S 移动包（0x1E-0x21）：ECS 转换为 `MovementUpdate` 排入 `MovementQueue`。
    Movement { addr: SocketAddr, conn_id: u64, movement: C2SMovement },
    /// 连接结束（正常 EOF/读错误/写失败/KeepAlive 超时/Join 序列失败），只发一次。
    /// ECS 按 `conn_id` 找到实体后立即 despawn；与 15s timeout 去重，online 只减一次。
    Disconnect { addr: SocketAddr, conn_id: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_event_is_copy_and_carries_addr_and_conn_id() {
        let addr: SocketAddr = "127.0.0.1:25565".parse().unwrap();
        let ev = PlayEvent::Movement {
            addr,
            conn_id: 9,
            movement: C2SMovement::Flags(mc_protocol::packets::MovePlayerFlags {
                on_ground: true,
                horizontal_collision: false,
            }),
        };
        let copy = ev;
        assert_eq!(ev, copy);
        match ev {
            PlayEvent::Movement { addr: a, conn_id, .. } => {
                assert_eq!(a, addr);
                assert_eq!(conn_id, 9);
            }
            _ => panic!("wrong variant"),
        }
        let dc = PlayEvent::Disconnect { addr, conn_id: 10 };
        match dc {
            PlayEvent::Disconnect { addr: a, conn_id } => {
                assert_eq!(a, addr);
                assert_eq!(conn_id, 10);
            }
            _ => panic!("wrong variant"),
        }
    }
}
