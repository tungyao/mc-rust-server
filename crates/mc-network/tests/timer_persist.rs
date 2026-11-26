//! P39：成长计时跨重启续算的服务端状态级回归。
//!
//! 用真实的 redb 库 + 真实的 50ms 全服 ticker（阶段间隔经测试钩子调小），
//! 关库重开后验证 planted_tick/stage 从原进度继续，而不是从 0 重计时。
//! 不碰协议包、作物 ID/阶段/掉落逻辑；legacy 模式另有一项不变性回归。
use mc_network::DropState;
use mc_world::crops::{PlantKind, WHEAT};
use mc_world::store::{TimerKind, WorldStore};
use mc_world::{crop_stage_id, MutableWorld, WorldGen, OAK_SAPLING_BLOCK_ID};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "mc-p39-timer-{}-{tag}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cleanup(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
}

fn wait_until<F: Fn() -> bool>(timeout: Duration, f: F) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

#[test]
fn crop_resumes_after_close_and_reopen() {
    let dir = tmp("crop");
    let seed = 4242;
    let spawn = WorldGen::new(seed).spawn_pos();
    let (x, y, z) = (spawn.0, spawn.1, spawn.2);
    let saved;
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = Arc::new(MutableWorld::with_store(seed, Arc::clone(&store)).unwrap());
        world.set(x, y, z, WHEAT.base_id);
        let ds = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
        ds.set_stage_ticks(2);
        ds.plant_crop_spec(&WHEAT, x, y, z);
        ds.adopt_existing_crops(&world);
        let timer = store.timer(TimerKind::Crop, (x, y, z)).unwrap().unwrap();
        assert_eq!(timer.planted_tick, 0);
        assert_eq!(timer.stage, 0);
        assert!(ds.start_global_tick(Arc::clone(&world)));
        assert!(wait_until(Duration::from_secs(5), || ds.crop_stage(x, y, z).unwrap_or(0) >= 2));
        ds.stop_global_tick();
        std::thread::sleep(Duration::from_millis(800));
        ds.save_all_inventories();
        world.flush_to_store(0).unwrap();
        saved = store.timer(TimerKind::Crop, (x, y, z)).unwrap().unwrap();
        assert_eq!(saved.stage, 2, "stage must be persisted, not restarted");
        assert!(saved.planted_tick < store.tick_offset().unwrap());
        drop(ds);
        drop(world);
        drop(store);
    }
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = Arc::new(MutableWorld::with_store(seed, Arc::clone(&store)).unwrap());
        let ds = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
        ds.set_stage_ticks(2);
        assert_eq!(ds.tick_count(), store.tick_offset().unwrap(), "tick baseline restored");
        assert_eq!(world.get(x, y, z), crop_stage_id(&WHEAT, saved.stage));
        ds.adopt_existing_crops(&world);
        assert_eq!(ds.crop_stage(x, y, z), Some(saved.stage), "original stage resumed");
        assert!(ds.start_global_tick(Arc::clone(&world)));
        assert!(wait_until(Duration::from_secs(5), || ds.crop_stage(x, y, z).unwrap_or(0) > saved.stage));
        ds.stop_global_tick();
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(
            world.get(x, y, z),
            crop_stage_id(&WHEAT, ds.crop_stage(x, y, z).unwrap()),
            "growth continues from persisted progress after restart"
        );
    }
    cleanup(&dir);
}

#[test]
fn sapling_grows_after_reopen_from_original_planted_tick() {
    let dir = tmp("sapling");
    let seed = 5150;
    let spawn = WorldGen::new(seed).spawn_pos();
    let (x, y, z) = (spawn.0, spawn.1, spawn.2);
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = Arc::new(MutableWorld::with_store(seed, Arc::clone(&store)).unwrap());
        world.set(x, y, z, OAK_SAPLING_BLOCK_ID);
        let ds = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
        ds.set_sapling_ticks(6);
        ds.plant_sapling(x, y, z);
        ds.adopt_existing_saplings(&world);
        assert_eq!(store.timer(TimerKind::Sapling, (x, y, z)).unwrap().unwrap().planted_tick, 0);
        assert!(ds.start_global_tick(Arc::clone(&world)));
        // 走掉一半延时后停机：重开后用剩余 tick 到期，而不是再等满 6 拍。
        assert!(wait_until(Duration::from_secs(5), || ds.tick_count() >= 3));
        ds.stop_global_tick();
        std::thread::sleep(Duration::from_millis(800));
        ds.save_all_inventories();
        world.flush_to_store(0).unwrap();
        let elapsed = store.tick_offset().unwrap();
        assert!(elapsed >= 3 && elapsed < 6, "stopped mid-growth: {elapsed}");
        drop(ds);
        drop(world);
        drop(store);
    }
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = Arc::new(MutableWorld::with_store(seed, Arc::clone(&store)).unwrap());
        let ds = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
        ds.set_sapling_ticks(6);
        assert_eq!(ds.tick_count(), store.tick_offset().unwrap());
        ds.adopt_existing_saplings(&world);
        assert_eq!(ds.sapling_count(), 1);
        assert!(ds.start_global_tick(Arc::clone(&world)));
        assert!(wait_until(Duration::from_secs(5), || world.get(x, y, z) != OAK_SAPLING_BLOCK_ID));
        ds.stop_global_tick();
        std::thread::sleep(Duration::from_millis(1000));
        assert_eq!(ds.sapling_count(), 0, "completed sapling must be uncoupled");
        assert!(wait_until(Duration::from_secs(5), || store
            .timer(TimerKind::Sapling, (x, y, z))
            .unwrap()
            .is_none()), "timer deleted on growth");
    }
    cleanup(&dir);
}

