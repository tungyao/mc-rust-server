//! P46 v2 单 chunk 小遗迹：纯函数、确定性、不跨 chunk、无 jigsaw/loot/block entity。
//!
//! 生成条件（全部由 `(seed, chunk_x, chunk_z)` hash 决定，rayon/ChunkCache 安全）：
//! - 每 chunk 约 [`RUIN_CHANCE`] 概率出现；
//! - 5×5 足迹完整落在本 chunk 内（中心 local 3..=11，±2）；
//! - 足迹 25 列高度平坦（落差 ≤1）、全部干燥（h > SEA_LEVEL+2）；
//! - 与出生保护圈（半径 16）无交集，不与水体/水面冲突。
//!
//! 结构：石砖地基 + 2 格高环墙（留一个门洞）+ 雕凿石砖柱 + 灯笼光源。
//! 光源供 `light::compute_block_light` 扫描生成 block light nibble。
//! 与原版差异：无战利品/箱子 block entity/怪物/跨 chunk 结构，且高度不平的
//! 地块会跳过（不做地形改造），详见 HANDOFF P3 记录。

use super::{height_cache_index, update_motion, WorldGen};
use super::{put_block, SECTION_VOLUME};
use super::SPAWN_PROTECT_RADIUS;
use crate::{
    CHISELED_STONE_BRICKS_BLOCK_ID, LANTERN_BLOCK_ID, SEA_LEVEL, SPAWN_BLOCK_X, SPAWN_BLOCK_Z,
    STONE_BRICKS_BLOCK_ID,
};
use mc_core::ChunkPos;

/// 出现概率（每 chunk 独立 hash）。
pub const RUIN_CHANCE: f64 = 0.06;
/// 中心 local 坐标范围（保证 ±2 的 5×5 足迹后落在 0..=15）。
const CENTER_MIN: i32 = 3;
const CENTER_MAX: i32 = 11;
/// 结构 salt（与地形/矿/树 salt 独立，避免联动漂移）。
const RUIN_SALT: u64 = 0x5A17_C0DE_7E46_9F31;

/// 遗迹布局（测试/诊断用；决定性与写入逻辑共用同一份计算）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuinSpec {
    pub cx: i32,
    pub cz: i32,
    pub floor_y: i32,
}

/// 纯函数：该 chunk 是否应生成遗迹，以及中心/地基高度。
/// `hcache` 是 `base-3..=base+18` 的高度缓存（与 generate_v2 同源）。
pub(crate) fn ruin_at(gen: &WorldGen, pos: ChunkPos, hcache: &[i32]) -> Option<RuinSpec> {
    if gen.chunk_biome(pos) == mc_protocol::packets::OCEAN_BIOME_ID_777 {
        return None;
    }
    let exist = super::noise::hash01(gen.seed, pos.0 as i64, 0, pos.1 as i64, RUIN_SALT);
    if exist >= RUIN_CHANCE {
        return None;
    }
    let cx = CENTER_MIN
        + (super::noise::hash01(gen.seed, pos.0 as i64, 1, pos.1 as i64, RUIN_SALT)
            * ((CENTER_MAX - CENTER_MIN + 1) as f64)) as i32;
    let cz = CENTER_MIN
        + (super::noise::hash01(gen.seed, pos.0 as i64, 2, pos.1 as i64, RUIN_SALT)
            * ((CENTER_MAX - CENTER_MIN + 1) as f64)) as i32;
    debug_assert!((cx - 2) >= 0 && (cx + 2) < 16 && (cz - 2) >= 0 && (cz + 2) < 16);

    let min_x = pos.0 * 16 + cx - 2;
    let max_x = pos.0 * 16 + cx + 2;
    let min_z = pos.1 * 16 + cz - 2;
    let max_z = pos.1 * 16 + cz + 2;
    // 到出生点的最近足迹角（clamp 的 min>max 会 panic，故手写区间最近点）。
    let nearest_x = if SPAWN_BLOCK_X < min_x { min_x } else if SPAWN_BLOCK_X > max_x { max_x } else { SPAWN_BLOCK_X };
    let nearest_z = if SPAWN_BLOCK_Z < min_z { min_z } else if SPAWN_BLOCK_Z > max_z { max_z } else { SPAWN_BLOCK_Z };
    let dx = (nearest_x - SPAWN_BLOCK_X) as f64;
    let dz = (nearest_z - SPAWN_BLOCK_Z) as f64;
    if (dx * dx + dz * dz).sqrt() < SPAWN_PROTECT_RADIUS {
        return None;
    }

    let mut min_h = i32::MAX;
    let mut max_h = i32::MIN;
    for dz in -2..=2 {
        for dx in -2..=2 {
            let h = hcache[height_cache_index(cx + dx, cz + dz)];
            if h <= SEA_LEVEL + 2 {
                return None; // 干燥要求：水/沙滩都不放遗迹
            }
            min_h = min_h.min(h);
            max_h = max_h.max(h);
        }
    }
    if max_h - min_h > 1 {
        return None;
    }
    Some(RuinSpec { cx, cz, floor_y: max_h + 1 })
}

