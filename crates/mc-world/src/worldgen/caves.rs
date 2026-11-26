//! P45 v2 洞穴：预计算 4×4×4 格点 + 三线性插值（cheese），
//! 插值结果按 **2×2×2 子格** 预计算成布尔表，叠加两条 2D 噪声带定义的起伏隧道；
//! 不再“整格 4×4×4 一起挖”，也不在逐方块循环里做插值/3D 噪声。
//!
//! 性能：格点 5×5×40 ≈ 1000 次 3D 噪声（~8k hash）；2×2×2 子格约 3800 次
//! 插值；隧道每列 3 次 2D 噪声。逐方块只剩 3 次数组索引 + 比较。
//! 纯函数、单 chunk 可独立生成：格点按世界坐标采样，边界格点由 seed 直接算。
//! 地表 8 格壳与“只对 stone 生效”由调用方 `generate_v2` 保证。
//! 布局依据：`docs/worldgen-plan.md` §3.3/§5.1。

use super::noise::{lerp, smootherstep, value_noise_2d, value_noise_3d};
use super::{MAX_SURFACE_Y, MIN_Y, SEA_LEVEL, SPAWN_BLOCK_X, SPAWN_BLOCK_Z};

const CELL: i32 = 4;
/// 水平格点数（0..=4；chunk 边界 16 格恰好落在格点上）。
const GRID_XZ: usize = 5;
/// 纵向格点数：洞穴只在 `y <= h-9 <= MAX_SURFACE_Y-9` 使用，多留插值边界。
const GRID_Y: usize = ((MAX_SURFACE_Y - 8 - MIN_Y) / CELL + 2) as usize;
const CAVE_SCALE: f64 = 24.0;
const CAVE_SALT: u64 = 0xE703_7ED1_A0B4_28DB;
const CHEESE_THRESHOLD: f64 = 0.68;

/// 2×2×2 子格覆盖的 y 区间（与调用方的 `MIN_Y+9 ..= h-9` 对齐）。
const CAVE_Y_MIN: i32 = MIN_Y + 9;
const CAVE_Y_MAX: i32 = MAX_SURFACE_Y - 8;
const SUB_Y_CELLS: usize = ((CAVE_Y_MAX - CAVE_Y_MIN) / 2 + 1) as usize;
const SUB_XZ_CELLS: usize = 8;

const TUNNEL_SALT_A: u64 = 0x27D4_EB2F_1656_7B1F;
const TUNNEL_SALT_B: u64 = 0x94D0_49BB_1331_11EB;
const TUNNEL_Y_SALT: u64 = 0xA076_1D64_78BD_642F;
const TUNNEL_SCALE_A: f64 = 180.0;
const TUNNEL_SCALE_B: f64 = 240.0;
const TUNNEL_Y_SCALE: f64 = 96.0;
const TUNNEL_MIN_STRENGTH: f64 = 0.55;
/// 出生圈下方额外 6 格不挖，防掉洞（地表壳由调用方另保 8 格）。
const SPAWN_CAVE_CLEAR: f64 = 6.0;

fn grid_index(gi: usize, gy: usize, gz: usize) -> usize {
    (gy * GRID_XZ + gz) * GRID_XZ + gi
}

fn sub_index(cx: usize, cy: usize, cz: usize) -> usize {
    (cy * SUB_XZ_CELLS + cz) * SUB_XZ_CELLS + cx
}

/// v2 洞穴掩码：预计算格点、2×2×2 cheese 表与逐列隧道参数。
pub(super) struct CaveMask {
    /// 2×2×2 子格的最终挖空表（cheese 插值结果 OR 隧道判定）。
    carved_table: Vec<bool>,
    spawn_clear: [bool; 256],
}

