//! Login 离线全流程集成测试：真实 TcpListener + 模拟客户端走完
//! Handshake(Login) → Start → Success → Ack，不断言失败即通过。
//! 同时覆盖：Login 成功才 push NewConnection（Status 不 push）。
use mc_network::{NetConfig, serve};
use mc_protocol::packets::{
    self, Handshake, HandshakeNextState, CONFIG_FINISH_ACK_C2S_ID, CONFIG_FINISH_S2C_ID,
    HANDSHAKE_PACKET_ID, LOGIN_ACKNOWLEDGED_PACKET_ID, LOGIN_START_PACKET_ID,
    LOGIN_SUCCESS_PACKET_ID, SET_COMPRESSION_PACKET_ID,
};
use mc_protocol::read_varint;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn read_frame(stream: &mut TcpStream) -> std::io::Result<(i32, Vec<u8>)> {
    let len = read_varint_async(stream).await?;
    assert!(len >= 0, "negative length {len}");
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    let (id, n) = read_varint(&buf).expect("packet id");
    Ok((id, buf[n..].to_vec()))
}

async fn read_varint_async(stream: &mut TcpStream) -> std::io::Result<i32> {
    let mut num = 0i32;
    let mut shift = 0u32;
    for _ in 0..5 {
        let mut b = [0u8; 1];
        stream.read_exact(&mut b).await?;
        num |= ((b[0] & 0x7F) as i32) << shift;
        if b[0] & 0x80 == 0 {
            return Ok(num);
        }
        shift += 7;
    }
    panic!("varint too big");
}

async fn send_handshake(stream: &mut TcpStream, protocol: i32, port: u16, next: HandshakeNextState) {
    let hs = Handshake {
        protocol_version: protocol,
        server_address: "localhost".to_string(),
        server_port: port,
        next_state: next,
    };
    let mut body = Vec::new();
    hs.encode_body(&mut body);
    let mut framed = Vec::new();
    packets::encode_packet(HANDSHAKE_PACKET_ID, &body, &mut framed);
    stream.write_all(&framed).await.unwrap();
}

async fn spawn_server() -> (
    std::net::SocketAddr,
    crossbeam_channel::Receiver<mc_network::NewConnection>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = crossbeam_channel::unbounded();
    tokio::spawn(async move {
        let _ = serve(listener, tx, NetConfig::default()).await;
    });
    (addr, rx)
}

