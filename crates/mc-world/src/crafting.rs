//! crafting: P40 2×2 玩家背包合成的纯逻辑（配方 + 匹配 + 消耗），只兼容 26.3/777。
//!
//! 所有物品 ID 均由 `mc_registry::ITEMS`（`data/777/items.json` 生成表）在编译期查得，
//! 与 `drops.rs` 同口径；不手写/猜物品 ID。本轮只做玩家背包 2×2（槽 1–4 输入、0 输出），
//! 3×3 工作台 / block entity 明确不做（见 HANDOFF 任务说明）。
//!
//! 配方为**无序精确匹配**：把 4 个合成格内的物品按 ID 计数，必须与某条配方的
//! 需求计数完全一致（不多不少）；输出栈直接返回给调用方，由网络层放光标/背包。
//!
//! P41 扩充：覆盖原版所有能在背包 2×2 中放下的合成（木板/木棍/工作台、全材质工具与
//! 可容纳盔甲、火把/面包/纸书/床箱/熔炉/漏斗/活塞/铁轨/告示牌/门/栅栏/台阶楼梯墙/
//! 按钮压力板拉杆/梯子/桶/打火石/指南针钟地图/干草捆等）。沿用本服既有简化：**只按
//! (id,count) 多重集匹配，不检查形状**，因此原版 3×3 配方只要输入种类 ≤4、数量能
//! 以堆叠放进 4 格（如 9 小麦→干草捆、8 木板→箱子）也在此实现；未实现需要多格
//! 不同物品（>4 种）或工作台专属的配方。
//!
//! **歧义处理**：原版若干配方的输入多重集完全相同（木棍/压力板、工作台/楼梯、
//! 镐/斧、门/活板门、栅栏/栅栏门、铁护腿/炼药锅），无序规则下无法仅靠物品集合区分。
//! 本模块用确定性的“槽分布指纹”（按 (id,count) 排序的占用槽列表，位置无关）拆分：
//! 每种配方有唯一规范槽型，见 [`canonical_layout`]；若输入的槽分布不匹配任何规范型
//! （例如把 4 木板全塞一格又给出了楼梯的候选），按 `RECIPES` 表序取首条。不用随机、
//! 不用 HashMap 迭代顺序；旧 7 条配方留在表首，既有行为逐字节不变。

use crate::registry_id;

/// 合成格数量（背包 1..=4）。
pub const GRID_SIZE: usize = 4;
/// 合成结果槽（背包 0）。
pub const RESULT_SLOT: usize = 0;

/// 单条无序配方：`needs` 为 (物品 ID, 数量)，`result` 为 (物品 ID, 数量)。
#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    pub needs: &'static [(i32, i32)],
    pub result: (i32, i32),
    /// 便于诊断/测试的名称（不参与匹配）。
    pub name: &'static str,
}