/// 在 `generate_v2` 的树/草之后写入遗迹；仅 v2 调用。
pub(crate) fn place_ruin(
    gen: &WorldGen,
    pos: ChunkPos,
    hcache: &[i32],
    sections: &mut [Box<[u16; SECTION_VOLUME]>],
    motion: &mut [i32; 256],
) -> Option<RuinSpec> {
    let spec = ruin_at(gen, pos, hcache)?;
    for dz in -2..=2 {
        for dx in -2..=2 {
            let lx = spec.cx + dx;
            let lz = spec.cz + dz;
            let h = hcache[height_cache_index(lx, lz)];
            // 轻微不平时用石砖补平到 floor_y，保证地板连续且不跨 chunk。
            for y in (h + 1)..=spec.floor_y {
                put_block(sections, lx, y, lz, STONE_BRICKS_BLOCK_ID);
                update_motion(motion, lx, lz, y);
            }
        }
    }
    // 2 格高环墙，南侧留 1 格门洞（纯几何规则，无随机）。
    for dy in 1..=2 {
        for dz in -2i32..=2 {
            for dx in -2i32..=2 {
                let edge = dx.abs() == 2 || dz.abs() == 2;
                if !edge {
                    continue;
                }
                if dz == 2 && dx == 0 {
                    continue; // 门洞
                }
                let y = spec.floor_y + dy;
                put_block(sections, spec.cx + dx, y, spec.cz + dz, STONE_BRICKS_BLOCK_ID);
                update_motion(motion, spec.cx + dx, spec.cz + dz, y);
            }
        }
    }
    // 雕凿石砖柱 + 灯笼光源（block light 的扫描入口）。
    let pillar_y = spec.floor_y + 1;
    put_block(sections, spec.cx, pillar_y, spec.cz, CHISELED_STONE_BRICKS_BLOCK_ID);
    update_motion(motion, spec.cx, spec.cz, pillar_y);
    let lamp_y = pillar_y + 1;
    put_block(sections, spec.cx, lamp_y, spec.cz, LANTERN_BLOCK_ID);
    update_motion(motion, spec.cx, spec.cz, lamp_y);
    Some(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GeneratorVersion, WorldGen, AIR_BLOCK_ID, LANTERN_BLOCK_ID};

    fn gen(seed: i64) -> WorldGen {
        WorldGen::with_version(seed, GeneratorVersion::V2)
    }

    fn hcache(g: &WorldGen, pos: ChunkPos) -> Vec<i32> {
        let base_x = pos.0 * 16;
        let base_z = pos.1 * 16;
        let mut h = vec![0i32; (super::super::HEIGHT_CACHE_SIDE.pow(2)) as usize];
        for oz in super::super::HEIGHT_CACHE_MIN..(super::super::HEIGHT_CACHE_MIN + super::super::HEIGHT_CACHE_SIDE) {
            for ox in super::super::HEIGHT_CACHE_MIN..(super::super::HEIGHT_CACHE_MIN + super::super::HEIGHT_CACHE_SIDE) {
                h[height_cache_index(ox, oz)] = g.height_at(base_x + ox, base_z + oz);
            }
        }
        h
    }

    #[test]
    fn ruin_is_deterministic_and_v1_never_generates_it() {
        for seed in [1i64, 42, -987654321] {
            for pos in [ChunkPos(5, 7), ChunkPos(-13, 2), ChunkPos(31, -4)] {
                let g = gen(seed);
                let h = hcache(&g, pos);
                let a = ruin_at(&g, pos, &h);
                let b = ruin_at(&g, pos, &h);
                assert_eq!(a, b, "same (seed,chunk) must yield the same ruin");
                // v1 基底不含遗迹：生成结果没有任何灯笼/石砖。
                let v1 = crate::WorldGen::new(seed).generate(pos);
                assert!(v1.sections.iter().all(|s| s.iter().all(|&id| {
                    id != LANTERN_BLOCK_ID && id != STONE_BRICKS_BLOCK_ID
                })), "v1 must stay byte-frozen (no ruin blocks)");
                let _ = hcache(&g, pos);
            }
        }
    }

    #[test]
    fn ruin_footprint_never_leaves_the_chunk_or_touches_spawn() {
        let mut found = 0;
        for seed in [1i64, 42, 777] {
            for cx in -8..8 {
                for cz in -8..8 {
                    let pos = ChunkPos(cx, cz);
                    let g = gen(seed);
                    let h = hcache(&g, pos);
                    let Some(spec) = ruin_at(&g, pos, &h) else { continue };
                    found += 1;
                    assert!((spec.cx - 2) >= 0 && (spec.cx + 2) < 16);
                    assert!((spec.cz - 2) >= 0 && (spec.cz + 2) < 16);
                    let x0 = pos.0 * 16 + spec.cx - 2;
                    let z0 = pos.1 * 16 + spec.cz - 2;
                    let x1 = x0 + 4;
                    let z1 = z0 + 4;
                    let nx = if SPAWN_BLOCK_X < x0 { x0 } else if SPAWN_BLOCK_X > x1 { x1 } else { SPAWN_BLOCK_X };
                    let nz = if SPAWN_BLOCK_Z < z0 { z0 } else if SPAWN_BLOCK_Z > z1 { z1 } else { SPAWN_BLOCK_Z };
                    let d = (((nx - SPAWN_BLOCK_X) as f64).powi(2)
                        + ((nz - SPAWN_BLOCK_Z) as f64).powi(2)).sqrt();
                    assert!(d >= SPAWN_PROTECT_RADIUS, "ruin too close to spawn: {d}");
                    assert!(spec.floor_y > SEA_LEVEL + 2, "ruin must be dry");
                    let _ = AIR_BLOCK_ID;
                }
            }
        }
        assert!(found > 0, "at least one ruin should appear in the sampled chunks");
    }

    #[test]
    fn placed_ruin_contains_lantern_and_is_in_base_only_for_v2() {
        let seed = 42;
        let mut target = None;
        for cx in -8..8 {
            for cz in -8..8 {
                let pos = ChunkPos(cx, cz);
                let g = gen(seed);
                let h = hcache(&g, pos);
                if let Some(spec) = ruin_at(&g, pos, &h) {
                    target = Some((pos, spec, h));
                    break;
                }
            }
            if target.is_some() { break; }
        }
        let Some((pos, spec, h)) = target else { panic!("no ruin in sampled range") };
        let mut sections: Vec<Box<[u16; SECTION_VOLUME]>> = (0..crate::SECTION_COUNT)
            .map(|_| Box::new([AIR_BLOCK_ID; SECTION_VOLUME]))
            .collect();
        let mut motion = [0i32; 256];
        let placed = place_ruin(&gen(seed), pos, &h, &mut sections, &mut motion);
        assert_eq!(placed, Some(spec));
        let section = ((spec.floor_y + 2 - crate::MIN_Y) >> 4) as usize;
        let local = ((spec.floor_y + 2 - crate::MIN_Y) & 15) as usize * 256
            + (spec.cz as usize) * 16 + spec.cx as usize;
        assert_eq!(sections[section][local], LANTERN_BLOCK_ID);
    }
}
