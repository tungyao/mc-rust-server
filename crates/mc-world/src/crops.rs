//! crops: P33 通用作物表（小麦/胡萝卜/马铃薯/甜菜）。
//!
//! 一条作物 = 手持种子 item_id + 方块 base（age=0 的 blockstate）+ 最大 age
//! + 每阶段 tick + 成熟/未熟掉落口径。所有 ID 都从 `mc_registry`（由
//! `data/777/*.json` 生成）在编译期查表得到，不在实现里写数字；单测才断言
//! 已知值防数据漂移。
//!
//! 数据依据：`data/777/blocks.json` 的 `default_state_id` 与
//! `data/777/full/blocks.json` 的 `states`（按 age 升序、id 连续）：
//! wheat 6867..=6874（8 态）、carrots 12402..=12409（8 态）、
//! potatoes 12410..=12417（8 态）、beetroots 16792..=16795（4 态）。
//! 掉落物品 ID 见 `data/777/items.json`，经 `mc_registry::ITEMS` 查表。
//!
//! nether_wart（11181..=11184，item 1269）本批**不做**：原版只种在灵魂沙上，
//! 现有种植/耕地/连带路径的土壤集合 `is_crop_soil` 只有草/泥土/耕地，
//! 单独扩 soul_sand 会改变小麦/树苗/锄头的现有行为（用户要求这些保持不动），
//! 留待后续单独处理。

use crate::drops::{ITEM_BEETROOT_SEEDS, ITEM_CARROT, ITEM_POTATO, ITEM_WHEAT_SEEDS};

/// 掉落口径（成熟/未熟两臂，见 `crate::drops::drops_for_block`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropKind {
    Wheat,
    Carrot,
    Potato,
    Beetroot,
}

/// 一种作物的静态规格（`CROPS` 的元素，全部 `Copy`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropSpec {
    pub name: &'static str,
    pub kind: CropKind,
    /// 手持种植用的种子物品 ID（胡萝卜/马铃薯本身就是“种子”）。
    pub seed_item: i32,
    /// age=0 的 blockstate ID；age N = base + N（数据顺序即偏移）。
    pub base_id: u16,
    pub max_age: u8,
    /// 生产环境每阶段的全局 tick 数（50ms/拍）；测试经 `DropState::set_stage_ticks` 整体调小。
    pub ticks_per_stage: u64,
}

/// 小麦：保留 P22/P27 的全部口径（7 阶段、1200 拍/阶段、成熟掉小麦+种子）。
pub const WHEAT: CropSpec = CropSpec {
    name: "wheat",
    kind: CropKind::Wheat,
    seed_item: ITEM_WHEAT_SEEDS,
    base_id: crate::WHEAT_CROP_BASE_ID,
    max_age: crate::WHEAT_MAX_AGE,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
};

/// 胡萝卜：手持 carrot（数据 `full/blocks.json` 的 carrots `item_id` 同为 1378）。
pub const CARROT: CropSpec = CropSpec {
    name: "carrots",
    kind: CropKind::Carrot,
    seed_item: ITEM_CARROT,
    base_id: crate::registry_id(mc_registry::BLOCKS, "minecraft:carrots") as u16,
    max_age: 7,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
};

/// 马铃薯：手持 potato（item 1379）。
pub const POTATO: CropSpec = CropSpec {
    name: "potatoes",
    kind: CropKind::Potato,
    seed_item: ITEM_POTATO,
    base_id: crate::registry_id(mc_registry::BLOCKS, "minecraft:potatoes") as u16,
    max_age: 7,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
};

/// 甜菜：手持 beetroot_seeds（item 1439；`full/blocks.json` 的 beetroots `item_id`
/// 也是 1439，即甜菜作物对应物品本身就是种子）。
pub const BEETROOT: CropSpec = CropSpec {
    name: "beetroots",
    kind: CropKind::Beetroot,
    seed_item: ITEM_BEETROOT_SEEDS,
    base_id: crate::registry_id(mc_registry::BLOCKS, "minecraft:beetroots") as u16,
    max_age: 3,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
};

/// 全部可种作物（顺序固定：小麦、胡萝卜、马铃薯、甜菜）。
/// blockstate 区间互不重叠，故 `crop_age_of` 的线性查找唯一命中。
pub const CROPS: [CropSpec; 4] = [WHEAT, CARROT, POTATO, BEETROOT];

/// 作物 age → blockstate ID（超龄钳制到成熟）。
pub fn crop_stage_id(crop: &CropSpec, age: u8) -> u16 {
    crop.base_id + age.min(crop.max_age) as u16
}

/// blockstate ID → 该作物的 age（非该作物区间返回 None）。
pub fn crop_age(crop: &CropSpec, id: u16) -> Option<u8> {
    let age = id.wrapping_sub(crop.base_id);
    if age <= crop.max_age as u16 {
        Some(age as u8)
    } else {
        None
    }
}

/// 任意作物方块 → (规格, age)；非作物返回 None。
pub fn crop_age_of(id: u16) -> Option<(&'static CropSpec, u8)> {
    for crop in &CROPS {
        if let Some(age) = crop_age(crop, id) {
            return Some((crop, age));
        }
    }
    None
}

