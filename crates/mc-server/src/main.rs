//! mc-server: Bevy App组装 + 50ms主循环，只兼容26.3/777
//! 模块：config/resources/systems/net_thread/play/movement/persist/admin
mod admin;
mod admin_items;
mod config;
mod movement;
mod net_thread;
mod persist;
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
    // P36.2：启动时按 seed 打开 redb（必要时一次性迁移旧档），把 blocks 注入内存热层。
    // 显式 legacy 配置仍走原 P17/P18 稀疏档；redb 打开失败不静默回退（避免绕过新格式）。
    let storage = config::load_world_storage("server.toml");
    let world_dir = cfg.world_dir.clone();
    let world_path = std::path::Path::new(&world_dir);
    let (mutable, shared_store): (
        Arc<mc_world::MutableWorld>,
        Option<Arc<mc_world::store::WorldStore>>,
    ) = match storage {
        config::WorldStorage::Redb => {
            let (store, report) = match mc_world::store::WorldStore::open_with_migration(world_path, cfg.seed) {
                Ok(v) => v,
                Err(e) => {
                    warn!("redb world open/migration failed dir={world_dir} seed={} ({e}), exiting", cfg.seed);
                    std::process::exit(1);
                }
            };
            info!(
                "redb world opened seed={} blocks={} migrated_players={} renamed={} legacy_ignored={}",
                cfg.seed, report.blocks, report.players.len(), report.renamed, report.legacy_ignored
            );
            let store = Arc::new(store);
            let loaded = match mc_world::MutableWorld::with_store(cfg.seed, Arc::clone(&store)) {
                Ok(w) => w,
                Err(e) => {
                    warn!("redb world load failed dir={world_dir} seed={} ({e}), exiting", cfg.seed);
                    std::process::exit(1);
                }
            };
            (Arc::new(loaded), Some(store))
        }
        config::WorldStorage::Legacy => {
            let loaded = mc_world::MutableWorld::load_from_dir(world_path, cfg.seed).unwrap_or_else(|e| {
                warn!("world load failed dir={world_dir} seed={} ({e}), starting fresh", cfg.seed);
                mc_world::MutableWorld::new(cfg.seed)
            });
            (Arc::new(loaded), None)
        }
    };
    info!(
        "world loaded storage={storage:?} dir={world_dir} seed={} overrides={} dirty={}",
        cfg.seed,
        mutable.override_count(),
        mutable.dirty_len()
    );
    // 多连接共享同一份已编码 ChunkData 缓存（seed 必须与 cfg.seed 一致）。
    // P17 起缓存绑定可变世界：编辑后失效对应 chunk，下次重编即含新方块。
    let chunk_cache = Arc::new(
        mc_network::ChunkCache::new(cfg.seed, mc_network::DEFAULT_CHUNK_CACHE_CAPACITY)
            .with_mutable(Arc::clone(&mutable)),
    );
    // P18 掉落物实体 + 背包：net 线程共享状态（存档与 overrides 同目录、seed 隔离）。
    // P36.2：redb 模式下背包/XP 读写也走同一个 `WorldStore`。
    let drop_state = std::sync::Arc::new(mc_network::DropState::with_store(
        world_dir.clone().into(),
        cfg.seed,
        shared_store.clone(),
    ));
    // P25 管理面板：与游戏线程共享的 hub（在线映射唯一来源，计划 §5.2 推荐方案）。
    let hub = Arc::new(mc_network::BroadcastHub::new());
    let net_config = mc_network::NetConfig::new(cfg.max_players, cfg.motd.clone(), Arc::clone(&online.0))
        .with_view_distance(cfg.view_distance)
        .with_seed(cfg.seed)
        .with_compression_threshold(cfg.compression_threshold)
        .with_play_events(play_tx)
        .with_chunk_cache(chunk_cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drop_state))
        .with_hub(Arc::clone(&hub));
    info!(
        "config view_distance={} effective_view_radius={} seed={} compression_threshold={}",
        cfg.view_distance, cfg.effective_view_distance(), cfg.seed, cfg.compression_threshold
    );
    let (tx, rx) = unbounded::<mc_network::NewConnection>();
    let mut app = App::new();
    // P25：cfg 进 Resource 前先把管理面板要的字段拷出来（`cfg` move 后不可再借）。
    let admin_bind = cfg.admin_bind.clone();
    let admin_seed = cfg.seed;
    let admin_view_distance = cfg.view_distance;
    app.insert_resource(cfg);
    app.insert_resource(TickCounter::default());
    app.insert_resource(online);
    app.insert_resource(NetReceiver(rx));
    app.insert_resource(PlayEventReceiver(play_rx));
    app.insert_resource(play::PlayScaffold::default());
    app.insert_resource(movement::MovementQueue::default());
    app.insert_resource(movement::PendingPlayerRemoval::default());
    app.insert_resource(persist::SharedWorld(Arc::clone(&mutable)));
    app.insert_resource(persist::WorldSaveDir(world_dir.clone().into()));
    app.insert_resource(persist::SharedStore(shared_store));
    app.insert_resource(persist::SharedDropState(Arc::clone(&drop_state)));
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
            // P17：脏 chunk 刷盘（60s 周期 / 断线即刷；退出时 main 全量刷）。
            // 注意顺序：必须在 player_removal 之前，断开事件由 ingest 登记后、
            // 唯一删除点 drain 之前才能看到非空 pending。
            persist::autosave_system,
            // 唯一删除点：Disconnect 与 timeout 都在这里去重后 despawn + online 饱和减。
            movement::player_removal_system,
        )
            .chain(),
    );
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_net = Arc::clone(&shutdown);
    std::thread::spawn(move || net_thread::run_net_thread(bind, tx, shutdown_net, net_config));
    // P25 管理面板：独立线程 + 独立监听（默认 127.0.0.1:25566），故障隔离：
    // admin 线程崩了/端口被占都不影响 25565 游戏线程。
    let admin_state = admin::AdminState {
        hub: Arc::clone(&hub),
        drops: Arc::clone(&drop_state),
        mutable: Arc::clone(&mutable),
        seed: admin_seed,
        world_dir: world_dir.clone().into(),
        view_distance: admin_view_distance,
    };
    let shutdown_admin = Arc::clone(&shutdown);
    std::thread::Builder::new()
        .name("mc-admin".to_string())
        .spawn(move || admin::serve_admin(&admin_bind, admin_state, shutdown_admin))
        .expect("spawn mc-admin thread");
    let tick_dur = Duration::from_millis(mc_core::TICK_MS);
    info!("main loop tick={}ms (26.3 only)", mc_core::TICK_MS);
    loop {
        if shutdown.load(Ordering::SeqCst) {
            info!("saving and exiting...");
            // 优雅退出刷盘（autosave 只管周期/断线，退出走这里全量刷）。
            if mutable.has_store() {
                drop_state.save_all_inventories();
                let tick = app.world().get_resource::<TickCounter>().map(|c| c.0).unwrap_or(0);
                match mutable.flush_to_store(tick) {
                    Ok(n) => info!("world saved on exit to redb edits={n}"),
                    Err(e) => warn!("world save on exit to redb failed ({e})"),
                }
            } else if let Err(e) = mutable.save_to_dir(std::path::Path::new(&world_dir)) {
                warn!("world save on exit failed dir={world_dir} ({e})");
            }
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
