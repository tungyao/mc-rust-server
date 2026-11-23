//! mc-server: Bevy App 组装 + 50ms 主循环 + 网络线程，只兼容 26.3/777
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use bevy_app::{App, Startup, Update};
use bevy_ecs::prelude::*;
use crossbeam_channel::{Receiver, unbounded};
use tracing::{error, info, warn};

mod play;

#[derive(Debug, Clone, Resource)]
struct ServerConfig {
    bind: String,
    motd: String,
    max_players: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:25565".to_string(),
            motd: "Rust MC 26.3 (777) self-made".to_string(),
            max_players: 100,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct TickCounter(pub u64);

#[derive(Resource, Clone, Debug, Default)]
struct PlayersOnline(pub Arc<AtomicUsize>);

#[derive(Resource)]
struct NetReceiver(pub Receiver<mc_network::NewConnection>);

fn tick_system(mut counter: ResMut<TickCounter>) {
    counter.0 += 1;
    if counter.0 % 100 == 0 {
        info!("tick {}", counter.0);
    }
}

/// 排空网络线程推过来的 NewConnection，计数 + log。
/// 只做 try_recv，不做任何 IO，满足“tick 里不做 IO”。
/// 计数器与网络线程共享同一 Arc：网络侧 Status 回包通过它读到真实在线数。
/// 任务F：同时把 Login 占位计数搬给 PlayScaffold，触发 "ready to join"。
fn handle_new_connections(
    rx: Res<NetReceiver>,
    online: Res<PlayersOnline>,
    mut play: ResMut<play::PlayScaffold>,
) {
    while let Ok(conn) = rx.0.try_recv() {
        let n = online.0.fetch_add(1, Ordering::SeqCst) + 1;
        info!("new connection {} (online={})", conn.0, n);
        // Login 成功占位：先用 NewConnection 触发，后续 P3 换真实 Login 事件。
        play.pending_login += 1;
    }
}

fn startup_system(cfg: Res<ServerConfig>) {
    info!(
        "starting mc-server {} protocol={} data={} bind={} motd={} max_players={}",
        mc_core::MC_VERSION,
        mc_core::PROTOCOL_VERSION,
        mc_core::DATA_VERSION,
        cfg.bind,
        cfg.motd,
        cfg.max_players,
    );
    // 只兼容 26.3，启动时断言一次。
    debug_assert_eq!(mc_core::PROTOCOL_VERSION, 777);
}

/// 网络线程入口：独立 tokio runtime，监听 + serve，Ctrl-C 时置 shutdown 标志。
fn run_net_thread(
    bind: String,
    tx: crossbeam_channel::Sender<mc_network::NewConnection>,
    shutdown: Arc<AtomicBool>,
    net_config: mc_network::NetConfig,
) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("mc-net")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!("tokio runtime build failed: {e}");
            shutdown.store(true, Ordering::SeqCst);
            return;
        }
    };

    rt.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                error!("bind {bind} failed: {e}");
                shutdown.store(true, Ordering::SeqCst);
                return;
            }
        };

        // Ctrl-C 监听：收到后只置标志，由主循环优雅退出，不 panic。
        let sd = Arc::clone(&shutdown);
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    info!("ctrl-c received, shutting down...");
                    sd.store(true, Ordering::SeqCst);
                }
                Err(e) => {
                    error!("ctrl-c watcher failed: {e}");
                }
            }
        });

        // serve 只调用 mc-network，不碰包格式；NetConfig 由 ServerConfig 透传，
        // Status 回包用真实 motd/max_players/online，Status ping 不 push、不计数。
        if let Err(e) = mc_network::serve(listener, tx, net_config).await {
            error!("network serve exited: {e}");
        }
        // serve 意外返回也触发关闭，让主循环退出。
        shutdown.store(true, Ordering::SeqCst);
    });
}

fn main() {
    tracing_subscriber::fmt::init();

    let cfg = ServerConfig::default();
    let bind = cfg.bind.clone();
    // ServerConfig 透传进网络线程：motd/max_players + 与 ECS 共享的在线计数器。
    let online = PlayersOnline::default();
    let net_config =
        mc_network::NetConfig::new(cfg.max_players, cfg.motd.clone(), Arc::clone(&online.0));

    let (tx, rx) = unbounded::<mc_network::NewConnection>();

    let mut app = App::new();
    app.insert_resource(cfg);
    app.insert_resource(TickCounter::default());
    app.insert_resource(online);
    app.insert_resource(NetReceiver(rx));
    app.insert_resource(play::PlayScaffold::default());
    app.add_systems(Startup, startup_system);
    // tick 计数 + 连接排空都在 Update 里，每次 app.update() 跑一次 = 1 tick。
    // Play 脚手架：login 占位 → ready to join + 每秒 KeepAlive 计数（只 log，TODO 接 socket 写）。
    app.add_systems(
        Update,
        (
            tick_system,
            handle_new_connections,
            play::login_join_system,
            play::keepalive_system,
        )
            .chain(),
    );

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_net = Arc::clone(&shutdown);
    std::thread::spawn(move || run_net_thread(bind, tx, shutdown_net, net_config));

    // 主线程固定 50ms tick，不用 bevy_internal 的调度计时，自己 Instant+sleep。
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
