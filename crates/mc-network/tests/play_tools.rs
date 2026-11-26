//! P47 端到端：工具档位门槛 / 时运 / 精准采集 / 工作台 3×3 / 挖掘动画 0x05。
//! 共享脚手架见 common.rs；旧测试文件不改一字。
mod common;

use common::*;
use mc_network::{BroadcastHub, ChunkCache, DropState, GameMode, NetConfig};
use mc_protocol::packets;
use mc_protocol::read_varint;
use mc_world::{PlayerInventory, MutableWorld, WorldGen, DEFAULT_SEED};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

fn click_frame(window: i32, state: i32, slot: i16, button: u8, ty: u8) -> Vec<u8> {
    let mut b = Vec::new();
    mc_protocol::write_varint(window, &mut b);
    mc_protocol::write_varint(state, &mut b);
    b.extend_from_slice(&slot.to_be_bytes());
    b.push(button);
    mc_protocol::write_varint(ty as i32, &mut b);
    mc_protocol::write_varint(0, &mut b);
    b.push(0);
    b
}

async fn send_click(s: &mut tokio::net::TcpStream, window: i32, state: i32, slot: i16, button: u8, ty: u8) {
    let frame = click_frame(window, state, slot, button, ty);
    mc_network::framing::write_packet(s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
}

async fn read_content(s: &mut tokio::net::TcpStream) -> packets::SetContainerContent {
    for _ in 0..40 {
        let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            return packets::SetContainerContent::decode_body(&payload).unwrap().0;
        }
    }
    panic!("no container content packet");
}

async fn dig(s: &mut tokio::net::TcpStream, x: i32, y: i32, z: i32, status: i32, seq: i32) {
    let mut body = Vec::new();
    packets::PlayerAction { status, x, y, z, face: 1, sequence: seq }.encode_body(&mut body);
    mc_network::framing::write_packet(s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
}

/// 读到 Ack(seq) 为止，顺带记录是否出现掉落 Spawn/Data（含数量）。
async fn read_dig_result(
    s: &mut tokio::net::TcpStream,
    seq: i32,
) -> (bool, Vec<(i32, i32)>) {
    let mut ack = false;
    let mut drops = Vec::new();
    let mut after_ack = 0;
    for _ in 0..24 {
        // Ack 与 Spawn/Data 的顺序不保证；Ack 后再多读几包（或短超时）以收齐掉落。
        let frame = tokio::time::timeout(Duration::from_millis(400), read_frame(s)).await;
        let Ok(Ok((id, payload))) = frame else { break };
        match id {
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == seq {
                    ack = true;
                }
            }
            packets::PLAY_SET_ENTITY_DATA_S2C_ID => {
                if let Ok((d, _)) = packets::ItemEntityData::decode_body(&payload) {
                    drops.push((d.slot.item_id, d.slot.count));
                }
            }
            _ => {}
        }
        if ack {
            after_ack += 1;
            if !drops.is_empty() || after_ack >= 4 {
                break;
            }
        }
    }
    assert!(ack, "dig seq {seq} must be acked");
    (ack, drops)
}

