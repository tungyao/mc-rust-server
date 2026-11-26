//! P33 多作物端到端：真空中走 Login/Configuration/Join → 手持种子右键土壤顶面 →
//! BlockUpdate + 扣种子 → 全服唯一 ticker 生长 → 成熟掉落 → 拾取入库；
//! 另覆盖挖土连带 pop 新作物。共享脚手架见 `common.rs`。
mod common;

use common::*;
use mc_network::{BroadcastHub, ChunkCache, DropState, NetConfig, PlayEvent};
use mc_protocol::{packets, read_varint};
use mc_world::crops::{crop_stage_id, BEETROOT, CARROT, POTATO};
use mc_world::drops::{ITEM_BEETROOT, ITEM_BEETROOT_SEEDS, ITEM_CARROT, ITEM_DIRT, ITEM_POTATO};
use mc_world::{MutableWorld, WorldGen, DEFAULT_SEED};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

/// 手持种子对 `(x,y,z)` 顶面右键（face=1）：等 BlockUpdate/Ack/Slot 三包齐 + BlockEdit 事件。
async fn plant_and_assert(
    s: &mut TcpStream,
    rx: &crossbeam_channel::Receiver<PlayEvent>,
    seq: i32,
    (x, y, z): (i32, i32, i32),
    base: u16,
    seed_item: i32,
    left: i32,
) {
    let mut body = Vec::new();
    packets::UseItemOn {
        hand: 0,
        x,
        y,
        z,
        face: 1,
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside_block: false,
        world_border_hit: false,
        sequence: seq,
    }
    .encode_body(&mut body);
    mc_network::framing::write_packet(s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut saw_update, mut saw_ack, mut saw_slot) = (false, false, false);
    while Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == (x, y + 1, z) {
                    assert_eq!(bu.block_id, base as i32, "planted crop must be age 0");
                    saw_update = true;
                }
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == seq {
                    saw_ack = true;
                }
            }
            packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => {
                let (slot, _) = packets::SetContainerSlot::decode_body(&payload).unwrap();
                assert_eq!(slot.data, packets::Slot { count: left, item_id: seed_item });
                saw_slot = true;
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {}
            other => panic!("unexpected packet {other:#04x} while planting seq={seq}"),
        }
        if saw_update && saw_ack && saw_slot {
            break;
        }
    }
    assert!(saw_update && saw_ack && saw_slot, "plant seq={seq} must land, ack and consume 1 seed");
    match next_event(rx).await {
        PlayEvent::BlockEdit { old_id: 0, new_id, .. } => assert_eq!(new_id, base),
        other => panic!("expected plant block edit, got {other:?}"),
    }
}

/// 等真实 ticker 把 `(x,y+1,z)` 逐阶段推到 `stages` 末尾（不伪造时钟）。
async fn wait_growth(s: &mut TcpStream, (x, y, z): (i32, i32, i32), stages: &[u16]) {
    let mut i = 0;
    let deadline = Instant::now() + Duration::from_secs(20);
    while i < stages.len() {
        assert!(Instant::now() < deadline, "crop must reach maturity in time");
        let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == (x, y + 1, z) {
                    assert_eq!(bu.block_id, stages[i] as i32, "crop must grow one stage at a time");
                    i += 1;
                }
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {}
            other => panic!("unexpected packet {other:#04x} while growing"),
        }
    }
}