// 物品 ID 全部来自生成表（括号里是 data/777/items.json 的实测值，改数据后编译期自动漂移）。
const OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_log") as i32; // 163
const OAK_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_planks") as i32; // 63
const STICK: i32 = registry_id(mc_registry::ITEMS, "minecraft:stick") as i32; // 1060
const CRAFTING_TABLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:crafting_table") as i32; // 405
const COBBLESTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobblestone") as i32; // 62
const WOODEN_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_pickaxe") as i32; // 1027
const STONE_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_pickaxe") as i32; // 1037
const WHEAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:wheat") as i32; // 1066
const BREAD: i32 = registry_id(mc_registry::ITEMS, "minecraft:bread") as i32; // 1067
const COAL: i32 = registry_id(mc_registry::ITEMS, "minecraft:coal") as i32; // 1010
const TORCH: i32 = registry_id(mc_registry::ITEMS, "minecraft:torch") as i32; // 395
const ITEM_ACACIA_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_boat") as i32; // 983
const ITEM_ACACIA_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_button") as i32; // 862
const ITEM_ACACIA_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_door") as i32; // 893
const ITEM_ACACIA_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_fence") as i32; // 421
const ITEM_ACACIA_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_fence_gate") as i32; // 936
const ITEM_ACACIA_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_log") as i32; // 167
const ITEM_ACACIA_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_planks") as i32; // 67
const ITEM_ACACIA_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_pressure_plate") as i32; // 879
const ITEM_ACACIA_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_sign") as i32; // 1106
const ITEM_ACACIA_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_slab") as i32; // 345
const ITEM_ACACIA_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_stairs") as i32; // 519
const ITEM_ACACIA_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_trapdoor") as i32; // 915
const ITEM_ACTIVATOR_RAIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:activator_rail") as i32; // 948
const ITEM_ANVIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:anvil") as i32; // 553
const ITEM_ARROW: i32 = registry_id(mc_registry::ITEMS, "minecraft:arrow") as i32; // 1009
const ITEM_BARREL: i32 = registry_id(mc_registry::ITEMS, "minecraft:barrel") as i32; // 1506
const ITEM_BIRCH_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_boat") as i32; // 979
const ITEM_BIRCH_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_button") as i32; // 860
const ITEM_BIRCH_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_door") as i32; // 891
const ITEM_BIRCH_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_fence") as i32; // 419
const ITEM_BIRCH_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_fence_gate") as i32; // 934
const ITEM_BIRCH_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_log") as i32; // 165
const ITEM_BIRCH_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_planks") as i32; // 65
const ITEM_BIRCH_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_pressure_plate") as i32; // 877
const ITEM_BIRCH_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_sign") as i32; // 1104
const ITEM_BIRCH_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_slab") as i32; // 343
const ITEM_BIRCH_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_stairs") as i32; // 517
const ITEM_BIRCH_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_trapdoor") as i32; // 913
const ITEM_BLACK_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_bed") as i32; // 1234
const ITEM_BLACK_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_carpet") as i32; // 595
const ITEM_BLACK_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_dye") as i32; // 1214
const ITEM_BLACK_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_wool") as i32; // 265
const ITEM_BLUE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_bed") as i32; // 1230
const ITEM_BLUE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_carpet") as i32; // 591
const ITEM_BLUE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_dye") as i32; // 1210
const ITEM_BLUE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_wool") as i32; // 261
const ITEM_BOOK: i32 = registry_id(mc_registry::ITEMS, "minecraft:book") as i32; // 1146
const ITEM_BOOKSHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:bookshelf") as i32; // 390
const ITEM_BOW: i32 = registry_id(mc_registry::ITEMS, "minecraft:bow") as i32; // 1008
const ITEM_BROWN_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_bed") as i32; // 1231
const ITEM_BROWN_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_carpet") as i32; // 592
const ITEM_BROWN_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_dye") as i32; // 1211
const ITEM_BROWN_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_wool") as i32; // 262
const ITEM_BUCKET: i32 = registry_id(mc_registry::ITEMS, "minecraft:bucket") as i32; // 1128
const ITEM_CAKE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cake") as i32; // 1218
const ITEM_CAULDRON: i32 = registry_id(mc_registry::ITEMS, "minecraft:cauldron") as i32; // 1277
const ITEM_CHERRY_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_boat") as i32; // 985
const ITEM_CHERRY_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_button") as i32; // 863
const ITEM_CHERRY_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_door") as i32; // 894
const ITEM_CHERRY_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_fence") as i32; // 422
const ITEM_CHERRY_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_fence_gate") as i32; // 937
const ITEM_CHERRY_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_log") as i32; // 168
const ITEM_CHERRY_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_planks") as i32; // 68
const ITEM_CHERRY_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_pressure_plate") as i32; // 880
const ITEM_CHERRY_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_sign") as i32; // 1107
const ITEM_CHERRY_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_slab") as i32; // 346
const ITEM_CHERRY_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_stairs") as i32; // 520
const ITEM_CHERRY_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_trapdoor") as i32; // 916
const ITEM_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:chest") as i32; // 404
const ITEM_CLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:clock") as i32; // 1187
const ITEM_COAL_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:coal_block") as i32; // 112
const ITEM_COBBLESTONE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobblestone_wall") as i32; // 531
const ITEM_COCOA_BEANS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cocoa_beans") as i32; // 1198
const ITEM_COMPASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:compass") as i32; // 1151
const ITEM_COOKIE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cookie") as i32; // 1236
const ITEM_COPPER_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_axe") as i32; // 1033
const ITEM_COPPER_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_hoe") as i32; // 1034
const ITEM_COPPER_INGOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_ingot") as i32; // 1020
const ITEM_COPPER_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_pickaxe") as i32; // 1032
const ITEM_COPPER_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_shovel") as i32; // 1031
const ITEM_COPPER_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_sword") as i32; // 1030
const ITEM_CRIMSON_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_door") as i32; // 900
const ITEM_CRIMSON_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_planks") as i32; // 74
const ITEM_CRIMSON_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_sign") as i32; // 1113
const ITEM_CRIMSON_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_slab") as i32; // 353
const ITEM_CRIMSON_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_stairs") as i32; // 527
const ITEM_CRIMSON_STEM: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_stem") as i32; // 175
const ITEM_CRIMSON_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_trapdoor") as i32; // 922
const ITEM_CYAN_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_bed") as i32; // 1228
const ITEM_CYAN_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_carpet") as i32; // 589
const ITEM_CYAN_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_dye") as i32; // 1208
const ITEM_CYAN_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_wool") as i32; // 259
const ITEM_DARK_OAK_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_boat") as i32; // 987
const ITEM_DARK_OAK_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_button") as i32; // 864
const ITEM_DARK_OAK_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_door") as i32; // 895
const ITEM_DARK_OAK_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_fence") as i32; // 423
const ITEM_DARK_OAK_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_fence_gate") as i32; // 938
const ITEM_DARK_OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_log") as i32; // 170
const ITEM_DARK_OAK_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_planks") as i32; // 69
const ITEM_DARK_OAK_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_pressure_plate") as i32; // 881
const ITEM_DARK_OAK_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_sign") as i32; // 1108
const ITEM_DARK_OAK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_slab") as i32; // 347
const ITEM_DARK_OAK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_stairs") as i32; // 521
const ITEM_DARK_OAK_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_trapdoor") as i32; // 917
const ITEM_DETECTOR_RAIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:detector_rail") as i32; // 946
const ITEM_DIAMOND: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond") as i32; // 1012
const ITEM_DIAMOND_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_axe") as i32; // 1053
const ITEM_DIAMOND_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_block") as i32; // 129
const ITEM_DIAMOND_BOOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_boots") as i32; // 1087
const ITEM_DIAMOND_CHESTPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_chestplate") as i32; // 1085
const ITEM_DIAMOND_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_helmet") as i32; // 1084
const ITEM_DIAMOND_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_hoe") as i32; // 1054
const ITEM_DIAMOND_LEGGINGS: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_leggings") as i32; // 1086
const ITEM_DIAMOND_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_pickaxe") as i32; // 1052
const ITEM_DIAMOND_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_shovel") as i32; // 1051
const ITEM_DIAMOND_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_sword") as i32; // 1050
const ITEM_EGG: i32 = registry_id(mc_registry::ITEMS, "minecraft:egg") as i32; // 1148
const ITEM_EMERALD: i32 = registry_id(mc_registry::ITEMS, "minecraft:emerald") as i32; // 1013
const ITEM_EMERALD_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:emerald_block") as i32; // 514
const ITEM_FEATHER: i32 = registry_id(mc_registry::ITEMS, "minecraft:feather") as i32; // 1063
const ITEM_FISHING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:fishing_rod") as i32; // 1186
const ITEM_FLINT: i32 = registry_id(mc_registry::ITEMS, "minecraft:flint") as i32; // 1096
const ITEM_FLINT_AND_STEEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:flint_and_steel") as i32; // 1005
const ITEM_FURNACE: i32 = registry_id(mc_registry::ITEMS, "minecraft:furnace") as i32; // 407
const ITEM_GOLD_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:gold_block") as i32; // 128
const ITEM_GOLD_INGOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:gold_ingot") as i32; // 1022
const ITEM_GOLDEN_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_axe") as i32; // 1043
const ITEM_GOLDEN_BOOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_boots") as i32; // 1091
const ITEM_GOLDEN_CHESTPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_chestplate") as i32; // 1089
const ITEM_GOLDEN_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_helmet") as i32; // 1088
const ITEM_GOLDEN_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_hoe") as i32; // 1044
const ITEM_GOLDEN_LEGGINGS: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_leggings") as i32; // 1090
const ITEM_GOLDEN_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_pickaxe") as i32; // 1042
const ITEM_GOLDEN_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_shovel") as i32; // 1041
const ITEM_GOLDEN_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_sword") as i32; // 1040
const ITEM_GRAY_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_bed") as i32; // 1226
const ITEM_GRAY_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_carpet") as i32; // 587
const ITEM_GRAY_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_dye") as i32; // 1206
const ITEM_GRAY_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_wool") as i32; // 257
const ITEM_GREEN_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_bed") as i32; // 1232
const ITEM_GREEN_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_carpet") as i32; // 593
const ITEM_GREEN_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_dye") as i32; // 1212
const ITEM_GREEN_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_wool") as i32; // 263
const ITEM_GUNPOWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:gunpowder") as i32; // 1064
const ITEM_HAY_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:hay_block") as i32; // 579
const ITEM_HOPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:hopper") as i32; // 834
const ITEM_IRON_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_axe") as i32; // 1048
const ITEM_IRON_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_block") as i32; // 119
const ITEM_IRON_BOOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_boots") as i32; // 1083
const ITEM_IRON_CHESTPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_chestplate") as i32; // 1081
const ITEM_IRON_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_helmet") as i32; // 1080
const ITEM_IRON_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_hoe") as i32; // 1049
const ITEM_IRON_INGOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_ingot") as i32; // 1018
const ITEM_IRON_LEGGINGS: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_leggings") as i32; // 1082
const ITEM_IRON_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_pickaxe") as i32; // 1047
const ITEM_IRON_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_shovel") as i32; // 1046
const ITEM_IRON_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_sword") as i32; // 1045
const ITEM_JUNGLE_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_boat") as i32; // 981
const ITEM_JUNGLE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_button") as i32; // 861
const ITEM_JUNGLE_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_door") as i32; // 892
const ITEM_JUNGLE_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_fence") as i32; // 420
const ITEM_JUNGLE_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_fence_gate") as i32; // 935
const ITEM_JUNGLE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_log") as i32; // 166
const ITEM_JUNGLE_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_planks") as i32; // 66
const ITEM_JUNGLE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_pressure_plate") as i32; // 878
const ITEM_JUNGLE_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_sign") as i32; // 1105
const ITEM_JUNGLE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_slab") as i32; // 344
const ITEM_JUNGLE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_stairs") as i32; // 518
const ITEM_JUNGLE_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_trapdoor") as i32; // 914
const ITEM_LADDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:ladder") as i32; // 408
const ITEM_LEATHER: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather") as i32; // 1133
const ITEM_LEATHER_BOOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_boots") as i32; // 1071
const ITEM_LEATHER_CHESTPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_chestplate") as i32; // 1069
const ITEM_LEATHER_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_helmet") as i32; // 1068
const ITEM_LEATHER_LEGGINGS: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_leggings") as i32; // 1070
const ITEM_LEVER: i32 = registry_id(mc_registry::ITEMS, "minecraft:lever") as i32; // 839
const ITEM_LIGHT_BLUE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_bed") as i32; // 1222
const ITEM_LIGHT_BLUE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_carpet") as i32; // 583
const ITEM_LIGHT_BLUE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_dye") as i32; // 1202
const ITEM_LIGHT_BLUE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_wool") as i32; // 253
const ITEM_LIGHT_GRAY_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_bed") as i32; // 1227
const ITEM_LIGHT_GRAY_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_carpet") as i32; // 588
const ITEM_LIGHT_GRAY_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_dye") as i32; // 1207
const ITEM_LIGHT_GRAY_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_wool") as i32; // 258
const ITEM_LIME_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_bed") as i32; // 1224
const ITEM_LIME_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_carpet") as i32; // 585
const ITEM_LIME_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_dye") as i32; // 1204
const ITEM_LIME_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_wool") as i32; // 255
const ITEM_MAGENTA_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_bed") as i32; // 1221
const ITEM_MAGENTA_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_carpet") as i32; // 582
const ITEM_MAGENTA_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_dye") as i32; // 1201
const ITEM_MAGENTA_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_wool") as i32; // 252
const ITEM_MANGROVE_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_boat") as i32; // 991
const ITEM_MANGROVE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_button") as i32; // 866
const ITEM_MANGROVE_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_door") as i32; // 897
const ITEM_MANGROVE_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_fence") as i32; // 425
const ITEM_MANGROVE_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_fence_gate") as i32; // 940
const ITEM_MANGROVE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_log") as i32; // 171
const ITEM_MANGROVE_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_planks") as i32; // 71
const ITEM_MANGROVE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_pressure_plate") as i32; // 883
const ITEM_MANGROVE_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_sign") as i32; // 1110
const ITEM_MANGROVE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_slab") as i32; // 349
const ITEM_MANGROVE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_stairs") as i32; // 523
const ITEM_MANGROVE_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_trapdoor") as i32; // 919
const ITEM_MAP: i32 = registry_id(mc_registry::ITEMS, "minecraft:map") as i32; // 1382
const ITEM_NETHERITE_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_axe") as i32; // 1058
const ITEM_NETHERITE_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_block") as i32; // 130
const ITEM_NETHERITE_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_hoe") as i32; // 1059
const ITEM_NETHERITE_INGOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_ingot") as i32; // 1023
const ITEM_NETHERITE_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_pickaxe") as i32; // 1057
const ITEM_NETHERITE_SCRAP: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_scrap") as i32; // 1024
const ITEM_NETHERITE_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_shovel") as i32; // 1056
const ITEM_NETHERITE_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_sword") as i32; // 1055
const ITEM_OAK_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_boat") as i32; // 975
const ITEM_OAK_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_button") as i32; // 858
const ITEM_OAK_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_door") as i32; // 889
const ITEM_OAK_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_fence") as i32; // 417
const ITEM_OAK_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_fence_gate") as i32; // 932
const ITEM_OAK_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_pressure_plate") as i32; // 875
const ITEM_OAK_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_sign") as i32; // 1102
const ITEM_OAK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_slab") as i32; // 341
const ITEM_OAK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_stairs") as i32; // 515
const ITEM_OAK_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_trapdoor") as i32; // 911
const ITEM_ORANGE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_bed") as i32; // 1220
const ITEM_ORANGE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_carpet") as i32; // 581
const ITEM_ORANGE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_dye") as i32; // 1200
const ITEM_ORANGE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_wool") as i32; // 251
const ITEM_PAPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:paper") as i32; // 1145
const ITEM_PINK_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_bed") as i32; // 1225
const ITEM_PINK_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_carpet") as i32; // 586
const ITEM_PINK_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_dye") as i32; // 1205
const ITEM_PINK_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_wool") as i32; // 256
const ITEM_PISTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:piston") as i32; // 829
const ITEM_POWERED_RAIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:powered_rail") as i32; // 945
const ITEM_PUMPKIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:pumpkin") as i32; // 430
const ITEM_PUMPKIN_PIE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pumpkin_pie") as i32; // 1392
const ITEM_PURPLE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_bed") as i32; // 1229
const ITEM_PURPLE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_carpet") as i32; // 590
const ITEM_PURPLE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_dye") as i32; // 1209
const ITEM_PURPLE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_wool") as i32; // 260
const ITEM_RAIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:rail") as i32; // 947
const ITEM_RED_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_bed") as i32; // 1233
const ITEM_RED_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_carpet") as i32; // 594
const ITEM_RED_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_dye") as i32; // 1213
const ITEM_RED_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_wool") as i32; // 264
const ITEM_REDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:redstone") as i32; // 824
const ITEM_REDSTONE_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:redstone_block") as i32; // 826
const ITEM_REDSTONE_TORCH: i32 = registry_id(mc_registry::ITEMS, "minecraft:redstone_torch") as i32; // 825
const ITEM_SAND: i32 = registry_id(mc_registry::ITEMS, "minecraft:sand") as i32; // 88
const ITEM_SHIELD: i32 = registry_id(mc_registry::ITEMS, "minecraft:shield") as i32; // 1446
const ITEM_SPRUCE_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_boat") as i32; // 977
const ITEM_SPRUCE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_button") as i32; // 859
const ITEM_SPRUCE_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_door") as i32; // 890
const ITEM_SPRUCE_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_fence") as i32; // 418
const ITEM_SPRUCE_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_fence_gate") as i32; // 933
const ITEM_SPRUCE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_log") as i32; // 164
const ITEM_SPRUCE_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_planks") as i32; // 64
const ITEM_SPRUCE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_pressure_plate") as i32; // 876
const ITEM_SPRUCE_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_sign") as i32; // 1103
const ITEM_SPRUCE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_slab") as i32; // 342
const ITEM_SPRUCE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_stairs") as i32; // 516
const ITEM_SPRUCE_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_trapdoor") as i32; // 912
const ITEM_STONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone") as i32; // 1
const ITEM_STONE_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_axe") as i32; // 1038
const ITEM_STONE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_button") as i32; // 856
const ITEM_STONE_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_hoe") as i32; // 1039
const ITEM_STONE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_pressure_plate") as i32; // 871
const ITEM_STONE_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_shovel") as i32; // 1036
const ITEM_STONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_slab") as i32; // 355
const ITEM_STONE_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_sword") as i32; // 1035
const ITEM_STRING: i32 = registry_id(mc_registry::ITEMS, "minecraft:string") as i32; // 1062
const ITEM_STRIPPED_ACACIA_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_acacia_log") as i32; // 182
const ITEM_STRIPPED_BIRCH_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_birch_log") as i32; // 180
const ITEM_STRIPPED_CHERRY_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_cherry_log") as i32; // 183
const ITEM_STRIPPED_CRIMSON_STEM: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_crimson_stem") as i32; // 188
const ITEM_STRIPPED_DARK_OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_dark_oak_log") as i32; // 184
const ITEM_STRIPPED_JUNGLE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_jungle_log") as i32; // 181
const ITEM_STRIPPED_MANGROVE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_mangrove_log") as i32; // 186
const ITEM_STRIPPED_OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_oak_log") as i32; // 178
const ITEM_STRIPPED_SPRUCE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_spruce_log") as i32; // 179
const ITEM_STRIPPED_WARPED_STEM: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_warped_stem") as i32; // 189
const ITEM_SUGAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:sugar") as i32; // 1217
const ITEM_SUGAR_CANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:sugar_cane") as i32; // 327
const ITEM_TNT: i32 = registry_id(mc_registry::ITEMS, "minecraft:tnt") as i32; // 853
const ITEM_WARPED_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_door") as i32; // 901
const ITEM_WARPED_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_planks") as i32; // 75
const ITEM_WARPED_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_sign") as i32; // 1114
const ITEM_WARPED_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_slab") as i32; // 354
const ITEM_WARPED_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_stairs") as i32; // 528
const ITEM_WARPED_STEM: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_stem") as i32; // 176
const ITEM_WARPED_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_trapdoor") as i32; // 923
const ITEM_WHITE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_bed") as i32; // 1219
const ITEM_WHITE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_carpet") as i32; // 580
const ITEM_WHITE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_wool") as i32; // 250
const ITEM_WOODEN_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_axe") as i32; // 1028
const ITEM_WOODEN_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_hoe") as i32; // 1029
const ITEM_WOODEN_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_shovel") as i32; // 1026
const ITEM_WOODEN_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_sword") as i32; // 1025
const ITEM_YELLOW_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_bed") as i32; // 1223
const ITEM_YELLOW_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_carpet") as i32; // 584
const ITEM_YELLOW_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_dye") as i32; // 1203
const ITEM_YELLOW_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_wool") as i32; // 254

