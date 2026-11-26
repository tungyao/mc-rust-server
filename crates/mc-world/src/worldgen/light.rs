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
use crate::{
    AIR_BLOCK_ID, GLOWSTONE_BLOCK_ID, LANTERN_BLOCK_ID, LAVA_BLOCK_ID, SEA_LANTERN_BLOCK_ID,
    SECTION_COUNT, SOUL_TORCH_BLOCK_ID, TORCH_BLOCK_ID, WATER_BLOCK_ID,
};
use std::collections::VecDeque;

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

// ---------------------------------------------------------------------------
// P46 v2 block light：真实光源 ID 的 luminance 作为 BFS 起点，每格 -1，
// 不穿过不透明方块；chunk 本地计算（rayon/缓存安全，跨 chunk 接缝侧向补光差异
// 列为真机待验）。亮度来源：`data/777/full/blocks.json` 每 state 的 luminance。
// ---------------------------------------------------------------------------

/// 返回该 blockstate 的简化光源亮度（非光源为 0）。
pub(crate) fn block_luminance(id: u16) -> u8 {
    match id {
        TORCH_BLOCK_ID | WALL_TORCH_ID => 14,
        SOUL_TORCH_BLOCK_ID => 10,
        LANTERN_BLOCK_ID | SEA_LANTERN_BLOCK_ID | GLOWSTONE_BLOCK_ID | LAVA_BLOCK_ID => 15,
        _ => 0,
    }
}

/// `wall_torch` 与 `torch` 同为 14（`data/777/full/blocks.json` 实测）。
const WALL_TORCH_ID: u16 = crate::registry_id(mc_registry::BLOCKS, "minecraft:wall_torch") as u16;

/// 不透明：不传光（光源本身仍写自己的 nibble）。
fn block_opaque(id: u16) -> bool {
    id != AIR_BLOCK_ID && !crate::fluid::is_water(id) && !is_plant(id)
}

