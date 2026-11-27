//! P41 扩充合成端到端：放料 → 结果 → 扣料 → 全量同步（共享脚手架见 common.rs）。
//! 两条新配方：铁镐（3 铁锭 + 2 木棍）、箱子与红床（含第二份光标处理）。
mod common;

use common::*;
use mc_network::{ChunkCache, DropState, NetConfig};
use mc_protocol::{packets, write_varint};
use mc_world::{crafting::Recipe, MutableWorld, PlayerInventory, DEFAULT_SEED};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

fn recipe(name: &str) -> &'static Recipe {
    mc_world::crafting::RECIPES
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no recipe {name}"))
}

fn click_frame(state: i32, slot: i16, button: u8, ty: u8) -> Vec<u8> {
    let mut b = Vec::new();
    write_varint(0, &mut b);
    write_varint(state, &mut b);
    b.extend_from_slice(&slot.to_be_bytes());
    b.push(button);
    write_varint(ty as i32, &mut b);
    write_varint(0, &mut b);
    b.push(0);
    b
}

async fn send_click(s: &mut tokio::net::TcpStream, state: i32, slot: i16, button: u8, ty: u8) {
    let frame = click_frame(state, slot, button, ty);
    mc_network::framing::write_packet(s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
}

/// 每次点击必发全量 46 槽内容（P18/P40 既有同步契约）。
async fn read_content(s: &mut tokio::net::TcpStream) -> packets::SetContainerContent {
    for _ in 0..40 {
        let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            return packets::SetContainerContent::decode_body(&payload)
                .unwrap()
                .0;
        }
    }
    panic!("no container content packet after click");
}

async fn serve_seeded(
    dir: &std::path::Path,
    user: &str,
    inv: PlayerInventory,
) -> std::net::SocketAddr {
    inv.save_to_dir(dir, user, DEFAULT_SEED).unwrap();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.to_path_buf(), DEFAULT_SEED));
    let config = NetConfig::new(100, "p41-recipes".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(mutable)
        .with_drops(drops);
    spawn_server(config).await
}

/// P60 2×2 形状口径：火把（煤上棍下左列）→ 取结果 → 扣料 → 全量同步。
#[tokio::test]
async fn craft_torch_full_sync_and_inputs_consumed() {
    let dir = std::env::temp_dir().join(format!("mc-p60-torch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let coal = mc_world::drops::ITEM_COAL;
    let stick = mc_world::drops::ITEM_STICK;
    let r = recipe("torch");
    let mut inv = PlayerInventory::new();
    inv.set_slot(9, Some((coal, 2)));
    inv.set_slot(10, Some((stick, 5)));
    let addr = serve_seeded(&dir, "TorchCrafter", inv).await;
    let mut s = login_and_join(addr, "TorchCrafter", 2).await;

    let c = read_content(&mut s).await;
    assert_eq!(c.slots[9], packets::Slot { count: 2, item_id: coal });
    assert_eq!(c.slots[10], packets::Slot { count: 5, item_id: stick });

    // 左列：煤放槽 1，棍放槽 3。
    send_click(&mut s, 1, 9, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 2, 1, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 3, 10, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 4, 3, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.slots[1], packets::Slot { count: 2, item_id: coal });
    assert_eq!(c.slots[3], packets::Slot { count: 5, item_id: stick });

    // 点结果槽：4 火把进光标，每格原料扣 1，全量同步回客户端。
    send_click(&mut s, 5, 0, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.carried, packets::Slot { count: r.result.1, item_id: r.result.0 });
    assert_eq!(c.slots[1], packets::Slot { count: 1, item_id: coal }, "torch takes 1 coal");
    assert_eq!(c.slots[3], packets::Slot { count: 4, item_id: stick }, "torch takes 1 stick");
    assert_eq!(c.slots[9], packets::Slot::empty());
    assert_eq!(c.slots[10], packets::Slot::empty());
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// P60 书 = shapeless（3 纸 + 皮革占满 4 格，顺序无关）。
#[tokio::test]
async fn craft_book_full_sync_and_inputs_consumed() {
    let dir = std::env::temp_dir().join(format!("mc-p60-book-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let r = recipe("book");
    // 具体 ID 从表里取（不硬编码）：出现 3 次的是纸，出现 1 次的是皮革。
    let mut counts = std::collections::HashMap::new();
    for (id, _) in r.needs {
        *counts.entry(id).or_insert(0) += 1;
    }
    let paper = **counts.iter().find(|(_, &c)| c == 3).unwrap().0;
    let leather = **counts.iter().find(|(_, &c)| c == 1).unwrap().0;
    let mut inv = PlayerInventory::new();
    inv.set_slot(9, Some((paper, 1)));
    inv.set_slot(10, Some((paper, 1)));
    inv.set_slot(11, Some((paper, 1)));
    inv.set_slot(12, Some((leather, 1)));
    let addr = serve_seeded(&dir, "BookCrafter", inv).await;
    let mut s = login_and_join(addr, "BookCrafter", 2).await;

    let _ = read_content(&mut s).await;
    // 4 格逐一放满。
    for (st, wslot, gslot) in [(1, 9, 1), (2, 10, 2), (3, 11, 3), (4, 12, 4)] {
        send_click(&mut s, st, wslot, 0, 0).await;
        let _ = read_content(&mut s).await;
        send_click(&mut s, st + 10, gslot, 0, 0).await;
        let _ = read_content(&mut s).await;
    }
    // 点结果槽：书进光标，4 格全空。
    send_click(&mut s, 30, 0, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.carried, packets::Slot { count: 1, item_id: r.result.0 });
    assert_eq!(c.slots[1], packets::Slot::empty(), "book inputs consumed");
    assert_eq!(c.slots[2], packets::Slot::empty());
    assert_eq!(c.slots[3], packets::Slot::empty());
    assert_eq!(c.slots[4], packets::Slot::empty());
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
