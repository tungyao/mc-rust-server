//! P55 B6 工作台 GUI e2e：右键/放置工作台恒发 Open Screen 0x3c +
//! Content 0x12(window 1)；点击服务端权威同步；Join 序列不变。
//! GUI 写死常开。menu type=12（真机已校准），标题为无名 NBT String tag。
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
    write_varint(0, &mut b);
    b.push(0); // carried：空栈 count=0
    b
}

async fn send_click(s: &mut tokio::net::TcpStream, container: i32, state: i32, slot: i16, ty: u8) {
    let frame = click_frame(container, state, slot, 0, ty);
    mc_network::framing::write_packet(s, packets::PLAY_CLICK_CONTAINER_C2S_ID, &frame)
        .await
        .unwrap();
}

async fn read_content_window(s: &mut tokio::net::TcpStream, window: i32) -> packets::SetContainerContent {
    for _ in 0..40 {
        let (id, body) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            let c = packets::SetContainerContent::decode_body(&body).unwrap().0;
            if c.window_id == window {
                return c;
            }
        }
    }
    panic!("no content packet for window {window}");
}

async fn serve_gui(
    dir: &std::path::Path,
    user: &str,
) -> (std::net::SocketAddr, Arc<DropState>, Arc<MutableWorld>) {
    let mut inv = PlayerInventory::new();
    inv.set_slot(36, Some((mc_world::drops::ITEM_CRAFTING_TABLE, 1)));
    inv.set_slot(9, Some((mc_world::drops::ITEM_WHEAT, 9)));
    inv.save_to_dir(dir, user, DEFAULT_SEED).unwrap();
    let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(dir.to_path_buf(), DEFAULT_SEED));
    let config = NetConfig::new(100, "p55-gui".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops));
    (spawn_server(config).await, drops, mutable)
}

