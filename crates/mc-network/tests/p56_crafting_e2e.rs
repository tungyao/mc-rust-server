//! P56 完整合成端到端：背包 2×2 的 SHIFT/SWAP/格内 SHIFT，工作台 3×3 的
//! 背包 SHIFT 补料，以及两种网格共用同一张 1185 条配方表。
//! 所有包序断言沿用既有契约：点击后主同步包是 0x12（window 0/1），
//! 工作台右键恒发 0x3c + window-1 全量包（GUI 写死常开）。
mod common;

use common::*;
use mc_network::{ChunkCache, DropState, NetConfig};
use mc_protocol::{packets, write_varint};
use mc_world::{MutableWorld, PlayerInventory, WorldGen, DEFAULT_SEED};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

fn click_frame(container: i32, state: i32, slot: i16, button: u8, ty: u8) -> Vec<u8> {
    let mut b = Vec::new();
    write_varint(container, &mut b);
    write_varint(state, &mut b);
    b.extend_from_slice(&slot.to_be_bytes());
    b.push(button);
    write_varint(ty as i32, &mut b);
    write_varint(0, &mut b); // changedSlots = 0
    b.push(0); // carried：空栈
    b
}

async fn send_click(
    s: &mut tokio::net::TcpStream,
    container: i32,
    state: i32,
    slot: i16,
    button: u8,
    ty: u8,
) {
    let frame = click_frame(container, state, slot, button, ty);
    mc_network::framing::write_packet(s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
}

/// 读到下一个指定 window 的 0x12，同时把途中所有包 ID 返回（用于断言没有新增包）。
async fn read_content_window(
    s: &mut tokio::net::TcpStream,
    window: i32,
) -> (packets::SetContainerContent, Vec<i32>) {
    let mut ids = Vec::new();
    for _ in 0..40 {
        let (id, body) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        ids.push(id);
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            let c = packets::SetContainerContent::decode_body(&body).unwrap().0;
            if c.window_id == window {
                return (c, ids);
            }
        }
    }
    panic!("no content packet for window {window}");
}

async fn serve(
    dir: &std::path::Path,
    user: &str,
    inv: PlayerInventory,
) -> std::net::SocketAddr {
    inv.save_to_dir(dir, user, DEFAULT_SEED).unwrap();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.to_path_buf(), DEFAULT_SEED));
    let config = NetConfig::new(100, "p56-crafting".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(mutable)
        .with_drops(drops);
    spawn_server(config).await
}

fn recipe(name: &str) -> &'static mc_world::crafting::Recipe {
    mc_world::crafting::RECIPES
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("recipe {name} missing"))
}

