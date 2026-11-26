//! P37 全作物端到端（只新增，不改旧测试）：真实 Login/Join + 手持右键种植 +
//! 全服唯一 50ms ticker 生长 + 广播 + 挖掘掉落/拾取。每类生长模型一个代表：
//! 灵魂沙地狱疣、其它树苗（白桦）、竖向甘蔗、甜浆果/可可、西瓜梗结瓜。
mod common;

use common::*;
use mc_network::{BroadcastHub, ChunkCache, DropState, NetConfig, PlayEvent};
use mc_protocol::{packets, read_varint};
use mc_world::crops::{
    age_stage_id, cocoa_state, BIRCH, COCOA, MELON_STEM, NETHER_WART, SUGAR_CANE,
    SWEET_BERRY_BUSH,
};
use mc_world::{Drop, MutableWorld, WorldGen, DEFAULT_SEED};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

struct Fixture {
    s: TcpStream,
    rx: crossbeam_channel::Receiver<PlayEvent>,
    dir: std::path::PathBuf,
    conn_id: u64,
    ground: (i32, i32, i32), // 草/沙土方块
    drops: Arc<DropState>,
    world: Arc<MutableWorld>,
}

impl Fixture {
    async fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mc-p37-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
        let world = Arc::new(MutableWorld::new(DEFAULT_SEED));
        let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&world)));
        let drops = Arc::new(DropState::new(dir.clone(), DEFAULT_SEED));
        drops.set_stage_ticks(4);
        drops.set_plant_ticks(4);
        let hub = Arc::new(BroadcastHub::new());
        let config = NetConfig::new(100, tag.into(), Arc::new(AtomicUsize::new(0)))
            .with_play_events(tx)
            .with_view_distance(2)
            .with_chunk_cache(cache)
            .with_mutable_world(Arc::clone(&world))
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
            other => panic!("expected movement, got {other:?}"),
        };
        Self { s, rx, dir, conn_id, ground: (spawn.0, spawn.1 - 1, spawn.2), drops, world }
    }

    fn give(&self, item: i32, count: i32) {
        assert_eq!(
            self.drops.with_inventory(self.conn_id, |inv| inv.add_item(item, count)),
            Some(0),
            "item {item} must fit"
        );
    }

    async fn select_slot(&mut self, slot: u8) {
        mc_network::framing::write_packet(
            &mut self.s,
            packets::PLAY_SET_HELD_ITEM_C2S_ID,
            &[0u8, slot],
        )
        .await
        .unwrap();
    }

    /// 发 UseItemOn 并等目标格出现期望 blockstate 的 BlockUpdate（含扣 1 的单槽同步）。
    async fn plant_and_wait(&mut self, seq: i32, face: i32, clicked: (i32, i32, i32), target: (i32, i32, i32), want: u16) {
        let mut body = Vec::new();
        packets::UseItemOn {
            hand: 0,
            x: clicked.0,
            y: clicked.1,
            z: clicked.2,
            face,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: seq,
        }
        .encode_body(&mut body);
        mc_network::framing::write_packet(&mut self.s, packets::PLAY_USE_ITEM_ON_C2S_ID, &body)
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut saw_update, mut saw_ack, mut saw_slot) = (false, false, false);
        while Instant::now() < deadline {
            let (id, payload) = read_frame_timeout(&mut self.s, Duration::from_secs(5)).await.unwrap();
            match id {
                packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                    let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                    if (bu.x, bu.y, bu.z) == target {
                        assert_eq!(bu.block_id, want as i32, "planted blockstate mismatch");
                        saw_update = true;
                    }
                }
                packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                    if read_varint(&payload).unwrap().0 == seq {
                        saw_ack = true;
                    }
                }
                packets::PLAY_SET_CONTAINER_SLOT_S2C_ID => saw_slot = true,
                packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {}
                other => panic!("unexpected packet {other:#04x} while planting seq={seq}"),
            }
            if saw_update && saw_ack && saw_slot {
                break;
            }
        }
        assert!(saw_update && saw_ack && saw_slot, "plant seq={seq} must land+ack+consume");
    }

    /// 轮询权威世界（真实 ticker 推进，不伪造时钟），返回是否达到目标 blockstate。
    async fn wait_for_block(&self, pos: (i32, i32, i32), want: u16) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if self.world.get(pos.0, pos.1, pos.2) == want {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "block {pos:?} must reach {want}, got {}",
            self.world.get(pos.0, pos.1, pos.2)
        );
    }

    /// 挖掘目标格并等 BlockUpdate(0)+Ack+掉落 Spawn/Data，最后确认进背包。
    async fn dig_and_collect(&mut self, seq: i32, pos: (i32, i32, i32), want: Vec<Drop>, expected: HashMap<i32, i32>) {
        let mut body = Vec::new();
        packets::PlayerAction { status: 3, x: pos.0, y: pos.1, z: pos.2, face: 1, sequence: seq }
            .encode_body(&mut body);
        mc_network::framing::write_packet(&mut self.s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body)
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut saw_zero, mut saw_ack) = (false, false);
        let mut spawned = Vec::new();
        while Instant::now() < deadline && (!saw_zero || !saw_ack || spawned.len() < want.len()) {
            let (id, payload) = read_frame_timeout(&mut self.s, Duration::from_secs(5)).await.unwrap();
            match id {
                packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                    let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                    if (bu.x, bu.y, bu.z) == pos && bu.block_id == 0 {
                        saw_zero = true;
                    }
                }
                packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                    if read_varint(&payload).unwrap().0 == seq {
                        saw_ack = true;
                    }
                }
                packets::PLAY_SPAWN_ENTITY_S2C_ID => {
                    let (sp, _) = packets::SpawnItemEntity::decode_body(&payload).unwrap();
                    spawned.push(sp.entity_id);
                }
                packets::PLAY_SET_ENTITY_DATA_S2C_ID => {}
                packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID | packets::PLAY_KEEPALIVE_S2C_ID => {}
                other => panic!("unexpected packet {other:#04x} while digging"),
            }
        }
        assert!(saw_zero && saw_ack && spawned.len() >= want.len(), "dig must land+ack+drop");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut saw_content = false;
        while Instant::now() < deadline {
            let (id, payload) = read_frame_timeout(&mut self.s, Duration::from_millis(2000)).await.unwrap();
            if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
                let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
                saw_content = expected.iter().all(|(item, need)| {
                    c.slots.iter().filter(|sl| sl.item_id == *item).map(|sl| sl.count).sum::<i32>() >= *need
                });
            }
            if saw_content {
                break;
            }
        }
        assert!(saw_content, "harvest drops must be picked up into the backpack");
    }

    async fn cleanup(self) {
        drop(self.s);
        loop {
            match next_event(&self.rx).await {
                PlayEvent::Disconnect { .. } => break,
                PlayEvent::BlockEdit { .. } => continue,
                other => panic!("expected disconnect, got {other:?}"),
            }
        }
        drop(self.rx);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 灵魂沙作物：手持 nether_wart 右键灵魂沙顶面 → 4 阶段 → 掉落可拾取。
#[tokio::test]
async fn p37_nether_wart_on_soul_sand_grows_and_harvests() {
    let mut f = Fixture::new("wart").await;
    let (x, y, z) = f.ground;
    f.world.set(x, y, z, mc_world::crops::SOUL_SAND_BLOCK_ID);
    f.give(NETHER_WART.seed_item, 2);
    f.select_slot(1).await;
    let target = (x, y + 1, z);
    f.plant_and_wait(101, 1, (x, y, z), target, NETHER_WART.base_id).await;
    assert_eq!(f.drops.plant_count(), 1);
    f.wait_for_block(target, age_stage_id(&NETHER_WART, 3)).await;
    assert_eq!(f.drops.plant_stage(target.0, target.1, target.2), Some(3));
    let want = mc_world::drops_for_block(age_stage_id(&NETHER_WART, 3), x, y + 1, z, DEFAULT_SEED);
    let mut expected = HashMap::new();
    expected.insert(NETHER_WART.seed_item, 1 + want.iter().map(|d| d.count).sum::<i32>());
    f.dig_and_collect(102, target, want, expected).await;
    assert_eq!(f.drops.plant_count(), 0);
    f.cleanup().await;
}

/// 其它树苗：手持 birch_sapling 种下 → 真实 ticker 长成白桦原木+白桦树叶。
#[tokio::test]
async fn p37_birch_sapling_grows_into_birch_tree() {
    let mut f = Fixture::new("birch").await;
    let (x, y, z) = f.ground;
    // 清出足够冠层空间（出生点附近有世界生成树/草，避免占位跳过叶片）。
    for dx in -3..=3 {
        for dz in -3..=3 {
            for dy in 1..=7 {
                f.world.set(x + dx, y + dy, z + dz, mc_world::AIR_BLOCK_ID);
            }
        }
    }
    f.world.set(x, y, z, mc_world::DIRT_BLOCK_ID);
    f.give(BIRCH.seed_item, 2);
    f.select_slot(1).await;
    let target = (x, y + 1, z);
    f.plant_and_wait(103, 1, (x, y, z), target, BIRCH.sapling_id).await;
    f.wait_for_block(target, BIRCH.log_id).await;
    // 冠层与树干在同一次生长里落子，但广播/读世界存在极短并发窗口，轮询到叶片出现。
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut leaves = 0;
    while Instant::now() < deadline {
        leaves = 0;
        for dy in 1..=5 {
            for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1)] {
                if f.world.get(x + dx, y + dy, z + dz) == BIRCH.leaves_id {
                    leaves += 1;
                }
            }
        }
        if leaves > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(leaves > 0, "tree must grow birch leaves in the crown");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && f.drops.plant_count() > 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(f.drops.plant_count(), 0, "grown tree must leave tracking");
    f.cleanup().await;
}

