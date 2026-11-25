//! mutable: 服务端权威可变世界（WorldGen 确定性基底 + 稀疏 override），只兼容 26.3/777。
//!
//! 设计：
//! - 读 = override 优先，未覆盖的回落到 [`WorldGen`] 确定性生成；同 seed 下
//!   override 为空时与纯 WorldGen 逐字节一致（`generate_chunk` 复用同一编码路径）。
//! - 写 = 单一方块 `set`，只记录与基底不同的方块（设回基底值时删除条目，
//!   保持稀疏），并把所在 chunk 记入 dirty 集供存档刷盘。
//! - 并发：内部 `RwLock`，net 线程多连接可共享 `Arc<MutableWorld>`；权威判定
//!   （距离/越界/可挖）在调用方做，`set` 只负责落子（调用方已验证合法）。
//! - 存档见 [`save_to_dir`]/[`load_from_dir`]：自有稀疏格式（与 Anvil 的差距见
//!   HANDOFF §5），文件名按 seed 隔离，换 seed 不混用旧存档。
//!
//! `heights`/`motion_heights` 语义与 [`GeneratedChunk`] 一致：编辑后只重算
//! 被触碰列（自顶向下扫描），`heights` 取“地形集合”（石头/土/草/沙/矿），
//! `motion` 取最高非空气（含水/树叶/原木），与 P16 基底规则对齐。
use crate::{
    AIR_BLOCK_ID, COAL_ORE_BLOCK_ID, COBBLESTONE_BLOCK_ID, DIRT_BLOCK_ID, FARMLAND_BLOCK_ID,
    GRASS_BLOCK_ID,
    IRON_ORE_BLOCK_ID, MIN_Y, OAK_LOG_BLOCK_ID, SAND_BLOCK_ID, SECTION_COUNT, STONE_BLOCK_ID,
    WATER_BLOCK_ID,
};
use mc_core::{BlockPos, ChunkPos};

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::RwLock;

/// 世界纵向有效范围：y ∈ [MIN_Y, WORLD_TOP_Y]，section y=-64..319。
pub const WORLD_TOP_Y: i32 = MIN_Y + SECTION_COUNT as i32 * 16 - 1;
/// 创造风格判定半径：眼睛到方块中心超过该值拒绝（原版约 6，留 0.5 网络延迟余量）。
pub const MAX_REACH: f64 = 6.5;
/// 眼睛相对脚的高度（站立）。
pub const EYE_HEIGHT: f64 = 1.62;

/// P17 放置默认材料：石头（创造背包解析见 HANDOFF 限制 §5，暂不支持客户端自选）。
pub const DEFAULT_PLACE_BLOCK_ID: u16 = STONE_BLOCK_ID;
/// P17 允许放置的方块（真实 blockstate ID）：石头/泥土/草/橡木原木/沙子/圆石。
pub const PLACEABLE_BLOCK_IDS: [u16; 6] = [
    STONE_BLOCK_ID,
    DIRT_BLOCK_ID,
    GRASS_BLOCK_ID,
    OAK_LOG_BLOCK_ID,
    SAND_BLOCK_ID,
    COBBLESTONE_BLOCK_ID,
];

/// 是否允许玩家放置该方块 ID（P17 allowlist，熔炉/水等不在内）。
pub fn is_placeable(id: u16) -> bool {
    PLACEABLE_BLOCK_IDS.contains(&id)
}

/// 可替换（放置目标必须）方块：空气/水。
pub fn is_replaceable(id: u16) -> bool {
    id == AIR_BLOCK_ID || id == WATER_BLOCK_ID
}

/// `heights` 地形集合：石头/泥土/草/沙/煤/铁/耕地（树叶/原木/水不算地形顶）。
/// P27 耕地计入地形：耕后地表高度不变（与草/泥土同口径），天空光起点也不动。
fn is_terrain(id: u16) -> bool {
    matches!(
        id,
        STONE_BLOCK_ID
            | DIRT_BLOCK_ID
            | GRASS_BLOCK_ID
            | SAND_BLOCK_ID
            | COAL_ORE_BLOCK_ID
            | IRON_ORE_BLOCK_ID
            | FARMLAND_BLOCK_ID
    )
}

