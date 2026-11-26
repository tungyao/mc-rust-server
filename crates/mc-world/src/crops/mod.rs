//! crops: P33 通用作物表 + P37 全部作物集中规格/路由。
//!
//! 一条作物 = 手持种子 item_id + 方块 base（age=0 的 blockstate）+ 最大 age
//! + 每阶段 tick + 成熟/未熟掉落口径。所有 ID 都从 `mc_registry`（由
//! `data/777/*.json` 生成）在编译期查表得到，不在实现里写数字；单测才断言
//! 已知值防数据漂移。
//!
//! P37 扩展五类生长模型（见 [`growth`]）：
//! - `Age`：单属性 age 作物（P33 四种 + 地狱疣/瓜梗/南瓜梗/甜浆果/火把花/猪笼草/可可）；
//! - `Tree`：树苗一次到期长成多格树（P26 橡树 + P37 其它 7 种）；
//! - `Vertical`：糖甘蔗/仙人掌/竹子向上堆叠（不属于 age 模型）；
//! - `Side`：可可豆贴 jungle_log 侧面（age×facing 多属性）；
//! - `Fruit`：瓜梗成熟后在邻格落整块瓜/南瓜（MVP，见 `growth::fruit_neighbors`）。
//!
//! 掉落规则集中在 [`drops`]；`mc_world::drops` 只保留薄 shim。网络侧
//! `mc_network::login_route::crops` 只做手持路由/判定/推进的纯函数，
//! socket/背包/经验球不进入本模块。

pub mod drops;
pub mod growth;

pub use drops::crop_block_drops;
pub use growth::{
    age_for_elapsed, cocoa_facing_index, cocoa_state, cocoa_support_pos, fruit_neighbors,
    pitcher_states, sapling_tree_blocks, tree_blocks, vertical_state,
};

use crate::drops::{
    ITEM_BEETROOT_SEEDS, ITEM_CARROT, ITEM_POTATO, ITEM_WHEAT_SEEDS,
};

/// 掉落口径（成熟/未熟两臂，见 [`drops`]）。
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

/// 全部 P33 作物（顺序固定：小麦、胡萝卜、马铃薯、甜菜）。
/// blockstate 区间互不重叠，故 `crop_age_of` 的线性查找唯一命中。
pub const CROPS: [CropSpec; 4] = [WHEAT, CARROT, POTATO, BEETROOT];

// ---------------------------------------------------------------------------
// P37：土壤类型（保留 `is_crop_soil` 旧语义，新增类型另立判定）
// ---------------------------------------------------------------------------

/// 种植土壤集合。旧 `is_crop_soil` 只含草/泥土/耕地，P37 新增 soul_sand/sand
/// 独立类型，避免改变小麦/树苗/锄头的既有行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Soil {
    /// 草/泥土/耕地（P22/P27 旧口径）。
    Crop,
    /// 仅灵魂沙（地狱疣）。
    SoulSand,
    /// 仅沙子（仙人掌）。
    Sand,
    /// 草/泥土/耕地或沙子（甘蔗/竹子）。
    CropOrSand,
}

pub const SOUL_SAND_BLOCK_ID: u16 =
    crate::registry_id(mc_registry::BLOCKS, "minecraft:soul_sand") as u16;

/// 按土壤类型判定是否可种（不修改旧 `is_crop_soil`）。
pub fn soil_accepts(soil: Soil, id: u16) -> bool {
    match soil {
        Soil::Crop => crate::is_crop_soil(id),
        Soil::SoulSand => id == SOUL_SAND_BLOCK_ID,
        Soil::Sand => id == crate::SAND_BLOCK_ID,
        Soil::CropOrSand => crate::is_crop_soil(id) || id == crate::SAND_BLOCK_ID,
    }
}

// ---------------------------------------------------------------------------
// P37：单属性 age 作物（新）
// ---------------------------------------------------------------------------

/// 成熟后可能结出的整块果实（瓜/南瓜）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FruitSpec {
    pub block_id: u16,
    pub item_id: i32,
    pub min_count: i32,
    pub max_count: i32,
}

/// 单属性 age 作物规格（`base_id` 是 age=0 的 blockstate）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgeCropSpec {
    pub name: &'static str,
    pub seed_item: i32,
    pub base_id: u16,
    pub max_age: u8,
    pub ticks_per_stage: u64,
    pub soil: Soil,
    /// 耕地是否减半（原本适用耕地加速的作物才为 true）。
    pub farmland_boost: bool,
    /// 成熟时在邻格落整块果实（瓜/南瓜）。
    pub fruit: Option<FruitSpec>,
}

