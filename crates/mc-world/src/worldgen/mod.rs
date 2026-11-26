//! worldgen: 由 `server.toml` seed 驱动的确定性地形（水/沙岸/树/洞穴/矿/biome），
//! 只兼容 26.3/777，无第三方依赖。
//!
//! 算法：
//! - 世界坐标 3 层平滑 value noise（scale 128/48/24，幅度 20/10/6），基高 58，
//!   地表 y ∈ 58..=94；相邻列高度差 ≤3；种子只影响噪声；
//! - 出生点 (369,381) 半径 16 内做 smootherstep 抬升：局部高度低于 66 的列平滑抬到 66
//!   （半径 4.5 内为平台），保证出生点及 3×3 干燥、草方块、不悬空；
//! - 每列：h > SEA_LEVEL+2（65）→ 草 + 3 土；h ≤ 65 → 顶层 4 格沙（沙滩/水下）；
//!   h < SEA_LEVEL(63) 时 h+1..=63 填水（water=89）；
//! - 地下 y < h-8 且 y > MIN_Y+8 的石头用 3D value noise（4×4×4 格）挖洞穴；
//!   剩余石头按确定性 hash 生成煤矿（y 0..80，1%）/铁矿（y -40..56，0.8%）；
//! - 草方块、h > 65、4 邻域坡度 ≤1 的列按 biome 密度生成 oak（forest 1/10、plains 1/40），
//!   扫描 base-2..=base+17 保证跨 chunk 树冠连续；出生点 4 格内不长树；
//! - 每 chunk 单一 biome（ocean/beach/forest/plains），所有 section 共用。
//!
//! `heights` 是地形顶（草/沙），`motion_heights` 才是 MOTION_BLOCKING（含水面/树）。
mod biome;
mod caves;
mod encode;
mod features;
mod light;
mod noise;
mod ores;
mod ruins;
mod terrain;
#[cfg(test)]
mod tests;

pub use features::is_plant;

use crate::{
    AIR_BLOCK_ID, COAL_ORE_BLOCK_ID, DIRT_BLOCK_ID, GRASS_BLOCK_ID, IRON_ORE_BLOCK_ID, MIN_Y,
    OAK_LEAVES_BLOCK_ID, OAK_LOG_BLOCK_ID, SAND_BLOCK_ID, SECTION_COUNT, SPAWN_BLOCK_X,
    SPAWN_BLOCK_Z, STONE_BLOCK_ID, WATER_BLOCK_ID,
};
use mc_core::{BlockPos, ChunkPos};
use noise::{hash01, smootherstep, value_noise_2d, value_noise_3d};

pub use encode::GeneratedChunk;

/// 默认世界种子（与 `server.toml` 默认值一致）。
pub const DEFAULT_SEED: i64 = 123456789;
/// 地表最低/最高 y（草/沙方块所在 y）。出生点 y = 地表 y + 1。
pub const MIN_SURFACE_Y: i32 = 58;
pub const MAX_SURFACE_Y: i32 = 94;
/// 海平面 y：与 LoginPlay 的 `sea_level`（63）一致；h < SEA_LEVEL 的列灌水到该高度。
pub const SEA_LEVEL: i32 = 63;
/// 出生点保证的最低干燥地表 y（> SEA_LEVEL+2，确保顶层是草而不是沙）。
pub const SPAWN_DRY_SURFACE_Y: i32 = SEA_LEVEL + 3;
/// 基高与 3 层噪声幅度（合计 36，落在 58..94）。
const BASE_HEIGHT: f64 = 58.0;
const OCTAVE_SCALES: [f64; 3] = [128.0, 48.0, 24.0];
const OCTAVE_AMPLITUDES: [f64; 3] = [20.0, 10.0, 6.0];
const OCTAVE_SALTS: [u64; 3] = [
    0x9E37_79B9_7F4A_7C15,
    0xC2B2_AE3D_27D4_EB4F,
    0x1656_67B1_9E37_79F9,
];
/// 出生点保护：半径 16 内把低地平滑抬到 [`SPAWN_DRY_SURFACE_Y`]，半径 4.5 内为平台。
const SPAWN_PROTECT_RADIUS: f64 = 16.0;
const SPAWN_PLATEAU_RADIUS: f64 = 4.5;
/// 出生点 4 格内不长树（含树冠）：树冠半径 2，故排除原点 7 格内的候选，
/// 保证最近树叶也在 5 格外，不挡玩家/视线。
pub(super) const SPAWN_TREE_CLEAR: i32 = 7;