/// 存档魔数 + 版本（自有稀疏格式，非 Anvil，差距见 HANDOFF）。
const SAVE_MAGIC: [u8; 4] = *b"P17W";
const SAVE_VERSION: u8 = 1;

/// 方块坐标 → 所在 chunk（负坐标向 -inf 取整）。
pub fn chunk_of_block(x: i32, z: i32) -> ChunkPos {
    ChunkPos(x.div_euclid(16), z.div_euclid(16))
}

#[derive(Debug, Default)]
struct Inner {
    /// 与基底不同的方块：(x,y,z) → blockstate ID。
    overrides: HashMap<(i32, i32, i32), u16>,
    /// 自上次刷盘后被写过的 chunk。
    dirty: HashSet<ChunkPos>,
}

/// 服务端权威可变世界：`Arc` 共享给 net 线程（读写）与 ECS（刷盘/出生点）。
#[derive(Debug)]
pub struct MutableWorld {
    seed: i64,
    base: crate::WorldGen,
    inner: RwLock<Inner>,
}

impl MutableWorld {
    pub fn new(seed: i64) -> Self {
        Self { seed, base: crate::WorldGen::new(seed), inner: RwLock::new(Inner::default()) }
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    fn lock_read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// override 条数（稀疏性诊断）。
    pub fn override_count(&self) -> usize {
        self.lock_read().overrides.len()
    }

    /// 脏 chunk 数（待刷盘）。
    pub fn dirty_len(&self) -> usize {
        self.lock_read().dirty.len()
    }

    /// P22：当前所有小麦方块（pos → blockstate，含 age 信息）。
    /// 小麦永不在确定性基底里，override 全量即全集；供重启后把存量小麦
    /// 重新纳入生长跟踪（`adopt_existing_crops`），以及 e2e 断言。
    pub fn wheat_blocks(&self) -> Vec<((i32, i32, i32), u16)> {
        self.lock_read()
            .overrides
            .iter()
            .filter(|(_, id)| crate::wheat_age(**id).is_some())
            .map(|(pos, id)| (*pos, *id))
            .collect()
    }

    /// P26：当前所有树苗方块（pos 列表；树苗永不在基底里，override 全量即全集）。
    /// 供重启后把存量树苗重新纳入生长（`adopt_existing_saplings`）。
    pub fn sapling_blocks(&self) -> Vec<(i32, i32, i32)> {
        self.lock_read()
            .overrides
            .iter()
            .filter(|(_, id)| **id == crate::OAK_SAPLING_BLOCK_ID)
            .map(|(pos, _)| *pos)
            .collect()
    }

    /// 读方块：override 优先，否则确定性基底；y 越界返回空气。
    pub fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        if !(MIN_Y..=WORLD_TOP_Y).contains(&y) {
            return AIR_BLOCK_ID;
        }
        if let Some(&id) = self.lock_read().overrides.get(&(x, y, z)) {
            return id;
        }
        let chunk = self.base.generate(chunk_of_block(x, z));
        let section = ((y - MIN_Y) >> 4) as usize;
        let local_y = ((y - MIN_Y) & 15) as usize;
        let lx = x.rem_euclid(16) as usize;
        let lz = z.rem_euclid(16) as usize;
        chunk.sections[section][local_y * 256 + lz * 16 + lx]
    }

    /// 写方块：返回旧值；y 越界时 no-op（返回空气）。
    /// 与基底相同的值会删除 override 条目（保持稀疏）；所在 chunk 记 dirty。
    pub fn set(&self, x: i32, y: i32, z: i32, id: u16) -> u16 {
        if !(MIN_Y..=WORLD_TOP_Y).contains(&y) {
            return AIR_BLOCK_ID;
        }
        // 基底值先算（纯计算，不持锁，避免锁内再生成 chunk 的开销/死锁）。
        let base = self.base_value(x, y, z);
        let mut inner = self.lock_write();
        let old = inner.overrides.get(&(x, y, z)).copied().unwrap_or(base);
        if id == base {
            inner.overrides.remove(&(x, y, z));
        } else {
            inner.overrides.insert((x, y, z), id);
        }
        inner.dirty.insert(chunk_of_block(x, z));
        old
    }