/// 地狱疣：只种灵魂沙（独立土壤），4 个 age 阶段。
pub const NETHER_WART: AgeCropSpec = AgeCropSpec {
    name: "nether_wart",
    seed_item: crate::drops::ITEM_NETHER_WART,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:nether_wart", 0),
    max_age: 3,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    soil: Soil::SoulSand,
    farmland_boost: false,
    fruit: None,
};

/// 西瓜梗：8 阶段，成熟在邻格落西瓜块。
pub const MELON_STEM: AgeCropSpec = AgeCropSpec {
    name: "melon_stem",
    seed_item: crate::drops::ITEM_MELON_SEEDS,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:melon_stem", 0),
    max_age: 7,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    soil: Soil::Crop,
    farmland_boost: true,
    fruit: Some(FruitSpec {
        block_id: crate::registry_id(mc_registry::BLOCKS, "minecraft:melon") as u16,
        item_id: crate::drops::ITEM_MELON_SLICE,
        min_count: 3,
        max_count: 7,
    }),
};

/// 南瓜梗：8 阶段，成熟在邻格落南瓜块。
pub const PUMPKIN_STEM: AgeCropSpec = AgeCropSpec {
    name: "pumpkin_stem",
    seed_item: crate::drops::ITEM_PUMPKIN_SEEDS,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:pumpkin_stem", 0),
    max_age: 7,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    soil: Soil::Crop,
    farmland_boost: true,
    fruit: Some(FruitSpec {
        block_id: crate::registry_id(mc_registry::BLOCKS, "minecraft:pumpkin") as u16,
        item_id: crate::drops::ITEM_PUMPKIN,
        min_count: 1,
        max_count: 1,
    }),
};

/// 甜浆果丛：4 阶段，age=2/3 可采果。
pub const SWEET_BERRY_BUSH: AgeCropSpec = AgeCropSpec {
    name: "sweet_berry_bush",
    seed_item: crate::drops::ITEM_SWEET_BERRIES,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:sweet_berry_bush", 0),
    max_age: 3,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    soil: Soil::Crop,
    farmland_boost: false,
    fruit: None,
};

/// 火把花种子：2 阶段，成熟时上方长花（MVP：只落种子/花物品，不生成花方块）。
pub const TORCHFLOWER_CROP: AgeCropSpec = AgeCropSpec {
    name: "torchflower_crop",
    seed_item: crate::drops::ITEM_TORCHFLOWER_SEEDS,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:torchflower_crop", 0),
    max_age: 1,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    soil: Soil::Crop,
    farmland_boost: true,
    fruit: None,
};

/// 猪笼草：5 个 age，每个 age 上下两半（half=upper/lower，lower 先列）。
pub const PITCHER_CROP: AgeCropSpec = AgeCropSpec {
    name: "pitcher_crop",
    seed_item: crate::drops::ITEM_PITCHER_POD,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:pitcher_crop", 1),
    max_age: 4,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    soil: Soil::Crop,
    farmland_boost: true,
    fruit: None,
};

pub const AGE_CROPS: [AgeCropSpec; 6] = [
    NETHER_WART,
    MELON_STEM,
    PUMPKIN_STEM,
    SWEET_BERRY_BUSH,
    TORCHFLOWER_CROP,
    PITCHER_CROP,
];

/// 单属性 age 作物：age → blockstate。瓜梗/浆果等直接 base+age；
/// 猪笼草特殊（上下半），走 [`pitcher_states`]。
pub fn age_stage_id(spec: &AgeCropSpec, age: u8) -> u16 {
    if spec.name == "pitcher_crop" {
        return pitcher_states(age).0;
    }
    spec.base_id + age.min(spec.max_age) as u16
}

