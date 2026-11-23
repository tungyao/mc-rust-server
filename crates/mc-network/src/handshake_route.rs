//! handshake_route: 首包Handshake + 777校验 + 分流 Status/Login
use super::config::NetConfig;
use super::framing::{read_packet, to_io_invalid};
use super::login_route::handle_login_configured;
use super::status_route::handle_status;
use super::NewConnection;
use crossbeam_channel::Sender;
use mc_protocol::packets::{Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID};
use std::io;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{debug, warn};

/// 首包必须是 Handshake 0x00；只认 26.3/777；Status用&mut，Login拿走所有权。
pub async fn handle_connection(mut stream: TcpStream, addr: SocketAddr, tx: Sender<NewConnection>, config: NetConfig) -> io::Result<()> {
    let (packet_id, payload) = read_packet(&mut stream).await?;
    if packet_id != HANDSHAKE_PACKET_ID {
        warn!("{addr} expected handshake 0x00, got {packet_id:#04x}, disconnect");
        return Ok(());
    }
    let (hs, _) = Handshake::decode_body(&payload).map_err(to_io_invalid)?;
    if let Err(e) = mc_core::check_protocol(hs.protocol_version) {
        warn!("{addr} unsupported protocol {} ({e}), disconnect", hs.protocol_version);
        return Ok(());
    }
    match hs.next_state {
        HandshakeNextState::Status => {
            debug!("{addr} status handshake (server={}:{})", hs.server_address, hs.server_port);
            handle_status(&mut stream, addr, &config).await?;
        }
        HandshakeNextState::Login => {
            debug!("{addr} login handshake (server={}:{})", hs.server_address, hs.server_port);
            handle_login_configured(
                stream,
                addr,
                &tx,
                config.compression_threshold,
                config.seed,
                config.play_events.clone(),
            )
            .await?;
        }
    }
    Ok(())
}
