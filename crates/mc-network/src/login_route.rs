//! login_route: Login离线流程 Start→Success→Ack→enter_play，只有Ack成功才计数
use super::framing::{read_packet, to_io_invalid, write_packet};
use crossbeam_channel::Sender;
use mc_protocol::packets;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use std::io;

use super::NewConnection;

/// Login 离线流程（无加密/无正版验证）。socket 所有权移交，与 Status 区分开。
pub async fn handle_login(mut stream: TcpStream, addr: SocketAddr, tx: &Sender<NewConnection>) -> io::Result<()> {
    let (id, payload) = read_packet(&mut stream).await?;
    if id != packets::LOGIN_START_PACKET_ID {
        warn!("{addr} login: expected start 0x00, got {id:#04x}, disconnect");
        return Ok(());
    }
    let (start, _) = packets::LoginStart::decode_body(&payload).map_err(to_io_invalid)?;
    if start.name.is_empty() || start.name.chars().count() > 16 {
        warn!("{addr} login: bad username {:?}, disconnect", start.name);
        return Ok(());
    }
    debug_assert_eq!(packets::COMPRESSION_DISABLED_THRESHOLD, -1);
    let uuid = packets::offline_uuid(&start.name);
    let profile = mc_core::PlayerProfile { uuid, username: start.name.clone() };
    let mut body = Vec::new();
    packets::LoginSuccess { uuid, username: profile.username.clone() }.encode_body(&mut body);
    write_packet(&mut stream, packets::LOGIN_SUCCESS_PACKET_ID, &body).await?;
    match read_packet(&mut stream).await {
        Ok((packets::LOGIN_ACKNOWLEDGED_PACKET_ID, p)) => {
            packets::decode_login_acknowledged(&p).map_err(to_io_invalid)?;
        }
        Ok((other, _)) => {
            warn!("{addr} login: expected ack 0x03, got {other:#04x}, disconnect");
            return Ok(());
        }
        Err(e) => {
            debug!("{addr} login: no ack ({e}), disconnect");
            return Ok(());
        }
    }
    info!("login ok uuid={} name={}", uuid, profile.username);
    notify_login_success(tx, addr, &profile);
    enter_play(stream, profile).await?;
    Ok(())
}

/// Login 成功后的唯一真实连接上报点（在线数只认这里）。
pub fn notify_login_success(tx: &Sender<NewConnection>, addr: SocketAddr, profile: &mc_core::PlayerProfile) {
    use tracing::debug;
    if let Err(e) = tx.try_send(NewConnection(addr)) {
        debug!("login success notify dropped for {addr} ({e}): {profile:?}");
    }
}

/// 进入 Play 的钩子：拿走 socket 所有权。P2 占位实现只记 log 后关闭。
pub async fn enter_play(stream: TcpStream, profile: mc_core::PlayerProfile) -> io::Result<()> {
    info!("enter_play uuid={} name={} (P2 TODO: real play pipeline)", profile.uuid, profile.username);
    drop(stream);
    Ok(())
}
