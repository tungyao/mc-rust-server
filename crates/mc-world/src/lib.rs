//! mc-world: Chunk 存储 + 生成 + 存档，不依赖网络
//! 对标 FerrumC world + SteelMC parity 思路
pub mod load;
pub mod mutable;
pub mod save;
pub mod worldgen;

pub use mutable::{
    MutableWorld, DEFAULT_PLACE_BLOCK_ID, EYE_HEIGHT, MAX_REACH, PLACEABLE_BLOCK_IDS,
    WORLD_TOP_Y, chunk_of_block, is_placeable, is_replaceable,
};
pub use worldgen::{
    GeneratedChunk, WorldGen, DEFAULT_SEED, MAX_SURFACE_Y, MIN_SURFACE_Y, SEA_LEVEL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Uniform(u16),
    Paletted,
    Direct,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub pos: mc_core::ChunkPos,
    pub sections: Vec<SectionKind>, // 24 节：整节同值 Uniform，跨层混合 Paletted 占位
}

impl Chunk {
    pub fn empty_flat(pos: mc_core::ChunkPos) -> Self {
        Self { pos, sections: vec![SectionKind::Uniform(0); 24] }
    }
}

// ---------------------------------------------------------------------------
// 旧占位世界：Chunk/save/load 兼容 + 早期 Play 脚手架。
// P14 起 Play 网络路径使用 `worldgen::WorldGen`（seed 驱动、逐列高度）；
// 本文件的 `chunk_for`/`chunk_*_bytes` 仅保留给存档占位与旧测试。
// ---------------------------------------------------------------------------

/// 空世界种子占位（P3 接真实存档/配置）。确定性生成依赖它。
pub const WORLD_SEED: i64 = worldgen::DEFAULT_SEED;

/// 出生点 x/z（方块坐标）：保留原版抓包位置 (369,381) → chunk (23,23)。
/// y 不再写死，由 [`worldgen::WorldGen::spawn_pos`] 按 seed 计算为地表上方 1 格。
pub const SPAWN_BLOCK_X: i32 = 369;
pub const SPAWN_BLOCK_Z: i32 = 381;

/// 方块 ID 与 26.3 `reports/blocks.json` 的全局 blockstate ID 对齐：
/// air=0、stone=1、grass_block（snowy=false）=9、dirt=10。
/// 这些 ID 也用于 Configuration 中回放的 `minecraft:block` 注册表。
/// P16 新增的水/沙/树/矿 ID 来自 26.3 客户端 jar 实测
/// （`SharedConstants.tryDetectVersion(); Bootstrap.bootStrap(); Block.getId(block.defaultBlockState())`），
/// 与 air/stone/grass/dirt 的已知值一致，可直接信任。
pub const AIR_BLOCK_ID: u16 = 0;
pub const STONE_BLOCK_ID: u16 = 1;
pub const GRASS_BLOCK_ID: u16 = 9;
pub const DIRT_BLOCK_ID: u16 = 10;
/// `Blocks.WATER.defaultBlockState()`（source，level=0）。
pub const WATER_BLOCK_ID: u16 = 89;
pub const SAND_BLOCK_ID: u16 = 121;
/// `Blocks.OAK_LOG.defaultBlockState()`（axis=y）。
pub const OAK_LOG_BLOCK_ID: u16 = 140;
pub const OAK_LEAVES_BLOCK_ID: u16 = 294;
pub const COAL_ORE_BLOCK_ID: u16 = 136;
pub const IRON_ORE_BLOCK_ID: u16 = 134;

/// 按 y 分层的地形带：y <= GROUND_LEVEL 石头，61..=63 泥土，64 草方块，其余空气。
pub const GROUND_LEVEL: i32 = 60;
/// 泥土带顶（61..=DIRT_TOP 为泥土）。
pub const DIRT_TOP: i32 = 63;
/// 草方块所在高度（单层盖顶）。
pub const GRASS_LEVEL: i32 = 64;
/// 24 节对应 y=-64..320（min_y=-64，每节 16 格）。
pub const MIN_Y: i32 = -64;
pub const SECTION_COUNT: usize = 24;

