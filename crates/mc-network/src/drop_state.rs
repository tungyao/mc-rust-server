//! drop_state: 掉落物实体 + 背包的 net 线程共享状态，只兼容 26.3/777。
//!
//! 设计（P17 模式的延续）：socket 仍在 net 线程，ECS 没有写通道，因此掉落物的
//! 生成/同步/拾取判定与背包同步都在 net 线程完成（权威逻辑见
//! `mc_world::{ItemEntities, PlayerInventory}` 纯逻辑模块，本文件只做
//! 并发容器 + 存档 + 发包所需的派生状态）。
//!
//! - `items`：全服唯一的 `ItemEntities`（entity_id 全局自增，`next_id` 从 1000 起，
//!   与 LoginPlay entity_id=1 不冲突，永不复用，重启后从 1000 重新开始）。
//! - `inventories`：conn_id → 背包；Join 时按用户名读盘（缺文件=新背包+石头开局），
//!   拾取/放置/断线时刷盘（`player-<user>-<seed>.dat`，seed 隔离；键用用户名而非 uuid，
//!   因离线模式 uuid 每次随机，见 `mc_world::inventory` 文档）。
//! - `known`：conn_id → 该连接已见实体（视距过滤：离开视距发 Remove，进入补发）。

use mc_world::{ItemEntities, MutableWorld, PlayerInventory};
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

/// 全服掉落物物理节拍（ms）：只由 [`DropState::start_global_tick`] 的一个 ticker 驱动。
pub const DROP_TICK_MS: u64 = 50;

/// 掉落物 entity_id 起始（避开 LoginPlay entity_id=1）。
pub const FIRST_DROP_ENTITY_ID: i32 = 1000;
/// 拾取动画 collector（本服 LoginPlay entity_id 恒为 1）。
pub const PLAYER_ENTITY_ID: i32 = 1;
/// 新背包开局石头数量（ hotbar 槽 36），保证“放置扣 1”可验证；后续按生存模式替换。
pub const STARTER_STONE_COUNT: i32 = mc_world::inventory::STARTER_COUNT;

/// P22 作物条目：种下时的全局 tick + 上次已广播的 age。
/// 生长只由全服唯一 ticker 推进（P18.1 R1：连接侧绝不 tick），阶段变更经
/// `MutableWorld::set` 落子并走 hub 广播 fan-out；挖掉后由挖掘路径显式摘除。
#[derive(Debug, Clone, Copy)]
struct CropEntry {
    planted_tick: u64,
    stage: u8,
}

/// net 线程共享的掉落/背包状态（`Arc` 进 `PlayOptions`）。
///
/// 物理权威（下落/age/despawn/支撑重算）只由 [`Self::start_global_tick`] 启动的
/// **单个** ticker 驱动，与本文件的连接态无关；每个连接只做拾取/视距同步。
#[derive(Debug)]
pub struct DropState {
    pub items: Mutex<ItemEntities>,
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
}

impl DropState {
    pub fn new(save_dir: PathBuf, seed: i64) -> Self {
        Self {
            items: Mutex::new(ItemEntities::new()),
            inventories: Mutex::new(HashMap::new()),
            known: Mutex::new(HashMap::new()),
            next_id: AtomicI32::new(FIRST_DROP_ENTITY_ID),
            tick_count: AtomicU64::new(0),
            crops: Mutex::new(HashMap::new()),
            stage_ticks: AtomicU64::new(mc_world::WHEAT_TICKS_PER_STAGE),
            saplings: Mutex::new(HashMap::new()),
            sapling_ticks: AtomicU64::new(mc_world::SAPLING_GROW_TICKS),
            hub: Mutex::new(None),
            cache: Mutex::new(None),
            tick_started: AtomicBool::new(false),
            stop_flag: Mutex::new(None),
            save_dir,
            seed,
        }
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
        // P26 树苗生长：同一 ticker 内推进，连接侧绝不碰（P18.1 R1 不重犯）。
        self.tick_saplings(world);
    }

