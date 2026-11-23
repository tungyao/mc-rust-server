//! mc-network: tokio 监听 + 包收发，只兼容26.3/777
//! 模块上限200行：config/framing/handshake_route/status_route/login_route
pub mod config;
pub mod framing;
pub mod handshake_route;
pub mod login_route;
pub mod status_route;

pub use config::{NetConfig, STATUS_MAX_PLAYERS, STATUS_MOTD};
pub use framing::{MAX_PACKET_LEN, READ_TIMEOUT};
pub use login_route::{enter_play, notify_login_success};

use crossbeam_channel::Sender;
use tokio::net::TcpListener;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct NewConnection(pub std::net::SocketAddr);

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub async fn serve(listener: TcpListener, tx: Sender<NewConnection>, config: NetConfig) -> Result<(), NetError> {
    info!("listening on {:?}", listener.local_addr()?);
    loop {
        let (socket, addr) = listener.accept().await?;
        info!("accept {addr}");
        let tx = tx.clone();
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(e) = handshake_route::handle_connection(socket, addr, tx, config).await {
                debug!("connection {addr} closed: {e}");
            }
        });
    }
}