    /// 原子条件写（CAS）：仅当当前方块仍为 `expected` 时写入 `id`，返回旧值；
    /// 已被其它连接改动则返回 `None` 且不落子。用于挖掘的 get+set 去重
    /// （两连接同时挖同一格只能有一个成功、只生成一份掉落物）。
    pub fn set_if(&self, x: i32, y: i32, z: i32, expected: u16, id: u16) -> Option<u16> {
        if !(MIN_Y..=WORLD_TOP_Y).contains(&y) {
            return None;
        }
        let base = self.base_value(x, y, z);
        let mut inner = self.lock_write();
        let old = inner.overrides.get(&(x, y, z)).copied().unwrap_or(base);
        if old != expected {
            return None;
        }
        if id == base {
            inner.overrides.remove(&(x, y, z));
        } else {
            inner.overrides.insert((x, y, z), id);
        }
        inner.dirty.insert(chunk_of_block(x, z));
        Some(old)
    }

    /// 基底单方块值（内部用：set 时判断是否可删条目；调用前已持写锁会死锁，
    /// 故本函数自己不再加锁，只做纯计算）。
    fn base_value(&self, x: i32, y: i32, z: i32) -> u16 {
        let chunk = self.base.generate(chunk_of_block(x, z));
        let section = ((y - MIN_Y) >> 4) as usize;
        let local_y = ((y - MIN_Y) & 15) as usize;
        let lx = x.rem_euclid(16) as usize;
        let lz = z.rem_euclid(16) as usize;
        chunk.sections[section][local_y * 256 + lz * 16 + lx]
    }

