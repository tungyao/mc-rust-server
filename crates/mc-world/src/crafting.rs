//! crafting: P40 2×2 玩家背包合成的纯逻辑（配方 + 匹配 + 消耗），只兼容 26.3/777。
//!
//! 所有物品 ID 均由 `mc_registry::ITEMS`（`data/777/items.json` 生成表）在编译期查得，
//! 与 `drops.rs` 同口径；不手写/猜物品 ID。本轮只做玩家背包 2×2（槽 1–4 输入、0 输出），
//! 3×3 工作台 / block entity 明确不做（见 HANDOFF 任务说明）。
//!
//! 配方为**无序匹配（原版 shapeless 口径）**：把合成格内的物品按 ID 计数，
//! 满足某条配方的需求（每种物品数量 ≥ 配方需求、且无配方外的杂物）即命中，
//! 多的留下（放一整组也只扣一份）。候选多条时按规范槽型拆分，
//! 再按总需求量降序（越具体越优先），同量按表序。
//! 输出栈直接返回给调用方，由网络层放光标/背包。
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
// === GEN-CONSTS-START === (scripts/gen_recipes.py 从 data/777/full/{recipes,tags_item} 生成；勿手改)
const ITEM_ACACIA_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_boat") as i32; // 983
const ITEM_ACACIA_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_button") as i32; // 862
const ITEM_ACACIA_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_chest_boat") as i32; // 984
const ITEM_ACACIA_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_door") as i32; // 893
const ITEM_ACACIA_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_fence") as i32; // 421
const ITEM_ACACIA_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_fence_gate") as i32; // 936
const ITEM_ACACIA_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_hanging_sign") as i32; // 1119
const ITEM_ACACIA_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_log") as i32; // 167
const ITEM_ACACIA_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_planks") as i32; // 67
const ITEM_ACACIA_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_pressure_plate") as i32; // 879
const ITEM_ACACIA_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_shelf") as i32; // 377
const ITEM_ACACIA_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_sign") as i32; // 1106
const ITEM_ACACIA_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_slab") as i32; // 345
const ITEM_ACACIA_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_stairs") as i32; // 519
const ITEM_ACACIA_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_trapdoor") as i32; // 915
const ITEM_ACACIA_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:acacia_wood") as i32; // 207
const ITEM_ACTIVATOR_RAIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:activator_rail") as i32; // 948
const ITEM_ALLIUM: i32 = registry_id(mc_registry::ITEMS, "minecraft:allium") as i32; // 304
const ITEM_AMETHYST_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:amethyst_block") as i32; // 117
const ITEM_AMETHYST_SHARD: i32 = registry_id(mc_registry::ITEMS, "minecraft:amethyst_shard") as i32; // 1016
const ITEM_ANDESITE: i32 = registry_id(mc_registry::ITEMS, "minecraft:andesite") as i32; // 6
const ITEM_ANDESITE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:andesite_slab") as i32; // 815
const ITEM_ANDESITE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:andesite_stairs") as i32; // 798
const ITEM_ANDESITE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:andesite_wall") as i32; // 541
const ITEM_ANVIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:anvil") as i32; // 553
const ITEM_APPLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:apple") as i32; // 1007
const ITEM_ARMADILLO_SCUTE: i32 = registry_id(mc_registry::ITEMS, "minecraft:armadillo_scute") as i32; // 1003
const ITEM_ARMOR_STAND: i32 = registry_id(mc_registry::ITEMS, "minecraft:armor_stand") as i32; // 1405
const ITEM_ARROW: i32 = registry_id(mc_registry::ITEMS, "minecraft:arrow") as i32; // 1009
const ITEM_AZURE_BLUET: i32 = registry_id(mc_registry::ITEMS, "minecraft:azure_bluet") as i32; // 305
const ITEM_BAKED_POTATO: i32 = registry_id(mc_registry::ITEMS, "minecraft:baked_potato") as i32; // 1380
const ITEM_BAMBOO: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo") as i32; // 340
const ITEM_BAMBOO_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_block") as i32; // 177
const ITEM_BAMBOO_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_button") as i32; // 868
const ITEM_BAMBOO_CHEST_RAFT: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_chest_raft") as i32; // 996
const ITEM_BAMBOO_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_door") as i32; // 899
const ITEM_BAMBOO_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_fence") as i32; // 427
const ITEM_BAMBOO_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_fence_gate") as i32; // 942
const ITEM_BAMBOO_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_hanging_sign") as i32; // 1125
const ITEM_BAMBOO_MOSAIC: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_mosaic") as i32; // 76
const ITEM_BAMBOO_MOSAIC_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_mosaic_slab") as i32; // 352
const ITEM_BAMBOO_MOSAIC_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_mosaic_stairs") as i32; // 526
const ITEM_BAMBOO_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_planks") as i32; // 73
const ITEM_BAMBOO_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_pressure_plate") as i32; // 885
const ITEM_BAMBOO_RAFT: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_raft") as i32; // 995
const ITEM_BAMBOO_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_shelf") as i32; // 378
const ITEM_BAMBOO_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_sign") as i32; // 1112
const ITEM_BAMBOO_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_slab") as i32; // 351
const ITEM_BAMBOO_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_stairs") as i32; // 525
const ITEM_BAMBOO_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:bamboo_trapdoor") as i32; // 921
const ITEM_BARREL: i32 = registry_id(mc_registry::ITEMS, "minecraft:barrel") as i32; // 1506
const ITEM_BASALT: i32 = registry_id(mc_registry::ITEMS, "minecraft:basalt") as i32; // 436
const ITEM_BEACON: i32 = registry_id(mc_registry::ITEMS, "minecraft:beacon") as i32; // 530
const ITEM_BEEHIVE: i32 = registry_id(mc_registry::ITEMS, "minecraft:beehive") as i32; // 1532
const ITEM_BEETROOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:beetroot") as i32; // 1438
const ITEM_BEETROOT_SOUP: i32 = registry_id(mc_registry::ITEMS, "minecraft:beetroot_soup") as i32; // 1440
const ITEM_BIRCH_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_boat") as i32; // 979
const ITEM_BIRCH_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_button") as i32; // 860
const ITEM_BIRCH_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_chest_boat") as i32; // 980
const ITEM_BIRCH_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_door") as i32; // 891
const ITEM_BIRCH_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_fence") as i32; // 419
const ITEM_BIRCH_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_fence_gate") as i32; // 934
const ITEM_BIRCH_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_hanging_sign") as i32; // 1117
const ITEM_BIRCH_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_log") as i32; // 165
const ITEM_BIRCH_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_planks") as i32; // 65
const ITEM_BIRCH_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_pressure_plate") as i32; // 877
const ITEM_BIRCH_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_shelf") as i32; // 379
const ITEM_BIRCH_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_sign") as i32; // 1104
const ITEM_BIRCH_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_slab") as i32; // 343
const ITEM_BIRCH_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_stairs") as i32; // 517
const ITEM_BIRCH_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_trapdoor") as i32; // 913
const ITEM_BIRCH_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:birch_wood") as i32; // 205
const ITEM_BLACK_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_banner") as i32; // 1432
const ITEM_BLACK_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_bed") as i32; // 1234
const ITEM_BLACK_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_candle") as i32; // 1566
const ITEM_BLACK_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_carpet") as i32; // 595
const ITEM_BLACK_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_concrete") as i32; // 704
const ITEM_BLACK_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_concrete_powder") as i32; // 752
const ITEM_BLACK_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_concrete_slab") as i32; // 736
const ITEM_BLACK_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_concrete_stairs") as i32; // 720
const ITEM_BLACK_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_cushion") as i32; // 1185
const ITEM_BLACK_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_dye") as i32; // 1214
const ITEM_BLACK_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_harness") as i32; // 965
const ITEM_BLACK_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_stained_glass") as i32; // 620
const ITEM_BLACK_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_stained_glass_pane") as i32; // 636
const ITEM_BLACK_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_terracotta") as i32; // 576
const ITEM_BLACK_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_wool") as i32; // 265
const ITEM_BLACK_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_wool_slab") as i32; // 297
const ITEM_BLACK_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:black_wool_stairs") as i32; // 281
const ITEM_BLACKSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:blackstone") as i32; // 1537
const ITEM_BLACKSTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:blackstone_slab") as i32; // 1538
const ITEM_BLACKSTONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:blackstone_stairs") as i32; // 1539
const ITEM_BLACKSTONE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:blackstone_wall") as i32; // 546
const ITEM_BLAST_FURNACE: i32 = registry_id(mc_registry::ITEMS, "minecraft:blast_furnace") as i32; // 1508
const ITEM_BLAZE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:blaze_powder") as i32; // 1274
const ITEM_BLAZE_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:blaze_rod") as i32; // 1266
const ITEM_BLUE_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_banner") as i32; // 1428
const ITEM_BLUE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_bed") as i32; // 1230
const ITEM_BLUE_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_candle") as i32; // 1562
const ITEM_BLUE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_carpet") as i32; // 591
const ITEM_BLUE_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_concrete") as i32; // 700
const ITEM_BLUE_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_concrete_powder") as i32; // 748
const ITEM_BLUE_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_concrete_slab") as i32; // 732
const ITEM_BLUE_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_concrete_stairs") as i32; // 716
const ITEM_BLUE_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_cushion") as i32; // 1181
const ITEM_BLUE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_dye") as i32; // 1210
const ITEM_BLUE_EGG: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_egg") as i32; // 1149
const ITEM_BLUE_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_harness") as i32; // 961
const ITEM_BLUE_ICE: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_ice") as i32; // 786
const ITEM_BLUE_ORCHID: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_orchid") as i32; // 303
const ITEM_BLUE_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_stained_glass") as i32; // 616
const ITEM_BLUE_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_stained_glass_pane") as i32; // 632
const ITEM_BLUE_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_terracotta") as i32; // 572
const ITEM_BLUE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_wool") as i32; // 261
const ITEM_BLUE_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_wool_slab") as i32; // 293
const ITEM_BLUE_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:blue_wool_stairs") as i32; // 277
const ITEM_BOLT_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:bolt_armor_trim_smithing_template") as i32; // 1597
const ITEM_BONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:bone") as i32; // 1216
const ITEM_BONE_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:bone_block") as i32; // 654
const ITEM_BONE_MEAL: i32 = registry_id(mc_registry::ITEMS, "minecraft:bone_meal") as i32; // 1215
const ITEM_BOOK: i32 = registry_id(mc_registry::ITEMS, "minecraft:book") as i32; // 1146
const ITEM_BOOKSHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:bookshelf") as i32; // 390
const ITEM_BORDURE_INDENTED_BANNER_PATTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:bordure_indented_banner_pattern") as i32; // 1503
const ITEM_BOW: i32 = registry_id(mc_registry::ITEMS, "minecraft:bow") as i32; // 1008
const ITEM_BOWL: i32 = registry_id(mc_registry::ITEMS, "minecraft:bowl") as i32; // 1006
const BREAD: i32 = registry_id(mc_registry::ITEMS, "minecraft:bread") as i32; // 1067
const ITEM_BREEZE_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:breeze_rod") as i32; // 1373
const ITEM_BREWING_STAND: i32 = registry_id(mc_registry::ITEMS, "minecraft:brewing_stand") as i32; // 1276
const ITEM_BRICK: i32 = registry_id(mc_registry::ITEMS, "minecraft:brick") as i32; // 1142
const ITEM_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:brick_slab") as i32; // 361
const ITEM_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:brick_stairs") as i32; // 493
const ITEM_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:brick_wall") as i32; // 533
const ITEM_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:bricks") as i32; // 376
const ITEM_BROWN_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_banner") as i32; // 1429
const ITEM_BROWN_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_bed") as i32; // 1231
const ITEM_BROWN_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_candle") as i32; // 1563
const ITEM_BROWN_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_carpet") as i32; // 592
const ITEM_BROWN_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_concrete") as i32; // 701
const ITEM_BROWN_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_concrete_powder") as i32; // 749
const ITEM_BROWN_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_concrete_slab") as i32; // 733
const ITEM_BROWN_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_concrete_stairs") as i32; // 717
const ITEM_BROWN_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_cushion") as i32; // 1182
const ITEM_BROWN_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_dye") as i32; // 1211
const ITEM_BROWN_EGG: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_egg") as i32; // 1150
const ITEM_BROWN_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_harness") as i32; // 962
const ITEM_BROWN_MUSHROOM: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_mushroom") as i32; // 317
const ITEM_BROWN_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_stained_glass") as i32; // 617
const ITEM_BROWN_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_stained_glass_pane") as i32; // 633
const ITEM_BROWN_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_terracotta") as i32; // 573
const ITEM_BROWN_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_wool") as i32; // 262
const ITEM_BROWN_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_wool_slab") as i32; // 294
const ITEM_BROWN_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:brown_wool_stairs") as i32; // 278
const ITEM_BRUSH: i32 = registry_id(mc_registry::ITEMS, "minecraft:brush") as i32; // 1578
const ITEM_BUCKET: i32 = registry_id(mc_registry::ITEMS, "minecraft:bucket") as i32; // 1128
const ITEM_BUNDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:bundle") as i32; // 1153
const ITEM_CACTUS_FLOWER: i32 = registry_id(mc_registry::ITEMS, "minecraft:cactus_flower") as i32; // 414
const ITEM_CAKE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cake") as i32; // 1218
const ITEM_CALIBRATED_SCULK_SENSOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:calibrated_sculk_sensor") as i32; // 850
const ITEM_CAMPFIRE: i32 = registry_id(mc_registry::ITEMS, "minecraft:campfire") as i32; // 1527
const ITEM_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:candle") as i32; // 1550
const ITEM_CARROT: i32 = registry_id(mc_registry::ITEMS, "minecraft:carrot") as i32; // 1378
const ITEM_CARROT_ON_A_STICK: i32 = registry_id(mc_registry::ITEMS, "minecraft:carrot_on_a_stick") as i32; // 971
const ITEM_CARTOGRAPHY_TABLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cartography_table") as i32; // 1509
const ITEM_CARVED_PUMPKIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:carved_pumpkin") as i32; // 431
const ITEM_CAULDRON: i32 = registry_id(mc_registry::ITEMS, "minecraft:cauldron") as i32; // 1277
const ITEM_CHARCOAL: i32 = registry_id(mc_registry::ITEMS, "minecraft:charcoal") as i32; // 1011
const ITEM_CHERRY_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_boat") as i32; // 985
const ITEM_CHERRY_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_button") as i32; // 863
const ITEM_CHERRY_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_chest_boat") as i32; // 986
const ITEM_CHERRY_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_door") as i32; // 894
const ITEM_CHERRY_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_fence") as i32; // 422
const ITEM_CHERRY_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_fence_gate") as i32; // 937
const ITEM_CHERRY_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_hanging_sign") as i32; // 1120
const ITEM_CHERRY_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_log") as i32; // 168
const ITEM_CHERRY_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_planks") as i32; // 68
const ITEM_CHERRY_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_pressure_plate") as i32; // 880
const ITEM_CHERRY_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_shelf") as i32; // 380
const ITEM_CHERRY_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_sign") as i32; // 1107
const ITEM_CHERRY_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_slab") as i32; // 346
const ITEM_CHERRY_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_stairs") as i32; // 520
const ITEM_CHERRY_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_trapdoor") as i32; // 916
const ITEM_CHERRY_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:cherry_wood") as i32; // 208
const ITEM_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:chest") as i32; // 404
const ITEM_CHEST_MINECART: i32 = registry_id(mc_registry::ITEMS, "minecraft:chest_minecart") as i32; // 967
const ITEM_CHISELED_BOOKSHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_bookshelf") as i32; // 391
const ITEM_CHISELED_CINNABAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_cinnabar") as i32; // 52
const ITEM_CHISELED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_copper") as i32; // 131
const ITEM_CHISELED_DEEPSLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_deepslate") as i32; // 459
const ITEM_CHISELED_NETHER_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_nether_bricks") as i32; // 500
const ITEM_CHISELED_POLISHED_BLACKSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_polished_blackstone") as i32; // 1544
const ITEM_CHISELED_QUARTZ_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_quartz_block") as i32; // 556
const ITEM_CHISELED_RED_SANDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_red_sandstone") as i32; // 645
const ITEM_CHISELED_RESIN_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_resin_bricks") as i32; // 492
const ITEM_CHISELED_SANDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_sandstone") as i32; // 235
const ITEM_CHISELED_STONE_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_stone_bricks") as i32; // 452
const ITEM_CHISELED_SULFUR: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_sulfur") as i32; // 39
const ITEM_CHISELED_TUFF: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_tuff") as i32; // 16
const ITEM_CHISELED_TUFF_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:chiseled_tuff_bricks") as i32; // 25
const ITEM_CINNABAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:cinnabar") as i32; // 40
const ITEM_CINNABAR_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cinnabar_brick_slab") as i32; // 49
const ITEM_CINNABAR_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cinnabar_brick_stairs") as i32; // 50
const ITEM_CINNABAR_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:cinnabar_brick_wall") as i32; // 51
const ITEM_CINNABAR_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cinnabar_bricks") as i32; // 48
const ITEM_CINNABAR_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cinnabar_slab") as i32; // 41
const ITEM_CINNABAR_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cinnabar_stairs") as i32; // 42
const ITEM_CINNABAR_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:cinnabar_wall") as i32; // 43
const ITEM_CLAY: i32 = registry_id(mc_registry::ITEMS, "minecraft:clay") as i32; // 415
const ITEM_CLAY_BALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:clay_ball") as i32; // 1143
const ITEM_CLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:clock") as i32; // 1187
const ITEM_CLOSED_EYEBLOSSOM: i32 = registry_id(mc_registry::ITEMS, "minecraft:closed_eyeblossom") as i32; // 301
const COAL: i32 = registry_id(mc_registry::ITEMS, "minecraft:coal") as i32; // 1010
const ITEM_COAL_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:coal_block") as i32; // 112
const ITEM_COARSE_DIRT: i32 = registry_id(mc_registry::ITEMS, "minecraft:coarse_dirt") as i32; // 56
const ITEM_COAST_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:coast_armor_trim_smithing_template") as i32; // 1582
const ITEM_COBBLED_DEEPSLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobbled_deepslate") as i32; // 9
const ITEM_COBBLED_DEEPSLATE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobbled_deepslate_slab") as i32; // 819
const ITEM_COBBLED_DEEPSLATE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobbled_deepslate_stairs") as i32; // 802
const ITEM_COBBLED_DEEPSLATE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobbled_deepslate_wall") as i32; // 549
const COBBLESTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobblestone") as i32; // 62
const ITEM_COBBLESTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobblestone_slab") as i32; // 360
const ITEM_COBBLESTONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobblestone_stairs") as i32; // 409
const ITEM_COBBLESTONE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:cobblestone_wall") as i32; // 531
const ITEM_COCOA_BEANS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cocoa_beans") as i32; // 1198
const ITEM_COMPARATOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:comparator") as i32; // 828
const ITEM_COMPASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:compass") as i32; // 1151
const ITEM_COMPOSTER: i32 = registry_id(mc_registry::ITEMS, "minecraft:composter") as i32; // 1505
const ITEM_CONDUIT: i32 = registry_id(mc_registry::ITEMS, "minecraft:conduit") as i32; // 787
const ITEM_COOKED_RABBIT: i32 = registry_id(mc_registry::ITEMS, "minecraft:cooked_rabbit") as i32; // 1401
const ITEM_COOKIE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cookie") as i32; // 1236
const ITEM_COPPER_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_axe") as i32; // 1033
const ITEM_COPPER_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_bars") as i32; // 465
const ITEM_COPPER_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_block") as i32; // 120
const ITEM_COPPER_BOOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_boots") as i32; // 1075
const ITEM_COPPER_BULB: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_bulb") as i32; // 1629
const ITEM_COPPER_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_chain") as i32; // 474
const ITEM_COPPER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_chest") as i32; // 1637
const ITEM_COPPER_CHESTPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_chestplate") as i32; // 1073
const ITEM_COPPER_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_door") as i32; // 902
const ITEM_COPPER_GOLEM_STATUE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_golem_statue") as i32; // 1645
const ITEM_COPPER_GRATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_grate") as i32; // 1621
const ITEM_COPPER_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_helmet") as i32; // 1072
const ITEM_COPPER_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_hoe") as i32; // 1034
const ITEM_COPPER_INGOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_ingot") as i32; // 1020
const ITEM_COPPER_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_lantern") as i32; // 1517
const ITEM_COPPER_LEGGINGS: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_leggings") as i32; // 1074
const ITEM_COPPER_NUGGET: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_nugget") as i32; // 1457
const ITEM_COPPER_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_pickaxe") as i32; // 1032
const ITEM_COPPER_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_shovel") as i32; // 1031
const ITEM_COPPER_SPEAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_spear") as i32; // 1449
const ITEM_COPPER_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_sword") as i32; // 1030
const ITEM_COPPER_TORCH: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_torch") as i32; // 440
const ITEM_COPPER_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:copper_trapdoor") as i32; // 924
const ITEM_CORNFLOWER: i32 = registry_id(mc_registry::ITEMS, "minecraft:cornflower") as i32; // 311
const ITEM_CRAFTER: i32 = registry_id(mc_registry::ITEMS, "minecraft:crafter") as i32; // 1237
const CRAFTING_TABLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:crafting_table") as i32; // 405
const ITEM_CREAKING_HEART: i32 = registry_id(mc_registry::ITEMS, "minecraft:creaking_heart") as i32; // 403
const ITEM_CREEPER_BANNER_PATTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:creeper_banner_pattern") as i32; // 1495
const ITEM_CREEPER_HEAD: i32 = registry_id(mc_registry::ITEMS, "minecraft:creeper_head") as i32; // 1388
const ITEM_CRIMSON_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_button") as i32; // 869
const ITEM_CRIMSON_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_door") as i32; // 900
const ITEM_CRIMSON_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_fence") as i32; // 428
const ITEM_CRIMSON_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_fence_gate") as i32; // 943
const ITEM_CRIMSON_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_hanging_sign") as i32; // 1126
const ITEM_CRIMSON_HYPHAE: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_hyphae") as i32; // 213
const ITEM_CRIMSON_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_planks") as i32; // 74
const ITEM_CRIMSON_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_pressure_plate") as i32; // 886
const ITEM_CRIMSON_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_shelf") as i32; // 381
const ITEM_CRIMSON_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_sign") as i32; // 1113
const ITEM_CRIMSON_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_slab") as i32; // 353
const ITEM_CRIMSON_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_stairs") as i32; // 527
const ITEM_CRIMSON_STEM: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_stem") as i32; // 175
const ITEM_CRIMSON_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:crimson_trapdoor") as i32; // 922
const ITEM_CROSSBOW: i32 = registry_id(mc_registry::ITEMS, "minecraft:crossbow") as i32; // 1491
const ITEM_CRYING_OBSIDIAN: i32 = registry_id(mc_registry::ITEMS, "minecraft:crying_obsidian") as i32; // 1536
const ITEM_CUT_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:cut_copper") as i32; // 139
const ITEM_CUT_COPPER_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cut_copper_slab") as i32; // 155
const ITEM_CUT_COPPER_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cut_copper_stairs") as i32; // 147
const ITEM_CUT_RED_SANDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cut_red_sandstone") as i32; // 646
const ITEM_CUT_RED_SANDSTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cut_red_sandstone_slab") as i32; // 367
const ITEM_CUT_SANDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cut_sandstone") as i32; // 236
const ITEM_CUT_SANDSTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cut_sandstone_slab") as i32; // 358
const ITEM_CYAN_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_banner") as i32; // 1426
const ITEM_CYAN_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_bed") as i32; // 1228
const ITEM_CYAN_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_candle") as i32; // 1560
const ITEM_CYAN_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_carpet") as i32; // 589
const ITEM_CYAN_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_concrete") as i32; // 698
const ITEM_CYAN_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_concrete_powder") as i32; // 746
const ITEM_CYAN_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_concrete_slab") as i32; // 730
const ITEM_CYAN_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_concrete_stairs") as i32; // 714
const ITEM_CYAN_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_cushion") as i32; // 1179
const ITEM_CYAN_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_dye") as i32; // 1208
const ITEM_CYAN_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_harness") as i32; // 959
const ITEM_CYAN_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_stained_glass") as i32; // 614
const ITEM_CYAN_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_stained_glass_pane") as i32; // 630
const ITEM_CYAN_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_terracotta") as i32; // 570
const ITEM_CYAN_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_wool") as i32; // 259
const ITEM_CYAN_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_wool_slab") as i32; // 291
const ITEM_CYAN_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:cyan_wool_stairs") as i32; // 275
const ITEM_DANDELION: i32 = registry_id(mc_registry::ITEMS, "minecraft:dandelion") as i32; // 298
const ITEM_DARK_OAK_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_boat") as i32; // 987
const ITEM_DARK_OAK_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_button") as i32; // 864
const ITEM_DARK_OAK_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_chest_boat") as i32; // 988
const ITEM_DARK_OAK_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_door") as i32; // 895
const ITEM_DARK_OAK_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_fence") as i32; // 423
const ITEM_DARK_OAK_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_fence_gate") as i32; // 938
const ITEM_DARK_OAK_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_hanging_sign") as i32; // 1121
const ITEM_DARK_OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_log") as i32; // 170
const ITEM_DARK_OAK_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_planks") as i32; // 69
const ITEM_DARK_OAK_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_pressure_plate") as i32; // 881
const ITEM_DARK_OAK_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_shelf") as i32; // 382
const ITEM_DARK_OAK_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_sign") as i32; // 1108
const ITEM_DARK_OAK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_slab") as i32; // 347
const ITEM_DARK_OAK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_stairs") as i32; // 521
const ITEM_DARK_OAK_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_trapdoor") as i32; // 917
const ITEM_DARK_OAK_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_oak_wood") as i32; // 210
const ITEM_DARK_PRISMARINE: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_prismarine") as i32; // 639
const ITEM_DARK_PRISMARINE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_prismarine_slab") as i32; // 371
const ITEM_DARK_PRISMARINE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:dark_prismarine_stairs") as i32; // 642
const ITEM_DAYLIGHT_DETECTOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:daylight_detector") as i32; // 848
const ITEM_DECORATED_POT: i32 = registry_id(mc_registry::ITEMS, "minecraft:decorated_pot") as i32; // 392
const ITEM_DEEPSLATE_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:deepslate_brick_slab") as i32; // 821
const ITEM_DEEPSLATE_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:deepslate_brick_stairs") as i32; // 804
const ITEM_DEEPSLATE_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:deepslate_brick_wall") as i32; // 551
const ITEM_DEEPSLATE_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:deepslate_bricks") as i32; // 455
const ITEM_DEEPSLATE_TILE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:deepslate_tile_slab") as i32; // 822
const ITEM_DEEPSLATE_TILE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:deepslate_tile_stairs") as i32; // 805
const ITEM_DEEPSLATE_TILE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:deepslate_tile_wall") as i32; // 552
const ITEM_DEEPSLATE_TILES: i32 = registry_id(mc_registry::ITEMS, "minecraft:deepslate_tiles") as i32; // 457
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
const ITEM_DIAMOND_SPEAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_spear") as i32; // 1452
const ITEM_DIAMOND_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:diamond_sword") as i32; // 1050
const ITEM_DIORITE: i32 = registry_id(mc_registry::ITEMS, "minecraft:diorite") as i32; // 4
const ITEM_DIORITE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:diorite_slab") as i32; // 818
const ITEM_DIORITE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:diorite_stairs") as i32; // 801
const ITEM_DIORITE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:diorite_wall") as i32; // 545
const ITEM_DIRT: i32 = registry_id(mc_registry::ITEMS, "minecraft:dirt") as i32; // 55
const ITEM_DISC_FRAGMENT_5: i32 = registry_id(mc_registry::ITEMS, "minecraft:disc_fragment_5") as i32; // 1482
const ITEM_DISPENSER: i32 = registry_id(mc_registry::ITEMS, "minecraft:dispenser") as i32; // 835
const ITEM_DRIED_GHAST: i32 = registry_id(mc_registry::ITEMS, "minecraft:dried_ghast") as i32; // 755
const ITEM_DRIED_KELP: i32 = registry_id(mc_registry::ITEMS, "minecraft:dried_kelp") as i32; // 1257
const ITEM_DRIED_KELP_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:dried_kelp_block") as i32; // 1144
const ITEM_DRIPSTONE_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:dripstone_block") as i32; // 53
const ITEM_DROPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:dropper") as i32; // 836
const ITEM_DUNE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:dune_armor_trim_smithing_template") as i32; // 1581
const ITEM_ECHO_SHARD: i32 = registry_id(mc_registry::ITEMS, "minecraft:echo_shard") as i32; // 1577
const ITEM_EGG: i32 = registry_id(mc_registry::ITEMS, "minecraft:egg") as i32; // 1148
const ITEM_EMERALD: i32 = registry_id(mc_registry::ITEMS, "minecraft:emerald") as i32; // 1013
const ITEM_EMERALD_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:emerald_block") as i32; // 514
const ITEM_ENCHANTED_GOLDEN_APPLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:enchanted_golden_apple") as i32; // 1101
const ITEM_ENCHANTING_TABLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:enchanting_table") as i32; // 507
const ITEM_END_CRYSTAL: i32 = registry_id(mc_registry::ITEMS, "minecraft:end_crystal") as i32; // 1433
const ITEM_END_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:end_rod") as i32; // 396
const ITEM_END_STONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:end_stone") as i32; // 509
const ITEM_END_STONE_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:end_stone_brick_slab") as i32; // 811
const ITEM_END_STONE_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:end_stone_brick_stairs") as i32; // 793
const ITEM_END_STONE_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:end_stone_brick_wall") as i32; // 544
const ITEM_END_STONE_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:end_stone_bricks") as i32; // 510
const ITEM_ENDER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:ender_chest") as i32; // 513
const ITEM_ENDER_EYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:ender_eye") as i32; // 1278
const ITEM_ENDER_PEARL: i32 = registry_id(mc_registry::ITEMS, "minecraft:ender_pearl") as i32; // 1265
const ITEM_EXPOSED_CHISELED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_chiseled_copper") as i32; // 132
const ITEM_EXPOSED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper") as i32; // 121
const ITEM_EXPOSED_COPPER_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_bars") as i32; // 466
const ITEM_EXPOSED_COPPER_BULB: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_bulb") as i32; // 1630
const ITEM_EXPOSED_COPPER_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_chain") as i32; // 475
const ITEM_EXPOSED_COPPER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_chest") as i32; // 1638
const ITEM_EXPOSED_COPPER_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_door") as i32; // 903
const ITEM_EXPOSED_COPPER_GOLEM_STATUE: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_golem_statue") as i32; // 1646
const ITEM_EXPOSED_COPPER_GRATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_grate") as i32; // 1622
const ITEM_EXPOSED_COPPER_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_lantern") as i32; // 1518
const ITEM_EXPOSED_COPPER_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_copper_trapdoor") as i32; // 925
const ITEM_EXPOSED_CUT_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_cut_copper") as i32; // 140
const ITEM_EXPOSED_CUT_COPPER_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_cut_copper_slab") as i32; // 156
const ITEM_EXPOSED_CUT_COPPER_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_cut_copper_stairs") as i32; // 148
const ITEM_EXPOSED_LIGHTNING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:exposed_lightning_rod") as i32; // 841
const ITEM_EYE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:eye_armor_trim_smithing_template") as i32; // 1585
const ITEM_FEATHER: i32 = registry_id(mc_registry::ITEMS, "minecraft:feather") as i32; // 1063
const ITEM_FERMENTED_SPIDER_EYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:fermented_spider_eye") as i32; // 1273
const ITEM_FIELD_MASONED_BANNER_PATTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:field_masoned_banner_pattern") as i32; // 1502
const ITEM_FIRE_CHARGE: i32 = registry_id(mc_registry::ITEMS, "minecraft:fire_charge") as i32; // 1369
const ITEM_FIREWORK_ROCKET: i32 = registry_id(mc_registry::ITEMS, "minecraft:firework_rocket") as i32; // 1393
const ITEM_FISHING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:fishing_rod") as i32; // 1186
const ITEM_FLETCHING_TABLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:fletching_table") as i32; // 1510
const ITEM_FLINT: i32 = registry_id(mc_registry::ITEMS, "minecraft:flint") as i32; // 1096
const ITEM_FLINT_AND_STEEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:flint_and_steel") as i32; // 1005
const ITEM_FLOW_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:flow_armor_trim_smithing_template") as i32; // 1596
const ITEM_FLOWER_BANNER_PATTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:flower_banner_pattern") as i32; // 1494
const ITEM_FLOWER_POT: i32 = registry_id(mc_registry::ITEMS, "minecraft:flower_pot") as i32; // 1377
const ITEM_FURNACE: i32 = registry_id(mc_registry::ITEMS, "minecraft:furnace") as i32; // 407
const ITEM_FURNACE_MINECART: i32 = registry_id(mc_registry::ITEMS, "minecraft:furnace_minecart") as i32; // 968
const ITEM_GHAST_TEAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:ghast_tear") as i32; // 1267
const ITEM_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:glass") as i32; // 231
const ITEM_GLASS_BOTTLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:glass_bottle") as i32; // 1270
const ITEM_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:glass_pane") as i32; // 482
const ITEM_GLISTERING_MELON_SLICE: i32 = registry_id(mc_registry::ITEMS, "minecraft:glistering_melon_slice") as i32; // 1279
const ITEM_GLOW_INK_SAC: i32 = registry_id(mc_registry::ITEMS, "minecraft:glow_ink_sac") as i32; // 1197
const ITEM_GLOW_ITEM_FRAME: i32 = registry_id(mc_registry::ITEMS, "minecraft:glow_item_frame") as i32; // 1376
const ITEM_GLOWSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:glowstone") as i32; // 441
const ITEM_GLOWSTONE_DUST: i32 = registry_id(mc_registry::ITEMS, "minecraft:glowstone_dust") as i32; // 1189
const ITEM_GOLD_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:gold_block") as i32; // 128
const ITEM_GOLD_INGOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:gold_ingot") as i32; // 1022
const ITEM_GOLD_NUGGET: i32 = registry_id(mc_registry::ITEMS, "minecraft:gold_nugget") as i32; // 1268
const ITEM_GOLDEN_APPLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_apple") as i32; // 1100
const ITEM_GOLDEN_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_axe") as i32; // 1043
const ITEM_GOLDEN_BOOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_boots") as i32; // 1091
const ITEM_GOLDEN_CARROT: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_carrot") as i32; // 1383
const ITEM_GOLDEN_CHESTPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_chestplate") as i32; // 1089
const ITEM_GOLDEN_DANDELION: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_dandelion") as i32; // 299
const ITEM_GOLDEN_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_helmet") as i32; // 1088
const ITEM_GOLDEN_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_hoe") as i32; // 1044
const ITEM_GOLDEN_LEGGINGS: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_leggings") as i32; // 1090
const ITEM_GOLDEN_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_pickaxe") as i32; // 1042
const ITEM_GOLDEN_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_shovel") as i32; // 1041
const ITEM_GOLDEN_SPEAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_spear") as i32; // 1451
const ITEM_GOLDEN_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:golden_sword") as i32; // 1040
const ITEM_GRANITE: i32 = registry_id(mc_registry::ITEMS, "minecraft:granite") as i32; // 2
const ITEM_GRANITE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:granite_slab") as i32; // 814
const ITEM_GRANITE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:granite_stairs") as i32; // 797
const ITEM_GRANITE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:granite_wall") as i32; // 537
const ITEM_GRAVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:gravel") as i32; // 92
const ITEM_GRAY_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_banner") as i32; // 1424
const ITEM_GRAY_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_bed") as i32; // 1226
const ITEM_GRAY_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_candle") as i32; // 1558
const ITEM_GRAY_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_carpet") as i32; // 587
const ITEM_GRAY_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_concrete") as i32; // 696
const ITEM_GRAY_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_concrete_powder") as i32; // 744
const ITEM_GRAY_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_concrete_slab") as i32; // 728
const ITEM_GRAY_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_concrete_stairs") as i32; // 712
const ITEM_GRAY_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_cushion") as i32; // 1177
const ITEM_GRAY_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_dye") as i32; // 1206
const ITEM_GRAY_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_harness") as i32; // 957
const ITEM_GRAY_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_stained_glass") as i32; // 612
const ITEM_GRAY_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_stained_glass_pane") as i32; // 628
const ITEM_GRAY_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_terracotta") as i32; // 568
const ITEM_GRAY_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_wool") as i32; // 257
const ITEM_GRAY_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_wool_slab") as i32; // 289
const ITEM_GRAY_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:gray_wool_stairs") as i32; // 273
const ITEM_GREEN_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_banner") as i32; // 1430
const ITEM_GREEN_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_bed") as i32; // 1232
const ITEM_GREEN_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_candle") as i32; // 1564
const ITEM_GREEN_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_carpet") as i32; // 593
const ITEM_GREEN_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_concrete") as i32; // 702
const ITEM_GREEN_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_concrete_powder") as i32; // 750
const ITEM_GREEN_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_concrete_slab") as i32; // 734
const ITEM_GREEN_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_concrete_stairs") as i32; // 718
const ITEM_GREEN_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_cushion") as i32; // 1183
const ITEM_GREEN_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_dye") as i32; // 1212
const ITEM_GREEN_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_harness") as i32; // 963
const ITEM_GREEN_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_stained_glass") as i32; // 618
const ITEM_GREEN_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_stained_glass_pane") as i32; // 634
const ITEM_GREEN_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_terracotta") as i32; // 574
const ITEM_GREEN_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_wool") as i32; // 263
const ITEM_GREEN_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_wool_slab") as i32; // 295
const ITEM_GREEN_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:green_wool_stairs") as i32; // 279
const ITEM_GRINDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:grindstone") as i32; // 1511
const ITEM_GUNPOWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:gunpowder") as i32; // 1064
const ITEM_HAY_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:hay_block") as i32; // 579
const ITEM_HEART_OF_THE_SEA: i32 = registry_id(mc_registry::ITEMS, "minecraft:heart_of_the_sea") as i32; // 1490
const ITEM_HEAVY_CORE: i32 = registry_id(mc_registry::ITEMS, "minecraft:heavy_core") as i32; // 116
const ITEM_HEAVY_WEIGHTED_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:heavy_weighted_pressure_plate") as i32; // 874
const ITEM_HONEY_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:honey_block") as i32; // 832
const ITEM_HONEY_BOTTLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:honey_bottle") as i32; // 1533
const ITEM_HONEYCOMB: i32 = registry_id(mc_registry::ITEMS, "minecraft:honeycomb") as i32; // 1530
const ITEM_HONEYCOMB_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:honeycomb_block") as i32; // 1534
const ITEM_HOPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:hopper") as i32; // 834
const ITEM_HOPPER_MINECART: i32 = registry_id(mc_registry::ITEMS, "minecraft:hopper_minecart") as i32; // 970
const ITEM_HOST_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:host_armor_trim_smithing_template") as i32; // 1595
const ITEM_ICE: i32 = registry_id(mc_registry::ITEMS, "minecraft:ice") as i32; // 411
const ITEM_INK_SAC: i32 = registry_id(mc_registry::ITEMS, "minecraft:ink_sac") as i32; // 1196
const ITEM_IRON_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_axe") as i32; // 1048
const ITEM_IRON_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_bars") as i32; // 464
const ITEM_IRON_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_block") as i32; // 119
const ITEM_IRON_BOOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_boots") as i32; // 1083
const ITEM_IRON_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_chain") as i32; // 473
const ITEM_IRON_CHESTPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_chestplate") as i32; // 1081
const ITEM_IRON_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_door") as i32; // 888
const ITEM_IRON_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_helmet") as i32; // 1080
const ITEM_IRON_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_hoe") as i32; // 1049
const ITEM_IRON_INGOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_ingot") as i32; // 1018
const ITEM_IRON_LEGGINGS: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_leggings") as i32; // 1082
const ITEM_IRON_NUGGET: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_nugget") as i32; // 1456
const ITEM_IRON_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_pickaxe") as i32; // 1047
const ITEM_IRON_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_shovel") as i32; // 1046
const ITEM_IRON_SPEAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_spear") as i32; // 1450
const ITEM_IRON_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_sword") as i32; // 1045
const ITEM_IRON_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:iron_trapdoor") as i32; // 910
const ITEM_ITEM_FRAME: i32 = registry_id(mc_registry::ITEMS, "minecraft:item_frame") as i32; // 1375
const ITEM_JACK_O_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:jack_o_lantern") as i32; // 432
const ITEM_JUKEBOX: i32 = registry_id(mc_registry::ITEMS, "minecraft:jukebox") as i32; // 416
const ITEM_JUNGLE_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_boat") as i32; // 981
const ITEM_JUNGLE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_button") as i32; // 861
const ITEM_JUNGLE_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_chest_boat") as i32; // 982
const ITEM_JUNGLE_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_door") as i32; // 892
const ITEM_JUNGLE_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_fence") as i32; // 420
const ITEM_JUNGLE_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_fence_gate") as i32; // 935
const ITEM_JUNGLE_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_hanging_sign") as i32; // 1118
const ITEM_JUNGLE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_log") as i32; // 166
const ITEM_JUNGLE_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_planks") as i32; // 66
const ITEM_JUNGLE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_pressure_plate") as i32; // 878
const ITEM_JUNGLE_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_shelf") as i32; // 383
const ITEM_JUNGLE_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_sign") as i32; // 1105
const ITEM_JUNGLE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_slab") as i32; // 344
const ITEM_JUNGLE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_stairs") as i32; // 518
const ITEM_JUNGLE_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_trapdoor") as i32; // 914
const ITEM_JUNGLE_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:jungle_wood") as i32; // 206
const ITEM_LADDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:ladder") as i32; // 408
const ITEM_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:lantern") as i32; // 1515
const ITEM_LAPIS_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:lapis_block") as i32; // 233
const ITEM_LAPIS_LAZULI: i32 = registry_id(mc_registry::ITEMS, "minecraft:lapis_lazuli") as i32; // 1014
const ITEM_LEAD: i32 = registry_id(mc_registry::ITEMS, "minecraft:lead") as i32; // 1412
const ITEM_LEATHER: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather") as i32; // 1133
const ITEM_LEATHER_BOOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_boots") as i32; // 1071
const ITEM_LEATHER_CHESTPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_chestplate") as i32; // 1069
const ITEM_LEATHER_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_helmet") as i32; // 1068
const ITEM_LEATHER_HORSE_ARMOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_horse_armor") as i32; // 1411
const ITEM_LEATHER_LEGGINGS: i32 = registry_id(mc_registry::ITEMS, "minecraft:leather_leggings") as i32; // 1070
const ITEM_LECTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:lectern") as i32; // 837
const ITEM_LEVER: i32 = registry_id(mc_registry::ITEMS, "minecraft:lever") as i32; // 839
const ITEM_LIGHT_BLUE_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_banner") as i32; // 1420
const ITEM_LIGHT_BLUE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_bed") as i32; // 1222
const ITEM_LIGHT_BLUE_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_candle") as i32; // 1554
const ITEM_LIGHT_BLUE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_carpet") as i32; // 583
const ITEM_LIGHT_BLUE_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_concrete") as i32; // 692
const ITEM_LIGHT_BLUE_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_concrete_powder") as i32; // 740
const ITEM_LIGHT_BLUE_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_concrete_slab") as i32; // 724
const ITEM_LIGHT_BLUE_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_concrete_stairs") as i32; // 708
const ITEM_LIGHT_BLUE_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_cushion") as i32; // 1173
const ITEM_LIGHT_BLUE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_dye") as i32; // 1202
const ITEM_LIGHT_BLUE_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_harness") as i32; // 953
const ITEM_LIGHT_BLUE_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_stained_glass") as i32; // 608
const ITEM_LIGHT_BLUE_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_stained_glass_pane") as i32; // 624
const ITEM_LIGHT_BLUE_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_terracotta") as i32; // 564
const ITEM_LIGHT_BLUE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_wool") as i32; // 253
const ITEM_LIGHT_BLUE_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_wool_slab") as i32; // 285
const ITEM_LIGHT_BLUE_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_blue_wool_stairs") as i32; // 269
const ITEM_LIGHT_GRAY_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_banner") as i32; // 1425
const ITEM_LIGHT_GRAY_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_bed") as i32; // 1227
const ITEM_LIGHT_GRAY_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_candle") as i32; // 1559
const ITEM_LIGHT_GRAY_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_carpet") as i32; // 588
const ITEM_LIGHT_GRAY_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_concrete") as i32; // 697
const ITEM_LIGHT_GRAY_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_concrete_powder") as i32; // 745
const ITEM_LIGHT_GRAY_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_concrete_slab") as i32; // 729
const ITEM_LIGHT_GRAY_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_concrete_stairs") as i32; // 713
const ITEM_LIGHT_GRAY_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_cushion") as i32; // 1178
const ITEM_LIGHT_GRAY_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_dye") as i32; // 1207
const ITEM_LIGHT_GRAY_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_harness") as i32; // 958
const ITEM_LIGHT_GRAY_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_stained_glass") as i32; // 613
const ITEM_LIGHT_GRAY_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_stained_glass_pane") as i32; // 629
const ITEM_LIGHT_GRAY_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_terracotta") as i32; // 569
const ITEM_LIGHT_GRAY_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_wool") as i32; // 258
const ITEM_LIGHT_GRAY_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_wool_slab") as i32; // 290
const ITEM_LIGHT_GRAY_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_gray_wool_stairs") as i32; // 274
const ITEM_LIGHT_WEIGHTED_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:light_weighted_pressure_plate") as i32; // 873
const ITEM_LIGHTNING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:lightning_rod") as i32; // 840
const ITEM_LILAC: i32 = registry_id(mc_registry::ITEMS, "minecraft:lilac") as i32; // 600
const ITEM_LILY_OF_THE_VALLEY: i32 = registry_id(mc_registry::ITEMS, "minecraft:lily_of_the_valley") as i32; // 312
const ITEM_LIME_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_banner") as i32; // 1422
const ITEM_LIME_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_bed") as i32; // 1224
const ITEM_LIME_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_candle") as i32; // 1556
const ITEM_LIME_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_carpet") as i32; // 585
const ITEM_LIME_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_concrete") as i32; // 694
const ITEM_LIME_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_concrete_powder") as i32; // 742
const ITEM_LIME_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_concrete_slab") as i32; // 726
const ITEM_LIME_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_concrete_stairs") as i32; // 710
const ITEM_LIME_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_cushion") as i32; // 1175
const ITEM_LIME_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_dye") as i32; // 1204
const ITEM_LIME_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_harness") as i32; // 955
const ITEM_LIME_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_stained_glass") as i32; // 610
const ITEM_LIME_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_stained_glass_pane") as i32; // 626
const ITEM_LIME_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_terracotta") as i32; // 566
const ITEM_LIME_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_wool") as i32; // 255
const ITEM_LIME_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_wool_slab") as i32; // 287
const ITEM_LIME_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:lime_wool_stairs") as i32; // 271
const ITEM_LODESTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:lodestone") as i32; // 1535
const ITEM_LOOM: i32 = registry_id(mc_registry::ITEMS, "minecraft:loom") as i32; // 1493
const ITEM_MACE: i32 = registry_id(mc_registry::ITEMS, "minecraft:mace") as i32; // 1374
const ITEM_MAGENTA_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_banner") as i32; // 1419
const ITEM_MAGENTA_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_bed") as i32; // 1221
const ITEM_MAGENTA_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_candle") as i32; // 1553
const ITEM_MAGENTA_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_carpet") as i32; // 582
const ITEM_MAGENTA_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_concrete") as i32; // 691
const ITEM_MAGENTA_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_concrete_powder") as i32; // 739
const ITEM_MAGENTA_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_concrete_slab") as i32; // 723
const ITEM_MAGENTA_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_concrete_stairs") as i32; // 707
const ITEM_MAGENTA_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_cushion") as i32; // 1172
const ITEM_MAGENTA_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_dye") as i32; // 1201
const ITEM_MAGENTA_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_harness") as i32; // 952
const ITEM_MAGENTA_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_stained_glass") as i32; // 607
const ITEM_MAGENTA_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_stained_glass_pane") as i32; // 623
const ITEM_MAGENTA_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_terracotta") as i32; // 563
const ITEM_MAGENTA_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_wool") as i32; // 252
const ITEM_MAGENTA_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_wool_slab") as i32; // 284
const ITEM_MAGENTA_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:magenta_wool_stairs") as i32; // 268
const ITEM_MAGMA_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:magma_block") as i32; // 650
const ITEM_MAGMA_CREAM: i32 = registry_id(mc_registry::ITEMS, "minecraft:magma_cream") as i32; // 1275
const ITEM_MANGROVE_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_boat") as i32; // 991
const ITEM_MANGROVE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_button") as i32; // 866
const ITEM_MANGROVE_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_chest_boat") as i32; // 992
const ITEM_MANGROVE_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_door") as i32; // 897
const ITEM_MANGROVE_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_fence") as i32; // 425
const ITEM_MANGROVE_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_fence_gate") as i32; // 940
const ITEM_MANGROVE_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_hanging_sign") as i32; // 1123
const ITEM_MANGROVE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_log") as i32; // 171
const ITEM_MANGROVE_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_planks") as i32; // 71
const ITEM_MANGROVE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_pressure_plate") as i32; // 883
const ITEM_MANGROVE_ROOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_roots") as i32; // 173
const ITEM_MANGROVE_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_shelf") as i32; // 384
const ITEM_MANGROVE_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_sign") as i32; // 1110
const ITEM_MANGROVE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_slab") as i32; // 349
const ITEM_MANGROVE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_stairs") as i32; // 523
const ITEM_MANGROVE_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_trapdoor") as i32; // 919
const ITEM_MANGROVE_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:mangrove_wood") as i32; // 211
const ITEM_MAP: i32 = registry_id(mc_registry::ITEMS, "minecraft:map") as i32; // 1382
const ITEM_MELON: i32 = registry_id(mc_registry::ITEMS, "minecraft:melon") as i32; // 483
const ITEM_MELON_SEEDS: i32 = registry_id(mc_registry::ITEMS, "minecraft:melon_seeds") as i32; // 1259
const ITEM_MELON_SLICE: i32 = registry_id(mc_registry::ITEMS, "minecraft:melon_slice") as i32; // 1256
const ITEM_MILK_BUCKET: i32 = registry_id(mc_registry::ITEMS, "minecraft:milk_bucket") as i32; // 1134
const ITEM_MINECART: i32 = registry_id(mc_registry::ITEMS, "minecraft:minecart") as i32; // 966
const ITEM_MOJANG_BANNER_PATTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:mojang_banner_pattern") as i32; // 1497
const ITEM_MOSS_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:moss_block") as i32; // 333
const ITEM_MOSS_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:moss_carpet") as i32; // 332
const ITEM_MOSSY_COBBLESTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:mossy_cobblestone") as i32; // 393
const ITEM_MOSSY_COBBLESTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:mossy_cobblestone_slab") as i32; // 810
const ITEM_MOSSY_COBBLESTONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mossy_cobblestone_stairs") as i32; // 792
const ITEM_MOSSY_COBBLESTONE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:mossy_cobblestone_wall") as i32; // 532
const ITEM_MOSSY_STONE_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:mossy_stone_brick_slab") as i32; // 808
const ITEM_MOSSY_STONE_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mossy_stone_brick_stairs") as i32; // 790
const ITEM_MOSSY_STONE_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:mossy_stone_brick_wall") as i32; // 536
const ITEM_MOSSY_STONE_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mossy_stone_bricks") as i32; // 450
const ITEM_MUD: i32 = registry_id(mc_registry::ITEMS, "minecraft:mud") as i32; // 59
const ITEM_MUD_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:mud_brick_slab") as i32; // 363
const ITEM_MUD_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mud_brick_stairs") as i32; // 495
const ITEM_MUD_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:mud_brick_wall") as i32; // 539
const ITEM_MUD_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:mud_bricks") as i32; // 454
const ITEM_MUDDY_MANGROVE_ROOTS: i32 = registry_id(mc_registry::ITEMS, "minecraft:muddy_mangrove_roots") as i32; // 174
const ITEM_MUSHROOM_STEW: i32 = registry_id(mc_registry::ITEMS, "minecraft:mushroom_stew") as i32; // 1061
const ITEM_MUSIC_DISC_5: i32 = registry_id(mc_registry::ITEMS, "minecraft:music_disc_5") as i32; // 1478
const ITEM_NAME_TAG: i32 = registry_id(mc_registry::ITEMS, "minecraft:name_tag") as i32; // 1413
const ITEM_NAUTILUS_SHELL: i32 = registry_id(mc_registry::ITEMS, "minecraft:nautilus_shell") as i32; // 1484
const ITEM_NETHER_BRICK: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_brick") as i32; // 1396
const ITEM_NETHER_BRICK_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_brick_fence") as i32; // 501
const ITEM_NETHER_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_brick_slab") as i32; // 364
const ITEM_NETHER_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_brick_stairs") as i32; // 502
const ITEM_NETHER_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_brick_wall") as i32; // 540
const ITEM_NETHER_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_bricks") as i32; // 498
const ITEM_NETHER_STAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_star") as i32; // 1391
const ITEM_NETHER_WART: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_wart") as i32; // 1269
const ITEM_NETHER_WART_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:nether_wart_block") as i32; // 651
const ITEM_NETHERITE_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_block") as i32; // 130
const ITEM_NETHERITE_INGOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_ingot") as i32; // 1023
const ITEM_NETHERITE_SCRAP: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_scrap") as i32; // 1024
const ITEM_NETHERITE_UPGRADE_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherite_upgrade_smithing_template") as i32; // 1579
const ITEM_NETHERRACK: i32 = registry_id(mc_registry::ITEMS, "minecraft:netherrack") as i32; // 433
const ITEM_NOTE_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:note_block") as i32; // 855
const ITEM_OAK_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_boat") as i32; // 975
const ITEM_OAK_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_button") as i32; // 858
const ITEM_OAK_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_chest_boat") as i32; // 976
const ITEM_OAK_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_door") as i32; // 889
const ITEM_OAK_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_fence") as i32; // 417
const ITEM_OAK_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_fence_gate") as i32; // 932
const ITEM_OAK_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_hanging_sign") as i32; // 1115
const OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_log") as i32; // 163
const OAK_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_planks") as i32; // 63
const ITEM_OAK_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_pressure_plate") as i32; // 875
const ITEM_OAK_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_shelf") as i32; // 386
const ITEM_OAK_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_sign") as i32; // 1102
const ITEM_OAK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_slab") as i32; // 341
const ITEM_OAK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_stairs") as i32; // 515
const ITEM_OAK_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_trapdoor") as i32; // 911
const ITEM_OAK_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:oak_wood") as i32; // 203
const ITEM_OBSERVER: i32 = registry_id(mc_registry::ITEMS, "minecraft:observer") as i32; // 833
const ITEM_OBSIDIAN: i32 = registry_id(mc_registry::ITEMS, "minecraft:obsidian") as i32; // 394
const ITEM_OPEN_EYEBLOSSOM: i32 = registry_id(mc_registry::ITEMS, "minecraft:open_eyeblossom") as i32; // 300
const ITEM_ORANGE_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_banner") as i32; // 1418
const ITEM_ORANGE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_bed") as i32; // 1220
const ITEM_ORANGE_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_candle") as i32; // 1552
const ITEM_ORANGE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_carpet") as i32; // 581
const ITEM_ORANGE_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_concrete") as i32; // 690
const ITEM_ORANGE_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_concrete_powder") as i32; // 738
const ITEM_ORANGE_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_concrete_slab") as i32; // 722
const ITEM_ORANGE_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_concrete_stairs") as i32; // 706
const ITEM_ORANGE_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_cushion") as i32; // 1171
const ITEM_ORANGE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_dye") as i32; // 1200
const ITEM_ORANGE_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_harness") as i32; // 951
const ITEM_ORANGE_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_stained_glass") as i32; // 606
const ITEM_ORANGE_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_stained_glass_pane") as i32; // 622
const ITEM_ORANGE_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_terracotta") as i32; // 562
const ITEM_ORANGE_TULIP: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_tulip") as i32; // 307
const ITEM_ORANGE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_wool") as i32; // 251
const ITEM_ORANGE_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_wool_slab") as i32; // 283
const ITEM_ORANGE_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:orange_wool_stairs") as i32; // 267
const ITEM_OXEYE_DAISY: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxeye_daisy") as i32; // 310
const ITEM_OXIDIZED_CHISELED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_chiseled_copper") as i32; // 134
const ITEM_OXIDIZED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper") as i32; // 123
const ITEM_OXIDIZED_COPPER_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_bars") as i32; // 468
const ITEM_OXIDIZED_COPPER_BULB: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_bulb") as i32; // 1632
const ITEM_OXIDIZED_COPPER_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_chain") as i32; // 477
const ITEM_OXIDIZED_COPPER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_chest") as i32; // 1640
const ITEM_OXIDIZED_COPPER_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_door") as i32; // 905
const ITEM_OXIDIZED_COPPER_GOLEM_STATUE: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_golem_statue") as i32; // 1648
const ITEM_OXIDIZED_COPPER_GRATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_grate") as i32; // 1624
const ITEM_OXIDIZED_COPPER_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_lantern") as i32; // 1520
const ITEM_OXIDIZED_COPPER_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_copper_trapdoor") as i32; // 927
const ITEM_OXIDIZED_CUT_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_cut_copper") as i32; // 142
const ITEM_OXIDIZED_CUT_COPPER_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_cut_copper_slab") as i32; // 158
const ITEM_OXIDIZED_CUT_COPPER_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_cut_copper_stairs") as i32; // 150
const ITEM_OXIDIZED_LIGHTNING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:oxidized_lightning_rod") as i32; // 843
const ITEM_PACKED_ICE: i32 = registry_id(mc_registry::ITEMS, "minecraft:packed_ice") as i32; // 597
const ITEM_PACKED_MUD: i32 = registry_id(mc_registry::ITEMS, "minecraft:packed_mud") as i32; // 453
const ITEM_PAINTING: i32 = registry_id(mc_registry::ITEMS, "minecraft:painting") as i32; // 1099
const ITEM_PALE_MOSS_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_moss_block") as i32; // 336
const ITEM_PALE_MOSS_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_moss_carpet") as i32; // 334
const ITEM_PALE_OAK_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_boat") as i32; // 989
const ITEM_PALE_OAK_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_button") as i32; // 865
const ITEM_PALE_OAK_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_chest_boat") as i32; // 990
const ITEM_PALE_OAK_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_door") as i32; // 896
const ITEM_PALE_OAK_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_fence") as i32; // 424
const ITEM_PALE_OAK_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_fence_gate") as i32; // 939
const ITEM_PALE_OAK_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_hanging_sign") as i32; // 1122
const ITEM_PALE_OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_log") as i32; // 169
const ITEM_PALE_OAK_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_planks") as i32; // 70
const ITEM_PALE_OAK_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_pressure_plate") as i32; // 882
const ITEM_PALE_OAK_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_shelf") as i32; // 387
const ITEM_PALE_OAK_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_sign") as i32; // 1109
const ITEM_PALE_OAK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_slab") as i32; // 348
const ITEM_PALE_OAK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_stairs") as i32; // 522
const ITEM_PALE_OAK_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_trapdoor") as i32; // 918
const ITEM_PALE_OAK_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:pale_oak_wood") as i32; // 209
const ITEM_PAPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:paper") as i32; // 1145
const ITEM_PEONY: i32 = registry_id(mc_registry::ITEMS, "minecraft:peony") as i32; // 602
const ITEM_PINK_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_banner") as i32; // 1423
const ITEM_PINK_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_bed") as i32; // 1225
const ITEM_PINK_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_candle") as i32; // 1557
const ITEM_PINK_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_carpet") as i32; // 586
const ITEM_PINK_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_concrete") as i32; // 695
const ITEM_PINK_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_concrete_powder") as i32; // 743
const ITEM_PINK_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_concrete_slab") as i32; // 727
const ITEM_PINK_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_concrete_stairs") as i32; // 711
const ITEM_PINK_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_cushion") as i32; // 1176
const ITEM_PINK_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_dye") as i32; // 1205
const ITEM_PINK_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_harness") as i32; // 956
const ITEM_PINK_PETALS: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_petals") as i32; // 329
const ITEM_PINK_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_stained_glass") as i32; // 611
const ITEM_PINK_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_stained_glass_pane") as i32; // 627
const ITEM_PINK_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_terracotta") as i32; // 567
const ITEM_PINK_TULIP: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_tulip") as i32; // 309
const ITEM_PINK_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_wool") as i32; // 256
const ITEM_PINK_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_wool_slab") as i32; // 288
const ITEM_PINK_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:pink_wool_stairs") as i32; // 272
const ITEM_PISTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:piston") as i32; // 829
const ITEM_PITCHER_PLANT: i32 = registry_id(mc_registry::ITEMS, "minecraft:pitcher_plant") as i32; // 315
const ITEM_POINTED_DRIPSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pointed_dripstone") as i32; // 1571
const ITEM_POLISHED_ANDESITE: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_andesite") as i32; // 7
const ITEM_POLISHED_ANDESITE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_andesite_slab") as i32; // 817
const ITEM_POLISHED_ANDESITE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_andesite_stairs") as i32; // 800
const ITEM_POLISHED_BASALT: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_basalt") as i32; // 437
const ITEM_POLISHED_BLACKSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone") as i32; // 1541
const ITEM_POLISHED_BLACKSTONE_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_brick_slab") as i32; // 1546
const ITEM_POLISHED_BLACKSTONE_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_brick_stairs") as i32; // 1547
const ITEM_POLISHED_BLACKSTONE_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_brick_wall") as i32; // 548
const ITEM_POLISHED_BLACKSTONE_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_bricks") as i32; // 1545
const ITEM_POLISHED_BLACKSTONE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_button") as i32; // 857
const ITEM_POLISHED_BLACKSTONE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_pressure_plate") as i32; // 872
const ITEM_POLISHED_BLACKSTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_slab") as i32; // 1542
const ITEM_POLISHED_BLACKSTONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_stairs") as i32; // 1543
const ITEM_POLISHED_BLACKSTONE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_blackstone_wall") as i32; // 547
const ITEM_POLISHED_CINNABAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_cinnabar") as i32; // 44
const ITEM_POLISHED_CINNABAR_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_cinnabar_slab") as i32; // 45
const ITEM_POLISHED_CINNABAR_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_cinnabar_stairs") as i32; // 46
const ITEM_POLISHED_CINNABAR_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_cinnabar_wall") as i32; // 47
const ITEM_POLISHED_DEEPSLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_deepslate") as i32; // 10
const ITEM_POLISHED_DEEPSLATE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_deepslate_slab") as i32; // 820
const ITEM_POLISHED_DEEPSLATE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_deepslate_stairs") as i32; // 803
const ITEM_POLISHED_DEEPSLATE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_deepslate_wall") as i32; // 550
const ITEM_POLISHED_DIORITE: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_diorite") as i32; // 5
const ITEM_POLISHED_DIORITE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_diorite_slab") as i32; // 809
const ITEM_POLISHED_DIORITE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_diorite_stairs") as i32; // 791
const ITEM_POLISHED_GRANITE: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_granite") as i32; // 3
const ITEM_POLISHED_GRANITE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_granite_slab") as i32; // 806
const ITEM_POLISHED_GRANITE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_granite_stairs") as i32; // 788
const ITEM_POLISHED_SULFUR: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_sulfur") as i32; // 31
const ITEM_POLISHED_SULFUR_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_sulfur_slab") as i32; // 32
const ITEM_POLISHED_SULFUR_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_sulfur_stairs") as i32; // 33
const ITEM_POLISHED_SULFUR_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_sulfur_wall") as i32; // 34
const ITEM_POLISHED_TUFF: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_tuff") as i32; // 17
const ITEM_POLISHED_TUFF_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_tuff_slab") as i32; // 18
const ITEM_POLISHED_TUFF_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_tuff_stairs") as i32; // 19
const ITEM_POLISHED_TUFF_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:polished_tuff_wall") as i32; // 20
const ITEM_POPLAR_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_boat") as i32; // 993
const ITEM_POPLAR_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_button") as i32; // 867
const ITEM_POPLAR_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_chest_boat") as i32; // 994
const ITEM_POPLAR_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_door") as i32; // 898
const ITEM_POPLAR_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_fence") as i32; // 426
const ITEM_POPLAR_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_fence_gate") as i32; // 941
const ITEM_POPLAR_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_hanging_sign") as i32; // 1124
const ITEM_POPLAR_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_log") as i32; // 172
const ITEM_POPLAR_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_planks") as i32; // 72
const ITEM_POPLAR_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_pressure_plate") as i32; // 884
const ITEM_POPLAR_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_shelf") as i32; // 385
const ITEM_POPLAR_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_sign") as i32; // 1111
const ITEM_POPLAR_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_slab") as i32; // 350
const ITEM_POPLAR_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_stairs") as i32; // 524
const ITEM_POPLAR_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_trapdoor") as i32; // 920
const ITEM_POPLAR_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:poplar_wood") as i32; // 212
const ITEM_POPPED_CHORUS_FRUIT: i32 = registry_id(mc_registry::ITEMS, "minecraft:popped_chorus_fruit") as i32; // 1435
const ITEM_POPPY: i32 = registry_id(mc_registry::ITEMS, "minecraft:poppy") as i32; // 302
const ITEM_POTENT_SULFUR: i32 = registry_id(mc_registry::ITEMS, "minecraft:potent_sulfur") as i32; // 27
const ITEM_POWERED_RAIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:powered_rail") as i32; // 945
const ITEM_PRISMARINE: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine") as i32; // 637
const ITEM_PRISMARINE_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine_brick_slab") as i32; // 370
const ITEM_PRISMARINE_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine_brick_stairs") as i32; // 641
const ITEM_PRISMARINE_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine_bricks") as i32; // 638
const ITEM_PRISMARINE_CRYSTALS: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine_crystals") as i32; // 1399
const ITEM_PRISMARINE_SHARD: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine_shard") as i32; // 1398
const ITEM_PRISMARINE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine_slab") as i32; // 369
const ITEM_PRISMARINE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine_stairs") as i32; // 640
const ITEM_PRISMARINE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:prismarine_wall") as i32; // 534
const ITEM_PUMPKIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:pumpkin") as i32; // 430
const ITEM_PUMPKIN_PIE: i32 = registry_id(mc_registry::ITEMS, "minecraft:pumpkin_pie") as i32; // 1392
const ITEM_PUMPKIN_SEEDS: i32 = registry_id(mc_registry::ITEMS, "minecraft:pumpkin_seeds") as i32; // 1258
const ITEM_PURPLE_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_banner") as i32; // 1427
const ITEM_PURPLE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_bed") as i32; // 1229
const ITEM_PURPLE_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_candle") as i32; // 1561
const ITEM_PURPLE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_carpet") as i32; // 590
const ITEM_PURPLE_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_concrete") as i32; // 699
const ITEM_PURPLE_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_concrete_powder") as i32; // 747
const ITEM_PURPLE_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_concrete_slab") as i32; // 731
const ITEM_PURPLE_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_concrete_stairs") as i32; // 715
const ITEM_PURPLE_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_cushion") as i32; // 1180
const ITEM_PURPLE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_dye") as i32; // 1209
const ITEM_PURPLE_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_harness") as i32; // 960
const ITEM_PURPLE_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_stained_glass") as i32; // 615
const ITEM_PURPLE_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_stained_glass_pane") as i32; // 631
const ITEM_PURPLE_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_terracotta") as i32; // 571
const ITEM_PURPLE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_wool") as i32; // 260
const ITEM_PURPLE_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_wool_slab") as i32; // 292
const ITEM_PURPLE_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:purple_wool_stairs") as i32; // 276
const ITEM_PURPUR_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:purpur_block") as i32; // 399
const ITEM_PURPUR_PILLAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:purpur_pillar") as i32; // 400
const ITEM_PURPUR_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:purpur_slab") as i32; // 368
const ITEM_PURPUR_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:purpur_stairs") as i32; // 401
const ITEM_QUARTZ: i32 = registry_id(mc_registry::ITEMS, "minecraft:quartz") as i32; // 1015
const ITEM_QUARTZ_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:quartz_block") as i32; // 557
const ITEM_QUARTZ_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:quartz_bricks") as i32; // 558
const ITEM_QUARTZ_PILLAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:quartz_pillar") as i32; // 559
const ITEM_QUARTZ_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:quartz_slab") as i32; // 365
const ITEM_QUARTZ_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:quartz_stairs") as i32; // 560
const ITEM_RABBIT_HIDE: i32 = registry_id(mc_registry::ITEMS, "minecraft:rabbit_hide") as i32; // 1404
const ITEM_RABBIT_STEW: i32 = registry_id(mc_registry::ITEMS, "minecraft:rabbit_stew") as i32; // 1402
const ITEM_RAIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:rail") as i32; // 947
const ITEM_RAISER_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:raiser_armor_trim_smithing_template") as i32; // 1594
const ITEM_RAW_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:raw_copper") as i32; // 1019
const ITEM_RAW_COPPER_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:raw_copper_block") as i32; // 114
const ITEM_RAW_GOLD: i32 = registry_id(mc_registry::ITEMS, "minecraft:raw_gold") as i32; // 1021
const ITEM_RAW_GOLD_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:raw_gold_block") as i32; // 115
const ITEM_RAW_IRON: i32 = registry_id(mc_registry::ITEMS, "minecraft:raw_iron") as i32; // 1017
const ITEM_RAW_IRON_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:raw_iron_block") as i32; // 113
const ITEM_RECOVERY_COMPASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:recovery_compass") as i32; // 1152
const ITEM_RED_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_banner") as i32; // 1431
const ITEM_RED_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_bed") as i32; // 1233
const ITEM_RED_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_candle") as i32; // 1565
const ITEM_RED_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_carpet") as i32; // 594
const ITEM_RED_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_concrete") as i32; // 703
const ITEM_RED_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_concrete_powder") as i32; // 751
const ITEM_RED_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_concrete_slab") as i32; // 735
const ITEM_RED_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_concrete_stairs") as i32; // 719
const ITEM_RED_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_cushion") as i32; // 1184
const ITEM_RED_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_dye") as i32; // 1213
const ITEM_RED_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_harness") as i32; // 964
const ITEM_RED_MUSHROOM: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_mushroom") as i32; // 318
const ITEM_RED_NETHER_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_nether_brick_slab") as i32; // 816
const ITEM_RED_NETHER_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_nether_brick_stairs") as i32; // 799
const ITEM_RED_NETHER_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_nether_brick_wall") as i32; // 542
const ITEM_RED_NETHER_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_nether_bricks") as i32; // 653
const ITEM_RED_SAND: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_sand") as i32; // 91
const ITEM_RED_SANDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_sandstone") as i32; // 644
const ITEM_RED_SANDSTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_sandstone_slab") as i32; // 366
const ITEM_RED_SANDSTONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_sandstone_stairs") as i32; // 647
const ITEM_RED_SANDSTONE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_sandstone_wall") as i32; // 535
const ITEM_RED_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_stained_glass") as i32; // 619
const ITEM_RED_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_stained_glass_pane") as i32; // 635
const ITEM_RED_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_terracotta") as i32; // 575
const ITEM_RED_TULIP: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_tulip") as i32; // 306
const ITEM_RED_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_wool") as i32; // 264
const ITEM_RED_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_wool_slab") as i32; // 296
const ITEM_RED_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:red_wool_stairs") as i32; // 280
const ITEM_REDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:redstone") as i32; // 824
const ITEM_REDSTONE_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:redstone_block") as i32; // 826
const ITEM_REDSTONE_LAMP: i32 = registry_id(mc_registry::ITEMS, "minecraft:redstone_lamp") as i32; // 854
const ITEM_REDSTONE_TORCH: i32 = registry_id(mc_registry::ITEMS, "minecraft:redstone_torch") as i32; // 825
const ITEM_REPEATER: i32 = registry_id(mc_registry::ITEMS, "minecraft:repeater") as i32; // 827
const ITEM_RESIN_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:resin_block") as i32; // 487
const ITEM_RESIN_BRICK: i32 = registry_id(mc_registry::ITEMS, "minecraft:resin_brick") as i32; // 1397
const ITEM_RESIN_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:resin_brick_slab") as i32; // 490
const ITEM_RESIN_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:resin_brick_stairs") as i32; // 489
const ITEM_RESIN_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:resin_brick_wall") as i32; // 491
const ITEM_RESIN_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:resin_bricks") as i32; // 488
const ITEM_RESIN_CLUMP: i32 = registry_id(mc_registry::ITEMS, "minecraft:resin_clump") as i32; // 486
const ITEM_RESPAWN_ANCHOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:respawn_anchor") as i32; // 1549
const ITEM_RIB_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:rib_armor_trim_smithing_template") as i32; // 1589
const ITEM_ROSE_BUSH: i32 = registry_id(mc_registry::ITEMS, "minecraft:rose_bush") as i32; // 601
const ITEM_SADDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:saddle") as i32; // 949
const ITEM_SAND: i32 = registry_id(mc_registry::ITEMS, "minecraft:sand") as i32; // 88
const ITEM_SANDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:sandstone") as i32; // 234
const ITEM_SANDSTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:sandstone_slab") as i32; // 357
const ITEM_SANDSTONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:sandstone_stairs") as i32; // 512
const ITEM_SANDSTONE_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:sandstone_wall") as i32; // 543
const ITEM_SCAFFOLDING: i32 = registry_id(mc_registry::ITEMS, "minecraft:scaffolding") as i32; // 823
const ITEM_SCULK_SENSOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:sculk_sensor") as i32; // 849
const ITEM_SEA_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:sea_lantern") as i32; // 643
const ITEM_SENTRY_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:sentry_armor_trim_smithing_template") as i32; // 1580
const ITEM_SHAPER_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:shaper_armor_trim_smithing_template") as i32; // 1592
const ITEM_SHEARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:shears") as i32; // 1255
const ITEM_SHELF_MUSHROOM: i32 = registry_id(mc_registry::ITEMS, "minecraft:shelf_mushroom") as i32; // 319
const ITEM_SHIELD: i32 = registry_id(mc_registry::ITEMS, "minecraft:shield") as i32; // 1446
const ITEM_SHULKER_BOX: i32 = registry_id(mc_registry::ITEMS, "minecraft:shulker_box") as i32; // 656
const ITEM_SHULKER_SHELL: i32 = registry_id(mc_registry::ITEMS, "minecraft:shulker_shell") as i32; // 1455
const ITEM_SILENCE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:silence_armor_trim_smithing_template") as i32; // 1593
const ITEM_SKULL_BANNER_PATTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:skull_banner_pattern") as i32; // 1496
const ITEM_SLIME_BALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:slime_ball") as i32; // 1147
const ITEM_SLIME_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:slime_block") as i32; // 831
const ITEM_SMITHING_TABLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:smithing_table") as i32; // 1512
const ITEM_SMOKER: i32 = registry_id(mc_registry::ITEMS, "minecraft:smoker") as i32; // 1507
const ITEM_SMOOTH_QUARTZ: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_quartz") as i32; // 372
const ITEM_SMOOTH_QUARTZ_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_quartz_slab") as i32; // 813
const ITEM_SMOOTH_QUARTZ_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_quartz_stairs") as i32; // 796
const ITEM_SMOOTH_RED_SANDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_red_sandstone") as i32; // 373
const ITEM_SMOOTH_RED_SANDSTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_red_sandstone_slab") as i32; // 807
const ITEM_SMOOTH_RED_SANDSTONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_red_sandstone_stairs") as i32; // 789
const ITEM_SMOOTH_SANDSTONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_sandstone") as i32; // 374
const ITEM_SMOOTH_SANDSTONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_sandstone_slab") as i32; // 812
const ITEM_SMOOTH_SANDSTONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_sandstone_stairs") as i32; // 795
const ITEM_SMOOTH_STONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_stone") as i32; // 375
const ITEM_SMOOTH_STONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:smooth_stone_slab") as i32; // 356
const ITEM_SNOUT_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:snout_armor_trim_smithing_template") as i32; // 1588
const ITEM_SNOW: i32 = registry_id(mc_registry::ITEMS, "minecraft:snow") as i32; // 410
const ITEM_SNOW_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:snow_block") as i32; // 412
const ITEM_SNOWBALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:snowball") as i32; // 1132
const ITEM_SOUL_CAMPFIRE: i32 = registry_id(mc_registry::ITEMS, "minecraft:soul_campfire") as i32; // 1528
const ITEM_SOUL_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:soul_lantern") as i32; // 1516
const ITEM_SOUL_SAND: i32 = registry_id(mc_registry::ITEMS, "minecraft:soul_sand") as i32; // 434
const ITEM_SOUL_SOIL: i32 = registry_id(mc_registry::ITEMS, "minecraft:soul_soil") as i32; // 435
const ITEM_SOUL_TORCH: i32 = registry_id(mc_registry::ITEMS, "minecraft:soul_torch") as i32; // 439
const ITEM_SPECTRAL_ARROW: i32 = registry_id(mc_registry::ITEMS, "minecraft:spectral_arrow") as i32; // 1443
const ITEM_SPIDER_EYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:spider_eye") as i32; // 1272
const ITEM_SPIRE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:spire_armor_trim_smithing_template") as i32; // 1590
const ITEM_SPRUCE_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_boat") as i32; // 977
const ITEM_SPRUCE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_button") as i32; // 859
const ITEM_SPRUCE_CHEST_BOAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_chest_boat") as i32; // 978
const ITEM_SPRUCE_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_door") as i32; // 890
const ITEM_SPRUCE_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_fence") as i32; // 418
const ITEM_SPRUCE_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_fence_gate") as i32; // 933
const ITEM_SPRUCE_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_hanging_sign") as i32; // 1116
const ITEM_SPRUCE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_log") as i32; // 164
const ITEM_SPRUCE_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_planks") as i32; // 64
const ITEM_SPRUCE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_pressure_plate") as i32; // 876
const ITEM_SPRUCE_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_shelf") as i32; // 388
const ITEM_SPRUCE_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_sign") as i32; // 1103
const ITEM_SPRUCE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_slab") as i32; // 342
const ITEM_SPRUCE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_stairs") as i32; // 516
const ITEM_SPRUCE_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_trapdoor") as i32; // 912
const ITEM_SPRUCE_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:spruce_wood") as i32; // 204
const ITEM_SPYGLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:spyglass") as i32; // 1188
const STICK: i32 = registry_id(mc_registry::ITEMS, "minecraft:stick") as i32; // 1060
const ITEM_STICKY_PISTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:sticky_piston") as i32; // 830
const ITEM_STONE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone") as i32; // 1
const ITEM_STONE_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_axe") as i32; // 1038
const ITEM_STONE_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_brick_slab") as i32; // 362
const ITEM_STONE_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_brick_stairs") as i32; // 494
const ITEM_STONE_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_brick_wall") as i32; // 538
const ITEM_STONE_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_bricks") as i32; // 449
const ITEM_STONE_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_button") as i32; // 856
const ITEM_STONE_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_hoe") as i32; // 1039
const STONE_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_pickaxe") as i32; // 1037
const ITEM_STONE_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_pressure_plate") as i32; // 871
const ITEM_STONE_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_shovel") as i32; // 1036
const ITEM_STONE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_slab") as i32; // 355
const ITEM_STONE_SPEAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_spear") as i32; // 1448
const ITEM_STONE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_stairs") as i32; // 794
const ITEM_STONE_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stone_sword") as i32; // 1035
const ITEM_STONECUTTER: i32 = registry_id(mc_registry::ITEMS, "minecraft:stonecutter") as i32; // 1513
const ITEM_STRAW_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:straw_bed") as i32; // 1235
const ITEM_STRING: i32 = registry_id(mc_registry::ITEMS, "minecraft:string") as i32; // 1062
const ITEM_STRIPPED_ACACIA_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_acacia_log") as i32; // 182
const ITEM_STRIPPED_ACACIA_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_acacia_wood") as i32; // 194
const ITEM_STRIPPED_BAMBOO_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_bamboo_block") as i32; // 202
const ITEM_STRIPPED_BIRCH_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_birch_log") as i32; // 180
const ITEM_STRIPPED_BIRCH_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_birch_wood") as i32; // 192
const ITEM_STRIPPED_CHERRY_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_cherry_log") as i32; // 183
const ITEM_STRIPPED_CHERRY_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_cherry_wood") as i32; // 195
const ITEM_STRIPPED_CRIMSON_HYPHAE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_crimson_hyphae") as i32; // 200
const ITEM_STRIPPED_CRIMSON_STEM: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_crimson_stem") as i32; // 188
const ITEM_STRIPPED_DARK_OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_dark_oak_log") as i32; // 184
const ITEM_STRIPPED_DARK_OAK_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_dark_oak_wood") as i32; // 196
const ITEM_STRIPPED_JUNGLE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_jungle_log") as i32; // 181
const ITEM_STRIPPED_JUNGLE_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_jungle_wood") as i32; // 193
const ITEM_STRIPPED_MANGROVE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_mangrove_log") as i32; // 186
const ITEM_STRIPPED_MANGROVE_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_mangrove_wood") as i32; // 198
const ITEM_STRIPPED_OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_oak_log") as i32; // 178
const ITEM_STRIPPED_OAK_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_oak_wood") as i32; // 190
const ITEM_STRIPPED_PALE_OAK_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_pale_oak_log") as i32; // 185
const ITEM_STRIPPED_PALE_OAK_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_pale_oak_wood") as i32; // 197
const ITEM_STRIPPED_POPLAR_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_poplar_log") as i32; // 187
const ITEM_STRIPPED_POPLAR_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_poplar_wood") as i32; // 199
const ITEM_STRIPPED_SPRUCE_LOG: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_spruce_log") as i32; // 179
const ITEM_STRIPPED_SPRUCE_WOOD: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_spruce_wood") as i32; // 191
const ITEM_STRIPPED_WARPED_HYPHAE: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_warped_hyphae") as i32; // 201
const ITEM_STRIPPED_WARPED_STEM: i32 = registry_id(mc_registry::ITEMS, "minecraft:stripped_warped_stem") as i32; // 189
const ITEM_SUGAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:sugar") as i32; // 1217
const ITEM_SUGAR_CANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:sugar_cane") as i32; // 327
const ITEM_SULFUR: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur") as i32; // 26
const ITEM_SULFUR_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur_brick_slab") as i32; // 36
const ITEM_SULFUR_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur_brick_stairs") as i32; // 37
const ITEM_SULFUR_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur_brick_wall") as i32; // 38
const ITEM_SULFUR_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur_bricks") as i32; // 35
const ITEM_SULFUR_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur_slab") as i32; // 28
const ITEM_SULFUR_SPIKE: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur_spike") as i32; // 1572
const ITEM_SULFUR_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur_stairs") as i32; // 29
const ITEM_SULFUR_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:sulfur_wall") as i32; // 30
const ITEM_SUNFLOWER: i32 = registry_id(mc_registry::ITEMS, "minecraft:sunflower") as i32; // 599
const ITEM_TARGET: i32 = registry_id(mc_registry::ITEMS, "minecraft:target") as i32; // 838
const ITEM_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:terracotta") as i32; // 596
const ITEM_TIDE_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:tide_armor_trim_smithing_template") as i32; // 1587
const ITEM_TINTED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:tinted_glass") as i32; // 232
const ITEM_TNT: i32 = registry_id(mc_registry::ITEMS, "minecraft:tnt") as i32; // 853
const ITEM_TNT_MINECART: i32 = registry_id(mc_registry::ITEMS, "minecraft:tnt_minecart") as i32; // 969
const TORCH: i32 = registry_id(mc_registry::ITEMS, "minecraft:torch") as i32; // 395
const ITEM_TORCHFLOWER: i32 = registry_id(mc_registry::ITEMS, "minecraft:torchflower") as i32; // 314
const ITEM_TRAPPED_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:trapped_chest") as i32; // 852
const ITEM_TRIPWIRE_HOOK: i32 = registry_id(mc_registry::ITEMS, "minecraft:tripwire_hook") as i32; // 851
const ITEM_TUFF: i32 = registry_id(mc_registry::ITEMS, "minecraft:tuff") as i32; // 12
const ITEM_TUFF_BRICK_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:tuff_brick_slab") as i32; // 22
const ITEM_TUFF_BRICK_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:tuff_brick_stairs") as i32; // 23
const ITEM_TUFF_BRICK_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:tuff_brick_wall") as i32; // 24
const ITEM_TUFF_BRICKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:tuff_bricks") as i32; // 21
const ITEM_TUFF_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:tuff_slab") as i32; // 13
const ITEM_TUFF_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:tuff_stairs") as i32; // 14
const ITEM_TUFF_WALL: i32 = registry_id(mc_registry::ITEMS, "minecraft:tuff_wall") as i32; // 15
const ITEM_TURTLE_HELMET: i32 = registry_id(mc_registry::ITEMS, "minecraft:turtle_helmet") as i32; // 1001
const ITEM_TURTLE_SCUTE: i32 = registry_id(mc_registry::ITEMS, "minecraft:turtle_scute") as i32; // 1002
const ITEM_VEX_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:vex_armor_trim_smithing_template") as i32; // 1586
const ITEM_VINE: i32 = registry_id(mc_registry::ITEMS, "minecraft:vine") as i32; // 484
const ITEM_WARD_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:ward_armor_trim_smithing_template") as i32; // 1584
const ITEM_WARPED_BUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_button") as i32; // 870
const ITEM_WARPED_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_door") as i32; // 901
const ITEM_WARPED_FENCE: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_fence") as i32; // 429
const ITEM_WARPED_FENCE_GATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_fence_gate") as i32; // 944
const ITEM_WARPED_FUNGUS: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_fungus") as i32; // 321
const ITEM_WARPED_FUNGUS_ON_A_STICK: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_fungus_on_a_stick") as i32; // 972
const ITEM_WARPED_HANGING_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_hanging_sign") as i32; // 1127
const ITEM_WARPED_HYPHAE: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_hyphae") as i32; // 214
const ITEM_WARPED_PLANKS: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_planks") as i32; // 75
const ITEM_WARPED_PRESSURE_PLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_pressure_plate") as i32; // 887
const ITEM_WARPED_SHELF: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_shelf") as i32; // 389
const ITEM_WARPED_SIGN: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_sign") as i32; // 1114
const ITEM_WARPED_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_slab") as i32; // 354
const ITEM_WARPED_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_stairs") as i32; // 528
const ITEM_WARPED_STEM: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_stem") as i32; // 176
const ITEM_WARPED_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:warped_trapdoor") as i32; // 923
const ITEM_WAXED_CHISELED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_chiseled_copper") as i32; // 135
const ITEM_WAXED_COPPER_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_bars") as i32; // 469
const ITEM_WAXED_COPPER_BLOCK: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_block") as i32; // 124
const ITEM_WAXED_COPPER_BULB: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_bulb") as i32; // 1633
const ITEM_WAXED_COPPER_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_chain") as i32; // 478
const ITEM_WAXED_COPPER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_chest") as i32; // 1641
const ITEM_WAXED_COPPER_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_door") as i32; // 906
const ITEM_WAXED_COPPER_GOLEM_STATUE: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_golem_statue") as i32; // 1649
const ITEM_WAXED_COPPER_GRATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_grate") as i32; // 1625
const ITEM_WAXED_COPPER_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_lantern") as i32; // 1521
const ITEM_WAXED_COPPER_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_copper_trapdoor") as i32; // 928
const ITEM_WAXED_CUT_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_cut_copper") as i32; // 143
const ITEM_WAXED_CUT_COPPER_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_cut_copper_slab") as i32; // 159
const ITEM_WAXED_CUT_COPPER_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_cut_copper_stairs") as i32; // 151
const ITEM_WAXED_EXPOSED_CHISELED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_chiseled_copper") as i32; // 136
const ITEM_WAXED_EXPOSED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper") as i32; // 125
const ITEM_WAXED_EXPOSED_COPPER_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_bars") as i32; // 470
const ITEM_WAXED_EXPOSED_COPPER_BULB: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_bulb") as i32; // 1634
const ITEM_WAXED_EXPOSED_COPPER_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_chain") as i32; // 479
const ITEM_WAXED_EXPOSED_COPPER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_chest") as i32; // 1642
const ITEM_WAXED_EXPOSED_COPPER_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_door") as i32; // 907
const ITEM_WAXED_EXPOSED_COPPER_GOLEM_STATUE: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_golem_statue") as i32; // 1650
const ITEM_WAXED_EXPOSED_COPPER_GRATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_grate") as i32; // 1626
const ITEM_WAXED_EXPOSED_COPPER_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_lantern") as i32; // 1522
const ITEM_WAXED_EXPOSED_COPPER_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_copper_trapdoor") as i32; // 929
const ITEM_WAXED_EXPOSED_CUT_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_cut_copper") as i32; // 144
const ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_cut_copper_slab") as i32; // 160
const ITEM_WAXED_EXPOSED_CUT_COPPER_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_cut_copper_stairs") as i32; // 152
const ITEM_WAXED_EXPOSED_LIGHTNING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_exposed_lightning_rod") as i32; // 845
const ITEM_WAXED_LIGHTNING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_lightning_rod") as i32; // 844
const ITEM_WAXED_OXIDIZED_CHISELED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_chiseled_copper") as i32; // 138
const ITEM_WAXED_OXIDIZED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper") as i32; // 127
const ITEM_WAXED_OXIDIZED_COPPER_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_bars") as i32; // 472
const ITEM_WAXED_OXIDIZED_COPPER_BULB: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_bulb") as i32; // 1636
const ITEM_WAXED_OXIDIZED_COPPER_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_chain") as i32; // 481
const ITEM_WAXED_OXIDIZED_COPPER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_chest") as i32; // 1644
const ITEM_WAXED_OXIDIZED_COPPER_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_door") as i32; // 909
const ITEM_WAXED_OXIDIZED_COPPER_GOLEM_STATUE: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_golem_statue") as i32; // 1652
const ITEM_WAXED_OXIDIZED_COPPER_GRATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_grate") as i32; // 1628
const ITEM_WAXED_OXIDIZED_COPPER_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_lantern") as i32; // 1524
const ITEM_WAXED_OXIDIZED_COPPER_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_copper_trapdoor") as i32; // 931
const ITEM_WAXED_OXIDIZED_CUT_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_cut_copper") as i32; // 146
const ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_cut_copper_slab") as i32; // 162
const ITEM_WAXED_OXIDIZED_CUT_COPPER_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_cut_copper_stairs") as i32; // 154
const ITEM_WAXED_OXIDIZED_LIGHTNING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_oxidized_lightning_rod") as i32; // 847
const ITEM_WAXED_WEATHERED_CHISELED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_chiseled_copper") as i32; // 137
const ITEM_WAXED_WEATHERED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper") as i32; // 126
const ITEM_WAXED_WEATHERED_COPPER_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_bars") as i32; // 471
const ITEM_WAXED_WEATHERED_COPPER_BULB: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_bulb") as i32; // 1635
const ITEM_WAXED_WEATHERED_COPPER_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_chain") as i32; // 480
const ITEM_WAXED_WEATHERED_COPPER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_chest") as i32; // 1643
const ITEM_WAXED_WEATHERED_COPPER_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_door") as i32; // 908
const ITEM_WAXED_WEATHERED_COPPER_GOLEM_STATUE: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_golem_statue") as i32; // 1651
const ITEM_WAXED_WEATHERED_COPPER_GRATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_grate") as i32; // 1627
const ITEM_WAXED_WEATHERED_COPPER_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_lantern") as i32; // 1523
const ITEM_WAXED_WEATHERED_COPPER_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_copper_trapdoor") as i32; // 930
const ITEM_WAXED_WEATHERED_CUT_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_cut_copper") as i32; // 145
const ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_cut_copper_slab") as i32; // 161
const ITEM_WAXED_WEATHERED_CUT_COPPER_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_cut_copper_stairs") as i32; // 153
const ITEM_WAXED_WEATHERED_LIGHTNING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:waxed_weathered_lightning_rod") as i32; // 846
const ITEM_WAYFINDER_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wayfinder_armor_trim_smithing_template") as i32; // 1591
const ITEM_WEATHERED_CHISELED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_chiseled_copper") as i32; // 133
const ITEM_WEATHERED_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper") as i32; // 122
const ITEM_WEATHERED_COPPER_BARS: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_bars") as i32; // 467
const ITEM_WEATHERED_COPPER_BULB: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_bulb") as i32; // 1631
const ITEM_WEATHERED_COPPER_CHAIN: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_chain") as i32; // 476
const ITEM_WEATHERED_COPPER_CHEST: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_chest") as i32; // 1639
const ITEM_WEATHERED_COPPER_DOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_door") as i32; // 904
const ITEM_WEATHERED_COPPER_GOLEM_STATUE: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_golem_statue") as i32; // 1647
const ITEM_WEATHERED_COPPER_GRATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_grate") as i32; // 1623
const ITEM_WEATHERED_COPPER_LANTERN: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_lantern") as i32; // 1519
const ITEM_WEATHERED_COPPER_TRAPDOOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_copper_trapdoor") as i32; // 926
const ITEM_WEATHERED_CUT_COPPER: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_cut_copper") as i32; // 141
const ITEM_WEATHERED_CUT_COPPER_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_cut_copper_slab") as i32; // 157
const ITEM_WEATHERED_CUT_COPPER_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_cut_copper_stairs") as i32; // 149
const ITEM_WEATHERED_LIGHTNING_ROD: i32 = registry_id(mc_registry::ITEMS, "minecraft:weathered_lightning_rod") as i32; // 842
const WHEAT: i32 = registry_id(mc_registry::ITEMS, "minecraft:wheat") as i32; // 1066
const ITEM_WHITE_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_banner") as i32; // 1417
const ITEM_WHITE_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_bed") as i32; // 1219
const ITEM_WHITE_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_candle") as i32; // 1551
const ITEM_WHITE_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_carpet") as i32; // 580
const ITEM_WHITE_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_concrete") as i32; // 689
const ITEM_WHITE_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_concrete_powder") as i32; // 737
const ITEM_WHITE_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_concrete_slab") as i32; // 721
const ITEM_WHITE_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_concrete_stairs") as i32; // 705
const ITEM_WHITE_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_cushion") as i32; // 1170
const ITEM_WHITE_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_dye") as i32; // 1199
const ITEM_WHITE_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_harness") as i32; // 950
const ITEM_WHITE_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_stained_glass") as i32; // 605
const ITEM_WHITE_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_stained_glass_pane") as i32; // 621
const ITEM_WHITE_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_terracotta") as i32; // 561
const ITEM_WHITE_TULIP: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_tulip") as i32; // 308
const ITEM_WHITE_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_wool") as i32; // 250
const ITEM_WHITE_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_wool_slab") as i32; // 282
const ITEM_WHITE_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:white_wool_stairs") as i32; // 266
const ITEM_WILD_ARMOR_TRIM_SMITHING_TEMPLATE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wild_armor_trim_smithing_template") as i32; // 1583
const ITEM_WILDFLOWERS: i32 = registry_id(mc_registry::ITEMS, "minecraft:wildflowers") as i32; // 330
const ITEM_WIND_CHARGE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wind_charge") as i32; // 1370
const ITEM_WITHER_ROSE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wither_rose") as i32; // 313
const ITEM_WITHER_SKELETON_SKULL: i32 = registry_id(mc_registry::ITEMS, "minecraft:wither_skeleton_skull") as i32; // 1385
const ITEM_WOLF_ARMOR: i32 = registry_id(mc_registry::ITEMS, "minecraft:wolf_armor") as i32; // 1004
const ITEM_WOODEN_AXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_axe") as i32; // 1028
const ITEM_WOODEN_HOE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_hoe") as i32; // 1029
const WOODEN_PICKAXE: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_pickaxe") as i32; // 1027
const ITEM_WOODEN_SHOVEL: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_shovel") as i32; // 1026
const ITEM_WOODEN_SPEAR: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_spear") as i32; // 1447
const ITEM_WOODEN_SWORD: i32 = registry_id(mc_registry::ITEMS, "minecraft:wooden_sword") as i32; // 1025
const ITEM_WRITABLE_BOOK: i32 = registry_id(mc_registry::ITEMS, "minecraft:writable_book") as i32; // 1371
const ITEM_YELLOW_BANNER: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_banner") as i32; // 1421
const ITEM_YELLOW_BED: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_bed") as i32; // 1223
const ITEM_YELLOW_CANDLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_candle") as i32; // 1555
const ITEM_YELLOW_CARPET: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_carpet") as i32; // 584
const ITEM_YELLOW_CONCRETE: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_concrete") as i32; // 693
const ITEM_YELLOW_CONCRETE_POWDER: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_concrete_powder") as i32; // 741
const ITEM_YELLOW_CONCRETE_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_concrete_slab") as i32; // 725
const ITEM_YELLOW_CONCRETE_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_concrete_stairs") as i32; // 709
const ITEM_YELLOW_CUSHION: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_cushion") as i32; // 1174
const ITEM_YELLOW_DYE: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_dye") as i32; // 1203
const ITEM_YELLOW_HARNESS: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_harness") as i32; // 954
const ITEM_YELLOW_STAINED_GLASS: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_stained_glass") as i32; // 609
const ITEM_YELLOW_STAINED_GLASS_PANE: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_stained_glass_pane") as i32; // 625
const ITEM_YELLOW_TERRACOTTA: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_terracotta") as i32; // 565
const ITEM_YELLOW_WOOL: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_wool") as i32; // 254
const ITEM_YELLOW_WOOL_SLAB: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_wool_slab") as i32; // 286
const ITEM_YELLOW_WOOL_STAIRS: i32 = registry_id(mc_registry::ITEMS, "minecraft:yellow_wool_stairs") as i32; // 270
// === GEN-CONSTS-END ===

