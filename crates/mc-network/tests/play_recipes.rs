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

#[tokio::test]
async fn craft_iron_pickaxe_full_sync_and_inputs_consumed() {
    let dir = std::env::temp_dir().join(format!("mc-p41-pick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let r = recipe("iron_pickaxe");
    // P59 tag-native：铁料需求是 TAG（负数 id），背包里放具体铁锭
    //（从纯具体配方 cauldron 取铁锭 ID，不硬编码数字）；
    // 木棍仍是具体 ID。数量口径不变（3 铁 + 2 棍）。
    let iron_id = recipe("cauldron").needs.iter().find(|(id, _)| *id >= 0).unwrap().0;
    let stick_id = mc_world::drops::ITEM_STICK;
    assert!(r.needs.iter().any(|(id, n)| *id < 0 && *n == 3));
    assert!(r.needs.iter().any(|(id, n)| *id == stick_id && *n == 2));
    let iron = (iron_id, 3);
    let stick = (stick_id, 2);
    let mut inv = PlayerInventory::new();
    inv.set_slot(9, Some(iron));
    inv.set_slot(10, Some(stick));
    let addr = serve_seeded(&dir, "PickCrafter", inv).await;
    let mut s = login_and_join(addr, "PickCrafter", 2).await;

    let c = read_content(&mut s).await;
    assert_eq!(
        c.slots[9],
        packets::Slot {
            count: 3,
            item_id: iron.0
        }
    );
    assert_eq!(
        c.slots[10],
        packets::Slot {
            count: 2,
            item_id: stick.0
        }
    );

    send_click(&mut s, 1, 9, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.carried.item_id, iron.0);
    assert_eq!(c.slots[9], packets::Slot::empty());
    send_click(&mut s, 2, 1, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(
        c.slots[1],
        packets::Slot {
            count: 3,
            item_id: iron.0
        }
    );

    send_click(&mut s, 3, 10, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.carried.item_id, stick.0);
    send_click(&mut s, 4, 2, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(
        c.slots[2],
        packets::Slot {
            count: 2,
            item_id: stick.0
        }
    );

    // 点结果槽：铁镐进光标，合成格两槽原料被权威扣除，全量同步回客户端。
    send_click(&mut s, 5, 0, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(
        c.carried,
        packets::Slot {
            count: r.result.1,
            item_id: r.result.0
        }
    );
    assert_eq!(c.slots[1], packets::Slot::empty(), "iron must be consumed");
    assert_eq!(
        c.slots[2],
        packets::Slot::empty(),
        "sticks must be consumed"
    );
    assert_eq!(c.slots[9], packets::Slot::empty());
    assert_eq!(c.slots[10], packets::Slot::empty());
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn craft_chest_then_bed_full_sync_and_inputs_consumed() {
    let dir = std::env::temp_dir().join(format!("mc-p41-bed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let chest = recipe("chest");
    let bed = recipe("red_bed");
    let planks = mc_world::drops::ITEM_OAK_PLANKS;
    // P59 tag-native：木板需求是 TAG（负数 id），背包里放具体橡木木板；
    // 羊毛仍是具体 ID（取 red_bed 的非负需求项，不硬编码数字）。
    assert!(chest.needs.iter().any(|(id, n)| *id < 0 && *n == 8));
    let wool = bed.needs.iter().find(|(id, _)| *id >= 0).unwrap().0;
    let mut inv = PlayerInventory::new();
    inv.set_slot(9, Some((planks, 8)));
    inv.set_slot(10, Some((planks, 3)));
    inv.set_slot(11, Some((wool, 3)));
    let addr = serve_seeded(&dir, "BedCrafter", inv).await;
    let mut s = login_and_join(addr, "BedCrafter", 2).await;

    let c = read_content(&mut s).await;
    assert_eq!(
        c.slots[9],
        packets::Slot {
            count: 8,
            item_id: planks
        }
    );

    // 箱子：8 木板单格 → 1 箱子。
    send_click(&mut s, 1, 9, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 2, 1, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(
        c.slots[1],
        packets::Slot {
            count: 8,
            item_id: planks
        }
    );
    send_click(&mut s, 3, 0, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(
        c.carried,
        packets::Slot {
            count: 1,
            item_id: chest.result.0
        }
    );
    assert_eq!(c.slots[1], packets::Slot::empty(), "chest inputs consumed");
    // 把箱子放进空槽 12，清空光标再做床。
    send_click(&mut s, 4, 12, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(c.carried, packets::Slot::empty());
    assert_eq!(c.slots[12].item_id, chest.result.0);

    // 床：3 木板 + 3 红羊毛 → 1 红床，两格原料都扣掉。
    send_click(&mut s, 5, 10, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 6, 1, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 7, 11, 0, 0).await;
    let _ = read_content(&mut s).await;
    send_click(&mut s, 8, 2, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(
        c.slots[1],
        packets::Slot {
            count: 3,
            item_id: planks
        }
    );
    assert_eq!(
        c.slots[2],
        packets::Slot {
            count: 3,
            item_id: wool
        }
    );
    send_click(&mut s, 9, 0, 0, 0).await;
    let c = read_content(&mut s).await;
    assert_eq!(
        c.carried,
        packets::Slot {
            count: 1,
            item_id: bed.result.0
        }
    );
    assert_eq!(c.slots[1], packets::Slot::empty());
    assert_eq!(c.slots[2], packets::Slot::empty());
    assert_eq!(c.slots[10], packets::Slot::empty());
    assert_eq!(c.slots[11], packets::Slot::empty());
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