/// biome：低频率森林噪声 + chunk 内水/沙占比。
const FOREST_SCALE: f64 = 96.0;
const FOREST_SALT: u64 = 0x51ED_2701_9E37_79B9;
const FOREST_THRESHOLD: f64 = 0.55;
/// chunk 内 4×4 采样：水/沙样本数 ≥7（>0.4）分别判 ocean/beach。
const BIOME_SAMPLE_OFFSETS: [i32; 4] = [2, 6, 10, 14];
const BIOME_WATER_SAMPLES: u32 = 7;
const BIOME_SAND_SAMPLES: u32 = 7;

/// 树：密度与形状（v1 使用；v2 复用 salt/干高范围，密度在 features 内）。
pub(super) const TREE_SALT: u64 = 0xA076_1D64_78BD_642F;
const FOREST_TREE_DENSITY: f64 = 0.10; // 1/10 列
const PLAINS_TREE_DENSITY: f64 = 0.025; // 1/40 列
pub(super) const TREE_MIN_TRUNK: i32 = 4;
pub(super) const TREE_MAX_TRUNK: i32 = 6;

/// 洞穴：3D value noise，按 4×4×4 格采样（同格 64 个方块一起挖，省 CPU），
/// y ∈ (MIN_Y+8, h-8) 且只挖石头。P16 只有地下数据、不可见，形状精度不是重点。
const CAVE_SCALE: f64 = 24.0;
const CAVE_SALT: u64 = 0xE703_7ED1_A0B4_28DB;
const CAVE_THRESHOLD: f64 = 0.63;
const CAVE_CELL: i32 = 4;
const CAVE_CELLS_XZ: usize = 4; // 16/4
const CAVE_CELLS_Y: usize = ((MAX_SURFACE_Y - 8 - MIN_Y) / CAVE_CELL + 1) as usize;

/// 矿：只替换 stone，确定性 hash 概率；坐标预筛选保留 1/8 方块，
/// 阈值在选中方块内放大 8 倍，整体仍约 1.0% 煤 / 0.8% 铁。
pub(super) const ORE_SALT: u64 = 0x8EBC_6AF0_9C88_C6E3;
const ORE_SELECT_ONE_IN: u32 = 8;
const COAL_RATE_PER_MILLE: u32 = 10; // 1.0%
const IRON_RATE_PER_MILLE: u32 = 8; // 0.8%
const COAL_MIN_Y: i32 = 0;
const COAL_MAX_Y: i32 = 80;
const IRON_MIN_Y: i32 = -40;
const IRON_MAX_Y: i32 = 56;
const COAL_ORE_THRESHOLD: u32 = COAL_RATE_PER_MILLE * ORE_SELECT_ONE_IN;
const IRON_ORE_THRESHOLD: u32 = (COAL_RATE_PER_MILLE + IRON_RATE_PER_MILLE) * ORE_SELECT_ONE_IN;

/// 一个 section 的方块数（16×16×16）。
const SECTION_VOLUME: usize = 4096;
/// 256 列（16×16）。
const CHUNK_COLUMNS: usize = 256;

/// 树扫描范围：base-2..=base+17（涵盖树冠半径 2），高度缓存再外扩 1 格用于坡度。
pub(super) const TREE_SCAN_MIN: i32 = -2;
pub(super) const TREE_SCAN_MAX: i32 = 17;
pub(super) const HEIGHT_CACHE_MIN: i32 = -3;
pub(super) const HEIGHT_CACHE_SIDE: i32 = 22; // -3..=18