/// 种子物品 → 作物规格（种植路由用；树苗不走此表，仍由 item 77 专属路由）。
pub fn crop_for_seed(item_id: i32) -> Option<&'static CropSpec> {
    CROPS.iter().find(|crop| crop.seed_item == item_id)
}

/// 种子物品 → 种下时的 age=0 方块 ID。
pub fn crop_base_for_seed(item_id: i32) -> Option<u16> {
    crop_for_seed(item_id).map(|crop| crop.base_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drops::{ITEM_BEETROOT, ITEM_POISONOUS_POTATO, ITEM_WHEAT};

    #[test]
    fn p33_crop_ids_and_seed_items_come_from_registry() {
        // 防 codegen 漂移：编译期常量仍等于 data/777 实测值，且与查询表一致。
        for (block, expected, name) in [
            (WHEAT.base_id, 6867, "minecraft:wheat"),
            (CARROT.base_id, 12402, "minecraft:carrots"),
            (POTATO.base_id, 12410, "minecraft:potatoes"),
            (BEETROOT.base_id, 16792, "minecraft:beetroots"),
        ] {
            assert_eq!(block as u32, expected, "wrong blockstate ID for {name}");
            assert_eq!(Some(block as u32), mc_registry::block_id(name));
        }
        for (item, expected, name) in [
            (ITEM_WHEAT_SEEDS, 1065, "minecraft:wheat_seeds"),
            (ITEM_WHEAT, 1066, "minecraft:wheat"),
            (ITEM_CARROT, 1378, "minecraft:carrot"),
            (ITEM_POTATO, 1379, "minecraft:potato"),
            (ITEM_POISONOUS_POTATO, 1381, "minecraft:poisonous_potato"),
            (ITEM_BEETROOT, 1438, "minecraft:beetroot"),
            (ITEM_BEETROOT_SEEDS, 1439, "minecraft:beetroot_seeds"),
        ] {
            assert_eq!(item as u32, expected, "wrong item ID for {name}");
            assert_eq!(Some(item as u32), mc_registry::item_id(name));
        }
        assert_eq!(WHEAT.max_age, crate::WHEAT_MAX_AGE);
        assert_eq!(WHEAT.ticks_per_stage, crate::WHEAT_TICKS_PER_STAGE);
        // 本批四种作物共用固定 1200 拍/阶段，测试钩子 set_stage_ticks 对全体生效。
        for crop in &CROPS {
            assert_eq!(crop.ticks_per_stage, crate::WHEAT_TICKS_PER_STAGE, "{}", crop.name);
        }
    }

    #[test]
    fn p33_crop_stages_are_base_plus_age_and_clamped() {
        for crop in &CROPS {
            for age in 0..=crop.max_age {
                let id = crop_stage_id(crop, age);
                assert_eq!(id, crop.base_id + age as u16, "{} age={age}", crop.name);
                assert_eq!(crop_age_of(id), Some((crop, age)), "{} age={age}", crop.name);
            }
            assert_eq!(crop_stage_id(crop, 99), crop_stage_id(crop, crop.max_age));
            // 紧邻区间的两侧不能串到别的作物（carrot 末尾 12409 / potato 起点 12410）。
            assert_eq!(crop_age(crop, crop.base_id.wrapping_sub(1)), None);
            assert_eq!(crop_age(crop, crop_stage_id(crop, crop.max_age) + 1), None);
        }
        assert_eq!(crop_age_of(12409), Some((&CARROT, 7)));
        assert_eq!(crop_age_of(12410), Some((&POTATO, 0)));
        assert_eq!(crop_age_of(16795), Some((&BEETROOT, 3)));
        assert_eq!(crop_age_of(16796), None);
        assert_eq!(crop_age_of(crate::FARMLAND_BLOCK_ID), None);
        assert_eq!(crop_age_of(crate::OAK_SAPLING_BLOCK_ID), None);
    }

    #[test]
    fn p33_seed_routing_maps_all_four_crops() {
        assert_eq!(crop_for_seed(ITEM_WHEAT_SEEDS), Some(&WHEAT));
        assert_eq!(crop_for_seed(ITEM_CARROT), Some(&CARROT));
        assert_eq!(crop_for_seed(ITEM_POTATO), Some(&POTATO));
        assert_eq!(crop_for_seed(ITEM_BEETROOT_SEEDS), Some(&BEETROOT));
        assert_eq!(crop_base_for_seed(ITEM_CARROT), Some(CARROT.base_id));
        assert_eq!(crop_base_for_seed(ITEM_POTATO), Some(POTATO.base_id));
        assert_eq!(crop_base_for_seed(ITEM_BEETROOT_SEEDS), Some(BEETROOT.base_id));
        // 成熟产物本身不是“种子”（除甜菜：种子就是 beetroot_seeds）。
        assert_eq!(crop_for_seed(ITEM_WHEAT), None);
        assert_eq!(crop_for_seed(ITEM_BEETROOT), None);
        assert_eq!(crop_for_seed(crate::drops::ITEM_OAK_SAPLING), None);
        assert_eq!(crop_for_seed(crate::drops::ITEM_BONE_MEAL), None);
    }
}
