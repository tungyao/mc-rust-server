//! net_thread: 独立tokio runtime + serve + Ctrl-C，只兼容26.3
use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};
use tracing::{error, info};

/// 网络线程入口：监听 + serve，Ctrl-C置shutdown标志。
pub fn run_net_thread(
    bind: String,
    tx: crossbeam_channel::Sender<mc_network::NewConnection>,
    shutdown: Arc<AtomicBool>,
    net_config: mc_network::NetConfig,
    rt: tokio::runtime::Runtime,
) {
    // P52：runtime 由 main 创建并把 Handle 交给 ECS SocketEcsRuntime；
    // 本线程只跑 accept 循环，连接任务的 spawn/句柄/写队列归 ECS 资源。
    rt.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                error!("bind {bind} failed: {e}");
                shutdown.store(true, Ordering::SeqCst);
                return;
            }
        };
        let sd = Arc::clone(&shutdown);
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    info!("ctrl-c received, shutting down...");
                    sd.store(true, Ordering::SeqCst);
                }
                Err(e) => error!("ctrl-c watcher failed: {e}"),
            }
        });
        if let Err(e) = mc_network::serve(listener, tx, net_config).await {
            error!("network serve exited: {e}");
        }
        shutdown.store(true, Ordering::SeqCst);
    });
}