#[test]
fn leaf_decay_resumes_after_reopen() {
    let dir = tmp("leaf");
    let seed = 8899;
    let (x, y, z) = (0, 100, 0); // 高空：半径内无原木，到期必 decay
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = Arc::new(MutableWorld::with_store(seed, Arc::clone(&store)).unwrap());
        world.set(x, y, z, mc_world::OAK_LEAVES_BLOCK_ID);
        let ds = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
        ds.set_leaf_decay_ticks(6);
        ds.schedule_leaf_decay_around(&world, x, y, z);
        assert!(store.timer(TimerKind::Leaf, (x, y, z)).unwrap().is_some());
        assert_eq!(ds.leaf_pending_count(), 1);
        assert!(ds.start_global_tick(Arc::clone(&world)));
        assert!(wait_until(Duration::from_secs(5), || ds.tick_count() >= 3));
        ds.stop_global_tick();
        std::thread::sleep(Duration::from_millis(800));
        ds.save_all_inventories();
        world.flush_to_store(0).unwrap();
        let elapsed = store.tick_offset().unwrap();
        assert!(elapsed >= 3 && elapsed < 6, "stopped mid-decay: {elapsed}");
        assert_eq!(world.get(x, y, z), mc_world::OAK_LEAVES_BLOCK_ID);
        drop(ds);
        drop(world);
        drop(store);
    }
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = Arc::new(MutableWorld::with_store(seed, Arc::clone(&store)).unwrap());
        let ds = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
        ds.set_leaf_decay_ticks(6);
        assert_eq!(ds.leaf_pending_count(), 1, "leaf observation restored from timers");
        assert!(ds.start_global_tick(Arc::clone(&world)));
        assert!(wait_until(Duration::from_secs(5), || world.get(x, y, z) == mc_world::AIR_BLOCK_ID));
        ds.stop_global_tick();
        std::thread::sleep(Duration::from_millis(800));
        assert!(store.timer(TimerKind::Leaf, (x, y, z)).unwrap().is_none());
    }
    cleanup(&dir);
}

#[test]
fn bonemeal_syncs_timer_and_uproot_deletes_it() {
    let dir = tmp("mutations");
    let seed = 31;
    let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
    let world = MutableWorld::with_store(seed, Arc::clone(&store)).unwrap();
    let ds = DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store)));

    // P33 作物：骨粉推进同步 stage，挖掉删 timer。
    ds.plant_crop_spec(&WHEAT, 1, 71, 1);
    let before = store.timer(TimerKind::Crop, (1, 71, 1)).unwrap().unwrap();
    assert_eq!(before.stage, 0);
    ds.apply_bonemeal_crop_spec(&WHEAT, 1, 71, 1, 3);
    let after = store.timer(TimerKind::Crop, (1, 71, 1)).unwrap().unwrap();
    assert_eq!(after.stage, 3);
    assert_eq!(after.planted_tick, before.planted_tick, "bonemeal must not rebase planted");
    ds.uproot_crop(1, 71, 1);
    assert!(store.timer(TimerKind::Crop, (1, 71, 1)).unwrap().is_none());

    // P37 植物（Age + Fruit）：同一套 timer 生命周期。
    use mc_world::crops::MELON_STEM;
    ds.plant_kind(PlantKind::Age(&MELON_STEM), 2, 71, 2, 0);
    assert!(store.timer(TimerKind::Crop, (2, 71, 2)).unwrap().is_some());
    ds.apply_bonemeal_plant(&world, 2, 71, 2, mc_world::crops::age_stage_id(&MELON_STEM, 5));
    assert_eq!(store.timer(TimerKind::Crop, (2, 71, 2)).unwrap().unwrap().stage, 5);
    ds.uproot_plant(2, 71, 2);
    assert!(store.timer(TimerKind::Crop, (2, 71, 2)).unwrap().is_none());

    // 树苗：种下写 timer，挖掉即删。
    ds.plant_sapling(3, 71, 3);
    assert!(store.timer(TimerKind::Sapling, (3, 71, 3)).unwrap().is_some());
    ds.uproot_sapling(3, 71, 3);
    assert!(store.timer(TimerKind::Sapling, (3, 71, 3)).unwrap().is_none());

    let clears = store
        .replay()
        .unwrap()
        .into_iter()
        .filter(|r| r.op == mc_world::store::Op::TimerClear)
        .count();
    assert_eq!(clears, 3, "one TimerClear per deleted timer");
    cleanup(&dir);
}

