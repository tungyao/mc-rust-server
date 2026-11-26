//! P40 合成 + 生存基础端到端测试：2×2 配方、SetHealth 同步、饥饿衰减/扣血、
//! 死亡掉落与重生、重进状态仍在、进食。共享脚手架见 common.rs。
mod common;

use common::*;
use mc_network::{BroadcastHub, ChunkCache, DropState, NetConfig};
use mc_protocol::packets;
use mc_protocol::write_varint;
use mc_world::{PlayerInventory, SurvivalState, MutableWorld, WorldGen, DEFAULT_SEED};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

fn click_frame(state: i32, slot: i16, button: u8, ty: u8) -> Vec<u8> {
    let mut b = Vec::new();
    write_varint(0, &mut b); // container_id = player inventory
    write_varint(state, &mut b);
    b.extend_from_slice(&slot.to_be_bytes());
    b.push(button);
    write_varint(ty as i32, &mut b);
    write_varint(0, &mut b); // changed slot count (server ignores)
    b.push(0); // HashedSlot marker (server skips)
    b
}

async fn send_click(s: &mut tokio::net::TcpStream, state: i32, slot: i16, button: u8, ty: u8) {
    let frame = click_frame(state, slot, button, ty);
    mc_network::framing::write_packet(s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
}

/// 读到下一份全量背包 Content（跳过 KeepAlive 等无关包）。
async fn read_content(s: &mut tokio::net::TcpStream) -> packets::SetContainerContent {
    for _ in 0..40 {
        let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            return packets::SetContainerContent::decode_body(&payload).unwrap().0;
        }
    }
    panic!("no container content packet");
}

