//! systems: tick + 连接排空 + 启动log
use super::play::PlayScaffold;
use super::resources::{NetReceiver, PlayersOnline, TickCounter};
use super::config::ServerConfig;
use super::movement::{PlayerAddr, PlayerConnId, PlayerKeepAlive, Position};
use bevy_ecs::prelude::*;
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use tracing::info;

pub fn tick_system(mut counter: ResMut<TickCounter>) {
    counter.0 += 1;
    if counter.0 % 100 == 0 {
        info!("tick {}", counter.0);
    }
}

/// 排空网络线程的 NewConnection，只 try_recv不做IO；按到达顺序压入 Join 队列，
/// 同时为每个玩家 spawn ECS 实体（Position/PlayerKeepAlive/PlayerAddr/PlayerConnId）：
/// 移动/心跳/断线事件按 conn_id 找到实体，`PlayerAddr` 只用于日志。
/// 在线数只在这里 `fetch_add` 一次；`Disconnect`/timeout 由 `player_removal_system`
/// 的唯一删除点 `fetch_sub` 一次。
pub fn handle_new_connections(
    mut commands: Commands,
    cfg: Res<ServerConfig>,
    rx: Res<NetReceiver>,
    online: Res<PlayersOnline>,
    existing: Query<&PlayerConnId>,
    mut play: ResMut<PlayScaffold>,
) {
    // 同 tick 内重复 conn_id 也幂等：query 里已有 + 本 tick 新 spawn 的都记入 seen。
    let mut seen: HashSet<u64> = existing.iter().map(|c| c.0).collect();
    while let Ok(conn) = rx.0.try_recv() {
        if !seen.insert(conn.conn_id) {
            info!(
                "duplicate NewConnection addr={} conn_id={} ignored (entity already exists)",
                conn.addr, conn.conn_id
            );
            continue;
        }
        let n = online.0.fetch_add(1, Ordering::SeqCst) + 1;
        let pos = Position::at_spawn(cfg.seed);
        let ent = commands
            .spawn((
                PlayerAddr(conn.addr),
                PlayerConnId(conn.conn_id),
                pos,
                PlayerKeepAlive::default(),
            ))
            .id();
        info!(
            "new connection addr={} conn_id={} (online={}) entity={ent:?} seed={} spawn_y={:.1}",
            conn.addr, conn.conn_id, n, cfg.seed, pos.y
        );
        // TODO(socket→ECS)：socket 句柄留在 net 线程，真下发继续走 enter_play。
        // 此处只维护 ECS 侧玩家状态，不动已可运行的发送链路。
        play.pending_login.push(conn);
    }
}

pub fn startup_system(cfg: Res<ServerConfig>) {
    info!(
        "starting mc-server {} protocol={} data={} bind={} motd={} max_players={} view_distance={} effective_view_radius={}",
        mc_core::MC_VERSION, mc_core::PROTOCOL_VERSION, mc_core::DATA_VERSION,
        cfg.bind, cfg.motd, cfg.max_players, cfg.view_distance, cfg.effective_view_distance(),
    );
    debug_assert_eq!(mc_core::PROTOCOL_VERSION, 777);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::NetReceiver;
    use mc_network::NewConnection;

    #[test]
    fn new_connection_spawns_one_player_entity_and_counts_online_once() {
        let mut world = World::new();
        let (tx, rx) = crossbeam_channel::unbounded();
        let cfg = ServerConfig { seed: 987654321, ..Default::default() };
        let expected_y = mc_world::WorldGen::new(cfg.seed).spawn_pos().1 as f64;
        world.insert_resource(cfg);
        world.insert_resource(NetReceiver(rx));
        world.insert_resource(PlayersOnline::default());
        world.insert_resource(PlayScaffold::default());
        let addr: std::net::SocketAddr = "127.0.0.1:41000".parse().unwrap();
        tx.send(NewConnection { addr, conn_id: 1 }).unwrap();
        let mut sched = Schedule::default();
        sched.add_systems(handle_new_connections);
        sched.run(&mut world);
        sched.run(&mut world);
        let online = world.get_resource::<PlayersOnline>().unwrap();
        assert_eq!(online.0.load(Ordering::SeqCst), 1, "one NewConnection must count once");
        let mut q = world.query::<(Entity, &PlayerAddr, &PlayerConnId, &Position)>();
        let players: Vec<_> = q.iter(&world).collect();
        assert_eq!(players.len(), 1, "one ECS player entity");
        assert_eq!(players[0].1.0, addr);
        assert_eq!(players[0].2 .0, 1);
        assert_eq!(players[0].3.y, expected_y, "ECS Position.y must use ServerConfig.seed");
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.pending_login.len(), 1);
        assert_eq!(play.pending_login[0].addr, addr);
        assert_eq!(play.pending_login[0].conn_id, 1);
    }

    #[test]
    fn duplicate_conn_id_is_idempotent() {
        let mut world = World::new();
        let (tx, rx) = crossbeam_channel::unbounded();
        world.insert_resource(ServerConfig::default());
        world.insert_resource(NetReceiver(rx));
        world.insert_resource(PlayersOnline::default());
        world.insert_resource(PlayScaffold::default());
        let addr: std::net::SocketAddr = "127.0.0.1:41001".parse().unwrap();
        // 同一 conn_id 在同一 tick 重复投递：只 spawn 一个实体、online 只加一次。
        tx.send(NewConnection { addr, conn_id: 42 }).unwrap();
        tx.send(NewConnection { addr, conn_id: 42 }).unwrap();
        let mut sched = Schedule::default();
        sched.add_systems(handle_new_connections);
        sched.run(&mut world);
        // 下一 tick 再投一次：query 已能看到实体，仍幂等。
        tx.send(NewConnection { addr, conn_id: 42 }).unwrap();
        sched.run(&mut world);
        assert_eq!(
            world.get_resource::<PlayersOnline>().unwrap().0.load(Ordering::SeqCst),
            1
        );
        let mut q = world.query::<&PlayerConnId>();
        assert_eq!(q.iter(&world).count(), 1);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.pending_login.len(), 1, "duplicate must not join twice");
    }
}
