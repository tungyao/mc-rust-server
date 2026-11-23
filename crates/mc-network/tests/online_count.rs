//! 任务E：配置注入 + 在线数语义（真实端口集成测试）。
//!
//! - Status ping 走完：不 push `NewConnection`，`online==0`；
//! - Login 全流程走完（Handshake → Start → Success → Ack → Config Finish/Ack）才 push 一次，排空后 `online==1`；
//!   （裸 Handshake 不算成功：服务端等待 Login Start，超时/错包断开且不计数）；
//! - 错误协议 / 非握手首包：不 push、不计数；
//! - `NetConfig{max_players,motd}` 透传进 Status JSON。
use mc_network::{NetConfig, serve};
use mc_protocol::packets::{self, Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID};
use mc_protocol::{read_varint, write_varint};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
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

/// 拉起一个带自定义配置的真实 serve，返回 (addr, rx, online)。
async fn spawn_server_with(
    max_players: usize,
    motd: &str,
) -> (
    std::net::SocketAddr,
    crossbeam_channel::Receiver<mc_network::NewConnection>,
    Arc<AtomicUsize>,
) {
    let online = Arc::new(AtomicUsize::new(0));
    let config = NetConfig::new(max_players, motd.to_string(), Arc::clone(&online))
        .with_view_distance(4);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = crossbeam_channel::unbounded();
    tokio::spawn(async move {
        let _ = serve(listener, tx, config).await;
    });
    (addr, rx, online)
}

/// 完整 Status 流程（Request → Response → Ping → Pong），返回 Response JSON。
async fn do_status_flow(addr: std::net::SocketAddr) -> String {
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 777, addr.port(), HandshakeNextState::Status).await;
    let mut req = Vec::new();
    packets::encode_status_request_packet(&mut req);
    s.write_all(&req).await.unwrap();
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, 0x00);
    let (json, _) = packets::decode_status_response(&payload).unwrap();
    let mut ping = Vec::new();
    packets::encode_ping_request_packet(7, &mut ping);
    s.write_all(&ping).await.unwrap();
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, 0x01);
    assert_eq!(packets::decode_pong_payload(&payload).unwrap(), 7);
    json
}

/// 模拟 mc-server 侧 `handle_new_connections`：排空 channel 并 fetch_add。
fn drain_like_server(
    rx: &crossbeam_channel::Receiver<mc_network::NewConnection>,
    online: &Arc<AtomicUsize>,
) -> usize {
    let mut n = 0;
    while rx.try_recv().is_ok() {
        online.fetch_add(1, Ordering::SeqCst);
        n += 1;
    }
    n
}

#[tokio::test]
async fn status_ping_does_not_count_online() {
    let (addr, rx, online) = spawn_server_with(42, "E2E-MOTD-xyz").await;
    let json = do_status_flow(addr).await;

    // 配置透传：Status JSON 用 NetConfig 的真实值。
    assert!(json.contains("E2E-MOTD-xyz"), "motd missing in {json}");
    assert!(json.contains("\"max\":42"), "max missing in {json}");
    // 初始在线数为 0。
    assert!(json.contains("\"online\":0"), "online should be 0 in {json}");

    // 给连接任务一点时间（若误 push 会在这段窗口出现），然后断言无 push、无计数。
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(online.load(Ordering::SeqCst), 0, "status must not count");
    assert!(
        rx.try_recv().is_err(),
        "status must not push NewConnection"
    );
}

