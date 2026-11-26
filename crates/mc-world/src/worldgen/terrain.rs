//! P45 v2 地形高度：在 v1 三层 value noise 上叠加侵蚀、山脊与河流雕刻。
//!
//! 纯函数：只由 `(seed, 世界 x/z)` 决定，不读邻居 chunk、无全局状态，
//! 因此 `ChunkCache` 并行生成与重连都安全。v1 的 `height_at` 不经过本模块。
//!
//! 出生点隔离：`(369,381)` 半径 ≤20 内完全使用 v1 高度（覆盖整个出生 chunk
//! 368..383 的最远角 ~19.1），20..64 用 smootherstep 淡入；河流权重在
//! 半径 ≤16 内为 0（比要求的更强的保护）。默认 seed 的出生 y=73 与 3×3 干燥
//! 因此逐格不变。算法来源：`docs/worldgen-plan.md` §3.1/§5.4。

use super::noise::{smootherstep, value_noise_2d};
use super::{
    WorldGen, MAX_SURFACE_Y, MIN_SURFACE_Y, SEA_LEVEL, SPAWN_BLOCK_X, SPAWN_BLOCK_Z,
};

/// v1 三层噪声常量（与 `mod.rs` 一致，复制在此只为让 v2 叠加互不污染 v1 代码）。
const BASE_HEIGHT: f64 = 58.0;
const SCALES: [f64; 3] = [128.0, 48.0, 24.0];
const AMPS: [f64; 3] = [20.0, 10.0, 6.0];
const SALTS: [u64; 3] = [
    0x9E37_79B9_7F4A_7C15,
    0xC2B2_AE3D_27D4_EB4F,
    0x1656_67B1_9E37_79F9,
];

const EROSION_SALT: u64 = 0x2545_F491_4F6C_DD1D;
const RIDGE_SALT: u64 = 0x85EB_CA77_B329_3E18;
const RIVER_SALT: u64 = 0xC2B2_AE3D_27D4_EB4F ^ 0xA5A5_A5A5_A5A5_A5A5;
/// 高频 octave 幅度在平坦区最多衰减 45%，模拟热侵蚀的削峰。
const EROSION_MAX: f64 = 0.45;
/// 山脊抬升最大幅度与生效海拔。
const RIDGE_AMPLITUDE: f64 = 22.0;
const RIDGE_START_HEIGHT: f64 = 68.0;
const RIDGE_RAMP: f64 = 14.0;
/// 河流：低频噪声接近 0.52 的带状区域下切到 SEA_LEVEL-2；半宽 0.05/scale≈10 格，
/// 下切坡度 ≤3/格（由新单测锁住）。
pub(super) const RIVER_SCALE: f64 = 192.0;
pub(super) const RIVER_CENTER: f64 = 0.52;
pub(super) const RIVER_HALF_WIDTH: f64 = 0.05;
const RIVER_FLOOR_BELOW_SEA: i32 = 2;

/// v2 全量生效半径（含整个出生 chunk）；半径 ≤20 完全用 v1。
const SPAWN_FULL_RADIUS: f64 = 20.0;
/// v2 淡入结束半径；20..64 平滑过渡，避免高度断层。
const SPAWN_FADE_RADIUS: f64 = 64.0;

/// 三层高度（未取整、未做出生保护），`high_amp` 可单独调高频幅度用于侵蚀。
fn raw_height(seed: i64, x: i32, z: i32, high_amp: f64) -> f64 {
    let fx = x as f64;
    let fz = z as f64;
    let mut h = BASE_HEIGHT;
    h += value_noise_2d(seed, fx, fz, SCALES[0], SALTS[0]) * AMPS[0];
    h += value_noise_2d(seed, fx, fz, SCALES[1], SALTS[1]) * AMPS[1];
    h += value_noise_2d(seed, fx, fz, SCALES[2], SALTS[2]) * high_amp;
    h
}

/// v2 高度权重：出生保护圈内 0，之外 smootherstep 淡入 1。
fn v2_weight(x: i32, z: i32) -> f64 {
    let dx = (x - SPAWN_BLOCK_X) as f64;
    let dz = (z - SPAWN_BLOCK_Z) as f64;
    let distance = (dx * dx + dz * dz).sqrt();
    if distance <= SPAWN_FULL_RADIUS {
        0.0
    } else if distance >= SPAWN_FADE_RADIUS {
        1.0
    } else {
        smootherstep((distance - SPAWN_FULL_RADIUS) / (SPAWN_FADE_RADIUS - SPAWN_FULL_RADIUS))
    }
}

/// 河流强度 ∈ [0,1]：1 在河道中心、0 在岸边之外。供地形与 biome 共用，
/// 保证 river biome 真的落在被下切的河谷上。
pub(super) fn river_strength(seed: i64, x: i32, z: i32) -> f64 {
    let n = value_noise_2d(seed, x as f64, z as f64, RIVER_SCALE, RIVER_SALT);
    let d = (n - RIVER_CENTER).abs();
    if d >= RIVER_HALF_WIDTH {
        return 0.0;
    }
    smootherstep((RIVER_HALF_WIDTH - d) / RIVER_HALF_WIDTH)
}

/// 河流在最终高度里的实际权重（出生保护圈内为 0）。仅回归测试用；
/// 生产路径的等价效果是 `river_strength` 乘末尾的整体 `weight`。
#[cfg(test)]
pub(super) fn river_weight(seed: i64, x: i32, z: i32) -> f64 {
    river_strength(seed, x, z) * v2_weight(x, z)
}

