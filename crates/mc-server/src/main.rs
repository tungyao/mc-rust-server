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
use resources::{NetReceiver, PlayEventReceiver, PlayersOnline, SocketEcs, TickCounter};
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
    // P53：四项玩法开关（生产默认全开，server.toml 可单独关闭）。
    let features = config::load_feature_toggles("server.toml");
    info!(
        "p53 features xp_merge={} hunger_detail={} settings_persist={} stats={}",
        features.xp_merge, features.hunger_detail, features.settings_persist, features.stats
    );
    info!(
        "p54 features broadcast_throttle={} journal_trim={}/{} weather_broadcast={} craft_persist={}",
        features.broadcast_throttle, features.journal_trim, features.journal_keep,
        features.weather_broadcast, features.craft_session_persist
    );
    // P52：socket 归属启动期锁定（默认 ecs，server.toml 可回退 net）。
    let socket_mode = config::load_socket_mode("server.toml");
    info!("socket_mode={} (ecs=full ECS ownership, net=emergency fallback)", socket_mode.as_str());
    admin::set_socket_mode(socket_mode);
    let online = PlayersOnline::default();
    let (play_tx, play_rx) = unbounded::<mc_network::PlayEvent>();
    // P36.2：启动时按 seed 打开 redb（必要时一次性迁移旧档），把 blocks 注入内存热层。
    // 显式 legacy 配置仍走原 P17/P18 稀疏档；redb 打开失败不静默回退（避免绕过新格式）。
    let storage = config::load_world_storage("server.toml");
    let generator_version = mc_world::GeneratorVersion::from_number(
        config::load_generator_version("server.toml"),
    )
    .unwrap_or(mc_world::GeneratorVersion::V1);
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
            // P44：存档若由其它生成器版本写入则拒绝启动，绝不静默混用基底。
            if let Err(e) = store.ensure_generator_version(generator_version.number()) {
                warn!(
                    "world generator version mismatch dir={world_dir} seed={} ({e}), exiting; \
                     use a new seed or set generator_version back to the stored value",
                    cfg.seed
                );
                std::process::exit(1);
            }
            let loaded = match mc_world::MutableWorld::with_store_and_version(
                cfg.seed,
                Arc::clone(&store),
                generator_version,
            ) {
                Ok(w) => w,
                Err(e) => {
                    warn!("redb world load failed dir={world_dir} seed={} ({e}), exiting", cfg.seed);
                    std::process::exit(1);
                }
            };
            (Arc::new(loaded), Some(store))
        }
        config::WorldStorage::Legacy => {
            let loaded = mc_world::MutableWorld::load_from_dir_with_version(
                world_path,
                cfg.seed,
                generator_version,
            )
            .unwrap_or_else(|e| {
                warn!("world load failed dir={world_dir} seed={} ({e}), starting fresh", cfg.seed);
                mc_world::MutableWorld::new_with_version(cfg.seed, generator_version)
            });
            (Arc::new(loaded), None)
        }
    };
    // P51：玩家存档 username 键 → 稳定 UUID 键一次性迁移（失败已内部回滚；
    // 即使跳过，DropState 仍按 UUID 主键读、缺档回退旧用户名键，不会丢背包/XP）。
    match mc_world::uuid_migration::migrate_saves(world_path, cfg.seed, shared_store.as_deref()) {
        Ok(report) => info!(
            "uuid migration seed={} ran={} files={:?} redb_keys={:?} skipped={}",
            cfg.seed, report.ran, report.files, report.redb_keys, report.skipped
        ),
        Err(e) => warn!("uuid migration failed dir={world_dir} seed={} ({e}); legacy keys remain readable", cfg.seed),
    }
    info!(
        "world loaded storage={storage:?} generator={:?} dir={world_dir} seed={} overrides={} dirty={}",
        generator_version,
        cfg.seed,
        mutable.override_count(),
        mutable.dirty_len()
    );
    // 多连接共享同一份已编码 ChunkData 缓存（seed 必须与 cfg.seed 一致）。
    // P17 起缓存绑定可变世界：编辑后失效对应 chunk，下次重编即含新方块。
    let chunk_cache = Arc::new(
        mc_network::ChunkCache::new_with_version(
            cfg.seed,
            mc_network::DEFAULT_CHUNK_CACHE_CAPACITY,
            generator_version,
        )
        .with_mutable(Arc::clone(&mutable)),
    );
    // P18 掉落物实体 + 背包：net 线程共享状态（存档与 overrides 同目录、seed 隔离）。
    // P36.2：redb 模式下背包/XP 读写也走同一个 `WorldStore`。
    // P36.3/P39：with_store 在 redb 模式从 meta.tick_offset 恢复全局 tick 基线，
    // 并从 timers 重建作物/树苗/树叶观察的原 planted_tick 与 stage；`serve`
    // 的 adopt_existing_* 会按这些记录续算而非从 0 重计时。legacy 模式不变。
    let drop_state = std::sync::Arc::new(mc_network::DropState::with_store(
        world_dir.clone().into(),
        cfg.seed,
        shared_store.clone(),
    ));
    // P54 A6：全局时间/天气从 meta 恢复（缺省白天/晴），net ticker 与 admin 共享同一 Arc。
    let world_time = Arc::new(mc_world::world_time::WorldTimeState::from_store(
        shared_store.as_deref(), cfg.seed,
    ));
    // P25 管理面板：与游戏线程共享的 hub（在线映射唯一来源，计划 §5.2 推荐方案）。
    let hub = Arc::new(mc_network::BroadcastHub::new());
    // P46 流体：状态由 serve 挂载全服唯一 ticker（仅 V2 世界真正传播）。
    let fluid = Arc::new(mc_network::FluidState::new());
    let net_config = mc_network::NetConfig::new(cfg.max_players, cfg.motd.clone(), Arc::clone(&online.0))
        .with_view_distance(cfg.view_distance)
        .with_seed(cfg.seed)
        .with_compression_threshold(cfg.compression_threshold)
        .with_play_events(play_tx)
        .with_chunk_cache(chunk_cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drop_state))
        .with_hub(Arc::clone(&hub))
        .with_fluid(Arc::clone(&fluid))
        // P43：生产入口开启玩家实体互见（旧入口/既有 e2e 仍走默认关闭）。
        .with_player_visibility(true)
        // P47：生产入口开启挖掘进度 0x05 广播（旧 e2e 默认关闭，包流不变）。
        .with_destroy_stage(true)
        // P47：生产入口开启工具等级/时运/精准采集（旧 e2e 默认关闭，掉落口径不变）。
        .with_tool_rules(true)
        // P52：生产默认 ecs；net 仅在 server.toml 显式配置时作为紧急回退。
        .with_socket_mode(socket_mode)
        // P53：生产默认全开；旧 e2e 入口的 NetConfig::default() 仍全关（包流不变）。
        .with_xp_merge(features.xp_merge)
        .with_hunger_detail(features.hunger_detail)
        .with_settings_persist(features.settings_persist)
        .with_stats(features.stats)
        // P54：四项独立开关（A3 默认关；A5/A6 时间/A8 默认开；天气广播默认关）。
        .with_broadcast_throttle(features.broadcast_throttle)
        .with_journal_trim(features.journal_trim)
        .with_journal_keep(features.journal_keep)
        .with_world_time(Arc::clone(&world_time))
        .with_weather_broadcast(features.weather_broadcast)
        .with_craft_session_persist(features.craft_session_persist);
    info!(
        "config view_distance={} effective_view_radius={} seed={} compression_threshold={}",
        cfg.view_distance, cfg.effective_view_distance(), cfg.seed, cfg.compression_threshold
    );
    let (tx, rx) = unbounded::<mc_network::NewConnection>();
    // P52 方案 B：mc-net runtime 的 Handle 交给 ECS 资源；ECS 在该 runtime 上 spawn
    // 每连接的 reader/writer/logic 任务并持有句柄/队列。net_thread 只跑 accept。
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("mc-net")
        .build()
        .expect("tokio runtime build failed");
    let socket_ecs = mc_network::SocketEcsRuntime::new(rt.handle().clone(), net_config.clone(), tx.clone());
    let net_config = net_config.with_socket_accept(socket_ecs.incoming_sender());
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
    app.insert_resource(SocketEcs(socket_ecs));
    app.insert_resource(play::PlayScaffold::default());
    app.insert_resource(movement::MovementQueue::default());
    app.insert_resource(movement::PendingPlayerRemoval::default());
    app.insert_resource(persist::SharedWorld(Arc::clone(&mutable)));
    app.insert_resource(persist::WorldSaveDir(world_dir.clone().into()));
    app.insert_resource(persist::SharedStore(shared_store.clone()));
    app.insert_resource(persist::JournalTrimConfig {
        enabled: features.journal_trim,
        keep: features.journal_keep,
    });
    app.insert_resource(persist::SharedDropState(Arc::clone(&drop_state)));
    app.add_systems(Startup, systems::startup_system);
    app.add_systems(
        Update,
        (
            systems::tick_system,
            // P52：连接登记先于 NewConnection 消费；只 try_recv + spawn，不阻塞 Update。
            systems::socket_ecs_system,
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
    std::thread::spawn(move || net_thread::run_net_thread(bind, tx, shutdown_net, net_config, rt));
    // P25 管理面板：独立线程 + 独立监听（默认 127.0.0.1:25566），故障隔离：
    // admin 线程崩了/端口被占都不影响 25565 游戏线程。
    // P54 后台联动：启动期注入共享 store/时间状态给 admin 线程（不给 AdminState 加字段）。
    admin::set_admin_extras(
        shared_store.clone(),
        Arc::clone(&world_time),
        features.journal_trim,
        features.journal_keep,
        features.craft_session_persist,
    );
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
