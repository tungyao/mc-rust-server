//! systems: tick + 连接排空 + 启动log
use super::play::PlayScaffold;
use super::resources::{NetReceiver, PlayersOnline, TickCounter};
use super::config::ServerConfig;
use super::movement::{PlayerAddr, PlayerKeepAlive, Position};
use bevy_ecs::prelude::*;
use std::sync::atomic::Ordering;
use tracing::info;

pub fn tick_system(mut counter: ResMut<TickCounter>) {
    counter.0 += 1;
    if counter.0 % 100 == 0 {
        info!("tick {}", counter.0);
    }
}

/// 排空网络线程的 NewConnection，只 try_recv不做IO；按到达顺序压入 Join 队列，
/// 同时为每个玩家 spawn ECS 实体（Position/PlayerKeepAlive/PlayerAddr）：
/// 移动事件按 addr 找到实体后写 `MovementQueue`，KeepAlive 回显刷新 `last_seen`。
/// 在线数只认这里；Join 真下发在 net 线程 enter_play 已做，ECS 侧 log 见 login_join_system。
pub fn handle_new_connections(
    mut commands: Commands,
    rx: Res<NetReceiver>,
    online: Res<PlayersOnline>,
    mut play: ResMut<PlayScaffold>,
) {
    while let Ok(conn) = rx.0.try_recv() {
        let n = online.0.fetch_add(1, Ordering::SeqCst) + 1;
        let ent = commands
            .spawn((PlayerAddr(conn.0), Position::default(), PlayerKeepAlive::default()))
            .id();
        info!("new connection {} (online={}) entity={ent:?}", conn.0, n);
        // TODO(socket→ECS)：NewConnection 仍只带 addr；socket 句柄留在 net 线程，
        // 真下发继续走 enter_play。此处只维护 ECS 侧玩家状态，不动已可运行的发送链路。
        play.pending_login.push(conn.0);
    }
}

pub fn startup_system(cfg: Res<ServerConfig>) {
    info!(
        "starting mc-server {} protocol={} data={} bind={} motd={} max_players={}",
        mc_core::MC_VERSION, mc_core::PROTOCOL_VERSION, mc_core::DATA_VERSION,
        cfg.bind, cfg.motd, cfg.max_players,
    );
    debug_assert_eq!(mc_core::PROTOCOL_VERSION, 777);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::NetReceiver;

    #[test]
    fn new_connection_spawns_one_player_entity_and_counts_online_once() {
        let mut world = World::new();
        let (tx, rx) = crossbeam_channel::unbounded();
        world.insert_resource(NetReceiver(rx));
        world.insert_resource(PlayersOnline::default());
        world.insert_resource(PlayScaffold::default());
        let addr: std::net::SocketAddr = "127.0.0.1:41000".parse().unwrap();
        tx.send(mc_network::NewConnection(addr)).unwrap();
        let mut sched = Schedule::default();
        sched.add_systems(handle_new_connections);
        sched.run(&mut world);
        sched.run(&mut world);
        let online = world.get_resource::<PlayersOnline>().unwrap();
        assert_eq!(online.0.load(Ordering::SeqCst), 1, "one NewConnection must count once");
        let mut q = world.query::<(Entity, &PlayerAddr)>();
        let players: Vec<_> = q.iter(&world).collect();
        assert_eq!(players.len(), 1, "one ECS player entity");
        assert_eq!(players[0].1.0, addr);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.pending_login.len(), 1);
        assert_eq!(play.pending_login[0], addr);
    }
}
