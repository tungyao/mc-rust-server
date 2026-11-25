//! Play 期真实端口集成测试：
//! 1) C2S 移动包经 crossbeam channel 到达 ECS 事件通道（PlayEvent::Movement/KeepAlive）；
//! 2) 玩家跨区块时 net 线程发 SetCenterChunk + 卸载边界列 + 补发边界列，
//!    ChunkBatchStart/Finished 成对且 batch_size 正确，不重复发送；
//! 3) view_distance 真正驱动初始块数 / LoginPlay 字段 / 跨区块 add-remove 数量。
use mc_network::{ChunkCache, NetConfig, PlayEvent, serve};
use mc_protocol::packets::{self, Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID};
use mc_protocol::read_varint;
use mc_world::{MutableWorld, WorldGen, DEFAULT_SEED};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const VIEW_SIDE: i32 = 4;

#[derive(Debug)]
struct JoinObservation {
    login: packets::LoginPlay,
    center: packets::SetCenterChunk,
    spawn: packets::PlayerPositionSync,
    first_chunk: packets::PlayChunkData,
    chunk_count: usize,
    batch_size: i32,
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

/// 走完 Login + Configuration + 初始 `(2r+1)^2` 块 Join 序列，返回处于 Play 的连接。
async fn login_and_join(addr: std::net::SocketAddr, name: &str, radius: i32) -> TcpStream {
    login_and_join_observed(addr, name, radius).await.0
}

/// 同 [`login_and_join`]，额外抓取 LoginPlay/SetCenter/PositionSync/第一块 ChunkData。
async fn login_and_join_observed(
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
        .with_compression_threshold(64)
        .with_view_distance(4);
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
async fn net_config_seed_drives_spawn_and_chunk_terrain() {
    let seed = 987654321i64;
    let config = NetConfig::new(100, "seed".into(), Arc::new(AtomicUsize::new(0)))
        .with_seed(seed)
        .with_view_distance(4);
    let addr = spawn_server(config).await;
    let (_s, obs) = login_and_join_observed(addr, "SeedProbe", 4).await;
    let world = WorldGen::new(seed);
    let spawn = world.spawn_pos();
    assert_eq!(obs.spawn.x, spawn.0 as f64 + 0.5, "PositionSync x");
    assert_eq!(obs.spawn.y, spawn.1 as f64, "PositionSync y must come from WorldGen");
    assert_eq!(obs.spawn.z, spawn.2 as f64 + 0.5, "PositionSync z");
    assert_eq!(obs.center.chunk_x, 23);
    assert_eq!(obs.center.chunk_z, 23);
    // 中心优先：第一块 ChunkData 就是出生区块，整包内容必须与同 seed WorldGen 一致。
    assert_eq!(obs.first_chunk.chunk_x, 23);
    assert_eq!(obs.first_chunk.chunk_z, 23);
    let expected = world.generate(mc_core::ChunkPos(23, 23));
    assert_eq!(obs.first_chunk.heightmaps_nbt, expected.heightmaps_bytes());
    assert_eq!(obs.first_chunk.section_bytes, expected.network_bytes());
    assert_eq!(obs.first_chunk.light_bytes, expected.light_bytes());
    // 另一个 seed 的出生 y 必须不同（默认 seed 73 / 987654321 = 83）。
    let default_spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    assert_ne!(obs.spawn.y, default_spawn.1 as f64);
}

#[tokio::test]
async fn movement_and_keepalive_packets_reach_play_event_channel() {
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let config = NetConfig::new(100, "events".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(4);
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "EventProbe", 4).await;

    // 出生区块内的移动：只转发事件，不触发动态区块补发。
    send_move(&mut s, 369.5, 381.5).await;
    match next_event(&rx).await {
        PlayEvent::Movement { conn_id, movement, .. } => {
            assert!(conn_id > 0, "conn_id must be assigned at accept");
            match movement {
                packets::C2SMovement::Pos(p) => {
                    assert_eq!((p.x, p.y, p.z), (369.5, 65.0, 381.5));
                    assert!(p.on_ground);
                }
                other => panic!("expected Pos movement, got {other:?}"),
            }
        }
        other => panic!("expected movement event, got {other:?}"),
    }

    // KeepAlive 回显 → ECS 事件（online 递减仍只在 ECS 唯一删除点）。
    let mut body = Vec::new();
    packets::KeepAliveC2S(1).encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_KEEPALIVE_C2S_ID, &body)
        .await
        .unwrap();
    match next_event(&rx).await {
        PlayEvent::KeepAlive { addr: ev_addr, conn_id } => {
            assert_eq!(ev_addr, s.local_addr().unwrap());
            assert!(conn_id > 0);
        }
        other => panic!("expected keepalive event, got {other:?}"),
    }
}

#[tokio::test]
async fn view_distance_two_streams_25_chunks_and_5_crossing() {
    let config = NetConfig::new(100, "radius2".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2);
    let addr = spawn_server(config).await;
    let (mut s, obs) = login_and_join_observed(addr, "RadiusTwo", 2).await;

    // 初始 25 块 + LoginPlay 下发 view_distance=2 / simulation_distance=2。
    assert_eq!(obs.chunk_count, 25, "radius=2 must send (2*2+1)^2=25 chunks");
    assert_eq!(obs.batch_size, 25, "initial batch_size must equal chunk count");
    assert_eq!(obs.login.view_distance, 2, "LoginPlay view_distance must match effective radius");
    assert_eq!(obs.login.simulation_distance, 2, "simulation_distance must not stay at 10");
    assert_eq!(obs.login.seed, DEFAULT_SEED);

    // 出生点 x=369.5 → chunk 23；走到 x=384.5 → chunk 24（跨一格区块边界）。
    send_move(&mut s, 384.5, 381.5).await;

    // SetCenterChunk(24,23) → 卸载西侧 5 块 → 一批补发东侧 5 块。
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CENTER_CHUNK_ID);
    let (center, _) = packets::SetCenterChunk::decode_body(&payload).unwrap();
    assert_eq!(center, packets::SetCenterChunk { chunk_x: 24, chunk_z: 23 });