    /// 生成带 override 的完整 chunk（编码/高度图/光照都由此走，与纯基底同路径）。
    /// 只重算被触碰列的 heights/motion，其余列沿用基底值。
    pub fn generate_chunk(&self, pos: ChunkPos) -> crate::worldgen::GeneratedChunk {
        let mut chunk = self.base.generate(pos);
        let inner = self.lock_read();
        let base_x = pos.0 * 16;
        let base_z = pos.1 * 16;
        let mut touched = [false; 256];
        for ((x, y, z), id) in inner.overrides.iter() {
            let lx = *x - base_x;
            let lz = *z - base_z;
            if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
                continue;
            }
            if !(MIN_Y..=WORLD_TOP_Y).contains(y) {
                continue;
            }
            let section = ((y - MIN_Y) >> 4) as usize;
            if section >= chunk.sections.len() {
                continue;
            }
            let local_y = ((y - MIN_Y) & 15) as usize;
            chunk.sections[section][local_y * 256 + lz as usize * 16 + lx as usize] = *id;
            touched[lz as usize * 16 + lx as usize] = true;
        }
        drop(inner);
        for (col, hit) in touched.iter().enumerate() {
            if *hit {
                let (h, m) = column_heights(&chunk.sections, col, chunk.heights[col]);
                chunk.heights[col] = h;
                chunk.motion_heights[col] = m;
            }
        }
        chunk
    }

    /// 取出并清空 dirty 集（刷盘用）。
    pub fn take_dirty(&self) -> Vec<ChunkPos> {
        let mut inner = self.lock_write();
        inner.dirty.drain().collect()
    }

    /// 清空 dirty 集（成功刷盘后调用；与 [`take_dirty`] 二选一）。
    pub fn clear_dirty(&self) {
        self.lock_write().dirty.clear();
    }

    /// 出生点：基底出生点若被 override 埋住则向上抬，直到空气或到顶。
    pub fn spawn_pos(&self) -> BlockPos {
        let base = self.base.spawn_pos();
        let mut y = base.1;
        while y < WORLD_TOP_Y && self.get(base.0, y, base.2) != AIR_BLOCK_ID {
            y += 1;
        }
        BlockPos(base.0, y, base.2)
    }

    /// 支撑面顶 y（兼容旧调用的“整列最高非空气方块上方 1 格”）；全空列回落 MIN_Y。
    /// 掉落物静置高度请用 [`Self::drop_rest_y`]：本函数会受柱内任意上方方块（树/水面/
    /// 洞穴顶）影响，把它们下方的掉落物错误地抬到列顶。
    pub fn support_top_y(&self, x: i32, z: i32) -> i32 {
        self.drop_rest_y(x, z, WORLD_TOP_Y)
    }

    /// 掉落物静置高度：从 `from_y - 1` 向下找第一个非空气方块，返回其顶部 y。
    /// `from_y` 传**被破坏方块的 y**：这样树冠/水面/上方方块不会把掉落物“抬”上去，
    /// 挖洞穴或地下矿物时掉落物也停在挖掘点附近（而不是瞬移到地表）。全空列回落 MIN_Y。
    /// 单次 `generate_chunk` 扫描（调用方在 spawn 时算一次，tick 内不再查世界）。
    /// 生成后支撑被挖的悬空重算见 `mc_world::items::ItemEntity::support` 与
    /// `DropState::start_global_tick`（全服唯一 ticker，每 50ms 检测并重算 rest_y）。
    pub fn drop_rest_y(&self, x: i32, z: i32, from_y: i32) -> i32 {
        let chunk = self.generate_chunk(chunk_of_block(x, z));
        let lx = x.rem_euclid(16) as usize;
        let lz = z.rem_euclid(16) as usize;
        let start = (from_y - 1).clamp(MIN_Y, WORLD_TOP_Y);
        for y in (MIN_Y..=start).rev() {
            let section = ((y - MIN_Y) >> 4) as usize;
            if section >= chunk.sections.len() {
                continue;
            }
            let local_y = ((y - MIN_Y) & 15) as usize;
            if chunk.sections[section][local_y * 256 + lz * 16 + lx] != AIR_BLOCK_ID {
                return y + 1;
            }
        }
        MIN_Y
    }

    /// 存档路径：`dir/overrides-<seed>.dat`（seed 隔离，换 seed 不混用）。
    pub fn save_path(dir: &Path, seed: i64) -> std::path::PathBuf {
        dir.join(format!("overrides-{seed}.dat"))
    }

    /// 全量刷盘（原子写 tmp + rename）：魔数/版本/seed/条数/条目(x,y,z i32be + id u16be，按坐标排序）。
    /// 成功后清空 dirty；返回写入条目数。
    pub fn save_to_dir(&self, dir: &Path) -> std::io::Result<usize> {
        std::fs::create_dir_all(dir)?;
        let mut entries: Vec<((i32, i32, i32), u16)> = {
            self.lock_read().overrides.iter().map(|(&k, &v)| (k, v)).collect()
        };
        entries.sort_by_key(|(k, _)| *k);
        let path = Self::save_path(dir, self.seed);
        let tmp = path.with_extension("tmp");
        let mut buf = Vec::with_capacity(13 + entries.len() * 14);
        buf.extend_from_slice(&SAVE_MAGIC);
        buf.push(SAVE_VERSION);
        buf.extend_from_slice(&self.seed.to_be_bytes());
        buf.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for ((x, y, z), id) in &entries {
            buf.extend_from_slice(&x.to_be_bytes());
            buf.extend_from_slice(&y.to_be_bytes());
            buf.extend_from_slice(&z.to_be_bytes());
            buf.extend_from_slice(&id.to_be_bytes());
        }
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&buf)?;
            f.flush()?;
        }
        std::fs::rename(&tmp, &path)?;
        self.clear_dirty();
        Ok(entries.len())
    }

    /// 启动加载：文件缺失 → 空世界；seed 由文件名隔离，文件内 seed 不符报错。
    pub fn load_from_dir(dir: &Path, seed: i64) -> std::io::Result<Self> {
        let world = Self::new(seed);
        let path = Self::save_path(dir, seed);
        let mut f = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(world),
            Err(e) => return Err(e),
        };
        let mut head = [0u8; 17];
        f.read_exact(&mut head).map_err(|e| {
            std::io::Error::new(e.kind(), format!("{}: 头损坏 ({e})", path.display()))
        })?;
        if head[0..4] != SAVE_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 魔数不符（非 P17 存档）", path.display()),
            ));
        }
        if head[4] != SAVE_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 版本 {} 不支持", path.display(), head[4]),
            ));
        }
        let file_seed = i64::from_be_bytes(head[5..13].try_into().unwrap());
        if file_seed != seed {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 存档 seed {file_seed} 与当前 {seed} 不符", path.display()),
            ));
        }
        let count = u32::from_be_bytes(head[13..17].try_into().unwrap()) as usize;
        if count > 16 * 1_000_000 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 条目数 {count} 过大", path.display()),
            ));
        }
        let mut inner = world.lock_write();
        for _ in 0..count {
            let mut e = [0u8; 14];
            f.read_exact(&mut e).map_err(|e| {
                std::io::Error::new(e.kind(), format!("{}: 条目截断 ({e})", path.display()))
            })?;
            let x = i32::from_be_bytes(e[0..4].try_into().unwrap());
            let y = i32::from_be_bytes(e[4..8].try_into().unwrap());
            let z = i32::from_be_bytes(e[8..12].try_into().unwrap());
            let id = u16::from_be_bytes(e[12..14].try_into().unwrap());
            if !(MIN_Y..=WORLD_TOP_Y).contains(&y) {
                continue;
            }
            // 与基底相同的条目不入库（旧文件脏数据自愈，保持稀疏）。
            if id != world.base_value(x, y, z) {
                inner.overrides.insert((x, y, z), id);
            }
        }
        // 读入即视为干净（刚落盘的状态）。
        inner.dirty.clear();
        drop(inner);
        // 文件尾有多余字节 → 损坏。
        let mut tail = [0u8; 1];
        if f.read(&mut tail)? != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: 文件尾有多余字节", path.display()),
            ));
        }
        Ok(world)
    }
}