/// MVP 配方表（均为原版 2×2 可合成口径）：
/// 原木→4 木板；2 木板→4 木棍；4 木板→工作台；
/// 3 木板+2 木棍→木镐；3 圆石+2 木棍→石镐；3 小麦→面包；煤+木棍→4 火把。
// === GEN-TAGS-START === (scripts/gen_recipes.py 生成；勿手改)
/// 合成标签成员表（负数 ID 引用）：need id = -(index+1)。
pub static CRAFT_TAGS: &[&[i32]] = &[
    &[ITEM_ACACIA_LOG, ITEM_ACACIA_WOOD, ITEM_BIRCH_LOG, ITEM_BIRCH_WOOD, ITEM_CHERRY_LOG, ITEM_CHERRY_WOOD, ITEM_CRIMSON_HYPHAE, ITEM_CRIMSON_STEM, ITEM_DARK_OAK_LOG, ITEM_DARK_OAK_WOOD, ITEM_JUNGLE_LOG, ITEM_JUNGLE_WOOD, ITEM_MANGROVE_LOG, ITEM_MANGROVE_WOOD, OAK_LOG, ITEM_OAK_WOOD, ITEM_PALE_OAK_LOG, ITEM_PALE_OAK_WOOD, ITEM_POPLAR_LOG, ITEM_POPLAR_WOOD, ITEM_SPRUCE_LOG, ITEM_SPRUCE_WOOD, ITEM_STRIPPED_ACACIA_LOG, ITEM_STRIPPED_ACACIA_WOOD, ITEM_STRIPPED_BIRCH_LOG, ITEM_STRIPPED_BIRCH_WOOD, ITEM_STRIPPED_CHERRY_LOG, ITEM_STRIPPED_CHERRY_WOOD, ITEM_STRIPPED_CRIMSON_HYPHAE, ITEM_STRIPPED_CRIMSON_STEM, ITEM_STRIPPED_DARK_OAK_LOG, ITEM_STRIPPED_DARK_OAK_WOOD, ITEM_STRIPPED_JUNGLE_LOG, ITEM_STRIPPED_JUNGLE_WOOD, ITEM_STRIPPED_MANGROVE_LOG, ITEM_STRIPPED_MANGROVE_WOOD, ITEM_STRIPPED_OAK_LOG, ITEM_STRIPPED_OAK_WOOD, ITEM_STRIPPED_PALE_OAK_LOG, ITEM_STRIPPED_PALE_OAK_WOOD, ITEM_STRIPPED_POPLAR_LOG, ITEM_STRIPPED_POPLAR_WOOD, ITEM_STRIPPED_SPRUCE_LOG, ITEM_STRIPPED_SPRUCE_WOOD, ITEM_STRIPPED_WARPED_HYPHAE, ITEM_STRIPPED_WARPED_STEM, ITEM_WARPED_HYPHAE, ITEM_WARPED_STEM],
    &[ITEM_ACACIA_LOG, ITEM_ACACIA_WOOD, ITEM_STRIPPED_ACACIA_LOG, ITEM_STRIPPED_ACACIA_WOOD],
    &[ITEM_ACACIA_PLANKS, ITEM_BAMBOO_PLANKS, ITEM_BIRCH_PLANKS, ITEM_CHERRY_PLANKS, ITEM_CRIMSON_PLANKS, ITEM_DARK_OAK_PLANKS, ITEM_JUNGLE_PLANKS, ITEM_MANGROVE_PLANKS, OAK_PLANKS, ITEM_PALE_OAK_PLANKS, ITEM_POPLAR_PLANKS, ITEM_SPRUCE_PLANKS, ITEM_WARPED_PLANKS],
    &[ITEM_ACACIA_SLAB, ITEM_BAMBOO_SLAB, ITEM_BIRCH_SLAB, ITEM_CHERRY_SLAB, ITEM_CRIMSON_SLAB, ITEM_DARK_OAK_SLAB, ITEM_JUNGLE_SLAB, ITEM_MANGROVE_SLAB, ITEM_OAK_SLAB, ITEM_PALE_OAK_SLAB, ITEM_POPLAR_SLAB, ITEM_SPRUCE_SLAB, ITEM_WARPED_SLAB],
    &[ITEM_BAMBOO_BLOCK, ITEM_STRIPPED_BAMBOO_BLOCK],
    &[ITEM_BIRCH_LOG, ITEM_BIRCH_WOOD, ITEM_STRIPPED_BIRCH_LOG, ITEM_STRIPPED_BIRCH_WOOD],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BLUE_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BLUE_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BLUE_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BLUE_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BLUE_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BLUE_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BLUE_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACK_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BLACKSTONE, ITEM_COBBLED_DEEPSLATE, COBBLESTONE],
    &[ITEM_BLUE_BED, ITEM_BROWN_BED, ITEM_CYAN_BED, ITEM_GRAY_BED, ITEM_GREEN_BED, ITEM_LIGHT_BLUE_BED, ITEM_LIGHT_GRAY_BED, ITEM_LIME_BED, ITEM_MAGENTA_BED, ITEM_ORANGE_BED, ITEM_PINK_BED, ITEM_PURPLE_BED, ITEM_RED_BED, ITEM_WHITE_BED, ITEM_YELLOW_BED],
    &[ITEM_BLUE_CARPET, ITEM_BROWN_CARPET, ITEM_CYAN_CARPET, ITEM_GRAY_CARPET, ITEM_GREEN_CARPET, ITEM_LIGHT_BLUE_CARPET, ITEM_LIGHT_GRAY_CARPET, ITEM_LIME_CARPET, ITEM_MAGENTA_CARPET, ITEM_ORANGE_CARPET, ITEM_PINK_CARPET, ITEM_PURPLE_CARPET, ITEM_RED_CARPET, ITEM_WHITE_CARPET, ITEM_YELLOW_CARPET],
    &[ITEM_BLUE_CUSHION, ITEM_BROWN_CUSHION, ITEM_CYAN_CUSHION, ITEM_GRAY_CUSHION, ITEM_GREEN_CUSHION, ITEM_LIGHT_BLUE_CUSHION, ITEM_LIGHT_GRAY_CUSHION, ITEM_LIME_CUSHION, ITEM_MAGENTA_CUSHION, ITEM_ORANGE_CUSHION, ITEM_PINK_CUSHION, ITEM_PURPLE_CUSHION, ITEM_RED_CUSHION, ITEM_WHITE_CUSHION, ITEM_YELLOW_CUSHION],
    &[ITEM_BLUE_EGG, ITEM_BROWN_EGG, ITEM_EGG],
    &[ITEM_BLUE_HARNESS, ITEM_BROWN_HARNESS, ITEM_CYAN_HARNESS, ITEM_GRAY_HARNESS, ITEM_GREEN_HARNESS, ITEM_LIGHT_BLUE_HARNESS, ITEM_LIGHT_GRAY_HARNESS, ITEM_LIME_HARNESS, ITEM_MAGENTA_HARNESS, ITEM_ORANGE_HARNESS, ITEM_PINK_HARNESS, ITEM_PURPLE_HARNESS, ITEM_RED_HARNESS, ITEM_WHITE_HARNESS, ITEM_YELLOW_HARNESS],
    &[ITEM_BLUE_WOOL, ITEM_BROWN_WOOL, ITEM_CYAN_WOOL, ITEM_GRAY_WOOL, ITEM_GREEN_WOOL, ITEM_LIGHT_BLUE_WOOL, ITEM_LIGHT_GRAY_WOOL, ITEM_LIME_WOOL, ITEM_MAGENTA_WOOL, ITEM_ORANGE_WOOL, ITEM_PINK_WOOL, ITEM_PURPLE_WOOL, ITEM_RED_WOOL, ITEM_WHITE_WOOL, ITEM_YELLOW_WOOL],
    &[ITEM_BLUE_WOOL_SLAB, ITEM_BROWN_WOOL_SLAB, ITEM_CYAN_WOOL_SLAB, ITEM_GRAY_WOOL_SLAB, ITEM_GREEN_WOOL_SLAB, ITEM_LIGHT_BLUE_WOOL_SLAB, ITEM_LIGHT_GRAY_WOOL_SLAB, ITEM_LIME_WOOL_SLAB, ITEM_MAGENTA_WOOL_SLAB, ITEM_ORANGE_WOOL_SLAB, ITEM_PINK_WOOL_SLAB, ITEM_PURPLE_WOOL_SLAB, ITEM_RED_WOOL_SLAB, ITEM_WHITE_WOOL_SLAB, ITEM_YELLOW_WOOL_SLAB],
    &[ITEM_BLUE_WOOL_STAIRS, ITEM_BROWN_WOOL_STAIRS, ITEM_CYAN_WOOL_STAIRS, ITEM_GRAY_WOOL_STAIRS, ITEM_GREEN_WOOL_STAIRS, ITEM_LIGHT_BLUE_WOOL_STAIRS, ITEM_LIGHT_GRAY_WOOL_STAIRS, ITEM_LIME_WOOL_STAIRS, ITEM_MAGENTA_WOOL_STAIRS, ITEM_ORANGE_WOOL_STAIRS, ITEM_PINK_WOOL_STAIRS, ITEM_PURPLE_WOOL_STAIRS, ITEM_RED_WOOL_STAIRS, ITEM_WHITE_WOOL_STAIRS, ITEM_YELLOW_WOOL_STAIRS],
    &[ITEM_BROWN_MUSHROOM, ITEM_RED_MUSHROOM, ITEM_SHELF_MUSHROOM],
    &[ITEM_CHARCOAL, COAL],
    &[ITEM_CHERRY_LOG, ITEM_CHERRY_WOOD, ITEM_STRIPPED_CHERRY_LOG, ITEM_STRIPPED_CHERRY_WOOD],
    &[ITEM_CHISELED_QUARTZ_BLOCK, ITEM_QUARTZ_BLOCK, ITEM_QUARTZ_PILLAR],
    &[ITEM_CHISELED_RED_SANDSTONE, ITEM_CUT_RED_SANDSTONE, ITEM_RED_SANDSTONE],
    &[ITEM_CHISELED_RED_SANDSTONE, ITEM_RED_SANDSTONE],
    &[ITEM_CHISELED_SANDSTONE, ITEM_CUT_SANDSTONE, ITEM_SANDSTONE],
    &[ITEM_CHISELED_SANDSTONE, ITEM_SANDSTONE],
    &[ITEM_COPPER_BLOCK, ITEM_WAXED_COPPER_BLOCK],
    &[ITEM_COPPER_INGOT],
    &[ITEM_COPPER_NUGGET, ITEM_GOLD_NUGGET, ITEM_IRON_NUGGET],
    &[ITEM_CRIMSON_HYPHAE, ITEM_CRIMSON_STEM, ITEM_STRIPPED_CRIMSON_HYPHAE, ITEM_STRIPPED_CRIMSON_STEM],
    &[ITEM_DARK_OAK_LOG, ITEM_DARK_OAK_WOOD, ITEM_STRIPPED_DARK_OAK_LOG, ITEM_STRIPPED_DARK_OAK_WOOD],
    &[ITEM_DIAMOND],
    &[ITEM_GOLD_INGOT],
    &[ITEM_IRON_INGOT],
    &[ITEM_JUNGLE_LOG, ITEM_JUNGLE_WOOD, ITEM_STRIPPED_JUNGLE_LOG, ITEM_STRIPPED_JUNGLE_WOOD],
    &[ITEM_MANGROVE_LOG, ITEM_MANGROVE_WOOD, ITEM_STRIPPED_MANGROVE_LOG, ITEM_STRIPPED_MANGROVE_WOOD],
    &[OAK_LOG, ITEM_OAK_WOOD, ITEM_STRIPPED_OAK_LOG, ITEM_STRIPPED_OAK_WOOD],
    &[ITEM_PALE_OAK_LOG, ITEM_PALE_OAK_WOOD, ITEM_STRIPPED_PALE_OAK_LOG, ITEM_STRIPPED_PALE_OAK_WOOD],
    &[ITEM_POPLAR_LOG, ITEM_POPLAR_WOOD, ITEM_STRIPPED_POPLAR_LOG, ITEM_STRIPPED_POPLAR_WOOD],
    &[ITEM_PURPUR_BLOCK, ITEM_PURPUR_PILLAR],
    &[ITEM_RED_SAND, ITEM_SAND],
    &[ITEM_SOUL_SAND, ITEM_SOUL_SOIL],
    &[ITEM_SPRUCE_LOG, ITEM_SPRUCE_WOOD, ITEM_STRIPPED_SPRUCE_LOG, ITEM_STRIPPED_SPRUCE_WOOD],
    &[ITEM_STRIPPED_WARPED_HYPHAE, ITEM_STRIPPED_WARPED_STEM, ITEM_WARPED_HYPHAE, ITEM_WARPED_STEM],
];
const TAG_LOGS: i32 = -1;
const TAG_ACACIA_LOGS: i32 = -2;
const TAG_PLANKS: i32 = -3;
const TAG_WOODEN_SLABS: i32 = -4;
const TAG_BAMBOO_BLOCKS: i32 = -5;
const TAG_BIRCH_LOGS: i32 = -6;
const TAG_ALT_107: i32 = -7;
const TAG_ALT_100: i32 = -8;
const TAG_ALT_93: i32 = -9;
const TAG_ALT_86: i32 = -10;
const TAG_ALT_79: i32 = -11;
const TAG_ALT_72: i32 = -12;
const TAG_ALT_65: i32 = -13;
const TAG_ALT_58: i32 = -14;
const TAG_ALT_51: i32 = -15;
const TAG_ALT_44: i32 = -16;
const TAG_ALT_37: i32 = -17;
const TAG_ALT_30: i32 = -18;
const TAG_ALT_23: i32 = -19;
const TAG_ALT_16: i32 = -20;
const TAG_ALT_9: i32 = -21;
const TAG_ALT_108: i32 = -22;
const TAG_ALT_101: i32 = -23;
const TAG_ALT_94: i32 = -24;
const TAG_ALT_87: i32 = -25;
const TAG_ALT_80: i32 = -26;
const TAG_ALT_73: i32 = -27;
const TAG_ALT_66: i32 = -28;
const TAG_ALT_59: i32 = -29;
const TAG_ALT_52: i32 = -30;
const TAG_ALT_45: i32 = -31;
const TAG_ALT_38: i32 = -32;
const TAG_ALT_31: i32 = -33;
const TAG_ALT_24: i32 = -34;
const TAG_ALT_17: i32 = -35;
const TAG_ALT_10: i32 = -36;
const TAG_ALT_109: i32 = -37;
const TAG_ALT_102: i32 = -38;
const TAG_ALT_95: i32 = -39;
const TAG_ALT_88: i32 = -40;
const TAG_ALT_81: i32 = -41;
const TAG_ALT_74: i32 = -42;
const TAG_ALT_67: i32 = -43;
const TAG_ALT_60: i32 = -44;
const TAG_ALT_53: i32 = -45;
const TAG_ALT_46: i32 = -46;
const TAG_ALT_39: i32 = -47;
const TAG_ALT_32: i32 = -48;
const TAG_ALT_25: i32 = -49;
const TAG_ALT_18: i32 = -50;
const TAG_ALT_11: i32 = -51;
const TAG_ALT_110: i32 = -52;
const TAG_ALT_103: i32 = -53;
const TAG_ALT_96: i32 = -54;
const TAG_ALT_89: i32 = -55;
const TAG_ALT_82: i32 = -56;
const TAG_ALT_75: i32 = -57;
const TAG_ALT_68: i32 = -58;
const TAG_ALT_61: i32 = -59;
const TAG_ALT_54: i32 = -60;
const TAG_ALT_47: i32 = -61;
const TAG_ALT_40: i32 = -62;
const TAG_ALT_33: i32 = -63;
const TAG_ALT_26: i32 = -64;
const TAG_ALT_19: i32 = -65;
const TAG_ALT_12: i32 = -66;
const TAG_ALT_111: i32 = -67;
const TAG_SULFUR_CUBE_ARCHETYPE_LIGHT: i32 = -68;
const TAG_ALT_104: i32 = -69;
const TAG_ALT_97: i32 = -70;
const TAG_ALT_90: i32 = -71;
const TAG_ALT_83: i32 = -72;
const TAG_ALT_76: i32 = -73;
const TAG_ALT_69: i32 = -74;
const TAG_ALT_62: i32 = -75;
const TAG_ALT_55: i32 = -76;
const TAG_ALT_48: i32 = -77;
const TAG_ALT_41: i32 = -78;
const TAG_ALT_34: i32 = -79;
const TAG_ALT_27: i32 = -80;
const TAG_ALT_20: i32 = -81;
const TAG_ALT_13: i32 = -82;
const TAG_ALT_112: i32 = -83;
const TAG_ALT_105: i32 = -84;
const TAG_ALT_98: i32 = -85;
const TAG_ALT_91: i32 = -86;
const TAG_ALT_84: i32 = -87;
const TAG_ALT_77: i32 = -88;
const TAG_ALT_70: i32 = -89;
const TAG_ALT_63: i32 = -90;
const TAG_ALT_56: i32 = -91;
const TAG_ALT_49: i32 = -92;
const TAG_ALT_42: i32 = -93;
const TAG_ALT_35: i32 = -94;
const TAG_ALT_28: i32 = -95;
const TAG_ALT_21: i32 = -96;
const TAG_ALT_14: i32 = -97;
const TAG_ALT_113: i32 = -98;
const TAG_ALT_106: i32 = -99;
const TAG_ALT_99: i32 = -100;
const TAG_ALT_92: i32 = -101;
const TAG_ALT_85: i32 = -102;
const TAG_ALT_78: i32 = -103;
const TAG_ALT_71: i32 = -104;
const TAG_ALT_64: i32 = -105;
const TAG_ALT_57: i32 = -106;
const TAG_ALT_50: i32 = -107;
const TAG_ALT_43: i32 = -108;
const TAG_ALT_36: i32 = -109;
const TAG_ALT_29: i32 = -110;
const TAG_ALT_22: i32 = -111;
const TAG_ALT_15: i32 = -112;
const TAG_STONE_CRAFTING_MATERIALS: i32 = -113;
const TAG_ALT_2: i32 = -114;
const TAG_ALT_3: i32 = -115;
const TAG_ALT_4: i32 = -116;
const TAG_EGGS: i32 = -117;
const TAG_ALT_5: i32 = -118;
const TAG_ALT_6: i32 = -119;
const TAG_ALT_7: i32 = -120;
const TAG_ALT_8: i32 = -121;
const TAG_MUSHROOMS: i32 = -122;
const TAG_COALS: i32 = -123;
const TAG_CHERRY_LOGS: i32 = -124;
const TAG_ALT_115: i32 = -125;
const TAG_ALT_117: i32 = -126;
const TAG_ALT_116: i32 = -127;
const TAG_ALT_119: i32 = -128;
const TAG_ALT_118: i32 = -129;
const TAG_ALT_0: i32 = -130;
const TAG_COPPER_TOOL_MATERIALS: i32 = -131;
const TAG_METAL_NUGGETS: i32 = -132;
const TAG_CRIMSON_STEMS: i32 = -133;
const TAG_DARK_OAK_LOGS: i32 = -134;
const TAG_DIAMOND_TOOL_MATERIALS: i32 = -135;
const TAG_GOLD_TOOL_MATERIALS: i32 = -136;
const TAG_IRON_TOOL_MATERIALS: i32 = -137;
const TAG_JUNGLE_LOGS: i32 = -138;
const TAG_MANGROVE_LOGS: i32 = -139;
const TAG_OAK_LOGS: i32 = -140;
const TAG_PALE_OAK_LOGS: i32 = -141;
const TAG_POPLAR_LOGS: i32 = -142;
const TAG_ALT_114: i32 = -143;
const TAG_SMELTS_TO_GLASS: i32 = -144;
const TAG_SOUL_FIRE_BASE_BLOCKS: i32 = -145;
const TAG_SPRUCE_LOGS: i32 = -146;
const TAG_WARPED_STEMS: i32 = -147;
// === GEN-TAGS-END ===

