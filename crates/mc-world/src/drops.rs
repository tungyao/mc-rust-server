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
//! | cobblestone(14) | cobblestone(62)×1 |
//! | sand(121) | sand(88)×1 |
//! | oak_log(140) | oak_log(163)×1 |
//! | oak_leaves(294) | 1/8 oak_sapling(77)×1，否则 1/8 stick(1060)×1（world+pos hash） |
//! | oak_sapling(30) | oak_sapling(77)×1 |
//! | coal_ore(136) | coal(1010)×1 |
//! | iron_ore(134) | raw_iron(1017)×1 |
//! | wheat age<7 | wheat_seeds(1065)×1 |
//! | wheat age=7 | wheat(1066)×1 + wheat_seeds(1065)×1~2 |
//! | farmland(6875, P27) | dirt(55)×1 |
//! | oak_leaves decay(P27, 非挖掘) | 同上 + 1/200 apple(1007)×1 |
//!
//! 限制（TODO）：无工具等级/精准采集检查（都按徒手掉落），无经验球。

use crate::{
    COAL_ORE_BLOCK_ID, COBBLESTONE_BLOCK_ID, DIRT_BLOCK_ID, FARMLAND_BLOCK_ID, GRASS_BLOCK_ID,
    IRON_ORE_BLOCK_ID,
    OAK_LEAVES_BLOCK_ID, OAK_LOG_BLOCK_ID, OAK_SAPLING_BLOCK_ID, SAND_BLOCK_ID, STONE_BLOCK_ID,
    WHEAT_MAX_AGE,
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
/// P27 骨粉：`data/777/items.json` 的 `bone_meal.id=1215`（手持对小麦/树苗右键催熟，
/// 获得途径见 HANDOFF §5：无合成，靠管理面板 `/api/give` 领取）。
pub const ITEM_BONE_MEAL: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:bone_meal") as i32;
/// P27 苹果：`data/777/items.json` 的 `apple.id=1007`（仅树叶 decay 小概率掉落；
/// 无饥饿/进食系统，吃苹果不做，见 HANDOFF §5）。
pub const ITEM_APPLE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:apple") as i32;
/// P27 锄头全系（`data/777/items.json`，获得途径同骨粉：管理面板领取，无合成/掉落；
/// 手持任意一种对草/泥土右键均可耕地，无耐久消耗，见 HANDOFF §5）。
pub const ITEM_WOODEN_HOE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:wooden_hoe") as i32;
pub const ITEM_STONE_HOE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:stone_hoe") as i32;
pub const ITEM_COPPER_HOE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:copper_hoe") as i32;
pub const ITEM_GOLDEN_HOE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:golden_hoe") as i32;
pub const ITEM_IRON_HOE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:iron_hoe") as i32;
pub const ITEM_DIAMOND_HOE: i32 = crate::registry_id(mc_registry::ITEMS, "minecraft:diamond_hoe") as i32;
pub const ITEM_NETHERITE_HOE: i32 =
    crate::registry_id(mc_registry::ITEMS, "minecraft:netherite_hoe") as i32;

/// 是否锄头（7 种材质任一；耕地判定用，不消耗）。
pub fn is_hoe_item(item_id: i32) -> bool {
    matches!(
        item_id,
        ITEM_WOODEN_HOE
            | ITEM_STONE_HOE
            | ITEM_COPPER_HOE
            | ITEM_GOLDEN_HOE
            | ITEM_IRON_HOE
            | ITEM_DIAMOND_HOE
            | ITEM_NETHERITE_HOE
    )
}

/// P27 骨粉一次推进的阶段数：1~2（确定性 hash，无随机状态；调用方负责钳制到成熟）。
pub fn bonemeal_advance(seed: i64, x: i32, y: i32, z: i32) -> u8 {
    (hash_pos(seed, x, y, z) % 2) as u8 + 1
}

/// P27 树叶 decay 掉落：复用徒手挖掘口径（树苗/木棍）+ 小概率苹果。
/// 苹果概率 1/200（与原版橡树树叶一致量级；独立 hash，不扰动原有 sapling/stick 分支）。
/// 挖掘掉落（`drops_for_block`）不受影响：徒手挖树叶不掉苹果（旧单测锁定）。
pub fn leaf_decay_drops(x: i32, y: i32, z: i32, seed: i64) -> Vec<Drop> {
    let mut out = drops_for_block(OAK_LEAVES_BLOCK_ID, x, y, z, seed);
    if hash_pos(seed ^ 0x6c656166, x, y, z) % 200 == 0 {
        out.push(Drop { item_id: ITEM_APPLE, count: 1 });
    }
    out
}

/// P31 挖矿经验球掉落：按确定性表产出经验值（每元素一个经验球的价值）。
///
/// - 煤矿石：0~2 点（`hash_pos` 的独立盐，不扰动物品掉落分支；与原版「煤矿 0–2」一致）；
/// - 铁矿石：1 点（任务要求矿物给经验；原版挖铁矿不给经验、熔炼才给，见 HANDOFF §5 取舍）；
/// - 其它方块：无经验球。
///
/// 经验值 ≤11 时直接产出一个同值球（原版小经验球允许 1–11）；更大值按原版基础面额
/// 2477/1237/617/307/149/73/37/17/7/3/1 贪心拆分（当前表用不到，留作扩展）。
pub fn xp_orbs_for_block(block_id: u16, x: i32, y: i32, z: i32, seed: i64) -> Vec<i32> {
    let total = match block_id {
        COAL_ORE_BLOCK_ID => (hash_pos(seed ^ 0x5850, x, y, z) % 3) as i32,
        IRON_ORE_BLOCK_ID => 1,
        _ => 0,
    };
    if total <= 0 {
        return Vec::new();
    }
    if total <= 11 {
        return vec![total];
    }
    let mut out = Vec::new();
    let mut left = total;
    for base in [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3, 1] {
        while left >= base {
            out.push(base);
            left -= base;
        }
    }
    out
}

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
        // 放置的圆石（14）挖掉返还圆石（原版口径；石头挖掉则掉圆石，见上）。
        COBBLESTONE_BLOCK_ID => vec![Drop { item_id: ITEM_COBBLESTONE, count: 1 }],
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
        // P27 耕地：挖掉落泥土×1（与原版一致；耕地本身不可由管理面板直接放置获得，
        // 靠锄头耕出，见 HANDOFF §5）。
        FARMLAND_BLOCK_ID => vec![Drop { item_id: ITEM_DIRT, count: 1 }],
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
        ITEM_COBBLESTONE => Some(COBBLESTONE_BLOCK_ID),
        ITEM_DIRT => Some(DIRT_BLOCK_ID),
        ITEM_SAND => Some(SAND_BLOCK_ID),
        ITEM_OAK_LOG => Some(OAK_LOG_BLOCK_ID),
        // 草方块没有对应物品（草掉泥土）；原木树叶等其余暂不支持；
        // 小麦/煤/铁/木棍在原版中不可放置，保持 None（只 ack）。
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
        // 放置的圆石挖掉返还圆石（不断链）。
        assert_eq!(
            drops_for_block(COBBLESTONE_BLOCK_ID, 5, 60, 5, 1),
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
        assert_eq!(block_for_item(ITEM_COBBLESTONE), Some(COBBLESTONE_BLOCK_ID));
        assert_eq!(block_for_item(ITEM_STICK), None);
    }

    #[test]
    fn p27_tool_and_food_ids_come_from_registry() {
        // P27 新增物品 ID 必须仍等于 26.3 实测值且与 codegen 查询一致。
        for (name, expected, id) in [
            ("minecraft:bone_meal", 1215, ITEM_BONE_MEAL),
            ("minecraft:apple", 1007, ITEM_APPLE),
            ("minecraft:wooden_hoe", 1029, ITEM_WOODEN_HOE),
            ("minecraft:stone_hoe", 1039, ITEM_STONE_HOE),
            ("minecraft:copper_hoe", 1034, ITEM_COPPER_HOE),
            ("minecraft:golden_hoe", 1044, ITEM_GOLDEN_HOE),
            ("minecraft:iron_hoe", 1049, ITEM_IRON_HOE),
            ("minecraft:diamond_hoe", 1054, ITEM_DIAMOND_HOE),
            ("minecraft:netherite_hoe", 1059, ITEM_NETHERITE_HOE),
        ] {
            assert_eq!(id as u32, expected, "wrong item ID for {name}");
            assert_eq!(Some(id as u32), mc_registry::item_id(name));
        }
        // 7 种锄头全认；骨粉/苹果/种子不是锄头。
        for hoe in [
            ITEM_WOODEN_HOE,
            ITEM_STONE_HOE,
            ITEM_COPPER_HOE,
            ITEM_GOLDEN_HOE,
            ITEM_IRON_HOE,
            ITEM_DIAMOND_HOE,
            ITEM_NETHERITE_HOE,
        ] {
            assert!(is_hoe_item(hoe), "hoe {hoe} must be recognised");
            assert_eq!(block_for_item(hoe), None, "hoe is not placeable");
        }
        assert!(!is_hoe_item(ITEM_BONE_MEAL));
        assert!(!is_hoe_item(ITEM_APPLE));
        assert!(!is_hoe_item(ITEM_WHEAT_SEEDS));
        assert!(!is_hoe_item(ITEM_STICK));
        assert_eq!(block_for_item(ITEM_BONE_MEAL), None);
        assert_eq!(block_for_item(ITEM_APPLE), None);
    }

    #[test]
    fn p27_farmland_digs_to_dirt() {
        assert_eq!(
            drops_for_block(crate::FARMLAND_BLOCK_ID, 0, 70, 0, 7),
            vec![Drop { item_id: ITEM_DIRT, count: 1 }]
        );
    }

    #[test]
    fn p27_bonemeal_advance_is_one_or_two() {
        // 确定性：同位点同结果；两个分支都存在。
        let mut seen = std::collections::HashSet::new();
        for x in 0..64 {
            let n = bonemeal_advance(123456789, x, 70, 381);
            assert!((1..=2).contains(&n), "bonemeal must advance 1~2 stages, got {n}");
            seen.insert(n);
            assert_eq!(bonemeal_advance(123456789, x, 70, 381), n, "must be deterministic");
        }
        assert!(seen.contains(&1) && seen.contains(&2), "both branches must exist: {seen:?}");
    }

    #[test]
    fn p27_leaf_decay_drops_are_leaves_table_plus_rare_apple() {
        // decay 基础口径 = 徒手挖掘口径（树苗/木棍/空），苹果是额外小概率。
        let seed = 123456789i64;
        let mut saw_apple = false;
        let mut saw_plain = false;
        for x in 0..4000 {
            let base = drops_for_block(crate::OAK_LEAVES_BLOCK_ID, x, 70, 0, seed);
            let decay = leaf_decay_drops(x, 70, 0, seed);
            assert!(decay.len() >= base.len(), "decay must not lose the base drops");
            assert_eq!(&decay[..base.len()], &base[..], "decay prefix must equal the dig table");
            for d in &decay[base.len()..] {
                assert_eq!(*d, Drop { item_id: ITEM_APPLE, count: 1 });
            }
            assert!(decay.len() - base.len() <= 1, "at most one apple");
            if decay.len() > base.len() {
                saw_apple = true;
            } else {
                saw_plain = true;
            }
        }
        assert!(saw_apple, "apple arm must trigger somewhere in 4000 tries at 1/200");
        assert!(saw_plain, "most decays drop no apple");
        // 徒手挖掘不受影响：同一批位点里挖树叶永不掉苹果（旧行为锁定）。
        for x in 0..64 {
            for d in drops_for_block(crate::OAK_LEAVES_BLOCK_ID, x, 70, 0, seed) {
                assert_ne!(d.item_id, ITEM_APPLE, "hand dig must never drop apples");
            }
        }
    }

    #[test]
    fn p31_ore_xp_table_is_deterministic_and_bounded() {
        // 煤矿 0~2（确定性，与物品掉落互不干扰），铁矿恒 1，其它方块无经验。
        let mut saw_zero = false;
        let mut saw_two = false;
        for x in 0..64 {
            let orb = xp_orbs_for_block(COAL_ORE_BLOCK_ID, x, 70, 381, 123456789);
            assert!(orb.len() <= 1);
            if orb.is_empty() {
                saw_zero = true;
            } else {
                assert!((1..=2).contains(&orb[0]), "coal orb must be 1~2, got {orb:?}");
                if orb[0] == 2 {
                    saw_two = true;
                }
            }
            assert_eq!(orb, xp_orbs_for_block(COAL_ORE_BLOCK_ID, x, 70, 381, 123456789));
        }
        assert!(saw_zero && saw_two, "coal XP must cover both 0 and 2 branches");
        assert_eq!(xp_orbs_for_block(IRON_ORE_BLOCK_ID, 0, 10, 0, 1), vec![1]);
        assert_eq!(xp_orbs_for_block(IRON_ORE_BLOCK_ID, 369, 12, 381, 42), vec![1]);
        for id in [STONE_BLOCK_ID, DIRT_BLOCK_ID, OAK_LOG_BLOCK_ID, crate::AIR_BLOCK_ID] {
            assert!(xp_orbs_for_block(id, 0, 70, 0, 1).is_empty());
        }
    }
}