/// 26.3 jar `InventoryMenu.quickMoveStack(0)` + `doClick` QUICK_MOVE 循环：
/// SHIFT 结果槽把连续合成的产物入包，不碰光标，且点击后仍是单个 0x12（无新增包）。
#[tokio::test]
async fn window0_shift_result_crafts_into_inventory_with_no_new_packets() {
    let dir = std::env::temp_dir().join(format!("mc-p56-w0-shift-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let planks = mc_world::drops::ITEM_OAK_PLANKS;
    let mut inv = PlayerInventory::new();
    inv.set_slot(9, Some((planks, 1)));
    inv.set_slot(10, Some((planks, 1)));
    let addr = serve(&dir, "P56Shift", inv).await;
    let mut s = login_and_join(addr, "P56Shift", 2).await;
    let _ = read_content_window(&mut s, 0).await.0;

    // 两格各 1 木板 → 4 木棍（26.3 `stick` 配方）。
    send_click(&mut s, 0, 1, 9, 0, packets::CLICK_PICKUP).await;
    let _ = read_content_window(&mut s, 0).await.0;
    send_click(&mut s, 0, 2, 1, 0, packets::CLICK_PICKUP).await;
    let _ = read_content_window(&mut s, 0).await.0;
    send_click(&mut s, 0, 3, 10, 0, packets::CLICK_PICKUP).await;
    let _ = read_content_window(&mut s, 0).await.0;
    send_click(&mut s, 0, 4, 2, 0, packets::CLICK_PICKUP).await;
    let c = read_content_window(&mut s, 0).await.0;
    assert_eq!(c.slots[1].item_id, planks);
    assert_eq!(c.slots[2].item_id, planks);
    assert_eq!(
        c.slots[mc_world::crafting::RESULT_SLOT],
        packets::Slot { item_id: recipe("stick").result.0, count: 4 },
        "server computes the result slot for the client (jar slotChangedCraftingGrid)"
    );

    send_click(&mut s, 0, 5, 0, 0, packets::CLICK_QUICK_MOVE).await;
    let (c, ids) = read_content_window(&mut s, 0).await;
    assert_eq!(c.carried, packets::Slot::empty(), "SHIFT 不动光标");
    assert_eq!(c.slots[1], packets::Slot::empty(), "inputs consumed");
    assert_eq!(c.slots[2], packets::Slot::empty(), "inputs consumed");
    assert_eq!(
        c.slots[36],
        packets::Slot { item_id: recipe("stick").result.0, count: 4 },
        "result stowed to hotbar"
    );
    assert!(!ids.contains(&packets::PLAY_SET_CONTAINER_SLOT_S2C_ID));
    assert!(!ids.contains(&packets::PLAY_OPEN_SCREEN_S2C_ID));
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 26.3 jar `InventoryMenu.quickMoveStack` 对合成格 1..=4 的 SHIFT：搬进背包 9..45。
#[tokio::test]
async fn window0_grid_shift_moves_stack_into_backpack() {
    let dir = std::env::temp_dir().join(format!("mc-p56-w0-grid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let log = mc_world::drops::ITEM_OAK_LOG;
    let mut inv = PlayerInventory::new();
    inv.set_slot(9, Some((log, 3)));
    let addr = serve(&dir, "P56Grid", inv).await;
    let mut s = login_and_join(addr, "P56Grid", 2).await;
    let _ = read_content_window(&mut s, 0).await.0;

    send_click(&mut s, 0, 1, 9, 0, packets::CLICK_PICKUP).await;
    let _ = read_content_window(&mut s, 0).await.0;
    send_click(&mut s, 0, 2, 1, 0, packets::CLICK_PICKUP).await;
    let _ = read_content_window(&mut s, 0).await.0;
    // 合成格 SHIFT（旧实现是 no-op，jar 要搬到背包）。
    send_click(&mut s, 0, 3, 1, 0, packets::CLICK_QUICK_MOVE).await;
    let c = read_content_window(&mut s, 0).await.0;
    assert_eq!(c.slots[1], packets::Slot::empty());
    assert_eq!(c.slots[36], packets::Slot { item_id: log, count: 3 });
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 两种网格共用同一张配方表：2×2 用 3 小麦做面包；3×3 用 9 小麦做干草捆。
/// GUI 开启后窗口 1 的背包 SHIFT 先补合成格（`CraftingMenu.quickMoveStack`），
/// 再从结果槽拿到产物；开关关闭时旧包流不变由 P55 e2e 覆盖。
#[tokio::test]
async fn gui_workbench_shift_fills_grid_and_shares_recipe_table() {
    let dir = std::env::temp_dir().join(format!("mc-p56-gui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let wheat = mc_world::drops::ITEM_WHEAT;
    let mut inv = PlayerInventory::new();
    inv.set_slot(36, Some((mc_world::drops::ITEM_CRAFTING_TABLE, 1)));
    inv.set_slot(9, Some((wheat, 9)));
    inv.set_slot(10, Some((wheat, 3)));
    let addr = serve(&dir, "P56Gui", inv).await;
    let mut s = login_and_join(addr, "P56Gui", 2).await;
    let _ = read_content_window(&mut s, 0).await.0;

    // --- 2×2：3 小麦 → 面包（同一张 RECIPES 表） ---
    send_click(&mut s, 0, 1, 10, 0, packets::CLICK_PICKUP).await;
    let _ = read_content_window(&mut s, 0).await.0;
    send_click(&mut s, 0, 2, 1, 0, packets::CLICK_PICKUP).await;
    let _ = read_content_window(&mut s, 0).await.0;
    send_click(&mut s, 0, 3, 0, 0, packets::CLICK_PICKUP).await;
    let c = read_content_window(&mut s, 0).await.0;
    let bread = recipe("bread").result.0;
    assert_eq!(c.carried.item_id, bread);
    // 光标里的面包放进主背包槽 11。
    send_click(&mut s, 0, 4, 11, 0, packets::CLICK_PICKUP).await;
    let _ = read_content_window(&mut s, 0).await.0;

    // --- 放工作台打开 GUI（BlockUpdate → Ack 相邻，随后 0x3c + 0x12(window 1)） ---
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    send_pos_y(
        &mut s,
        spawn.0 as f64 + 0.5,
        spawn.1 as f64,
        spawn.2 as f64 + 0.5,
    )
    .await;
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
        sequence: 56,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let (c, ids) = read_content_window(&mut s, mc_world::crafting::CRAFT9_WINDOW_ID).await;
    assert!(ids.contains(&packets::PLAY_OPEN_SCREEN_S2C_ID));
    assert_eq!(c.slots.len(), 46);
    assert_eq!(c.slots[0], packets::Slot::empty());

    // --- 工作台：窗口槽 10 = 背包槽 9 的 9 小麦，SHIFT 先补合成格 1 ---
    let window = mc_world::crafting::CRAFT9_WINDOW_ID;
    send_click(&mut s, window, 1, 10, 0, packets::CLICK_QUICK_MOVE).await;
    let c = read_content_window(&mut s, window).await.0;
    assert_eq!(
        c.slots[1],
        packets::Slot { item_id: wheat, count: 9 },
        "SHIFT must fill the 3×3 grid before moving within the inventory"
    );
    assert_eq!(c.slots[10], packets::Slot::empty());
    assert_eq!(
        c.slots[mc_world::crafting::CRAFT9_RESULT_SLOT as usize],
        packets::Slot { item_id: recipe("hay_block").result.0, count: 1 },
        "3×3 result slot is authoritative on the server, not client-predicted"
    );
    // 结果槽：9 小麦 → 干草捆（与 2×2 共用配方表）。
    send_click(&mut s, window, 2, 0, 0, packets::CLICK_PICKUP).await;
    let c = read_content_window(&mut s, window).await.0;
    let hay = recipe("hay_block").result.0;
    assert_eq!(c.carried.item_id, hay);
    assert_eq!(c.slots[1], packets::Slot::empty(), "grid consumed authoritatively");
    assert_eq!(c.slots[12].item_id, bread, "2×2 result from the same table survives");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
