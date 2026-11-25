//! drops: 确定性掉落表（无随机种子污染），只兼容 26.3/777。
//!
//! 输入 blockstate ID（见 `mc_world` 常量），输出 `(item_id, count)`（`minecraft:item`
//! 注册表 ID，26.3 客户端 jar 实测，见 HANDOFF）：
//!
//! | 方块 | 掉落 |
//! |---|----|
//! | grass_block(9) | dirt(55)×1 + 1/8 wheat_seeds(1065)×1（world+pos hash，三叶草口径，无高草故借草方块） |
//! | dirt(10) | dirt(55)×1 |
//! | stone(1) | cobblestone(62)×1 |
//! | sand(121) | sand(88)×1 |
//! | oak_log(140) | oak_log(163)×1 |
//! | oak_leaves(294) | 1/8 oak_sapling(77)×1，否则 1/8 stick(1060)×1（world+pos hash） |
//! | oak_sapling(30) | oak_sapling(77)×1 |
//! | coal_ore(136) | coal(1010)×1 |
//! | iron_ore(134) | raw_iron(1017)×1 |
//! | wheat age<7 | wheat_seeds(1065)×1 |
//! | wheat age=7 | wheat(1066)×1 + wheat_seeds(1065)×1~2 |
//!
//! 限制（TODO）：无工具等级/精准采集检查（都按徒手掉落），无经验球。

use crate::{
    COAL_ORE_BLOCK_ID, DIRT_BLOCK_ID, GRASS_BLOCK_ID, IRON_ORE_BLOCK_ID, OAK_LEAVES_BLOCK_ID,
    OAK_LOG_BLOCK_ID, OAK_SAPLING_BLOCK_ID, SAND_BLOCK_ID, STONE_BLOCK_ID, WHEAT_MAX_AGE,
};

/// `minecraft:item` 注册表 ID，编译期从 `mc_registry::ITEMS`（由 `build.rs` 从
/// `data/777/items.json` 生成）查表，命名空间写法与 `mc_registry::item_id` 一致。
pub const ITEM_STONE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:stone") as i32;
pub const ITEM_DIRT: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:dirt") as i32;
pub const ITEM_COBBLESTONE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:cobblestone") as i32;
pub const ITEM_OAK_SAPLING: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:oak_sapling") as i32;
pub const ITEM_SAND: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:sand") as i32;
pub const ITEM_OAK_LOG: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:oak_log") as i32;
pub const ITEM_COAL: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:coal") as i32;
pub const ITEM_STICK: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:stick") as i32;
pub const ITEM_RAW_IRON: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:raw_iron") as i32;
/// `data/777/items.json` 的 `wheat_seeds.id=1065`（P22 种植用手持物品）。
pub const ITEM_WHEAT_SEEDS: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:wheat_seeds") as i32;
/// `data/777/items.json` 的 `wheat.id=1066`（成熟小麦掉落）。
pub const ITEM_WHEAT: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:wheat") as i32;

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
        // 草方块：恒掉泥土；另按树叶同口径 hash 1/8 掉种子（无高草，用草方块做唯一种子来源）。
        GRASS_BLOCK_ID => {
            let mut out = vec![Drop { item_id: ITEM_DIRT, count: 1 }];
            if hash_pos(seed, x, y, z) % 8 == 2 {
                out.push(Drop { item_id: ITEM_WHEAT_SEEDS, count: 1 });
            }
            out
        }
        DIRT_BLOCK_ID => vec![Drop { item_id: ITEM_DIRT, count: 1 }],
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
        // P23 树苗：挖掉种下的树苗原样返还（与小麦未熟口径一致；长成树/树叶 decay 留后续）。
        OAK_SAPLING_BLOCK_ID => vec![Drop { item_id: ITEM_OAK_SAPLING, count: 1 }],
        // P22 小麦：成熟（age=7）掉小麦×1 + 种子 1~2（hash 最低位决定，无随机状态）；
        // 未熟只掉种子×1。耕地/精准采集等差异见 HANDOFF §5。
        id if crate::wheat_age(id).is_some() => {
            let age = crate::wheat_age(id).unwrap_or(0);
            if age >= WHEAT_MAX_AGE {
                let extra = (hash_pos(seed, x, y, z) % 2) as i32;
                vec![
                    Drop { item_id: ITEM_WHEAT, count: 1 },
                    Drop { item_id: ITEM_WHEAT_SEEDS, count: 1 + extra },
                ]
            } else {
                vec![Drop { item_id: ITEM_WHEAT_SEEDS, count: 1 }]
            }
        }
        _ => Vec::new(),
    }
}