    let mut unloaded = Vec::new();
    for _ in 0..(2 * 2 + 1) {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_UNLOAD_CHUNK_ID, "expected unload before new batch");
        let (u, _) = packets::UnloadChunk::decode_body(&payload).unwrap();
        unloaded.push((u.chunk_x, u.chunk_z));
    }
    unloaded.sort();
    let expected_unload: Vec<(i32, i32)> = (21..=25).map(|z| (21, z)).collect();
    assert_eq!(unloaded, expected_unload);

    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_CHUNK_BATCH_START_ID);
    let mut loaded = Vec::new();
    for _ in 0..(2 * 2 + 1) {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_CHUNK_DATA_ID);
        let (chunk, used) = packets::PlayChunkData::decode_body(&payload).unwrap();
        assert_eq!(used, payload.len(), "chunk body must be fully consumed");
        loaded.push((chunk.chunk_x, chunk.chunk_z));
    }
    loaded.sort();
    let expected_load: Vec<(i32, i32)> = (21..=25).map(|z| (26, z)).collect();
    assert_eq!(loaded, expected_load);

    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_CHUNK_BATCH_FINISHED_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 5, "crossing batch_size must be 5");
}

#[tokio::test]
async fn crossing_chunk_streams_exact_add_unload_batch_without_duplicates() {
    let config = NetConfig::new(100, "chunks".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(VIEW_SIDE);
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "ChunkWalker", VIEW_SIDE).await;

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

async fn send_pos_y(stream: &mut TcpStream, x: f64, y: f64, z: f64) {
    let mut body = Vec::new();
    packets::MovePlayerPos { x, y, z, on_ground: true, horizontal_collision: false }
        .encode_body(&mut body);
    mc_network::framing::write_packet(stream, packets::PLAY_MOVE_POS_C2S_ID, &body).await.unwrap();
}

/// P17 端到端：挖出生点脚下草方块 → 放石头 → BlockUpdate+Ack 回包 → 事件 → 落盘重读一致。
#[tokio::test]
async fn dig_and_place_roundtrip_with_blockupdate_ack_and_persistence() {
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let config = NetConfig::new(100, "edit".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Editor", 2).await;

    // 出生点脚下是草方块（3×3 干燥平台保证）。
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9, "spawn footing must be grass_block(9)");
    // 先同步服务端 last_pos（判定距离用它），消费掉移动事件。
    send_pos_y(&mut s, fx, fy, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }

    // 挖：Player Action finish digging，seq=11 → BlockUpdate(air) + Ack(11)。
    let mut body = Vec::new();
    packets::PlayerAction { status: 2, x: gx, y: gy, z: gz, face: 1, sequence: 11 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID, "dig must push BlockUpdate first");
    let (u, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
    assert_eq!((u.x, u.y, u.z, u.block_id), (gx, gy, gz, 0), "dug block must become air(0)");
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, "dig must be acked");
    assert_eq!(read_varint(&payload).unwrap().0, 11);
    assert_eq!(mutable.get(gx, gy, gz), 0, "server world must record air");
    match next_event(&rx).await {
        PlayEvent::BlockEdit { x, y, z, old_id, new_id, .. } => {
            assert_eq!((x, y, z, old_id, new_id), (gx, gy, gz, 9, 0));
        }
        other => panic!("expected block edit event, got {other:?}"),
    }

    // 放：点下方泥土顶面，seq=12 → BlockUpdate(stone) + Ack(12)。
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx,
        y: gy - 1,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 12,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID, "place must push BlockUpdate first");
    let (u, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
    assert_eq!((u.x, u.y, u.z, u.block_id), (gx, gy, gz, 1), "placed block must be stone(1)");
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, "place must be acked");
    assert_eq!(read_varint(&payload).unwrap().0, 12);
    assert_eq!(mutable.get(gx, gy, gz), 1, "server world must record stone");
    match next_event(&rx).await {
        PlayEvent::BlockEdit { x, y, z, old_id, new_id, .. } => {
            assert_eq!((x, y, z, old_id, new_id), (gx, gy, gz, 0, 1));
        }
        other => panic!("expected block edit event, got {other:?}"),
    }

    // 断线 → Disconnect 事件；落盘重读：石头仍在（重启不丢）。
    drop(s);
    match next_event(&rx).await {
        PlayEvent::Disconnect { .. } => {}
        other => panic!("expected disconnect event, got {other:?}"),
    }
    let dir = std::env::temp_dir().join(format!("mc-p17-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(mutable.save_to_dir(&dir).unwrap(), 1, "dig+place on one column = 1 override");
    let back = MutableWorld::load_from_dir(&dir, DEFAULT_SEED).unwrap();
    assert_eq!(back.get(gx, gy, gz), 1, "placed stone must survive save/load");
    assert_eq!(back.override_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P18 端到端：Join 背包同步 → 挖草掉 dirt 实体 → 走近拾取 → 背包同步 →
/// 切手持 → 放石头扣 1 → 断线存档重读一致。
#[tokio::test]
async fn dig_spawns_pickable_drop_and_inventory_syncs() {
    use mc_network::DropState;
    use mc_world::PlayerInventory;
    let dir = std::env::temp_dir().join(format!("mc-p18-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "drops".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Dropper", 2).await;

    // Join 背包：全量 Content，46 槽，hotbar 槽 36 = 64 石头开局。
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, "join must sync inventory");
    let (content, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
    assert_eq!(content.window_id, 0);
    assert_eq!(content.slots.len(), 46);
    assert_eq!(content.slots[36], packets::Slot { count: 64, item_id: 1 });

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9);
    send_pos_y(&mut s, fx, fy, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }

    // 挖草：BlockUpdate(air) + Ack + Spawn(掉落物) + Data(dirt×1)。
    let mut body = Vec::new();
    packets::PlayerAction { status: 2, x: gx, y: gy, z: gz, face: 1, sequence: 11 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 0);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 11);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID, "dig must spawn a drop entity");
    let (spawned, _) = packets::SpawnItemEntity::decode_body(&payload).unwrap();
    let drop_id = spawned.entity_id;
    assert!(drop_id >= 1000, "drop ids must not collide with the player entity");
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_ENTITY_DATA_S2C_ID);
    let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
    assert_eq!(data.entity_id, drop_id);
    assert_eq!(data.slot, packets::Slot { count: 1, item_id: 55 }, "grass must drop dirt");
    match next_event(&rx).await {
        PlayEvent::BlockEdit { old_id: 9, new_id: 0, .. } => {}
        other => panic!("expected block edit event, got {other:?}"),
    }

    // 拾取：站在掉落物上（<1.5 格），delay 10 ticks 后 TakeItem + Remove + 全量 Content。
    // 下落同步 Teleport(0x23) 会穿插，先跳过。
    let mut saw_take = false;
    let mut saw_remove = false;
    let mut saw_content = false;
    for _ in 0..60 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_TELEPORT_ENTITY_S2C_ID => continue,
            packets::PLAY_PICKUP_ITEM_S2C_ID => {
                let (t, _) = packets::TakeItemEntity::decode_body(&payload).unwrap();
                assert_eq!((t.collected_id, t.collector_id), (drop_id, 1));
                saw_take = true;
            }
            packets::PLAY_REMOVE_ENTITIES_S2C_ID => {
                let (r, _) = packets::RemoveEntities::decode_body(&payload).unwrap();
                assert!(r.ids.contains(&drop_id));
                saw_remove = true;
            }
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                assert_eq!(c.slots.len(), 46);
                let dirt: i32 = c.slots.iter().filter(|s| **s == packets::Slot { count: 1, item_id: 55 }).count() as i32;
                assert_eq!(dirt, 1, "picked dirt must appear exactly once, got {c:?}");
                saw_content = true;
            }
            other => panic!("unexpected packet {other:#04x} while waiting for pickup"),
        }
        if saw_take && saw_remove && saw_content {
            break;
        }
    }
    assert!(saw_take && saw_remove && saw_content, "pickup animation + remove + inventory sync");

    // 切手持到槽 1（空）再切回槽 0：不断线（无回包，只验证连接存活）。
    for slot in [1i16, 0i16] {
        let body = [(slot >> 8) as u8, slot as u8];
        mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
            .await
            .unwrap();
    }

    // 放石头：手持槽 0 有 64 石头 → 映射 stone(1) → 扣 1 + 单槽同步 63。
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx,
        y: gy - 1,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 12,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 1);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 12);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_SLOT_S2C_ID, "place must sync the held slot");
    let (slot_pkt, _) = packets::SetContainerSlot::decode_body(&payload).unwrap();
    assert_eq!(slot_pkt.slot, 36);
    assert_eq!(slot_pkt.data, packets::Slot { count: 63, item_id: 1 });
    match next_event(&rx).await {
        PlayEvent::BlockEdit { old_id: 0, new_id: 1, .. } => {}
        other => panic!("expected block edit event, got {other:?}"),
    }

    // 断线 → Disconnect；背包存档重读：63 石头 + 1 泥土。
    drop(s);
    match next_event(&rx).await {
        PlayEvent::Disconnect { .. } => {}
        other => panic!("expected disconnect event, got {other:?}"),
    }
    // net 线程断线刷盘是异步的：轮询等文件出现（键为用户名，见 inventory 文档）。
    for _ in 0..100 {
        if PlayerInventory::save_path(&dir, "Dropper", DEFAULT_SEED).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let (back, is_new) = PlayerInventory::load_from_dir(&dir, "Dropper", DEFAULT_SEED).unwrap();
    assert!(!is_new, "reconnect must load the saved backpack");
    assert_eq!(back.get(36), Some((1, 63)), "placed stone must be consumed");
    assert_eq!(back.get(37), Some((55, 1)), "picked dirt must persist in hotbar");
    let _ = std::fs::remove_dir_all(&dir);
}