// === GEN-TABLE-START === (scripts/gen_recipes.py 生成；勿手改)
pub const RECIPES: &[Recipe] = &[
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 5)], result: (ITEM_ACACIA_BOAT, 1), name: "acacia_boat" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 1)], result: (ITEM_ACACIA_BUTTON, 1), name: "acacia_button" },
    Recipe { needs: &[(ITEM_ACACIA_BOAT, 1), (ITEM_CHEST, 1)], result: (ITEM_ACACIA_CHEST_BOAT, 1), name: "acacia_chest_boat" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6)], result: (ITEM_ACACIA_DOOR, 3), name: "acacia_door" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 4), (STICK, 2)], result: (ITEM_ACACIA_FENCE, 3), name: "acacia_fence" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 2), (STICK, 4)], result: (ITEM_ACACIA_FENCE_GATE, 1), name: "acacia_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_ACACIA_LOG, 6)], result: (ITEM_ACACIA_HANGING_SIGN, 6), name: "acacia_hanging_sign" },
    Recipe { needs: &[(TAG_ACACIA_LOGS, 1)], result: (ITEM_ACACIA_PLANKS, 4), name: "acacia_planks" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 2)], result: (ITEM_ACACIA_PRESSURE_PLATE, 1), name: "acacia_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_ACACIA_LOG, 6)], result: (ITEM_ACACIA_SHELF, 6), name: "acacia_shelf" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6), (STICK, 1)], result: (ITEM_ACACIA_SIGN, 3), name: "acacia_sign" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 3)], result: (ITEM_ACACIA_SLAB, 6), name: "acacia_slab" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6)], result: (ITEM_ACACIA_STAIRS, 4), name: "acacia_stairs" },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6)], result: (ITEM_ACACIA_TRAPDOOR, 2), name: "acacia_trapdoor" },
    Recipe { needs: &[(ITEM_ACACIA_LOG, 4)], result: (ITEM_ACACIA_WOOD, 3), name: "acacia_wood" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (ITEM_REDSTONE_TORCH, 1), (STICK, 2)], result: (ITEM_ACTIVATOR_RAIL, 6), name: "activator_rail" },
    Recipe { needs: &[(ITEM_AMETHYST_SHARD, 4)], result: (ITEM_AMETHYST_BLOCK, 1), name: "amethyst_block" },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_DIORITE, 1)], result: (ITEM_ANDESITE, 2), name: "andesite" },
    Recipe { needs: &[(ITEM_ANDESITE, 3)], result: (ITEM_ANDESITE_SLAB, 6), name: "andesite_slab" },
    Recipe { needs: &[(ITEM_ANDESITE, 6)], result: (ITEM_ANDESITE_STAIRS, 4), name: "andesite_stairs" },
    Recipe { needs: &[(ITEM_ANDESITE, 6)], result: (ITEM_ANDESITE_WALL, 6), name: "andesite_wall" },
    Recipe { needs: &[(ITEM_IRON_BLOCK, 3), (ITEM_IRON_INGOT, 4)], result: (ITEM_ANVIL, 1), name: "anvil" },
    Recipe { needs: &[(ITEM_SMOOTH_STONE_SLAB, 1), (STICK, 6)], result: (ITEM_ARMOR_STAND, 1), name: "armor_stand" },
    Recipe { needs: &[(ITEM_FEATHER, 1), (ITEM_FLINT, 1), (STICK, 1)], result: (ITEM_ARROW, 4), name: "arrow" },
    Recipe { needs: &[(ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1)], result: (ITEM_BAMBOO_BLOCK, 1), name: "bamboo_block" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 1)], result: (ITEM_BAMBOO_BUTTON, 1), name: "bamboo_button" },
    Recipe { needs: &[(ITEM_BAMBOO_RAFT, 1), (ITEM_CHEST, 1)], result: (ITEM_BAMBOO_CHEST_RAFT, 1), name: "bamboo_chest_raft" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 6)], result: (ITEM_BAMBOO_DOOR, 3), name: "bamboo_door" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 4), (STICK, 2)], result: (ITEM_BAMBOO_FENCE, 3), name: "bamboo_fence" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 2), (STICK, 4)], result: (ITEM_BAMBOO_FENCE_GATE, 1), name: "bamboo_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_BAMBOO_BLOCK, 6)], result: (ITEM_BAMBOO_HANGING_SIGN, 6), name: "bamboo_hanging_sign" },
    Recipe { needs: &[(ITEM_BAMBOO_SLAB, 2)], result: (ITEM_BAMBOO_MOSAIC, 1), name: "bamboo_mosaic" },
    Recipe { needs: &[(ITEM_BAMBOO_MOSAIC, 3)], result: (ITEM_BAMBOO_MOSAIC_SLAB, 6), name: "bamboo_mosaic_slab" },
    Recipe { needs: &[(ITEM_BAMBOO_MOSAIC, 6)], result: (ITEM_BAMBOO_MOSAIC_STAIRS, 4), name: "bamboo_mosaic_stairs" },
    Recipe { needs: &[(TAG_BAMBOO_BLOCKS, 1)], result: (ITEM_BAMBOO_PLANKS, 2), name: "bamboo_planks" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 2)], result: (ITEM_BAMBOO_PRESSURE_PLATE, 1), name: "bamboo_pressure_plate" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 5)], result: (ITEM_BAMBOO_RAFT, 1), name: "bamboo_raft" },
    Recipe { needs: &[(ITEM_STRIPPED_BAMBOO_BLOCK, 6)], result: (ITEM_BAMBOO_SHELF, 6), name: "bamboo_shelf" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 6), (STICK, 1)], result: (ITEM_BAMBOO_SIGN, 3), name: "bamboo_sign" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 3)], result: (ITEM_BAMBOO_SLAB, 6), name: "bamboo_slab" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 6)], result: (ITEM_BAMBOO_STAIRS, 4), name: "bamboo_stairs" },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 6)], result: (ITEM_BAMBOO_TRAPDOOR, 2), name: "bamboo_trapdoor" },
    Recipe { needs: &[(TAG_PLANKS, 6), (TAG_WOODEN_SLABS, 2)], result: (ITEM_BARREL, 1), name: "barrel" },
    Recipe { needs: &[(ITEM_GLASS, 5), (ITEM_NETHER_STAR, 1), (ITEM_OBSIDIAN, 3)], result: (ITEM_BEACON, 1), name: "beacon" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 3), (TAG_PLANKS, 6)], result: (ITEM_BEEHIVE, 1), name: "beehive" },
    Recipe { needs: &[(ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BOWL, 1)], result: (ITEM_BEETROOT_SOUP, 1), name: "beetroot_soup" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 5)], result: (ITEM_BIRCH_BOAT, 1), name: "birch_boat" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 1)], result: (ITEM_BIRCH_BUTTON, 1), name: "birch_button" },
    Recipe { needs: &[(ITEM_BIRCH_BOAT, 1), (ITEM_CHEST, 1)], result: (ITEM_BIRCH_CHEST_BOAT, 1), name: "birch_chest_boat" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6)], result: (ITEM_BIRCH_DOOR, 3), name: "birch_door" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 4), (STICK, 2)], result: (ITEM_BIRCH_FENCE, 3), name: "birch_fence" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 2), (STICK, 4)], result: (ITEM_BIRCH_FENCE_GATE, 1), name: "birch_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_BIRCH_LOG, 6)], result: (ITEM_BIRCH_HANGING_SIGN, 6), name: "birch_hanging_sign" },
    Recipe { needs: &[(TAG_BIRCH_LOGS, 1)], result: (ITEM_BIRCH_PLANKS, 4), name: "birch_planks" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 2)], result: (ITEM_BIRCH_PRESSURE_PLATE, 1), name: "birch_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_BIRCH_LOG, 6)], result: (ITEM_BIRCH_SHELF, 6), name: "birch_shelf" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6), (STICK, 1)], result: (ITEM_BIRCH_SIGN, 3), name: "birch_sign" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 3)], result: (ITEM_BIRCH_SLAB, 6), name: "birch_slab" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6)], result: (ITEM_BIRCH_STAIRS, 4), name: "birch_stairs" },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6)], result: (ITEM_BIRCH_TRAPDOOR, 2), name: "birch_trapdoor" },
    Recipe { needs: &[(ITEM_BIRCH_LOG, 4)], result: (ITEM_BIRCH_WOOD, 3), name: "birch_wood" },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 6), (STICK, 1)], result: (ITEM_BLACK_BANNER, 1), name: "black_banner" },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_BLACK_BED, 1), name: "black_bed" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_CANDLE, 1)], result: (ITEM_BLACK_CANDLE, 1), name: "black_candle" },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 2)], result: (ITEM_BLACK_CARPET, 3), name: "black_carpet" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_BLACK_CONCRETE_POWDER, 8), name: "black_concrete_powder" },
    Recipe { needs: &[(ITEM_BLACK_CONCRETE, 3)], result: (ITEM_BLACK_CONCRETE_SLAB, 6), name: "black_concrete_slab" },
    Recipe { needs: &[(ITEM_BLACK_CONCRETE, 6)], result: (ITEM_BLACK_CONCRETE_STAIRS, 4), name: "black_concrete_stairs" },
    Recipe { needs: &[(ITEM_BLACK_WOOL_SLAB, 3)], result: (ITEM_BLACK_CUSHION, 1), name: "black_cushion" },
    Recipe { needs: &[(ITEM_INK_SAC, 1)], result: (ITEM_BLACK_DYE, 1), name: "black_dye" },
    Recipe { needs: &[(ITEM_WITHER_ROSE, 1)], result: (ITEM_BLACK_DYE, 1), name: "black_dye_from_wither_rose" },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 1), (ITEM_GLASS, 2), (ITEM_LEATHER, 3)], result: (ITEM_BLACK_HARNESS, 1), name: "black_harness" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_GLASS, 8)], result: (ITEM_BLACK_STAINED_GLASS, 8), name: "black_stained_glass" },
    Recipe { needs: &[(ITEM_BLACK_STAINED_GLASS, 6)], result: (ITEM_BLACK_STAINED_GLASS_PANE, 16), name: "black_stained_glass_pane" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_GLASS_PANE, 8)], result: (ITEM_BLACK_STAINED_GLASS_PANE, 8), name: "black_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_BLACK_TERRACOTTA, 8), name: "black_terracotta" },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 3)], result: (ITEM_BLACK_WOOL_SLAB, 6), name: "black_wool_slab" },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 6)], result: (ITEM_BLACK_WOOL_STAIRS, 4), name: "black_wool_stairs" },
    Recipe { needs: &[(ITEM_BLACKSTONE, 3)], result: (ITEM_BLACKSTONE_SLAB, 6), name: "blackstone_slab" },
    Recipe { needs: &[(ITEM_BLACKSTONE, 6)], result: (ITEM_BLACKSTONE_STAIRS, 4), name: "blackstone_stairs" },
    Recipe { needs: &[(ITEM_BLACKSTONE, 6)], result: (ITEM_BLACKSTONE_WALL, 6), name: "blackstone_wall" },
    Recipe { needs: &[(ITEM_FURNACE, 1), (ITEM_IRON_INGOT, 5), (ITEM_SMOOTH_STONE, 3)], result: (ITEM_BLAST_FURNACE, 1), name: "blast_furnace" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1)], result: (ITEM_BLAZE_POWDER, 2), name: "blaze_powder" },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 6), (STICK, 1)], result: (ITEM_BLUE_BANNER, 1), name: "blue_banner" },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_BLUE_BED, 1), name: "blue_bed" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_CANDLE, 1)], result: (ITEM_BLUE_CANDLE, 1), name: "blue_candle" },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 2)], result: (ITEM_BLUE_CARPET, 3), name: "blue_carpet" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_BLUE_CONCRETE_POWDER, 8), name: "blue_concrete_powder" },
    Recipe { needs: &[(ITEM_BLUE_CONCRETE, 3)], result: (ITEM_BLUE_CONCRETE_SLAB, 6), name: "blue_concrete_slab" },
    Recipe { needs: &[(ITEM_BLUE_CONCRETE, 6)], result: (ITEM_BLUE_CONCRETE_STAIRS, 4), name: "blue_concrete_stairs" },
    Recipe { needs: &[(ITEM_BLUE_WOOL_SLAB, 3)], result: (ITEM_BLUE_CUSHION, 1), name: "blue_cushion" },
    Recipe { needs: &[(ITEM_LAPIS_LAZULI, 1)], result: (ITEM_BLUE_DYE, 1), name: "blue_dye" },
    Recipe { needs: &[(ITEM_CORNFLOWER, 1)], result: (ITEM_BLUE_DYE, 1), name: "blue_dye_from_cornflower" },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 1), (ITEM_GLASS, 2), (ITEM_LEATHER, 3)], result: (ITEM_BLUE_HARNESS, 1), name: "blue_harness" },
    Recipe { needs: &[(ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1)], result: (ITEM_BLUE_ICE, 1), name: "blue_ice" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_GLASS, 8)], result: (ITEM_BLUE_STAINED_GLASS, 8), name: "blue_stained_glass" },
    Recipe { needs: &[(ITEM_BLUE_STAINED_GLASS, 6)], result: (ITEM_BLUE_STAINED_GLASS_PANE, 16), name: "blue_stained_glass_pane" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_GLASS_PANE, 8)], result: (ITEM_BLUE_STAINED_GLASS_PANE, 8), name: "blue_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_BLUE_TERRACOTTA, 8), name: "blue_terracotta" },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 3)], result: (ITEM_BLUE_WOOL_SLAB, 6), name: "blue_wool_slab" },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 6)], result: (ITEM_BLUE_WOOL_STAIRS, 4), name: "blue_wool_stairs" },
    Recipe { needs: &[(ITEM_BOLT_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_DIAMOND, 7), (TAG_ALT_0, 1)], result: (ITEM_BOLT_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "bolt_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_BONE_MEAL, 9)], result: (ITEM_BONE_BLOCK, 1), name: "bone_block" },
    Recipe { needs: &[(ITEM_BONE, 1)], result: (ITEM_BONE_MEAL, 3), name: "bone_meal" },
    Recipe { needs: &[(ITEM_BONE_BLOCK, 1)], result: (ITEM_BONE_MEAL, 9), name: "bone_meal_from_bone_block" },
    Recipe { needs: &[(ITEM_LEATHER, 1), (ITEM_PAPER, 1), (ITEM_PAPER, 1), (ITEM_PAPER, 1)], result: (ITEM_BOOK, 1), name: "book" },
    Recipe { needs: &[(ITEM_BOOK, 3), (TAG_PLANKS, 6)], result: (ITEM_BOOKSHELF, 1), name: "bookshelf" },
    Recipe { needs: &[(ITEM_PAPER, 1), (ITEM_VINE, 1)], result: (ITEM_BORDURE_INDENTED_BANNER_PATTERN, 1), name: "bordure_indented_banner_pattern" },
    Recipe { needs: &[(STICK, 3), (ITEM_STRING, 3)], result: (ITEM_BOW, 1), name: "bow" },
    Recipe { needs: &[(TAG_PLANKS, 3)], result: (ITEM_BOWL, 4), name: "bowl" },
    Recipe { needs: &[(WHEAT, 3)], result: (BREAD, 1), name: "bread" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (TAG_STONE_CRAFTING_MATERIALS, 3)], result: (ITEM_BREWING_STAND, 1), name: "brewing_stand" },
    Recipe { needs: &[(ITEM_BRICKS, 3)], result: (ITEM_BRICK_SLAB, 6), name: "brick_slab" },
    Recipe { needs: &[(ITEM_BRICKS, 6)], result: (ITEM_BRICK_STAIRS, 4), name: "brick_stairs" },
    Recipe { needs: &[(ITEM_BRICKS, 6)], result: (ITEM_BRICK_WALL, 6), name: "brick_wall" },
    Recipe { needs: &[(ITEM_BRICK, 4)], result: (ITEM_BRICKS, 1), name: "bricks" },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 6), (STICK, 1)], result: (ITEM_BROWN_BANNER, 1), name: "brown_banner" },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_BROWN_BED, 1), name: "brown_bed" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_CANDLE, 1)], result: (ITEM_BROWN_CANDLE, 1), name: "brown_candle" },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 2)], result: (ITEM_BROWN_CARPET, 3), name: "brown_carpet" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_BROWN_CONCRETE_POWDER, 8), name: "brown_concrete_powder" },
    Recipe { needs: &[(ITEM_BROWN_CONCRETE, 3)], result: (ITEM_BROWN_CONCRETE_SLAB, 6), name: "brown_concrete_slab" },
    Recipe { needs: &[(ITEM_BROWN_CONCRETE, 6)], result: (ITEM_BROWN_CONCRETE_STAIRS, 4), name: "brown_concrete_stairs" },
    Recipe { needs: &[(ITEM_BROWN_WOOL_SLAB, 3)], result: (ITEM_BROWN_CUSHION, 1), name: "brown_cushion" },
    Recipe { needs: &[(ITEM_COCOA_BEANS, 1)], result: (ITEM_BROWN_DYE, 1), name: "brown_dye" },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 1), (ITEM_GLASS, 2), (ITEM_LEATHER, 3)], result: (ITEM_BROWN_HARNESS, 1), name: "brown_harness" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_GLASS, 8)], result: (ITEM_BROWN_STAINED_GLASS, 8), name: "brown_stained_glass" },
    Recipe { needs: &[(ITEM_BROWN_STAINED_GLASS, 6)], result: (ITEM_BROWN_STAINED_GLASS_PANE, 16), name: "brown_stained_glass_pane" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_GLASS_PANE, 8)], result: (ITEM_BROWN_STAINED_GLASS_PANE, 8), name: "brown_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_BROWN_TERRACOTTA, 8), name: "brown_terracotta" },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 3)], result: (ITEM_BROWN_WOOL_SLAB, 6), name: "brown_wool_slab" },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 6)], result: (ITEM_BROWN_WOOL_STAIRS, 4), name: "brown_wool_stairs" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 1), (ITEM_FEATHER, 1), (STICK, 1)], result: (ITEM_BRUSH, 1), name: "brush" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 3)], result: (ITEM_BUCKET, 1), name: "bucket" },
    Recipe { needs: &[(ITEM_LEATHER, 1), (ITEM_STRING, 1)], result: (ITEM_BUNDLE, 1), name: "bundle" },
    Recipe { needs: &[(ITEM_MILK_BUCKET, 3), (ITEM_SUGAR, 2), (WHEAT, 3), (TAG_EGGS, 1)], result: (ITEM_CAKE, 1), name: "cake" },
    Recipe { needs: &[(ITEM_AMETHYST_SHARD, 3), (ITEM_SCULK_SENSOR, 1)], result: (ITEM_CALIBRATED_SCULK_SENSOR, 1), name: "calibrated_sculk_sensor" },
    Recipe { needs: &[(STICK, 3), (TAG_LOGS, 3), (TAG_COALS, 1)], result: (ITEM_CAMPFIRE, 1), name: "campfire" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_STRING, 1)], result: (ITEM_CANDLE, 1), name: "candle" },
    Recipe { needs: &[(ITEM_CARROT, 1), (ITEM_FISHING_ROD, 1)], result: (ITEM_CARROT_ON_A_STICK, 1), name: "carrot_on_a_stick" },
    Recipe { needs: &[(ITEM_PAPER, 2), (TAG_PLANKS, 4)], result: (ITEM_CARTOGRAPHY_TABLE, 1), name: "cartography_table" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 7)], result: (ITEM_CAULDRON, 1), name: "cauldron" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 5)], result: (ITEM_CHERRY_BOAT, 1), name: "cherry_boat" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 1)], result: (ITEM_CHERRY_BUTTON, 1), name: "cherry_button" },
    Recipe { needs: &[(ITEM_CHERRY_BOAT, 1), (ITEM_CHEST, 1)], result: (ITEM_CHERRY_CHEST_BOAT, 1), name: "cherry_chest_boat" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6)], result: (ITEM_CHERRY_DOOR, 3), name: "cherry_door" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 4), (STICK, 2)], result: (ITEM_CHERRY_FENCE, 3), name: "cherry_fence" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 2), (STICK, 4)], result: (ITEM_CHERRY_FENCE_GATE, 1), name: "cherry_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_CHERRY_LOG, 6)], result: (ITEM_CHERRY_HANGING_SIGN, 6), name: "cherry_hanging_sign" },
    Recipe { needs: &[(TAG_CHERRY_LOGS, 1)], result: (ITEM_CHERRY_PLANKS, 4), name: "cherry_planks" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 2)], result: (ITEM_CHERRY_PRESSURE_PLATE, 1), name: "cherry_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_CHERRY_LOG, 6)], result: (ITEM_CHERRY_SHELF, 6), name: "cherry_shelf" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6), (STICK, 1)], result: (ITEM_CHERRY_SIGN, 3), name: "cherry_sign" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 3)], result: (ITEM_CHERRY_SLAB, 6), name: "cherry_slab" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6)], result: (ITEM_CHERRY_STAIRS, 4), name: "cherry_stairs" },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6)], result: (ITEM_CHERRY_TRAPDOOR, 2), name: "cherry_trapdoor" },
    Recipe { needs: &[(ITEM_CHERRY_LOG, 4)], result: (ITEM_CHERRY_WOOD, 3), name: "cherry_wood" },
    Recipe { needs: &[(TAG_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "chest" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_MINECART, 1)], result: (ITEM_CHEST_MINECART, 1), name: "chest_minecart" },
    Recipe { needs: &[(TAG_PLANKS, 6), (TAG_WOODEN_SLABS, 3)], result: (ITEM_CHISELED_BOOKSHELF, 1), name: "chiseled_bookshelf" },
    Recipe { needs: &[(ITEM_CINNABAR_SLAB, 2)], result: (ITEM_CHISELED_CINNABAR, 1), name: "chiseled_cinnabar" },
    Recipe { needs: &[(ITEM_CUT_COPPER_SLAB, 2)], result: (ITEM_CHISELED_COPPER, 1), name: "chiseled_copper" },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE_SLAB, 2)], result: (ITEM_CHISELED_DEEPSLATE, 1), name: "chiseled_deepslate" },
    Recipe { needs: &[(ITEM_NETHER_BRICK_SLAB, 2)], result: (ITEM_CHISELED_NETHER_BRICKS, 1), name: "chiseled_nether_bricks" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE_SLAB, 2)], result: (ITEM_CHISELED_POLISHED_BLACKSTONE, 1), name: "chiseled_polished_blackstone" },
    Recipe { needs: &[(ITEM_QUARTZ_SLAB, 2)], result: (ITEM_CHISELED_QUARTZ_BLOCK, 1), name: "chiseled_quartz_block" },
    Recipe { needs: &[(ITEM_RED_SANDSTONE_SLAB, 2)], result: (ITEM_CHISELED_RED_SANDSTONE, 1), name: "chiseled_red_sandstone" },
    Recipe { needs: &[(ITEM_RESIN_BRICK_SLAB, 2)], result: (ITEM_CHISELED_RESIN_BRICKS, 1), name: "chiseled_resin_bricks" },
    Recipe { needs: &[(ITEM_SANDSTONE_SLAB, 2)], result: (ITEM_CHISELED_SANDSTONE, 1), name: "chiseled_sandstone" },
    Recipe { needs: &[(ITEM_STONE_BRICK_SLAB, 2)], result: (ITEM_CHISELED_STONE_BRICKS, 1), name: "chiseled_stone_bricks" },
    Recipe { needs: &[(ITEM_SULFUR_SLAB, 2)], result: (ITEM_CHISELED_SULFUR, 1), name: "chiseled_sulfur" },
    Recipe { needs: &[(ITEM_TUFF_SLAB, 2)], result: (ITEM_CHISELED_TUFF, 1), name: "chiseled_tuff" },
    Recipe { needs: &[(ITEM_TUFF_BRICK_SLAB, 2)], result: (ITEM_CHISELED_TUFF_BRICKS, 1), name: "chiseled_tuff_bricks" },
    Recipe { needs: &[(ITEM_CINNABAR_BRICKS, 3)], result: (ITEM_CINNABAR_BRICK_SLAB, 6), name: "cinnabar_brick_slab" },
    Recipe { needs: &[(ITEM_CINNABAR_BRICKS, 6)], result: (ITEM_CINNABAR_BRICK_STAIRS, 4), name: "cinnabar_brick_stairs" },
    Recipe { needs: &[(ITEM_CINNABAR_BRICKS, 6)], result: (ITEM_CINNABAR_BRICK_WALL, 6), name: "cinnabar_brick_wall" },
    Recipe { needs: &[(ITEM_POLISHED_CINNABAR, 4)], result: (ITEM_CINNABAR_BRICKS, 4), name: "cinnabar_bricks" },
    Recipe { needs: &[(ITEM_CINNABAR, 3)], result: (ITEM_CINNABAR_SLAB, 6), name: "cinnabar_slab" },
    Recipe { needs: &[(ITEM_CINNABAR, 6)], result: (ITEM_CINNABAR_STAIRS, 4), name: "cinnabar_stairs" },
    Recipe { needs: &[(ITEM_CINNABAR, 6)], result: (ITEM_CINNABAR_WALL, 6), name: "cinnabar_wall" },
    Recipe { needs: &[(ITEM_CLAY_BALL, 4)], result: (ITEM_CLAY, 1), name: "clay" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 4), (ITEM_REDSTONE, 1)], result: (ITEM_CLOCK, 1), name: "clock" },
    Recipe { needs: &[(ITEM_COAL_BLOCK, 1)], result: (COAL, 9), name: "coal" },
    Recipe { needs: &[(COAL, 9)], result: (ITEM_COAL_BLOCK, 1), name: "coal_block" },
    Recipe { needs: &[(ITEM_DIRT, 2), (ITEM_GRAVEL, 2)], result: (ITEM_COARSE_DIRT, 4), name: "coarse_dirt" },
    Recipe { needs: &[(ITEM_COAST_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (COBBLESTONE, 1), (ITEM_DIAMOND, 7)], result: (ITEM_COAST_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "coast_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 3)], result: (ITEM_COBBLED_DEEPSLATE_SLAB, 6), name: "cobbled_deepslate_slab" },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 6)], result: (ITEM_COBBLED_DEEPSLATE_STAIRS, 4), name: "cobbled_deepslate_stairs" },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 6)], result: (ITEM_COBBLED_DEEPSLATE_WALL, 6), name: "cobbled_deepslate_wall" },
    Recipe { needs: &[(COBBLESTONE, 3)], result: (ITEM_COBBLESTONE_SLAB, 6), name: "cobblestone_slab" },
    Recipe { needs: &[(COBBLESTONE, 6)], result: (ITEM_COBBLESTONE_STAIRS, 4), name: "cobblestone_stairs" },
    Recipe { needs: &[(COBBLESTONE, 6)], result: (ITEM_COBBLESTONE_WALL, 6), name: "cobblestone_wall" },
    Recipe { needs: &[(ITEM_QUARTZ, 1), (ITEM_REDSTONE_TORCH, 3), (ITEM_STONE, 3)], result: (ITEM_COMPARATOR, 1), name: "comparator" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 4), (ITEM_REDSTONE, 1)], result: (ITEM_COMPASS, 1), name: "compass" },
    Recipe { needs: &[(TAG_WOODEN_SLABS, 7)], result: (ITEM_COMPOSTER, 1), name: "composter" },
    Recipe { needs: &[(ITEM_HEART_OF_THE_SEA, 1), (ITEM_NAUTILUS_SHELL, 8)], result: (ITEM_CONDUIT, 1), name: "conduit" },
    Recipe { needs: &[(ITEM_COCOA_BEANS, 1), (WHEAT, 2)], result: (ITEM_COOKIE, 8), name: "cookie" },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 3)], result: (ITEM_COPPER_AXE, 1), name: "copper_axe" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 6)], result: (ITEM_COPPER_BARS, 16), name: "copper_bars" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 9)], result: (ITEM_COPPER_BLOCK, 1), name: "copper_block" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 4)], result: (ITEM_COPPER_BOOTS, 1), name: "copper_boots" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_COPPER_BLOCK, 3), (ITEM_REDSTONE, 1)], result: (ITEM_COPPER_BULB, 4), name: "copper_bulb" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 1), (ITEM_COPPER_NUGGET, 2)], result: (ITEM_COPPER_CHAIN, 1), name: "copper_chain" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_COPPER_INGOT, 8)], result: (ITEM_COPPER_CHEST, 1), name: "copper_chest" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 8)], result: (ITEM_COPPER_CHESTPLATE, 1), name: "copper_chestplate" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 6)], result: (ITEM_COPPER_DOOR, 3), name: "copper_door" },
    Recipe { needs: &[(ITEM_COPPER_BLOCK, 4)], result: (ITEM_COPPER_GRATE, 4), name: "copper_grate" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 5)], result: (ITEM_COPPER_HELMET, 1), name: "copper_helmet" },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 2)], result: (ITEM_COPPER_HOE, 1), name: "copper_hoe" },
    Recipe { needs: &[(ITEM_COPPER_BLOCK, 1)], result: (ITEM_COPPER_INGOT, 9), name: "copper_ingot" },
    Recipe { needs: &[(ITEM_COPPER_NUGGET, 9)], result: (ITEM_COPPER_INGOT, 1), name: "copper_ingot_from_nuggets" },
    Recipe { needs: &[(ITEM_WAXED_COPPER_BLOCK, 1)], result: (ITEM_COPPER_INGOT, 9), name: "copper_ingot_from_waxed_copper_block" },
    Recipe { needs: &[(ITEM_COPPER_NUGGET, 8), (ITEM_COPPER_TORCH, 1)], result: (ITEM_COPPER_LANTERN, 1), name: "copper_lantern" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 7)], result: (ITEM_COPPER_LEGGINGS, 1), name: "copper_leggings" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 1)], result: (ITEM_COPPER_NUGGET, 9), name: "copper_nugget" },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 3)], result: (ITEM_COPPER_PICKAXE, 1), name: "copper_pickaxe" },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 1)], result: (ITEM_COPPER_SHOVEL, 1), name: "copper_shovel" },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 1)], result: (ITEM_COPPER_SPEAR, 1), name: "copper_spear" },
    Recipe { needs: &[(STICK, 1), (TAG_COPPER_TOOL_MATERIALS, 2)], result: (ITEM_COPPER_SWORD, 1), name: "copper_sword" },
    Recipe { needs: &[(ITEM_COPPER_NUGGET, 1), (STICK, 1), (TAG_COALS, 1)], result: (ITEM_COPPER_TORCH, 4), name: "copper_torch" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 4)], result: (ITEM_COPPER_TRAPDOOR, 1), name: "copper_trapdoor" },
    Recipe { needs: &[(CRAFTING_TABLE, 1), (ITEM_DROPPER, 1), (ITEM_IRON_INGOT, 5), (ITEM_REDSTONE, 2)], result: (ITEM_CRAFTER, 1), name: "crafter" },
    Recipe { needs: &[(TAG_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "crafting_table" },
    Recipe { needs: &[(ITEM_PALE_OAK_LOG, 2), (ITEM_RESIN_BLOCK, 1)], result: (ITEM_CREAKING_HEART, 1), name: "creaking_heart" },
    Recipe { needs: &[(ITEM_CREEPER_HEAD, 1), (ITEM_PAPER, 1)], result: (ITEM_CREEPER_BANNER_PATTERN, 1), name: "creeper_banner_pattern" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 1)], result: (ITEM_CRIMSON_BUTTON, 1), name: "crimson_button" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6)], result: (ITEM_CRIMSON_DOOR, 3), name: "crimson_door" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 4), (STICK, 2)], result: (ITEM_CRIMSON_FENCE, 3), name: "crimson_fence" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 2), (STICK, 4)], result: (ITEM_CRIMSON_FENCE_GATE, 1), name: "crimson_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_CRIMSON_STEM, 6)], result: (ITEM_CRIMSON_HANGING_SIGN, 6), name: "crimson_hanging_sign" },
    Recipe { needs: &[(ITEM_CRIMSON_STEM, 4)], result: (ITEM_CRIMSON_HYPHAE, 3), name: "crimson_hyphae" },
    Recipe { needs: &[(TAG_CRIMSON_STEMS, 1)], result: (ITEM_CRIMSON_PLANKS, 4), name: "crimson_planks" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 2)], result: (ITEM_CRIMSON_PRESSURE_PLATE, 1), name: "crimson_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_CRIMSON_STEM, 6)], result: (ITEM_CRIMSON_SHELF, 6), name: "crimson_shelf" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6), (STICK, 1)], result: (ITEM_CRIMSON_SIGN, 3), name: "crimson_sign" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 3)], result: (ITEM_CRIMSON_SLAB, 6), name: "crimson_slab" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6)], result: (ITEM_CRIMSON_STAIRS, 4), name: "crimson_stairs" },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6)], result: (ITEM_CRIMSON_TRAPDOOR, 2), name: "crimson_trapdoor" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (STICK, 3), (ITEM_STRING, 2), (ITEM_TRIPWIRE_HOOK, 1)], result: (ITEM_CROSSBOW, 1), name: "crossbow" },
    Recipe { needs: &[(ITEM_COPPER_BLOCK, 4)], result: (ITEM_CUT_COPPER, 4), name: "cut_copper" },
    Recipe { needs: &[(ITEM_CUT_COPPER, 3)], result: (ITEM_CUT_COPPER_SLAB, 6), name: "cut_copper_slab" },
    Recipe { needs: &[(ITEM_CUT_COPPER, 6)], result: (ITEM_CUT_COPPER_STAIRS, 4), name: "cut_copper_stairs" },
    Recipe { needs: &[(ITEM_RED_SANDSTONE, 4)], result: (ITEM_CUT_RED_SANDSTONE, 4), name: "cut_red_sandstone" },
    Recipe { needs: &[(ITEM_CUT_RED_SANDSTONE, 3)], result: (ITEM_CUT_RED_SANDSTONE_SLAB, 6), name: "cut_red_sandstone_slab" },
    Recipe { needs: &[(ITEM_SANDSTONE, 4)], result: (ITEM_CUT_SANDSTONE, 4), name: "cut_sandstone" },
    Recipe { needs: &[(ITEM_CUT_SANDSTONE, 3)], result: (ITEM_CUT_SANDSTONE_SLAB, 6), name: "cut_sandstone_slab" },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 6), (STICK, 1)], result: (ITEM_CYAN_BANNER, 1), name: "cyan_banner" },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_CYAN_BED, 1), name: "cyan_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_CYAN_DYE, 1)], result: (ITEM_CYAN_CANDLE, 1), name: "cyan_candle" },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 2)], result: (ITEM_CYAN_CARPET, 3), name: "cyan_carpet" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_CYAN_CONCRETE_POWDER, 8), name: "cyan_concrete_powder" },
    Recipe { needs: &[(ITEM_CYAN_CONCRETE, 3)], result: (ITEM_CYAN_CONCRETE_SLAB, 6), name: "cyan_concrete_slab" },
    Recipe { needs: &[(ITEM_CYAN_CONCRETE, 6)], result: (ITEM_CYAN_CONCRETE_STAIRS, 4), name: "cyan_concrete_stairs" },
    Recipe { needs: &[(ITEM_CYAN_WOOL_SLAB, 3)], result: (ITEM_CYAN_CUSHION, 1), name: "cyan_cushion" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_GREEN_DYE, 1)], result: (ITEM_CYAN_DYE, 2), name: "cyan_dye" },
    Recipe { needs: &[(ITEM_PITCHER_PLANT, 1)], result: (ITEM_CYAN_DYE, 2), name: "cyan_dye_from_pitcher_plant" },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 1), (ITEM_GLASS, 2), (ITEM_LEATHER, 3)], result: (ITEM_CYAN_HARNESS, 1), name: "cyan_harness" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_GLASS, 8)], result: (ITEM_CYAN_STAINED_GLASS, 8), name: "cyan_stained_glass" },
    Recipe { needs: &[(ITEM_CYAN_STAINED_GLASS, 6)], result: (ITEM_CYAN_STAINED_GLASS_PANE, 16), name: "cyan_stained_glass_pane" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_GLASS_PANE, 8)], result: (ITEM_CYAN_STAINED_GLASS_PANE, 8), name: "cyan_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_CYAN_TERRACOTTA, 8), name: "cyan_terracotta" },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 3)], result: (ITEM_CYAN_WOOL_SLAB, 6), name: "cyan_wool_slab" },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 6)], result: (ITEM_CYAN_WOOL_STAIRS, 4), name: "cyan_wool_stairs" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 5)], result: (ITEM_DARK_OAK_BOAT, 1), name: "dark_oak_boat" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 1)], result: (ITEM_DARK_OAK_BUTTON, 1), name: "dark_oak_button" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_DARK_OAK_BOAT, 1)], result: (ITEM_DARK_OAK_CHEST_BOAT, 1), name: "dark_oak_chest_boat" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6)], result: (ITEM_DARK_OAK_DOOR, 3), name: "dark_oak_door" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_DARK_OAK_FENCE, 3), name: "dark_oak_fence" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 2), (STICK, 4)], result: (ITEM_DARK_OAK_FENCE_GATE, 1), name: "dark_oak_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_DARK_OAK_LOG, 6)], result: (ITEM_DARK_OAK_HANGING_SIGN, 6), name: "dark_oak_hanging_sign" },
    Recipe { needs: &[(TAG_DARK_OAK_LOGS, 1)], result: (ITEM_DARK_OAK_PLANKS, 4), name: "dark_oak_planks" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 2)], result: (ITEM_DARK_OAK_PRESSURE_PLATE, 1), name: "dark_oak_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_DARK_OAK_LOG, 6)], result: (ITEM_DARK_OAK_SHELF, 6), name: "dark_oak_shelf" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6), (STICK, 1)], result: (ITEM_DARK_OAK_SIGN, 3), name: "dark_oak_sign" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 3)], result: (ITEM_DARK_OAK_SLAB, 6), name: "dark_oak_slab" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6)], result: (ITEM_DARK_OAK_STAIRS, 4), name: "dark_oak_stairs" },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6)], result: (ITEM_DARK_OAK_TRAPDOOR, 2), name: "dark_oak_trapdoor" },
    Recipe { needs: &[(ITEM_DARK_OAK_LOG, 4)], result: (ITEM_DARK_OAK_WOOD, 3), name: "dark_oak_wood" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_PRISMARINE_SHARD, 8)], result: (ITEM_DARK_PRISMARINE, 1), name: "dark_prismarine" },
    Recipe { needs: &[(ITEM_DARK_PRISMARINE, 3)], result: (ITEM_DARK_PRISMARINE_SLAB, 6), name: "dark_prismarine_slab" },
    Recipe { needs: &[(ITEM_DARK_PRISMARINE, 6)], result: (ITEM_DARK_PRISMARINE_STAIRS, 4), name: "dark_prismarine_stairs" },
    Recipe { needs: &[(ITEM_GLASS, 3), (ITEM_QUARTZ, 3), (TAG_WOODEN_SLABS, 3)], result: (ITEM_DAYLIGHT_DETECTOR, 1), name: "daylight_detector" },
    Recipe { needs: &[(ITEM_BRICK, 4)], result: (ITEM_DECORATED_POT, 1), name: "decorated_pot_simple" },
    Recipe { needs: &[(ITEM_DEEPSLATE_BRICKS, 3)], result: (ITEM_DEEPSLATE_BRICK_SLAB, 6), name: "deepslate_brick_slab" },
    Recipe { needs: &[(ITEM_DEEPSLATE_BRICKS, 6)], result: (ITEM_DEEPSLATE_BRICK_STAIRS, 4), name: "deepslate_brick_stairs" },
    Recipe { needs: &[(ITEM_DEEPSLATE_BRICKS, 6)], result: (ITEM_DEEPSLATE_BRICK_WALL, 6), name: "deepslate_brick_wall" },
    Recipe { needs: &[(ITEM_POLISHED_DEEPSLATE, 4)], result: (ITEM_DEEPSLATE_BRICKS, 4), name: "deepslate_bricks" },
    Recipe { needs: &[(ITEM_DEEPSLATE_TILES, 3)], result: (ITEM_DEEPSLATE_TILE_SLAB, 6), name: "deepslate_tile_slab" },
    Recipe { needs: &[(ITEM_DEEPSLATE_TILES, 6)], result: (ITEM_DEEPSLATE_TILE_STAIRS, 4), name: "deepslate_tile_stairs" },
    Recipe { needs: &[(ITEM_DEEPSLATE_TILES, 6)], result: (ITEM_DEEPSLATE_TILE_WALL, 6), name: "deepslate_tile_wall" },
    Recipe { needs: &[(ITEM_DEEPSLATE_BRICKS, 4)], result: (ITEM_DEEPSLATE_TILES, 4), name: "deepslate_tiles" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (ITEM_REDSTONE, 1), (ITEM_STONE_PRESSURE_PLATE, 1)], result: (ITEM_DETECTOR_RAIL, 6), name: "detector_rail" },
    Recipe { needs: &[(ITEM_DIAMOND_BLOCK, 1)], result: (ITEM_DIAMOND, 9), name: "diamond" },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 3)], result: (ITEM_DIAMOND_AXE, 1), name: "diamond_axe" },
    Recipe { needs: &[(ITEM_DIAMOND, 9)], result: (ITEM_DIAMOND_BLOCK, 1), name: "diamond_block" },
    Recipe { needs: &[(ITEM_DIAMOND, 4)], result: (ITEM_DIAMOND_BOOTS, 1), name: "diamond_boots" },
    Recipe { needs: &[(ITEM_DIAMOND, 8)], result: (ITEM_DIAMOND_CHESTPLATE, 1), name: "diamond_chestplate" },
    Recipe { needs: &[(ITEM_DIAMOND, 5)], result: (ITEM_DIAMOND_HELMET, 1), name: "diamond_helmet" },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 2)], result: (ITEM_DIAMOND_HOE, 1), name: "diamond_hoe" },
    Recipe { needs: &[(ITEM_DIAMOND, 7)], result: (ITEM_DIAMOND_LEGGINGS, 1), name: "diamond_leggings" },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 3)], result: (ITEM_DIAMOND_PICKAXE, 1), name: "diamond_pickaxe" },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 1)], result: (ITEM_DIAMOND_SHOVEL, 1), name: "diamond_shovel" },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 1)], result: (ITEM_DIAMOND_SPEAR, 1), name: "diamond_spear" },
    Recipe { needs: &[(STICK, 1), (TAG_DIAMOND_TOOL_MATERIALS, 2)], result: (ITEM_DIAMOND_SWORD, 1), name: "diamond_sword" },
    Recipe { needs: &[(COBBLESTONE, 2), (ITEM_QUARTZ, 2)], result: (ITEM_DIORITE, 2), name: "diorite" },
    Recipe { needs: &[(ITEM_DIORITE, 3)], result: (ITEM_DIORITE_SLAB, 6), name: "diorite_slab" },
    Recipe { needs: &[(ITEM_DIORITE, 6)], result: (ITEM_DIORITE_STAIRS, 4), name: "diorite_stairs" },
    Recipe { needs: &[(ITEM_DIORITE, 6)], result: (ITEM_DIORITE_WALL, 6), name: "diorite_wall" },
    Recipe { needs: &[(ITEM_BOW, 1), (COBBLESTONE, 7), (ITEM_REDSTONE, 1)], result: (ITEM_DISPENSER, 1), name: "dispenser" },
    Recipe { needs: &[(ITEM_GHAST_TEAR, 8), (ITEM_SOUL_SAND, 1)], result: (ITEM_DRIED_GHAST, 1), name: "dried_ghast" },
    Recipe { needs: &[(ITEM_DRIED_KELP_BLOCK, 1)], result: (ITEM_DRIED_KELP, 9), name: "dried_kelp" },
    Recipe { needs: &[(ITEM_DRIED_KELP, 9)], result: (ITEM_DRIED_KELP_BLOCK, 1), name: "dried_kelp_block" },
    Recipe { needs: &[(ITEM_POINTED_DRIPSTONE, 4)], result: (ITEM_DRIPSTONE_BLOCK, 1), name: "dripstone_block" },
    Recipe { needs: &[(COBBLESTONE, 7), (ITEM_REDSTONE, 1)], result: (ITEM_DROPPER, 1), name: "dropper" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_DUNE_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_SANDSTONE, 1)], result: (ITEM_DUNE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "dune_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_2, 1)], result: (ITEM_BLACK_BED, 1), name: "dye_black_bed" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_3, 1)], result: (ITEM_BLACK_CARPET, 1), name: "dye_black_carpet" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_4, 1)], result: (ITEM_BLACK_CUSHION, 1), name: "dye_black_cushion" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_5, 1)], result: (ITEM_BLACK_HARNESS, 1), name: "dye_black_harness" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_6, 1)], result: (ITEM_BLACK_WOOL, 1), name: "dye_black_wool" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_7, 1)], result: (ITEM_BLACK_WOOL_SLAB, 1), name: "dye_black_wool_slab" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_8, 1)], result: (ITEM_BLACK_WOOL_STAIRS, 1), name: "dye_black_wool_stairs" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_9, 1)], result: (ITEM_BLUE_BED, 1), name: "dye_blue_bed" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_10, 1)], result: (ITEM_BLUE_CARPET, 1), name: "dye_blue_carpet" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_11, 1)], result: (ITEM_BLUE_CUSHION, 1), name: "dye_blue_cushion" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_12, 1)], result: (ITEM_BLUE_HARNESS, 1), name: "dye_blue_harness" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_13, 1)], result: (ITEM_BLUE_WOOL, 1), name: "dye_blue_wool" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_14, 1)], result: (ITEM_BLUE_WOOL_SLAB, 1), name: "dye_blue_wool_slab" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_15, 1)], result: (ITEM_BLUE_WOOL_STAIRS, 1), name: "dye_blue_wool_stairs" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_16, 1)], result: (ITEM_BROWN_BED, 1), name: "dye_brown_bed" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_17, 1)], result: (ITEM_BROWN_CARPET, 1), name: "dye_brown_carpet" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_18, 1)], result: (ITEM_BROWN_CUSHION, 1), name: "dye_brown_cushion" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_19, 1)], result: (ITEM_BROWN_HARNESS, 1), name: "dye_brown_harness" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_20, 1)], result: (ITEM_BROWN_WOOL, 1), name: "dye_brown_wool" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_21, 1)], result: (ITEM_BROWN_WOOL_SLAB, 1), name: "dye_brown_wool_slab" },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_22, 1)], result: (ITEM_BROWN_WOOL_STAIRS, 1), name: "dye_brown_wool_stairs" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_23, 1)], result: (ITEM_CYAN_BED, 1), name: "dye_cyan_bed" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_24, 1)], result: (ITEM_CYAN_CARPET, 1), name: "dye_cyan_carpet" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_25, 1)], result: (ITEM_CYAN_CUSHION, 1), name: "dye_cyan_cushion" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_26, 1)], result: (ITEM_CYAN_HARNESS, 1), name: "dye_cyan_harness" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_27, 1)], result: (ITEM_CYAN_WOOL, 1), name: "dye_cyan_wool" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_28, 1)], result: (ITEM_CYAN_WOOL_SLAB, 1), name: "dye_cyan_wool_slab" },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_29, 1)], result: (ITEM_CYAN_WOOL_STAIRS, 1), name: "dye_cyan_wool_stairs" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_30, 1)], result: (ITEM_GRAY_BED, 1), name: "dye_gray_bed" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_31, 1)], result: (ITEM_GRAY_CARPET, 1), name: "dye_gray_carpet" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_32, 1)], result: (ITEM_GRAY_CUSHION, 1), name: "dye_gray_cushion" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_33, 1)], result: (ITEM_GRAY_HARNESS, 1), name: "dye_gray_harness" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_34, 1)], result: (ITEM_GRAY_WOOL, 1), name: "dye_gray_wool" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_35, 1)], result: (ITEM_GRAY_WOOL_SLAB, 1), name: "dye_gray_wool_slab" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_36, 1)], result: (ITEM_GRAY_WOOL_STAIRS, 1), name: "dye_gray_wool_stairs" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_37, 1)], result: (ITEM_GREEN_BED, 1), name: "dye_green_bed" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_38, 1)], result: (ITEM_GREEN_CARPET, 1), name: "dye_green_carpet" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_39, 1)], result: (ITEM_GREEN_CUSHION, 1), name: "dye_green_cushion" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_40, 1)], result: (ITEM_GREEN_HARNESS, 1), name: "dye_green_harness" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_41, 1)], result: (ITEM_GREEN_WOOL, 1), name: "dye_green_wool" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_42, 1)], result: (ITEM_GREEN_WOOL_SLAB, 1), name: "dye_green_wool_slab" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_43, 1)], result: (ITEM_GREEN_WOOL_STAIRS, 1), name: "dye_green_wool_stairs" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_44, 1)], result: (ITEM_LIGHT_BLUE_BED, 1), name: "dye_light_blue_bed" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_45, 1)], result: (ITEM_LIGHT_BLUE_CARPET, 1), name: "dye_light_blue_carpet" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_46, 1)], result: (ITEM_LIGHT_BLUE_CUSHION, 1), name: "dye_light_blue_cushion" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_47, 1)], result: (ITEM_LIGHT_BLUE_HARNESS, 1), name: "dye_light_blue_harness" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_48, 1)], result: (ITEM_LIGHT_BLUE_WOOL, 1), name: "dye_light_blue_wool" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_49, 1)], result: (ITEM_LIGHT_BLUE_WOOL_SLAB, 1), name: "dye_light_blue_wool_slab" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_50, 1)], result: (ITEM_LIGHT_BLUE_WOOL_STAIRS, 1), name: "dye_light_blue_wool_stairs" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_51, 1)], result: (ITEM_LIGHT_GRAY_BED, 1), name: "dye_light_gray_bed" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_52, 1)], result: (ITEM_LIGHT_GRAY_CARPET, 1), name: "dye_light_gray_carpet" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_53, 1)], result: (ITEM_LIGHT_GRAY_CUSHION, 1), name: "dye_light_gray_cushion" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_54, 1)], result: (ITEM_LIGHT_GRAY_HARNESS, 1), name: "dye_light_gray_harness" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_55, 1)], result: (ITEM_LIGHT_GRAY_WOOL, 1), name: "dye_light_gray_wool" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_56, 1)], result: (ITEM_LIGHT_GRAY_WOOL_SLAB, 1), name: "dye_light_gray_wool_slab" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_57, 1)], result: (ITEM_LIGHT_GRAY_WOOL_STAIRS, 1), name: "dye_light_gray_wool_stairs" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_58, 1)], result: (ITEM_LIME_BED, 1), name: "dye_lime_bed" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_59, 1)], result: (ITEM_LIME_CARPET, 1), name: "dye_lime_carpet" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_60, 1)], result: (ITEM_LIME_CUSHION, 1), name: "dye_lime_cushion" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_61, 1)], result: (ITEM_LIME_HARNESS, 1), name: "dye_lime_harness" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_62, 1)], result: (ITEM_LIME_WOOL, 1), name: "dye_lime_wool" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_63, 1)], result: (ITEM_LIME_WOOL_SLAB, 1), name: "dye_lime_wool_slab" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_64, 1)], result: (ITEM_LIME_WOOL_STAIRS, 1), name: "dye_lime_wool_stairs" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_65, 1)], result: (ITEM_MAGENTA_BED, 1), name: "dye_magenta_bed" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_66, 1)], result: (ITEM_MAGENTA_CARPET, 1), name: "dye_magenta_carpet" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_67, 1)], result: (ITEM_MAGENTA_CUSHION, 1), name: "dye_magenta_cushion" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_68, 1)], result: (ITEM_MAGENTA_HARNESS, 1), name: "dye_magenta_harness" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_69, 1)], result: (ITEM_MAGENTA_WOOL, 1), name: "dye_magenta_wool" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_70, 1)], result: (ITEM_MAGENTA_WOOL_SLAB, 1), name: "dye_magenta_wool_slab" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_71, 1)], result: (ITEM_MAGENTA_WOOL_STAIRS, 1), name: "dye_magenta_wool_stairs" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_72, 1)], result: (ITEM_ORANGE_BED, 1), name: "dye_orange_bed" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_73, 1)], result: (ITEM_ORANGE_CARPET, 1), name: "dye_orange_carpet" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_74, 1)], result: (ITEM_ORANGE_CUSHION, 1), name: "dye_orange_cushion" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_75, 1)], result: (ITEM_ORANGE_HARNESS, 1), name: "dye_orange_harness" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_76, 1)], result: (ITEM_ORANGE_WOOL, 1), name: "dye_orange_wool" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_77, 1)], result: (ITEM_ORANGE_WOOL_SLAB, 1), name: "dye_orange_wool_slab" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_78, 1)], result: (ITEM_ORANGE_WOOL_STAIRS, 1), name: "dye_orange_wool_stairs" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_79, 1)], result: (ITEM_PINK_BED, 1), name: "dye_pink_bed" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_80, 1)], result: (ITEM_PINK_CARPET, 1), name: "dye_pink_carpet" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_81, 1)], result: (ITEM_PINK_CUSHION, 1), name: "dye_pink_cushion" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_82, 1)], result: (ITEM_PINK_HARNESS, 1), name: "dye_pink_harness" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_83, 1)], result: (ITEM_PINK_WOOL, 1), name: "dye_pink_wool" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_84, 1)], result: (ITEM_PINK_WOOL_SLAB, 1), name: "dye_pink_wool_slab" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_85, 1)], result: (ITEM_PINK_WOOL_STAIRS, 1), name: "dye_pink_wool_stairs" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_86, 1)], result: (ITEM_PURPLE_BED, 1), name: "dye_purple_bed" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_87, 1)], result: (ITEM_PURPLE_CARPET, 1), name: "dye_purple_carpet" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_88, 1)], result: (ITEM_PURPLE_CUSHION, 1), name: "dye_purple_cushion" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_89, 1)], result: (ITEM_PURPLE_HARNESS, 1), name: "dye_purple_harness" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_90, 1)], result: (ITEM_PURPLE_WOOL, 1), name: "dye_purple_wool" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_91, 1)], result: (ITEM_PURPLE_WOOL_SLAB, 1), name: "dye_purple_wool_slab" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_92, 1)], result: (ITEM_PURPLE_WOOL_STAIRS, 1), name: "dye_purple_wool_stairs" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_93, 1)], result: (ITEM_RED_BED, 1), name: "dye_red_bed" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_94, 1)], result: (ITEM_RED_CARPET, 1), name: "dye_red_carpet" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_95, 1)], result: (ITEM_RED_CUSHION, 1), name: "dye_red_cushion" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_96, 1)], result: (ITEM_RED_HARNESS, 1), name: "dye_red_harness" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_97, 1)], result: (ITEM_RED_WOOL, 1), name: "dye_red_wool" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_98, 1)], result: (ITEM_RED_WOOL_SLAB, 1), name: "dye_red_wool_slab" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_99, 1)], result: (ITEM_RED_WOOL_STAIRS, 1), name: "dye_red_wool_stairs" },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_100, 1)], result: (ITEM_WHITE_BED, 1), name: "dye_white_bed" },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_101, 1)], result: (ITEM_WHITE_CARPET, 1), name: "dye_white_carpet" },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_102, 1)], result: (ITEM_WHITE_CUSHION, 1), name: "dye_white_cushion" },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_103, 1)], result: (ITEM_WHITE_HARNESS, 1), name: "dye_white_harness" },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_104, 1)], result: (ITEM_WHITE_WOOL, 1), name: "dye_white_wool" },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_105, 1)], result: (ITEM_WHITE_WOOL_SLAB, 1), name: "dye_white_wool_slab" },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_106, 1)], result: (ITEM_WHITE_WOOL_STAIRS, 1), name: "dye_white_wool_stairs" },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_107, 1)], result: (ITEM_YELLOW_BED, 1), name: "dye_yellow_bed" },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_108, 1)], result: (ITEM_YELLOW_CARPET, 1), name: "dye_yellow_carpet" },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_109, 1)], result: (ITEM_YELLOW_CUSHION, 1), name: "dye_yellow_cushion" },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_110, 1)], result: (ITEM_YELLOW_HARNESS, 1), name: "dye_yellow_harness" },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_111, 1)], result: (ITEM_YELLOW_WOOL, 1), name: "dye_yellow_wool" },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_112, 1)], result: (ITEM_YELLOW_WOOL_SLAB, 1), name: "dye_yellow_wool_slab" },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_113, 1)], result: (ITEM_YELLOW_WOOL_STAIRS, 1), name: "dye_yellow_wool_stairs" },
    Recipe { needs: &[(ITEM_EMERALD_BLOCK, 1)], result: (ITEM_EMERALD, 9), name: "emerald" },
    Recipe { needs: &[(ITEM_EMERALD, 9)], result: (ITEM_EMERALD_BLOCK, 1), name: "emerald_block" },
    Recipe { needs: &[(ITEM_BOOK, 1), (ITEM_DIAMOND, 2), (ITEM_OBSIDIAN, 4)], result: (ITEM_ENCHANTING_TABLE, 1), name: "enchanting_table" },
    Recipe { needs: &[(ITEM_ENDER_EYE, 1), (ITEM_GHAST_TEAR, 1), (ITEM_GLASS, 7)], result: (ITEM_END_CRYSTAL, 1), name: "end_crystal" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_POPPED_CHORUS_FRUIT, 1)], result: (ITEM_END_ROD, 4), name: "end_rod" },
    Recipe { needs: &[(ITEM_END_STONE_BRICKS, 3)], result: (ITEM_END_STONE_BRICK_SLAB, 6), name: "end_stone_brick_slab" },
    Recipe { needs: &[(ITEM_END_STONE_BRICKS, 6)], result: (ITEM_END_STONE_BRICK_STAIRS, 4), name: "end_stone_brick_stairs" },
    Recipe { needs: &[(ITEM_END_STONE_BRICKS, 6)], result: (ITEM_END_STONE_BRICK_WALL, 6), name: "end_stone_brick_wall" },
    Recipe { needs: &[(ITEM_END_STONE, 4)], result: (ITEM_END_STONE_BRICKS, 4), name: "end_stone_bricks" },
    Recipe { needs: &[(ITEM_ENDER_EYE, 1), (ITEM_OBSIDIAN, 8)], result: (ITEM_ENDER_CHEST, 1), name: "ender_chest" },
    Recipe { needs: &[(ITEM_BLAZE_POWDER, 1), (ITEM_ENDER_PEARL, 1)], result: (ITEM_ENDER_EYE, 1), name: "ender_eye" },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER_SLAB, 2)], result: (ITEM_EXPOSED_CHISELED_COPPER, 1), name: "exposed_chiseled_copper" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_EXPOSED_COPPER, 3), (ITEM_REDSTONE, 1)], result: (ITEM_EXPOSED_COPPER_BULB, 4), name: "exposed_copper_bulb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER, 4)], result: (ITEM_EXPOSED_COPPER_GRATE, 4), name: "exposed_copper_grate" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER, 4)], result: (ITEM_EXPOSED_CUT_COPPER, 4), name: "exposed_cut_copper" },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER, 3)], result: (ITEM_EXPOSED_CUT_COPPER_SLAB, 6), name: "exposed_cut_copper_slab" },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER, 6)], result: (ITEM_EXPOSED_CUT_COPPER_STAIRS, 4), name: "exposed_cut_copper_stairs" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_END_STONE, 1), (ITEM_EYE_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_EYE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "eye_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_BROWN_MUSHROOM, 1), (ITEM_SPIDER_EYE, 1), (ITEM_SUGAR, 1)], result: (ITEM_FERMENTED_SPIDER_EYE, 1), name: "fermented_spider_eye" },
    Recipe { needs: &[(ITEM_BRICKS, 1), (ITEM_PAPER, 1)], result: (ITEM_FIELD_MASONED_BANNER_PATTERN, 1), name: "field_masoned_banner_pattern" },
    Recipe { needs: &[(ITEM_BLAZE_POWDER, 1), (ITEM_GUNPOWDER, 1), (TAG_COALS, 1)], result: (ITEM_FIRE_CHARGE, 3), name: "fire_charge" },
    Recipe { needs: &[(ITEM_GUNPOWDER, 1), (ITEM_PAPER, 1)], result: (ITEM_FIREWORK_ROCKET, 3), name: "firework_rocket_simple" },
    Recipe { needs: &[(STICK, 3), (ITEM_STRING, 2)], result: (ITEM_FISHING_ROD, 1), name: "fishing_rod" },
    Recipe { needs: &[(ITEM_FLINT, 2), (TAG_PLANKS, 4)], result: (ITEM_FLETCHING_TABLE, 1), name: "fletching_table" },
    Recipe { needs: &[(ITEM_FLINT, 1), (ITEM_IRON_INGOT, 1)], result: (ITEM_FLINT_AND_STEEL, 1), name: "flint_and_steel" },
    Recipe { needs: &[(ITEM_BREEZE_ROD, 1), (ITEM_DIAMOND, 7), (ITEM_FLOW_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_FLOW_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "flow_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_OXEYE_DAISY, 1), (ITEM_PAPER, 1)], result: (ITEM_FLOWER_BANNER_PATTERN, 1), name: "flower_banner_pattern" },
    Recipe { needs: &[(ITEM_BRICK, 3)], result: (ITEM_FLOWER_POT, 1), name: "flower_pot" },
    Recipe { needs: &[(TAG_STONE_CRAFTING_MATERIALS, 8)], result: (ITEM_FURNACE, 1), name: "furnace" },
    Recipe { needs: &[(ITEM_FURNACE, 1), (ITEM_MINECART, 1)], result: (ITEM_FURNACE_MINECART, 1), name: "furnace_minecart" },
    Recipe { needs: &[(ITEM_GLASS, 3)], result: (ITEM_GLASS_BOTTLE, 3), name: "glass_bottle" },
    Recipe { needs: &[(ITEM_GLASS, 6)], result: (ITEM_GLASS_PANE, 16), name: "glass_pane" },
    Recipe { needs: &[(ITEM_GOLD_NUGGET, 8), (ITEM_MELON_SLICE, 1)], result: (ITEM_GLISTERING_MELON_SLICE, 1), name: "glistering_melon_slice" },
    Recipe { needs: &[(ITEM_GLOW_INK_SAC, 1), (ITEM_ITEM_FRAME, 1)], result: (ITEM_GLOW_ITEM_FRAME, 1), name: "glow_item_frame" },
    Recipe { needs: &[(ITEM_GLOWSTONE_DUST, 4)], result: (ITEM_GLOWSTONE, 1), name: "glowstone" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 9)], result: (ITEM_GOLD_BLOCK, 1), name: "gold_block" },
    Recipe { needs: &[(ITEM_GOLD_BLOCK, 1)], result: (ITEM_GOLD_INGOT, 9), name: "gold_ingot_from_gold_block" },
    Recipe { needs: &[(ITEM_GOLD_NUGGET, 9)], result: (ITEM_GOLD_INGOT, 1), name: "gold_ingot_from_nuggets" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 1)], result: (ITEM_GOLD_NUGGET, 9), name: "gold_nugget" },
    Recipe { needs: &[(ITEM_APPLE, 1), (ITEM_GOLD_INGOT, 8)], result: (ITEM_GOLDEN_APPLE, 1), name: "golden_apple" },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 3)], result: (ITEM_GOLDEN_AXE, 1), name: "golden_axe" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 4)], result: (ITEM_GOLDEN_BOOTS, 1), name: "golden_boots" },
    Recipe { needs: &[(ITEM_CARROT, 1), (ITEM_GOLD_NUGGET, 8)], result: (ITEM_GOLDEN_CARROT, 1), name: "golden_carrot" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 8)], result: (ITEM_GOLDEN_CHESTPLATE, 1), name: "golden_chestplate" },
    Recipe { needs: &[(ITEM_DANDELION, 1), (ITEM_GOLD_NUGGET, 8)], result: (ITEM_GOLDEN_DANDELION, 1), name: "golden_dandelion" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 5)], result: (ITEM_GOLDEN_HELMET, 1), name: "golden_helmet" },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 2)], result: (ITEM_GOLDEN_HOE, 1), name: "golden_hoe" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 7)], result: (ITEM_GOLDEN_LEGGINGS, 1), name: "golden_leggings" },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 3)], result: (ITEM_GOLDEN_PICKAXE, 1), name: "golden_pickaxe" },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 1)], result: (ITEM_GOLDEN_SHOVEL, 1), name: "golden_shovel" },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 1)], result: (ITEM_GOLDEN_SPEAR, 1), name: "golden_spear" },
    Recipe { needs: &[(STICK, 1), (TAG_GOLD_TOOL_MATERIALS, 2)], result: (ITEM_GOLDEN_SWORD, 1), name: "golden_sword" },
    Recipe { needs: &[(ITEM_DIORITE, 1), (ITEM_QUARTZ, 1)], result: (ITEM_GRANITE, 1), name: "granite" },
    Recipe { needs: &[(ITEM_GRANITE, 3)], result: (ITEM_GRANITE_SLAB, 6), name: "granite_slab" },
    Recipe { needs: &[(ITEM_GRANITE, 6)], result: (ITEM_GRANITE_STAIRS, 4), name: "granite_stairs" },
    Recipe { needs: &[(ITEM_GRANITE, 6)], result: (ITEM_GRANITE_WALL, 6), name: "granite_wall" },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 6), (STICK, 1)], result: (ITEM_GRAY_BANNER, 1), name: "gray_banner" },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_GRAY_BED, 1), name: "gray_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_GRAY_DYE, 1)], result: (ITEM_GRAY_CANDLE, 1), name: "gray_candle" },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 2)], result: (ITEM_GRAY_CARPET, 3), name: "gray_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAY_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_GRAY_CONCRETE_POWDER, 8), name: "gray_concrete_powder" },
    Recipe { needs: &[(ITEM_GRAY_CONCRETE, 3)], result: (ITEM_GRAY_CONCRETE_SLAB, 6), name: "gray_concrete_slab" },
    Recipe { needs: &[(ITEM_GRAY_CONCRETE, 6)], result: (ITEM_GRAY_CONCRETE_STAIRS, 4), name: "gray_concrete_stairs" },
    Recipe { needs: &[(ITEM_GRAY_WOOL_SLAB, 3)], result: (ITEM_GRAY_CUSHION, 1), name: "gray_cushion" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_GRAY_DYE, 2), name: "gray_dye" },
    Recipe { needs: &[(ITEM_CLOSED_EYEBLOSSOM, 1)], result: (ITEM_GRAY_DYE, 1), name: "gray_dye_from_closed_eyeblossom" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_GRAY_WOOL, 1), (ITEM_LEATHER, 3)], result: (ITEM_GRAY_HARNESS, 1), name: "gray_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_GRAY_DYE, 1)], result: (ITEM_GRAY_STAINED_GLASS, 8), name: "gray_stained_glass" },
    Recipe { needs: &[(ITEM_GRAY_STAINED_GLASS, 6)], result: (ITEM_GRAY_STAINED_GLASS_PANE, 16), name: "gray_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_GRAY_DYE, 1)], result: (ITEM_GRAY_STAINED_GLASS_PANE, 8), name: "gray_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_GRAY_TERRACOTTA, 8), name: "gray_terracotta" },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 3)], result: (ITEM_GRAY_WOOL_SLAB, 6), name: "gray_wool_slab" },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 6)], result: (ITEM_GRAY_WOOL_STAIRS, 4), name: "gray_wool_stairs" },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 6), (STICK, 1)], result: (ITEM_GREEN_BANNER, 1), name: "green_banner" },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_GREEN_BED, 1), name: "green_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_GREEN_DYE, 1)], result: (ITEM_GREEN_CANDLE, 1), name: "green_candle" },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 2)], result: (ITEM_GREEN_CARPET, 3), name: "green_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GREEN_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_GREEN_CONCRETE_POWDER, 8), name: "green_concrete_powder" },
    Recipe { needs: &[(ITEM_GREEN_CONCRETE, 3)], result: (ITEM_GREEN_CONCRETE_SLAB, 6), name: "green_concrete_slab" },
    Recipe { needs: &[(ITEM_GREEN_CONCRETE, 6)], result: (ITEM_GREEN_CONCRETE_STAIRS, 4), name: "green_concrete_stairs" },
    Recipe { needs: &[(ITEM_GREEN_WOOL_SLAB, 3)], result: (ITEM_GREEN_CUSHION, 1), name: "green_cushion" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_GREEN_WOOL, 1), (ITEM_LEATHER, 3)], result: (ITEM_GREEN_HARNESS, 1), name: "green_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_GREEN_DYE, 1)], result: (ITEM_GREEN_STAINED_GLASS, 8), name: "green_stained_glass" },
    Recipe { needs: &[(ITEM_GREEN_STAINED_GLASS, 6)], result: (ITEM_GREEN_STAINED_GLASS_PANE, 16), name: "green_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_GREEN_DYE, 1)], result: (ITEM_GREEN_STAINED_GLASS_PANE, 8), name: "green_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_GREEN_TERRACOTTA, 8), name: "green_terracotta" },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 3)], result: (ITEM_GREEN_WOOL_SLAB, 6), name: "green_wool_slab" },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 6)], result: (ITEM_GREEN_WOOL_STAIRS, 4), name: "green_wool_stairs" },
    Recipe { needs: &[(STICK, 2), (ITEM_STONE_SLAB, 1), (TAG_PLANKS, 2)], result: (ITEM_GRINDSTONE, 1), name: "grindstone" },
    Recipe { needs: &[(WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1)], result: (ITEM_HAY_BLOCK, 1), name: "hay_block" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 2)], result: (ITEM_HEAVY_WEIGHTED_PRESSURE_PLATE, 1), name: "heavy_weighted_pressure_plate" },
    Recipe { needs: &[(ITEM_HONEY_BOTTLE, 4)], result: (ITEM_HONEY_BLOCK, 1), name: "honey_block" },
    Recipe { needs: &[(ITEM_GLASS_BOTTLE, 1), (ITEM_GLASS_BOTTLE, 1), (ITEM_GLASS_BOTTLE, 1), (ITEM_GLASS_BOTTLE, 1), (ITEM_HONEY_BLOCK, 1)], result: (ITEM_HONEY_BOTTLE, 4), name: "honey_bottle" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 4)], result: (ITEM_HONEYCOMB_BLOCK, 1), name: "honeycomb_block" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_IRON_INGOT, 5)], result: (ITEM_HOPPER, 1), name: "hopper" },
    Recipe { needs: &[(ITEM_HOPPER, 1), (ITEM_MINECART, 1)], result: (ITEM_HOPPER_MINECART, 1), name: "hopper_minecart" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_HOST_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_TERRACOTTA, 1)], result: (ITEM_HOST_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "host_armor_trim_smithing_template" },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 3)], result: (ITEM_IRON_AXE, 1), name: "iron_axe" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6)], result: (ITEM_IRON_BARS, 16), name: "iron_bars" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 9)], result: (ITEM_IRON_BLOCK, 1), name: "iron_block" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 4)], result: (ITEM_IRON_BOOTS, 1), name: "iron_boots" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (ITEM_IRON_NUGGET, 2)], result: (ITEM_IRON_CHAIN, 1), name: "iron_chain" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 8)], result: (ITEM_IRON_CHESTPLATE, 1), name: "iron_chestplate" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6)], result: (ITEM_IRON_DOOR, 3), name: "iron_door" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 5)], result: (ITEM_IRON_HELMET, 1), name: "iron_helmet" },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 2)], result: (ITEM_IRON_HOE, 1), name: "iron_hoe" },
    Recipe { needs: &[(ITEM_IRON_BLOCK, 1)], result: (ITEM_IRON_INGOT, 9), name: "iron_ingot_from_iron_block" },
    Recipe { needs: &[(ITEM_IRON_NUGGET, 9)], result: (ITEM_IRON_INGOT, 1), name: "iron_ingot_from_nuggets" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 7)], result: (ITEM_IRON_LEGGINGS, 1), name: "iron_leggings" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1)], result: (ITEM_IRON_NUGGET, 9), name: "iron_nugget" },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 3)], result: (ITEM_IRON_PICKAXE, 1), name: "iron_pickaxe" },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 1)], result: (ITEM_IRON_SHOVEL, 1), name: "iron_shovel" },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 1)], result: (ITEM_IRON_SPEAR, 1), name: "iron_spear" },
    Recipe { needs: &[(STICK, 1), (TAG_IRON_TOOL_MATERIALS, 2)], result: (ITEM_IRON_SWORD, 1), name: "iron_sword" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 4)], result: (ITEM_IRON_TRAPDOOR, 1), name: "iron_trapdoor" },
    Recipe { needs: &[(ITEM_LEATHER, 1), (STICK, 8)], result: (ITEM_ITEM_FRAME, 1), name: "item_frame" },
    Recipe { needs: &[(ITEM_CARVED_PUMPKIN, 1), (TORCH, 1)], result: (ITEM_JACK_O_LANTERN, 1), name: "jack_o_lantern" },
    Recipe { needs: &[(ITEM_DIAMOND, 1), (TAG_PLANKS, 8)], result: (ITEM_JUKEBOX, 1), name: "jukebox" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 5)], result: (ITEM_JUNGLE_BOAT, 1), name: "jungle_boat" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 1)], result: (ITEM_JUNGLE_BUTTON, 1), name: "jungle_button" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_JUNGLE_BOAT, 1)], result: (ITEM_JUNGLE_CHEST_BOAT, 1), name: "jungle_chest_boat" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6)], result: (ITEM_JUNGLE_DOOR, 3), name: "jungle_door" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 4), (STICK, 2)], result: (ITEM_JUNGLE_FENCE, 3), name: "jungle_fence" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 2), (STICK, 4)], result: (ITEM_JUNGLE_FENCE_GATE, 1), name: "jungle_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_JUNGLE_LOG, 6)], result: (ITEM_JUNGLE_HANGING_SIGN, 6), name: "jungle_hanging_sign" },
    Recipe { needs: &[(TAG_JUNGLE_LOGS, 1)], result: (ITEM_JUNGLE_PLANKS, 4), name: "jungle_planks" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 2)], result: (ITEM_JUNGLE_PRESSURE_PLATE, 1), name: "jungle_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_JUNGLE_LOG, 6)], result: (ITEM_JUNGLE_SHELF, 6), name: "jungle_shelf" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6), (STICK, 1)], result: (ITEM_JUNGLE_SIGN, 3), name: "jungle_sign" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 3)], result: (ITEM_JUNGLE_SLAB, 6), name: "jungle_slab" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6)], result: (ITEM_JUNGLE_STAIRS, 4), name: "jungle_stairs" },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6)], result: (ITEM_JUNGLE_TRAPDOOR, 2), name: "jungle_trapdoor" },
    Recipe { needs: &[(ITEM_JUNGLE_LOG, 4)], result: (ITEM_JUNGLE_WOOD, 3), name: "jungle_wood" },
    Recipe { needs: &[(STICK, 7)], result: (ITEM_LADDER, 3), name: "ladder" },
    Recipe { needs: &[(ITEM_IRON_NUGGET, 8), (TORCH, 1)], result: (ITEM_LANTERN, 1), name: "lantern" },
    Recipe { needs: &[(ITEM_LAPIS_LAZULI, 9)], result: (ITEM_LAPIS_BLOCK, 1), name: "lapis_block" },
    Recipe { needs: &[(ITEM_LAPIS_BLOCK, 1)], result: (ITEM_LAPIS_LAZULI, 9), name: "lapis_lazuli" },
    Recipe { needs: &[(ITEM_STRING, 5)], result: (ITEM_LEAD, 2), name: "lead" },
    Recipe { needs: &[(ITEM_RABBIT_HIDE, 4)], result: (ITEM_LEATHER, 1), name: "leather" },
    Recipe { needs: &[(ITEM_LEATHER, 4)], result: (ITEM_LEATHER_BOOTS, 1), name: "leather_boots" },
    Recipe { needs: &[(ITEM_LEATHER, 8)], result: (ITEM_LEATHER_CHESTPLATE, 1), name: "leather_chestplate" },
    Recipe { needs: &[(ITEM_LEATHER, 5)], result: (ITEM_LEATHER_HELMET, 1), name: "leather_helmet" },
    Recipe { needs: &[(ITEM_LEATHER, 7)], result: (ITEM_LEATHER_HORSE_ARMOR, 1), name: "leather_horse_armor" },
    Recipe { needs: &[(ITEM_LEATHER, 7)], result: (ITEM_LEATHER_LEGGINGS, 1), name: "leather_leggings" },
    Recipe { needs: &[(ITEM_BOOKSHELF, 1), (TAG_WOODEN_SLABS, 4)], result: (ITEM_LECTERN, 1), name: "lectern" },
    Recipe { needs: &[(COBBLESTONE, 1), (STICK, 1)], result: (ITEM_LEVER, 1), name: "lever" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 6), (STICK, 1)], result: (ITEM_LIGHT_BLUE_BANNER, 1), name: "light_blue_banner" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_LIGHT_BLUE_BED, 1), name: "light_blue_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_LIGHT_BLUE_DYE, 1)], result: (ITEM_LIGHT_BLUE_CANDLE, 1), name: "light_blue_candle" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 2)], result: (ITEM_LIGHT_BLUE_CARPET, 3), name: "light_blue_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_LIGHT_BLUE_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_LIGHT_BLUE_CONCRETE_POWDER, 8), name: "light_blue_concrete_powder" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_CONCRETE, 3)], result: (ITEM_LIGHT_BLUE_CONCRETE_SLAB, 6), name: "light_blue_concrete_slab" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_CONCRETE, 6)], result: (ITEM_LIGHT_BLUE_CONCRETE_STAIRS, 4), name: "light_blue_concrete_stairs" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL_SLAB, 3)], result: (ITEM_LIGHT_BLUE_CUSHION, 1), name: "light_blue_cushion" },
    Recipe { needs: &[(ITEM_BLUE_ORCHID, 1)], result: (ITEM_LIGHT_BLUE_DYE, 1), name: "light_blue_dye_from_blue_orchid" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_LIGHT_BLUE_DYE, 2), name: "light_blue_dye_from_blue_white_dye" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_LIGHT_BLUE_WOOL, 1)], result: (ITEM_LIGHT_BLUE_HARNESS, 1), name: "light_blue_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_LIGHT_BLUE_DYE, 1)], result: (ITEM_LIGHT_BLUE_STAINED_GLASS, 8), name: "light_blue_stained_glass" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_STAINED_GLASS, 6)], result: (ITEM_LIGHT_BLUE_STAINED_GLASS_PANE, 16), name: "light_blue_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_LIGHT_BLUE_DYE, 1)], result: (ITEM_LIGHT_BLUE_STAINED_GLASS_PANE, 8), name: "light_blue_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_LIGHT_BLUE_TERRACOTTA, 8), name: "light_blue_terracotta" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 3)], result: (ITEM_LIGHT_BLUE_WOOL_SLAB, 6), name: "light_blue_wool_slab" },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 6)], result: (ITEM_LIGHT_BLUE_WOOL_STAIRS, 4), name: "light_blue_wool_stairs" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 6), (STICK, 1)], result: (ITEM_LIGHT_GRAY_BANNER, 1), name: "light_gray_banner" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_LIGHT_GRAY_BED, 1), name: "light_gray_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_LIGHT_GRAY_DYE, 1)], result: (ITEM_LIGHT_GRAY_CANDLE, 1), name: "light_gray_candle" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 2)], result: (ITEM_LIGHT_GRAY_CARPET, 3), name: "light_gray_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_LIGHT_GRAY_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_LIGHT_GRAY_CONCRETE_POWDER, 8), name: "light_gray_concrete_powder" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_CONCRETE, 3)], result: (ITEM_LIGHT_GRAY_CONCRETE_SLAB, 6), name: "light_gray_concrete_slab" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_CONCRETE, 6)], result: (ITEM_LIGHT_GRAY_CONCRETE_STAIRS, 4), name: "light_gray_concrete_stairs" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL_SLAB, 3)], result: (ITEM_LIGHT_GRAY_CUSHION, 1), name: "light_gray_cushion" },
    Recipe { needs: &[(ITEM_AZURE_BLUET, 1)], result: (ITEM_LIGHT_GRAY_DYE, 1), name: "light_gray_dye_from_azure_bluet" },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_WHITE_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_LIGHT_GRAY_DYE, 3), name: "light_gray_dye_from_black_white_dye" },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_LIGHT_GRAY_DYE, 2), name: "light_gray_dye_from_gray_white_dye" },
    Recipe { needs: &[(ITEM_OXEYE_DAISY, 1)], result: (ITEM_LIGHT_GRAY_DYE, 1), name: "light_gray_dye_from_oxeye_daisy" },
    Recipe { needs: &[(ITEM_WHITE_TULIP, 1)], result: (ITEM_LIGHT_GRAY_DYE, 1), name: "light_gray_dye_from_white_tulip" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_LIGHT_GRAY_WOOL, 1)], result: (ITEM_LIGHT_GRAY_HARNESS, 1), name: "light_gray_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_LIGHT_GRAY_DYE, 1)], result: (ITEM_LIGHT_GRAY_STAINED_GLASS, 8), name: "light_gray_stained_glass" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_STAINED_GLASS, 6)], result: (ITEM_LIGHT_GRAY_STAINED_GLASS_PANE, 16), name: "light_gray_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_LIGHT_GRAY_DYE, 1)], result: (ITEM_LIGHT_GRAY_STAINED_GLASS_PANE, 8), name: "light_gray_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_LIGHT_GRAY_TERRACOTTA, 8), name: "light_gray_terracotta" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 3)], result: (ITEM_LIGHT_GRAY_WOOL_SLAB, 6), name: "light_gray_wool_slab" },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 6)], result: (ITEM_LIGHT_GRAY_WOOL_STAIRS, 4), name: "light_gray_wool_stairs" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 2)], result: (ITEM_LIGHT_WEIGHTED_PRESSURE_PLATE, 1), name: "light_weighted_pressure_plate" },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 3)], result: (ITEM_LIGHTNING_ROD, 1), name: "lightning_rod" },
    Recipe { needs: &[(ITEM_LIME_WOOL, 6), (STICK, 1)], result: (ITEM_LIME_BANNER, 1), name: "lime_banner" },
    Recipe { needs: &[(ITEM_LIME_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_LIME_BED, 1), name: "lime_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_LIME_DYE, 1)], result: (ITEM_LIME_CANDLE, 1), name: "lime_candle" },
    Recipe { needs: &[(ITEM_LIME_WOOL, 2)], result: (ITEM_LIME_CARPET, 3), name: "lime_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_LIME_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_LIME_CONCRETE_POWDER, 8), name: "lime_concrete_powder" },
    Recipe { needs: &[(ITEM_LIME_CONCRETE, 3)], result: (ITEM_LIME_CONCRETE_SLAB, 6), name: "lime_concrete_slab" },
    Recipe { needs: &[(ITEM_LIME_CONCRETE, 6)], result: (ITEM_LIME_CONCRETE_STAIRS, 4), name: "lime_concrete_stairs" },
    Recipe { needs: &[(ITEM_LIME_WOOL_SLAB, 3)], result: (ITEM_LIME_CUSHION, 1), name: "lime_cushion" },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_LIME_DYE, 2), name: "lime_dye" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_LIME_WOOL, 1)], result: (ITEM_LIME_HARNESS, 1), name: "lime_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_LIME_DYE, 1)], result: (ITEM_LIME_STAINED_GLASS, 8), name: "lime_stained_glass" },
    Recipe { needs: &[(ITEM_LIME_STAINED_GLASS, 6)], result: (ITEM_LIME_STAINED_GLASS_PANE, 16), name: "lime_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_LIME_DYE, 1)], result: (ITEM_LIME_STAINED_GLASS_PANE, 8), name: "lime_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_LIME_TERRACOTTA, 8), name: "lime_terracotta" },
    Recipe { needs: &[(ITEM_LIME_WOOL, 3)], result: (ITEM_LIME_WOOL_SLAB, 6), name: "lime_wool_slab" },
    Recipe { needs: &[(ITEM_LIME_WOOL, 6)], result: (ITEM_LIME_WOOL_STAIRS, 4), name: "lime_wool_stairs" },
    Recipe { needs: &[(ITEM_CHISELED_STONE_BRICKS, 8), (ITEM_IRON_INGOT, 1)], result: (ITEM_LODESTONE, 1), name: "lodestone" },
    Recipe { needs: &[(ITEM_STRING, 2), (TAG_PLANKS, 2)], result: (ITEM_LOOM, 1), name: "loom" },
    Recipe { needs: &[(ITEM_BREEZE_ROD, 1), (ITEM_HEAVY_CORE, 1)], result: (ITEM_MACE, 1), name: "mace" },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 6), (STICK, 1)], result: (ITEM_MAGENTA_BANNER, 1), name: "magenta_banner" },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_MAGENTA_BED, 1), name: "magenta_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_MAGENTA_DYE, 1)], result: (ITEM_MAGENTA_CANDLE, 1), name: "magenta_candle" },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 2)], result: (ITEM_MAGENTA_CARPET, 3), name: "magenta_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_MAGENTA_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_MAGENTA_CONCRETE_POWDER, 8), name: "magenta_concrete_powder" },
    Recipe { needs: &[(ITEM_MAGENTA_CONCRETE, 3)], result: (ITEM_MAGENTA_CONCRETE_SLAB, 6), name: "magenta_concrete_slab" },
    Recipe { needs: &[(ITEM_MAGENTA_CONCRETE, 6)], result: (ITEM_MAGENTA_CONCRETE_STAIRS, 4), name: "magenta_concrete_stairs" },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL_SLAB, 3)], result: (ITEM_MAGENTA_CUSHION, 1), name: "magenta_cushion" },
    Recipe { needs: &[(ITEM_ALLIUM, 1)], result: (ITEM_MAGENTA_DYE, 1), name: "magenta_dye_from_allium" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_PINK_DYE, 1), (ITEM_RED_DYE, 1)], result: (ITEM_MAGENTA_DYE, 3), name: "magenta_dye_from_blue_red_pink" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_RED_DYE, 1), (ITEM_RED_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_MAGENTA_DYE, 4), name: "magenta_dye_from_blue_red_white_dye" },
    Recipe { needs: &[(ITEM_LILAC, 1)], result: (ITEM_MAGENTA_DYE, 2), name: "magenta_dye_from_lilac" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (ITEM_PURPLE_DYE, 1)], result: (ITEM_MAGENTA_DYE, 2), name: "magenta_dye_from_purple_and_pink" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_MAGENTA_WOOL, 1)], result: (ITEM_MAGENTA_HARNESS, 1), name: "magenta_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_MAGENTA_DYE, 1)], result: (ITEM_MAGENTA_STAINED_GLASS, 8), name: "magenta_stained_glass" },
    Recipe { needs: &[(ITEM_MAGENTA_STAINED_GLASS, 6)], result: (ITEM_MAGENTA_STAINED_GLASS_PANE, 16), name: "magenta_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_MAGENTA_DYE, 1)], result: (ITEM_MAGENTA_STAINED_GLASS_PANE, 8), name: "magenta_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_MAGENTA_TERRACOTTA, 8), name: "magenta_terracotta" },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 3)], result: (ITEM_MAGENTA_WOOL_SLAB, 6), name: "magenta_wool_slab" },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 6)], result: (ITEM_MAGENTA_WOOL_STAIRS, 4), name: "magenta_wool_stairs" },
    Recipe { needs: &[(ITEM_MAGMA_CREAM, 4)], result: (ITEM_MAGMA_BLOCK, 1), name: "magma_block" },
    Recipe { needs: &[(ITEM_BLAZE_POWDER, 1), (ITEM_SLIME_BALL, 1)], result: (ITEM_MAGMA_CREAM, 1), name: "magma_cream" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 5)], result: (ITEM_MANGROVE_BOAT, 1), name: "mangrove_boat" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 1)], result: (ITEM_MANGROVE_BUTTON, 1), name: "mangrove_button" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_MANGROVE_BOAT, 1)], result: (ITEM_MANGROVE_CHEST_BOAT, 1), name: "mangrove_chest_boat" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6)], result: (ITEM_MANGROVE_DOOR, 3), name: "mangrove_door" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 4), (STICK, 2)], result: (ITEM_MANGROVE_FENCE, 3), name: "mangrove_fence" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 2), (STICK, 4)], result: (ITEM_MANGROVE_FENCE_GATE, 1), name: "mangrove_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_MANGROVE_LOG, 6)], result: (ITEM_MANGROVE_HANGING_SIGN, 6), name: "mangrove_hanging_sign" },
    Recipe { needs: &[(TAG_MANGROVE_LOGS, 1)], result: (ITEM_MANGROVE_PLANKS, 4), name: "mangrove_planks" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 2)], result: (ITEM_MANGROVE_PRESSURE_PLATE, 1), name: "mangrove_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_MANGROVE_LOG, 6)], result: (ITEM_MANGROVE_SHELF, 6), name: "mangrove_shelf" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6), (STICK, 1)], result: (ITEM_MANGROVE_SIGN, 3), name: "mangrove_sign" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 3)], result: (ITEM_MANGROVE_SLAB, 6), name: "mangrove_slab" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6)], result: (ITEM_MANGROVE_STAIRS, 4), name: "mangrove_stairs" },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6)], result: (ITEM_MANGROVE_TRAPDOOR, 2), name: "mangrove_trapdoor" },
    Recipe { needs: &[(ITEM_MANGROVE_LOG, 4)], result: (ITEM_MANGROVE_WOOD, 3), name: "mangrove_wood" },
    Recipe { needs: &[(ITEM_COMPASS, 1), (ITEM_PAPER, 8)], result: (ITEM_MAP, 1), name: "map" },
    Recipe { needs: &[(ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1)], result: (ITEM_MELON, 1), name: "melon" },
    Recipe { needs: &[(ITEM_MELON_SLICE, 1)], result: (ITEM_MELON_SEEDS, 1), name: "melon_seeds" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 5)], result: (ITEM_MINECART, 1), name: "minecart" },
    Recipe { needs: &[(ITEM_ENCHANTED_GOLDEN_APPLE, 1), (ITEM_PAPER, 1)], result: (ITEM_MOJANG_BANNER_PATTERN, 1), name: "mojang_banner_pattern" },
    Recipe { needs: &[(ITEM_MOSS_BLOCK, 2)], result: (ITEM_MOSS_CARPET, 3), name: "moss_carpet" },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_MOSS_BLOCK, 1)], result: (ITEM_MOSSY_COBBLESTONE, 1), name: "mossy_cobblestone_from_moss_block" },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_VINE, 1)], result: (ITEM_MOSSY_COBBLESTONE, 1), name: "mossy_cobblestone_from_vine" },
    Recipe { needs: &[(ITEM_MOSSY_COBBLESTONE, 3)], result: (ITEM_MOSSY_COBBLESTONE_SLAB, 6), name: "mossy_cobblestone_slab" },
    Recipe { needs: &[(ITEM_MOSSY_COBBLESTONE, 6)], result: (ITEM_MOSSY_COBBLESTONE_STAIRS, 4), name: "mossy_cobblestone_stairs" },
    Recipe { needs: &[(ITEM_MOSSY_COBBLESTONE, 6)], result: (ITEM_MOSSY_COBBLESTONE_WALL, 6), name: "mossy_cobblestone_wall" },
    Recipe { needs: &[(ITEM_MOSSY_STONE_BRICKS, 3)], result: (ITEM_MOSSY_STONE_BRICK_SLAB, 6), name: "mossy_stone_brick_slab" },
    Recipe { needs: &[(ITEM_MOSSY_STONE_BRICKS, 6)], result: (ITEM_MOSSY_STONE_BRICK_STAIRS, 4), name: "mossy_stone_brick_stairs" },
    Recipe { needs: &[(ITEM_MOSSY_STONE_BRICKS, 6)], result: (ITEM_MOSSY_STONE_BRICK_WALL, 6), name: "mossy_stone_brick_wall" },
    Recipe { needs: &[(ITEM_MOSS_BLOCK, 1), (ITEM_STONE_BRICKS, 1)], result: (ITEM_MOSSY_STONE_BRICKS, 1), name: "mossy_stone_bricks_from_moss_block" },
    Recipe { needs: &[(ITEM_STONE_BRICKS, 1), (ITEM_VINE, 1)], result: (ITEM_MOSSY_STONE_BRICKS, 1), name: "mossy_stone_bricks_from_vine" },
    Recipe { needs: &[(ITEM_MUD_BRICKS, 3)], result: (ITEM_MUD_BRICK_SLAB, 6), name: "mud_brick_slab" },
    Recipe { needs: &[(ITEM_MUD_BRICKS, 6)], result: (ITEM_MUD_BRICK_STAIRS, 4), name: "mud_brick_stairs" },
    Recipe { needs: &[(ITEM_MUD_BRICKS, 6)], result: (ITEM_MUD_BRICK_WALL, 6), name: "mud_brick_wall" },
    Recipe { needs: &[(ITEM_PACKED_MUD, 4)], result: (ITEM_MUD_BRICKS, 4), name: "mud_bricks" },
    Recipe { needs: &[(ITEM_MANGROVE_ROOTS, 1), (ITEM_MUD, 1)], result: (ITEM_MUDDY_MANGROVE_ROOTS, 1), name: "muddy_mangrove_roots" },
    Recipe { needs: &[(ITEM_BOWL, 1), (TAG_MUSHROOMS, 1), (TAG_MUSHROOMS, 1)], result: (ITEM_MUSHROOM_STEW, 1), name: "mushroom_stew" },
    Recipe { needs: &[(ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1)], result: (ITEM_MUSIC_DISC_5, 1), name: "music_disc_5" },
    Recipe { needs: &[(ITEM_PAPER, 1), (TAG_METAL_NUGGETS, 1)], result: (ITEM_NAME_TAG, 1), name: "name_tag" },
    Recipe { needs: &[(ITEM_NETHER_BRICK, 2), (ITEM_NETHER_BRICKS, 4)], result: (ITEM_NETHER_BRICK_FENCE, 6), name: "nether_brick_fence" },
    Recipe { needs: &[(ITEM_NETHER_BRICKS, 3)], result: (ITEM_NETHER_BRICK_SLAB, 6), name: "nether_brick_slab" },
    Recipe { needs: &[(ITEM_NETHER_BRICKS, 6)], result: (ITEM_NETHER_BRICK_STAIRS, 4), name: "nether_brick_stairs" },
    Recipe { needs: &[(ITEM_NETHER_BRICKS, 6)], result: (ITEM_NETHER_BRICK_WALL, 6), name: "nether_brick_wall" },
    Recipe { needs: &[(ITEM_NETHER_BRICK, 4)], result: (ITEM_NETHER_BRICKS, 1), name: "nether_bricks" },
    Recipe { needs: &[(ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1)], result: (ITEM_NETHER_WART_BLOCK, 1), name: "nether_wart_block" },
    Recipe { needs: &[(ITEM_NETHERITE_INGOT, 9)], result: (ITEM_NETHERITE_BLOCK, 1), name: "netherite_block" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 1), (ITEM_GOLD_INGOT, 1), (ITEM_GOLD_INGOT, 1), (ITEM_GOLD_INGOT, 1), (ITEM_NETHERITE_SCRAP, 1), (ITEM_NETHERITE_SCRAP, 1), (ITEM_NETHERITE_SCRAP, 1), (ITEM_NETHERITE_SCRAP, 1)], result: (ITEM_NETHERITE_INGOT, 1), name: "netherite_ingot" },
    Recipe { needs: &[(ITEM_NETHERITE_BLOCK, 1)], result: (ITEM_NETHERITE_INGOT, 9), name: "netherite_ingot_from_netherite_block" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_NETHERITE_UPGRADE_SMITHING_TEMPLATE, 1), (ITEM_NETHERRACK, 1)], result: (ITEM_NETHERITE_UPGRADE_SMITHING_TEMPLATE, 2), name: "netherite_upgrade_smithing_template" },
    Recipe { needs: &[(ITEM_REDSTONE, 1), (TAG_PLANKS, 8)], result: (ITEM_NOTE_BLOCK, 1), name: "note_block" },
    Recipe { needs: &[(OAK_PLANKS, 5)], result: (ITEM_OAK_BOAT, 1), name: "oak_boat" },
    Recipe { needs: &[(OAK_PLANKS, 1)], result: (ITEM_OAK_BUTTON, 1), name: "oak_button" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_OAK_BOAT, 1)], result: (ITEM_OAK_CHEST_BOAT, 1), name: "oak_chest_boat" },
    Recipe { needs: &[(OAK_PLANKS, 6)], result: (ITEM_OAK_DOOR, 3), name: "oak_door" },
    Recipe { needs: &[(OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_OAK_FENCE, 3), name: "oak_fence" },
    Recipe { needs: &[(OAK_PLANKS, 2), (STICK, 4)], result: (ITEM_OAK_FENCE_GATE, 1), name: "oak_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_OAK_LOG, 6)], result: (ITEM_OAK_HANGING_SIGN, 6), name: "oak_hanging_sign" },
    Recipe { needs: &[(TAG_OAK_LOGS, 1)], result: (OAK_PLANKS, 4), name: "oak_planks" },
    Recipe { needs: &[(OAK_PLANKS, 2)], result: (ITEM_OAK_PRESSURE_PLATE, 1), name: "oak_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_OAK_LOG, 6)], result: (ITEM_OAK_SHELF, 6), name: "oak_shelf" },
    Recipe { needs: &[(OAK_PLANKS, 6), (STICK, 1)], result: (ITEM_OAK_SIGN, 3), name: "oak_sign" },
    Recipe { needs: &[(OAK_PLANKS, 3)], result: (ITEM_OAK_SLAB, 6), name: "oak_slab" },
    Recipe { needs: &[(OAK_PLANKS, 6)], result: (ITEM_OAK_STAIRS, 4), name: "oak_stairs" },
    Recipe { needs: &[(OAK_PLANKS, 6)], result: (ITEM_OAK_TRAPDOOR, 2), name: "oak_trapdoor" },
    Recipe { needs: &[(OAK_LOG, 4)], result: (ITEM_OAK_WOOD, 3), name: "oak_wood" },
    Recipe { needs: &[(COBBLESTONE, 6), (ITEM_QUARTZ, 1), (ITEM_REDSTONE, 2)], result: (ITEM_OBSERVER, 1), name: "observer" },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 6), (STICK, 1)], result: (ITEM_ORANGE_BANNER, 1), name: "orange_banner" },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_ORANGE_BED, 1), name: "orange_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_ORANGE_DYE, 1)], result: (ITEM_ORANGE_CANDLE, 1), name: "orange_candle" },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 2)], result: (ITEM_ORANGE_CARPET, 3), name: "orange_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_ORANGE_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_ORANGE_CONCRETE_POWDER, 8), name: "orange_concrete_powder" },
    Recipe { needs: &[(ITEM_ORANGE_CONCRETE, 3)], result: (ITEM_ORANGE_CONCRETE_SLAB, 6), name: "orange_concrete_slab" },
    Recipe { needs: &[(ITEM_ORANGE_CONCRETE, 6)], result: (ITEM_ORANGE_CONCRETE_STAIRS, 4), name: "orange_concrete_stairs" },
    Recipe { needs: &[(ITEM_ORANGE_WOOL_SLAB, 3)], result: (ITEM_ORANGE_CUSHION, 1), name: "orange_cushion" },
    Recipe { needs: &[(ITEM_OPEN_EYEBLOSSOM, 1)], result: (ITEM_ORANGE_DYE, 1), name: "orange_dye_from_open_eyeblossom" },
    Recipe { needs: &[(ITEM_ORANGE_TULIP, 1)], result: (ITEM_ORANGE_DYE, 1), name: "orange_dye_from_orange_tulip" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (ITEM_YELLOW_DYE, 1)], result: (ITEM_ORANGE_DYE, 2), name: "orange_dye_from_red_yellow" },
    Recipe { needs: &[(ITEM_TORCHFLOWER, 1)], result: (ITEM_ORANGE_DYE, 1), name: "orange_dye_from_torchflower" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_ORANGE_WOOL, 1)], result: (ITEM_ORANGE_HARNESS, 1), name: "orange_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_ORANGE_DYE, 1)], result: (ITEM_ORANGE_STAINED_GLASS, 8), name: "orange_stained_glass" },
    Recipe { needs: &[(ITEM_ORANGE_STAINED_GLASS, 6)], result: (ITEM_ORANGE_STAINED_GLASS_PANE, 16), name: "orange_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_ORANGE_DYE, 1)], result: (ITEM_ORANGE_STAINED_GLASS_PANE, 8), name: "orange_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_ORANGE_TERRACOTTA, 8), name: "orange_terracotta" },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 3)], result: (ITEM_ORANGE_WOOL_SLAB, 6), name: "orange_wool_slab" },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 6)], result: (ITEM_ORANGE_WOOL_STAIRS, 4), name: "orange_wool_stairs" },
    Recipe { needs: &[(ITEM_OXIDIZED_CUT_COPPER_SLAB, 2)], result: (ITEM_OXIDIZED_CHISELED_COPPER, 1), name: "oxidized_chiseled_copper" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_OXIDIZED_COPPER, 3), (ITEM_REDSTONE, 1)], result: (ITEM_OXIDIZED_COPPER_BULB, 4), name: "oxidized_copper_bulb" },
    Recipe { needs: &[(ITEM_OXIDIZED_COPPER, 4)], result: (ITEM_OXIDIZED_COPPER_GRATE, 4), name: "oxidized_copper_grate" },
    Recipe { needs: &[(ITEM_OXIDIZED_COPPER, 4)], result: (ITEM_OXIDIZED_CUT_COPPER, 4), name: "oxidized_cut_copper" },
    Recipe { needs: &[(ITEM_OXIDIZED_CUT_COPPER, 3)], result: (ITEM_OXIDIZED_CUT_COPPER_SLAB, 6), name: "oxidized_cut_copper_slab" },
    Recipe { needs: &[(ITEM_OXIDIZED_CUT_COPPER, 6)], result: (ITEM_OXIDIZED_CUT_COPPER_STAIRS, 4), name: "oxidized_cut_copper_stairs" },
    Recipe { needs: &[(ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1)], result: (ITEM_PACKED_ICE, 1), name: "packed_ice" },
    Recipe { needs: &[(ITEM_MUD, 1), (WHEAT, 1)], result: (ITEM_PACKED_MUD, 1), name: "packed_mud" },
    Recipe { needs: &[(STICK, 8), (TAG_SULFUR_CUBE_ARCHETYPE_LIGHT, 1)], result: (ITEM_PAINTING, 1), name: "painting" },
    Recipe { needs: &[(ITEM_PALE_MOSS_BLOCK, 2)], result: (ITEM_PALE_MOSS_CARPET, 3), name: "pale_moss_carpet" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 5)], result: (ITEM_PALE_OAK_BOAT, 1), name: "pale_oak_boat" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 1)], result: (ITEM_PALE_OAK_BUTTON, 1), name: "pale_oak_button" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_PALE_OAK_BOAT, 1)], result: (ITEM_PALE_OAK_CHEST_BOAT, 1), name: "pale_oak_chest_boat" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 6)], result: (ITEM_PALE_OAK_DOOR, 3), name: "pale_oak_door" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_PALE_OAK_FENCE, 3), name: "pale_oak_fence" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 2), (STICK, 4)], result: (ITEM_PALE_OAK_FENCE_GATE, 1), name: "pale_oak_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_PALE_OAK_LOG, 6)], result: (ITEM_PALE_OAK_HANGING_SIGN, 6), name: "pale_oak_hanging_sign" },
    Recipe { needs: &[(TAG_PALE_OAK_LOGS, 1)], result: (ITEM_PALE_OAK_PLANKS, 4), name: "pale_oak_planks" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 2)], result: (ITEM_PALE_OAK_PRESSURE_PLATE, 1), name: "pale_oak_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_PALE_OAK_LOG, 6)], result: (ITEM_PALE_OAK_SHELF, 6), name: "pale_oak_shelf" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 6), (STICK, 1)], result: (ITEM_PALE_OAK_SIGN, 3), name: "pale_oak_sign" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 3)], result: (ITEM_PALE_OAK_SLAB, 6), name: "pale_oak_slab" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 6)], result: (ITEM_PALE_OAK_STAIRS, 4), name: "pale_oak_stairs" },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 6)], result: (ITEM_PALE_OAK_TRAPDOOR, 2), name: "pale_oak_trapdoor" },
    Recipe { needs: &[(ITEM_PALE_OAK_LOG, 4)], result: (ITEM_PALE_OAK_WOOD, 3), name: "pale_oak_wood" },
    Recipe { needs: &[(ITEM_SUGAR_CANE, 3)], result: (ITEM_PAPER, 3), name: "paper" },
    Recipe { needs: &[(ITEM_PINK_WOOL, 6), (STICK, 1)], result: (ITEM_PINK_BANNER, 1), name: "pink_banner" },
    Recipe { needs: &[(ITEM_PINK_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_PINK_BED, 1), name: "pink_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_PINK_DYE, 1)], result: (ITEM_PINK_CANDLE, 1), name: "pink_candle" },
    Recipe { needs: &[(ITEM_PINK_WOOL, 2)], result: (ITEM_PINK_CARPET, 3), name: "pink_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_PINK_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_PINK_CONCRETE_POWDER, 8), name: "pink_concrete_powder" },
    Recipe { needs: &[(ITEM_PINK_CONCRETE, 3)], result: (ITEM_PINK_CONCRETE_SLAB, 6), name: "pink_concrete_slab" },
    Recipe { needs: &[(ITEM_PINK_CONCRETE, 6)], result: (ITEM_PINK_CONCRETE_STAIRS, 4), name: "pink_concrete_stairs" },
    Recipe { needs: &[(ITEM_PINK_WOOL_SLAB, 3)], result: (ITEM_PINK_CUSHION, 1), name: "pink_cushion" },
    Recipe { needs: &[(ITEM_CACTUS_FLOWER, 1)], result: (ITEM_PINK_DYE, 1), name: "pink_dye_from_cactus_flower" },
    Recipe { needs: &[(ITEM_PEONY, 1)], result: (ITEM_PINK_DYE, 2), name: "pink_dye_from_peony" },
    Recipe { needs: &[(ITEM_PINK_PETALS, 1)], result: (ITEM_PINK_DYE, 1), name: "pink_dye_from_pink_petals" },
    Recipe { needs: &[(ITEM_PINK_TULIP, 1)], result: (ITEM_PINK_DYE, 1), name: "pink_dye_from_pink_tulip" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_PINK_DYE, 2), name: "pink_dye_from_red_white_dye" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_PINK_WOOL, 1)], result: (ITEM_PINK_HARNESS, 1), name: "pink_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_PINK_DYE, 1)], result: (ITEM_PINK_STAINED_GLASS, 8), name: "pink_stained_glass" },
    Recipe { needs: &[(ITEM_PINK_STAINED_GLASS, 6)], result: (ITEM_PINK_STAINED_GLASS_PANE, 16), name: "pink_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_PINK_DYE, 1)], result: (ITEM_PINK_STAINED_GLASS_PANE, 8), name: "pink_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_PINK_TERRACOTTA, 8), name: "pink_terracotta" },
    Recipe { needs: &[(ITEM_PINK_WOOL, 3)], result: (ITEM_PINK_WOOL_SLAB, 6), name: "pink_wool_slab" },
    Recipe { needs: &[(ITEM_PINK_WOOL, 6)], result: (ITEM_PINK_WOOL_STAIRS, 4), name: "pink_wool_stairs" },
    Recipe { needs: &[(COBBLESTONE, 4), (ITEM_IRON_INGOT, 1), (ITEM_REDSTONE, 1), (TAG_PLANKS, 3)], result: (ITEM_PISTON, 1), name: "piston" },
    Recipe { needs: &[(ITEM_ANDESITE, 4)], result: (ITEM_POLISHED_ANDESITE, 4), name: "polished_andesite" },
    Recipe { needs: &[(ITEM_POLISHED_ANDESITE, 3)], result: (ITEM_POLISHED_ANDESITE_SLAB, 6), name: "polished_andesite_slab" },
    Recipe { needs: &[(ITEM_POLISHED_ANDESITE, 6)], result: (ITEM_POLISHED_ANDESITE_STAIRS, 4), name: "polished_andesite_stairs" },
    Recipe { needs: &[(ITEM_BASALT, 4)], result: (ITEM_POLISHED_BASALT, 4), name: "polished_basalt" },
    Recipe { needs: &[(ITEM_BLACKSTONE, 4)], result: (ITEM_POLISHED_BLACKSTONE, 4), name: "polished_blackstone" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE_BRICKS, 3)], result: (ITEM_POLISHED_BLACKSTONE_BRICK_SLAB, 6), name: "polished_blackstone_brick_slab" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE_BRICKS, 6)], result: (ITEM_POLISHED_BLACKSTONE_BRICK_STAIRS, 4), name: "polished_blackstone_brick_stairs" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE_BRICKS, 6)], result: (ITEM_POLISHED_BLACKSTONE_BRICK_WALL, 6), name: "polished_blackstone_brick_wall" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 4)], result: (ITEM_POLISHED_BLACKSTONE_BRICKS, 4), name: "polished_blackstone_bricks" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 1)], result: (ITEM_POLISHED_BLACKSTONE_BUTTON, 1), name: "polished_blackstone_button" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 2)], result: (ITEM_POLISHED_BLACKSTONE_PRESSURE_PLATE, 1), name: "polished_blackstone_pressure_plate" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 3)], result: (ITEM_POLISHED_BLACKSTONE_SLAB, 6), name: "polished_blackstone_slab" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 6)], result: (ITEM_POLISHED_BLACKSTONE_STAIRS, 4), name: "polished_blackstone_stairs" },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 6)], result: (ITEM_POLISHED_BLACKSTONE_WALL, 6), name: "polished_blackstone_wall" },
    Recipe { needs: &[(ITEM_CINNABAR, 4)], result: (ITEM_POLISHED_CINNABAR, 4), name: "polished_cinnabar" },
    Recipe { needs: &[(ITEM_POLISHED_CINNABAR, 3)], result: (ITEM_POLISHED_CINNABAR_SLAB, 6), name: "polished_cinnabar_slab" },
    Recipe { needs: &[(ITEM_POLISHED_CINNABAR, 6)], result: (ITEM_POLISHED_CINNABAR_STAIRS, 4), name: "polished_cinnabar_stairs" },
    Recipe { needs: &[(ITEM_POLISHED_CINNABAR, 6)], result: (ITEM_POLISHED_CINNABAR_WALL, 6), name: "polished_cinnabar_wall" },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 4)], result: (ITEM_POLISHED_DEEPSLATE, 4), name: "polished_deepslate" },
    Recipe { needs: &[(ITEM_POLISHED_DEEPSLATE, 3)], result: (ITEM_POLISHED_DEEPSLATE_SLAB, 6), name: "polished_deepslate_slab" },
    Recipe { needs: &[(ITEM_POLISHED_DEEPSLATE, 6)], result: (ITEM_POLISHED_DEEPSLATE_STAIRS, 4), name: "polished_deepslate_stairs" },
    Recipe { needs: &[(ITEM_POLISHED_DEEPSLATE, 6)], result: (ITEM_POLISHED_DEEPSLATE_WALL, 6), name: "polished_deepslate_wall" },
    Recipe { needs: &[(ITEM_DIORITE, 4)], result: (ITEM_POLISHED_DIORITE, 4), name: "polished_diorite" },
    Recipe { needs: &[(ITEM_POLISHED_DIORITE, 3)], result: (ITEM_POLISHED_DIORITE_SLAB, 6), name: "polished_diorite_slab" },
    Recipe { needs: &[(ITEM_POLISHED_DIORITE, 6)], result: (ITEM_POLISHED_DIORITE_STAIRS, 4), name: "polished_diorite_stairs" },
    Recipe { needs: &[(ITEM_GRANITE, 4)], result: (ITEM_POLISHED_GRANITE, 4), name: "polished_granite" },
    Recipe { needs: &[(ITEM_POLISHED_GRANITE, 3)], result: (ITEM_POLISHED_GRANITE_SLAB, 6), name: "polished_granite_slab" },
    Recipe { needs: &[(ITEM_POLISHED_GRANITE, 6)], result: (ITEM_POLISHED_GRANITE_STAIRS, 4), name: "polished_granite_stairs" },
    Recipe { needs: &[(ITEM_SULFUR, 4)], result: (ITEM_POLISHED_SULFUR, 4), name: "polished_sulfur" },
    Recipe { needs: &[(ITEM_POLISHED_SULFUR, 3)], result: (ITEM_POLISHED_SULFUR_SLAB, 6), name: "polished_sulfur_slab" },
    Recipe { needs: &[(ITEM_POLISHED_SULFUR, 6)], result: (ITEM_POLISHED_SULFUR_STAIRS, 4), name: "polished_sulfur_stairs" },
    Recipe { needs: &[(ITEM_POLISHED_SULFUR, 6)], result: (ITEM_POLISHED_SULFUR_WALL, 6), name: "polished_sulfur_wall" },
    Recipe { needs: &[(ITEM_TUFF, 4)], result: (ITEM_POLISHED_TUFF, 4), name: "polished_tuff" },
    Recipe { needs: &[(ITEM_POLISHED_TUFF, 3)], result: (ITEM_POLISHED_TUFF_SLAB, 6), name: "polished_tuff_slab" },
    Recipe { needs: &[(ITEM_POLISHED_TUFF, 6)], result: (ITEM_POLISHED_TUFF_STAIRS, 4), name: "polished_tuff_stairs" },
    Recipe { needs: &[(ITEM_POLISHED_TUFF, 6)], result: (ITEM_POLISHED_TUFF_WALL, 6), name: "polished_tuff_wall" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 5)], result: (ITEM_POPLAR_BOAT, 1), name: "poplar_boat" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 1)], result: (ITEM_POPLAR_BUTTON, 1), name: "poplar_button" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_POPLAR_BOAT, 1)], result: (ITEM_POPLAR_CHEST_BOAT, 1), name: "poplar_chest_boat" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 6)], result: (ITEM_POPLAR_DOOR, 3), name: "poplar_door" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 4), (STICK, 2)], result: (ITEM_POPLAR_FENCE, 3), name: "poplar_fence" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 2), (STICK, 4)], result: (ITEM_POPLAR_FENCE_GATE, 1), name: "poplar_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_POPLAR_LOG, 6)], result: (ITEM_POPLAR_HANGING_SIGN, 6), name: "poplar_hanging_sign" },
    Recipe { needs: &[(TAG_POPLAR_LOGS, 1)], result: (ITEM_POPLAR_PLANKS, 4), name: "poplar_planks" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 2)], result: (ITEM_POPLAR_PRESSURE_PLATE, 1), name: "poplar_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_POPLAR_LOG, 6)], result: (ITEM_POPLAR_SHELF, 6), name: "poplar_shelf" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 6), (STICK, 1)], result: (ITEM_POPLAR_SIGN, 3), name: "poplar_sign" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 3)], result: (ITEM_POPLAR_SLAB, 6), name: "poplar_slab" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 6)], result: (ITEM_POPLAR_STAIRS, 4), name: "poplar_stairs" },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 6)], result: (ITEM_POPLAR_TRAPDOOR, 2), name: "poplar_trapdoor" },
    Recipe { needs: &[(ITEM_POPLAR_LOG, 4)], result: (ITEM_POPLAR_WOOD, 3), name: "poplar_wood" },
    Recipe { needs: &[(ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1)], result: (ITEM_POTENT_SULFUR, 1), name: "potent_sulfur" },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 6), (ITEM_REDSTONE, 1), (STICK, 1)], result: (ITEM_POWERED_RAIL, 6), name: "powered_rail" },
    Recipe { needs: &[(ITEM_PRISMARINE_SHARD, 4)], result: (ITEM_PRISMARINE, 1), name: "prismarine" },
    Recipe { needs: &[(ITEM_PRISMARINE_BRICKS, 3)], result: (ITEM_PRISMARINE_BRICK_SLAB, 6), name: "prismarine_brick_slab" },
    Recipe { needs: &[(ITEM_PRISMARINE_BRICKS, 6)], result: (ITEM_PRISMARINE_BRICK_STAIRS, 4), name: "prismarine_brick_stairs" },
    Recipe { needs: &[(ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1)], result: (ITEM_PRISMARINE_BRICKS, 1), name: "prismarine_bricks" },
    Recipe { needs: &[(ITEM_PRISMARINE, 3)], result: (ITEM_PRISMARINE_SLAB, 6), name: "prismarine_slab" },
    Recipe { needs: &[(ITEM_PRISMARINE, 6)], result: (ITEM_PRISMARINE_STAIRS, 4), name: "prismarine_stairs" },
    Recipe { needs: &[(ITEM_PRISMARINE, 6)], result: (ITEM_PRISMARINE_WALL, 6), name: "prismarine_wall" },
    Recipe { needs: &[(ITEM_PUMPKIN, 1), (ITEM_SUGAR, 1), (TAG_EGGS, 1)], result: (ITEM_PUMPKIN_PIE, 1), name: "pumpkin_pie" },
    Recipe { needs: &[(ITEM_PUMPKIN, 1)], result: (ITEM_PUMPKIN_SEEDS, 4), name: "pumpkin_seeds" },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 6), (STICK, 1)], result: (ITEM_PURPLE_BANNER, 1), name: "purple_banner" },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_PURPLE_BED, 1), name: "purple_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_PURPLE_DYE, 1)], result: (ITEM_PURPLE_CANDLE, 1), name: "purple_candle" },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 2)], result: (ITEM_PURPLE_CARPET, 3), name: "purple_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_PURPLE_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_PURPLE_CONCRETE_POWDER, 8), name: "purple_concrete_powder" },
    Recipe { needs: &[(ITEM_PURPLE_CONCRETE, 3)], result: (ITEM_PURPLE_CONCRETE_SLAB, 6), name: "purple_concrete_slab" },
    Recipe { needs: &[(ITEM_PURPLE_CONCRETE, 6)], result: (ITEM_PURPLE_CONCRETE_STAIRS, 4), name: "purple_concrete_stairs" },
    Recipe { needs: &[(ITEM_PURPLE_WOOL_SLAB, 3)], result: (ITEM_PURPLE_CUSHION, 1), name: "purple_cushion" },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_RED_DYE, 1)], result: (ITEM_PURPLE_DYE, 2), name: "purple_dye" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_PURPLE_WOOL, 1)], result: (ITEM_PURPLE_HARNESS, 1), name: "purple_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_PURPLE_DYE, 1)], result: (ITEM_PURPLE_STAINED_GLASS, 8), name: "purple_stained_glass" },
    Recipe { needs: &[(ITEM_PURPLE_STAINED_GLASS, 6)], result: (ITEM_PURPLE_STAINED_GLASS_PANE, 16), name: "purple_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_PURPLE_DYE, 1)], result: (ITEM_PURPLE_STAINED_GLASS_PANE, 8), name: "purple_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_PURPLE_TERRACOTTA, 8), name: "purple_terracotta" },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 3)], result: (ITEM_PURPLE_WOOL_SLAB, 6), name: "purple_wool_slab" },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 6)], result: (ITEM_PURPLE_WOOL_STAIRS, 4), name: "purple_wool_stairs" },
    Recipe { needs: &[(ITEM_POPPED_CHORUS_FRUIT, 4)], result: (ITEM_PURPUR_BLOCK, 4), name: "purpur_block" },
    Recipe { needs: &[(ITEM_PURPUR_SLAB, 2)], result: (ITEM_PURPUR_PILLAR, 1), name: "purpur_pillar" },
    Recipe { needs: &[(TAG_ALT_114, 3)], result: (ITEM_PURPUR_SLAB, 6), name: "purpur_slab" },
    Recipe { needs: &[(TAG_ALT_114, 6)], result: (ITEM_PURPUR_STAIRS, 4), name: "purpur_stairs" },
    Recipe { needs: &[(ITEM_QUARTZ, 4)], result: (ITEM_QUARTZ_BLOCK, 1), name: "quartz_block" },
    Recipe { needs: &[(ITEM_QUARTZ_BLOCK, 4)], result: (ITEM_QUARTZ_BRICKS, 4), name: "quartz_bricks" },
    Recipe { needs: &[(ITEM_QUARTZ_BLOCK, 2)], result: (ITEM_QUARTZ_PILLAR, 2), name: "quartz_pillar" },
    Recipe { needs: &[(TAG_ALT_115, 3)], result: (ITEM_QUARTZ_SLAB, 6), name: "quartz_slab" },
    Recipe { needs: &[(TAG_ALT_115, 6)], result: (ITEM_QUARTZ_STAIRS, 4), name: "quartz_stairs" },
    Recipe { needs: &[(ITEM_BAKED_POTATO, 1), (ITEM_BOWL, 1), (ITEM_BROWN_MUSHROOM, 1), (ITEM_CARROT, 1), (ITEM_COOKED_RABBIT, 1)], result: (ITEM_RABBIT_STEW, 1), name: "rabbit_stew_from_brown_mushroom" },
    Recipe { needs: &[(ITEM_BAKED_POTATO, 1), (ITEM_BOWL, 1), (ITEM_CARROT, 1), (ITEM_COOKED_RABBIT, 1), (ITEM_RED_MUSHROOM, 1)], result: (ITEM_RABBIT_STEW, 1), name: "rabbit_stew_from_red_mushroom" },
    Recipe { needs: &[(ITEM_BAKED_POTATO, 1), (ITEM_BOWL, 1), (ITEM_CARROT, 1), (ITEM_COOKED_RABBIT, 1), (ITEM_SHELF_MUSHROOM, 1)], result: (ITEM_RABBIT_STEW, 1), name: "rabbit_stew_from_shelf_mushroom" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (STICK, 1)], result: (ITEM_RAIL, 16), name: "rail" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_RAISER_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_TERRACOTTA, 1)], result: (ITEM_RAISER_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "raiser_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_RAW_COPPER_BLOCK, 1)], result: (ITEM_RAW_COPPER, 9), name: "raw_copper" },
    Recipe { needs: &[(ITEM_RAW_COPPER, 9)], result: (ITEM_RAW_COPPER_BLOCK, 1), name: "raw_copper_block" },
    Recipe { needs: &[(ITEM_RAW_GOLD_BLOCK, 1)], result: (ITEM_RAW_GOLD, 9), name: "raw_gold" },
    Recipe { needs: &[(ITEM_RAW_GOLD, 9)], result: (ITEM_RAW_GOLD_BLOCK, 1), name: "raw_gold_block" },
    Recipe { needs: &[(ITEM_RAW_IRON_BLOCK, 1)], result: (ITEM_RAW_IRON, 9), name: "raw_iron" },
    Recipe { needs: &[(ITEM_RAW_IRON, 9)], result: (ITEM_RAW_IRON_BLOCK, 1), name: "raw_iron_block" },
    Recipe { needs: &[(ITEM_COMPASS, 1), (ITEM_ECHO_SHARD, 8)], result: (ITEM_RECOVERY_COMPASS, 1), name: "recovery_compass" },
    Recipe { needs: &[(ITEM_RED_WOOL, 6), (STICK, 1)], result: (ITEM_RED_BANNER, 1), name: "red_banner" },
    Recipe { needs: &[(ITEM_RED_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_RED_BED, 1), name: "red_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_RED_DYE, 1)], result: (ITEM_RED_CANDLE, 1), name: "red_candle" },
    Recipe { needs: &[(ITEM_RED_WOOL, 2)], result: (ITEM_RED_CARPET, 3), name: "red_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_RED_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_RED_CONCRETE_POWDER, 8), name: "red_concrete_powder" },
    Recipe { needs: &[(ITEM_RED_CONCRETE, 3)], result: (ITEM_RED_CONCRETE_SLAB, 6), name: "red_concrete_slab" },
    Recipe { needs: &[(ITEM_RED_CONCRETE, 6)], result: (ITEM_RED_CONCRETE_STAIRS, 4), name: "red_concrete_stairs" },
    Recipe { needs: &[(ITEM_RED_WOOL_SLAB, 3)], result: (ITEM_RED_CUSHION, 1), name: "red_cushion" },
    Recipe { needs: &[(ITEM_BEETROOT, 1)], result: (ITEM_RED_DYE, 1), name: "red_dye_from_beetroot" },
    Recipe { needs: &[(ITEM_POPPY, 1)], result: (ITEM_RED_DYE, 1), name: "red_dye_from_poppy" },
    Recipe { needs: &[(ITEM_ROSE_BUSH, 1)], result: (ITEM_RED_DYE, 2), name: "red_dye_from_rose_bush" },
    Recipe { needs: &[(ITEM_RED_TULIP, 1)], result: (ITEM_RED_DYE, 1), name: "red_dye_from_tulip" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_RED_WOOL, 1)], result: (ITEM_RED_HARNESS, 1), name: "red_harness" },
    Recipe { needs: &[(ITEM_RED_NETHER_BRICKS, 3)], result: (ITEM_RED_NETHER_BRICK_SLAB, 6), name: "red_nether_brick_slab" },
    Recipe { needs: &[(ITEM_RED_NETHER_BRICKS, 6)], result: (ITEM_RED_NETHER_BRICK_STAIRS, 4), name: "red_nether_brick_stairs" },
    Recipe { needs: &[(ITEM_RED_NETHER_BRICKS, 6)], result: (ITEM_RED_NETHER_BRICK_WALL, 6), name: "red_nether_brick_wall" },
    Recipe { needs: &[(ITEM_NETHER_BRICK, 2), (ITEM_NETHER_WART, 2)], result: (ITEM_RED_NETHER_BRICKS, 1), name: "red_nether_bricks" },
    Recipe { needs: &[(ITEM_RED_SAND, 4)], result: (ITEM_RED_SANDSTONE, 1), name: "red_sandstone" },
    Recipe { needs: &[(TAG_ALT_116, 3)], result: (ITEM_RED_SANDSTONE_SLAB, 6), name: "red_sandstone_slab" },
    Recipe { needs: &[(TAG_ALT_117, 6)], result: (ITEM_RED_SANDSTONE_STAIRS, 4), name: "red_sandstone_stairs" },
    Recipe { needs: &[(ITEM_RED_SANDSTONE, 6)], result: (ITEM_RED_SANDSTONE_WALL, 6), name: "red_sandstone_wall" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_RED_DYE, 1)], result: (ITEM_RED_STAINED_GLASS, 8), name: "red_stained_glass" },
    Recipe { needs: &[(ITEM_RED_STAINED_GLASS, 6)], result: (ITEM_RED_STAINED_GLASS_PANE, 16), name: "red_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_RED_DYE, 1)], result: (ITEM_RED_STAINED_GLASS_PANE, 8), name: "red_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_RED_TERRACOTTA, 8), name: "red_terracotta" },
    Recipe { needs: &[(ITEM_RED_WOOL, 3)], result: (ITEM_RED_WOOL_SLAB, 6), name: "red_wool_slab" },
    Recipe { needs: &[(ITEM_RED_WOOL, 6)], result: (ITEM_RED_WOOL_STAIRS, 4), name: "red_wool_stairs" },
    Recipe { needs: &[(ITEM_REDSTONE_BLOCK, 1)], result: (ITEM_REDSTONE, 9), name: "redstone" },
    Recipe { needs: &[(ITEM_REDSTONE, 9)], result: (ITEM_REDSTONE_BLOCK, 1), name: "redstone_block" },
    Recipe { needs: &[(ITEM_GLOWSTONE, 1), (ITEM_REDSTONE, 4)], result: (ITEM_REDSTONE_LAMP, 1), name: "redstone_lamp" },
    Recipe { needs: &[(ITEM_REDSTONE, 1), (STICK, 1)], result: (ITEM_REDSTONE_TORCH, 1), name: "redstone_torch" },
    Recipe { needs: &[(ITEM_REDSTONE, 1), (ITEM_REDSTONE_TORCH, 2), (ITEM_STONE, 3)], result: (ITEM_REPEATER, 1), name: "repeater" },
    Recipe { needs: &[(ITEM_RESIN_CLUMP, 9)], result: (ITEM_RESIN_BLOCK, 1), name: "resin_block" },
    Recipe { needs: &[(ITEM_RESIN_BRICKS, 3)], result: (ITEM_RESIN_BRICK_SLAB, 6), name: "resin_brick_slab" },
    Recipe { needs: &[(ITEM_RESIN_BRICKS, 6)], result: (ITEM_RESIN_BRICK_STAIRS, 4), name: "resin_brick_stairs" },
    Recipe { needs: &[(ITEM_RESIN_BRICKS, 6)], result: (ITEM_RESIN_BRICK_WALL, 6), name: "resin_brick_wall" },
    Recipe { needs: &[(ITEM_RESIN_BRICK, 4)], result: (ITEM_RESIN_BRICKS, 1), name: "resin_bricks" },
    Recipe { needs: &[(ITEM_RESIN_BLOCK, 1)], result: (ITEM_RESIN_CLUMP, 9), name: "resin_clump" },
    Recipe { needs: &[(ITEM_CRYING_OBSIDIAN, 6), (ITEM_GLOWSTONE, 3)], result: (ITEM_RESPAWN_ANCHOR, 1), name: "respawn_anchor" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_NETHERRACK, 1), (ITEM_RIB_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_RIB_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "rib_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (ITEM_LEATHER, 3)], result: (ITEM_SADDLE, 1), name: "saddle" },
    Recipe { needs: &[(ITEM_SAND, 4)], result: (ITEM_SANDSTONE, 1), name: "sandstone" },
    Recipe { needs: &[(TAG_ALT_118, 3)], result: (ITEM_SANDSTONE_SLAB, 6), name: "sandstone_slab" },
    Recipe { needs: &[(TAG_ALT_119, 6)], result: (ITEM_SANDSTONE_STAIRS, 4), name: "sandstone_stairs" },
    Recipe { needs: &[(ITEM_SANDSTONE, 6)], result: (ITEM_SANDSTONE_WALL, 6), name: "sandstone_wall" },
    Recipe { needs: &[(ITEM_BAMBOO, 6), (ITEM_STRING, 1)], result: (ITEM_SCAFFOLDING, 6), name: "scaffolding" },
    Recipe { needs: &[(ITEM_PRISMARINE_CRYSTALS, 5), (ITEM_PRISMARINE_SHARD, 4)], result: (ITEM_SEA_LANTERN, 1), name: "sea_lantern" },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_DIAMOND, 7), (ITEM_SENTRY_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_SENTRY_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "sentry_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_SHAPER_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_TERRACOTTA, 1)], result: (ITEM_SHAPER_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "shaper_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 2)], result: (ITEM_SHEARS, 1), name: "shears" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (TAG_PLANKS, 6)], result: (ITEM_SHIELD, 1), name: "shield" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_SHULKER_SHELL, 2)], result: (ITEM_SHULKER_BOX, 1), name: "shulker_box" },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 1), (ITEM_DIAMOND, 7), (ITEM_SILENCE_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_SILENCE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "silence_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_PAPER, 1), (ITEM_WITHER_SKELETON_SKULL, 1)], result: (ITEM_SKULL_BANNER_PATTERN, 1), name: "skull_banner_pattern" },
    Recipe { needs: &[(ITEM_SLIME_BLOCK, 1)], result: (ITEM_SLIME_BALL, 9), name: "slime_ball" },
    Recipe { needs: &[(ITEM_SLIME_BALL, 9)], result: (ITEM_SLIME_BLOCK, 1), name: "slime_block" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 2), (TAG_PLANKS, 4)], result: (ITEM_SMITHING_TABLE, 1), name: "smithing_table" },
    Recipe { needs: &[(ITEM_FURNACE, 1), (TAG_LOGS, 4)], result: (ITEM_SMOKER, 1), name: "smoker" },
    Recipe { needs: &[(ITEM_SMOOTH_QUARTZ, 3)], result: (ITEM_SMOOTH_QUARTZ_SLAB, 6), name: "smooth_quartz_slab" },
    Recipe { needs: &[(ITEM_SMOOTH_QUARTZ, 6)], result: (ITEM_SMOOTH_QUARTZ_STAIRS, 4), name: "smooth_quartz_stairs" },
    Recipe { needs: &[(ITEM_SMOOTH_RED_SANDSTONE, 3)], result: (ITEM_SMOOTH_RED_SANDSTONE_SLAB, 6), name: "smooth_red_sandstone_slab" },
    Recipe { needs: &[(ITEM_SMOOTH_RED_SANDSTONE, 6)], result: (ITEM_SMOOTH_RED_SANDSTONE_STAIRS, 4), name: "smooth_red_sandstone_stairs" },
    Recipe { needs: &[(ITEM_SMOOTH_SANDSTONE, 3)], result: (ITEM_SMOOTH_SANDSTONE_SLAB, 6), name: "smooth_sandstone_slab" },
    Recipe { needs: &[(ITEM_SMOOTH_SANDSTONE, 6)], result: (ITEM_SMOOTH_SANDSTONE_STAIRS, 4), name: "smooth_sandstone_stairs" },
    Recipe { needs: &[(ITEM_SMOOTH_STONE, 3)], result: (ITEM_SMOOTH_STONE_SLAB, 6), name: "smooth_stone_slab" },
    Recipe { needs: &[(ITEM_BLACKSTONE, 1), (ITEM_DIAMOND, 7), (ITEM_SNOUT_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_SNOUT_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "snout_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_SNOW_BLOCK, 3)], result: (ITEM_SNOW, 6), name: "snow" },
    Recipe { needs: &[(ITEM_SNOWBALL, 4)], result: (ITEM_SNOW_BLOCK, 1), name: "snow_block" },
    Recipe { needs: &[(STICK, 3), (TAG_LOGS, 3), (TAG_SOUL_FIRE_BASE_BLOCKS, 1)], result: (ITEM_SOUL_CAMPFIRE, 1), name: "soul_campfire" },
    Recipe { needs: &[(ITEM_IRON_NUGGET, 8), (ITEM_SOUL_TORCH, 1)], result: (ITEM_SOUL_LANTERN, 1), name: "soul_lantern" },
    Recipe { needs: &[(STICK, 1), (TAG_COALS, 1), (TAG_SOUL_FIRE_BASE_BLOCKS, 1)], result: (ITEM_SOUL_TORCH, 4), name: "soul_torch" },
    Recipe { needs: &[(ITEM_ARROW, 1), (ITEM_GLOWSTONE_DUST, 4)], result: (ITEM_SPECTRAL_ARROW, 2), name: "spectral_arrow" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_PURPUR_BLOCK, 1), (ITEM_SPIRE_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_SPIRE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "spire_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 5)], result: (ITEM_SPRUCE_BOAT, 1), name: "spruce_boat" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 1)], result: (ITEM_SPRUCE_BUTTON, 1), name: "spruce_button" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_SPRUCE_BOAT, 1)], result: (ITEM_SPRUCE_CHEST_BOAT, 1), name: "spruce_chest_boat" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6)], result: (ITEM_SPRUCE_DOOR, 3), name: "spruce_door" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 4), (STICK, 2)], result: (ITEM_SPRUCE_FENCE, 3), name: "spruce_fence" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 2), (STICK, 4)], result: (ITEM_SPRUCE_FENCE_GATE, 1), name: "spruce_fence_gate" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_SPRUCE_LOG, 6)], result: (ITEM_SPRUCE_HANGING_SIGN, 6), name: "spruce_hanging_sign" },
    Recipe { needs: &[(TAG_SPRUCE_LOGS, 1)], result: (ITEM_SPRUCE_PLANKS, 4), name: "spruce_planks" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 2)], result: (ITEM_SPRUCE_PRESSURE_PLATE, 1), name: "spruce_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_SPRUCE_LOG, 6)], result: (ITEM_SPRUCE_SHELF, 6), name: "spruce_shelf" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6), (STICK, 1)], result: (ITEM_SPRUCE_SIGN, 3), name: "spruce_sign" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 3)], result: (ITEM_SPRUCE_SLAB, 6), name: "spruce_slab" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6)], result: (ITEM_SPRUCE_STAIRS, 4), name: "spruce_stairs" },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6)], result: (ITEM_SPRUCE_TRAPDOOR, 2), name: "spruce_trapdoor" },
    Recipe { needs: &[(ITEM_SPRUCE_LOG, 4)], result: (ITEM_SPRUCE_WOOD, 3), name: "spruce_wood" },
    Recipe { needs: &[(ITEM_AMETHYST_SHARD, 1), (ITEM_COPPER_INGOT, 2)], result: (ITEM_SPYGLASS, 1), name: "spyglass" },
    Recipe { needs: &[(TAG_PLANKS, 2)], result: (STICK, 4), name: "stick" },
    Recipe { needs: &[(ITEM_BAMBOO, 2)], result: (STICK, 1), name: "stick_from_bamboo_item" },
    Recipe { needs: &[(ITEM_PISTON, 1), (ITEM_SLIME_BALL, 1)], result: (ITEM_STICKY_PISTON, 1), name: "sticky_piston" },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 3)], result: (ITEM_STONE_AXE, 1), name: "stone_axe" },
    Recipe { needs: &[(ITEM_STONE_BRICKS, 3)], result: (ITEM_STONE_BRICK_SLAB, 6), name: "stone_brick_slab" },
    Recipe { needs: &[(ITEM_STONE_BRICKS, 6)], result: (ITEM_STONE_BRICK_STAIRS, 4), name: "stone_brick_stairs" },
    Recipe { needs: &[(ITEM_STONE_BRICKS, 6)], result: (ITEM_STONE_BRICK_WALL, 6), name: "stone_brick_wall" },
    Recipe { needs: &[(ITEM_STONE, 4)], result: (ITEM_STONE_BRICKS, 4), name: "stone_bricks" },
    Recipe { needs: &[(ITEM_STONE, 1)], result: (ITEM_STONE_BUTTON, 1), name: "stone_button" },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 2)], result: (ITEM_STONE_HOE, 1), name: "stone_hoe" },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 3)], result: (STONE_PICKAXE, 1), name: "stone_pickaxe" },
    Recipe { needs: &[(ITEM_STONE, 2)], result: (ITEM_STONE_PRESSURE_PLATE, 1), name: "stone_pressure_plate" },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 1)], result: (ITEM_STONE_SHOVEL, 1), name: "stone_shovel" },
    Recipe { needs: &[(ITEM_STONE, 3)], result: (ITEM_STONE_SLAB, 6), name: "stone_slab" },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 1)], result: (ITEM_STONE_SPEAR, 1), name: "stone_spear" },
    Recipe { needs: &[(ITEM_STONE, 6)], result: (ITEM_STONE_STAIRS, 4), name: "stone_stairs" },
    Recipe { needs: &[(STICK, 1), (TAG_STONE_CRAFTING_MATERIALS, 2)], result: (ITEM_STONE_SWORD, 1), name: "stone_sword" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (ITEM_STONE, 3)], result: (ITEM_STONECUTTER, 1), name: "stonecutter" },
    Recipe { needs: &[(ITEM_HAY_BLOCK, 3)], result: (ITEM_STRAW_BED, 4), name: "straw_bed" },
    Recipe { needs: &[(ITEM_STRIPPED_ACACIA_LOG, 4)], result: (ITEM_STRIPPED_ACACIA_WOOD, 3), name: "stripped_acacia_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_BIRCH_LOG, 4)], result: (ITEM_STRIPPED_BIRCH_WOOD, 3), name: "stripped_birch_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_CHERRY_LOG, 4)], result: (ITEM_STRIPPED_CHERRY_WOOD, 3), name: "stripped_cherry_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_CRIMSON_STEM, 4)], result: (ITEM_STRIPPED_CRIMSON_HYPHAE, 3), name: "stripped_crimson_hyphae" },
    Recipe { needs: &[(ITEM_STRIPPED_DARK_OAK_LOG, 4)], result: (ITEM_STRIPPED_DARK_OAK_WOOD, 3), name: "stripped_dark_oak_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_JUNGLE_LOG, 4)], result: (ITEM_STRIPPED_JUNGLE_WOOD, 3), name: "stripped_jungle_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_MANGROVE_LOG, 4)], result: (ITEM_STRIPPED_MANGROVE_WOOD, 3), name: "stripped_mangrove_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_OAK_LOG, 4)], result: (ITEM_STRIPPED_OAK_WOOD, 3), name: "stripped_oak_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_PALE_OAK_LOG, 4)], result: (ITEM_STRIPPED_PALE_OAK_WOOD, 3), name: "stripped_pale_oak_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_POPLAR_LOG, 4)], result: (ITEM_STRIPPED_POPLAR_WOOD, 3), name: "stripped_poplar_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_SPRUCE_LOG, 4)], result: (ITEM_STRIPPED_SPRUCE_WOOD, 3), name: "stripped_spruce_wood" },
    Recipe { needs: &[(ITEM_STRIPPED_WARPED_STEM, 4)], result: (ITEM_STRIPPED_WARPED_HYPHAE, 3), name: "stripped_warped_hyphae" },
    Recipe { needs: &[(ITEM_HONEY_BOTTLE, 1)], result: (ITEM_SUGAR, 3), name: "sugar_from_honey_bottle" },
    Recipe { needs: &[(ITEM_SUGAR_CANE, 1)], result: (ITEM_SUGAR, 1), name: "sugar_from_sugar_cane" },
    Recipe { needs: &[(ITEM_SULFUR_BRICKS, 3)], result: (ITEM_SULFUR_BRICK_SLAB, 6), name: "sulfur_brick_slab" },
    Recipe { needs: &[(ITEM_SULFUR_BRICKS, 6)], result: (ITEM_SULFUR_BRICK_STAIRS, 4), name: "sulfur_brick_stairs" },
    Recipe { needs: &[(ITEM_SULFUR_BRICKS, 6)], result: (ITEM_SULFUR_BRICK_WALL, 6), name: "sulfur_brick_wall" },
    Recipe { needs: &[(ITEM_POLISHED_SULFUR, 4)], result: (ITEM_SULFUR_BRICKS, 4), name: "sulfur_bricks" },
    Recipe { needs: &[(ITEM_SULFUR_SPIKE, 4)], result: (ITEM_SULFUR, 1), name: "sulfur_from_sulfur_spikes" },
    Recipe { needs: &[(ITEM_SULFUR, 3)], result: (ITEM_SULFUR_SLAB, 6), name: "sulfur_slab" },
    Recipe { needs: &[(ITEM_SULFUR, 6)], result: (ITEM_SULFUR_STAIRS, 4), name: "sulfur_stairs" },
    Recipe { needs: &[(ITEM_SULFUR, 6)], result: (ITEM_SULFUR_WALL, 6), name: "sulfur_wall" },
    Recipe { needs: &[(ITEM_HAY_BLOCK, 1), (ITEM_REDSTONE, 4)], result: (ITEM_TARGET, 1), name: "target" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_PRISMARINE, 1), (ITEM_TIDE_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_TIDE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "tide_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_AMETHYST_SHARD, 4), (ITEM_GLASS, 1)], result: (ITEM_TINTED_GLASS, 2), name: "tinted_glass" },
    Recipe { needs: &[(ITEM_GUNPOWDER, 5), (TAG_SMELTS_TO_GLASS, 4)], result: (ITEM_TNT, 1), name: "tnt" },
    Recipe { needs: &[(ITEM_MINECART, 1), (ITEM_TNT, 1)], result: (ITEM_TNT_MINECART, 1), name: "tnt_minecart" },
    Recipe { needs: &[(STICK, 1), (TAG_COALS, 1)], result: (TORCH, 4), name: "torch" },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_TRIPWIRE_HOOK, 1)], result: (ITEM_TRAPPED_CHEST, 1), name: "trapped_chest" },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (STICK, 1), (TAG_PLANKS, 1)], result: (ITEM_TRIPWIRE_HOOK, 2), name: "tripwire_hook" },
    Recipe { needs: &[(ITEM_TUFF_BRICKS, 3)], result: (ITEM_TUFF_BRICK_SLAB, 6), name: "tuff_brick_slab" },
    Recipe { needs: &[(ITEM_TUFF_BRICKS, 6)], result: (ITEM_TUFF_BRICK_STAIRS, 4), name: "tuff_brick_stairs" },
    Recipe { needs: &[(ITEM_TUFF_BRICKS, 6)], result: (ITEM_TUFF_BRICK_WALL, 6), name: "tuff_brick_wall" },
    Recipe { needs: &[(ITEM_POLISHED_TUFF, 4)], result: (ITEM_TUFF_BRICKS, 4), name: "tuff_bricks" },
    Recipe { needs: &[(ITEM_TUFF, 3)], result: (ITEM_TUFF_SLAB, 6), name: "tuff_slab" },
    Recipe { needs: &[(ITEM_TUFF, 6)], result: (ITEM_TUFF_STAIRS, 4), name: "tuff_stairs" },
    Recipe { needs: &[(ITEM_TUFF, 6)], result: (ITEM_TUFF_WALL, 6), name: "tuff_wall" },
    Recipe { needs: &[(ITEM_TURTLE_SCUTE, 5)], result: (ITEM_TURTLE_HELMET, 1), name: "turtle_helmet" },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_DIAMOND, 7), (ITEM_VEX_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_VEX_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "vex_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 1), (ITEM_DIAMOND, 7), (ITEM_WARD_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_WARD_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "ward_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 1)], result: (ITEM_WARPED_BUTTON, 1), name: "warped_button" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_DOOR, 3), name: "warped_door" },
    Recipe { needs: &[(STICK, 2), (ITEM_WARPED_PLANKS, 4)], result: (ITEM_WARPED_FENCE, 3), name: "warped_fence" },
    Recipe { needs: &[(STICK, 4), (ITEM_WARPED_PLANKS, 2)], result: (ITEM_WARPED_FENCE_GATE, 1), name: "warped_fence_gate" },
    Recipe { needs: &[(ITEM_FISHING_ROD, 1), (ITEM_WARPED_FUNGUS, 1)], result: (ITEM_WARPED_FUNGUS_ON_A_STICK, 1), name: "warped_fungus_on_a_stick" },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_WARPED_STEM, 6)], result: (ITEM_WARPED_HANGING_SIGN, 6), name: "warped_hanging_sign" },
    Recipe { needs: &[(ITEM_WARPED_STEM, 4)], result: (ITEM_WARPED_HYPHAE, 3), name: "warped_hyphae" },
    Recipe { needs: &[(TAG_WARPED_STEMS, 1)], result: (ITEM_WARPED_PLANKS, 4), name: "warped_planks" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 2)], result: (ITEM_WARPED_PRESSURE_PLATE, 1), name: "warped_pressure_plate" },
    Recipe { needs: &[(ITEM_STRIPPED_WARPED_STEM, 6)], result: (ITEM_WARPED_SHELF, 6), name: "warped_shelf" },
    Recipe { needs: &[(STICK, 1), (ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_SIGN, 3), name: "warped_sign" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 3)], result: (ITEM_WARPED_SLAB, 6), name: "warped_slab" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_STAIRS, 4), name: "warped_stairs" },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_TRAPDOOR, 2), name: "warped_trapdoor" },
    Recipe { needs: &[(ITEM_WAXED_CUT_COPPER_SLAB, 2)], result: (ITEM_WAXED_CHISELED_COPPER, 1), name: "waxed_chiseled_copper" },
    Recipe { needs: &[(ITEM_CHISELED_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_CHISELED_COPPER, 1), name: "waxed_chiseled_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_COPPER_BARS, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_BARS, 1), name: "waxed_copper_bars_from_honeycomb" },
    Recipe { needs: &[(ITEM_COPPER_BLOCK, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_BLOCK, 1), name: "waxed_copper_block_from_honeycomb" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WAXED_COPPER_BLOCK, 3)], result: (ITEM_WAXED_COPPER_BULB, 4), name: "waxed_copper_bulb" },
    Recipe { needs: &[(ITEM_COPPER_BULB, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_BULB, 1), name: "waxed_copper_bulb_from_honeycomb" },
    Recipe { needs: &[(ITEM_COPPER_CHAIN, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_CHAIN, 1), name: "waxed_copper_chain_from_honeycomb" },
    Recipe { needs: &[(ITEM_COPPER_CHEST, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_CHEST, 1), name: "waxed_copper_chest_from_honeycomb" },
    Recipe { needs: &[(ITEM_COPPER_DOOR, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_DOOR, 1), name: "waxed_copper_door_from_honeycomb" },
    Recipe { needs: &[(ITEM_COPPER_GOLEM_STATUE, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_GOLEM_STATUE, 1), name: "waxed_copper_golem_statue_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_COPPER_BLOCK, 4)], result: (ITEM_WAXED_COPPER_GRATE, 4), name: "waxed_copper_grate" },
    Recipe { needs: &[(ITEM_COPPER_GRATE, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_GRATE, 1), name: "waxed_copper_grate_from_honeycomb" },
    Recipe { needs: &[(ITEM_COPPER_LANTERN, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_LANTERN, 1), name: "waxed_copper_lantern_from_honeycomb" },
    Recipe { needs: &[(ITEM_COPPER_TRAPDOOR, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_TRAPDOOR, 1), name: "waxed_copper_trapdoor_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_COPPER_BLOCK, 4)], result: (ITEM_WAXED_CUT_COPPER, 4), name: "waxed_cut_copper" },
    Recipe { needs: &[(ITEM_CUT_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_CUT_COPPER, 1), name: "waxed_cut_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_CUT_COPPER, 3)], result: (ITEM_WAXED_CUT_COPPER_SLAB, 6), name: "waxed_cut_copper_slab" },
    Recipe { needs: &[(ITEM_CUT_COPPER_SLAB, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_CUT_COPPER_SLAB, 1), name: "waxed_cut_copper_slab_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_CUT_COPPER, 6)], result: (ITEM_WAXED_CUT_COPPER_STAIRS, 4), name: "waxed_cut_copper_stairs" },
    Recipe { needs: &[(ITEM_CUT_COPPER_STAIRS, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_CUT_COPPER_STAIRS, 1), name: "waxed_cut_copper_stairs_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB, 2)], result: (ITEM_WAXED_EXPOSED_CHISELED_COPPER, 1), name: "waxed_exposed_chiseled_copper" },
    Recipe { needs: &[(ITEM_EXPOSED_CHISELED_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_CHISELED_COPPER, 1), name: "waxed_exposed_chiseled_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_BARS, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_BARS, 1), name: "waxed_exposed_copper_bars_from_honeycomb" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WAXED_EXPOSED_COPPER, 3)], result: (ITEM_WAXED_EXPOSED_COPPER_BULB, 4), name: "waxed_exposed_copper_bulb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_BULB, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_BULB, 1), name: "waxed_exposed_copper_bulb_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_CHAIN, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_CHAIN, 1), name: "waxed_exposed_copper_chain_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_CHEST, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_CHEST, 1), name: "waxed_exposed_copper_chest_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_DOOR, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_DOOR, 1), name: "waxed_exposed_copper_door_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER, 1), name: "waxed_exposed_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_GOLEM_STATUE, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_GOLEM_STATUE, 1), name: "waxed_exposed_copper_golem_statue_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_COPPER, 4)], result: (ITEM_WAXED_EXPOSED_COPPER_GRATE, 4), name: "waxed_exposed_copper_grate" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_GRATE, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_GRATE, 1), name: "waxed_exposed_copper_grate_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_LANTERN, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_LANTERN, 1), name: "waxed_exposed_copper_lantern_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_TRAPDOOR, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_TRAPDOOR, 1), name: "waxed_exposed_copper_trapdoor_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_COPPER, 4)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER, 4), name: "waxed_exposed_cut_copper" },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER, 1), name: "waxed_exposed_cut_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_CUT_COPPER, 3)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB, 6), name: "waxed_exposed_cut_copper_slab" },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER_SLAB, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB, 1), name: "waxed_exposed_cut_copper_slab_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_CUT_COPPER, 6)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER_STAIRS, 4), name: "waxed_exposed_cut_copper_stairs" },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER_STAIRS, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER_STAIRS, 1), name: "waxed_exposed_cut_copper_stairs_from_honeycomb" },
    Recipe { needs: &[(ITEM_EXPOSED_LIGHTNING_ROD, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_LIGHTNING_ROD, 1), name: "waxed_exposed_lightning_rod_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_LIGHTNING_ROD, 1)], result: (ITEM_WAXED_LIGHTNING_ROD, 1), name: "waxed_lightning_rod_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB, 2)], result: (ITEM_WAXED_OXIDIZED_CHISELED_COPPER, 1), name: "waxed_oxidized_chiseled_copper" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_CHISELED_COPPER, 1)], result: (ITEM_WAXED_OXIDIZED_CHISELED_COPPER, 1), name: "waxed_oxidized_chiseled_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_BARS, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_BARS, 1), name: "waxed_oxidized_copper_bars_from_honeycomb" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WAXED_OXIDIZED_COPPER, 3)], result: (ITEM_WAXED_OXIDIZED_COPPER_BULB, 4), name: "waxed_oxidized_copper_bulb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_BULB, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_BULB, 1), name: "waxed_oxidized_copper_bulb_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_CHAIN, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_CHAIN, 1), name: "waxed_oxidized_copper_chain_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_CHEST, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_CHEST, 1), name: "waxed_oxidized_copper_chest_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_DOOR, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_DOOR, 1), name: "waxed_oxidized_copper_door_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER, 1), name: "waxed_oxidized_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_GOLEM_STATUE, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_GOLEM_STATUE, 1), name: "waxed_oxidized_copper_golem_statue_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_COPPER, 4)], result: (ITEM_WAXED_OXIDIZED_COPPER_GRATE, 4), name: "waxed_oxidized_copper_grate" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_GRATE, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_GRATE, 1), name: "waxed_oxidized_copper_grate_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_LANTERN, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_LANTERN, 1), name: "waxed_oxidized_copper_lantern_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_TRAPDOOR, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_TRAPDOOR, 1), name: "waxed_oxidized_copper_trapdoor_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_COPPER, 4)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER, 4), name: "waxed_oxidized_cut_copper" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_CUT_COPPER, 1)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER, 1), name: "waxed_oxidized_cut_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_CUT_COPPER, 3)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB, 6), name: "waxed_oxidized_cut_copper_slab" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_CUT_COPPER_SLAB, 1)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB, 1), name: "waxed_oxidized_cut_copper_slab_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_CUT_COPPER, 6)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER_STAIRS, 4), name: "waxed_oxidized_cut_copper_stairs" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_CUT_COPPER_STAIRS, 1)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER_STAIRS, 1), name: "waxed_oxidized_cut_copper_stairs_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_LIGHTNING_ROD, 1)], result: (ITEM_WAXED_OXIDIZED_LIGHTNING_ROD, 1), name: "waxed_oxidized_lightning_rod_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB, 2)], result: (ITEM_WAXED_WEATHERED_CHISELED_COPPER, 1), name: "waxed_weathered_chiseled_copper" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_CHISELED_COPPER, 1)], result: (ITEM_WAXED_WEATHERED_CHISELED_COPPER, 1), name: "waxed_weathered_chiseled_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_BARS, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_BARS, 1), name: "waxed_weathered_copper_bars_from_honeycomb" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WAXED_WEATHERED_COPPER, 3)], result: (ITEM_WAXED_WEATHERED_COPPER_BULB, 4), name: "waxed_weathered_copper_bulb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_BULB, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_BULB, 1), name: "waxed_weathered_copper_bulb_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_CHAIN, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_CHAIN, 1), name: "waxed_weathered_copper_chain_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_CHEST, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_CHEST, 1), name: "waxed_weathered_copper_chest_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_DOOR, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_DOOR, 1), name: "waxed_weathered_copper_door_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER, 1)], result: (ITEM_WAXED_WEATHERED_COPPER, 1), name: "waxed_weathered_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_GOLEM_STATUE, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_GOLEM_STATUE, 1), name: "waxed_weathered_copper_golem_statue_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_COPPER, 4)], result: (ITEM_WAXED_WEATHERED_COPPER_GRATE, 4), name: "waxed_weathered_copper_grate" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_GRATE, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_GRATE, 1), name: "waxed_weathered_copper_grate_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_LANTERN, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_LANTERN, 1), name: "waxed_weathered_copper_lantern_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_TRAPDOOR, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_TRAPDOOR, 1), name: "waxed_weathered_copper_trapdoor_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_COPPER, 4)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER, 4), name: "waxed_weathered_cut_copper" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_CUT_COPPER, 1)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER, 1), name: "waxed_weathered_cut_copper_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_CUT_COPPER, 3)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB, 6), name: "waxed_weathered_cut_copper_slab" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_CUT_COPPER_SLAB, 1)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB, 1), name: "waxed_weathered_cut_copper_slab_from_honeycomb" },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_CUT_COPPER, 6)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER_STAIRS, 4), name: "waxed_weathered_cut_copper_stairs" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_CUT_COPPER_STAIRS, 1)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER_STAIRS, 1), name: "waxed_weathered_cut_copper_stairs_from_honeycomb" },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_LIGHTNING_ROD, 1)], result: (ITEM_WAXED_WEATHERED_LIGHTNING_ROD, 1), name: "waxed_weathered_lightning_rod_from_honeycomb" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_TERRACOTTA, 1), (ITEM_WAYFINDER_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_WAYFINDER_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "wayfinder_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_WEATHERED_CUT_COPPER_SLAB, 2)], result: (ITEM_WEATHERED_CHISELED_COPPER, 1), name: "weathered_chiseled_copper" },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WEATHERED_COPPER, 3)], result: (ITEM_WEATHERED_COPPER_BULB, 4), name: "weathered_copper_bulb" },
    Recipe { needs: &[(ITEM_WEATHERED_COPPER, 4)], result: (ITEM_WEATHERED_COPPER_GRATE, 4), name: "weathered_copper_grate" },
    Recipe { needs: &[(ITEM_WEATHERED_COPPER, 4)], result: (ITEM_WEATHERED_CUT_COPPER, 4), name: "weathered_cut_copper" },
    Recipe { needs: &[(ITEM_WEATHERED_CUT_COPPER, 3)], result: (ITEM_WEATHERED_CUT_COPPER_SLAB, 6), name: "weathered_cut_copper_slab" },
    Recipe { needs: &[(ITEM_WEATHERED_CUT_COPPER, 6)], result: (ITEM_WEATHERED_CUT_COPPER_STAIRS, 4), name: "weathered_cut_copper_stairs" },
    Recipe { needs: &[(ITEM_HAY_BLOCK, 1)], result: (WHEAT, 9), name: "wheat" },
    Recipe { needs: &[(STICK, 1), (ITEM_WHITE_WOOL, 6)], result: (ITEM_WHITE_BANNER, 1), name: "white_banner" },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_WHITE_BED, 1), name: "white_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_CANDLE, 1), name: "white_candle" },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 2)], result: (ITEM_WHITE_CARPET, 3), name: "white_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_CONCRETE_POWDER, 8), name: "white_concrete_powder" },
    Recipe { needs: &[(ITEM_WHITE_CONCRETE, 3)], result: (ITEM_WHITE_CONCRETE_SLAB, 6), name: "white_concrete_slab" },
    Recipe { needs: &[(ITEM_WHITE_CONCRETE, 6)], result: (ITEM_WHITE_CONCRETE_STAIRS, 4), name: "white_concrete_stairs" },
    Recipe { needs: &[(ITEM_WHITE_WOOL_SLAB, 3)], result: (ITEM_WHITE_CUSHION, 1), name: "white_cushion" },
    Recipe { needs: &[(ITEM_BONE_MEAL, 1)], result: (ITEM_WHITE_DYE, 1), name: "white_dye" },
    Recipe { needs: &[(ITEM_LILY_OF_THE_VALLEY, 1)], result: (ITEM_WHITE_DYE, 1), name: "white_dye_from_lily_of_the_valley" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_WHITE_WOOL, 1)], result: (ITEM_WHITE_HARNESS, 1), name: "white_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_STAINED_GLASS, 8), name: "white_stained_glass" },
    Recipe { needs: &[(ITEM_WHITE_STAINED_GLASS, 6)], result: (ITEM_WHITE_STAINED_GLASS_PANE, 16), name: "white_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_STAINED_GLASS_PANE, 8), name: "white_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_TERRACOTTA, 8), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_TERRACOTTA, 8), name: "white_terracotta" },
    Recipe { needs: &[(ITEM_STRING, 4)], result: (ITEM_WHITE_WOOL, 1), name: "white_wool_from_string" },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 3)], result: (ITEM_WHITE_WOOL_SLAB, 6), name: "white_wool_slab" },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 6)], result: (ITEM_WHITE_WOOL_STAIRS, 4), name: "white_wool_stairs" },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_MOSSY_COBBLESTONE, 1), (ITEM_WILD_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_WILD_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "wild_armor_trim_smithing_template" },
    Recipe { needs: &[(ITEM_BREEZE_ROD, 1)], result: (ITEM_WIND_CHARGE, 4), name: "wind_charge" },
    Recipe { needs: &[(ITEM_ARMADILLO_SCUTE, 6)], result: (ITEM_WOLF_ARMOR, 1), name: "wolf_armor" },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 3)], result: (ITEM_WOODEN_AXE, 1), name: "wooden_axe" },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 2)], result: (ITEM_WOODEN_HOE, 1), name: "wooden_hoe" },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 3)], result: (WOODEN_PICKAXE, 1), name: "wooden_pickaxe" },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 1)], result: (ITEM_WOODEN_SHOVEL, 1), name: "wooden_shovel" },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 1)], result: (ITEM_WOODEN_SPEAR, 1), name: "wooden_spear" },
    Recipe { needs: &[(STICK, 1), (TAG_PLANKS, 2)], result: (ITEM_WOODEN_SWORD, 1), name: "wooden_sword" },
    Recipe { needs: &[(ITEM_BOOK, 1), (ITEM_FEATHER, 1), (ITEM_INK_SAC, 1)], result: (ITEM_WRITABLE_BOOK, 1), name: "writable_book" },
    Recipe { needs: &[(STICK, 1), (ITEM_YELLOW_WOOL, 6)], result: (ITEM_YELLOW_BANNER, 1), name: "yellow_banner" },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_YELLOW_BED, 1), name: "yellow_bed" },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_CANDLE, 1), name: "yellow_candle" },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 2)], result: (ITEM_YELLOW_CARPET, 3), name: "yellow_carpet" },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_CONCRETE_POWDER, 8), name: "yellow_concrete_powder" },
    Recipe { needs: &[(ITEM_YELLOW_CONCRETE, 3)], result: (ITEM_YELLOW_CONCRETE_SLAB, 6), name: "yellow_concrete_slab" },
    Recipe { needs: &[(ITEM_YELLOW_CONCRETE, 6)], result: (ITEM_YELLOW_CONCRETE_STAIRS, 4), name: "yellow_concrete_stairs" },
    Recipe { needs: &[(ITEM_YELLOW_WOOL_SLAB, 3)], result: (ITEM_YELLOW_CUSHION, 1), name: "yellow_cushion" },
    Recipe { needs: &[(ITEM_DANDELION, 1)], result: (ITEM_YELLOW_DYE, 1), name: "yellow_dye_from_dandelion" },
    Recipe { needs: &[(ITEM_GOLDEN_DANDELION, 1)], result: (ITEM_YELLOW_DYE, 1), name: "yellow_dye_from_golden_dandelion" },
    Recipe { needs: &[(ITEM_SUNFLOWER, 1)], result: (ITEM_YELLOW_DYE, 2), name: "yellow_dye_from_sunflower" },
    Recipe { needs: &[(ITEM_WILDFLOWERS, 1)], result: (ITEM_YELLOW_DYE, 1), name: "yellow_dye_from_wildflowers" },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_YELLOW_WOOL, 1)], result: (ITEM_YELLOW_HARNESS, 1), name: "yellow_harness" },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_STAINED_GLASS, 8), name: "yellow_stained_glass" },
    Recipe { needs: &[(ITEM_YELLOW_STAINED_GLASS, 6)], result: (ITEM_YELLOW_STAINED_GLASS_PANE, 16), name: "yellow_stained_glass_pane" },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_STAINED_GLASS_PANE, 8), name: "yellow_stained_glass_pane_from_glass_pane" },
    Recipe { needs: &[(ITEM_TERRACOTTA, 8), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_TERRACOTTA, 8), name: "yellow_terracotta" },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 3)], result: (ITEM_YELLOW_WOOL_SLAB, 6), name: "yellow_wool_slab" },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 6)], result: (ITEM_YELLOW_WOOL_STAIRS, 4), name: "yellow_wool_stairs" },
];
// === GEN-TABLE-END ===