impl CaveMask {
    pub(super) fn new(seed: i64, base_x: i32, base_z: i32) -> Self {
        let mut grid = vec![0.0; GRID_XZ * GRID_XZ * GRID_Y];
        for gy in 0..GRID_Y {
            let wy = (MIN_Y + gy as i32 * CELL) as f64;
            for gz in 0..GRID_XZ {
                let wz = (base_z + gz as i32 * CELL) as f64;
                for gi in 0..GRID_XZ {
                    let wx = (base_x + gi as i32 * CELL) as f64;
                    grid[grid_index(gi, gy, gz)] =
                        value_noise_3d(seed, wx, wy, wz, CAVE_SCALE, CAVE_SALT);
                }
            }
        }
        // 逐列隧道参数（只在建表时使用，不在逐方块循环里做 f64 运算）。
        let mut tunnel_center = [0.0; 256];
        let mut tunnel_half_width = [0.0; 256];
        for lz in 0..16i32 {
            for lx in 0..16i32 {
                let wx = base_x + lx;
                let wz = base_z + lz;
                let fx = wx as f64;
                let fz = wz as f64;
                let a = value_noise_2d(seed, fx, fz, TUNNEL_SCALE_A, TUNNEL_SALT_A);
                let b = value_noise_2d(seed, fx, fz, TUNNEL_SCALE_B, TUNNEL_SALT_B);
                let strength = (a - 0.5).abs().max((b - 0.5).abs()) * 2.0;
                let col = (lz * 16 + lx) as usize;
                tunnel_center[col] = SEA_LEVEL as f64 - 18.0
                    + (value_noise_2d(seed, fx, fz, TUNNEL_Y_SCALE, TUNNEL_Y_SALT) - 0.5) * 30.0;
                tunnel_half_width[col] = if strength < TUNNEL_MIN_STRENGTH {
                    0.0
                } else {
                    2.0 + strength * 2.0
                };
            }
        }
        // 先按 2×2×2 子格插值并把隧道合并进来，逐方块只剩一次数组索引。
        let mut carved_table = vec![false; SUB_Y_CELLS * SUB_XZ_CELLS * SUB_XZ_CELLS];
        for cy in 0..SUB_Y_CELLS {
            let y = CAVE_Y_MIN + cy as i32 * 2 + 1;
            let gy = ((y - MIN_Y) / CELL) as usize;
            let ty = smootherstep(((y - MIN_Y) % CELL) as f64 / CELL as f64);
            for cz in 0..SUB_XZ_CELLS {
                let lz = cz as i32 * 2 + 1;
                let gz = (lz / CELL) as usize;
                let tz = smootherstep((lz % CELL) as f64 / CELL as f64);
                for cx in 0..SUB_XZ_CELLS {
                    let lx = cx as i32 * 2 + 1;
                    let gi = (lx / CELL) as usize;
                    let tx = smootherstep((lx % CELL) as f64 / CELL as f64);
                    let v000 = grid[grid_index(gi, gy, gz)];
                    let v100 = grid[grid_index(gi + 1, gy, gz)];
                    let v010 = grid[grid_index(gi, gy + 1, gz)];
                    let v110 = grid[grid_index(gi + 1, gy + 1, gz)];
                    let v001 = grid[grid_index(gi, gy, gz + 1)];
                    let v101 = grid[grid_index(gi + 1, gy, gz + 1)];
                    let v011 = grid[grid_index(gi, gy + 1, gz + 1)];
                    let v111 = grid[grid_index(gi + 1, gy + 1, gz + 1)];
                    let a = lerp(lerp(v000, v100, tx), lerp(v010, v110, tx), ty);
                    let b = lerp(lerp(v001, v101, tx), lerp(v011, v111, tx), ty);
                    let cell_col = lz as usize * 16 + lx as usize;
                    let tunnel = tunnel_half_width[cell_col] > 0.0
                        && ((y as f64) - tunnel_center[cell_col]).abs() <= tunnel_half_width[cell_col];
                    carved_table[sub_index(cx, cy, cz)] =
                        tunnel || lerp(a, b, tz) > CHEESE_THRESHOLD;
                }
            }
        }
        let mut spawn_clear = [false; 256];
        for lz in 0..16i32 {
            for lx in 0..16i32 {
                let wx = base_x + lx;
                let wz = base_z + lz;
                let dx = (wx - SPAWN_BLOCK_X) as f64;
                let dz = (wz - SPAWN_BLOCK_Z) as f64;
                spawn_clear[(lz * 16 + lx) as usize] =
                    (dx * dx + dz * dz).sqrt() <= SPAWN_CAVE_CLEAR;
            }
        }
        Self { carved_table, spawn_clear }
    }

    /// 该方块是否应挖空（调用方已保证是 stone 且在壳以下）。
    pub(super) fn carved(&self, _wx: i32, lx: i32, y: i32, _wz: i32, lz: i32) -> bool {
        let col = (lz * 16 + lx) as usize;
        if self.spawn_clear[col] {
            return false;
        }
        if y < CAVE_Y_MIN || y > CAVE_Y_MAX {
            return false;
        }
        let cy = ((y - CAVE_Y_MIN) / 2) as usize;
        self.carved_table[sub_index((lx / 2) as usize, cy, (lz / 2) as usize)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_covers_all_block_interpolation_positions() {
        assert_eq!(GRID_XZ, 5);
        assert!(GRID_Y > ((MAX_SURFACE_Y - 8 - MIN_Y) / CELL) as usize);
        assert_eq!(SUB_XZ_CELLS, 8);
        assert!(SUB_Y_CELLS >= 39);
        let mask = CaveMask::new(1, 32, 48);
        assert_eq!(mask.carved_table.len(), SUB_Y_CELLS * 8 * 8);
    }

    #[test]
    fn carving_is_smooth_not_whole_4x4x4_cells() {
        let mask = CaveMask::new(crate::DEFAULT_SEED, 0, 0);
        // 同一 4×4×4 格内至少出现一次“部分挖空”，证明不是整格同挖。
        let mut partial = false;
        'outer: for cz in 0..4 {
            for cx in 0..4 {
                for cell_y in 0..6 {
                    let mut states = Vec::new();
                    for dz in 0..CELL {
                        for dx in 0..CELL {
                            let y = CAVE_Y_MIN + cell_y * CELL + dz; // 借 dz 让 y 在格内变化
                            states.push(mask.carved(0, cx * CELL + dx, y, 0, cz * CELL + dz));
                        }
                    }
                    if states.iter().any(|&s| s) && states.iter().any(|&s| !s) {
                        partial = true;
                        break 'outer;
                    }
                }
            }
        }
        assert!(partial, "expected interpolated partial cells, not blocky 4×4×4");
    }
}
