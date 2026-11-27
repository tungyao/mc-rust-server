//! P52 方案 B（ECS socket I/O）逐字节 / 生命周期 e2e。
//! 复用 `common.rs` 的客户端基线工具；不修改既有 509 个测试。
mod common;

use common::{
    login_and_join, next_event, read_frame, read_frame_timeout, send_handshake, send_pos_y,
};
use mc_network::{
    BroadcastHub, ChunkCache, DropState, NetConfig, PlayEvent, SocketEcsRuntime, SocketMode,
};
use mc_protocol::packets::{self, HandshakeNextState};
use mc_protocol::read_varint;
use mc_world::{MutableWorld, WorldGen, DEFAULT_SEED};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

/// 启动 `serve()` + ECS 运行时；测试里的 2ms pump 模拟 Bevy Update 的
/// `socket_ecs_system::drain_accept()`（只 try_recv + spawn，不做 IO）。
async fn spawn_ecs_server(config: NetConfig) -> (std::net::SocketAddr, Arc<SocketEcsRuntime>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, _rx) = crossbeam_channel::unbounded();
    // 测试直接复用 `#[tokio::test]` 的 runtime（生产由 main 创建独立 mc-net runtime）。
    let runtime = Arc::new(SocketEcsRuntime::new(
        tokio::runtime::Handle::current(),
        config.clone(),
        tx.clone(),
    ));
    let cfg = config
        .with_socket_mode(SocketMode::Ecs)
        .with_socket_accept(runtime.incoming_sender());
    tokio::spawn(async move {
        let _ = mc_network::serve(listener, tx, cfg).await;
    });
    let pump_rt = Arc::clone(&runtime);
    tokio::spawn(async move {
        loop {
            pump_rt.drain_accept();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    (addr, runtime)
}

fn base_config(view: i32) -> NetConfig {
    NetConfig::new(100, "p52".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(view)
        .with_seed(DEFAULT_SEED)
}

/// 走完 Handshake/Login/Configuration，停在 Play 首帧之前。
async fn login_to_play_start(addr: std::net::SocketAddr, name: &str) -> TcpStream {
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
    s
}

async fn capture_join(s: &mut TcpStream, radius: i32) -> Vec<(i32, Vec<u8>)> {
    let mut frames = Vec::new();
    for _ in 0..(4 + mc_network::view_chunk_count(radius) as usize + 3) {
        frames.push(read_frame_timeout(s, Duration::from_secs(10)).await.unwrap());
    }
    frames
}

#[tokio::test]
async fn ecs_join_eight_segments_are_byte_exact_and_match_net_mode() {
    let ecs_cfg = base_config(2);
    let net_cfg = base_config(2);
    let (ecs_addr, runtime) = spawn_ecs_server(ecs_cfg).await;
    let net_addr = common::spawn_server(net_cfg).await;

    let mut ecs = login_to_play_start(ecs_addr, "EcsJoin").await;
    let mut net = login_to_play_start(net_addr, "NetJoin").await;
    let ecs_frames = capture_join(&mut ecs, 2).await;
    let net_frames = capture_join(&mut net, 2).await;

    let ids: Vec<i32> = ecs_frames.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        &ids[..4],
        &[
            packets::PLAY_LOGIN_PLAY_ID,
            packets::PLAY_SET_CENTER_CHUNK_ID,
            packets::PLAY_GAME_EVENT_ID,
            packets::PLAY_CHUNK_BATCH_START_ID,
        ],
        "Join 前四段包序必须不变"
    );
    assert!(
        ids[4..29].iter().all(|id| *id == packets::PLAY_CHUNK_DATA_ID),
        "必须整批发完 (2r+1)^2 = 25 块 ChunkData"
    );
    assert_eq!(
        &ids[29..],
        &[
            packets::PLAY_CHUNK_BATCH_FINISHED_ID,
            packets::PLAY_PLAYER_POSITION_SYNC_ID,
            packets::PLAY_KEEPALIVE_S2C_ID,
        ],
        "BatchFinished → PositionSync → KeepAlive 包序必须不变"
    );
    assert_eq!(ecs_frames, net_frames, "ECS 与 net 模式的 Join 逐字节一致");
    let (accepted, registered, retired, queued) = runtime.stats();
    assert_eq!(accepted, 1);
    assert_eq!(registered, 1);
    assert!(queued > 0, "写队列必须真实承载过 Join 字节");
    assert_eq!(retired, 0);
}

#[tokio::test]
async fn ecs_dig_blockupdate_ack_adjacent_and_no_ack_broadcast() {
    let drop_dir = std::env::temp_dir().join(format!("mc-p52-e2e-drops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&drop_dir);
    let hub = Arc::new(BroadcastHub::new());
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(drop_dir.clone(), DEFAULT_SEED));
    let cfg = base_config(2)
        .with_play_events(tx)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let (addr, runtime) = spawn_ecs_server(cfg).await;
    let mut a = login_and_join(addr, "EcsMiner", 2).await;
    let mut b = login_and_join(addr, "EcsViewer", 2).await;
    // 两边 Join 尾部的全量背包同步包先消费掉。
    let _ = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    let _ = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9);
    send_pos_y(&mut a, gx as f64 + 0.5, gy as f64 + 1.0, gz as f64 + 0.5).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }

    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x: gx, y: gy, z: gz, face: 1, sequence: 77 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut a, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();

    // A：BlockUpdate 与 Ack(seq) 在同一条写流上严格相邻。
    let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    let (u, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
    assert_eq!((u.x, u.y, u.z, u.block_id), (gx, gy, gz, 0));
    let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, "Ack 必须紧随 BlockUpdate");
    assert_eq!(read_varint(&payload).unwrap().0, 77);

    // B：能收到 BlockUpdate，但绝不能收到 Ack（旁人用自己的 sequence 解释会错位）。
    let (bid, bpayload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(bid, packets::PLAY_BLOCK_UPDATE_S2C_ID, "视距内玩家必须收到方块广播");
    let (bu, _) = packets::BlockUpdate::decode_body(&bpayload).unwrap();
    assert_eq!((bu.x, bu.y, bu.z, bu.block_id), (gx, gy, gz, 0));
    // B 可能收到掉落 Spawn/Data 等广播，但其中绝不能出现 Ack。
    let mut extra = Vec::new();
    for _ in 0..12 {
        match tokio::time::timeout(Duration::from_millis(40), read_frame(&mut b)).await {
            Ok(Ok(frame)) => extra.push(frame),
            _ => break,
        }
    }
    assert!(
        extra.iter().all(|(id, _)| *id != packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID),
        "Ack 绝不进 hub 广播"
    );

    let (_, registered, _, _) = runtime.stats();
    assert_eq!(registered, 2, "双连接都由 ECS 登记");
    drop(a);
    drop(b);
    tokio::time::sleep(Duration::from_millis(120)).await;
    let (_, _, retired, _) = runtime.stats();
    assert_eq!(retired, 2, "断线后 ECS 任务句柄必须清理");
    let _ = std::fs::remove_dir_all(&drop_dir);
}

#[tokio::test]
async fn ecs_movement_keepalive_and_disconnect_reach_play_events() {
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let cfg = base_config(2).with_play_events(tx);
    let (addr, runtime) = spawn_ecs_server(cfg).await;
    let mut s = login_and_join(addr, "EcsMover", 2).await;

    send_pos_y(&mut s, 370.5, 75.0, 381.5).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement, got {other:?}"),
    }
    let mut body = Vec::new();
    packets::KeepAliveC2S(9).encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_KEEPALIVE_C2S_ID, &body)
        .await
        .unwrap();
    match next_event(&rx).await {
        PlayEvent::KeepAlive { .. } => {}
        other => panic!("expected keepalive, got {other:?}"),
    }
    drop(s);
    match next_event(&rx).await {
        PlayEvent::Disconnect { .. } => {}
        other => panic!("expected disconnect, got {other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(runtime.active_connections(), 0, "断线必须清除 ECS 连接登记");
}
