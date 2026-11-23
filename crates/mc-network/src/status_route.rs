//! status_route: Status Request→Response→Ping→Pong，随后断开，不计数
use super::config::NetConfig;
use super::framing::{read_packet, to_io_invalid, write_packet};
use mc_protocol::packets::{self, PING_PACKET_ID, STATUS_REQUEST_PACKET_ID, STATUS_RESPONSE_PACKET_ID};
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use tokio::net::TcpStream;
use tracing::{debug, warn};
use std::io;

/// Status 流程：Request → Response → (可选) Ping → Pong，随后由客户端断开。
pub async fn handle_status(stream: &mut TcpStream, addr: SocketAddr, config: &NetConfig) -> io::Result<()> {
    let (id, payload) = read_packet(stream).await?;
    if id != STATUS_REQUEST_PACKET_ID {
        warn!("{addr} status: expected request 0x00, got {id:#04x}, disconnect");
        return Ok(());
    }
    if !payload.is_empty() {
        debug!("{addr} status: request has {} trailing bytes, ignored", payload.len());
    }
    packets::decode_status_request(&payload).map_err(to_io_invalid)?;
    let json = packets::status_json(config.max_players, config.online_count.load(Ordering::SeqCst), &config.motd);
    let mut payload = Vec::new();
    packets::encode_string(&json, &mut payload);
    write_packet(stream, STATUS_RESPONSE_PACKET_ID, &payload).await?;
    debug!("{addr} status response sent ({} json bytes)", json.len());
    match read_packet(stream).await {
        Ok((PING_PACKET_ID, p)) => {
            let v = packets::decode_ping_request(&p).map_err(to_io_invalid)?;
            let mut body = Vec::new();
            packets::write_i64_be(v, &mut body);
            write_packet(stream, mc_protocol::packets::PONG_PACKET_ID, &body).await?;
            debug!("{addr} pong sent ({v})");
        }
        Ok((other, _)) => warn!("{addr} status: expected ping 0x01, got {other:#04x}, disconnect"),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => debug!("{addr} closed after status response"),
        Err(e) => return Err(e),
    }
    Ok(())
}
