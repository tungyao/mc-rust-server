//! worldgen: 由 `server.toml` seed 驱动的确定性 value-noise 地形，只兼容 26.3/777。
//!
//! 算法（无第三方依赖）：
//! - 世界坐标的 3 层平滑 value noise（scale 128/48/24，幅度 20/10/6 格）；
//! - 基高 58，因此地表 y ∈ 58..=94，丘陵幅度约 36 格；
//! - 每列：地表 grass_block(9)、下面 3 格 dirt(10)、再下面 stone(1) 到 MIN_Y，上面 air(0)；
//! - 高度图按该 chunk 256 列真实高度打包；天空光从最低地表 section 向上 15 级。
//!
//! 相邻 chunk 直接用世界坐标采样，因此边界连续；`WorldGen` 只保存 seed。
use crate::{
    AIR_BLOCK_ID, DIRT_BLOCK_ID, GRASS_BLOCK_ID, MIN_Y, SECTION_COUNT, SPAWN_BLOCK_X,
    SPAWN_BLOCK_Z, STONE_BLOCK_ID,
};

/// 默认世界种子（与 `server.toml` 默认值一致）。
pub const DEFAULT_SEED: i64 = 123456789;
/// 地表最低/最高 y（草方块所在 y）。出生点 y = 地表 y + 1。
pub const MIN_SURFACE_Y: i32 = 58;
pub const MAX_SURFACE_Y: i32 = 94;
/// 基高与 3 层噪声幅度（合计 36，落在 58..94）。
const BASE_HEIGHT: f64 = 58.0;
const OCTAVE_SCALES: [f64; 3] = [128.0, 48.0, 24.0];
const OCTAVE_AMPLITUDES: [f64; 3] = [20.0, 10.0, 6.0];
const OCTAVE_SALTS: [u64; 3] = [
    0x9E37_79B9_7F4A_7C15,
    0xC2B2_AE3D_27D4_EB4F,
    0x1656_67B1_9E37_79F9,
];
/// 一个 section 的方块数（16×16×16）。
const SECTION_VOLUME: usize = 4096;
/// 256 列（16×16）。
const CHUNK_COLUMNS: usize = 256;

/// 确定性世界生成器：同一 seed + 坐标永远生成同一地形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldGen {
    pub seed: i64,
}

impl Default for WorldGen {
    fn default() -> Self {
        Self::new(DEFAULT_SEED)
    }
}

impl WorldGen {
    pub fn new(seed: i64) -> Self {
        Self { seed }
    }

