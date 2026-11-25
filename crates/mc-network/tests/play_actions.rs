//! Play 动作类集成测试（Q 扔/容器点击/多人广播/种植）：共享脚手架见 \common.rs\。
//! P19 Q 扔（单扔/整扔/空手）、容器 THROW；P21 双连接广播；P22 小麦种植；P23 树苗种植。
mod common;

use common::*;
use mc_network::{ChunkCache, NetConfig, PlayEvent};
use mc_protocol::packets;
use mc_protocol::read_varint;
use mc_world::{MutableWorld, WorldGen, DEFAULT_SEED};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

#[tokio::test]
async fn throw_held_q_drops_one_then_all_with_sync() {
    use mc_network::DropState;
    let dir = std::env::temp_dir().join(format!("mc-p19-throw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "throw".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Thrower", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    send_pos_y(&mut s, fx, fy, fz).await;

    // Q（DROP_ITEM=5，seq=71）：手持 64 石头 → 扔 1 个。
    let mut body = Vec::new();
    packets::PlayerAction { status: 5, x: spawn.0, y: spawn.1 - 1, z: spawn.2, face: 1, sequence: 71 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    // 扔完立刻走开 4 格（同 chunk，不触发视距批次），避免 10 ticks 后被拾取。
    send_pos_y(&mut s, fx + 4.0, fy, fz).await;

    let mut first_id = None;
    let mut saw_content = false;
    let mut saw_ack = false;
    for _ in 0..16 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                let sp = packets::SpawnItemEntity::decode_body(&payload).unwrap().0;
                assert!(sp.entity_id >= 1000, "drop ids must not collide with the player");
                assert!(
                    (sp.x - fx).abs() < 1.5 && (sp.z - fz).abs() < 1.5,
                    "drop must spawn at the feet, got ({},{},{})",
                    sp.x, sp.y, sp.z
                );
                assert!((sp.y - fy).abs() < 2.0, "drop y must be near the feet, got {}", sp.y);
                first_id = Some(sp.entity_id);
            }
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
                assert_eq!(data.slot, packets::Slot { count: 1, item_id: 1 }, "Q must throw one stone");
            }
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                assert_eq!(c.slots[36], packets::Slot { count: 63, item_id: 1 });
                assert_eq!(c.carried, packets::Slot::empty());
                saw_content = true;
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 71 {
                    saw_ack = true;
                }
            }
            other => panic!("unexpected packet {other:#04x} after Q throw"),
        }
        if first_id.is_some() && saw_content && saw_ack {
            break;
        }
    }
    let first_id = first_id.expect("Q must spawn a drop entity");
    assert!(saw_content && saw_ack, "Q must sync inventory and ack the sequence");

    // Ctrl+Q（DROP_ALL_ITEMS=4，seq=72）：剩余 63 个整组扔完，槽变空。
    // 新掉落点距上一个 4 格（>1m 合并半径），不会合并成一包。
    let mut body = Vec::new();
    packets::PlayerAction { status: 4, x: spawn.0, y: spawn.1 - 1, z: spawn.2, face: 1, sequence: 72 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    send_pos_y(&mut s, fx + 8.0, fy, fz).await;

    let mut second_id = None;
    let mut saw_empty = false;
    let mut saw_ack2 = false;
    for _ in 0..16 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                let sp = packets::SpawnItemEntity::decode_body(&payload).unwrap().0;
                assert_ne!(sp.entity_id, first_id, "far-apart throws must not merge");
                second_id = Some(sp.entity_id);
            }
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
                assert_eq!(data.slot, packets::Slot { count: 63, item_id: 1 });
            }
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                assert_eq!(c.slots[36], packets::Slot::empty(), "whole stack must be gone");
                saw_empty = true;
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 72 {
                    saw_ack2 = true;
                }
            }
            other => panic!("unexpected packet {other:#04x} after Ctrl+Q throw"),
        }
        if second_id.is_some() && saw_empty && saw_ack2 {
            break;
        }
    }
    assert!(second_id.is_some() && saw_empty && saw_ack2, "Ctrl+Q must drop all + sync + ack");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P19 回归：空手按 Q → 只回 ack，无掉落、无同步、不断连。