/// 竖向作物：手持 sugar_cane 种植 → 向上堆叠到 3 格；挖基部连根掉落。
#[tokio::test]
async fn p37_sugar_cane_stacks_and_dig_clears_whole_column() {
    let mut f = Fixture::new("cane").await;
    let (x, y, z) = f.ground;
    f.world.set(x, y, z, mc_world::SAND_BLOCK_ID);
    f.give(SUGAR_CANE.item_id, 2);
    f.select_slot(1).await;
    let target = (x, y + 1, z);
    f.plant_and_wait(104, 1, (x, y, z), target, SUGAR_CANE.base_id).await;
    f.wait_for_block((x, y + 3, z), SUGAR_CANE.base_id).await;
    assert_eq!(f.drops.plant_stage(target.0, target.1, target.2), Some(2));
    // 挖基部：上方两段连带变空气，共 3 份甘蔗掉落。
    let mut body = Vec::new();
    packets::PlayerAction { status: 3, x, y: y + 1, z, face: 1, sequence: 105 }.encode_body(&mut body);
    mc_network::framing::write_packet(&mut f.s, packets::PLAY_PLAYER_ACTION_C2S_ID, &body).await.unwrap();
    // 广播可能经 hub outbox 与直接回包交错：只要求基部清零的 BlockUpdate
    // 与 Ack 可见，上方连带以权威世界轮询为准（真实 ticker 不伪造时钟）。
    let deadline = Instant::now() + Duration::from_secs(5);
    let (mut saw_zero, mut saw_ack) = (false, false);
    while Instant::now() < deadline && !(saw_zero && saw_ack) {
        let (id, payload) = read_frame_timeout(&mut f.s, Duration::from_secs(5)).await.unwrap();
        match id {
            packets::PLAY_BLOCK_UPDATE_S2C_ID => {
                let (bu, _) = packets::BlockUpdate::decode_body(&payload).unwrap();
                if (bu.x, bu.y, bu.z) == target && bu.block_id == 0 {
                    saw_zero = true;
                }
            }
            packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID => {
                if read_varint(&payload).unwrap().0 == 105 {
                    saw_ack = true;
                }
            }
            _ => {}
        }
    }
    assert!(saw_zero && saw_ack, "dig must broadcast air and ack");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && [y + 1, y + 2, y + 3].iter().any(|cy| f.world.get(x, *cy, z) != mc_world::AIR_BLOCK_ID)
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    for cy in [y + 1, y + 2, y + 3] {
        assert_eq!(f.world.get(x, cy, z), mc_world::AIR_BLOCK_ID, "cane column must collapse");
    }
    assert_eq!(f.drops.plant_count(), 0);
    f.cleanup().await;
}