/// 候选树：世界坐标 + 地表 y + 树干高度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tree {
    x: i32,
    z: i32,
    ground_y: i32,
    trunk_h: i32,
}

impl Tree {
    /// 树干顶（树冠以它为基准：-2/-1 两层 5×5，0 层 3×3，+1 层十字）。
    fn top_y(&self) -> i32 {
        self.ground_y + self.trunk_h
    }
}

/// 生成器版本：
/// - `V1`（legacy）永久冻结，是所有旧 seed / 现有测试的默认口径；
/// - `V2` 仅对新 seed 或显式配置启用：不改高度场，只补矿种、地表材质、
///   多树种与草花、静态水岸植被。
///
/// 版本变化意味着基底输出变化：必须让 `ChunkCache` 全失效，并对 redb 的
/// `meta.generator` 做启动校验（见 `store::WorldStore::ensure_generator_version`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorVersion {
    V1,
    V2,
}

impl Default for GeneratorVersion {
    fn default() -> Self {
        Self::V1
    }
}

impl GeneratorVersion {
    pub const fn number(self) -> u32 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
        }
    }

    pub const fn from_number(n: u32) -> Option<Self> {
        match n {
            1 => Some(Self::V1),
            2 => Some(Self::V2),
            _ => None,
        }
    }
}

impl Default for WorldGen {
    fn default() -> Self {
        Self::new(DEFAULT_SEED)
    }
}

/// 确定性世界生成器：同一 `(seed, version, 坐标)` 永远生成同一地形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldGen {
    pub seed: i64,
    pub version: GeneratorVersion,
}

impl WorldGen {
    /// 默认构造器固定 v1：所有旧调用方/旧测试逐字节不变。
    pub fn new(seed: i64) -> Self {
        Self::with_version(seed, GeneratorVersion::V1)
    }

    /// 显式指定生成器版本（v2 只应由新 seed 或显式配置启用）。
    pub fn with_version(seed: i64, version: GeneratorVersion) -> Self {
        Self { seed, version }
    }