#[test]
fn legacy_mode_keeps_old_retiming_and_writes_no_redb() {
    let dir = tmp("legacy");
    let seed = 77;
    let ds = DropState::new(dir.clone(), seed);
    let world = MutableWorld::new(seed);
    ds.set_stage_ticks(2);
    ds.plant_crop_spec(&WHEAT, 4, 71, 4);
    assert_eq!(ds.crop_stage(4, 71, 4), Some(0));
    assert_eq!(ds.tick_count(), 0);
    world.set(4, 71, 4, WHEAT.base_id);
    ds.adopt_existing_crops(&world);
    assert_eq!(ds.crop_stage(4, 71, 4), Some(0), "legacy adoption keeps saved age");
    assert!(!WorldStore::store_path(&dir, seed).exists(), "legacy path never opens redb");
    cleanup(&dir);
}


#[test]
fn p37_plants_resume_after_reopen() {
    use mc_world::crops::{
        age_stage_id, cocoa_state, BIRCH, COCOA, MELON_STEM, SUGAR_CANE,
    };
    let dir = tmp("p37");
    let seed = 2024;
    let spawn = WorldGen::new(seed).spawn_pos();
    let (sx, sy, sz) = (spawn.0, spawn.1, spawn.2);
    let age_pos = (sx, sy, sz);
    let tree_pos = (sx + 20, sy, sz);
    let vert_pos = (sx + 40, sy, sz);
    let side_pos = (sx + 60, sy, sz);
    let support_pos = (sx + 60, sy, sz + 1);
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = Arc::new(MutableWorld::with_store(seed, Arc::clone(&store)).unwrap());
        world.set(age_pos.0, age_pos.1, age_pos.2, age_stage_id(&MELON_STEM, 0));
        world.set(tree_pos.0, tree_pos.1, tree_pos.2, BIRCH.sapling_id);
        world.set(vert_pos.0, vert_pos.1, vert_pos.2, SUGAR_CANE.base_id);
        world.set(vert_pos.0, vert_pos.1 + 1, vert_pos.2, mc_world::AIR_BLOCK_ID);
        world.set(support_pos.0, support_pos.1, support_pos.2, COCOA.support_id);
        world.set(side_pos.0, side_pos.1, side_pos.2, cocoa_state(0, 0));
        let ds = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
        ds.set_plant_ticks(4);
        ds.plant_kind(PlantKind::Age(&MELON_STEM), age_pos.0, age_pos.1, age_pos.2, 0);
        ds.plant_kind(PlantKind::Tree(&BIRCH), tree_pos.0, tree_pos.1, tree_pos.2, 0);
        ds.plant_kind(PlantKind::Vertical(&SUGAR_CANE), vert_pos.0, vert_pos.1, vert_pos.2, 0);
        ds.plant_kind(PlantKind::Side(&COCOA), side_pos.0, side_pos.1, side_pos.2, 0);
        ds.adopt_existing_plants(&world);
        assert_eq!(store.timers().unwrap().len(), 4);
        assert!(ds.start_global_tick(Arc::clone(&world)));
        assert!(wait_until(Duration::from_secs(5), || ds.tick_count() >= 2));
        ds.stop_global_tick();
        std::thread::sleep(Duration::from_millis(800));
        ds.save_all_inventories();
        world.flush_to_store(0).unwrap();
        assert_eq!(store.timers().unwrap().len(), 4, "half-grown timers persisted");
        drop(ds);
        drop(world);
        drop(store);
    }
    {
        let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = Arc::new(MutableWorld::with_store(seed, Arc::clone(&store)).unwrap());
        let ds = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
        ds.set_plant_ticks(4);
        assert_eq!(ds.tick_count(), store.tick_offset().unwrap());
        ds.adopt_existing_plants(&world);
        assert_eq!(ds.plant_count(), 4, "all P37 models restored from timers");
        assert!(ds.start_global_tick(Arc::clone(&world)));
        assert!(wait_until(Duration::from_secs(10), || ds.tick_count() >= 7));
        ds.stop_global_tick();
        std::thread::sleep(Duration::from_millis(800));
        assert!(ds.plant_stage(age_pos.0, age_pos.1, age_pos.2).unwrap() >= 1);
        assert!(ds.plant_stage(vert_pos.0, vert_pos.1, vert_pos.2).unwrap() >= 1);
        assert!(ds.plant_stage(side_pos.0, side_pos.1, side_pos.2).unwrap() >= 1);
        assert_ne!(world.get(tree_pos.0, tree_pos.1, tree_pos.2), BIRCH.sapling_id, "tree grew");
        assert!(
            store.timer(TimerKind::Sapling, tree_pos).unwrap().is_none(),
            "completed tree timer deleted"
        );
        // Fruit 标志在 spec 高位：成熟瓜梗结瓜后重开不会重复落瓜。
        let melon = store.timer(TimerKind::Crop, age_pos).unwrap().unwrap();
        assert_eq!(melon.planted_tick, 0, "age model keeps original planted tick");
    }
    cleanup(&dir);
}
