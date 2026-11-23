//! Play 期真实端口集成测试：
//! 1) C2S 移动包经 crossbeam channel 到达 ECS 事件通道（PlayEvent::Movement/KeepAlive）；
//! 2) 玩家跨区块时 net 线程发 SetCenterChunk + 卸载 9 块 + 补发 9 块，
//!    ChunkBatchStart/Finished 成对且 batch_size 正确，不重复发送。
use mc_network::{NetConfig, PlayEvent, serve};
use mc_protocol::packets::{self, Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID};
use mc_protocol::read_varint;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const VIEW_SIDE: i32 = 4;
const INITIAL_JOIN_PACKETS: usize = 4 + 81 + 3; // Login/SetCenter/GameEvent/BatchStart + 81 + Finished/Pos/KeepAlive

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
    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "varint too big"))
}

async fn read_frame(stream: &mut TcpStream) -> std::io::Result<(i32, Vec<u8>)> {
    let len = read_varint_async(stream).await?;
    assert!(len >= 0, "negative length {len}");
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    let (id, n) = read_varint(&buf).expect("packet id");
    Ok((id, buf[n..].to_vec()))
}

async fn read_frame_timeout(stream: &mut TcpStream, dur: Duration) -> std::io::Result<(i32, Vec<u8>)> {
    tokio::time::timeout(dur, read_frame(stream))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "test read timeout"))?
}

async fn spawn_server(config: NetConfig) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, _rx) = crossbeam_channel::unbounded();
    tokio::spawn(async move {
        let _ = serve(listener, tx, config).await;
    });
    addr
}

async fn send_handshake(stream: &mut TcpStream, port: u16, next: HandshakeNextState) {
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

/// 走完 Login + Configuration + 初始 81 块 Join 序列，返回处于 Play 的连接。
async fn login_and_join(addr: std::net::SocketAddr, name: &str) -> TcpStream {
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

    for _ in 0..INITIAL_JOIN_PACKETS {
        read_frame_timeout(&mut s, Duration::from_secs(10)).await.unwrap();
    }
    s
}

async fn send_move(stream: &mut TcpStream, x: f64, z: f64) {
    let mut body = Vec::new();
    packets::MovePlayerPos { x, y: 65.0, z, on_ground: true, horizontal_collision: false }
        .encode_body(&mut body);
    mc_network::framing::write_packet(stream, packets::PLAY_MOVE_POS_C2S_ID, &body)
        .await
        .unwrap();
}

async fn next_event(rx: &crossbeam_channel::Receiver<PlayEvent>) -> PlayEvent {
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

#[tokio::test]
async fn serve_honors_net_config_compression_threshold() {
    use mc_network::framing::decode_compressed_frame;
    let config = NetConfig::new(100, "compressed".into(), Arc::new(AtomicUsize::new(0)))
        .with_compression_threshold(64);
    let addr = spawn_server(config).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, addr.port(), HandshakeNextState::Login).await;

    let mut body = Vec::new();
    packets::encode_string("CompressedProbe", &mut body);
    body.extend_from_slice(&[0u8; 16]);
    let mut start = Vec::new();
    packets::encode_packet(packets::LOGIN_START_PACKET_ID, &body, &mut start);
    s.write_all(&start).await.unwrap();

    // 第一包必须是明文 SetCompression(0x03)，阈值来自 NetConfig。
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::SET_COMPRESSION_PACKET_ID);
    assert_eq!(packets::decode_set_compression(&payload).unwrap(), 64);

    // 之后 Login Success 走压缩帧（小包 data_len=0 直通，但仍带压缩帧头）。
    let len = read_varint_async(&mut s).await.unwrap();
    let mut frame = vec![0u8; len as usize];
    s.read_exact(&mut frame).await.unwrap();
    let (id, payload) = decode_compressed_frame(&frame).unwrap();
    assert_eq!(id, packets::LOGIN_SUCCESS_PACKET_ID);
    let (success, _) = packets::decode_login_success(&payload).unwrap();
    assert_eq!(success.username, "CompressedProbe");
}