/// blockstate → (规格, age)，非该作物区间返回 None。
pub fn age_crop_of(id: u16) -> Option<(&'static AgeCropSpec, u8)> {
    for crop in &AGE_CROPS {
        if crop.name == "pitcher_crop" {
            for age in 0..=crop.max_age {
                let (lower, upper) = pitcher_states(age);
                if id == lower || id == upper {
                    return Some((crop, age));
                }
            }
            continue;
        }
        let age = id.wrapping_sub(crop.base_id);
        if age <= crop.max_age as u16 {
            return Some((crop, age as u8));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// P37：树苗（一次到期长成多格树）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeSpec {
    pub name: &'static str,
    pub seed_item: i32,
    pub sapling_id: u16,
    pub log_id: u16,
    /// 对应物品 ID（logs 与 items 同名，编译期查 items 生成表）。
    pub log_item_id: i32,
    pub leaves_id: u16,
    pub grow_ticks: u64,
}

const fn tree_spec(
    name: &'static str,
    seed_item: i32,
    sapling: &'static str,
    log: &'static str,
    leaves: &'static str,
) -> TreeSpec {
    TreeSpec {
        name,
        seed_item,
        sapling_id: crate::registry_id(mc_registry::BLOCKS, sapling) as u16,
        log_id: crate::registry_id(mc_registry::BLOCKS, log) as u16,
        log_item_id: crate::registry_id(mc_registry::ITEMS, log) as i32,
        leaves_id: crate::registry_id(mc_registry::BLOCKS, leaves) as u16,
        grow_ticks: crate::SAPLING_GROW_TICKS,
    }
}

/// 橡树规格：只用于让 `sapling_oak_blocks` 复用通用形状函数；追踪仍走 P26 旧路径。
pub const OAK_TREE: TreeSpec = tree_spec(
    "oak", 77, "minecraft:oak_sapling", "minecraft:oak_log", "minecraft:oak_leaves",
);

/// P37 新树苗（橡树走 P26 旧路径，不在表内以保证旧行为逐字节不变）。
pub const BIRCH: TreeSpec = tree_spec(
    "birch", 79, "minecraft:birch_sapling", "minecraft:birch_log", "minecraft:birch_leaves",
);
pub const SPRUCE: TreeSpec = tree_spec(
    "spruce", 78, "minecraft:spruce_sapling", "minecraft:spruce_log", "minecraft:spruce_leaves",
);
pub const JUNGLE: TreeSpec = tree_spec(
    "jungle", 80, "minecraft:jungle_sapling", "minecraft:jungle_log", "minecraft:jungle_leaves",
);
pub const ACACIA: TreeSpec = tree_spec(
    "acacia", 81, "minecraft:acacia_sapling", "minecraft:acacia_log", "minecraft:acacia_leaves",
);
pub const DARK_OAK: TreeSpec = tree_spec(
    "dark_oak", 83, "minecraft:dark_oak_sapling", "minecraft:dark_oak_log", "minecraft:dark_oak_leaves",
);
pub const CHERRY: TreeSpec = tree_spec(
    "cherry", 82, "minecraft:cherry_sapling", "minecraft:cherry_log", "minecraft:cherry_leaves",
);
/// 红树繁殖体：用默认态种下（其 states 含 hanging/stage/waterlogged 多属性，
/// 不按 base+1 解析），长成形状复用同一冠形，材质为红树原木/树叶。
pub const MANGROVE: TreeSpec = tree_spec(
    "mangrove_propagule", 85, "minecraft:mangrove_propagule", "minecraft:mangrove_log", "minecraft:mangrove_leaves",
);

pub const TREE_SPECS: [TreeSpec; 7] =
    [BIRCH, SPRUCE, JUNGLE, ACACIA, DARK_OAK, CHERRY, MANGROVE];

pub fn tree_spec_for_sapling(id: u16) -> Option<&'static TreeSpec> {
    TREE_SPECS.iter().find(|spec| spec.sapling_id == id)
}

pub fn tree_spec_for_log(id: u16) -> Option<&'static TreeSpec> {
    TREE_SPECS.iter().find(|spec| spec.log_id == id)
}

pub fn tree_spec_for_leaves(id: u16) -> Option<&'static TreeSpec> {
    TREE_SPECS.iter().find(|spec| spec.leaves_id == id)
}

// ---------------------------------------------------------------------------
// P37：竖向堆叠（甘蔗/仙人掌/竹子）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct VerticalSpec {
    pub name: &'static str,
    pub item_id: i32,
    pub base_id: u16,
    pub soil: Soil,
    pub max_height: u8,
    pub ticks_per_stage: u64,
    /// 该高度（0 = 基部）应放的 blockstate；竹子按 leaves/stage 属性换材质。
    pub state_fn: fn(u8) -> u16,
}

pub const SUGAR_CANE: VerticalSpec = VerticalSpec {
    name: "sugar_cane",
    item_id: crate::drops::ITEM_SUGAR_CANE,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:sugar_cane", 0),
    soil: Soil::CropOrSand,
    max_height: 3,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    state_fn: |_| crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:sugar_cane", 0),
};

