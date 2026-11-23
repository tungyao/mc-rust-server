//! mc-world: Chunk 存储 + 生成 + 存档，不依赖网络
//! 对标 FerrumC world + SteelMC parity 思路

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Uniform(u16),
    Paletted,
    Direct,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub pos: mc_core::ChunkPos,
    pub sections: Vec<SectionKind>, // P0 占位 24 节，P2 填 paletted_container
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

/// 空气 / 石头占位方块 ID（registry 真实 ID 在 P3 对齐，此处仅脚手架）。
pub const AIR_BLOCK_ID: u16 = 0;
pub const STONE_BLOCK_ID: u16 = 1;

/// 地面高度占位：y <= GROUND_LEVEL 按石头填（整节粒度），以上空气。
pub const GROUND_LEVEL: i32 = 60;
/// 24 节对应 y=-64..320（min_y=-64，每节 16 格）。
pub const MIN_Y: i32 = -64;
pub const SECTION_COUNT: usize = 24;

/// 确定性空世界：同一 `pos` 永远返回相同 Chunk。
/// 当前为整节粒度占位：section_idx < 8（y<64）填石头，其余空气；
/// P3 按方块精确填充/接真实高度图。`empty_flat` 保留，行为不变。
pub fn chunk_for(pos: mc_core::ChunkPos) -> Chunk {
    let mut sections = Vec::with_capacity(SECTION_COUNT);
    for idx in 0..SECTION_COUNT {
        // section idx 0..7 覆盖 y=-64..63，视为 y<=60 以下石头占位
        if idx < 8 {
            sections.push(SectionKind::Uniform(STONE_BLOCK_ID));
        } else {
            sections.push(SectionKind::Uniform(AIR_BLOCK_ID));
        }
    }
    Chunk { pos, sections }
}

/// Chunk 网络字节占位：先返回空 paletted section 字节，
/// P3 填真实高度图/光照（复用 mc-protocol 空 section 占位布局，每节 8B）。
pub fn chunk_network_bytes(chunk: &Chunk) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunk.sections.len() * 8);
    for _ in &chunk.sections {
        // 先返回空 paletted section 字节，P3 填真实高度图/光照。
        mc_protocol::packets::encode_empty_section_placeholder(&mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_for_is_deterministic() {
        let pos = mc_core::ChunkPos(0, 0);
        let a = chunk_for(pos);
        let b = chunk_for(pos);
        assert_eq!(a.sections, b.sections);
        assert_eq!(a.sections.len(), SECTION_COUNT);
        // y=60 以下占位：前 8 节石头，后 16 节空气
        for (i, s) in a.sections.iter().enumerate() {
            if i < 8 {
                assert_eq!(*s, SectionKind::Uniform(STONE_BLOCK_ID), "section {i} should be stone");
            } else {
                assert_eq!(*s, SectionKind::Uniform(AIR_BLOCK_ID), "section {i} should be air");
            }
        }
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
        assert!(bytes.iter().all(|&b| b == 0));
    }
}
