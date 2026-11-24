//! drops: 确定性掉落表（无随机种子污染），只兼容 26.3/777。
//!
//! 输入 blockstate ID（见 `mc_world` 常量），输出 `(item_id, count)`（`minecraft:item`
//! 注册表 ID，26.3 客户端 jar 实测，见 HANDOFF）：
//!
//! | 方块 | 掉落 |
//! |---|----|
//! | grass_block(9) | dirt(55)×1 |
//! | dirt(10) | dirt(55)×1 |
//! | stone(1) | cobblestone(62)×1 |
//! | sand(121) | sand(88)×1 |
//! | oak_log(140) | oak_log(163)×1 |
//! | oak_leaves(294) | 1/8 oak_sapling(77)×1，否则 1/8 stick(1060)×1（world+pos hash） |
//! | coal_ore(136) | coal(1010)×1 |
//! | iron_ore(134) | raw_iron(1017)×1 |
//!
//! 限制（TODO）：无工具等级/精准采集检查（都按徒手掉落），无经验球。

use crate::{
    COAL_ORE_BLOCK_ID, DIRT_BLOCK_ID, GRASS_BLOCK_ID, IRON_ORE_BLOCK_ID, OAK_LEAVES_BLOCK_ID,
    OAK_LOG_BLOCK_ID, SAND_BLOCK_ID, STONE_BLOCK_ID,
};

/// `minecraft:item` 注册表 ID（26.3 jar `BuiltInRegistries.ITEM` 实测）。
pub const ITEM_STONE: i32 = 1;
pub const ITEM_DIRT: i32 = 55;
pub const ITEM_COBBLESTONE: i32 = 62;
pub const ITEM_OAK_SAPLING: i32 = 77;
pub const ITEM_SAND: i32 = 88;
pub const ITEM_OAK_LOG: i32 = 163;
pub const ITEM_COAL: i32 = 1010;
pub const ITEM_STICK: i32 = 1060;
pub const ITEM_RAW_IRON: i32 = 1017;

/// 单种掉落：物品 + 数量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drop {
    pub item_id: i32,
    pub count: i32,
}

/// 确定性 64 位 hash（splitmix64，world seed 与坐标混合，无全局随机状态）。
fn hash_pos(seed: i64, x: i32, y: i32, z: i32) -> u64 {
    let mut h = seed as u64 ^ 0x9e3779b97f4a7c15;
    h = h.wrapping_add(x as u64).wrapping_mul(0xbf58476d1ce4e5b9);
    h = h.wrapping_add(y as u64).wrapping_mul(0x94d049bb133111eb);
    h = h.wrapping_add(z as u64).wrapping_mul(0xbf58476d1ce4e5b9);
    h ^= h >> 29;
    h = h.wrapping_mul(0xbf58476d1ce4e5b9);
    h ^= h >> 32;
    h = h.wrapping_mul(0x94d049bb133111eb);
    h ^= h >> 29;
    h
}

/// 方块破坏掉落（创造/生存徒手口径；工具等级与精准采集留 TODO）。
/// `seed` 只用于树叶概率分支，其它掉落与 seed 无关。
pub fn drops_for_block(block_id: u16, x: i32, y: i32, z: i32, seed: i64) -> Vec<Drop> {
    match block_id {
        GRASS_BLOCK_ID | DIRT_BLOCK_ID => vec![Drop { item_id: ITEM_DIRT, count: 1 }],
        STONE_BLOCK_ID => vec![Drop { item_id: ITEM_COBBLESTONE, count: 1 }],
        SAND_BLOCK_ID => vec![Drop { item_id: ITEM_SAND, count: 1 }],
        OAK_LOG_BLOCK_ID => vec![Drop { item_id: ITEM_OAK_LOG, count: 1 }],
        OAK_LEAVES_BLOCK_ID => {
            let h = hash_pos(seed, x, y, z) % 8;
            if h == 0 {
                vec![Drop { item_id: ITEM_OAK_SAPLING, count: 1 }]
            } else if h == 1 {
                vec![Drop { item_id: ITEM_STICK, count: 1 }]
            } else {
                Vec::new()
            }
        }
        COAL_ORE_BLOCK_ID => vec![Drop { item_id: ITEM_COAL, count: 1 }],
        IRON_ORE_BLOCK_ID => vec![Drop { item_id: ITEM_RAW_IRON, count: 1 }],
        _ => Vec::new(),
    }
}