/// 全流程：Handshake(Login) → Login Start → Login Success → Login Ack → Config(Finish→Ack) → Play 加入序列。
/// 服务端回的 Success 用户名必须与发送一致；Ack 后先走 Configuration Finish 握手，再 enter_play 真下发 5 包且不断开。
#[tokio::test]
async fn login_offline_full_flow() {
    let (addr, rx) = spawn_server().await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 777, addr.port(), HandshakeNextState::Login).await;

    // Login Start（name + 16 字节 UUID；离线模式服务端忽略客户端 UUID，此处填全 0）。
    let mut body = Vec::new();
    packets::encode_string("Steve", &mut body);
    body.extend_from_slice(&[0u8; 16]);
    let mut req = Vec::new();
    packets::encode_packet(LOGIN_START_PACKET_ID, &body, &mut req);
    s.write_all(&req).await.unwrap();

    // Login Success（clientbound 0x02）：UUID + name。
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, LOGIN_SUCCESS_PACKET_ID, "expected login success 0x02");
    let (success, _) = packets::decode_login_success(&payload).unwrap();
    assert_eq!(success.username, "Steve");
    assert_ne!(success.uuid.as_bytes(), &[0u8; 16], "offline uuid must be server-assigned");

    // Login Acknowledged（serverbound 0x03 空包）。
    let mut ack = Vec::new();
    packets::encode_login_acknowledged_packet(&mut ack);
    s.write_all(&ack).await.unwrap();

    // Configuration：KnownPacks S2C→C2S→Registry Data(4包)→Update Tags→Finish(CB 0x03)→Ack(SB 0x03)；ID已按P4A对照表核对777。
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, packets::CONFIG_KNOWN_PACKS_S2C_ID, "expected known packs 0x0F");
    packets::decode_config_known_packs_s2c(&payload).unwrap();
    let mut kpack = Vec::new();
    packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kpack);
    s.write_all(&kpack).await.unwrap();
    for want in ["minecraft:dimension_type", "minecraft:worldgen/biome", "minecraft:timeline", "minecraft:world_clock"] {
        let (id, payload) = read_frame(&mut s).await.unwrap();
        assert_eq!(id, packets::CONFIG_REGISTRY_DATA_S2C_ID, "expected registry 0x07");
        assert_eq!(packets::decode_config_registry_data(&payload).unwrap().0.registry_id, want);
    }
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, packets::CONFIG_UPDATE_TAGS_S2C_ID, "expected update tags 0x0E");
    assert_eq!(packets::decode_config_update_tags(&payload).unwrap().0, packets::ConfigUpdateTags::minimal_timeline_tags());
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, CONFIG_FINISH_S2C_ID, "expected config finish 0x03");
    packets::decode_config_finish_s2c(&payload).unwrap();
    let mut fack = Vec::new();
    packets::encode_config_finish_ack_packet(&mut fack);
    s.write_all(&fack).await.unwrap();

    // Login 成功才算真实连接：channel 应收到一次 NewConnection。
    // 注：`#[tokio::test]` 默认单线程 runtime，不能用阻塞的 `recv_timeout`，
    // 用 try_recv 轮询代替。
    let conn = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(c) = rx.try_recv() {
                return c;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("login success must push NewConnection");
    assert_eq!(conn.0.ip(), std::net::IpAddr::from([127, 0, 0, 1]));

    // enter_play 真下发：按序读完 Join 序列 5 包（ID已按P4A对照表核对777）。
    let expect = [
        packets::PLAY_LOGIN_PLAY_ID,
        packets::PLAY_SET_CENTER_CHUNK_ID,
        packets::PLAY_CHUNK_DATA_ID,
        packets::PLAY_PLAYER_POSITION_SYNC_ID,
        packets::PLAY_KEEPALIVE_S2C_ID,
    ];
    for want in expect {
        let (id, _) = read_frame(&mut s).await.unwrap();
        assert_eq!(id, want, "join sequence order");
    }
    // C2S KeepAlive 回显后连接保持（Play 循环只 log 不踢）：短超时内不应 EOF。
    let mut body = Vec::new();
    packets::KeepAliveC2S(1).encode_body(&mut body);
    let mut echo = Vec::new();
    packets::encode_packet(packets::PLAY_KEEPALIVE_C2S_ID, &body, &mut echo);
    s.write_all(&echo).await.unwrap();
    let mut buf = [0u8; 1];
    match tokio::time::timeout(std::time::Duration::from_millis(300), s.read_exact(&mut buf)).await {
        Err(_) => {} // 超时 = 连接仍开着，符合预期
        Ok(Ok(_)) => panic!("unexpected extra byte after keepalive echo"),
        Ok(Err(e)) => panic!("server closed play connection early: {e:?}"),
    }
}

/// Login Start 包 ID 断言：首包错 ID 应断开（与 Status 流程区分：这里不断言 Status 行为）。
#[tokio::test]
async fn login_wrong_first_packet_disconnects() {
    let (addr, _rx) = spawn_server().await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 777, addr.port(), HandshakeNextState::Login).await;

    // 发一个错 ID 的包（0x7F）代替 Login Start。
    let mut bad = Vec::new();
    packets::encode_packet(0x7F, &[], &mut bad);
    s.write_all(&bad).await.unwrap();

    let mut buf = [0u8; 1];
    match s.read_exact(&mut buf).await {
        Ok(_) => panic!("server should disconnect on wrong login packet id"),
        Err(e) => assert!(
            e.kind() == std::io::ErrorKind::UnexpectedEof,
            "expected EOF, got {e:?}"
        ),
    }
}

/// Status 流程不 push NewConnection（只有 Login 成功才算真实连接）。
#[tokio::test]
async fn status_flow_pushes_no_connection() {
    let (addr, rx) = spawn_server().await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 777, addr.port(), HandshakeNextState::Status).await;

    let mut req = Vec::new();
    packets::encode_status_request_packet(&mut req);
    s.write_all(&req).await.unwrap();
    let (id, _) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, 0x00);

    // 给服务端一点时间（若误 push 会落进 channel），随后断言为空。
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        rx.try_recv().is_err(),
        "status flow must not push NewConnection"
    );
}

