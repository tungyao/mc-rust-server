//! P44 v2 低风险视觉：地表材质过渡、多树种、草花、静态水岸植被。
//!
//! 全部是 `(seed, 世界坐标)` 的纯函数，跨 chunk 连续、可独立生成；
//! 不碰高度场、biome 容器、光照编码，因此协议包体格式零变化。
//!
//! 新方块 ID 全部来自 `crate::registry_id`（`data/777/blocks.json`），
//! 树材质复用 P37 `crops::TREE_SPECS` 的真实 log/leaves ID 与去角冠形。

use super::noise::hash01;
use super::{
    height_cache_index, set_block_if, update_motion, SEA_LEVEL, SPAWN_BLOCK_X, SPAWN_BLOCK_Z,
    SPAWN_TREE_CLEAR, TREE_MAX_TRUNK, TREE_MIN_TRUNK, TREE_SALT,
};
use crate::crops::{TreeSpec, BIRCH, DARK_OAK, OAK_TREE, SPRUCE};
use crate::{
    AIR_BLOCK_ID, BUSH_BLOCK_ID, CLAY_BLOCK_ID, COARSE_DIRT_BLOCK_ID, CORNFLOWER_BLOCK_ID,
    DANDELION_BLOCK_ID, DEAD_BUSH_BLOCK_ID, DIRT_BLOCK_ID, FERN_BLOCK_ID, GRAVEL_BLOCK_ID,
    GRASS_BLOCK_ID, ICE_BLOCK_ID, KELP_BLOCK_ID, LILY_PAD_BLOCK_ID, POPPY_BLOCK_ID,
    PODZOL_BLOCK_ID, SAND_BLOCK_ID, SANDSTONE_BLOCK_ID, SEAGRASS_BLOCK_ID,
    SHORT_GRASS_BLOCK_ID, SNOW_BLOCK_ID, STONE_BLOCK_ID, TALL_GRASS_BLOCK_ID,
    TALL_SEAGRASS_BLOCK_ID, WATER_BLOCK_ID,
};
use mc_protocol::packets::{FOREST_BIOME_ID_777, PLAINS_BIOME_ID_777};

/// 一个 section 的方块数。
const SECTION_VOLUME: usize = 4096;
const CHUNK_COLUMNS: usize = 256;

/// 树种选择 salt：与 tree_for 的密度/干高 salt 正交，改变它不影响树的位置。
const SPECIES_SALT: u64 = 0x6A09_E667_F3BC_C909;
const SURFACE_SALT: u64 = 0xBB67_AE85_84CA_A73B;
const WATER_SALT: u64 = 0x3C6E_F372_FE94_F82B;
const PLANT_SALT: u64 = 0x510E_527F_ADE6_48D1;
const PLANT_KIND_SALT: u64 = 0x9B05_688C_612B_F415;
/// 森林 1/10、平原 1/40（沿用 v1 密度，避免视距内树暴增）。
const FOREST_TREE_DENSITY: f64 = 0.10;
const PLAINS_TREE_DENSITY: f64 = 0.025;
/// 地表植被密度（每列 4.5% 概率），比树稀疏得多。
const PLANT_DENSITY: f64 = 0.045;
const WATER_PLANT_DENSITY: f64 = 0.06;
const ICE_DENSITY: f64 = 0.06;

/// v2 候选树：世界坐标 + 地表 y + 树干高度 + 树种。
#[derive(Debug, Clone, Copy)]
pub(super) struct TreeV2 {
    pub(super) x: i32,
    pub(super) z: i32,
    pub(super) ground_y: i32,
    pub(super) trunk_h: i32,
    pub(super) spec: &'static TreeSpec,
}

impl TreeV2 {
    pub(super) fn top_y(&self) -> i32 {
        self.ground_y + self.trunk_h
    }
}

/// 出生保护圈（7 格内无树/无植物/材质固定为草土）判定。
fn near_spawn(x: i32, z: i32) -> bool {
    (x - SPAWN_BLOCK_X).abs() <= SPAWN_TREE_CLEAR
        && (z - SPAWN_BLOCK_Z).abs() <= SPAWN_TREE_CLEAR
}