/// MVP 配方表（均为原版 2×2 可合成口径）：
/// 原木→4 木板；2 木板→4 木棍；4 木板→工作台；
/// 3 木板+2 木棍→木镐；3 圆石+2 木棍→石镐；3 小麦→面包；煤+木棍→4 火把。
pub const RECIPES: &[Recipe] = &[
    Recipe { needs: &[(OAK_LOG, 1)], result: (OAK_PLANKS, 4), name: "oak_planks" },
    Recipe { needs: &[(OAK_PLANKS, 2)], result: (STICK, 4), name: "stick" },
    Recipe { needs: &[(OAK_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "crafting_table" },
    Recipe { needs: &[(OAK_PLANKS, 3), (STICK, 2)], result: (WOODEN_PICKAXE, 1), name: "wooden_pickaxe" },
    Recipe { needs: &[(COBBLESTONE, 3), (STICK, 2)], result: (STONE_PICKAXE, 1), name: "stone_pickaxe" },
    Recipe { needs: &[(WHEAT, 3)], result: (BREAD, 1), name: "bread" },
    Recipe { needs: &[(COAL, 1), (STICK, 1)], result: (TORCH, 4), name: "torch" },
    Recipe { needs: &[(ITEM_STRIPPED_OAK_LOG, 1)], result: (OAK_PLANKS, 4), name: "stripped_oak_planks" },
    Recipe { needs: &[(OAK_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "oak_chest" },
    Recipe { needs: &[(OAK_PLANKS, 3)], result: (ITEM_OAK_SLAB, 6), name: "oak_slab" },
    Recipe { needs: &[(OAK_PLANKS, 4)], result: (ITEM_OAK_STAIRS, 4), name: "oak_stairs" },
    Recipe { needs: &[(OAK_PLANKS, 6)], result: (ITEM_OAK_DOOR, 3), name: "oak_door" },
    Recipe { needs: &[(OAK_PLANKS, 6)], result: (ITEM_OAK_TRAPDOOR, 2), name: "oak_trapdoor" },
    Recipe { needs: &[(OAK_PLANKS, 6), (STICK, 1)], result: (ITEM_OAK_SIGN, 3), name: "oak_sign" },
    Recipe { needs: &[(OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_OAK_FENCE, 3), name: "oak_fence" },
    Recipe { needs: &[(OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_OAK_FENCE_GATE, 1), name: "oak_fence_gate" },
    Recipe { needs: &[(OAK_PLANKS, 5)], result: (ITEM_OAK_BOAT, 1), name: "oak_boat" },
    Recipe { needs: &[(OAK_PLANKS, 2)], result: (ITEM_OAK_PRESSURE_PLATE, 1), name: "oak_pressure_plate" },
    Recipe { needs: &[(OAK_PLANKS, 1)], result: (ITEM_OAK_BUTTON, 1), name: "oak_button" },
    Recipe { needs: &[(ITEM_SPRUCE_LOG, 1)], result: (ITEM_SPRUCE_PLANKS, 4), name: "spruce_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_SPRUCE_LOG, 1)], result: (ITEM_SPRUCE_PLANKS, 4), name: "stripped_spruce_planks" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 2)], result: (STICK, 4), name: "spruce_stick" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "spruce_crafting_table" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "spruce_chest" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 3)], result: (ITEM_SPRUCE_SLAB, 6), name: "spruce_slab" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 4)], result: (ITEM_SPRUCE_STAIRS, 4), name: "spruce_stairs" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6)], result: (ITEM_SPRUCE_DOOR, 3), name: "spruce_door" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6)], result: (ITEM_SPRUCE_TRAPDOOR, 2), name: "spruce_trapdoor" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6), (STICK, 1)], result: (ITEM_SPRUCE_SIGN, 3), name: "spruce_sign" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 4), (STICK, 2)], result: (ITEM_SPRUCE_FENCE, 3), name: "spruce_fence" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 4), (STICK, 2)], result: (ITEM_SPRUCE_FENCE_GATE, 1), name: "spruce_fence_gate" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 5)], result: (ITEM_SPRUCE_BOAT, 1), name: "spruce_boat" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 2)], result: (ITEM_SPRUCE_PRESSURE_PLATE, 1), name: "spruce_pressure_plate" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 1)], result: (ITEM_SPRUCE_BUTTON, 1), name: "spruce_button" },
    Recipe { needs: &[(ITEM_BIRCH_LOG, 1)], result: (ITEM_BIRCH_PLANKS, 4), name: "birch_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_BIRCH_LOG, 1)], result: (ITEM_BIRCH_PLANKS, 4), name: "stripped_birch_planks" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 2)], result: (STICK, 4), name: "birch_stick" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "birch_crafting_table" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "birch_chest" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 3)], result: (ITEM_BIRCH_SLAB, 6), name: "birch_slab" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 4)], result: (ITEM_BIRCH_STAIRS, 4), name: "birch_stairs" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6)], result: (ITEM_BIRCH_DOOR, 3), name: "birch_door" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6)], result: (ITEM_BIRCH_TRAPDOOR, 2), name: "birch_trapdoor" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6), (STICK, 1)], result: (ITEM_BIRCH_SIGN, 3), name: "birch_sign" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 4), (STICK, 2)], result: (ITEM_BIRCH_FENCE, 3), name: "birch_fence" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 4), (STICK, 2)], result: (ITEM_BIRCH_FENCE_GATE, 1), name: "birch_fence_gate" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 5)], result: (ITEM_BIRCH_BOAT, 1), name: "birch_boat" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 2)], result: (ITEM_BIRCH_PRESSURE_PLATE, 1), name: "birch_pressure_plate" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 1)], result: (ITEM_BIRCH_BUTTON, 1), name: "birch_button" },
    Recipe { needs: &[(ITEM_JUNGLE_LOG, 1)], result: (ITEM_JUNGLE_PLANKS, 4), name: "jungle_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_JUNGLE_LOG, 1)], result: (ITEM_JUNGLE_PLANKS, 4), name: "stripped_jungle_planks" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 2)], result: (STICK, 4), name: "jungle_stick" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "jungle_crafting_table" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "jungle_chest" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 3)], result: (ITEM_JUNGLE_SLAB, 6), name: "jungle_slab" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 4)], result: (ITEM_JUNGLE_STAIRS, 4), name: "jungle_stairs" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6)], result: (ITEM_JUNGLE_DOOR, 3), name: "jungle_door" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6)], result: (ITEM_JUNGLE_TRAPDOOR, 2), name: "jungle_trapdoor" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6), (STICK, 1)], result: (ITEM_JUNGLE_SIGN, 3), name: "jungle_sign" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 4), (STICK, 2)], result: (ITEM_JUNGLE_FENCE, 3), name: "jungle_fence" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 4), (STICK, 2)], result: (ITEM_JUNGLE_FENCE_GATE, 1), name: "jungle_fence_gate" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 5)], result: (ITEM_JUNGLE_BOAT, 1), name: "jungle_boat" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 2)], result: (ITEM_JUNGLE_PRESSURE_PLATE, 1), name: "jungle_pressure_plate" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 1)], result: (ITEM_JUNGLE_BUTTON, 1), name: "jungle_button" },
    Recipe { needs: &[(ITEM_ACACIA_LOG, 1)], result: (ITEM_ACACIA_PLANKS, 4), name: "acacia_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_ACACIA_LOG, 1)], result: (ITEM_ACACIA_PLANKS, 4), name: "stripped_acacia_planks" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 2)], result: (STICK, 4), name: "acacia_stick" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "acacia_crafting_table" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "acacia_chest" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 3)], result: (ITEM_ACACIA_SLAB, 6), name: "acacia_slab" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 4)], result: (ITEM_ACACIA_STAIRS, 4), name: "acacia_stairs" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6)], result: (ITEM_ACACIA_DOOR, 3), name: "acacia_door" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6)], result: (ITEM_ACACIA_TRAPDOOR, 2), name: "acacia_trapdoor" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6), (STICK, 1)], result: (ITEM_ACACIA_SIGN, 3), name: "acacia_sign" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 4), (STICK, 2)], result: (ITEM_ACACIA_FENCE, 3), name: "acacia_fence" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 4), (STICK, 2)], result: (ITEM_ACACIA_FENCE_GATE, 1), name: "acacia_fence_gate" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 5)], result: (ITEM_ACACIA_BOAT, 1), name: "acacia_boat" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 2)], result: (ITEM_ACACIA_PRESSURE_PLATE, 1), name: "acacia_pressure_plate" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 1)], result: (ITEM_ACACIA_BUTTON, 1), name: "acacia_button" },
    Recipe { needs: &[(ITEM_DARK_OAK_LOG, 1)], result: (ITEM_DARK_OAK_PLANKS, 4), name: "dark_oak_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_DARK_OAK_LOG, 1)], result: (ITEM_DARK_OAK_PLANKS, 4), name: "stripped_dark_oak_planks" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 2)], result: (STICK, 4), name: "dark_oak_stick" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "dark_oak_crafting_table" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "dark_oak_chest" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 3)], result: (ITEM_DARK_OAK_SLAB, 6), name: "dark_oak_slab" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 4)], result: (ITEM_DARK_OAK_STAIRS, 4), name: "dark_oak_stairs" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6)], result: (ITEM_DARK_OAK_DOOR, 3), name: "dark_oak_door" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6)], result: (ITEM_DARK_OAK_TRAPDOOR, 2), name: "dark_oak_trapdoor" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6), (STICK, 1)], result: (ITEM_DARK_OAK_SIGN, 3), name: "dark_oak_sign" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_DARK_OAK_FENCE, 3), name: "dark_oak_fence" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_DARK_OAK_FENCE_GATE, 1), name: "dark_oak_fence_gate" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 5)], result: (ITEM_DARK_OAK_BOAT, 1), name: "dark_oak_boat" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 2)], result: (ITEM_DARK_OAK_PRESSURE_PLATE, 1), name: "dark_oak_pressure_plate" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 1)], result: (ITEM_DARK_OAK_BUTTON, 1), name: "dark_oak_button" },
    Recipe { needs: &[(ITEM_CHERRY_LOG, 1)], result: (ITEM_CHERRY_PLANKS, 4), name: "cherry_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_CHERRY_LOG, 1)], result: (ITEM_CHERRY_PLANKS, 4), name: "stripped_cherry_planks" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 2)], result: (STICK, 4), name: "cherry_stick" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "cherry_crafting_table" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "cherry_chest" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 3)], result: (ITEM_CHERRY_SLAB, 6), name: "cherry_slab" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 4)], result: (ITEM_CHERRY_STAIRS, 4), name: "cherry_stairs" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6)], result: (ITEM_CHERRY_DOOR, 3), name: "cherry_door" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6)], result: (ITEM_CHERRY_TRAPDOOR, 2), name: "cherry_trapdoor" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6), (STICK, 1)], result: (ITEM_CHERRY_SIGN, 3), name: "cherry_sign" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 4), (STICK, 2)], result: (ITEM_CHERRY_FENCE, 3), name: "cherry_fence" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 4), (STICK, 2)], result: (ITEM_CHERRY_FENCE_GATE, 1), name: "cherry_fence_gate" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 5)], result: (ITEM_CHERRY_BOAT, 1), name: "cherry_boat" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 2)], result: (ITEM_CHERRY_PRESSURE_PLATE, 1), name: "cherry_pressure_plate" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 1)], result: (ITEM_CHERRY_BUTTON, 1), name: "cherry_button" },
    Recipe { needs: &[(ITEM_MANGROVE_LOG, 1)], result: (ITEM_MANGROVE_PLANKS, 4), name: "mangrove_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_MANGROVE_LOG, 1)], result: (ITEM_MANGROVE_PLANKS, 4), name: "stripped_mangrove_planks" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 2)], result: (STICK, 4), name: "mangrove_stick" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "mangrove_crafting_table" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "mangrove_chest" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 3)], result: (ITEM_MANGROVE_SLAB, 6), name: "mangrove_slab" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 4)], result: (ITEM_MANGROVE_STAIRS, 4), name: "mangrove_stairs" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6)], result: (ITEM_MANGROVE_DOOR, 3), name: "mangrove_door" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6)], result: (ITEM_MANGROVE_TRAPDOOR, 2), name: "mangrove_trapdoor" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6), (STICK, 1)], result: (ITEM_MANGROVE_SIGN, 3), name: "mangrove_sign" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 4), (STICK, 2)], result: (ITEM_MANGROVE_FENCE, 3), name: "mangrove_fence" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 4), (STICK, 2)], result: (ITEM_MANGROVE_FENCE_GATE, 1), name: "mangrove_fence_gate" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 5)], result: (ITEM_MANGROVE_BOAT, 1), name: "mangrove_boat" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 2)], result: (ITEM_MANGROVE_PRESSURE_PLATE, 1), name: "mangrove_pressure_plate" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 1)], result: (ITEM_MANGROVE_BUTTON, 1), name: "mangrove_button" },
    Recipe { needs: &[(ITEM_CRIMSON_STEM, 1)], result: (ITEM_CRIMSON_PLANKS, 4), name: "crimson_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_CRIMSON_STEM, 1)], result: (ITEM_CRIMSON_PLANKS, 4), name: "stripped_crimson_planks" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 2)], result: (STICK, 4), name: "crimson_stick" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "crimson_crafting_table" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "crimson_chest" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 3)], result: (ITEM_CRIMSON_SLAB, 6), name: "crimson_slab" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 4)], result: (ITEM_CRIMSON_STAIRS, 4), name: "crimson_stairs" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6)], result: (ITEM_CRIMSON_DOOR, 3), name: "crimson_door" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6)], result: (ITEM_CRIMSON_TRAPDOOR, 2), name: "crimson_trapdoor" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6), (STICK, 1)], result: (ITEM_CRIMSON_SIGN, 3), name: "crimson_sign" },
    Recipe { needs: &[(ITEM_WARPED_STEM, 1)], result: (ITEM_WARPED_PLANKS, 4), name: "warped_planks" },
    Recipe { needs: &[(ITEM_STRIPPED_WARPED_STEM, 1)], result: (ITEM_WARPED_PLANKS, 4), name: "stripped_warped_planks" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 2)], result: (STICK, 4), name: "warped_stick" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "warped_crafting_table" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "warped_chest" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 3)], result: (ITEM_WARPED_SLAB, 6), name: "warped_slab" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 4)], result: (ITEM_WARPED_STAIRS, 4), name: "warped_stairs" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_DOOR, 3), name: "warped_door" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_TRAPDOOR, 2), name: "warped_trapdoor" },
    Recipe { needs: &[(STICK, 1), (ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_SIGN, 3), name: "warped_sign" },
    Recipe { needs: &[(OAK_PLANKS, 2), (ITEM_OAK_SLAB, 2)], result: (ITEM_BARREL, 1), name: "barrel" },
    Recipe { needs: &[(OAK_PLANKS, 3), (STICK, 2)], result: (ITEM_WOODEN_AXE, 1), name: "wooden_axe" },
    Recipe { needs: &[(OAK_PLANKS, 1), (STICK, 2)], result: (ITEM_WOODEN_SHOVEL, 1), name: "wooden_shovel" },
    Recipe { needs: &[(OAK_PLANKS, 2), (STICK, 2)], result: (ITEM_WOODEN_HOE, 1), name: "wooden_hoe" },
    Recipe { needs: &[(OAK_PLANKS, 2), (STICK, 1)], result: (ITEM_WOODEN_SWORD, 1), name: "wooden_sword" },
    Recipe { needs: &[(COBBLESTONE, 3), (STICK, 2)], result: (ITEM_STONE_AXE, 1), name: "stone_axe" },
    Recipe { needs: &[(COBBLESTONE, 1), (STICK, 2)], result: (ITEM_STONE_SHOVEL, 1), name: "stone_shovel" },
    Recipe { needs: &[(COBBLESTONE, 2), (STICK, 2)], result: (ITEM_STONE_HOE, 1), name: "stone_hoe" },
    Recipe { needs: &[(COBBLESTONE, 2), (STICK, 1)], result: (ITEM_STONE_SWORD, 1), name: "stone_sword" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 3), (STICK, 2)], result: (ITEM_COPPER_PICKAXE, 1), name: "copper_pickaxe" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 3), (STICK, 2)], result: (ITEM_COPPER_AXE, 1), name: "copper_axe" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 1), (STICK, 2)], result: (ITEM_COPPER_SHOVEL, 1), name: "copper_shovel" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 2), (STICK, 2)], result: (ITEM_COPPER_HOE, 1), name: "copper_hoe" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 2), (STICK, 1)], result: (ITEM_COPPER_SWORD, 1), name: "copper_sword" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 3), (STICK, 2)], result: (ITEM_IRON_PICKAXE, 1), name: "iron_pickaxe" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 3), (STICK, 2)], result: (ITEM_IRON_AXE, 1), name: "iron_axe" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (STICK, 2)], result: (ITEM_IRON_SHOVEL, 1), name: "iron_shovel" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 2), (STICK, 2)], result: (ITEM_IRON_HOE, 1), name: "iron_hoe" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 2), (STICK, 1)], result: (ITEM_IRON_SWORD, 1), name: "iron_sword" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 3), (STICK, 2)], result: (ITEM_GOLDEN_PICKAXE, 1), name: "golden_pickaxe" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 3), (STICK, 2)], result: (ITEM_GOLDEN_AXE, 1), name: "golden_axe" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 1), (STICK, 2)], result: (ITEM_GOLDEN_SHOVEL, 1), name: "golden_shovel" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 2), (STICK, 2)], result: (ITEM_GOLDEN_HOE, 1), name: "golden_hoe" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 2), (STICK, 1)], result: (ITEM_GOLDEN_SWORD, 1), name: "golden_sword" },
    Recipe { needs: &[(ITEM_DIAMOND, 3), (STICK, 2)], result: (ITEM_DIAMOND_PICKAXE, 1), name: "diamond_pickaxe" },
    Recipe { needs: &[(ITEM_DIAMOND, 3), (STICK, 2)], result: (ITEM_DIAMOND_AXE, 1), name: "diamond_axe" },
    Recipe { needs: &[(ITEM_DIAMOND, 1), (STICK, 2)], result: (ITEM_DIAMOND_SHOVEL, 1), name: "diamond_shovel" },
    Recipe { needs: &[(ITEM_DIAMOND, 2), (STICK, 2)], result: (ITEM_DIAMOND_HOE, 1), name: "diamond_hoe" },
    Recipe { needs: &[(ITEM_DIAMOND, 2), (STICK, 1)], result: (ITEM_DIAMOND_SWORD, 1), name: "diamond_sword" },
    Recipe { needs: &[(ITEM_NETHERITE_INGOT, 3), (STICK, 2)], result: (ITEM_NETHERITE_PICKAXE, 1), name: "netherite_pickaxe" },
    Recipe { needs: &[(ITEM_NETHERITE_INGOT, 3), (STICK, 2)], result: (ITEM_NETHERITE_AXE, 1), name: "netherite_axe" },
    Recipe { needs: &[(ITEM_NETHERITE_INGOT, 1), (STICK, 2)], result: (ITEM_NETHERITE_SHOVEL, 1), name: "netherite_shovel" },
    Recipe { needs: &[(ITEM_NETHERITE_INGOT, 2), (STICK, 2)], result: (ITEM_NETHERITE_HOE, 1), name: "netherite_hoe" },
    Recipe { needs: &[(ITEM_NETHERITE_INGOT, 2), (STICK, 1)], result: (ITEM_NETHERITE_SWORD, 1), name: "netherite_sword" },
    Recipe { needs: &[(ITEM_LEATHER, 5)], result: (ITEM_LEATHER_HELMET, 1), name: "leather_helmet" },
    Recipe { needs: &[(ITEM_LEATHER, 8)], result: (ITEM_LEATHER_CHESTPLATE, 1), name: "leather_chestplate" },
    Recipe { needs: &[(ITEM_LEATHER, 7)], result: (ITEM_LEATHER_LEGGINGS, 1), name: "leather_leggings" },
    Recipe { needs: &[(ITEM_LEATHER, 4)], result: (ITEM_LEATHER_BOOTS, 1), name: "leather_boots" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 5)], result: (ITEM_IRON_HELMET, 1), name: "iron_helmet" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 8)], result: (ITEM_IRON_CHESTPLATE, 1), name: "iron_chestplate" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 7)], result: (ITEM_IRON_LEGGINGS, 1), name: "iron_leggings" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 4)], result: (ITEM_IRON_BOOTS, 1), name: "iron_boots" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 5)], result: (ITEM_GOLDEN_HELMET, 1), name: "golden_helmet" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 8)], result: (ITEM_GOLDEN_CHESTPLATE, 1), name: "golden_chestplate" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 7)], result: (ITEM_GOLDEN_LEGGINGS, 1), name: "golden_leggings" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 4)], result: (ITEM_GOLDEN_BOOTS, 1), name: "golden_boots" },
    Recipe { needs: &[(ITEM_DIAMOND, 5)], result: (ITEM_DIAMOND_HELMET, 1), name: "diamond_helmet" },
    Recipe { needs: &[(ITEM_DIAMOND, 8)], result: (ITEM_DIAMOND_CHESTPLATE, 1), name: "diamond_chestplate" },
    Recipe { needs: &[(ITEM_DIAMOND, 7)], result: (ITEM_DIAMOND_LEGGINGS, 1), name: "diamond_leggings" },
    Recipe { needs: &[(ITEM_DIAMOND, 4)], result: (ITEM_DIAMOND_BOOTS, 1), name: "diamond_boots" },
    Recipe { needs: &[(COBBLESTONE, 8)], result: (ITEM_FURNACE, 1), name: "furnace" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_IRON_INGOT, 5)], result: (ITEM_HOPPER, 1), name: "hopper" },
    Recipe { needs: &[(COBBLESTONE, 3), (ITEM_IRON_INGOT, 1), (OAK_PLANKS, 4)], result: (ITEM_PISTON, 1), name: "piston" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (STICK, 1)], result: (ITEM_RAIL, 16), name: "rail" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (ITEM_REDSTONE, 1), (STICK, 1)], result: (ITEM_POWERED_RAIL, 6), name: "powered_rail" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (ITEM_REDSTONE, 1), (ITEM_STONE_PRESSURE_PLATE, 1)], result: (ITEM_DETECTOR_RAIL, 6), name: "detector_rail" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (STICK, 2)], result: (ITEM_ACTIVATOR_RAIL, 6), name: "activator_rail" },
    Recipe { needs: &[(COBBLESTONE, 1), (STICK, 1)], result: (ITEM_LEVER, 1), name: "lever" },
    Recipe { needs: &[(STICK, 7)], result: (ITEM_LADDER, 3), name: "ladder" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 3)], result: (ITEM_BUCKET, 1), name: "bucket" },
    Recipe { needs: &[(ITEM_FLINT, 1), (ITEM_IRON_INGOT, 1)], result: (ITEM_FLINT_AND_STEEL, 1), name: "flint_and_steel" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 4), (ITEM_REDSTONE, 1)], result: (ITEM_COMPASS, 1), name: "compass" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 4), (ITEM_REDSTONE, 1)], result: (ITEM_CLOCK, 1), name: "clock" },
    Recipe { needs: &[(ITEM_PAPER, 8)], result: (ITEM_MAP, 1), name: "map" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 7)], result: (ITEM_CAULDRON, 1), name: "cauldron" },
    Recipe { needs: &[(ITEM_IRON_BLOCK, 3), (ITEM_IRON_INGOT, 4)], result: (ITEM_ANVIL, 1), name: "anvil" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (OAK_PLANKS, 6)], result: (ITEM_SHIELD, 1), name: "shield" },
    Recipe { needs: &[(ITEM_BOOK, 3), (OAK_PLANKS, 6)], result: (ITEM_BOOKSHELF, 1), name: "bookshelf" },
    Recipe { needs: &[(ITEM_LEATHER, 1), (ITEM_PAPER, 3)], result: (ITEM_BOOK, 1), name: "book" },
    Recipe { needs: &[(ITEM_SUGAR_CANE, 3)], result: (ITEM_PAPER, 3), name: "paper" },
    Recipe { needs: &[(WHEAT, 9)], result: (ITEM_HAY_BLOCK, 1), name: "hay_block" },
    Recipe { needs: &[(ITEM_REDSTONE, 1), (STICK, 1)], result: (ITEM_REDSTONE_TORCH, 2), name: "redstone_torch" },
    Recipe { needs: &[(COBBLESTONE, 3)], result: (ITEM_STONE_SLAB, 6), name: "stone_slab" },
    Recipe { needs: &[(COBBLESTONE, 6)], result: (ITEM_COBBLESTONE_WALL, 6), name: "cobblestone_wall" },
    Recipe { needs: &[(ITEM_STONE, 1)], result: (ITEM_STONE_BUTTON, 1), name: "stone_button" },
    Recipe { needs: &[(ITEM_STONE, 2)], result: (ITEM_STONE_PRESSURE_PLATE, 1), name: "stone_pressure_plate" },
    Recipe { needs: &[(ITEM_STRING, 4)], result: (ITEM_WHITE_WOOL, 1), name: "white_wool_from_string" },
    Recipe { needs: &[(OAK_PLANKS, 3), (ITEM_WHITE_WOOL, 3)], result: (ITEM_WHITE_BED, 1), name: "white_bed" },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 2)], result: (ITEM_WHITE_CARPET, 3), name: "white_carpet" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_RED_WOOL, 1), name: "red_wool" },
    Recipe { needs: &[(OAK_PLANKS, 3), (ITEM_RED_WOOL, 3)], result: (ITEM_RED_BED, 1), name: "red_bed" },
    Recipe { needs: &[(ITEM_RED_WOOL, 2)], result: (ITEM_RED_CARPET, 3), name: "red_carpet" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_BLUE_WOOL, 1), name: "blue_wool" },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_BLUE_BED, 1), name: "blue_bed" },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 2)], result: (ITEM_BLUE_CARPET, 3), name: "blue_carpet" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_GREEN_WOOL, 1), name: "green_wool" },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_GREEN_BED, 1), name: "green_bed" },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 2)], result: (ITEM_GREEN_CARPET, 3), name: "green_carpet" },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 1), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_WOOL, 1), name: "yellow_wool" },
    Recipe { needs: &[(OAK_PLANKS, 3), (ITEM_YELLOW_WOOL, 3)], result: (ITEM_YELLOW_BED, 1), name: "yellow_bed" },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 2)], result: (ITEM_YELLOW_CARPET, 3), name: "yellow_carpet" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_BLACK_WOOL, 1), name: "black_wool" },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_BLACK_BED, 1), name: "black_bed" },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 2)], result: (ITEM_BLACK_CARPET, 3), name: "black_carpet" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_ORANGE_WOOL, 1), name: "orange_wool" },
    Recipe { needs: &[(OAK_PLANKS, 3), (ITEM_ORANGE_WOOL, 3)], result: (ITEM_ORANGE_BED, 1), name: "orange_bed" },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 2)], result: (ITEM_ORANGE_CARPET, 3), name: "orange_carpet" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_LIGHT_BLUE_WOOL, 1), name: "light_blue_wool" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_LIGHT_BLUE_BED, 1), name: "light_blue_bed" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 2)], result: (ITEM_LIGHT_BLUE_CARPET, 3), name: "light_blue_carpet" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_LIME_WOOL, 1), name: "lime_wool" },
    Recipe { needs: &[(ITEM_LIME_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_LIME_BED, 1), name: "lime_bed" },
    Recipe { needs: &[(ITEM_LIME_WOOL, 2)], result: (ITEM_LIME_CARPET, 3), name: "lime_carpet" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_CYAN_WOOL, 1), name: "cyan_wool" },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_CYAN_BED, 1), name: "cyan_bed" },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 2)], result: (ITEM_CYAN_CARPET, 3), name: "cyan_carpet" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_PURPLE_WOOL, 1), name: "purple_wool" },
    Recipe { needs: &[(OAK_PLANKS, 3), (ITEM_PURPLE_WOOL, 3)], result: (ITEM_PURPLE_BED, 1), name: "purple_bed" },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 2)], result: (ITEM_PURPLE_CARPET, 3), name: "purple_carpet" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_PINK_WOOL, 1), name: "pink_wool" },
    Recipe { needs: &[(OAK_PLANKS, 3), (ITEM_PINK_WOOL, 3)], result: (ITEM_PINK_BED, 1), name: "pink_bed" },
    Recipe { needs: &[(ITEM_PINK_WOOL, 2)], result: (ITEM_PINK_CARPET, 3), name: "pink_carpet" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_GRAY_WOOL, 1), name: "gray_wool" },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_GRAY_BED, 1), name: "gray_bed" },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 2)], result: (ITEM_GRAY_CARPET, 3), name: "gray_carpet" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_LIGHT_GRAY_WOOL, 1), name: "light_gray_wool" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_LIGHT_GRAY_BED, 1), name: "light_gray_bed" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 2)], result: (ITEM_LIGHT_GRAY_CARPET, 3), name: "light_gray_carpet" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_MAGENTA_WOOL, 1), name: "magenta_wool" },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_MAGENTA_BED, 1), name: "magenta_bed" },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 2)], result: (ITEM_MAGENTA_CARPET, 3), name: "magenta_carpet" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_WHITE_WOOL, 1)], result: (ITEM_BROWN_WOOL, 1), name: "brown_wool" },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 3), (OAK_PLANKS, 3)], result: (ITEM_BROWN_BED, 1), name: "brown_bed" },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 2)], result: (ITEM_BROWN_CARPET, 3), name: "brown_carpet" },
    Recipe { needs: &[(ITEM_EGG, 1), (ITEM_SUGAR, 2), (WHEAT, 3)], result: (ITEM_CAKE, 1), name: "cake" },
    Recipe { needs: &[(ITEM_COCOA_BEANS, 1), (WHEAT, 2)], result: (ITEM_COOKIE, 8), name: "cookie" },
    Recipe { needs: &[(ITEM_EGG, 1), (ITEM_PUMPKIN, 1), (ITEM_SUGAR, 1)], result: (ITEM_PUMPKIN_PIE, 1), name: "pumpkin_pie" },
    Recipe { needs: &[(ITEM_FEATHER, 1), (ITEM_FLINT, 1), (STICK, 1)], result: (ITEM_ARROW, 4), name: "arrow" },
    Recipe { needs: &[(STICK, 3), (ITEM_STRING, 3)], result: (ITEM_BOW, 1), name: "bow" },
    Recipe { needs: &[(STICK, 3), (ITEM_STRING, 2)], result: (ITEM_FISHING_ROD, 1), name: "fishing_rod" },
    Recipe { needs: &[(ITEM_GUNPOWDER, 4), (ITEM_SAND, 4)], result: (ITEM_TNT, 1), name: "tnt" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 4), (ITEM_NETHERITE_SCRAP, 4)], result: (ITEM_NETHERITE_INGOT, 1), name: "netherite_ingot" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 9)], result: (ITEM_IRON_BLOCK, 1), name: "iron_block" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 9)], result: (ITEM_GOLD_BLOCK, 1), name: "gold_block" },
    Recipe { needs: &[(ITEM_DIAMOND, 9)], result: (ITEM_DIAMOND_BLOCK, 1), name: "diamond_block" },
    Recipe { needs: &[(ITEM_EMERALD, 9)], result: (ITEM_EMERALD_BLOCK, 1), name: "emerald_block" },
    Recipe { needs: &[(ITEM_NETHERITE_INGOT, 9)], result: (ITEM_NETHERITE_BLOCK, 1), name: "netherite_block" },
    Recipe { needs: &[(COAL, 9)], result: (ITEM_COAL_BLOCK, 1), name: "coal_block" },
    Recipe { needs: &[(ITEM_REDSTONE, 9)], result: (ITEM_REDSTONE_BLOCK, 1), name: "redstone_block" },
];