/// 挖掉成熟作物（或连带挖土）→ 验 BlockUpdate/Ack/掉落实体 → 走近拾取入库。
/// `expected` 是拾取后背包中每种物品至少应有的数量（含种下前剩余）。
async fn dig_and_collect(
    s: &mut TcpStream,
    rx: &crossbeam_channel::Receiver<PlayEvent>,
    seq: i32,
    pos: (i32, i32, i32),
    want_drops: Vec<mc_world::Drop>,
    expected: HashMap<i32, i32>,
) {
    let (x, y, z) = pos;
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x, y, z, face: 1, sequence: seq }
        .encode_body(&mut body);
    mc_network::framing::write_packet(s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    assert_eq!(packets::BlockUpdate::decode_body(&payload).unwrap().0.block_id, 0);
    let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, seq);
    for w in &want_drops {
        let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID, "harvest drop must spawn");
        let (spawned, _) = packets::SpawnItemEntity::decode_body(&payload).unwrap();
        let (id, payload) = read_frame_timeout(s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SET_ENTITY_DATA_S2C_ID);
        let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
        assert_eq!(data.entity_id, spawned.entity_id);
        assert_eq!(data.slot, packets::Slot { count: w.count, item_id: w.item_id });
    }
    match next_event(rx).await {
        PlayEvent::BlockEdit { new_id: 0, .. } => {}
        other => panic!("expected dig block edit, got {other:?}"),
    }
    // 拾取：站立半径内，delay 到期后 TakeItem/Remove + 全量背包同步。
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut takes, mut saw_content) = (0, false);
    while Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(s, Duration::from_millis(2000)).await.unwrap();
        match id {
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {}
            packets::PLAY_PICKUP_ITEM_S2C_ID => {
                let (t, _) = packets::TakeItemEntity::decode_body(&payload).unwrap();
                assert_eq!(t.collector_id, 1);
                takes += 1;
            }
            packets::PLAY_REMOVE_ENTITIES_S2C_ID => {}
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                saw_content = expected.iter().all(|(item, need)| {
                    c.slots.iter().filter(|sl| sl.item_id == *item).map(|sl| sl.count).sum::<i32>()
                        >= *need
                });
            }
            other => panic!("unexpected packet {other:#04x} while picking up"),
        }
        if takes >= want_drops.len() && saw_content {
            break;
        }
    }
    assert!(takes >= want_drops.len() && saw_content, "harvest must be picked up into the backpack");
}

/// 公共 Play 夹具：出生点草顶面 + 视距 2 + hub/cache/真实 ticker。
struct Fixture {
    s: TcpStream,
    rx: crossbeam_channel::Receiver<PlayEvent>,
    dir: std::path::PathBuf,
    conn_id: u64,
    pos: (i32, i32, i32), // 地面草方块
    drops: Arc<DropState>,
    world: Arc<MutableWorld>,
}

