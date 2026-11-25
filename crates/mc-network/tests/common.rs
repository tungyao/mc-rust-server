#![allow(dead_code)]
//! Play 集成测试共享脚手架（`play_stream.rs` / `play_actions.rs` 共用）。
use mc_network::{NetConfig, PlayEvent, serve};
use mc_protocol::packets::{self, Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID};
use mc_protocol::read_varint;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const VIEW_SIDE: i32 = 4;

#[derive(Debug)]
pub struct JoinObservation {
    pub login: packets::LoginPlay,
    pub center: packets::SetCenterChunk,
    pub spawn: packets::PlayerPositionSync,
    pub first_chunk: packets::PlayChunkData,
    pub chunk_count: usize,
    pub batch_size: i32,
}

pub async fn read_varint_async(stream: &mut TcpStream) -> std::io::Result<i32> {
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
    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "varint too big"))
}

pub async fn read_frame(stream: &mut TcpStream) -> std::io::Result<(i32, Vec<u8>)> {
    let len = read_varint_async(stream).await?;
    assert!(len >= 0, "negative length {len}");
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    let (id, n) = read_varint(&buf).expect("packet id");
    Ok((id, buf[n..].to_vec()))
}

pub async fn read_frame_timeout(
    stream: &mut TcpStream,
    dur: Duration,
) -> std::io::Result<(i32, Vec<u8>)> {
    tokio::time::timeout(dur, read_frame(stream))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "test read timeout"))?
}

pub async fn spawn_server(config: NetConfig) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, _rx) = crossbeam_channel::unbounded();
    tokio::spawn(async move {
        let _ = serve(listener, tx, config).await;
    });
    addr
}

pub async fn send_handshake(stream: &mut TcpStream, port: u16, next: HandshakeNextState) {
    let hs = Handshake {
        protocol_version: 777,
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

/// 走完 Login + Configuration + 初始 `(2r+1)^2` 块 Join 序列，返回处于 Play 的连接。
pub async fn login_and_join(addr: std::net::SocketAddr, name: &str, radius: i32) -> TcpStream {
    login_and_join_observed(addr, name, radius).await.0
}

/// 同 [`login_and_join`]，额外抓取 LoginPlay/SetCenter/PositionSync/第一块 ChunkData。
pub async fn login_and_join_observed(
    addr: std::net::SocketAddr,
    name: &str,
    radius: i32,
) -> (TcpStream, JoinObservation) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, addr.port(), HandshakeNextState::Login).await;

    let mut body = Vec::new();
    packets::encode_string(name, &mut body);
    body.extend_from_slice(&[0u8; 16]);
    let mut start = Vec::new();
    packets::encode_packet(packets::LOGIN_START_PACKET_ID, &body, &mut start);
    s.write_all(&start).await.unwrap();
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::LOGIN_SUCCESS_PACKET_ID);
    let mut ack = Vec::new();
    packets::encode_login_acknowledged_packet(&mut ack);
    s.write_all(&ack).await.unwrap();

    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::CONFIG_KNOWN_PACKS_S2C_ID);
    let mut kp = Vec::new();
    packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kp);
    s.write_all(&kp).await.unwrap();
    for _ in 0..32 {
        let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::CONFIG_REGISTRY_DATA_S2C_ID);
    }
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::CONFIG_UPDATE_TAGS_S2C_ID);
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::CONFIG_FINISH_S2C_ID);
    let mut fack = Vec::new();
    packets::encode_config_finish_ack_packet(&mut fack);
    s.write_all(&fack).await.unwrap();

    let mut login = None;
    let mut center = None;
    let mut spawn = None;
    let mut first_chunk = None;
    let mut chunk_count = 0usize;
    let mut batch_size = None;
    let expected_chunks = mc_network::view_chunk_count(radius);
    let total = 4 + expected_chunks + 3;
    for _ in 0..total {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(10)).await.unwrap();
        match id {
            packets::PLAY_LOGIN_PLAY_ID if login.is_none() => {
                login = Some(packets::LoginPlay::decode_body(&payload).unwrap().0);
            }
            packets::PLAY_SET_CENTER_CHUNK_ID if center.is_none() => {
                center = Some(packets::SetCenterChunk::decode_body(&payload).unwrap().0);
            }
            packets::PLAY_PLAYER_POSITION_SYNC_ID if spawn.is_none() => {
                spawn = Some(packets::PlayerPositionSync::decode_body(&payload).unwrap().0);
            }
            packets::PLAY_CHUNK_DATA_ID => {
                chunk_count += 1;
                if first_chunk.is_none() {
                    first_chunk = Some(packets::PlayChunkData::decode_body(&payload).unwrap().0);
                }
            }
            packets::PLAY_CHUNK_BATCH_FINISHED_ID if batch_size.is_none() => {
                batch_size = Some(read_varint(&payload).unwrap().0);
            }
            _ => {}
        }
    }
    let obs = JoinObservation {
        login: login.expect("join must send LoginPlay"),
        center: center.expect("join must send SetCenterChunk"),
        spawn: spawn.expect("join must send PlayerPositionSync"),
        first_chunk: first_chunk.expect("join must send ChunkData"),
        chunk_count,
        batch_size: batch_size.expect("join must send ChunkBatchFinished"),
    };
    (s, obs)
}

pub async fn send_move(stream: &mut TcpStream, x: f64, z: f64) {
    let mut body = Vec::new();
    packets::MovePlayerPos { x, y: 65.0, z, on_ground: true, horizontal_collision: false }
        .encode_body(&mut body);
    mc_network::framing::write_packet(stream, packets::PLAY_MOVE_POS_C2S_ID, &body)
        .await
        .unwrap();
}

pub async fn send_pos_y(stream: &mut TcpStream, x: f64, y: f64, z: f64) {
    let mut body = Vec::new();
    packets::MovePlayerPos { x, y, z, on_ground: true, horizontal_collision: false }
        .encode_body(&mut body);
    mc_network::framing::write_packet(stream, packets::PLAY_MOVE_POS_C2S_ID, &body).await.unwrap();
}

pub async fn next_event(rx: &crossbeam_channel::Receiver<PlayEvent>) -> PlayEvent {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(ev) = rx.try_recv() {
                return ev;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("expected PlayEvent from net thread")
}
