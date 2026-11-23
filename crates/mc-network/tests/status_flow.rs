//! serve 端到端测试：真实 TcpListener + 客户端走完整 Status 握手。
use mc_network::{NetConfig, serve};
use mc_protocol::packets::{
    self, Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID,
};
use mc_protocol::{read_varint, write_varint};
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

async fn spawn_server() -> std::net::SocketAddr {
    spawn_server_with_config(NetConfig::default()).await.0
}

async fn spawn_server_with_config(
    config: NetConfig,
) -> (std::net::SocketAddr, crossbeam_channel::Receiver<mc_network::NewConnection>, NetConfig) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = crossbeam_channel::unbounded();
    let cfg = config.clone();
    tokio::spawn(async move {
        let _ = serve(listener, tx, cfg).await;
    });
    (addr, rx, config)
}

#[tokio::test]
async fn status_flow_roundtrip() {
    let addr = spawn_server().await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 777, addr.port(), HandshakeNextState::Status).await;

    let mut req = Vec::new();
    packets::encode_status_request_packet(&mut req);
    s.write_all(&req).await.unwrap();

    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, 0x00);
    let (json, _) = packets::decode_status_response(&payload).unwrap();
    assert!(json.contains("26.3"), "missing 26.3 in {json}");
    assert!(json.contains("777"), "missing 777 in {json}");

    let mut ping = Vec::new();
    packets::encode_ping_request_packet(123_456_789, &mut ping);
    s.write_all(&ping).await.unwrap();
    let (id, payload) = read_frame(&mut s).await.unwrap();
    assert_eq!(id, 0x01);
    assert_eq!(packets::decode_pong_payload(&payload).unwrap(), 123_456_789);
}

#[tokio::test]
async fn wrong_protocol_disconnects() {
    let addr = spawn_server().await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, 76, addr.port(), HandshakeNextState::Status).await;

    // 服务端应直接断开：读到 EOF（read_exact 返回 UnexpectedEof，或读到 0 字节）。
    let mut buf = [0u8; 1];
    match s.read_exact(&mut buf).await {
        Ok(_) => panic!("server should have disconnected on protocol 76"),
        Err(e) => assert!(
            e.kind() == std::io::ErrorKind::UnexpectedEof,
            "expected EOF, got {e:?}"
        ),
    }
}

#[test]
fn varint_helper_sanity() {
    // 客户端侧读包 helper 自检（与服务端实现对称）。
    let mut b = Vec::new();
    write_varint(300, &mut b);
    let (v, _) = read_varint(&b).unwrap();
    assert_eq!(v, 300);
}