/// 四层地表（top=h，d=1..3 往下）；dry/beach/水下各自过渡。
pub(super) fn surface_layers(
    seed: i64,
    x: i32,
    z: i32,
    h: i32,
    biome: i32,
) -> [u16; 4] {
    if near_spawn(x, z) {
        return if h > SEA_LEVEL + 2 {
            [GRASS_BLOCK_ID, DIRT_BLOCK_ID, DIRT_BLOCK_ID, DIRT_BLOCK_ID]
        } else {
            [SAND_BLOCK_ID, SAND_BLOCK_ID, SAND_BLOCK_ID, SANDSTONE_BLOCK_ID]
        };
    }
    let roll = |d: i32| hash01(seed, x as i64, d as i64, z as i64, SURFACE_SALT);
    if h <= SEA_LEVEL + 2 {
        // 沙滩/水下：沙 + 砂岩过渡，偶有砾石滩与黏土斑块。
        let sub1 = if roll(1) < 0.18 { GRAVEL_BLOCK_ID } else { SAND_BLOCK_ID };
        let sub2 = if roll(2) < 0.45 { SANDSTONE_BLOCK_ID } else { SAND_BLOCK_ID };
        let sub3 = if roll(3) < 0.22 { CLAY_BLOCK_ID } else { SANDSTONE_BLOCK_ID };
        [SAND_BLOCK_ID, sub1, sub2, sub3]
    } else if h <= 78 {
        [GRASS_BLOCK_ID, DIRT_BLOCK_ID, DIRT_BLOCK_ID, DIRT_BLOCK_ID]
    } else if h <= 84 {
        let top = if roll(0) < 0.35 { COARSE_DIRT_BLOCK_ID } else { GRASS_BLOCK_ID };
        let sub1 = if roll(1) < 0.30 { COARSE_DIRT_BLOCK_ID } else { DIRT_BLOCK_ID };
        let sub2 = if biome == FOREST_BIOME_ID_777 && roll(2) < 0.30 {
            PODZOL_BLOCK_ID
        } else {
            DIRT_BLOCK_ID
        };
        [top, sub1, sub2, DIRT_BLOCK_ID]
    } else {
        // 高地：粗土/裸岩/积雪，制造高度分带但不改变高度场。
        let r = roll(0);
        let top = if r < 0.45 {
            SNOW_BLOCK_ID
        } else if r < 0.75 {
            COARSE_DIRT_BLOCK_ID
        } else {
            STONE_BLOCK_ID
        };
        let sub1 = if roll(1) < 0.5 { COARSE_DIRT_BLOCK_ID } else { STONE_BLOCK_ID };
        [top, sub1, STONE_BLOCK_ID, STONE_BLOCK_ID]
    }
}

/// 水柱顶层：浅水偶有冰封（替换 source water，不引入新 water state，
/// 因此 `fluid_count` 口径仍是 water source 数）。
pub(super) fn water_at(seed: i64, x: i32, z: i32, h: i32) -> u16 {
    if !near_spawn(x, z)
        && h >= SEA_LEVEL - 4
        && hash01(seed, x as i64, 0, z as i64, WATER_SALT) < ICE_DENSITY
    {
        ICE_BLOCK_ID
    } else {
        WATER_BLOCK_ID
    }
}

/// 按 biome 与坐标 hash 选树种：森林有橡/桦/云杉/黑橡，平原只有橡/桦。
fn species_for(seed: i64, x: i32, z: i32, biome: i32) -> &'static TreeSpec {
    let roll = hash01(seed, x as i64, 9, z as i64, SPECIES_SALT);
    match biome {
        FOREST_BIOME_ID_777 => {
            if roll < 0.40 {
                &OAK_TREE
            } else if roll < 0.70 {
                &BIRCH
            } else if roll < 0.90 {
                &SPRUCE
            } else {
                &DARK_OAK
            }
        }
        PLAINS_BIOME_ID_777 => {
            if roll < 0.80 { &OAK_TREE } else { &BIRCH }
        }
        _ => &OAK_TREE,
    }
}

/// 单列树判定（v2）：干燥、平缓、远离出生点，按 biome 密度投骰后选树种。
pub(super) fn tree_for(
    gen: &super::WorldGen,
    x: i32,
    z: i32,
    h: i32,
    slope_ok: bool,
    biome: i32,
) -> Option<TreeV2> {
    if h <= SEA_LEVEL + 2 || !slope_ok || near_spawn(x, z) {
        return None;
    }
    let density = match biome {
        FOREST_BIOME_ID_777 => FOREST_TREE_DENSITY,
        PLAINS_BIOME_ID_777 => PLAINS_TREE_DENSITY,
        _ => return None,
    };
    if hash01(gen.seed, x as i64, 7, z as i64, TREE_SALT) >= density {
        return None;
    }
    let trunk_h = TREE_MIN_TRUNK
        + (hash01(gen.seed, x as i64, 8, z as i64, TREE_SALT ^ 0x55)
            * (TREE_MAX_TRUNK - TREE_MIN_TRUNK + 1) as f64) as i32;
    Some(TreeV2 {
        x,
        z,
        ground_y: h,
        trunk_h,
        spec: species_for(gen.seed, x, z, biome),
    })
}