/// 半开连接：Handshake(Login) 后客户端直接断开 → 服务端读到 EOF，不 push。
#[tokio::test]
async fn login_aborted_before_start_pushes_nothing() {
    let (addr, rx) = spawn_server().await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 777, addr.port(), HandshakeNextState::Login).await;
    drop(s);

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        rx.try_recv().is_err(),
        "aborted login must not push NewConnection"
    );
}

/// 包 ID 常量自检：Login 各包与 Status/Handshake 方向标注一致。
#[test]
fn login_packet_ids_sanity() {
    assert_eq!(LOGIN_START_PACKET_ID, 0x00);
    assert_eq!(LOGIN_SUCCESS_PACKET_ID, 0x02);
    assert_eq!(LOGIN_ACKNOWLEDGED_PACKET_ID, 0x03);
}

/// 包 ID 自检：Play 6 个 + Configuration 5 个必须与 P4A 对照表（777）一致。
/// Play 错值在 777 里另有含义（0x2B= Hurt Animation 等），错一个真进服直接断。
#[test]
fn play_and_config_packet_ids_match_p4a_map() {
    assert_eq!(packets::PLAY_LOGIN_PLAY_ID, 0x32);
    assert_eq!(packets::PLAY_SET_CENTER_CHUNK_ID, 0x60);
    assert_eq!(packets::PLAY_CHUNK_DATA_ID, 0x2E);
    assert_eq!(packets::PLAY_PLAYER_POSITION_SYNC_ID, 0x49);
    assert_eq!(packets::PLAY_KEEPALIVE_S2C_ID, 0x2D);
    assert_eq!(packets::PLAY_KEEPALIVE_C2S_ID, 0x1C);
    assert_eq!(packets::CONFIG_FINISH_S2C_ID, 0x03);
    assert_eq!(packets::CONFIG_FINISH_ACK_C2S_ID, 0x03);
    assert_eq!(packets::CONFIG_REGISTRY_DATA_S2C_ID, 0x07);
    assert_eq!(packets::CONFIG_KNOWN_PACKS_S2C_ID, 0x0F);
    assert_eq!(packets::CONFIG_KNOWN_PACKS_C2S_ID, 0x07);
    assert_eq!(packets::CONFIG_UPDATE_TAGS_S2C_ID, 0x0E);
}