/// 按 y 查地形方块（与 x/z 无关，全图同形；P3 按种子接真实高度图）。
/// y<=60 石头，61..=63 泥土，64 草方块，其余空气。
pub fn block_at_y(y: i32) -> u16 {
    if y <= GROUND_LEVEL {
        STONE_BLOCK_ID
    } else if y <= DIRT_TOP {
        DIRT_BLOCK_ID
    } else if y == GRASS_LEVEL {
        GRASS_BLOCK_ID
    } else {
        AIR_BLOCK_ID
    }
}

/// 旧占位：确定性平面 Chunk（save/load 测试兼容）。Play 网络路径请用
/// [`worldgen::WorldGen::generate`]。
/// 按 y 分层填节：整节同值记 Uniform，跨层混合节（48..63 石头+泥土、
/// 64..79 草+空气）记 Paletted 占位，P3 填真实 paletted 容器。
/// `empty_flat` 保留，行为不变。
pub fn chunk_for(pos: mc_core::ChunkPos) -> Chunk {
    let mut sections = Vec::with_capacity(SECTION_COUNT);
    for idx in 0..SECTION_COUNT {
        let base = MIN_Y + idx as i32 * 16;
        let first = block_at_y(base);
        let last = block_at_y(base + 15);
        if first == last {
            sections.push(SectionKind::Uniform(first));
        } else {
            sections.push(SectionKind::Paletted);
        }
    }
    Chunk { pos, sections }
}

/// 视距内区块列表（P5b）：以 `center` 为中心、切比雪夫半径 `radius` 的正方形。
/// `radius=4` 时 `(2*4+1)^2=81` 块。按环递增排序（中心先发），环内按 dx/dz 递增，
/// 保证每 tick 限流 1 块时出生点区块最先到客户端。`radius<0` 返回空。
pub fn view_chunks(center: mc_core::ChunkPos, radius: i32) -> Vec<mc_core::ChunkPos> {
    if radius < 0 {
        return Vec::new();
    }
    let side = (radius as usize) * 2 + 1;
    let mut out = Vec::with_capacity(side * side);
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            out.push(mc_core::ChunkPos(center.0 + dx, center.1 + dz));
        }
    }
    // 中心优先：环（chebyshev 距离）递增，环内 dx/dz 递增（确定性顺序）。
    out.sort_by_key(|p| {
        let dx = p.0 - center.0;
        let dz = p.1 - center.1;
        (dx.abs().max(dz.abs()), dx, dz)
    });
    out
}
/// Chunk 网络字节：24节真布局串接（每节8B起，复用 mc-protocol 单值/间接容器）。
/// 来源：Chunk_format#Chunk_Section结构 + Paletted容器（1.21.5+无data_len），与protocol对齐。
/// 按高度区分：空气节全零占位；均匀石头节调色板记石头ID、block_count=4096；
/// 混合节（石头+泥土、草+空气）走间接palette（BPE=4，4096项真实数据），
/// 保证出生点所在 section 有几何体，客户端才会完成 “player section compiled”。
pub fn chunk_network_bytes(chunk: &Chunk) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.sections.len() * 8);
    let mut states = [0i32; mc_protocol::packets::SECTION_VOLUME];
    for (idx, kind) in chunk.sections.iter().enumerate() {
        match *kind {
            SectionKind::Uniform(AIR_BLOCK_ID) => {
                mc_protocol::packets::encode_empty_section_placeholder(&mut out);
            }
            SectionKind::Uniform(id) => {
                // 均匀非空节：单值palette记该blockstate ID，fluid=0，非空气数4096。
                mc_protocol::packets::encode_uniform_section_single(
                    id as i32, mc_protocol::packets::PLAINS_BIOME_ID_777, 4096, 0, &mut out,
                );
            }
            SectionKind::Paletted | SectionKind::Direct => {
                // 混合节：按 y 层生成真实 4096 项（局部索引 y*256+z*16+x），走间接 palette。
                let base = MIN_Y + idx as i32 * 16;
                for dy in 0..16 {
                    let id = block_at_y(base + dy) as i32;
                    states[dy as usize * 256..(dy as usize + 1) * 256].fill(id);
                }
                mc_protocol::packets::encode_block_section(
                    &states, mc_protocol::packets::PLAINS_BIOME_ID_777, &mut out,
                );
            }
        }
    }
    out
}

