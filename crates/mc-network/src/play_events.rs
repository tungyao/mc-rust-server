//! play_events: net 线程 → ECS 的 Play 期事件（移动/KeepAlive），只兼容 26.3/777。
//! 设计：socket 仍留在 net 线程（读包/写包/超时都在 tokio 任务里），只把 ECS 需要的
//! 语义事件经 crossbeam channel 投递，避免把可运行链路拆成半截。
use mc_protocol::packets::C2SMovement;
use std::net::SocketAddr;

/// Play 期从 net 线程投递给 ECS 的事件。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayEvent {
    /// C2S KeepAlive 回显：ECS 只更新 `PlayerKeepAlive.last_seen`，不做在线数增减。
    KeepAlive { addr: SocketAddr },
    /// C2S 移动包（0x1E-0x21）：ECS 转换为 `MovementUpdate` 排入 `MovementQueue`。
    Movement { addr: SocketAddr, movement: C2SMovement },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_event_is_copy_and_carries_addr() {
        let addr: SocketAddr = "127.0.0.1:25565".parse().unwrap();
        let ev = PlayEvent::Movement {
            addr,
            movement: C2SMovement::Flags(mc_protocol::packets::MovePlayerFlags {
                on_ground: true,
                horizontal_collision: false,
            }),
        };
        let copy = ev;
        assert_eq!(ev, copy);
        match ev {
            PlayEvent::Movement { addr: a, .. } => assert_eq!(a, addr),
            _ => panic!("wrong variant"),
        }
    }
}
