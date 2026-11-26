//! drops: 全部作物/新树/瓜果的掉落规则（P33 原样继承 + P37 扩展）。
//!
//! `mc_world::drops::drops_for_block` 在通用表之前先匹配旧方块，本模块只负责
//! 作物专属方块：命中返回 `Some(Vec<Drop>)`，非作物返回 `None`（由调用方继续
//! 走空掉落）。所有概率分支都走 `crate::drops::hash_pos` 的独立盐，确定性且
//! 与位置绑定，无随机状态；掉落数量口径见各臂注释。

use crate::drops::{hash_pos, Drop, ITEM_STICK};
use super::{
    age_crop_of, cocoa_of, tree_spec_for_leaves, tree_spec_for_log, tree_spec_for_sapling,
    vertical_spec_of, AgeCropSpec, CropKind, COCOA,
};

/// P33 老四种作物：小麦/胡萝卜/马铃薯/甜菜（旧口径逐字节保持）。
fn old_crop_drops(
    crop: &crate::crops::CropSpec,
    age: u8,
    x: i32,
    y: i32,
    z: i32,
    seed: i64,
) -> Vec<Drop> {
    use crate::drops::{
        ITEM_BEETROOT, ITEM_BEETROOT_SEEDS, ITEM_CARROT, ITEM_POISONOUS_POTATO, ITEM_POTATO,
        ITEM_WHEAT, ITEM_WHEAT_SEEDS,
    };
    let ripe = age >= crop.max_age;
    match crop.kind {
        CropKind::Wheat => {
            if ripe {
                let extra = (hash_pos(seed, x, y, z) % 2) as i32;
                vec![
                    Drop { item_id: ITEM_WHEAT, count: 1 },
                    Drop { item_id: ITEM_WHEAT_SEEDS, count: 1 + extra },
                ]
            } else {
                vec![Drop { item_id: ITEM_WHEAT_SEEDS, count: 1 }]
            }
        }
        CropKind::Carrot => {
            let mut count = 1 + (hash_pos(seed, x, y, z) % 2) as i32;
            if hash_pos(seed ^ 0x63617272, x, y, z) % 4 == 0 {
                count += 1;
            }
            vec![Drop { item_id: ITEM_CARROT, count: if ripe { count } else { 1 } }]
        }
        CropKind::Potato => {
            let mut out = vec![Drop {
                item_id: ITEM_POTATO,
                count: if ripe { 1 + (hash_pos(seed, x, y, z) % 2) as i32 } else { 1 },
            }];
            if ripe && hash_pos(seed ^ 0x706f6973, x, y, z) % 100 == 0 {
                out.push(Drop { item_id: ITEM_POISONOUS_POTATO, count: 1 });
            }
            out
        }
        CropKind::Beetroot => {
            if ripe {
                vec![
                    Drop { item_id: ITEM_BEETROOT, count: 1 },
                    Drop {
                        item_id: ITEM_BEETROOT_SEEDS,
                        count: 1 + (hash_pos(seed, x, y, z) % 2) as i32,
                    },
                ]
            } else {
                vec![Drop { item_id: ITEM_BEETROOT_SEEDS, count: 1 }]
            }
        }
    }
}

/// P37 单属性 age 作物掉落。
fn age_crop_drops(
    crop: &AgeCropSpec,
    age: u8,
    x: i32,
    y: i32,
    z: i32,
    seed: i64,
) -> Vec<Drop> {
    use crate::drops::{
        ITEM_MELON_SEEDS, ITEM_NETHER_WART, ITEM_PITCHER_PLANT, ITEM_PUMPKIN_SEEDS,
        ITEM_SWEET_BERRIES, ITEM_TORCHFLOWER, ITEM_TORCHFLOWER_SEEDS,
    };
    let ripe = age >= crop.max_age;
    let h = hash_pos(seed ^ (crop.name.len() as i64), x, y, z);
    match crop.name {
        "nether_wart" => vec![Drop { item_id: ITEM_NETHER_WART, count: if ripe { 1 + (h % 3) as i32 } else { 1 } }],
        "melon_stem" => vec![Drop { item_id: ITEM_MELON_SEEDS, count: 1 }],
        "pumpkin_stem" => vec![Drop { item_id: ITEM_PUMPKIN_SEEDS, count: 1 }],
        // 未熟也返 1 颗（原版幼丛无掉落；这里避免玩家损失种子，见 HANDOFF 取舍）。
        "sweet_berry_bush" => vec![Drop {
            item_id: ITEM_SWEET_BERRIES,
            count: if age >= 2 { 2 + (h % 2) as i32 } else { 1 },
        }],
        "torchflower_crop" => {
            let mut out = vec![Drop { item_id: ITEM_TORCHFLOWER_SEEDS, count: 1 }];
            if ripe {
                out.push(Drop { item_id: ITEM_TORCHFLOWER, count: 1 });
            }
            out
        }
        "pitcher_crop" => {
            let mut out = vec![Drop { item_id: crop.seed_item, count: 1 }];
            if ripe {
                out.push(Drop { item_id: ITEM_PITCHER_PLANT, count: 1 });
            }
            out
        }
        _ => Vec::new(),
    }
}