/// 真实 MOTION_BLOCKING 高度图：当前平原地形最高非空气方块 = 草方块 y=64，
/// 高度值 = 65 - min_y(-64) = 129。
pub fn chunk_heightmaps_bytes() -> Vec<u8> {
    mc_protocol::packets::motion_blocking_heightmap_uniform((GRASS_LEVEL + 1) - MIN_Y)
}

/// 平原天空光照：草方块所在 section（y=64..79，index 8）及以上 15 级全亮；
/// 地下 section 无光照数组（客户端按 0 处理）。block light 为空。
pub fn chunk_light_bytes() -> Vec<u8> {
    let first_lit = ((GRASS_LEVEL - MIN_Y) / 16) as u32;
    let mask = ((1u64 << SECTION_COUNT) - 1) & !((1u64 << first_lit) - 1);
    let mut out = Vec::new();
    mc_protocol::packets::encode_sky_light_data(mask, 15, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_layer_correct() {
        // 按 y 分层：y<=60 石头，61-63 泥土，64 草，其余空气
        for y in [MIN_Y, -65, -1, 0, 59, 60] {
            assert_eq!(block_at_y(y), STONE_BLOCK_ID, "y={y} should be stone");
        }
        for y in [61, 62, 63] {
            assert_eq!(block_at_y(y), DIRT_BLOCK_ID, "y={y} should be dirt");
        }
        assert_eq!(block_at_y(64), GRASS_BLOCK_ID);
        for y in [65, 79, 80, 319, 320] {
            assert_eq!(block_at_y(y), AIR_BLOCK_ID, "y={y} should be air");
        }
        // section 落位：0..6 整节石头，7/8 混合，9..23 整节空气
        let c = chunk_for(mc_core::ChunkPos(0, 0));
        assert_eq!(c.sections.len(), SECTION_COUNT);
        for (i, s) in c.sections.iter().enumerate().take(7) {
            assert_eq!(*s, SectionKind::Uniform(STONE_BLOCK_ID), "section {i} stone");
        }
        assert_eq!(c.sections[7], SectionKind::Paletted, "48..63 stone+dirt");
        assert_eq!(c.sections[8], SectionKind::Paletted, "64..79 grass+air");
        for (i, s) in c.sections.iter().enumerate().skip(9) {
            assert_eq!(*s, SectionKind::Uniform(AIR_BLOCK_ID), "section {i} air");
        }
        // 确定性：同 pos 同形
        let again = chunk_for(mc_core::ChunkPos(0, 0));
        assert_eq!(c.sections, again.sections);
    }

    #[test]
    fn chunk_for_is_deterministic() {
        let pos = mc_core::ChunkPos(0, 0);
        let a = chunk_for(pos);
        let b = chunk_for(pos);
        assert_eq!(a.sections, b.sections);
        assert_eq!(a.sections.len(), SECTION_COUNT);
        // 不同坐标同样确定（当前全图同形，P3 按种子起伏）
        let c = chunk_for(mc_core::ChunkPos(5, -7));
        assert_eq!(c.sections, a.sections);
        // empty_flat 保留：全空气 24 节
        let flat = Chunk::empty_flat(pos);
        assert!(flat.sections.iter().all(|s| *s == SectionKind::Uniform(0)));
    }

    #[test]
    fn chunk_network_bytes_not_empty() {
        let c = chunk_for(mc_core::ChunkPos(0, 0));
        let bytes = chunk_network_bytes(&c);
        // 7 个均匀石头节(8B) + 2 个间接 palette 混合节(2058B) + 15 个空气节(8B)
        assert_eq!(bytes.len(), 7 * 8 + 2 * 2058 + 15 * 8);
        // 均匀石头节：block_count 4096, fluid 0, 单值 stone=1, biome plains=41
        let stone0 = &bytes[0..8];
        assert_eq!(stone0, &[0x10, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x29]);
        for i in 1..7 {
            assert_eq!(&bytes[i * 8..i * 8 + 8], stone0, "stone section {i}");
        }
        // 混合节7（48..63 石头+泥土）：block_count 4096, fluid 0, BPE=4, palette=[1,10],
        // 之后是 4096 项 ×4bit = 256 longs（无长度前缀）。
        let s7 = &bytes[7 * 8..7 * 8 + 2058];
        assert_eq!(&s7[..4], &[0x10, 0x00, 0x00, 0x00]);
        assert_eq!(s7[4], 4, "BPE=4");
        assert_eq!(s7[5], 2, "palette len=2");
        assert_eq!(&s7[6..8], &[1, 10], "palette=[stone,dirt]");
        // 混合节8（64 草方块 + 65..79 空气）：block_count 256，palette=[9,0]。
        let s8 = &bytes[7 * 8 + 2058..7 * 8 + 2058 + 2058];
        assert_eq!(&s8[..4], &[0x01, 0x00, 0x00, 0x00]);
        assert_eq!(s8[4], 4);
        assert_eq!(s8[5], 2);
        assert_eq!(&s8[6..8], &[9, 0], "palette=[grass_block,air]");
        // 空气节（idx>=9）写 plains=41 的 biome palette，其余字节为0
        let air = &bytes[7 * 8 + 2 * 2058..];
        assert!(air.chunks(8).all(|s| s == [0, 0, 0, 0, 0, 0, 0, 41]), "air sections keep plains biome id");
    }

    #[test]
    fn chunk_heightmaps_is_real_motion_blocking() {
        // 平原地表：草方块 y=64 → height = 65 - (-64) = 129
        let h = chunk_heightmaps_bytes();
        assert_eq!(&h[..3], &[0x01, 0x04, 0x25]);
        assert_eq!(h.len(), 3 + 37 * 8);
        let mut word_bytes = [0u8; 8];
        word_bytes.copy_from_slice(&h[3..11]);
        let first = u64::from_be_bytes(word_bytes);
        for k in 0..7 {
            assert_eq!((first >> (k * 9)) & 0x1ff, 129, "first long item {k}");
        }
        // 最后一个 long 装 256-36*7=4 项，其余为空。
        word_bytes.copy_from_slice(&h[3 + 36 * 8..]);
        let last = u64::from_be_bytes(word_bytes);
        for k in 0..4 {
            assert_eq!((last >> (k * 9)) & 0x1ff, 129, "last long item {k}");
        }
        assert_eq!(last >> (4 * 9), 0, "last long padding must be zero");
    }

    #[test]
    fn chunk_light_is_sky_lit_above_ground() {
        let l = chunk_light_bytes();
        assert_eq!(l.len(), 1 + 3 + 3 + 1 + 16 * (2 + 2048) + 1);
        // sky mask bits 8..23（y=64 及以上 16 个 section），BitSet 字节 [0,0xFF,0xFF]
        assert_eq!(&l[..4], &[3, 0, 0xFF, 0xFF]);
        assert_eq!(l[7], 16);
        assert_eq!(&l[8..10], &[0x80, 0x10]);
        let mut off = 8;
        for _ in 0..16 {
            assert_eq!(&l[off..off + 2], &[0x80, 0x10], "array length 2048");
            off += 2;
            assert!(l[off..off + 2048].iter().all(|&b| b == 0xFF), "sky level 15");
            off += 2048;
        }
        assert_eq!(off, l.len() - 1, "only block array count remains");
    }

    #[test]
    fn view_chunks_radius4_is_81_center_first() {
        let center = mc_core::ChunkPos(0, 0);
        let v = view_chunks(center, 4);
        assert_eq!(v.len(), 81, "radius=4 -> (2*4+1)^2=81");
        // 中心先发：限流 1/tick 时出生点区块最先到
        assert_eq!(v[0], center);
        // 全覆盖：dx/dz in -4..=4 且无重复
        let mut sorted = v.clone();
        sorted.sort_by_key(|p| (p.0, p.1));
        sorted.dedup();
        assert_eq!(sorted.len(), 81);
        for p in &v {
            assert!((p.0 - center.0).abs() <= 4 && (p.1 - center.1).abs() <= 4);
        }
        // 非中心平移：相对偏移不变
        let c2 = mc_core::ChunkPos(-3, 7);
        let v2 = view_chunks(c2, 4);
        assert_eq!(v2.len(), 81);
        assert_eq!(v2[0], c2);
    }

    #[test]
    fn view_chunks_edge_radii() {
        assert_eq!(view_chunks(mc_core::ChunkPos(1, 2), 0), vec![mc_core::ChunkPos(1, 2)]);
        assert!(view_chunks(mc_core::ChunkPos(0, 0), -1).is_empty());
        assert_eq!(view_chunks(mc_core::ChunkPos(0, 0), 1).len(), 9);
    }
}