#[tokio::test]
async fn empty_hand_q_only_acks() {
    use mc_network::DropState;
    let dir = std::env::temp_dir().join(format!("mc-p19-emptythrow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "emptythrow".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "EmptyThrow", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    send_pos_y(&mut s, fx, fy, fz).await;
    // 切到空槽（selected=1 → 槽 37 空）即空手。
    let body = [0u8, 1u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();

    // 空手 Q（DROP_ITEM=5，seq=77）：只允许 ack；绝不能出现掉落/同步包。
    let mut body = Vec::new();
    packets::PlayerAction { status: 5, x: spawn.0, y: spawn.1 - 1, z: spawn.2, face: 1, sequence: 77 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let mut saw_ack = false;
    for _ in 0..40 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_ne!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID, "empty hand must not spawn");
        assert_ne!(id, packets::PLAY_SET_ENTITY_DATA_S2C_ID, "empty hand must not sync data");
        assert_ne!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, "empty hand must not sync");
        assert_ne!(id, packets::PLAY_SET_CONTAINER_SLOT_S2C_ID, "empty hand must not sync");
        assert_ne!(id, packets::PLAY_REMOVE_ENTITIES_S2C_ID, "empty hand must not remove");
        if id == packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID {
            if read_varint(&payload).unwrap().0 == 77 {
                saw_ack = true;
                break;
            }
        }
    }
    assert!(saw_ack, "empty-hand Q must still be acked");
    assert!(drops.items.lock().unwrap().is_empty(), "no drop entity may exist");
    // 连接仍活着：移动包后 300ms 内无 EOF（只超时，不断连）。
    send_pos_y(&mut s, fx, fy, fz).await;
    assert!(
        read_frame_timeout(&mut s, Duration::from_millis(300)).await.is_err(),
        "connection must stay open after empty-hand Q"
    );
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P19 端到端（背包打开时按 Q）：ContainerClick THROW（mode=4）从点击槽扔
///（button 0=1 个，1=整栈）；光标上有东西时按原版无操作（只全量同步回滚预测）。
/// 原版依据：26.3 jar `ContainerInput::THROW` + `AbstractContainerMenu.doClick`
///（THROW 要求 carried 为空；button=0 取 1 个，button=1 取整栈）。
#[tokio::test]
async fn container_throw_single_all_and_cursor_noop() {
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
    let dir = std::env::temp_dir().join(format!("mc-p19-clickthrow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "clickthrow".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "ClickThrow", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    send_pos_y(&mut s, fx, fy, fz).await;

    // 准备：左键拿起 36 号槽石头（64）→ 光标 64 石头，槽空。
    let frame = click_frame(0, 1, 36, 0, 0);
    mc_network::framing::write_packet(&mut s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
    let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
    assert_eq!(c.slots[36], packets::Slot::empty());
    assert_eq!(c.carried, packets::Slot { count: 64, item_id: 1 });

    // 光标上有东西时按 Q（THROW 槽 9 button 0）：原版无操作——
    // 槽 9 不动、光标 64 石头不动、不生成掉落，只全量同步。
    let frame = click_frame(0, 2, 9, 0, 4);
    mc_network::framing::write_packet(&mut s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
    let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
    assert_eq!(c.slots[9], packets::Slot::empty());
    assert_eq!(c.carried, packets::Slot { count: 64, item_id: 1 }, "cursor must survive THROW");
    assert!(drops.items.lock().unwrap().is_empty(), "cursor THROW must not spawn");

    // 放下光标：左键点 37（空）→ 槽 37 = 64 石头，光标空。
    let frame = click_frame(0, 3, 37, 0, 0);
    mc_network::framing::write_packet(&mut s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
    let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
    assert_eq!(c.slots[37], packets::Slot { count: 64, item_id: 1 });
    assert_eq!(c.carried, packets::Slot::empty());

    // THROW button 0（扔 1 个）：Spawn(1 石头) + Content(37=63)。
    let frame = click_frame(0, 4, 37, 0, 4);
    mc_network::framing::write_packet(&mut s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
    send_pos_y(&mut s, fx + 4.0, fy, fz).await;
    let mut saw_spawn1 = false;
    for _ in 0..16 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                assert!(packets::SpawnItemEntity::decode_body(&payload).unwrap().0.entity_id >= 1000);
                saw_spawn1 = true;
            }
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
                assert_eq!(data.slot, packets::Slot { count: 1, item_id: 1 });
            }
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                assert_eq!(c.slots[37], packets::Slot { count: 63, item_id: 1 });
                assert_eq!(c.carried, packets::Slot::empty());
                break;
            }
            other => panic!("unexpected packet {other:#04x} after THROW button=0"),
        }
    }
    assert!(saw_spawn1, "THROW button=0 must spawn one item");

    // THROW button 1（扔整栈 63）：Spawn(63 石头) + Content(37 空)。
    let frame = click_frame(0, 5, 37, 1, 4);
    mc_network::framing::write_packet(&mut s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
    send_pos_y(&mut s, fx + 8.0, fy, fz).await;
    let mut saw_spawn2 = false;
    for _ in 0..16 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                saw_spawn2 = true;
            }
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
                assert_eq!(data.slot, packets::Slot { count: 63, item_id: 1 });
            }
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                assert_eq!(c.slots[37], packets::Slot::empty(), "whole stack must be gone");
                break;
            }
            other => panic!("unexpected packet {other:#04x} after THROW button=1"),
        }
    }
    assert!(saw_spawn2, "THROW button=1 must spawn the whole stack");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P21：双连接同视距——A 挖/放，B 实时收到 BlockUpdate；Ack 只回操作者本人。