/// 灌木/可可代表：甜浆果丛长到成熟；可可对 jungle_log 侧面右键并按 age 成长。
#[tokio::test]
async fn p37_berry_bush_and_cocoa_side_planting() {
    let mut f = Fixture::new("bush-cocoa").await;
    let (x, y, z) = f.ground;
    f.give(SWEET_BERRY_BUSH.seed_item, 2);
    f.select_slot(1).await;
    let bush = (x, y + 1, z);
    f.plant_and_wait(106, 1, (x, y, z), bush, SWEET_BERRY_BUSH.base_id).await;
    f.wait_for_block(bush, age_stage_id(&SWEET_BERRY_BUSH, 3)).await;
    // 可可：jungle_log 在郊外高处（避开地形），点它的南面（face=3 → facing=1）。
    let (cx, cy, cz) = (x + 4, y + 4, z);
    f.world.set(cx, cy, cz, COCOA.support_id);
    let cocoa_pos = (cx, cy, cz + 1);
    f.world.set(cocoa_pos.0, cocoa_pos.1, cocoa_pos.2, mc_world::AIR_BLOCK_ID);
    f.give(COCOA.seed_item, 2);
    f.select_slot(2).await;
    f.plant_and_wait(107, 3, (cx, cy, cz), cocoa_pos, cocoa_state(0, 1)).await;
    f.wait_for_block(cocoa_pos, cocoa_state(2, 1)).await;
    assert_eq!(f.drops.plant_count(), 2);
    f.cleanup().await;
}