    /// 世界坐标 (x,z) 的地表 y（草/沙方块），已包含出生点平滑保护。
    /// v1 是永久冻结的 legacy 公式；v2 在出生保护圈外叠加侵蚀/山脊/河流。
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        match self.version {
            GeneratorVersion::V1 => {
                let fx = x as f64;
                let fz = z as f64;
                let mut h = BASE_HEIGHT;
                for i in 0..OCTAVE_SCALES.len() {
                    h += value_noise_2d(self.seed, fx, fz, OCTAVE_SCALES[i], OCTAVE_SALTS[i])
                        * OCTAVE_AMPLITUDES[i];
                }
                let h = (h.round() as i32).clamp(MIN_SURFACE_Y, MAX_SURFACE_Y);
                self.spawn_protection(x, z, h)
            }
            GeneratorVersion::V2 => terrain::height_at_v2(self, x, z),
        }
    }

    /// 出生点干燥保护：半径 16 内低地按 smootherstep 抬到 66，半径 4.5 内为平台。
    /// 只抬不降；smootherstep 两端导数为 0，相邻列高度差仍 ≤3。
    fn spawn_protection(&self, x: i32, z: i32, h: i32) -> i32 {
        if h >= SPAWN_DRY_SURFACE_Y {
            return h;
        }
        let dx = (x - SPAWN_BLOCK_X) as f64;
        let dz = (z - SPAWN_BLOCK_Z) as f64;
        let distance = (dx * dx + dz * dz).sqrt();
        if distance >= SPAWN_PROTECT_RADIUS {
            return h;
        }
        let weight = if distance <= SPAWN_PLATEAU_RADIUS {
            1.0
        } else {
            smootherstep(
                (SPAWN_PROTECT_RADIUS - distance)
                    / (SPAWN_PROTECT_RADIUS - SPAWN_PLATEAU_RADIUS),
            )
        };
        let raised =
            h as f64 + (SPAWN_DRY_SURFACE_Y - h) as f64 * weight;
        (raised.round() as i32).max(h)
    }

    /// 出生点：x/z 保留 369/381（chunk 23,23），y 为地表上方 1 格（P16 起 ≥67）。
    pub fn spawn_pos(&self) -> BlockPos {
        let x = SPAWN_BLOCK_X;
        let z = SPAWN_BLOCK_Z;
        BlockPos(x, self.height_at(x, z) + 1, z)
    }

    /// 出生点所在区块（视距中心）。
    pub fn spawn_chunk(&self) -> ChunkPos {
        ChunkPos(SPAWN_BLOCK_X.div_euclid(16), SPAWN_BLOCK_Z.div_euclid(16))
    }

    /// 列 (x,z) 所在 chunk 的单一 biome（26.3 网络 biome ID）。
    pub fn biome_at(&self, x: i32, z: i32) -> i32 {
        self.chunk_biome(ChunkPos(x.div_euclid(16), z.div_euclid(16)))
    }

    /// chunk 单一 biome：先看水/沙占比（4×4 采样），再看低频森林噪声。
    /// 出生 chunk 若算成 ocean/beach（出生保护圈外仍是低地）则强制为 forest/plains，
    /// 避免玩家出生在“沙滩/海底”biome 里。
    pub fn chunk_biome(&self, pos: ChunkPos) -> i32 {
        let (water, sand) = self.chunk_water_sand(pos);
        let forest = value_noise_2d(
            self.seed,
            (pos.0 * 16 + 8) as f64,
            (pos.1 * 16 + 8) as f64,
            FOREST_SCALE,
            FOREST_SALT,
        );
        let biome = biome_from_stats(water, sand, forest);
        let wet = biome == mc_protocol::packets::OCEAN_BIOME_ID_777
            || biome == mc_protocol::packets::BEACH_BIOME_ID_777;
        if wet && pos == self.spawn_chunk() {
            return if forest > FOREST_THRESHOLD {
                mc_protocol::packets::FOREST_BIOME_ID_777
            } else {
                mc_protocol::packets::PLAINS_BIOME_ID_777
            };
        }
        biome
    }

    /// chunk 内 4×4 列采样的 (水下样本数, 沙样本数)。
    fn chunk_water_sand(&self, pos: ChunkPos) -> (u32, u32) {
        let mut water = 0;
        let mut sand = 0;
        for dx in BIOME_SAMPLE_OFFSETS {
            for dz in BIOME_SAMPLE_OFFSETS {
                let h = self.height_at(pos.0 * 16 + dx, pos.1 * 16 + dz);
                if h < SEA_LEVEL {
                    water += 1;
                }
                if h <= SEA_LEVEL + 2 {
                    sand += 1;
                }
            }
        }
        (water, sand)
    }

    /// 生成一个完整 chunk：24 个 section + 地形高度 + MOTION_BLOCKING + biome。
    pub fn generate(&self, pos: ChunkPos) -> GeneratedChunk {
        match self.version {
            GeneratorVersion::V1 => self.generate_v1(pos),
            GeneratorVersion::V2 => self.generate_v2(pos),
        }
    }

    /// v1 legacy 基底：永久冻结，算法与字节输出不得改动。
    fn generate_v1(&self, pos: ChunkPos) -> GeneratedChunk {
        let base_x = pos.0 * 16;
        let base_z = pos.1 * 16;
        let mut sections: Vec<Box<[u16; SECTION_VOLUME]>> = (0..SECTION_COUNT)
            .map(|_| Box::new([AIR_BLOCK_ID; SECTION_VOLUME]))
            .collect();
        let mut heights = [0i32; CHUNK_COLUMNS];
        let mut motion = [0i32; CHUNK_COLUMNS];
        // 扩展高度缓存：树扫描 ±2，坡度再看 ±1，故 base-3..=base+18。
        let mut hcache = [0i32; (HEIGHT_CACHE_SIDE * HEIGHT_CACHE_SIDE) as usize];
        for oz in HEIGHT_CACHE_MIN..(HEIGHT_CACHE_MIN + HEIGHT_CACHE_SIDE) {
            for ox in HEIGHT_CACHE_MIN..(HEIGHT_CACHE_MIN + HEIGHT_CACHE_SIDE) {
                hcache[height_cache_index(ox, oz)] =
                    self.height_at(base_x + ox, base_z + oz);
            }
        }
        let caves = self.cave_mask(base_x, base_z);
        for lz in 0..16i32 {
            for lx in 0..16i32 {
                let column = (lz * 16 + lx) as usize;
                let h = hcache[height_cache_index(lx, lz)];
                heights[column] = h;
                let sandy = h <= SEA_LEVEL + 2;
                let wx = base_x + lx;
                let wz = base_z + lz;
                for y in MIN_Y..=h {
                    let mut id = if y == h {
                        if sandy {
                            SAND_BLOCK_ID
                        } else {
                            GRASS_BLOCK_ID
                        }
                    } else if y >= h - 3 {
                        if sandy {
                            SAND_BLOCK_ID
                        } else {
                            DIRT_BLOCK_ID
                        }
                    } else {
                        STONE_BLOCK_ID
                    };
                    if id == STONE_BLOCK_ID {
                        if y <= h - 9 && y >= MIN_Y + 9 && cave_carved(&caves, lx, y, lz) {
                            id = AIR_BLOCK_ID;
                        } else if let Some(ore) = self.ore_at(wx, y, wz) {
                            id = ore;
                        }
                    }
                    put_block(&mut sections, lx, y, lz, id);
                }
                if h < SEA_LEVEL {
                    for y in (h + 1)..=SEA_LEVEL {
                        put_block(&mut sections, lx, y, lz, WATER_BLOCK_ID);
                    }
                }
                let blocking_top = h.max(SEA_LEVEL);
                motion[column] = blocking_top + 1 - MIN_Y;
            }
        }
        self.place_trees(base_x, base_z, &hcache, &mut sections, &mut motion);
        let biome_id = self.chunk_biome(pos);
        GeneratedChunk {
            pos,
            sections,
            heights,
            motion_heights: motion,
            biome_id,
        }
    }

    /// v2 基底：高度场/biome/洞穴/编码与 v1 完全一致，只替换地表材质、
    /// 矿物集合，并追加多树种、草花与静态水岸植被。仍是单 chunk 纯函数。
    fn generate_v2(&self, pos: ChunkPos) -> GeneratedChunk {
        let base_x = pos.0 * 16;
        let base_z = pos.1 * 16;
        let mut sections: Vec<Box<[u16; SECTION_VOLUME]>> = (0..SECTION_COUNT)
            .map(|_| Box::new([AIR_BLOCK_ID; SECTION_VOLUME]))
            .collect();
        let mut heights = [0i32; CHUNK_COLUMNS];
        let mut motion = [0i32; CHUNK_COLUMNS];
        let mut hcache = [0i32; (HEIGHT_CACHE_SIDE * HEIGHT_CACHE_SIDE) as usize];
        for oz in HEIGHT_CACHE_MIN..(HEIGHT_CACHE_MIN + HEIGHT_CACHE_SIDE) {
            for ox in HEIGHT_CACHE_MIN..(HEIGHT_CACHE_MIN + HEIGHT_CACHE_SIDE) {
                hcache[height_cache_index(ox, oz)] = self.height_at(base_x + ox, base_z + oz);
            }
        }
        let caves = caves::CaveMask::new(self.seed, base_x, base_z);
        let biome_id = self.chunk_biome(pos);
        for lz in 0..16i32 {
            for lx in 0..16i32 {
                let column = (lz * 16 + lx) as usize;
                let h = hcache[height_cache_index(lx, lz)];
                heights[column] = h;
                let wx = base_x + lx;
                let wz = base_z + lz;
                let layers = features::surface_layers(self.seed, wx, wz, h, biome_id);
                for y in MIN_Y..=h {
                    let d = (h - y) as usize;
                    let mut id = if d < layers.len() {
                        layers[d]
                    } else {
                        STONE_BLOCK_ID
                    };
                    if id == STONE_BLOCK_ID {
                        if y <= h - 9 && y >= MIN_Y + 9 && caves.carved(wx, lx, y, wz, lz) {
                            id = AIR_BLOCK_ID;
                        } else if let Some(ore) = ores::ore_at(self.seed, wx, y, wz) {
                            id = ore;
                        }
                    }
                    put_block(&mut sections, lx, y, lz, id);
                }
                if h < SEA_LEVEL {
                    // 底部 h+1..=SEA_LEVEL-1 保持 source water；顶层可能是冰。
                    for y in (h + 1)..SEA_LEVEL {
                        put_block(&mut sections, lx, y, lz, WATER_BLOCK_ID);
                    }
                    put_block(&mut sections, lx, SEA_LEVEL, lz, features::water_at(self.seed, wx, wz, h));
                }
                let blocking_top = h.max(SEA_LEVEL);
                motion[column] = blocking_top + 1 - MIN_Y;
            }
        }
        features::place_trees(self, base_x, base_z, &hcache, &mut sections, &mut motion);
        features::place_plants(self.seed, base_x, base_z, &hcache, &mut sections);
        // P46：单 chunk 小遗迹（纯函数，仅 v2）；光源供 block light 扫描。
        ruins::place_ruin(self, pos, &hcache, &mut sections, &mut motion);
        GeneratedChunk {
            pos,
            sections,
            heights,
            motion_heights: motion,
            biome_id,
        }
    }

    /// 3D 洞穴掩码：每个 4×4×4 格采样一次，索引 `(cz*4+cx)*CAVE_CELLS_Y + cy`。
    fn cave_mask(&self, base_x: i32, base_z: i32) -> Vec<bool> {
        let mut mask = vec![false; CAVE_CELLS_XZ * CAVE_CELLS_XZ * CAVE_CELLS_Y];
        for cx in 0..CAVE_CELLS_XZ {
            for cz in 0..CAVE_CELLS_XZ {
                let wx = base_x + cx as i32 * CAVE_CELL + 1;
                let wz = base_z + cz as i32 * CAVE_CELL + 1;
                let base = (cz * CAVE_CELLS_XZ + cx) * CAVE_CELLS_Y;
                for cy in 0..CAVE_CELLS_Y {
                    let wy = MIN_Y + cy as i32 * CAVE_CELL + 1;
                    mask[base + cy] = value_noise_3d(
                        self.seed,
                        wx as f64,
                        wy as f64,
                        wz as f64,
                        CAVE_SCALE,
                        CAVE_SALT,
                    ) > CAVE_THRESHOLD;
                }
            }
        }
        mask
    }

    /// 矿：只对 stone 调用；先用坐标奇偶预筛掉 7/8 的方块，再投骰子，
    /// 概率在选中方块内放大 8 倍以保持整体约 1.0% 煤 / 0.8% 铁。
    fn ore_at(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        let in_coal = (COAL_MIN_Y..=COAL_MAX_Y).contains(&y);
        let in_iron = (IRON_MIN_Y..=IRON_MAX_Y).contains(&y);
        if !in_coal && !in_iron {
            return None;
        }
        let select = (x as u32).wrapping_mul(0x9E37_79B1)
            ^ (y as u32).wrapping_mul(0x85EB_CA77)
            ^ (z as u32).wrapping_mul(0xC2B2_AE3D);
        if select & 7 != 0 {
            return None;
        }
        let roll =
            (hash01(self.seed, x as i64, y as i64, z as i64, ORE_SALT) * 1000.0) as u32;
        if in_coal && roll < COAL_ORE_THRESHOLD {
            Some(COAL_ORE_BLOCK_ID)
        } else if in_iron && (COAL_ORE_THRESHOLD..IRON_ORE_THRESHOLD).contains(&roll) {
            Some(IRON_ORE_BLOCK_ID)
        } else {
            None
        }
    }

    /// 扫描 base-2..=base+17 的候选树，只写入落在本 chunk 内的方块，保证跨 chunk 连续。
    fn place_trees(
        &self,
        base_x: i32,
        base_z: i32,
        hcache: &[i32],
        sections: &mut [Box<[u16; SECTION_VOLUME]>],
        motion: &mut [i32; CHUNK_COLUMNS],
    ) {
        let mut biome_cache: Vec<((i32, i32), i32)> = Vec::with_capacity(9);
        for oz in TREE_SCAN_MIN..=TREE_SCAN_MAX {
            for ox in TREE_SCAN_MIN..=TREE_SCAN_MAX {
                let wx = base_x + ox;
                let wz = base_z + oz;
                let h = hcache[height_cache_index(ox, oz)];
                let slope_ok = (hcache[height_cache_index(ox - 1, oz)] - h).abs() <= 1
                    && (hcache[height_cache_index(ox + 1, oz)] - h).abs() <= 1
                    && (hcache[height_cache_index(ox, oz - 1)] - h).abs() <= 1
                    && (hcache[height_cache_index(ox, oz + 1)] - h).abs() <= 1;
                let biome = self.cached_chunk_biome(
                    &mut biome_cache,
                    wx.div_euclid(16),
                    wz.div_euclid(16),
                );
                let Some(tree) = self.tree_for(wx, wz, h, slope_ok, biome) else {
                    continue;
                };
                self.write_tree(&tree, base_x, base_z, sections, motion);
            }
        }
    }

    fn cached_chunk_biome(
        &self,
        cache: &mut Vec<((i32, i32), i32)>,
        cx: i32,
        cz: i32,
    ) -> i32 {
        if let Some(&(_, biome)) = cache.iter().find(|(pos, _)| *pos == (cx, cz)) {
            return biome;
        }
        let biome = self.chunk_biome(ChunkPos(cx, cz));
        cache.push(((cx, cz), biome));
        biome
    }

    /// 单列树判定：地块干燥、坡度平缓、不在出生点 4 格内，再按 biome 密度投骰子。
    fn tree_for(&self, x: i32, z: i32, h: i32, slope_ok: bool, biome: i32) -> Option<Tree> {
        if h <= SEA_LEVEL + 2 || !slope_ok {
            return None;
        }
        if (x - SPAWN_BLOCK_X).abs() <= SPAWN_TREE_CLEAR
            && (z - SPAWN_BLOCK_Z).abs() <= SPAWN_TREE_CLEAR
        {
            return None;
        }
        let density = match biome {
            mc_protocol::packets::FOREST_BIOME_ID_777 => FOREST_TREE_DENSITY,
            mc_protocol::packets::PLAINS_BIOME_ID_777 => PLAINS_TREE_DENSITY,
            _ => return None,
        };
        if hash01(self.seed, x as i64, 7, z as i64, TREE_SALT) >= density {
            return None;
        }
        let trunk_h = TREE_MIN_TRUNK
            + (hash01(self.seed, x as i64, 8, z as i64, TREE_SALT ^ 0x55)
                * (TREE_MAX_TRUNK - TREE_MIN_TRUNK + 1) as f64) as i32;
        Some(Tree {
            x,
            z,
            ground_y: h,
            trunk_h,
        })
    }

    /// 写入一棵树：树干优先（可覆盖树叶），树叶只覆盖 air/leaves；只写本 chunk 内方块。
    fn write_tree(
        &self,
        tree: &Tree,
        base_x: i32,
        base_z: i32,
        sections: &mut [Box<[u16; SECTION_VOLUME]>],
        motion: &mut [i32; CHUNK_COLUMNS],
    ) {
        let lx = tree.x - base_x;
        let lz = tree.z - base_z;
        let top = tree.top_y();
        for y in (tree.ground_y + 1)..=top {
            if set_block_if(
                sections,
                lx,
                y,
                lz,
                OAK_LOG_BLOCK_ID,
                &[AIR_BLOCK_ID, OAK_LEAVES_BLOCK_ID],
            ) {
                update_motion(motion, lx, lz, y);
            }
        }
        for dy in -2i32..=1 {
            let y = top + dy;
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    let shape = match dy {
                        -2 | -1 => !(dx.abs() == 2 && dz.abs() == 2),
                        0 => dx.abs() <= 1 && dz.abs() <= 1,
                        _ => dx.abs() + dz.abs() <= 1,
                    };
                    if !shape {
                        continue;
                    }
                    if set_block_if(
                        sections,
                        lx + dx,
                        y,
                        lz + dz,
                        OAK_LEAVES_BLOCK_ID,
                        &[AIR_BLOCK_ID, OAK_LEAVES_BLOCK_ID],
                    ) {
                        update_motion(motion, lx + dx, lz + dz, y);
                    }
                }
            }
        }
    }
}