/// 读到第一份满足 predicate 的指定包（跳过无关包）。
async fn read_until<T>(
    s: &mut tokio::net::TcpStream,
    id: i32,
    mut decode: impl FnMut(&[u8]) -> T,
    mut accept: impl FnMut(&T) -> bool,
) -> T {
    for _ in 0..80 {
        let (got, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        if got == id {
            let v = decode(&payload);
            if accept(&v) {
                return v;
            }
        }
    }
    panic!("packet {id:#04x} matching predicate never arrived");
}

fn health_of(payload: &[u8]) -> packets::SetHealth {
    packets::SetHealth::decode_body(payload).unwrap().0
}

async fn seed_save(dir: &std::path::Path, name: &str, mut inv: PlayerInventory, seed: i64) {
    inv.stow_cursor();
    inv.save_to_dir(dir, name, seed).unwrap();
}

#[tokio::test]
async fn craft_oak_planks_then_sticks_in_2x2_grid() {
    let dir = std::env::temp_dir().join(format!("mc-p40-craft-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut inv = PlayerInventory::new();
    inv.set_slot(9, Some((mc_world::drops::ITEM_OAK_LOG, 1)));
    seed_save(&dir, "Crafter", inv, DEFAULT_SEED).await;

    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "craft".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Crafter", 2).await;

    let c = read_content(&mut s).await;
    assert_eq!(c.slots[9], packets::Slot { count: 1, item_id: mc_world::drops::ITEM_OAK_LOG });

    // 原木 9 → 合成格 1 → 点结果槽 → 4 木板进光标。
    send_click(&mut s, 1, 9, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.carried.item_id, mc_world::drops::ITEM_OAK_LOG);
    send_click(&mut s, 2, 1, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.slots[1], packets::Slot { count: 1, item_id: mc_world::drops::ITEM_OAK_LOG });
    send_click(&mut s, 3, 0, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.carried, packets::Slot { count: 4, item_id: mc_world::drops::ITEM_OAK_PLANKS });
    assert_eq!(c.slots[1], packets::Slot::empty(), "inputs must be consumed");

    // 把 4 木板放回主包，再每次右键放 1 个到格 1/2 → 2 木板 → 4 木棍。
    send_click(&mut s, 4, 9, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 5, 9, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 6, 1, 1, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 7, 2, 1, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.slots[1], packets::Slot { count: 1, item_id: mc_world::drops::ITEM_OAK_PLANKS });
    assert_eq!(c.slots[2], packets::Slot { count: 1, item_id: mc_world::drops::ITEM_OAK_PLANKS });
    // 光标还剩 2 木板（与结果不同物）：先放回主包，再点结果槽取木棍。
    send_click(&mut s, 8, 9, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 9, 0, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.carried, packets::Slot { count: 4, item_id: mc_world::drops::ITEM_STICK });
    assert_eq!(c.slots[1], packets::Slot::empty());
    assert_eq!(c.slots[2], packets::Slot::empty());
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn join_set_health_sync_and_state_survives_rejoin() {
    let dir = std::env::temp_dir().join(format!("mc-p40-health-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut inv = PlayerInventory::new();
    inv.set_slot(9, Some((mc_world::drops::ITEM_STONE, 2)));
    inv.set_survival(SurvivalState { health: 15.0, food: 10, saturation: 2.5 });
    seed_save(&dir, "Hungry", inv, DEFAULT_SEED).await;

    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "health".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;

    for _ in 0..2 {
        let mut s = login_and_join(addr, "Hungry", 2).await;
        let c = read_content(&mut s).await;
        assert_eq!(c.slots[9].item_id, mc_world::drops::ITEM_STONE);
        let h = read_until(&mut s, packets::PLAY_SET_HEALTH_S2C_ID, health_of, |h| {
            h.food == 10
        })
        .await;
        assert_eq!((h.health, h.food, h.saturation), (15.0, 10, 2.5));
        drop(s);
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn hunger_decays_to_zero_and_starvation_damages_on_global_ticker() {
    let dir = std::env::temp_dir().join(format!("mc-p40-starve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut inv = PlayerInventory::new();
    inv.set_survival(SurvivalState { health: 20.0, food: 1, saturation: 0.0 });
    seed_save(&dir, "Starving", inv, DEFAULT_SEED).await;

    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let hub = Arc::new(BroadcastHub::new());
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    drops.set_hunger_ticks(2);
    drops.set_starvation_ticks(2);
    let config = NetConfig::new(100, "starve".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_hub(Arc::clone(&hub))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Starving", 2).await;
    let _ = read_content(&mut s).await;
    let _ = read_until(&mut s, packets::PLAY_SET_HEALTH_S2C_ID, health_of, |h| h.food == 1).await;

    let hurt = read_until(&mut s, packets::PLAY_SET_HEALTH_S2C_ID, health_of, |h| {
        h.food == 0 && h.health < 20.0
    })
    .await;
    assert!(hurt.health < 20.0, "starvation must subtract health once food hits zero");
    assert!(drops.survival_state(hub.find_conn_by_name("Starving").unwrap()).unwrap().food == 0);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn fall_death_drops_inventory_clears_bag_and_respawns_at_spawn() {
    let dir = std::env::temp_dir().join(format!("mc-p40-death-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut inv = PlayerInventory::new();
    inv.set_slot(36, Some((mc_world::drops::ITEM_STONE, 3)));
    inv.set_survival(SurvivalState { health: 1.0, food: 20, saturation: 5.0 });
    seed_save(&dir, "Faller", inv, DEFAULT_SEED).await;

    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let hub = Arc::new(BroadcastHub::new());
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "death".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_hub(Arc::clone(&hub))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Faller", 2).await;
    let _ = read_content(&mut s).await;
    let _ = read_until(&mut s, packets::PLAY_SET_HEALTH_S2C_ID, health_of, |h| h.health == 1.0).await;

    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (x, y, z) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    let mut airborne = Vec::new();
    packets::MovePlayerPos {
        x,
        y: y + 12.0,
        z,
        on_ground: false,
        horizontal_collision: false,
    }
    .encode_body(&mut airborne);
    mc_network::framing::write_packet(&mut s, packets::PLAY_MOVE_POS_C2S_ID, &airborne).await.unwrap();
    let mut landed = Vec::new();
    packets::MovePlayerPos { x, y, z, on_ground: true, horizontal_collision: false }
        .encode_body(&mut landed);
    mc_network::framing::write_packet(&mut s, packets::PLAY_MOVE_POS_C2S_ID, &landed).await.unwrap();

    // 死亡闭环：掉落 Spawn + 清空后的 Content + 回出生点 PositionSync + 满血 SetHealth。
    let mut saw_spawn = false;
    let mut saw_empty = false;
    let mut saw_pos = false;
    let mut saw_full_health = false;
    for _ in 0..100 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_SPAWN_ENTITY_S2C_ID => saw_spawn = true,
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let c = packets::SetContainerContent::decode_body(&payload).unwrap().0;
                if c.slots.iter().all(|slot| *slot == packets::Slot::empty()) {
                    saw_empty = true;
                }
            }
            packets::PLAY_PLAYER_POSITION_SYNC_ID => {
                let p = packets::PlayerPositionSync::decode_body(&payload).unwrap().0;
                assert_eq!((p.x, p.y, p.z), (x, y, z));
                saw_pos = true;
            }
            packets::PLAY_SET_HEALTH_S2C_ID => {
                if health_of(&payload).health == 20.0 {
                    saw_full_health = true;
                }
            }
            _ => {}
        }
        if saw_spawn && saw_empty && saw_pos && saw_full_health {
            break;
        }
    }
    assert!(saw_spawn && saw_empty && saw_pos && saw_full_health);
    let items = drops.items.lock().unwrap();
    let ids = items.ids();
    assert_eq!(ids.len(), 1, "bag contents must become one dropped stack");
    let e = items.get(ids[0]).unwrap();
    assert_eq!((e.item_id, e.count), (mc_world::drops::ITEM_STONE, 3));
    drop(items);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn eating_food_restores_hunger_and_sends_set_health() {
    let dir = std::env::temp_dir().join(format!("mc-p40-eat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut inv = PlayerInventory::new();
    inv.set_slot(36, Some((mc_world::drops::ITEM_APPLE, 1)));
    inv.set_survival(SurvivalState { health: 20.0, food: 10, saturation: 0.0 });
    seed_save(&dir, "Eater", inv, DEFAULT_SEED).await;

    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let hub = Arc::new(BroadcastHub::new());
    let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
    let config = NetConfig::new(100, "eat".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_hub(Arc::clone(&hub))
        .with_drops(Arc::clone(&drops));
    let addr = spawn_server(config).await;
    let mut s = login_and_join(addr, "Eater", 2).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.slots[36].item_id, mc_world::drops::ITEM_APPLE);
    let _ = read_until(&mut s, packets::PLAY_SET_HEALTH_S2C_ID, health_of, |h| h.food == 10).await;

    let mut body = Vec::new();
    packets::UseItem { hand: 0, sequence: 7, yaw: 0.0, pitch: 0.0 }.encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_C2S_ID, &body).await.unwrap();
    let healed = read_until(&mut s, packets::PLAY_SET_HEALTH_S2C_ID, health_of, |h| h.food > 10).await;
    assert_eq!(healed.food, 14, "apple restores 4 food");
    let conn = hub.find_conn_by_name("Eater").unwrap();
    assert_eq!(drops.with_inventory(conn, |inv| inv.survival().food), Some(14));
    assert!(drops.with_inventory(conn, |inv| inv.held().is_none()).unwrap_or(false));
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