impl Fixture {
    async fn new(tag: &str, stage_ticks: u64) -> Self {
        let dir = std::env::temp_dir().join(format!("mc-p33-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
        let mutable = Arc::new(MutableWorld::new(DEFAULT_SEED));
        let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
        let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
        drops.set_stage_ticks(stage_ticks);
        let hub = Arc::new(BroadcastHub::new());
        let config = NetConfig::new(100, tag.into(), Arc::new(AtomicUsize::new(0)))
            .with_play_events(tx)
            .with_view_distance(2)
            .with_chunk_cache(cache)
            .with_mutable_world(Arc::clone(&mutable))
            .with_drops(Arc::clone(&drops))
            .with_hub(hub);
        let addr = spawn_server(config).await;
        let mut s = login_and_join(addr, tag, 2).await;
        let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
        let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
        send_pos_y(&mut s, spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5).await;
        let conn_id = match next_event(&rx).await {
            PlayEvent::Movement { conn_id, .. } => conn_id,
            other => panic!("expected movement event, got {other:?}"),
        };
        Self { s, rx, dir, conn_id, pos: (spawn.0, spawn.1 - 1, spawn.2), drops, world: mutable }
    }

    fn give(&self, item: i32, count: i32) {
        let leftover = self.drops.with_inventory(self.conn_id, |inv| inv.add_item(item, count));
        assert_eq!(leftover, Some(0), "item {item} must fit the backpack");
    }

    async fn cleanup(self) {
        drop(self.s);
        match next_event(&self.rx).await {
            PlayEvent::Disconnect { .. } => {}
            other => panic!("expected disconnect event, got {other:?}"),
        }
        drop(self.rx);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// P33 e2e：种胡萝卜 → BlockUpdate 12402 → 真实 ticker 逐阶段长到 12409 →
/// 挖成熟株掉胡萝卜 → 拾取入库。
#[tokio::test]
async fn p33_plant_carrot_grows_and_harvest_drops_carrot() {
    assert_eq!(CARROT.base_id, 12402);
    assert_eq!(ITEM_CARROT, 1378);
    let mut f = Fixture::new("carrot", 10).await;
    let (gx, gy, gz) = f.pos;
    assert_eq!(f.world.get(gx, gy, gz), 9, "spawn feet must be grass");
    f.give(ITEM_CARROT, 3);
    // 选中 hotbar 槽 1（carrot 在 37）。
    mc_network::framing::write_packet(&mut f.s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &[0u8, 1u8])
        .await
        .unwrap();
    plant_and_assert(&mut f.s, &f.rx, 91, f.pos, CARROT.base_id, ITEM_CARROT, 2).await;
    assert_eq!(f.drops.crop_count(), 1);
    let stages: Vec<u16> = (1..=7).map(|age| crop_stage_id(&CARROT, age)).collect();
    wait_growth(&mut f.s, f.pos, &stages).await;
    assert_eq!(f.world.get(gx, gy + 1, gz), crop_stage_id(&CARROT, 7));
    assert_eq!(f.drops.crop_stage(gx, gy + 1, gz), Some(7));
    let want = mc_world::drops_for_block(crop_stage_id(&CARROT, 7), gx, gy + 1, gz, DEFAULT_SEED);
    assert_eq!(want[0].item_id, ITEM_CARROT);
    let mut expected = HashMap::new();
    expected.insert(ITEM_CARROT, 2 + want.iter().filter(|d| d.item_id == ITEM_CARROT).map(|d| d.count).sum::<i32>());
    dig_and_collect(&mut f.s, &f.rx, 92, (gx, gy + 1, gz), want, expected).await;
    assert_eq!(f.drops.crop_count(), 0);
    f.cleanup().await;
}

/// P33 e2e：耕地上种甜菜（beetroot_seeds）→ 3 阶段成熟 → 掉 beetroot + seeds 可再种。
#[tokio::test]
async fn p33_beetroot_on_farmland_grows_and_drops_seeds_to_replant() {
    assert_eq!(BEETROOT.base_id, 16792);
    assert_eq!(ITEM_BEETROOT_SEEDS, 1439);
    let mut f = Fixture::new("beetroot", 10).await;
    let (gx, gy, gz) = f.pos;
    // 直接置耕地（耕地加速的权威判定由 drop_state 单测覆盖；这里走真实成熟度链路）。
    f.world.set(gx, gy, gz, mc_world::FARMLAND_BLOCK_ID);
    f.give(ITEM_BEETROOT_SEEDS, 3);
    mc_network::framing::write_packet(&mut f.s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &[0u8, 1u8])
        .await
        .unwrap();
    plant_and_assert(&mut f.s, &f.rx, 93, f.pos, BEETROOT.base_id, ITEM_BEETROOT_SEEDS, 2).await;
    assert_eq!(f.drops.crop_count(), 1);
    let stages: Vec<u16> = (1..=3).map(|age| crop_stage_id(&BEETROOT, age)).collect();
    wait_growth(&mut f.s, f.pos, &stages).await;
    assert_eq!(f.world.get(gx, gy + 1, gz), crop_stage_id(&BEETROOT, 3));
    let want = mc_world::drops_for_block(crop_stage_id(&BEETROOT, 3), gx, gy + 1, gz, DEFAULT_SEED);
    assert_eq!(want[0], mc_world::Drop { item_id: ITEM_BEETROOT, count: 1 });
    assert_eq!(want[1].item_id, ITEM_BEETROOT_SEEDS);
    let mut expected = HashMap::new();
    expected.insert(ITEM_BEETROOT, 1);
    expected.insert(ITEM_BEETROOT_SEEDS, 2 + want[1].count);
    dig_and_collect(&mut f.s, &f.rx, 94, (gx, gy + 1, gz), want, expected).await;
    assert_eq!(f.drops.crop_count(), 0);
    f.cleanup().await;
}

/// P33 e2e：挖土连带 pop 新作物——种马铃薯后挖掉脚下的草，土壤和作物一起
/// 变空气并各自产掉落，跟踪摘除，作物不会悬空。
#[tokio::test]
async fn p33_digging_soil_pops_potato_and_drops_it() {
    assert_eq!(POTATO.base_id, 12410);
    assert_eq!(ITEM_POTATO, 1379);
    let mut f = Fixture::new("potato-soil", 10).await;
    let (gx, gy, gz) = f.pos;
    f.give(ITEM_POTATO, 3);
    mc_network::framing::write_packet(&mut f.s, packets::PLAY_SET_HELD_ITEM_C2S_ID, &[0u8, 1u8])
        .await
        .unwrap();
    plant_and_assert(&mut f.s, &f.rx, 95, f.pos, POTATO.base_id, ITEM_POTATO, 2).await;
    assert_eq!(f.drops.crop_count(), 1);
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x: gx, y: gy, z: gz, face: 1, sequence: 96 }
        .encode_body(&mut body);
    mc_network::framing::write_packet(&mut f.s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
        .await
        .unwrap();
    // 两个 BlockUpdate：被挖的土 + 连带 pop 的马铃薯（顺序由 outcome.changes 保证）。
    let mut popped = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while popped.len() < 2 {
        assert!(Instant::now() < deadline, "soil dig must pop the crop above");
        let (id, payload) = read_frame_timeout(&mut f.s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                popped.push(((bu.x, bu.y, bu.z), bu.block_id));
            }
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {}
            other => panic!("unexpected packet {other:#04x} while digging soil"),
        }
    }
    assert_eq!(popped[0], ((gx, gy, gz), 0));
    assert_eq!(popped[1], ((gx, gy + 1, gz), 0));
    let (id, payload) = read_frame_timeout(&mut f.s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
    assert_eq!(read_varint(&payload).unwrap().0, 96);
    let mut want = mc_world::drops_for_block(mc_world::GRASS_BLOCK_ID, gx, gy, gz, DEFAULT_SEED);
    want.extend(mc_world::drops_for_block(POTATO.base_id, gx, gy + 1, gz, DEFAULT_SEED));
    for w in &want {
        let (id, payload) = read_frame_timeout(&mut f.s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SPAWN_ENTITY_S2C_ID);
        let (spawned, _) = packets::SpawnItemEntity::decode_body(&payload).unwrap();
        let (id, payload) = read_frame_timeout(&mut f.s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::PLAY_SET_ENTITY_DATA_S2C_ID);
        let (data, _) = packets::ItemEntityData::decode_body(&payload).unwrap();
        assert_eq!(data.entity_id, spawned.entity_id);
        assert_eq!(data.slot, packets::Slot { count: w.count, item_id: w.item_id });
    }
    // 两次 BlockEdit（土 + 作物）。
    for _ in 0..2 {
        match next_event(&f.rx).await {
            PlayEvent::BlockEdit { new_id: 0, .. } => {}
            other => panic!("expected soil/crop block edit, got {other:?}"),
        }
    }
    assert_eq!(f.world.get(gx, gy + 1, gz), 0, "crop must not be left floating");
    assert_eq!(f.drops.crop_count(), 0);
    let potato_count: i32 = want.iter().filter(|d| d.item_id == ITEM_POTATO).map(|d| d.count).sum();
    let mut expected = HashMap::new();
    expected.insert(ITEM_POTATO, 2 + potato_count);
    expected.insert(ITEM_DIRT, 1);
    // 复用拾取段：这里 dig 包已发过，只验背包入库。
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut takes, mut saw_content) = (0, false);
    while Instant::now() < deadline {
        let (id, payload) = read_frame_timeout(&mut f.s, Duration::from_millis(2000)).await.unwrap();
        match id {
            packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {}
            packets::PLAY_PICKUP_ITEM_S2C_ID => takes += 1,
            packets::PLAY_REMOVE_ENTITIES_S2C_ID => {}
            packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID => {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                saw_content = expected.iter().all(|(item, need)| {
                    c.slots.iter().filter(|sl| sl.item_id == *item).map(|sl| sl.count).sum::<i32>()
                        >= *need
                });
            }
            other => panic!("unexpected packet {other:#04x} while picking up"),
        }
        if takes >= want.len() && saw_content {
            break;
        }
    }
    assert!(takes >= want.len() && saw_content, "soil+crop drops must be picked up");
    f.cleanup().await;
}