    /// 世界坐标 (x,z) 的地表 y（最高非空气方块 = 草方块）。
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        let fx = x as f64;
        let fz = z as f64;
        let mut h = BASE_HEIGHT;
        for i in 0..OCTAVE_SCALES.len() {
            h += self.value_noise(fx, fz, OCTAVE_SCALES[i], OCTAVE_SALTS[i])
                * OCTAVE_AMPLITUDES[i];
        }
        (h.round() as i32).clamp(MIN_SURFACE_Y, MAX_SURFACE_Y)
    }

    /// 出生点：x/z 保留 369/381（chunk 23,23），y 为地表上方 1 格。
    pub fn spawn_pos(&self) -> mc_core::BlockPos {
        let x = SPAWN_BLOCK_X;
        let z = SPAWN_BLOCK_Z;
        mc_core::BlockPos(x, self.height_at(x, z) + 1, z)
    }

    /// 出生点所在区块（视距中心）。
    pub fn spawn_chunk(&self) -> mc_core::ChunkPos {
        mc_core::ChunkPos(
            SPAWN_BLOCK_X.div_euclid(16),
            SPAWN_BLOCK_Z.div_euclid(16),
        )
    }

    /// 生成一个完整 chunk：24 个 section（每节 4096 个真实 blockstate ID）+ 256 列高度。
    pub fn generate(&self, pos: mc_core::ChunkPos) -> GeneratedChunk {
        let mut sections: Vec<Box<[u16; SECTION_VOLUME]>> = (0..SECTION_COUNT)
            .map(|_| Box::new([AIR_BLOCK_ID; SECTION_VOLUME]))
            .collect();
        let mut heights = [0i32; CHUNK_COLUMNS];
        let base_x = pos.0 * 16;
        let base_z = pos.1 * 16;
        for lz in 0..16 {
            for lx in 0..16 {
                let h = self.height_at(base_x + lx, base_z + lz);
                heights[(lz * 16 + lx) as usize] = h;
                for y in MIN_Y..=h {
                    let id = if y == h {
                        GRASS_BLOCK_ID
                    } else if y >= h - 3 {
                        DIRT_BLOCK_ID
                    } else {
                        STONE_BLOCK_ID
                    };
                    let section_idx = ((y - MIN_Y) >> 4) as usize;
                    let local_y = ((y - MIN_Y) & 15) as usize;
                    let idx = local_y * CHUNK_COLUMNS + lz as usize * 16 + lx as usize;
                    sections[section_idx][idx] = id;
                }
            }
        }
        GeneratedChunk { pos, sections, heights }
    }

    /// 3 层平滑 value noise：每个格点 hash，quintic 插值。
    fn value_noise(&self, x: f64, z: f64, scale: f64, salt: u64) -> f64 {
        let fx = x / scale;
        let fz = z / scale;
        let x0f = fx.floor();
        let z0f = fz.floor();
        let tx = smootherstep(fx - x0f);
        let tz = smootherstep(fz - z0f);
        let x0 = x0f as i64;
        let z0 = z0f as i64;
        let v00 = self.lattice(x0, z0, salt);
        let v10 = self.lattice(x0 + 1, z0, salt);
        let v01 = self.lattice(x0, z0 + 1, salt);
        let v11 = self.lattice(x0 + 1, z0 + 1, salt);
        let a = lerp(v00, v10, tx);
        let b = lerp(v01, v11, tx);
        lerp(a, b, tz)
    }

    /// 格点 hash → [0,1)。
    fn lattice(&self, x: i64, z: i64, salt: u64) -> f64 {
        let mut h = (x as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (self.seed as u64)
            ^ salt;
        h = mix64(h);
        // 取高 53 位，保证 f64 精确可表示。
        ((h >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
    }
}

fn smootherstep(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 生成后的 chunk：真实逐列高度 + 24 个 section 的 blockstate ID。
#[derive(Clone, PartialEq, Eq)]
pub struct GeneratedChunk {
    pub pos: mc_core::ChunkPos,
    /// 24 节，每节 4096 项，索引 `y*256 + z*16 + x`（section 内局部坐标）。
    pub sections: Vec<Box<[u16; SECTION_VOLUME]>>,
    /// 256 列地表 y（草方块 y），索引 `z*16+x`。
    pub heights: [i32; CHUNK_COLUMNS],
}

impl GeneratedChunk {
    /// ChunkData 的 Data 字段：24 节网络字节串接（真实 palette/block_count）。
    pub fn network_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.sections.len() * 16);
        let mut states = [0i32; SECTION_VOLUME];
        for section in &self.sections {
            let first = section[0];
            if section.iter().all(|&b| b == first) {
                if first == AIR_BLOCK_ID {
                    mc_protocol::packets::encode_empty_section_placeholder(&mut out);
                } else {
                    mc_protocol::packets::encode_uniform_section_single(
                        first as i32,
                        mc_protocol::packets::PLAINS_BIOME_ID_777,
                        SECTION_VOLUME as u16,
                        0,
                        &mut out,
                    );
                }
            } else {
                for (i, &b) in section.iter().enumerate() {
                    states[i] = b as i32;
                }
                mc_protocol::packets::encode_block_section(
                    &states,
                    mc_protocol::packets::PLAINS_BIOME_ID_777,
                    &mut out,
                );
            }
        }
        out
    }

    /// 真实 MOTION_BLOCKING 高度图：每列 `地表 y + 1 - MIN_Y`。
    pub fn heightmaps_bytes(&self) -> Vec<u8> {
        let mut columns = [0i32; CHUNK_COLUMNS];
        for (i, &h) in self.heights.iter().enumerate() {
            columns[i] = h + 1 - MIN_Y;
        }
        mc_protocol::packets::motion_blocking_heightmap(&columns)
    }

    /// 天空光：从本 chunk 最低地表所在 section 向上发 15 级，地下不发。
    pub fn light_bytes(&self) -> Vec<u8> {
        let min_h = *self.heights.iter().min().unwrap_or(&MIN_SURFACE_Y);
        let first_lit = ((min_h - MIN_Y) / 16).clamp(0, SECTION_COUNT as i32 - 1) as u32;
        let mask = ((1u64 << SECTION_COUNT) - 1) & !((1u64 << first_lit) - 1);
        let mut out = Vec::new();
        mc_protocol::packets::encode_sky_light_data(mask, 15, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_at(g: &GeneratedChunk, lx: i32, y: i32, lz: i32) -> u16 {
        let section = ((y - MIN_Y) >> 4) as usize;
        let local_y = ((y - MIN_Y) & 15) as usize;
        g.sections[section][local_y * 256 + lz as usize * 16 + lx as usize]
    }

    #[test]
    fn same_seed_same_chunk_is_identical() {
        let gen = WorldGen::new(DEFAULT_SEED);
        let a = gen.generate(mc_core::ChunkPos(23, 23));
        let b = gen.generate(mc_core::ChunkPos(23, 23));
        assert_eq!(a.heights, b.heights);
        assert_eq!(a.sections, b.sections);
        // 跨实例同 seed 也一致。
        let c = WorldGen::new(DEFAULT_SEED).generate(mc_core::ChunkPos(23, 23));
        assert_eq!(a.sections, c.sections);
    }

    #[test]
    fn different_seed_changes_terrain() {
        let a = WorldGen::new(DEFAULT_SEED);
        let b = WorldGen::new(42);
        let mut diff = 0;
        for x in (320..=420).step_by(4) {
            for z in (330..=430).step_by(4) {
                if a.height_at(x, z) != b.height_at(x, z) {
                    diff += 1;
                }
            }
        }
        assert!(diff > 20, "different seeds must produce visibly different terrain, diff={diff}");
        let g1 = a.generate(mc_core::ChunkPos(23, 23));
        let g2 = b.generate(mc_core::ChunkPos(23, 23));
        assert_ne!(g1.heights, g2.heights);
        assert_ne!(g1.sections, g2.sections);
    }

    #[test]
    fn heights_are_bounded_and_adjacent_difference_is_small() {
        let gen = WorldGen::new(987654321);
        let mut prev_row = vec![0i32; 97];
        for z in -48..=48 {
            let mut row = Vec::with_capacity(97);
            for x in -48..=48 {
                let h = gen.height_at(x, z);
                assert!((MIN_SURFACE_Y..=MAX_SURFACE_Y).contains(&h), "height {h} at {x},{z}");
                row.push(h);
            }
            for pair in row.windows(2) {
                assert!(
                    (pair[0] - pair[1]).abs() <= 3,
                    "adjacent columns differ too much: {pair:?}"
                );
            }
            if z > -48 {
                for (x, (&h, &prev)) in row.iter().zip(prev_row.iter()).enumerate() {
                    assert!(
                        (h - prev).abs() <= 3,
                        "z-adjacent columns differ too much at x={}: {h} vs {prev}",
                        x as i32 - 48
                    );
                }
            }
            prev_row = row;
        }
    }

    #[test]
    fn chunk_boundaries_and_generated_heights_are_continuous() {
        let gen = WorldGen::new(DEFAULT_SEED);
        let west = gen.generate(mc_core::ChunkPos(0, 0));
        let east = gen.generate(mc_core::ChunkPos(1, 0));
        for lz in 0..16 {
            for lx in 0..16 {
                let wx = lx;
                let wz = lz;
                assert_eq!(west.heights[(lz * 16 + lx) as usize], gen.height_at(wx, wz));
                assert_eq!(
                    east.heights[(lz * 16 + lx) as usize],
                    gen.height_at(16 + lx, wz),
                    "generated height must match height_at in next chunk"
                );
            }
            // 跨 chunk 边界的相邻列（世界 x=15/16）。
            let a = gen.height_at(15, lz);
            let b = gen.height_at(16, lz);
            assert!((a - b).abs() <= 3, "chunk boundary step {a}->{b} at z={lz}");
        }
    }

    #[test]
    fn layers_are_grass_dirt_stone_and_air() {
        let gen = WorldGen::new(DEFAULT_SEED);
        let g = gen.generate(mc_core::ChunkPos(23, 23));
        for (lx, lz) in [(0, 0), (7, 9), (15, 15)] {
            let h = g.heights[(lz * 16 + lx) as usize];
            assert_eq!(block_at(&g, lx, h, lz), GRASS_BLOCK_ID);
            for d in 1..=3 {
                assert_eq!(block_at(&g, lx, h - d, lz), DIRT_BLOCK_ID, "dirt layer {d}");
            }
            assert_eq!(block_at(&g, lx, h - 4, lz), STONE_BLOCK_ID, "stone below dirt");
            assert_eq!(block_at(&g, lx, h + 1, lz), AIR_BLOCK_ID, "air above grass");
            assert_eq!(block_at(&g, lx, MIN_Y, lz), STONE_BLOCK_ID, "bedrock-ish floor is stone");
        }
    }

    #[test]
    fn spawn_is_one_above_surface_in_expected_chunk() {
        let gen = WorldGen::new(DEFAULT_SEED);
        let spawn = gen.spawn_pos();
        assert_eq!((spawn.0, spawn.2), (SPAWN_BLOCK_X, SPAWN_BLOCK_Z));
        assert_eq!(spawn.1, gen.height_at(SPAWN_BLOCK_X, SPAWN_BLOCK_Z) + 1);
        assert!((MIN_SURFACE_Y + 1..=MAX_SURFACE_Y + 1).contains(&spawn.1));
        assert_eq!(gen.spawn_chunk(), mc_core::ChunkPos(23, 23));
        // 出生点草方块就在脚下一格。
        let g = gen.generate(gen.spawn_chunk());
        let lx = SPAWN_BLOCK_X - 23 * 16;
        let lz = SPAWN_BLOCK_Z - 23 * 16;
        assert_eq!(block_at(&g, lx, spawn.1 - 1, lz), GRASS_BLOCK_ID);
        assert_eq!(block_at(&g, lx, spawn.1, lz), AIR_BLOCK_ID);
    }

    #[test]
    fn heightmap_and_light_follow_actual_columns() {
        let gen = WorldGen::new(DEFAULT_SEED);
        let g = gen.generate(mc_core::ChunkPos(23, 23));
        let mut columns = [0i32; 256];
        for (i, &h) in g.heights.iter().enumerate() {
            columns[i] = h + 1 - MIN_Y;
        }
        assert_eq!(g.heightmaps_bytes(), mc_protocol::packets::motion_blocking_heightmap(&columns));
        let min_h = *g.heights.iter().min().unwrap();
        let first_lit = ((min_h - MIN_Y) / 16) as u32;
        let mask = ((1u64 << SECTION_COUNT) - 1) & !((1u64 << first_lit) - 1);
        let mut expected = Vec::new();
        mc_protocol::packets::encode_sky_light_data(mask, 15, &mut expected);
        assert_eq!(g.light_bytes(), expected);
        assert!(g.light_bytes().len() > 6, "hilly chunk must carry sky light arrays");
        assert!(!g.network_bytes().is_empty());
    }

    #[test]
    fn negative_and_far_coordinates_are_deterministic() {
        let gen = WorldGen::new(-987654321);
        for pos in [mc_core::ChunkPos(-1, -1), mc_core::ChunkPos(-40, 33), mc_core::ChunkPos(1_800_000, -900_000)] {
            let a = gen.generate(pos);
            let b = gen.generate(pos);
            assert_eq!(a.heights, b.heights);
            assert_eq!(a.sections, b.sections);
            assert!(a.heights.iter().all(|h| (MIN_SURFACE_Y..=MAX_SURFACE_Y).contains(h)));
        }
    }

    #[test]
    fn default_seed_matches_server_default() {
        assert_eq!(DEFAULT_SEED, 123456789);
        assert_eq!(WorldGen::default(), WorldGen::new(123456789));
        // 固定回归：默认种子出生点地表 72 → 站立 y=73；另一个种子 y 不同。
        let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
        assert_eq!(spawn, mc_core::BlockPos(369, 73, 381));
        assert_ne!(
            spawn.1,
            WorldGen::new(987654321).spawn_pos().1,
            "different seed must produce a different spawn y"
        );
    }
}