// ---------------------------------------------------------------------------
// P47：3×3 工作台合成（与背包 2×2 完全隔离，GRID_SIZE/match_recipe/consume_grid 不动）。
// 槽语义：窗口 1 = 工作台；槽 0 = 结果，槽 1..=9 = 合成格，10..=45 = 玩家背包。
// 复用同一张 283 条配方表，只按多重集匹配（工作台专属形状/歧义槽型不在 MVP 范围）。
// ---------------------------------------------------------------------------

/// 工作台合成格数量（槽 1..=9）。
pub const GRID9_SIZE: usize = 9;
/// 工作台窗口 ID（玩家背包窗口仍是 0；非 0 的点击只在工作台会话里处理）。
pub const CRAFT9_WINDOW_ID: i32 = 1;
/// 工作台结果槽。
pub const CRAFT9_RESULT_SLOT: i16 = 0;

/// 3×3 配方匹配：与 2×2 共用配方表/多重集口径，但不套用 2×2 的槽分布指纹
/// （那是为 4 格歧义组准备的）；多条候选按表序取首条，确定性且与 HashMap 无关。
pub fn match_recipe_9(grid: [Option<(i32, i32)>; GRID9_SIZE]) -> Option<&'static Recipe> {
    let input = normalized_multiset(&grid_stacks9(grid));
    RECIPES.iter().find(|r| normalized_multiset(r.needs) == input)
}

