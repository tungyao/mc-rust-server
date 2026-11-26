//! P43：双连接玩家实体互见 e2e（A 进服 B 收到 Spawn、A 移动 B 收到位置包、
//! A 断线 B 收到 Remove；B 全程不得收到 Ack）。共享脚手架见 common.rs。
//! 与 P21 旧 e2e 不同：这里显式 `.with_player_visibility(true)`——旧入口默认关闭，
//! 保证既有包流一字不变。
mod common;

use common::*;
use mc_network::{BroadcastHub, NetConfig, PlayEvent};
use mc_protocol::packets;
use mc_world::{WorldGen, DEFAULT_SEED};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

async fn read_player_spawn(stream: &mut tokio::net::TcpStream) -> packets::SpawnPlayerEntity {
    for _ in 0..16 {
        let (id, payload) = read_frame_timeout(stream, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                return packets::SpawnPlayerEntity::decode_body(&payload).unwrap().0;
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                panic!("Ack must never be broadcast to a viewer")
            }
            other => panic!("unexpected packet {other:#04x} before player Spawn"),
        }
    }
    panic!("player Spawn not received");
}

#[tokio::test]
async fn two_connections_see_spawn_movement_and_disconnect() {
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p43-visible".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_play_events(tx)
        .with_hub(Arc::clone(&hub))
        .with_player_visibility(true);
    let addr = spawn_server(config).await;
    let mut a = login_and_join(addr, "VisibleA", 2).await;
    let mut b = login_and_join(addr, "VisibleB", 2).await;

    // Join 注册：双方各收到对方的玩家实体 Spawn（entity_id 唯一且不是自己的 1）。
    let spawn_b_seen_by_a = read_player_spawn(&mut a).await;
    let spawn_a_seen_by_b = read_player_spawn(&mut b).await;
    let id_a = spawn_a_seen_by_b.entity_id;
    let id_b = spawn_b_seen_by_a.entity_id;
    assert_ne!(id_a, 1);
    assert_ne!(id_b, 1);
    assert_ne!(id_a, id_b);
    assert!(id_a >= mc_network::FIRST_PLAYER_ENTITY_ID);
    assert!(id_b >= mc_network::FIRST_PLAYER_ENTITY_ID);
    assert_eq!(spawn_a_seen_by_b.vel, packets::LpVec3::zero());
    assert_eq!(spawn_a_seen_by_b.yaw, 0);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);

    // A 位置包：B 收到 move_entity_pos(54)，差量 = 1 格；Ack 只回操作者。
    send_pos_y(&mut a, fx, fy, fz).await;
    let conn_a = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_UPDATE_ENTITY_POS_S2C_ID, "Pos must use move_entity_pos");
    let mv = packets::UpdateEntityPos::decode_body(&payload).unwrap().0;
    assert_eq!(mv.entity_id, id_a);
    assert_eq!((mv.dx, mv.dy, mv.dz), (0, 0, 0));

    // A PosRot：B 收到 move_entity_pos_rot(55)，yaw 90° = Angle 64。
    let mut body = Vec::new();
    packets::MovePlayerPosRot {
        x: fx + 1.0, y: fy, z: fz, yaw: 90.0, pitch: -45.0,
        on_ground: true, horizontal_collision: false,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut a, packets::PLAY_MOVE_POSROT_C2S_ID, &body)
        .await
        .unwrap();
    match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => assert_eq!(conn_id, conn_a),
        other => panic!("expected movement event, got {other:?}"),
    }
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_UPDATE_ENTITY_POS_ROT_S2C_ID, "PosRot must use move_entity_pos_rot");
    let mv = packets::UpdateEntityPosRot::decode_body(&payload).unwrap().0;
    assert_eq!(mv.entity_id, id_a);
    assert_eq!(mv.dx, 4096);
    assert_eq!((mv.yaw, mv.pitch), (64, packets::angle_byte(-45.0)));

    // A 纯旋转：B 收到 move_entity_rot(57)，yaw 180° = Angle 128。
    let mut body = Vec::new();
    packets::MovePlayerRot {
        yaw: 180.0, pitch: 0.0, on_ground: true, horizontal_collision: false,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut a, packets::PLAY_MOVE_ROT_C2S_ID, &body)
        .await
        .unwrap();
    match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => assert_eq!(conn_id, conn_a),
        other => panic!("expected movement event, got {other:?}"),
    }
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_UPDATE_ENTITY_ROT_S2C_ID, "Rot must use move_entity_rot");
    let rot = packets::UpdateEntityRot::decode_body(&payload).unwrap().0;
    assert_eq!(rot.entity_id, id_a);
    assert_eq!(rot.yaw, 128);

    // A 断线：B 收到 RemoveEntities(0x4E) 且带 A 的对外 id；无 Ack。
    drop(a);
    match next_event(&rx).await {
        PlayEvent::Disconnect { conn_id, .. } => assert_eq!(conn_id, conn_a),
        other => panic!("expected disconnect event, got {other:?}"),
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while hub.contains(conn_a) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!hub.contains(conn_a));
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_REMOVE_ENTITIES_S2C_ID);
    let rm = packets::RemoveEntities::decode_body(&payload).unwrap().0;
    assert_eq!(rm.ids, vec![id_a]);
    assert!(
        read_frame_timeout(&mut b, Duration::from_millis(300)).await.is_err(),
        "viewer must not receive Ack or duplicate Remove after disconnect"
    );

    drop(b);
}
