//! mc-server: Bevy App组装 + 50ms主循环，只兼容26.3/777
//! 模块：config/resources/systems/net_thread/play/movement
mod config;
mod movement;
mod net_thread;
mod play;
mod resources;
mod systems;

use bevy_app::{App, Startup, Update};
use bevy_ecs::schedule::IntoSystemConfigs;
use crossbeam_channel::unbounded;
use resources::{NetReceiver, PlayEventReceiver, PlayersOnline, TickCounter};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use tracing::{info, warn};

fn main() {
    tracing_subscriber::fmt::init();
    let cfg = match config::ServerConfig::load_or_create("server.toml") {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!("server.toml load failed ({e}), falling back to defaults");
            config::ServerConfig::default()
        }
    };
    let bind = cfg.bind.clone();
    let online = PlayersOnline::default();
    let (play_tx, play_rx) = unbounded::<mc_network::PlayEvent>();
    // 多连接共享同一份已编码 ChunkData 缓存（seed 必须与 cfg.seed 一致）。
    let chunk_cache = Arc::new(mc_network::ChunkCache::new(
        cfg.seed,
        mc_network::DEFAULT_CHUNK_CACHE_CAPACITY,
    ));
    let net_config = mc_network::NetConfig::new(cfg.max_players, cfg.motd.clone(), Arc::clone(&online.0))
        .with_view_distance(cfg.view_distance)
        .with_seed(cfg.seed)
        .with_compression_threshold(cfg.compression_threshold)
        .with_play_events(play_tx)
        .with_chunk_cache(chunk_cache);
    info!(
        "config view_distance={} effective_view_radius={} seed={} compression_threshold={}",
        cfg.view_distance, cfg.effective_view_distance(), cfg.seed, cfg.compression_threshold
    );
    let (tx, rx) = unbounded::<mc_network::NewConnection>();
    let mut app = App::new();
    app.insert_resource(cfg);
    app.insert_resource(TickCounter::default());
    app.insert_resource(online);
    app.insert_resource(NetReceiver(rx));
    app.insert_resource(PlayEventReceiver(play_rx));
    app.insert_resource(play::PlayScaffold::default());
    app.insert_resource(movement::MovementQueue::default());
    app.insert_resource(movement::PendingPlayerRemoval::default());
    app.add_systems(Startup, systems::startup_system);
    app.add_systems(
        Update,
        (
            systems::tick_system,
            systems::handle_new_connections,
            movement::movement_ingest_system,
            play::login_join_system,
            movement::movement_system,
            play::keepalive_system,
            movement::keepalive_timeout_system,
            // 唯一删除点：Disconnect 与 timeout 都在这里去重后 despawn + online 饱和减。
            movement::player_removal_system,
        )
            .chain(),
    );
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_net = Arc::clone(&shutdown);
    std::thread::spawn(move || net_thread::run_net_thread(bind, tx, shutdown_net, net_config));
    let tick_dur = Duration::from_millis(mc_core::TICK_MS);
    info!("main loop tick={}ms (26.3 only)", mc_core::TICK_MS);
    loop {
        if shutdown.load(Ordering::SeqCst) {
            info!("saving and exiting...");
            break;
        }
        let start = Instant::now();
        app.update();
        let elapsed = start.elapsed();
        if elapsed < tick_dur {
            std::thread::sleep(tick_dur - elapsed);
        } else {
            warn!("tick lag: {:?} > {:?}", elapsed, tick_dur);
        }
    }
    info!("mc-server stopped, ticks={}", app.world().get_resource::<TickCounter>().map(|c| c.0).unwrap_or(0));
}
