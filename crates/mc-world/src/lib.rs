//! mc-world: Chunk 存储 + 生成 + 存档，不依赖网络
//! 对标 FerrumC world + SteelMC parity 思路
pub mod load;
pub mod save;

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
// 任务F：空世界 + Play 脚手架常量/函数
// 只兼容 26.3/777：主世界单一种子，重生点固定，方便客户端进服不掉线。
// ---------------------------------------------------------------------------

/// 空世界种子占位（P3 接真实存档/配置）。确定性生成依赖它。
pub const WORLD_SEED: i64 = 123456789;

/// 重生点（方块坐标）：出生岛中心附近 y=80 安全高度。
pub const SPAWN_POS: mc_core::BlockPos = mc_core::BlockPos(8, 80, 8);

/// 空气 / 石头 / 草方块 / 泥土占位方块 ID（与 mc-registry 占位表对齐：
/// air=0 stone=1 grass_block=2 dirt=3，真实 blockstate ID 待对齐，此处仅脚手架）。
pub const AIR_BLOCK_ID: u16 = 0;
pub const STONE_BLOCK_ID: u16 = 1;
pub const GRASS_BLOCK_ID: u16 = 2;
pub const DIRT_BLOCK_ID: u16 = 3;

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

/// 确定性空世界：同一 `pos` 永远返回相同 Chunk。
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
/// Chunk 网络字节：按真实 section 数逐节填 8B 占位，P3 填真实高度图/光照
/// （复用 mc-protocol 空 section 占位布局，每节 8B）。
/// 按高度区分：空气节全零占位；均匀石头节同布局但调色板记石头 ID、
/// block_count=4096；混合节记真实非空方块数、调色板 P3 再补。
pub fn chunk_network_bytes(chunk: &Chunk) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.sections.len() * 8);
    for (idx, kind) in chunk.sections.iter().enumerate() {
        match *kind {
            SectionKind::Uniform(AIR_BLOCK_ID) => {
                mc_protocol::packets::encode_empty_section_placeholder(&mut out);
            }
            SectionKind::Uniform(id) => {
                // 均匀非空节占位（ID 占位值 <256，直接记调色板低字节，待对齐）。
                out.extend_from_slice(&4096i16.to_be_bytes());
                out.extend_from_slice(&[0x00, id as u8, 0x00]);
                out.extend_from_slice(&[0x00, 0x00, 0x00]);
            }
            SectionKind::Paletted | SectionKind::Direct => {
                // 混合节占位：逐层按 block_at_y 计非空数（每层 16x16）。
                let base = MIN_Y + idx as i32 * 16;
                let mut non_air: i16 = 0;
                for dy in 0..16 {
                    if block_at_y(base + dy) != AIR_BLOCK_ID {
                        non_air += 256;
                    }
                }
                out.extend_from_slice(&non_air.to_be_bytes());
                out.extend_from_slice(&[0x00, 0x00, 0x00]);
                out.extend_from_slice(&[0x00, 0x00, 0x00]);
            }
        }
    }
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
        assert_eq!(bytes.len(), SECTION_COUNT * 8);
        // 空气节（idx>=9）全零占位，石头节（idx<7）非零且互等，混合节非零
        let air = &bytes[9 * 8..];
        assert!(air.iter().all(|&b| b == 0), "air sections stay zero placeholder");
        let stone0 = &bytes[0..8];
        assert!(stone0.iter().any(|&b| b != 0), "stone sections differ from air");
        for i in 1..7 {
            assert_eq!(&bytes[i * 8..i * 8 + 8], stone0, "stone section {i}");
        }
        assert!(bytes[7 * 8..8 * 8].iter().any(|&b| b != 0));
        assert!(bytes[8 * 8..9 * 8].iter().any(|&b| b != 0));
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