/// 手持物品 → 可放置方块映射（放置用；未知物品返回 None，调用方走石头回退）。
pub fn block_for_item(item_id: i32) -> Option<u16> {
    match item_id {
        ITEM_STONE => Some(STONE_BLOCK_ID),
        ITEM_DIRT => Some(DIRT_BLOCK_ID),
        ITEM_SAND => Some(SAND_BLOCK_ID),
        ITEM_OAK_LOG => Some(OAK_LOG_BLOCK_ID),
        // 草方块没有对应物品（草掉泥土）；原木树叶等其余暂不支持。
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_drops_are_deterministic() {
        assert_eq!(
            drops_for_block(GRASS_BLOCK_ID, 369, 72, 381, 123),
            vec![Drop { item_id: ITEM_DIRT, count: 1 }]
        );
        assert_eq!(
            drops_for_block(DIRT_BLOCK_ID, 0, 64, 0, 999),
            vec![Drop { item_id: ITEM_DIRT, count: 1 }]
        );
        assert_eq!(
            drops_for_block(STONE_BLOCK_ID, 0, 60, 0, 1),
            vec![Drop { item_id: ITEM_COBBLESTONE, count: 1 }]
        );
        assert_eq!(
            drops_for_block(SAND_BLOCK_ID, 5, 63, 5, 1),
            vec![Drop { item_id: ITEM_SAND, count: 1 }]
        );
        assert_eq!(
            drops_for_block(OAK_LOG_BLOCK_ID, 5, 70, 5, 1),
            vec![Drop { item_id: ITEM_OAK_LOG, count: 1 }]
        );
        assert_eq!(
            drops_for_block(COAL_ORE_BLOCK_ID, 0, 40, 0, 1),
            vec![Drop { item_id: ITEM_COAL, count: 1 }]
        );
        assert_eq!(
            drops_for_block(IRON_ORE_BLOCK_ID, 0, 10, 0, 1),
            vec![Drop { item_id: ITEM_RAW_IRON, count: 1 }]
        );
        // 空气/水/未知无掉落。
        assert!(drops_for_block(crate::AIR_BLOCK_ID, 0, 64, 0, 1).is_empty());
        assert!(drops_for_block(crate::WATER_BLOCK_ID, 0, 62, 0, 1).is_empty());
        assert!(drops_for_block(9999, 0, 64, 0, 1).is_empty());
    }

    #[test]
    fn leaves_probability_is_stable_for_fixed_pos() {
        // 固定 pos 多次调用结果一致（无随机污染）。
        let a = drops_for_block(OAK_LEAVES_BLOCK_ID, 369, 75, 381, 123456789);
        let b = drops_for_block(OAK_LEAVES_BLOCK_ID, 369, 75, 381, 123456789);
        assert_eq!(a, b);
        for d in &a {
            assert!(d.item_id == ITEM_OAK_SAPLING || d.item_id == ITEM_STICK);
            assert_eq!(d.count, 1);
        }
        // 不同 seed 可能不同分支，但只可能是 sapling/stick/空。
        for seed in [1i64, 2, 42, 123456789] {
            let d = drops_for_block(OAK_LEAVES_BLOCK_ID, 100, 70, -200, seed);
            assert!(d.len() <= 1);
            if let Some(x) = d.first() {
                assert!(x.item_id == ITEM_OAK_SAPLING || x.item_id == ITEM_STICK);
            }
        }
        // 找一个 sapling 位点与一个空位点，证明分支确实存在。
        let mut saw_sapling = false;
        let mut saw_empty = false;
        for x in 0..64 {
            match drops_for_block(OAK_LEAVES_BLOCK_ID, x, 70, 0, 7).as_slice() {
                [Drop { item_id: ITEM_OAK_SAPLING, .. }] => saw_sapling = true,
                [] => saw_empty = true,
                _ => {}
            }
        }
        assert!(saw_sapling && saw_empty, "leaves must have both drop and empty branches");
    }

    #[test]
    fn block_for_item_mapping() {
        assert_eq!(block_for_item(ITEM_STONE), Some(STONE_BLOCK_ID));
        assert_eq!(block_for_item(ITEM_DIRT), Some(DIRT_BLOCK_ID));
        assert_eq!(block_for_item(ITEM_SAND), Some(SAND_BLOCK_ID));
        assert_eq!(block_for_item(ITEM_OAK_LOG), Some(OAK_LOG_BLOCK_ID));
        assert_eq!(block_for_item(ITEM_COBBLESTONE), None);
        assert_eq!(block_for_item(ITEM_STICK), None);
    }
}