// ---------------------------------------------------------------------------
// P47：3×3 工作台合成（与背包 2×2 完全隔离，GRID_SIZE/match_recipe/consume_grid 不动）。
// 槽语义：窗口 1 = 工作台；槽 0 = 结果，槽 1..=9 = 合成格，10..=45 = 玩家背包。
// 复用同一张 1185 条配方表，只按多重集匹配（工作台专属形状/歧义槽型不在 MVP 范围）。
// ---------------------------------------------------------------------------

/// 工作台合成格数量（槽 1..=9）。
pub const GRID9_SIZE: usize = 9;
/// 工作台窗口 ID（玩家背包窗口仍是 0；非 0 的点击只在工作台会话里处理）。
pub const CRAFT9_WINDOW_ID: i32 = 1;
/// 工作台结果槽。
pub const CRAFT9_RESULT_SLOT: i16 = 0;

/// 3×3 配方匹配：与 2×2 共用配方表/无序口径（子集匹配 + 规范槽型拆分），
/// 9 格用自己的占用槽指纹做规范槽型拆分，仍无命中时按
/// 总需求量降序 + 表序兜底，确定性且与 HashMap 无关。
pub fn match_recipe_9(grid: [Option<(i32, i32)>; GRID9_SIZE]) -> Option<&'static Recipe> {
    let input = normalized_multiset(&grid_stacks9(grid));
    let candidates: Vec<&'static Recipe> =
        RECIPES.iter().filter(|r| needs_subset_of(r.needs, &input)).collect();
    resolve_candidates(candidates, slot_fingerprint9(grid))
}

