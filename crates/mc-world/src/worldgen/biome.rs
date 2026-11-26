//! P45 v2 多 biome：每 section 的 4×4×4 = 64 个 biome 单元按世界坐标采样，
//! 用温度/湿度/高度/河流场选择已注册 biome（river/birch/snowy/...）。
//!
//! 纯函数、单 chunk 独立：`biomes_for_section(gen, pos, section)` 只由
//! `(seed, pos, section)` 决定；出生保护圈内锁定 v1 的 forest/plains，
//! 深层 y<-40 使用洞穴 biome。`GeneratedChunk.biome_id` 仍是主 biome
//! （`chunk_biome`，v1 口径），旧 API/测试不受影响。
//! biome 网络 ID 全部来自 `registry_00.bin`（67 项，下标即 ID），
//! 新增常量在 `mc_protocol::packets` 并由逐字节注册表单测锁定。
//! 布局依据：`docs/worldgen-plan.md` §3.2/§3.9。

use super::noise::value_noise_2d;
use super::terrain::river_strength;
use super::{WorldGen, SEA_LEVEL, SPAWN_BLOCK_X, SPAWN_BLOCK_Z};
use mc_core::ChunkPos;
use mc_protocol::packets::*;

const TEMP_SALT: u64 = 0x6A09_E667_F3BC_C909;
const DOWNFALL_SALT: u64 = 0xBB67_AE85_84CA_A73B;
const CAVE_BIOME_SALT: u64 = 0x3C6E_F372_FE94_F82B;
const TEMP_SCALE: f64 = 256.0;
const DOWNFALL_SCALE: f64 = 192.0;
const CAVE_BIOME_SCALE: f64 = 64.0;
/// 深层洞穴 biome 起点（深板岩带与之无关，只是让地下不再是单一地表 biome）。
const CAVE_BIOME_MAX_Y: i32 = -40;

/// 出生圈内强制 v1 的 forest/plains（与 `chunk_biome` 同阈值），
/// 防止 P1 的“出生在沙滩/海底”回归。
fn spawn_locked_biome(wx: i32, wz: i32) -> bool {
    let dx = (wx - SPAWN_BLOCK_X) as f64;
    let dz = (wz - SPAWN_BLOCK_Z) as f64;
    (dx * dx + dz * dz).sqrt() <= 16.0
}

fn surface_biome(gen: &WorldGen, wx: i32, wz: i32, temp: f64, downfall: f64, river: f64) -> i32 {
    let h = gen.height_at(wx, wz);
    let forest = value_noise_2d(gen.seed, wx as f64, wz as f64, 96.0, 0x51ED_2701_9E37_79B9);
    if spawn_locked_biome(wx, wz) {
        return if forest > 0.55 { FOREST_BIOME_ID_777 } else { PLAINS_BIOME_ID_777 };
    }
    if river > 0.5 {
        return if temp < 0.25 { FROZEN_RIVER_BIOME_ID_777 } else { RIVER_BIOME_ID_777 };
    }
    if h < SEA_LEVEL - 3 {
        return if temp > 0.72 {
            WARM_OCEAN_BIOME_ID_777
        } else if temp < 0.25 {
            FROZEN_OCEAN_BIOME_ID_777
        } else if temp < 0.42 {
            COLD_OCEAN_BIOME_ID_777
        } else {
            OCEAN_BIOME_ID_777
        };
    }
    if h < SEA_LEVEL {
        return if temp < 0.25 { SNOWY_BEACH_BIOME_ID_777 } else { BEACH_BIOME_ID_777 };
    }
    if h == SEA_LEVEL && temp < 0.22 {
        return STONY_SHORE_BIOME_ID_777;
    }
    if temp < 0.22 {
        return if downfall < 0.48 { SNOWY_PLAINS_BIOME_ID_777 } else { SNOWY_TAIGA_BIOME_ID_777 };
    }
    if temp < 0.38 {
        return if downfall > 0.6 { TAIGA_BIOME_ID_777 } else { PLAINS_BIOME_ID_777 };
    }
    if temp > 0.74 && downfall < 0.32 {
        return DESERT_BIOME_ID_777;
    }
    if temp > 0.7 && downfall < 0.5 {
        return SAVANNA_BIOME_ID_777;
    }
    if downfall > 0.66 {
        if temp > 0.62 {
            return MANGROVE_SWAMP_BIOME_ID_777;
        }
        return match (forest > 0.62, downfall > 0.82) {
            (true, true) => DARK_FOREST_BIOME_ID_777,
            (true, false) => BIRCH_FOREST_BIOME_ID_777,
            (false, _) => SWAMP_BIOME_ID_777,
        };
    }
    if h > 84 {
        return if downfall > 0.5 { GROVE_BIOME_ID_777 } else { MEADOW_BIOME_ID_777 };
    }
    if forest > 0.58 {
        if downfall > 0.55 {
            FLOWER_FOREST_BIOME_ID_777
        } else if downfall < 0.28 {
            BIRCH_FOREST_BIOME_ID_777
        } else {
            FOREST_BIOME_ID_777
        }
    } else if downfall < 0.3 && temp > 0.78 {
        CHERRY_GROVE_BIOME_ID_777
    } else {
        PLAINS_BIOME_ID_777
    }
}