/// 瓜果类：西瓜梗在耕地成熟后，邻格真实落 minecraft:melon 方块并可按表掉落。
#[tokio::test]
async fn p37_melon_stem_places_real_melon_block() {
    let mut f = Fixture::new("melon").await;
    let (x, y, z) = f.ground;
    f.world.set(x, y, z, mc_world::FARMLAND_BLOCK_ID);
    f.give(MELON_STEM.seed_item, 2);
    f.select_slot(1).await;
    let stem = (x, y + 1, z);
    f.plant_and_wait(108, 1, (x, y, z), stem, MELON_STEM.base_id).await;
    f.wait_for_block(stem, age_stage_id(&MELON_STEM, 7)).await;
    let fruit = MELON_STEM.fruit.unwrap();
    let neighbors = [(x + 1, y + 1, z), (x, y + 1, z + 1), (x - 1, y + 1, z), (x, y + 1, z - 1)];
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline
        && !neighbors.iter().any(|p| f.world.get(p.0, p.1, p.2) == fruit.block_id)
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        neighbors.iter().any(|p| f.world.get(p.0, p.1, p.2) == fruit.block_id),
        "ripe stem must place a real melon in one of the 4 horizontal neighbors"
    );
    assert_eq!(fruit.item_id, mc_world::drops::ITEM_MELON_SLICE);
    f.cleanup().await;
}