fn biome_from_stats(water: u32, sand: u32, forest: f64) -> i32 {
    use mc_protocol::packets::{
        BEACH_BIOME_ID_777, FOREST_BIOME_ID_777, OCEAN_BIOME_ID_777, PLAINS_BIOME_ID_777,
    };
    if water >= BIOME_WATER_SAMPLES {
        OCEAN_BIOME_ID_777
    } else if sand >= BIOME_SAND_SAMPLES {
        BEACH_BIOME_ID_777
    } else if forest > FOREST_THRESHOLD {
        FOREST_BIOME_ID_777
    } else {
        PLAINS_BIOME_ID_777
    }
}

pub(super) fn height_cache_index(ox: i32, oz: i32) -> usize {
    ((oz - HEIGHT_CACHE_MIN) * HEIGHT_CACHE_SIDE + (ox - HEIGHT_CACHE_MIN)) as usize
}

fn cave_carved(mask: &[bool], lx: i32, y: i32, lz: i32) -> bool {
    let cx = (lx / CAVE_CELL) as usize;
    let cz = (lz / CAVE_CELL) as usize;
    let cy = ((y - MIN_Y) / CAVE_CELL) as usize;
    mask[(cz * CAVE_CELLS_XZ + cx) * CAVE_CELLS_Y + cy]
}