fn cave_biome(gen: &WorldGen, wx: i32, wz: i32) -> i32 {
    let n = value_noise_2d(gen.seed, wx as f64, wz as f64, CAVE_BIOME_SCALE, CAVE_BIOME_SALT);
    if n < 0.25 {
        DEEP_DARK_BIOME_ID_777
    } else if n < 0.5 {
        DRIPSTONE_CAVES_BIOME_ID_777
    } else if n < 0.75 {
        LUSH_CAVES_BIOME_ID_777
    } else {
        MUSHROOM_FIELDS_BIOME_ID_777
    }
}

/// 一次算出 24 个 section 的 64 项 biome 容器（索引 `(by*4+bz)*4+bx`，
/// x 最快、z 次之、y 最慢）。只采样 16 个水平子格一次再向 y 复用，
/// 避免逐 section 重复做 `height_at` / 气候噪声（441 块批量的热点）。
pub(super) fn biomes_for_chunk(gen: &WorldGen, pos: ChunkPos) -> [[i32; 64]; crate::SECTION_COUNT] {
    let mut surface = [PLAINS_BIOME_ID_777; 16];
    let mut cave = [DEEP_DARK_BIOME_ID_777; 16];
    for bz in 0..4usize {
        for bx in 0..4usize {
            let wx = pos.0 * 16 + bx as i32 * 4 + 2;
            let wz = pos.1 * 16 + bz as i32 * 4 + 2;
            let temp = value_noise_2d(gen.seed, wx as f64, wz as f64, TEMP_SCALE, TEMP_SALT);
            let downfall = value_noise_2d(gen.seed, wx as f64, wz as f64, DOWNFALL_SCALE, DOWNFALL_SALT);
            let river = river_strength(gen.seed, wx, wz);
            let cell = bz * 4 + bx;
            surface[cell] = surface_biome(gen, wx, wz, temp, downfall, river);
            cave[cell] = cave_biome(gen, wx, wz);
        }
    }
    let mut out = [[PLAINS_BIOME_ID_777; 64]; crate::SECTION_COUNT];
    for (section, container) in out.iter_mut().enumerate() {
        let section_y = crate::MIN_Y + section as i32 * 16;
        for by in 0..4usize {
            let wy = section_y + by as i32 * 4 + 2;
            let deep = wy < CAVE_BIOME_MAX_Y;
            for bz in 0..4usize {
                for bx in 0..4usize {
                    container[(by * 4 + bz) * 4 + bx] = if deep {
                        cave[bz * 4 + bx]
                    } else {
                        surface[bz * 4 + bx]
                    };
                }
            }
        }
    }
    out
}

/// 单 section 查询（仅测试用；生产路径走一次性的 `biomes_for_chunk`）。
#[cfg(test)]
pub(super) fn biomes_for_section(gen: &WorldGen, pos: ChunkPos, section: usize) -> [i32; 64] {
    biomes_for_chunk(gen, pos)[section]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GeneratorVersion;

    #[test]
    fn v2_biomes_cover_registered_ids_and_spawn_is_locked() {
        let gen = WorldGen::with_version(424242, GeneratorVersion::V2);
        let mut found = std::collections::HashSet::new();
        for dx in -12..=12 {
            for dz in -12..=12 {
                let pos = ChunkPos(gen.spawn_chunk().0 + dx, gen.spawn_chunk().1 + dz);
                for section in [0usize, 1, 5, 8, 12] {
                    found.extend(biomes_for_section(&gen, pos, section));
                }
            }
        }
        for id in [
            RIVER_BIOME_ID_777,
            BIRCH_FOREST_BIOME_ID_777,
            SNOWY_PLAINS_BIOME_ID_777,
            MEADOW_BIOME_ID_777,
            DEEP_DARK_BIOME_ID_777,
            LUSH_CAVES_BIOME_ID_777,
        ] {
            assert!(found.contains(&id), "biome {id} never assigned");
        }
        // 出生圈锁定 forest/plains。
        let spawn = gen.spawn_chunk();
        for section in [8usize, 9, 10] {
            for b in biomes_for_section(&gen, spawn, section) {
                assert!(b == FOREST_BIOME_ID_777 || b == PLAINS_BIOME_ID_777, "spawn biome {b}");
            }
        }
    }
}