/// 扫描 base-2..=base+17 的候选树，只写本 chunk 内的方块，跨 chunk 树冠连续。
pub(super) fn place_trees(
    gen: &super::WorldGen,
    base_x: i32,
    base_z: i32,
    hcache: &[i32],
    sections: &mut [Box<[u16; SECTION_VOLUME]>],
    motion: &mut [i32; CHUNK_COLUMNS],
) {
    let mut biome_cache: Vec<((i32, i32), i32)> = Vec::with_capacity(9);
    for oz in super::TREE_SCAN_MIN..=super::TREE_SCAN_MAX {
        for ox in super::TREE_SCAN_MIN..=super::TREE_SCAN_MAX {
            let wx = base_x + ox;
            let wz = base_z + oz;
            let h = hcache[height_cache_index(ox, oz)];
            let slope_ok = (hcache[height_cache_index(ox - 1, oz)] - h).abs() <= 1
                && (hcache[height_cache_index(ox + 1, oz)] - h).abs() <= 1
                && (hcache[height_cache_index(ox, oz - 1)] - h).abs() <= 1
                && (hcache[height_cache_index(ox, oz + 1)] - h).abs() <= 1;
            let cx = wx.div_euclid(16);
            let cz = wz.div_euclid(16);
            let biome = if let Some(&(_, b)) = biome_cache.iter().find(|(p, _)| *p == (cx, cz)) {
                b
            } else {
                let b = gen.chunk_biome(mc_core::ChunkPos(cx, cz));
                biome_cache.push(((cx, cz), b));
                b
            };
            let Some(tree) = tree_for(gen, wx, wz, h, slope_ok, biome) else {
                continue;
            };
            write_tree(&tree, base_x, base_z, sections, motion);
        }
    }
}

/// 写入一棵 v2 树：树干用该树种真实 log ID，树叶用真实 leaves ID；
/// 形状仍是 5×5 去角，云杉改为逐层收缩的圆锥（冠半径始终 ≤2，不改扫描范围）。
fn write_tree(
    tree: &TreeV2,
    base_x: i32,
    base_z: i32,
    sections: &mut [Box<[u16; SECTION_VOLUME]>],
    motion: &mut [i32; CHUNK_COLUMNS],
) {
    let lx = tree.x - base_x;
    let lz = tree.z - base_z;
    let top = tree.top_y();
    let log = tree.spec.log_id;
    let leaves = tree.spec.leaves_id;
    for y in (tree.ground_y + 1)..=top {
        if set_block_if(sections, lx, y, lz, log, &[AIR_BLOCK_ID, leaves]) {
            update_motion(motion, lx, lz, y);
        }
    }
    let spruce = tree.spec.name == "spruce";
    for dy in -2i32..=1 {
        let y = top + dy;
        let radius: i32 = if spruce {
            match dy {
                -2 => 1,
                -1 => 2,
                0 => 1,
                _ => 0,
            }
        } else {
            2
        };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let shape = if spruce {
                    true
                } else {
                    match dy {
                        -2 | -1 => !(dx.abs() == 2 && dz.abs() == 2),
                        0 => dx.abs() <= 1 && dz.abs() <= 1,
                        _ => dx.abs() + dz.abs() <= 1,
                    }
                };
                if !shape {
                    continue;
                }
                if set_block_if(sections, lx + dx, y, lz + dz, leaves, &[AIR_BLOCK_ID, leaves]) {
                    update_motion(motion, lx + dx, lz + dz, y);
                }
            }
        }
    }
}

