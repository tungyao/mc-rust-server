//! P46：流动水存档重开/广播 + 游泳位置合法性经既有玩家互见广播的服务端状态级回归。
//! 真机手感（26.3 客户端水位渲染/游泳动画/接缝）本环境无法验证，见 HANDOFF P3。

use mc_network::{BroadcastHub, ChunkCache, FluidState, PlayerEntityState};
use mc_protocol::packets;
use mc_world::fluid::{self as rules, swim_state};
use mc_world::store::WorldStore;
use mc_world::{GeneratorVersion, MutableWorld, AIR_BLOCK_ID, STONE_BLOCK_ID};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("mc-p46-fluid-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn fluid_state_survives_redb_close_and_reopen() {
    let dir = tmp("save");
    let seed = 4242;
    // y=200 远高于任何 seed 的地表，基底必为空气。
    let (source, dug) = ((20, 200, 20), (21, 200, 20));
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        store.set_generator_version(2).unwrap();
        let world = Arc::new(
            MutableWorld::with_store_and_version(seed, Arc::clone(&store), GeneratorVersion::V2)
                .unwrap(),
        );
        assert_eq!(world.get(dug.0, dug.1, dug.2), AIR_BLOCK_ID);
        world.set(source.0, source.1, source.2, rules::water_source_id());
        let fluid = Arc::new(FluidState::new());
        fluid.on_block_change(source.0, source.1, source.2, AIR_BLOCK_ID, rules::water_source_id());
        assert!(fluid.tick(&world, None, None) >= 1);
        assert_eq!(world.get(dug.0, dug.1, dug.2), rules::water_state_id(1));
        assert!(world.flush_to_store(0).unwrap() >= 2, "source + flowed cells journaled");
        drop(fluid);
        drop(world);
        drop(store);
    }
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = MutableWorld::with_store_and_version(seed, Arc::clone(&store), GeneratorVersion::V2)
            .unwrap();
        assert_eq!(world.get(source.0, source.1, source.2), rules::water_source_id());
        assert_eq!(
            world.get(dug.0, dug.1, dug.2),
            rules::water_state_id(1),
            "flowing water must survive close/reopen"
        );
        drop(world);
        drop(store);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fluid_tick_broadcasts_block_update_in_view_and_invalidates_cache() {
    let seed = 777;
    let world = Arc::new(MutableWorld::new_with_version(seed, GeneratorVersion::V2));
    let cache = Arc::new(
        ChunkCache::new_with_version(seed, 8, GeneratorVersion::V2)
            .with_mutable(Arc::clone(&world)),
    );
    let hub = Arc::new(BroadcastHub::new());
    let (near_tx, near_rx) = crossbeam_channel::unbounded();
    let (far_tx, far_rx) = crossbeam_channel::unbounded();
    // 流体没有操作者：except=0，视距内连接（含 Viewer=1）都收到，远处连接收不到。
    hub.register(1, near_tx, 2, (20.5, 200.0, 20.5));
    hub.register(2, far_tx, 2, (600.5, 200.0, 600.5));

    let (x, y, z) = (20, 200, 20);
    let chunk = mc_world::chunk_of_block(x + 1, z);
    let _ = cache.body(chunk);
    assert!(cache.contains(chunk));

    world.set(x, y, z, rules::water_source_id());
    let fluid = FluidState::new();
    fluid.on_block_change(x, y, z, AIR_BLOCK_ID, rules::water_source_id());
    assert!(fluid.tick(&world, Some(&cache), Some(&hub)) >= 1);

    assert!(!cache.contains(chunk), "fluid write must invalidate the chunk cache");
    let pkt = near_rx.try_recv().expect("viewer in range must receive BlockUpdate");
    assert_eq!(pkt.id, packets::PLAY_BLOCK_UPDATE_S2C_ID);
    let (update, _) = packets::BlockUpdate::decode_body(&pkt.body).unwrap();
    assert_eq!(update.x, x + 1);
    assert_eq!(update.block_id, rules::water_state_id(1) as i32);
    assert!(far_rx.try_recv().is_err(), "far connection must not receive the update");
}

#[test]
fn swim_position_legality_and_movement_reach_viewer() {
    // 水在 y=70..72，石头地表 y=69；脚 y=70.2 合法，y=68.5 需钳到 70。
    let world = MutableWorld::new_with_version(42, GeneratorVersion::V2);
    let get = |x, y, z| world.get(x, y, z);
    for y in 70..=72 {
        world.set(4, y, 9, rules::water_source_id());
    }
    world.set(4, 68, 9, STONE_BLOCK_ID);
    world.set(4, 69, 9, STONE_BLOCK_ID);
    let swim = swim_state(get, 4.5, 70.2, 9.5);
    assert!(swim.in_water);
    assert_eq!(swim.min_feet_y, 70.0);
    let clamped = swim_state(get, 4.5, 69.5, 9.5);
    assert!(clamped.in_water);
    assert_eq!(clamped.min_feet_y, 70.0, "server clamps feet above support block");

    // 水中位置变化经既有玩家实体互见广播给视距内连接（不新增包、不改协议）。
    let hub = Arc::new(BroadcastHub::new());
    let (viewer_tx, viewer_rx) = crossbeam_channel::unbounded();
    hub.register(1, viewer_tx, 2, (4.5, 70.0, 9.5));
    hub.register(2, crossbeam_channel::unbounded().0, 2, (8.5, 70.0, 9.5));
    hub.enable_player(
        2,
        PlayerEntityState {
            entity_id: 1_000_000_100,
            uuid: 2,
            pos: (4.5, 70.2, 9.5),
            yaw: 0.0,
            pitch: 0.0,
        },
    );
    assert!(hub.mark_player_known(1, 1_000_000_100));
    mc_network::player_visibility::broadcast_movement(
        &hub,
        2,
        packets::C2SMovement::Pos(packets::MovePlayerPos {
            x: 4.5,
            y: 71.0,
            z: 9.5,
            on_ground: false,
            horizontal_collision: false,
        }),
        false,
    );
    let pkt = viewer_rx.try_recv().expect("swimming y change must reach viewer");
    assert_eq!(pkt.id, packets::PLAY_UPDATE_ENTITY_POS_S2C_ID);
    let mv = packets::UpdateEntityPos::decode_body(&pkt.body).unwrap().0;
    assert_eq!(mv.entity_id, 1_000_000_100);
    assert!(mv.dy > 0, "upward swimming movement must be visible to viewers");
}
