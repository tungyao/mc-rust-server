    use super::*;

    #[test]
    fn alloc_ids_are_unique_and_above_player() {
        let st = DropState::new(std::env::temp_dir(), 1);
        let a = st.alloc_id();
        let b = st.alloc_id();
        assert!(a >= FIRST_DROP_ENTITY_ID && b == a + 1 && a > PLAYER_ENTITY_ID);
    }

    #[test]
    fn inventory_load_unload_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mc-p18-drop-{}-rt", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let st = DropState::new(dir.clone(), 42);
        st.load_inventory(7, "some-uuid");
        let held = st.with_inventory(7, |inv| {
            inv.slots_hotbar_starter();
            inv.selected()
        });
        assert_eq!(held, Some(0));
        st.save_inventory(7, "some-uuid");
        assert!(PlayerInventory::save_path(&dir, "some-uuid", 42).exists());
        st.unload_inventory(7, "some-uuid");
        assert!(st.with_inventory(7, |_| ()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn known_tracking() {
        let st = DropState::new(std::env::temp_dir(), 1);
        st.load_inventory(9, "u");
        assert!(!st.is_known(9, 1001));
        st.mark_known(9, 1001);
        assert!(st.is_known(9, 1001));
        st.unmark_known(9, 1001);
        assert!(!st.is_known(9, 1001));
    }

    #[test]
    fn physics_tick_counts_once_and_recovers_dug_support() {
        use mc_world::MutableWorld;
        let world = MutableWorld::new(1);
        let st = DropState::new(std::env::temp_dir(), 1);
        let id = st.alloc_id();
        // 支撑面 y=69（方块 (0,68,0)），掉落物落到该高度后静息。
        world.set(0, 68, 0, mc_world::STONE_BLOCK_ID);
        {
            let mut items = st.items.lock().unwrap();
            items.spawn(id, 55, 1, 0.5, 70.0, 0.5, 69.0).unwrap();
        }
        for _ in 0..200 {
            st.physics_tick(&world);
        }
        assert_eq!(st.tick_count(), 200);
        let snap = st.items.lock().unwrap();
        let e = snap.get(id).unwrap();
        assert!(e.on_ground, "drop must rest on the support block");
        assert_eq!(e.support, Some((0, 68, 0)));
        drop(snap);

        // 挖掉支撑、留下更低一层：下一次全局 tick 必须重算并恢复下落。
        world.set(0, 68, 0, mc_world::AIR_BLOCK_ID);
        world.set(0, 67, 0, mc_world::STONE_BLOCK_ID);
        st.physics_tick(&world);
        assert_eq!(st.tick_count(), 201, "one tick call = one count, independent of connections");
        let snap = st.items.lock().unwrap();
        let e = snap.get(id).unwrap();
        assert_eq!(e.rest_y, 68.0, "rest_y must follow the next support down");
        assert!(!e.on_ground);
    }

    /// P31：经验球物理只随全服 ticker 推进；XP 经 per-conn 背包累计并落盘。
    #[test]
    fn orb_physics_and_xp_persist_through_inventory() {
        use mc_world::MutableWorld;
        let dir = std::env::temp_dir().join(format!("mc-p31-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let world = MutableWorld::new(1);
        let st = DropState::new(dir.clone(), 1);
        st.load_inventory(7, "xp-user");
        let id = st.alloc_id();
        {
            let mut orbs = st.orbs.lock().unwrap();
            orbs.spawn(id, 3, 0.5, 70.0, 0.5, 69.0).unwrap();
        }
        for _ in 0..20 {
            st.physics_tick(&world);
        }
        assert_eq!(st.tick_count(), 20);
        let delay = st.orbs.lock().unwrap().get(id).unwrap().pickup_delay;
        assert_eq!(delay, 0, "pickup delay must decay on the global ticker");
        let picked = st.orbs.lock().unwrap().remove(id).unwrap();
        st.with_inventory(7, |inv| {
            inv.add_experience(picked.amount);
        });
        st.save_inventory(7, "xp-user");
        let (back, _) = PlayerInventory::load_from_dir(&dir, "xp-user", 1).unwrap();
        assert_eq!(back.total_xp(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crops_advance_on_global_tick_and_uproot_on_dig() {
        use mc_world::MutableWorld;
        let world = MutableWorld::new(1);
        let st = DropState::new(std::env::temp_dir(), 1);
        st.set_stage_ticks(2);
        // 在泥土上种一株（直接落子模拟种植路径的 set）。
        world.set(0, 70, 0, mc_world::DIRT_BLOCK_ID);
        world.set(0, 71, 0, mc_world::WHEAT_CROP_BASE_ID);
        st.plant_crop(0, 71, 0);
        assert_eq!(st.crop_count(), 1);
        // 未到阶段：tick 不推进。
        st.physics_tick(&world);
        assert_eq!(world.get(0, 71, 0), mc_world::WHEAT_CROP_BASE_ID);
        // 第 2 拍：长到 age=1（无 hub 也落子，只是不广播）。
        st.physics_tick(&world);
        assert_eq!(world.get(0, 71, 0), mc_world::wheat_stage_id(1));
        // retime 回拨到 0：下 4 拍后 elapsed=6 → 跳到 age=3。
        assert!(st.retime_crop(0, 71, 0, 0));
        for _ in 0..4 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 71, 0), mc_world::wheat_stage_id(3));
        // 阶段上限钳制在 7。
        for _ in 0..8 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 71, 0), mc_world::wheat_stage_id(7));
        // 挖掉后摘除跟踪（挖掘路径显式调用；ticker 懒摘除是兜底）。
        world.set(0, 71, 0, mc_world::AIR_BLOCK_ID);
        st.uproot_crop(0, 71, 0);
        assert_eq!(st.crop_count(), 0);
        assert!(!st.retime_crop(0, 71, 0, 0), "unknown crop must not be retimed");
    }

    #[test]
    fn p33_new_crops_grow_by_own_max_age_and_farmland_doubles_speed() {
        use mc_world::crops::{crop_stage_id, BEETROOT, CARROT};
        use mc_world::MutableWorld;
        let world = MutableWorld::new(31);
        let st = DropState::new(std::env::temp_dir(), 31);
        st.set_stage_ticks(2);
        // 胡萝卜在泥土上：普通速度；甜菜在耕地上：间隔减半，但最多只到自己的 age=3。
        world.set(0, 70, 0, mc_world::DIRT_BLOCK_ID);
        world.set(0, 71, 0, CARROT.base_id);
        st.plant_crop_spec(&CARROT, 0, 71, 0);
        world.set(4, 70, 0, mc_world::FARMLAND_BLOCK_ID);
        world.set(4, 71, 0, BEETROOT.base_id);
        st.plant_crop_spec(&BEETROOT, 4, 71, 0);
        assert_eq!(st.crop_count(), 2);
        st.physics_tick(&world);
        assert_eq!(world.get(0, 71, 0), CARROT.base_id, "carrot needs two ticks per stage");
        assert_eq!(world.get(4, 71, 0), crop_stage_id(&BEETROOT, 1), "farmland halves interval");
        for _ in 0..15 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 71, 0), crop_stage_id(&CARROT, 7), "carrot matures at age 7");
        assert_eq!(world.get(4, 71, 0), crop_stage_id(&BEETROOT, 3), "beetroot clamps at age 3");
        // 挖掉后跟踪摘除（与小麦同链路）。
        world.set(0, 71, 0, mc_world::AIR_BLOCK_ID);
        st.uproot_crop(0, 71, 0);
        world.set(4, 71, 0, mc_world::AIR_BLOCK_ID);
        st.uproot_crop(4, 71, 0);
        assert_eq!(st.crop_count(), 0);
    }

    #[test]
    fn p33_adopt_and_bonemeal_sync_for_new_crops() {
        use mc_world::crops::{crop_stage_id, BEETROOT, CARROT};
        use mc_world::MutableWorld;
        let world = MutableWorld::new(32);
        let st = DropState::new(std::env::temp_dir(), 32);
        st.set_stage_ticks(4);
        world.set(0, 70, 0, mc_world::DIRT_BLOCK_ID);
        world.set(0, 71, 0, crop_stage_id(&CARROT, 5));
        world.set(4, 70, 0, mc_world::DIRT_BLOCK_ID);
        world.set(4, 71, 0, crop_stage_id(&BEETROOT, 2));
        st.adopt_existing_crops(&world);
        assert_eq!(st.crop_count(), 2);
        assert_eq!(st.crop_stage(0, 71, 0), Some(5));
        assert_eq!(st.crop_stage(4, 71, 0), Some(2));
        // 骨粉把胡萝卜推到 6（模拟 decide_bonemeal 已落子），跟踪同步不跳变。
        world.set(0, 71, 0, crop_stage_id(&CARROT, 6));
        st.apply_bonemeal_crop_spec(&CARROT, 0, 71, 0, 6);
        assert_eq!(st.crop_stage(0, 71, 0), Some(6));
        for _ in 0..3 {
            st.physics_tick(&world);
        }
        assert_eq!(st.crop_stage(0, 71, 0), Some(6));
        assert_eq!(world.get(0, 71, 0), crop_stage_id(&CARROT, 6));
    }

    #[test]
    fn adopt_existing_crops_recovers_saved_wheat_after_restart() {
        use mc_world::MutableWorld;
        let world = MutableWorld::new(3);
        world.set(5, 70, 5, mc_world::DIRT_BLOCK_ID);
        world.set(5, 71, 5, mc_world::wheat_stage_id(4));
        // 新 DropState（模拟重启后内存丢失）：认领存量小麦。
        let st = DropState::new(std::env::temp_dir(), 3);
        st.set_stage_ticks(4);
        assert_eq!(st.crop_count(), 0);
        st.adopt_existing_crops(&world);
        assert_eq!(st.crop_count(), 1);
        // 从认领时刻重计时：stage=4、间隔 4 拍 → 第 20 拍时 want=5，长到 age=5。
        for _ in 0..20 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(5, 71, 5), mc_world::wheat_stage_id(5));
    }

    /// 停止句柄的可验证语义（服务端状态级验证，真机手感 T1–T13 仍需人工确认）：
    /// 启动后 tick_count 增长 → stop 后不再增长 → 可再次 start 且计数继续累加。
    #[test]
    fn global_ticker_can_be_stopped_and_restarted() {
        let world = Arc::new(MutableWorld::new(1));
        let st = Arc::new(DropState::new(std::env::temp_dir(), 1));

        assert!(st.start_global_tick(Arc::clone(&world)));
        assert!(!st.start_global_tick(Arc::clone(&world)), "duplicate start must be rejected");

        let c0 = st.tick_count();
        let deadline = Instant::now() + Duration::from_secs(2);
        while st.tick_count() < c0 + 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(st.tick_count() >= c0 + 2, "started ticker must advance tick_count");

        assert!(st.stop_global_tick(), "stop must report it stopped a live ticker");
        assert!(!st.stop_global_tick(), "repeated stop is a no-op");
        // 允许 stop 前已开始的最后一次 physics_tick 落地，之后再观察一个静默窗口。
        std::thread::sleep(Duration::from_millis(100));
        let stopped_at = st.tick_count();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(st.tick_count(), stopped_at, "stopped ticker must not tick again");

        assert!(st.start_global_tick(Arc::clone(&world)), "restart after stop must work");
        assert!(!st.start_global_tick(Arc::clone(&world)), "restart still allows only one ticker");
        let deadline = Instant::now() + Duration::from_secs(2);
        while st.tick_count() < stopped_at + 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let after_restart = st.tick_count();
        assert!(after_restart >= stopped_at + 2, "restarted ticker must keep ticking");
        assert!(st.stop_global_tick(), "clean up the restarted ticker");
    }

    #[test]
    fn saplings_grow_into_small_oak_on_global_tick() {
        use mc_world::{MutableWorld, WorldGen};
        let world = MutableWorld::new(11);
        let st = DropState::new(std::env::temp_dir(), 11);
        st.set_sapling_ticks(3);
        // 出生平台上种（平坦 + 上方空气，冠层 ±2 不撞地形）。
        let s = WorldGen::new(11).spawn_pos();
        let (x, y, z) = (s.0, s.1, s.2);
        world.set(x, y - 1, z, mc_world::DIRT_BLOCK_ID);
        world.set(x, y, z, mc_world::OAK_SAPLING_BLOCK_ID);
        st.plant_sapling(x, y, z);
        assert_eq!(st.sapling_count(), 1);
        // 未到期：还是树苗。
        st.physics_tick(&world);
        st.physics_tick(&world);
        assert_eq!(world.get(x, y, z), mc_world::OAK_SAPLING_BLOCK_ID);
        // 第 3 拍到期：树苗格变原木，冠层出现树叶。
        st.physics_tick(&world);
        assert_eq!(world.get(x, y, z), mc_world::OAK_LOG_BLOCK_ID);
        assert_eq!(world.get(x, y + 3, z), mc_world::OAK_LOG_BLOCK_ID, "trunk top");
        assert_eq!(world.get(x + 1, y + 1, z), mc_world::OAK_LEAVES_BLOCK_ID);
        assert_eq!(st.sapling_count(), 0, "grown sapling leaves tracking");
    }

    #[test]
    fn saplings_skip_occupied_cells_and_never_overwrite() {
        use mc_world::{MutableWorld, WorldGen};
        let world = MutableWorld::new(12);
        let st = DropState::new(std::env::temp_dir(), 12);
        st.set_sapling_ticks(2);
        let s = WorldGen::new(12).spawn_pos();
        let (x, y, z) = (s.0, s.1, s.2);
        world.set(x, y - 1, z, mc_world::DIRT_BLOCK_ID);
        world.set(x, y, z, mc_world::OAK_SAPLING_BLOCK_ID);
        // 在冠层预埋石头：生长必须跳过该格。
        world.set(x + 1, y + 1, z, mc_world::STONE_BLOCK_ID);
        st.plant_sapling(x, y, z);
        st.physics_tick(&world);
        st.physics_tick(&world);
        assert_eq!(world.get(x, y, z), mc_world::OAK_LOG_BLOCK_ID);
        assert_eq!(
            world.get(x + 1, y + 1, z),
            mc_world::STONE_BLOCK_ID,
            "occupied canopy cell must not be overwritten"
        );
        assert_eq!(st.sapling_count(), 0);
    }

    #[test]
    fn saplings_cancel_when_dug_or_soil_gone() {
        use mc_world::{MutableWorld, WorldGen};
        let world = MutableWorld::new(13);
        let st = DropState::new(std::env::temp_dir(), 13);
        st.set_sapling_ticks(2);
        let s = WorldGen::new(13).spawn_pos();
        let (x, y, z) = (s.0, s.1, s.2);
        // 挖苗取消。
        world.set(x, y - 1, z, mc_world::DIRT_BLOCK_ID);
        world.set(x, y, z, mc_world::OAK_SAPLING_BLOCK_ID);
        st.plant_sapling(x, y, z);
        world.set(x, y, z, mc_world::AIR_BLOCK_ID);
        st.uproot_sapling(x, y, z);
        for _ in 0..4 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(x, y, z), mc_world::AIR_BLOCK_ID);
        assert_eq!(st.sapling_count(), 0);
        // 挖土取消（ticker 兜底：土壤没了就不长）。
        world.set(x + 3, y - 1, z, mc_world::DIRT_BLOCK_ID);
        world.set(x + 3, y, z, mc_world::OAK_SAPLING_BLOCK_ID);
        st.plant_sapling(x + 3, y, z);
        world.set(x + 3, y - 1, z, mc_world::AIR_BLOCK_ID);
        for _ in 0..4 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(x + 3, y, z), mc_world::OAK_SAPLING_BLOCK_ID, "soil gone must not grow");
        assert_eq!(st.sapling_count(), 0, "soil gone must leave tracking");
    }

    #[test]
    fn adopt_existing_saplings_recovers_after_restart() {
        use mc_world::{MutableWorld, WorldGen};
        let world = MutableWorld::new(14);
        let s = WorldGen::new(14).spawn_pos();
        world.set(s.0, s.1 - 1, s.2, mc_world::DIRT_BLOCK_ID);
        world.set(s.0, s.1, s.2, mc_world::OAK_SAPLING_BLOCK_ID);
        let st = DropState::new(std::env::temp_dir(), 14);
        st.set_sapling_ticks(3);
        assert_eq!(st.sapling_count(), 0);
        st.adopt_existing_saplings(&world);
        assert_eq!(st.sapling_count(), 1);
        for _ in 0..3 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(s.0, s.1, s.2), mc_world::OAK_LOG_BLOCK_ID);
    }

    #[test]
    fn p27_crops_on_farmland_grow_twice_as_fast_and_revert_on_soil_change() {
        use mc_world::MutableWorld;
        let world = MutableWorld::new(21);
        let st = DropState::new(std::env::temp_dir(), 21);
        // 基础间隔 8 拍：泥土 8 拍/阶段，耕地 4 拍/阶段。
        st.set_stage_ticks(8);
        world.set(0, 70, 0, mc_world::DIRT_BLOCK_ID);
        world.set(0, 71, 0, mc_world::WHEAT_CROP_BASE_ID);
        st.plant_crop(0, 71, 0);
        world.set(4, 70, 0, mc_world::FARMLAND_BLOCK_ID);
        world.set(4, 71, 0, mc_world::WHEAT_CROP_BASE_ID);
        st.plant_crop(4, 71, 0);
        // 8 拍后：耕地 age=2，泥土 age=1。
        for _ in 0..8 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(4, 71, 0), mc_world::wheat_stage_id(2), "farmland must halve the interval");
        assert_eq!(world.get(0, 71, 0), mc_world::wheat_stage_id(1), "dirt keeps the base interval");
        // 耕地换回泥土（直接换土模拟）：下一阶段回到普通速度，不回退已有阶段。
        // 数学：种下 tick=0；前 8 拍耕地按间隔 4 走到 want=8/4=2；
        // 换土后间隔 8：now=16 时 want=16/8=2（不变，只前进不回退）；
        // now=24 时 want=3。泥土株全程间隔 8：8→1、16→2、24→3。
        world.set(4, 70, 0, mc_world::DIRT_BLOCK_ID);
        for _ in 0..8 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(4, 71, 0), mc_world::wheat_stage_id(2), "soil change must not regress");
        assert_eq!(world.get(0, 71, 0), mc_world::wheat_stage_id(2));
        // 再 8 拍（elapsed=24）：两株都 want=3。
        for _ in 0..8 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(4, 71, 0), mc_world::wheat_stage_id(3));
        assert_eq!(world.get(0, 71, 0), mc_world::wheat_stage_id(3));
    }

    #[test]
    fn p27_bonemealed_wheat_updates_tracking_without_stage_jump() {
        use mc_world::MutableWorld;
        let world = MutableWorld::new(22);
        let st = DropState::new(std::env::temp_dir(), 22);
        st.set_stage_ticks(10);
        world.set(0, 70, 0, mc_world::DIRT_BLOCK_ID);
        world.set(0, 71, 0, mc_world::WHEAT_CROP_BASE_ID);
        st.plant_crop(0, 71, 0);
        // 骨粉落子到 age=2（模拟 `decide_bonemeal` 已 set），同步跟踪。
        world.set(0, 71, 0, mc_world::wheat_stage_id(2));
        st.apply_bonemeal_crop(0, 71, 0, 2);
        assert_eq!(st.crop_stage(0, 71, 0), Some(2));
        // ticker 不跳变：elapsed 很小，want 仍 < 2，不回退也不快进。
        for _ in 0..5 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 71, 0), mc_world::wheat_stage_id(2));
        assert_eq!(st.crop_stage(0, 71, 0), Some(2));
        // 未跟踪作物被骨粉：补登记后继续生长（planted 按补登记时刻记，
        // 需走满一个整间隔才进下一阶段：planted=5、间隔 10 → now=25 时 want=2）。
        world.set(9, 70, 0, mc_world::DIRT_BLOCK_ID);
        world.set(9, 71, 0, mc_world::wheat_stage_id(1));
        st.apply_bonemeal_crop(9, 71, 0, 1);
        assert_eq!(st.crop_stage(9, 71, 0), Some(1));
        for _ in 0..10 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(9, 71, 0), mc_world::wheat_stage_id(1), "one interval not yet elapsed");
        for _ in 0..10 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(9, 71, 0), mc_world::wheat_stage_id(2));
    }

    #[test]
    fn p27_grow_sapling_now_places_tree_and_rejects_bad_soil() {
        use mc_world::{MutableWorld, WorldGen};
        let world = MutableWorld::new(23);
        let st = DropState::new(std::env::temp_dir(), 23);
        let s = WorldGen::new(23).spawn_pos();
        let (x, y, z) = (s.0, s.1, s.2);
        // 好土壤（耕地也行，P27 简化）：长树成功，跟踪摘除。
        world.set(x, y - 1, z, mc_world::FARMLAND_BLOCK_ID);
        world.set(x, y, z, mc_world::OAK_SAPLING_BLOCK_ID);
        st.plant_sapling(x, y, z);
        assert!(st.grow_sapling_now(&world, x, y, z));
        assert_eq!(world.get(x, y, z), mc_world::OAK_LOG_BLOCK_ID);
        assert_eq!(world.get(x + 1, y + 1, z), mc_world::OAK_LEAVES_BLOCK_ID);
        assert_eq!(st.sapling_count(), 0);
        // 坏土壤（石头）：拒绝并摘除。
        world.set(x + 5, y - 1, z, mc_world::STONE_BLOCK_ID);
        world.set(x + 5, y, z, mc_world::OAK_SAPLING_BLOCK_ID);
        st.plant_sapling(x + 5, y, z);
        assert!(!st.grow_sapling_now(&world, x + 5, y, z));
        assert_eq!(world.get(x + 5, y, z), mc_world::OAK_SAPLING_BLOCK_ID, "refused tree stays a sapling");
        assert_eq!(st.sapling_count(), 0);
    }

    #[test]
    fn p27_leaves_decay_when_no_log_nearby_and_survive_with_support() {
        use mc_world::MutableWorld;
        let world = MutableWorld::new(24);
        let st = DropState::new(std::env::temp_dir(), 24);
        st.set_leaf_decay_ticks(3);
        // 高空孤叶（y=200，世界生成树冠到不了，基底全空气）：登记 → 3 拍后变空气。
        world.set(50, 200, 50, mc_world::OAK_LEAVES_BLOCK_ID);
        st.schedule_leaf_decay_around(&world, 50, 200, 50);
        assert_eq!(st.leaf_pending_count(), 1);
        st.physics_tick(&world);
        assert_eq!(world.get(50, 200, 50), mc_world::OAK_LEAVES_BLOCK_ID, "delay not yet reached");
        st.physics_tick(&world);
        st.physics_tick(&world);
        assert_eq!(world.get(50, 200, 50), mc_world::AIR_BLOCK_ID, "unsupported leaves must decay");
        assert_eq!(st.leaf_pending_count(), 0);
        // 有支撑的叶子：登记后因 3 格内有原木而取消，不 decay。
        world.set(60, 200, 60, mc_world::OAK_LEAVES_BLOCK_ID);
        world.set(62, 200, 60, mc_world::OAK_LOG_BLOCK_ID);
        st.schedule_leaf_decay_around(&world, 62, 200, 60);
        assert_eq!(st.leaf_pending_count(), 1);
        for _ in 0..6 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(60, 200, 60), mc_world::OAK_LEAVES_BLOCK_ID, "supported leaves must stay");
        assert_eq!(st.leaf_pending_count(), 0);
    }

    #[test]
    fn p27_leaf_decay_spawns_drops_and_clears_pending() {
        use mc_world::MutableWorld;
        let seed = 25i64;
        let world = MutableWorld::new(seed);
        let st = DropState::new(std::env::temp_dir(), seed);
        st.set_leaf_decay_ticks(2);
        // 高空选一个 decay 必掉东西的位点（避开 6/8 空分支与苹果随机性，掉什么都行；
        // y=200 基底全空气，保证 3 格内无世界生成原木、不被支撑误取消）。
        let mut spot = None;
        for x in 0..200 {
            if !mc_world::drops::leaf_decay_drops(x, 200, 7, seed).is_empty() {
                spot = Some(x);
                break;
            }
        }
        let x = spot.expect("some leaf position must drop something");
        let want = mc_world::drops::leaf_decay_drops(x, 200, 7, seed);
        world.set(x, 200, 7, mc_world::OAK_LEAVES_BLOCK_ID);
        st.schedule_leaf_decay_around(&world, x, 200, 7);
        st.physics_tick(&world);
        st.physics_tick(&world);
        assert_eq!(world.get(x, 200, 7), mc_world::AIR_BLOCK_ID);
        // 掉落已生成实体（各连接 tick_drops 视距补发会自动同步，ticker 内不直接发包）。
        let items = st.items.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(items.len(), want.len(), "each decay drop becomes one entity");
        for d in &want {
            assert!(
                items.ids().iter().any(|id| {
                    let e = items.get(*id).unwrap();
                    e.item_id == d.item_id && e.count == d.count
                }),
                "decay must spawn {d:?}"
            );
        }
    }

    // ---- P37：五种生长模型的单测代表（新 map + 全服唯一 ticker）----

    #[test]
    fn p37_nether_wart_grows_on_soul_sand_and_ignores_farmland_boost() {
        use mc_world::crops::{age_stage_id, NETHER_WART, SOUL_SAND_BLOCK_ID, PlantKind};
        let world = MutableWorld::new(41);
        let st = DropState::new(std::env::temp_dir(), 41);
        st.set_plant_ticks(2);
        world.set(0, 100, 0, SOUL_SAND_BLOCK_ID);
        world.set(0, 101, 0, NETHER_WART.base_id);
        st.plant_kind(PlantKind::Age(&NETHER_WART), 0, 101, 0, 0);
        assert_eq!(st.plant_count(), 1);
        st.physics_tick(&world);
        st.physics_tick(&world);
        assert_eq!(world.get(0, 101, 0), age_stage_id(&NETHER_WART, 1));
        // 换成耕地也不加速（该作物 farmland_boost=false）。
        world.set(0, 100, 0, mc_world::FARMLAND_BLOCK_ID);
        for _ in 0..2 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 101, 0), age_stage_id(&NETHER_WART, 2));
        for _ in 0..6 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 101, 0), age_stage_id(&NETHER_WART, 3));
    }

    #[test]
    fn p37_other_sapling_grows_with_own_log_and_leaves() {
        use mc_world::{crops::{BIRCH, PlantKind}, WorldGen};
        let world = MutableWorld::new(42);
        let st = DropState::new(std::env::temp_dir(), 42);
        st.set_plant_ticks(3);
        let s = WorldGen::new(42).spawn_pos();
        let (x, y, z) = (s.0, s.1, s.2);
        world.set(x, y - 1, z, mc_world::DIRT_BLOCK_ID);
        world.set(x, y, z, BIRCH.sapling_id);
        st.plant_kind(PlantKind::Tree(&BIRCH), x, y, z, 0);
        for _ in 0..2 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(x, y, z), BIRCH.sapling_id);
        st.physics_tick(&world);
        assert_eq!(world.get(x, y, z), BIRCH.log_id, "birch must use birch_log");
        assert_eq!(world.get(x, y + 3, z), BIRCH.log_id);
        assert_eq!(world.get(x + 1, y + 1, z), BIRCH.leaves_id, "birch leaves not oak");
        assert_eq!(st.plant_count(), 0);
    }

    #[test]
    fn p37_vertical_cane_stacks_up_to_max_height() {
        use mc_world::crops::{PlantKind, SUGAR_CANE};
        let world = MutableWorld::new(43);
        let st = DropState::new(std::env::temp_dir(), 43);
        st.set_plant_ticks(2);
        world.set(0, 100, 0, mc_world::SAND_BLOCK_ID);
        world.set(0, 101, 0, SUGAR_CANE.base_id);
        st.plant_kind(PlantKind::Vertical(&SUGAR_CANE), 0, 101, 0, 0);
        for _ in 0..2 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 102, 0), SUGAR_CANE.base_id, "one segment per interval");
        assert_eq!(st.plant_stage(0, 101, 0), Some(1));
        for _ in 0..2 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 103, 0), SUGAR_CANE.base_id);
        assert_eq!(st.plant_stage(0, 101, 0), Some(2), "max height 3 = stage 2");
        for _ in 0..4 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 104, 0), mc_world::AIR_BLOCK_ID, "must stop at 3 segments");
    }

    #[test]
    fn p37_berry_bush_and_cocoa_advance_their_own_ages() {
        use mc_world::crops::{cocoa_state, PlantKind, COCOA, SWEET_BERRY_BUSH};
        let world = MutableWorld::new(44);
        let st = DropState::new(std::env::temp_dir(), 44);
        st.set_plant_ticks(2);
        // 甜浆果丛（普通土）。
        world.set(0, 100, 0, mc_world::DIRT_BLOCK_ID);
        world.set(0, 101, 0, SWEET_BERRY_BUSH.base_id);
        st.plant_kind(PlantKind::Age(&SWEET_BERRY_BUSH), 0, 101, 0, 0);
        // 可可贴在 jungle_log 北面（支撑在 z+1）。
        world.set(4, 101, 1, COCOA.support_id);
        world.set(4, 101, 0, cocoa_state(0, 0));
        st.plant_kind(PlantKind::Side(&COCOA), 4, 101, 0, 0);
        assert_eq!(st.plant_count(), 2);
        for _ in 0..2 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 101, 0), mc_world::crops::age_stage_id(&SWEET_BERRY_BUSH, 1));
        assert_eq!(world.get(4, 101, 0), cocoa_state(1, 0));
        // 支撑被挖：可可跟踪摘除、不再生长。
        world.set(4, 101, 1, mc_world::AIR_BLOCK_ID);
        for _ in 0..4 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(4, 101, 0), cocoa_state(1, 0));
        assert_eq!(st.plant_stage(4, 101, 0), None);
    }

    #[test]
    fn p37_melon_stem_matures_and_places_real_fruit_block() {
        use mc_world::crops::{age_stage_id, FruitSpec, MELON_STEM, PlantKind};
        let world = MutableWorld::new(45);
        let st = DropState::new(std::env::temp_dir(), 45);
        st.set_plant_ticks(2);
        world.set(0, 100, 0, mc_world::FARMLAND_BLOCK_ID);
        world.set(0, 101, 0, MELON_STEM.base_id);
        st.plant_kind(PlantKind::Age(&MELON_STEM), 0, 101, 0, 0);
        // 耕地加速：间隔 1 拍/阶段 → 7 拍成熟。
        for _ in 0..7 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(0, 101, 0), age_stage_id(&MELON_STEM, 7));
        let fruit: FruitSpec = MELON_STEM.fruit.unwrap();
        assert_eq!(fruit.block_id, 10067, "melon blockstate from data/777");
        assert_eq!(world.get(1, 101, 0), fruit.block_id, "ripe stem must place a melon next door");
        assert!(!mc_world::drops_for_block(fruit.block_id, 1, 101, 0, 45).is_empty());
        // 只结一次：邻格改回空气后不再补瓜（MVP 与原版差异）。
        world.set(1, 101, 0, mc_world::AIR_BLOCK_ID);
        for _ in 0..4 {
            st.physics_tick(&world);
        }
        assert_eq!(world.get(1, 101, 0), mc_world::AIR_BLOCK_ID);
    }

    /// P36.2：redb 模式下背包/XP 走 WorldStore，缺旧 .dat 文件，重启/换连接仍在。
    #[test]
    fn redb_inventory_survives_reopen_and_periodic_save() {
        use mc_world::store::WorldStore;
        let dir = std::env::temp_dir().join(format!("mc-p38-drop-{}-redb", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seed = 909;
        {
            let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
            let st = DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store)));
            st.load_inventory(7, "alice");
            st.with_inventory(7, |inv| {
                inv.add_item(55, 3);
                inv.add_experience(9);
            });
            st.save_inventory(7, "alice"); // 拾取后即时刷盘
            st.with_inventory(7, |inv| inv.add_experience(4));
            st.save_all_inventories(); // 60s 周期刷盘
            assert_eq!(store.load_player("alice").unwrap().unwrap().total_xp(), 13);
            assert!(!PlayerInventory::save_path(&dir, "alice", seed).exists());
        }
        let st = DropState::with_store(
            dir.clone(),
            seed,
            Some(Arc::new(WorldStore::open(&dir, seed).unwrap())),
        );
        let (filled, held) = st.load_inventory(8, "alice"); // 换连接按用户名恢复
        assert_eq!(filled, 2, "starter stone in 36 + picked stack in 37");
        assert_eq!(held, 0);
        st.with_inventory(8, |inv| {
            assert_eq!(inv.get(36), Some((mc_world::drops::ITEM_STONE, 64)));
            assert_eq!(inv.get(37), Some((55, 3)));
            assert_eq!(inv.total_xp(), 13);
        });
        st.unload_inventory(8, "alice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P40：生存状态随 redb players 表跨 DropState 重建保持（断线/周期刷盘路径）。
    #[test]
    fn redb_player_survival_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mc-p40-redb-surv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seed = 91;
        let target = SurvivalState { health: 12.5, food: 6, saturation: 1.5 };
        {
            let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
            let st = DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store)));
            st.load_inventory(11, "survivor");
            st.apply_survival(11, |s| *s = target);
            assert_eq!(st.survival_state(11), Some(target));
            st.save_inventory(11, "survivor");
            st.unload_inventory(11, "survivor");
        }
        let reopened = WorldStore::open(&dir, seed).unwrap();
        let persisted = reopened.load_player("survivor").unwrap().unwrap();
        assert_eq!(persisted.survival(), target);
        let st2 = DropState::with_store(dir.clone(), seed, Some(Arc::new(reopened)));
        st2.load_inventory(12, "survivor");
        assert_eq!(st2.survival_state(12), Some(target));
        let _ = std::fs::remove_dir_all(&dir);
    }