#[tokio::test]
async fn movement_and_keepalive_packets_reach_play_event_channel() {
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let config = NetConfig::new(100, "events".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx);
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "EventProbe").await;

    // 出生区块内的移动：只转发事件，不触发动态区块补发。
    send_move(&mut s, 369.5, 381.5).await;
    match next_event(&rx).await {
        PlayEvent::Movement { movement, .. } => match movement {
            packets::C2SMovement::Pos(p) => {
                assert_eq!((p.x, p.y, p.z), (369.5, 65.0, 381.5));
                assert!(p.on_ground);
            }
            other => panic!("expected Pos movement, got {other:?}"),
        },
        other => panic!("expected movement event, got {other:?}"),
    }

    // KeepAlive 回显 → ECS 事件（online 递减仍只在 ECS timeout 系统）。
    let mut body = Vec::new();
    packets::KeepAliveC2S(1).encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_KEEPALIVE_C2S_ID, &body)
        .await
        .unwrap();
    match next_event(&rx).await {
        PlayEvent::KeepAlive { addr: ev_addr } => assert_eq!(ev_addr, s.local_addr().unwrap()),
        other => panic!("expected keepalive event, got {other:?}"),
    }
}

#[tokio::test]
async fn crossing_chunk_streams_exact_add_unload_batch_without_duplicates() {
    let config = NetConfig::new(100, "chunks".into(), Arc::new(AtomicUsize::new(0)));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "ChunkWalker").await;

    // 出生点 x=369.5 → chunk 23；走到 x=384.5 → chunk 24（跨 1 格区块边界）。
    send_move(&mut s, 384.5, 381.5).await;

    // 1) 先发 SetCenterChunk(24,23)。
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CENTER_CHUNK_ID);
    let (center, _) = packets::SetCenterChunk::decode_body(&payload).unwrap();
    assert_eq!(center, packets::SetCenterChunk { chunk_x: 24, chunk_z: 23 });

    // 2) 卸载旧视距最西一列：x=19, z=19..27。
    let mut unloaded = Vec::new();
    for _ in 0..(2 * VIEW_SIDE + 1) {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_UNLOAD_CHUNK_ID, "expected unload before new batch");
        let (u, _) = packets::UnloadChunk::decode_body(&payload).unwrap();
        unloaded.push((u.chunk_x, u.chunk_z));
    }
    unloaded.sort();
    let expected_unload: Vec<(i32, i32)> = (19..=27).map(|z| (19, z)).collect();
    assert_eq!(unloaded, expected_unload);

    // 3) 一批新区块：BatchStart + 9×ChunkData(新最东一列 x=28) + BatchFinished(9)。
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_CHUNK_BATCH_START_ID);
    let mut loaded = Vec::new();
    for _ in 0..(2 * VIEW_SIDE + 1) {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_CHUNK_DATA_ID);
        let (chunk, used) = packets::PlayChunkData::decode_body(&payload).unwrap();
        assert_eq!(used, payload.len(), "chunk body must be fully consumed");
        assert_eq!(chunk.chunk_x, 28, "only the new east column is sent");
        loaded.push((chunk.chunk_x, chunk.chunk_z));
    }
    loaded.sort();
    let expected_load: Vec<(i32, i32)> = (19..=27).map(|z| (28, z)).collect();
    assert_eq!(loaded, expected_load);
    loaded.dedup();
    assert_eq!(loaded.len(), 9, "no duplicate chunk data in one batch");

    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_CHUNK_BATCH_FINISHED_ID);
    let (batch_size, _) = read_varint(&payload).unwrap();
    assert_eq!(batch_size, 9, "ChunkBatchFinished batch_size must equal sent chunk count");

    // 4) 同区块内再移动：不发任何区块包（300ms 只能超时）。
    send_move(&mut s, 385.5, 381.5).await;
    assert!(
        read_frame_timeout(&mut s, Duration::from_millis(300)).await.is_err(),
        "same-chunk movement must not resend chunks"
    );

    // 5) 继续跨到 chunk 25：再补 9 块、卸 9 块，边界对称。
    send_move(&mut s, 400.5, 381.5).await;
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CENTER_CHUNK_ID);
    let (center, _) = packets::SetCenterChunk::decode_body(&payload).unwrap();
    assert_eq!(center, packets::SetCenterChunk { chunk_x: 25, chunk_z: 23 });
    let mut unloaded2 = Vec::new();
    for _ in 0..9 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_UNLOAD_CHUNK_ID);
        let (u, _) = packets::UnloadChunk::decode_body(&payload).unwrap();
        unloaded2.push((u.chunk_x, u.chunk_z));
    }
    assert!(unloaded2.iter().all(|&(x, _)| x == 20));
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_CHUNK_BATCH_START_ID);
    let mut loaded2 = Vec::new();
    for _ in 0..9 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_CHUNK_DATA_ID);
        let (chunk, _) = packets::PlayChunkData::decode_body(&payload).unwrap();
        loaded2.push((chunk.chunk_x, chunk.chunk_z));
    }
    assert!(loaded2.iter().all(|&(x, _)| x == 29));
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_CHUNK_BATCH_FINISHED_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 9);
}