/// 手持物品 → 可放置方块映射（放置用；未知物品返回 None，调用方只 ack 不落子）。
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
    fn item_ids_come_from_registry_table() {
        // 防止 codegen 数据漂移：编译期常量必须仍等于 26.3 实测值，
        // 且与公开查询 API 的运行时结果一致。
        for (name, expected, id) in [
            ("minecraft:stone", 1, ITEM_STONE),
            ("minecraft:dirt", 55, ITEM_DIRT),
            ("minecraft:cobblestone", 62, ITEM_COBBLESTONE),
            ("minecraft:oak_sapling", 77, ITEM_OAK_SAPLING),
            ("minecraft:sand", 88, ITEM_SAND),
            ("minecraft:oak_log", 163, ITEM_OAK_LOG),
            ("minecraft:coal", 1010, ITEM_COAL),
            ("minecraft:stick", 1060, ITEM_STICK),
            ("minecraft:raw_iron", 1017, ITEM_RAW_IRON),
            ("minecraft:wheat_seeds", 1065, ITEM_WHEAT_SEEDS),
            ("minecraft:wheat", 1066, ITEM_WHEAT),
        ] {
            assert_eq!(id as u32, expected, "wrong item ID for {name}");
            assert_eq!(Some(id as u32), mc_registry::item_id(name));
        }
    }

    #[test]
    fn basic_drops_are_deterministic() {
        // 草方块恒掉泥土；找一个出种子的位点和一个不出种子的位点（hash%8==2 出种子）。
        let mut saw_seeds = false;
        let mut saw_plain = false;
        for x in 0..64 {
            let d = drops_for_block(GRASS_BLOCK_ID, x, 72, 381, 123);
            assert_eq!(d.first(), Some(&Drop { item_id: ITEM_DIRT, count: 1 }));
            assert!(d.len() <= 2);
            if d.len() == 2 {
                assert_eq!(d[1], Drop { item_id: ITEM_WHEAT_SEEDS, count: 1 });
                saw_seeds = true;
            } else {
                saw_plain = true;
            }
        }
        assert!(saw_seeds && saw_plain, "grass seed branch must exist and be gated");
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
    fn wheat_drops_ripe_wheat_plus_seeds_unripe_only_seeds() {
        use crate::{wheat_stage_id, WHEAT_CROP_BASE_ID};
        // 未熟（age 0..6）：只掉 1 种子，与位置/seed 无关。
        for age in 0..7u8 {
            for (x, seed) in [(0, 1i64), (369, 123456789), (-100, 42)] {
                assert_eq!(
                    drops_for_block(wheat_stage_id(age), x, 70, 381, seed),
                    vec![Drop { item_id: ITEM_WHEAT_SEEDS, count: 1 }],
                    "unripe age={age} must drop exactly 1 seed"
                );
            }
        }
        // 成熟（age=7）：恒掉小麦×1 + 种子 1~2（确定性 hash，无随机污染）。
        let ripe = wheat_stage_id(7);
        assert_eq!(ripe, WHEAT_CROP_BASE_ID + 7);
        let a = drops_for_block(ripe, 369, 73, 381, 123456789);
        let b = drops_for_block(ripe, 369, 73, 381, 123456789);
        assert_eq!(a, b, "same pos+seed must drop identically");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0], Drop { item_id: ITEM_WHEAT, count: 1 });
        assert_eq!(a[1].item_id, ITEM_WHEAT_SEEDS);
        assert!((1..=2).contains(&a[1].count), "ripe seeds count must be 1..=2, got {:?}", a);
        // 两个分支都存在（不同 pos 覆盖 1 与 2）。
        let mut counts = std::collections::HashSet::new();
        for x in 0..32 {
            counts.insert(drops_for_block(ripe, x, 70, 0, 7)[1].count);
        }
        assert!(counts.contains(&1) && counts.contains(&2), "ripe must have both 1-seed and 2-seed branches, got {counts:?}");
        // 种子不是可放置方块（种植走 P22 专属路径，不走 block_for_item）。
        assert_eq!(block_for_item(ITEM_WHEAT_SEEDS), None);
        assert_eq!(block_for_item(ITEM_WHEAT), None);
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
