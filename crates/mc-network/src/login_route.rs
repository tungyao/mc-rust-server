//! login_route: Login离线流程 Start→[SetCompression]→Success→Ack→Config(Finish→Ack)→enter_play(真下发)
use super::framing::{read_packet, read_packet_framed, to_io_invalid, write_packet, write_packet_framed};
use crossbeam_channel::Sender;
use mc_protocol::packets;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use std::io;

use super::NewConnection;

/// 默认压缩禁用（阈值 -1，原行为）。
pub async fn handle_login(stream: TcpStream, addr: SocketAddr, tx: &Sender<NewConnection>) -> io::Result<()> {
    handle_login_with_threshold(stream, addr, tx, packets::COMPRESSION_DISABLED_THRESHOLD).await
}

/// 阈值可配：threshold>=0 时 Success 前先发 SetCompression，其后走压缩帧。
pub async fn handle_login_with_threshold(mut stream: TcpStream, addr: SocketAddr, tx: &Sender<NewConnection>, threshold: i32) -> io::Result<()> {
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
    let compressed = packets::compression_enabled(threshold);
    if compressed {
        debug!("{addr} login: enable compression threshold={threshold}");
        let mut sc = Vec::new();
        packets::encode_set_compression_packet(threshold, &mut sc);
        use tokio::io::AsyncWriteExt;
        stream.write_all(&sc).await?;
    }
    let uuid = packets::offline_uuid(&start.name);
    let profile = mc_core::PlayerProfile { uuid, username: start.name.clone() };
    let mut body = Vec::new();
    packets::LoginSuccess::offline(uuid, profile.username.clone()).encode_body(&mut body);
    if compressed {
        write_packet_framed(&mut stream, packets::LOGIN_SUCCESS_PACKET_ID, &body, threshold).await?;
    } else {
        write_packet(&mut stream, packets::LOGIN_SUCCESS_PACKET_ID, &body).await?;
    }
    let ack = if compressed { read_packet_framed(&mut stream, threshold).await } else { read_packet(&mut stream).await };
    match ack {
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
    if let Err(e) = config_finish(&mut stream, threshold).await {
        warn!("{addr} config: finish failed ({e}), disconnect");
        return Ok(());
    }
    notify_login_success(tx, addr, &profile);
    enter_play(stream, profile, threshold).await?;
    Ok(())
}

/// Configuration阶段骨架：空 registry 直接发 Finish S2C → 等 Finish Ack C2S。
/// 包 ID 已按P4A对照表核对777；Registry Data / Known Packs 暂不下发，只留 packets::config 占位。
/// threshold 透传（压缩开启后 Configuration 包同样走压缩帧）。签名保持 `enter_play` 不变，中间只插这一段。
pub async fn config_finish(stream: &mut TcpStream, threshold: i32) -> io::Result<()> {
    let compressed = packets::compression_enabled(threshold);
    debug!("config finish send id={:#04x}", packets::CONFIG_FINISH_S2C_ID);
    if compressed {
        write_packet_framed(stream, packets::CONFIG_FINISH_S2C_ID, &[], threshold).await?;
    } else {
        write_packet(stream, packets::CONFIG_FINISH_S2C_ID, &[]).await?;
    }
    let (id, payload) = if compressed { read_packet_framed(stream, threshold).await } else { read_packet(stream).await }?;
    if id != packets::CONFIG_FINISH_ACK_C2S_ID {
        return Err(to_io_invalid(format!(
            "expected config finish ack {:#04x}, got {id:#04x}",
            packets::CONFIG_FINISH_ACK_C2S_ID
        )));
    }
    packets::decode_config_finish_ack(&payload).map_err(to_io_invalid)?;
    debug!("config finish ack ok");
    Ok(())
}

/// Login 成功后的唯一真实连接上报点（在线数只认这里）。
pub fn notify_login_success(tx: &Sender<NewConnection>, addr: SocketAddr, profile: &mc_core::PlayerProfile) {
    if let Err(e) = tx.try_send(NewConnection(addr)) {
        debug!("login success notify dropped for {addr} ({e}): {profile:?}");
    }
}

/// 进入 Play：拿住 socket，按序真下发后进循环。threshold透传，压缩开启后Play包走压缩帧。
pub async fn enter_play(mut stream: TcpStream, profile: mc_core::PlayerProfile, threshold: i32) -> io::Result<()> {
    info!("enter_play uuid={} name={} threshold={threshold}", profile.uuid, profile.username);
    let keepalive_id = send_join_sequence(&mut stream, &profile, threshold).await?;
    info!("play join sent uuid={} name={} keepalive={keepalive_id}", profile.uuid, profile.username);
    play_loop(&mut stream, &profile, keepalive_id, threshold).await
}

async fn send_join_sequence(stream: &mut TcpStream, profile: &mc_core::PlayerProfile, threshold: i32) -> io::Result<i64> {
    let compressed = packets::compression_enabled(threshold);
    let mut body = Vec::new();
    packets::LoginPlay::default().encode_body(&mut body);
    if compressed { write_packet_framed(stream, packets::PLAY_LOGIN_PLAY_ID, &body, threshold).await?; }
    else { write_packet(stream, packets::PLAY_LOGIN_PLAY_ID, &body).await?; }
    debug!("play {} login play sent", profile.username);
    body.clear();
    packets::SetCenterChunk { chunk_x: 0, chunk_z: 0 }.encode_body(&mut body);
    if compressed { write_packet_framed(stream, packets::PLAY_SET_CENTER_CHUNK_ID, &body, threshold).await?; }
    else { write_packet(stream, packets::PLAY_SET_CENTER_CHUNK_ID, &body).await?; }
    body.clear();
    packets::PlayChunkData::empty(0, 0).encode_body(&mut body);
    if compressed { write_packet_framed(stream, packets::PLAY_CHUNK_DATA_ID, &body, threshold).await?; }
    else { write_packet(stream, packets::PLAY_CHUNK_DATA_ID, &body).await?; }
    body.clear();
    packets::PlayerPositionSync { x: 8.5, y: 80.0, z: 8.5, yaw: 0.0, pitch: 0.0, flags: 0, teleport_id: 1 }.encode_body(&mut body);
    if compressed { write_packet_framed(stream, packets::PLAY_PLAYER_POSITION_SYNC_ID, &body, threshold).await?; }
    else { write_packet(stream, packets::PLAY_PLAYER_POSITION_SYNC_ID, &body).await?; }
    let keepalive_id = 1i64;
    body.clear();
    packets::KeepAliveS2C(keepalive_id).encode_body(&mut body);
    if compressed { write_packet_framed(stream, packets::PLAY_KEEPALIVE_S2C_ID, &body, threshold).await?; }
    else { write_packet(stream, packets::PLAY_KEEPALIVE_S2C_ID, &body).await?; }
    Ok(keepalive_id)
}

async fn play_loop(stream: &mut TcpStream, profile: &mc_core::PlayerProfile, mut seq: i64, threshold: i32) -> io::Result<()> {
    let compressed = packets::compression_enabled(threshold);
    loop {
        let next = if compressed { read_packet_framed(stream, threshold).await } else { read_packet(stream).await };
        match next {
            Ok((id, payload)) if id == packets::PLAY_KEEPALIVE_C2S_ID => {
                match packets::decode_keepalive_c2s(&payload) {
                    Ok(echo) => info!("play keepalive echo uuid={} name={} id={echo}", profile.uuid, profile.username),
                    Err(e) => debug!("play uuid={} bad keepalive: {e}", profile.uuid),
                }
            }
            Ok((id, payload)) => {
                debug!("play uuid={} c2s id={id:#04x} len={} (ignored)", profile.uuid, payload.len());
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                seq += 1;
                let mut body = Vec::new();
                packets::KeepAliveS2C(seq).encode_body(&mut body);
                let r = if compressed { write_packet_framed(stream, packets::PLAY_KEEPALIVE_S2C_ID, &body, threshold).await }
                        else { write_packet(stream, packets::PLAY_KEEPALIVE_S2C_ID, &body).await };
                if let Err(e) = r {
                    debug!("play uuid={} keepalive resend failed ({e}), closing", profile.uuid);
                    break;
                }
            }
            Err(e) => {
                debug!("play uuid={} closed ({e})", profile.uuid);
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn config_finish_sends_finish_then_reads_ack() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            config_finish(&mut s, packets::COMPRESSION_DISABLED_THRESHOLD).await.unwrap();
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::CONFIG_FINISH_S2C_ID);
        packets::decode_config_finish_s2c(&payload).unwrap();
        let mut ack = Vec::new();
        packets::encode_config_finish_ack_packet(&mut ack);
        use tokio::io::AsyncWriteExt;
        c.write_all(&ack).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn config_finish_rejects_wrong_ack_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            assert!(config_finish(&mut s, packets::COMPRESSION_DISABLED_THRESHOLD).await.is_err());
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        super::super::framing::write_packet(&mut c, 0x7F, &[]).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn enter_play_sends_join_then_reads_keepalive_echo() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let profile = mc_core::PlayerProfile { uuid: packets::offline_uuid("Steve"), username: "Steve".into() };
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            enter_play(s, profile, packets::COMPRESSION_DISABLED_THRESHOLD).await.unwrap();
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 读5包：login/setcenter/chunk/pos/keepalive
        for _ in 0..5 {
            let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        }
        // 回一个 keepalive echo，服务端只log不断开；随后客户端断开，服务端正常退出
        let mut b = Vec::new();
        packets::KeepAliveC2S(1).encode_body(&mut b);
        super::super::framing::write_packet(&mut c, packets::PLAY_KEEPALIVE_C2S_ID, &b).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        drop(c);
        server.await.unwrap();
    }
}
