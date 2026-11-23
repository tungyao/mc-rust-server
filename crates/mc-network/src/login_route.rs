//! login_route: Login离线流程 Start→[SetCompression]→Success→Ack→Config(Finish→Ack)→enter_play(真下发)
use super::framing::{read_packet, read_packet_framed, to_io_invalid, write_packet, write_packet_framed};
use crossbeam_channel::Sender;
use mc_protocol::packets;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use std::io;

use super::NewConnection;

use std::time::{Duration, Instant};

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

/// Configuration阶段最小可用：按序 KnownPacks S2C→KnownPacks C2S→Registry Data(4包)→Update Tags→Finish S2C→等 Finish Ack C2S。
/// 包 ID 已按P4A对照表核对777（CB 0x0F/0x07/0x0E/0x03，SB 0x07/0x03）；字段见 packets::config（来源 minecraft.wiki 777）。
/// Update Tags（CB 0x0E，见 config.rs 勘误注释，旧文档 0x0D 是 26.1 值）在 Finish 前发送：
/// tag 永不走 known packs，timeline 4 tag 缺失则 dimension 引用的 `#minecraft:in_overworld` 悬空，真机拒收 Finish。
/// 容忍度：真机在 Configuration 会穿插 ClientInformation(0x00)/PluginMessage/Pong/KeepAlive 等，
/// 这些与进服无关，先 debug 忽略，只认 KnownPacks 回包与 Finish Ack；规定步数内收不齐才 Err。
/// threshold 透传（压缩开启后 Configuration 包同样走压缩帧）。
pub async fn config_finish(stream: &mut TcpStream, threshold: i32) -> io::Result<()> {
    use tokio::time::{timeout, Duration};
    let compressed = packets::compression_enabled(threshold);
    // 1. Known Packs S2C（单vanilla包）。
    let known = packets::ConfigKnownPacks::vanilla();
    let mut body = Vec::new();
    known.encode_body(&mut body);
    debug!("config known packs send id={:#04x} n={}", packets::CONFIG_KNOWN_PACKS_S2C_ID, known.packs.len());
    if compressed {
        write_packet_framed(stream, packets::CONFIG_KNOWN_PACKS_S2C_ID, &body, threshold).await?;
    } else {
        write_packet(stream, packets::CONFIG_KNOWN_PACKS_S2C_ID, &body).await?;
    }
    // 2. 等 Known Packs C2S：中间穿插包全部忽略，最多等 32 个包/整体 15s。
    let mut got_known = false;
    for _ in 0..32 {
        let next = timeout(Duration::from_secs(15), async {
            if compressed { read_packet_framed(stream, threshold).await } else { read_packet(stream).await }
        })
        .await
        .map_err(|_| to_io_invalid("config known packs wait timed out"))??;
        let (id, payload) = next;
        if id == packets::CONFIG_KNOWN_PACKS_C2S_ID {
            let reply = packets::decode_config_known_packs_c2s(&payload).map_err(to_io_invalid)?.0;
            debug!("config known packs ack n={}", reply.packs.len());
            got_known = true;
            break;
        }
        debug!("config: ignore interleaved id={id:#04x} len={} while waiting known packs", payload.len());
    }
    if !got_known {
        return Err(to_io_invalid("config known packs response missing"));
    }
    // 3. Registry Data 4包（dimension_type + biome + timeline + world_clock，各条目 has_data=true真NBT）。
    for reg in packets::default_configuration_registries() {
        let mut b = Vec::new();
        reg.encode_body(&mut b);
        debug!("config registry send {} entries={}", reg.registry_id, reg.entries.len());
        if compressed {
            write_packet_framed(stream, packets::CONFIG_REGISTRY_DATA_S2C_ID, &b, threshold).await?;
        } else {
            write_packet(stream, packets::CONFIG_REGISTRY_DATA_S2C_ID, &b).await?;
        }
    }
    // 4. Update Tags（timeline 4 tag，供 dimension `#minecraft:in_overworld` 引用消解）。
    {
        let tags = packets::ConfigUpdateTags::minimal_timeline_tags();
        let mut b = Vec::new();
        tags.encode_body(&mut b);
        debug!("config update tags send registries={}", tags.registries.len());
        if compressed {
            write_packet_framed(stream, packets::CONFIG_UPDATE_TAGS_S2C_ID, &b, threshold).await?;
        } else {
            write_packet(stream, packets::CONFIG_UPDATE_TAGS_S2C_ID, &b).await?;
        }
    }
    // 5. Finish S2C → 等 Finish Ack C2S，中间穿插包同样忽略。
    debug!("config finish send id={:#04x}", packets::CONFIG_FINISH_S2C_ID);
    if compressed {
        write_packet_framed(stream, packets::CONFIG_FINISH_S2C_ID, &[], threshold).await?;
    } else {
        write_packet(stream, packets::CONFIG_FINISH_S2C_ID, &[]).await?;
    }
    for _ in 0..32 {
        let next = timeout(Duration::from_secs(15), async {
            if compressed { read_packet_framed(stream, threshold).await } else { read_packet(stream).await }
        })
        .await
        .map_err(|_| to_io_invalid("config finish ack wait timed out"))??;
        let (id, payload) = next;
        if id == packets::CONFIG_FINISH_ACK_C2S_ID {
            packets::decode_config_finish_ack(&payload).map_err(to_io_invalid)?;
            debug!("config finish ack ok");
            return Ok(());
        }
        debug!("config: ignore interleaved id={id:#04x} len={} while waiting finish ack", payload.len());
    }
    Err(to_io_invalid("config finish ack missing"))
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
    // 最后一次收到KeepAlive回显的时间；移动包不刷新它（只证明位置变化，不证明存活）。
    let mut last_seen = Instant::now();
    loop {
        let next = if compressed { read_packet_framed(stream, threshold).await } else { read_packet(stream).await };
        match next {
            Ok((id, payload)) if id == packets::PLAY_KEEPALIVE_C2S_ID => {
                match packets::decode_keepalive_c2s(&payload) {
                    Ok(echo) => {
                        last_seen = Instant::now();
                        info!("play keepalive echo uuid={} name={} id={echo}", profile.uuid, profile.username);
                    }
                    Err(e) => debug!("play uuid={} bad keepalive: {e}", profile.uuid),
                }
            }
            Ok((id, payload))
                if id == packets::PLAY_MOVE_POS_C2S_ID
                    || id == packets::PLAY_MOVE_POSROT_C2S_ID
                    || id == packets::PLAY_MOVE_ROT_C2S_ID
                    || id == packets::PLAY_MOVE_FLAGS_C2S_ID =>
            {
                match packets::decode_c2s_movement(id, &payload) {
                    Ok(m) => info!(
                        "play move uuid={} name={} id={:#04x} m={:?} (TODO P5a: forward via channel to ECS MovementQueue)",
                        profile.uuid, profile.username, id, m
                    ),
                    Err(e) => debug!("play uuid={} bad movement id={id:#04x}: {e}", profile.uuid),
                }
            }
            Ok((id, payload)) => {
                debug!("play uuid={} c2s id={id:#04x} len={} (ignored)", profile.uuid, payload.len());
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                if last_seen.elapsed() > Duration::from_secs(15) {
                    warn!("play uuid={} keepalive timeout >15s without echo, kicking (online-- in ECS timeout system)", profile.uuid);
                    // online递减单点放在ECS keepalive_timeout_system，避免net/ECS双减；
                    // net线程只关socket（踢出）。
                    break;
                }
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
    async fn config_finish_sends_known_registry_tags_finish_in_order() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            config_finish(&mut s, packets::COMPRESSION_DISABLED_THRESHOLD).await.unwrap();
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 1. KnownPacks S2C（CB 0x0F，单vanilla包）。
        let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::CONFIG_KNOWN_PACKS_S2C_ID);
        let packs = packets::decode_config_known_packs_s2c(&payload).unwrap().0;
        assert_eq!(packs.packs.len(), 1);
        assert_eq!(packs.packs[0].namespace, "minecraft");
        // 2. 回 KnownPacks C2S（SB 0x07）。
        let mut kp = Vec::new();
        packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kp);
        use tokio::io::AsyncWriteExt;
        c.write_all(&kp).await.unwrap();
        // 3. Registry Data 4包（CB 0x07：dimension_type + biome + timeline + world_clock）。
        for want in ["minecraft:dimension_type", "minecraft:worldgen/biome", "minecraft:timeline", "minecraft:world_clock"] {
            let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
            assert_eq!(id, packets::CONFIG_REGISTRY_DATA_S2C_ID);
            let (reg, _) = packets::decode_config_registry_data(&payload).unwrap();
            assert_eq!(reg.registry_id, want);
            assert!(!reg.entries.is_empty());
        }
        // 4. Update Tags 1包（CB 0x0E：timeline 4 tag）。
        let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::CONFIG_UPDATE_TAGS_S2C_ID);
        let (tags, _) = packets::decode_config_update_tags(&payload).unwrap();
        assert_eq!(tags, packets::ConfigUpdateTags::minimal_timeline_tags());
        // 5. Finish S2C → 回 Finish Ack。
        let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::CONFIG_FINISH_S2C_ID);
        packets::decode_config_finish_s2c(&payload).unwrap();
        let mut ack = Vec::new();
        packets::encode_config_finish_ack_packet(&mut ack);
        c.write_all(&ack).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn config_finish_rejects_wrong_known_packs_id() {
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
    async fn config_finish_rejects_wrong_ack_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            assert!(config_finish(&mut s, packets::COMPRESSION_DISABLED_THRESHOLD).await.is_err());
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 先走完 KnownPacks 往返 + 读掉 4 个 Registry + 1 个 Update Tags，再在 Finish 处错包。
        let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        let mut kp = Vec::new();
        packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kp);
        use tokio::io::AsyncWriteExt;
        c.write_all(&kp).await.unwrap();
        for _ in 0..5 {
            let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        }
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

    #[tokio::test]
    async fn enter_play_accepts_movement_packets_without_disconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let profile = mc_core::PlayerProfile { uuid: packets::offline_uuid("Alex"), username: "Alex".into() };
        let server = tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            enter_play(s, profile, packets::COMPRESSION_DISABLED_THRESHOLD).await.unwrap();
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        for _ in 0..5 {
            let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        }
        // 依次发 0x1E/0x1F/0x20/0x21，服务端只log TODO进ECS，不断开
        let mut body = Vec::new();
        packets::MovePlayerPos { x: 9.5, y: 80.0, z: 9.5, on_ground: true, horizontal_collision: false }
            .encode_body(&mut body);
        super::super::framing::write_packet(&mut c, packets::PLAY_MOVE_POS_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerPosRot { x: 9.5, y: 80.0, z: 9.5, yaw: 90.0, pitch: 0.0, on_ground: true, horizontal_collision: false }
            .encode_body(&mut body);
        super::super::framing::write_packet(&mut c, packets::PLAY_MOVE_POSROT_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerRot { yaw: 180.0, pitch: 0.0, on_ground: false, horizontal_collision: false }
            .encode_body(&mut body);
        super::super::framing::write_packet(&mut c, packets::PLAY_MOVE_ROT_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerFlags { on_ground: true, horizontal_collision: false }.encode_body(&mut body);
        super::super::framing::write_packet(&mut c, packets::PLAY_MOVE_FLAGS_C2S_ID, &body).await.unwrap();
        // 300ms内不应EOF（连接仍开着）
        let mut buf = [0u8; 1];
        match tokio::time::timeout(std::time::Duration::from_millis(300), super::super::framing::read_packet(&mut c)).await {
            Err(_) => {} // 超时=服务端没发新包但连接仍开着，符合预期
            Ok(Ok(_)) => {} // 若收到keepalive重发也算连接正常
            Ok(Err(e)) => panic!("server closed play after movement packets: {e:?}"),
        }
        drop(c);
        server.await.unwrap();
    }
}
