//! drop_state: 掉落物实体 + 背包的 net 线程共享状态，只兼容 26.3/777。
//!
//! 设计（P17 模式的延续）：socket 仍在 net 线程，ECS 没有写通道，因此掉落物的
//! 生成/同步/拾取判定与背包同步都在 net 线程完成（权威逻辑见
//! `mc_world::{ItemEntities, PlayerInventory}` 纯逻辑模块，本文件只做
//! 并发容器 + 存档 + 发包所需的派生状态）。
//!
//! - `items`：全服唯一的 `ItemEntities`（entity_id 全局自增，`next_id` 从 1000 起，
//!   与 LoginPlay entity_id=1 不冲突，永不复用，重启后从 1000 重新开始）。
//! - `orbs`（P31）：独立的 `XpOrbEntities` 经验球池，共用上面的全局 id 分配；
//!   物理与 items 同在全服唯一 ticker 推进，拾取/视距同步在连接侧做。
//! - `inventories`：conn_id → 背包；Join 时按用户名读盘（缺文件=新背包+石头开局），
//!   拾取/放置/断线时刷盘（`player-<user>-<seed>.dat`，seed 隔离；键用用户名而非 uuid，
//!   因离线模式 uuid 每次随机，见 `mc_world::inventory` 文档）。
//! - `known`：conn_id → 该连接已见实体（视距过滤：离开视距发 Remove，进入补发）。

use mc_world::crops::PlantKind;
use mc_world::store::{TimerKind, TimerRecord, WorldStore};
use mc_world::{ItemEntities, MutableWorld, PlayerInventory, XpOrbEntities};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tracing::warn;

use super::broadcast::BroadcastHub;
use super::chunk_cache::ChunkCache;
use super::timer_spec::{self, Restored};
mod timer_io;

/// 全服掉落物物理节拍（ms）：只由 [`DropState::start_global_tick`] 的一个 ticker 驱动。
pub const DROP_TICK_MS: u64 = 50;

/// 掉落物 entity_id 起始（避开 LoginPlay entity_id=1）。
pub const FIRST_DROP_ENTITY_ID: i32 = 1000;
/// 拾取动画 collector（本服 LoginPlay entity_id 恒为 1）。
pub const PLAYER_ENTITY_ID: i32 = 1;
/// 新背包开局石头数量（ hotbar 槽 36），保证“放置扣 1”可验证；后续按生存模式替换。
pub const STARTER_STONE_COUNT: i32 = mc_world::inventory::STARTER_COUNT;

/// P22/P33 作物条目：种下时的全局 tick + 上次已广播的 age + 作物规格。
/// 生长只由全服唯一 ticker 推进（P18.1 R1：连接侧绝不 tick），阶段变更经
/// `MutableWorld::set` 落子并走 hub 广播 fan-out；挖掉后由挖掘路径显式摘除。
#[derive(Debug, Clone, Copy)]
struct CropEntry {
    planted_tick: u64,
    stage: u8,
    /// P33：按作物自己的 base/max_age 计算阶段与落子（小麦仍是同一张表的一项）。
    crop: &'static mc_world::crops::CropSpec,
}

/// P37 新植物条目：五种生长模型共用一个跟踪 map。
/// `stage` 在 age 模型里是 age，在竖向模型里是已长高度（0=基部），
/// 在树模型里恒 0（单次到期），在侧面模型里是 age。
#[derive(Debug, Clone, Copy)]
struct PlantEntry {
    kind: mc_world::crops::PlantKind,
    planted_tick: u64,
    stage: u8,
    /// 可可的 facing 索引（0..3），其它模型为 0。
    facing: u8,
    /// 瓜果已落过一颗（MVP：每株只结一次）。
    fruit_done: bool,
}

/// net 线程共享的掉落/背包状态（`Arc` 进 `PlayOptions`）。
///
/// 物理权威（下落/age/despawn/支撑重算）只由 [`Self::start_global_tick`] 启动的
/// **单个** ticker 驱动，与本文件的连接态无关；每个连接只做拾取/视距同步。
#[derive(Debug)]
pub struct DropState {
    pub items: Mutex<ItemEntities>,
    /// P31 经验球独立实体池（entity_id 与物品共用 `alloc_id`，不会冲突）。
    /// 物理只由 [`Self::physics_tick`] 推进，连接侧绝不 tick（P18.1 R1）。
    pub orbs: Mutex<XpOrbEntities>,
    inventories: Mutex<HashMap<u64, PlayerInventory>>,
    known: Mutex<HashMap<u64, HashSet<i32>>>,
    next_id: AtomicI32,
    /// 物理 tick 次数（只统计 `ItemEntities::tick()`，与在线连接数无关；测试可观测）。
    tick_count: AtomicU64,
    /// P22 作物：(x,y,z) → 条目；种下时登记，挖掉时摘除，生长由全局 ticker 推进。
    crops: Mutex<HashMap<(i32, i32, i32), CropEntry>>,
    /// P22 每阶段 tick 数（生产 1200；测试可经 `set_stage_ticks` 调小）。
    stage_ticks: AtomicU64,
    /// P26 树苗：(x,y,z) → 种下 tick；到期长成小橡树，挖苗/挖土取消（仿 `uproot_crop`）。
    /// 另起同构结构而不复用 `crops`（理由见 `tick_saplings`）：作物是多阶段 age 推进，
    /// 树苗是单次到期替换为多格树，计时（固定 7200）与落子（57 格广播）语义完全不同，
    /// 共用一个 map 会把阶段/间隔耦合在一起，重启认领与摘除条件也不同（树苗多查土壤）。
    saplings: Mutex<HashMap<(i32, i32, i32), u64>>,
    /// P26 树苗生长 tick 数（生产 `SAPLING_GROW_TICKS` = 7200 ≈ 6min；测试调小）。
    sapling_ticks: AtomicU64,
    /// P37：新植物（新 age 作物/新树苗/竖向/侧面/瓜果梗）跟踪。
    /// 与旧 `crops`/`saplings` 分表，避免改变 P22/P26/P33 的计数与摘除语义。
    plants: Mutex<HashMap<(i32, i32, i32), PlantEntry>>,
    /// P37：新植物每阶段 tick 的生产值（`WHEAT_TICKS_PER_STAGE`），测试可整体调小。
    plant_ticks: AtomicU64,
    /// P27 失支撑树叶：pos → 登记时的全局 tick；到期且半径内无原木即 decay。
    /// 登记只由原木被挖触发（`schedule_leaf_decay_around`），自然树天然有支撑，
    /// 无需全世界扫描（见 HANDOFF §5）。
    pending_leaves: Mutex<HashMap<(i32, i32, i32), u64>>,
    /// P27 树叶 decay 延迟 tick 数（生产 `LEAF_DECAY_TICKS` = 100 ≈ 5s；测试调小）。
    leaf_decay_ticks: AtomicU64,
    /// P22 生长广播用 hub（`serve` 在 world+drops+hub 齐备时注入；None 则生长只落子不广播）。
    hub: Mutex<Option<Arc<BroadcastHub>>>,
    /// P22 生长缓存失效用（同上；None 则跳过失效，下次编辑时自然重编）。
    cache: Mutex<Option<Arc<ChunkCache>>>,
    /// 全服 ticker 是否已启动（每份 DropState 至多一个）。
    tick_started: AtomicBool,
    /// 当前代 ticker 的停止标志；每次 `start_global_tick` 装一个全新的 Arc。
    /// `stop` 只置旧代标志，`restart` 装新标志，旧线程即使还在 sleep 也不会误读新标志。
    stop_flag: Mutex<Option<Arc<AtomicBool>>>,
    save_dir: PathBuf,
    seed: i64,
    /// P36.2：redb 模式共享存储（`new()` 仍为 None，旧路径逐字节不变）。
    store: Option<Arc<WorldStore>>,
    /// P39：构造时从 redb `timers` 一次读入、尚未被 `adopt_existing_*` 认领的记录。
    /// 树叶观察在构造时直接进 `pending_leaves`；其余三类由 `serve` 的认领入口消耗。
    restored_timers: Mutex<Vec<TimerRecord>>,
    /// conn_id → 用户名（redb 的 players 表键；load/unload 时维护）。
    player_tags: Mutex<HashMap<u64, String>>,
}