/// 重算单列 heights/motion（自顶向下扫描；无方块时回落基底值）。
fn column_heights(
    sections: &[Box<[u16; 4096]>],
    col: usize,
    fallback_h: i32,
) -> (i32, i32) {
    let lx = (col % 16) as i32;
    let lz = (col / 16) as i32;
    let mut motion_top: Option<i32> = None;
    let mut terrain_top: Option<i32> = None;
    for y in (MIN_Y..=WORLD_TOP_Y).rev() {
        let section = ((y - MIN_Y) >> 4) as usize;
        if section >= sections.len() {
            continue;
        }
        let local_y = ((y - MIN_Y) & 15) as usize;
        let id = sections[section][local_y * 256 + lz as usize * 16 + lx as usize];
        if id == AIR_BLOCK_ID {
            continue;
        }
        if motion_top.is_none() {
            motion_top = Some(y);
        }
        if is_terrain(id) {
            terrain_top = Some(y);
            break;
        }
    }
    match (terrain_top, motion_top) {
        (Some(h), Some(m)) => (h, m + 1 - MIN_Y),
        // 防御分支：地形块必是非空气，motion_top 不可能为 None；若发生按地形顶算。
        (Some(h), None) => (h, h + 1 - MIN_Y),
        (None, Some(m)) => (fallback_h.min(m), m + 1 - MIN_Y),
        (None, None) => (fallback_h.min(MIN_Y), 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_SEED, OAK_LEAVES_BLOCK_ID, STONE_BLOCK_ID};

    fn spawn_top(seed: i64) -> (i32, i32, i32) {
        let s = crate::WorldGen::new(seed).spawn_pos();
        (s.0, s.1 - 1, s.2)
    }

    #[test]
    fn empty_world_reads_match_deterministic_base() {
        let m = MutableWorld::new(DEFAULT_SEED);
        assert_eq!(m.override_count(), 0);
        assert_eq!(m.dirty_len(), 0);
        // 出生点脚下是草方块（3×3 干燥平台）。
        let (x, y, z) = spawn_top(DEFAULT_SEED);
        assert_eq!(m.get(x, y, z), crate::GRASS_BLOCK_ID);
        // 同 seed 纯基底逐字节一致（override 为空时无污染）。
        let gen = m.generate_chunk(chunk_of_block(x, z));
        let want = crate::WorldGen::new(DEFAULT_SEED).generate(chunk_of_block(x, z));
        assert_eq!(gen.sections, want.sections);
        assert_eq!(gen.heights, want.heights);
        assert_eq!(gen.motion_heights, want.motion_heights);
        assert_eq!(gen.network_bytes(), want.network_bytes());
    }

    #[test]
    fn set_get_override_and_sparse_removal() {
        let m = MutableWorld::new(DEFAULT_SEED);
        let (x, y, z) = spawn_top(DEFAULT_SEED);
        let base = m.get(x, y, z);
        // 挖掉 → 空气，记 1 条 override + 1 脏 chunk。
        assert_eq!(m.set(x, y, z, AIR_BLOCK_ID), base);
        assert_eq!(m.get(x, y, z), AIR_BLOCK_ID);
        assert_eq!(m.override_count(), 1);
        assert_eq!(m.dirty_len(), 1);
        // 设回基底值 → 条目删除（保持稀疏），但 chunk 仍是脏的（需重发/刷盘）。
        assert_eq!(m.set(x, y, z, base), AIR_BLOCK_ID);
        assert_eq!(m.get(x, y, z), base);
        assert_eq!(m.override_count(), 0, "setting base value must drop the entry");
        assert_eq!(m.dirty_len(), 1);
    }

    #[test]
    fn set_if_is_atomic_and_rejects_stale_expected() {
        let m = MutableWorld::new(DEFAULT_SEED);
        let (x, y, z) = spawn_top(DEFAULT_SEED);
        let base = m.get(x, y, z);
        // 两个连接同时挖同一格：只有期望值匹配的那次能落子。
        assert_eq!(m.set_if(x, y, z, base, AIR_BLOCK_ID), Some(base));
        assert_eq!(m.set_if(x, y, z, base, AIR_BLOCK_ID), None, "stale CAS must not drop twice");
        assert_eq!(m.get(x, y, z), AIR_BLOCK_ID);
        // 期望为当前空气值时仍可继续写（后续放置走 set，CAS 只用于挖掘去重）。
        assert_eq!(m.set_if(x, y, z, AIR_BLOCK_ID, STONE_BLOCK_ID), Some(AIR_BLOCK_ID));
        assert_eq!(m.get(x, y, z), STONE_BLOCK_ID);
    }

    #[test]
    fn dig_top_updates_motion_and_refill_restores() {
        let m = MutableWorld::new(DEFAULT_SEED);
        let (x, y, z) = spawn_top(DEFAULT_SEED);
        let pos = chunk_of_block(x, z);
        let col = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
        let before = m.generate_chunk(pos);
        // 挖掉地表草 → motion 下降（至少降 1：草本身被挖）。
        m.set(x, y, z, AIR_BLOCK_ID);
        let dug = m.generate_chunk(pos);
        assert_eq!(dug.get_column_top(x, y, z), AIR_BLOCK_ID);
        assert!(
            dug.motion_heights[col] < before.motion_heights[col],
            "motion must drop after digging the top block"
        );
        // 填回草 → 与基底逐字节一致。
        m.set(x, y, z, crate::GRASS_BLOCK_ID);
        let back = m.generate_chunk(pos);
        let want = crate::WorldGen::new(DEFAULT_SEED).generate(pos);
        assert_eq!(back.sections, want.sections);
        assert_eq!(back.motion_heights, want.motion_heights);
        assert_eq!(back.network_bytes(), want.network_bytes());
    }

    #[test]
    fn place_tower_raises_motion_and_heightmap_encodes_it() {
        let m = MutableWorld::new(DEFAULT_SEED);
        let s = crate::WorldGen::new(DEFAULT_SEED).spawn_pos();
        let (x, z) = (s.0, s.2);
        let pos = chunk_of_block(x, z);
        let col = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
        let before = m.generate_chunk(pos).motion_heights[col];
        // 在出生点上方放 3 格石头塔。
        for y in s.1..s.1 + 3 {
            m.set(x, y, z, STONE_BLOCK_ID);
        }
        let after = m.generate_chunk(pos);
        assert_eq!(after.motion_heights[col], before + 3);
        // 高度图字节包含新值（MOTION_BLOCKING BPE=9 打包）。
        let bytes = after.heightmaps_bytes();
        let want = before + 3;
        let mut found = false;
        for li in 0..37 {
            let word = u64::from_be_bytes(bytes[3 + li * 8..3 + li * 8 + 8].try_into().unwrap());
            for k in 0..7 {
                if li * 7 + k == col {
                    assert_eq!((word >> (k * 9)) & 0x1ff, want as u64, "column {col}");
                    found = true;
                }
            }
        }
        assert!(found);
        assert_eq!(m.dirty_len(), 1, "same chunk edits share one dirty entry");
    }

    #[test]
    fn out_of_range_y_is_air_and_noop() {
        let m = MutableWorld::new(DEFAULT_SEED);
        assert_eq!(m.get(0, WORLD_TOP_Y + 1, 0), AIR_BLOCK_ID);
        assert_eq!(m.get(0, MIN_Y - 1, 0), AIR_BLOCK_ID);
        assert_eq!(m.set(0, WORLD_TOP_Y + 1, 0, STONE_BLOCK_ID), AIR_BLOCK_ID);
        assert_eq!(m.override_count(), 0);
        assert_eq!(m.dirty_len(), 0);
    }

    #[test]
    fn save_load_roundtrip_preserves_overrides() {
        let dir = std::env::temp_dir().join(format!("mc-p17-{}-rt", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let m = MutableWorld::new(42);
        m.set(369, 70, 381, AIR_BLOCK_ID);
        m.set(369, 71, 381, STONE_BLOCK_ID);
        m.set(-100, 0, -200, OAK_LOG_BLOCK_ID);
        assert_eq!(m.save_to_dir(&dir).unwrap(), 3);
        assert_eq!(m.dirty_len(), 0, "save must clear dirty");
        let back = MutableWorld::load_from_dir(&dir, 42).unwrap();
        assert_eq!(back.override_count(), 3);
        assert_eq!(back.get(369, 70, 381), AIR_BLOCK_ID);
        assert_eq!(back.get(369, 71, 381), STONE_BLOCK_ID);
        assert_eq!(back.get(-100, 0, -200), OAK_LOG_BLOCK_ID);
        assert_eq!(back.dirty_len(), 0, "loaded world starts clean");
        // 未改动的列仍走基底。
        let (x, y, z) = spawn_top(42);
        assert_eq!(back.get(x, y, z), MutableWorld::new(42).get(x, y, z));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_load_seed_isolation_and_missing_file() {
        let dir = std::env::temp_dir().join(format!("mc-p17-{}-iso", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // 文件缺失 → 空世界，不报错。
        let empty = MutableWorld::load_from_dir(&dir, 7).unwrap();
        assert_eq!(empty.override_count(), 0);
        // seed=1 存盘后，seed=2 读到空（不同文件），seed=1 读回数据。
        let m = MutableWorld::new(1);
        m.set(0, 64, 0, STONE_BLOCK_ID);
        m.save_to_dir(&dir).unwrap();
        assert_eq!(MutableWorld::load_from_dir(&dir, 2).unwrap().override_count(), 0);
        assert_eq!(MutableWorld::load_from_dir(&dir, 1).unwrap().get(0, 64, 0), STONE_BLOCK_ID);
        assert!(MutableWorld::save_path(&dir, 1) != MutableWorld::save_path(&dir, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_save_errors() {
        let dir = std::env::temp_dir().join(format!("mc-p17-{}-bad", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 魔数不符。
        std::fs::write(MutableWorld::save_path(&dir, 9), b"NOPE").unwrap();
        assert!(MutableWorld::load_from_dir(&dir, 9).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_lifts_over_buried_overrides() {
        let m = MutableWorld::new(DEFAULT_SEED);
        let base = crate::WorldGen::new(DEFAULT_SEED).spawn_pos();
        // 把出生点埋 2 格石头。
        m.set(base.0, base.1, base.2, STONE_BLOCK_ID);
        m.set(base.0, base.1 + 1, base.2, STONE_BLOCK_ID);
        let fixed = m.spawn_pos();
        assert_eq!((fixed.0, fixed.2), (base.0, base.2));
        assert_eq!(fixed.1, base.1 + 2);
        assert_eq!(m.get(fixed.0, fixed.1, fixed.2), AIR_BLOCK_ID);
    }

    #[test]
    fn support_top_y_tracks_dig_and_place() {
        let m = MutableWorld::new(DEFAULT_SEED);
        let (x, _, z) = spawn_top(DEFAULT_SEED);
        let s = crate::WorldGen::new(DEFAULT_SEED).spawn_pos();
        // 出生点支撑面 = 出生 y（空气格），挖掉脚下方块后下降 1。
        assert_eq!(m.support_top_y(x, z), s.1);
        m.set(x, s.1 - 1, z, AIR_BLOCK_ID);
        assert_eq!(m.support_top_y(x, z), s.1 - 1);
    }

    #[test]
    fn drop_rest_y_uses_support_below_not_column_top() {
        let m = MutableWorld::new(DEFAULT_SEED);
        let (x, _, z) = spawn_top(DEFAULT_SEED);
        let s = crate::WorldGen::new(DEFAULT_SEED).spawn_pos();
        let ground = s.1 - 1; // 脚下方块（草）

        // 在正上方放一个“树冠”方块：整列最高方块被抬高，但掉落物静置高度不受影响。
        m.set(x, ground + 5, z, OAK_LEAVES_BLOCK_ID);
        assert_eq!(m.support_top_y(x, z), ground + 6, "column top followed the fake leaves");
        // 从被破坏的草方块 (y=ground) 往下找支撑：结果是草下方的泥土顶 = ground。
        assert_eq!(m.drop_rest_y(x, z, ground), ground);
        // 挖掉脚下方块后掉落物应停在更低一层。
        m.set(x, ground - 1, z, AIR_BLOCK_ID);
        assert_eq!(m.drop_rest_y(x, z, ground), ground - 1);
        // 从高处掉落时静置高度仍是该列最高非空气方块顶（兼容旧语义的起点）。
        assert_eq!(m.drop_rest_y(x, z, WORLD_TOP_Y), ground + 6);
    }

    #[test]
    fn placeable_allowlist() {
        assert!(is_placeable(STONE_BLOCK_ID));
        assert!(is_placeable(GRASS_BLOCK_ID));
        assert!(is_placeable(OAK_LOG_BLOCK_ID));
        assert!(!is_placeable(AIR_BLOCK_ID));
        assert!(!is_placeable(WATER_BLOCK_ID));
        assert!(!is_placeable(crate::COAL_ORE_BLOCK_ID));
        assert!(is_replaceable(AIR_BLOCK_ID));
        assert!(is_replaceable(WATER_BLOCK_ID));
        assert!(!is_replaceable(STONE_BLOCK_ID));
    }

    #[test]
    fn p27_farmland_keeps_terrain_height_and_motion() {
        // 耕地替换草方块：地形高度/天空光起点都不动（耕地是地形块，与草同口径）。
        let m = MutableWorld::new(DEFAULT_SEED);
        let (x, y, z) = spawn_top(DEFAULT_SEED);
        assert_eq!(m.get(x, y, z), crate::GRASS_BLOCK_ID);
        let pos = chunk_of_block(x, z);
        let col = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
        let before = m.generate_chunk(pos);
        m.set(x, y, z, FARMLAND_BLOCK_ID);
        let after = m.generate_chunk(pos);
        assert_eq!(after.get_column_top(x, y, z), FARMLAND_BLOCK_ID);
        assert_eq!(after.heights[col], before.heights[col], "till must not move terrain top");
        assert_eq!(
            after.motion_heights[col], before.motion_heights[col],
            "till must not move the motion top"
        );
        // 耕地仍可挖：挖后该格变空气（掉泥土由掉落表口径保证，`drops.rs` 覆盖）。
        m.set(x, y, z, AIR_BLOCK_ID);
        assert_eq!(m.generate_chunk(pos).get_column_top(x, y, z), crate::AIR_BLOCK_ID);
    }

    /// 测试辅助：读某列在某 y 的方块（仅断言生成结果用）。
    trait ColumnTop {
        fn get_column_top(&self, x: i32, y: i32, z: i32) -> u16;
    }

    impl ColumnTop for crate::worldgen::GeneratedChunk {
        fn get_column_top(&self, x: i32, y: i32, z: i32) -> u16 {
            let section = ((y - MIN_Y) >> 4) as usize;
            let local_y = ((y - MIN_Y) & 15) as usize;
            let lx = x.rem_euclid(16) as usize;
            let lz = z.rem_euclid(16) as usize;
            self.sections[section][local_y * 256 + lz * 16 + lx]
        }
    }
}
