//! `GeneratedChunk` 与 ChunkData 的 Data/Heightmaps/Light 编码。
//!
//! P16 起每个 section：
//! - `block_count` = 该节非空气方块数（水也算非空气）；
//! - `fluid_count` = 该节 water 方块数（0..=4096）；
//! - 均匀 air 用带 biome 的空 section；均匀水/石用真实 fluid_count 单值容器；
//! - 混合节走间接 palette（BPE 按 palette 大小自适应，>256 走 direct），
//!   并把 fluid_count 一起写入。
use crate::{AIR_BLOCK_ID, MIN_Y, SECTION_COUNT, WATER_BLOCK_ID};

/// 一个 section 的方块数（16×16×16）。
const SECTION_VOLUME: usize = 4096;
/// 256 列（16×16）。
const CHUNK_COLUMNS: usize = 256;

/// 生成后的 chunk：真实逐列高度 + 24 个 section + MOTION_BLOCKING + biome。
#[derive(Clone, PartialEq, Eq)]
pub struct GeneratedChunk {
    pub pos: mc_core::ChunkPos,
    /// 24 节，每节 4096 项，索引 `y*256 + z*16 + x`（section 内局部坐标）。
    pub sections: Vec<Box<[u16; SECTION_VOLUME]>>,
    /// 256 列地形顶（草/沙方块 y），索引 `z*16+x`；spawn/测试/天空光起点用它。
    pub heights: [i32; CHUNK_COLUMNS],
    /// 256 列 MOTION_BLOCKING 高度值（最高阻挡方块 y + 1 - MIN_Y），
    /// 已含 +1 与 min_y 偏移，直接交给 `motion_blocking_heightmap`。
    /// 水面、log、树叶都算阻挡；`heights` 只表示地形顶。
    pub motion_heights: [i32; CHUNK_COLUMNS],
    /// 本 chunk 单一 biome 的 26.3 网络 biome ID（所有 section 共用）。
    pub biome_id: i32,
}

impl GeneratedChunk {
    /// ChunkData 的 Data 字段：24 节网络字节串接（真实 palette/block_count/fluid_count）。
    pub fn network_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.sections.len() * 16);
        let mut states = [0i32; SECTION_VOLUME];
        let section_volume = SECTION_VOLUME as u16;
        for section in &self.sections {
            let first = section[0];
            let uniform = section.iter().all(|&b| b == first);
            if uniform {
                if first == AIR_BLOCK_ID {
                    mc_protocol::packets::encode_empty_section_with_biome(self.biome_id, &mut out);
                } else {
                    let fluid = if first == WATER_BLOCK_ID { section_volume } else { 0 };
                    mc_protocol::packets::encode_uniform_section_single(
                        first as i32,
                        self.biome_id,
                        section_volume,
                        fluid,
                        &mut out,
                    );
                }
                continue;
            }
            let mut fluid_count = 0u16;
            for (i, &b) in section.iter().enumerate() {
                states[i] = b as i32;
                if b == WATER_BLOCK_ID {
                    fluid_count += 1;
                }
            }
            mc_protocol::packets::encode_block_section_with_fluid(
                &states,
                self.biome_id,
                fluid_count,
                &mut out,
            );
        }
        out
    }

    /// 真实 MOTION_BLOCKING 高度图：每列最高阻挡方块（含水面/树）+1-MIN_Y。
    pub fn heightmaps_bytes(&self) -> Vec<u8> {
        mc_protocol::packets::motion_blocking_heightmap(&self.motion_heights)
    }

    /// 天空光：从本 chunk 最低地表所在 section 向上发 15 级，地下不发。
    /// 水面/树不改变起点（否则山谷/树冠会把整段变暗）；水下 15 级为已知简化。
    pub fn light_bytes(&self) -> Vec<u8> {
        let min_h = *self.heights.iter().min().unwrap_or(&MIN_Y);
        let first_lit = ((min_h - MIN_Y) / 16).clamp(0, SECTION_COUNT as i32 - 1) as u32;
        let mask = ((1u64 << SECTION_COUNT) - 1) & !((1u64 << first_lit) - 1);
        let mut out = Vec::new();
        mc_protocol::packets::encode_sky_light_data(mask, 15, &mut out);
        out
    }

    // -----------------------------------------------------------------------
    // P45 v2 编码：v1 的两个方法一字不改；v2 走下面两个显式入口，
    // 由 ChunkCache 按 GeneratorVersion 选择。线格式不变（0x2E）。
    // -----------------------------------------------------------------------

    /// v2 Data 字段：24 节串接，每节 block 容器与 v1 完全相同，
    /// biome 从单值改为 64 项 paletted container（4×4×4 子格）。
    pub fn network_bytes_v2(&self, gen: &super::WorldGen) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.sections.len() * 16);
        let mut states = [0i32; SECTION_VOLUME];
        let section_volume = SECTION_VOLUME as u16;
        // 只采样 16 个水平子格一次，再向 24 个 section 复用。
        let biome_containers = super::biome::biomes_for_chunk(gen, self.pos);
        for (section_index, section) in self.sections.iter().enumerate() {
            let biomes = &biome_containers[section_index];
            let first = section[0];
            let uniform = section.iter().all(|&b| b == first);
            if uniform {
                if first == AIR_BLOCK_ID {
                    mc_protocol::packets::encode_empty_section_with_biomes(&biomes, &mut out);
                } else {
                    let fluid = if first == WATER_BLOCK_ID { section_volume } else { 0 };
                    mc_protocol::packets::encode_uniform_section_single_with_biomes(
                        first as i32,
                        section_volume,
                        fluid,
                        &biomes,
                        &mut out,
                    );
                }
                continue;
            }
            let mut fluid_count = 0u16;
            for (i, &b) in section.iter().enumerate() {
                states[i] = b as i32;
                if b == WATER_BLOCK_ID {
                    fluid_count += 1;
                }
            }
            mc_protocol::packets::encode_block_section_with_fluid_and_biomes(
                &states,
                fluid_count,
                &biomes,
                &mut out,
            );
        }
        out
    }

    /// v2 LightData：按列天空光起点 + 非零 section 的 2048B nibble 数组。
    pub fn light_bytes_v2(&self) -> Vec<u8> {
        let sky = super::light::compute_sky_light(self);
        let block = super::light::compute_block_light(self);
        let mut out = Vec::new();
        // P46：block light 与 P45 天空光合并在同一份 LightData；无光源时 block 为空，
        // 字节与 P45 完全相同。v1 仍走 light_bytes()（encode_sky_light_data）。
        mc_protocol::packets::encode_light_data(&sky, &block, &mut out);
        out
    }
}
