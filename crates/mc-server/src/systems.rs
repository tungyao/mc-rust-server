//! systems: tick + 连接排空 + 启动log
use super::play::PlayScaffold;
use super::resources::{NetReceiver, PlayersOnline, TickCounter};
use super::config::ServerConfig;
use bevy_ecs::prelude::*;
use std::sync::atomic::Ordering;
use tracing::info;

pub fn tick_system(mut counter: ResMut<TickCounter>) {
    counter.0 += 1;
    if counter.0 % 100 == 0 {
        info!("tick {}", counter.0);
    }
}

/// 排空网络线程的 NewConnection，只 try_recv不做IO。
pub fn handle_new_connections(rx: Res<NetReceiver>, online: Res<PlayersOnline>, mut play: ResMut<PlayScaffold>) {
    while let Ok(conn) = rx.0.try_recv() {
        let n = online.0.fetch_add(1, Ordering::SeqCst) + 1;
        info!("new connection {} (online={})", conn.0, n);
        play.pending_login += 1;
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
