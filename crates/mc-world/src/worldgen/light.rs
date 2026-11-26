//! P45 v2 逐列天空光：从每列最高不透明方块向下衰减，按 section 输出 2048B nibble。
//!
//! 替换 v1 的“最低地表 section 以上全 15”：水下不再是 15，洞穴为 0，
//! 树冠/不透明块产生阴影。衰减规则（自研简化、无第三方依赖）：
//! - air / 装饰植物：光级不变；
//! - 树叶：衰减 2（半透明）；
//! - water source：该格保留入射光级，向下衰减 2；
//! - 其余不透明方块：该格 0，以下全 0。
//!
//! 不做水平 BFS：保证单 chunk 独立生成（rayon/缓存安全），接缝处的
//! 侧向补光差异列为真机待验项。BitSet/数组线格式见 `play_chunk.rs`
//! 的 `encode_nonuniform_sky_light_data`（wiki `Data_types#Light_Data`）。
//! 布局依据：`docs/worldgen-plan.md` §3.8 前半。

use super::features::is_plant;
use super::encode::GeneratedChunk;
use crate::{AIR_BLOCK_ID, SECTION_COUNT, WATER_BLOCK_ID};

const CHUNK_COLUMNS: usize = 256;
const SECTION_VOLUME: usize = 4096;

fn is_leaves(id: u16) -> bool {
    id == crate::OAK_LEAVES_BLOCK_ID
        || ((290..=520).contains(&id) && crate::crops::TREE_SPECS.iter().any(|s| s.leaves_id == id))
}

/// 计算有非零 nibble 的 section 列表（按 section 索引升序）。
/// 用 `motion_heights` 作为每列最高阻挡块的提示：其上全是 15，无需逐格读方块；
/// 所有列都衰减到 0 后立即停止（地下 section 全零，不写数组）。
pub(super) fn compute_sky_light(chunk: &GeneratedChunk) -> Vec<(u32, [u8; 2048])> {
    let mut incoming = [15i32; CHUNK_COLUMNS];
    let mut top = [crate::MIN_Y - 1; CHUNK_COLUMNS];
    for (column, (col, height)) in top.iter_mut().zip(chunk.motion_heights.iter()).enumerate() {
        let _ = column;
        *col = (crate::MIN_Y + *height - 1).min(crate::WORLD_TOP_Y);
    }
    let mut out: Vec<(u32, [u8; 2048])> = Vec::with_capacity(SECTION_COUNT);
    for section in (0..SECTION_COUNT).rev() {
        let base = chunk.sections[section].as_ref();
        let section_base_y = crate::MIN_Y + section as i32 * 16;
        let mut nibble = [0u8; 2048];
        let mut any = false;
        for lz in 0..16usize {
            for lx in 0..16usize {
                let column = lz * 16 + lx;
                let mut level = incoming[column];
                let col_top = top[column];
                for local_y in (0..16usize).rev() {
                    let y = section_base_y + local_y as i32;
                    let light = if y > col_top {
                        level
                    } else {
                        let id = base[local_y * CHUNK_COLUMNS + column];
                        // 常见不透明方块（id<=2000）走一条比较；树叶 294..518、
                        // 装饰植物全部 >2000，避免对石头逐格跑 12 路 matches!。
                        if id == AIR_BLOCK_ID {
                            level
                        } else if id == WATER_BLOCK_ID {
                            let at = level;
                            level = level.saturating_sub(2);
                            at
                        } else if (290..=520).contains(&id) && is_leaves(id) {
                            level = level.saturating_sub(2);
                            level
                        } else if id > 2000 && is_plant(id) {
                            level
                        } else {
                            level = 0;
                            0
                        }
                    };
                    let light = (light.max(0).min(15)) as u8;
                    if light != 0 {
                        any = true;
                        let idx = local_y * CHUNK_COLUMNS + column;
                        nibble[idx / 2] |= light << if idx % 2 == 0 { 0 } else { 4 };
                    }
                }
                incoming[column] = level;
            }
        }
        if any {
            out.push((section as u32, nibble));
        }
        if incoming.iter().all(|&level| level == 0) {
            break; // 地下所有列全暗，更少 section、更小的包体
        }
    }
    out.reverse();
    let _ = SECTION_VOLUME;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_nibbles_use_column_start_and_are_nonuniform() {
        let gen = crate::WorldGen::with_version(crate::DEFAULT_SEED, crate::GeneratorVersion::V2);
        let chunk = gen.generate(gen.spawn_chunk());
        let sections = compute_sky_light(&chunk);
        assert!(!sections.is_empty(), "surface sections must carry sky light");
        // 地表以上 section 全 15，地表 section 因树/草产生非均匀 nibble。
        let top = sections.last().unwrap();
        assert!(top.1.iter().all(|&b| b == 0xff), "top section fully lit");
        let mut mixed = false;
        for (_, arr) in &sections {
            if arr.iter().any(|&b| b != 0xff && b != 0x00) {
                mixed = true;
            }
        }
        assert!(mixed, "expected non-uniform nibble values near surface");
        // 地下 section 为空（不发数组），即洞穴不再全亮。
        assert!(sections.iter().all(|(s, _)| *s >= 8), "underground sections must be dark");
    }
}