fn put_block(
    sections: &mut [Box<[u16; SECTION_VOLUME]>],
    lx: i32,
    y: i32,
    lz: i32,
    id: u16,
) {
    if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
        return;
    }
    let section = ((y - MIN_Y) >> 4) as usize;
    if section >= sections.len() {
        return;
    }
    let local_y = ((y - MIN_Y) & 15) as usize;
    sections[section][local_y * CHUNK_COLUMNS + lz as usize * 16 + lx as usize] = id;
}

/// 仅当目标方块属于 `replaceable` 时写入；返回是否真的写入。
pub(super) fn set_block_if(
    sections: &mut [Box<[u16; SECTION_VOLUME]>],
    lx: i32,
    y: i32,
    lz: i32,
    id: u16,
    replaceable: &[u16],
) -> bool {
    if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
        return false;
    }
    let section = ((y - MIN_Y) >> 4) as usize;
    if section >= sections.len() {
        return false;
    }
    let local_y = ((y - MIN_Y) & 15) as usize;
    let idx = local_y * CHUNK_COLUMNS + lz as usize * 16 + lx as usize;
    if !replaceable.contains(&sections[section][idx]) {
        return false;
    }
    sections[section][idx] = id;
    true
}

fn update_motion(motion: &mut [i32; CHUNK_COLUMNS], lx: i32, lz: i32, y: i32) {
    if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
        return;
    }
    let column = (lz * 16 + lx) as usize;
    let value = y + 1 - MIN_Y;
    if value > motion[column] {
        motion[column] = value;
    }
}