/// 3×3 合成格展平。
pub fn grid_stacks9(grid: [Option<(i32, i32)>; GRID9_SIZE]) -> Vec<(i32, i32)> {
    grid.into_iter().flatten().collect()
}

/// 按 ID 从 3×3 格扣除原料（与 `consume_grid` 同语义，返回是否足够）。
/// 标签需求从成员槽按槽序扣（确定性）。
pub fn consume_grid9(grid: &mut [Option<(i32, i32)>; GRID9_SIZE], recipe: &Recipe) -> bool {
    for (id, need) in recipe.needs {
        let mut left = *need;
        let members;
        let want_tag = *id < 0;
        if want_tag {
            members = tag_members(*id);
        } else {
            members = &[];
        }
        for slot in grid.iter_mut() {
            if left <= 0 {
                break;
            }
            if let Some((sid, count)) = slot {
                if !(if want_tag { members.contains(sid) } else { *sid == *id }) {
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

/// 某物品能否作为配方的原料：具体需求精确对应，标签需求按成员归属。
/// 供工作台补料时判断“只补锁定配方的料”，避免把产物又当原料拉回来
///（如干草块 ↔ 小麦的可逆配方）或拉进无关的石头。
pub fn recipe_accepts_ingredient(recipe: &Recipe, item_id: i32) -> bool {
    recipe.needs.iter().any(|(need, _)| {
        if *need >= 0 {
            *need == item_id
        } else {
            tag_members(*need).contains(&item_id)
        }
    })
}

/// 标签成员解析：need id < 0 时为标签引用，`-(index+1)` 索引 [`CRAFT_TAGS`]。
/// 不在表内的负数索引视为空集（生成器保证不产生）。
fn tag_members(tag_id: i32) -> &'static [i32] {
    if tag_id >= 0 {
        return &[];
    }
    CRAFT_TAGS.get((-tag_id - 1) as usize).copied().unwrap_or(&[])
}

/// 配方需求是否被输入多重集满足：输入的每种物品必须被某条需求覆盖
///（具体物品需求精确对应；标签需求按成员归属），且每条需求的数量被满足
///（具体物品按该 ID 计数，标签按成员总量计数）。多的留下，杂物不行。
/// `needs`/`input` 均为 [`normalized_multiset`] 后的 `(id, count)` 有序表。
fn needs_subset_of(needs: &[(i32, i32)], input: &[(i32, i32)]) -> bool {
    let agg = normalized_multiset(needs);
    agg.iter().all(|(id, n)| {
        if *id >= 0 {
            input.iter().any(|(i, h)| i == id && h >= n)
        } else {
            let members = tag_members(*id);
            input.iter().filter(|(i, _)| members.contains(i)).map(|(_, h)| h).sum::<i32>() >= *n
        }
    }) && input.iter().all(|(id, _)| {
        agg.iter().any(|(i, _)| i == id)
            || agg.iter().any(|(i, _)| *i < 0 && tag_members(*i).contains(id))
    })
}

/// 规范槽型是否命中指纹：先直接比较；含标签需求的配方把指纹里的成员 ID
/// 折叠成标签占位后再比（同一成员若属多个需求标签则判为不命中，保证确定性）。
/// 纯具体配方走旧逐字节口径，行为不变。
fn layout_matches(recipe: &Recipe, fingerprint: &[(i32, i32)]) -> bool {
    let Some(layout) = canonical_layout(recipe) else { return false };
    if layout.as_slice() == fingerprint {
        return true;
    }
    if layout.iter().all(|(id, _)| *id >= 0) {
        return false;
    }
    let mut tags: Vec<(i32, &[i32])> = Vec::new();
    for (id, _) in recipe.needs {
        if *id < 0 && !tags.iter().any(|(t, _)| t == id) {
            tags.push((*id, tag_members(*id)));
        }
    }
    let mut mapped: Vec<(i32, i32)> = Vec::with_capacity(fingerprint.len());
    for (id, c) in fingerprint {
        let mut hit: Option<i32> = None;
        if layout.iter().any(|(lid, _)| *lid == *id) {
            hit = Some(*id);
        } else {
            for (tid, members) in &tags {
                if members.contains(id) {
                    if hit.is_some() {
                        return false;
                    }
                    hit = Some(*tid);
                }
            }
        }
        let Some(h) = hit else { return false };
        mapped.push((h, *c));
    }
    mapped.sort_unstable();
    mapped.as_slice() == layout.as_slice()
}

/// 候选配方确定性拆分：规范槽型优先（歧义组），否则总需求量降序
///（越具体越优先），同量按表序（`candidates` 本来就是表序，稳定排序保持它）。
/// 严格子集候选永远匹配不上任何指纹（槽位总数对不上），
/// 因此指纹轮只在"精确命中"的候选里选，与旧"精确优先"口径一致。
fn resolve_candidates(
    candidates: Vec<&'static Recipe>,
    fingerprint: Vec<(i32, i32)>,
) -> Option<&'static Recipe> {
    if candidates.is_empty() {
        return None;
    }
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }
    for recipe in &candidates {
        if layout_matches(recipe, &fingerprint) {
            return Some(recipe);
        }
    }
    let mut ranked = candidates;
    ranked.sort_by_key(|r| std::cmp::Reverse(r.needs.iter().map(|(_, n)| *n).sum::<i32>()));
    ranked.into_iter().next()
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

/// 9 格占用槽指纹（与 [`slot_fingerprint`] 同口径，供 3×3 规范槽型拆分）。
fn slot_fingerprint9(grid: [Option<(i32, i32)>; GRID9_SIZE]) -> Vec<(i32, i32)> {
    let mut slots: Vec<(i32, i32)> = grid.into_iter().flatten().filter(|(_, c)| *c > 0).collect();
    slots.sort_unstable();
    slots
}

/// 歧义组的规范槽型（按 (id,count) 排序后比较）。
///
/// 原版里下列配方恰好消耗相同的物品集合（本服不检查形状），用规范槽型做确定性拆分；
/// 槽型本身也相同的按总需求量降序 + 表序回退（确定性，见 [`resolve_candidates`]）：
/// - 木棍/压力板：2 木板分两格(1+1) → 木棍；2 木板放单格 → 压力板。
/// - 工作台：4 木板单格（或 2+2 等其它拆分经回退）→ 工作台。
/// - 镐/斧：材料 3 单格 + 木棍 2 单格 → 镐；材料 2+1 + 木棍 1+1（占满 4 格）→ 斧。
/// - 门/活板门/楼梯（6 木板）：3+3 → 门（楼梯同槽型，按表序归门）；
///   2+2+2 → 活板门；1+2+3 台阶形 → 楼梯。
/// - 栅栏/栅栏门：木板 2+2 + 木棍 1+1 → 栅栏；木板 2+1+1 + 木棍 2 单格 → 栅栏门。
/// - 护腿/炼药锅：7 材料拆 3+2+2 → 护腿；7 材料单格 → 炼药锅。
/// - 碗/台阶：3 木板拆 1+1+1 → 碗；3 木板单格 → 台阶。
/// - 铲/矛：材料 1 + 木棍 1+1 → 铲（矛同槽型，按表序归铲；木棍叠放 2 格 → 矛）。
/// - 靴子/切制铜/砖块/陶罐（4 同材）：1+1+1+1 → 靴子（铜靴）/栅格；
///   2+2 → 切制铜/砖块；1+2+1 → 饰纹陶罐；其余回退按表序。
/// - 头盔/矿车：3+2 → 头盔（矿车同槽型，按表序归头盔；2+2+1 → 矿车）。
/// - 栅格：4 材料分 4 格 → 栅格。
/// - 马铠/护腿：槽型相同，按表序归马铠（单格 7 也归马铠；3+2+2 拆分归护腿）。
fn canonical_layout(recipe: &Recipe) -> Option<Vec<(i32, i32)>> {
    // 主材料判定（与生成表 needs 顺序无关，确定性）：
    // 有 tag 需求取首个 tag（tag 配方的主材料恒为 tag）；
    // 否则取唯一的非木棍具体材料（栅栏门/铲等多 sticks 配方也能找对）；
    // 兜底取首条需求。木棍 ID 见 STICK（1060）。
    let (mat, n) = recipe
        .needs
        .iter()
        .find(|(id, _)| *id < 0)
        .or_else(|| {
            let mut concrete = recipe
                .needs
                .iter()
                .filter(|(id, _)| *id >= 0 && *id != STICK);
            let first = concrete.next()?;
            if concrete.next().is_some() {
                return None;
            }
            Some(first)
        })
        .or_else(|| recipe.needs.first())
        .copied()?;
    let mut layout = if recipe.name == "stick" || recipe.name.ends_with("_stick") {
        vec![(mat, 1), (mat, 1)]
    } else if recipe.name == "crafting_table" || recipe.name.ends_with("_crafting_table") {
        vec![(mat, 4)]
    } else if recipe.name.ends_with("_pressure_plate") {
        vec![(mat, n)]
    } else if recipe.name.ends_with("_stairs") {
        // 台阶 vanialla 形状是 1+2+3 的台阶（不是 3+3，后者归门）。
        vec![(mat, 1), (mat, 2), (mat, 3)]
    } else if recipe.name.ends_with("_pickaxe") {
        vec![(mat, 3), (STICK, 2)]
    } else if recipe.name.ends_with("_axe") {
        vec![(mat, 2), (mat, 1), (STICK, 1), (STICK, 1)]
    } else if recipe.name.ends_with("_door") {
        vec![(mat, 3), (mat, 3)]
    } else if recipe.name.ends_with("_trapdoor") {
        // 木活板门 6 个（2+2+2）；铁/铜活板门 4 个（2×2），按总需求量区分。
        let total: i32 = recipe.needs.iter().map(|(_, n)| *n).sum();
        if total == 4 {
            vec![(mat, 2), (mat, 2)]
        } else {
            vec![(mat, 2), (mat, 2), (mat, 2)]
        }
    } else if recipe.name.ends_with("_fence_gate") {
        // 栅栏门：木板 2+1+1 + 木棍整组（vanilla 为 4 木棍一格；旧表曾简写成 2 棍）。
        let sn: i32 = recipe.needs.iter().filter(|(id, _)| *id == STICK).map(|(_, n)| *n).sum();
        vec![(mat, 2), (mat, 1), (mat, 1), (STICK, sn)]
    } else if recipe.name.ends_with("_fence") {
        vec![(mat, 2), (mat, 2), (STICK, 1), (STICK, 1)]
    } else if recipe.name == "cauldron" {
        vec![(mat, 7)]
    } else if recipe.name == "bowl" {
        vec![(mat, 1), (mat, 1), (mat, 1)]
    } else if recipe.name.ends_with("_slab") {
        vec![(mat, 3)]
    } else if recipe.name == "decorated_pot_simple" {
        vec![(mat, 1), (mat, 1), (mat, 2)]
    } else if recipe.name.ends_with("_boots") {
        vec![(mat, 1), (mat, 1), (mat, 1), (mat, 1)]
    } else if recipe.name == "cut_copper" || recipe.name.ends_with("_cut_copper") {
        vec![(mat, 2), (mat, 2)]
    } else if recipe.name == "bricks" {
        vec![(mat, 2), (mat, 2)]
    } else if recipe.name == "shears" {
        vec![(mat, 1), (mat, 1)]
    } else if recipe.name == "minecart" {
        vec![(mat, 2), (mat, 2), (mat, 1)]
    } else if recipe.name.ends_with("_spear") {
        // 矛（对角线）与铲（竖列）槽位 multiset 相同，按表序归铲；
        // 木棍叠放的 2 格安排只匹配矛，保证矛也可合成。
        vec![(mat, 1), (STICK, 2)]
    } else if recipe.name.ends_with("_grate") {
        vec![(mat, 1), (mat, 1), (mat, 1), (mat, 1)]
    } else if recipe.name.ends_with("_shovel") {
        vec![(mat, 1), (STICK, 1), (STICK, 1)]
    } else if recipe.name.ends_with("_helmet") {
        vec![(mat, 3), (mat, 2)]
    } else if recipe.name.ends_with("_leggings") {
        vec![(mat, 3), (mat, 2), (mat, 2)]
    } else {
        return None;
    };
    layout.sort_unstable();
    Some(layout)
}

/// 无序匹配（原版 shapeless 口径）：输入多重集满足某配方的需求
///（每种物品数量 ≥ 配方需求、且无配方外的杂物）即命中，多的留下
///（原版放一整组也只扣一份）。候选多条时先按规范槽型拆分
///（[`canonical_layout`] + [`layout_matches`]，tag 成员归属折叠后比较），
/// 再按总需求量降序（越具体越优先），同量按表序，绝不随机、也不按 HashMap 迭代顺序出结果。
pub fn match_recipe(grid: [Option<(i32, i32)>; GRID_SIZE]) -> Option<&'static Recipe> {
    let input = normalized_multiset(&grid_stacks(grid));
    let candidates: Vec<&'static Recipe> =
        RECIPES.iter().filter(|r| needs_subset_of(r.needs, &input)).collect();
    resolve_candidates(candidates, slot_fingerprint(grid))
}
/// 配方命中后按 ID 从合成格扣除原料（每格最多取自己数量；返回是否全部扣成功）。
/// 标签需求从成员槽按槽序扣（确定性）。调用方应先 `match_recipe`；不匹配时不要调用，避免误扣。
pub fn consume_grid(grid: &mut [Option<(i32, i32)>; GRID_SIZE], recipe: &Recipe) -> bool {
    for (id, need) in recipe.needs {
        let mut left = *need;
        let members;
        let want_tag = *id < 0;
        if want_tag {
            members = tag_members(*id);
        } else {
            members = &[];
        }
        for slot in grid.iter_mut() {
            if left <= 0 {
                break;
            }
            if let Some((sid, count)) = slot {
                if !(if want_tag { members.contains(sid) } else { *sid == *id }) {
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
        // 同种多一件仍命中（只扣一份，多的留下；原版放一整组也一样）。
        let mut grid = [Some((OAK_LOG, 2)), None, None, None];
        assert_eq!(match_recipe(grid).unwrap().result, (OAK_PLANKS, 4));
        let r = match_recipe(grid).unwrap();
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [Some((OAK_LOG, 1)), None, None, None]);
        // 多一个无关物品 → 原木配方不成立（避免白送木板）。
        assert!(match_recipe([Some((OAK_LOG, 1)), Some((STICK, 1)), None, None]).is_none());
        // 3 圆石 + 1 木棍 → 石剑成立（总需求量 3 > 拉杆 2，越具体越优先；
        // 只扣 2 圆石 + 1 木棍，剩 1 圆石）。
        let mut grid = [Some((COBBLESTONE, 3)), Some((STICK, 1)), None, None];
        let r = match_recipe(grid).expect("3 cobble + 1 stick must match stone_sword");
        assert_eq!(r.name, "stone_sword");
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [Some((COBBLESTONE, 1)), None, None, None]);
        // 少一件原料 → 面包不成立（2 小麦不够 3 个，其它小麦配方也差东西）。
        assert!(match_recipe([Some((WHEAT, 2)), None, None, None]).is_none());
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
        // 18 个小麦（每格 2 个）→ 干草捆成立，扣 9 个剩 9 个（子集语义）。
        let mut too_many = [None; GRID9_SIZE];
        for slot in too_many.iter_mut() {
            *slot = Some((WHEAT, 2));
        }
        let r = match_recipe_9(too_many).expect("18 wheat must still match hay_block");
        assert_eq!(r.name, "hay_block");
        assert!(consume_grid9(&mut too_many, r));
        assert_eq!(too_many.iter().flatten().map(|(_, c)| c).sum::<i32>(), 9);
        assert!(match_recipe_9(too_many).is_some(), "leftover 9 wheat still crafts");
    }

    #[test]
    fn p41_all_recipe_ids_resolve_in_generated_table() {
        let mut seen = std::collections::BTreeSet::new();
        // P59 全量表：1185 条 vanilla（data/777/full/recipes 生成，scripts/gen_recipes.py 复核）。
        assert_eq!(RECIPES.len(), 1185, "P59 配方总数（vanilla 26.3 全量，tag 原生）");
        for recipe in RECIPES {
            assert!(!recipe.needs.is_empty(), "empty needs in {}", recipe.name);
            let total: i32 = recipe.needs.iter().map(|(_, c)| *c).sum();
            // 单格堆叠上限 64，9 格合计不超过 576（vanilla 最大约 9）。
            assert!(total > 0 && total <= 576, "{} needs {total} items", recipe.name);
            assert!(recipe.needs.iter().all(|(_, c)| *c > 0), "{} has non-positive need", recipe.name);
            // 标签需求展开成员逐个验证（负数 ID 是 CRAFT_TAGS 索引，不是物品）。
            let mut concrete: Vec<i32> = Vec::new();
            for &(id, _) in recipe.needs {
                if id >= 0 {
                    concrete.push(id);
                } else {
                    let members = tag_members(id);
                    assert!(!members.is_empty(), "{} references empty tag {id}", recipe.name);
                    concrete.extend_from_slice(members);
                }
            }
            concrete.push(recipe.result.0);
            for id in concrete {
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

    /// 输入多重集相同的配方组必须恰好是已知的 72 个（P59 全量表锁定）；
    /// 有规范槽型的成员按槽型拆分（逐个验证），其余按总需求量降序 + 表序回退（确定性）。
    #[test]
    fn p41_input_multiset_conflicts_are_known_and_splittable() {
        use std::collections::BTreeMap;
        let mut groups: BTreeMap<Vec<(i32, i32)>, Vec<&Recipe>> = BTreeMap::new();
        for recipe in RECIPES {
            groups.entry(normalized_multiset(recipe.needs)).or_default().push(recipe);
        }
        let conflicts: Vec<Vec<&Recipe>> =
            groups.into_values().filter(|group| group.len() > 1).collect();
        assert_eq!(conflicts.len(), 72, "unexpected number of ambiguous input groups");
        // 表序位置（回退 tie-break 用）。
        let pos_of = |r: &Recipe| RECIPES.iter().position(|x| x.name == r.name).unwrap();
        for group in &conflicts {
            // 1) 有规范槽型的成员：在其槽型上放料必须命中它（tag 成员取首个具体物品）。
            for r in group.iter().filter(|r| canonical_layout(r).is_some()) {
                let layout = canonical_layout(r).unwrap();
                let mut grid = [None; GRID_SIZE];
                // 槽型条目数一定 ≤4（布局定义保证；超限直接判失败）。
                assert!(layout.len() <= GRID_SIZE, "{} layout too big", r.name);
                for (i, (id, c)) in layout.iter().enumerate() {
                    let concrete = if *id >= 0 {
                        *id
                    } else {
                        tag_members(*id).first().copied().unwrap_or(*id)
                    };
                    grid[i] = Some((concrete, *c));
                }
                let got = match_recipe(grid).unwrap_or_else(|| {
                    panic!("canonical layout of {} matches nothing", r.name)
                });
                // 同一槽型被多配方共享时（如门/楼梯 3+3），表序首个获胜。
                let sharers: Vec<&&Recipe> = group
                    .iter()
                    .filter(|c| {
                        canonical_layout(c).as_deref()
                            == canonical_layout(r).as_deref()
                    })
                    .collect();
                let expected = sharers.iter().min_by_key(|c| pos_of(c)).unwrap().name;
                assert_eq!(got.name, expected, "canonical split of {}", r.name);
            }
            // 2) 无槽型成员：回退路径是确定性的（重复解析结果一致）。
            let mut fp: Vec<(i32, i32)> = group[0]
                .needs
                .iter()
                .map(|(id, c)| {
                    let concrete = if *id >= 0 {
                        *id
                    } else {
                        tag_members(*id).first().copied().unwrap_or(*id)
                    };
                    (concrete, *c)
                })
                .collect();
            fp.sort_unstable();
            let a = resolve_candidates(group.clone(), fp.clone());
            let b = resolve_candidates(group.clone(), fp);
            assert_eq!(
                a.map(|r| r.name),
                b.map(|r| r.name),
                "fallback must be deterministic"
            );
        }
        // 3) 回退赢家在 p41_ambiguous_families_are_deterministic 里逐个 pin 住（文档化口径）。
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

        // 箱子：8 木板放一格（vanilla 通用箱子配方，tag 口径）。
        let mut grid = [Some((OAK_PLANKS, 8)), None, None, None];
        let r = match_recipe(grid).expect("chest");
        assert_eq!(r.name, "chest");
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
        // 多一件无关物品仍不命中（杂物照样挡配方，只有"同种多件"放宽）。
        assert!(match_recipe([Some((ITEM_IRON_INGOT, 3)), Some((STICK, 2)), Some((COBBLESTONE, 1)), None]).is_none());
    }

    /// 歧义组的确定性拆分：每种规范槽型给出唯一结果，重复调用不变；
    /// 未命中任何规范槽型时按总需求量降序 + 表序回退，绝不随机。
    #[test]
    fn p41_ambiguous_families_are_deterministic() {
        let cases: [([Option<(i32, i32)>; 4], &str); 10] = [
            // 木棍（1+1）/ 压力板（单格 2）
            ([Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), None, None], "stick"),
            ([Some((OAK_PLANKS, 2)), None, None, None], "oak_pressure_plate"),
            // 工作台（单格 4）；2+2 木板也是 2×2 铺满 → 工作台（vanilla 真值）。
            ([Some((OAK_PLANKS, 4)), None, None, None], "crafting_table"),
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 2)), None, None], "crafting_table"),
            // 木镐（3 单格 + 2 单格）/ 木斧（2+1 + 1+1）
            ([Some((OAK_PLANKS, 3)), Some((STICK, 2)), None, None], "wooden_pickaxe"),
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 1)), Some((STICK, 1)), Some((STICK, 1))], "wooden_axe"),
            // 门（3+3）/ 活板门（2+2+2）；台阶须摆 1+2+3 才命中
            ([Some((OAK_PLANKS, 3)), Some((OAK_PLANKS, 3)), None, None], "oak_door"),
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 2)), None], "oak_trapdoor"),
            // 栅栏（2+2 + 1+1）/ 栅栏门（2+1+1 + 棍 4 单格；vanilla 真值，旧表曾简写成 2 棍）
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 2)), Some((STICK, 1)), Some((STICK, 1))], "oak_fence"),
            ([Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), Some((STICK, 4))], "oak_fence_gate"),
        ];
        for (grid, expected) in cases {
            assert_eq!(match_recipe(grid).unwrap().name, expected);
            assert_eq!(match_recipe(grid).unwrap().name, expected, "must be repeatable");
        }
        // 铁护腿（3+2+2）/ 炼药锅（单格 7）。
        let leggings = [Some((ITEM_IRON_INGOT, 3)), Some((ITEM_IRON_INGOT, 2)), Some((ITEM_IRON_INGOT, 2)), None];
        assert_eq!(match_recipe(leggings).unwrap().name, "iron_leggings");
        assert_eq!(match_recipe([Some((ITEM_IRON_INGOT, 7)), None, None, None]).unwrap().name, "cauldron");
        // 未命中规范槽型（3+1 木板）→ 总需求量最大者 crafting_table，绝不随机。
        let odd = [Some((OAK_PLANKS, 3)), Some((OAK_PLANKS, 1)), None, None];
        assert_eq!(match_recipe(odd).unwrap().name, "crafting_table");
        // P59 新增槽型：碗（1+1+1）/ 台阶（单格 3）/ 台阶形 1+2+3 / 铁活板门（2+2）。
        let bowl = [Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), None];
        assert_eq!(match_recipe(bowl).unwrap().name, "bowl");
        assert_eq!(match_recipe([Some((OAK_PLANKS, 3)), None, None, None]).unwrap().name, "oak_slab");
        let stairs = [Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 3)), None];
        assert_eq!(match_recipe(stairs).unwrap().name, "oak_stairs");
        let trap4 = [Some((ITEM_IRON_INGOT, 2)), Some((ITEM_IRON_INGOT, 2)), None, None];
        assert_eq!(match_recipe(trap4).unwrap().name, "iron_trapdoor");
        // TAG 混合：橡木 + 云杉木板混搭也能出木棍（子集语义 + 标签归属）。
        let mixed = [Some((OAK_PLANKS, 1)), Some((ITEM_SPRUCE_PLANKS, 1)), None, None];
        assert_eq!(match_recipe(mixed).unwrap().name, "stick");
        let mut mixed_grid = mixed;
        let r = match_recipe(mixed_grid).unwrap();
        assert!(consume_grid(&mut mixed_grid, r));
        assert_eq!(mixed_grid, [None, None, None, None]);
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
