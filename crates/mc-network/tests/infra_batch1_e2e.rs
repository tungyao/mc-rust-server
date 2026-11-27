//! P54 A3 双连接 e2e：默认关时广播包流与基线一致（BlockUpdate→Ack 相邻、Ack 绝不广播）；
//! 显式开启限流后视者仍能在 50ms 节拍收到方块更新，操作者流不受影响。
mod common;

use common::*;
use mc_network::{BroadcastHub, NetConfig, PlayEvent};
use mc_protocol::{packets, read_varint};
use mc_world::{WorldGen, DEFAULT_SEED};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

fn config(hub: Arc<BroadcastHub>, throttle: bool) -> (NetConfig, crossbeam_channel::Receiver<PlayEvent>) {
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let cfg = NetConfig::new(100, "p54-a3".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_play_events(tx)
        .with_hub(hub)
        .with_mutable_world(Arc::new(mc_world::MutableWorld::new(DEFAULT_SEED)))
        .with_broadcast_throttle(throttle);
    (cfg, rx)
}

async fn dig_footing(stream: &mut tokio::net::TcpStream, rx: &crossbeam_channel::Receiver<PlayEvent>) {
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    send_pos_y(stream, fx, fy, fz).await;
    match next_event(rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x: gx, y: gy, z: gz, face: 1, sequence: 77 }.encode_body(&mut body);
    mc_network::framing::write_packet(stream, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
}

fn expect_block_update(id: i32, payload: &[u8]) -> packets::BlockUpdate {
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID, "expected BlockUpdate, got {id:#04x}");
    packets::BlockUpdate::decode_body(payload).unwrap().0
}

#[tokio::test]
async fn throttle_default_off_keeps_baseline_two_connection_packet_stream() {
    let hub = Arc::new(BroadcastHub::new());
    let (cfg, rx) = config(Arc::clone(&hub), false);
    assert!(!cfg.broadcast_throttle, "A3 must be opt-in");
    let addr = spawn_server(cfg).await;
    let mut a = login_and_join(addr, "ThrottleOffA", 2).await;
    let mut b = login_and_join(addr, "ThrottleOffB", 2).await;
    dig_footing(&mut a, &rx).await;

    // 操作者：BlockUpdate → Ack(seq) 相邻，Ack 绝不进 hub。
    let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    let update_a = expect_block_update(id, &payload);
    let (id, ack) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&ack).unwrap().0, 77);

    // 视者：同 tick 直接收到 BlockUpdate（未限流），且永不收到 Ack。
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    let update_b = expect_block_update(id, &payload);
    assert_eq!(update_b, update_a);
    for _ in 0..4 {
        match read_frame_timeout(&mut b, Duration::from_millis(300)).await {
            Ok((id, _)) => assert_ne!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, "Ack must never be broadcast"),
            Err(_) => break,
        }
    }
    assert_eq!(hub.throttle_stats(), (0, 0, 0), "baseline must not use the merge queue");
}

#[tokio::test]
async fn throttle_on_still_delivers_updates_and_keeps_ack_adjacent() {
    let hub = Arc::new(BroadcastHub::new());
    let (cfg, rx) = config(Arc::clone(&hub), true);
    let addr = spawn_server(cfg).await;
    let mut a = login_and_join(addr, "ThrottleOnA", 2).await;
    let mut b = login_and_join(addr, "ThrottleOnB", 2).await;
    dig_footing(&mut a, &rx).await;

    let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    let update_a = expect_block_update(id, &payload);
    let (id, ack) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, "throttle must not touch Ack order");
    assert_eq!(read_varint(&ack).unwrap().0, 77);

    // 视者包被延后到本连接的 50ms drain，但仍能收到同一份方块更新。
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(expect_block_update(id, &payload), update_a);
    assert!(hub.throttle_enabled());
}