/// 任意作物/新方块 → 掉落；非作物返回 `None`（调用方按空处理）。
pub fn crop_block_drops(id: u16, x: i32, y: i32, z: i32, seed: i64) -> Option<Vec<Drop>> {
    // P33 老表优先（与旧 drops_for_block 的 match 臂顺序等价）。
    if let Some((crop, age)) = crate::crop_age_of(id) {
        return Some(old_crop_drops(crop, age, x, y, z, seed));
    }
    if let Some((crop, age)) = age_crop_of(id) {
        return Some(age_crop_drops(crop, age, x, y, z, seed));
    }
    if let Some((_, age, _)) = cocoa_of(id) {
        let beans = crate::drops::ITEM_COCOA_BEANS;
        let count = if age >= COCOA.max_age { 1 + (hash_pos(seed, x, y, z) % 3) as i32 } else { 1 };
        return Some(vec![Drop { item_id: beans, count }]);
    }
    if let Some(spec) = vertical_spec_of(id) {
        return Some(vec![Drop { item_id: spec.item_id, count: 1 }]);
    }
    if let Some(spec) = tree_spec_for_sapling(id) {
        return Some(vec![Drop { item_id: spec.seed_item, count: 1 }]);
    }
    if let Some(spec) = tree_spec_for_log(id) {
        return Some(vec![Drop { item_id: spec.log_item_id, count: 1 }]);
    }
    if let Some(spec) = tree_spec_for_leaves(id) {
        // 与 P26 橡树叶同口径：1/8 掉树苗、1/8 掉木棍、其余为空。
        let h = hash_pos(seed, x, y, z) % 8;
        return Some(match h {
            0 => vec![Drop { item_id: spec.seed_item, count: 1 }],
            1 => vec![Drop { item_id: ITEM_STICK, count: 1 }],
            _ => Vec::new(),
        });
    }
    // 整块瓜/南瓜：按 `AgeCropSpec::FruitSpec` 的真实物品与数量区间。
    for spec in &super::AGE_CROPS {
        if let Some(fruit) = spec.fruit {
            if id == fruit.block_id {
                let count = fruit.min_count
                    + (hash_pos(seed ^ 0x66727569, x, y, z) % (fruit.max_count - fruit.min_count + 1) as u64)
                        as i32;
                return Some(vec![Drop { item_id: fruit.item_id, count }]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crops::{
        age_stage_id, vertical_state, AGE_CROPS, CACTUS, MELON_STEM, NETHER_WART, SUGAR_CANE,
        SWEET_BERRY_BUSH, TORCHFLOWER_CROP,
    };
    use crate::drops::{ITEM_MELON_SLICE, ITEM_NETHER_WART, ITEM_SWEET_BERRIES};

    #[test]
    fn p37_age_crop_drops_are_deterministic_and_ripe_aware() {
        let seed = 123456789i64;
        for spec in &AGE_CROPS {
            for age in 0..=spec.max_age {
                let a = crop_block_drops(age_stage_id(spec, age), 3, 70, 4, seed).unwrap();
                let b = crop_block_drops(age_stage_id(spec, age), 3, 70, 4, seed).unwrap();
                assert_eq!(a, b, "drops must be deterministic: {}", spec.name);
                assert!(!a.is_empty() || spec.name == "sweet_berry_bush");
            }
        }
        // 地狱疣：未熟恒 1，成熟 1~3。
        assert_eq!(
            crop_block_drops(NETHER_WART.base_id, 1, 70, 1, seed),
            Some(vec![Drop { item_id: ITEM_NETHER_WART, count: 1 }])
        );
        let mut counts = std::collections::HashSet::new();
        for x in 0..64 {
            let d = crop_block_drops(age_stage_id(&NETHER_WART, 3), x, 70, 1, seed).unwrap();
            assert_eq!(d[0].item_id, ITEM_NETHER_WART);
            counts.insert(d[0].count);
            assert!((1..=3).contains(&d[0].count));
        }
        assert!(counts.len() >= 2, "ripe nether wart must vary: {counts:?}");
        // 甜浆果：age=1 一颗、age=3 两颗或三颗。
        assert_eq!(
            crop_block_drops(age_stage_id(&SWEET_BERRY_BUSH, 1), 2, 70, 2, seed),
            Some(vec![Drop { item_id: ITEM_SWEET_BERRIES, count: 1 }])
        );
        let mut ripe = std::collections::HashSet::new();
        for x in 0..64 {
            let d = crop_block_drops(age_stage_id(&SWEET_BERRY_BUSH, 3), x, 70, 2, seed).unwrap();
            ripe.insert(d[0].count);
        }
        assert!(ripe.contains(&2) && ripe.contains(&3), "{ripe:?}");
        // 火把花成熟额外掉花。
        let ripe_tf = crop_block_drops(age_stage_id(&TORCHFLOWER_CROP, 1), 5, 70, 5, seed).unwrap();
        assert_eq!(ripe_tf.len(), 2);
        assert_eq!(ripe_tf[1].item_id, crate::drops::ITEM_TORCHFLOWER);
    }

    #[test]
    fn p37_vertical_side_and_fruit_drops() {
        let seed = 7i64;
        for (id, item) in [
            (vertical_state(&SUGAR_CANE, 0), crate::drops::ITEM_SUGAR_CANE),
            (vertical_state(&CACTUS, 2), crate::drops::ITEM_CACTUS),
        ] {
            assert_eq!(crop_block_drops(id, 0, 70, 0, seed), Some(vec![Drop { item_id: item, count: 1 }]));
        }
        // 可可成熟 1~3，未熟 1。
        let cocoa = super::super::cocoa_state;
        assert_eq!(
            crop_block_drops(cocoa(0, 0), 0, 70, 0, seed).unwrap()[0].count,
            1
        );
        let mut beans = std::collections::HashSet::new();
        for x in 0..64 {
            beans.insert(crop_block_drops(cocoa(2, 1), x, 70, 0, seed).unwrap()[0].count);
        }
        assert!(beans.contains(&1) && beans.contains(&3), "{beans:?}");
        // 瓜块掉 3~7 瓜片，南瓜块掉 1 南瓜。
        let fruit = MELON_STEM.fruit.unwrap();
        let mut melons = std::collections::HashSet::new();
        for x in 0..64 {
            let d = crop_block_drops(fruit.block_id, x, 70, 0, seed).unwrap();
            assert_eq!(d[0].item_id, ITEM_MELON_SLICE);
            melons.insert(d[0].count);
        }
        assert!(melons.contains(&3) && melons.contains(&7), "{melons:?}");
        let pumpkin = super::super::PUMPKIN_STEM.fruit.unwrap();
        assert_eq!(
            crop_block_drops(pumpkin.block_id, 0, 70, 0, seed),
            Some(vec![Drop { item_id: crate::drops::ITEM_PUMPKIN, count: 1 }])
        );
    }

    #[test]
    fn p37_new_tree_drops_follow_oak_table_shape() {
        use crate::crops::BIRCH;
        let seed = 42i64;
        assert_eq!(
            crop_block_drops(BIRCH.log_id, 0, 70, 0, seed),
            Some(vec![Drop { item_id: BIRCH.log_item_id, count: 1 }])
        );
        assert_eq!(
            crop_block_drops(BIRCH.sapling_id, 0, 70, 0, seed),
            Some(vec![Drop { item_id: BIRCH.seed_item, count: 1 }])
        );
        let mut saw_sapling = false;
        let mut saw_stick = false;
        let mut saw_empty = false;
        for x in 0..64 {
            match crop_block_drops(BIRCH.leaves_id, x, 70, 0, seed).unwrap().as_slice() {
                [Drop { item_id, .. }] if *item_id == BIRCH.seed_item => saw_sapling = true,
                [Drop { item_id, .. }] if *item_id == ITEM_STICK => saw_stick = true,
                [] => saw_empty = true,
                other => panic!("unexpected leaf drops {other:?}"),
            }
        }
        assert!(saw_sapling && saw_stick && saw_empty);
    }
}