impl DropState {
    /// 旧稀疏档模式（测试/显式 legacy 配置）。生产 redb 模式请用 `with_store`。
    pub fn new(save_dir: PathBuf, seed: i64) -> Self {
        Self::with_store(save_dir, seed, None)
    }

    /// redb / legacy 双模式构造；背包读写只改这两个入口，发包与判定逻辑不变。
    ///
    /// P39 redb 模式：启动时把 `meta.tick_offset` 恢复为全局 tick 基线，并一次读入
    /// `timers`（树叶观察直接进内存，其余留给 `adopt_existing_*` 按原 planted/stage
    /// 认领）。legacy 模式（`store=None`）完全不变：tick 从 0 起、认领重计时。
    pub fn with_store(save_dir: PathBuf, seed: i64, store: Option<Arc<WorldStore>>) -> Self {
        let tick_offset = store
            .as_ref()
            .map(|s| s.tick_offset().unwrap_or_else(|e| {
                warn!("redb tick_offset load failed seed={seed} ({e})");
                0
            }))
            .unwrap_or(0);
        let me = Self {
            items: Mutex::new(ItemEntities::new()),
            orbs: Mutex::new(XpOrbEntities::new()),
            inventories: Mutex::new(HashMap::new()),
            known: Mutex::new(HashMap::new()),
            next_id: AtomicI32::new(FIRST_DROP_ENTITY_ID),
            tick_count: AtomicU64::new(tick_offset),
            crops: Mutex::new(HashMap::new()),
            stage_ticks: AtomicU64::new(mc_world::WHEAT_TICKS_PER_STAGE),
            saplings: Mutex::new(HashMap::new()),
            sapling_ticks: AtomicU64::new(mc_world::SAPLING_GROW_TICKS),
            plants: Mutex::new(HashMap::new()),
            plant_ticks: AtomicU64::new(mc_world::WHEAT_TICKS_PER_STAGE),
            pending_leaves: Mutex::new(HashMap::new()),
            leaf_decay_ticks: AtomicU64::new(mc_world::LEAF_DECAY_TICKS),
            hub: Mutex::new(None),
            cache: Mutex::new(None),
            tick_started: AtomicBool::new(false),
            stop_flag: Mutex::new(None),
            save_dir,
            seed,
            store,
            restored_timers: Mutex::new(Vec::new()),
            player_tags: Mutex::new(HashMap::new()),
        };
        if let Some(store) = &me.store {
            match store.timers() {
                Ok(timers) => {
                    let mut pending = me.pending_leaves.lock().unwrap_or_else(|p| p.into_inner());
                    let mut restored = me.restored_timers.lock().unwrap_or_else(|p| p.into_inner());
                    for rec in timers {
                        match Restored::from_record(&rec) {
                            Some(Restored::Leaf) => {
                                pending.entry(rec.pos).or_insert(rec.planted_tick);
                            }
                            Some(_) => restored.push(rec),
                            None => warn!("redb timer ignored: unknown spec={} kind={:?}", rec.spec, rec.kind),
                        }
                    }
                }
                Err(e) => warn!("redb timers load failed seed={} ({e})", me.seed),
            }
        }
        me
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// 背包存档目录（P25 管理面板测试定位落盘文件用；生产路径不变）。
    pub fn save_dir(&self) -> PathBuf {
        self.save_dir.clone()
    }

    /// 全服物理 tick 次数；仅 `physics_tick` 递增，连接侧不会改动。
    pub fn tick_count(&self) -> u64 {
        self.tick_count.load(Ordering::Relaxed)
    }

    /// 启动全服唯一的掉落物理 ticker（50ms 一次）：支撑重算 + 物理 + despawn。
    ///
    /// - 用独立 `std::thread` 而非 tokio task：`DropState::new` 在单测（无 runtime）
    ///   里也会被调用，本函数只在显式调用时启动，不依赖任何 runtime。
    /// - 重复调用只生效一次（返回 `false`）；多连接共享同一份 `Arc<DropState>`，
    ///   不会因为连接数增加而产生第二份物理 tick。
    /// - 显式停止用 [`Self::stop_global_tick`]：线程在下一个 50ms 周期检查停止标志后
    ///   退出（最多再等一个节拍，不再多跑一次 `physics_tick`）。停止后可再次 start：
    ///   `tick_count` **不重置**、继续累加；实体/背包等服务端状态也原样保留。
    /// - 每代 ticker 用独立的 `Arc<AtomicBool>`：stop 只影响旧代，restart 装新标志，
    ///   旧线程即使仍在 sleep 也不会因共用标志被复位而变成第二个 ticker。
    /// - 服务端状态级验证；真机手感 T1–T13 仍需人工确认。
    pub fn start_global_tick(self: &Arc<Self>, world: Arc<MutableWorld>) -> bool {
        // 与 stop 共用同一把锁：tick_started 切换与 stop_flag 安装必须原子，
        // 否则 stop 与 restart 交错时会误停新代，或留下一个无人能停的幽灵 ticker。
        let mut guard = self.stop_flag.lock().unwrap_or_else(|p| p.into_inner());
        if self.tick_started.swap(true, Ordering::SeqCst) {
            return false;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *guard = Some(Arc::clone(&stop));
        drop(guard);

        let me = Arc::clone(self);
        let thread_stop = Arc::clone(&stop);
        let spawned = std::thread::Builder::new()
            .name("mc-drop-tick".to_string())
            .spawn(move || {
                let stop = thread_stop;
                let period = Duration::from_millis(DROP_TICK_MS);
                let mut next = Instant::now();
                loop {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    next += period;
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        // 落后超过一个节拍：重新对齐，避免忙等追帧。
                        next = now + period;
                    }
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    me.physics_tick(&world);
                }
            });
        match spawned {
            Ok(_) => true,
            Err(e) => {
                warn!("start global drop tick failed ({e})");
                stop.store(true, Ordering::SeqCst);
                let mut guard = self.stop_flag.lock().unwrap_or_else(|p| p.into_inner());
                self.tick_started.store(false, Ordering::SeqCst);
                *guard = None;
                false
            }
        }
    }

    /// 停止全服唯一的掉落物理 ticker（幂等）。
    ///
    /// - 返回 `true` 表示确实停了一个运行中的 ticker；`false` 表示当前没有 ticker。
    /// - 只置停止标志、不 join：线程在下一个 50ms 检查点自行退出；`tick_count`
    ///   不重置，实体/背包状态也不动。之后可再次调用 [`Self::start_global_tick`]，
    ///   新代 ticker 从原计数继续累加。
    /// - 服务端状态级验证；真机手感 T1–T13 仍需人工确认。
    pub fn stop_global_tick(&self) -> bool {
        // 与 start 同一把锁：先确认是哪一代、再置停，避免新代标志被旧 stop 误伤。
        let guard = self.stop_flag.lock().unwrap_or_else(|p| p.into_inner());
        if !self.tick_started.swap(false, Ordering::SeqCst) {
            return false;
        }
        if let Some(stop) = guard.as_ref() {
            stop.store(true, Ordering::SeqCst);
        }
        true
    }

    /// 全服唯一物理 tick：支撑被挖空的重算与 `ItemEntities::tick()` 在同一把写锁内，
    /// 避免连接侧看到「已解除静息但还没下落」的中间态。
    fn physics_tick(&self, world: &MutableWorld) {
        let mut items = self.items.lock().unwrap_or_else(|p| p.into_inner());
        let unsupported = items
            .find_unsupported(|s| world.get(s.0, s.1, s.2) != mc_world::AIR_BLOCK_ID);
        for (id, s) in unsupported {
            let new_rest = world.drop_rest_y(s.0, s.2, s.1) as f64;
            items.resume_fall(id, new_rest);
        }
        items.tick();
        self.tick_count.fetch_add(1, Ordering::Relaxed);
        drop(items);
        // P22 作物生长：同一 ticker 内推进，与掉落物理同节拍（50ms），连接侧不参与。
        self.tick_crops(world);
        // P37 新植物生长：与旧作物/树苗同一个 50ms ticker，连接侧绝不推进。
        self.tick_plants(world);
        // P26 树苗生长：同一 ticker 内推进，连接侧绝不碰（P18.1 R1 不重犯）。
        self.tick_saplings(world);
        // P27 树叶 decay：同一 ticker 内推进，连接侧绝不碰。
        self.tick_leaves(world);
        // P31 经验球物理：与掉落物同一全服 ticker、同一节拍（50ms），连接侧绝不 tick。
        // 放在原有作物/树苗/树叶之后，保持旧行为时序不变；锁顺序恒为 items → orbs
        //（连接侧不同时持两把锁），避免死锁。
        let mut orbs = self.orbs.lock().unwrap_or_else(|p| p.into_inner());
        let unsupported =
            orbs.find_unsupported(|s| world.get(s.0, s.1, s.2) != mc_world::AIR_BLOCK_ID);
        for (id, s) in unsupported {
            let new_rest = world.drop_rest_y(s.0, s.2, s.1) as f64;
            orbs.resume_fall(id, new_rest);
        }
        orbs.tick();
    }

    /// P22：按 `(now - planted) / interval` 算应处 age，到阶段即
    /// `world.set` + 缓存失效 + hub 广播 BlockUpdate（`u64::MAX` 占位 except，
    /// 全体视距内连接都收，含种植者本人）；方块已不是预期旧阶段（被挖/被改）
    /// 则静默摘除，不广播。
    /// P27：interval 按脚下土壤动态取——仍是耕地才减半（`stage_ticks / 2`），
    /// 否则普通速度（耕地被挖/被换后自动回落，无需 retime；只前进不回退）。
    fn tick_crops(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let global = self.stage_ticks.load(Ordering::Relaxed).max(1);
        let due: Vec<(
            (i32, i32, i32),
            u8,
            u64,
            u8,
            &'static mc_world::crops::CropSpec,
        )> = {
            let crops = self.crops.lock().unwrap_or_else(|p| p.into_inner());
            crops
                .iter()
                .map(|(pos, e)| {
                    let soil = world.get(pos.0, pos.1 - 1, pos.2);
                    // 生产用各自表的每阶段 tick（当前四种均为 1200，与小麦一致）；
                    // set_stage_ticks 一旦调小则对全体作物生效（测试钩子，旧语义不变）。
                    let base = if global == mc_world::WHEAT_TICKS_PER_STAGE {
                        e.crop.ticks_per_stage
                    } else {
                        global
                    }
                    .max(1);
                    let interval = if soil == mc_world::FARMLAND_BLOCK_ID {
                        (base / 2).max(1)
                    } else {
                        base
                    };
                    let want = ((now.saturating_sub(e.planted_tick)) / interval)
                        .min(e.crop.max_age as u64) as u8;
                    (*pos, e.stage, e.planted_tick, want, e.crop)
                })
                .filter(|(_, old, _, want, _)| want != old)
                .collect()
        };
        if due.is_empty() {
            return;
        }
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut updates: HashMap<(TimerKind, (i32, i32, i32)), TimerRecord> = HashMap::new();
        let mut stale = Vec::new();
        for ((x, y, z), old_stage, planted, want, crop) in due {
            let expected_old = mc_world::crop_stage_id(crop, old_stage);
            if world.get(x, y, z) != expected_old {
                // 已被挖掉或改掉：摘除跟踪，不广播（挖掘路径的 BlockUpdate 已发出）。
                self.crops.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
                stale.push((TimerKind::Crop, (x, y, z)));
                continue;
            }
            if want <= old_stage {
                continue;
            }
            let new_id = mc_world::crop_stage_id(crop, want);
            world.set(x, y, z, new_id);
            if let Some(cache) = &cache {
                cache.invalidate(mc_world::chunk_of_block(x, z));
            }
            if let Some(hub) = &hub {
                let mut body = Vec::new();
                mc_protocol::packets::BlockUpdate { x, y, z, block_id: new_id as i32 }
                    .encode_body(&mut body);
                hub.broadcast(
                    u64::MAX,
                    mc_world::chunk_of_block(x, z),
                    mc_protocol::packets::PLAY_BLOCK_UPDATE_S2C_ID,
                    &body,
                );
            }
            if let Some(e) = self.crops.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&(x, y, z))
            {
                e.stage = want;
            }
            // P39：stage 更新而 planted 不变（口径与内存一致）；同 tick 多条按 pos 去重。
            updates.insert(
                (TimerKind::Crop, (x, y, z)),
                TimerRecord {
                    kind: TimerKind::Crop,
                    pos: (x, y, z),
                    planted_tick: planted,
                    stage: want,
                    spec: timer_spec::p33(crop),
                },
            );
        }
        // 锁已全部释放才做 redb IO（P38 锁纪律）；每 tick 至多一个事务。
        self.clear_timers(&stale);
        self.persist_timers(&updates.into_values().collect::<Vec<_>>());
    }