/// v2 世界坐标地表高度：v1 基底 + 侵蚀/山脊/河流（按出生权重缩放）+
/// 与 v1 相同的出生点平滑保护。
pub(super) fn height_at_v2(gen: &WorldGen, x: i32, z: i32) -> i32 {
    let weight = v2_weight(x, z);
    if weight == 0.0 {
        // 出生保护圈内完全走 v1 的 3 次噪声，逐格与 legacy 一致。
        let h = raw_height(gen.seed, x, z, AMPS[2]).round() as i32;
        return gen.spawn_protection(x, z, h.clamp(MIN_SURFACE_Y, MAX_SURFACE_Y));
    }
    // 侵蚀：低频噪声决定高频 octave 的衰减，平坦/低地区更平滑。
    // 直接以“侵蚀后的三层基底”起点，weight=1 时不必再算未侵蚀的 v1 基底。
    let erosion = value_noise_2d(gen.seed, x as f64, z as f64, 160.0, EROSION_SALT);
    let mut h = raw_height(gen.seed, x, z, AMPS[2] * (1.0 - EROSION_MAX * erosion));
    // 山脊：1-|2n-1|，平方后只在高海拔抬升（山地而非全域尖刺）。
    let ridge = 1.0 - (2.0 * value_noise_2d(gen.seed, x as f64, z as f64, 96.0, RIDGE_SALT) - 1.0).abs();
    let mountain = smootherstep(
        ((h - RIDGE_START_HEIGHT) / RIDGE_RAMP).clamp(0.0, 1.0),
    );
    h += ridge * ridge * RIDGE_AMPLITUDE * mountain;
    // 河流：向 SEA_LEVEL-2 下切，只降不升。
    let river = river_strength(gen.seed, x, z);
    if river > 0.0 {
        let floor = (SEA_LEVEL - RIVER_FLOOR_BELOW_SEA) as f64;
        if h > floor {
            h += (floor - h) * river;
        }
    }
    // 只在淡入环（20..64）里补算 v1 基底并混合；weight=1 时省 3 次噪声。
    let h = if weight < 1.0 {
        let plain = raw_height(gen.seed, x, z, AMPS[2]);
        plain + (h - plain) * weight
    } else {
        h
    };
    let h = (h.round() as i32).clamp(MIN_SURFACE_Y, MAX_SURFACE_Y);
    gen.spawn_protection(x, z, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GeneratorVersion;

    #[test]
    fn v2_spawn_chunk_is_byte_identical_to_v1() {
        for seed in [crate::DEFAULT_SEED, 42, -987654321] {
            let v1 = WorldGen::with_version(seed, GeneratorVersion::V1);
            let v2 = WorldGen::with_version(seed, GeneratorVersion::V2);
            // 整个出生 chunk（含最远角）都在 v1 保护圈内。
            for z in 368..=383 {
                for x in 368..=383 {
                    assert_eq!(v2.height_at(x, z), v1.height_at(x, z), "seed {seed} at {x},{z}");
                }
            }
            assert_eq!(v2.spawn_pos(), v1.spawn_pos());
        }
    }

    #[test]
    fn v2_river_does_not_enter_spawn_circle_and_is_continuous() {
        let seed = 424242;
        // 保护圈内河流权重必须为 0。
        for z in 365..=397 {
            for x in 353..=385 {
                let d = (((x - SPAWN_BLOCK_X).pow(2) + (z - SPAWN_BLOCK_Z).pow(2)) as f64).sqrt();
                if d <= 16.0 {
                    assert_eq!(river_weight(seed, x, z), 0.0, "river inside spawn at {x},{z}");
                }
            }
        }
        // 圈外存在连续 ≥8 格的河段（同一个 z 行上 river_strength>0.5 的连续列）。
        let mut best = 0usize;
        for z in -64..=320 {
            let mut run = 0usize;
            for x in -256..=512 {
                if river_weight(seed, x, z) > 0.5 {
                    run += 1;
                    best = best.max(run);
                } else {
                    run = 0;
                }
            }
        }
        assert!(best >= 8, "expected a continuous river run, got {best}");
    }

    #[test]
    fn v2_height_is_bounded_and_adjacent_difference_is_small() {
        for seed in [crate::DEFAULT_SEED, 42, 424242] {
            let gen = WorldGen::with_version(seed, GeneratorVersion::V2);
            let mut prev_row: Option<Vec<i32>> = None;
            for z in -64..=64 {
                let mut row = Vec::with_capacity(129);
                for x in -64..=64 {
                    let h = gen.height_at(x, z);
                    assert!((MIN_SURFACE_Y..=MAX_SURFACE_Y).contains(&h), "seed {seed} h={h} at {x},{z}");
                    row.push(h);
                }
                for pair in row.windows(2) {
                    assert!((pair[0] - pair[1]).abs() <= 3, "seed {seed}: x step {pair:?} at z={z}");
                }
                if let Some(previous) = &prev_row {
                    for (i, (&h, &prev)) in row.iter().zip(previous.iter()).enumerate() {
                        assert!((h - prev).abs() <= 3, "seed {seed}: z step at x={} z={z}: {h} vs {prev}", i as i32 - 64);
                    }
                }
                prev_row = Some(row);
            }
        }
    }
}