/// P18 回归：忙连接（持续发移动包，模拟原版客户端每 tick 发包）下拾取仍能发生。
/// 旧实现把掉落 tick 只放在 50ms 读超时分支，包流不断时超时永不触发，
/// pickup_delay 减不下去，掉落物永远捡不起来。本用例在修之前必超时失败。
#[tokio::test]
async fn drop_pickup_works_on_busy_connection() {
    use mc_network::DropState;
    let dir = std::env::temp_dir().join(format!("mc-p18-busy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "drops-busy".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Busy", 2).await;

    // Join 背包同步先消费掉（不断线，只读第一帧 Content 的存在性）。
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);

    // 挖草显式站位后开挖。
    send_pos_y(&mut s, fx, fy, fz).await;
    let mut body = Vec::new();
    packets::PlayerAction { status: 2, x: gx, y: gy, z: gz, face: 1, sequence: 11 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    // BlockUpdate + Ack + Spawn + Data，先消费掉。
    for _ in 0..4 {
        let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert!(
            id == packets::PLAY_BLOCK_UPDATE_S2C_ID
                || id == packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID
                || id == packets::PLAY_SPAWN_ENTITY_S2C_ID
                || id == packets::PLAY_SET_ENTITY_DATA_S2C_ID,
            "unexpected packet {id:#04x} before pickup phase"
        );
    }

    // 忙碌阶段：独立写任务每 10ms 发一个移动包（读超时 50ms 永不触发，
    // 模拟原版客户端每 tick 发包），10s 内必须看到 TakeItem + Remove + Content，
    // 否则掉落 tick 被饿死。读写必须拆分到两个 half，否则读阻塞会拖慢发送节拍。
    let (mut rh, mut wh) = s.into_split();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let stop = Arc::clone(&stop);
        tokio::spawn(async move {
            use std::sync::atomic::Ordering;
            use tokio::io::AsyncWriteExt;
            while !stop.load(Ordering::Relaxed) {
                let mut body = Vec::new();
                packets::MovePlayerPos { x: fx, y: fy, z: fz, on_ground: true, horizontal_collision: false }
                    .encode_body(&mut body);
                let mut framed = Vec::new();
                packets::encode_packet(packets::PLAY_MOVE_POS_C2S_ID, &body, &mut framed);
                if wh.write_all(&framed).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    };
    async fn read_half_frame(rh: &mut tokio::net::tcp::OwnedReadHalf) -> std::io::Result<(i32, Vec<u8>)> {
        use tokio::io::AsyncReadExt;
        let mut num = 0i32;
        let mut shift = 0u32;
        for _ in 0..5 {
            let mut b = [0u8; 1];
            rh.read_exact(&mut b).await?;
            num |= ((b[0] & 0x7F) as i32) << shift;
            if b[0] & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        assert!(num >= 0, "negative length {num}");
        let mut buf = vec![0u8; num as usize];
        rh.read_exact(&mut buf).await?;
        let (id, n) = read_varint(&buf).expect("packet id");
        Ok((id, buf[n..].to_vec()))
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let (mut saw_take, mut saw_remove, mut saw_content) = (false, false, false);
    while std::time::Instant::now() < deadline {
        let frame = tokio::time::timeout(Duration::from_millis(500), read_half_frame(&mut rh))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "test read timeout"));
        let (id, payload) = match frame {
            Err(_) => continue,
            Ok(Ok(r)) => r,
            Ok(Err(e)) => panic!("read failed on busy connection: {e}"),
        };
        match id {
            packets::PLAY_TELEPORT_ENTITY_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            packets::PLAY_PICKUP_ITEM_S2C_ID => {
                let (t, _) = packets::TakeItemEntity::decode_body(&payload).unwrap();
                assert_eq!(t.collector_id, 1);
                saw_take = true;
            }
            packets::PLAY_REMOVE_ENTITIES_S2C_ID => {
                saw_remove = true;
            }
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                assert!(c.slots.iter().any(|sl| *sl == packets::Slot { count: 1, item_id: 55 }));
                saw_content = true;
            }
            other => panic!("unexpected packet {other:#04x} on busy connection"),
        }
        if saw_take && saw_remove && saw_content {
            break;
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = writer.await;
    assert!(saw_take && saw_remove && saw_content, "pickup must happen on a busy connection");
    drop(rh);
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P18.1 端到端：拾取进快捷栏 → 拖拽换槽（ContainerClick）→ 切手持 → 放出对应方块。
/// 回归“拖了但服务端没收到、放出隔壁石头”的分叉 bug。
#[tokio::test]
async fn drag_then_place_uses_held_block() {
    use mc_network::DropState;
    fn click_frame(container: i32, state: i32, slot: i16, button: u8, ty: u8) -> Vec<u8> {
        let mut b = Vec::new();
        mc_protocol::write_varint(container, &mut b);
        mc_protocol::write_varint(state, &mut b);
        b.extend_from_slice(&slot.to_be_bytes());
        b.push(button);
        mc_protocol::write_varint(ty as i32, &mut b);
        mc_protocol::write_varint(0, &mut b);
        b.push(0);
        b
    }
    let dir = std::env::temp_dir().join(format!("mc-p18-drag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "drag".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Dragger", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    send_pos_y(&mut s, fx, fy, fz).await;
    // 挖草出土，站着等拾取（快捷栏优先 → 槽 37）。
    let mut body = Vec::new();
    packets::PlayerAction { status: 2, x: gx, y: gy, z: gz, face: 1, sequence: 11 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let mut saw_dirt_content = false;
    for _ in 0..60 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
            if c.slots.get(37) == Some(&packets::Slot { count: 1, item_id: 55 }) {
                saw_dirt_content = true;
                break;
            }
        }
    }
    assert!(saw_dirt_content, "picked dirt must land in hotbar slot 37");

    // 拖拽：左键拿起 37 号槽 → 左键放到 40 号槽（每次点击服务端都全量同步）。
    for (slot, button) in [(37i16, 0u8), (40i16, 0u8)] {
        let frame = click_frame(0, 1, slot, button, 0);
        mc_network::framing::write_packet(&mut s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
            .await
            .unwrap();
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, "click must full-sync");
        let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
        if slot == 37 {
            assert_eq!(c.carried, packets::Slot { count: 1, item_id: 55 }, "pickup must lift to cursor");
            assert_eq!(c.slots[37], packets::Slot::empty());
        } else {
            assert_eq!(c.slots[40], packets::Slot { count: 1, item_id: 55 }, "drop must land in 40");
            assert_eq!(c.carried, packets::Slot::empty());
        }
    }

    // 切手持到 4（槽 40）→ 放：必须落 dirt(10) 而不是石头。
    let body = [0u8, 4u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();
    let mut ubody = Vec::new();
    packets::UseItemOn {
        hand: 0, x: gx, y: gy - 1, z: gz, face: 1,
        cursor_x: 0.5, cursor_y: 1.0, cursor_z: 0.5,
        inside_block: false, world_border_hit: false, sequence: 12,
    }
    .encode_body(&mut ubody);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &ubody)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(
        packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 10,
        "held dirt must place dirt, not fallback stone"
    );
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 回归：远离出生点挖方块仍必须生成掉落物。
/// 之前 spawn_drops 用 `drop_in_view(view.center(), ...)` 做门禁；如果视距中心尚未
/// 跟随移动更新（或半径很小），远处的掉落物会被整包丢弃，表现为“走远了打碎啥也没有”。
#[tokio::test]
async fn dig_far_from_spawn_still_spawns_drop() {
    use mc_network::DropState;
    let dir = std::env::temp_dir().join(format!("mc-far-drop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "far-drop".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "FarDig", 2).await;

    // Join  Content（背包），之后才能收到掉落相关包。
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    // 走到远离出生点的位置（x=600,z=700，距离 spawn ~370 格）。
    let world = WorldGen::new(DEFAULT_SEED);
    let (fx, fz): (f64, f64) = (600.5, 700.5);
    let top = world.height_at(fx.floor() as i32, fz.floor() as i32);
    let feet_y = (top + 1) as f64;
    let (gx, gy, gz) = (fx.floor() as i32, top, fz.floor() as i32);
    let block = mutable.get(gx, gy, gz);
    assert_ne!(block, 0, "far target must be solid");
    assert_ne!(block, 89, "far target must not be water");
    send_pos_y(&mut s, fx, feet_y, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }

    let mut body = Vec::new();
    packets::PlayerAction { status: 2, x: gx, y: gy, z: gz, face: 1, sequence: 21 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    // 远距离移动会先触发跨视距批次（SetCenterChunk/UnloadChunk/ChunkData...），
    // 一直读到 BlockUpdate 为止。
    let u_payload = loop {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_BLOCK_UPDATE_S2C_ID {
            break payload;
        }
    };
    let (u, _) = packets::BlockUpdate::decode_body(&u_payload).unwrap();
    assert_eq!((u.x, u.y, u.z, u.block_id), (gx, gy, gz, 0), "far dig must push BlockUpdate");
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 21);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID, "far dig must still spawn a drop");
    let (spawned, _) = packets::SpawnItemEntity::decode_body(&payload).unwrap();
    assert!((spawned.y - (gy as f64 + 0.5)).abs() < 1.5,
        "drop must appear near the broken block, got y={}", spawned.y);
    drop(s);
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 回归：在远离出生点的地方挖地下方块（头顶有地表/树冠），掉落物必须停在挖掘点
/// 附近，而不是被“整列最高方块”抬到地表（用户反馈“不同高度打碎没掉落”）。
#[tokio::test]
async fn dig_underground_far_from_spawn_drops_near_the_hole() {
    use mc_network::DropState;
    let dir = std::env::temp_dir().join(format!("mc-underground-drop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "underground-drop".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Miner", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let world = WorldGen::new(DEFAULT_SEED);
    let (bx, bz) = (600i32, 700i32);
    let top = world.height_at(bx, bz);
    let (gx, gy, gz) = (bx, top - 5, bz); // 地表下 5 格
    let block = mutable.get(gx, gy, gz);
    assert_ne!(block, 0, "underground target must be solid");
    assert_ne!(block, 89, "underground target must not be water");

    // 玩家“站”在基坑里（脚 y = 被挖方块 y + 1），满足 reach 判定。
    let feet = (gx as f64 + 0.5, (gy + 1) as f64, gz as f64 + 0.5);
    send_pos_y(&mut s, feet.0, feet.1, feet.2).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }

    let mut body = Vec::new();
    packets::PlayerAction { status: 2, x: gx, y: gy, z: gz, face: 1, sequence: 31 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let _ = loop {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_BLOCK_UPDATE_S2C_ID {
            break payload;
        }
    };
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID, "underground dig must spawn a drop");
    let (spawned, _) = packets::SpawnItemEntity::decode_body(&payload).unwrap();
    let drop_id = spawned.entity_id;

    // 关键：服务端权威静息高度必须仍在挖掘点附近。
    // 旧实现用 support_top_y 取“整列最高方块”，掉落物会被钳到地表 top+1，
    // 地下玩家既看不见也捡不到（用户反馈“不同高度打碎没掉落”）。
    let rest_y = drops.items.lock().unwrap().get(drop_id).map(|e| e.rest_y);
    assert!(
        rest_y.map(|y| (y - gy as f64).abs() < 1.5).unwrap_or(false),
        "server drop rest_y must be near dug block y={gy}, got {rest_y:?}"
    );
    drop(s);
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 模拟原版客户端完整挖掘流程：逐步走到远处 → START_DIG → 持续移动包 → FINISH_DIG。
/// 用于复现“原版客户端走远后地表方块不掉落”。
#[tokio::test]
async fn vanilla_style_walk_then_dig_far_spawns_drop() {
    use mc_network::DropState;
    let dir = std::env::temp_dir().join(format!("mc-vanilla-walk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "vanilla-walk".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Walker", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let world = WorldGen::new(DEFAULT_SEED);
    let spawn = world.spawn_pos();
    // 模拟“走远”：小步移动多次（每步跨约半个区块），原版客户端会持续发 PosRot。
    let (target_x, target_z) = (600.5, 700.5);
    let steps = 24;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let x = spawn.0 as f64 + 0.5 + (target_x - spawn.0 as f64 - 0.5) * t;
        let z = spawn.2 as f64 + 0.5 + (target_z - spawn.2 as f64 - 0.5) * t;
        let h = world.height_at(x.floor() as i32, z.floor() as i32);
        send_pos_y(&mut s, x, (h + 1) as f64, z).await;
    }
    // 给服务端一点时间处理移动包（TCP 顺序保证 view.center() 已更新）。
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (gx, gz) = (target_x.floor() as i32, target_z.floor() as i32);
    let gy = world.height_at(gx, gz);
    let block = mutable.get(gx, gy, gz);
    assert_ne!(block, 0, "target must be solid");
    let feet = (target_x, (gy + 1) as f64, target_z);

    // 原版：START_DIG（进度动画）→ 若干移动包 → FINISH_DIG。
    let mut body = Vec::new();
    packets::PlayerAction { status: 0, x: gx, y: gy, z: gz, face: 1, sequence: 40 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body).await.unwrap();
    // START_DIG 的 ack 不应破坏方块（view move 包可能仍在流里，循环找 ack）。
    // 走远会触发大量跨区块补发（radius=2 时约 380 个包），循环找 ack。
    let mut saw_start_ack = false;
    for _ in 0..2000 {
        let (id, _) = read_frame_timeout(&mut s, Duration::from_millis(100)).await.unwrap();
        if id == packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID {
            saw_start_ack = true;
            break;
        }
    }
    assert!(saw_start_ack, "START_DIG must be acked");
    assert_eq!(mutable.get(gx, gy, gz), block, "START_DIG must not break");

    // 挖掘期间原版客户端继续发移动包。
    body.clear();
    packets::MovePlayerPos {
        x: feet.0, y: feet.1, z: feet.2, on_ground: true, horizontal_collision: false,
    }.encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_MOVE_POS_C2S_ID, &body).await.unwrap();

    let mut body = Vec::new();
    packets::PlayerAction { status: 2, x: gx, y: gy, z: gz, face: 1, sequence: 41 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body).await.unwrap();

    let mut saw_update = false;
    let mut saw_spawn = false;
    for _ in 0..200 {
        let (id, _payload) = match read_frame_timeout(&mut s, Duration::from_millis(100)).await.ok() {
            Some(v) => v,
            None => continue,
        };
        if id == packets::PLAY_BLOCK_UPDATE_S2C_ID {
            saw_update = true;
        }
        if id == packets::PLAY_SPAWN_ENTITY_S2C_ID {
            saw_spawn = true;
            break;
        }
    }
    assert!(saw_update, "vanilla-style far dig must break block (BlockUpdate)");
    assert!(saw_spawn, "vanilla-style far dig must spawn a drop");
    drop(s);
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 落地高度重算：掉落物静息后其支撑方块被挖，服务端必须重算 rest_y 并让
/// 掉落物继续下落，而不是悬空在原高度。
#[tokio::test]
async fn drop_recomputes_rest_y_when_support_is_dug() {
    use mc_network::DropState;
    let dir = std::env::temp_dir().join(format!("mc-recompute-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "recompute".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Digger", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    let feet = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    send_pos_y(&mut s, feet.0, feet.1, feet.2).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement, got {other:?}"),
    }

    // 挖草 → 掉落 dirt。
    let mut body = Vec::new();
    packets::PlayerAction { status: 2, x: gx, y: gy, z: gz, face: 1, sequence: 51 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body).await.unwrap();
    let mut drop_id = None;
    for _ in 0..50 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_millis(100)).await.unwrap();
        if id == packets::PLAY_SPAWN_ENTITY_S2C_ID {
            drop_id = Some(packets::SpawnItemEntity::decode_body(&payload).unwrap().0.entity_id);
            break;
        }
    }
    let drop_id = drop_id.expect("dig must spawn a drop");

    // 玩家走开 3 格（同 chunk，不触发视距批次），避免掉落物被立即拾取。
    send_pos_y(&mut s, feet.0 + 3.0, feet.1, feet.2).await;

    // 等掉落物落地。
    let mut landed = false;
    for _ in 0..100 {
        let snap = drops.items.lock().unwrap().get(drop_id).map(|e| e.on_ground);
        if snap == Some(true) {
            landed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(landed, "drop must land before support is dug");
    let old = drops.items.lock().unwrap().get(drop_id)
        .map(|e| (e.rest_y, e.support)).unwrap();
    assert_eq!(old.1, Some((gx, gy - 1, gz)), "support must be the dirt under the grass");

    // 挖掉支撑方块（泥土）。
    assert_eq!(mutable.get(gx, gy - 1, gz), mc_world::DIRT_BLOCK_ID);
    mutable.set(gx, gy - 1, gz, mc_world::AIR_BLOCK_ID);

    // 等服务端 tick 检测到悬空并重算。
    let mut recomputed = None;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let snap = drops.items.lock().unwrap().get(drop_id)
            .map(|e| (e.rest_y, e.on_ground));
        if snap.map(|(r, _)| r < old.0).unwrap_or(false) {
            recomputed = snap;
            break;
        }
    }
    let (new_rest, on_ground) = recomputed.expect("rest_y must drop after support is dug");
    assert_eq!(new_rest, (gy - 1) as f64, "new rest must be top of the next block down");
    assert!(!on_ground, "drop must resume falling after recompute");
    drop(s);
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}
