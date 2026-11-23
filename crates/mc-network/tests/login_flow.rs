//! Login 离线全流程集成测试：真实 TcpListener + 模拟客户端走完
//! Handshake(Login) → Start → Success → Ack，不断言失败即通过。
//! 同时覆盖：Login 成功才 push NewConnection（Status 不 push）。
use mc_network::{NetConfig, serve};
use mc_protocol::packets::{
    self, Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID, LOGIN_ACKNOWLEDGED_PACKET_ID,
    LOGIN_START_PACKET_ID, LOGIN_SUCCESS_PACKET_ID,
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

/// 全流程：Handshake(Login) → Login Start → Login Success → Login Ack。
/// 服务端回的 Success 用户名必须与发送一致；Ack 后服务端 enter_play 关闭连接（读到 EOF）。
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

    // enter_play 占位实现关闭连接：读到 EOF 即正常结束。
    let mut buf = [0u8; 1];
    match s.read_exact(&mut buf).await {
        Ok(_) => panic!("server should close after enter_play placeholder"),
        Err(e) => assert!(
            e.kind() == std::io::ErrorKind::UnexpectedEof,
            "expected EOF, got {e:?}"
        ),
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
