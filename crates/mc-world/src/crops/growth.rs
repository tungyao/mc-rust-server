//! growth: P37 各生长模型的纯函数（不依赖网络/背包/协议）。
//!
//! - `Age`：单属性 age 的阶段推进与 blockstate 解析在 `crops/mod.rs`；
//! - `Tree`：复用 P26 小树冠形，材质换成对应物种的真实 log/leaves ID；
//! - `Vertical`：竖向堆叠时按高度逐段放置（甘蔗/仙人掌/竹子）；
//! - `Side`：可可 age×facing 双属性解析；
//! - `Fruit`：瓜梗/南瓜梗成熟后邻格落整块瓜（MVP）。

use super::{AgeCropSpec, TreeSpec, COCOA};

/// 按全局 tick 差算 age（只前进不回退，超高钳制）。
pub fn age_for_elapsed(elapsed: u64, interval: u64, max_age: u8) -> u8 {
    (elapsed / interval.max(1)).min(max_age as u64) as u8
}

/// 小树形状（P26 冠形）：树干 4 格 + 冠层 53 格，trunk 在前、leaves 在后。
/// 材质由调用方指定，`sapling_oak_blocks` 用橡树 ID 调用本函数（薄 shim）。
pub fn tree_blocks(log_id: u16, leaves_id: u16, sx: i32, sy: i32, sz: i32) -> Vec<((i32, i32, i32), u16)> {
    let top = sy + crate::SAPLING_TRUNK_H as i32 - 1;
    let mut out = Vec::with_capacity(57);
    for y in sy..=top {
        out.push(((sx, y, sz), log_id));
    }
    for dy in -2i32..=1 {
        let y = top + dy;
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                let shape = match dy {
                    -2 | -1 => !(dx.abs() == 2 && dz.abs() == 2),
                    0 => dx.abs() <= 1 && dz.abs() <= 1,
                    _ => dx.abs() + dz.abs() <= 1,
                };
                if !shape {
                    continue;
                }
                if dx == 0 && y >= sy && y <= top && dz == 0 {
                    continue; // 树干柱：trunk 已占，leaves 不重复
                }
                out.push(((sx + dx, y, sz + dz), leaves_id));
            }
        }
    }
    out
}

/// 按树苗规格生成小树形状。
pub fn sapling_tree_blocks(spec: &TreeSpec, x: i32, y: i32, z: i32) -> Vec<((i32, i32, i32), u16)> {
    tree_blocks(spec.log_id, spec.leaves_id, x, y, z)
}

/// 甘蔗/仙人掌/竹子的第 `height`（0=基部）格应放的 blockstate。
pub fn vertical_state(
    spec: &super::VerticalSpec,
    height: u8,
) -> u16 {
    (spec.state_fn)(height.min(spec.max_height - 1))
}

/// 可可面朝索引：协议 face 2=north/3=south/4=west/5=east → 0..3。
/// 侧面 face（0/1 或非法）返回 None。
pub fn cocoa_facing_index(face: i32) -> Option<u8> {
    match face {
        2 => Some(0),
        3 => Some(1),
        4 => Some(2),
        5 => Some(3),
        _ => None,
    }
}

/// 可可所贴原木的坐标（facing 0=north → 支撑在 z+1，依此类推）。
pub fn cocoa_support_pos(x: i32, y: i32, z: i32, facing: u8) -> (i32, i32, i32) {
    match facing % 4 {
        0 => (x, y, z + 1),
        1 => (x, y, z - 1),
        2 => (x + 1, y, z),
        _ => (x - 1, y, z),
    }
}

/// 可可 (age, facing) → blockstate：`states` 按 age×facing 排列，facing 最快。
pub fn cocoa_state(age: u8, facing: u8) -> u16 {
    COCOA.base_id + (age.min(COCOA.max_age) as u16) * 4 + (facing % 4) as u16
}

/// 猪笼草 (age) → (lower, upper)：states 按 age×half 排列，upper 在前。
pub fn pitcher_states(age: u8) -> (u16, u16) {
    let first = super::PITCHER_CROP.base_id - 1; // upper age=0
    let upper = first + (age.min(super::PITCHER_CROP.max_age) as u16) * 2;
    (upper + 1, upper)
}

/// 瓜果成熟后的候选邻格（东/南/西/北，确定性顺序）。原版要求邻格有耕地，
/// 本服 MVP 只要可替换（空气/水）即可，见 `crops/mod.rs` 文档。
pub fn fruit_neighbors(x: i32, y: i32, z: i32) -> [(i32, i32, i32); 4] {
    [(x + 1, y, z), (x, y, z + 1), (x - 1, y, z), (x, y, z - 1)]
}

/// 该 age 作物是否结瓜（成熟且配置了果实）。
pub fn ripe_fruit(spec: &AgeCropSpec, age: u8) -> bool {
    spec.fruit.is_some() && age >= spec.max_age
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crops::{SUGAR_CANE, BAMBOO, OAK_TREE};

    #[test]
    fn age_elapsed_is_clamped_and_never_regresses() {
        assert_eq!(age_for_elapsed(0, 2, 7), 0);
        assert_eq!(age_for_elapsed(3, 2, 7), 1);
        assert_eq!(age_for_elapsed(100, 2, 7), 7);
        assert_eq!(age_for_elapsed(u64::MAX, 0, 3), 3);
    }

    #[test]
    fn tree_blocks_keep_p26_shape_with_custom_material() {
        let oak = tree_blocks(OAK_TREE.log_id, OAK_TREE.leaves_id, 0, 70, 0);
        assert_eq!(oak.len(), 57);
        assert_eq!(oak.iter().filter(|(_, id)| *id == OAK_TREE.log_id).count(), 4);
        let birch = sapling_tree_blocks(&super::super::BIRCH, 10, 70, -5);
        assert_eq!(birch.len(), oak.len());
        for (((px, py, pz), _), ((bx, by, bz), _)) in oak.iter().zip(birch.iter()) {
            assert_eq!((bx - px, by - py, bz - pz), (10, 0, -5));
        }
        assert!(birch.iter().any(|(_, id)| *id == super::super::BIRCH.log_id));
        assert!(birch.iter().any(|(_, id)| *id == super::super::BIRCH.leaves_id));
    }

    #[test]
    fn vertical_and_fruit_helpers() {
        assert_eq!(vertical_state(&SUGAR_CANE, 0), SUGAR_CANE.base_id);
        assert_eq!(vertical_state(&BAMBOO, 5), 18640);
        assert_eq!(cocoa_facing_index(2), Some(0));
        assert_eq!(cocoa_facing_index(5), Some(3));
        assert_eq!(cocoa_facing_index(1), None);
        assert_eq!(
            fruit_neighbors(0, 70, 0),
            [(1, 70, 0), (0, 70, 1), (-1, 70, 0), (0, 70, -1)]
        );
        assert!(ripe_fruit(&super::super::MELON_STEM, 7));
        assert!(!ripe_fruit(&super::super::NETHER_WART, 3));
    }
}