    /// P22：按 `(now - planted) / WHEAT_TICKS_PER_STAGE` 算应处 age，到阶段即
    /// `world.set` + 缓存失效 + hub 广播 BlockUpdate（`u64::MAX` 占位 except，
    /// 全体视距内连接都收，含种植者本人）；方块已不是预期旧阶段（被挖/被改）
    /// 则静默摘除，不广播。
    fn tick_crops(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let interval = self.stage_ticks.load(Ordering::Relaxed).max(1);
        let due: Vec<((i32, i32, i32), u8, u8)> = {
            let crops = self.crops.lock().unwrap_or_else(|p| p.into_inner());
            crops
                .iter()
                .map(|(pos, e)| {
                    let want = ((now.saturating_sub(e.planted_tick)) / interval)
                        .min(mc_world::WHEAT_MAX_AGE as u64) as u8;
                    (*pos, e.stage, want)
                })
                .filter(|(_, old, want)| want != old)
                .collect()
        };
        if due.is_empty() {
            return;
        }
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner()).clone();
        for ((x, y, z), old_stage, want) in due {
            let expected_old = mc_world::wheat_stage_id(old_stage);
            if world.get(x, y, z) != expected_old {
                // 已被挖掉或改掉：摘除跟踪，不广播（挖掘路径的 BlockUpdate 已发出）。
                self.crops.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
                continue;
            }
            if want <= old_stage {
                continue;
            }
            let new_id = mc_world::wheat_stage_id(want);
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
        }
    }

    /// P22：种下登记（UseItemOn 种植路径调用，落子成功后）。
    pub fn plant_crop(&self, x: i32, y: i32, z: i32) {
        self.crops.lock().unwrap_or_else(|p| p.into_inner()).insert(
            (x, y, z),
            CropEntry { planted_tick: self.tick_count.load(Ordering::Relaxed), stage: 0 },
        );
    }

    /// P22：挖掉/移除时摘除跟踪（挖掘路径调用；ticker 内的懒摘除是兜底）。
    pub fn uproot_crop(&self, x: i32, y: i32, z: i32) {
        self.crops.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
    }

    /// P22：跟踪中的作物数（e2e/诊断用）。
    pub fn crop_count(&self) -> usize {
        self.crops.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// P22 测试钩子：覆盖每阶段 tick 数（生产值 `WHEAT_TICKS_PER_STAGE` = 1200，
    /// 即 60s；e2e/单测用小值加速，验证真实 ticker 路径而非伪造时钟）。
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

    /// P22：重启后把存量小麦重新纳入生长（stage 按存盘 age，planted 按现在，
    /// 即从重启时刻起重新计时满阶段；与 Anvil 的差距见 HANDOFF §5）。
    pub fn adopt_existing_crops(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let mut crops = self.crops.lock().unwrap_or_else(|p| p.into_inner());
        for ((x, y, z), id) in world.wheat_blocks() {
            if let Some(age) = mc_world::wheat_age(id) {
                crops
                    .entry((x, y, z))
                    .or_insert(CropEntry { planted_tick: now, stage: age });
            }
        }
    }

    /// P22 测试钩子：把某作物的种下 tick 改写为指定值（模拟流逝），返回是否命中。
    /// 生产代码不用；`set_stage_ticks` 调小间隔后一般不需要它。
    /// 注意：`planted_tick` 是绝对 tick（`tick_count` 坐标系），回拨过大—
    /// 即早于 `tick_count` 减去 `stage_ticks*7`—会直接跳到成熟，这是预期语义。
    pub fn retime_crop(&self, x: i32, y: i32, z: i32, planted_tick: u64) -> bool {
        match self.crops.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&(x, y, z)) {
            Some(e) => {
                e.planted_tick = planted_tick;
                true
            }
            None => false,
        }
    }

    /// P26：种下树苗登记（UseItemOn 种植路径调用，落子成功后）。
    pub fn plant_sapling(&self, x: i32, y: i32, z: i32) {
        self.saplings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert((x, y, z), self.tick_count.load(Ordering::Relaxed));
    }

    /// P26：树苗被挖/所在土被挖时摘除跟踪（挖掘路径调用；ticker 懒摘除是兜底）。
    pub fn uproot_sapling(&self, x: i32, y: i32, z: i32) {
        self.saplings.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
    }

    /// P26：跟踪中的树苗数（e2e/诊断用）。
    pub fn sapling_count(&self) -> usize {
        self.saplings.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// P26 测试钩子：覆盖树苗生长 tick 数（生产 `SAPLING_GROW_TICKS` = 7200 ≈ 6min）。
    pub fn set_sapling_ticks(&self, n: u64) {
        self.sapling_ticks.store(n.max(1), Ordering::Relaxed);
    }

    /// P26：重启后把存量树苗重新纳入生长（从重启时刻重计时；与小麦同口径）。
    pub fn adopt_existing_saplings(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let mut saplings = self.saplings.lock().unwrap_or_else(|p| p.into_inner());
        for pos in world.sapling_blocks() {
            saplings.entry(pos).or_insert(now);
        }
    }

    /// P26：树苗到期长成小橡树（全服唯一 ticker 内，连接侧绝不调用）。
    /// - 到期条件：`now - planted >= sapling_ticks`；
    /// - 树苗格已不是树苗（被挖/被改）→ 静默摘除；土壤（y-1）不是草/泥土 → 取消生长摘除；
    /// - 树形经 `mc_world::sapling_oak_blocks` 计算（trunk 在前、leaves 在后）：
    ///   树苗格本身恒替换为原木，其余格只在空气时放（被占跳过，不覆盖玩家建筑）；
    /// - 每格经 `MutableWorld::set` 落子，受影响 chunk 失效，多格变更逐格 hub 广播。
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
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let cache = self.cache.lock().unwrap_or_else(|p| p.into_inner()).clone();
        for (x, y, z) in due {
            if world.get(x, y, z) != mc_world::OAK_SAPLING_BLOCK_ID {
                self.saplings.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
                continue;
            }
            let soil = world.get(x, y - 1, z);
            if soil != mc_world::GRASS_BLOCK_ID && soil != mc_world::DIRT_BLOCK_ID {
                self.saplings.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
                continue;
            }
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
            self.saplings.lock().unwrap_or_else(|p| p.into_inner()).remove(&(x, y, z));
        }
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

    /// Join 时调入背包：读盘命中则用盘档，否则新背包 + 石头开局（后续替换为生存模式）。
    pub fn load_inventory(&self, conn_id: u64, user_tag: &str) -> (usize, u8) {
        let (mut inv, is_new) =
            PlayerInventory::load_from_dir(&self.save_dir, user_tag, self.seed).unwrap_or_else(
                |_| (PlayerInventory::new(), true),
            );
        if is_new && inv.is_empty() {
            inv.slots_hotbar_starter();
        }
        let held = inv.selected();
        let filled = inv.filled_count();
        self.lock_invs().insert(conn_id, inv);
        self.lock_known().insert(conn_id, HashSet::new());
        let _ = (filled, held);
        (filled, held)
    }

    /// 断线时刷盘 + 释放连接状态（known/背包内存；手持栈先收回背包再存）。
    pub fn unload_inventory(&self, conn_id: u64, user_tag: &str) {
        if let Some(mut inv) = self.lock_invs().remove(&conn_id) {
            inv.stow_cursor();
            let _ = inv.save_to_dir(&self.save_dir, user_tag, self.seed);
        }
        self.lock_known().remove(&conn_id);
    }

    /// 背包刷盘（拾取/放置后调用）。
    pub fn save_inventory(&self, conn_id: u64, user_tag: &str) {
        if let Some(inv) = self.lock_invs().get(&conn_id) {
            let _ = inv.save_to_dir(&self.save_dir, user_tag, self.seed);
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
}
