//! P51：稳定 UUID 身份端到端（共享脚手架见 common.rs）。
//! - 同名两次登录即使 LoginStart 带**不同的随机 v4**，LoginPlay UUID 仍是同一个
//!   用户名派生值，背包/XP 不丢。
//! - 两个用户派生 UUID 不冲突；P43 Spawn Player 的实体 UUID 就是各自稳定 UUID。
//! 不依赖具体 v4：服务端本来就不信任客户端携带的 uuid。
mod common;

use common::*;
use mc_network::{BroadcastHub, DropState, NetConfig, PlayEvent};
use mc_protocol::packets::{self, HandshakeNextState};
use mc_world::DEFAULT_SEED;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use uuid::Uuid;

/// 与 common::login_and_join 同序，但 LoginStart 携带随机 v4，并回读 LoginSuccess UUID。
async fn login_with_random_v4(
    addr: std::net::SocketAddr,
    name: &str,
    radius: i32,
) -> (TcpStream, Uuid) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    send_handshake(&mut s, addr.port(), HandshakeNextState::Login).await;
    let mut body = Vec::new();
    packets::encode_string(name, &mut body);
    body.extend_from_slice(Uuid::new_v4().as_bytes());
    let mut start = Vec::new();
    packets::encode_packet(packets::LOGIN_START_PACKET_ID, &body, &mut start);
    s.write_all(&start).await.unwrap();

    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::LOGIN_SUCCESS_PACKET_ID);
    let (success, _) = packets::decode_login_success(&payload).unwrap();
    assert_eq!(success.username, name);

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

    for _ in 0..(4 + mc_network::view_chunk_count(radius) + 3) {
        let _ = read_frame_timeout(&mut s, Duration::from_secs(10)).await.unwrap();
    }
    (s, success.uuid)
}

fn conn_of(hub: &BroadcastHub, name: &str) -> u64 {
    hub.snapshot()
        .into_iter()
        .find(|e| e.username == name)
        .unwrap_or_else(|| panic!("{name} must be registered in hub"))
        .conn_id
}

#[tokio::test]
async fn same_name_different_random_v4_keeps_inventory_and_xp() {
    let dir = std::env::temp_dir().join(format!("mc-p51-e2e-{}-rejoin", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let seed = DEFAULT_SEED;
    let drops = Arc::new(DropState::new(dir.clone(), seed));
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p51-uuid".into(), Arc::new(AtomicUsize::new(0)))
        .with_seed(seed)
        .with_view_distance(2)
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let expected = packets::stable_offline_uuid("SameName");

    let (first, uuid1) = login_with_random_v4(addr, "SameName", 2).await;
    assert_eq!(uuid1, expected, "LoginSuccess UUID must be username-derived v3");
    let conn1 = conn_of(&hub, "SameName");
    drops.with_inventory(conn1, |inv| {
        inv.add_item(55, 12);
        inv.add_experience(21);
    });
    drops.save_inventory(conn1, "SameName");
    drop(first);
    // 断线刷盘异步；轮询等 UUID 主键文件出现。
    for _ in 0..100 {
        if mc_world::PlayerInventory::save_path(&dir, &expected.to_string(), seed).exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(mc_world::PlayerInventory::save_path(&dir, &expected.to_string(), seed).exists());

    // 第二次登录带另一个随机 v4：身份、背包、XP 全部仍属于 SameName。
    let (mut second, uuid2) = login_with_random_v4(addr, "SameName", 2).await;
    assert_eq!(uuid2, expected);
    let conn2 = conn_of(&hub, "SameName");
    assert_ne!(conn1, conn2);
    assert_eq!(drops.with_inventory(conn2, |inv| inv.total_xp()), Some(21));
    assert_eq!(
        drops.with_inventory(conn2, |inv| (0..mc_world::inventory::SLOT_COUNT)
            .any(|i| inv.get(i) == Some((55, 12)))),
        Some(true)
    );
    // Join 补发的 Content 包确实带回了物品。
    let mut saw_content = false;
    for _ in 0..8 {
        match read_frame_timeout(&mut second, Duration::from_millis(500)).await {
            Ok((id, payload)) if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (content, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                assert!(content.slots.iter().any(|s| s.item_id == 55 && s.count == 12));
                saw_content = true;
                break;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    assert!(saw_content, "rejoin must sync the restored inventory");
    drop(second);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn two_users_have_distinct_uuids_and_spawn_uses_them() {
    let dir = std::env::temp_dir().join(format!("mc-p51-e2e-{}-two", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let seed = DEFAULT_SEED;
    let drops = Arc::new(DropState::new(dir.clone(), seed));
    let hub = Arc::new(BroadcastHub::new());
    let (tx, _rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let config = NetConfig::new(100, "p51-two".into(), Arc::new(AtomicUsize::new(0)))
        .with_seed(seed)
        .with_view_distance(2)
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub))
        .with_play_events(tx)
        .with_player_visibility(true);
    let addr = spawn_server(config).await;

    let (mut a, uuid_a) = login_with_random_v4(addr, "UuidAlpha", 2).await;
    let (mut b, uuid_b) = login_with_random_v4(addr, "UuidBeta", 2).await;
    let expected_a = packets::stable_offline_uuid("UuidAlpha");
    let expected_b = packets::stable_offline_uuid("UuidBeta");
    assert_eq!(uuid_a, expected_a);
    assert_eq!(uuid_b, expected_b);
    assert_ne!(uuid_a, uuid_b, "不同用户名不能撞 UUID");
    assert_eq!(hub.snapshot().len(), 2);
    assert_eq!(hub.find_conn_by_uuid(&uuid_a.to_string()), Some(conn_of(&hub, "UuidAlpha")));
    assert_eq!(hub.find_conn_by_uuid(&uuid_b.to_string()), Some(conn_of(&hub, "UuidBeta")));

    // P43：对端看到的 Spawn Player 实体 UUID 就是该玩家的稳定 UUID（自己仍用 id=1）。
    async fn read_spawn(stream: &mut TcpStream) -> Option<packets::SpawnPlayerEntity> {
        for _ in 0..8 {
            match read_frame_timeout(stream, Duration::from_millis(500)).await {
                Ok((id, payload)) if id == packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                    return Some(packets::SpawnPlayerEntity::decode_body(&payload).unwrap().0);
                }
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
        None
    }
    let spawn_b = read_spawn(&mut a).await.expect("A must see B's Spawn Player");
    assert_eq!(spawn_b.uuid, uuid_b.as_u128(), "A 看到的 B 必须是 B 的稳定 UUID");
    assert_ne!(spawn_b.entity_id, 1, "对端实体 id 不能是自己的 1");
    let spawn_a = read_spawn(&mut b).await.expect("B must see A's Spawn Player");
    assert_eq!(spawn_a.uuid, uuid_a.as_u128(), "B 看到的 A 必须是 A 的稳定 UUID");
    drop(a);
    drop(b);
    let _ = std::fs::remove_dir_all(&dir);
}