    /// P22：种下登记（UseItemOn 种植路径调用，落子成功后）。
    /// 旧签名按小麦登记（旧调用方/单测行为不变）；P33 新作物走 [`Self::plant_crop_spec`]。
    pub fn plant_crop(&self, x: i32, y: i32, z: i32) {
        self.plant_crop_spec(&mc_world::crops::WHEAT, x, y, z);
    }

    /// P33：按作物规格登记（种植路由已从种子映射到 `crop`，不再假定小麦）。
    pub fn plant_crop_spec(
        &self,
        crop: &'static mc_world::crops::CropSpec,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let now = self.tick_count.load(Ordering::Relaxed);
        {
            self.crops.lock().unwrap_or_else(|p| p.into_inner()).insert(
                (x, y, z),
                CropEntry { planted_tick: now, stage: 0, crop },
            );
        }
        // P39：种下即写一条 timer，并把当前全局 tick 存为断点基线。
        self.persist_timers(&[TimerRecord {
            kind: TimerKind::Crop,
            pos: (x, y, z),
            planted_tick: now,
            stage: 0,
            spec: timer_spec::p33(crop),
        }]);
    }

    /// P27：某作物的当前跟踪阶段（骨粉同步/e2e 断言用；未跟踪返回 None）。
    pub fn crop_stage(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        self.crops.lock().unwrap_or_else(|p| p.into_inner()).get(&(x, y, z)).map(|e| e.stage)
    }