/// B 站 4 格外（同视距、超拾取半径），避免抢捡干扰。
#[tokio::test]
async fn broadcast_block_update_reaches_viewer_but_ack_only_to_actor() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p21-block-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p21-block".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut a = login_and_join(addr, "BcastA", 2).await;
    let mut b = login_and_join(addr, "BcastB", 2).await;
    for s in [&mut a, &mut b] {
        let (id, _) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
    }

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9);
    send_pos_y(&mut a, fx, fy, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }
    send_pos_y(&mut b, fx + 4.0, fy, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    // A 挖脚下草：FINISH seq=11。
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x: gx, y: gy, z: gz, face: 1, sequence: 11 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut a, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    // A 本人：BlockUpdate(air) + Ack(11) + Spawn + Data。
    let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 0);
    let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 11);
    let (id, _) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID);
    let (id, _) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_ENTITY_DATA_S2C_ID);
    // 权威判定的 BlockEdit 事件（net 线程 → ECS）。
    match next_event(&rx).await {
        PlayEvent::BlockEdit { x, y, z, old_id: 9, new_id: 0, .. } => {
            assert_eq!((x, y, z), (gx, gy, gz));
        }
        other => panic!("expected block edit event, got {other:?}"),
    }
    // A 走到 2.5 格外（同区块 23，不触发视距批次），避免 10 ticks 后自己捡走掉落。
    send_pos_y(&mut a, fx + 2.5, fy, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }

    // B：必须实时收到 BlockUpdate(air)；绝不能收到 Ack(11)。
    let (mut saw_update, mut saw_spawn, mut saw_data) = (false, false, false);
    for _ in 0..16 {
        let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (u, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                assert_eq!((u.x, u.y, u.z, u.block_id), (gx, gy, gz, 0));
                saw_update = true;
            }
            packets::PLAY_SPAWN_ENTITY_S2C_ID => saw_spawn = true,
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
                assert_eq!(data.slot, packets::Slot { count: 1, item_id: 55 });
                saw_data = true;
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                panic!("ack must only go to the actor, viewer got {:?}", payload)
            }
            other => panic!("unexpected packet {other:#04x} for viewer after dig"),
        }
        if saw_update && saw_spawn && saw_data {
            break;
        }
    }
    assert!(saw_update && saw_spawn && saw_data, "viewer must see dig update + drop spawn");

    // A 放置：点下方泥土顶面 seq=12 → 手持石头（selected=0 → 槽 36）。
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0, x: gx, y: gy - 1, z: gz, face: 1,
        cursor_x: 0.5, cursor_y: 1.0, cursor_z: 0.5,
        inside_block: false, world_border_hit: false, sequence: 12,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut a, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 1);
    let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 12);
    let (id, _) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_SLOT_S2C_ID);

    // B：必须实时收到 BlockUpdate(stone)；之后 300ms 内无其它包（无 Ack(12)）。
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID, "viewer must see place update");
    assert_eq!(packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 1);
    assert!(
        read_frame_timeout(&mut b, Duration::from_millis(300)).await.is_err(),
        "viewer must not get ack/sync after place"
    );
    assert_eq!(mutable.get(gx, gy, gz), 1);
    drop(a);
    drop(b);
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P21：A 按 Q 扔东西，B 实时收到 Spawn；A 拾取后 B 收到 TakeItem + Remove。
#[tokio::test]
async fn broadcast_thrown_drop_spawns_for_viewer_and_pickup_removes() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p21-throw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p21-throw".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut a = login_and_join(addr, "ThrowA", 2).await;
    let mut b = login_and_join(addr, "ThrowB", 2).await;
    for s in [&mut a, &mut b] {
        let (id, _) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
    }
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    send_pos_y(&mut a, fx, fy, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }
    send_pos_y(&mut b, fx + 4.0, fy, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    // A 按 Q 扔 1 个石头（DROP_ITEM=5，seq=71）；A 原地不动等拾取。
    let mut body = Vec::new();
    packets::PlayerAction { status: 5, x: spawn.0, y: spawn.1 - 1, z: spawn.2, face: 1, sequence: 71 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut a, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let mut drop_id = None;
    let (mut saw_data, mut saw_content, mut saw_ack) = (false, false, false);
    for _ in 0..8 {
        let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                drop_id = Some(packets::SpawnItemEntity::decode_body(&payload).unwrap().0.entity_id);
            }
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
                assert_eq!(data.slot, packets::Slot { count: 1, item_id: 1 });
                saw_data = true;
            }
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                assert_eq!(c.slots[36], packets::Slot { count: 63, item_id: 1 });
                saw_content = true;
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                assert_eq!(read_varint(&payload).unwrap().0, 71);
                saw_ack = true;
            }
            other => panic!("unexpected packet {other:#04x} for actor after throw"),
        }
        if drop_id.is_some() && saw_data && saw_content && saw_ack {
            break;
        }
    }
    let drop_id = drop_id.expect("throw must spawn a drop");
    assert!(saw_data && saw_content && saw_ack, "throw must sync inventory and ack");

    // B：Spawn（同一 entity_id）+ Data；拾取前绝不能出现 Ack(71)；
    // A 原地拾取后 B 收到 TakeItem + Remove。
    let (mut saw_spawn, mut saw_data, mut saw_take, mut saw_remove) = (false, false, false, false);
    for _ in 0..60 {
        let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                let (sp, _) = packets::SpawnItemEntity::decode_body(&payload).unwrap();
                assert_eq!(sp.entity_id, drop_id, "viewer must see the same drop entity");
                saw_spawn = true;
            }
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => saw_data = true,
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
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                panic!("ack must only go to the actor, viewer got {:?}", payload)
            }
            other => panic!("unexpected packet {other:#04x} for viewer"),
        }
        if saw_spawn && saw_data && saw_take && saw_remove {
            break;
        }
    }
    assert!(
        saw_spawn && saw_data && saw_take && saw_remove,
        "viewer must see throw spawn and pickup removal"
    );
    assert!(drops.items.lock().unwrap().is_empty(), "picked drop must be gone server-side");
    drop(a);
    drop(b);
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P21：断开一端清 hub 条目（与 Disconnect 同点），另一端不受影响照常挖放。
#[tokio::test]
async fn broadcast_disconnect_clears_hub_entry_and_survivor_unaffected() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p21-disc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p21-disc".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut a = login_and_join(addr, "GoneA", 2).await;
    let mut b = login_and_join(addr, "StayB", 2).await;
    for s in [&mut a, &mut b] {
        let (id, _) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
    }
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    send_pos_y(&mut a, fx, fy, fz).await;
    let conn_a = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };
    send_pos_y(&mut b, fx + 4.0, fy, fz).await;
    let conn_b = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while hub.len() != 2 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(hub.len(), 2, "both connections must register");
    assert!(hub.contains(conn_a) && hub.contains(conn_b));

    // A 断开：Disconnect 事件 + hub 条目 cleared。
    drop(a);
    match next_event(&rx).await {
        PlayEvent::Disconnect { conn_id, .. } => assert_eq!(conn_id, conn_a),
        other => panic!("expected disconnect event, got {other:?}"),
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while (hub.len() != 1 || hub.contains(conn_a)) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(hub.len(), 1, "disconnect must clear the hub entry");
    assert!(!hub.contains(conn_a) && hub.contains(conn_b));

    // 存活端照常挖放：B 移到脚下挖草 seq=91。
    send_pos_y(&mut b, fx, fy, fz).await;
    match next_event(&rx).await {
        PlayEvent::Movement { .. } => {}
        other => panic!("expected movement event, got {other:?}"),
    }
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x: gx, y: gy, z: gz, face: 1, sequence: 91 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut b, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 0);
    let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 91);
    let (id, _) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID);
    assert_eq!(mutable.get(gx, gy, gz), 0);
    drop(b);
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P22 端到端：手持小麦种子对草方块顶面右键 → 种下 0 阶段小麦 + 扣 1 种子；
/// 全服唯一 ticker 推生长 → 逐阶段 BlockUpdate（含 P21 hub 广播）；
/// 挖成熟小麦 → 掉小麦 + 种子 → 走近拾取入库。
/// 阶段间隔经 `set_stage_ticks` 调小（生产 1200 拍 = 60s，见 HANDOFF §5），
/// 走真实 ticker + 真实 hub 广播，不伪造时钟。
#[tokio::test]
async fn plant_wheat_seed_grows_and_harvest_drops_wheat_and_seeds() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p22-plant-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    // 10 拍 = 0.5s/阶段：7 阶段约 3.5s 成熟，e2e 可等；生产值 1200 拍 = 60s。
    drops.set_stage_ticks(10);
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p22-plant".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "WheatP", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9, "spawn feet must be grass");
    send_pos_y(&mut s, fx, fy, fz).await;
    let conn_id = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };

    // 发 3 颗种子（hotbar 槽 37），切手持到槽 1 选中它们。
    let leftover =
        drops.with_inventory(conn_id, |inv| inv.add_item(mc_world::drops::ITEM_WHEAT_SEEDS, 3));
    assert_eq!(leftover, Some(0), "seeds must fit the backpack");
    let body = [0u8, 1u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();

    // 对草方块顶面右键（face=1）：目标空气格落 0 阶段小麦（6867），扣 1 种子。
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx,
        y: gy,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 21,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
    assert_eq!((bu.x, bu.y, bu.z), (gx, gy + 1, gz));
    assert_eq!(bu.block_id, 6867, "planted wheat must be age 0 (default_state_id)");
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 21);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_SLOT_S2C_ID, "planting must consume 1 seed");
    let (slot_pkt, _) = packets::SetContainerSlot::decode_body(&payload).unwrap();
    assert_eq!(slot_pkt.slot, 37);
    assert_eq!(slot_pkt.data, packets::Slot { count: 2, item_id: 1065 });
    match next_event(&rx).await {
        PlayEvent::BlockEdit { old_id: 0, new_id: 6867, .. } => {}
        other => panic!("expected plant block edit event, got {other:?}"),
    }
    assert_eq!(mutable.get(gx, gy + 1, gz), 6867);
    assert_eq!(drops.crop_count(), 1);

    // 生长：真实 ticker + hub 广播，逐阶段 BlockUpdate 6868..=6874（成熟 6874）。
    let mut stages = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        let (id, payload) =
            read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                assert_eq!((bu.x, bu.y, bu.z), (gx, gy + 1, gz));
                stages.push(bu.block_id);
                if bu.block_id == 6874 {
                    break;
                }
            }
            other => panic!("unexpected packet {other:#04x} while waiting for growth"),
        }
    }
    assert_eq!(
        stages,
        vec![6868, 6869, 6870, 6871, 6872, 6873, 6874],
        "must grow one stage at a time to maturity"
    );
    assert_eq!(mutable.get(gx, gy + 1, gz), 6874);

    // 挖成熟小麦：掉小麦×1 + 种子 1~2（确定性掉落表，期望值现场算）。
    let want = mc_world::drops_for_block(6874, gx, gy + 1, gz, DEFAULT_SEED);
    assert_eq!(want.len(), 2);
    assert_eq!(want[0], mc_world::Drop { item_id: 1066, count: 1 });
    assert_eq!(want[1].item_id, 1065);
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x: gx, y: gy + 1, z: gz, face: 1, sequence: 22 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 0);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 22);
    for (i, w) in want.iter().enumerate() {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID, "harvest drop {i} must spawn");
        let (spawned, _) = packets::SpawnItemEntity::decode_body(&payload).unwrap();
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SET_ENTITY_DATA_S2C_ID);
        let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
        assert_eq!(data.entity_id, spawned.entity_id);
        assert_eq!(data.slot, packets::Slot { count: w.count, item_id: w.item_id });
    }
    assert_eq!(mutable.get(gx, gy + 1, gz), 0);
    assert_eq!(drops.crop_count(), 0, "dug wheat must leave growth tracking");
    match next_event(&rx).await {
        PlayEvent::BlockEdit { old_id: 6874, new_id: 0, .. } => {}
        other => panic!("expected dig block edit event, got {other:?}"),
    }

    // 拾取：站在 1 格内，delay 到期后 TakeItem + Remove + 全量背包同步。
    // 单次读超时放宽到 2s：本机多测试二进制并行时调度间隙可能超过 500ms，
    // 超时只是重试（10s 总期限不变），不断言强度不变。
    let want_seeds = 2 + want[1].count;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let (mut takes, mut saw_content) = (0, false);
    while std::time::Instant::now() < deadline {
        let (id, payload) =
            read_frame_timeout(&mut s, Duration::from_millis(2000)).await.unwrap();
        match id {
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            packets::PLAY_PICKUP_ITEM_S2C_ID => {
                let (t, _) = packets::TakeItemEntity::decode_body(&payload).unwrap();
                assert_eq!(t.collector_id, 1);
                takes += 1;
            }
            packets::PLAY_REMOVE_ENTITIES_S2C_ID => {}
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                let mut wheat = 0;
                let mut seeds = 0;
                for sl in &c.slots {
                    match sl.item_id {
                        1066 => wheat += sl.count,
                        1065 => seeds += sl.count,
                        _ => {}
                    }
                }
                if wheat == 1 && seeds == want_seeds {
                    saw_content = true;
                }
            }
            other => panic!("unexpected packet {other:#04x} while waiting for pickup"),
        }
        if takes >= 2 && saw_content {
            break;
        }
    }
    assert!(takes >= 2 && saw_content, "harvest must be picked up into the backpack");

    drop(s);
    match next_event(&rx).await {
        PlayEvent::Disconnect { .. } => {}
        other => panic!("expected disconnect event, got {other:?}"),
    }
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P23：手持树苗对草顶面右键 → 落树苗方块（30）、扣 1；挖掉原样返还，可再种。
#[tokio::test]
async fn plant_oak_sapling_and_dig_returns_sapling() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p23-sapling-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p23-sapling".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "SaplingP", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9, "spawn feet must be grass");
    send_pos_y(&mut s, fx, fy, fz).await;
    let conn_id = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };

    // 发 1 个树苗（hotbar 槽 37），切手持选中。
    let leftover =
        drops.with_inventory(conn_id, |inv| inv.add_item(mc_world::drops::ITEM_OAK_SAPLING, 1));
    assert_eq!(leftover, Some(0), "sapling must fit the backpack");
    let body = [0u8, 1u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();

    // 对草方块顶面右键：目标空气格落树苗（30），扣光手持。
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx,
        y: gy,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 21,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
    assert_eq!((bu.x, bu.y, bu.z), (gx, gy + 1, gz));
    assert_eq!(bu.block_id, mc_world::OAK_SAPLING_BLOCK_ID as i32);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 21);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_SLOT_S2C_ID, "planting must consume the sapling");
    let (slot_pkt, _) = packets::SetContainerSlot::decode_body(&payload).unwrap();
    assert_eq!(slot_pkt.slot, 37);
    // 扣光整组后槽位发空槽（count=0/item=0），不是残留物品 id。
    assert_eq!(slot_pkt.data.count, 0);
    match next_event(&rx).await {
        PlayEvent::BlockEdit { old_id: 0, new_id: 30, .. } => {}
        other => panic!("expected plant block edit event, got {other:?}"),
    }
    assert_eq!(mutable.get(gx, gy + 1, gz), mc_world::OAK_SAPLING_BLOCK_ID);
    // 树苗不登记生长（只跟小麦）。
    assert_eq!(drops.crop_count(), 0, "sapling must not join growth tracking");
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P26：种下树苗 → 调小计时 → 长成小橡树（log + leaves BlockUpdate）→ 挖树干掉 log。
/// 全跑真实全局 ticker + hub 广播路径（生产 7200 拍 ≈ 6min，e2e 用 10 拍加速）。
#[tokio::test]
async fn plant_oak_sapling_grows_into_small_oak_and_trunk_drops_log() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p26-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    drops.set_sapling_ticks(10);
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p26-tree".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "TreeP", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9, "spawn feet must be grass");
    send_pos_y(&mut s, fx, fy, fz).await;
    let conn_id = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };

    let leftover =
        drops.with_inventory(conn_id, |inv| inv.add_item(mc_world::drops::ITEM_OAK_SAPLING, 1));
    assert_eq!(leftover, Some(0), "sapling must fit the backpack");
    let body = [0u8, 1u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();

    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx,
        y: gy,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 31,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
    assert_eq!((bu.x, bu.y, bu.z), (gx, gy + 1, gz));
    assert_eq!(bu.block_id, mc_world::OAK_SAPLING_BLOCK_ID as i32);
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 31);
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_SLOT_S2C_ID, "planting must consume the sapling");
    assert_eq!(drops.sapling_count(), 1, "sapling must join growth tracking");
    assert_eq!(drops.crop_count(), 0, "sapling must not pollute wheat tracking");

    // 生长：真实 ticker 到期后树苗格变原木（140），冠层出现树叶（294）。
    let (sx, sy, sz) = (gx, gy + 1, gz);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let mut saw_log = false;
    let mut saw_leaves = false;
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == (sx, sy, sz) && bu.block_id == 140 {
                    saw_log = true;
                }
                if bu.block_id == 294 {
                    saw_leaves = true;
                }
                if saw_log && saw_leaves {
                    break;
                }
            }
            other => panic!("unexpected packet {other:#04x} while waiting for tree growth"),
        }
    }
    assert!(saw_log, "sapling cell must become oak_log");
    assert!(saw_leaves, "canopy must broadcast oak_leaves");
    assert_eq!(mutable.get(sx, sy, sz), mc_world::OAK_LOG_BLOCK_ID);
    assert_eq!(drops.sapling_count(), 0, "grown tree leaves tracking");

    // 挖树干：掉原木（163），走通用挖掘 + 掉落 Spawn/Data。
    // 生长一次广播 ~57 格，break 时可能还有尾包在流里：挖后只认本格 0，余包跳过。
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x: sx, y: sy, z: sz, face: 1, sequence: 32 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let (mut saw_update, mut saw_ack) = (false, false);
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == (sx, sy, sz) && bu.block_id == 0 {
                    saw_update = true;
                }
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 32 {
                    saw_ack = true;
                }
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            other => panic!("unexpected packet {other:#04x} after digging the trunk"),
        }
        if saw_update && saw_ack {
            break;
        }
    }
    assert!(saw_update, "dug trunk must broadcast air at its own cell");
    assert!(saw_ack, "dig must ack sequence 32");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut spawned = None;
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                spawned = Some(packets::SpawnItemEntity::decode_body(&payload).unwrap().0);
                break;
            }
            packets::PLAY_BLOCK_UPDATE_S2C_ID
            | packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID
            | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            other => panic!("unexpected packet {other:#04x} waiting for trunk drop Spawn"),
        }
    }
    let spawned = spawned.expect("dug trunk must spawn a drop");
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_ENTITY_DATA_S2C_ID);
    let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
    assert_eq!(data.entity_id, spawned.entity_id);
    assert_eq!(data.slot, packets::Slot { count: 1, item_id: 163 });
    assert_eq!(mutable.get(sx, sy, sz), 0);

    drop(s);
    match next_event(&rx).await {
        PlayEvent::BlockEdit { .. } => {}
        other => panic!("expected block edit event, got {other:?}"),
    }
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P27：手持锄头对草顶面右键 → 耕地（6875，不消耗）；种在耕地上的小麦生长减半。
/// 泥土对照组同播：耕地株成熟时泥土株远未熟（28 拍 vs 56 拍，基础间隔 8 拍）。
#[tokio::test]
async fn p27_till_soil_to_farmland_and_wheat_grows_faster_on_it() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p27-till-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    drops.set_stage_ticks(8);
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p27-till".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "TillP", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9, "spawn feet must be grass");
    // 对照组泥土柱：测试 setup 直接放（决定种植只看服务端状态，不依赖客户端 chunk）。
    mutable.set(gx + 1, gy, gz, mc_world::DIRT_BLOCK_ID);
    mutable.set(gx + 1, gy + 1, gz, mc_world::AIR_BLOCK_ID);
    send_pos_y(&mut s, fx, fy, fz).await;
    let conn_id = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };

    // 管理面板口径：直接发锄头 + 2 种子（hotbar 37/38），模拟 `/api/give` 领取。
    let leftover =
        drops.with_inventory(conn_id, |inv| inv.add_item(mc_world::drops::ITEM_WOODEN_HOE, 1));
    assert_eq!(leftover, Some(0), "hoe must fit the backpack");
    let leftover =
        drops.with_inventory(conn_id, |inv| inv.add_item(mc_world::drops::ITEM_WHEAT_SEEDS, 2));
    assert_eq!(leftover, Some(0), "seeds must fit the backpack");

    // 选中锄头（槽 1）：对草顶面右键 → 耕地 6875 + Ack，不扣锄头、无 Slot 同步。
    let body = [0u8, 1u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx,
        y: gy,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 61,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let (mut saw_update, mut saw_ack) = (false, false);
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                assert_eq!((bu.x, bu.y, bu.z), (gx, gy, gz));
                assert_eq!(bu.block_id, 6875, "tilled soil must become farmland");
                saw_update = true;
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 61 {
                    saw_ack = true;
                }
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => panic!("tilling must not consume the hoe"),
            other => panic!("unexpected packet {other:#04x} after tilling"),
        }
        if saw_update && saw_ack {
            break;
        }
    }
    assert!(saw_update && saw_ack, "till must broadcast farmland and ack the sequence");
    assert_eq!(mutable.get(gx, gy, gz), mc_world::FARMLAND_BLOCK_ID);
    assert_eq!(
        drops.with_inventory(conn_id, |inv| inv.held()).flatten(),
        Some((mc_world::drops::ITEM_WOODEN_HOE, 1)),
        "hoe is never consumed (no durability system)"
    );

    // 选中种子（槽 2）：耕地上种一株（seq 62），泥土上种一株对照（seq 63）。
    let body = [0u8, 2u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();
    for (seq, (px, py, pz), left) in [(62, (gx, gy, gz), 1), (63, (gx + 1, gy, gz), 0)] {
        let mut body = Vec::new();
        packets::UseItemOn {
            hand: 0,
            x: px,
            y: py,
            z: pz,
            face: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: seq,
        }
        .encode_body(&mut body);
        mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (mut saw_update, mut saw_ack, mut saw_slot) = (false, false, false);
        while std::time::Instant::now() < deadline {
            let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
            match id {
                packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                    let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                    assert_eq!((bu.x, bu.y, bu.z), (px, py + 1, pz));
                    assert_eq!(bu.block_id, 6867, "planted wheat must be age 0");
                    saw_update = true;
                }
                packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                    if read_varint(&payload).unwrap().0 == seq {
                        saw_ack = true;
                    }
                }
                packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => {
                    let (slot_pkt, _) = packets::SetContainerSlot::decode_body(&payload).unwrap();
                    assert_eq!(slot_pkt.data.count, left, "planting must consume 1 seed");
                    saw_slot = true;
                }
                packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {
                    continue
                }
                other => panic!("unexpected packet {other:#04x} while planting seq={seq}"),
            }
            if saw_update && saw_ack && saw_slot {
                break;
            }
        }
        assert!(saw_update && saw_ack && saw_slot, "planting seq={seq} must land, ack and sync");
    }
    assert_eq!(drops.crop_count(), 2);

    // 生长：耕地株先成熟（~28 拍 ≈1.4s）；成熟瞬间泥土株 age≈3，远未熟。
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        assert!(std::time::Instant::now() < deadline, "farmland wheat must mature first");
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == (gx, gy + 1, gz) && bu.block_id == 6874 {
                    break;
                }
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            other => panic!("unexpected packet {other:#04x} while waiting for growth"),
        }
    }
    assert_eq!(mutable.get(gx, gy + 1, gz), 6874, "farmland wheat must be mature");
    let dirt_age = mc_world::wheat_age(mutable.get(gx + 1, gy + 1, gz))
        .expect("dirt wheat must still be growing");
    assert!(dirt_age < 7, "dirt wheat must lag far behind, got age={dirt_age}");
    assert!(dirt_age >= 1, "dirt wheat must still advance, got age={dirt_age}");

    drop(s);
    // 生长期的 BlockEdit 事件积压在 channel 里：排空直到 Disconnect。
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        assert!(std::time::Instant::now() < deadline, "must see disconnect after close");
        match next_event(&rx).await {
            PlayEvent::Disconnect { .. } => break,
            PlayEvent::BlockEdit { .. } => continue,
            other => panic!("expected only block edits then disconnect, got {other:?}"),
        }
    }
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P27：手持骨粉右键小麦 → 推进 1~2 阶段并扣 1；成熟后再用不扣；
/// 手持骨粉右键树苗 → 直接长成小橡树并扣 1。
#[tokio::test]
async fn p27_bonemeal_advances_wheat_and_grows_sapling() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p27-bone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p27-bone".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "BoneP", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    assert_eq!(mutable.get(gx, gy, gz), 9, "spawn feet must be grass");
    // 树苗试验柱：测试 setup 直接放泥土 + 清顶（与耕地 e2e 同口径）。
    mutable.set(gx + 2, gy, gz, mc_world::DIRT_BLOCK_ID);
    mutable.set(gx + 2, gy + 1, gz, mc_world::AIR_BLOCK_ID);
    send_pos_y(&mut s, fx, fy, fz).await;
    let conn_id = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };

    // 管理面板口径：种子×1、骨粉×10、树苗×1（hotbar 37/38/39）。
    for (item, n) in [
        (mc_world::drops::ITEM_WHEAT_SEEDS, 1),
        (mc_world::drops::ITEM_BONE_MEAL, 10),
        (mc_world::drops::ITEM_OAK_SAPLING, 1),
    ] {
        let leftover = drops.with_inventory(conn_id, |inv| inv.add_item(item, n));
        assert_eq!(leftover, Some(0), "item {item} must fit the backpack");
    }

    // 种下小麦（槽 1，seq 71）：6867 + Ack + 扣光种子。
    let body = [0u8, 1u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx,
        y: gy,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 71,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let (mut saw_update, mut saw_ack, mut saw_slot) = (false, false, false);
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                assert_eq!(
                    packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id,
                    6867
                );
                saw_update = true;
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 71 {
                    saw_ack = true;
                }
            }
            packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => saw_slot = true,
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            other => panic!("unexpected packet {other:#04x} while planting wheat"),
        }
        if saw_update && saw_ack && saw_slot {
            break;
        }
    }
    assert!(saw_update && saw_ack && saw_slot, "wheat must be planted");
    assert_eq!(drops.crop_count(), 1);

    // 选中骨粉（槽 2）：连续催熟到满级；每拍 +adv（同位点确定性），各扣 1。
    let body = [0u8, 2u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();
    let adv = mc_world::drops::bonemeal_advance(DEFAULT_SEED, gx, gy + 1, gz);
    let mut age = 0u8;
    let mut bone_left = 10i32;
    let mut seq = 72i32;
    while age < 7 {
        let mut body = Vec::new();
        packets::UseItemOn {
            hand: 0,
            x: gx,
            y: gy + 1,
            z: gz,
            face: 1,
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: seq,
        }
        .encode_body(&mut body);
        mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
            .await
            .unwrap();
        age = age.saturating_add(adv).min(7);
        bone_left -= 1;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let (mut saw_update, mut saw_ack, mut saw_slot) = (false, false, false);
        while std::time::Instant::now() < deadline {
            let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
            match id {
                packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                    let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                    assert_eq!((bu.x, bu.y, bu.z), (gx, gy + 1, gz));
                    assert_eq!(bu.block_id, 6867 + age as i32, "bonemeal must advance to {age}");
                    saw_update = true;
                }
                packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                    if read_varint(&payload).unwrap().0 == seq {
                        saw_ack = true;
                    }
                }
                packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => {
                    let (slot_pkt, _) = packets::SetContainerSlot::decode_body(&payload).unwrap();
                    assert_eq!(slot_pkt.slot, 38);
                    assert_eq!(slot_pkt.data.count, bone_left, "bonemeal must cost 1");
                    saw_slot = true;
                }
                packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {
                    continue
                }
                other => panic!("unexpected packet {other:#04x} on bonemeal seq={seq}"),
            }
            if saw_update && saw_ack && saw_slot {
                break;
            }
        }
        assert!(saw_update && saw_ack && saw_slot, "bonemeal seq={seq} must land, ack and sync");
        assert_eq!(drops.crop_stage(gx, gy + 1, gz), Some(age), "tracking must follow bonemeal");
        seq += 1;
    }
    assert_eq!(mutable.get(gx, gy + 1, gz), 6874, "wheat must be mature after bonemeal");

    // 成熟后再催：只重发 6874 现状 + Ack，不扣骨粉。
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx,
        y: gy + 1,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 0.5,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: seq,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let (mut saw_update, mut saw_ack) = (false, false);
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                assert_eq!(
                    packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id,
                    6874
                );
                saw_update = true;
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == seq {
                    saw_ack = true;
                }
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => panic!("mature bonemeal must not consume"),
            other => panic!("unexpected packet {other:#04x} on mature bonemeal"),
        }
        if saw_update && saw_ack {
            break;
        }
    }
    assert!(saw_update && saw_ack, "mature bonemeal must resend and ack");
    assert_eq!(
        drops.with_inventory(conn_id, |inv| inv.held()).flatten(),
        Some((mc_world::drops::ITEM_BONE_MEAL, bone_left)),
        "mature bonemeal must not consume"
    );

    // 种下树苗（槽 3，seq 91），再用骨粉催（seq 92）：直接长成小橡树。
    let body = [0u8, 3u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx + 2,
        y: gy,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 91,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let (mut saw_update, mut saw_ack, mut saw_slot) = (false, false, false);
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => saw_update = true,
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 91 {
                    saw_ack = true;
                }
            }
            packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => saw_slot = true,
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            other => panic!("unexpected packet {other:#04x} while planting sapling"),
        }
        if saw_update && saw_ack && saw_slot {
            break;
        }
    }
    assert!(saw_update && saw_ack && saw_slot, "sapling must be planted");
    assert_eq!(mutable.get(gx + 2, gy + 1, gz), mc_world::OAK_SAPLING_BLOCK_ID);
    assert_eq!(drops.sapling_count(), 1);

    let body = [0u8, 2u8];
    mc_network::framing::write_packet(&mut s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &body)
        .await
        .unwrap();
    bone_left -= 1;
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x: gx + 2,
        y: gy + 1,
        z: gz,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 0.5,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: 92,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let (mut saw_log, mut saw_leaves, mut saw_ack, mut saw_slot) = (false, false, false, false);
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == (gx + 2, gy + 1, gz) && bu.block_id == 140 {
                    saw_log = true;
                }
                if bu.block_id == 294 {
                    saw_leaves = true;
                }
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 92 {
                    saw_ack = true;
                }
            }
            packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => {
                let (slot_pkt, _) = packets::SetContainerSlot::decode_body(&payload).unwrap();
                assert_eq!(slot_pkt.data.count, bone_left, "tree bonemeal must cost 1");
                saw_slot = true;
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            other => panic!("unexpected packet {other:#04x} on sapling bonemeal"),
        }
        if saw_log && saw_leaves && saw_ack && saw_slot {
            break;
        }
    }
    assert!(saw_log, "bonemealed sapling cell must become oak_log");
    assert!(saw_leaves, "bonemealed sapling must broadcast canopy leaves");
    assert!(saw_ack && saw_slot, "sapling bonemeal must ack and consume 1");
    assert_eq!(mutable.get(gx + 2, gy + 1, gz), mc_world::OAK_LOG_BLOCK_ID);
    assert_eq!(drops.sapling_count(), 0, "grown tree leaves tracking");
    assert_eq!(drops.crop_count(), 1, "wheat tracking must be intact");

    drop(s);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        assert!(std::time::Instant::now() < deadline, "must see disconnect after close");
        match next_event(&rx).await {
            PlayEvent::Disconnect { .. } => break,
            PlayEvent::BlockEdit { .. } => continue,
            other => panic!("expected only block edits then disconnect, got {other:?}"),
        }
    }
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P27：砍掉原木后，3 格内无支撑的树叶随 ticker 消失（BlockUpdate 0 + decay 掉落）。
#[tokio::test]
async fn p27_leaves_decay_after_trunk_removed() {
    use mc_network::{BroadcastHub, DropState};
    let dir = std::env::temp_dir().join(format!("mc-p27-decay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    drops.set_leaf_decay_ticks(5);
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "p27-decay".into(), Arc::new(AtomicUsize::new(0)))
        .with_play_events(tx)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "DecayP", 2).await;
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    send_pos_y(&mut s, fx, fy, fz).await;
    let _conn_id = match next_event(&rx).await {
        PlayEvent::Movement { conn_id, .. } => conn_id,
        other => panic!("expected movement event, got {other:?}"),
    };

    // 试验台（测试 setup 直接落子）：原木 + 一片孤叶，要求
    // a) 该叶 decay 必掉东西（Spawn 可断言），b) 3 格内无世界生成原木（不被支撑误取消）。
    // c) 距原木 ≤3（decay 登记半径）。全立方扫描代替固定偏移（掉落 1/8 口径，固定点易掷空）。
    let (lx, ly, lz) = (gx, gy + 4, gz);
    let mut leaf = None;
    'search: for dx in -3..=3 {
        for dy in -3..=3 {
            for dz in -3..=3 {
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }
                let (cx, cy, cz) = (lx + dx, ly + dy, lz + dz);
                if mutable.get(cx, cy, cz) != mc_world::AIR_BLOCK_ID {
                    continue;
                }
                if mc_world::drops::leaf_decay_drops(cx, cy, cz, DEFAULT_SEED).is_empty() {
                    continue;
                }
                let mut supported = false;
                for ox in -3..=3 {
                    for oy in -3..=3 {
                        for oz in -3..=3 {
                            if mutable.get(cx + ox, cy + oy, cz + oz)
                                == mc_world::OAK_LOG_BLOCK_ID
                            {
                                supported = true;
                                break;
                            }
                        }
                        if supported {
                            break;
                        }
                    }
                    if supported {
                        break;
                    }
                }
                if !supported {
                    leaf = Some((cx, cy, cz));
                    break 'search;
                }
            }
        }
    }
    let (ex, ey, ez) = leaf.expect("must find an isolated leaf spot near spawn");
    let want = mc_world::drops::leaf_decay_drops(ex, ey, ez, DEFAULT_SEED);
    assert!(!want.is_empty());
    mutable.set(lx, ly, lz, mc_world::OAK_LOG_BLOCK_ID);
    mutable.set(ex, ey, ez, mc_world::OAK_LEAVES_BLOCK_ID);

    // 砍掉原木（seq 81）：BlockUpdate(0) + Ack + 原木掉落 Spawn/Data，同时登记 decay 观察。
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x: lx, y: ly, z: lz, face: 1, sequence: 81 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let (mut saw_update, mut saw_ack, mut saw_spawn, mut saw_data) = (false, false, false, false);
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == (lx, ly, lz) {
                    assert_eq!(bu.block_id, 0);
                    saw_update = true;
                }
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 81 {
                    saw_ack = true;
                }
            }
            packets::PLAY_SPAWN_ENTITY_S2C_ID => saw_spawn = true,
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
                assert_eq!(
                    data.slot,
                    packets::Slot { count: 1, item_id: mc_world::drops::ITEM_OAK_LOG },
                    "dug trunk must drop its log"
                );
                saw_data = true;
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            other => panic!("unexpected packet {other:#04x} after digging the trunk"),
        }
        if saw_update && saw_ack && saw_spawn && saw_data {
            break;
        }
    }
    assert!(saw_update && saw_ack && saw_spawn && saw_data, "trunk dig must land with its drop");
    assert!(mutable.get(ex, ey, ez) == mc_world::OAK_LEAVES_BLOCK_ID, "leaf must stand until decay");

    // decay（5 拍 ≈0.25s）：该叶变空气 + BlockUpdate(0) + 掉落 Spawn/Data（视距补发）。
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let (mut saw_zero, mut saw_entities) = (false, false);
    let mut seen_drops = std::collections::HashSet::new();
    while std::time::Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == (ex, ey, ez) {
                    assert_eq!(bu.block_id, 0, "decayed leaf must broadcast air");
                    saw_zero = true;
                }
            }
            packets::PLAY_SPAWN_ENTITY_S2C_ID => {}
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
                seen_drops.insert((data.slot.item_id, data.slot.count));
                if want.iter().all(|d| seen_drops.contains(&(d.item_id, d.count))) {
                    saw_entities = true;
                }
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => continue,
            other => panic!("unexpected packet {other:#04x} while waiting for decay"),
        }
        if saw_zero && saw_entities {
            break;
        }
    }
    assert!(saw_zero, "decayed leaf must broadcast air at its own cell");
    assert!(saw_entities, "decay drops must spawn as entities, want={want:?}");
    assert_eq!(mutable.get(ex, ey, ez), mc_world::AIR_BLOCK_ID);
    assert_eq!(drops.leaf_pending_count(), 0, "decayed leaf leaves tracking");

    drop(s);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        assert!(std::time::Instant::now() < deadline, "must see disconnect after close");
        match next_event(&rx).await {
            PlayEvent::Disconnect { .. } => break,
            PlayEvent::BlockEdit { .. } => continue,
            other => panic!("expected only block edits then disconnect, got {other:?}"),
        }
    }
    drop(rx);
    let _ = std::fs::remove_dir_all(&dir);
}