/// 计算非零 block light section 列表（section 索引升序，2048B nibble/节）。
pub(super) fn compute_block_light(chunk: &GeneratedChunk) -> Vec<(u32, [u8; 2048])> {
    let volume = SECTION_COUNT * SECTION_VOLUME;
    let mut level = vec![0u8; volume];
    let mut queue = VecDeque::new();
    for (section, cells) in chunk.sections.iter().enumerate() {
        for (idx, &id) in cells.iter().enumerate() {
            let lum = block_luminance(id);
            if lum > 0 {
                let gi = section * SECTION_VOLUME + idx;
                level[gi] = lum;
                queue.push_back(gi);
            }
        }
    }
    if queue.is_empty() {
        return Vec::new();
    }
    // 6 邻域：x±1 / z±1 同 section；y±1 可能跨 section。
    while let Some(gi) = queue.pop_front() {
        let cur = level[gi];
        if cur <= 1 {
            continue;
        }
        let section = gi / SECTION_VOLUME;
        let idx = gi % SECTION_VOLUME;
        let ly = idx / CHUNK_COLUMNS;
        let lz = (idx % CHUNK_COLUMNS) / 16;
        let lx = idx % 16;
        let next = cur - 1;
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0), (0, -1, 0)] {
            let nx = lx as i32 + dx;
            let nz = lz as i32 + dz;
            let ny = ly as i32 + dy;
            if !(0..16).contains(&nx) || !(0..16).contains(&nz) {
                continue;
            }
            let s = (section as i32 + if ny < 0 { -1 } else if ny >= 16 { 1 } else { 0 }) as usize;
            if !(0..SECTION_COUNT).contains(&s) {
                continue;
            }
            let local_y = ny.rem_euclid(16) as usize;
            let nidx = local_y * CHUNK_COLUMNS + (nz as usize) * 16 + nx as usize;
            let ngi = s * SECTION_VOLUME + nidx;
            if block_opaque(chunk.sections[s][nidx]) {
                // 不透明格仍存光（客户端方块面按此着色），但不继续传播。
                if level[ngi] < next {
                    level[ngi] = next;
                }
                continue;
            }
            if level[ngi] < next {
                level[ngi] = next;
                queue.push_back(ngi);
            }
        }
    }
    let mut out = Vec::new();
    for section in 0..SECTION_COUNT {
        let mut nibble = [0u8; 2048];
        let mut any = false;
        for idx in 0..SECTION_VOLUME {
            let v = level[section * SECTION_VOLUME + idx];
            if v > 0 {
                any = true;
                nibble[idx / 2] |= v << if idx % 2 == 0 { 0 } else { 4 };
            }
        }
        if any {
            out.push((section as u32, nibble));
        }
    }
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

    #[test]
    fn block_light_luminance_uses_real_26_3_ids() {
        assert_eq!(block_luminance(TORCH_BLOCK_ID), 14);
        assert_eq!(block_luminance(WALL_TORCH_ID), 14);
        assert_eq!(block_luminance(SOUL_TORCH_BLOCK_ID), 10);
        assert_eq!(block_luminance(LANTERN_BLOCK_ID), 15);
        assert_eq!(block_luminance(SEA_LANTERN_BLOCK_ID), 15);
        assert_eq!(block_luminance(GLOWSTONE_BLOCK_ID), 15);
        assert_eq!(block_luminance(LAVA_BLOCK_ID), 15);
        assert_eq!(block_luminance(crate::STONE_BLOCK_ID), 0);
    }

    /// 手工造一个 section：灯笼在 local (x=3,y=4,z=5)，周围空气。
    /// nibble 索引 `y*256+z*16+x`，偶索引低位/奇索引高位。
    #[test]
    fn block_light_nibble_decays_one_per_air_cell_and_stops_at_opaque() {
        let pos = mc_core::ChunkPos(0, 0);
        let mut sections: Vec<Box<[u16; SECTION_VOLUME]>> = (0..SECTION_COUNT)
            .map(|_| Box::new([AIR_BLOCK_ID; SECTION_VOLUME]))
            .collect();
        let idx = |x: usize, y: usize, z: usize| y * CHUNK_COLUMNS + z * 16 + x;
        sections[0][idx(3, 4, 5)] = LANTERN_BLOCK_ID;
        sections[0][idx(4, 4, 5)] = crate::STONE_BLOCK_ID; // 不透明墙不传光
        sections[0][idx(3, 6, 5)] = crate::STONE_BLOCK_ID;
        let gen = crate::WorldGen::with_version(42, crate::GeneratorVersion::V2);
        let chunk = crate::worldgen::GeneratedChunk {
            pos,
            sections,
            heights: [70i32; CHUNK_COLUMNS],
            motion_heights: [8i32; CHUNK_COLUMNS],
            biome_id: gen.chunk_biome(pos),
        };
        let out = compute_block_light(&chunk);
        // 光源在 section0，但 x=3,z=4 的竖直空气柱一路通到 section1 底部。
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, 0);
        assert_eq!(out[1].0, 1);
        let n = &out[0].1;
        let get = |x: usize, y: usize, z: usize| -> u8 {
            let i = idx(x, y, z);
            (n[i / 2] >> if i % 2 == 0 { 0 } else { 4 }) & 0x0f
        };
        assert_eq!(get(3, 4, 5), 15, "source nibble");
        assert_eq!(get(3, 4, 4), 14, "one step in air");
        assert_eq!(get(3, 4, 3), 13);
        assert_eq!(get(4, 4, 5), 14, "opaque cell stores light but stops propagation");
        assert_eq!(get(3, 5, 5), 14, "air above source");
        // y=6 的石砖挡住正上方通路，但光可从相邻空气柱绕行（原版同样是 6 邻域）。
        assert_eq!(get(3, 6, 5), 13, "opaque cell stores but does not re-emit");
        assert!(n.iter().any(|&b| b != 0));
    }
}