#[tokio::test]
async fn login_full_flow_counts_online_once() {
    let (addr, rx, online) = spawn_server_with(100, "login-count-test").await;

    // 任务D离线全流程：Handshake(Login) → Start → Success → Ack → Config Finish/Ack。
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 777, addr.port(), HandshakeNextState::Login).await;

    let mut body = Vec::new();
    packets::encode_string("Counter", &mut body);
    body.extend_from_slice(&[0u8; 16]); // 客户端 UUID：离线模式服务端忽略
    let mut start = Vec::new();
    packets::encode_packet(0x00, &body, &mut start);
    s.write_all(&start).await.unwrap();

    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, 0x02, "expected login success 0x02");
    let (success, _) = packets::decode_login_success(&payload).unwrap();
    assert_eq!(success.username, "Counter");

    let mut ack = Vec::new();
    packets::encode_login_acknowledged_packet(&mut ack);
    s.write_all(&ack).await.unwrap();
    // Configuration：KnownPacks S2C→C2S→Registry Data(4包)→Update Tags→Finish(CB 0x03)→Ack(SB 0x03)，ID已按P4A对照表核对777。
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, packets::CONFIG_KNOWN_PACKS_S2C_ID, "expected known packs 0x0F");
    packets::decode_config_known_packs_s2c(&payload).unwrap();
    let mut kpack = Vec::new();
    packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kpack);
    s.write_all(&kpack).await.unwrap();
    for (_name, want) in packets::vanilla_config_777::VANILLA_CONFIG_REGISTRY_BODIES_777 {
        let (id, payload) = read_frame(&mut s).await.unwrap();
        assert_eq!(id, packets::CONFIG_REGISTRY_DATA_S2C_ID, "expected registry 0x07");
        assert_eq!(&payload, want);
    }
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, packets::CONFIG_UPDATE_TAGS_S2C_ID, "expected update tags 0x0E");
    assert_eq!(payload, packets::vanilla_config_777::VANILLA_CONFIG_UPDATE_TAGS_BODY_777);
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, packets::CONFIG_FINISH_S2C_ID, "expected config finish 0x03");
    packets::decode_config_finish_s2c(&payload).unwrap();
    let mut fack = Vec::new();
    packets::encode_config_finish_ack_packet(&mut fack);
    s.write_all(&fack).await.unwrap();
    // enter_play 真下发 Join 序列：首包必须是 LoginPlay（CB 0x32，已按P4A对照表核对），随后连接保持进 Play 循环。
    let (id, _) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, packets::PLAY_LOGIN_PLAY_ID, "expected join game 0x32");

    // 等待 push（轮询代替固定 sleep，防 CI flake），最多 2s。
    let mut pushed = 0;
    for _ in 0..40 {
        pushed += drain_like_server(&rx, &online);
        if online.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(pushed, 1, "login should push exactly one NewConnection");
    assert_eq!(online.load(Ordering::SeqCst), 1, "online should be 1 after login");

    // 第二次 Status 应看到 online==1（读的是同一 Arc）。
    let json = do_status_flow(addr).await;
    assert!(json.contains("\"online\":1"), "expected online 1 in {json}");
    // 第二次 Status 本身不额外计数。
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(online.load(Ordering::SeqCst), 1);
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn wrong_protocol_does_not_count() {
    let (addr, rx, online) = spawn_server_with(100, "wrong-proto").await;

    // 错误协议 + Status 意图：直接断开。
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 76, addr.port(), HandshakeNextState::Status).await;
    let mut buf = [0u8; 1];
    let _ = s.read_exact(&mut buf).await.expect_err("should disconnect");

    // 错误协议 + Login 意图：同样直接断开，不计数。
    let mut s2 = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s2, 76, addr.port(), HandshakeNextState::Login).await;
    let _ = s2.read_exact(&mut buf).await.expect_err("should disconnect");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(online.load(Ordering::SeqCst), 0);
    assert!(rx.try_recv().is_err());

    // 首包不是 Handshake：也不计数。
    let mut s3 = TcpStream::connect(addr).await.unwrap();
    let mut garbage = Vec::new();
    packets::encode_status_request_packet(&mut garbage);
    s3.write_all(&garbage).await.unwrap();
    let _ = s3.read_exact(&mut buf).await.expect_err("should disconnect");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(online.load(Ordering::SeqCst), 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn varint_helper_sanity_online_count() {
    let mut b = Vec::new();
    write_varint(300, &mut b);
    let (v, _) = read_varint(&b).unwrap();
    assert_eq!(v, 300);
}