/// 草花与静态水岸植被：只在本 chunk 内落子，出生圈内不放任何高于地面的植物，
/// 且不写 `motion`（草花/海草/睡莲不构成 MOTION_BLOCKING）。
pub(super) fn place_plants(
    seed: i64,
    base_x: i32,
    base_z: i32,
    hcache: &[i32],
    sections: &mut [Box<[u16; SECTION_VOLUME]>],
) {
    for lz in 0..16i32 {
        for lx in 0..16i32 {
            let wx = base_x + lx;
            let wz = base_z + lz;
            if near_spawn(wx, wz) {
                continue;
            }
            let h = hcache[height_cache_index(lx, lz)];
            let roll = hash01(seed, wx as i64, 0, wz as i64, PLANT_SALT);
            if h > SEA_LEVEL + 2 {
                if roll < PLANT_DENSITY {
                    let kind = hash01(seed, wx as i64, 0, wz as i64, PLANT_KIND_SALT);
                    let id = if kind < 0.26 {
                        SHORT_GRASS_BLOCK_ID
                    } else if kind < 0.42 {
                        TALL_GRASS_BLOCK_ID
                    } else if kind < 0.56 {
                        FERN_BLOCK_ID
                    } else if kind < 0.68 {
                        DANDELION_BLOCK_ID
                    } else if kind < 0.80 {
                        POPPY_BLOCK_ID
                    } else if kind < 0.88 {
                        CORNFLOWER_BLOCK_ID
                    } else if kind < 0.95 {
                        DEAD_BUSH_BLOCK_ID
                    } else {
                        BUSH_BLOCK_ID
                    };
                    set_block_if(sections, lx, h + 1, lz, id, &[AIR_BLOCK_ID]);
                }
            } else if h < SEA_LEVEL {
                if roll < WATER_PLANT_DENSITY {
                    let kind = hash01(seed, wx as i64, 1, wz as i64, PLANT_KIND_SALT);
                    let id = if kind < 0.55 {
                        SEAGRASS_BLOCK_ID
                    } else if kind < 0.85 {
                        TALL_SEAGRASS_BLOCK_ID
                    } else {
                        KELP_BLOCK_ID
                    };
                    set_block_if(sections, lx, h + 1, lz, id, &[WATER_BLOCK_ID]);
                }
                if roll > 1.0 - WATER_PLANT_DENSITY / 2.0
                    && get_block(sections, lx, SEA_LEVEL, lz) == WATER_BLOCK_ID
                {
                    set_block_if(sections, lx, SEA_LEVEL + 1, lz, LILY_PAD_BLOCK_ID, &[AIR_BLOCK_ID]);
                }
            }
        }
    }
}

/// 装饰方块：不计入 `heights`/`motion`，编辑重算高度时也要跳过。
pub fn is_plant(id: u16) -> bool {
    matches!(
        id,
        SHORT_GRASS_BLOCK_ID
            | TALL_GRASS_BLOCK_ID
            | FERN_BLOCK_ID
            | DEAD_BUSH_BLOCK_ID
            | BUSH_BLOCK_ID
            | DANDELION_BLOCK_ID
            | POPPY_BLOCK_ID
            | CORNFLOWER_BLOCK_ID
            | SEAGRASS_BLOCK_ID
            | TALL_SEAGRASS_BLOCK_ID
            | KELP_BLOCK_ID
            | LILY_PAD_BLOCK_ID
    )
}

fn get_block(
    sections: &[Box<[u16; SECTION_VOLUME]>],
    lx: i32,
    y: i32,
    lz: i32,
) -> u16 {
    if !(0..16).contains(&lx) || !(0..16).contains(&lz) {
        return AIR_BLOCK_ID;
    }
    let section = ((y - crate::MIN_Y) >> 4) as usize;
    if section >= sections.len() {
        return AIR_BLOCK_ID;
    }
    let local_y = ((y - crate::MIN_Y) & 15) as usize;
    sections[section][local_y * CHUNK_COLUMNS + lz as usize * 16 + lx as usize]
}

/// 高度缓存边长的自检（v2 冠半径仍为 2，与 v1 共用同一缓存）。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_constants_stay_within_v1_scan_range() {
        assert_eq!(super::super::HEIGHT_CACHE_SIDE, 22);
        assert_eq!(super::super::HEIGHT_CACHE_MIN, -3);
        assert!(super::super::TREE_SCAN_MIN <= -2);
        assert!(super::super::TREE_SCAN_MAX >= 17);
        // 冠半径 ≤2，不超出 v1 的扫描窗口。
        let _ = SECTION_VOLUME;
    }

    #[test]
    fn plants_are_not_terrain_or_motion_blocks() {
        for id in [
            SHORT_GRASS_BLOCK_ID,
            TALL_GRASS_BLOCK_ID,
            FERN_BLOCK_ID,
            DEAD_BUSH_BLOCK_ID,
            BUSH_BLOCK_ID,
            DANDELION_BLOCK_ID,
            POPPY_BLOCK_ID,
            CORNFLOWER_BLOCK_ID,
            SEAGRASS_BLOCK_ID,
            TALL_SEAGRASS_BLOCK_ID,
            KELP_BLOCK_ID,
            LILY_PAD_BLOCK_ID,
        ] {
            assert!(is_plant(id));
            assert_ne!(id, STONE_BLOCK_ID);
        }
        assert!(!is_plant(OAK_TREE.leaves_id));
    }
}