/// 3×3 合成格展平。
pub fn grid_stacks9(grid: [Option<(i32, i32)>; GRID9_SIZE]) -> Vec<(i32, i32)> {
    grid.into_iter().flatten().collect()
}

/// 按 ID 从 3×3 格扣除原料（与 `consume_grid` 同语义，返回是否足够）。
pub fn consume_grid9(grid: &mut [Option<(i32, i32)>; GRID9_SIZE], recipe: &Recipe) -> bool {
    for (id, need) in recipe.needs {
        let mut left = *need;
        for slot in grid.iter_mut() {
            if left <= 0 {
                break;
            }
            if let Some((sid, count)) = slot {
                if *sid != *id {
                    continue;
                }
                let take = (*count).min(left);
                *count -= take;
                left -= take;
                if *count <= 0 {
                    *slot = None;
                }
            }
        }
        if left > 0 {
            return false;
        }
    }
    true
}

/// 把合成格展平成 `(id, count)` 列表（跳过空槽；未合并，匹配时按 ID 累计）。
pub fn grid_stacks(grid: [Option<(i32, i32)>; GRID_SIZE]) -> Vec<(i32, i32)> {
    grid.into_iter().flatten().collect()
}

/// 把物品列表按 ID 聚合、排序成规范多重集（跳过空/负数数量，0 视为不存在）。
fn normalized_multiset(items: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::with_capacity(items.len());
    for &(id, count) in items {
        if count <= 0 {
            continue;
        }
        match out.iter_mut().find(|(i, _)| *i == id) {
            Some(entry) => entry.1 += count,
            None => out.push((id, count)),
        }
    }
    out.sort_unstable();
    out
}