/// 压缩Login流程：threshold>=0 时 Success 前先发 SetCompression，其后走压缩帧。
#[tokio::test]
async fn login_compression_flow_sends_set_compression_first() {
    use mc_network::framing::{decode_compressed_frame, encode_compressed_frame};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = crossbeam_channel::unbounded::<mc_network::NewConnection>();
    tokio::spawn(async move {
        let (socket, caddr) = listener.accept().await.unwrap();
        mc_network::login_route::handle_login_with_threshold(socket, caddr, &tx, 64)
            .await
            .unwrap();
    });
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut body = Vec::new();
    packets::encode_string("Steve", &mut body);
    body.extend_from_slice(&[0u8; 16]);
    let mut req = Vec::new();
    packets::encode_packet(LOGIN_START_PACKET_ID, &body, &mut req);
    s.write_all(&req).await.unwrap();
    // 第一包必须是 SetCompression（普通帧），阈值回显 64。
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, SET_COMPRESSION_PACKET_ID);
    assert_eq!(packets::decode_set_compression(&payload).unwrap(), 64);
    // 第二包是压缩帧 Success（小包 data_len=0 直通），用户名一致、属性表为空。
    let len = read_varint_async(&mut s).await.unwrap();
    let mut frame = vec![0u8; len as usize];
    s.read_exact(&mut frame).await.unwrap();
    let (id, payload) = decode_compressed_frame(&frame).unwrap();
    assert_eq!(id, LOGIN_SUCCESS_PACKET_ID);
    let (success, _) = packets::decode_login_success(&payload).unwrap();
    assert_eq!(success.username, "Steve");
    assert!(success.properties.is_empty());
    // Ack 用压缩帧回发；随后走 Configuration Finish 握手（压缩帧），成功后才 push NewConnection，随后 enter_play。
    let ack = encode_compressed_frame(LOGIN_ACKNOWLEDGED_PACKET_ID, &[], 64);
    let mut lb = Vec::new();
    mc_protocol::write_varint(ack.len() as i32, &mut lb);
    s.write_all(&lb).await.unwrap();
    s.write_all(&ack).await.unwrap();
    // Configuration（压缩帧）：KnownPacks S2C→C2S→Registry 4包→Update Tags→Finish CB 0x03→Ack SB 0x03。
    let len = read_varint_async(&mut s).await.unwrap();
    let mut frame = vec![0u8; len as usize];
    s.read_exact(&mut frame).await.unwrap();
    let (id, payload) = decode_compressed_frame(&frame).unwrap();
    assert_eq!(id, packets::CONFIG_KNOWN_PACKS_S2C_ID);
    packets::decode_config_known_packs_s2c(&payload).unwrap();
    let kp = encode_compressed_frame(packets::CONFIG_KNOWN_PACKS_C2S_ID, &{
        let mut b = Vec::new();
        packets::ConfigKnownPacks::vanilla().encode_body(&mut b);
        b
    }, 64);
    let mut klb = Vec::new();
    mc_protocol::write_varint(kp.len() as i32, &mut klb);
    s.write_all(&klb).await.unwrap();
    s.write_all(&kp).await.unwrap();
    for want in ["minecraft:dimension_type", "minecraft:worldgen/biome", "minecraft:timeline", "minecraft:world_clock"] {
        let len = read_varint_async(&mut s).await.unwrap();
        let mut frame = vec![0u8; len as usize];
        s.read_exact(&mut frame).await.unwrap();
        let (id, payload) = decode_compressed_frame(&frame).unwrap();
        assert_eq!(id, packets::CONFIG_REGISTRY_DATA_S2C_ID);
        assert_eq!(packets::decode_config_registry_data(&payload).unwrap().0.registry_id, want);
    }
    let len = read_varint_async(&mut s).await.unwrap();
    let mut frame = vec![0u8; len as usize];
    s.read_exact(&mut frame).await.unwrap();
    let (id, payload) = decode_compressed_frame(&frame).unwrap();
    assert_eq!(id, packets::CONFIG_UPDATE_TAGS_S2C_ID);
    assert_eq!(packets::decode_config_update_tags(&payload).unwrap().0, packets::ConfigUpdateTags::minimal_timeline_tags());
    let len = read_varint_async(&mut s).await.unwrap();
    let mut frame = vec![0u8; len as usize];
    s.read_exact(&mut frame).await.unwrap();
    let (id, payload) = decode_compressed_frame(&frame).unwrap();
    assert_eq!(id, CONFIG_FINISH_S2C_ID);
    packets::decode_config_finish_s2c(&payload).unwrap();
    let fack = encode_compressed_frame(CONFIG_FINISH_ACK_C2S_ID, &[], 64);
    let mut flb = Vec::new();
    mc_protocol::write_varint(fack.len() as i32, &mut flb);
    s.write_all(&flb).await.unwrap();
    s.write_all(&fack).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(c) = rx.try_recv() {
                return c;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("compressed login must push NewConnection");
    // enter_play 真下发：压缩帧 5 包 Join 序列，随后连接保持（Play 循环）。
    let expect = [
        packets::PLAY_LOGIN_PLAY_ID,
        packets::PLAY_SET_CENTER_CHUNK_ID,
        packets::PLAY_CHUNK_DATA_ID,
        packets::PLAY_PLAYER_POSITION_SYNC_ID,
        packets::PLAY_KEEPALIVE_S2C_ID,
    ];
    for want in expect {
        let len = read_varint_async(&mut s).await.unwrap();
        let mut frame = vec![0u8; len as usize];
        s.read_exact(&mut frame).await.unwrap();
        let (id, _) = decode_compressed_frame(&frame).unwrap();
        assert_eq!(id, want, "compressed join sequence order");
    }
    // Play 循环保持：300ms 内不应 EOF。
    let mut buf = [0u8; 1];
    match tokio::time::timeout(std::time::Duration::from_millis(300), s.read_exact(&mut buf)).await {
        Err(_) => {} // 超时 = 连接仍开着，符合预期
        Ok(Ok(_)) => panic!("unexpected extra byte after compressed join"),
        Ok(Err(e)) => panic!("server closed compressed play early: {e:?}"),
    }
}