async fn place_workbench(s: &mut tokio::net::TcpStream) {
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    send_pos_y(s, fx, fy, fz).await;
    let _ = read_content_window(s, 0).await; // Join 后的背包全量同步
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
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
    mc_network::framing::write_packet(s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
}

#[tokio::test]
async fn gui_enabled_place_open_screen_content_and_authoritative_clicks() {
    let dir = std::env::temp_dir().join(format!("mc-p55-gui-on-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (addr, drops, _mutable) =
        serve_gui(&dir, "GuiCrafter").await;
    let mut s = login_and_join(addr, "GuiCrafter", 2).await;
    place_workbench(&mut s).await;

    // 关键包序：BlockUpdate → Ack 相邻，随后才是 0x3c + 0x12（window 1）。
    let mut ids = Vec::new();
    let mut open = None;
    let mut content = None;
    for _ in 0..30 {
        let (id, body) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        ids.push(id);
        if id == packets::PLAY_OPEN_SCREEN_S2C_ID {
            open = Some(packets::OpenScreen::decode_body(&body).unwrap().0);
        }
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            let c = packets::SetContainerContent::decode_body(&body).unwrap().0;
            if c.window_id == mc_world::crafting::CRAFT9_WINDOW_ID {
                content = Some(c);
            }
        }
        if open.is_some() && content.is_some() {
            break;
        }
    }
    let ack = ids
        .iter()
        .position(|&id| id == packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID)
        .expect("place must ack");
    let update = ids
        .iter()
        .position(|&id| id == packets::PLAY_BLOCK_UPDATE_S2C_ID)
        .expect("place must send block update");
    assert_eq!(ack, update + 1, "Ack must stay adjacent to BlockUpdate (P52 red line)");
    let screen_idx = ids
        .iter()
        .position(|&id| id == packets::PLAY_OPEN_SCREEN_S2C_ID)
        .expect("GUI must send Open Screen when enabled");
    assert!(screen_idx > ack, "GUI packets must follow BlockUpdate→Ack");
    let open = open.expect("Open Screen body");
    assert_eq!(open.window_id, mc_world::crafting::CRAFT9_WINDOW_ID);
    assert_eq!(open.menu_type, 12, "26.3 jar: MenuType.CRAFTING is 13th builtin menu");
    assert_eq!(open.title, "Crafting");
    let content = content.expect("window 1 content");
    assert_eq!(content.slots.len(), 46);
    assert_eq!(content.slots[0], packets::Slot::empty());

    // 点击合成窗口：背包槽 9（窗口槽 10）取 9 小麦 → 合成格 1 → 结果槽拿干草捆。
    let window = mc_world::crafting::CRAFT9_WINDOW_ID;
    send_click(&mut s, window, 1, 10, packets::CLICK_PICKUP).await;
    let c = read_content_window(&mut s, window).await;
    assert_eq!(c.carried.item_id, mc_world::drops::ITEM_WHEAT);
    send_click(&mut s, window, 2, 1, packets::CLICK_PICKUP).await;
    let c = read_content_window(&mut s, window).await;
    assert_eq!(
        c.slots[1],
        packets::Slot { item_id: mc_world::drops::ITEM_WHEAT, count: 9 }
    );
    send_click(&mut s, window, 3, 0, packets::CLICK_PICKUP).await;
    let c = read_content_window(&mut s, window).await;
    let hay = mc_world::crafting::RECIPES
        .iter()
        .find(|r| r.name == "hay_block")
        .unwrap()
        .result
        .0;
    assert_eq!(c.carried.item_id, hay);
    assert_eq!(c.slots[1], packets::Slot::empty(), "inputs must be authoritative");
    assert_eq!(drops.craft9_sessions().len(), 1, "GUI uses the single craft9 session");
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// GUI 写死常开：放置工作台恒发 0x3c + window-1 全量包（旧的"关闭零包"行为已删除）。
#[tokio::test]
async fn gui_disabled_keeps_old_packet_stream_with_no_open_screen() {
    let dir = std::env::temp_dir().join(format!("mc-p55-gui-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (addr, drops, _mutable) =
        serve_gui(&dir, "NoGuiCrafter").await;
    let mut s = login_and_join(addr, "NoGuiCrafter", 2).await;
    place_workbench(&mut s).await;

    let mut ids = Vec::new();
    for _ in 0..30 {
        let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        ids.push(id);
        if ids.contains(&packets::PLAY_OPEN_SCREEN_S2C_ID) {
            break;
        }
    }
    assert!(ids.contains(&packets::PLAY_BLOCK_UPDATE_S2C_ID));
    assert!(ids.contains(&packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID));
    assert!(
        ids.contains(&packets::PLAY_OPEN_SCREEN_S2C_ID),
        "always-on GUI must emit Open Screen on place"
    );
    // P47 行为保留：会话在服务端（同时有客户端 GUI）。
    assert_eq!(drops.craft9_sessions().len(), 1);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 右键**已存在**的工作台方块（不是放置）也走 use_item_on 路由开 GUI，
/// 且不产生方块编辑/Ack（客户端对交互无预测，Ack 只属于方块变更）。
#[tokio::test]
async fn right_click_existing_workbench_opens_gui_without_block_edit() {
    let dir = std::env::temp_dir().join(format!("mc-p55-rightclick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (addr, drops, mutable) =
        serve_gui(&dir, "RightClicker").await;
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    mutable.set(gx, gy, gz, mc_world::drops::CRAFTING_TABLE_BLOCK_ID);

    let mut s = login_and_join(addr, "RightClicker", 2).await;
    let (fx, fy, fz) = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    send_pos_y(&mut s, fx, fy, fz).await;
    let _ = read_content_window(&mut s, 0).await;
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
        sequence: 44,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(&mut s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();

    let mut ids = Vec::new();
    for _ in 0..20 {
        let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        ids.push(id);
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            break;
        }
    }
    assert!(ids.contains(&packets::PLAY_OPEN_SCREEN_S2C_ID));
    assert!(ids.contains(&packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID));
    assert!(
        !ids.contains(&packets::PLAY_BLOCK_UPDATE_S2C_ID),
        "interacting with a workbench must not edit blocks"
    );
    assert!(!ids.contains(&packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID));
    assert_eq!(drops.craft9_sessions()[0].x, gx);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn gui_enabled_join_sequence_unchanged() {
    let dir = std::env::temp_dir().join(format!("mc-p55-join-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (addr, _drops, _mutable) = serve_gui(&dir, "JoinCrafter").await;
    let (mut s, obs) = login_and_join_observed(addr, "JoinCrafter", 2).await;
    assert_eq!(obs.chunk_count, mc_network::view_chunk_count(2), "GUI must not add Join packets");
    assert_eq!(obs.batch_size, mc_network::view_chunk_count(2) as i32);
    // Join 完成后仍无 Open Screen（只在右键工作台时触发）。
    let extra = tokio::time::timeout(Duration::from_millis(250), read_frame(&mut s)).await;
    if let Ok(Ok((id, _))) = extra {
        assert_ne!(id, packets::PLAY_OPEN_SCREEN_S2C_ID);
    }
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