/// 槽分布指纹：按 (id,count) 排序的占用槽列表，位置无关（4 格内）。
/// 只在“多条配方输入多重集完全相同”的歧义组里用作确定性拆分键。
fn slot_fingerprint(grid: [Option<(i32, i32)>; GRID_SIZE]) -> Vec<(i32, i32)> {
    let mut slots: Vec<(i32, i32)> = grid.into_iter().flatten().filter(|(_, c)| *c > 0).collect();
    slots.sort_unstable();
    slots
}

/// 歧义组的规范槽型（按 (id,count) 排序后比较）。
///
/// 原版里下列配方恰好消耗相同的物品集合，但形状不同；本服既有的“无序精确匹配”
/// 不检查形状，因此用规范槽型做确定性拆分：
/// - 木棍/压力板：2 木板分两格(1+1) → 4 木棍；2 木板放单格 → 压力板（旧 2 木板→木棍
///   测试使用两格，故保留这一口径）。
/// - 工作台/楼梯：4 木板单格 → 工作台；4 木板拆成 2+2 → 楼梯。
/// - 镐/斧：材料 3 单格 + 木棍 2 单格 → 镐；材料 2+1 + 木棍 1+1（占满 4 格）→ 斧。
/// - 门/活板门：6 木板拆 3+3 → 门；拆 2+2+2 → 活板门。
/// - 栅栏/栅栏门：木板 2+2 + 木棍 1+1 → 栅栏；木板 2+1+1 + 木棍 2 单格 → 栅栏门。
/// - 铁护腿/炼药锅：7 铁锭拆 3+2+2 → 护腿；7 铁锭单格 → 炼药锅。
fn canonical_layout(recipe: &Recipe) -> Option<Vec<(i32, i32)>> {
    let (mat, n) = *recipe.needs.first()?;
    let mut layout = if recipe.name == "stick" || recipe.name.ends_with("_stick") {
        vec![(mat, 1), (mat, 1)]
    } else if recipe.name == "crafting_table" || recipe.name.ends_with("_crafting_table") {
        vec![(mat, 4)]
    } else if recipe.name.ends_with("_pressure_plate") {
        vec![(mat, n)]
    } else if recipe.name.ends_with("_stairs") {
        vec![(mat, n / 2), (mat, n - n / 2)]
    } else if recipe.name.ends_with("_pickaxe") {
        vec![(mat, 3), (STICK, 2)]
    } else if recipe.name.ends_with("_axe") {
        vec![(mat, 2), (mat, 1), (STICK, 1), (STICK, 1)]
    } else if recipe.name.ends_with("_door") {
        vec![(mat, 3), (mat, 3)]
    } else if recipe.name.ends_with("_trapdoor") {
        vec![(mat, 2), (mat, 2), (mat, 2)]
    } else if recipe.name.ends_with("_fence_gate") {
        vec![(mat, 2), (mat, 1), (mat, 1), (STICK, 2)]
    } else if recipe.name.ends_with("_fence") {
        vec![(mat, 2), (mat, 2), (STICK, 1), (STICK, 1)]
    } else if recipe.name == "cauldron" {
        vec![(mat, 7)]
    } else if recipe.name.ends_with("_leggings") {
        vec![(mat, 3), (mat, 2), (mat, 2)]
    } else {
        return None;
    };
    layout.sort_unstable();
    Some(layout)
}