pub const CACTUS: VerticalSpec = VerticalSpec {
    name: "cactus",
    item_id: crate::drops::ITEM_CACTUS,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:cactus", 0),
    soil: Soil::Sand,
    max_height: 3,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    state_fn: |_| crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:cactus", 0),
};

pub const BAMBOO: VerticalSpec = VerticalSpec {
    name: "bamboo",
    item_id: crate::drops::ITEM_BAMBOO,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:bamboo", 0),
    soil: Soil::CropOrSand,
    max_height: 6,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
    state_fn: vertical::bamboo_state,
};

pub const VERTICAL_SPECS: [VerticalSpec; 3] = [SUGAR_CANE, CACTUS, BAMBOO];

pub fn vertical_spec_of(id: u16) -> Option<&'static VerticalSpec> {
    VERTICAL_SPECS.iter().find(|spec| (spec.state_fn)(0) == id)
}

/// 可生长植物统一视图（挖掘连带/跟踪/认领共用）。
#[derive(Debug, Clone, Copy)]
pub enum PlantKind {
    Age(&'static AgeCropSpec),
    Tree(&'static TreeSpec),
    Vertical(&'static VerticalSpec),
    Side(&'static SideSpec),
}

/// blockstate → 植物 + 当前 age/高度（P33 老作物不在本表，仍走 `crop_age_of`）。
pub fn plant_at(id: u16) -> Option<(PlantKind, u8)> {
    if let Some((spec, age)) = age_crop_of(id) {
        return Some((PlantKind::Age(spec), age));
    }
    if let Some(spec) = tree_spec_for_sapling(id) {
        return Some((PlantKind::Tree(spec), 0));
    }
    if let Some(spec) = vertical_spec_of(id) {
        return Some((PlantKind::Vertical(spec), 0));
    }
    if let Some((spec, age, _)) = cocoa_of(id) {
        return Some((PlantKind::Side(spec), age));
    }
    None
}

// ---------------------------------------------------------------------------
// P37：贴侧面生长（可可豆）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideSpec {
    pub name: &'static str,
    pub seed_item: i32,
    pub base_id: u16,
    /// 必须贴在 jungle_log 侧面。
    pub support_id: u16,
    pub max_age: u8,
    pub ticks_per_stage: u64,
}

pub const COCOA: SideSpec = SideSpec {
    name: "cocoa",
    seed_item: crate::drops::ITEM_COCOA_BEANS,
    base_id: crate::state_index_id(mc_registry::BLOCK_STATES, "minecraft:cocoa", 0),
    support_id: crate::registry_id(mc_registry::BLOCKS, "minecraft:jungle_log") as u16,
    max_age: 2,
    ticks_per_stage: crate::WHEAT_TICKS_PER_STAGE,
};

/// blockstate → (可可规格, age, facing 索引 0=north/1=south/2=west/3=east)。
pub fn cocoa_of(id: u16) -> Option<(&'static SideSpec, u8, u8)> {
    let idx = id.wrapping_sub(COCOA.base_id);
    if idx < 12 {
        Some((&COCOA, (idx / 4) as u8, (idx % 4) as u8))
    } else {
        None
    }
}

pub mod vertical {
    use crate::state_index_id;
    /// 竹子：age=0、leaves 按高度 none→small→large、stage=0。
    /// states 顺序 [age(0/1), leaves(none/small/large), stage(0/1)]，末位最快。
    pub fn bamboo_state(height: u8) -> u16 {
        let leaves = match height {
            0..=3 => 0,
            4 => 1,
            _ => 2,
        };
        state_index_id(mc_registry::BLOCK_STATES, "minecraft:bamboo", leaves * 2)
    }
}

// ---------------------------------------------------------------------------
// 通用 stage/路由 API（旧签名保持不变，新增 P37 API 并列）
// ---------------------------------------------------------------------------

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

/// 任意 P33 作物方块 → (规格, age)；非作物返回 None。
pub fn crop_age_of(id: u16) -> Option<(&'static CropSpec, u8)> {
    for crop in &CROPS {
        if let Some(age) = crop_age(crop, id) {
            return Some((crop, age));
        }
    }
    None
}

/// 种子物品 → P33 作物规格（种植路由用；树苗/新作物由 `held_plant` 统一路由）。
pub fn crop_for_seed(item_id: i32) -> Option<&'static CropSpec> {
    CROPS.iter().find(|crop| crop.seed_item == item_id)
}

/// 种子物品 → 种下时的 age=0 方块 ID。
pub fn crop_base_for_seed(item_id: i32) -> Option<u16> {
    crop_for_seed(item_id).map(|crop| crop.base_id)
}

/// 统一 blockstate → (规格, age)：老作物与新 age 作物共用一个入口。
pub fn any_age_crop_of(id: u16) -> Option<(Option<&'static CropSpec>, Option<&'static AgeCropSpec>, u8)> {
    if let Some((crop, age)) = crop_age_of(id) {
        return Some((Some(crop), None, age));
    }
    if let Some((crop, age)) = age_crop_of(id) {
        return Some((None, Some(crop), age));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drops::{ITEM_BEETROOT, ITEM_POISONOUS_POTATO, ITEM_WHEAT};

    #[test]
    fn p33_crop_ids_and_seed_items_come_from_registry() {
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
        assert_eq!(crop_for_seed(ITEM_WHEAT), None);
        assert_eq!(crop_for_seed(ITEM_BEETROOT), None);
        assert_eq!(crop_for_seed(crate::drops::ITEM_OAK_SAPLING), None);
        assert_eq!(crop_for_seed(crate::drops::ITEM_BONE_MEAL), None);
    }

    /// P37：全部新 ID 来自 codegen 生成表，断言实测值防漂移。
    #[test]
    fn p37_new_crop_ids_come_from_registry() {
        for (item, expected, name) in [
            (crate::drops::ITEM_NETHER_WART, 1269, "nether_wart"),
            (crate::drops::ITEM_MELON_SEEDS, 1259, "melon_seeds"),
            (crate::drops::ITEM_PUMPKIN_SEEDS, 1258, "pumpkin_seeds"),
            (crate::drops::ITEM_SWEET_BERRIES, 1525, "sweet_berries"),
            (crate::drops::ITEM_TORCHFLOWER_SEEDS, 1436, "torchflower_seeds"),
            (crate::drops::ITEM_PITCHER_POD, 1437, "pitcher_pod"),
            (crate::drops::ITEM_COCOA_BEANS, 1198, "cocoa_beans"),
            (crate::drops::ITEM_SUGAR_CANE, 327, "sugar_cane"),
            (crate::drops::ITEM_BAMBOO, 340, "bamboo"),
            (crate::drops::ITEM_CACTUS, 413, "cactus"),
            (BIRCH.seed_item, 79, "birch_sapling"),
            (MANGROVE.seed_item, 85, "mangrove_propagule"),
        ] {
            assert_eq!(item as u32, expected, "wrong item ID for {name}");
            assert_eq!(Some(item as u32), mc_registry::item_id(name));
        }
        for (block, expected, name) in [
            (NETHER_WART.base_id, 11181, "nether_wart"),
            (MELON_STEM.base_id, 10084, "melon_stem"),
            (PUMPKIN_STEM.base_id, 10076, "pumpkin_stem"),
            (SWEET_BERRY_BUSH.base_id, 24298, "sweet_berry_bush"),
            (TORCHFLOWER_CROP.base_id, 16778, "torchflower_crop"),
            (PITCHER_CROP.base_id, 16781, "pitcher_crop"),
            (COCOA.base_id, 11215, "cocoa"),
            (SUGAR_CANE.base_id, 8617, "sugar_cane"),
            (CACTUS.base_id, 8599, "cactus"),
            (BAMBOO.base_id, 18636, "bamboo"),
            (BIRCH.sapling_id, 34, "birch_sapling"),
            (MANGROVE.sapling_id, 51, "mangrove_propagule"),
            (SOUL_SAND_BLOCK_ID, 8668, "soul_sand"),
        ] {
            assert_eq!(block as u32, expected, "wrong blockstate ID for {name}");
            assert_eq!(Some(block as u32), mc_registry::block_id(name));
        }
        // 每种树苗的苗/原木/树叶/原木物品都用对应物种真实 ID（不复用橡树材质）。
        // 红树繁殖体名字不带 _sapling，故物种短名单独列出。
        for (spec, short) in TREE_SPECS.iter().zip([
            "birch", "spruce", "jungle", "acacia", "dark_oak", "cherry", "mangrove",
        ]) {
            let (sapling, log, leaves) = if short == "mangrove" {
                ("minecraft:mangrove_propagule", "minecraft:mangrove_log", "minecraft:mangrove_leaves")
            } else {
                let s: &'static str = Box::leak(format!("minecraft:{short}_sapling").into_boxed_str());
                let l: &'static str = Box::leak(format!("minecraft:{short}_log").into_boxed_str());
                let v: &'static str = Box::leak(format!("minecraft:{short}_leaves").into_boxed_str());
                (s, l, v)
            };
            assert_eq!(Some(spec.sapling_id as u32), mc_registry::block_id(sapling));
            assert_eq!(Some(spec.log_id as u32), mc_registry::block_id(log));
            assert_eq!(Some(spec.leaves_id as u32), mc_registry::block_id(leaves));
            assert_eq!(spec.log_item_id as u32, mc_registry::item_id(log).unwrap());
        }
        // 果实/花物品 ID 同样来自生成表。
        for (item, name) in [
            (crate::drops::ITEM_TORCHFLOWER, "minecraft:torchflower"),
            (crate::drops::ITEM_PITCHER_PLANT, "minecraft:pitcher_plant"),
            (crate::drops::ITEM_MELON_SLICE, "minecraft:melon_slice"),
            (crate::drops::ITEM_PUMPKIN, "minecraft:pumpkin"),
        ] {
            assert_eq!(Some(item as u32), mc_registry::item_id(name));
        }
        for spec in &AGE_CROPS {
            if let Some(fruit) = spec.fruit {
                assert_eq!(Some(fruit.block_id as u32), mc_registry::block_id(if spec.name == "melon_stem" { "minecraft:melon" } else { "minecraft:pumpkin" }));
                assert_eq!(Some(fruit.item_id as u32), mc_registry::item_id(if spec.name == "melon_stem" { "minecraft:melon_slice" } else { "minecraft:pumpkin" }));
            }
        }
    }

    #[test]
    fn p37_multi_property_stages_are_resolved_by_attribute_index() {
        // 地狱疣/瓜梗/浆果/火把花是单属性 age：base+age。
        for (spec, last, name) in [
            (&NETHER_WART, 11184u16, "nether_wart"),
            (&MELON_STEM, 10091, "melon_stem"),
            (&PUMPKIN_STEM, 10083, "pumpkin_stem"),
            (&SWEET_BERRY_BUSH, 24301, "sweet_berry_bush"),
            (&TORCHFLOWER_CROP, 16779, "torchflower_crop"),
        ] {
            assert_eq!(age_stage_id(spec, spec.max_age), last, "{name}");
            assert_eq!(age_crop_of(last).map(|(s, a)| (s.name, a)), Some((spec.name, spec.max_age)));
        }
        // 猪笼草：age×half（half 有 upper/lower 两态，低位是 upper）。
        assert_eq!(pitcher_states(0), (16781, 16780));
        assert_eq!(pitcher_states(4), (16789, 16788));
        assert_eq!(age_stage_id(&PITCHER_CROP, 4), 16789);
        assert_eq!(age_crop_of(16788), Some((&PITCHER_CROP, 4)));
        // 可可：age×facing（north/south/west/east），不是 base+age。
        assert_eq!(cocoa_state(0, 0), 11215);
        assert_eq!(cocoa_state(2, 3), 11226);
        assert_eq!(cocoa_of(11226), Some((&COCOA, 2, 3)));
        assert_eq!(cocoa_of(11227), None);
        // 竹子：leaves/stage 属性，age=0 时 leaves=large/stage=0 是第 5 个状态。
        assert_eq!(vertical::bamboo_state(0), 18636);
        assert_eq!(vertical::bamboo_state(4), 18638);
        assert_eq!(vertical::bamboo_state(5), 18640);
        assert_eq!((BAMBOO.state_fn)(5), 18640);
    }

    #[test]
    fn p37_soil_types_keep_old_semantics() {
        for id in [crate::GRASS_BLOCK_ID, crate::DIRT_BLOCK_ID, crate::FARMLAND_BLOCK_ID] {
            assert!(soil_accepts(Soil::Crop, id));
        }
        assert!(!soil_accepts(Soil::Crop, SOUL_SAND_BLOCK_ID));
        assert!(!crate::is_crop_soil(SOUL_SAND_BLOCK_ID), "old soil set must not change");
        assert!(soil_accepts(Soil::SoulSand, SOUL_SAND_BLOCK_ID));
        assert!(!soil_accepts(Soil::SoulSand, crate::FARMLAND_BLOCK_ID));
        assert!(soil_accepts(Soil::Sand, crate::SAND_BLOCK_ID));
        assert!(soil_accepts(Soil::CropOrSand, crate::SAND_BLOCK_ID));
        assert!(soil_accepts(Soil::CropOrSand, crate::DIRT_BLOCK_ID));
        assert!(!soil_accepts(Soil::CropOrSand, crate::STONE_BLOCK_ID));
    }
}