#[tokio::test]
async fn tool_tier_fortune_and_silk_touch_change_drops() {
    let dir = std::env::temp_dir().join(format!("mc-p47-tools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut inv = PlayerInventory::new();
    inv.set_slot(36, Some((mc_world::tools::PICKAXES[0].item_id, 1)));
    inv.save_to_dir(&dir, "Miner", DEFAULT_SEED).unwrap();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let hub = Arc::new(BroadcastHub::new());
    let config = NetConfig::new(100, "tools".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub))
        .with_tool_rules(true);
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Miner", 2).await;
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    send_pos_y(&mut s, fx, fy, fz).await;
    let conn = hub.find_conn_by_name("Miner").expect("join must register conn");

    let iron_ore = mc_world::tools::MINING_RULES
        .iter().find(|r| r.block_name == "minecraft:iron_ore").unwrap().block_id;
    let coal_ore = mc_world::tools::MINING_RULES
        .iter().find(|r| r.block_name == "minecraft:coal_ore").unwrap().block_id;
    let stone = mc_world::STONE_BLOCK_ID;

    // 木镐挖铁矿：方块破坏但零掉落。
    mutable.set(gx, gy, gz, iron_ore);
    dig(&mut s, gx, gy, gz, packets::PLAYER_ACTION_FINISH_DIG, 11).await;
    let (_, drops_seen) = read_dig_result(&mut s, 11).await;
    assert!(drops_seen.is_empty(), "wood pickaxe must not drop iron ore");

    // 木镐 + 时运挖煤矿：确定性额外 0~2 个。
    drops.set_tool_enchant(conn, mc_world::tools::ToolEnchant::Fortune);
    mutable.set(gx, gy, gz, coal_ore);
    dig(&mut s, gx, gy, gz, packets::PLAYER_ACTION_FINISH_DIG, 12).await;
    let (_, drops_seen) = read_dig_result(&mut s, 12).await;
    assert_eq!(drops_seen.len(), 1, "one coal drop entity");
    assert_eq!(drops_seen[0].0, mc_world::drops::ITEM_COAL);
    assert!(drops_seen[0].1 >= 1 && drops_seen[0].1 <= 3, "fortune extra is 0..2");

    // 精准采集：挖石头掉石头自身物品（不是圆石）。
    drops.set_tool_enchant(conn, mc_world::tools::ToolEnchant::SilkTouch);
    mutable.set(gx, gy, gz, stone);
    dig(&mut s, gx, gy, gz, packets::PLAYER_ACTION_FINISH_DIG, 13).await;
    let (_, drops_seen) = read_dig_result(&mut s, 13).await;
    assert_eq!(drops_seen, vec![(mc_world::drops::ITEM_STONE, 1)]);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn workbench_grid9_and_quick_move_are_authoritative() {
    let dir = std::env::temp_dir().join(format!("mc-p47-craft9-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut inv = PlayerInventory::new();
    inv.set_slot(36, Some((mc_world::drops::ITEM_CRAFTING_TABLE, 1)));
    inv.set_slot(9, Some((mc_world::drops::ITEM_WHEAT, 9)));
    inv.save_to_dir(&dir, "Crafter9", DEFAULT_SEED).unwrap();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "craft9".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Crafter9", 2).await;
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    send_pos_y(&mut s, fx, fy, fz).await;
    let _ = read_content(&mut s).await;

    // 手持工作台右键顶面放置 → 会话登记 + 手持扣 1。
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0, x: gx, y: gy, z: gz, face: 1,
        cursor_x: 0.5, cursor_y: 1.0, cursor_z: 0.5,
        inside_block: false, world_border_hit: false, sequence: 21,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(
        packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id,
        mc_world::drops::CRAFTING_TABLE_BLOCK_ID as i32
    );
    let sessions = drops.craft9_sessions();
    let session = sessions.first().cloned().expect("placing a workbench opens a 3×3 session");
    assert_eq!((session.x, session.y, session.z), (gx, gy + 1, gz));

    // 窗口 1：背包槽 9（窗口 10）拿 9 小麦 → 合成格 1 → 结果槽 SHIFT 整理入包。
    let window = mc_world::crafting::CRAFT9_WINDOW_ID;
    send_click(&mut s, window, 1, 10, 0, packets::CLICK_PICKUP).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.window_id, window);
    send_click(&mut s, window, 2, 1, 0, packets::CLICK_PICKUP).await;
    let c = read_content(&mut s).await;
    assert_eq!(
        c.slots[1],
        packets::Slot { item_id: mc_world::drops::ITEM_WHEAT, count: 9 }
    );
    send_click(&mut s, window, 3, 0, 0, packets::CLICK_QUICK_MOVE).await;
    let c = read_content(&mut s).await;
    let hay = mc_world::crafting::RECIPES
        .iter().find(|r| r.name == "hay_block").unwrap().result.0;
    assert!(c.slots.iter().any(|s| s.item_id == hay && s.count == 1));
    assert!(drops.craft9_sessions()[0].grid.iter().all(|s| s.is_none()));
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn destroy_stage_is_broadcast_to_viewers_but_not_actor_ack() {
    let dir = std::env::temp_dir().join(format!("mc-p47-stage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let hub = Arc::new(BroadcastHub::new());
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let (events, rx) = crossbeam_channel::unbounded();
    let config = NetConfig::new(100, "stage".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub))
        .with_play_events(events)
        .with_destroy_stage(true);
    let addr = spawn_server(config).await;
    let mut a = login_and_join(addr, "Digger", 2).await;
    let mut b = login_and_join(addr, "Viewer", 2).await;
    for s in [&mut a, &mut b] {
        let _ = read_content(s).await;
    }
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    send_pos_y(&mut a, fx, fy, fz).await;
    send_pos_y(&mut b, fx + 4.0, fy, fz).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 生存 START → 视者收 stage 0；本人只收 Ack，不收任何 0x05。
    dig(&mut a, gx, gy, gz, packets::PLAYER_ACTION_START_DIG, 31).await;
    let mut actor_stage = false;
    for _ in 0..8 {
        let (id, payload) = read_frame_timeout(&mut a, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_BLOCK_DESTRUCTION_S2C_ID { actor_stage = true }
        if id == packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID
            && read_varint(&payload).unwrap().0 == 31 { break }
    }
    assert!(!actor_stage, "actor must not get destroy-stage broadcast");
    let mut saw_start = false;
    for _ in 0..20 {
        let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_BLOCK_DESTRUCTION_S2C_ID {
            let p = packets::BlockDestruction::decode_body(&payload).unwrap().0;
            if p.stage == 0 { saw_start = true; break }
        }
    }
    assert!(saw_start, "viewers must receive stage 0 on START");

    // STOP → 视者收 stage 255（取消动画）。
    dig(&mut a, gx, gy, gz, packets::PLAYER_ACTION_FINISH_DIG, 32).await;
    let _ = read_dig_result(&mut a, 32).await;
    let mut saw_cancel = false;
    for _ in 0..20 {
        let (id, payload) = read_frame_timeout(&mut b, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_BLOCK_DESTRUCTION_S2C_ID {
            let p = packets::BlockDestruction::decode_body(&payload).unwrap().0;
            if p.stage == packets::DESTROY_STAGE_CANCEL { saw_cancel = true; break }
        }
    }
    assert!(saw_cancel, "viewers must receive stage 255 on STOP");

    // 创造模式：START 即破坏，且无掉落（mode 由 hub 按连接设置）。
    let digger = hub.find_conn_by_name("Digger").expect("digger registered");
    assert!(hub.set_gamemode(digger, GameMode::Creative));
    mutable.set(gx, gy, gz, mc_world::GRASS_BLOCK_ID);
    dig(&mut a, gx, gy, gz, packets::PLAYER_ACTION_START_DIG, 33).await;
    let (_, drops_seen) = read_dig_result(&mut a, 33).await;
    assert!(drops_seen.is_empty(), "creative dig must not spawn drops");
    assert_eq!(mutable.get(gx, gy, gz), mc_world::AIR_BLOCK_ID, "creative START breaks instantly");
    drop(rx);
    drop(b);
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}