    /// P27：骨粉推进后同步跟踪（方块已由 `decide_bonemeal` 落子）：
    /// 已跟踪 → stage 更新为催熟后值（ticker 后续从此继续，不跳变）；
    /// 未跟踪（重启后未认领等边缘）→ 按现状补登记，避免此后生长停滞。
    /// 旧签名按小麦同步（旧调用方/单测行为不变）；P33 走 [`Self::apply_bonemeal_crop_spec`]。
    pub fn apply_bonemeal_crop(&self, x: i32, y: i32, z: i32, new_stage: u8) {
        self.apply_bonemeal_crop_spec(&mc_world::crops::WHEAT, x, y, z, new_stage);
    }

    /// P33：按作物规格同步骨粉催熟后的阶段。
    pub fn apply_bonemeal_crop_spec(
        &self,
        crop: &'static mc_world::crops::CropSpec,
        x: i32,
        y: i32,
        z: i32,
        new_stage: u8,
    ) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let rec = {
            let mut crops = self.crops.lock().unwrap_or_else(|p| p.into_inner());
            match crops.get_mut(&(x, y, z)) {
                Some(e) => {
                    e.stage = new_stage;
                    TimerRecord {
                        kind: TimerKind::Crop,
                        pos: (x, y, z),
                        planted_tick: e.planted_tick,
                        stage: new_stage,
                        spec: timer_spec::p33(crop),
                    }
                }
                None => {
                    crops.insert(
                        (x, y, z),
                        CropEntry { planted_tick: now, stage: new_stage, crop },
                    );
                    TimerRecord {
                        kind: TimerKind::Crop,
                        pos: (x, y, z),
                        planted_tick: now,
                        stage: new_stage,
                        spec: timer_spec::p33(crop),
                    }
                }
            }
        };
        // P39：骨粉推进后同步 stage（planted 不变），避免重启回到催熟前阶段。
        self.persist_timers(&[rec]);
    }

    /// P22：挖掉/移除时摘除跟踪（挖掘路径调用；ticker 内的懒摘除是兜底）。
    pub fn uproot_crop(&self, x: i32, y: i32, z: i32) {
        let removed = self.crops.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
        if removed.is_some() {
            self.clear_timer(TimerKind::Crop, (x, y, z));
        }
    }

    /// P22：跟踪中的作物数（e2e/诊断用）。
    pub fn crop_count(&self) -> usize {
        self.crops.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// P22/P33 测试钩子：覆盖每阶段 tick 数（生产值 `WHEAT_TICKS_PER_STAGE` = 1200，
    /// 即 60s；e2e/单测用小值加速，验证真实 ticker 路径而非伪造时钟）。
    /// 一旦调离生产值，对**所有**作物统一生效（四种作物生产值相同）。
    pub fn set_stage_ticks(&self, n: u64) {
        self.stage_ticks.store(n.max(1), Ordering::Relaxed);
    }

    /// P22：注入生长广播 hub（`serve` 在配置齐备时调用一次；测试可直接调）。
    pub fn set_hub(&self, hub: Arc<BroadcastHub>) {
        *self.hub.lock().unwrap_or_else(|p| p.into_inner()) = Some(hub);
    }

    /// P22：注入生长缓存失效用 cache（同上）。
    pub fn set_cache(&self, cache: Arc<ChunkCache>) {
        *self.cache.lock().unwrap_or_else(|p| p.into_inner()) = Some(cache);
    }

    /// P22 测试钩子：把某作物的种下 tick 改写为指定值（模拟流逝），返回是否命中。
    /// 生产代码不用；`set_stage_ticks` 调小间隔后一般不需要它。
    /// 注意：`planted_tick` 是绝对 tick（`tick_count` 坐标系），回拨过大—
    /// 即早于 `tick_count` 减去 `stage_ticks*7`—会直接跳到成熟，这是预期语义。
    pub fn retime_crop(&self, x: i32, y: i32, z: i32, planted_tick: u64) -> bool {
        let rec = {
            let mut crops = self.crops.lock().unwrap_or_else(|p| p.into_inner());
            match crops.get_mut(&(x, y, z)) {
                Some(e) => {
                    e.planted_tick = planted_tick;
                    Some(TimerRecord {
                        kind: TimerKind::Crop,
                        pos: (x, y, z),
                        planted_tick,
                        stage: e.stage,
                        spec: timer_spec::p33(e.crop),
                    })
                }
                None => None,
            }
        };
        if let Some(rec) = rec {
            self.persist_timers(&[rec]);
            true
        } else {
            false
        }
    }

    // -----------------------------------------------------------------------
    // P37：新植物跟踪与生长（与旧 crops/saplings 分表，旧行为不受影响）
    // -----------------------------------------------------------------------

    /// 新植物种下登记。`facing` 只对可可有意义（0..3）。
    pub fn plant_kind(
        &self,
        kind: mc_world::crops::PlantKind,
        x: i32,
        y: i32,
        z: i32,
        facing: u8,
    ) {
        let now = self.tick_count.load(Ordering::Relaxed);
        {
            self.plants.lock().unwrap_or_else(|p| p.into_inner()).insert(
                (x, y, z),
                PlantEntry { kind, planted_tick: now, stage: 0, facing, fruit_done: false },
            );
        }
        // P39：P37 五种模型统一登记，age/tree/vertical/side 各有自己的 spec 编号。
        self.persist_timers(&[Self::plant_timer(kind, (x, y, z), now, 0, false)]);
    }

    /// 新植物挖掉/连带摘除（ticker 内的懒摘除是兜底）。
    pub fn uproot_plant(&self, x: i32, y: i32, z: i32) {
        let removed = self.plants.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
        if let Some(e) = removed {
            self.clear_timer(Self::plant_timer_kind(e.kind), (x, y, z));
        }
    }

    /// 跟踪中的新植物数（e2e/诊断用）。
    pub fn plant_count(&self) -> usize {
        self.plants.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// 新植物当前阶段（age/高度；树恒 0）。
    pub fn plant_stage(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        self.plants.lock().unwrap_or_else(|p| p.into_inner()).get(&(x, y, z)).map(|e| e.stage)
    }

    /// P37 测试钩子：整体调整新植物每阶段 tick（生产值同小麦 1200）。
    pub fn set_plant_ticks(&self, n: u64) {
        self.plant_ticks.store(n.max(1), Ordering::Relaxed);
    }

    /// 骨粉推进后同步新植物跟踪（方块已由 `decide_bonemeal` 落子）。
    pub fn apply_bonemeal_plant(&self, world: &MutableWorld, x: i32, y: i32, z: i32, new_id: u16) {
        use mc_world::crops::{vertical_spec_of, PlantKind};
        let stage = match mc_world::plant_at(new_id) {
            Some((PlantKind::Age(_), age)) => age,
            Some((PlantKind::Side(_), age)) => age,
            Some((PlantKind::Vertical(spec), _)) => {
                // 竖向：从基部往上数已长段数（点击哪一段都不影响计数）。
                let mut base_y = y;
                while vertical_spec_of(world.get(x, base_y - 1, z)).map(|s| s.name) == Some(spec.name) {
                    base_y -= 1;
                }
                let mut h = 0u8;
                while vertical_spec_of(world.get(x, base_y + h as i32, z)).map(|s| s.name) == Some(spec.name) {
                    h += 1;
                }
                h.saturating_sub(1)
            }
            _ => 0,
        };
        let now = self.tick_count.load(Ordering::Relaxed);
        let rec = {
            let mut plants = self.plants.lock().unwrap_or_else(|p| p.into_inner());
            match plants.get_mut(&(x, y, z)) {
                Some(e) => {
                    e.stage = e.stage.max(stage);
                    Some(Self::plant_timer(e.kind, (x, y, z), e.planted_tick, e.stage, e.fruit_done))
                }
                None => {
                    let kind = mc_world::plant_at(new_id).map(|(k, _)| k).unwrap_or(
                        mc_world::crops::PlantKind::Age(&mc_world::crops::NETHER_WART),
                    );
                    plants.insert(
                        (x, y, z),
                        PlantEntry {
                            kind,
                            planted_tick: now,
                            stage,
                            facing: 0,
                            fruit_done: false,
                        },
                    );
                    Some(Self::plant_timer(kind, (x, y, z), now, stage, false))
                }
            }
        };
        // P39：骨粉后 planted 不变、stage 与内存同步，重启不回退到催熟前。
        if let Some(rec) = rec {
            self.persist_timers(&[rec]);
        }
    }

    /// P37：按物种规格立即长树（骨粉/ticker 共用），土壤与占位口径同 P26。
    pub fn grow_tree_kind_now(
        &self,
        world: &MutableWorld,
        x: i32,
        y: i32,
        z: i32,
        spec: &'static mc_world::crops::TreeSpec,
    ) -> bool {
        if world.get(x, y, z) != spec.sapling_id {
            self.uproot_plant(x, y, z);
            return false;
        }
        if !mc_world::is_crop_soil(world.get(x, y - 1, z)) {
            self.uproot_plant(x, y, z);
            return false;
        }
        let mut placed = Vec::new();
        for (pos, id) in mc_world::crops::sapling_tree_blocks(spec, x, y, z) {
            if pos == (x, y, z) || world.get(pos.0, pos.1, pos.2) == mc_world::AIR_BLOCK_ID {
                world.set(pos.0, pos.1, pos.2, id);
                placed.push((pos, id));
            }
        }
        let mut chunks = std::collections::HashSet::new();
        for ((bx, _, bz), _) in &placed {
            chunks.insert(mc_world::chunk_of_block(*bx, *bz));
        }
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(cache) = &cache {
            for c in chunks {
                cache.invalidate(c);
            }
        }
        if let Some(hub) = &hub {
            for ((bx, by, bz), id) in &placed {
                self.emit_block_update(&hub, *bx, *by, *bz, *id);
            }
        }
        self.uproot_plant(x, y, z);
        true
    }

    fn emit_block_update(&self, hub: &std::sync::Arc<BroadcastHub>, x: i32, y: i32, z: i32, id: u16) {
        let mut body = Vec::new();
        mc_protocol::packets::BlockUpdate { x, y, z, block_id: id as i32 }.encode_body(&mut body);
        hub.broadcast(
            u64::MAX,
            mc_world::chunk_of_block(x, z),
            mc_protocol::packets::PLAY_BLOCK_UPDATE_S2C_ID,
            &body,
        );
    }

    /// 新植物单格成长的缓存失效 + hub 广播。
    fn commit_plant_block(&self, x: i32, y: i32, z: i32, id: u16) {
        if let Some(cache) = self.cache.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            cache.invalidate(mc_world::chunk_of_block(x, z));
        }
        if let Some(hub) = self.hub.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            self.emit_block_update(&hub, x, y, z, id);
        }
    }

    /// P37 生长间隔：生产用各自规格；测试钩子一旦调离生产值则全体统一生效。
    /// 耕地加速只对原本适用的 age 作物开启（瓜梗/南瓜梗/火把花/猪笼草）。
    fn plant_interval(&self, spec_ticks: u64, farmland_boost: bool, soil: u16) -> u64 {
        let global = self.plant_ticks.load(Ordering::Relaxed);
        let base = if global == mc_world::WHEAT_TICKS_PER_STAGE { spec_ticks } else { global }.max(1);
        if farmland_boost && soil == mc_world::FARMLAND_BLOCK_ID {
            (base / 2).max(1)
        } else {
            base
        }
    }

    /// 全服唯一 50ms ticker 内推进全部新植物；连接侧绝不调用。
    fn tick_plants(&self, world: &MutableWorld) {
        use mc_world::crops::{
            age_for_elapsed, age_stage_id, cocoa_state, fruit_neighbors, pitcher_states,
            vertical_spec_of, PlantKind,
        };
        let now = self.tick_count.load(Ordering::Relaxed);
        let entries: Vec<((i32, i32, i32), PlantEntry)> = self
            .plants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|(pos, e)| (*pos, *e))
            .collect();
        // P39：同一 tick 的多次推进按 pos 去重，循环结束后一次性写 redb。
        let mut updates: HashMap<(TimerKind, (i32, i32, i32)), TimerRecord> = HashMap::new();
        for ((x, y, z), e) in entries {
            match e.kind {
                PlantKind::Age(spec) => {
                    let old_id = if spec.name == "pitcher_crop" {
                        pitcher_states(e.stage).0
                    } else {
                        age_stage_id(spec, e.stage)
                    };
                    if world.get(x, y, z) != old_id {
                        self.uproot_plant(x, y, z);
                        continue;
                    }
                    let interval = self.plant_interval(
                        spec.ticks_per_stage,
                        spec.farmland_boost,
                        world.get(x, y - 1, z),
                    );
                    let want = age_for_elapsed(now - e.planted_tick, interval, spec.max_age);
                    if want > e.stage {
                        let new_id = age_stage_id(spec, want);
                        world.set(x, y, z, new_id);
                        self.commit_plant_block(x, y, z, new_id);
                        if spec.name == "pitcher_crop" {
                            let (_, upper) = pitcher_states(want);
                            if world.get(x, y + 1, z) == mc_world::AIR_BLOCK_ID {
                                world.set(x, y + 1, z, upper);
                                self.commit_plant_block(x, y + 1, z, upper);
                            }
                        }
                        let mut grown = e;
                        grown.stage = want;
                        if let Some(slot) = self.plants.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&(x, y, z)) {
                            *slot = grown;
                        }
                        updates.insert(
                            (TimerKind::Crop, (x, y, z)),
                            Self::plant_timer(PlantKind::Age(spec), (x, y, z), e.planted_tick, want, e.fruit_done),
                        );
                    }
                    // 瓜果：成熟后在邻格落整块瓜（每株仅一次）。
                    if !e.fruit_done && want >= spec.max_age {
                        if let Some(fruit) = spec.fruit {
                            if let Some((fx, fy, fz)) = fruit_neighbors(x, y, z)
                                .into_iter()
                                .find(|p| mc_world::is_replaceable(world.get(p.0, p.1, p.2)))
                            {
                                world.set(fx, fy, fz, fruit.block_id);
                                self.commit_plant_block(fx, fy, fz, fruit.block_id);
                                if let Some(slot) =
                                    self.plants.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&(x, y, z))
                                {
                                    slot.fruit_done = true;
                                }
                                updates.insert(
                                    (TimerKind::Crop, (x, y, z)),
                                    Self::plant_timer(PlantKind::Age(spec), (x, y, z), e.planted_tick, want, true),
                                );
                            }
                        }
                    }
                }
                PlantKind::Tree(spec) => {
                    let interval = self.plant_interval(spec.grow_ticks, false, world.get(x, y - 1, z));
                    if now.saturating_sub(e.planted_tick) >= interval {
                        self.grow_tree_kind_now(world, x, y, z, spec);
                    } else if world.get(x, y, z) != spec.sapling_id {
                        self.uproot_plant(x, y, z);
                    }
                }
                PlantKind::Vertical(spec) => {
                    if vertical_spec_of(world.get(x, y, z)).map(|s| s.name) != Some(spec.name) {
                        self.uproot_plant(x, y, z);
                        continue;
                    }
                    if e.stage >= spec.max_height - 1 {
                        continue;
                    }
                    let interval = self.plant_interval(spec.ticks_per_stage, false, world.get(x, y - 1, z));
                    if now.saturating_sub(e.planted_tick) < interval {
                        continue;
                    }
                    // stage=0 表示只有基部，新一段落在 y+1；stage=1 落在 y+2。
                    let (nx, ny, nz) = (x, y + e.stage as i32 + 1, z);
                    if world.get(nx, ny, nz) != mc_world::AIR_BLOCK_ID {
                        continue;
                    }
                    let new_id = (spec.state_fn)(e.stage);
                    world.set(nx, ny, nz, new_id);
                    self.commit_plant_block(nx, ny, nz, new_id);
                    if let Some(slot) = self.plants.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&(x, y, z)) {
                        slot.stage += 1;
                        slot.planted_tick = now;
                    }
                    // P39：竖著作物每长一段把 planted 重置为 now（与内存同口径）。
                    updates.insert(
                        (TimerKind::Crop, (x, y, z)),
                        Self::plant_timer(PlantKind::Vertical(spec), (x, y, z), now, e.stage + 1, false),
                    );
                }
                PlantKind::Side(spec) => {
                    if world.get(x, y, z) != cocoa_state(e.stage, e.facing) {
                        self.uproot_plant(x, y, z);
                        continue;
                    }
                    let (sx, sy, sz) = mc_world::crops::cocoa_support_pos(x, y, z, e.facing);
                    if world.get(sx, sy, sz) != spec.support_id {
                        self.uproot_plant(x, y, z);
                        continue;
                    }
                    let interval = self.plant_interval(spec.ticks_per_stage, false, world.get(sx, sy, sz));
                    let want = age_for_elapsed(now - e.planted_tick, interval, spec.max_age);
                    if want > e.stage {
                        let new_id = cocoa_state(want, e.facing);
                        world.set(x, y, z, new_id);
                        self.commit_plant_block(x, y, z, new_id);
                        if let Some(slot) = self.plants.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&(x, y, z)) {
                            slot.stage = want;
                        }
                        updates.insert(
                            (TimerKind::Crop, (x, y, z)),
                            Self::plant_timer(PlantKind::Side(spec), (x, y, z), e.planted_tick, want, false),
                        );
                    }
                }
            }
        }
        // P39：锁已释放，才把本 tick 的阶段推进合成一个事务写盘。
        self.persist_timers(&updates.into_values().collect::<Vec<_>>());
    }

    /// P26：种下树苗登记（UseItemOn 种植路径调用，落子成功后）。
    pub fn plant_sapling(&self, x: i32, y: i32, z: i32) {
        let now = self.tick_count.load(Ordering::Relaxed);
        {
            self.saplings
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert((x, y, z), now);
        }
        // P39：橡树苗 spec=0（P37 其它树种走 plants 表，spec 带 TREE tag）。
        self.persist_timers(&[TimerRecord {
            kind: TimerKind::Sapling,
            pos: (x, y, z),
            planted_tick: now,
            stage: 0,
            spec: 0,
        }]);
    }

    /// P26：树苗被挖/所在土被挖时摘除跟踪（挖掘路径调用；ticker 懒摘除是兜底）。
    pub fn uproot_sapling(&self, x: i32, y: i32, z: i32) {
        let removed = self.saplings.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
        if removed.is_some() {
            self.clear_timer(TimerKind::Sapling, (x, y, z));
        }
    }

    /// P26：跟踪中的树苗数（e2e/诊断用）。
    pub fn sapling_count(&self) -> usize {
        self.saplings.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// P26 测试钩子：覆盖树苗生长 tick 数（生产 `SAPLING_GROW_TICKS` = 7200 ≈ 6min）。
    pub fn set_sapling_ticks(&self, n: u64) {
        self.sapling_ticks.store(n.max(1), Ordering::Relaxed);
    }

    /// P27：立即长树（骨粉催熟用；ticker 到期长树复用同一函数）：
    /// - 树苗格已不是树苗（被挖/被改）→ 摘除跟踪，返回 false；
    /// - 土壤（y-1）不是草/泥土/耕地 → 摘除跟踪，返回 false；
    /// - 否则按 `mc_world::sapling_oak_blocks` 落子（树苗格恒变原木，其余只在空气时放，
    ///   被占跳过不覆盖玩家建筑），受影响 chunk 失效，多格变更逐格 hub 广播，
    ///   摘除跟踪，返回 true。
    pub fn grow_sapling_now(&self, world: &MutableWorld, x: i32, y: i32, z: i32) -> bool {
        if world.get(x, y, z) != mc_world::OAK_SAPLING_BLOCK_ID {
            self.uproot_sapling(x, y, z);
            return false;
        }
        let soil = world.get(x, y - 1, z);
        if !mc_world::is_crop_soil(soil) {
            self.uproot_sapling(x, y, z);
            return false;
        }
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut placed: Vec<((i32, i32, i32), u16)> = Vec::new();
        for (pos, id) in mc_world::sapling_oak_blocks(x, y, z) {
            if pos == (x, y, z) {
                world.set(pos.0, pos.1, pos.2, id);
                placed.push((pos, id));
            } else if world.get(pos.0, pos.1, pos.2) == mc_world::AIR_BLOCK_ID {
                world.set(pos.0, pos.1, pos.2, id);
                placed.push((pos, id));
            }
        }
        if let Some(cache) = &cache {
            use std::collections::HashSet;
            let mut chunks: HashSet<mc_core::ChunkPos> = HashSet::new();
            for ((bx, _, bz), _) in &placed {
                chunks.insert(mc_world::chunk_of_block(*bx, *bz));
            }
            for c in chunks {
                cache.invalidate(c);
            }
        }
        if let Some(hub) = &hub {
            for ((bx, by, bz), id) in &placed {
                let mut body = Vec::new();
                mc_protocol::packets::BlockUpdate { x: *bx, y: *by, z: *bz, block_id: *id as i32 }
                    .encode_body(&mut body);
                hub.broadcast(
                    u64::MAX,
                    mc_world::chunk_of_block(*bx, *bz),
                    mc_protocol::packets::PLAY_BLOCK_UPDATE_S2C_ID,
                    &body,
                );
            }
        }
        self.uproot_sapling(x, y, z);
        true
    }

    /// P26：树苗到期长成小橡树（全服唯一 ticker 内，连接侧绝不调用）。
    /// 到期条件：`now - planted >= sapling_ticks`；落子走 [`Self::grow_sapling_now`]。
    fn tick_saplings(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let interval = self.sapling_ticks.load(Ordering::Relaxed).max(1);
        let due: Vec<(i32, i32, i32)> = {
            let saplings = self.saplings.lock().unwrap_or_else(|p| p.into_inner());
            saplings
                .iter()
                .filter(|(_, planted)| now.saturating_sub(**planted) >= interval)
                .map(|(pos, _)| *pos)
                .collect()
        };
        if due.is_empty() {
            return;
        }
        for (x, y, z) in due {
            self.grow_sapling_now(world, x, y, z);
        }
    }

    /// P27 测试钩子：覆盖树叶 decay 延迟（生产 `LEAF_DECAY_TICKS` = 100 ≈ 5s）。
    pub fn set_leaf_decay_ticks(&self, n: u64) {
        self.leaf_decay_ticks.store(n.max(1), Ordering::Relaxed);
    }

    /// P27：decay 观察中的树叶数（e2e/诊断用）。
    pub fn leaf_pending_count(&self) -> usize {
        self.pending_leaves.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// P27：原木被挖后，把周围 [`mc_world::LEAF_DECAY_RADIUS`] 内树叶纳入 decay 观察
    ///（挖掘路径调用；`or_insert` 保留最早登记时刻，多次挖同一片不续命）。
    pub fn schedule_leaf_decay_around(&self, world: &MutableWorld, x: i32, y: i32, z: i32) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let r = mc_world::LEAF_DECAY_RADIUS;
        let mut added = Vec::new();
        {
            let mut pending = self.pending_leaves.lock().unwrap_or_else(|p| p.into_inner());
            for dx in -r..=r {
                for dy in -r..=r {
                    for dz in -r..=r {
                        let p = (x + dx, y + dy, z + dz);
                        let id = world.get(p.0, p.1, p.2);
                        if id == mc_world::OAK_LEAVES_BLOCK_ID
                            || mc_world::crops::tree_spec_for_leaves(id).is_some()
                        {
                            // or_insert 保留最早登记时刻，多次挖同一片不续命。
                            if let std::collections::hash_map::Entry::Vacant(e) = pending.entry(p) {
                                e.insert(now);
                                added.push(p);
                            }
                        }
                    }
                }
            }
        }
        // P39：新登记的观察写 timer；已有观察不续命、不重写。
        self.persist_timers(&added.into_iter().map(|pos| TimerRecord {
            kind: TimerKind::Leaf,
            pos,
            planted_tick: now,
            stage: 0,
            spec: 0,
        }).collect::<Vec<_>>());
    }

    /// P27：半径内是否有橡木原木（有即有支撑，不 decay；简化判定，见 HANDOFF §5）。
    fn has_log_nearby(world: &MutableWorld, x: i32, y: i32, z: i32) -> bool {
        let r = mc_world::LEAF_DECAY_RADIUS;
        for dx in -r..=r {
            for dy in -r..=r {
                for dz in -r..=r {
                    let id = world.get(x + dx, y + dy, z + dz);
                    if id == mc_world::OAK_LOG_BLOCK_ID
                        || mc_world::crops::tree_spec_for_log(id).is_some()
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// P27：失支撑树叶随全局 ticker 消失（连接侧绝不调用）：
    /// - 已不是树叶（被挖/被改）→ 摘除观察；半径内有原木 → 有支撑，摘除观察；
    /// - 否则登记满 `leaf_decay_ticks` 后：设空气 + decay 掉落生成实体
    ///  （`leaf_decay_drops`：树苗/木棍口径 + 1/200 苹果）+ 缓存失效 + hub 广播。
    ///   掉落实体靠各连接 `tick_drops` 的视距补发自动同步（ticker 内无 socket，不直接发包）。
    fn tick_leaves(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let delay = self.leaf_decay_ticks.load(Ordering::Relaxed).max(1);
        let due: Vec<(i32, i32, i32)> = {
            let pending = self.pending_leaves.lock().unwrap_or_else(|p| p.into_inner());
            pending
                .iter()
                .filter(|(_, since)| now.saturating_sub(**since) >= delay)
                .map(|(pos, _)| *pos)
                .collect()
        };
        if due.is_empty() {
            return;
        }
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut stale = Vec::new();
        for (x, y, z) in due {
            let leaf = world.get(x, y, z);
            let is_leaf = leaf == mc_world::OAK_LEAVES_BLOCK_ID
                || mc_world::crops::tree_spec_for_leaves(leaf).is_some();
            if !is_leaf {
                self.pending_leaves.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
                stale.push((TimerKind::Leaf, (x, y, z)));
                continue;
            }
            if Self::has_log_nearby(world, x, y, z) {
                self.pending_leaves.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
                stale.push((TimerKind::Leaf, (x, y, z)));
                continue;
            }
            world.set(x, y, z, mc_world::AIR_BLOCK_ID);
            // 橡树沿用 P27 苹果口径；新树种按各自树叶掉落（对应树苗/木棍）。
            let decay = if leaf == mc_world::OAK_LEAVES_BLOCK_ID {
                mc_world::drops::leaf_decay_drops(x, y, z, world.seed())
            } else {
                mc_world::drops_for_block(leaf, x, y, z, world.seed())
            };
            for d in decay {
                let rest = world.drop_rest_y(x, z, y) as f64;
                let mut items = self.items.lock().unwrap_or_else(|p| p.into_inner());
                let _ = items.spawn(
                    self.alloc_id(),
                    d.item_id,
                    d.count,
                    x as f64 + 0.5,
                    y as f64 + 0.5,
                    z as f64 + 0.5,
                    rest,
                );
            }
            if let Some(cache) = &cache {
                cache.invalidate(mc_world::chunk_of_block(x, z));
            }
            if let Some(hub) = &hub {
                let mut body = Vec::new();
                mc_protocol::packets::BlockUpdate { x, y, z, block_id: mc_world::AIR_BLOCK_ID as i32 }
                    .encode_body(&mut body);
                hub.broadcast(
                    u64::MAX,
                    mc_world::chunk_of_block(x, z),
                    mc_protocol::packets::PLAY_BLOCK_UPDATE_S2C_ID,
                    &body,
                );
            }
            self.pending_leaves.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
            stale.push((TimerKind::Leaf, (x, y, z)));
        }
        // P39：观察结束（decay/被挖/恢复支撑）都删 timer，重启不再重复 decay。
        self.clear_timers(&stale);
    }

    /// 分配全局唯一 entity_id（永不复用）。
    pub fn alloc_id(&self) -> i32 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    fn lock_invs(&self) -> std::sync::MutexGuard<'_, HashMap<u64, PlayerInventory>> {
        self.inventories.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn lock_known(&self) -> std::sync::MutexGuard<'_, HashMap<u64, HashSet<i32>>> {
        self.known.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn lock_tags(&self) -> std::sync::MutexGuard<'_, HashMap<u64, String>> {
        self.player_tags.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 把一份背包写到当前后端（redb 一个事务写 players + journal）。
    fn persist_inventory(&self, user_tag: &str, inv: &PlayerInventory) {
        if let Some(store) = &self.store {
            if let Err(e) = store.save_player(user_tag, inv) {
                warn!("redb player save failed tag={user_tag} seed={} ({e})", self.seed);
            }
        } else if let Err(e) = inv.save_to_dir(&self.save_dir, user_tag, self.seed) {
            warn!("player save failed tag={user_tag} seed={} ({e})", self.seed);
        }
    }

    /// Join 时调入背包：读盘命中则用盘档，否则新背包 + 石头开局（后续替换为生存模式）。
    pub fn load_inventory(&self, conn_id: u64, user_tag: &str) -> (usize, u8) {
        let (mut inv, is_new) = match &self.store {
            Some(store) => {
                let found = store.load_player(user_tag).unwrap_or_else(|e| {
                    warn!("redb player load failed tag={user_tag} seed={} ({e})", self.seed);
                    None
                });
                // 无记录 = 旧档缺文件；已有记录即使背包空也不能再发石头。
                let is_new = found.is_none();
                (found.unwrap_or_default(), is_new)
            }
            None => PlayerInventory::load_from_dir(&self.save_dir, user_tag, self.seed)
                .unwrap_or_else(|_| (PlayerInventory::new(), true)),
        };
        if is_new && inv.is_empty() {
            inv.slots_hotbar_starter();
        }
        let held = inv.selected();
        let filled = inv.filled_count();
        self.lock_tags().insert(conn_id, user_tag.to_string());
        self.lock_invs().insert(conn_id, inv);
        self.lock_known().insert(conn_id, HashSet::new());
        let _ = (filled, held);
        (filled, held)
    }

    /// 断线时刷盘 + 释放连接状态（known/背包内存；手持栈先收回背包再存）。
    pub fn unload_inventory(&self, conn_id: u64, user_tag: &str) {
        self.lock_tags().remove(&conn_id);
        if let Some(mut inv) = self.lock_invs().remove(&conn_id) {
            inv.stow_cursor();
            self.persist_inventory(user_tag, &inv);
        }
        self.lock_known().remove(&conn_id);
    }

    /// 背包刷盘（拾取/放置后调用）。
    pub fn save_inventory(&self, conn_id: u64, user_tag: &str) {
        if let Some(inv) = self.lock_invs().get(&conn_id) {
            self.persist_inventory(user_tag, inv);
        }
    }

    /// 60s 周期/退出时把全部在线背包 + XP 刷到当前后端（redb 模式一次一人一事务）。
    pub fn save_all_inventories(&self) {
        if self.store.is_none() {
            return;
        }
        let tick = self.tick_count.load(Ordering::Relaxed);
        {
            let tags = self.lock_tags();
            let invs = self.lock_invs();
            for (conn_id, inv) in invs.iter() {
                if let Some(tag) = tags.get(conn_id) {
                    self.persist_inventory(tag, inv);
                }
            }
        }
        // P39：60s/退出也刷一次 tick 基线（即使这期间没有任何 timer 变化），
        // 使重启后的 now 尽量贴近停机时刻；timer 写入本身已同事务更新基线。
        if let Some(store) = &self.store {
            if let Err(e) = store.set_tick_offset(tick) {
                warn!("redb tick_offset save failed seed={} tick={tick} ({e})", self.seed);
            }
        }
    }

    /// 对背包加独占锁执行闭包（调用方负责发包/刷盘）。
    pub fn with_inventory<R>(&self, conn_id: u64, f: impl FnOnce(&mut PlayerInventory) -> R) -> Option<R> {
        self.lock_invs().get_mut(&conn_id).map(f)
    }

    pub fn mark_known(&self, conn_id: u64, id: i32) {
        self.lock_known().entry(conn_id).or_default().insert(id);
    }

    pub fn unmark_known(&self, conn_id: u64, id: i32) {
        if let Some(set) = self.lock_known().get_mut(&conn_id) {
            set.remove(&id);
        }
    }

    pub fn is_known(&self, conn_id: u64, id: i32) -> bool {
        self.lock_known().get(&conn_id).map(|s| s.contains(&id)).unwrap_or(false)
    }

    pub fn known_ids(&self, conn_id: u64) -> Vec<i32> {
        self.lock_known().get(&conn_id).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
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
}
