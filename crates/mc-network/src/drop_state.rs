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

/// 全服掉落物物理节拍（ms）：只由 [`DropState::start_global_tick`] 的一个 ticker 驱动。
pub const DROP_TICK_MS: u64 = 50;

/// 掉落物 entity_id 起始（避开 LoginPlay entity_id=1）。
pub const FIRST_DROP_ENTITY_ID: i32 = 1000;
/// 拾取动画 collector（本服 LoginPlay entity_id 恒为 1）。
pub const PLAYER_ENTITY_ID: i32 = 1;
/// 新背包开局石头数量（ hotbar 槽 36），保证“放置扣 1”可验证；后续按生存模式替换。
pub const STARTER_STONE_COUNT: i32 = mc_world::inventory::STARTER_COUNT;

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
            tick_started: AtomicBool::new(false),
            stop_flag: Mutex::new(None),
            save_dir,
            seed,
        }
    }

    pub fn seed(&self) -> i64 {
        self.seed
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
}