/// 无序精确匹配：返回命中的配方；格内多/少任意一件都视为无配方（结果槽保持空）。
/// 输入多重集相同且存在多条配方时，按 [`canonical_layout`] 的槽型确定性拆分；
/// 若槽型未命中任何规范型（例如原料全塞在一格），取配方表中的首条（表序稳定），
/// 绝不随机、也不按 HashMap 迭代顺序出结果。
pub fn match_recipe(grid: [Option<(i32, i32)>; GRID_SIZE]) -> Option<&'static Recipe> {
    let input = normalized_multiset(&grid_stacks(grid));
    let candidates: Vec<&'static Recipe> =
        RECIPES.iter().filter(|r| normalized_multiset(r.needs) == input).collect();
    let first = *candidates.first()?;
    if candidates.len() == 1 {
        return Some(first);
    }
    let fingerprint = slot_fingerprint(grid);
    for recipe in &candidates {
        if canonical_layout(recipe).as_deref() == Some(fingerprint.as_slice()) {
            return Some(recipe);
        }
    }
    Some(first)
}
/// 配方命中后按 ID 从合成格扣除原料（每格最多取自己数量；返回是否全部扣成功）。
/// 调用方应先 `match_recipe`；不匹配时不要调用，避免误扣。
pub fn consume_grid(grid: &mut [Option<(i32, i32)>; GRID_SIZE], recipe: &Recipe) -> bool {
    for (id, need) in recipe.needs {
        let mut left = *need;
        for slot in grid.iter_mut() {
            if left <= 0 {
                break;
            }
            if let Some((sid, count)) = slot {
                if *sid != *id {
                    continue;
                }
                let take = (*count).min(left);
                *count -= take;
                left -= take;
                if *count <= 0 {
                    *slot = None;
                }
            }
        }
        if left > 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipe_item_ids_come_from_generated_table() {
        for (name, expected, id) in [
            ("minecraft:oak_log", 163, OAK_LOG),
            ("minecraft:oak_planks", 63, OAK_PLANKS),
            ("minecraft:stick", 1060, STICK),
            ("minecraft:crafting_table", 405, CRAFTING_TABLE),
            ("minecraft:cobblestone", 62, COBBLESTONE),
            ("minecraft:wooden_pickaxe", 1027, WOODEN_PICKAXE),
            ("minecraft:stone_pickaxe", 1037, STONE_PICKAXE),
            ("minecraft:wheat", 1066, WHEAT),
            ("minecraft:bread", 1067, BREAD),
            ("minecraft:coal", 1010, COAL),
            ("minecraft:torch", 395, TORCH),
        ] {
            assert_eq!(id as u32, expected, "wrong item ID for {name}");
            assert_eq!(mc_registry::item_id(name), Some(expected));
        }
    }

    #[test]
    fn match_and_consume_basic_recipes() {
        let mut grid = [None, Some((OAK_LOG, 1)), None, None];
        let r = match_recipe(grid).expect("one log must make planks");
        assert_eq!(r.result, (OAK_PLANKS, 4));
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [None; GRID_SIZE]);

        // 2 木板 → 4 木棍（无序，位置无关）。
        let grid = [Some((OAK_PLANKS, 1)), None, None, Some((OAK_PLANKS, 1))];
        assert_eq!(match_recipe(grid).unwrap().result, (STICK, 4));

        // 3 木板 + 2 木棍 → 木镐（堆叠也算数量）。
        let grid = [Some((OAK_PLANKS, 3)), Some((STICK, 2)), None, None];
        assert_eq!(match_recipe(grid).unwrap().result, (WOODEN_PICKAXE, 1));

        // 3 小麦 → 面包；煤 + 木棍 → 火把。
        assert_eq!(match_recipe([Some((WHEAT, 3)), None, None, None]).unwrap().result, (BREAD, 1));
        assert_eq!(match_recipe([Some((COAL, 1)), Some((STICK, 1)), None, None]).unwrap().result, (TORCH, 4));
    }

    #[test]
    fn extra_or_missing_items_never_match() {
        assert!(match_recipe([None; GRID_SIZE]).is_none());
        assert!(match_recipe([Some((OAK_LOG, 2)), None, None, None]).is_none());
        // 多一个无关物品 → 原木配方不成立（避免白送木板）。
        assert!(match_recipe([Some((OAK_LOG, 1)), Some((STICK, 1)), None, None]).is_none());
        // 少一件原料 → 石镐不成立。
        assert!(match_recipe([Some((COBBLESTONE, 3)), Some((STICK, 1)), None, None]).is_none());
    }

    #[test]
    fn consume_grid_takes_only_matching_stacks() {
        let mut grid = [Some((OAK_PLANKS, 3)), Some((STICK, 2)), None, None];
        let r = match_recipe(grid).unwrap();
        assert_eq!(r.name, "wooden_pickaxe");
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [None; GRID_SIZE]);
    }

    // ---- P41：扩充配方的回归测试（下面 4 个旧测试保持原样） ----

    /// 每条配方（含旧 7 条）的每个输入/输出 ID 都必须可在生成表中解析；
    /// 同时锁定若干关键 ID 的实测值，防 codegen 漂移。
    #[test]
    fn p47_grid9_uses_same_recipe_table_without_touching_2x2() {
        assert_eq!(GRID_SIZE, 4, "2×2 slot semantics must stay intact");
        assert_eq!(CRAFT9_WINDOW_ID, 1);
        // 3×3 的 9 个小麦槽 → 干草捆（同一张配方表）。
        let mut g9 = [None; GRID9_SIZE];
        for slot in g9.iter_mut() {
            *slot = Some((WHEAT, 1));
        }
        let r = match_recipe_9(g9).expect("9 wheat must match hay_block");
        assert_eq!(r.name, "hay_block");
        assert_eq!(r.result, (ITEM_HAY_BLOCK, 1));
        assert!(consume_grid9(&mut g9, r));
        assert!(g9.iter().all(|s| s.is_none()));
        assert!(match_recipe_9(g9).is_none(), "empty grid has no recipe");
        // 同一配方在 2×2（9 个塞一格）仍可命中，旧口径不变。
        let mut g4 = [None; GRID_SIZE];
        g4[0] = Some((WHEAT, 9));
        assert_eq!(match_recipe(g4).unwrap().name, "hay_block");
        // 工作台不存在 10 件小麦的配方：不多不少才算命中。
        let mut too_many = [None; GRID9_SIZE];
        for slot in too_many.iter_mut() {
            *slot = Some((WHEAT, 2));
        }
        assert!(match_recipe_9(too_many).is_none());
    }

    #[test]
    fn p41_all_recipe_ids_resolve_in_generated_table() {
        let mut seen = std::collections::BTreeSet::new();
        assert_eq!(RECIPES.len(), 283, "P41 配方总数（7 旧 + 276 新）");
        for recipe in RECIPES {
            assert!(!recipe.needs.is_empty(), "empty needs in {}", recipe.name);
            let total: i32 = recipe.needs.iter().map(|(_, c)| *c).sum();
            assert!(total > 0 && total <= 64, "{} needs {total} items", recipe.name);
            assert!(recipe.needs.iter().all(|(_, c)| *c > 0), "{} has non-positive need", recipe.name);
            for &(id, _) in recipe.needs.iter().chain(std::iter::once(&recipe.result)) {
                assert!(id > 0, "{} has non-positive id {id}", recipe.name);
                let name = mc_registry::item_name(id as u32)
                    .unwrap_or_else(|| panic!("{} uses unknown item id {id}", recipe.name));
                assert_eq!(mc_registry::item_id(name), Some(id as u32), "roundtrip of {name}");
                seen.insert(id);
            }
        }
        assert!(seen.len() > 150, "expected a broad item coverage, got {}", seen.len());
        // data/777/items.json 实测值；与公开查询 API 双向锁定。
        for (name, expected, id) in [
            ("minecraft:iron_ingot", 1018, ITEM_IRON_INGOT),
            ("minecraft:iron_pickaxe", 1047, ITEM_IRON_PICKAXE),
            ("minecraft:iron_axe", 1048, ITEM_IRON_AXE),
            ("minecraft:chest", 404, ITEM_CHEST),
            ("minecraft:red_bed", 1233, ITEM_RED_BED),
            ("minecraft:hay_block", 579, ITEM_HAY_BLOCK),
            ("minecraft:paper", 1145, ITEM_PAPER),
            ("minecraft:book", 1146, ITEM_BOOK),
            ("minecraft:oak_fence", 417, ITEM_OAK_FENCE),
            ("minecraft:netherite_ingot", 1023, ITEM_NETHERITE_INGOT),
        ] {
            assert_eq!(id as u32, expected, "wrong item ID for {name}");
            assert_eq!(mc_registry::item_id(name), Some(expected));
        }
    }

    /// 输入多重集相同的配方必须恰好是文档列出的 44 个歧义组；
    /// 每组内规范槽型互不相同，保证拆分结果唯一确定。
    #[test]
    fn p41_input_multiset_conflicts_are_known_and_splittable() {
        use std::collections::{BTreeMap, BTreeSet};
        let mut groups: BTreeMap<Vec<(i32, i32)>, Vec<&Recipe>> = BTreeMap::new();
        for recipe in RECIPES {
            groups.entry(normalized_multiset(recipe.needs)).or_default().push(recipe);
        }
        let conflicts: Vec<Vec<&Recipe>> =
            groups.into_values().filter(|group| group.len() > 1).collect();
        assert_eq!(conflicts.len(), 44, "unexpected number of ambiguous input groups");
        assert_eq!(conflicts.iter().map(Vec::len).sum::<usize>(), 88);
        for group in conflicts {
            let names: Vec<&str> = group.iter().map(|r| r.name).collect();
            let mut layouts = BTreeSet::new();
            for recipe in &group {
                let layout = canonical_layout(recipe)
                    .unwrap_or_else(|| panic!("{} lacks a canonical split layout", recipe.name));
                assert!(layouts.insert(layout), "duplicate canonical layout in {names:?}");
            }
        }
    }

    /// 代表性新配方：匹配结果、数量与扣料（位置/堆叠顺序无关）。
    #[test]
    fn p41_representative_new_recipes_match_and_consume() {
        // 铁镐：3 铁锭 + 2 木棍，分两格。
        let mut grid = [Some((ITEM_IRON_INGOT, 3)), Some((STICK, 2)), None, None];
        let r = match_recipe(grid).expect("iron pickaxe");
        assert_eq!(r.name, "iron_pickaxe");
        assert_eq!(r.result, (ITEM_IRON_PICKAXE, 1));
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [None; GRID_SIZE]);

        // 箱子：8 木板放一格（原版 2×2 围一圈，简化为按数量匹配）。
        let mut grid = [Some((OAK_PLANKS, 8)), None, None, None];
        let r = match_recipe(grid).expect("chest");
        assert_eq!(r.name, "oak_chest");
        assert_eq!(r.result, (ITEM_CHEST, 1));
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [None; GRID_SIZE]);

        // 红床：3 木板 + 3 羊毛；3 种输入以内仍可放进 2×2。
        let mut grid = [Some((OAK_PLANKS, 3)), Some((ITEM_RED_WOOL, 3)), None, None];
        let r = match_recipe(grid).expect("red bed");
        assert_eq!(r.name, "red_bed");
        assert_eq!(r.result, (ITEM_RED_BED, 1));
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [None; GRID_SIZE]);

        // 干草捆：9 小麦单格；纸/书/火把（旧）依旧命中。
        assert_eq!(
            match_recipe([Some((WHEAT, 9)), None, None, None]).unwrap().result,
            (ITEM_HAY_BLOCK, 1)
        );
        assert_eq!(
            match_recipe([Some((ITEM_SUGAR_CANE, 3)), None, None, None]).unwrap().result,
            (ITEM_PAPER, 3)
        );
        assert_eq!(
            match_recipe([Some((ITEM_PAPER, 3)), Some((ITEM_LEATHER, 1)), None, None])
                .unwrap()
                .result,
            (ITEM_BOOK, 1)
        );
        assert_eq!(
            match_recipe([Some((COAL, 1)), Some((STICK, 1)), None, None]).unwrap().result,
            (TORCH, 4)
        );
        // 多一件无关物品仍不命中（新配方同样遵守精确多重集）。
        assert!(match_recipe([Some((ITEM_IRON_INGOT, 3)), Some((STICK, 2)), Some((COBBLESTONE, 1)), None]).is_none());
    }

    /// 歧义组的确定性拆分：每种规范槽型给出唯一结果，重复调用不变；
    /// 未命中任何规范槽型时按配方表首条（稳定表序）回退。
    #[test]
    fn p41_ambiguous_families_are_deterministic() {
        let cases: [([Option<(i32, i32)>; 4], &str); 10] = [
            // 木棍（1+1）/ 压力板（单格 2）
            ([Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), None, None], "stick"),
            ([Some((OAK_PLANKS, 2)), None, None, None], "oak_pressure_plate"),
            // 工作台（单格 4）/ 楼梯（2+2）
            ([Some((OAK_PLANKS, 4)), None, None, None], "crafting_table"),
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 2)), None, None], "oak_stairs"),
            // 木镐（3 单格 + 2 单格）/ 木斧（2+1 + 1+1）
            ([Some((OAK_PLANKS, 3)), Some((STICK, 2)), None, None], "wooden_pickaxe"),
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 1)), Some((STICK, 1)), Some((STICK, 1))], "wooden_axe"),
            // 门（3+3）/ 活板门（2+2+2）
            ([Some((OAK_PLANKS, 3)), Some((OAK_PLANKS, 3)), None, None], "oak_door"),
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 2)), None], "oak_trapdoor"),
            // 栅栏（2+2 + 1+1）/ 栅栏门（2+1+1 + 棍 2 单格）
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 2)), Some((STICK, 1)), Some((STICK, 1))], "oak_fence"),
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), Some((STICK, 2))], "oak_fence_gate"),
        ];
        for (grid, expected) in cases {
            assert_eq!(match_recipe(grid).unwrap().name, expected);
            assert_eq!(match_recipe(grid).unwrap().name, expected, "must be repeatable");
        }
        // 铁护腿（3+2+2）/ 炼药锅（单格 7）。
        let leggings = [Some((ITEM_IRON_INGOT, 3)), Some((ITEM_IRON_INGOT, 2)), Some((ITEM_IRON_INGOT, 2)), None];
        assert_eq!(match_recipe(leggings).unwrap().name, "iron_leggings");
        assert_eq!(match_recipe([Some((ITEM_IRON_INGOT, 7)), None, None, None]).unwrap().name, "cauldron");
        // 未命中规范槽型（3+1 木板）→ 表序首条 crafting_table，绝不随机。
        let odd = [Some((OAK_PLANKS, 3)), Some((OAK_PLANKS, 1)), None, None];
        assert_eq!(match_recipe(odd).unwrap().name, "crafting_table");
    }

    /// P56：2×2 与 3×3 必须共用同一张配方表（同一 recipe / 同一 ID），不得分化。
    #[test]
    fn p56_both_grids_share_one_recipe_table() {
        // 3 小麦 → 面包：2×2 单格与 3×3 单格都命中同一条配方、同一结果。
        let g4 = [Some((WHEAT, 3)), None, None, None];
        let mut g9 = [None; GRID9_SIZE];
        g9[4] = Some((WHEAT, 3));
        let r4 = match_recipe(g4).expect("2×2 bread");
        let r9 = match_recipe_9(g9).expect("3×3 bread");
        assert_eq!(r4.name, "bread");
        assert_eq!(r9.name, r4.name);
        assert_eq!(r4.result, r9.result);
        assert_eq!(r4.needs, r9.needs);
        // 9 小麦 → 干草捆在两种网格里都命中同一条（2×2 是既有“单格堆叠”口径）。
        let mut hay9 = [None; GRID9_SIZE];
        hay9.iter_mut().for_each(|s| *s = Some((WHEAT, 1)));
        assert_eq!(match_recipe_9(hay9).unwrap().name, "hay_block");
        assert_eq!(match_recipe([Some((WHEAT, 9)), None, None, None]).unwrap().name, "hay_block");
    }
}
