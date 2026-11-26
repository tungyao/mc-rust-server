//! P46 v2 流动水：全服唯一 50ms ticker（P18.1 R1，绝不每连接 tick）。
//!
//! 只改 `MutableWorld`（override + dirty + redb 刷盘）与 `ChunkCache::invalidate`；
//! 每次实际方块变更向视距内连接广播 `BlockUpdate 0x08`（复用 P21 hub，`except=0`
//! 因为流体没有操作者；玩家挖/放的 Ack 仍由 `block_edit` 只回操作者本人）。
//! 规则来自 `mc_world::fluid`（简化，与原版差异见该模块头注释；需真机复验）。
//!
//! 写风暴防护：每拍最多处理 [`MAX_TASKS_PER_TICK`] 个任务、最多落
//! [`MAX_WRITES_PER_TICK`] 个方块；未按配额处理的任务重新入队，下一拍继续。
//! 传播距离上限 `FLOW_SPREAD_MAX=8`，source 只填相邻空气，不凭空造水。

use mc_protocol::packets;
use mc_world::{
    chunk_of_block, fluid as rules, GeneratorVersion, MutableWorld, AIR_BLOCK_ID,
};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::warn;

/// 全服流体节拍（ms）：与掉落物理同为 50ms/20Hz。
pub const FLUID_TICK_MS: u64 = 50;
/// 每拍最多出队的传播/重算任务数（防止单拍写风暴）。
pub const MAX_TASKS_PER_TICK: usize = 64;
/// 每拍最多实际落子数（超出重排队，下一拍继续）。
pub const MAX_WRITES_PER_TICK: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TaskKind {
    /// 从当前水源向相邻空气扩散。
    Spread,
    /// 当前流水失去 inflow 时退成空气（挖开/堵住容器后的停止）。
    Recompute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Task {
    x: i32,
    y: i32,
    z: i32,
    kind: TaskKind,
}

/// 六个邻格方向：`dy` 是相对本格的偏移（+1=上方，-1=下方）。
const NEIGHBORS: [(i32, i32, i32); 6] = [
    (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0), (0, -1, 0),
];

/// 全服共享流体状态：`Arc` 进 `NetConfig`/`PlayOptions`。
#[derive(Debug)]
pub struct FluidState {
    pending: Mutex<VecDeque<Task>>,
    /// 已入队去重（同一格同类型只留一个任务）。
    queued: Mutex<HashSet<(i32, i32, i32, TaskKind)>>,
    ticks: AtomicU64,
    writes: AtomicU64,
    last_writes: AtomicU64,
    running: AtomicBool,
    stop: Mutex<Option<Arc<AtomicBool>>>,
}

impl Default for FluidState {
    fn default() -> Self {
        Self::new()
    }
}

impl FluidState {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(VecDeque::new()),
            queued: Mutex::new(HashSet::new()),
            ticks: AtomicU64::new(0),
            writes: AtomicU64::new(0),
            last_writes: AtomicU64::new(0),
            running: AtomicBool::new(false),
            stop: Mutex::new(None),
        }
    }

    pub fn tick_count(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }
    pub fn total_writes(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }
    pub fn last_tick_writes(&self) -> u64 {
        self.last_writes.load(Ordering::Relaxed)
    }
    pub fn pending_len(&self) -> usize {
        self.pending.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    fn enqueue(&self, task: Task) {
        let key = (task.x, task.y, task.z, task.kind);
        if !self.queued.lock().unwrap_or_else(|p| p.into_inner()).insert(key) {
            return;
        }
        self.pending.lock().unwrap_or_else(|p| p.into_inner()).push_back(task);
    }

    /// 玩家挖/放改变方块后调用：相邻水源重新填满空格（恢复），
    /// 下游流水重算 inflow（堵住后停止）。`Ack` 不属于这里。
    pub fn on_block_change(&self, x: i32, y: i32, z: i32, old: u16, new: u16) {
        if rules::is_water(new) {
            self.enqueue(Task { x, y, z, kind: TaskKind::Spread });
        }
        for (dx, dy, dz) in NEIGHBORS {
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            // 下游重算：挖开容器后失去 inflow 的流水退成空气。
            self.enqueue(Task { x: nx, y: ny, z: nz, kind: TaskKind::Recompute });
            // 邻格若为 source，重新填满刚挖出的空格（水恢复）；非水源 no-op。
            // 不看 `old` 具体值：无论挖掉水还是挖掉容器，邻源都应重新扩散。
            if old != new {
                self.enqueue(Task { x: nx, y: ny, z: nz, kind: TaskKind::Spread });
            }
        }
    }

    /// 启动全服唯一 ticker（重复调用只生效一次）。仅 V2 世界会真正跑；
    /// 停复用 [`Self::stop_global_tick`]（测试/生命周期用，生产入口不调用）。
    pub fn start_global_tick(
        self: &Arc<Self>,
        world: Arc<MutableWorld>,
        cache: Option<Arc<crate::ChunkCache>>,
        hub: Option<Arc<crate::BroadcastHub>>,
    ) -> bool {
        if world.generator_version() != GeneratorVersion::V2 {
            return false;
        }
        let mut guard = self.stop.lock().unwrap_or_else(|p| p.into_inner());
        if self.running.swap(true, Ordering::SeqCst) {
            return false;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *guard = Some(Arc::clone(&stop));
        drop(guard);

        let me = Arc::clone(self);
        let thread_stop = Arc::clone(&stop);
        let spawned = std::thread::Builder::new().name("mc-fluid-tick".to_string()).spawn(
            move || {
                let period = Duration::from_millis(FLUID_TICK_MS);
                let mut next = Instant::now();
                loop {
                    if thread_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    next += period;
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now + period; // 落后超过一拍：重新对齐，不忙等追帧
                    }
                    if thread_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    me.tick(&world, cache.as_deref(), hub.as_deref());
                }
            },
        );
        match spawned {
            Ok(_) => true,
            Err(e) => {
                warn!("start global fluid tick failed ({e})");
                stop.store(true, Ordering::SeqCst);
                self.running.store(false, Ordering::SeqCst);
                *self.stop.lock().unwrap_or_else(|p| p.into_inner()) = None;
                false
            }
        }
    }

    pub fn stop_global_tick(&self) -> bool {
        let guard = self.stop.lock().unwrap_or_else(|p| p.into_inner());
        if !self.running.swap(false, Ordering::SeqCst) {
            return false;
        }
        if let Some(flag) = guard.as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
        true
    }

    /// 推进一拍（测试可直接调用，不需要真线程）。返回本次实际落子数。
    pub fn tick(
        &self,
        world: &MutableWorld,
        cache: Option<&crate::ChunkCache>,
        hub: Option<&crate::BroadcastHub>,
    ) -> usize {
        if world.generator_version() != GeneratorVersion::V2 {
            return 0;
        }
        self.ticks.fetch_add(1, Ordering::Relaxed);
        let batch: Vec<Task> = {
            let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            let n = pending.len().min(MAX_TASKS_PER_TICK);
            let taken: Vec<_> = pending.drain(..n).collect();
            let mut queued = self.queued.lock().unwrap_or_else(|p| p.into_inner());
            for t in &taken {
                queued.remove(&(t.x, t.y, t.z, t.kind));
            }
            taken
        };
        let mut writes = 0usize;
        let mut seen = HashSet::new();
        for task in batch {
            if writes >= MAX_WRITES_PER_TICK {
                self.enqueue(task); // 配额用尽：下一拍继续，不丢任务
                continue;
            }
            // 同格同类型一拍只处理一次；Spread 与 Recompute 可同拍各处理一次
            // （挖墙后邻源既要重算支撑，也要扩散进新空格）。
            if !seen.insert((task.x, task.y, task.z, task.kind)) {
                continue;
            }
            writes += self.process(task, world, cache, hub);
        }
        self.last_writes.store(writes as u64, Ordering::Relaxed);
        self.writes.fetch_add(writes as u64, Ordering::Relaxed);
        writes
    }

    /// 单个任务：返回实际写入数（0 或 1）。
    fn process(
        &self,
        task: Task,
        world: &MutableWorld,
        cache: Option<&crate::ChunkCache>,
        hub: Option<&crate::BroadcastHub>,
    ) -> usize {
        let pos = (task.x, task.y, task.z);
        let id = world.get(pos.0, pos.1, pos.2);
        let Some(level) = rules::water_level(id) else { return 0 };
        match task.kind {
            TaskKind::Spread => {
                if level > rules::FLOW_SPREAD_MAX {
                    return 0;
                }
                for (dx, dy, dz) in NEIGHBORS {
                    let (nx, ny, nz) = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                    if world.get(nx, ny, nz) != AIR_BLOCK_ID {
                        continue;
                    }
                    let rule = if dy == -1 { rules::FlowRule::Down } else { rules::FlowRule::Horizontal };
                    let Some(next) = rules::flow_out_level(level, rule) else { continue };
                    let new_id = rules::water_state_id(next);
                    if world.set(nx, ny, nz, new_id) == new_id {
                        continue; // 已是该值（并发重复），不重复计入
                    }
                    self.publish(nx, ny, nz, new_id, world, cache, hub);
                    self.enqueue(Task { x: nx, y: ny, z: nz, kind: TaskKind::Spread });
                    for (ox, oy, oz) in NEIGHBORS {
                        self.enqueue(Task { x: nx + ox, y: ny + oy, z: nz + oz, kind: TaskKind::Recompute });
                    }
                    return 1;
                }
                0
            }
            TaskKind::Recompute => {
                if level == 0 {
                    return 0; // source 常驻
                }
                let supported = NEIGHBORS.iter().any(|&(dx, dy, dz)| {
                    let nid = world.get(pos.0 + dx, pos.1 + dy, pos.2 + dz);
                    rules::water_level(nid)
                        .map(|nl| rules::supports_inflow(level, dy, nl))
                        .unwrap_or(false)
                });
                if supported {
                    return 0;
                }
                if world.set(pos.0, pos.1, pos.2, AIR_BLOCK_ID) == AIR_BLOCK_ID {
                    return 0;
                }
                self.publish(pos.0, pos.1, pos.2, AIR_BLOCK_ID, world, cache, hub);
                for (dx, dy, dz) in NEIGHBORS {
                    let (nx, ny, nz) = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
                    self.enqueue(Task { x: nx, y: ny, z: nz, kind: TaskKind::Recompute });
                    if rules::is_water(world.get(nx, ny, nz)) {
                        self.enqueue(Task { x: nx, y: ny, z: nz, kind: TaskKind::Spread });
                    }
                }
                1
            }
        }
    }

    /// 落子后的三件事：缓存失效 + 视距广播 + dirty 刷盘由 MutableWorld 自己负责。
    fn publish(
        &self,
        x: i32,
        y: i32,
        z: i32,
        id: u16,
        _world: &MutableWorld,
        cache: Option<&crate::ChunkCache>,
        hub: Option<&crate::BroadcastHub>,
    ) {
        let chunk = chunk_of_block(x, z);
        if let Some(cache) = cache {
            cache.invalidate(chunk);
        }
        if let Some(hub) = hub {
            let mut body = Vec::new();
            packets::BlockUpdate { x, y, z, block_id: id as i32 }.encode_body(&mut body);
            // 流体是系统行为，没有操作者；AKF 只属于玩家编辑，不走这里。
            hub.broadcast(
                0,
                chunk,
                packets::PLAY_BLOCK_UPDATE_S2C_ID,
                &body,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_world(seed: i64) -> Arc<MutableWorld> {
        Arc::new(MutableWorld::new_with_version(seed, GeneratorVersion::V2))
    }

    #[test]
    fn fluid_propagates_from_a_source_into_dug_hole() {
        let world = v2_world(9);
        // 在干燥地表挖一个空气坑，旁边放 source。
        let (x, y, z) = (20, 80, 20);
        world.set(x, y, z, AIR_BLOCK_ID);
        world.set(x - 1, y, z, rules::water_source_id());
        let f = Arc::new(FluidState::new());
        f.on_block_change(x - 1, y, z, AIR_BLOCK_ID, rules::water_source_id());
        let mut total = 0;
        for _ in 0..4 {
            total += f.tick(&world, None, None);
        }
        assert!(total >= 1, "source must refill the dug air cell");
        assert_eq!(world.get(x, y, z), rules::water_state_id(1));
        assert!(f.total_writes() >= 1);
    }

    #[test]
    fn digging_a_wall_lets_an_adjacent_source_refill_the_hole() {
        let world = v2_world(31);
        let (x, y, z) = (30, 200, 30);
        // 墙格 (x,y,z) 挡住旁边 source；玩家挖开墙 → 应立即被 source 重新灌入。
        world.set(x - 1, y, z, rules::water_source_id());
        world.set(x, y, z, mc_world::STONE_BLOCK_ID);
        let f = FluidState::new();
        // 玩家挖掘已落子，随后排队（与 apply_block_outcome 同顺序）。
        world.set(x, y, z, AIR_BLOCK_ID);
        f.on_block_change(x, y, z, mc_world::STONE_BLOCK_ID, AIR_BLOCK_ID);
        assert!(f.tick(&world, None, None) >= 1);
        assert_eq!(world.get(x, y, z), rules::water_state_id(1));
    }

    #[test]
    fn blocked_flow_recedes_and_source_removal_stops_flow() {
        let world = v2_world(11);
        let (x, y, z) = (20, 80, 20);
        // 人工造：source→level1→level2 一行，随后堵住 source。
        world.set(x, y, z, rules::water_source_id());
        world.set(x + 1, y, z, rules::water_state_id(1));
        world.set(x + 2, y, z, rules::water_state_id(2));
        let f = Arc::new(FluidState::new());
        // 堵住源头后，下游两级都应因失去 inflow 退成空气。
        world.set(x, y, z, mc_world::STONE_BLOCK_ID);
        f.on_block_change(x, y, z, rules::water_source_id(), mc_world::STONE_BLOCK_ID);
        for _ in 0..4 {
            f.tick(&world, None, None);
        }
        assert_eq!(world.get(x + 1, y, z), AIR_BLOCK_ID);
        assert_eq!(world.get(x + 2, y, z), AIR_BLOCK_ID);
    }

    #[test]
    fn v1_world_never_ticks_and_global_ticker_is_unique() {
        let v1 = Arc::new(MutableWorld::new(9));
        let f = Arc::new(FluidState::new());
        assert!(!f.start_global_tick(Arc::clone(&v1), None, None), "v1 must stay frozen");
        let g = Arc::new(FluidState::new());
        let w = v2_world(9);
        assert!(g.start_global_tick(Arc::clone(&w), None, None));
        assert!(!g.start_global_tick(Arc::clone(&w), None, None), "only one ticker allowed");
        assert!(g.stop_global_tick());
        assert!(!g.stop_global_tick());
    }

    #[test]
    fn work_per_tick_is_bounded() {
        let world = v2_world(42);
        let f = Arc::new(FluidState::new());
        // 灌 200 个任务：单拍最多处理 MAX_TASKS_PER_TICK，落子不超过 MAX_WRITES。
        for i in 0..200 {
            f.on_block_change(i % 16, 80, i / 16, mc_world::STONE_BLOCK_ID, AIR_BLOCK_ID);
        }
        let before = f.pending_len();
        assert!(before > 0);
        let wrote = f.tick(&world, None, None);
        assert!(wrote <= MAX_WRITES_PER_TICK);
        assert_eq!(f.last_tick_writes(), wrote as u64);
        assert!(f.pending_len() > 0, "remaining work must be queued for later ticks");
    }
}
