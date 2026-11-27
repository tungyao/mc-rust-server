//! crafting: P40 2x2 玩家背包合成 + P47 3x3 工作台合成的纯逻辑（配方 + 匹配 + 消耗），只兼容 26.3/777。
//!
//! 所有物品 ID 均由 mc_registry::ITEMS（data/777/items.json 生成表）在编译期查得，
//! 与 drops.rs 同口径；不手写/猜物品 ID。
//!
//! P60 vanilla 形状口径（scripts/gen_recipes.py 从原版 JSON 逐条生成形状）：
//! - shaped：去边稀疏格 cells，允许平移 + 水平镜像；覆盖格须非空（每格 >=1，
//!   tag 按成员归属），其余格须空。堆叠多件只扣 1，多的留下。
//! - shapeless：needs 展开为逐单位列表，每个占用格提供 1 单位；占用格数须与
//!   单位数相等（格外有货就不中），tag 灵活归属。
//! 输出栈直接返回给调用方，由网络层放光标/背包。
//!
//! 歧义（同格同料不同结果）按 RECIPES 表序取首条，确定性；2x2 网格只能放下
//! 不大于 2x2 的形状（工具类须去工作台，原版一致）。
//!
//! P41/P59 扩充：覆盖原版全部工作台配方（1185 条，tag 原生，scripts/check_recipes.py
//! 独立复核 0 缺失 0 重复）。

use crate::registry_id;

/// 合成格数量（背包 1..=4）。
pub const GRID_SIZE: usize = 4;
/// 合成结果槽（背包 0）。
pub const RESULT_SLOT: usize = 0;

/// 单条配方：`needs` 为 (物品 ID/负tag, 数量)，`result` 为 (物品 ID, 数量)。
/// P60 vanilla 形状：`shaped` 为 true 时 `cells` 为去边后稀疏格 (x, y, 料ID/负tag)，
/// 匹配可平移+镜像、格外须空、每格至少 1 个；为 false 时是 shapeless，
/// 每个占用格提供 1 单位（多的留下），格数须与 units 总数相等。
#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    pub needs: &'static [(i32, i32)],
    pub result: (i32, i32),
    /// 便于诊断/测试的名称（不参与匹配）。
    pub name: &'static str,
    pub shaped: bool,
    pub cells: &'static [(u8, u8, i32)],
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
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 5)], result: (ITEM_ACACIA_BOAT, 1), name: "acacia_boat", shaped: true, cells: &[(0, 0, ITEM_ACACIA_PLANKS), (2, 0, ITEM_ACACIA_PLANKS), (0, 1, ITEM_ACACIA_PLANKS), (1, 1, ITEM_ACACIA_PLANKS), (2, 1, ITEM_ACACIA_PLANKS)] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 1)], result: (ITEM_ACACIA_BUTTON, 1), name: "acacia_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ACACIA_BOAT, 1), (ITEM_CHEST, 1)], result: (ITEM_ACACIA_CHEST_BOAT, 1), name: "acacia_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6)], result: (ITEM_ACACIA_DOOR, 3), name: "acacia_door", shaped: true, cells: &[(0, 0, ITEM_ACACIA_PLANKS), (1, 0, ITEM_ACACIA_PLANKS), (0, 1, ITEM_ACACIA_PLANKS), (1, 1, ITEM_ACACIA_PLANKS), (0, 2, ITEM_ACACIA_PLANKS), (1, 2, ITEM_ACACIA_PLANKS)] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 4), (STICK, 2)], result: (ITEM_ACACIA_FENCE, 3), name: "acacia_fence", shaped: true, cells: &[(0, 0, ITEM_ACACIA_PLANKS), (1, 0, STICK), (2, 0, ITEM_ACACIA_PLANKS), (0, 1, ITEM_ACACIA_PLANKS), (1, 1, STICK), (2, 1, ITEM_ACACIA_PLANKS)] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 2), (STICK, 4)], result: (ITEM_ACACIA_FENCE_GATE, 1), name: "acacia_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_ACACIA_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_ACACIA_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_ACACIA_LOG, 6)], result: (ITEM_ACACIA_HANGING_SIGN, 6), name: "acacia_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_ACACIA_LOG), (1, 1, ITEM_STRIPPED_ACACIA_LOG), (2, 1, ITEM_STRIPPED_ACACIA_LOG), (0, 2, ITEM_STRIPPED_ACACIA_LOG), (1, 2, ITEM_STRIPPED_ACACIA_LOG), (2, 2, ITEM_STRIPPED_ACACIA_LOG)] },
    Recipe { needs: &[(TAG_ACACIA_LOGS, 1)], result: (ITEM_ACACIA_PLANKS, 4), name: "acacia_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 2)], result: (ITEM_ACACIA_PRESSURE_PLATE, 1), name: "acacia_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_ACACIA_PLANKS), (1, 0, ITEM_ACACIA_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_ACACIA_LOG, 6)], result: (ITEM_ACACIA_SHELF, 6), name: "acacia_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_ACACIA_LOG), (1, 0, ITEM_STRIPPED_ACACIA_LOG), (2, 0, ITEM_STRIPPED_ACACIA_LOG), (0, 2, ITEM_STRIPPED_ACACIA_LOG), (1, 2, ITEM_STRIPPED_ACACIA_LOG), (2, 2, ITEM_STRIPPED_ACACIA_LOG)] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6), (STICK, 1)], result: (ITEM_ACACIA_SIGN, 3), name: "acacia_sign", shaped: true, cells: &[(0, 0, ITEM_ACACIA_PLANKS), (1, 0, ITEM_ACACIA_PLANKS), (2, 0, ITEM_ACACIA_PLANKS), (0, 1, ITEM_ACACIA_PLANKS), (1, 1, ITEM_ACACIA_PLANKS), (2, 1, ITEM_ACACIA_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 3)], result: (ITEM_ACACIA_SLAB, 6), name: "acacia_slab", shaped: true, cells: &[(0, 0, ITEM_ACACIA_PLANKS), (1, 0, ITEM_ACACIA_PLANKS), (2, 0, ITEM_ACACIA_PLANKS)] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6)], result: (ITEM_ACACIA_STAIRS, 4), name: "acacia_stairs", shaped: true, cells: &[(0, 0, ITEM_ACACIA_PLANKS), (0, 1, ITEM_ACACIA_PLANKS), (1, 1, ITEM_ACACIA_PLANKS), (0, 2, ITEM_ACACIA_PLANKS), (1, 2, ITEM_ACACIA_PLANKS), (2, 2, ITEM_ACACIA_PLANKS)] },
    Recipe { needs: &[(ITEM_ACACIA_PLANKS, 6)], result: (ITEM_ACACIA_TRAPDOOR, 2), name: "acacia_trapdoor", shaped: true, cells: &[(0, 0, ITEM_ACACIA_PLANKS), (1, 0, ITEM_ACACIA_PLANKS), (2, 0, ITEM_ACACIA_PLANKS), (0, 1, ITEM_ACACIA_PLANKS), (1, 1, ITEM_ACACIA_PLANKS), (2, 1, ITEM_ACACIA_PLANKS)] },
    Recipe { needs: &[(ITEM_ACACIA_LOG, 4)], result: (ITEM_ACACIA_WOOD, 3), name: "acacia_wood", shaped: true, cells: &[(0, 0, ITEM_ACACIA_LOG), (1, 0, ITEM_ACACIA_LOG), (0, 1, ITEM_ACACIA_LOG), (1, 1, ITEM_ACACIA_LOG)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (ITEM_REDSTONE_TORCH, 1), (STICK, 2)], result: (ITEM_ACTIVATOR_RAIL, 6), name: "activator_rail", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, STICK), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_REDSTONE_TORCH), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (1, 2, STICK), (2, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_AMETHYST_SHARD, 4)], result: (ITEM_AMETHYST_BLOCK, 1), name: "amethyst_block", shaped: true, cells: &[(0, 0, ITEM_AMETHYST_SHARD), (1, 0, ITEM_AMETHYST_SHARD), (0, 1, ITEM_AMETHYST_SHARD), (1, 1, ITEM_AMETHYST_SHARD)] },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_DIORITE, 1)], result: (ITEM_ANDESITE, 2), name: "andesite", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ANDESITE, 3)], result: (ITEM_ANDESITE_SLAB, 6), name: "andesite_slab", shaped: true, cells: &[(0, 0, ITEM_ANDESITE), (1, 0, ITEM_ANDESITE), (2, 0, ITEM_ANDESITE)] },
    Recipe { needs: &[(ITEM_ANDESITE, 6)], result: (ITEM_ANDESITE_STAIRS, 4), name: "andesite_stairs", shaped: true, cells: &[(0, 0, ITEM_ANDESITE), (0, 1, ITEM_ANDESITE), (1, 1, ITEM_ANDESITE), (0, 2, ITEM_ANDESITE), (1, 2, ITEM_ANDESITE), (2, 2, ITEM_ANDESITE)] },
    Recipe { needs: &[(ITEM_ANDESITE, 6)], result: (ITEM_ANDESITE_WALL, 6), name: "andesite_wall", shaped: true, cells: &[(0, 0, ITEM_ANDESITE), (1, 0, ITEM_ANDESITE), (2, 0, ITEM_ANDESITE), (0, 1, ITEM_ANDESITE), (1, 1, ITEM_ANDESITE), (2, 1, ITEM_ANDESITE)] },
    Recipe { needs: &[(ITEM_IRON_BLOCK, 3), (ITEM_IRON_INGOT, 4)], result: (ITEM_ANVIL, 1), name: "anvil", shaped: true, cells: &[(0, 0, ITEM_IRON_BLOCK), (1, 0, ITEM_IRON_BLOCK), (2, 0, ITEM_IRON_BLOCK), (1, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (1, 2, ITEM_IRON_INGOT), (2, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_SMOOTH_STONE_SLAB, 1), (STICK, 6)], result: (ITEM_ARMOR_STAND, 1), name: "armor_stand", shaped: true, cells: &[(0, 0, STICK), (1, 0, STICK), (2, 0, STICK), (1, 1, STICK), (0, 2, STICK), (1, 2, ITEM_SMOOTH_STONE_SLAB), (2, 2, STICK)] },
    Recipe { needs: &[(ITEM_FEATHER, 1), (ITEM_FLINT, 1), (STICK, 1)], result: (ITEM_ARROW, 4), name: "arrow", shaped: true, cells: &[(0, 0, ITEM_FLINT), (0, 1, STICK), (0, 2, ITEM_FEATHER)] },
    Recipe { needs: &[(ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1), (ITEM_BAMBOO, 1)], result: (ITEM_BAMBOO_BLOCK, 1), name: "bamboo_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 1)], result: (ITEM_BAMBOO_BUTTON, 1), name: "bamboo_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BAMBOO_RAFT, 1), (ITEM_CHEST, 1)], result: (ITEM_BAMBOO_CHEST_RAFT, 1), name: "bamboo_chest_raft", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 6)], result: (ITEM_BAMBOO_DOOR, 3), name: "bamboo_door", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_PLANKS), (1, 0, ITEM_BAMBOO_PLANKS), (0, 1, ITEM_BAMBOO_PLANKS), (1, 1, ITEM_BAMBOO_PLANKS), (0, 2, ITEM_BAMBOO_PLANKS), (1, 2, ITEM_BAMBOO_PLANKS)] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 4), (STICK, 2)], result: (ITEM_BAMBOO_FENCE, 3), name: "bamboo_fence", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_PLANKS), (1, 0, STICK), (2, 0, ITEM_BAMBOO_PLANKS), (0, 1, ITEM_BAMBOO_PLANKS), (1, 1, STICK), (2, 1, ITEM_BAMBOO_PLANKS)] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 2), (STICK, 4)], result: (ITEM_BAMBOO_FENCE_GATE, 1), name: "bamboo_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_BAMBOO_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_BAMBOO_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_BAMBOO_BLOCK, 6)], result: (ITEM_BAMBOO_HANGING_SIGN, 6), name: "bamboo_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_BAMBOO_BLOCK), (1, 1, ITEM_STRIPPED_BAMBOO_BLOCK), (2, 1, ITEM_STRIPPED_BAMBOO_BLOCK), (0, 2, ITEM_STRIPPED_BAMBOO_BLOCK), (1, 2, ITEM_STRIPPED_BAMBOO_BLOCK), (2, 2, ITEM_STRIPPED_BAMBOO_BLOCK)] },
    Recipe { needs: &[(ITEM_BAMBOO_SLAB, 2)], result: (ITEM_BAMBOO_MOSAIC, 1), name: "bamboo_mosaic", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_SLAB), (0, 1, ITEM_BAMBOO_SLAB)] },
    Recipe { needs: &[(ITEM_BAMBOO_MOSAIC, 3)], result: (ITEM_BAMBOO_MOSAIC_SLAB, 6), name: "bamboo_mosaic_slab", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_MOSAIC), (1, 0, ITEM_BAMBOO_MOSAIC), (2, 0, ITEM_BAMBOO_MOSAIC)] },
    Recipe { needs: &[(ITEM_BAMBOO_MOSAIC, 6)], result: (ITEM_BAMBOO_MOSAIC_STAIRS, 4), name: "bamboo_mosaic_stairs", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_MOSAIC), (0, 1, ITEM_BAMBOO_MOSAIC), (1, 1, ITEM_BAMBOO_MOSAIC), (0, 2, ITEM_BAMBOO_MOSAIC), (1, 2, ITEM_BAMBOO_MOSAIC), (2, 2, ITEM_BAMBOO_MOSAIC)] },
    Recipe { needs: &[(TAG_BAMBOO_BLOCKS, 1)], result: (ITEM_BAMBOO_PLANKS, 2), name: "bamboo_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 2)], result: (ITEM_BAMBOO_PRESSURE_PLATE, 1), name: "bamboo_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_PLANKS), (1, 0, ITEM_BAMBOO_PLANKS)] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 5)], result: (ITEM_BAMBOO_RAFT, 1), name: "bamboo_raft", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_PLANKS), (2, 0, ITEM_BAMBOO_PLANKS), (0, 1, ITEM_BAMBOO_PLANKS), (1, 1, ITEM_BAMBOO_PLANKS), (2, 1, ITEM_BAMBOO_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_BAMBOO_BLOCK, 6)], result: (ITEM_BAMBOO_SHELF, 6), name: "bamboo_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_BAMBOO_BLOCK), (1, 0, ITEM_STRIPPED_BAMBOO_BLOCK), (2, 0, ITEM_STRIPPED_BAMBOO_BLOCK), (0, 2, ITEM_STRIPPED_BAMBOO_BLOCK), (1, 2, ITEM_STRIPPED_BAMBOO_BLOCK), (2, 2, ITEM_STRIPPED_BAMBOO_BLOCK)] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 6), (STICK, 1)], result: (ITEM_BAMBOO_SIGN, 3), name: "bamboo_sign", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_PLANKS), (1, 0, ITEM_BAMBOO_PLANKS), (2, 0, ITEM_BAMBOO_PLANKS), (0, 1, ITEM_BAMBOO_PLANKS), (1, 1, ITEM_BAMBOO_PLANKS), (2, 1, ITEM_BAMBOO_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 3)], result: (ITEM_BAMBOO_SLAB, 6), name: "bamboo_slab", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_PLANKS), (1, 0, ITEM_BAMBOO_PLANKS), (2, 0, ITEM_BAMBOO_PLANKS)] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 6)], result: (ITEM_BAMBOO_STAIRS, 4), name: "bamboo_stairs", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_PLANKS), (0, 1, ITEM_BAMBOO_PLANKS), (1, 1, ITEM_BAMBOO_PLANKS), (0, 2, ITEM_BAMBOO_PLANKS), (1, 2, ITEM_BAMBOO_PLANKS), (2, 2, ITEM_BAMBOO_PLANKS)] },
    Recipe { needs: &[(ITEM_BAMBOO_PLANKS, 6)], result: (ITEM_BAMBOO_TRAPDOOR, 2), name: "bamboo_trapdoor", shaped: true, cells: &[(0, 0, ITEM_BAMBOO_PLANKS), (1, 0, ITEM_BAMBOO_PLANKS), (2, 0, ITEM_BAMBOO_PLANKS), (0, 1, ITEM_BAMBOO_PLANKS), (1, 1, ITEM_BAMBOO_PLANKS), (2, 1, ITEM_BAMBOO_PLANKS)] },
    Recipe { needs: &[(TAG_PLANKS, 6), (TAG_WOODEN_SLABS, 2)], result: (ITEM_BARREL, 1), name: "barrel", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_WOODEN_SLABS), (2, 0, TAG_PLANKS), (0, 1, TAG_PLANKS), (2, 1, TAG_PLANKS), (0, 2, TAG_PLANKS), (1, 2, TAG_WOODEN_SLABS), (2, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_GLASS, 5), (ITEM_NETHER_STAR, 1), (ITEM_OBSIDIAN, 3)], result: (ITEM_BEACON, 1), name: "beacon", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_NETHER_STAR), (2, 1, ITEM_GLASS), (0, 2, ITEM_OBSIDIAN), (1, 2, ITEM_OBSIDIAN), (2, 2, ITEM_OBSIDIAN)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 3), (TAG_PLANKS, 6)], result: (ITEM_BEEHIVE, 1), name: "beehive", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (0, 1, ITEM_HONEYCOMB), (1, 1, ITEM_HONEYCOMB), (2, 1, ITEM_HONEYCOMB), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS), (2, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BEETROOT, 1), (ITEM_BOWL, 1)], result: (ITEM_BEETROOT_SOUP, 1), name: "beetroot_soup", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 5)], result: (ITEM_BIRCH_BOAT, 1), name: "birch_boat", shaped: true, cells: &[(0, 0, ITEM_BIRCH_PLANKS), (2, 0, ITEM_BIRCH_PLANKS), (0, 1, ITEM_BIRCH_PLANKS), (1, 1, ITEM_BIRCH_PLANKS), (2, 1, ITEM_BIRCH_PLANKS)] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 1)], result: (ITEM_BIRCH_BUTTON, 1), name: "birch_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BIRCH_BOAT, 1), (ITEM_CHEST, 1)], result: (ITEM_BIRCH_CHEST_BOAT, 1), name: "birch_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6)], result: (ITEM_BIRCH_DOOR, 3), name: "birch_door", shaped: true, cells: &[(0, 0, ITEM_BIRCH_PLANKS), (1, 0, ITEM_BIRCH_PLANKS), (0, 1, ITEM_BIRCH_PLANKS), (1, 1, ITEM_BIRCH_PLANKS), (0, 2, ITEM_BIRCH_PLANKS), (1, 2, ITEM_BIRCH_PLANKS)] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 4), (STICK, 2)], result: (ITEM_BIRCH_FENCE, 3), name: "birch_fence", shaped: true, cells: &[(0, 0, ITEM_BIRCH_PLANKS), (1, 0, STICK), (2, 0, ITEM_BIRCH_PLANKS), (0, 1, ITEM_BIRCH_PLANKS), (1, 1, STICK), (2, 1, ITEM_BIRCH_PLANKS)] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 2), (STICK, 4)], result: (ITEM_BIRCH_FENCE_GATE, 1), name: "birch_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_BIRCH_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_BIRCH_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_BIRCH_LOG, 6)], result: (ITEM_BIRCH_HANGING_SIGN, 6), name: "birch_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_BIRCH_LOG), (1, 1, ITEM_STRIPPED_BIRCH_LOG), (2, 1, ITEM_STRIPPED_BIRCH_LOG), (0, 2, ITEM_STRIPPED_BIRCH_LOG), (1, 2, ITEM_STRIPPED_BIRCH_LOG), (2, 2, ITEM_STRIPPED_BIRCH_LOG)] },
    Recipe { needs: &[(TAG_BIRCH_LOGS, 1)], result: (ITEM_BIRCH_PLANKS, 4), name: "birch_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 2)], result: (ITEM_BIRCH_PRESSURE_PLATE, 1), name: "birch_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_BIRCH_PLANKS), (1, 0, ITEM_BIRCH_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_BIRCH_LOG, 6)], result: (ITEM_BIRCH_SHELF, 6), name: "birch_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_BIRCH_LOG), (1, 0, ITEM_STRIPPED_BIRCH_LOG), (2, 0, ITEM_STRIPPED_BIRCH_LOG), (0, 2, ITEM_STRIPPED_BIRCH_LOG), (1, 2, ITEM_STRIPPED_BIRCH_LOG), (2, 2, ITEM_STRIPPED_BIRCH_LOG)] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6), (STICK, 1)], result: (ITEM_BIRCH_SIGN, 3), name: "birch_sign", shaped: true, cells: &[(0, 0, ITEM_BIRCH_PLANKS), (1, 0, ITEM_BIRCH_PLANKS), (2, 0, ITEM_BIRCH_PLANKS), (0, 1, ITEM_BIRCH_PLANKS), (1, 1, ITEM_BIRCH_PLANKS), (2, 1, ITEM_BIRCH_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 3)], result: (ITEM_BIRCH_SLAB, 6), name: "birch_slab", shaped: true, cells: &[(0, 0, ITEM_BIRCH_PLANKS), (1, 0, ITEM_BIRCH_PLANKS), (2, 0, ITEM_BIRCH_PLANKS)] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6)], result: (ITEM_BIRCH_STAIRS, 4), name: "birch_stairs", shaped: true, cells: &[(0, 0, ITEM_BIRCH_PLANKS), (0, 1, ITEM_BIRCH_PLANKS), (1, 1, ITEM_BIRCH_PLANKS), (0, 2, ITEM_BIRCH_PLANKS), (1, 2, ITEM_BIRCH_PLANKS), (2, 2, ITEM_BIRCH_PLANKS)] },
    Recipe { needs: &[(ITEM_BIRCH_PLANKS, 6)], result: (ITEM_BIRCH_TRAPDOOR, 2), name: "birch_trapdoor", shaped: true, cells: &[(0, 0, ITEM_BIRCH_PLANKS), (1, 0, ITEM_BIRCH_PLANKS), (2, 0, ITEM_BIRCH_PLANKS), (0, 1, ITEM_BIRCH_PLANKS), (1, 1, ITEM_BIRCH_PLANKS), (2, 1, ITEM_BIRCH_PLANKS)] },
    Recipe { needs: &[(ITEM_BIRCH_LOG, 4)], result: (ITEM_BIRCH_WOOD, 3), name: "birch_wood", shaped: true, cells: &[(0, 0, ITEM_BIRCH_LOG), (1, 0, ITEM_BIRCH_LOG), (0, 1, ITEM_BIRCH_LOG), (1, 1, ITEM_BIRCH_LOG)] },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 6), (STICK, 1)], result: (ITEM_BLACK_BANNER, 1), name: "black_banner", shaped: true, cells: &[(0, 0, ITEM_BLACK_WOOL), (1, 0, ITEM_BLACK_WOOL), (2, 0, ITEM_BLACK_WOOL), (0, 1, ITEM_BLACK_WOOL), (1, 1, ITEM_BLACK_WOOL), (2, 1, ITEM_BLACK_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_BLACK_BED, 1), name: "black_bed", shaped: true, cells: &[(0, 0, ITEM_BLACK_WOOL), (1, 0, ITEM_BLACK_WOOL), (2, 0, ITEM_BLACK_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_CANDLE, 1)], result: (ITEM_BLACK_CANDLE, 1), name: "black_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 2)], result: (ITEM_BLACK_CARPET, 3), name: "black_carpet", shaped: true, cells: &[(0, 0, ITEM_BLACK_WOOL), (1, 0, ITEM_BLACK_WOOL)] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_BLACK_CONCRETE_POWDER, 8), name: "black_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_CONCRETE, 3)], result: (ITEM_BLACK_CONCRETE_SLAB, 6), name: "black_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_BLACK_CONCRETE), (1, 0, ITEM_BLACK_CONCRETE), (2, 0, ITEM_BLACK_CONCRETE)] },
    Recipe { needs: &[(ITEM_BLACK_CONCRETE, 6)], result: (ITEM_BLACK_CONCRETE_STAIRS, 4), name: "black_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_BLACK_CONCRETE), (0, 1, ITEM_BLACK_CONCRETE), (1, 1, ITEM_BLACK_CONCRETE), (0, 2, ITEM_BLACK_CONCRETE), (1, 2, ITEM_BLACK_CONCRETE), (2, 2, ITEM_BLACK_CONCRETE)] },
    Recipe { needs: &[(ITEM_BLACK_WOOL_SLAB, 3)], result: (ITEM_BLACK_CUSHION, 1), name: "black_cushion", shaped: true, cells: &[(0, 0, ITEM_BLACK_WOOL_SLAB), (1, 0, ITEM_BLACK_WOOL_SLAB), (2, 0, ITEM_BLACK_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_INK_SAC, 1)], result: (ITEM_BLACK_DYE, 1), name: "black_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WITHER_ROSE, 1)], result: (ITEM_BLACK_DYE, 1), name: "black_dye_from_wither_rose", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 1), (ITEM_GLASS, 2), (ITEM_LEATHER, 3)], result: (ITEM_BLACK_HARNESS, 1), name: "black_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_BLACK_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_GLASS, 8)], result: (ITEM_BLACK_STAINED_GLASS, 8), name: "black_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_BLACK_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_BLACK_STAINED_GLASS, 6)], result: (ITEM_BLACK_STAINED_GLASS_PANE, 16), name: "black_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_BLACK_STAINED_GLASS), (1, 0, ITEM_BLACK_STAINED_GLASS), (2, 0, ITEM_BLACK_STAINED_GLASS), (0, 1, ITEM_BLACK_STAINED_GLASS), (1, 1, ITEM_BLACK_STAINED_GLASS), (2, 1, ITEM_BLACK_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_GLASS_PANE, 8)], result: (ITEM_BLACK_STAINED_GLASS_PANE, 8), name: "black_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_BLACK_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_BLACK_TERRACOTTA, 8), name: "black_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_BLACK_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 3)], result: (ITEM_BLACK_WOOL_SLAB, 6), name: "black_wool_slab", shaped: true, cells: &[(0, 0, ITEM_BLACK_WOOL), (1, 0, ITEM_BLACK_WOOL), (2, 0, ITEM_BLACK_WOOL)] },
    Recipe { needs: &[(ITEM_BLACK_WOOL, 6)], result: (ITEM_BLACK_WOOL_STAIRS, 4), name: "black_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_BLACK_WOOL), (0, 1, ITEM_BLACK_WOOL), (1, 1, ITEM_BLACK_WOOL), (0, 2, ITEM_BLACK_WOOL), (1, 2, ITEM_BLACK_WOOL), (2, 2, ITEM_BLACK_WOOL)] },
    Recipe { needs: &[(ITEM_BLACKSTONE, 3)], result: (ITEM_BLACKSTONE_SLAB, 6), name: "blackstone_slab", shaped: true, cells: &[(0, 0, ITEM_BLACKSTONE), (1, 0, ITEM_BLACKSTONE), (2, 0, ITEM_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_BLACKSTONE, 6)], result: (ITEM_BLACKSTONE_STAIRS, 4), name: "blackstone_stairs", shaped: true, cells: &[(0, 0, ITEM_BLACKSTONE), (0, 1, ITEM_BLACKSTONE), (1, 1, ITEM_BLACKSTONE), (0, 2, ITEM_BLACKSTONE), (1, 2, ITEM_BLACKSTONE), (2, 2, ITEM_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_BLACKSTONE, 6)], result: (ITEM_BLACKSTONE_WALL, 6), name: "blackstone_wall", shaped: true, cells: &[(0, 0, ITEM_BLACKSTONE), (1, 0, ITEM_BLACKSTONE), (2, 0, ITEM_BLACKSTONE), (0, 1, ITEM_BLACKSTONE), (1, 1, ITEM_BLACKSTONE), (2, 1, ITEM_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_FURNACE, 1), (ITEM_IRON_INGOT, 5), (ITEM_SMOOTH_STONE, 3)], result: (ITEM_BLAST_FURNACE, 1), name: "blast_furnace", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_FURNACE), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_SMOOTH_STONE), (1, 2, ITEM_SMOOTH_STONE), (2, 2, ITEM_SMOOTH_STONE)] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1)], result: (ITEM_BLAZE_POWDER, 2), name: "blaze_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 6), (STICK, 1)], result: (ITEM_BLUE_BANNER, 1), name: "blue_banner", shaped: true, cells: &[(0, 0, ITEM_BLUE_WOOL), (1, 0, ITEM_BLUE_WOOL), (2, 0, ITEM_BLUE_WOOL), (0, 1, ITEM_BLUE_WOOL), (1, 1, ITEM_BLUE_WOOL), (2, 1, ITEM_BLUE_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_BLUE_BED, 1), name: "blue_bed", shaped: true, cells: &[(0, 0, ITEM_BLUE_WOOL), (1, 0, ITEM_BLUE_WOOL), (2, 0, ITEM_BLUE_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_CANDLE, 1)], result: (ITEM_BLUE_CANDLE, 1), name: "blue_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 2)], result: (ITEM_BLUE_CARPET, 3), name: "blue_carpet", shaped: true, cells: &[(0, 0, ITEM_BLUE_WOOL), (1, 0, ITEM_BLUE_WOOL)] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_BLUE_CONCRETE_POWDER, 8), name: "blue_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_CONCRETE, 3)], result: (ITEM_BLUE_CONCRETE_SLAB, 6), name: "blue_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_BLUE_CONCRETE), (1, 0, ITEM_BLUE_CONCRETE), (2, 0, ITEM_BLUE_CONCRETE)] },
    Recipe { needs: &[(ITEM_BLUE_CONCRETE, 6)], result: (ITEM_BLUE_CONCRETE_STAIRS, 4), name: "blue_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_BLUE_CONCRETE), (0, 1, ITEM_BLUE_CONCRETE), (1, 1, ITEM_BLUE_CONCRETE), (0, 2, ITEM_BLUE_CONCRETE), (1, 2, ITEM_BLUE_CONCRETE), (2, 2, ITEM_BLUE_CONCRETE)] },
    Recipe { needs: &[(ITEM_BLUE_WOOL_SLAB, 3)], result: (ITEM_BLUE_CUSHION, 1), name: "blue_cushion", shaped: true, cells: &[(0, 0, ITEM_BLUE_WOOL_SLAB), (1, 0, ITEM_BLUE_WOOL_SLAB), (2, 0, ITEM_BLUE_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_LAPIS_LAZULI, 1)], result: (ITEM_BLUE_DYE, 1), name: "blue_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CORNFLOWER, 1)], result: (ITEM_BLUE_DYE, 1), name: "blue_dye_from_cornflower", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 1), (ITEM_GLASS, 2), (ITEM_LEATHER, 3)], result: (ITEM_BLUE_HARNESS, 1), name: "blue_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_BLUE_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1), (ITEM_PACKED_ICE, 1)], result: (ITEM_BLUE_ICE, 1), name: "blue_ice", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_GLASS, 8)], result: (ITEM_BLUE_STAINED_GLASS, 8), name: "blue_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_BLUE_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_BLUE_STAINED_GLASS, 6)], result: (ITEM_BLUE_STAINED_GLASS_PANE, 16), name: "blue_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_BLUE_STAINED_GLASS), (1, 0, ITEM_BLUE_STAINED_GLASS), (2, 0, ITEM_BLUE_STAINED_GLASS), (0, 1, ITEM_BLUE_STAINED_GLASS), (1, 1, ITEM_BLUE_STAINED_GLASS), (2, 1, ITEM_BLUE_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_GLASS_PANE, 8)], result: (ITEM_BLUE_STAINED_GLASS_PANE, 8), name: "blue_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_BLUE_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_BLUE_TERRACOTTA, 8), name: "blue_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_BLUE_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 3)], result: (ITEM_BLUE_WOOL_SLAB, 6), name: "blue_wool_slab", shaped: true, cells: &[(0, 0, ITEM_BLUE_WOOL), (1, 0, ITEM_BLUE_WOOL), (2, 0, ITEM_BLUE_WOOL)] },
    Recipe { needs: &[(ITEM_BLUE_WOOL, 6)], result: (ITEM_BLUE_WOOL_STAIRS, 4), name: "blue_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_BLUE_WOOL), (0, 1, ITEM_BLUE_WOOL), (1, 1, ITEM_BLUE_WOOL), (0, 2, ITEM_BLUE_WOOL), (1, 2, ITEM_BLUE_WOOL), (2, 2, ITEM_BLUE_WOOL)] },
    Recipe { needs: &[(ITEM_BOLT_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_DIAMOND, 7), (TAG_ALT_0, 1)], result: (ITEM_BOLT_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "bolt_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_BOLT_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, TAG_ALT_0), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_BONE_MEAL, 9)], result: (ITEM_BONE_BLOCK, 1), name: "bone_block", shaped: true, cells: &[(0, 0, ITEM_BONE_MEAL), (1, 0, ITEM_BONE_MEAL), (2, 0, ITEM_BONE_MEAL), (0, 1, ITEM_BONE_MEAL), (1, 1, ITEM_BONE_MEAL), (2, 1, ITEM_BONE_MEAL), (0, 2, ITEM_BONE_MEAL), (1, 2, ITEM_BONE_MEAL), (2, 2, ITEM_BONE_MEAL)] },
    Recipe { needs: &[(ITEM_BONE, 1)], result: (ITEM_BONE_MEAL, 3), name: "bone_meal", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BONE_BLOCK, 1)], result: (ITEM_BONE_MEAL, 9), name: "bone_meal_from_bone_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LEATHER, 1), (ITEM_PAPER, 1), (ITEM_PAPER, 1), (ITEM_PAPER, 1)], result: (ITEM_BOOK, 1), name: "book", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BOOK, 3), (TAG_PLANKS, 6)], result: (ITEM_BOOKSHELF, 1), name: "bookshelf", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (0, 1, ITEM_BOOK), (1, 1, ITEM_BOOK), (2, 1, ITEM_BOOK), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS), (2, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_PAPER, 1), (ITEM_VINE, 1)], result: (ITEM_BORDURE_INDENTED_BANNER_PATTERN, 1), name: "bordure_indented_banner_pattern", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 3), (ITEM_STRING, 3)], result: (ITEM_BOW, 1), name: "bow", shaped: true, cells: &[(1, 0, STICK), (2, 0, ITEM_STRING), (0, 1, STICK), (2, 1, ITEM_STRING), (1, 2, STICK), (2, 2, ITEM_STRING)] },
    Recipe { needs: &[(TAG_PLANKS, 3)], result: (ITEM_BOWL, 4), name: "bowl", shaped: true, cells: &[(0, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (1, 1, TAG_PLANKS)] },
    Recipe { needs: &[(WHEAT, 3)], result: (BREAD, 1), name: "bread", shaped: true, cells: &[(0, 0, WHEAT), (1, 0, WHEAT), (2, 0, WHEAT)] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (TAG_STONE_CRAFTING_MATERIALS, 3)], result: (ITEM_BREWING_STAND, 1), name: "brewing_stand", shaped: true, cells: &[(1, 0, ITEM_BLAZE_ROD), (0, 1, TAG_STONE_CRAFTING_MATERIALS), (1, 1, TAG_STONE_CRAFTING_MATERIALS), (2, 1, TAG_STONE_CRAFTING_MATERIALS)] },
    Recipe { needs: &[(ITEM_BRICKS, 3)], result: (ITEM_BRICK_SLAB, 6), name: "brick_slab", shaped: true, cells: &[(0, 0, ITEM_BRICKS), (1, 0, ITEM_BRICKS), (2, 0, ITEM_BRICKS)] },
    Recipe { needs: &[(ITEM_BRICKS, 6)], result: (ITEM_BRICK_STAIRS, 4), name: "brick_stairs", shaped: true, cells: &[(0, 0, ITEM_BRICKS), (0, 1, ITEM_BRICKS), (1, 1, ITEM_BRICKS), (0, 2, ITEM_BRICKS), (1, 2, ITEM_BRICKS), (2, 2, ITEM_BRICKS)] },
    Recipe { needs: &[(ITEM_BRICKS, 6)], result: (ITEM_BRICK_WALL, 6), name: "brick_wall", shaped: true, cells: &[(0, 0, ITEM_BRICKS), (1, 0, ITEM_BRICKS), (2, 0, ITEM_BRICKS), (0, 1, ITEM_BRICKS), (1, 1, ITEM_BRICKS), (2, 1, ITEM_BRICKS)] },
    Recipe { needs: &[(ITEM_BRICK, 4)], result: (ITEM_BRICKS, 1), name: "bricks", shaped: true, cells: &[(0, 0, ITEM_BRICK), (1, 0, ITEM_BRICK), (0, 1, ITEM_BRICK), (1, 1, ITEM_BRICK)] },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 6), (STICK, 1)], result: (ITEM_BROWN_BANNER, 1), name: "brown_banner", shaped: true, cells: &[(0, 0, ITEM_BROWN_WOOL), (1, 0, ITEM_BROWN_WOOL), (2, 0, ITEM_BROWN_WOOL), (0, 1, ITEM_BROWN_WOOL), (1, 1, ITEM_BROWN_WOOL), (2, 1, ITEM_BROWN_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_BROWN_BED, 1), name: "brown_bed", shaped: true, cells: &[(0, 0, ITEM_BROWN_WOOL), (1, 0, ITEM_BROWN_WOOL), (2, 0, ITEM_BROWN_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_CANDLE, 1)], result: (ITEM_BROWN_CANDLE, 1), name: "brown_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 2)], result: (ITEM_BROWN_CARPET, 3), name: "brown_carpet", shaped: true, cells: &[(0, 0, ITEM_BROWN_WOOL), (1, 0, ITEM_BROWN_WOOL)] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_BROWN_CONCRETE_POWDER, 8), name: "brown_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_CONCRETE, 3)], result: (ITEM_BROWN_CONCRETE_SLAB, 6), name: "brown_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_BROWN_CONCRETE), (1, 0, ITEM_BROWN_CONCRETE), (2, 0, ITEM_BROWN_CONCRETE)] },
    Recipe { needs: &[(ITEM_BROWN_CONCRETE, 6)], result: (ITEM_BROWN_CONCRETE_STAIRS, 4), name: "brown_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_BROWN_CONCRETE), (0, 1, ITEM_BROWN_CONCRETE), (1, 1, ITEM_BROWN_CONCRETE), (0, 2, ITEM_BROWN_CONCRETE), (1, 2, ITEM_BROWN_CONCRETE), (2, 2, ITEM_BROWN_CONCRETE)] },
    Recipe { needs: &[(ITEM_BROWN_WOOL_SLAB, 3)], result: (ITEM_BROWN_CUSHION, 1), name: "brown_cushion", shaped: true, cells: &[(0, 0, ITEM_BROWN_WOOL_SLAB), (1, 0, ITEM_BROWN_WOOL_SLAB), (2, 0, ITEM_BROWN_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_COCOA_BEANS, 1)], result: (ITEM_BROWN_DYE, 1), name: "brown_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 1), (ITEM_GLASS, 2), (ITEM_LEATHER, 3)], result: (ITEM_BROWN_HARNESS, 1), name: "brown_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_BROWN_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_GLASS, 8)], result: (ITEM_BROWN_STAINED_GLASS, 8), name: "brown_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_BROWN_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_BROWN_STAINED_GLASS, 6)], result: (ITEM_BROWN_STAINED_GLASS_PANE, 16), name: "brown_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_BROWN_STAINED_GLASS), (1, 0, ITEM_BROWN_STAINED_GLASS), (2, 0, ITEM_BROWN_STAINED_GLASS), (0, 1, ITEM_BROWN_STAINED_GLASS), (1, 1, ITEM_BROWN_STAINED_GLASS), (2, 1, ITEM_BROWN_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_GLASS_PANE, 8)], result: (ITEM_BROWN_STAINED_GLASS_PANE, 8), name: "brown_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_BROWN_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_BROWN_TERRACOTTA, 8), name: "brown_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_BROWN_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 3)], result: (ITEM_BROWN_WOOL_SLAB, 6), name: "brown_wool_slab", shaped: true, cells: &[(0, 0, ITEM_BROWN_WOOL), (1, 0, ITEM_BROWN_WOOL), (2, 0, ITEM_BROWN_WOOL)] },
    Recipe { needs: &[(ITEM_BROWN_WOOL, 6)], result: (ITEM_BROWN_WOOL_STAIRS, 4), name: "brown_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_BROWN_WOOL), (0, 1, ITEM_BROWN_WOOL), (1, 1, ITEM_BROWN_WOOL), (0, 2, ITEM_BROWN_WOOL), (1, 2, ITEM_BROWN_WOOL), (2, 2, ITEM_BROWN_WOOL)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 1), (ITEM_FEATHER, 1), (STICK, 1)], result: (ITEM_BRUSH, 1), name: "brush", shaped: true, cells: &[(0, 0, ITEM_FEATHER), (0, 1, ITEM_COPPER_INGOT), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 3)], result: (ITEM_BUCKET, 1), name: "bucket", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (1, 1, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_LEATHER, 1), (ITEM_STRING, 1)], result: (ITEM_BUNDLE, 1), name: "bundle", shaped: true, cells: &[(0, 0, ITEM_STRING), (0, 1, ITEM_LEATHER)] },
    Recipe { needs: &[(ITEM_MILK_BUCKET, 3), (ITEM_SUGAR, 2), (WHEAT, 3), (TAG_EGGS, 1)], result: (ITEM_CAKE, 1), name: "cake", shaped: true, cells: &[(0, 0, ITEM_MILK_BUCKET), (1, 0, ITEM_MILK_BUCKET), (2, 0, ITEM_MILK_BUCKET), (0, 1, ITEM_SUGAR), (1, 1, TAG_EGGS), (2, 1, ITEM_SUGAR), (0, 2, WHEAT), (1, 2, WHEAT), (2, 2, WHEAT)] },
    Recipe { needs: &[(ITEM_AMETHYST_SHARD, 3), (ITEM_SCULK_SENSOR, 1)], result: (ITEM_CALIBRATED_SCULK_SENSOR, 1), name: "calibrated_sculk_sensor", shaped: true, cells: &[(1, 0, ITEM_AMETHYST_SHARD), (0, 1, ITEM_AMETHYST_SHARD), (1, 1, ITEM_SCULK_SENSOR), (2, 1, ITEM_AMETHYST_SHARD)] },
    Recipe { needs: &[(STICK, 3), (TAG_LOGS, 3), (TAG_COALS, 1)], result: (ITEM_CAMPFIRE, 1), name: "campfire", shaped: true, cells: &[(1, 0, STICK), (0, 1, STICK), (1, 1, TAG_COALS), (2, 1, STICK), (0, 2, TAG_LOGS), (1, 2, TAG_LOGS), (2, 2, TAG_LOGS)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_STRING, 1)], result: (ITEM_CANDLE, 1), name: "candle", shaped: true, cells: &[(0, 0, ITEM_STRING), (0, 1, ITEM_HONEYCOMB)] },
    Recipe { needs: &[(ITEM_CARROT, 1), (ITEM_FISHING_ROD, 1)], result: (ITEM_CARROT_ON_A_STICK, 1), name: "carrot_on_a_stick", shaped: true, cells: &[(0, 0, ITEM_FISHING_ROD), (1, 1, ITEM_CARROT)] },
    Recipe { needs: &[(ITEM_PAPER, 2), (TAG_PLANKS, 4)], result: (ITEM_CARTOGRAPHY_TABLE, 1), name: "cartography_table", shaped: true, cells: &[(0, 0, ITEM_PAPER), (1, 0, ITEM_PAPER), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 7)], result: (ITEM_CAULDRON, 1), name: "cauldron", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (1, 2, ITEM_IRON_INGOT), (2, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 5)], result: (ITEM_CHERRY_BOAT, 1), name: "cherry_boat", shaped: true, cells: &[(0, 0, ITEM_CHERRY_PLANKS), (2, 0, ITEM_CHERRY_PLANKS), (0, 1, ITEM_CHERRY_PLANKS), (1, 1, ITEM_CHERRY_PLANKS), (2, 1, ITEM_CHERRY_PLANKS)] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 1)], result: (ITEM_CHERRY_BUTTON, 1), name: "cherry_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHERRY_BOAT, 1), (ITEM_CHEST, 1)], result: (ITEM_CHERRY_CHEST_BOAT, 1), name: "cherry_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6)], result: (ITEM_CHERRY_DOOR, 3), name: "cherry_door", shaped: true, cells: &[(0, 0, ITEM_CHERRY_PLANKS), (1, 0, ITEM_CHERRY_PLANKS), (0, 1, ITEM_CHERRY_PLANKS), (1, 1, ITEM_CHERRY_PLANKS), (0, 2, ITEM_CHERRY_PLANKS), (1, 2, ITEM_CHERRY_PLANKS)] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 4), (STICK, 2)], result: (ITEM_CHERRY_FENCE, 3), name: "cherry_fence", shaped: true, cells: &[(0, 0, ITEM_CHERRY_PLANKS), (1, 0, STICK), (2, 0, ITEM_CHERRY_PLANKS), (0, 1, ITEM_CHERRY_PLANKS), (1, 1, STICK), (2, 1, ITEM_CHERRY_PLANKS)] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 2), (STICK, 4)], result: (ITEM_CHERRY_FENCE_GATE, 1), name: "cherry_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_CHERRY_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_CHERRY_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_CHERRY_LOG, 6)], result: (ITEM_CHERRY_HANGING_SIGN, 6), name: "cherry_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_CHERRY_LOG), (1, 1, ITEM_STRIPPED_CHERRY_LOG), (2, 1, ITEM_STRIPPED_CHERRY_LOG), (0, 2, ITEM_STRIPPED_CHERRY_LOG), (1, 2, ITEM_STRIPPED_CHERRY_LOG), (2, 2, ITEM_STRIPPED_CHERRY_LOG)] },
    Recipe { needs: &[(TAG_CHERRY_LOGS, 1)], result: (ITEM_CHERRY_PLANKS, 4), name: "cherry_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 2)], result: (ITEM_CHERRY_PRESSURE_PLATE, 1), name: "cherry_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_CHERRY_PLANKS), (1, 0, ITEM_CHERRY_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_CHERRY_LOG, 6)], result: (ITEM_CHERRY_SHELF, 6), name: "cherry_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_CHERRY_LOG), (1, 0, ITEM_STRIPPED_CHERRY_LOG), (2, 0, ITEM_STRIPPED_CHERRY_LOG), (0, 2, ITEM_STRIPPED_CHERRY_LOG), (1, 2, ITEM_STRIPPED_CHERRY_LOG), (2, 2, ITEM_STRIPPED_CHERRY_LOG)] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6), (STICK, 1)], result: (ITEM_CHERRY_SIGN, 3), name: "cherry_sign", shaped: true, cells: &[(0, 0, ITEM_CHERRY_PLANKS), (1, 0, ITEM_CHERRY_PLANKS), (2, 0, ITEM_CHERRY_PLANKS), (0, 1, ITEM_CHERRY_PLANKS), (1, 1, ITEM_CHERRY_PLANKS), (2, 1, ITEM_CHERRY_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 3)], result: (ITEM_CHERRY_SLAB, 6), name: "cherry_slab", shaped: true, cells: &[(0, 0, ITEM_CHERRY_PLANKS), (1, 0, ITEM_CHERRY_PLANKS), (2, 0, ITEM_CHERRY_PLANKS)] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6)], result: (ITEM_CHERRY_STAIRS, 4), name: "cherry_stairs", shaped: true, cells: &[(0, 0, ITEM_CHERRY_PLANKS), (0, 1, ITEM_CHERRY_PLANKS), (1, 1, ITEM_CHERRY_PLANKS), (0, 2, ITEM_CHERRY_PLANKS), (1, 2, ITEM_CHERRY_PLANKS), (2, 2, ITEM_CHERRY_PLANKS)] },
    Recipe { needs: &[(ITEM_CHERRY_PLANKS, 6)], result: (ITEM_CHERRY_TRAPDOOR, 2), name: "cherry_trapdoor", shaped: true, cells: &[(0, 0, ITEM_CHERRY_PLANKS), (1, 0, ITEM_CHERRY_PLANKS), (2, 0, ITEM_CHERRY_PLANKS), (0, 1, ITEM_CHERRY_PLANKS), (1, 1, ITEM_CHERRY_PLANKS), (2, 1, ITEM_CHERRY_PLANKS)] },
    Recipe { needs: &[(ITEM_CHERRY_LOG, 4)], result: (ITEM_CHERRY_WOOD, 3), name: "cherry_wood", shaped: true, cells: &[(0, 0, ITEM_CHERRY_LOG), (1, 0, ITEM_CHERRY_LOG), (0, 1, ITEM_CHERRY_LOG), (1, 1, ITEM_CHERRY_LOG)] },
    Recipe { needs: &[(TAG_PLANKS, 8)], result: (ITEM_CHEST, 1), name: "chest", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (0, 1, TAG_PLANKS), (2, 1, TAG_PLANKS), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS), (2, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_MINECART, 1)], result: (ITEM_CHEST_MINECART, 1), name: "chest_minecart", shaped: false, cells: &[] },
    Recipe { needs: &[(TAG_PLANKS, 6), (TAG_WOODEN_SLABS, 3)], result: (ITEM_CHISELED_BOOKSHELF, 1), name: "chiseled_bookshelf", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (0, 1, TAG_WOODEN_SLABS), (1, 1, TAG_WOODEN_SLABS), (2, 1, TAG_WOODEN_SLABS), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS), (2, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CINNABAR_SLAB, 2)], result: (ITEM_CHISELED_CINNABAR, 1), name: "chiseled_cinnabar", shaped: true, cells: &[(0, 0, ITEM_CINNABAR_SLAB), (0, 1, ITEM_CINNABAR_SLAB)] },
    Recipe { needs: &[(ITEM_CUT_COPPER_SLAB, 2)], result: (ITEM_CHISELED_COPPER, 1), name: "chiseled_copper", shaped: true, cells: &[(0, 0, ITEM_CUT_COPPER_SLAB), (0, 1, ITEM_CUT_COPPER_SLAB)] },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE_SLAB, 2)], result: (ITEM_CHISELED_DEEPSLATE, 1), name: "chiseled_deepslate", shaped: true, cells: &[(0, 0, ITEM_COBBLED_DEEPSLATE_SLAB), (0, 1, ITEM_COBBLED_DEEPSLATE_SLAB)] },
    Recipe { needs: &[(ITEM_NETHER_BRICK_SLAB, 2)], result: (ITEM_CHISELED_NETHER_BRICKS, 1), name: "chiseled_nether_bricks", shaped: true, cells: &[(0, 0, ITEM_NETHER_BRICK_SLAB), (0, 1, ITEM_NETHER_BRICK_SLAB)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE_SLAB, 2)], result: (ITEM_CHISELED_POLISHED_BLACKSTONE, 1), name: "chiseled_polished_blackstone", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE_SLAB), (0, 1, ITEM_POLISHED_BLACKSTONE_SLAB)] },
    Recipe { needs: &[(ITEM_QUARTZ_SLAB, 2)], result: (ITEM_CHISELED_QUARTZ_BLOCK, 1), name: "chiseled_quartz_block", shaped: true, cells: &[(0, 0, ITEM_QUARTZ_SLAB), (0, 1, ITEM_QUARTZ_SLAB)] },
    Recipe { needs: &[(ITEM_RED_SANDSTONE_SLAB, 2)], result: (ITEM_CHISELED_RED_SANDSTONE, 1), name: "chiseled_red_sandstone", shaped: true, cells: &[(0, 0, ITEM_RED_SANDSTONE_SLAB), (0, 1, ITEM_RED_SANDSTONE_SLAB)] },
    Recipe { needs: &[(ITEM_RESIN_BRICK_SLAB, 2)], result: (ITEM_CHISELED_RESIN_BRICKS, 1), name: "chiseled_resin_bricks", shaped: true, cells: &[(0, 0, ITEM_RESIN_BRICK_SLAB), (0, 1, ITEM_RESIN_BRICK_SLAB)] },
    Recipe { needs: &[(ITEM_SANDSTONE_SLAB, 2)], result: (ITEM_CHISELED_SANDSTONE, 1), name: "chiseled_sandstone", shaped: true, cells: &[(0, 0, ITEM_SANDSTONE_SLAB), (0, 1, ITEM_SANDSTONE_SLAB)] },
    Recipe { needs: &[(ITEM_STONE_BRICK_SLAB, 2)], result: (ITEM_CHISELED_STONE_BRICKS, 1), name: "chiseled_stone_bricks", shaped: true, cells: &[(0, 0, ITEM_STONE_BRICK_SLAB), (0, 1, ITEM_STONE_BRICK_SLAB)] },
    Recipe { needs: &[(ITEM_SULFUR_SLAB, 2)], result: (ITEM_CHISELED_SULFUR, 1), name: "chiseled_sulfur", shaped: true, cells: &[(0, 0, ITEM_SULFUR_SLAB), (0, 1, ITEM_SULFUR_SLAB)] },
    Recipe { needs: &[(ITEM_TUFF_SLAB, 2)], result: (ITEM_CHISELED_TUFF, 1), name: "chiseled_tuff", shaped: true, cells: &[(0, 0, ITEM_TUFF_SLAB), (0, 1, ITEM_TUFF_SLAB)] },
    Recipe { needs: &[(ITEM_TUFF_BRICK_SLAB, 2)], result: (ITEM_CHISELED_TUFF_BRICKS, 1), name: "chiseled_tuff_bricks", shaped: true, cells: &[(0, 0, ITEM_TUFF_BRICK_SLAB), (0, 1, ITEM_TUFF_BRICK_SLAB)] },
    Recipe { needs: &[(ITEM_CINNABAR_BRICKS, 3)], result: (ITEM_CINNABAR_BRICK_SLAB, 6), name: "cinnabar_brick_slab", shaped: true, cells: &[(0, 0, ITEM_CINNABAR_BRICKS), (1, 0, ITEM_CINNABAR_BRICKS), (2, 0, ITEM_CINNABAR_BRICKS)] },
    Recipe { needs: &[(ITEM_CINNABAR_BRICKS, 6)], result: (ITEM_CINNABAR_BRICK_STAIRS, 4), name: "cinnabar_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_CINNABAR_BRICKS), (0, 1, ITEM_CINNABAR_BRICKS), (1, 1, ITEM_CINNABAR_BRICKS), (0, 2, ITEM_CINNABAR_BRICKS), (1, 2, ITEM_CINNABAR_BRICKS), (2, 2, ITEM_CINNABAR_BRICKS)] },
    Recipe { needs: &[(ITEM_CINNABAR_BRICKS, 6)], result: (ITEM_CINNABAR_BRICK_WALL, 6), name: "cinnabar_brick_wall", shaped: true, cells: &[(0, 0, ITEM_CINNABAR_BRICKS), (1, 0, ITEM_CINNABAR_BRICKS), (2, 0, ITEM_CINNABAR_BRICKS), (0, 1, ITEM_CINNABAR_BRICKS), (1, 1, ITEM_CINNABAR_BRICKS), (2, 1, ITEM_CINNABAR_BRICKS)] },
    Recipe { needs: &[(ITEM_POLISHED_CINNABAR, 4)], result: (ITEM_CINNABAR_BRICKS, 4), name: "cinnabar_bricks", shaped: true, cells: &[(0, 0, ITEM_POLISHED_CINNABAR), (1, 0, ITEM_POLISHED_CINNABAR), (0, 1, ITEM_POLISHED_CINNABAR), (1, 1, ITEM_POLISHED_CINNABAR)] },
    Recipe { needs: &[(ITEM_CINNABAR, 3)], result: (ITEM_CINNABAR_SLAB, 6), name: "cinnabar_slab", shaped: true, cells: &[(0, 0, ITEM_CINNABAR), (1, 0, ITEM_CINNABAR), (2, 0, ITEM_CINNABAR)] },
    Recipe { needs: &[(ITEM_CINNABAR, 6)], result: (ITEM_CINNABAR_STAIRS, 4), name: "cinnabar_stairs", shaped: true, cells: &[(0, 0, ITEM_CINNABAR), (0, 1, ITEM_CINNABAR), (1, 1, ITEM_CINNABAR), (0, 2, ITEM_CINNABAR), (1, 2, ITEM_CINNABAR), (2, 2, ITEM_CINNABAR)] },
    Recipe { needs: &[(ITEM_CINNABAR, 6)], result: (ITEM_CINNABAR_WALL, 6), name: "cinnabar_wall", shaped: true, cells: &[(0, 0, ITEM_CINNABAR), (1, 0, ITEM_CINNABAR), (2, 0, ITEM_CINNABAR), (0, 1, ITEM_CINNABAR), (1, 1, ITEM_CINNABAR), (2, 1, ITEM_CINNABAR)] },
    Recipe { needs: &[(ITEM_CLAY_BALL, 4)], result: (ITEM_CLAY, 1), name: "clay", shaped: true, cells: &[(0, 0, ITEM_CLAY_BALL), (1, 0, ITEM_CLAY_BALL), (0, 1, ITEM_CLAY_BALL), (1, 1, ITEM_CLAY_BALL)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 4), (ITEM_REDSTONE, 1)], result: (ITEM_CLOCK, 1), name: "clock", shaped: true, cells: &[(1, 0, ITEM_GOLD_INGOT), (0, 1, ITEM_GOLD_INGOT), (1, 1, ITEM_REDSTONE), (2, 1, ITEM_GOLD_INGOT), (1, 2, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(ITEM_COAL_BLOCK, 1)], result: (COAL, 9), name: "coal", shaped: false, cells: &[] },
    Recipe { needs: &[(COAL, 9)], result: (ITEM_COAL_BLOCK, 1), name: "coal_block", shaped: true, cells: &[(0, 0, COAL), (1, 0, COAL), (2, 0, COAL), (0, 1, COAL), (1, 1, COAL), (2, 1, COAL), (0, 2, COAL), (1, 2, COAL), (2, 2, COAL)] },
    Recipe { needs: &[(ITEM_DIRT, 2), (ITEM_GRAVEL, 2)], result: (ITEM_COARSE_DIRT, 4), name: "coarse_dirt", shaped: true, cells: &[(0, 0, ITEM_DIRT), (1, 0, ITEM_GRAVEL), (0, 1, ITEM_GRAVEL), (1, 1, ITEM_DIRT)] },
    Recipe { needs: &[(ITEM_COAST_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (COBBLESTONE, 1), (ITEM_DIAMOND, 7)], result: (ITEM_COAST_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "coast_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_COAST_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, COBBLESTONE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 3)], result: (ITEM_COBBLED_DEEPSLATE_SLAB, 6), name: "cobbled_deepslate_slab", shaped: true, cells: &[(0, 0, ITEM_COBBLED_DEEPSLATE), (1, 0, ITEM_COBBLED_DEEPSLATE), (2, 0, ITEM_COBBLED_DEEPSLATE)] },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 6)], result: (ITEM_COBBLED_DEEPSLATE_STAIRS, 4), name: "cobbled_deepslate_stairs", shaped: true, cells: &[(0, 0, ITEM_COBBLED_DEEPSLATE), (0, 1, ITEM_COBBLED_DEEPSLATE), (1, 1, ITEM_COBBLED_DEEPSLATE), (0, 2, ITEM_COBBLED_DEEPSLATE), (1, 2, ITEM_COBBLED_DEEPSLATE), (2, 2, ITEM_COBBLED_DEEPSLATE)] },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 6)], result: (ITEM_COBBLED_DEEPSLATE_WALL, 6), name: "cobbled_deepslate_wall", shaped: true, cells: &[(0, 0, ITEM_COBBLED_DEEPSLATE), (1, 0, ITEM_COBBLED_DEEPSLATE), (2, 0, ITEM_COBBLED_DEEPSLATE), (0, 1, ITEM_COBBLED_DEEPSLATE), (1, 1, ITEM_COBBLED_DEEPSLATE), (2, 1, ITEM_COBBLED_DEEPSLATE)] },
    Recipe { needs: &[(COBBLESTONE, 3)], result: (ITEM_COBBLESTONE_SLAB, 6), name: "cobblestone_slab", shaped: true, cells: &[(0, 0, COBBLESTONE), (1, 0, COBBLESTONE), (2, 0, COBBLESTONE)] },
    Recipe { needs: &[(COBBLESTONE, 6)], result: (ITEM_COBBLESTONE_STAIRS, 4), name: "cobblestone_stairs", shaped: true, cells: &[(0, 0, COBBLESTONE), (0, 1, COBBLESTONE), (1, 1, COBBLESTONE), (0, 2, COBBLESTONE), (1, 2, COBBLESTONE), (2, 2, COBBLESTONE)] },
    Recipe { needs: &[(COBBLESTONE, 6)], result: (ITEM_COBBLESTONE_WALL, 6), name: "cobblestone_wall", shaped: true, cells: &[(0, 0, COBBLESTONE), (1, 0, COBBLESTONE), (2, 0, COBBLESTONE), (0, 1, COBBLESTONE), (1, 1, COBBLESTONE), (2, 1, COBBLESTONE)] },
    Recipe { needs: &[(ITEM_QUARTZ, 1), (ITEM_REDSTONE_TORCH, 3), (ITEM_STONE, 3)], result: (ITEM_COMPARATOR, 1), name: "comparator", shaped: true, cells: &[(1, 0, ITEM_REDSTONE_TORCH), (0, 1, ITEM_REDSTONE_TORCH), (1, 1, ITEM_QUARTZ), (2, 1, ITEM_REDSTONE_TORCH), (0, 2, ITEM_STONE), (1, 2, ITEM_STONE), (2, 2, ITEM_STONE)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 4), (ITEM_REDSTONE, 1)], result: (ITEM_COMPASS, 1), name: "compass", shaped: true, cells: &[(1, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_REDSTONE), (2, 1, ITEM_IRON_INGOT), (1, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(TAG_WOODEN_SLABS, 7)], result: (ITEM_COMPOSTER, 1), name: "composter", shaped: true, cells: &[(0, 0, TAG_WOODEN_SLABS), (2, 0, TAG_WOODEN_SLABS), (0, 1, TAG_WOODEN_SLABS), (2, 1, TAG_WOODEN_SLABS), (0, 2, TAG_WOODEN_SLABS), (1, 2, TAG_WOODEN_SLABS), (2, 2, TAG_WOODEN_SLABS)] },
    Recipe { needs: &[(ITEM_HEART_OF_THE_SEA, 1), (ITEM_NAUTILUS_SHELL, 8)], result: (ITEM_CONDUIT, 1), name: "conduit", shaped: true, cells: &[(0, 0, ITEM_NAUTILUS_SHELL), (1, 0, ITEM_NAUTILUS_SHELL), (2, 0, ITEM_NAUTILUS_SHELL), (0, 1, ITEM_NAUTILUS_SHELL), (1, 1, ITEM_HEART_OF_THE_SEA), (2, 1, ITEM_NAUTILUS_SHELL), (0, 2, ITEM_NAUTILUS_SHELL), (1, 2, ITEM_NAUTILUS_SHELL), (2, 2, ITEM_NAUTILUS_SHELL)] },
    Recipe { needs: &[(ITEM_COCOA_BEANS, 1), (WHEAT, 2)], result: (ITEM_COOKIE, 8), name: "cookie", shaped: true, cells: &[(0, 0, WHEAT), (1, 0, ITEM_COCOA_BEANS), (2, 0, WHEAT)] },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 3)], result: (ITEM_COPPER_AXE, 1), name: "copper_axe", shaped: true, cells: &[(0, 0, TAG_COPPER_TOOL_MATERIALS), (1, 0, TAG_COPPER_TOOL_MATERIALS), (0, 1, TAG_COPPER_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 6)], result: (ITEM_COPPER_BARS, 16), name: "copper_bars", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (1, 0, ITEM_COPPER_INGOT), (2, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (1, 1, ITEM_COPPER_INGOT), (2, 1, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 9)], result: (ITEM_COPPER_BLOCK, 1), name: "copper_block", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (1, 0, ITEM_COPPER_INGOT), (2, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (1, 1, ITEM_COPPER_INGOT), (2, 1, ITEM_COPPER_INGOT), (0, 2, ITEM_COPPER_INGOT), (1, 2, ITEM_COPPER_INGOT), (2, 2, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 4)], result: (ITEM_COPPER_BOOTS, 1), name: "copper_boots", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (2, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (2, 1, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_COPPER_BLOCK, 3), (ITEM_REDSTONE, 1)], result: (ITEM_COPPER_BULB, 4), name: "copper_bulb", shaped: true, cells: &[(1, 0, ITEM_COPPER_BLOCK), (0, 1, ITEM_COPPER_BLOCK), (1, 1, ITEM_BLAZE_ROD), (2, 1, ITEM_COPPER_BLOCK), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 1), (ITEM_COPPER_NUGGET, 2)], result: (ITEM_COPPER_CHAIN, 1), name: "copper_chain", shaped: true, cells: &[(0, 0, ITEM_COPPER_NUGGET), (0, 1, ITEM_COPPER_INGOT), (0, 2, ITEM_COPPER_NUGGET)] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_COPPER_INGOT, 8)], result: (ITEM_COPPER_CHEST, 1), name: "copper_chest", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (1, 0, ITEM_COPPER_INGOT), (2, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (1, 1, ITEM_CHEST), (2, 1, ITEM_COPPER_INGOT), (0, 2, ITEM_COPPER_INGOT), (1, 2, ITEM_COPPER_INGOT), (2, 2, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 8)], result: (ITEM_COPPER_CHESTPLATE, 1), name: "copper_chestplate", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (2, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (1, 1, ITEM_COPPER_INGOT), (2, 1, ITEM_COPPER_INGOT), (0, 2, ITEM_COPPER_INGOT), (1, 2, ITEM_COPPER_INGOT), (2, 2, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 6)], result: (ITEM_COPPER_DOOR, 3), name: "copper_door", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (1, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (1, 1, ITEM_COPPER_INGOT), (0, 2, ITEM_COPPER_INGOT), (1, 2, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(ITEM_COPPER_BLOCK, 4)], result: (ITEM_COPPER_GRATE, 4), name: "copper_grate", shaped: true, cells: &[(1, 0, ITEM_COPPER_BLOCK), (0, 1, ITEM_COPPER_BLOCK), (2, 1, ITEM_COPPER_BLOCK), (1, 2, ITEM_COPPER_BLOCK)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 5)], result: (ITEM_COPPER_HELMET, 1), name: "copper_helmet", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (1, 0, ITEM_COPPER_INGOT), (2, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (2, 1, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 2)], result: (ITEM_COPPER_HOE, 1), name: "copper_hoe", shaped: true, cells: &[(0, 0, TAG_COPPER_TOOL_MATERIALS), (1, 0, TAG_COPPER_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_COPPER_BLOCK, 1)], result: (ITEM_COPPER_INGOT, 9), name: "copper_ingot", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_NUGGET, 9)], result: (ITEM_COPPER_INGOT, 1), name: "copper_ingot_from_nuggets", shaped: true, cells: &[(0, 0, ITEM_COPPER_NUGGET), (1, 0, ITEM_COPPER_NUGGET), (2, 0, ITEM_COPPER_NUGGET), (0, 1, ITEM_COPPER_NUGGET), (1, 1, ITEM_COPPER_NUGGET), (2, 1, ITEM_COPPER_NUGGET), (0, 2, ITEM_COPPER_NUGGET), (1, 2, ITEM_COPPER_NUGGET), (2, 2, ITEM_COPPER_NUGGET)] },
    Recipe { needs: &[(ITEM_WAXED_COPPER_BLOCK, 1)], result: (ITEM_COPPER_INGOT, 9), name: "copper_ingot_from_waxed_copper_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_NUGGET, 8), (ITEM_COPPER_TORCH, 1)], result: (ITEM_COPPER_LANTERN, 1), name: "copper_lantern", shaped: true, cells: &[(0, 0, ITEM_COPPER_NUGGET), (1, 0, ITEM_COPPER_NUGGET), (2, 0, ITEM_COPPER_NUGGET), (0, 1, ITEM_COPPER_NUGGET), (1, 1, ITEM_COPPER_TORCH), (2, 1, ITEM_COPPER_NUGGET), (0, 2, ITEM_COPPER_NUGGET), (1, 2, ITEM_COPPER_NUGGET), (2, 2, ITEM_COPPER_NUGGET)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 7)], result: (ITEM_COPPER_LEGGINGS, 1), name: "copper_leggings", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (1, 0, ITEM_COPPER_INGOT), (2, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (2, 1, ITEM_COPPER_INGOT), (0, 2, ITEM_COPPER_INGOT), (2, 2, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 1)], result: (ITEM_COPPER_NUGGET, 9), name: "copper_nugget", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 3)], result: (ITEM_COPPER_PICKAXE, 1), name: "copper_pickaxe", shaped: true, cells: &[(0, 0, TAG_COPPER_TOOL_MATERIALS), (1, 0, TAG_COPPER_TOOL_MATERIALS), (2, 0, TAG_COPPER_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 1)], result: (ITEM_COPPER_SHOVEL, 1), name: "copper_shovel", shaped: true, cells: &[(0, 0, TAG_COPPER_TOOL_MATERIALS), (0, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_COPPER_TOOL_MATERIALS, 1)], result: (ITEM_COPPER_SPEAR, 1), name: "copper_spear", shaped: true, cells: &[(2, 0, TAG_COPPER_TOOL_MATERIALS), (1, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 1), (TAG_COPPER_TOOL_MATERIALS, 2)], result: (ITEM_COPPER_SWORD, 1), name: "copper_sword", shaped: true, cells: &[(0, 0, TAG_COPPER_TOOL_MATERIALS), (0, 1, TAG_COPPER_TOOL_MATERIALS), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_COPPER_NUGGET, 1), (STICK, 1), (TAG_COALS, 1)], result: (ITEM_COPPER_TORCH, 4), name: "copper_torch", shaped: true, cells: &[(0, 0, ITEM_COPPER_NUGGET), (0, 1, TAG_COALS), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 4)], result: (ITEM_COPPER_TRAPDOOR, 1), name: "copper_trapdoor", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (1, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (1, 1, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(CRAFTING_TABLE, 1), (ITEM_DROPPER, 1), (ITEM_IRON_INGOT, 5), (ITEM_REDSTONE, 2)], result: (ITEM_CRAFTER, 1), name: "crafter", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, CRAFTING_TABLE), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_REDSTONE), (1, 2, ITEM_DROPPER), (2, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(TAG_PLANKS, 4)], result: (CRAFTING_TABLE, 1), name: "crafting_table", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_PALE_OAK_LOG, 2), (ITEM_RESIN_BLOCK, 1)], result: (ITEM_CREAKING_HEART, 1), name: "creaking_heart", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_LOG), (0, 1, ITEM_RESIN_BLOCK), (0, 2, ITEM_PALE_OAK_LOG)] },
    Recipe { needs: &[(ITEM_CREEPER_HEAD, 1), (ITEM_PAPER, 1)], result: (ITEM_CREEPER_BANNER_PATTERN, 1), name: "creeper_banner_pattern", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 1)], result: (ITEM_CRIMSON_BUTTON, 1), name: "crimson_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6)], result: (ITEM_CRIMSON_DOOR, 3), name: "crimson_door", shaped: true, cells: &[(0, 0, ITEM_CRIMSON_PLANKS), (1, 0, ITEM_CRIMSON_PLANKS), (0, 1, ITEM_CRIMSON_PLANKS), (1, 1, ITEM_CRIMSON_PLANKS), (0, 2, ITEM_CRIMSON_PLANKS), (1, 2, ITEM_CRIMSON_PLANKS)] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 4), (STICK, 2)], result: (ITEM_CRIMSON_FENCE, 3), name: "crimson_fence", shaped: true, cells: &[(0, 0, ITEM_CRIMSON_PLANKS), (1, 0, STICK), (2, 0, ITEM_CRIMSON_PLANKS), (0, 1, ITEM_CRIMSON_PLANKS), (1, 1, STICK), (2, 1, ITEM_CRIMSON_PLANKS)] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 2), (STICK, 4)], result: (ITEM_CRIMSON_FENCE_GATE, 1), name: "crimson_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_CRIMSON_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_CRIMSON_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_CRIMSON_STEM, 6)], result: (ITEM_CRIMSON_HANGING_SIGN, 6), name: "crimson_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_CRIMSON_STEM), (1, 1, ITEM_STRIPPED_CRIMSON_STEM), (2, 1, ITEM_STRIPPED_CRIMSON_STEM), (0, 2, ITEM_STRIPPED_CRIMSON_STEM), (1, 2, ITEM_STRIPPED_CRIMSON_STEM), (2, 2, ITEM_STRIPPED_CRIMSON_STEM)] },
    Recipe { needs: &[(ITEM_CRIMSON_STEM, 4)], result: (ITEM_CRIMSON_HYPHAE, 3), name: "crimson_hyphae", shaped: true, cells: &[(0, 0, ITEM_CRIMSON_STEM), (1, 0, ITEM_CRIMSON_STEM), (0, 1, ITEM_CRIMSON_STEM), (1, 1, ITEM_CRIMSON_STEM)] },
    Recipe { needs: &[(TAG_CRIMSON_STEMS, 1)], result: (ITEM_CRIMSON_PLANKS, 4), name: "crimson_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 2)], result: (ITEM_CRIMSON_PRESSURE_PLATE, 1), name: "crimson_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_CRIMSON_PLANKS), (1, 0, ITEM_CRIMSON_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_CRIMSON_STEM, 6)], result: (ITEM_CRIMSON_SHELF, 6), name: "crimson_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_CRIMSON_STEM), (1, 0, ITEM_STRIPPED_CRIMSON_STEM), (2, 0, ITEM_STRIPPED_CRIMSON_STEM), (0, 2, ITEM_STRIPPED_CRIMSON_STEM), (1, 2, ITEM_STRIPPED_CRIMSON_STEM), (2, 2, ITEM_STRIPPED_CRIMSON_STEM)] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6), (STICK, 1)], result: (ITEM_CRIMSON_SIGN, 3), name: "crimson_sign", shaped: true, cells: &[(0, 0, ITEM_CRIMSON_PLANKS), (1, 0, ITEM_CRIMSON_PLANKS), (2, 0, ITEM_CRIMSON_PLANKS), (0, 1, ITEM_CRIMSON_PLANKS), (1, 1, ITEM_CRIMSON_PLANKS), (2, 1, ITEM_CRIMSON_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 3)], result: (ITEM_CRIMSON_SLAB, 6), name: "crimson_slab", shaped: true, cells: &[(0, 0, ITEM_CRIMSON_PLANKS), (1, 0, ITEM_CRIMSON_PLANKS), (2, 0, ITEM_CRIMSON_PLANKS)] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6)], result: (ITEM_CRIMSON_STAIRS, 4), name: "crimson_stairs", shaped: true, cells: &[(0, 0, ITEM_CRIMSON_PLANKS), (0, 1, ITEM_CRIMSON_PLANKS), (1, 1, ITEM_CRIMSON_PLANKS), (0, 2, ITEM_CRIMSON_PLANKS), (1, 2, ITEM_CRIMSON_PLANKS), (2, 2, ITEM_CRIMSON_PLANKS)] },
    Recipe { needs: &[(ITEM_CRIMSON_PLANKS, 6)], result: (ITEM_CRIMSON_TRAPDOOR, 2), name: "crimson_trapdoor", shaped: true, cells: &[(0, 0, ITEM_CRIMSON_PLANKS), (1, 0, ITEM_CRIMSON_PLANKS), (2, 0, ITEM_CRIMSON_PLANKS), (0, 1, ITEM_CRIMSON_PLANKS), (1, 1, ITEM_CRIMSON_PLANKS), (2, 1, ITEM_CRIMSON_PLANKS)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (STICK, 3), (ITEM_STRING, 2), (ITEM_TRIPWIRE_HOOK, 1)], result: (ITEM_CROSSBOW, 1), name: "crossbow", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_IRON_INGOT), (2, 0, STICK), (0, 1, ITEM_STRING), (1, 1, ITEM_TRIPWIRE_HOOK), (2, 1, ITEM_STRING), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_COPPER_BLOCK, 4)], result: (ITEM_CUT_COPPER, 4), name: "cut_copper", shaped: true, cells: &[(0, 0, ITEM_COPPER_BLOCK), (1, 0, ITEM_COPPER_BLOCK), (0, 1, ITEM_COPPER_BLOCK), (1, 1, ITEM_COPPER_BLOCK)] },
    Recipe { needs: &[(ITEM_CUT_COPPER, 3)], result: (ITEM_CUT_COPPER_SLAB, 6), name: "cut_copper_slab", shaped: true, cells: &[(0, 0, ITEM_CUT_COPPER), (1, 0, ITEM_CUT_COPPER), (2, 0, ITEM_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_CUT_COPPER, 6)], result: (ITEM_CUT_COPPER_STAIRS, 4), name: "cut_copper_stairs", shaped: true, cells: &[(0, 0, ITEM_CUT_COPPER), (0, 1, ITEM_CUT_COPPER), (1, 1, ITEM_CUT_COPPER), (0, 2, ITEM_CUT_COPPER), (1, 2, ITEM_CUT_COPPER), (2, 2, ITEM_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_RED_SANDSTONE, 4)], result: (ITEM_CUT_RED_SANDSTONE, 4), name: "cut_red_sandstone", shaped: true, cells: &[(0, 0, ITEM_RED_SANDSTONE), (1, 0, ITEM_RED_SANDSTONE), (0, 1, ITEM_RED_SANDSTONE), (1, 1, ITEM_RED_SANDSTONE)] },
    Recipe { needs: &[(ITEM_CUT_RED_SANDSTONE, 3)], result: (ITEM_CUT_RED_SANDSTONE_SLAB, 6), name: "cut_red_sandstone_slab", shaped: true, cells: &[(0, 0, ITEM_CUT_RED_SANDSTONE), (1, 0, ITEM_CUT_RED_SANDSTONE), (2, 0, ITEM_CUT_RED_SANDSTONE)] },
    Recipe { needs: &[(ITEM_SANDSTONE, 4)], result: (ITEM_CUT_SANDSTONE, 4), name: "cut_sandstone", shaped: true, cells: &[(0, 0, ITEM_SANDSTONE), (1, 0, ITEM_SANDSTONE), (0, 1, ITEM_SANDSTONE), (1, 1, ITEM_SANDSTONE)] },
    Recipe { needs: &[(ITEM_CUT_SANDSTONE, 3)], result: (ITEM_CUT_SANDSTONE_SLAB, 6), name: "cut_sandstone_slab", shaped: true, cells: &[(0, 0, ITEM_CUT_SANDSTONE), (1, 0, ITEM_CUT_SANDSTONE), (2, 0, ITEM_CUT_SANDSTONE)] },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 6), (STICK, 1)], result: (ITEM_CYAN_BANNER, 1), name: "cyan_banner", shaped: true, cells: &[(0, 0, ITEM_CYAN_WOOL), (1, 0, ITEM_CYAN_WOOL), (2, 0, ITEM_CYAN_WOOL), (0, 1, ITEM_CYAN_WOOL), (1, 1, ITEM_CYAN_WOOL), (2, 1, ITEM_CYAN_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_CYAN_BED, 1), name: "cyan_bed", shaped: true, cells: &[(0, 0, ITEM_CYAN_WOOL), (1, 0, ITEM_CYAN_WOOL), (2, 0, ITEM_CYAN_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_CYAN_DYE, 1)], result: (ITEM_CYAN_CANDLE, 1), name: "cyan_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 2)], result: (ITEM_CYAN_CARPET, 3), name: "cyan_carpet", shaped: true, cells: &[(0, 0, ITEM_CYAN_WOOL), (1, 0, ITEM_CYAN_WOOL)] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_CYAN_CONCRETE_POWDER, 8), name: "cyan_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_CONCRETE, 3)], result: (ITEM_CYAN_CONCRETE_SLAB, 6), name: "cyan_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_CYAN_CONCRETE), (1, 0, ITEM_CYAN_CONCRETE), (2, 0, ITEM_CYAN_CONCRETE)] },
    Recipe { needs: &[(ITEM_CYAN_CONCRETE, 6)], result: (ITEM_CYAN_CONCRETE_STAIRS, 4), name: "cyan_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_CYAN_CONCRETE), (0, 1, ITEM_CYAN_CONCRETE), (1, 1, ITEM_CYAN_CONCRETE), (0, 2, ITEM_CYAN_CONCRETE), (1, 2, ITEM_CYAN_CONCRETE), (2, 2, ITEM_CYAN_CONCRETE)] },
    Recipe { needs: &[(ITEM_CYAN_WOOL_SLAB, 3)], result: (ITEM_CYAN_CUSHION, 1), name: "cyan_cushion", shaped: true, cells: &[(0, 0, ITEM_CYAN_WOOL_SLAB), (1, 0, ITEM_CYAN_WOOL_SLAB), (2, 0, ITEM_CYAN_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_GREEN_DYE, 1)], result: (ITEM_CYAN_DYE, 2), name: "cyan_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PITCHER_PLANT, 1)], result: (ITEM_CYAN_DYE, 2), name: "cyan_dye_from_pitcher_plant", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 1), (ITEM_GLASS, 2), (ITEM_LEATHER, 3)], result: (ITEM_CYAN_HARNESS, 1), name: "cyan_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_CYAN_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_GLASS, 8)], result: (ITEM_CYAN_STAINED_GLASS, 8), name: "cyan_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_CYAN_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_CYAN_STAINED_GLASS, 6)], result: (ITEM_CYAN_STAINED_GLASS_PANE, 16), name: "cyan_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_CYAN_STAINED_GLASS), (1, 0, ITEM_CYAN_STAINED_GLASS), (2, 0, ITEM_CYAN_STAINED_GLASS), (0, 1, ITEM_CYAN_STAINED_GLASS), (1, 1, ITEM_CYAN_STAINED_GLASS), (2, 1, ITEM_CYAN_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_GLASS_PANE, 8)], result: (ITEM_CYAN_STAINED_GLASS_PANE, 8), name: "cyan_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_CYAN_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_CYAN_TERRACOTTA, 8), name: "cyan_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_CYAN_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 3)], result: (ITEM_CYAN_WOOL_SLAB, 6), name: "cyan_wool_slab", shaped: true, cells: &[(0, 0, ITEM_CYAN_WOOL), (1, 0, ITEM_CYAN_WOOL), (2, 0, ITEM_CYAN_WOOL)] },
    Recipe { needs: &[(ITEM_CYAN_WOOL, 6)], result: (ITEM_CYAN_WOOL_STAIRS, 4), name: "cyan_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_CYAN_WOOL), (0, 1, ITEM_CYAN_WOOL), (1, 1, ITEM_CYAN_WOOL), (0, 2, ITEM_CYAN_WOOL), (1, 2, ITEM_CYAN_WOOL), (2, 2, ITEM_CYAN_WOOL)] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 5)], result: (ITEM_DARK_OAK_BOAT, 1), name: "dark_oak_boat", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_PLANKS), (2, 0, ITEM_DARK_OAK_PLANKS), (0, 1, ITEM_DARK_OAK_PLANKS), (1, 1, ITEM_DARK_OAK_PLANKS), (2, 1, ITEM_DARK_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 1)], result: (ITEM_DARK_OAK_BUTTON, 1), name: "dark_oak_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_DARK_OAK_BOAT, 1)], result: (ITEM_DARK_OAK_CHEST_BOAT, 1), name: "dark_oak_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6)], result: (ITEM_DARK_OAK_DOOR, 3), name: "dark_oak_door", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_PLANKS), (1, 0, ITEM_DARK_OAK_PLANKS), (0, 1, ITEM_DARK_OAK_PLANKS), (1, 1, ITEM_DARK_OAK_PLANKS), (0, 2, ITEM_DARK_OAK_PLANKS), (1, 2, ITEM_DARK_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_DARK_OAK_FENCE, 3), name: "dark_oak_fence", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_PLANKS), (1, 0, STICK), (2, 0, ITEM_DARK_OAK_PLANKS), (0, 1, ITEM_DARK_OAK_PLANKS), (1, 1, STICK), (2, 1, ITEM_DARK_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 2), (STICK, 4)], result: (ITEM_DARK_OAK_FENCE_GATE, 1), name: "dark_oak_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_DARK_OAK_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_DARK_OAK_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_DARK_OAK_LOG, 6)], result: (ITEM_DARK_OAK_HANGING_SIGN, 6), name: "dark_oak_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_DARK_OAK_LOG), (1, 1, ITEM_STRIPPED_DARK_OAK_LOG), (2, 1, ITEM_STRIPPED_DARK_OAK_LOG), (0, 2, ITEM_STRIPPED_DARK_OAK_LOG), (1, 2, ITEM_STRIPPED_DARK_OAK_LOG), (2, 2, ITEM_STRIPPED_DARK_OAK_LOG)] },
    Recipe { needs: &[(TAG_DARK_OAK_LOGS, 1)], result: (ITEM_DARK_OAK_PLANKS, 4), name: "dark_oak_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 2)], result: (ITEM_DARK_OAK_PRESSURE_PLATE, 1), name: "dark_oak_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_PLANKS), (1, 0, ITEM_DARK_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_DARK_OAK_LOG, 6)], result: (ITEM_DARK_OAK_SHELF, 6), name: "dark_oak_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_DARK_OAK_LOG), (1, 0, ITEM_STRIPPED_DARK_OAK_LOG), (2, 0, ITEM_STRIPPED_DARK_OAK_LOG), (0, 2, ITEM_STRIPPED_DARK_OAK_LOG), (1, 2, ITEM_STRIPPED_DARK_OAK_LOG), (2, 2, ITEM_STRIPPED_DARK_OAK_LOG)] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6), (STICK, 1)], result: (ITEM_DARK_OAK_SIGN, 3), name: "dark_oak_sign", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_PLANKS), (1, 0, ITEM_DARK_OAK_PLANKS), (2, 0, ITEM_DARK_OAK_PLANKS), (0, 1, ITEM_DARK_OAK_PLANKS), (1, 1, ITEM_DARK_OAK_PLANKS), (2, 1, ITEM_DARK_OAK_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 3)], result: (ITEM_DARK_OAK_SLAB, 6), name: "dark_oak_slab", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_PLANKS), (1, 0, ITEM_DARK_OAK_PLANKS), (2, 0, ITEM_DARK_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6)], result: (ITEM_DARK_OAK_STAIRS, 4), name: "dark_oak_stairs", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_PLANKS), (0, 1, ITEM_DARK_OAK_PLANKS), (1, 1, ITEM_DARK_OAK_PLANKS), (0, 2, ITEM_DARK_OAK_PLANKS), (1, 2, ITEM_DARK_OAK_PLANKS), (2, 2, ITEM_DARK_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_DARK_OAK_PLANKS, 6)], result: (ITEM_DARK_OAK_TRAPDOOR, 2), name: "dark_oak_trapdoor", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_PLANKS), (1, 0, ITEM_DARK_OAK_PLANKS), (2, 0, ITEM_DARK_OAK_PLANKS), (0, 1, ITEM_DARK_OAK_PLANKS), (1, 1, ITEM_DARK_OAK_PLANKS), (2, 1, ITEM_DARK_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_DARK_OAK_LOG, 4)], result: (ITEM_DARK_OAK_WOOD, 3), name: "dark_oak_wood", shaped: true, cells: &[(0, 0, ITEM_DARK_OAK_LOG), (1, 0, ITEM_DARK_OAK_LOG), (0, 1, ITEM_DARK_OAK_LOG), (1, 1, ITEM_DARK_OAK_LOG)] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_PRISMARINE_SHARD, 8)], result: (ITEM_DARK_PRISMARINE, 1), name: "dark_prismarine", shaped: true, cells: &[(0, 0, ITEM_PRISMARINE_SHARD), (1, 0, ITEM_PRISMARINE_SHARD), (2, 0, ITEM_PRISMARINE_SHARD), (0, 1, ITEM_PRISMARINE_SHARD), (1, 1, ITEM_BLACK_DYE), (2, 1, ITEM_PRISMARINE_SHARD), (0, 2, ITEM_PRISMARINE_SHARD), (1, 2, ITEM_PRISMARINE_SHARD), (2, 2, ITEM_PRISMARINE_SHARD)] },
    Recipe { needs: &[(ITEM_DARK_PRISMARINE, 3)], result: (ITEM_DARK_PRISMARINE_SLAB, 6), name: "dark_prismarine_slab", shaped: true, cells: &[(0, 0, ITEM_DARK_PRISMARINE), (1, 0, ITEM_DARK_PRISMARINE), (2, 0, ITEM_DARK_PRISMARINE)] },
    Recipe { needs: &[(ITEM_DARK_PRISMARINE, 6)], result: (ITEM_DARK_PRISMARINE_STAIRS, 4), name: "dark_prismarine_stairs", shaped: true, cells: &[(0, 0, ITEM_DARK_PRISMARINE), (0, 1, ITEM_DARK_PRISMARINE), (1, 1, ITEM_DARK_PRISMARINE), (0, 2, ITEM_DARK_PRISMARINE), (1, 2, ITEM_DARK_PRISMARINE), (2, 2, ITEM_DARK_PRISMARINE)] },
    Recipe { needs: &[(ITEM_GLASS, 3), (ITEM_QUARTZ, 3), (TAG_WOODEN_SLABS, 3)], result: (ITEM_DAYLIGHT_DETECTOR, 1), name: "daylight_detector", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_QUARTZ), (1, 1, ITEM_QUARTZ), (2, 1, ITEM_QUARTZ), (0, 2, TAG_WOODEN_SLABS), (1, 2, TAG_WOODEN_SLABS), (2, 2, TAG_WOODEN_SLABS)] },
    Recipe { needs: &[(ITEM_BRICK, 4)], result: (ITEM_DECORATED_POT, 1), name: "decorated_pot_simple", shaped: true, cells: &[(1, 0, ITEM_BRICK), (0, 1, ITEM_BRICK), (2, 1, ITEM_BRICK), (1, 2, ITEM_BRICK)] },
    Recipe { needs: &[(ITEM_DEEPSLATE_BRICKS, 3)], result: (ITEM_DEEPSLATE_BRICK_SLAB, 6), name: "deepslate_brick_slab", shaped: true, cells: &[(0, 0, ITEM_DEEPSLATE_BRICKS), (1, 0, ITEM_DEEPSLATE_BRICKS), (2, 0, ITEM_DEEPSLATE_BRICKS)] },
    Recipe { needs: &[(ITEM_DEEPSLATE_BRICKS, 6)], result: (ITEM_DEEPSLATE_BRICK_STAIRS, 4), name: "deepslate_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_DEEPSLATE_BRICKS), (0, 1, ITEM_DEEPSLATE_BRICKS), (1, 1, ITEM_DEEPSLATE_BRICKS), (0, 2, ITEM_DEEPSLATE_BRICKS), (1, 2, ITEM_DEEPSLATE_BRICKS), (2, 2, ITEM_DEEPSLATE_BRICKS)] },
    Recipe { needs: &[(ITEM_DEEPSLATE_BRICKS, 6)], result: (ITEM_DEEPSLATE_BRICK_WALL, 6), name: "deepslate_brick_wall", shaped: true, cells: &[(0, 0, ITEM_DEEPSLATE_BRICKS), (1, 0, ITEM_DEEPSLATE_BRICKS), (2, 0, ITEM_DEEPSLATE_BRICKS), (0, 1, ITEM_DEEPSLATE_BRICKS), (1, 1, ITEM_DEEPSLATE_BRICKS), (2, 1, ITEM_DEEPSLATE_BRICKS)] },
    Recipe { needs: &[(ITEM_POLISHED_DEEPSLATE, 4)], result: (ITEM_DEEPSLATE_BRICKS, 4), name: "deepslate_bricks", shaped: true, cells: &[(0, 0, ITEM_POLISHED_DEEPSLATE), (1, 0, ITEM_POLISHED_DEEPSLATE), (0, 1, ITEM_POLISHED_DEEPSLATE), (1, 1, ITEM_POLISHED_DEEPSLATE)] },
    Recipe { needs: &[(ITEM_DEEPSLATE_TILES, 3)], result: (ITEM_DEEPSLATE_TILE_SLAB, 6), name: "deepslate_tile_slab", shaped: true, cells: &[(0, 0, ITEM_DEEPSLATE_TILES), (1, 0, ITEM_DEEPSLATE_TILES), (2, 0, ITEM_DEEPSLATE_TILES)] },
    Recipe { needs: &[(ITEM_DEEPSLATE_TILES, 6)], result: (ITEM_DEEPSLATE_TILE_STAIRS, 4), name: "deepslate_tile_stairs", shaped: true, cells: &[(0, 0, ITEM_DEEPSLATE_TILES), (0, 1, ITEM_DEEPSLATE_TILES), (1, 1, ITEM_DEEPSLATE_TILES), (0, 2, ITEM_DEEPSLATE_TILES), (1, 2, ITEM_DEEPSLATE_TILES), (2, 2, ITEM_DEEPSLATE_TILES)] },
    Recipe { needs: &[(ITEM_DEEPSLATE_TILES, 6)], result: (ITEM_DEEPSLATE_TILE_WALL, 6), name: "deepslate_tile_wall", shaped: true, cells: &[(0, 0, ITEM_DEEPSLATE_TILES), (1, 0, ITEM_DEEPSLATE_TILES), (2, 0, ITEM_DEEPSLATE_TILES), (0, 1, ITEM_DEEPSLATE_TILES), (1, 1, ITEM_DEEPSLATE_TILES), (2, 1, ITEM_DEEPSLATE_TILES)] },
    Recipe { needs: &[(ITEM_DEEPSLATE_BRICKS, 4)], result: (ITEM_DEEPSLATE_TILES, 4), name: "deepslate_tiles", shaped: true, cells: &[(0, 0, ITEM_DEEPSLATE_BRICKS), (1, 0, ITEM_DEEPSLATE_BRICKS), (0, 1, ITEM_DEEPSLATE_BRICKS), (1, 1, ITEM_DEEPSLATE_BRICKS)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (ITEM_REDSTONE, 1), (ITEM_STONE_PRESSURE_PLATE, 1)], result: (ITEM_DETECTOR_RAIL, 6), name: "detector_rail", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_STONE_PRESSURE_PLATE), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (1, 2, ITEM_REDSTONE), (2, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_DIAMOND_BLOCK, 1)], result: (ITEM_DIAMOND, 9), name: "diamond", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 3)], result: (ITEM_DIAMOND_AXE, 1), name: "diamond_axe", shaped: true, cells: &[(0, 0, TAG_DIAMOND_TOOL_MATERIALS), (1, 0, TAG_DIAMOND_TOOL_MATERIALS), (0, 1, TAG_DIAMOND_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_DIAMOND, 9)], result: (ITEM_DIAMOND_BLOCK, 1), name: "diamond_block", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_DIAMOND), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_DIAMOND), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_DIAMOND, 4)], result: (ITEM_DIAMOND_BOOTS, 1), name: "diamond_boots", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (2, 1, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_DIAMOND, 8)], result: (ITEM_DIAMOND_CHESTPLATE, 1), name: "diamond_chestplate", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_DIAMOND), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_DIAMOND, 5)], result: (ITEM_DIAMOND_HELMET, 1), name: "diamond_helmet", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_DIAMOND), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (2, 1, ITEM_DIAMOND)] },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 2)], result: (ITEM_DIAMOND_HOE, 1), name: "diamond_hoe", shaped: true, cells: &[(0, 0, TAG_DIAMOND_TOOL_MATERIALS), (1, 0, TAG_DIAMOND_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7)], result: (ITEM_DIAMOND_LEGGINGS, 1), name: "diamond_leggings", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_DIAMOND), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 3)], result: (ITEM_DIAMOND_PICKAXE, 1), name: "diamond_pickaxe", shaped: true, cells: &[(0, 0, TAG_DIAMOND_TOOL_MATERIALS), (1, 0, TAG_DIAMOND_TOOL_MATERIALS), (2, 0, TAG_DIAMOND_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 1)], result: (ITEM_DIAMOND_SHOVEL, 1), name: "diamond_shovel", shaped: true, cells: &[(0, 0, TAG_DIAMOND_TOOL_MATERIALS), (0, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_DIAMOND_TOOL_MATERIALS, 1)], result: (ITEM_DIAMOND_SPEAR, 1), name: "diamond_spear", shaped: true, cells: &[(2, 0, TAG_DIAMOND_TOOL_MATERIALS), (1, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 1), (TAG_DIAMOND_TOOL_MATERIALS, 2)], result: (ITEM_DIAMOND_SWORD, 1), name: "diamond_sword", shaped: true, cells: &[(0, 0, TAG_DIAMOND_TOOL_MATERIALS), (0, 1, TAG_DIAMOND_TOOL_MATERIALS), (0, 2, STICK)] },
    Recipe { needs: &[(COBBLESTONE, 2), (ITEM_QUARTZ, 2)], result: (ITEM_DIORITE, 2), name: "diorite", shaped: true, cells: &[(0, 0, COBBLESTONE), (1, 0, ITEM_QUARTZ), (0, 1, ITEM_QUARTZ), (1, 1, COBBLESTONE)] },
    Recipe { needs: &[(ITEM_DIORITE, 3)], result: (ITEM_DIORITE_SLAB, 6), name: "diorite_slab", shaped: true, cells: &[(0, 0, ITEM_DIORITE), (1, 0, ITEM_DIORITE), (2, 0, ITEM_DIORITE)] },
    Recipe { needs: &[(ITEM_DIORITE, 6)], result: (ITEM_DIORITE_STAIRS, 4), name: "diorite_stairs", shaped: true, cells: &[(0, 0, ITEM_DIORITE), (0, 1, ITEM_DIORITE), (1, 1, ITEM_DIORITE), (0, 2, ITEM_DIORITE), (1, 2, ITEM_DIORITE), (2, 2, ITEM_DIORITE)] },
    Recipe { needs: &[(ITEM_DIORITE, 6)], result: (ITEM_DIORITE_WALL, 6), name: "diorite_wall", shaped: true, cells: &[(0, 0, ITEM_DIORITE), (1, 0, ITEM_DIORITE), (2, 0, ITEM_DIORITE), (0, 1, ITEM_DIORITE), (1, 1, ITEM_DIORITE), (2, 1, ITEM_DIORITE)] },
    Recipe { needs: &[(ITEM_BOW, 1), (COBBLESTONE, 7), (ITEM_REDSTONE, 1)], result: (ITEM_DISPENSER, 1), name: "dispenser", shaped: true, cells: &[(0, 0, COBBLESTONE), (1, 0, COBBLESTONE), (2, 0, COBBLESTONE), (0, 1, COBBLESTONE), (1, 1, ITEM_BOW), (2, 1, COBBLESTONE), (0, 2, COBBLESTONE), (1, 2, ITEM_REDSTONE), (2, 2, COBBLESTONE)] },
    Recipe { needs: &[(ITEM_GHAST_TEAR, 8), (ITEM_SOUL_SAND, 1)], result: (ITEM_DRIED_GHAST, 1), name: "dried_ghast", shaped: true, cells: &[(0, 0, ITEM_GHAST_TEAR), (1, 0, ITEM_GHAST_TEAR), (2, 0, ITEM_GHAST_TEAR), (0, 1, ITEM_GHAST_TEAR), (1, 1, ITEM_SOUL_SAND), (2, 1, ITEM_GHAST_TEAR), (0, 2, ITEM_GHAST_TEAR), (1, 2, ITEM_GHAST_TEAR), (2, 2, ITEM_GHAST_TEAR)] },
    Recipe { needs: &[(ITEM_DRIED_KELP_BLOCK, 1)], result: (ITEM_DRIED_KELP, 9), name: "dried_kelp", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_DRIED_KELP, 9)], result: (ITEM_DRIED_KELP_BLOCK, 1), name: "dried_kelp_block", shaped: true, cells: &[(0, 0, ITEM_DRIED_KELP), (1, 0, ITEM_DRIED_KELP), (2, 0, ITEM_DRIED_KELP), (0, 1, ITEM_DRIED_KELP), (1, 1, ITEM_DRIED_KELP), (2, 1, ITEM_DRIED_KELP), (0, 2, ITEM_DRIED_KELP), (1, 2, ITEM_DRIED_KELP), (2, 2, ITEM_DRIED_KELP)] },
    Recipe { needs: &[(ITEM_POINTED_DRIPSTONE, 4)], result: (ITEM_DRIPSTONE_BLOCK, 1), name: "dripstone_block", shaped: true, cells: &[(0, 0, ITEM_POINTED_DRIPSTONE), (1, 0, ITEM_POINTED_DRIPSTONE), (0, 1, ITEM_POINTED_DRIPSTONE), (1, 1, ITEM_POINTED_DRIPSTONE)] },
    Recipe { needs: &[(COBBLESTONE, 7), (ITEM_REDSTONE, 1)], result: (ITEM_DROPPER, 1), name: "dropper", shaped: true, cells: &[(0, 0, COBBLESTONE), (1, 0, COBBLESTONE), (2, 0, COBBLESTONE), (0, 1, COBBLESTONE), (2, 1, COBBLESTONE), (0, 2, COBBLESTONE), (1, 2, ITEM_REDSTONE), (2, 2, COBBLESTONE)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_DUNE_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_SANDSTONE, 1)], result: (ITEM_DUNE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "dune_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_DUNE_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_SANDSTONE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_2, 1)], result: (ITEM_BLACK_BED, 1), name: "dye_black_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_3, 1)], result: (ITEM_BLACK_CARPET, 1), name: "dye_black_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_4, 1)], result: (ITEM_BLACK_CUSHION, 1), name: "dye_black_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_5, 1)], result: (ITEM_BLACK_HARNESS, 1), name: "dye_black_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_6, 1)], result: (ITEM_BLACK_WOOL, 1), name: "dye_black_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_7, 1)], result: (ITEM_BLACK_WOOL_SLAB, 1), name: "dye_black_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (TAG_ALT_8, 1)], result: (ITEM_BLACK_WOOL_STAIRS, 1), name: "dye_black_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_9, 1)], result: (ITEM_BLUE_BED, 1), name: "dye_blue_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_10, 1)], result: (ITEM_BLUE_CARPET, 1), name: "dye_blue_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_11, 1)], result: (ITEM_BLUE_CUSHION, 1), name: "dye_blue_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_12, 1)], result: (ITEM_BLUE_HARNESS, 1), name: "dye_blue_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_13, 1)], result: (ITEM_BLUE_WOOL, 1), name: "dye_blue_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_14, 1)], result: (ITEM_BLUE_WOOL_SLAB, 1), name: "dye_blue_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (TAG_ALT_15, 1)], result: (ITEM_BLUE_WOOL_STAIRS, 1), name: "dye_blue_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_16, 1)], result: (ITEM_BROWN_BED, 1), name: "dye_brown_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_17, 1)], result: (ITEM_BROWN_CARPET, 1), name: "dye_brown_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_18, 1)], result: (ITEM_BROWN_CUSHION, 1), name: "dye_brown_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_19, 1)], result: (ITEM_BROWN_HARNESS, 1), name: "dye_brown_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_20, 1)], result: (ITEM_BROWN_WOOL, 1), name: "dye_brown_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_21, 1)], result: (ITEM_BROWN_WOOL_SLAB, 1), name: "dye_brown_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BROWN_DYE, 1), (TAG_ALT_22, 1)], result: (ITEM_BROWN_WOOL_STAIRS, 1), name: "dye_brown_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_23, 1)], result: (ITEM_CYAN_BED, 1), name: "dye_cyan_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_24, 1)], result: (ITEM_CYAN_CARPET, 1), name: "dye_cyan_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_25, 1)], result: (ITEM_CYAN_CUSHION, 1), name: "dye_cyan_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_26, 1)], result: (ITEM_CYAN_HARNESS, 1), name: "dye_cyan_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_27, 1)], result: (ITEM_CYAN_WOOL, 1), name: "dye_cyan_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_28, 1)], result: (ITEM_CYAN_WOOL_SLAB, 1), name: "dye_cyan_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CYAN_DYE, 1), (TAG_ALT_29, 1)], result: (ITEM_CYAN_WOOL_STAIRS, 1), name: "dye_cyan_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_30, 1)], result: (ITEM_GRAY_BED, 1), name: "dye_gray_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_31, 1)], result: (ITEM_GRAY_CARPET, 1), name: "dye_gray_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_32, 1)], result: (ITEM_GRAY_CUSHION, 1), name: "dye_gray_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_33, 1)], result: (ITEM_GRAY_HARNESS, 1), name: "dye_gray_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_34, 1)], result: (ITEM_GRAY_WOOL, 1), name: "dye_gray_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_35, 1)], result: (ITEM_GRAY_WOOL_SLAB, 1), name: "dye_gray_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (TAG_ALT_36, 1)], result: (ITEM_GRAY_WOOL_STAIRS, 1), name: "dye_gray_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_37, 1)], result: (ITEM_GREEN_BED, 1), name: "dye_green_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_38, 1)], result: (ITEM_GREEN_CARPET, 1), name: "dye_green_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_39, 1)], result: (ITEM_GREEN_CUSHION, 1), name: "dye_green_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_40, 1)], result: (ITEM_GREEN_HARNESS, 1), name: "dye_green_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_41, 1)], result: (ITEM_GREEN_WOOL, 1), name: "dye_green_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_42, 1)], result: (ITEM_GREEN_WOOL_SLAB, 1), name: "dye_green_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (TAG_ALT_43, 1)], result: (ITEM_GREEN_WOOL_STAIRS, 1), name: "dye_green_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_44, 1)], result: (ITEM_LIGHT_BLUE_BED, 1), name: "dye_light_blue_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_45, 1)], result: (ITEM_LIGHT_BLUE_CARPET, 1), name: "dye_light_blue_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_46, 1)], result: (ITEM_LIGHT_BLUE_CUSHION, 1), name: "dye_light_blue_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_47, 1)], result: (ITEM_LIGHT_BLUE_HARNESS, 1), name: "dye_light_blue_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_48, 1)], result: (ITEM_LIGHT_BLUE_WOOL, 1), name: "dye_light_blue_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_49, 1)], result: (ITEM_LIGHT_BLUE_WOOL_SLAB, 1), name: "dye_light_blue_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (TAG_ALT_50, 1)], result: (ITEM_LIGHT_BLUE_WOOL_STAIRS, 1), name: "dye_light_blue_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_51, 1)], result: (ITEM_LIGHT_GRAY_BED, 1), name: "dye_light_gray_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_52, 1)], result: (ITEM_LIGHT_GRAY_CARPET, 1), name: "dye_light_gray_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_53, 1)], result: (ITEM_LIGHT_GRAY_CUSHION, 1), name: "dye_light_gray_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_54, 1)], result: (ITEM_LIGHT_GRAY_HARNESS, 1), name: "dye_light_gray_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_55, 1)], result: (ITEM_LIGHT_GRAY_WOOL, 1), name: "dye_light_gray_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_56, 1)], result: (ITEM_LIGHT_GRAY_WOOL_SLAB, 1), name: "dye_light_gray_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (TAG_ALT_57, 1)], result: (ITEM_LIGHT_GRAY_WOOL_STAIRS, 1), name: "dye_light_gray_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_58, 1)], result: (ITEM_LIME_BED, 1), name: "dye_lime_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_59, 1)], result: (ITEM_LIME_CARPET, 1), name: "dye_lime_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_60, 1)], result: (ITEM_LIME_CUSHION, 1), name: "dye_lime_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_61, 1)], result: (ITEM_LIME_HARNESS, 1), name: "dye_lime_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_62, 1)], result: (ITEM_LIME_WOOL, 1), name: "dye_lime_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_63, 1)], result: (ITEM_LIME_WOOL_SLAB, 1), name: "dye_lime_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (TAG_ALT_64, 1)], result: (ITEM_LIME_WOOL_STAIRS, 1), name: "dye_lime_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_65, 1)], result: (ITEM_MAGENTA_BED, 1), name: "dye_magenta_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_66, 1)], result: (ITEM_MAGENTA_CARPET, 1), name: "dye_magenta_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_67, 1)], result: (ITEM_MAGENTA_CUSHION, 1), name: "dye_magenta_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_68, 1)], result: (ITEM_MAGENTA_HARNESS, 1), name: "dye_magenta_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_69, 1)], result: (ITEM_MAGENTA_WOOL, 1), name: "dye_magenta_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_70, 1)], result: (ITEM_MAGENTA_WOOL_SLAB, 1), name: "dye_magenta_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (TAG_ALT_71, 1)], result: (ITEM_MAGENTA_WOOL_STAIRS, 1), name: "dye_magenta_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_72, 1)], result: (ITEM_ORANGE_BED, 1), name: "dye_orange_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_73, 1)], result: (ITEM_ORANGE_CARPET, 1), name: "dye_orange_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_74, 1)], result: (ITEM_ORANGE_CUSHION, 1), name: "dye_orange_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_75, 1)], result: (ITEM_ORANGE_HARNESS, 1), name: "dye_orange_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_76, 1)], result: (ITEM_ORANGE_WOOL, 1), name: "dye_orange_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_77, 1)], result: (ITEM_ORANGE_WOOL_SLAB, 1), name: "dye_orange_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (TAG_ALT_78, 1)], result: (ITEM_ORANGE_WOOL_STAIRS, 1), name: "dye_orange_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_79, 1)], result: (ITEM_PINK_BED, 1), name: "dye_pink_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_80, 1)], result: (ITEM_PINK_CARPET, 1), name: "dye_pink_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_81, 1)], result: (ITEM_PINK_CUSHION, 1), name: "dye_pink_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_82, 1)], result: (ITEM_PINK_HARNESS, 1), name: "dye_pink_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_83, 1)], result: (ITEM_PINK_WOOL, 1), name: "dye_pink_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_84, 1)], result: (ITEM_PINK_WOOL_SLAB, 1), name: "dye_pink_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (TAG_ALT_85, 1)], result: (ITEM_PINK_WOOL_STAIRS, 1), name: "dye_pink_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_86, 1)], result: (ITEM_PURPLE_BED, 1), name: "dye_purple_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_87, 1)], result: (ITEM_PURPLE_CARPET, 1), name: "dye_purple_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_88, 1)], result: (ITEM_PURPLE_CUSHION, 1), name: "dye_purple_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_89, 1)], result: (ITEM_PURPLE_HARNESS, 1), name: "dye_purple_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_90, 1)], result: (ITEM_PURPLE_WOOL, 1), name: "dye_purple_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_91, 1)], result: (ITEM_PURPLE_WOOL_SLAB, 1), name: "dye_purple_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (TAG_ALT_92, 1)], result: (ITEM_PURPLE_WOOL_STAIRS, 1), name: "dye_purple_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_93, 1)], result: (ITEM_RED_BED, 1), name: "dye_red_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_94, 1)], result: (ITEM_RED_CARPET, 1), name: "dye_red_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_95, 1)], result: (ITEM_RED_CUSHION, 1), name: "dye_red_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_96, 1)], result: (ITEM_RED_HARNESS, 1), name: "dye_red_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_97, 1)], result: (ITEM_RED_WOOL, 1), name: "dye_red_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_98, 1)], result: (ITEM_RED_WOOL_SLAB, 1), name: "dye_red_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (TAG_ALT_99, 1)], result: (ITEM_RED_WOOL_STAIRS, 1), name: "dye_red_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_100, 1)], result: (ITEM_WHITE_BED, 1), name: "dye_white_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_101, 1)], result: (ITEM_WHITE_CARPET, 1), name: "dye_white_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_102, 1)], result: (ITEM_WHITE_CUSHION, 1), name: "dye_white_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_103, 1)], result: (ITEM_WHITE_HARNESS, 1), name: "dye_white_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_104, 1)], result: (ITEM_WHITE_WOOL, 1), name: "dye_white_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_105, 1)], result: (ITEM_WHITE_WOOL_SLAB, 1), name: "dye_white_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_DYE, 1), (TAG_ALT_106, 1)], result: (ITEM_WHITE_WOOL_STAIRS, 1), name: "dye_white_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_107, 1)], result: (ITEM_YELLOW_BED, 1), name: "dye_yellow_bed", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_108, 1)], result: (ITEM_YELLOW_CARPET, 1), name: "dye_yellow_carpet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_109, 1)], result: (ITEM_YELLOW_CUSHION, 1), name: "dye_yellow_cushion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_110, 1)], result: (ITEM_YELLOW_HARNESS, 1), name: "dye_yellow_harness", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_111, 1)], result: (ITEM_YELLOW_WOOL, 1), name: "dye_yellow_wool", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_112, 1)], result: (ITEM_YELLOW_WOOL_SLAB, 1), name: "dye_yellow_wool_slab", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_DYE, 1), (TAG_ALT_113, 1)], result: (ITEM_YELLOW_WOOL_STAIRS, 1), name: "dye_yellow_wool_stairs", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EMERALD_BLOCK, 1)], result: (ITEM_EMERALD, 9), name: "emerald", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EMERALD, 9)], result: (ITEM_EMERALD_BLOCK, 1), name: "emerald_block", shaped: true, cells: &[(0, 0, ITEM_EMERALD), (1, 0, ITEM_EMERALD), (2, 0, ITEM_EMERALD), (0, 1, ITEM_EMERALD), (1, 1, ITEM_EMERALD), (2, 1, ITEM_EMERALD), (0, 2, ITEM_EMERALD), (1, 2, ITEM_EMERALD), (2, 2, ITEM_EMERALD)] },
    Recipe { needs: &[(ITEM_BOOK, 1), (ITEM_DIAMOND, 2), (ITEM_OBSIDIAN, 4)], result: (ITEM_ENCHANTING_TABLE, 1), name: "enchanting_table", shaped: true, cells: &[(1, 0, ITEM_BOOK), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_OBSIDIAN), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_OBSIDIAN), (1, 2, ITEM_OBSIDIAN), (2, 2, ITEM_OBSIDIAN)] },
    Recipe { needs: &[(ITEM_ENDER_EYE, 1), (ITEM_GHAST_TEAR, 1), (ITEM_GLASS, 7)], result: (ITEM_END_CRYSTAL, 1), name: "end_crystal", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_ENDER_EYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GHAST_TEAR), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_POPPED_CHORUS_FRUIT, 1)], result: (ITEM_END_ROD, 4), name: "end_rod", shaped: true, cells: &[(0, 0, ITEM_BLAZE_ROD), (0, 1, ITEM_POPPED_CHORUS_FRUIT)] },
    Recipe { needs: &[(ITEM_END_STONE_BRICKS, 3)], result: (ITEM_END_STONE_BRICK_SLAB, 6), name: "end_stone_brick_slab", shaped: true, cells: &[(0, 0, ITEM_END_STONE_BRICKS), (1, 0, ITEM_END_STONE_BRICKS), (2, 0, ITEM_END_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_END_STONE_BRICKS, 6)], result: (ITEM_END_STONE_BRICK_STAIRS, 4), name: "end_stone_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_END_STONE_BRICKS), (0, 1, ITEM_END_STONE_BRICKS), (1, 1, ITEM_END_STONE_BRICKS), (0, 2, ITEM_END_STONE_BRICKS), (1, 2, ITEM_END_STONE_BRICKS), (2, 2, ITEM_END_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_END_STONE_BRICKS, 6)], result: (ITEM_END_STONE_BRICK_WALL, 6), name: "end_stone_brick_wall", shaped: true, cells: &[(0, 0, ITEM_END_STONE_BRICKS), (1, 0, ITEM_END_STONE_BRICKS), (2, 0, ITEM_END_STONE_BRICKS), (0, 1, ITEM_END_STONE_BRICKS), (1, 1, ITEM_END_STONE_BRICKS), (2, 1, ITEM_END_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_END_STONE, 4)], result: (ITEM_END_STONE_BRICKS, 4), name: "end_stone_bricks", shaped: true, cells: &[(0, 0, ITEM_END_STONE), (1, 0, ITEM_END_STONE), (0, 1, ITEM_END_STONE), (1, 1, ITEM_END_STONE)] },
    Recipe { needs: &[(ITEM_ENDER_EYE, 1), (ITEM_OBSIDIAN, 8)], result: (ITEM_ENDER_CHEST, 1), name: "ender_chest", shaped: true, cells: &[(0, 0, ITEM_OBSIDIAN), (1, 0, ITEM_OBSIDIAN), (2, 0, ITEM_OBSIDIAN), (0, 1, ITEM_OBSIDIAN), (1, 1, ITEM_ENDER_EYE), (2, 1, ITEM_OBSIDIAN), (0, 2, ITEM_OBSIDIAN), (1, 2, ITEM_OBSIDIAN), (2, 2, ITEM_OBSIDIAN)] },
    Recipe { needs: &[(ITEM_BLAZE_POWDER, 1), (ITEM_ENDER_PEARL, 1)], result: (ITEM_ENDER_EYE, 1), name: "ender_eye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER_SLAB, 2)], result: (ITEM_EXPOSED_CHISELED_COPPER, 1), name: "exposed_chiseled_copper", shaped: true, cells: &[(0, 0, ITEM_EXPOSED_CUT_COPPER_SLAB), (0, 1, ITEM_EXPOSED_CUT_COPPER_SLAB)] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_EXPOSED_COPPER, 3), (ITEM_REDSTONE, 1)], result: (ITEM_EXPOSED_COPPER_BULB, 4), name: "exposed_copper_bulb", shaped: true, cells: &[(1, 0, ITEM_EXPOSED_COPPER), (0, 1, ITEM_EXPOSED_COPPER), (1, 1, ITEM_BLAZE_ROD), (2, 1, ITEM_EXPOSED_COPPER), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER, 4)], result: (ITEM_EXPOSED_COPPER_GRATE, 4), name: "exposed_copper_grate", shaped: true, cells: &[(1, 0, ITEM_EXPOSED_COPPER), (0, 1, ITEM_EXPOSED_COPPER), (2, 1, ITEM_EXPOSED_COPPER), (1, 2, ITEM_EXPOSED_COPPER)] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER, 4)], result: (ITEM_EXPOSED_CUT_COPPER, 4), name: "exposed_cut_copper", shaped: true, cells: &[(0, 0, ITEM_EXPOSED_COPPER), (1, 0, ITEM_EXPOSED_COPPER), (0, 1, ITEM_EXPOSED_COPPER), (1, 1, ITEM_EXPOSED_COPPER)] },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER, 3)], result: (ITEM_EXPOSED_CUT_COPPER_SLAB, 6), name: "exposed_cut_copper_slab", shaped: true, cells: &[(0, 0, ITEM_EXPOSED_CUT_COPPER), (1, 0, ITEM_EXPOSED_CUT_COPPER), (2, 0, ITEM_EXPOSED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER, 6)], result: (ITEM_EXPOSED_CUT_COPPER_STAIRS, 4), name: "exposed_cut_copper_stairs", shaped: true, cells: &[(0, 0, ITEM_EXPOSED_CUT_COPPER), (0, 1, ITEM_EXPOSED_CUT_COPPER), (1, 1, ITEM_EXPOSED_CUT_COPPER), (0, 2, ITEM_EXPOSED_CUT_COPPER), (1, 2, ITEM_EXPOSED_CUT_COPPER), (2, 2, ITEM_EXPOSED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_END_STONE, 1), (ITEM_EYE_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_EYE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "eye_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_EYE_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_END_STONE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_BROWN_MUSHROOM, 1), (ITEM_SPIDER_EYE, 1), (ITEM_SUGAR, 1)], result: (ITEM_FERMENTED_SPIDER_EYE, 1), name: "fermented_spider_eye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BRICKS, 1), (ITEM_PAPER, 1)], result: (ITEM_FIELD_MASONED_BANNER_PATTERN, 1), name: "field_masoned_banner_pattern", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLAZE_POWDER, 1), (ITEM_GUNPOWDER, 1), (TAG_COALS, 1)], result: (ITEM_FIRE_CHARGE, 3), name: "fire_charge", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GUNPOWDER, 1), (ITEM_PAPER, 1)], result: (ITEM_FIREWORK_ROCKET, 3), name: "firework_rocket_simple", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 3), (ITEM_STRING, 2)], result: (ITEM_FISHING_ROD, 1), name: "fishing_rod", shaped: true, cells: &[(2, 0, STICK), (1, 1, STICK), (2, 1, ITEM_STRING), (0, 2, STICK), (2, 2, ITEM_STRING)] },
    Recipe { needs: &[(ITEM_FLINT, 2), (TAG_PLANKS, 4)], result: (ITEM_FLETCHING_TABLE, 1), name: "fletching_table", shaped: true, cells: &[(0, 0, ITEM_FLINT), (1, 0, ITEM_FLINT), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_FLINT, 1), (ITEM_IRON_INGOT, 1)], result: (ITEM_FLINT_AND_STEEL, 1), name: "flint_and_steel", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BREEZE_ROD, 1), (ITEM_DIAMOND, 7), (ITEM_FLOW_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_FLOW_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "flow_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_FLOW_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_BREEZE_ROD), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_OXEYE_DAISY, 1), (ITEM_PAPER, 1)], result: (ITEM_FLOWER_BANNER_PATTERN, 1), name: "flower_banner_pattern", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BRICK, 3)], result: (ITEM_FLOWER_POT, 1), name: "flower_pot", shaped: true, cells: &[(0, 0, ITEM_BRICK), (2, 0, ITEM_BRICK), (1, 1, ITEM_BRICK)] },
    Recipe { needs: &[(TAG_STONE_CRAFTING_MATERIALS, 8)], result: (ITEM_FURNACE, 1), name: "furnace", shaped: true, cells: &[(0, 0, TAG_STONE_CRAFTING_MATERIALS), (1, 0, TAG_STONE_CRAFTING_MATERIALS), (2, 0, TAG_STONE_CRAFTING_MATERIALS), (0, 1, TAG_STONE_CRAFTING_MATERIALS), (2, 1, TAG_STONE_CRAFTING_MATERIALS), (0, 2, TAG_STONE_CRAFTING_MATERIALS), (1, 2, TAG_STONE_CRAFTING_MATERIALS), (2, 2, TAG_STONE_CRAFTING_MATERIALS)] },
    Recipe { needs: &[(ITEM_FURNACE, 1), (ITEM_MINECART, 1)], result: (ITEM_FURNACE_MINECART, 1), name: "furnace_minecart", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 3)], result: (ITEM_GLASS_BOTTLE, 3), name: "glass_bottle", shaped: true, cells: &[(0, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (1, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 6)], result: (ITEM_GLASS_PANE, 16), name: "glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_GLASS), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GOLD_NUGGET, 8), (ITEM_MELON_SLICE, 1)], result: (ITEM_GLISTERING_MELON_SLICE, 1), name: "glistering_melon_slice", shaped: true, cells: &[(0, 0, ITEM_GOLD_NUGGET), (1, 0, ITEM_GOLD_NUGGET), (2, 0, ITEM_GOLD_NUGGET), (0, 1, ITEM_GOLD_NUGGET), (1, 1, ITEM_MELON_SLICE), (2, 1, ITEM_GOLD_NUGGET), (0, 2, ITEM_GOLD_NUGGET), (1, 2, ITEM_GOLD_NUGGET), (2, 2, ITEM_GOLD_NUGGET)] },
    Recipe { needs: &[(ITEM_GLOW_INK_SAC, 1), (ITEM_ITEM_FRAME, 1)], result: (ITEM_GLOW_ITEM_FRAME, 1), name: "glow_item_frame", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLOWSTONE_DUST, 4)], result: (ITEM_GLOWSTONE, 1), name: "glowstone", shaped: true, cells: &[(0, 0, ITEM_GLOWSTONE_DUST), (1, 0, ITEM_GLOWSTONE_DUST), (0, 1, ITEM_GLOWSTONE_DUST), (1, 1, ITEM_GLOWSTONE_DUST)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 9)], result: (ITEM_GOLD_BLOCK, 1), name: "gold_block", shaped: true, cells: &[(0, 0, ITEM_GOLD_INGOT), (1, 0, ITEM_GOLD_INGOT), (2, 0, ITEM_GOLD_INGOT), (0, 1, ITEM_GOLD_INGOT), (1, 1, ITEM_GOLD_INGOT), (2, 1, ITEM_GOLD_INGOT), (0, 2, ITEM_GOLD_INGOT), (1, 2, ITEM_GOLD_INGOT), (2, 2, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(ITEM_GOLD_BLOCK, 1)], result: (ITEM_GOLD_INGOT, 9), name: "gold_ingot_from_gold_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GOLD_NUGGET, 9)], result: (ITEM_GOLD_INGOT, 1), name: "gold_ingot_from_nuggets", shaped: true, cells: &[(0, 0, ITEM_GOLD_NUGGET), (1, 0, ITEM_GOLD_NUGGET), (2, 0, ITEM_GOLD_NUGGET), (0, 1, ITEM_GOLD_NUGGET), (1, 1, ITEM_GOLD_NUGGET), (2, 1, ITEM_GOLD_NUGGET), (0, 2, ITEM_GOLD_NUGGET), (1, 2, ITEM_GOLD_NUGGET), (2, 2, ITEM_GOLD_NUGGET)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 1)], result: (ITEM_GOLD_NUGGET, 9), name: "gold_nugget", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_APPLE, 1), (ITEM_GOLD_INGOT, 8)], result: (ITEM_GOLDEN_APPLE, 1), name: "golden_apple", shaped: true, cells: &[(0, 0, ITEM_GOLD_INGOT), (1, 0, ITEM_GOLD_INGOT), (2, 0, ITEM_GOLD_INGOT), (0, 1, ITEM_GOLD_INGOT), (1, 1, ITEM_APPLE), (2, 1, ITEM_GOLD_INGOT), (0, 2, ITEM_GOLD_INGOT), (1, 2, ITEM_GOLD_INGOT), (2, 2, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 3)], result: (ITEM_GOLDEN_AXE, 1), name: "golden_axe", shaped: true, cells: &[(0, 0, TAG_GOLD_TOOL_MATERIALS), (1, 0, TAG_GOLD_TOOL_MATERIALS), (0, 1, TAG_GOLD_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 4)], result: (ITEM_GOLDEN_BOOTS, 1), name: "golden_boots", shaped: true, cells: &[(0, 0, ITEM_GOLD_INGOT), (2, 0, ITEM_GOLD_INGOT), (0, 1, ITEM_GOLD_INGOT), (2, 1, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(ITEM_CARROT, 1), (ITEM_GOLD_NUGGET, 8)], result: (ITEM_GOLDEN_CARROT, 1), name: "golden_carrot", shaped: true, cells: &[(0, 0, ITEM_GOLD_NUGGET), (1, 0, ITEM_GOLD_NUGGET), (2, 0, ITEM_GOLD_NUGGET), (0, 1, ITEM_GOLD_NUGGET), (1, 1, ITEM_CARROT), (2, 1, ITEM_GOLD_NUGGET), (0, 2, ITEM_GOLD_NUGGET), (1, 2, ITEM_GOLD_NUGGET), (2, 2, ITEM_GOLD_NUGGET)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 8)], result: (ITEM_GOLDEN_CHESTPLATE, 1), name: "golden_chestplate", shaped: true, cells: &[(0, 0, ITEM_GOLD_INGOT), (2, 0, ITEM_GOLD_INGOT), (0, 1, ITEM_GOLD_INGOT), (1, 1, ITEM_GOLD_INGOT), (2, 1, ITEM_GOLD_INGOT), (0, 2, ITEM_GOLD_INGOT), (1, 2, ITEM_GOLD_INGOT), (2, 2, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(ITEM_DANDELION, 1), (ITEM_GOLD_NUGGET, 8)], result: (ITEM_GOLDEN_DANDELION, 1), name: "golden_dandelion", shaped: true, cells: &[(0, 0, ITEM_GOLD_NUGGET), (1, 0, ITEM_GOLD_NUGGET), (2, 0, ITEM_GOLD_NUGGET), (0, 1, ITEM_GOLD_NUGGET), (1, 1, ITEM_DANDELION), (2, 1, ITEM_GOLD_NUGGET), (0, 2, ITEM_GOLD_NUGGET), (1, 2, ITEM_GOLD_NUGGET), (2, 2, ITEM_GOLD_NUGGET)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 5)], result: (ITEM_GOLDEN_HELMET, 1), name: "golden_helmet", shaped: true, cells: &[(0, 0, ITEM_GOLD_INGOT), (1, 0, ITEM_GOLD_INGOT), (2, 0, ITEM_GOLD_INGOT), (0, 1, ITEM_GOLD_INGOT), (2, 1, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 2)], result: (ITEM_GOLDEN_HOE, 1), name: "golden_hoe", shaped: true, cells: &[(0, 0, TAG_GOLD_TOOL_MATERIALS), (1, 0, TAG_GOLD_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 7)], result: (ITEM_GOLDEN_LEGGINGS, 1), name: "golden_leggings", shaped: true, cells: &[(0, 0, ITEM_GOLD_INGOT), (1, 0, ITEM_GOLD_INGOT), (2, 0, ITEM_GOLD_INGOT), (0, 1, ITEM_GOLD_INGOT), (2, 1, ITEM_GOLD_INGOT), (0, 2, ITEM_GOLD_INGOT), (2, 2, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 3)], result: (ITEM_GOLDEN_PICKAXE, 1), name: "golden_pickaxe", shaped: true, cells: &[(0, 0, TAG_GOLD_TOOL_MATERIALS), (1, 0, TAG_GOLD_TOOL_MATERIALS), (2, 0, TAG_GOLD_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 1)], result: (ITEM_GOLDEN_SHOVEL, 1), name: "golden_shovel", shaped: true, cells: &[(0, 0, TAG_GOLD_TOOL_MATERIALS), (0, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_GOLD_TOOL_MATERIALS, 1)], result: (ITEM_GOLDEN_SPEAR, 1), name: "golden_spear", shaped: true, cells: &[(2, 0, TAG_GOLD_TOOL_MATERIALS), (1, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 1), (TAG_GOLD_TOOL_MATERIALS, 2)], result: (ITEM_GOLDEN_SWORD, 1), name: "golden_sword", shaped: true, cells: &[(0, 0, TAG_GOLD_TOOL_MATERIALS), (0, 1, TAG_GOLD_TOOL_MATERIALS), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_DIORITE, 1), (ITEM_QUARTZ, 1)], result: (ITEM_GRANITE, 1), name: "granite", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRANITE, 3)], result: (ITEM_GRANITE_SLAB, 6), name: "granite_slab", shaped: true, cells: &[(0, 0, ITEM_GRANITE), (1, 0, ITEM_GRANITE), (2, 0, ITEM_GRANITE)] },
    Recipe { needs: &[(ITEM_GRANITE, 6)], result: (ITEM_GRANITE_STAIRS, 4), name: "granite_stairs", shaped: true, cells: &[(0, 0, ITEM_GRANITE), (0, 1, ITEM_GRANITE), (1, 1, ITEM_GRANITE), (0, 2, ITEM_GRANITE), (1, 2, ITEM_GRANITE), (2, 2, ITEM_GRANITE)] },
    Recipe { needs: &[(ITEM_GRANITE, 6)], result: (ITEM_GRANITE_WALL, 6), name: "granite_wall", shaped: true, cells: &[(0, 0, ITEM_GRANITE), (1, 0, ITEM_GRANITE), (2, 0, ITEM_GRANITE), (0, 1, ITEM_GRANITE), (1, 1, ITEM_GRANITE), (2, 1, ITEM_GRANITE)] },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 6), (STICK, 1)], result: (ITEM_GRAY_BANNER, 1), name: "gray_banner", shaped: true, cells: &[(0, 0, ITEM_GRAY_WOOL), (1, 0, ITEM_GRAY_WOOL), (2, 0, ITEM_GRAY_WOOL), (0, 1, ITEM_GRAY_WOOL), (1, 1, ITEM_GRAY_WOOL), (2, 1, ITEM_GRAY_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_GRAY_BED, 1), name: "gray_bed", shaped: true, cells: &[(0, 0, ITEM_GRAY_WOOL), (1, 0, ITEM_GRAY_WOOL), (2, 0, ITEM_GRAY_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_GRAY_DYE, 1)], result: (ITEM_GRAY_CANDLE, 1), name: "gray_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 2)], result: (ITEM_GRAY_CARPET, 3), name: "gray_carpet", shaped: true, cells: &[(0, 0, ITEM_GRAY_WOOL), (1, 0, ITEM_GRAY_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAY_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_GRAY_CONCRETE_POWDER, 8), name: "gray_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_CONCRETE, 3)], result: (ITEM_GRAY_CONCRETE_SLAB, 6), name: "gray_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_GRAY_CONCRETE), (1, 0, ITEM_GRAY_CONCRETE), (2, 0, ITEM_GRAY_CONCRETE)] },
    Recipe { needs: &[(ITEM_GRAY_CONCRETE, 6)], result: (ITEM_GRAY_CONCRETE_STAIRS, 4), name: "gray_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_GRAY_CONCRETE), (0, 1, ITEM_GRAY_CONCRETE), (1, 1, ITEM_GRAY_CONCRETE), (0, 2, ITEM_GRAY_CONCRETE), (1, 2, ITEM_GRAY_CONCRETE), (2, 2, ITEM_GRAY_CONCRETE)] },
    Recipe { needs: &[(ITEM_GRAY_WOOL_SLAB, 3)], result: (ITEM_GRAY_CUSHION, 1), name: "gray_cushion", shaped: true, cells: &[(0, 0, ITEM_GRAY_WOOL_SLAB), (1, 0, ITEM_GRAY_WOOL_SLAB), (2, 0, ITEM_GRAY_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_GRAY_DYE, 2), name: "gray_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CLOSED_EYEBLOSSOM, 1)], result: (ITEM_GRAY_DYE, 1), name: "gray_dye_from_closed_eyeblossom", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_GRAY_WOOL, 1), (ITEM_LEATHER, 3)], result: (ITEM_GRAY_HARNESS, 1), name: "gray_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_GRAY_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_GRAY_DYE, 1)], result: (ITEM_GRAY_STAINED_GLASS, 8), name: "gray_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_GRAY_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GRAY_STAINED_GLASS, 6)], result: (ITEM_GRAY_STAINED_GLASS_PANE, 16), name: "gray_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GRAY_STAINED_GLASS), (1, 0, ITEM_GRAY_STAINED_GLASS), (2, 0, ITEM_GRAY_STAINED_GLASS), (0, 1, ITEM_GRAY_STAINED_GLASS), (1, 1, ITEM_GRAY_STAINED_GLASS), (2, 1, ITEM_GRAY_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_GRAY_DYE, 1)], result: (ITEM_GRAY_STAINED_GLASS_PANE, 8), name: "gray_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_GRAY_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_GRAY_TERRACOTTA, 8), name: "gray_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_GRAY_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 3)], result: (ITEM_GRAY_WOOL_SLAB, 6), name: "gray_wool_slab", shaped: true, cells: &[(0, 0, ITEM_GRAY_WOOL), (1, 0, ITEM_GRAY_WOOL), (2, 0, ITEM_GRAY_WOOL)] },
    Recipe { needs: &[(ITEM_GRAY_WOOL, 6)], result: (ITEM_GRAY_WOOL_STAIRS, 4), name: "gray_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_GRAY_WOOL), (0, 1, ITEM_GRAY_WOOL), (1, 1, ITEM_GRAY_WOOL), (0, 2, ITEM_GRAY_WOOL), (1, 2, ITEM_GRAY_WOOL), (2, 2, ITEM_GRAY_WOOL)] },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 6), (STICK, 1)], result: (ITEM_GREEN_BANNER, 1), name: "green_banner", shaped: true, cells: &[(0, 0, ITEM_GREEN_WOOL), (1, 0, ITEM_GREEN_WOOL), (2, 0, ITEM_GREEN_WOOL), (0, 1, ITEM_GREEN_WOOL), (1, 1, ITEM_GREEN_WOOL), (2, 1, ITEM_GREEN_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_GREEN_BED, 1), name: "green_bed", shaped: true, cells: &[(0, 0, ITEM_GREEN_WOOL), (1, 0, ITEM_GREEN_WOOL), (2, 0, ITEM_GREEN_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_GREEN_DYE, 1)], result: (ITEM_GREEN_CANDLE, 1), name: "green_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 2)], result: (ITEM_GREEN_CARPET, 3), name: "green_carpet", shaped: true, cells: &[(0, 0, ITEM_GREEN_WOOL), (1, 0, ITEM_GREEN_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GREEN_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_GREEN_CONCRETE_POWDER, 8), name: "green_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GREEN_CONCRETE, 3)], result: (ITEM_GREEN_CONCRETE_SLAB, 6), name: "green_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_GREEN_CONCRETE), (1, 0, ITEM_GREEN_CONCRETE), (2, 0, ITEM_GREEN_CONCRETE)] },
    Recipe { needs: &[(ITEM_GREEN_CONCRETE, 6)], result: (ITEM_GREEN_CONCRETE_STAIRS, 4), name: "green_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_GREEN_CONCRETE), (0, 1, ITEM_GREEN_CONCRETE), (1, 1, ITEM_GREEN_CONCRETE), (0, 2, ITEM_GREEN_CONCRETE), (1, 2, ITEM_GREEN_CONCRETE), (2, 2, ITEM_GREEN_CONCRETE)] },
    Recipe { needs: &[(ITEM_GREEN_WOOL_SLAB, 3)], result: (ITEM_GREEN_CUSHION, 1), name: "green_cushion", shaped: true, cells: &[(0, 0, ITEM_GREEN_WOOL_SLAB), (1, 0, ITEM_GREEN_WOOL_SLAB), (2, 0, ITEM_GREEN_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_GREEN_WOOL, 1), (ITEM_LEATHER, 3)], result: (ITEM_GREEN_HARNESS, 1), name: "green_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_GREEN_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_GREEN_DYE, 1)], result: (ITEM_GREEN_STAINED_GLASS, 8), name: "green_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_GREEN_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GREEN_STAINED_GLASS, 6)], result: (ITEM_GREEN_STAINED_GLASS_PANE, 16), name: "green_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GREEN_STAINED_GLASS), (1, 0, ITEM_GREEN_STAINED_GLASS), (2, 0, ITEM_GREEN_STAINED_GLASS), (0, 1, ITEM_GREEN_STAINED_GLASS), (1, 1, ITEM_GREEN_STAINED_GLASS), (2, 1, ITEM_GREEN_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_GREEN_DYE, 1)], result: (ITEM_GREEN_STAINED_GLASS_PANE, 8), name: "green_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_GREEN_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_GREEN_TERRACOTTA, 8), name: "green_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_GREEN_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 3)], result: (ITEM_GREEN_WOOL_SLAB, 6), name: "green_wool_slab", shaped: true, cells: &[(0, 0, ITEM_GREEN_WOOL), (1, 0, ITEM_GREEN_WOOL), (2, 0, ITEM_GREEN_WOOL)] },
    Recipe { needs: &[(ITEM_GREEN_WOOL, 6)], result: (ITEM_GREEN_WOOL_STAIRS, 4), name: "green_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_GREEN_WOOL), (0, 1, ITEM_GREEN_WOOL), (1, 1, ITEM_GREEN_WOOL), (0, 2, ITEM_GREEN_WOOL), (1, 2, ITEM_GREEN_WOOL), (2, 2, ITEM_GREEN_WOOL)] },
    Recipe { needs: &[(STICK, 2), (ITEM_STONE_SLAB, 1), (TAG_PLANKS, 2)], result: (ITEM_GRINDSTONE, 1), name: "grindstone", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_STONE_SLAB), (2, 0, STICK), (0, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1), (WHEAT, 1)], result: (ITEM_HAY_BLOCK, 1), name: "hay_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 2)], result: (ITEM_HEAVY_WEIGHTED_PRESSURE_PLATE, 1), name: "heavy_weighted_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_HONEY_BOTTLE, 4)], result: (ITEM_HONEY_BLOCK, 1), name: "honey_block", shaped: true, cells: &[(0, 0, ITEM_HONEY_BOTTLE), (1, 0, ITEM_HONEY_BOTTLE), (0, 1, ITEM_HONEY_BOTTLE), (1, 1, ITEM_HONEY_BOTTLE)] },
    Recipe { needs: &[(ITEM_GLASS_BOTTLE, 1), (ITEM_GLASS_BOTTLE, 1), (ITEM_GLASS_BOTTLE, 1), (ITEM_GLASS_BOTTLE, 1), (ITEM_HONEY_BLOCK, 1)], result: (ITEM_HONEY_BOTTLE, 4), name: "honey_bottle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 4)], result: (ITEM_HONEYCOMB_BLOCK, 1), name: "honeycomb_block", shaped: true, cells: &[(0, 0, ITEM_HONEYCOMB), (1, 0, ITEM_HONEYCOMB), (0, 1, ITEM_HONEYCOMB), (1, 1, ITEM_HONEYCOMB)] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_IRON_INGOT, 5)], result: (ITEM_HOPPER, 1), name: "hopper", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_CHEST), (2, 1, ITEM_IRON_INGOT), (1, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_HOPPER, 1), (ITEM_MINECART, 1)], result: (ITEM_HOPPER_MINECART, 1), name: "hopper_minecart", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_HOST_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_TERRACOTTA, 1)], result: (ITEM_HOST_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "host_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_HOST_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_TERRACOTTA), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 3)], result: (ITEM_IRON_AXE, 1), name: "iron_axe", shaped: true, cells: &[(0, 0, TAG_IRON_TOOL_MATERIALS), (1, 0, TAG_IRON_TOOL_MATERIALS), (0, 1, TAG_IRON_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6)], result: (ITEM_IRON_BARS, 16), name: "iron_bars", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_IRON_INGOT), (2, 1, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 9)], result: (ITEM_IRON_BLOCK, 1), name: "iron_block", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_IRON_INGOT), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (1, 2, ITEM_IRON_INGOT), (2, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 4)], result: (ITEM_IRON_BOOTS, 1), name: "iron_boots", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (2, 1, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (ITEM_IRON_NUGGET, 2)], result: (ITEM_IRON_CHAIN, 1), name: "iron_chain", shaped: true, cells: &[(0, 0, ITEM_IRON_NUGGET), (0, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_NUGGET)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 8)], result: (ITEM_IRON_CHESTPLATE, 1), name: "iron_chestplate", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_IRON_INGOT), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (1, 2, ITEM_IRON_INGOT), (2, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6)], result: (ITEM_IRON_DOOR, 3), name: "iron_door", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (1, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 5)], result: (ITEM_IRON_HELMET, 1), name: "iron_helmet", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (2, 1, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 2)], result: (ITEM_IRON_HOE, 1), name: "iron_hoe", shaped: true, cells: &[(0, 0, TAG_IRON_TOOL_MATERIALS), (1, 0, TAG_IRON_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_IRON_BLOCK, 1)], result: (ITEM_IRON_INGOT, 9), name: "iron_ingot_from_iron_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_IRON_NUGGET, 9)], result: (ITEM_IRON_INGOT, 1), name: "iron_ingot_from_nuggets", shaped: true, cells: &[(0, 0, ITEM_IRON_NUGGET), (1, 0, ITEM_IRON_NUGGET), (2, 0, ITEM_IRON_NUGGET), (0, 1, ITEM_IRON_NUGGET), (1, 1, ITEM_IRON_NUGGET), (2, 1, ITEM_IRON_NUGGET), (0, 2, ITEM_IRON_NUGGET), (1, 2, ITEM_IRON_NUGGET), (2, 2, ITEM_IRON_NUGGET)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 7)], result: (ITEM_IRON_LEGGINGS, 1), name: "iron_leggings", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (2, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1)], result: (ITEM_IRON_NUGGET, 9), name: "iron_nugget", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 3)], result: (ITEM_IRON_PICKAXE, 1), name: "iron_pickaxe", shaped: true, cells: &[(0, 0, TAG_IRON_TOOL_MATERIALS), (1, 0, TAG_IRON_TOOL_MATERIALS), (2, 0, TAG_IRON_TOOL_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 1)], result: (ITEM_IRON_SHOVEL, 1), name: "iron_shovel", shaped: true, cells: &[(0, 0, TAG_IRON_TOOL_MATERIALS), (0, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_IRON_TOOL_MATERIALS, 1)], result: (ITEM_IRON_SPEAR, 1), name: "iron_spear", shaped: true, cells: &[(2, 0, TAG_IRON_TOOL_MATERIALS), (1, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 1), (TAG_IRON_TOOL_MATERIALS, 2)], result: (ITEM_IRON_SWORD, 1), name: "iron_sword", shaped: true, cells: &[(0, 0, TAG_IRON_TOOL_MATERIALS), (0, 1, TAG_IRON_TOOL_MATERIALS), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 4)], result: (ITEM_IRON_TRAPDOOR, 1), name: "iron_trapdoor", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_LEATHER, 1), (STICK, 8)], result: (ITEM_ITEM_FRAME, 1), name: "item_frame", shaped: true, cells: &[(0, 0, STICK), (1, 0, STICK), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_LEATHER), (2, 1, STICK), (0, 2, STICK), (1, 2, STICK), (2, 2, STICK)] },
    Recipe { needs: &[(ITEM_CARVED_PUMPKIN, 1), (TORCH, 1)], result: (ITEM_JACK_O_LANTERN, 1), name: "jack_o_lantern", shaped: true, cells: &[(0, 0, ITEM_CARVED_PUMPKIN), (0, 1, TORCH)] },
    Recipe { needs: &[(ITEM_DIAMOND, 1), (TAG_PLANKS, 8)], result: (ITEM_JUKEBOX, 1), name: "jukebox", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (0, 1, TAG_PLANKS), (1, 1, ITEM_DIAMOND), (2, 1, TAG_PLANKS), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS), (2, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 5)], result: (ITEM_JUNGLE_BOAT, 1), name: "jungle_boat", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_PLANKS), (2, 0, ITEM_JUNGLE_PLANKS), (0, 1, ITEM_JUNGLE_PLANKS), (1, 1, ITEM_JUNGLE_PLANKS), (2, 1, ITEM_JUNGLE_PLANKS)] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 1)], result: (ITEM_JUNGLE_BUTTON, 1), name: "jungle_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_JUNGLE_BOAT, 1)], result: (ITEM_JUNGLE_CHEST_BOAT, 1), name: "jungle_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6)], result: (ITEM_JUNGLE_DOOR, 3), name: "jungle_door", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_PLANKS), (1, 0, ITEM_JUNGLE_PLANKS), (0, 1, ITEM_JUNGLE_PLANKS), (1, 1, ITEM_JUNGLE_PLANKS), (0, 2, ITEM_JUNGLE_PLANKS), (1, 2, ITEM_JUNGLE_PLANKS)] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 4), (STICK, 2)], result: (ITEM_JUNGLE_FENCE, 3), name: "jungle_fence", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_PLANKS), (1, 0, STICK), (2, 0, ITEM_JUNGLE_PLANKS), (0, 1, ITEM_JUNGLE_PLANKS), (1, 1, STICK), (2, 1, ITEM_JUNGLE_PLANKS)] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 2), (STICK, 4)], result: (ITEM_JUNGLE_FENCE_GATE, 1), name: "jungle_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_JUNGLE_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_JUNGLE_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_JUNGLE_LOG, 6)], result: (ITEM_JUNGLE_HANGING_SIGN, 6), name: "jungle_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_JUNGLE_LOG), (1, 1, ITEM_STRIPPED_JUNGLE_LOG), (2, 1, ITEM_STRIPPED_JUNGLE_LOG), (0, 2, ITEM_STRIPPED_JUNGLE_LOG), (1, 2, ITEM_STRIPPED_JUNGLE_LOG), (2, 2, ITEM_STRIPPED_JUNGLE_LOG)] },
    Recipe { needs: &[(TAG_JUNGLE_LOGS, 1)], result: (ITEM_JUNGLE_PLANKS, 4), name: "jungle_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 2)], result: (ITEM_JUNGLE_PRESSURE_PLATE, 1), name: "jungle_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_PLANKS), (1, 0, ITEM_JUNGLE_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_JUNGLE_LOG, 6)], result: (ITEM_JUNGLE_SHELF, 6), name: "jungle_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_JUNGLE_LOG), (1, 0, ITEM_STRIPPED_JUNGLE_LOG), (2, 0, ITEM_STRIPPED_JUNGLE_LOG), (0, 2, ITEM_STRIPPED_JUNGLE_LOG), (1, 2, ITEM_STRIPPED_JUNGLE_LOG), (2, 2, ITEM_STRIPPED_JUNGLE_LOG)] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6), (STICK, 1)], result: (ITEM_JUNGLE_SIGN, 3), name: "jungle_sign", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_PLANKS), (1, 0, ITEM_JUNGLE_PLANKS), (2, 0, ITEM_JUNGLE_PLANKS), (0, 1, ITEM_JUNGLE_PLANKS), (1, 1, ITEM_JUNGLE_PLANKS), (2, 1, ITEM_JUNGLE_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 3)], result: (ITEM_JUNGLE_SLAB, 6), name: "jungle_slab", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_PLANKS), (1, 0, ITEM_JUNGLE_PLANKS), (2, 0, ITEM_JUNGLE_PLANKS)] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6)], result: (ITEM_JUNGLE_STAIRS, 4), name: "jungle_stairs", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_PLANKS), (0, 1, ITEM_JUNGLE_PLANKS), (1, 1, ITEM_JUNGLE_PLANKS), (0, 2, ITEM_JUNGLE_PLANKS), (1, 2, ITEM_JUNGLE_PLANKS), (2, 2, ITEM_JUNGLE_PLANKS)] },
    Recipe { needs: &[(ITEM_JUNGLE_PLANKS, 6)], result: (ITEM_JUNGLE_TRAPDOOR, 2), name: "jungle_trapdoor", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_PLANKS), (1, 0, ITEM_JUNGLE_PLANKS), (2, 0, ITEM_JUNGLE_PLANKS), (0, 1, ITEM_JUNGLE_PLANKS), (1, 1, ITEM_JUNGLE_PLANKS), (2, 1, ITEM_JUNGLE_PLANKS)] },
    Recipe { needs: &[(ITEM_JUNGLE_LOG, 4)], result: (ITEM_JUNGLE_WOOD, 3), name: "jungle_wood", shaped: true, cells: &[(0, 0, ITEM_JUNGLE_LOG), (1, 0, ITEM_JUNGLE_LOG), (0, 1, ITEM_JUNGLE_LOG), (1, 1, ITEM_JUNGLE_LOG)] },
    Recipe { needs: &[(STICK, 7)], result: (ITEM_LADDER, 3), name: "ladder", shaped: true, cells: &[(0, 0, STICK), (2, 0, STICK), (0, 1, STICK), (1, 1, STICK), (2, 1, STICK), (0, 2, STICK), (2, 2, STICK)] },
    Recipe { needs: &[(ITEM_IRON_NUGGET, 8), (TORCH, 1)], result: (ITEM_LANTERN, 1), name: "lantern", shaped: true, cells: &[(0, 0, ITEM_IRON_NUGGET), (1, 0, ITEM_IRON_NUGGET), (2, 0, ITEM_IRON_NUGGET), (0, 1, ITEM_IRON_NUGGET), (1, 1, TORCH), (2, 1, ITEM_IRON_NUGGET), (0, 2, ITEM_IRON_NUGGET), (1, 2, ITEM_IRON_NUGGET), (2, 2, ITEM_IRON_NUGGET)] },
    Recipe { needs: &[(ITEM_LAPIS_LAZULI, 9)], result: (ITEM_LAPIS_BLOCK, 1), name: "lapis_block", shaped: true, cells: &[(0, 0, ITEM_LAPIS_LAZULI), (1, 0, ITEM_LAPIS_LAZULI), (2, 0, ITEM_LAPIS_LAZULI), (0, 1, ITEM_LAPIS_LAZULI), (1, 1, ITEM_LAPIS_LAZULI), (2, 1, ITEM_LAPIS_LAZULI), (0, 2, ITEM_LAPIS_LAZULI), (1, 2, ITEM_LAPIS_LAZULI), (2, 2, ITEM_LAPIS_LAZULI)] },
    Recipe { needs: &[(ITEM_LAPIS_BLOCK, 1)], result: (ITEM_LAPIS_LAZULI, 9), name: "lapis_lazuli", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_STRING, 5)], result: (ITEM_LEAD, 2), name: "lead", shaped: true, cells: &[(0, 0, ITEM_STRING), (1, 0, ITEM_STRING), (0, 1, ITEM_STRING), (1, 1, ITEM_STRING), (2, 2, ITEM_STRING)] },
    Recipe { needs: &[(ITEM_RABBIT_HIDE, 4)], result: (ITEM_LEATHER, 1), name: "leather", shaped: true, cells: &[(0, 0, ITEM_RABBIT_HIDE), (1, 0, ITEM_RABBIT_HIDE), (0, 1, ITEM_RABBIT_HIDE), (1, 1, ITEM_RABBIT_HIDE)] },
    Recipe { needs: &[(ITEM_LEATHER, 4)], result: (ITEM_LEATHER_BOOTS, 1), name: "leather_boots", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_LEATHER), (2, 1, ITEM_LEATHER)] },
    Recipe { needs: &[(ITEM_LEATHER, 8)], result: (ITEM_LEATHER_CHESTPLATE, 1), name: "leather_chestplate", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_LEATHER), (1, 1, ITEM_LEATHER), (2, 1, ITEM_LEATHER), (0, 2, ITEM_LEATHER), (1, 2, ITEM_LEATHER), (2, 2, ITEM_LEATHER)] },
    Recipe { needs: &[(ITEM_LEATHER, 5)], result: (ITEM_LEATHER_HELMET, 1), name: "leather_helmet", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_LEATHER), (2, 1, ITEM_LEATHER)] },
    Recipe { needs: &[(ITEM_LEATHER, 7)], result: (ITEM_LEATHER_HORSE_ARMOR, 1), name: "leather_horse_armor", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_LEATHER), (1, 1, ITEM_LEATHER), (2, 1, ITEM_LEATHER), (0, 2, ITEM_LEATHER), (2, 2, ITEM_LEATHER)] },
    Recipe { needs: &[(ITEM_LEATHER, 7)], result: (ITEM_LEATHER_LEGGINGS, 1), name: "leather_leggings", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_LEATHER), (2, 1, ITEM_LEATHER), (0, 2, ITEM_LEATHER), (2, 2, ITEM_LEATHER)] },
    Recipe { needs: &[(ITEM_BOOKSHELF, 1), (TAG_WOODEN_SLABS, 4)], result: (ITEM_LECTERN, 1), name: "lectern", shaped: true, cells: &[(0, 0, TAG_WOODEN_SLABS), (1, 0, TAG_WOODEN_SLABS), (2, 0, TAG_WOODEN_SLABS), (1, 1, ITEM_BOOKSHELF), (1, 2, TAG_WOODEN_SLABS)] },
    Recipe { needs: &[(COBBLESTONE, 1), (STICK, 1)], result: (ITEM_LEVER, 1), name: "lever", shaped: true, cells: &[(0, 0, STICK), (0, 1, COBBLESTONE)] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 6), (STICK, 1)], result: (ITEM_LIGHT_BLUE_BANNER, 1), name: "light_blue_banner", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_WOOL), (1, 0, ITEM_LIGHT_BLUE_WOOL), (2, 0, ITEM_LIGHT_BLUE_WOOL), (0, 1, ITEM_LIGHT_BLUE_WOOL), (1, 1, ITEM_LIGHT_BLUE_WOOL), (2, 1, ITEM_LIGHT_BLUE_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_LIGHT_BLUE_BED, 1), name: "light_blue_bed", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_WOOL), (1, 0, ITEM_LIGHT_BLUE_WOOL), (2, 0, ITEM_LIGHT_BLUE_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_LIGHT_BLUE_DYE, 1)], result: (ITEM_LIGHT_BLUE_CANDLE, 1), name: "light_blue_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 2)], result: (ITEM_LIGHT_BLUE_CARPET, 3), name: "light_blue_carpet", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_WOOL), (1, 0, ITEM_LIGHT_BLUE_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_LIGHT_BLUE_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_LIGHT_BLUE_CONCRETE_POWDER, 8), name: "light_blue_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_CONCRETE, 3)], result: (ITEM_LIGHT_BLUE_CONCRETE_SLAB, 6), name: "light_blue_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_CONCRETE), (1, 0, ITEM_LIGHT_BLUE_CONCRETE), (2, 0, ITEM_LIGHT_BLUE_CONCRETE)] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_CONCRETE, 6)], result: (ITEM_LIGHT_BLUE_CONCRETE_STAIRS, 4), name: "light_blue_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_CONCRETE), (0, 1, ITEM_LIGHT_BLUE_CONCRETE), (1, 1, ITEM_LIGHT_BLUE_CONCRETE), (0, 2, ITEM_LIGHT_BLUE_CONCRETE), (1, 2, ITEM_LIGHT_BLUE_CONCRETE), (2, 2, ITEM_LIGHT_BLUE_CONCRETE)] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL_SLAB, 3)], result: (ITEM_LIGHT_BLUE_CUSHION, 1), name: "light_blue_cushion", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_WOOL_SLAB), (1, 0, ITEM_LIGHT_BLUE_WOOL_SLAB), (2, 0, ITEM_LIGHT_BLUE_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_BLUE_ORCHID, 1)], result: (ITEM_LIGHT_BLUE_DYE, 1), name: "light_blue_dye_from_blue_orchid", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_LIGHT_BLUE_DYE, 2), name: "light_blue_dye_from_blue_white_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_LIGHT_BLUE_WOOL, 1)], result: (ITEM_LIGHT_BLUE_HARNESS, 1), name: "light_blue_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_LIGHT_BLUE_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_LIGHT_BLUE_DYE, 1)], result: (ITEM_LIGHT_BLUE_STAINED_GLASS, 8), name: "light_blue_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_LIGHT_BLUE_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_STAINED_GLASS, 6)], result: (ITEM_LIGHT_BLUE_STAINED_GLASS_PANE, 16), name: "light_blue_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_STAINED_GLASS), (1, 0, ITEM_LIGHT_BLUE_STAINED_GLASS), (2, 0, ITEM_LIGHT_BLUE_STAINED_GLASS), (0, 1, ITEM_LIGHT_BLUE_STAINED_GLASS), (1, 1, ITEM_LIGHT_BLUE_STAINED_GLASS), (2, 1, ITEM_LIGHT_BLUE_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_LIGHT_BLUE_DYE, 1)], result: (ITEM_LIGHT_BLUE_STAINED_GLASS_PANE, 8), name: "light_blue_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_LIGHT_BLUE_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_LIGHT_BLUE_TERRACOTTA, 8), name: "light_blue_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_LIGHT_BLUE_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 3)], result: (ITEM_LIGHT_BLUE_WOOL_SLAB, 6), name: "light_blue_wool_slab", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_WOOL), (1, 0, ITEM_LIGHT_BLUE_WOOL), (2, 0, ITEM_LIGHT_BLUE_WOOL)] },
    Recipe { needs: &[(ITEM_LIGHT_BLUE_WOOL, 6)], result: (ITEM_LIGHT_BLUE_WOOL_STAIRS, 4), name: "light_blue_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_LIGHT_BLUE_WOOL), (0, 1, ITEM_LIGHT_BLUE_WOOL), (1, 1, ITEM_LIGHT_BLUE_WOOL), (0, 2, ITEM_LIGHT_BLUE_WOOL), (1, 2, ITEM_LIGHT_BLUE_WOOL), (2, 2, ITEM_LIGHT_BLUE_WOOL)] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 6), (STICK, 1)], result: (ITEM_LIGHT_GRAY_BANNER, 1), name: "light_gray_banner", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_WOOL), (1, 0, ITEM_LIGHT_GRAY_WOOL), (2, 0, ITEM_LIGHT_GRAY_WOOL), (0, 1, ITEM_LIGHT_GRAY_WOOL), (1, 1, ITEM_LIGHT_GRAY_WOOL), (2, 1, ITEM_LIGHT_GRAY_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_LIGHT_GRAY_BED, 1), name: "light_gray_bed", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_WOOL), (1, 0, ITEM_LIGHT_GRAY_WOOL), (2, 0, ITEM_LIGHT_GRAY_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_LIGHT_GRAY_DYE, 1)], result: (ITEM_LIGHT_GRAY_CANDLE, 1), name: "light_gray_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 2)], result: (ITEM_LIGHT_GRAY_CARPET, 3), name: "light_gray_carpet", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_WOOL), (1, 0, ITEM_LIGHT_GRAY_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_LIGHT_GRAY_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_LIGHT_GRAY_CONCRETE_POWDER, 8), name: "light_gray_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_CONCRETE, 3)], result: (ITEM_LIGHT_GRAY_CONCRETE_SLAB, 6), name: "light_gray_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_CONCRETE), (1, 0, ITEM_LIGHT_GRAY_CONCRETE), (2, 0, ITEM_LIGHT_GRAY_CONCRETE)] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_CONCRETE, 6)], result: (ITEM_LIGHT_GRAY_CONCRETE_STAIRS, 4), name: "light_gray_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_CONCRETE), (0, 1, ITEM_LIGHT_GRAY_CONCRETE), (1, 1, ITEM_LIGHT_GRAY_CONCRETE), (0, 2, ITEM_LIGHT_GRAY_CONCRETE), (1, 2, ITEM_LIGHT_GRAY_CONCRETE), (2, 2, ITEM_LIGHT_GRAY_CONCRETE)] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL_SLAB, 3)], result: (ITEM_LIGHT_GRAY_CUSHION, 1), name: "light_gray_cushion", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_WOOL_SLAB), (1, 0, ITEM_LIGHT_GRAY_WOOL_SLAB), (2, 0, ITEM_LIGHT_GRAY_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_AZURE_BLUET, 1)], result: (ITEM_LIGHT_GRAY_DYE, 1), name: "light_gray_dye_from_azure_bluet", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLACK_DYE, 1), (ITEM_WHITE_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_LIGHT_GRAY_DYE, 3), name: "light_gray_dye_from_black_white_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GRAY_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_LIGHT_GRAY_DYE, 2), name: "light_gray_dye_from_gray_white_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_OXEYE_DAISY, 1)], result: (ITEM_LIGHT_GRAY_DYE, 1), name: "light_gray_dye_from_oxeye_daisy", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_TULIP, 1)], result: (ITEM_LIGHT_GRAY_DYE, 1), name: "light_gray_dye_from_white_tulip", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_LIGHT_GRAY_WOOL, 1)], result: (ITEM_LIGHT_GRAY_HARNESS, 1), name: "light_gray_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_LIGHT_GRAY_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_LIGHT_GRAY_DYE, 1)], result: (ITEM_LIGHT_GRAY_STAINED_GLASS, 8), name: "light_gray_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_LIGHT_GRAY_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_STAINED_GLASS, 6)], result: (ITEM_LIGHT_GRAY_STAINED_GLASS_PANE, 16), name: "light_gray_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_STAINED_GLASS), (1, 0, ITEM_LIGHT_GRAY_STAINED_GLASS), (2, 0, ITEM_LIGHT_GRAY_STAINED_GLASS), (0, 1, ITEM_LIGHT_GRAY_STAINED_GLASS), (1, 1, ITEM_LIGHT_GRAY_STAINED_GLASS), (2, 1, ITEM_LIGHT_GRAY_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_LIGHT_GRAY_DYE, 1)], result: (ITEM_LIGHT_GRAY_STAINED_GLASS_PANE, 8), name: "light_gray_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_LIGHT_GRAY_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_LIGHT_GRAY_TERRACOTTA, 8), name: "light_gray_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_LIGHT_GRAY_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 3)], result: (ITEM_LIGHT_GRAY_WOOL_SLAB, 6), name: "light_gray_wool_slab", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_WOOL), (1, 0, ITEM_LIGHT_GRAY_WOOL), (2, 0, ITEM_LIGHT_GRAY_WOOL)] },
    Recipe { needs: &[(ITEM_LIGHT_GRAY_WOOL, 6)], result: (ITEM_LIGHT_GRAY_WOOL_STAIRS, 4), name: "light_gray_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_LIGHT_GRAY_WOOL), (0, 1, ITEM_LIGHT_GRAY_WOOL), (1, 1, ITEM_LIGHT_GRAY_WOOL), (0, 2, ITEM_LIGHT_GRAY_WOOL), (1, 2, ITEM_LIGHT_GRAY_WOOL), (2, 2, ITEM_LIGHT_GRAY_WOOL)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 2)], result: (ITEM_LIGHT_WEIGHTED_PRESSURE_PLATE, 1), name: "light_weighted_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_GOLD_INGOT), (1, 0, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(ITEM_COPPER_INGOT, 3)], result: (ITEM_LIGHTNING_ROD, 1), name: "lightning_rod", shaped: true, cells: &[(0, 0, ITEM_COPPER_INGOT), (0, 1, ITEM_COPPER_INGOT), (0, 2, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(ITEM_LIME_WOOL, 6), (STICK, 1)], result: (ITEM_LIME_BANNER, 1), name: "lime_banner", shaped: true, cells: &[(0, 0, ITEM_LIME_WOOL), (1, 0, ITEM_LIME_WOOL), (2, 0, ITEM_LIME_WOOL), (0, 1, ITEM_LIME_WOOL), (1, 1, ITEM_LIME_WOOL), (2, 1, ITEM_LIME_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_LIME_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_LIME_BED, 1), name: "lime_bed", shaped: true, cells: &[(0, 0, ITEM_LIME_WOOL), (1, 0, ITEM_LIME_WOOL), (2, 0, ITEM_LIME_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_LIME_DYE, 1)], result: (ITEM_LIME_CANDLE, 1), name: "lime_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_WOOL, 2)], result: (ITEM_LIME_CARPET, 3), name: "lime_carpet", shaped: true, cells: &[(0, 0, ITEM_LIME_WOOL), (1, 0, ITEM_LIME_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_LIME_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_LIME_CONCRETE_POWDER, 8), name: "lime_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LIME_CONCRETE, 3)], result: (ITEM_LIME_CONCRETE_SLAB, 6), name: "lime_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_LIME_CONCRETE), (1, 0, ITEM_LIME_CONCRETE), (2, 0, ITEM_LIME_CONCRETE)] },
    Recipe { needs: &[(ITEM_LIME_CONCRETE, 6)], result: (ITEM_LIME_CONCRETE_STAIRS, 4), name: "lime_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_LIME_CONCRETE), (0, 1, ITEM_LIME_CONCRETE), (1, 1, ITEM_LIME_CONCRETE), (0, 2, ITEM_LIME_CONCRETE), (1, 2, ITEM_LIME_CONCRETE), (2, 2, ITEM_LIME_CONCRETE)] },
    Recipe { needs: &[(ITEM_LIME_WOOL_SLAB, 3)], result: (ITEM_LIME_CUSHION, 1), name: "lime_cushion", shaped: true, cells: &[(0, 0, ITEM_LIME_WOOL_SLAB), (1, 0, ITEM_LIME_WOOL_SLAB), (2, 0, ITEM_LIME_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_GREEN_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_LIME_DYE, 2), name: "lime_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_LIME_WOOL, 1)], result: (ITEM_LIME_HARNESS, 1), name: "lime_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_LIME_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_LIME_DYE, 1)], result: (ITEM_LIME_STAINED_GLASS, 8), name: "lime_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_LIME_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_LIME_STAINED_GLASS, 6)], result: (ITEM_LIME_STAINED_GLASS_PANE, 16), name: "lime_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_LIME_STAINED_GLASS), (1, 0, ITEM_LIME_STAINED_GLASS), (2, 0, ITEM_LIME_STAINED_GLASS), (0, 1, ITEM_LIME_STAINED_GLASS), (1, 1, ITEM_LIME_STAINED_GLASS), (2, 1, ITEM_LIME_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_LIME_DYE, 1)], result: (ITEM_LIME_STAINED_GLASS_PANE, 8), name: "lime_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_LIME_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_LIME_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_LIME_TERRACOTTA, 8), name: "lime_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_LIME_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_LIME_WOOL, 3)], result: (ITEM_LIME_WOOL_SLAB, 6), name: "lime_wool_slab", shaped: true, cells: &[(0, 0, ITEM_LIME_WOOL), (1, 0, ITEM_LIME_WOOL), (2, 0, ITEM_LIME_WOOL)] },
    Recipe { needs: &[(ITEM_LIME_WOOL, 6)], result: (ITEM_LIME_WOOL_STAIRS, 4), name: "lime_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_LIME_WOOL), (0, 1, ITEM_LIME_WOOL), (1, 1, ITEM_LIME_WOOL), (0, 2, ITEM_LIME_WOOL), (1, 2, ITEM_LIME_WOOL), (2, 2, ITEM_LIME_WOOL)] },
    Recipe { needs: &[(ITEM_CHISELED_STONE_BRICKS, 8), (ITEM_IRON_INGOT, 1)], result: (ITEM_LODESTONE, 1), name: "lodestone", shaped: true, cells: &[(0, 0, ITEM_CHISELED_STONE_BRICKS), (1, 0, ITEM_CHISELED_STONE_BRICKS), (2, 0, ITEM_CHISELED_STONE_BRICKS), (0, 1, ITEM_CHISELED_STONE_BRICKS), (1, 1, ITEM_IRON_INGOT), (2, 1, ITEM_CHISELED_STONE_BRICKS), (0, 2, ITEM_CHISELED_STONE_BRICKS), (1, 2, ITEM_CHISELED_STONE_BRICKS), (2, 2, ITEM_CHISELED_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_STRING, 2), (TAG_PLANKS, 2)], result: (ITEM_LOOM, 1), name: "loom", shaped: true, cells: &[(0, 0, ITEM_STRING), (1, 0, ITEM_STRING), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_BREEZE_ROD, 1), (ITEM_HEAVY_CORE, 1)], result: (ITEM_MACE, 1), name: "mace", shaped: true, cells: &[(0, 0, ITEM_HEAVY_CORE), (0, 1, ITEM_BREEZE_ROD)] },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 6), (STICK, 1)], result: (ITEM_MAGENTA_BANNER, 1), name: "magenta_banner", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_WOOL), (1, 0, ITEM_MAGENTA_WOOL), (2, 0, ITEM_MAGENTA_WOOL), (0, 1, ITEM_MAGENTA_WOOL), (1, 1, ITEM_MAGENTA_WOOL), (2, 1, ITEM_MAGENTA_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_MAGENTA_BED, 1), name: "magenta_bed", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_WOOL), (1, 0, ITEM_MAGENTA_WOOL), (2, 0, ITEM_MAGENTA_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_MAGENTA_DYE, 1)], result: (ITEM_MAGENTA_CANDLE, 1), name: "magenta_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 2)], result: (ITEM_MAGENTA_CARPET, 3), name: "magenta_carpet", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_WOOL), (1, 0, ITEM_MAGENTA_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_MAGENTA_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_MAGENTA_CONCRETE_POWDER, 8), name: "magenta_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MAGENTA_CONCRETE, 3)], result: (ITEM_MAGENTA_CONCRETE_SLAB, 6), name: "magenta_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_CONCRETE), (1, 0, ITEM_MAGENTA_CONCRETE), (2, 0, ITEM_MAGENTA_CONCRETE)] },
    Recipe { needs: &[(ITEM_MAGENTA_CONCRETE, 6)], result: (ITEM_MAGENTA_CONCRETE_STAIRS, 4), name: "magenta_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_CONCRETE), (0, 1, ITEM_MAGENTA_CONCRETE), (1, 1, ITEM_MAGENTA_CONCRETE), (0, 2, ITEM_MAGENTA_CONCRETE), (1, 2, ITEM_MAGENTA_CONCRETE), (2, 2, ITEM_MAGENTA_CONCRETE)] },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL_SLAB, 3)], result: (ITEM_MAGENTA_CUSHION, 1), name: "magenta_cushion", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_WOOL_SLAB), (1, 0, ITEM_MAGENTA_WOOL_SLAB), (2, 0, ITEM_MAGENTA_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_ALLIUM, 1)], result: (ITEM_MAGENTA_DYE, 1), name: "magenta_dye_from_allium", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_PINK_DYE, 1), (ITEM_RED_DYE, 1)], result: (ITEM_MAGENTA_DYE, 3), name: "magenta_dye_from_blue_red_pink", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_RED_DYE, 1), (ITEM_RED_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_MAGENTA_DYE, 4), name: "magenta_dye_from_blue_red_white_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LILAC, 1)], result: (ITEM_MAGENTA_DYE, 2), name: "magenta_dye_from_lilac", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (ITEM_PURPLE_DYE, 1)], result: (ITEM_MAGENTA_DYE, 2), name: "magenta_dye_from_purple_and_pink", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_MAGENTA_WOOL, 1)], result: (ITEM_MAGENTA_HARNESS, 1), name: "magenta_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_MAGENTA_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_MAGENTA_DYE, 1)], result: (ITEM_MAGENTA_STAINED_GLASS, 8), name: "magenta_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_MAGENTA_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_MAGENTA_STAINED_GLASS, 6)], result: (ITEM_MAGENTA_STAINED_GLASS_PANE, 16), name: "magenta_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_STAINED_GLASS), (1, 0, ITEM_MAGENTA_STAINED_GLASS), (2, 0, ITEM_MAGENTA_STAINED_GLASS), (0, 1, ITEM_MAGENTA_STAINED_GLASS), (1, 1, ITEM_MAGENTA_STAINED_GLASS), (2, 1, ITEM_MAGENTA_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_MAGENTA_DYE, 1)], result: (ITEM_MAGENTA_STAINED_GLASS_PANE, 8), name: "magenta_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_MAGENTA_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_MAGENTA_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_MAGENTA_TERRACOTTA, 8), name: "magenta_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_MAGENTA_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 3)], result: (ITEM_MAGENTA_WOOL_SLAB, 6), name: "magenta_wool_slab", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_WOOL), (1, 0, ITEM_MAGENTA_WOOL), (2, 0, ITEM_MAGENTA_WOOL)] },
    Recipe { needs: &[(ITEM_MAGENTA_WOOL, 6)], result: (ITEM_MAGENTA_WOOL_STAIRS, 4), name: "magenta_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_MAGENTA_WOOL), (0, 1, ITEM_MAGENTA_WOOL), (1, 1, ITEM_MAGENTA_WOOL), (0, 2, ITEM_MAGENTA_WOOL), (1, 2, ITEM_MAGENTA_WOOL), (2, 2, ITEM_MAGENTA_WOOL)] },
    Recipe { needs: &[(ITEM_MAGMA_CREAM, 4)], result: (ITEM_MAGMA_BLOCK, 1), name: "magma_block", shaped: true, cells: &[(0, 0, ITEM_MAGMA_CREAM), (1, 0, ITEM_MAGMA_CREAM), (0, 1, ITEM_MAGMA_CREAM), (1, 1, ITEM_MAGMA_CREAM)] },
    Recipe { needs: &[(ITEM_BLAZE_POWDER, 1), (ITEM_SLIME_BALL, 1)], result: (ITEM_MAGMA_CREAM, 1), name: "magma_cream", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 5)], result: (ITEM_MANGROVE_BOAT, 1), name: "mangrove_boat", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_PLANKS), (2, 0, ITEM_MANGROVE_PLANKS), (0, 1, ITEM_MANGROVE_PLANKS), (1, 1, ITEM_MANGROVE_PLANKS), (2, 1, ITEM_MANGROVE_PLANKS)] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 1)], result: (ITEM_MANGROVE_BUTTON, 1), name: "mangrove_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_MANGROVE_BOAT, 1)], result: (ITEM_MANGROVE_CHEST_BOAT, 1), name: "mangrove_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6)], result: (ITEM_MANGROVE_DOOR, 3), name: "mangrove_door", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_PLANKS), (1, 0, ITEM_MANGROVE_PLANKS), (0, 1, ITEM_MANGROVE_PLANKS), (1, 1, ITEM_MANGROVE_PLANKS), (0, 2, ITEM_MANGROVE_PLANKS), (1, 2, ITEM_MANGROVE_PLANKS)] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 4), (STICK, 2)], result: (ITEM_MANGROVE_FENCE, 3), name: "mangrove_fence", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_PLANKS), (1, 0, STICK), (2, 0, ITEM_MANGROVE_PLANKS), (0, 1, ITEM_MANGROVE_PLANKS), (1, 1, STICK), (2, 1, ITEM_MANGROVE_PLANKS)] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 2), (STICK, 4)], result: (ITEM_MANGROVE_FENCE_GATE, 1), name: "mangrove_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_MANGROVE_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_MANGROVE_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_MANGROVE_LOG, 6)], result: (ITEM_MANGROVE_HANGING_SIGN, 6), name: "mangrove_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_MANGROVE_LOG), (1, 1, ITEM_STRIPPED_MANGROVE_LOG), (2, 1, ITEM_STRIPPED_MANGROVE_LOG), (0, 2, ITEM_STRIPPED_MANGROVE_LOG), (1, 2, ITEM_STRIPPED_MANGROVE_LOG), (2, 2, ITEM_STRIPPED_MANGROVE_LOG)] },
    Recipe { needs: &[(TAG_MANGROVE_LOGS, 1)], result: (ITEM_MANGROVE_PLANKS, 4), name: "mangrove_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 2)], result: (ITEM_MANGROVE_PRESSURE_PLATE, 1), name: "mangrove_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_PLANKS), (1, 0, ITEM_MANGROVE_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_MANGROVE_LOG, 6)], result: (ITEM_MANGROVE_SHELF, 6), name: "mangrove_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_MANGROVE_LOG), (1, 0, ITEM_STRIPPED_MANGROVE_LOG), (2, 0, ITEM_STRIPPED_MANGROVE_LOG), (0, 2, ITEM_STRIPPED_MANGROVE_LOG), (1, 2, ITEM_STRIPPED_MANGROVE_LOG), (2, 2, ITEM_STRIPPED_MANGROVE_LOG)] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6), (STICK, 1)], result: (ITEM_MANGROVE_SIGN, 3), name: "mangrove_sign", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_PLANKS), (1, 0, ITEM_MANGROVE_PLANKS), (2, 0, ITEM_MANGROVE_PLANKS), (0, 1, ITEM_MANGROVE_PLANKS), (1, 1, ITEM_MANGROVE_PLANKS), (2, 1, ITEM_MANGROVE_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 3)], result: (ITEM_MANGROVE_SLAB, 6), name: "mangrove_slab", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_PLANKS), (1, 0, ITEM_MANGROVE_PLANKS), (2, 0, ITEM_MANGROVE_PLANKS)] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6)], result: (ITEM_MANGROVE_STAIRS, 4), name: "mangrove_stairs", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_PLANKS), (0, 1, ITEM_MANGROVE_PLANKS), (1, 1, ITEM_MANGROVE_PLANKS), (0, 2, ITEM_MANGROVE_PLANKS), (1, 2, ITEM_MANGROVE_PLANKS), (2, 2, ITEM_MANGROVE_PLANKS)] },
    Recipe { needs: &[(ITEM_MANGROVE_PLANKS, 6)], result: (ITEM_MANGROVE_TRAPDOOR, 2), name: "mangrove_trapdoor", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_PLANKS), (1, 0, ITEM_MANGROVE_PLANKS), (2, 0, ITEM_MANGROVE_PLANKS), (0, 1, ITEM_MANGROVE_PLANKS), (1, 1, ITEM_MANGROVE_PLANKS), (2, 1, ITEM_MANGROVE_PLANKS)] },
    Recipe { needs: &[(ITEM_MANGROVE_LOG, 4)], result: (ITEM_MANGROVE_WOOD, 3), name: "mangrove_wood", shaped: true, cells: &[(0, 0, ITEM_MANGROVE_LOG), (1, 0, ITEM_MANGROVE_LOG), (0, 1, ITEM_MANGROVE_LOG), (1, 1, ITEM_MANGROVE_LOG)] },
    Recipe { needs: &[(ITEM_COMPASS, 1), (ITEM_PAPER, 8)], result: (ITEM_MAP, 1), name: "map", shaped: true, cells: &[(0, 0, ITEM_PAPER), (1, 0, ITEM_PAPER), (2, 0, ITEM_PAPER), (0, 1, ITEM_PAPER), (1, 1, ITEM_COMPASS), (2, 1, ITEM_PAPER), (0, 2, ITEM_PAPER), (1, 2, ITEM_PAPER), (2, 2, ITEM_PAPER)] },
    Recipe { needs: &[(ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1), (ITEM_MELON_SLICE, 1)], result: (ITEM_MELON, 1), name: "melon", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MELON_SLICE, 1)], result: (ITEM_MELON_SEEDS, 1), name: "melon_seeds", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 5)], result: (ITEM_MINECART, 1), name: "minecart", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, ITEM_IRON_INGOT), (2, 1, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_ENCHANTED_GOLDEN_APPLE, 1), (ITEM_PAPER, 1)], result: (ITEM_MOJANG_BANNER_PATTERN, 1), name: "mojang_banner_pattern", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MOSS_BLOCK, 2)], result: (ITEM_MOSS_CARPET, 3), name: "moss_carpet", shaped: true, cells: &[(0, 0, ITEM_MOSS_BLOCK), (1, 0, ITEM_MOSS_BLOCK)] },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_MOSS_BLOCK, 1)], result: (ITEM_MOSSY_COBBLESTONE, 1), name: "mossy_cobblestone_from_moss_block", shaped: false, cells: &[] },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_VINE, 1)], result: (ITEM_MOSSY_COBBLESTONE, 1), name: "mossy_cobblestone_from_vine", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MOSSY_COBBLESTONE, 3)], result: (ITEM_MOSSY_COBBLESTONE_SLAB, 6), name: "mossy_cobblestone_slab", shaped: true, cells: &[(0, 0, ITEM_MOSSY_COBBLESTONE), (1, 0, ITEM_MOSSY_COBBLESTONE), (2, 0, ITEM_MOSSY_COBBLESTONE)] },
    Recipe { needs: &[(ITEM_MOSSY_COBBLESTONE, 6)], result: (ITEM_MOSSY_COBBLESTONE_STAIRS, 4), name: "mossy_cobblestone_stairs", shaped: true, cells: &[(0, 0, ITEM_MOSSY_COBBLESTONE), (0, 1, ITEM_MOSSY_COBBLESTONE), (1, 1, ITEM_MOSSY_COBBLESTONE), (0, 2, ITEM_MOSSY_COBBLESTONE), (1, 2, ITEM_MOSSY_COBBLESTONE), (2, 2, ITEM_MOSSY_COBBLESTONE)] },
    Recipe { needs: &[(ITEM_MOSSY_COBBLESTONE, 6)], result: (ITEM_MOSSY_COBBLESTONE_WALL, 6), name: "mossy_cobblestone_wall", shaped: true, cells: &[(0, 0, ITEM_MOSSY_COBBLESTONE), (1, 0, ITEM_MOSSY_COBBLESTONE), (2, 0, ITEM_MOSSY_COBBLESTONE), (0, 1, ITEM_MOSSY_COBBLESTONE), (1, 1, ITEM_MOSSY_COBBLESTONE), (2, 1, ITEM_MOSSY_COBBLESTONE)] },
    Recipe { needs: &[(ITEM_MOSSY_STONE_BRICKS, 3)], result: (ITEM_MOSSY_STONE_BRICK_SLAB, 6), name: "mossy_stone_brick_slab", shaped: true, cells: &[(0, 0, ITEM_MOSSY_STONE_BRICKS), (1, 0, ITEM_MOSSY_STONE_BRICKS), (2, 0, ITEM_MOSSY_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_MOSSY_STONE_BRICKS, 6)], result: (ITEM_MOSSY_STONE_BRICK_STAIRS, 4), name: "mossy_stone_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_MOSSY_STONE_BRICKS), (0, 1, ITEM_MOSSY_STONE_BRICKS), (1, 1, ITEM_MOSSY_STONE_BRICKS), (0, 2, ITEM_MOSSY_STONE_BRICKS), (1, 2, ITEM_MOSSY_STONE_BRICKS), (2, 2, ITEM_MOSSY_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_MOSSY_STONE_BRICKS, 6)], result: (ITEM_MOSSY_STONE_BRICK_WALL, 6), name: "mossy_stone_brick_wall", shaped: true, cells: &[(0, 0, ITEM_MOSSY_STONE_BRICKS), (1, 0, ITEM_MOSSY_STONE_BRICKS), (2, 0, ITEM_MOSSY_STONE_BRICKS), (0, 1, ITEM_MOSSY_STONE_BRICKS), (1, 1, ITEM_MOSSY_STONE_BRICKS), (2, 1, ITEM_MOSSY_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_MOSS_BLOCK, 1), (ITEM_STONE_BRICKS, 1)], result: (ITEM_MOSSY_STONE_BRICKS, 1), name: "mossy_stone_bricks_from_moss_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_STONE_BRICKS, 1), (ITEM_VINE, 1)], result: (ITEM_MOSSY_STONE_BRICKS, 1), name: "mossy_stone_bricks_from_vine", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MUD_BRICKS, 3)], result: (ITEM_MUD_BRICK_SLAB, 6), name: "mud_brick_slab", shaped: true, cells: &[(0, 0, ITEM_MUD_BRICKS), (1, 0, ITEM_MUD_BRICKS), (2, 0, ITEM_MUD_BRICKS)] },
    Recipe { needs: &[(ITEM_MUD_BRICKS, 6)], result: (ITEM_MUD_BRICK_STAIRS, 4), name: "mud_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_MUD_BRICKS), (0, 1, ITEM_MUD_BRICKS), (1, 1, ITEM_MUD_BRICKS), (0, 2, ITEM_MUD_BRICKS), (1, 2, ITEM_MUD_BRICKS), (2, 2, ITEM_MUD_BRICKS)] },
    Recipe { needs: &[(ITEM_MUD_BRICKS, 6)], result: (ITEM_MUD_BRICK_WALL, 6), name: "mud_brick_wall", shaped: true, cells: &[(0, 0, ITEM_MUD_BRICKS), (1, 0, ITEM_MUD_BRICKS), (2, 0, ITEM_MUD_BRICKS), (0, 1, ITEM_MUD_BRICKS), (1, 1, ITEM_MUD_BRICKS), (2, 1, ITEM_MUD_BRICKS)] },
    Recipe { needs: &[(ITEM_PACKED_MUD, 4)], result: (ITEM_MUD_BRICKS, 4), name: "mud_bricks", shaped: true, cells: &[(0, 0, ITEM_PACKED_MUD), (1, 0, ITEM_PACKED_MUD), (0, 1, ITEM_PACKED_MUD), (1, 1, ITEM_PACKED_MUD)] },
    Recipe { needs: &[(ITEM_MANGROVE_ROOTS, 1), (ITEM_MUD, 1)], result: (ITEM_MUDDY_MANGROVE_ROOTS, 1), name: "muddy_mangrove_roots", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BOWL, 1), (TAG_MUSHROOMS, 1), (TAG_MUSHROOMS, 1)], result: (ITEM_MUSHROOM_STEW, 1), name: "mushroom_stew", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1), (ITEM_DISC_FRAGMENT_5, 1)], result: (ITEM_MUSIC_DISC_5, 1), name: "music_disc_5", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PAPER, 1), (TAG_METAL_NUGGETS, 1)], result: (ITEM_NAME_TAG, 1), name: "name_tag", shaped: true, cells: &[(1, 0, TAG_METAL_NUGGETS), (0, 1, ITEM_PAPER)] },
    Recipe { needs: &[(ITEM_NETHER_BRICK, 2), (ITEM_NETHER_BRICKS, 4)], result: (ITEM_NETHER_BRICK_FENCE, 6), name: "nether_brick_fence", shaped: true, cells: &[(0, 0, ITEM_NETHER_BRICKS), (1, 0, ITEM_NETHER_BRICK), (2, 0, ITEM_NETHER_BRICKS), (0, 1, ITEM_NETHER_BRICKS), (1, 1, ITEM_NETHER_BRICK), (2, 1, ITEM_NETHER_BRICKS)] },
    Recipe { needs: &[(ITEM_NETHER_BRICKS, 3)], result: (ITEM_NETHER_BRICK_SLAB, 6), name: "nether_brick_slab", shaped: true, cells: &[(0, 0, ITEM_NETHER_BRICKS), (1, 0, ITEM_NETHER_BRICKS), (2, 0, ITEM_NETHER_BRICKS)] },
    Recipe { needs: &[(ITEM_NETHER_BRICKS, 6)], result: (ITEM_NETHER_BRICK_STAIRS, 4), name: "nether_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_NETHER_BRICKS), (0, 1, ITEM_NETHER_BRICKS), (1, 1, ITEM_NETHER_BRICKS), (0, 2, ITEM_NETHER_BRICKS), (1, 2, ITEM_NETHER_BRICKS), (2, 2, ITEM_NETHER_BRICKS)] },
    Recipe { needs: &[(ITEM_NETHER_BRICKS, 6)], result: (ITEM_NETHER_BRICK_WALL, 6), name: "nether_brick_wall", shaped: true, cells: &[(0, 0, ITEM_NETHER_BRICKS), (1, 0, ITEM_NETHER_BRICKS), (2, 0, ITEM_NETHER_BRICKS), (0, 1, ITEM_NETHER_BRICKS), (1, 1, ITEM_NETHER_BRICKS), (2, 1, ITEM_NETHER_BRICKS)] },
    Recipe { needs: &[(ITEM_NETHER_BRICK, 4)], result: (ITEM_NETHER_BRICKS, 1), name: "nether_bricks", shaped: true, cells: &[(0, 0, ITEM_NETHER_BRICK), (1, 0, ITEM_NETHER_BRICK), (0, 1, ITEM_NETHER_BRICK), (1, 1, ITEM_NETHER_BRICK)] },
    Recipe { needs: &[(ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1), (ITEM_NETHER_WART, 1)], result: (ITEM_NETHER_WART_BLOCK, 1), name: "nether_wart_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_NETHERITE_INGOT, 9)], result: (ITEM_NETHERITE_BLOCK, 1), name: "netherite_block", shaped: true, cells: &[(0, 0, ITEM_NETHERITE_INGOT), (1, 0, ITEM_NETHERITE_INGOT), (2, 0, ITEM_NETHERITE_INGOT), (0, 1, ITEM_NETHERITE_INGOT), (1, 1, ITEM_NETHERITE_INGOT), (2, 1, ITEM_NETHERITE_INGOT), (0, 2, ITEM_NETHERITE_INGOT), (1, 2, ITEM_NETHERITE_INGOT), (2, 2, ITEM_NETHERITE_INGOT)] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 1), (ITEM_GOLD_INGOT, 1), (ITEM_GOLD_INGOT, 1), (ITEM_GOLD_INGOT, 1), (ITEM_NETHERITE_SCRAP, 1), (ITEM_NETHERITE_SCRAP, 1), (ITEM_NETHERITE_SCRAP, 1), (ITEM_NETHERITE_SCRAP, 1)], result: (ITEM_NETHERITE_INGOT, 1), name: "netherite_ingot", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_NETHERITE_BLOCK, 1)], result: (ITEM_NETHERITE_INGOT, 9), name: "netherite_ingot_from_netherite_block", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_NETHERITE_UPGRADE_SMITHING_TEMPLATE, 1), (ITEM_NETHERRACK, 1)], result: (ITEM_NETHERITE_UPGRADE_SMITHING_TEMPLATE, 2), name: "netherite_upgrade_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_NETHERITE_UPGRADE_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_NETHERRACK), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_REDSTONE, 1), (TAG_PLANKS, 8)], result: (ITEM_NOTE_BLOCK, 1), name: "note_block", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (0, 1, TAG_PLANKS), (1, 1, ITEM_REDSTONE), (2, 1, TAG_PLANKS), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS), (2, 2, TAG_PLANKS)] },
    Recipe { needs: &[(OAK_PLANKS, 5)], result: (ITEM_OAK_BOAT, 1), name: "oak_boat", shaped: true, cells: &[(0, 0, OAK_PLANKS), (2, 0, OAK_PLANKS), (0, 1, OAK_PLANKS), (1, 1, OAK_PLANKS), (2, 1, OAK_PLANKS)] },
    Recipe { needs: &[(OAK_PLANKS, 1)], result: (ITEM_OAK_BUTTON, 1), name: "oak_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_OAK_BOAT, 1)], result: (ITEM_OAK_CHEST_BOAT, 1), name: "oak_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(OAK_PLANKS, 6)], result: (ITEM_OAK_DOOR, 3), name: "oak_door", shaped: true, cells: &[(0, 0, OAK_PLANKS), (1, 0, OAK_PLANKS), (0, 1, OAK_PLANKS), (1, 1, OAK_PLANKS), (0, 2, OAK_PLANKS), (1, 2, OAK_PLANKS)] },
    Recipe { needs: &[(OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_OAK_FENCE, 3), name: "oak_fence", shaped: true, cells: &[(0, 0, OAK_PLANKS), (1, 0, STICK), (2, 0, OAK_PLANKS), (0, 1, OAK_PLANKS), (1, 1, STICK), (2, 1, OAK_PLANKS)] },
    Recipe { needs: &[(OAK_PLANKS, 2), (STICK, 4)], result: (ITEM_OAK_FENCE_GATE, 1), name: "oak_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, OAK_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, OAK_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_OAK_LOG, 6)], result: (ITEM_OAK_HANGING_SIGN, 6), name: "oak_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_OAK_LOG), (1, 1, ITEM_STRIPPED_OAK_LOG), (2, 1, ITEM_STRIPPED_OAK_LOG), (0, 2, ITEM_STRIPPED_OAK_LOG), (1, 2, ITEM_STRIPPED_OAK_LOG), (2, 2, ITEM_STRIPPED_OAK_LOG)] },
    Recipe { needs: &[(TAG_OAK_LOGS, 1)], result: (OAK_PLANKS, 4), name: "oak_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(OAK_PLANKS, 2)], result: (ITEM_OAK_PRESSURE_PLATE, 1), name: "oak_pressure_plate", shaped: true, cells: &[(0, 0, OAK_PLANKS), (1, 0, OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_OAK_LOG, 6)], result: (ITEM_OAK_SHELF, 6), name: "oak_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_OAK_LOG), (1, 0, ITEM_STRIPPED_OAK_LOG), (2, 0, ITEM_STRIPPED_OAK_LOG), (0, 2, ITEM_STRIPPED_OAK_LOG), (1, 2, ITEM_STRIPPED_OAK_LOG), (2, 2, ITEM_STRIPPED_OAK_LOG)] },
    Recipe { needs: &[(OAK_PLANKS, 6), (STICK, 1)], result: (ITEM_OAK_SIGN, 3), name: "oak_sign", shaped: true, cells: &[(0, 0, OAK_PLANKS), (1, 0, OAK_PLANKS), (2, 0, OAK_PLANKS), (0, 1, OAK_PLANKS), (1, 1, OAK_PLANKS), (2, 1, OAK_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(OAK_PLANKS, 3)], result: (ITEM_OAK_SLAB, 6), name: "oak_slab", shaped: true, cells: &[(0, 0, OAK_PLANKS), (1, 0, OAK_PLANKS), (2, 0, OAK_PLANKS)] },
    Recipe { needs: &[(OAK_PLANKS, 6)], result: (ITEM_OAK_STAIRS, 4), name: "oak_stairs", shaped: true, cells: &[(0, 0, OAK_PLANKS), (0, 1, OAK_PLANKS), (1, 1, OAK_PLANKS), (0, 2, OAK_PLANKS), (1, 2, OAK_PLANKS), (2, 2, OAK_PLANKS)] },
    Recipe { needs: &[(OAK_PLANKS, 6)], result: (ITEM_OAK_TRAPDOOR, 2), name: "oak_trapdoor", shaped: true, cells: &[(0, 0, OAK_PLANKS), (1, 0, OAK_PLANKS), (2, 0, OAK_PLANKS), (0, 1, OAK_PLANKS), (1, 1, OAK_PLANKS), (2, 1, OAK_PLANKS)] },
    Recipe { needs: &[(OAK_LOG, 4)], result: (ITEM_OAK_WOOD, 3), name: "oak_wood", shaped: true, cells: &[(0, 0, OAK_LOG), (1, 0, OAK_LOG), (0, 1, OAK_LOG), (1, 1, OAK_LOG)] },
    Recipe { needs: &[(COBBLESTONE, 6), (ITEM_QUARTZ, 1), (ITEM_REDSTONE, 2)], result: (ITEM_OBSERVER, 1), name: "observer", shaped: true, cells: &[(0, 0, COBBLESTONE), (1, 0, COBBLESTONE), (2, 0, COBBLESTONE), (0, 1, ITEM_REDSTONE), (1, 1, ITEM_REDSTONE), (2, 1, ITEM_QUARTZ), (0, 2, COBBLESTONE), (1, 2, COBBLESTONE), (2, 2, COBBLESTONE)] },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 6), (STICK, 1)], result: (ITEM_ORANGE_BANNER, 1), name: "orange_banner", shaped: true, cells: &[(0, 0, ITEM_ORANGE_WOOL), (1, 0, ITEM_ORANGE_WOOL), (2, 0, ITEM_ORANGE_WOOL), (0, 1, ITEM_ORANGE_WOOL), (1, 1, ITEM_ORANGE_WOOL), (2, 1, ITEM_ORANGE_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_ORANGE_BED, 1), name: "orange_bed", shaped: true, cells: &[(0, 0, ITEM_ORANGE_WOOL), (1, 0, ITEM_ORANGE_WOOL), (2, 0, ITEM_ORANGE_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_ORANGE_DYE, 1)], result: (ITEM_ORANGE_CANDLE, 1), name: "orange_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 2)], result: (ITEM_ORANGE_CARPET, 3), name: "orange_carpet", shaped: true, cells: &[(0, 0, ITEM_ORANGE_WOOL), (1, 0, ITEM_ORANGE_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_ORANGE_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_ORANGE_CONCRETE_POWDER, 8), name: "orange_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_CONCRETE, 3)], result: (ITEM_ORANGE_CONCRETE_SLAB, 6), name: "orange_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_ORANGE_CONCRETE), (1, 0, ITEM_ORANGE_CONCRETE), (2, 0, ITEM_ORANGE_CONCRETE)] },
    Recipe { needs: &[(ITEM_ORANGE_CONCRETE, 6)], result: (ITEM_ORANGE_CONCRETE_STAIRS, 4), name: "orange_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_ORANGE_CONCRETE), (0, 1, ITEM_ORANGE_CONCRETE), (1, 1, ITEM_ORANGE_CONCRETE), (0, 2, ITEM_ORANGE_CONCRETE), (1, 2, ITEM_ORANGE_CONCRETE), (2, 2, ITEM_ORANGE_CONCRETE)] },
    Recipe { needs: &[(ITEM_ORANGE_WOOL_SLAB, 3)], result: (ITEM_ORANGE_CUSHION, 1), name: "orange_cushion", shaped: true, cells: &[(0, 0, ITEM_ORANGE_WOOL_SLAB), (1, 0, ITEM_ORANGE_WOOL_SLAB), (2, 0, ITEM_ORANGE_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_OPEN_EYEBLOSSOM, 1)], result: (ITEM_ORANGE_DYE, 1), name: "orange_dye_from_open_eyeblossom", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ORANGE_TULIP, 1)], result: (ITEM_ORANGE_DYE, 1), name: "orange_dye_from_orange_tulip", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (ITEM_YELLOW_DYE, 1)], result: (ITEM_ORANGE_DYE, 2), name: "orange_dye_from_red_yellow", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_TORCHFLOWER, 1)], result: (ITEM_ORANGE_DYE, 1), name: "orange_dye_from_torchflower", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_ORANGE_WOOL, 1)], result: (ITEM_ORANGE_HARNESS, 1), name: "orange_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_ORANGE_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_ORANGE_DYE, 1)], result: (ITEM_ORANGE_STAINED_GLASS, 8), name: "orange_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_ORANGE_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_ORANGE_STAINED_GLASS, 6)], result: (ITEM_ORANGE_STAINED_GLASS_PANE, 16), name: "orange_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_ORANGE_STAINED_GLASS), (1, 0, ITEM_ORANGE_STAINED_GLASS), (2, 0, ITEM_ORANGE_STAINED_GLASS), (0, 1, ITEM_ORANGE_STAINED_GLASS), (1, 1, ITEM_ORANGE_STAINED_GLASS), (2, 1, ITEM_ORANGE_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_ORANGE_DYE, 1)], result: (ITEM_ORANGE_STAINED_GLASS_PANE, 8), name: "orange_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_ORANGE_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_ORANGE_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_ORANGE_TERRACOTTA, 8), name: "orange_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_ORANGE_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 3)], result: (ITEM_ORANGE_WOOL_SLAB, 6), name: "orange_wool_slab", shaped: true, cells: &[(0, 0, ITEM_ORANGE_WOOL), (1, 0, ITEM_ORANGE_WOOL), (2, 0, ITEM_ORANGE_WOOL)] },
    Recipe { needs: &[(ITEM_ORANGE_WOOL, 6)], result: (ITEM_ORANGE_WOOL_STAIRS, 4), name: "orange_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_ORANGE_WOOL), (0, 1, ITEM_ORANGE_WOOL), (1, 1, ITEM_ORANGE_WOOL), (0, 2, ITEM_ORANGE_WOOL), (1, 2, ITEM_ORANGE_WOOL), (2, 2, ITEM_ORANGE_WOOL)] },
    Recipe { needs: &[(ITEM_OXIDIZED_CUT_COPPER_SLAB, 2)], result: (ITEM_OXIDIZED_CHISELED_COPPER, 1), name: "oxidized_chiseled_copper", shaped: true, cells: &[(0, 0, ITEM_OXIDIZED_CUT_COPPER_SLAB), (0, 1, ITEM_OXIDIZED_CUT_COPPER_SLAB)] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_OXIDIZED_COPPER, 3), (ITEM_REDSTONE, 1)], result: (ITEM_OXIDIZED_COPPER_BULB, 4), name: "oxidized_copper_bulb", shaped: true, cells: &[(1, 0, ITEM_OXIDIZED_COPPER), (0, 1, ITEM_OXIDIZED_COPPER), (1, 1, ITEM_BLAZE_ROD), (2, 1, ITEM_OXIDIZED_COPPER), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_OXIDIZED_COPPER, 4)], result: (ITEM_OXIDIZED_COPPER_GRATE, 4), name: "oxidized_copper_grate", shaped: true, cells: &[(1, 0, ITEM_OXIDIZED_COPPER), (0, 1, ITEM_OXIDIZED_COPPER), (2, 1, ITEM_OXIDIZED_COPPER), (1, 2, ITEM_OXIDIZED_COPPER)] },
    Recipe { needs: &[(ITEM_OXIDIZED_COPPER, 4)], result: (ITEM_OXIDIZED_CUT_COPPER, 4), name: "oxidized_cut_copper", shaped: true, cells: &[(0, 0, ITEM_OXIDIZED_COPPER), (1, 0, ITEM_OXIDIZED_COPPER), (0, 1, ITEM_OXIDIZED_COPPER), (1, 1, ITEM_OXIDIZED_COPPER)] },
    Recipe { needs: &[(ITEM_OXIDIZED_CUT_COPPER, 3)], result: (ITEM_OXIDIZED_CUT_COPPER_SLAB, 6), name: "oxidized_cut_copper_slab", shaped: true, cells: &[(0, 0, ITEM_OXIDIZED_CUT_COPPER), (1, 0, ITEM_OXIDIZED_CUT_COPPER), (2, 0, ITEM_OXIDIZED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_OXIDIZED_CUT_COPPER, 6)], result: (ITEM_OXIDIZED_CUT_COPPER_STAIRS, 4), name: "oxidized_cut_copper_stairs", shaped: true, cells: &[(0, 0, ITEM_OXIDIZED_CUT_COPPER), (0, 1, ITEM_OXIDIZED_CUT_COPPER), (1, 1, ITEM_OXIDIZED_CUT_COPPER), (0, 2, ITEM_OXIDIZED_CUT_COPPER), (1, 2, ITEM_OXIDIZED_CUT_COPPER), (2, 2, ITEM_OXIDIZED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1), (ITEM_ICE, 1)], result: (ITEM_PACKED_ICE, 1), name: "packed_ice", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_MUD, 1), (WHEAT, 1)], result: (ITEM_PACKED_MUD, 1), name: "packed_mud", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 8), (TAG_SULFUR_CUBE_ARCHETYPE_LIGHT, 1)], result: (ITEM_PAINTING, 1), name: "painting", shaped: true, cells: &[(0, 0, STICK), (1, 0, STICK), (2, 0, STICK), (0, 1, STICK), (1, 1, TAG_SULFUR_CUBE_ARCHETYPE_LIGHT), (2, 1, STICK), (0, 2, STICK), (1, 2, STICK), (2, 2, STICK)] },
    Recipe { needs: &[(ITEM_PALE_MOSS_BLOCK, 2)], result: (ITEM_PALE_MOSS_CARPET, 3), name: "pale_moss_carpet", shaped: true, cells: &[(0, 0, ITEM_PALE_MOSS_BLOCK), (1, 0, ITEM_PALE_MOSS_BLOCK)] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 5)], result: (ITEM_PALE_OAK_BOAT, 1), name: "pale_oak_boat", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_PLANKS), (2, 0, ITEM_PALE_OAK_PLANKS), (0, 1, ITEM_PALE_OAK_PLANKS), (1, 1, ITEM_PALE_OAK_PLANKS), (2, 1, ITEM_PALE_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 1)], result: (ITEM_PALE_OAK_BUTTON, 1), name: "pale_oak_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_PALE_OAK_BOAT, 1)], result: (ITEM_PALE_OAK_CHEST_BOAT, 1), name: "pale_oak_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 6)], result: (ITEM_PALE_OAK_DOOR, 3), name: "pale_oak_door", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_PLANKS), (1, 0, ITEM_PALE_OAK_PLANKS), (0, 1, ITEM_PALE_OAK_PLANKS), (1, 1, ITEM_PALE_OAK_PLANKS), (0, 2, ITEM_PALE_OAK_PLANKS), (1, 2, ITEM_PALE_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 4), (STICK, 2)], result: (ITEM_PALE_OAK_FENCE, 3), name: "pale_oak_fence", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_PLANKS), (1, 0, STICK), (2, 0, ITEM_PALE_OAK_PLANKS), (0, 1, ITEM_PALE_OAK_PLANKS), (1, 1, STICK), (2, 1, ITEM_PALE_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 2), (STICK, 4)], result: (ITEM_PALE_OAK_FENCE_GATE, 1), name: "pale_oak_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_PALE_OAK_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_PALE_OAK_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_PALE_OAK_LOG, 6)], result: (ITEM_PALE_OAK_HANGING_SIGN, 6), name: "pale_oak_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_PALE_OAK_LOG), (1, 1, ITEM_STRIPPED_PALE_OAK_LOG), (2, 1, ITEM_STRIPPED_PALE_OAK_LOG), (0, 2, ITEM_STRIPPED_PALE_OAK_LOG), (1, 2, ITEM_STRIPPED_PALE_OAK_LOG), (2, 2, ITEM_STRIPPED_PALE_OAK_LOG)] },
    Recipe { needs: &[(TAG_PALE_OAK_LOGS, 1)], result: (ITEM_PALE_OAK_PLANKS, 4), name: "pale_oak_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 2)], result: (ITEM_PALE_OAK_PRESSURE_PLATE, 1), name: "pale_oak_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_PLANKS), (1, 0, ITEM_PALE_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_PALE_OAK_LOG, 6)], result: (ITEM_PALE_OAK_SHELF, 6), name: "pale_oak_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_PALE_OAK_LOG), (1, 0, ITEM_STRIPPED_PALE_OAK_LOG), (2, 0, ITEM_STRIPPED_PALE_OAK_LOG), (0, 2, ITEM_STRIPPED_PALE_OAK_LOG), (1, 2, ITEM_STRIPPED_PALE_OAK_LOG), (2, 2, ITEM_STRIPPED_PALE_OAK_LOG)] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 6), (STICK, 1)], result: (ITEM_PALE_OAK_SIGN, 3), name: "pale_oak_sign", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_PLANKS), (1, 0, ITEM_PALE_OAK_PLANKS), (2, 0, ITEM_PALE_OAK_PLANKS), (0, 1, ITEM_PALE_OAK_PLANKS), (1, 1, ITEM_PALE_OAK_PLANKS), (2, 1, ITEM_PALE_OAK_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 3)], result: (ITEM_PALE_OAK_SLAB, 6), name: "pale_oak_slab", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_PLANKS), (1, 0, ITEM_PALE_OAK_PLANKS), (2, 0, ITEM_PALE_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 6)], result: (ITEM_PALE_OAK_STAIRS, 4), name: "pale_oak_stairs", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_PLANKS), (0, 1, ITEM_PALE_OAK_PLANKS), (1, 1, ITEM_PALE_OAK_PLANKS), (0, 2, ITEM_PALE_OAK_PLANKS), (1, 2, ITEM_PALE_OAK_PLANKS), (2, 2, ITEM_PALE_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_PALE_OAK_PLANKS, 6)], result: (ITEM_PALE_OAK_TRAPDOOR, 2), name: "pale_oak_trapdoor", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_PLANKS), (1, 0, ITEM_PALE_OAK_PLANKS), (2, 0, ITEM_PALE_OAK_PLANKS), (0, 1, ITEM_PALE_OAK_PLANKS), (1, 1, ITEM_PALE_OAK_PLANKS), (2, 1, ITEM_PALE_OAK_PLANKS)] },
    Recipe { needs: &[(ITEM_PALE_OAK_LOG, 4)], result: (ITEM_PALE_OAK_WOOD, 3), name: "pale_oak_wood", shaped: true, cells: &[(0, 0, ITEM_PALE_OAK_LOG), (1, 0, ITEM_PALE_OAK_LOG), (0, 1, ITEM_PALE_OAK_LOG), (1, 1, ITEM_PALE_OAK_LOG)] },
    Recipe { needs: &[(ITEM_SUGAR_CANE, 3)], result: (ITEM_PAPER, 3), name: "paper", shaped: true, cells: &[(0, 0, ITEM_SUGAR_CANE), (1, 0, ITEM_SUGAR_CANE), (2, 0, ITEM_SUGAR_CANE)] },
    Recipe { needs: &[(ITEM_PINK_WOOL, 6), (STICK, 1)], result: (ITEM_PINK_BANNER, 1), name: "pink_banner", shaped: true, cells: &[(0, 0, ITEM_PINK_WOOL), (1, 0, ITEM_PINK_WOOL), (2, 0, ITEM_PINK_WOOL), (0, 1, ITEM_PINK_WOOL), (1, 1, ITEM_PINK_WOOL), (2, 1, ITEM_PINK_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_PINK_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_PINK_BED, 1), name: "pink_bed", shaped: true, cells: &[(0, 0, ITEM_PINK_WOOL), (1, 0, ITEM_PINK_WOOL), (2, 0, ITEM_PINK_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_PINK_DYE, 1)], result: (ITEM_PINK_CANDLE, 1), name: "pink_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_WOOL, 2)], result: (ITEM_PINK_CARPET, 3), name: "pink_carpet", shaped: true, cells: &[(0, 0, ITEM_PINK_WOOL), (1, 0, ITEM_PINK_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_PINK_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_PINK_CONCRETE_POWDER, 8), name: "pink_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_CONCRETE, 3)], result: (ITEM_PINK_CONCRETE_SLAB, 6), name: "pink_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_PINK_CONCRETE), (1, 0, ITEM_PINK_CONCRETE), (2, 0, ITEM_PINK_CONCRETE)] },
    Recipe { needs: &[(ITEM_PINK_CONCRETE, 6)], result: (ITEM_PINK_CONCRETE_STAIRS, 4), name: "pink_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_PINK_CONCRETE), (0, 1, ITEM_PINK_CONCRETE), (1, 1, ITEM_PINK_CONCRETE), (0, 2, ITEM_PINK_CONCRETE), (1, 2, ITEM_PINK_CONCRETE), (2, 2, ITEM_PINK_CONCRETE)] },
    Recipe { needs: &[(ITEM_PINK_WOOL_SLAB, 3)], result: (ITEM_PINK_CUSHION, 1), name: "pink_cushion", shaped: true, cells: &[(0, 0, ITEM_PINK_WOOL_SLAB), (1, 0, ITEM_PINK_WOOL_SLAB), (2, 0, ITEM_PINK_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_CACTUS_FLOWER, 1)], result: (ITEM_PINK_DYE, 1), name: "pink_dye_from_cactus_flower", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PEONY, 1)], result: (ITEM_PINK_DYE, 2), name: "pink_dye_from_peony", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_PETALS, 1)], result: (ITEM_PINK_DYE, 1), name: "pink_dye_from_pink_petals", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PINK_TULIP, 1)], result: (ITEM_PINK_DYE, 1), name: "pink_dye_from_pink_tulip", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_PINK_DYE, 2), name: "pink_dye_from_red_white_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_PINK_WOOL, 1)], result: (ITEM_PINK_HARNESS, 1), name: "pink_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_PINK_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_PINK_DYE, 1)], result: (ITEM_PINK_STAINED_GLASS, 8), name: "pink_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_PINK_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_PINK_STAINED_GLASS, 6)], result: (ITEM_PINK_STAINED_GLASS_PANE, 16), name: "pink_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_PINK_STAINED_GLASS), (1, 0, ITEM_PINK_STAINED_GLASS), (2, 0, ITEM_PINK_STAINED_GLASS), (0, 1, ITEM_PINK_STAINED_GLASS), (1, 1, ITEM_PINK_STAINED_GLASS), (2, 1, ITEM_PINK_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_PINK_DYE, 1)], result: (ITEM_PINK_STAINED_GLASS_PANE, 8), name: "pink_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_PINK_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_PINK_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_PINK_TERRACOTTA, 8), name: "pink_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_PINK_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_PINK_WOOL, 3)], result: (ITEM_PINK_WOOL_SLAB, 6), name: "pink_wool_slab", shaped: true, cells: &[(0, 0, ITEM_PINK_WOOL), (1, 0, ITEM_PINK_WOOL), (2, 0, ITEM_PINK_WOOL)] },
    Recipe { needs: &[(ITEM_PINK_WOOL, 6)], result: (ITEM_PINK_WOOL_STAIRS, 4), name: "pink_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_PINK_WOOL), (0, 1, ITEM_PINK_WOOL), (1, 1, ITEM_PINK_WOOL), (0, 2, ITEM_PINK_WOOL), (1, 2, ITEM_PINK_WOOL), (2, 2, ITEM_PINK_WOOL)] },
    Recipe { needs: &[(COBBLESTONE, 4), (ITEM_IRON_INGOT, 1), (ITEM_REDSTONE, 1), (TAG_PLANKS, 3)], result: (ITEM_PISTON, 1), name: "piston", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (0, 1, COBBLESTONE), (1, 1, ITEM_IRON_INGOT), (2, 1, COBBLESTONE), (0, 2, COBBLESTONE), (1, 2, ITEM_REDSTONE), (2, 2, COBBLESTONE)] },
    Recipe { needs: &[(ITEM_ANDESITE, 4)], result: (ITEM_POLISHED_ANDESITE, 4), name: "polished_andesite", shaped: true, cells: &[(0, 0, ITEM_ANDESITE), (1, 0, ITEM_ANDESITE), (0, 1, ITEM_ANDESITE), (1, 1, ITEM_ANDESITE)] },
    Recipe { needs: &[(ITEM_POLISHED_ANDESITE, 3)], result: (ITEM_POLISHED_ANDESITE_SLAB, 6), name: "polished_andesite_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_ANDESITE), (1, 0, ITEM_POLISHED_ANDESITE), (2, 0, ITEM_POLISHED_ANDESITE)] },
    Recipe { needs: &[(ITEM_POLISHED_ANDESITE, 6)], result: (ITEM_POLISHED_ANDESITE_STAIRS, 4), name: "polished_andesite_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_ANDESITE), (0, 1, ITEM_POLISHED_ANDESITE), (1, 1, ITEM_POLISHED_ANDESITE), (0, 2, ITEM_POLISHED_ANDESITE), (1, 2, ITEM_POLISHED_ANDESITE), (2, 2, ITEM_POLISHED_ANDESITE)] },
    Recipe { needs: &[(ITEM_BASALT, 4)], result: (ITEM_POLISHED_BASALT, 4), name: "polished_basalt", shaped: true, cells: &[(0, 0, ITEM_BASALT), (1, 0, ITEM_BASALT), (0, 1, ITEM_BASALT), (1, 1, ITEM_BASALT)] },
    Recipe { needs: &[(ITEM_BLACKSTONE, 4)], result: (ITEM_POLISHED_BLACKSTONE, 4), name: "polished_blackstone", shaped: true, cells: &[(0, 0, ITEM_BLACKSTONE), (1, 0, ITEM_BLACKSTONE), (0, 1, ITEM_BLACKSTONE), (1, 1, ITEM_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE_BRICKS, 3)], result: (ITEM_POLISHED_BLACKSTONE_BRICK_SLAB, 6), name: "polished_blackstone_brick_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE_BRICKS), (1, 0, ITEM_POLISHED_BLACKSTONE_BRICKS), (2, 0, ITEM_POLISHED_BLACKSTONE_BRICKS)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE_BRICKS, 6)], result: (ITEM_POLISHED_BLACKSTONE_BRICK_STAIRS, 4), name: "polished_blackstone_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE_BRICKS), (0, 1, ITEM_POLISHED_BLACKSTONE_BRICKS), (1, 1, ITEM_POLISHED_BLACKSTONE_BRICKS), (0, 2, ITEM_POLISHED_BLACKSTONE_BRICKS), (1, 2, ITEM_POLISHED_BLACKSTONE_BRICKS), (2, 2, ITEM_POLISHED_BLACKSTONE_BRICKS)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE_BRICKS, 6)], result: (ITEM_POLISHED_BLACKSTONE_BRICK_WALL, 6), name: "polished_blackstone_brick_wall", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE_BRICKS), (1, 0, ITEM_POLISHED_BLACKSTONE_BRICKS), (2, 0, ITEM_POLISHED_BLACKSTONE_BRICKS), (0, 1, ITEM_POLISHED_BLACKSTONE_BRICKS), (1, 1, ITEM_POLISHED_BLACKSTONE_BRICKS), (2, 1, ITEM_POLISHED_BLACKSTONE_BRICKS)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 4)], result: (ITEM_POLISHED_BLACKSTONE_BRICKS, 4), name: "polished_blackstone_bricks", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE), (1, 0, ITEM_POLISHED_BLACKSTONE), (0, 1, ITEM_POLISHED_BLACKSTONE), (1, 1, ITEM_POLISHED_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 1)], result: (ITEM_POLISHED_BLACKSTONE_BUTTON, 1), name: "polished_blackstone_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 2)], result: (ITEM_POLISHED_BLACKSTONE_PRESSURE_PLATE, 1), name: "polished_blackstone_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE), (1, 0, ITEM_POLISHED_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 3)], result: (ITEM_POLISHED_BLACKSTONE_SLAB, 6), name: "polished_blackstone_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE), (1, 0, ITEM_POLISHED_BLACKSTONE), (2, 0, ITEM_POLISHED_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 6)], result: (ITEM_POLISHED_BLACKSTONE_STAIRS, 4), name: "polished_blackstone_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE), (0, 1, ITEM_POLISHED_BLACKSTONE), (1, 1, ITEM_POLISHED_BLACKSTONE), (0, 2, ITEM_POLISHED_BLACKSTONE), (1, 2, ITEM_POLISHED_BLACKSTONE), (2, 2, ITEM_POLISHED_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_POLISHED_BLACKSTONE, 6)], result: (ITEM_POLISHED_BLACKSTONE_WALL, 6), name: "polished_blackstone_wall", shaped: true, cells: &[(0, 0, ITEM_POLISHED_BLACKSTONE), (1, 0, ITEM_POLISHED_BLACKSTONE), (2, 0, ITEM_POLISHED_BLACKSTONE), (0, 1, ITEM_POLISHED_BLACKSTONE), (1, 1, ITEM_POLISHED_BLACKSTONE), (2, 1, ITEM_POLISHED_BLACKSTONE)] },
    Recipe { needs: &[(ITEM_CINNABAR, 4)], result: (ITEM_POLISHED_CINNABAR, 4), name: "polished_cinnabar", shaped: true, cells: &[(0, 0, ITEM_CINNABAR), (1, 0, ITEM_CINNABAR), (0, 1, ITEM_CINNABAR), (1, 1, ITEM_CINNABAR)] },
    Recipe { needs: &[(ITEM_POLISHED_CINNABAR, 3)], result: (ITEM_POLISHED_CINNABAR_SLAB, 6), name: "polished_cinnabar_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_CINNABAR), (1, 0, ITEM_POLISHED_CINNABAR), (2, 0, ITEM_POLISHED_CINNABAR)] },
    Recipe { needs: &[(ITEM_POLISHED_CINNABAR, 6)], result: (ITEM_POLISHED_CINNABAR_STAIRS, 4), name: "polished_cinnabar_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_CINNABAR), (0, 1, ITEM_POLISHED_CINNABAR), (1, 1, ITEM_POLISHED_CINNABAR), (0, 2, ITEM_POLISHED_CINNABAR), (1, 2, ITEM_POLISHED_CINNABAR), (2, 2, ITEM_POLISHED_CINNABAR)] },
    Recipe { needs: &[(ITEM_POLISHED_CINNABAR, 6)], result: (ITEM_POLISHED_CINNABAR_WALL, 6), name: "polished_cinnabar_wall", shaped: true, cells: &[(0, 0, ITEM_POLISHED_CINNABAR), (1, 0, ITEM_POLISHED_CINNABAR), (2, 0, ITEM_POLISHED_CINNABAR), (0, 1, ITEM_POLISHED_CINNABAR), (1, 1, ITEM_POLISHED_CINNABAR), (2, 1, ITEM_POLISHED_CINNABAR)] },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 4)], result: (ITEM_POLISHED_DEEPSLATE, 4), name: "polished_deepslate", shaped: true, cells: &[(0, 0, ITEM_COBBLED_DEEPSLATE), (1, 0, ITEM_COBBLED_DEEPSLATE), (0, 1, ITEM_COBBLED_DEEPSLATE), (1, 1, ITEM_COBBLED_DEEPSLATE)] },
    Recipe { needs: &[(ITEM_POLISHED_DEEPSLATE, 3)], result: (ITEM_POLISHED_DEEPSLATE_SLAB, 6), name: "polished_deepslate_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_DEEPSLATE), (1, 0, ITEM_POLISHED_DEEPSLATE), (2, 0, ITEM_POLISHED_DEEPSLATE)] },
    Recipe { needs: &[(ITEM_POLISHED_DEEPSLATE, 6)], result: (ITEM_POLISHED_DEEPSLATE_STAIRS, 4), name: "polished_deepslate_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_DEEPSLATE), (0, 1, ITEM_POLISHED_DEEPSLATE), (1, 1, ITEM_POLISHED_DEEPSLATE), (0, 2, ITEM_POLISHED_DEEPSLATE), (1, 2, ITEM_POLISHED_DEEPSLATE), (2, 2, ITEM_POLISHED_DEEPSLATE)] },
    Recipe { needs: &[(ITEM_POLISHED_DEEPSLATE, 6)], result: (ITEM_POLISHED_DEEPSLATE_WALL, 6), name: "polished_deepslate_wall", shaped: true, cells: &[(0, 0, ITEM_POLISHED_DEEPSLATE), (1, 0, ITEM_POLISHED_DEEPSLATE), (2, 0, ITEM_POLISHED_DEEPSLATE), (0, 1, ITEM_POLISHED_DEEPSLATE), (1, 1, ITEM_POLISHED_DEEPSLATE), (2, 1, ITEM_POLISHED_DEEPSLATE)] },
    Recipe { needs: &[(ITEM_DIORITE, 4)], result: (ITEM_POLISHED_DIORITE, 4), name: "polished_diorite", shaped: true, cells: &[(0, 0, ITEM_DIORITE), (1, 0, ITEM_DIORITE), (0, 1, ITEM_DIORITE), (1, 1, ITEM_DIORITE)] },
    Recipe { needs: &[(ITEM_POLISHED_DIORITE, 3)], result: (ITEM_POLISHED_DIORITE_SLAB, 6), name: "polished_diorite_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_DIORITE), (1, 0, ITEM_POLISHED_DIORITE), (2, 0, ITEM_POLISHED_DIORITE)] },
    Recipe { needs: &[(ITEM_POLISHED_DIORITE, 6)], result: (ITEM_POLISHED_DIORITE_STAIRS, 4), name: "polished_diorite_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_DIORITE), (0, 1, ITEM_POLISHED_DIORITE), (1, 1, ITEM_POLISHED_DIORITE), (0, 2, ITEM_POLISHED_DIORITE), (1, 2, ITEM_POLISHED_DIORITE), (2, 2, ITEM_POLISHED_DIORITE)] },
    Recipe { needs: &[(ITEM_GRANITE, 4)], result: (ITEM_POLISHED_GRANITE, 4), name: "polished_granite", shaped: true, cells: &[(0, 0, ITEM_GRANITE), (1, 0, ITEM_GRANITE), (0, 1, ITEM_GRANITE), (1, 1, ITEM_GRANITE)] },
    Recipe { needs: &[(ITEM_POLISHED_GRANITE, 3)], result: (ITEM_POLISHED_GRANITE_SLAB, 6), name: "polished_granite_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_GRANITE), (1, 0, ITEM_POLISHED_GRANITE), (2, 0, ITEM_POLISHED_GRANITE)] },
    Recipe { needs: &[(ITEM_POLISHED_GRANITE, 6)], result: (ITEM_POLISHED_GRANITE_STAIRS, 4), name: "polished_granite_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_GRANITE), (0, 1, ITEM_POLISHED_GRANITE), (1, 1, ITEM_POLISHED_GRANITE), (0, 2, ITEM_POLISHED_GRANITE), (1, 2, ITEM_POLISHED_GRANITE), (2, 2, ITEM_POLISHED_GRANITE)] },
    Recipe { needs: &[(ITEM_SULFUR, 4)], result: (ITEM_POLISHED_SULFUR, 4), name: "polished_sulfur", shaped: true, cells: &[(0, 0, ITEM_SULFUR), (1, 0, ITEM_SULFUR), (0, 1, ITEM_SULFUR), (1, 1, ITEM_SULFUR)] },
    Recipe { needs: &[(ITEM_POLISHED_SULFUR, 3)], result: (ITEM_POLISHED_SULFUR_SLAB, 6), name: "polished_sulfur_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_SULFUR), (1, 0, ITEM_POLISHED_SULFUR), (2, 0, ITEM_POLISHED_SULFUR)] },
    Recipe { needs: &[(ITEM_POLISHED_SULFUR, 6)], result: (ITEM_POLISHED_SULFUR_STAIRS, 4), name: "polished_sulfur_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_SULFUR), (0, 1, ITEM_POLISHED_SULFUR), (1, 1, ITEM_POLISHED_SULFUR), (0, 2, ITEM_POLISHED_SULFUR), (1, 2, ITEM_POLISHED_SULFUR), (2, 2, ITEM_POLISHED_SULFUR)] },
    Recipe { needs: &[(ITEM_POLISHED_SULFUR, 6)], result: (ITEM_POLISHED_SULFUR_WALL, 6), name: "polished_sulfur_wall", shaped: true, cells: &[(0, 0, ITEM_POLISHED_SULFUR), (1, 0, ITEM_POLISHED_SULFUR), (2, 0, ITEM_POLISHED_SULFUR), (0, 1, ITEM_POLISHED_SULFUR), (1, 1, ITEM_POLISHED_SULFUR), (2, 1, ITEM_POLISHED_SULFUR)] },
    Recipe { needs: &[(ITEM_TUFF, 4)], result: (ITEM_POLISHED_TUFF, 4), name: "polished_tuff", shaped: true, cells: &[(0, 0, ITEM_TUFF), (1, 0, ITEM_TUFF), (0, 1, ITEM_TUFF), (1, 1, ITEM_TUFF)] },
    Recipe { needs: &[(ITEM_POLISHED_TUFF, 3)], result: (ITEM_POLISHED_TUFF_SLAB, 6), name: "polished_tuff_slab", shaped: true, cells: &[(0, 0, ITEM_POLISHED_TUFF), (1, 0, ITEM_POLISHED_TUFF), (2, 0, ITEM_POLISHED_TUFF)] },
    Recipe { needs: &[(ITEM_POLISHED_TUFF, 6)], result: (ITEM_POLISHED_TUFF_STAIRS, 4), name: "polished_tuff_stairs", shaped: true, cells: &[(0, 0, ITEM_POLISHED_TUFF), (0, 1, ITEM_POLISHED_TUFF), (1, 1, ITEM_POLISHED_TUFF), (0, 2, ITEM_POLISHED_TUFF), (1, 2, ITEM_POLISHED_TUFF), (2, 2, ITEM_POLISHED_TUFF)] },
    Recipe { needs: &[(ITEM_POLISHED_TUFF, 6)], result: (ITEM_POLISHED_TUFF_WALL, 6), name: "polished_tuff_wall", shaped: true, cells: &[(0, 0, ITEM_POLISHED_TUFF), (1, 0, ITEM_POLISHED_TUFF), (2, 0, ITEM_POLISHED_TUFF), (0, 1, ITEM_POLISHED_TUFF), (1, 1, ITEM_POLISHED_TUFF), (2, 1, ITEM_POLISHED_TUFF)] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 5)], result: (ITEM_POPLAR_BOAT, 1), name: "poplar_boat", shaped: true, cells: &[(0, 0, ITEM_POPLAR_PLANKS), (2, 0, ITEM_POPLAR_PLANKS), (0, 1, ITEM_POPLAR_PLANKS), (1, 1, ITEM_POPLAR_PLANKS), (2, 1, ITEM_POPLAR_PLANKS)] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 1)], result: (ITEM_POPLAR_BUTTON, 1), name: "poplar_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_POPLAR_BOAT, 1)], result: (ITEM_POPLAR_CHEST_BOAT, 1), name: "poplar_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 6)], result: (ITEM_POPLAR_DOOR, 3), name: "poplar_door", shaped: true, cells: &[(0, 0, ITEM_POPLAR_PLANKS), (1, 0, ITEM_POPLAR_PLANKS), (0, 1, ITEM_POPLAR_PLANKS), (1, 1, ITEM_POPLAR_PLANKS), (0, 2, ITEM_POPLAR_PLANKS), (1, 2, ITEM_POPLAR_PLANKS)] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 4), (STICK, 2)], result: (ITEM_POPLAR_FENCE, 3), name: "poplar_fence", shaped: true, cells: &[(0, 0, ITEM_POPLAR_PLANKS), (1, 0, STICK), (2, 0, ITEM_POPLAR_PLANKS), (0, 1, ITEM_POPLAR_PLANKS), (1, 1, STICK), (2, 1, ITEM_POPLAR_PLANKS)] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 2), (STICK, 4)], result: (ITEM_POPLAR_FENCE_GATE, 1), name: "poplar_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_POPLAR_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_POPLAR_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_POPLAR_LOG, 6)], result: (ITEM_POPLAR_HANGING_SIGN, 6), name: "poplar_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_POPLAR_LOG), (1, 1, ITEM_STRIPPED_POPLAR_LOG), (2, 1, ITEM_STRIPPED_POPLAR_LOG), (0, 2, ITEM_STRIPPED_POPLAR_LOG), (1, 2, ITEM_STRIPPED_POPLAR_LOG), (2, 2, ITEM_STRIPPED_POPLAR_LOG)] },
    Recipe { needs: &[(TAG_POPLAR_LOGS, 1)], result: (ITEM_POPLAR_PLANKS, 4), name: "poplar_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 2)], result: (ITEM_POPLAR_PRESSURE_PLATE, 1), name: "poplar_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_POPLAR_PLANKS), (1, 0, ITEM_POPLAR_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_POPLAR_LOG, 6)], result: (ITEM_POPLAR_SHELF, 6), name: "poplar_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_POPLAR_LOG), (1, 0, ITEM_STRIPPED_POPLAR_LOG), (2, 0, ITEM_STRIPPED_POPLAR_LOG), (0, 2, ITEM_STRIPPED_POPLAR_LOG), (1, 2, ITEM_STRIPPED_POPLAR_LOG), (2, 2, ITEM_STRIPPED_POPLAR_LOG)] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 6), (STICK, 1)], result: (ITEM_POPLAR_SIGN, 3), name: "poplar_sign", shaped: true, cells: &[(0, 0, ITEM_POPLAR_PLANKS), (1, 0, ITEM_POPLAR_PLANKS), (2, 0, ITEM_POPLAR_PLANKS), (0, 1, ITEM_POPLAR_PLANKS), (1, 1, ITEM_POPLAR_PLANKS), (2, 1, ITEM_POPLAR_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 3)], result: (ITEM_POPLAR_SLAB, 6), name: "poplar_slab", shaped: true, cells: &[(0, 0, ITEM_POPLAR_PLANKS), (1, 0, ITEM_POPLAR_PLANKS), (2, 0, ITEM_POPLAR_PLANKS)] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 6)], result: (ITEM_POPLAR_STAIRS, 4), name: "poplar_stairs", shaped: true, cells: &[(0, 0, ITEM_POPLAR_PLANKS), (0, 1, ITEM_POPLAR_PLANKS), (1, 1, ITEM_POPLAR_PLANKS), (0, 2, ITEM_POPLAR_PLANKS), (1, 2, ITEM_POPLAR_PLANKS), (2, 2, ITEM_POPLAR_PLANKS)] },
    Recipe { needs: &[(ITEM_POPLAR_PLANKS, 6)], result: (ITEM_POPLAR_TRAPDOOR, 2), name: "poplar_trapdoor", shaped: true, cells: &[(0, 0, ITEM_POPLAR_PLANKS), (1, 0, ITEM_POPLAR_PLANKS), (2, 0, ITEM_POPLAR_PLANKS), (0, 1, ITEM_POPLAR_PLANKS), (1, 1, ITEM_POPLAR_PLANKS), (2, 1, ITEM_POPLAR_PLANKS)] },
    Recipe { needs: &[(ITEM_POPLAR_LOG, 4)], result: (ITEM_POPLAR_WOOD, 3), name: "poplar_wood", shaped: true, cells: &[(0, 0, ITEM_POPLAR_LOG), (1, 0, ITEM_POPLAR_LOG), (0, 1, ITEM_POPLAR_LOG), (1, 1, ITEM_POPLAR_LOG)] },
    Recipe { needs: &[(ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1), (ITEM_SULFUR, 1)], result: (ITEM_POTENT_SULFUR, 1), name: "potent_sulfur", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GOLD_INGOT, 6), (ITEM_REDSTONE, 1), (STICK, 1)], result: (ITEM_POWERED_RAIL, 6), name: "powered_rail", shaped: true, cells: &[(0, 0, ITEM_GOLD_INGOT), (2, 0, ITEM_GOLD_INGOT), (0, 1, ITEM_GOLD_INGOT), (1, 1, STICK), (2, 1, ITEM_GOLD_INGOT), (0, 2, ITEM_GOLD_INGOT), (1, 2, ITEM_REDSTONE), (2, 2, ITEM_GOLD_INGOT)] },
    Recipe { needs: &[(ITEM_PRISMARINE_SHARD, 4)], result: (ITEM_PRISMARINE, 1), name: "prismarine", shaped: true, cells: &[(0, 0, ITEM_PRISMARINE_SHARD), (1, 0, ITEM_PRISMARINE_SHARD), (0, 1, ITEM_PRISMARINE_SHARD), (1, 1, ITEM_PRISMARINE_SHARD)] },
    Recipe { needs: &[(ITEM_PRISMARINE_BRICKS, 3)], result: (ITEM_PRISMARINE_BRICK_SLAB, 6), name: "prismarine_brick_slab", shaped: true, cells: &[(0, 0, ITEM_PRISMARINE_BRICKS), (1, 0, ITEM_PRISMARINE_BRICKS), (2, 0, ITEM_PRISMARINE_BRICKS)] },
    Recipe { needs: &[(ITEM_PRISMARINE_BRICKS, 6)], result: (ITEM_PRISMARINE_BRICK_STAIRS, 4), name: "prismarine_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_PRISMARINE_BRICKS), (0, 1, ITEM_PRISMARINE_BRICKS), (1, 1, ITEM_PRISMARINE_BRICKS), (0, 2, ITEM_PRISMARINE_BRICKS), (1, 2, ITEM_PRISMARINE_BRICKS), (2, 2, ITEM_PRISMARINE_BRICKS)] },
    Recipe { needs: &[(ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1), (ITEM_PRISMARINE_SHARD, 1)], result: (ITEM_PRISMARINE_BRICKS, 1), name: "prismarine_bricks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PRISMARINE, 3)], result: (ITEM_PRISMARINE_SLAB, 6), name: "prismarine_slab", shaped: true, cells: &[(0, 0, ITEM_PRISMARINE), (1, 0, ITEM_PRISMARINE), (2, 0, ITEM_PRISMARINE)] },
    Recipe { needs: &[(ITEM_PRISMARINE, 6)], result: (ITEM_PRISMARINE_STAIRS, 4), name: "prismarine_stairs", shaped: true, cells: &[(0, 0, ITEM_PRISMARINE), (0, 1, ITEM_PRISMARINE), (1, 1, ITEM_PRISMARINE), (0, 2, ITEM_PRISMARINE), (1, 2, ITEM_PRISMARINE), (2, 2, ITEM_PRISMARINE)] },
    Recipe { needs: &[(ITEM_PRISMARINE, 6)], result: (ITEM_PRISMARINE_WALL, 6), name: "prismarine_wall", shaped: true, cells: &[(0, 0, ITEM_PRISMARINE), (1, 0, ITEM_PRISMARINE), (2, 0, ITEM_PRISMARINE), (0, 1, ITEM_PRISMARINE), (1, 1, ITEM_PRISMARINE), (2, 1, ITEM_PRISMARINE)] },
    Recipe { needs: &[(ITEM_PUMPKIN, 1), (ITEM_SUGAR, 1), (TAG_EGGS, 1)], result: (ITEM_PUMPKIN_PIE, 1), name: "pumpkin_pie", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PUMPKIN, 1)], result: (ITEM_PUMPKIN_SEEDS, 4), name: "pumpkin_seeds", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 6), (STICK, 1)], result: (ITEM_PURPLE_BANNER, 1), name: "purple_banner", shaped: true, cells: &[(0, 0, ITEM_PURPLE_WOOL), (1, 0, ITEM_PURPLE_WOOL), (2, 0, ITEM_PURPLE_WOOL), (0, 1, ITEM_PURPLE_WOOL), (1, 1, ITEM_PURPLE_WOOL), (2, 1, ITEM_PURPLE_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_PURPLE_BED, 1), name: "purple_bed", shaped: true, cells: &[(0, 0, ITEM_PURPLE_WOOL), (1, 0, ITEM_PURPLE_WOOL), (2, 0, ITEM_PURPLE_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_PURPLE_DYE, 1)], result: (ITEM_PURPLE_CANDLE, 1), name: "purple_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 2)], result: (ITEM_PURPLE_CARPET, 3), name: "purple_carpet", shaped: true, cells: &[(0, 0, ITEM_PURPLE_WOOL), (1, 0, ITEM_PURPLE_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_PURPLE_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_PURPLE_CONCRETE_POWDER, 8), name: "purple_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_PURPLE_CONCRETE, 3)], result: (ITEM_PURPLE_CONCRETE_SLAB, 6), name: "purple_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_PURPLE_CONCRETE), (1, 0, ITEM_PURPLE_CONCRETE), (2, 0, ITEM_PURPLE_CONCRETE)] },
    Recipe { needs: &[(ITEM_PURPLE_CONCRETE, 6)], result: (ITEM_PURPLE_CONCRETE_STAIRS, 4), name: "purple_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_PURPLE_CONCRETE), (0, 1, ITEM_PURPLE_CONCRETE), (1, 1, ITEM_PURPLE_CONCRETE), (0, 2, ITEM_PURPLE_CONCRETE), (1, 2, ITEM_PURPLE_CONCRETE), (2, 2, ITEM_PURPLE_CONCRETE)] },
    Recipe { needs: &[(ITEM_PURPLE_WOOL_SLAB, 3)], result: (ITEM_PURPLE_CUSHION, 1), name: "purple_cushion", shaped: true, cells: &[(0, 0, ITEM_PURPLE_WOOL_SLAB), (1, 0, ITEM_PURPLE_WOOL_SLAB), (2, 0, ITEM_PURPLE_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_BLUE_DYE, 1), (ITEM_RED_DYE, 1)], result: (ITEM_PURPLE_DYE, 2), name: "purple_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_PURPLE_WOOL, 1)], result: (ITEM_PURPLE_HARNESS, 1), name: "purple_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_PURPLE_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_PURPLE_DYE, 1)], result: (ITEM_PURPLE_STAINED_GLASS, 8), name: "purple_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_PURPLE_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_PURPLE_STAINED_GLASS, 6)], result: (ITEM_PURPLE_STAINED_GLASS_PANE, 16), name: "purple_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_PURPLE_STAINED_GLASS), (1, 0, ITEM_PURPLE_STAINED_GLASS), (2, 0, ITEM_PURPLE_STAINED_GLASS), (0, 1, ITEM_PURPLE_STAINED_GLASS), (1, 1, ITEM_PURPLE_STAINED_GLASS), (2, 1, ITEM_PURPLE_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_PURPLE_DYE, 1)], result: (ITEM_PURPLE_STAINED_GLASS_PANE, 8), name: "purple_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_PURPLE_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_PURPLE_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_PURPLE_TERRACOTTA, 8), name: "purple_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_PURPLE_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 3)], result: (ITEM_PURPLE_WOOL_SLAB, 6), name: "purple_wool_slab", shaped: true, cells: &[(0, 0, ITEM_PURPLE_WOOL), (1, 0, ITEM_PURPLE_WOOL), (2, 0, ITEM_PURPLE_WOOL)] },
    Recipe { needs: &[(ITEM_PURPLE_WOOL, 6)], result: (ITEM_PURPLE_WOOL_STAIRS, 4), name: "purple_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_PURPLE_WOOL), (0, 1, ITEM_PURPLE_WOOL), (1, 1, ITEM_PURPLE_WOOL), (0, 2, ITEM_PURPLE_WOOL), (1, 2, ITEM_PURPLE_WOOL), (2, 2, ITEM_PURPLE_WOOL)] },
    Recipe { needs: &[(ITEM_POPPED_CHORUS_FRUIT, 4)], result: (ITEM_PURPUR_BLOCK, 4), name: "purpur_block", shaped: true, cells: &[(0, 0, ITEM_POPPED_CHORUS_FRUIT), (1, 0, ITEM_POPPED_CHORUS_FRUIT), (0, 1, ITEM_POPPED_CHORUS_FRUIT), (1, 1, ITEM_POPPED_CHORUS_FRUIT)] },
    Recipe { needs: &[(ITEM_PURPUR_SLAB, 2)], result: (ITEM_PURPUR_PILLAR, 1), name: "purpur_pillar", shaped: true, cells: &[(0, 0, ITEM_PURPUR_SLAB), (0, 1, ITEM_PURPUR_SLAB)] },
    Recipe { needs: &[(TAG_ALT_114, 3)], result: (ITEM_PURPUR_SLAB, 6), name: "purpur_slab", shaped: true, cells: &[(0, 0, TAG_ALT_114), (1, 0, TAG_ALT_114), (2, 0, TAG_ALT_114)] },
    Recipe { needs: &[(TAG_ALT_114, 6)], result: (ITEM_PURPUR_STAIRS, 4), name: "purpur_stairs", shaped: true, cells: &[(0, 0, TAG_ALT_114), (0, 1, TAG_ALT_114), (1, 1, TAG_ALT_114), (0, 2, TAG_ALT_114), (1, 2, TAG_ALT_114), (2, 2, TAG_ALT_114)] },
    Recipe { needs: &[(ITEM_QUARTZ, 4)], result: (ITEM_QUARTZ_BLOCK, 1), name: "quartz_block", shaped: true, cells: &[(0, 0, ITEM_QUARTZ), (1, 0, ITEM_QUARTZ), (0, 1, ITEM_QUARTZ), (1, 1, ITEM_QUARTZ)] },
    Recipe { needs: &[(ITEM_QUARTZ_BLOCK, 4)], result: (ITEM_QUARTZ_BRICKS, 4), name: "quartz_bricks", shaped: true, cells: &[(0, 0, ITEM_QUARTZ_BLOCK), (1, 0, ITEM_QUARTZ_BLOCK), (0, 1, ITEM_QUARTZ_BLOCK), (1, 1, ITEM_QUARTZ_BLOCK)] },
    Recipe { needs: &[(ITEM_QUARTZ_BLOCK, 2)], result: (ITEM_QUARTZ_PILLAR, 2), name: "quartz_pillar", shaped: true, cells: &[(0, 0, ITEM_QUARTZ_BLOCK), (0, 1, ITEM_QUARTZ_BLOCK)] },
    Recipe { needs: &[(TAG_ALT_115, 3)], result: (ITEM_QUARTZ_SLAB, 6), name: "quartz_slab", shaped: true, cells: &[(0, 0, TAG_ALT_115), (1, 0, TAG_ALT_115), (2, 0, TAG_ALT_115)] },
    Recipe { needs: &[(TAG_ALT_115, 6)], result: (ITEM_QUARTZ_STAIRS, 4), name: "quartz_stairs", shaped: true, cells: &[(0, 0, TAG_ALT_115), (0, 1, TAG_ALT_115), (1, 1, TAG_ALT_115), (0, 2, TAG_ALT_115), (1, 2, TAG_ALT_115), (2, 2, TAG_ALT_115)] },
    Recipe { needs: &[(ITEM_BAKED_POTATO, 1), (ITEM_BOWL, 1), (ITEM_BROWN_MUSHROOM, 1), (ITEM_CARROT, 1), (ITEM_COOKED_RABBIT, 1)], result: (ITEM_RABBIT_STEW, 1), name: "rabbit_stew_from_brown_mushroom", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BAKED_POTATO, 1), (ITEM_BOWL, 1), (ITEM_CARROT, 1), (ITEM_COOKED_RABBIT, 1), (ITEM_RED_MUSHROOM, 1)], result: (ITEM_RABBIT_STEW, 1), name: "rabbit_stew_from_red_mushroom", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BAKED_POTATO, 1), (ITEM_BOWL, 1), (ITEM_CARROT, 1), (ITEM_COOKED_RABBIT, 1), (ITEM_SHELF_MUSHROOM, 1)], result: (ITEM_RABBIT_STEW, 1), name: "rabbit_stew_from_shelf_mushroom", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 6), (STICK, 1)], result: (ITEM_RAIL, 16), name: "rail", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (2, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT), (1, 1, STICK), (2, 1, ITEM_IRON_INGOT), (0, 2, ITEM_IRON_INGOT), (2, 2, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_RAISER_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_TERRACOTTA, 1)], result: (ITEM_RAISER_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "raiser_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_RAISER_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_TERRACOTTA), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_RAW_COPPER_BLOCK, 1)], result: (ITEM_RAW_COPPER, 9), name: "raw_copper", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RAW_COPPER, 9)], result: (ITEM_RAW_COPPER_BLOCK, 1), name: "raw_copper_block", shaped: true, cells: &[(0, 0, ITEM_RAW_COPPER), (1, 0, ITEM_RAW_COPPER), (2, 0, ITEM_RAW_COPPER), (0, 1, ITEM_RAW_COPPER), (1, 1, ITEM_RAW_COPPER), (2, 1, ITEM_RAW_COPPER), (0, 2, ITEM_RAW_COPPER), (1, 2, ITEM_RAW_COPPER), (2, 2, ITEM_RAW_COPPER)] },
    Recipe { needs: &[(ITEM_RAW_GOLD_BLOCK, 1)], result: (ITEM_RAW_GOLD, 9), name: "raw_gold", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RAW_GOLD, 9)], result: (ITEM_RAW_GOLD_BLOCK, 1), name: "raw_gold_block", shaped: true, cells: &[(0, 0, ITEM_RAW_GOLD), (1, 0, ITEM_RAW_GOLD), (2, 0, ITEM_RAW_GOLD), (0, 1, ITEM_RAW_GOLD), (1, 1, ITEM_RAW_GOLD), (2, 1, ITEM_RAW_GOLD), (0, 2, ITEM_RAW_GOLD), (1, 2, ITEM_RAW_GOLD), (2, 2, ITEM_RAW_GOLD)] },
    Recipe { needs: &[(ITEM_RAW_IRON_BLOCK, 1)], result: (ITEM_RAW_IRON, 9), name: "raw_iron", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RAW_IRON, 9)], result: (ITEM_RAW_IRON_BLOCK, 1), name: "raw_iron_block", shaped: true, cells: &[(0, 0, ITEM_RAW_IRON), (1, 0, ITEM_RAW_IRON), (2, 0, ITEM_RAW_IRON), (0, 1, ITEM_RAW_IRON), (1, 1, ITEM_RAW_IRON), (2, 1, ITEM_RAW_IRON), (0, 2, ITEM_RAW_IRON), (1, 2, ITEM_RAW_IRON), (2, 2, ITEM_RAW_IRON)] },
    Recipe { needs: &[(ITEM_COMPASS, 1), (ITEM_ECHO_SHARD, 8)], result: (ITEM_RECOVERY_COMPASS, 1), name: "recovery_compass", shaped: true, cells: &[(0, 0, ITEM_ECHO_SHARD), (1, 0, ITEM_ECHO_SHARD), (2, 0, ITEM_ECHO_SHARD), (0, 1, ITEM_ECHO_SHARD), (1, 1, ITEM_COMPASS), (2, 1, ITEM_ECHO_SHARD), (0, 2, ITEM_ECHO_SHARD), (1, 2, ITEM_ECHO_SHARD), (2, 2, ITEM_ECHO_SHARD)] },
    Recipe { needs: &[(ITEM_RED_WOOL, 6), (STICK, 1)], result: (ITEM_RED_BANNER, 1), name: "red_banner", shaped: true, cells: &[(0, 0, ITEM_RED_WOOL), (1, 0, ITEM_RED_WOOL), (2, 0, ITEM_RED_WOOL), (0, 1, ITEM_RED_WOOL), (1, 1, ITEM_RED_WOOL), (2, 1, ITEM_RED_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_RED_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_RED_BED, 1), name: "red_bed", shaped: true, cells: &[(0, 0, ITEM_RED_WOOL), (1, 0, ITEM_RED_WOOL), (2, 0, ITEM_RED_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_RED_DYE, 1)], result: (ITEM_RED_CANDLE, 1), name: "red_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_WOOL, 2)], result: (ITEM_RED_CARPET, 3), name: "red_carpet", shaped: true, cells: &[(0, 0, ITEM_RED_WOOL), (1, 0, ITEM_RED_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_RED_DYE, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1)], result: (ITEM_RED_CONCRETE_POWDER, 8), name: "red_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_CONCRETE, 3)], result: (ITEM_RED_CONCRETE_SLAB, 6), name: "red_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_RED_CONCRETE), (1, 0, ITEM_RED_CONCRETE), (2, 0, ITEM_RED_CONCRETE)] },
    Recipe { needs: &[(ITEM_RED_CONCRETE, 6)], result: (ITEM_RED_CONCRETE_STAIRS, 4), name: "red_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_RED_CONCRETE), (0, 1, ITEM_RED_CONCRETE), (1, 1, ITEM_RED_CONCRETE), (0, 2, ITEM_RED_CONCRETE), (1, 2, ITEM_RED_CONCRETE), (2, 2, ITEM_RED_CONCRETE)] },
    Recipe { needs: &[(ITEM_RED_WOOL_SLAB, 3)], result: (ITEM_RED_CUSHION, 1), name: "red_cushion", shaped: true, cells: &[(0, 0, ITEM_RED_WOOL_SLAB), (1, 0, ITEM_RED_WOOL_SLAB), (2, 0, ITEM_RED_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_BEETROOT, 1)], result: (ITEM_RED_DYE, 1), name: "red_dye_from_beetroot", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_POPPY, 1)], result: (ITEM_RED_DYE, 1), name: "red_dye_from_poppy", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ROSE_BUSH, 1)], result: (ITEM_RED_DYE, 2), name: "red_dye_from_rose_bush", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_RED_TULIP, 1)], result: (ITEM_RED_DYE, 1), name: "red_dye_from_tulip", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_RED_WOOL, 1)], result: (ITEM_RED_HARNESS, 1), name: "red_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_RED_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_RED_NETHER_BRICKS, 3)], result: (ITEM_RED_NETHER_BRICK_SLAB, 6), name: "red_nether_brick_slab", shaped: true, cells: &[(0, 0, ITEM_RED_NETHER_BRICKS), (1, 0, ITEM_RED_NETHER_BRICKS), (2, 0, ITEM_RED_NETHER_BRICKS)] },
    Recipe { needs: &[(ITEM_RED_NETHER_BRICKS, 6)], result: (ITEM_RED_NETHER_BRICK_STAIRS, 4), name: "red_nether_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_RED_NETHER_BRICKS), (0, 1, ITEM_RED_NETHER_BRICKS), (1, 1, ITEM_RED_NETHER_BRICKS), (0, 2, ITEM_RED_NETHER_BRICKS), (1, 2, ITEM_RED_NETHER_BRICKS), (2, 2, ITEM_RED_NETHER_BRICKS)] },
    Recipe { needs: &[(ITEM_RED_NETHER_BRICKS, 6)], result: (ITEM_RED_NETHER_BRICK_WALL, 6), name: "red_nether_brick_wall", shaped: true, cells: &[(0, 0, ITEM_RED_NETHER_BRICKS), (1, 0, ITEM_RED_NETHER_BRICKS), (2, 0, ITEM_RED_NETHER_BRICKS), (0, 1, ITEM_RED_NETHER_BRICKS), (1, 1, ITEM_RED_NETHER_BRICKS), (2, 1, ITEM_RED_NETHER_BRICKS)] },
    Recipe { needs: &[(ITEM_NETHER_BRICK, 2), (ITEM_NETHER_WART, 2)], result: (ITEM_RED_NETHER_BRICKS, 1), name: "red_nether_bricks", shaped: true, cells: &[(0, 0, ITEM_NETHER_BRICK), (1, 0, ITEM_NETHER_WART), (0, 1, ITEM_NETHER_WART), (1, 1, ITEM_NETHER_BRICK)] },
    Recipe { needs: &[(ITEM_RED_SAND, 4)], result: (ITEM_RED_SANDSTONE, 1), name: "red_sandstone", shaped: true, cells: &[(0, 0, ITEM_RED_SAND), (1, 0, ITEM_RED_SAND), (0, 1, ITEM_RED_SAND), (1, 1, ITEM_RED_SAND)] },
    Recipe { needs: &[(TAG_ALT_116, 3)], result: (ITEM_RED_SANDSTONE_SLAB, 6), name: "red_sandstone_slab", shaped: true, cells: &[(0, 0, TAG_ALT_116), (1, 0, TAG_ALT_116), (2, 0, TAG_ALT_116)] },
    Recipe { needs: &[(TAG_ALT_117, 6)], result: (ITEM_RED_SANDSTONE_STAIRS, 4), name: "red_sandstone_stairs", shaped: true, cells: &[(0, 0, TAG_ALT_117), (0, 1, TAG_ALT_117), (1, 1, TAG_ALT_117), (0, 2, TAG_ALT_117), (1, 2, TAG_ALT_117), (2, 2, TAG_ALT_117)] },
    Recipe { needs: &[(ITEM_RED_SANDSTONE, 6)], result: (ITEM_RED_SANDSTONE_WALL, 6), name: "red_sandstone_wall", shaped: true, cells: &[(0, 0, ITEM_RED_SANDSTONE), (1, 0, ITEM_RED_SANDSTONE), (2, 0, ITEM_RED_SANDSTONE), (0, 1, ITEM_RED_SANDSTONE), (1, 1, ITEM_RED_SANDSTONE), (2, 1, ITEM_RED_SANDSTONE)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_RED_DYE, 1)], result: (ITEM_RED_STAINED_GLASS, 8), name: "red_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_RED_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_RED_STAINED_GLASS, 6)], result: (ITEM_RED_STAINED_GLASS_PANE, 16), name: "red_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_RED_STAINED_GLASS), (1, 0, ITEM_RED_STAINED_GLASS), (2, 0, ITEM_RED_STAINED_GLASS), (0, 1, ITEM_RED_STAINED_GLASS), (1, 1, ITEM_RED_STAINED_GLASS), (2, 1, ITEM_RED_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_RED_DYE, 1)], result: (ITEM_RED_STAINED_GLASS_PANE, 8), name: "red_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_RED_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_RED_DYE, 1), (ITEM_TERRACOTTA, 8)], result: (ITEM_RED_TERRACOTTA, 8), name: "red_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_RED_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_RED_WOOL, 3)], result: (ITEM_RED_WOOL_SLAB, 6), name: "red_wool_slab", shaped: true, cells: &[(0, 0, ITEM_RED_WOOL), (1, 0, ITEM_RED_WOOL), (2, 0, ITEM_RED_WOOL)] },
    Recipe { needs: &[(ITEM_RED_WOOL, 6)], result: (ITEM_RED_WOOL_STAIRS, 4), name: "red_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_RED_WOOL), (0, 1, ITEM_RED_WOOL), (1, 1, ITEM_RED_WOOL), (0, 2, ITEM_RED_WOOL), (1, 2, ITEM_RED_WOOL), (2, 2, ITEM_RED_WOOL)] },
    Recipe { needs: &[(ITEM_REDSTONE_BLOCK, 1)], result: (ITEM_REDSTONE, 9), name: "redstone", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_REDSTONE, 9)], result: (ITEM_REDSTONE_BLOCK, 1), name: "redstone_block", shaped: true, cells: &[(0, 0, ITEM_REDSTONE), (1, 0, ITEM_REDSTONE), (2, 0, ITEM_REDSTONE), (0, 1, ITEM_REDSTONE), (1, 1, ITEM_REDSTONE), (2, 1, ITEM_REDSTONE), (0, 2, ITEM_REDSTONE), (1, 2, ITEM_REDSTONE), (2, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_GLOWSTONE, 1), (ITEM_REDSTONE, 4)], result: (ITEM_REDSTONE_LAMP, 1), name: "redstone_lamp", shaped: true, cells: &[(1, 0, ITEM_REDSTONE), (0, 1, ITEM_REDSTONE), (1, 1, ITEM_GLOWSTONE), (2, 1, ITEM_REDSTONE), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_REDSTONE, 1), (STICK, 1)], result: (ITEM_REDSTONE_TORCH, 1), name: "redstone_torch", shaped: true, cells: &[(0, 0, ITEM_REDSTONE), (0, 1, STICK)] },
    Recipe { needs: &[(ITEM_REDSTONE, 1), (ITEM_REDSTONE_TORCH, 2), (ITEM_STONE, 3)], result: (ITEM_REPEATER, 1), name: "repeater", shaped: true, cells: &[(0, 0, ITEM_REDSTONE_TORCH), (1, 0, ITEM_REDSTONE), (2, 0, ITEM_REDSTONE_TORCH), (0, 1, ITEM_STONE), (1, 1, ITEM_STONE), (2, 1, ITEM_STONE)] },
    Recipe { needs: &[(ITEM_RESIN_CLUMP, 9)], result: (ITEM_RESIN_BLOCK, 1), name: "resin_block", shaped: true, cells: &[(0, 0, ITEM_RESIN_CLUMP), (1, 0, ITEM_RESIN_CLUMP), (2, 0, ITEM_RESIN_CLUMP), (0, 1, ITEM_RESIN_CLUMP), (1, 1, ITEM_RESIN_CLUMP), (2, 1, ITEM_RESIN_CLUMP), (0, 2, ITEM_RESIN_CLUMP), (1, 2, ITEM_RESIN_CLUMP), (2, 2, ITEM_RESIN_CLUMP)] },
    Recipe { needs: &[(ITEM_RESIN_BRICKS, 3)], result: (ITEM_RESIN_BRICK_SLAB, 6), name: "resin_brick_slab", shaped: true, cells: &[(0, 0, ITEM_RESIN_BRICKS), (1, 0, ITEM_RESIN_BRICKS), (2, 0, ITEM_RESIN_BRICKS)] },
    Recipe { needs: &[(ITEM_RESIN_BRICKS, 6)], result: (ITEM_RESIN_BRICK_STAIRS, 4), name: "resin_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_RESIN_BRICKS), (0, 1, ITEM_RESIN_BRICKS), (1, 1, ITEM_RESIN_BRICKS), (0, 2, ITEM_RESIN_BRICKS), (1, 2, ITEM_RESIN_BRICKS), (2, 2, ITEM_RESIN_BRICKS)] },
    Recipe { needs: &[(ITEM_RESIN_BRICKS, 6)], result: (ITEM_RESIN_BRICK_WALL, 6), name: "resin_brick_wall", shaped: true, cells: &[(0, 0, ITEM_RESIN_BRICKS), (1, 0, ITEM_RESIN_BRICKS), (2, 0, ITEM_RESIN_BRICKS), (0, 1, ITEM_RESIN_BRICKS), (1, 1, ITEM_RESIN_BRICKS), (2, 1, ITEM_RESIN_BRICKS)] },
    Recipe { needs: &[(ITEM_RESIN_BRICK, 4)], result: (ITEM_RESIN_BRICKS, 1), name: "resin_bricks", shaped: true, cells: &[(0, 0, ITEM_RESIN_BRICK), (1, 0, ITEM_RESIN_BRICK), (0, 1, ITEM_RESIN_BRICK), (1, 1, ITEM_RESIN_BRICK)] },
    Recipe { needs: &[(ITEM_RESIN_BLOCK, 1)], result: (ITEM_RESIN_CLUMP, 9), name: "resin_clump", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CRYING_OBSIDIAN, 6), (ITEM_GLOWSTONE, 3)], result: (ITEM_RESPAWN_ANCHOR, 1), name: "respawn_anchor", shaped: true, cells: &[(0, 0, ITEM_CRYING_OBSIDIAN), (1, 0, ITEM_CRYING_OBSIDIAN), (2, 0, ITEM_CRYING_OBSIDIAN), (0, 1, ITEM_GLOWSTONE), (1, 1, ITEM_GLOWSTONE), (2, 1, ITEM_GLOWSTONE), (0, 2, ITEM_CRYING_OBSIDIAN), (1, 2, ITEM_CRYING_OBSIDIAN), (2, 2, ITEM_CRYING_OBSIDIAN)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_NETHERRACK, 1), (ITEM_RIB_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_RIB_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "rib_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_RIB_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_NETHERRACK), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (ITEM_LEATHER, 3)], result: (ITEM_SADDLE, 1), name: "saddle", shaped: true, cells: &[(1, 0, ITEM_LEATHER), (0, 1, ITEM_LEATHER), (1, 1, ITEM_IRON_INGOT), (2, 1, ITEM_LEATHER)] },
    Recipe { needs: &[(ITEM_SAND, 4)], result: (ITEM_SANDSTONE, 1), name: "sandstone", shaped: true, cells: &[(0, 0, ITEM_SAND), (1, 0, ITEM_SAND), (0, 1, ITEM_SAND), (1, 1, ITEM_SAND)] },
    Recipe { needs: &[(TAG_ALT_118, 3)], result: (ITEM_SANDSTONE_SLAB, 6), name: "sandstone_slab", shaped: true, cells: &[(0, 0, TAG_ALT_118), (1, 0, TAG_ALT_118), (2, 0, TAG_ALT_118)] },
    Recipe { needs: &[(TAG_ALT_119, 6)], result: (ITEM_SANDSTONE_STAIRS, 4), name: "sandstone_stairs", shaped: true, cells: &[(0, 0, TAG_ALT_119), (0, 1, TAG_ALT_119), (1, 1, TAG_ALT_119), (0, 2, TAG_ALT_119), (1, 2, TAG_ALT_119), (2, 2, TAG_ALT_119)] },
    Recipe { needs: &[(ITEM_SANDSTONE, 6)], result: (ITEM_SANDSTONE_WALL, 6), name: "sandstone_wall", shaped: true, cells: &[(0, 0, ITEM_SANDSTONE), (1, 0, ITEM_SANDSTONE), (2, 0, ITEM_SANDSTONE), (0, 1, ITEM_SANDSTONE), (1, 1, ITEM_SANDSTONE), (2, 1, ITEM_SANDSTONE)] },
    Recipe { needs: &[(ITEM_BAMBOO, 6), (ITEM_STRING, 1)], result: (ITEM_SCAFFOLDING, 6), name: "scaffolding", shaped: true, cells: &[(0, 0, ITEM_BAMBOO), (1, 0, ITEM_STRING), (2, 0, ITEM_BAMBOO), (0, 1, ITEM_BAMBOO), (2, 1, ITEM_BAMBOO), (0, 2, ITEM_BAMBOO), (2, 2, ITEM_BAMBOO)] },
    Recipe { needs: &[(ITEM_PRISMARINE_CRYSTALS, 5), (ITEM_PRISMARINE_SHARD, 4)], result: (ITEM_SEA_LANTERN, 1), name: "sea_lantern", shaped: true, cells: &[(0, 0, ITEM_PRISMARINE_SHARD), (1, 0, ITEM_PRISMARINE_CRYSTALS), (2, 0, ITEM_PRISMARINE_SHARD), (0, 1, ITEM_PRISMARINE_CRYSTALS), (1, 1, ITEM_PRISMARINE_CRYSTALS), (2, 1, ITEM_PRISMARINE_CRYSTALS), (0, 2, ITEM_PRISMARINE_SHARD), (1, 2, ITEM_PRISMARINE_CRYSTALS), (2, 2, ITEM_PRISMARINE_SHARD)] },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_DIAMOND, 7), (ITEM_SENTRY_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_SENTRY_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "sentry_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_SENTRY_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, COBBLESTONE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_SHAPER_ARMOR_TRIM_SMITHING_TEMPLATE, 1), (ITEM_TERRACOTTA, 1)], result: (ITEM_SHAPER_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "shaper_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_SHAPER_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_TERRACOTTA), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 2)], result: (ITEM_SHEARS, 1), name: "shears", shaped: true, cells: &[(1, 0, ITEM_IRON_INGOT), (0, 1, ITEM_IRON_INGOT)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (TAG_PLANKS, 6)], result: (ITEM_SHIELD, 1), name: "shield", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, ITEM_IRON_INGOT), (2, 0, TAG_PLANKS), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS), (1, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_SHULKER_SHELL, 2)], result: (ITEM_SHULKER_BOX, 1), name: "shulker_box", shaped: true, cells: &[(0, 0, ITEM_SHULKER_SHELL), (0, 1, ITEM_CHEST), (0, 2, ITEM_SHULKER_SHELL)] },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 1), (ITEM_DIAMOND, 7), (ITEM_SILENCE_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_SILENCE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "silence_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_SILENCE_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_COBBLED_DEEPSLATE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_PAPER, 1), (ITEM_WITHER_SKELETON_SKULL, 1)], result: (ITEM_SKULL_BANNER_PATTERN, 1), name: "skull_banner_pattern", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_SLIME_BLOCK, 1)], result: (ITEM_SLIME_BALL, 9), name: "slime_ball", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_SLIME_BALL, 9)], result: (ITEM_SLIME_BLOCK, 1), name: "slime_block", shaped: true, cells: &[(0, 0, ITEM_SLIME_BALL), (1, 0, ITEM_SLIME_BALL), (2, 0, ITEM_SLIME_BALL), (0, 1, ITEM_SLIME_BALL), (1, 1, ITEM_SLIME_BALL), (2, 1, ITEM_SLIME_BALL), (0, 2, ITEM_SLIME_BALL), (1, 2, ITEM_SLIME_BALL), (2, 2, ITEM_SLIME_BALL)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 2), (TAG_PLANKS, 4)], result: (ITEM_SMITHING_TABLE, 1), name: "smithing_table", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (1, 0, ITEM_IRON_INGOT), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (0, 2, TAG_PLANKS), (1, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_FURNACE, 1), (TAG_LOGS, 4)], result: (ITEM_SMOKER, 1), name: "smoker", shaped: true, cells: &[(1, 0, TAG_LOGS), (0, 1, TAG_LOGS), (1, 1, ITEM_FURNACE), (2, 1, TAG_LOGS), (1, 2, TAG_LOGS)] },
    Recipe { needs: &[(ITEM_SMOOTH_QUARTZ, 3)], result: (ITEM_SMOOTH_QUARTZ_SLAB, 6), name: "smooth_quartz_slab", shaped: true, cells: &[(0, 0, ITEM_SMOOTH_QUARTZ), (1, 0, ITEM_SMOOTH_QUARTZ), (2, 0, ITEM_SMOOTH_QUARTZ)] },
    Recipe { needs: &[(ITEM_SMOOTH_QUARTZ, 6)], result: (ITEM_SMOOTH_QUARTZ_STAIRS, 4), name: "smooth_quartz_stairs", shaped: true, cells: &[(0, 0, ITEM_SMOOTH_QUARTZ), (0, 1, ITEM_SMOOTH_QUARTZ), (1, 1, ITEM_SMOOTH_QUARTZ), (0, 2, ITEM_SMOOTH_QUARTZ), (1, 2, ITEM_SMOOTH_QUARTZ), (2, 2, ITEM_SMOOTH_QUARTZ)] },
    Recipe { needs: &[(ITEM_SMOOTH_RED_SANDSTONE, 3)], result: (ITEM_SMOOTH_RED_SANDSTONE_SLAB, 6), name: "smooth_red_sandstone_slab", shaped: true, cells: &[(0, 0, ITEM_SMOOTH_RED_SANDSTONE), (1, 0, ITEM_SMOOTH_RED_SANDSTONE), (2, 0, ITEM_SMOOTH_RED_SANDSTONE)] },
    Recipe { needs: &[(ITEM_SMOOTH_RED_SANDSTONE, 6)], result: (ITEM_SMOOTH_RED_SANDSTONE_STAIRS, 4), name: "smooth_red_sandstone_stairs", shaped: true, cells: &[(0, 0, ITEM_SMOOTH_RED_SANDSTONE), (0, 1, ITEM_SMOOTH_RED_SANDSTONE), (1, 1, ITEM_SMOOTH_RED_SANDSTONE), (0, 2, ITEM_SMOOTH_RED_SANDSTONE), (1, 2, ITEM_SMOOTH_RED_SANDSTONE), (2, 2, ITEM_SMOOTH_RED_SANDSTONE)] },
    Recipe { needs: &[(ITEM_SMOOTH_SANDSTONE, 3)], result: (ITEM_SMOOTH_SANDSTONE_SLAB, 6), name: "smooth_sandstone_slab", shaped: true, cells: &[(0, 0, ITEM_SMOOTH_SANDSTONE), (1, 0, ITEM_SMOOTH_SANDSTONE), (2, 0, ITEM_SMOOTH_SANDSTONE)] },
    Recipe { needs: &[(ITEM_SMOOTH_SANDSTONE, 6)], result: (ITEM_SMOOTH_SANDSTONE_STAIRS, 4), name: "smooth_sandstone_stairs", shaped: true, cells: &[(0, 0, ITEM_SMOOTH_SANDSTONE), (0, 1, ITEM_SMOOTH_SANDSTONE), (1, 1, ITEM_SMOOTH_SANDSTONE), (0, 2, ITEM_SMOOTH_SANDSTONE), (1, 2, ITEM_SMOOTH_SANDSTONE), (2, 2, ITEM_SMOOTH_SANDSTONE)] },
    Recipe { needs: &[(ITEM_SMOOTH_STONE, 3)], result: (ITEM_SMOOTH_STONE_SLAB, 6), name: "smooth_stone_slab", shaped: true, cells: &[(0, 0, ITEM_SMOOTH_STONE), (1, 0, ITEM_SMOOTH_STONE), (2, 0, ITEM_SMOOTH_STONE)] },
    Recipe { needs: &[(ITEM_BLACKSTONE, 1), (ITEM_DIAMOND, 7), (ITEM_SNOUT_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_SNOUT_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "snout_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_SNOUT_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_BLACKSTONE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_SNOW_BLOCK, 3)], result: (ITEM_SNOW, 6), name: "snow", shaped: true, cells: &[(0, 0, ITEM_SNOW_BLOCK), (1, 0, ITEM_SNOW_BLOCK), (2, 0, ITEM_SNOW_BLOCK)] },
    Recipe { needs: &[(ITEM_SNOWBALL, 4)], result: (ITEM_SNOW_BLOCK, 1), name: "snow_block", shaped: true, cells: &[(0, 0, ITEM_SNOWBALL), (1, 0, ITEM_SNOWBALL), (0, 1, ITEM_SNOWBALL), (1, 1, ITEM_SNOWBALL)] },
    Recipe { needs: &[(STICK, 3), (TAG_LOGS, 3), (TAG_SOUL_FIRE_BASE_BLOCKS, 1)], result: (ITEM_SOUL_CAMPFIRE, 1), name: "soul_campfire", shaped: true, cells: &[(1, 0, STICK), (0, 1, STICK), (1, 1, TAG_SOUL_FIRE_BASE_BLOCKS), (2, 1, STICK), (0, 2, TAG_LOGS), (1, 2, TAG_LOGS), (2, 2, TAG_LOGS)] },
    Recipe { needs: &[(ITEM_IRON_NUGGET, 8), (ITEM_SOUL_TORCH, 1)], result: (ITEM_SOUL_LANTERN, 1), name: "soul_lantern", shaped: true, cells: &[(0, 0, ITEM_IRON_NUGGET), (1, 0, ITEM_IRON_NUGGET), (2, 0, ITEM_IRON_NUGGET), (0, 1, ITEM_IRON_NUGGET), (1, 1, ITEM_SOUL_TORCH), (2, 1, ITEM_IRON_NUGGET), (0, 2, ITEM_IRON_NUGGET), (1, 2, ITEM_IRON_NUGGET), (2, 2, ITEM_IRON_NUGGET)] },
    Recipe { needs: &[(STICK, 1), (TAG_COALS, 1), (TAG_SOUL_FIRE_BASE_BLOCKS, 1)], result: (ITEM_SOUL_TORCH, 4), name: "soul_torch", shaped: true, cells: &[(0, 0, TAG_COALS), (0, 1, STICK), (0, 2, TAG_SOUL_FIRE_BASE_BLOCKS)] },
    Recipe { needs: &[(ITEM_ARROW, 1), (ITEM_GLOWSTONE_DUST, 4)], result: (ITEM_SPECTRAL_ARROW, 2), name: "spectral_arrow", shaped: true, cells: &[(1, 0, ITEM_GLOWSTONE_DUST), (0, 1, ITEM_GLOWSTONE_DUST), (1, 1, ITEM_ARROW), (2, 1, ITEM_GLOWSTONE_DUST), (1, 2, ITEM_GLOWSTONE_DUST)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_PURPUR_BLOCK, 1), (ITEM_SPIRE_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_SPIRE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "spire_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_SPIRE_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_PURPUR_BLOCK), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 5)], result: (ITEM_SPRUCE_BOAT, 1), name: "spruce_boat", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_PLANKS), (2, 0, ITEM_SPRUCE_PLANKS), (0, 1, ITEM_SPRUCE_PLANKS), (1, 1, ITEM_SPRUCE_PLANKS), (2, 1, ITEM_SPRUCE_PLANKS)] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 1)], result: (ITEM_SPRUCE_BUTTON, 1), name: "spruce_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_SPRUCE_BOAT, 1)], result: (ITEM_SPRUCE_CHEST_BOAT, 1), name: "spruce_chest_boat", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6)], result: (ITEM_SPRUCE_DOOR, 3), name: "spruce_door", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_PLANKS), (1, 0, ITEM_SPRUCE_PLANKS), (0, 1, ITEM_SPRUCE_PLANKS), (1, 1, ITEM_SPRUCE_PLANKS), (0, 2, ITEM_SPRUCE_PLANKS), (1, 2, ITEM_SPRUCE_PLANKS)] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 4), (STICK, 2)], result: (ITEM_SPRUCE_FENCE, 3), name: "spruce_fence", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_PLANKS), (1, 0, STICK), (2, 0, ITEM_SPRUCE_PLANKS), (0, 1, ITEM_SPRUCE_PLANKS), (1, 1, STICK), (2, 1, ITEM_SPRUCE_PLANKS)] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 2), (STICK, 4)], result: (ITEM_SPRUCE_FENCE_GATE, 1), name: "spruce_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_SPRUCE_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_SPRUCE_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_SPRUCE_LOG, 6)], result: (ITEM_SPRUCE_HANGING_SIGN, 6), name: "spruce_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_SPRUCE_LOG), (1, 1, ITEM_STRIPPED_SPRUCE_LOG), (2, 1, ITEM_STRIPPED_SPRUCE_LOG), (0, 2, ITEM_STRIPPED_SPRUCE_LOG), (1, 2, ITEM_STRIPPED_SPRUCE_LOG), (2, 2, ITEM_STRIPPED_SPRUCE_LOG)] },
    Recipe { needs: &[(TAG_SPRUCE_LOGS, 1)], result: (ITEM_SPRUCE_PLANKS, 4), name: "spruce_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 2)], result: (ITEM_SPRUCE_PRESSURE_PLATE, 1), name: "spruce_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_PLANKS), (1, 0, ITEM_SPRUCE_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_SPRUCE_LOG, 6)], result: (ITEM_SPRUCE_SHELF, 6), name: "spruce_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_SPRUCE_LOG), (1, 0, ITEM_STRIPPED_SPRUCE_LOG), (2, 0, ITEM_STRIPPED_SPRUCE_LOG), (0, 2, ITEM_STRIPPED_SPRUCE_LOG), (1, 2, ITEM_STRIPPED_SPRUCE_LOG), (2, 2, ITEM_STRIPPED_SPRUCE_LOG)] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6), (STICK, 1)], result: (ITEM_SPRUCE_SIGN, 3), name: "spruce_sign", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_PLANKS), (1, 0, ITEM_SPRUCE_PLANKS), (2, 0, ITEM_SPRUCE_PLANKS), (0, 1, ITEM_SPRUCE_PLANKS), (1, 1, ITEM_SPRUCE_PLANKS), (2, 1, ITEM_SPRUCE_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 3)], result: (ITEM_SPRUCE_SLAB, 6), name: "spruce_slab", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_PLANKS), (1, 0, ITEM_SPRUCE_PLANKS), (2, 0, ITEM_SPRUCE_PLANKS)] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6)], result: (ITEM_SPRUCE_STAIRS, 4), name: "spruce_stairs", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_PLANKS), (0, 1, ITEM_SPRUCE_PLANKS), (1, 1, ITEM_SPRUCE_PLANKS), (0, 2, ITEM_SPRUCE_PLANKS), (1, 2, ITEM_SPRUCE_PLANKS), (2, 2, ITEM_SPRUCE_PLANKS)] },
    Recipe { needs: &[(ITEM_SPRUCE_PLANKS, 6)], result: (ITEM_SPRUCE_TRAPDOOR, 2), name: "spruce_trapdoor", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_PLANKS), (1, 0, ITEM_SPRUCE_PLANKS), (2, 0, ITEM_SPRUCE_PLANKS), (0, 1, ITEM_SPRUCE_PLANKS), (1, 1, ITEM_SPRUCE_PLANKS), (2, 1, ITEM_SPRUCE_PLANKS)] },
    Recipe { needs: &[(ITEM_SPRUCE_LOG, 4)], result: (ITEM_SPRUCE_WOOD, 3), name: "spruce_wood", shaped: true, cells: &[(0, 0, ITEM_SPRUCE_LOG), (1, 0, ITEM_SPRUCE_LOG), (0, 1, ITEM_SPRUCE_LOG), (1, 1, ITEM_SPRUCE_LOG)] },
    Recipe { needs: &[(ITEM_AMETHYST_SHARD, 1), (ITEM_COPPER_INGOT, 2)], result: (ITEM_SPYGLASS, 1), name: "spyglass", shaped: true, cells: &[(0, 0, ITEM_AMETHYST_SHARD), (0, 1, ITEM_COPPER_INGOT), (0, 2, ITEM_COPPER_INGOT)] },
    Recipe { needs: &[(TAG_PLANKS, 2)], result: (STICK, 4), name: "stick", shaped: true, cells: &[(0, 0, TAG_PLANKS), (0, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_BAMBOO, 2)], result: (STICK, 1), name: "stick_from_bamboo_item", shaped: true, cells: &[(0, 0, ITEM_BAMBOO), (0, 1, ITEM_BAMBOO)] },
    Recipe { needs: &[(ITEM_PISTON, 1), (ITEM_SLIME_BALL, 1)], result: (ITEM_STICKY_PISTON, 1), name: "sticky_piston", shaped: true, cells: &[(0, 0, ITEM_SLIME_BALL), (0, 1, ITEM_PISTON)] },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 3)], result: (ITEM_STONE_AXE, 1), name: "stone_axe", shaped: true, cells: &[(0, 0, TAG_STONE_CRAFTING_MATERIALS), (1, 0, TAG_STONE_CRAFTING_MATERIALS), (0, 1, TAG_STONE_CRAFTING_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_STONE_BRICKS, 3)], result: (ITEM_STONE_BRICK_SLAB, 6), name: "stone_brick_slab", shaped: true, cells: &[(0, 0, ITEM_STONE_BRICKS), (1, 0, ITEM_STONE_BRICKS), (2, 0, ITEM_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_STONE_BRICKS, 6)], result: (ITEM_STONE_BRICK_STAIRS, 4), name: "stone_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_STONE_BRICKS), (0, 1, ITEM_STONE_BRICKS), (1, 1, ITEM_STONE_BRICKS), (0, 2, ITEM_STONE_BRICKS), (1, 2, ITEM_STONE_BRICKS), (2, 2, ITEM_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_STONE_BRICKS, 6)], result: (ITEM_STONE_BRICK_WALL, 6), name: "stone_brick_wall", shaped: true, cells: &[(0, 0, ITEM_STONE_BRICKS), (1, 0, ITEM_STONE_BRICKS), (2, 0, ITEM_STONE_BRICKS), (0, 1, ITEM_STONE_BRICKS), (1, 1, ITEM_STONE_BRICKS), (2, 1, ITEM_STONE_BRICKS)] },
    Recipe { needs: &[(ITEM_STONE, 4)], result: (ITEM_STONE_BRICKS, 4), name: "stone_bricks", shaped: true, cells: &[(0, 0, ITEM_STONE), (1, 0, ITEM_STONE), (0, 1, ITEM_STONE), (1, 1, ITEM_STONE)] },
    Recipe { needs: &[(ITEM_STONE, 1)], result: (ITEM_STONE_BUTTON, 1), name: "stone_button", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 2)], result: (ITEM_STONE_HOE, 1), name: "stone_hoe", shaped: true, cells: &[(0, 0, TAG_STONE_CRAFTING_MATERIALS), (1, 0, TAG_STONE_CRAFTING_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 3)], result: (STONE_PICKAXE, 1), name: "stone_pickaxe", shaped: true, cells: &[(0, 0, TAG_STONE_CRAFTING_MATERIALS), (1, 0, TAG_STONE_CRAFTING_MATERIALS), (2, 0, TAG_STONE_CRAFTING_MATERIALS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_STONE, 2)], result: (ITEM_STONE_PRESSURE_PLATE, 1), name: "stone_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_STONE), (1, 0, ITEM_STONE)] },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 1)], result: (ITEM_STONE_SHOVEL, 1), name: "stone_shovel", shaped: true, cells: &[(0, 0, TAG_STONE_CRAFTING_MATERIALS), (0, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_STONE, 3)], result: (ITEM_STONE_SLAB, 6), name: "stone_slab", shaped: true, cells: &[(0, 0, ITEM_STONE), (1, 0, ITEM_STONE), (2, 0, ITEM_STONE)] },
    Recipe { needs: &[(STICK, 2), (TAG_STONE_CRAFTING_MATERIALS, 1)], result: (ITEM_STONE_SPEAR, 1), name: "stone_spear", shaped: true, cells: &[(2, 0, TAG_STONE_CRAFTING_MATERIALS), (1, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_STONE, 6)], result: (ITEM_STONE_STAIRS, 4), name: "stone_stairs", shaped: true, cells: &[(0, 0, ITEM_STONE), (0, 1, ITEM_STONE), (1, 1, ITEM_STONE), (0, 2, ITEM_STONE), (1, 2, ITEM_STONE), (2, 2, ITEM_STONE)] },
    Recipe { needs: &[(STICK, 1), (TAG_STONE_CRAFTING_MATERIALS, 2)], result: (ITEM_STONE_SWORD, 1), name: "stone_sword", shaped: true, cells: &[(0, 0, TAG_STONE_CRAFTING_MATERIALS), (0, 1, TAG_STONE_CRAFTING_MATERIALS), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (ITEM_STONE, 3)], result: (ITEM_STONECUTTER, 1), name: "stonecutter", shaped: true, cells: &[(1, 0, ITEM_IRON_INGOT), (0, 1, ITEM_STONE), (1, 1, ITEM_STONE), (2, 1, ITEM_STONE)] },
    Recipe { needs: &[(ITEM_HAY_BLOCK, 3)], result: (ITEM_STRAW_BED, 4), name: "straw_bed", shaped: true, cells: &[(0, 0, ITEM_HAY_BLOCK), (1, 0, ITEM_HAY_BLOCK), (2, 0, ITEM_HAY_BLOCK)] },
    Recipe { needs: &[(ITEM_STRIPPED_ACACIA_LOG, 4)], result: (ITEM_STRIPPED_ACACIA_WOOD, 3), name: "stripped_acacia_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_ACACIA_LOG), (1, 0, ITEM_STRIPPED_ACACIA_LOG), (0, 1, ITEM_STRIPPED_ACACIA_LOG), (1, 1, ITEM_STRIPPED_ACACIA_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_BIRCH_LOG, 4)], result: (ITEM_STRIPPED_BIRCH_WOOD, 3), name: "stripped_birch_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_BIRCH_LOG), (1, 0, ITEM_STRIPPED_BIRCH_LOG), (0, 1, ITEM_STRIPPED_BIRCH_LOG), (1, 1, ITEM_STRIPPED_BIRCH_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_CHERRY_LOG, 4)], result: (ITEM_STRIPPED_CHERRY_WOOD, 3), name: "stripped_cherry_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_CHERRY_LOG), (1, 0, ITEM_STRIPPED_CHERRY_LOG), (0, 1, ITEM_STRIPPED_CHERRY_LOG), (1, 1, ITEM_STRIPPED_CHERRY_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_CRIMSON_STEM, 4)], result: (ITEM_STRIPPED_CRIMSON_HYPHAE, 3), name: "stripped_crimson_hyphae", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_CRIMSON_STEM), (1, 0, ITEM_STRIPPED_CRIMSON_STEM), (0, 1, ITEM_STRIPPED_CRIMSON_STEM), (1, 1, ITEM_STRIPPED_CRIMSON_STEM)] },
    Recipe { needs: &[(ITEM_STRIPPED_DARK_OAK_LOG, 4)], result: (ITEM_STRIPPED_DARK_OAK_WOOD, 3), name: "stripped_dark_oak_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_DARK_OAK_LOG), (1, 0, ITEM_STRIPPED_DARK_OAK_LOG), (0, 1, ITEM_STRIPPED_DARK_OAK_LOG), (1, 1, ITEM_STRIPPED_DARK_OAK_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_JUNGLE_LOG, 4)], result: (ITEM_STRIPPED_JUNGLE_WOOD, 3), name: "stripped_jungle_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_JUNGLE_LOG), (1, 0, ITEM_STRIPPED_JUNGLE_LOG), (0, 1, ITEM_STRIPPED_JUNGLE_LOG), (1, 1, ITEM_STRIPPED_JUNGLE_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_MANGROVE_LOG, 4)], result: (ITEM_STRIPPED_MANGROVE_WOOD, 3), name: "stripped_mangrove_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_MANGROVE_LOG), (1, 0, ITEM_STRIPPED_MANGROVE_LOG), (0, 1, ITEM_STRIPPED_MANGROVE_LOG), (1, 1, ITEM_STRIPPED_MANGROVE_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_OAK_LOG, 4)], result: (ITEM_STRIPPED_OAK_WOOD, 3), name: "stripped_oak_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_OAK_LOG), (1, 0, ITEM_STRIPPED_OAK_LOG), (0, 1, ITEM_STRIPPED_OAK_LOG), (1, 1, ITEM_STRIPPED_OAK_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_PALE_OAK_LOG, 4)], result: (ITEM_STRIPPED_PALE_OAK_WOOD, 3), name: "stripped_pale_oak_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_PALE_OAK_LOG), (1, 0, ITEM_STRIPPED_PALE_OAK_LOG), (0, 1, ITEM_STRIPPED_PALE_OAK_LOG), (1, 1, ITEM_STRIPPED_PALE_OAK_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_POPLAR_LOG, 4)], result: (ITEM_STRIPPED_POPLAR_WOOD, 3), name: "stripped_poplar_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_POPLAR_LOG), (1, 0, ITEM_STRIPPED_POPLAR_LOG), (0, 1, ITEM_STRIPPED_POPLAR_LOG), (1, 1, ITEM_STRIPPED_POPLAR_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_SPRUCE_LOG, 4)], result: (ITEM_STRIPPED_SPRUCE_WOOD, 3), name: "stripped_spruce_wood", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_SPRUCE_LOG), (1, 0, ITEM_STRIPPED_SPRUCE_LOG), (0, 1, ITEM_STRIPPED_SPRUCE_LOG), (1, 1, ITEM_STRIPPED_SPRUCE_LOG)] },
    Recipe { needs: &[(ITEM_STRIPPED_WARPED_STEM, 4)], result: (ITEM_STRIPPED_WARPED_HYPHAE, 3), name: "stripped_warped_hyphae", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_WARPED_STEM), (1, 0, ITEM_STRIPPED_WARPED_STEM), (0, 1, ITEM_STRIPPED_WARPED_STEM), (1, 1, ITEM_STRIPPED_WARPED_STEM)] },
    Recipe { needs: &[(ITEM_HONEY_BOTTLE, 1)], result: (ITEM_SUGAR, 3), name: "sugar_from_honey_bottle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_SUGAR_CANE, 1)], result: (ITEM_SUGAR, 1), name: "sugar_from_sugar_cane", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_SULFUR_BRICKS, 3)], result: (ITEM_SULFUR_BRICK_SLAB, 6), name: "sulfur_brick_slab", shaped: true, cells: &[(0, 0, ITEM_SULFUR_BRICKS), (1, 0, ITEM_SULFUR_BRICKS), (2, 0, ITEM_SULFUR_BRICKS)] },
    Recipe { needs: &[(ITEM_SULFUR_BRICKS, 6)], result: (ITEM_SULFUR_BRICK_STAIRS, 4), name: "sulfur_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_SULFUR_BRICKS), (0, 1, ITEM_SULFUR_BRICKS), (1, 1, ITEM_SULFUR_BRICKS), (0, 2, ITEM_SULFUR_BRICKS), (1, 2, ITEM_SULFUR_BRICKS), (2, 2, ITEM_SULFUR_BRICKS)] },
    Recipe { needs: &[(ITEM_SULFUR_BRICKS, 6)], result: (ITEM_SULFUR_BRICK_WALL, 6), name: "sulfur_brick_wall", shaped: true, cells: &[(0, 0, ITEM_SULFUR_BRICKS), (1, 0, ITEM_SULFUR_BRICKS), (2, 0, ITEM_SULFUR_BRICKS), (0, 1, ITEM_SULFUR_BRICKS), (1, 1, ITEM_SULFUR_BRICKS), (2, 1, ITEM_SULFUR_BRICKS)] },
    Recipe { needs: &[(ITEM_POLISHED_SULFUR, 4)], result: (ITEM_SULFUR_BRICKS, 4), name: "sulfur_bricks", shaped: true, cells: &[(0, 0, ITEM_POLISHED_SULFUR), (1, 0, ITEM_POLISHED_SULFUR), (0, 1, ITEM_POLISHED_SULFUR), (1, 1, ITEM_POLISHED_SULFUR)] },
    Recipe { needs: &[(ITEM_SULFUR_SPIKE, 4)], result: (ITEM_SULFUR, 1), name: "sulfur_from_sulfur_spikes", shaped: true, cells: &[(0, 0, ITEM_SULFUR_SPIKE), (1, 0, ITEM_SULFUR_SPIKE), (0, 1, ITEM_SULFUR_SPIKE), (1, 1, ITEM_SULFUR_SPIKE)] },
    Recipe { needs: &[(ITEM_SULFUR, 3)], result: (ITEM_SULFUR_SLAB, 6), name: "sulfur_slab", shaped: true, cells: &[(0, 0, ITEM_SULFUR), (1, 0, ITEM_SULFUR), (2, 0, ITEM_SULFUR)] },
    Recipe { needs: &[(ITEM_SULFUR, 6)], result: (ITEM_SULFUR_STAIRS, 4), name: "sulfur_stairs", shaped: true, cells: &[(0, 0, ITEM_SULFUR), (0, 1, ITEM_SULFUR), (1, 1, ITEM_SULFUR), (0, 2, ITEM_SULFUR), (1, 2, ITEM_SULFUR), (2, 2, ITEM_SULFUR)] },
    Recipe { needs: &[(ITEM_SULFUR, 6)], result: (ITEM_SULFUR_WALL, 6), name: "sulfur_wall", shaped: true, cells: &[(0, 0, ITEM_SULFUR), (1, 0, ITEM_SULFUR), (2, 0, ITEM_SULFUR), (0, 1, ITEM_SULFUR), (1, 1, ITEM_SULFUR), (2, 1, ITEM_SULFUR)] },
    Recipe { needs: &[(ITEM_HAY_BLOCK, 1), (ITEM_REDSTONE, 4)], result: (ITEM_TARGET, 1), name: "target", shaped: true, cells: &[(1, 0, ITEM_REDSTONE), (0, 1, ITEM_REDSTONE), (1, 1, ITEM_HAY_BLOCK), (2, 1, ITEM_REDSTONE), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_PRISMARINE, 1), (ITEM_TIDE_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_TIDE_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "tide_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_TIDE_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_PRISMARINE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_AMETHYST_SHARD, 4), (ITEM_GLASS, 1)], result: (ITEM_TINTED_GLASS, 2), name: "tinted_glass", shaped: true, cells: &[(1, 0, ITEM_AMETHYST_SHARD), (0, 1, ITEM_AMETHYST_SHARD), (1, 1, ITEM_GLASS), (2, 1, ITEM_AMETHYST_SHARD), (1, 2, ITEM_AMETHYST_SHARD)] },
    Recipe { needs: &[(ITEM_GUNPOWDER, 5), (TAG_SMELTS_TO_GLASS, 4)], result: (ITEM_TNT, 1), name: "tnt", shaped: true, cells: &[(0, 0, ITEM_GUNPOWDER), (1, 0, TAG_SMELTS_TO_GLASS), (2, 0, ITEM_GUNPOWDER), (0, 1, TAG_SMELTS_TO_GLASS), (1, 1, ITEM_GUNPOWDER), (2, 1, TAG_SMELTS_TO_GLASS), (0, 2, ITEM_GUNPOWDER), (1, 2, TAG_SMELTS_TO_GLASS), (2, 2, ITEM_GUNPOWDER)] },
    Recipe { needs: &[(ITEM_MINECART, 1), (ITEM_TNT, 1)], result: (ITEM_TNT_MINECART, 1), name: "tnt_minecart", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 1), (TAG_COALS, 1)], result: (TORCH, 4), name: "torch", shaped: true, cells: &[(0, 0, TAG_COALS), (0, 1, STICK)] },
    Recipe { needs: &[(ITEM_CHEST, 1), (ITEM_TRIPWIRE_HOOK, 1)], result: (ITEM_TRAPPED_CHEST, 1), name: "trapped_chest", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_IRON_INGOT, 1), (STICK, 1), (TAG_PLANKS, 1)], result: (ITEM_TRIPWIRE_HOOK, 2), name: "tripwire_hook", shaped: true, cells: &[(0, 0, ITEM_IRON_INGOT), (0, 1, STICK), (0, 2, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_TUFF_BRICKS, 3)], result: (ITEM_TUFF_BRICK_SLAB, 6), name: "tuff_brick_slab", shaped: true, cells: &[(0, 0, ITEM_TUFF_BRICKS), (1, 0, ITEM_TUFF_BRICKS), (2, 0, ITEM_TUFF_BRICKS)] },
    Recipe { needs: &[(ITEM_TUFF_BRICKS, 6)], result: (ITEM_TUFF_BRICK_STAIRS, 4), name: "tuff_brick_stairs", shaped: true, cells: &[(0, 0, ITEM_TUFF_BRICKS), (0, 1, ITEM_TUFF_BRICKS), (1, 1, ITEM_TUFF_BRICKS), (0, 2, ITEM_TUFF_BRICKS), (1, 2, ITEM_TUFF_BRICKS), (2, 2, ITEM_TUFF_BRICKS)] },
    Recipe { needs: &[(ITEM_TUFF_BRICKS, 6)], result: (ITEM_TUFF_BRICK_WALL, 6), name: "tuff_brick_wall", shaped: true, cells: &[(0, 0, ITEM_TUFF_BRICKS), (1, 0, ITEM_TUFF_BRICKS), (2, 0, ITEM_TUFF_BRICKS), (0, 1, ITEM_TUFF_BRICKS), (1, 1, ITEM_TUFF_BRICKS), (2, 1, ITEM_TUFF_BRICKS)] },
    Recipe { needs: &[(ITEM_POLISHED_TUFF, 4)], result: (ITEM_TUFF_BRICKS, 4), name: "tuff_bricks", shaped: true, cells: &[(0, 0, ITEM_POLISHED_TUFF), (1, 0, ITEM_POLISHED_TUFF), (0, 1, ITEM_POLISHED_TUFF), (1, 1, ITEM_POLISHED_TUFF)] },
    Recipe { needs: &[(ITEM_TUFF, 3)], result: (ITEM_TUFF_SLAB, 6), name: "tuff_slab", shaped: true, cells: &[(0, 0, ITEM_TUFF), (1, 0, ITEM_TUFF), (2, 0, ITEM_TUFF)] },
    Recipe { needs: &[(ITEM_TUFF, 6)], result: (ITEM_TUFF_STAIRS, 4), name: "tuff_stairs", shaped: true, cells: &[(0, 0, ITEM_TUFF), (0, 1, ITEM_TUFF), (1, 1, ITEM_TUFF), (0, 2, ITEM_TUFF), (1, 2, ITEM_TUFF), (2, 2, ITEM_TUFF)] },
    Recipe { needs: &[(ITEM_TUFF, 6)], result: (ITEM_TUFF_WALL, 6), name: "tuff_wall", shaped: true, cells: &[(0, 0, ITEM_TUFF), (1, 0, ITEM_TUFF), (2, 0, ITEM_TUFF), (0, 1, ITEM_TUFF), (1, 1, ITEM_TUFF), (2, 1, ITEM_TUFF)] },
    Recipe { needs: &[(ITEM_TURTLE_SCUTE, 5)], result: (ITEM_TURTLE_HELMET, 1), name: "turtle_helmet", shaped: true, cells: &[(0, 0, ITEM_TURTLE_SCUTE), (1, 0, ITEM_TURTLE_SCUTE), (2, 0, ITEM_TURTLE_SCUTE), (0, 1, ITEM_TURTLE_SCUTE), (2, 1, ITEM_TURTLE_SCUTE)] },
    Recipe { needs: &[(COBBLESTONE, 1), (ITEM_DIAMOND, 7), (ITEM_VEX_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_VEX_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "vex_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_VEX_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, COBBLESTONE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_COBBLED_DEEPSLATE, 1), (ITEM_DIAMOND, 7), (ITEM_WARD_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_WARD_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "ward_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_WARD_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_COBBLED_DEEPSLATE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 1)], result: (ITEM_WARPED_BUTTON, 1), name: "warped_button", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_DOOR, 3), name: "warped_door", shaped: true, cells: &[(0, 0, ITEM_WARPED_PLANKS), (1, 0, ITEM_WARPED_PLANKS), (0, 1, ITEM_WARPED_PLANKS), (1, 1, ITEM_WARPED_PLANKS), (0, 2, ITEM_WARPED_PLANKS), (1, 2, ITEM_WARPED_PLANKS)] },
    Recipe { needs: &[(STICK, 2), (ITEM_WARPED_PLANKS, 4)], result: (ITEM_WARPED_FENCE, 3), name: "warped_fence", shaped: true, cells: &[(0, 0, ITEM_WARPED_PLANKS), (1, 0, STICK), (2, 0, ITEM_WARPED_PLANKS), (0, 1, ITEM_WARPED_PLANKS), (1, 1, STICK), (2, 1, ITEM_WARPED_PLANKS)] },
    Recipe { needs: &[(STICK, 4), (ITEM_WARPED_PLANKS, 2)], result: (ITEM_WARPED_FENCE_GATE, 1), name: "warped_fence_gate", shaped: true, cells: &[(0, 0, STICK), (1, 0, ITEM_WARPED_PLANKS), (2, 0, STICK), (0, 1, STICK), (1, 1, ITEM_WARPED_PLANKS), (2, 1, STICK)] },
    Recipe { needs: &[(ITEM_FISHING_ROD, 1), (ITEM_WARPED_FUNGUS, 1)], result: (ITEM_WARPED_FUNGUS_ON_A_STICK, 1), name: "warped_fungus_on_a_stick", shaped: true, cells: &[(0, 0, ITEM_FISHING_ROD), (1, 1, ITEM_WARPED_FUNGUS)] },
    Recipe { needs: &[(ITEM_IRON_CHAIN, 2), (ITEM_STRIPPED_WARPED_STEM, 6)], result: (ITEM_WARPED_HANGING_SIGN, 6), name: "warped_hanging_sign", shaped: true, cells: &[(0, 0, ITEM_IRON_CHAIN), (2, 0, ITEM_IRON_CHAIN), (0, 1, ITEM_STRIPPED_WARPED_STEM), (1, 1, ITEM_STRIPPED_WARPED_STEM), (2, 1, ITEM_STRIPPED_WARPED_STEM), (0, 2, ITEM_STRIPPED_WARPED_STEM), (1, 2, ITEM_STRIPPED_WARPED_STEM), (2, 2, ITEM_STRIPPED_WARPED_STEM)] },
    Recipe { needs: &[(ITEM_WARPED_STEM, 4)], result: (ITEM_WARPED_HYPHAE, 3), name: "warped_hyphae", shaped: true, cells: &[(0, 0, ITEM_WARPED_STEM), (1, 0, ITEM_WARPED_STEM), (0, 1, ITEM_WARPED_STEM), (1, 1, ITEM_WARPED_STEM)] },
    Recipe { needs: &[(TAG_WARPED_STEMS, 1)], result: (ITEM_WARPED_PLANKS, 4), name: "warped_planks", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 2)], result: (ITEM_WARPED_PRESSURE_PLATE, 1), name: "warped_pressure_plate", shaped: true, cells: &[(0, 0, ITEM_WARPED_PLANKS), (1, 0, ITEM_WARPED_PLANKS)] },
    Recipe { needs: &[(ITEM_STRIPPED_WARPED_STEM, 6)], result: (ITEM_WARPED_SHELF, 6), name: "warped_shelf", shaped: true, cells: &[(0, 0, ITEM_STRIPPED_WARPED_STEM), (1, 0, ITEM_STRIPPED_WARPED_STEM), (2, 0, ITEM_STRIPPED_WARPED_STEM), (0, 2, ITEM_STRIPPED_WARPED_STEM), (1, 2, ITEM_STRIPPED_WARPED_STEM), (2, 2, ITEM_STRIPPED_WARPED_STEM)] },
    Recipe { needs: &[(STICK, 1), (ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_SIGN, 3), name: "warped_sign", shaped: true, cells: &[(0, 0, ITEM_WARPED_PLANKS), (1, 0, ITEM_WARPED_PLANKS), (2, 0, ITEM_WARPED_PLANKS), (0, 1, ITEM_WARPED_PLANKS), (1, 1, ITEM_WARPED_PLANKS), (2, 1, ITEM_WARPED_PLANKS), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 3)], result: (ITEM_WARPED_SLAB, 6), name: "warped_slab", shaped: true, cells: &[(0, 0, ITEM_WARPED_PLANKS), (1, 0, ITEM_WARPED_PLANKS), (2, 0, ITEM_WARPED_PLANKS)] },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_STAIRS, 4), name: "warped_stairs", shaped: true, cells: &[(0, 0, ITEM_WARPED_PLANKS), (0, 1, ITEM_WARPED_PLANKS), (1, 1, ITEM_WARPED_PLANKS), (0, 2, ITEM_WARPED_PLANKS), (1, 2, ITEM_WARPED_PLANKS), (2, 2, ITEM_WARPED_PLANKS)] },
    Recipe { needs: &[(ITEM_WARPED_PLANKS, 6)], result: (ITEM_WARPED_TRAPDOOR, 2), name: "warped_trapdoor", shaped: true, cells: &[(0, 0, ITEM_WARPED_PLANKS), (1, 0, ITEM_WARPED_PLANKS), (2, 0, ITEM_WARPED_PLANKS), (0, 1, ITEM_WARPED_PLANKS), (1, 1, ITEM_WARPED_PLANKS), (2, 1, ITEM_WARPED_PLANKS)] },
    Recipe { needs: &[(ITEM_WAXED_CUT_COPPER_SLAB, 2)], result: (ITEM_WAXED_CHISELED_COPPER, 1), name: "waxed_chiseled_copper", shaped: true, cells: &[(0, 0, ITEM_WAXED_CUT_COPPER_SLAB), (0, 1, ITEM_WAXED_CUT_COPPER_SLAB)] },
    Recipe { needs: &[(ITEM_CHISELED_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_CHISELED_COPPER, 1), name: "waxed_chiseled_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_BARS, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_BARS, 1), name: "waxed_copper_bars_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_BLOCK, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_BLOCK, 1), name: "waxed_copper_block_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WAXED_COPPER_BLOCK, 3)], result: (ITEM_WAXED_COPPER_BULB, 4), name: "waxed_copper_bulb", shaped: true, cells: &[(1, 0, ITEM_WAXED_COPPER_BLOCK), (0, 1, ITEM_WAXED_COPPER_BLOCK), (1, 1, ITEM_BLAZE_ROD), (2, 1, ITEM_WAXED_COPPER_BLOCK), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_COPPER_BULB, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_BULB, 1), name: "waxed_copper_bulb_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_CHAIN, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_CHAIN, 1), name: "waxed_copper_chain_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_CHEST, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_CHEST, 1), name: "waxed_copper_chest_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_DOOR, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_DOOR, 1), name: "waxed_copper_door_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_GOLEM_STATUE, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_GOLEM_STATUE, 1), name: "waxed_copper_golem_statue_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_COPPER_BLOCK, 4)], result: (ITEM_WAXED_COPPER_GRATE, 4), name: "waxed_copper_grate", shaped: true, cells: &[(1, 0, ITEM_WAXED_COPPER_BLOCK), (0, 1, ITEM_WAXED_COPPER_BLOCK), (2, 1, ITEM_WAXED_COPPER_BLOCK), (1, 2, ITEM_WAXED_COPPER_BLOCK)] },
    Recipe { needs: &[(ITEM_COPPER_GRATE, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_GRATE, 1), name: "waxed_copper_grate_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_LANTERN, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_LANTERN, 1), name: "waxed_copper_lantern_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_COPPER_TRAPDOOR, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_COPPER_TRAPDOOR, 1), name: "waxed_copper_trapdoor_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_COPPER_BLOCK, 4)], result: (ITEM_WAXED_CUT_COPPER, 4), name: "waxed_cut_copper", shaped: true, cells: &[(0, 0, ITEM_WAXED_COPPER_BLOCK), (1, 0, ITEM_WAXED_COPPER_BLOCK), (0, 1, ITEM_WAXED_COPPER_BLOCK), (1, 1, ITEM_WAXED_COPPER_BLOCK)] },
    Recipe { needs: &[(ITEM_CUT_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_CUT_COPPER, 1), name: "waxed_cut_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_CUT_COPPER, 3)], result: (ITEM_WAXED_CUT_COPPER_SLAB, 6), name: "waxed_cut_copper_slab", shaped: true, cells: &[(0, 0, ITEM_WAXED_CUT_COPPER), (1, 0, ITEM_WAXED_CUT_COPPER), (2, 0, ITEM_WAXED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_CUT_COPPER_SLAB, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_CUT_COPPER_SLAB, 1), name: "waxed_cut_copper_slab_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_CUT_COPPER, 6)], result: (ITEM_WAXED_CUT_COPPER_STAIRS, 4), name: "waxed_cut_copper_stairs", shaped: true, cells: &[(0, 0, ITEM_WAXED_CUT_COPPER), (0, 1, ITEM_WAXED_CUT_COPPER), (1, 1, ITEM_WAXED_CUT_COPPER), (0, 2, ITEM_WAXED_CUT_COPPER), (1, 2, ITEM_WAXED_CUT_COPPER), (2, 2, ITEM_WAXED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_CUT_COPPER_STAIRS, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_CUT_COPPER_STAIRS, 1), name: "waxed_cut_copper_stairs_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB, 2)], result: (ITEM_WAXED_EXPOSED_CHISELED_COPPER, 1), name: "waxed_exposed_chiseled_copper", shaped: true, cells: &[(0, 0, ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB), (0, 1, ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB)] },
    Recipe { needs: &[(ITEM_EXPOSED_CHISELED_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_CHISELED_COPPER, 1), name: "waxed_exposed_chiseled_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_BARS, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_BARS, 1), name: "waxed_exposed_copper_bars_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WAXED_EXPOSED_COPPER, 3)], result: (ITEM_WAXED_EXPOSED_COPPER_BULB, 4), name: "waxed_exposed_copper_bulb", shaped: true, cells: &[(1, 0, ITEM_WAXED_EXPOSED_COPPER), (0, 1, ITEM_WAXED_EXPOSED_COPPER), (1, 1, ITEM_BLAZE_ROD), (2, 1, ITEM_WAXED_EXPOSED_COPPER), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_BULB, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_BULB, 1), name: "waxed_exposed_copper_bulb_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_CHAIN, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_CHAIN, 1), name: "waxed_exposed_copper_chain_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_CHEST, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_CHEST, 1), name: "waxed_exposed_copper_chest_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_DOOR, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_DOOR, 1), name: "waxed_exposed_copper_door_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER, 1), name: "waxed_exposed_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_GOLEM_STATUE, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_GOLEM_STATUE, 1), name: "waxed_exposed_copper_golem_statue_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_COPPER, 4)], result: (ITEM_WAXED_EXPOSED_COPPER_GRATE, 4), name: "waxed_exposed_copper_grate", shaped: true, cells: &[(1, 0, ITEM_WAXED_EXPOSED_COPPER), (0, 1, ITEM_WAXED_EXPOSED_COPPER), (2, 1, ITEM_WAXED_EXPOSED_COPPER), (1, 2, ITEM_WAXED_EXPOSED_COPPER)] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_GRATE, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_GRATE, 1), name: "waxed_exposed_copper_grate_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_LANTERN, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_LANTERN, 1), name: "waxed_exposed_copper_lantern_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_COPPER_TRAPDOOR, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_COPPER_TRAPDOOR, 1), name: "waxed_exposed_copper_trapdoor_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_COPPER, 4)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER, 4), name: "waxed_exposed_cut_copper", shaped: true, cells: &[(0, 0, ITEM_WAXED_EXPOSED_COPPER), (1, 0, ITEM_WAXED_EXPOSED_COPPER), (0, 1, ITEM_WAXED_EXPOSED_COPPER), (1, 1, ITEM_WAXED_EXPOSED_COPPER)] },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER, 1), name: "waxed_exposed_cut_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_CUT_COPPER, 3)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB, 6), name: "waxed_exposed_cut_copper_slab", shaped: true, cells: &[(0, 0, ITEM_WAXED_EXPOSED_CUT_COPPER), (1, 0, ITEM_WAXED_EXPOSED_CUT_COPPER), (2, 0, ITEM_WAXED_EXPOSED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER_SLAB, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER_SLAB, 1), name: "waxed_exposed_cut_copper_slab_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_EXPOSED_CUT_COPPER, 6)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER_STAIRS, 4), name: "waxed_exposed_cut_copper_stairs", shaped: true, cells: &[(0, 0, ITEM_WAXED_EXPOSED_CUT_COPPER), (0, 1, ITEM_WAXED_EXPOSED_CUT_COPPER), (1, 1, ITEM_WAXED_EXPOSED_CUT_COPPER), (0, 2, ITEM_WAXED_EXPOSED_CUT_COPPER), (1, 2, ITEM_WAXED_EXPOSED_CUT_COPPER), (2, 2, ITEM_WAXED_EXPOSED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_EXPOSED_CUT_COPPER_STAIRS, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_CUT_COPPER_STAIRS, 1), name: "waxed_exposed_cut_copper_stairs_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_EXPOSED_LIGHTNING_ROD, 1), (ITEM_HONEYCOMB, 1)], result: (ITEM_WAXED_EXPOSED_LIGHTNING_ROD, 1), name: "waxed_exposed_lightning_rod_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_LIGHTNING_ROD, 1)], result: (ITEM_WAXED_LIGHTNING_ROD, 1), name: "waxed_lightning_rod_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB, 2)], result: (ITEM_WAXED_OXIDIZED_CHISELED_COPPER, 1), name: "waxed_oxidized_chiseled_copper", shaped: true, cells: &[(0, 0, ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB), (0, 1, ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_CHISELED_COPPER, 1)], result: (ITEM_WAXED_OXIDIZED_CHISELED_COPPER, 1), name: "waxed_oxidized_chiseled_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_BARS, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_BARS, 1), name: "waxed_oxidized_copper_bars_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WAXED_OXIDIZED_COPPER, 3)], result: (ITEM_WAXED_OXIDIZED_COPPER_BULB, 4), name: "waxed_oxidized_copper_bulb", shaped: true, cells: &[(1, 0, ITEM_WAXED_OXIDIZED_COPPER), (0, 1, ITEM_WAXED_OXIDIZED_COPPER), (1, 1, ITEM_BLAZE_ROD), (2, 1, ITEM_WAXED_OXIDIZED_COPPER), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_BULB, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_BULB, 1), name: "waxed_oxidized_copper_bulb_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_CHAIN, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_CHAIN, 1), name: "waxed_oxidized_copper_chain_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_CHEST, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_CHEST, 1), name: "waxed_oxidized_copper_chest_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_DOOR, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_DOOR, 1), name: "waxed_oxidized_copper_door_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER, 1), name: "waxed_oxidized_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_GOLEM_STATUE, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_GOLEM_STATUE, 1), name: "waxed_oxidized_copper_golem_statue_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_COPPER, 4)], result: (ITEM_WAXED_OXIDIZED_COPPER_GRATE, 4), name: "waxed_oxidized_copper_grate", shaped: true, cells: &[(1, 0, ITEM_WAXED_OXIDIZED_COPPER), (0, 1, ITEM_WAXED_OXIDIZED_COPPER), (2, 1, ITEM_WAXED_OXIDIZED_COPPER), (1, 2, ITEM_WAXED_OXIDIZED_COPPER)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_GRATE, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_GRATE, 1), name: "waxed_oxidized_copper_grate_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_LANTERN, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_LANTERN, 1), name: "waxed_oxidized_copper_lantern_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_COPPER_TRAPDOOR, 1)], result: (ITEM_WAXED_OXIDIZED_COPPER_TRAPDOOR, 1), name: "waxed_oxidized_copper_trapdoor_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_COPPER, 4)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER, 4), name: "waxed_oxidized_cut_copper", shaped: true, cells: &[(0, 0, ITEM_WAXED_OXIDIZED_COPPER), (1, 0, ITEM_WAXED_OXIDIZED_COPPER), (0, 1, ITEM_WAXED_OXIDIZED_COPPER), (1, 1, ITEM_WAXED_OXIDIZED_COPPER)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_CUT_COPPER, 1)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER, 1), name: "waxed_oxidized_cut_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_CUT_COPPER, 3)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB, 6), name: "waxed_oxidized_cut_copper_slab", shaped: true, cells: &[(0, 0, ITEM_WAXED_OXIDIZED_CUT_COPPER), (1, 0, ITEM_WAXED_OXIDIZED_CUT_COPPER), (2, 0, ITEM_WAXED_OXIDIZED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_CUT_COPPER_SLAB, 1)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER_SLAB, 1), name: "waxed_oxidized_cut_copper_slab_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_OXIDIZED_CUT_COPPER, 6)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER_STAIRS, 4), name: "waxed_oxidized_cut_copper_stairs", shaped: true, cells: &[(0, 0, ITEM_WAXED_OXIDIZED_CUT_COPPER), (0, 1, ITEM_WAXED_OXIDIZED_CUT_COPPER), (1, 1, ITEM_WAXED_OXIDIZED_CUT_COPPER), (0, 2, ITEM_WAXED_OXIDIZED_CUT_COPPER), (1, 2, ITEM_WAXED_OXIDIZED_CUT_COPPER), (2, 2, ITEM_WAXED_OXIDIZED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_CUT_COPPER_STAIRS, 1)], result: (ITEM_WAXED_OXIDIZED_CUT_COPPER_STAIRS, 1), name: "waxed_oxidized_cut_copper_stairs_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_OXIDIZED_LIGHTNING_ROD, 1)], result: (ITEM_WAXED_OXIDIZED_LIGHTNING_ROD, 1), name: "waxed_oxidized_lightning_rod_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB, 2)], result: (ITEM_WAXED_WEATHERED_CHISELED_COPPER, 1), name: "waxed_weathered_chiseled_copper", shaped: true, cells: &[(0, 0, ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB), (0, 1, ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_CHISELED_COPPER, 1)], result: (ITEM_WAXED_WEATHERED_CHISELED_COPPER, 1), name: "waxed_weathered_chiseled_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_BARS, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_BARS, 1), name: "waxed_weathered_copper_bars_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WAXED_WEATHERED_COPPER, 3)], result: (ITEM_WAXED_WEATHERED_COPPER_BULB, 4), name: "waxed_weathered_copper_bulb", shaped: true, cells: &[(1, 0, ITEM_WAXED_WEATHERED_COPPER), (0, 1, ITEM_WAXED_WEATHERED_COPPER), (1, 1, ITEM_BLAZE_ROD), (2, 1, ITEM_WAXED_WEATHERED_COPPER), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_BULB, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_BULB, 1), name: "waxed_weathered_copper_bulb_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_CHAIN, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_CHAIN, 1), name: "waxed_weathered_copper_chain_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_CHEST, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_CHEST, 1), name: "waxed_weathered_copper_chest_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_DOOR, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_DOOR, 1), name: "waxed_weathered_copper_door_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER, 1)], result: (ITEM_WAXED_WEATHERED_COPPER, 1), name: "waxed_weathered_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_GOLEM_STATUE, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_GOLEM_STATUE, 1), name: "waxed_weathered_copper_golem_statue_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_COPPER, 4)], result: (ITEM_WAXED_WEATHERED_COPPER_GRATE, 4), name: "waxed_weathered_copper_grate", shaped: true, cells: &[(1, 0, ITEM_WAXED_WEATHERED_COPPER), (0, 1, ITEM_WAXED_WEATHERED_COPPER), (2, 1, ITEM_WAXED_WEATHERED_COPPER), (1, 2, ITEM_WAXED_WEATHERED_COPPER)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_GRATE, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_GRATE, 1), name: "waxed_weathered_copper_grate_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_LANTERN, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_LANTERN, 1), name: "waxed_weathered_copper_lantern_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_COPPER_TRAPDOOR, 1)], result: (ITEM_WAXED_WEATHERED_COPPER_TRAPDOOR, 1), name: "waxed_weathered_copper_trapdoor_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_COPPER, 4)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER, 4), name: "waxed_weathered_cut_copper", shaped: true, cells: &[(0, 0, ITEM_WAXED_WEATHERED_COPPER), (1, 0, ITEM_WAXED_WEATHERED_COPPER), (0, 1, ITEM_WAXED_WEATHERED_COPPER), (1, 1, ITEM_WAXED_WEATHERED_COPPER)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_CUT_COPPER, 1)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER, 1), name: "waxed_weathered_cut_copper_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_CUT_COPPER, 3)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB, 6), name: "waxed_weathered_cut_copper_slab", shaped: true, cells: &[(0, 0, ITEM_WAXED_WEATHERED_CUT_COPPER), (1, 0, ITEM_WAXED_WEATHERED_CUT_COPPER), (2, 0, ITEM_WAXED_WEATHERED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_CUT_COPPER_SLAB, 1)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER_SLAB, 1), name: "waxed_weathered_cut_copper_slab_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WAXED_WEATHERED_CUT_COPPER, 6)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER_STAIRS, 4), name: "waxed_weathered_cut_copper_stairs", shaped: true, cells: &[(0, 0, ITEM_WAXED_WEATHERED_CUT_COPPER), (0, 1, ITEM_WAXED_WEATHERED_CUT_COPPER), (1, 1, ITEM_WAXED_WEATHERED_CUT_COPPER), (0, 2, ITEM_WAXED_WEATHERED_CUT_COPPER), (1, 2, ITEM_WAXED_WEATHERED_CUT_COPPER), (2, 2, ITEM_WAXED_WEATHERED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_CUT_COPPER_STAIRS, 1)], result: (ITEM_WAXED_WEATHERED_CUT_COPPER_STAIRS, 1), name: "waxed_weathered_cut_copper_stairs_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_HONEYCOMB, 1), (ITEM_WEATHERED_LIGHTNING_ROD, 1)], result: (ITEM_WAXED_WEATHERED_LIGHTNING_ROD, 1), name: "waxed_weathered_lightning_rod_from_honeycomb", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_TERRACOTTA, 1), (ITEM_WAYFINDER_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_WAYFINDER_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "wayfinder_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_WAYFINDER_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_TERRACOTTA), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_WEATHERED_CUT_COPPER_SLAB, 2)], result: (ITEM_WEATHERED_CHISELED_COPPER, 1), name: "weathered_chiseled_copper", shaped: true, cells: &[(0, 0, ITEM_WEATHERED_CUT_COPPER_SLAB), (0, 1, ITEM_WEATHERED_CUT_COPPER_SLAB)] },
    Recipe { needs: &[(ITEM_BLAZE_ROD, 1), (ITEM_REDSTONE, 1), (ITEM_WEATHERED_COPPER, 3)], result: (ITEM_WEATHERED_COPPER_BULB, 4), name: "weathered_copper_bulb", shaped: true, cells: &[(1, 0, ITEM_WEATHERED_COPPER), (0, 1, ITEM_WEATHERED_COPPER), (1, 1, ITEM_BLAZE_ROD), (2, 1, ITEM_WEATHERED_COPPER), (1, 2, ITEM_REDSTONE)] },
    Recipe { needs: &[(ITEM_WEATHERED_COPPER, 4)], result: (ITEM_WEATHERED_COPPER_GRATE, 4), name: "weathered_copper_grate", shaped: true, cells: &[(1, 0, ITEM_WEATHERED_COPPER), (0, 1, ITEM_WEATHERED_COPPER), (2, 1, ITEM_WEATHERED_COPPER), (1, 2, ITEM_WEATHERED_COPPER)] },
    Recipe { needs: &[(ITEM_WEATHERED_COPPER, 4)], result: (ITEM_WEATHERED_CUT_COPPER, 4), name: "weathered_cut_copper", shaped: true, cells: &[(0, 0, ITEM_WEATHERED_COPPER), (1, 0, ITEM_WEATHERED_COPPER), (0, 1, ITEM_WEATHERED_COPPER), (1, 1, ITEM_WEATHERED_COPPER)] },
    Recipe { needs: &[(ITEM_WEATHERED_CUT_COPPER, 3)], result: (ITEM_WEATHERED_CUT_COPPER_SLAB, 6), name: "weathered_cut_copper_slab", shaped: true, cells: &[(0, 0, ITEM_WEATHERED_CUT_COPPER), (1, 0, ITEM_WEATHERED_CUT_COPPER), (2, 0, ITEM_WEATHERED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_WEATHERED_CUT_COPPER, 6)], result: (ITEM_WEATHERED_CUT_COPPER_STAIRS, 4), name: "weathered_cut_copper_stairs", shaped: true, cells: &[(0, 0, ITEM_WEATHERED_CUT_COPPER), (0, 1, ITEM_WEATHERED_CUT_COPPER), (1, 1, ITEM_WEATHERED_CUT_COPPER), (0, 2, ITEM_WEATHERED_CUT_COPPER), (1, 2, ITEM_WEATHERED_CUT_COPPER), (2, 2, ITEM_WEATHERED_CUT_COPPER)] },
    Recipe { needs: &[(ITEM_HAY_BLOCK, 1)], result: (WHEAT, 9), name: "wheat", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 1), (ITEM_WHITE_WOOL, 6)], result: (ITEM_WHITE_BANNER, 1), name: "white_banner", shaped: true, cells: &[(0, 0, ITEM_WHITE_WOOL), (1, 0, ITEM_WHITE_WOOL), (2, 0, ITEM_WHITE_WOOL), (0, 1, ITEM_WHITE_WOOL), (1, 1, ITEM_WHITE_WOOL), (2, 1, ITEM_WHITE_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_WHITE_BED, 1), name: "white_bed", shaped: true, cells: &[(0, 0, ITEM_WHITE_WOOL), (1, 0, ITEM_WHITE_WOOL), (2, 0, ITEM_WHITE_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_CANDLE, 1), name: "white_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 2)], result: (ITEM_WHITE_CARPET, 3), name: "white_carpet", shaped: true, cells: &[(0, 0, ITEM_WHITE_WOOL), (1, 0, ITEM_WHITE_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_CONCRETE_POWDER, 8), name: "white_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WHITE_CONCRETE, 3)], result: (ITEM_WHITE_CONCRETE_SLAB, 6), name: "white_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_WHITE_CONCRETE), (1, 0, ITEM_WHITE_CONCRETE), (2, 0, ITEM_WHITE_CONCRETE)] },
    Recipe { needs: &[(ITEM_WHITE_CONCRETE, 6)], result: (ITEM_WHITE_CONCRETE_STAIRS, 4), name: "white_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_WHITE_CONCRETE), (0, 1, ITEM_WHITE_CONCRETE), (1, 1, ITEM_WHITE_CONCRETE), (0, 2, ITEM_WHITE_CONCRETE), (1, 2, ITEM_WHITE_CONCRETE), (2, 2, ITEM_WHITE_CONCRETE)] },
    Recipe { needs: &[(ITEM_WHITE_WOOL_SLAB, 3)], result: (ITEM_WHITE_CUSHION, 1), name: "white_cushion", shaped: true, cells: &[(0, 0, ITEM_WHITE_WOOL_SLAB), (1, 0, ITEM_WHITE_WOOL_SLAB), (2, 0, ITEM_WHITE_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_BONE_MEAL, 1)], result: (ITEM_WHITE_DYE, 1), name: "white_dye", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_LILY_OF_THE_VALLEY, 1)], result: (ITEM_WHITE_DYE, 1), name: "white_dye_from_lily_of_the_valley", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_WHITE_WOOL, 1)], result: (ITEM_WHITE_HARNESS, 1), name: "white_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_WHITE_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_STAINED_GLASS, 8), name: "white_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_WHITE_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_WHITE_STAINED_GLASS, 6)], result: (ITEM_WHITE_STAINED_GLASS_PANE, 16), name: "white_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_WHITE_STAINED_GLASS), (1, 0, ITEM_WHITE_STAINED_GLASS), (2, 0, ITEM_WHITE_STAINED_GLASS), (0, 1, ITEM_WHITE_STAINED_GLASS), (1, 1, ITEM_WHITE_STAINED_GLASS), (2, 1, ITEM_WHITE_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_STAINED_GLASS_PANE, 8), name: "white_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_WHITE_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_TERRACOTTA, 8), (ITEM_WHITE_DYE, 1)], result: (ITEM_WHITE_TERRACOTTA, 8), name: "white_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_WHITE_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_STRING, 4)], result: (ITEM_WHITE_WOOL, 1), name: "white_wool_from_string", shaped: true, cells: &[(0, 0, ITEM_STRING), (1, 0, ITEM_STRING), (0, 1, ITEM_STRING), (1, 1, ITEM_STRING)] },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 3)], result: (ITEM_WHITE_WOOL_SLAB, 6), name: "white_wool_slab", shaped: true, cells: &[(0, 0, ITEM_WHITE_WOOL), (1, 0, ITEM_WHITE_WOOL), (2, 0, ITEM_WHITE_WOOL)] },
    Recipe { needs: &[(ITEM_WHITE_WOOL, 6)], result: (ITEM_WHITE_WOOL_STAIRS, 4), name: "white_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_WHITE_WOOL), (0, 1, ITEM_WHITE_WOOL), (1, 1, ITEM_WHITE_WOOL), (0, 2, ITEM_WHITE_WOOL), (1, 2, ITEM_WHITE_WOOL), (2, 2, ITEM_WHITE_WOOL)] },
    Recipe { needs: &[(ITEM_DIAMOND, 7), (ITEM_MOSSY_COBBLESTONE, 1), (ITEM_WILD_ARMOR_TRIM_SMITHING_TEMPLATE, 1)], result: (ITEM_WILD_ARMOR_TRIM_SMITHING_TEMPLATE, 2), name: "wild_armor_trim_smithing_template", shaped: true, cells: &[(0, 0, ITEM_DIAMOND), (1, 0, ITEM_WILD_ARMOR_TRIM_SMITHING_TEMPLATE), (2, 0, ITEM_DIAMOND), (0, 1, ITEM_DIAMOND), (1, 1, ITEM_MOSSY_COBBLESTONE), (2, 1, ITEM_DIAMOND), (0, 2, ITEM_DIAMOND), (1, 2, ITEM_DIAMOND), (2, 2, ITEM_DIAMOND)] },
    Recipe { needs: &[(ITEM_BREEZE_ROD, 1)], result: (ITEM_WIND_CHARGE, 4), name: "wind_charge", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_ARMADILLO_SCUTE, 6)], result: (ITEM_WOLF_ARMOR, 1), name: "wolf_armor", shaped: true, cells: &[(0, 0, ITEM_ARMADILLO_SCUTE), (0, 1, ITEM_ARMADILLO_SCUTE), (1, 1, ITEM_ARMADILLO_SCUTE), (2, 1, ITEM_ARMADILLO_SCUTE), (0, 2, ITEM_ARMADILLO_SCUTE), (2, 2, ITEM_ARMADILLO_SCUTE)] },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 3)], result: (ITEM_WOODEN_AXE, 1), name: "wooden_axe", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (0, 1, TAG_PLANKS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 2)], result: (ITEM_WOODEN_HOE, 1), name: "wooden_hoe", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 3)], result: (WOODEN_PICKAXE, 1), name: "wooden_pickaxe", shaped: true, cells: &[(0, 0, TAG_PLANKS), (1, 0, TAG_PLANKS), (2, 0, TAG_PLANKS), (1, 1, STICK), (1, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 1)], result: (ITEM_WOODEN_SHOVEL, 1), name: "wooden_shovel", shaped: true, cells: &[(0, 0, TAG_PLANKS), (0, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 2), (TAG_PLANKS, 1)], result: (ITEM_WOODEN_SPEAR, 1), name: "wooden_spear", shaped: true, cells: &[(2, 0, TAG_PLANKS), (1, 1, STICK), (0, 2, STICK)] },
    Recipe { needs: &[(STICK, 1), (TAG_PLANKS, 2)], result: (ITEM_WOODEN_SWORD, 1), name: "wooden_sword", shaped: true, cells: &[(0, 0, TAG_PLANKS), (0, 1, TAG_PLANKS), (0, 2, STICK)] },
    Recipe { needs: &[(ITEM_BOOK, 1), (ITEM_FEATHER, 1), (ITEM_INK_SAC, 1)], result: (ITEM_WRITABLE_BOOK, 1), name: "writable_book", shaped: false, cells: &[] },
    Recipe { needs: &[(STICK, 1), (ITEM_YELLOW_WOOL, 6)], result: (ITEM_YELLOW_BANNER, 1), name: "yellow_banner", shaped: true, cells: &[(0, 0, ITEM_YELLOW_WOOL), (1, 0, ITEM_YELLOW_WOOL), (2, 0, ITEM_YELLOW_WOOL), (0, 1, ITEM_YELLOW_WOOL), (1, 1, ITEM_YELLOW_WOOL), (2, 1, ITEM_YELLOW_WOOL), (1, 2, STICK)] },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 3), (TAG_PLANKS, 3)], result: (ITEM_YELLOW_BED, 1), name: "yellow_bed", shaped: true, cells: &[(0, 0, ITEM_YELLOW_WOOL), (1, 0, ITEM_YELLOW_WOOL), (2, 0, ITEM_YELLOW_WOOL), (0, 1, TAG_PLANKS), (1, 1, TAG_PLANKS), (2, 1, TAG_PLANKS)] },
    Recipe { needs: &[(ITEM_CANDLE, 1), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_CANDLE, 1), name: "yellow_candle", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 2)], result: (ITEM_YELLOW_CARPET, 3), name: "yellow_carpet", shaped: true, cells: &[(0, 0, ITEM_YELLOW_WOOL), (1, 0, ITEM_YELLOW_WOOL)] },
    Recipe { needs: &[(ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_GRAVEL, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_SAND, 1), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_CONCRETE_POWDER, 8), name: "yellow_concrete_powder", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_YELLOW_CONCRETE, 3)], result: (ITEM_YELLOW_CONCRETE_SLAB, 6), name: "yellow_concrete_slab", shaped: true, cells: &[(0, 0, ITEM_YELLOW_CONCRETE), (1, 0, ITEM_YELLOW_CONCRETE), (2, 0, ITEM_YELLOW_CONCRETE)] },
    Recipe { needs: &[(ITEM_YELLOW_CONCRETE, 6)], result: (ITEM_YELLOW_CONCRETE_STAIRS, 4), name: "yellow_concrete_stairs", shaped: true, cells: &[(0, 0, ITEM_YELLOW_CONCRETE), (0, 1, ITEM_YELLOW_CONCRETE), (1, 1, ITEM_YELLOW_CONCRETE), (0, 2, ITEM_YELLOW_CONCRETE), (1, 2, ITEM_YELLOW_CONCRETE), (2, 2, ITEM_YELLOW_CONCRETE)] },
    Recipe { needs: &[(ITEM_YELLOW_WOOL_SLAB, 3)], result: (ITEM_YELLOW_CUSHION, 1), name: "yellow_cushion", shaped: true, cells: &[(0, 0, ITEM_YELLOW_WOOL_SLAB), (1, 0, ITEM_YELLOW_WOOL_SLAB), (2, 0, ITEM_YELLOW_WOOL_SLAB)] },
    Recipe { needs: &[(ITEM_DANDELION, 1)], result: (ITEM_YELLOW_DYE, 1), name: "yellow_dye_from_dandelion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GOLDEN_DANDELION, 1)], result: (ITEM_YELLOW_DYE, 1), name: "yellow_dye_from_golden_dandelion", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_SUNFLOWER, 1)], result: (ITEM_YELLOW_DYE, 2), name: "yellow_dye_from_sunflower", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_WILDFLOWERS, 1)], result: (ITEM_YELLOW_DYE, 1), name: "yellow_dye_from_wildflowers", shaped: false, cells: &[] },
    Recipe { needs: &[(ITEM_GLASS, 2), (ITEM_LEATHER, 3), (ITEM_YELLOW_WOOL, 1)], result: (ITEM_YELLOW_HARNESS, 1), name: "yellow_harness", shaped: true, cells: &[(0, 0, ITEM_LEATHER), (1, 0, ITEM_LEATHER), (2, 0, ITEM_LEATHER), (0, 1, ITEM_GLASS), (1, 1, ITEM_YELLOW_WOOL), (2, 1, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS, 8), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_STAINED_GLASS, 8), name: "yellow_stained_glass", shaped: true, cells: &[(0, 0, ITEM_GLASS), (1, 0, ITEM_GLASS), (2, 0, ITEM_GLASS), (0, 1, ITEM_GLASS), (1, 1, ITEM_YELLOW_DYE), (2, 1, ITEM_GLASS), (0, 2, ITEM_GLASS), (1, 2, ITEM_GLASS), (2, 2, ITEM_GLASS)] },
    Recipe { needs: &[(ITEM_YELLOW_STAINED_GLASS, 6)], result: (ITEM_YELLOW_STAINED_GLASS_PANE, 16), name: "yellow_stained_glass_pane", shaped: true, cells: &[(0, 0, ITEM_YELLOW_STAINED_GLASS), (1, 0, ITEM_YELLOW_STAINED_GLASS), (2, 0, ITEM_YELLOW_STAINED_GLASS), (0, 1, ITEM_YELLOW_STAINED_GLASS), (1, 1, ITEM_YELLOW_STAINED_GLASS), (2, 1, ITEM_YELLOW_STAINED_GLASS)] },
    Recipe { needs: &[(ITEM_GLASS_PANE, 8), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_STAINED_GLASS_PANE, 8), name: "yellow_stained_glass_pane_from_glass_pane", shaped: true, cells: &[(0, 0, ITEM_GLASS_PANE), (1, 0, ITEM_GLASS_PANE), (2, 0, ITEM_GLASS_PANE), (0, 1, ITEM_GLASS_PANE), (1, 1, ITEM_YELLOW_DYE), (2, 1, ITEM_GLASS_PANE), (0, 2, ITEM_GLASS_PANE), (1, 2, ITEM_GLASS_PANE), (2, 2, ITEM_GLASS_PANE)] },
    Recipe { needs: &[(ITEM_TERRACOTTA, 8), (ITEM_YELLOW_DYE, 1)], result: (ITEM_YELLOW_TERRACOTTA, 8), name: "yellow_terracotta", shaped: true, cells: &[(0, 0, ITEM_TERRACOTTA), (1, 0, ITEM_TERRACOTTA), (2, 0, ITEM_TERRACOTTA), (0, 1, ITEM_TERRACOTTA), (1, 1, ITEM_YELLOW_DYE), (2, 1, ITEM_TERRACOTTA), (0, 2, ITEM_TERRACOTTA), (1, 2, ITEM_TERRACOTTA), (2, 2, ITEM_TERRACOTTA)] },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 3)], result: (ITEM_YELLOW_WOOL_SLAB, 6), name: "yellow_wool_slab", shaped: true, cells: &[(0, 0, ITEM_YELLOW_WOOL), (1, 0, ITEM_YELLOW_WOOL), (2, 0, ITEM_YELLOW_WOOL)] },
    Recipe { needs: &[(ITEM_YELLOW_WOOL, 6)], result: (ITEM_YELLOW_WOOL_STAIRS, 4), name: "yellow_wool_stairs", shaped: true, cells: &[(0, 0, ITEM_YELLOW_WOOL), (0, 1, ITEM_YELLOW_WOOL), (1, 1, ITEM_YELLOW_WOOL), (0, 2, ITEM_YELLOW_WOOL), (1, 2, ITEM_YELLOW_WOOL), (2, 2, ITEM_YELLOW_WOOL)] },
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

/// 3×3 配方匹配：P60 vanilla 口径（shaped 定位+镜像+平移，shapeless 逐格单位）。
/// 9 格行序（idx = y*3+x），与窗口槽 1..=9 一一对应。
pub fn match_recipe_9(grid: [Option<(i32, i32)>; GRID9_SIZE]) -> Option<&'static Recipe> {
    match_in_grid(&grid, 3)
}

/// 3×3 合成格展平。
pub fn grid_stacks9(grid: [Option<(i32, i32)>; GRID9_SIZE]) -> Vec<(i32, i32)> {
    grid.into_iter().flatten().collect()
}

/// 按 ID 从 3×3 格扣除原料（P60 vanilla 口径：shaped 按命中定位格各扣 1；
/// shapeless 每个占用格扣 1，多的留下）。返回是否全部扣成功。
pub fn consume_grid9(grid: &mut [Option<(i32, i32)>; GRID9_SIZE], recipe: &Recipe) -> bool {
    consume_in_grid(grid, 3, recipe)
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

/// P60 vanilla 形状匹配核心（`match_recipe`/`match_recipe_9` 共用）。
///
/// 网格行序（2×2 idx = y*2+x；3×3 idx = y*3+x）：
/// - shaped：`cells` 为去边稀疏格；允许平移 + 水平镜像；
///   覆盖格须非空（数量 ≥1，tag 按成员归属），其余格须空。
/// - shapeless：`needs` 展开为逐单位列表（每占用格提供 1 单位，多的留下）；
///   占用格数须等于单位数（格外有货就不中，原版口径），tag 灵活归属。
/// 首个命中的配方即结果（表序，确定性；原版同输入形状冲突时同样按配方序）。

/// 料引用是否接受槽内物品（负数 id = [`CRAFT_TAGS`] 索引）。
/// 匹配/补料共用同一口径。
pub fn need_accepts(need_id: i32, slot_id: i32) -> bool {
    if need_id >= 0 {
        need_id == slot_id
    } else {
        tag_members(need_id).contains(&slot_id)
    }
}

/// 槽是否视为空（None 或数量 ≤0）。
fn slot_empty(s: &Option<(i32, i32)>) -> bool {
    match s {
        None => true,
        Some((_, c)) => *c <= 0,
    }
}

/// 在 gw×gh 行序网格中找 shaped 配方的首个命中
///（镜像 × 平移均按先上后下/先左后右），返回命中的格下标（升序）。
fn find_placement(
    cells: &[(u8, u8, i32)],
    grid: &[Option<(i32, i32)>],
    gw: usize,
) -> Option<Vec<usize>> {
    if cells.is_empty() {
        return None;
    }
    let pw = cells.iter().map(|(x, _, _)| *x as usize).max().unwrap_or(0) + 1;
    let ph = cells.iter().map(|(_, y, _)| *y as usize).max().unwrap_or(0) + 1;
    let gh = grid.len() / gw;
    if pw > gw || ph > gh {
        return None;
    }
    for mirror in [false, true] {
        for oy in 0..=(gh - ph) {
            for ox in 0..=(gw - pw) {
                let mut covered = [false; 9];
                let mut hit = Vec::with_capacity(cells.len());
                let mut ok = true;
                for (x, y, need) in cells {
                    let cx = (if mirror { pw - 1 - *x as usize } else { *x as usize }) + ox;
                    let cy = *y as usize + oy;
                    let idx = cy * gw + cx;
                    match grid[idx] {
                        Some((sid, c)) if c > 0 && need_accepts(*need, sid) => {
                            if covered[idx] {
                                ok = false;
                                break;
                            }
                            covered[idx] = true;
                            hit.push(idx);
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    continue;
                }
                if grid.iter().enumerate().any(|(i, s)| !covered[i] && !slot_empty(s)) {
                    continue;
                }
                hit.sort_unstable();
                return Some(hit);
            }
        }
    }
    None
}

/// shapeless 单位展开：needs (id, n) → n 个 id（表序，确定性）。
fn shapeless_units(needs: &[(i32, i32)]) -> Vec<i32> {
    let mut units = Vec::new();
    for (id, n) in needs {
        for _ in 0..*n {
            units.push(*id);
        }
    }
    units
}

/// 网格占用槽（行序 idx，id；跳过空槽）。
fn occupied_slots(grid: &[Option<(i32, i32)>]) -> Vec<(usize, i32)> {
    grid.iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Some((id, c)) if *c > 0 => Some((i, *id)),
            _ => None,
        })
        .collect()
}

/// shapeless 匹配存在性：占用格与单位二分图匹配（Kuhn，行序/表序确定性）。
/// 占用格数须等于单位数（格外有货就不中）。
fn shapeless_match(units: &[i32], slots: &[(usize, i32)]) -> bool {
    if units.len() != slots.len() {
        return false;
    }
    fn dfs(
        u: usize,
        units: &[i32],
        slots: &[(usize, i32)],
        mt: &mut [Option<usize>],
        seen: &mut [bool],
    ) -> bool {
        for (s, &(_, sid)) in slots.iter().enumerate() {
            if seen[s] || !need_accepts(units[u], sid) {
                continue;
            }
            seen[s] = true;
            if mt[s].is_none() || dfs(mt[s].unwrap(), units, slots, mt, seen) {
                mt[s] = Some(u);
                return true;
            }
        }
        false
    }
    let mut mt: Vec<Option<usize>> = vec![None; slots.len()];
    for u in 0..units.len() {
        let mut seen = vec![false; slots.len()];
        if !dfs(u, units, slots, &mut mt, &mut seen) {
            return false;
        }
    }
    true
}

/// 通用匹配：在 gw×gh 行序网格中按表序找首个命中的配方。
fn match_in_grid(grid: &[Option<(i32, i32)>], gw: usize) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| {
        if r.shaped {
            find_placement(r.cells, grid, gw).is_some()
        } else {
            shapeless_match(&shapeless_units(r.needs), &occupied_slots(grid))
        }
    })
}

/// 命中后扣料：shaped 按首个命中的定位格各扣 1；
/// shapeless 每个占用格扣 1（归属无关：全占用格恰好各扣 1，多的留下）。
/// 调用方应先 match；不匹配时返回 false，不扣。格外有货时同样拒绝。
fn consume_in_grid(grid: &mut [Option<(i32, i32)>], gw: usize, recipe: &Recipe) -> bool {
    if recipe.shaped {
        let Some(hit) = find_placement(recipe.cells, grid, gw) else {
            return false;
        };
        for i in hit {
            match grid[i].as_mut() {
                Some((_, c)) if *c > 0 => {
                    *c -= 1;
                    if *c <= 0 {
                        grid[i] = None;
                    }
                }
                _ => return false,
            }
        }
        true
    } else {
        let occ = occupied_slots(grid);
        if !shapeless_match(&shapeless_units(recipe.needs), &occ) {
            return false;
        }
        for (i, _) in occ {
            match grid[i].as_mut() {
                Some((_, c)) if *c > 0 => {
                    *c -= 1;
                    if *c <= 0 {
                        grid[i] = None;
                    }
                }
                _ => return false,
            }
        }
        true
    }
}

/// P60：找首个"可继续补料"的 shaped 放置：非覆盖格须空；
/// 覆盖格空着待补，或放着可接受的料。返回 (格下标, 该格料引用)（下标升序）。
/// shapeless 不用此函数（见工作台补料：已占格须全可接受，填空格即可）。
pub fn refill_placement(
    recipe: &Recipe,
    grid: &[Option<(i32, i32)>],
    gw: usize,
) -> Option<Vec<(usize, i32)>> {
    if !recipe.shaped || recipe.cells.is_empty() {
        return None;
    }
    let pw = recipe.cells.iter().map(|(x, _, _)| *x as usize).max().unwrap_or(0) + 1;
    let ph = recipe.cells.iter().map(|(_, y, _)| *y as usize).max().unwrap_or(0) + 1;
    let gh = grid.len() / gw;
    if pw > gw || ph > gh {
        return None;
    }
    for mirror in [false, true] {
        for oy in 0..=(gh - ph) {
            for ox in 0..=(gw - pw) {
                let mut covered = [false; 9];
                let mut hit = Vec::with_capacity(recipe.cells.len());
                let mut ok = true;
                for (x, y, need) in recipe.cells {
                    let cx = (if mirror { pw - 1 - *x as usize } else { *x as usize }) + ox;
                    let cy = *y as usize + oy;
                    let idx = cy * gw + cx;
                    match grid[idx] {
                        Some((sid, c)) if c > 0 => {
                            if covered[idx] || !need_accepts(*need, sid) {
                                ok = false;
                                break;
                            }
                            covered[idx] = true;
                        }
                        _ => {
                            if covered[idx] {
                                ok = false;
                                break;
                            }
                            covered[idx] = true;
                        }
                    }
                    hit.push((idx, *need));
                }
                if !ok {
                    continue;
                }
                if grid.iter().enumerate().any(|(i, s)| !covered[i] && !slot_empty(s)) {
                    continue;
                }
                hit.sort_unstable();
                return Some(hit);
            }
        }
    }
    None
}

/// P60：旧 multiset 指纹拆分已删除（形状即真相，无需规范槽型）。

/// P60：旧展平/多重集/指纹 helper 已删除（见 [`match_in_grid`]）。

/// P60：旧规范槽型表已删除（vanilla 形状由生成器逐条给出，见 `cells`）。

/// 2×2 配方匹配：P60 vanilla 口径（shaped 定位+镜像+平移，shapeless 逐格单位）。
/// 4 格行序（idx = y*2+x），与背包槽 1..=4 一一对应；表序首中，确定性。
pub fn match_recipe(grid: [Option<(i32, i32)>; GRID_SIZE]) -> Option<&'static Recipe> {
    match_in_grid(&grid, 2)
}
/// 配方命中后按 vanilla 口径从 2×2 格扣除原料（每命中格扣 1，多的留下；
/// 返回是否全部扣成功）。调用方应先 `match_recipe`；不匹配时不要调用。
pub fn consume_grid(grid: &mut [Option<(i32, i32)>; GRID_SIZE], recipe: &Recipe) -> bool {
    consume_in_grid(grid, 2, recipe)
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

    /// P60 vanilla 形状：位置/朝向决定一切，堆叠多件只算 1 单位/格。
    #[test]
    fn match_and_consume_basic_recipes() {
        // 1 原木（单格，shapeless 单单位）→ 4 木板；整组也只扣 1 个。
        let mut grid = [None, Some((OAK_LOG, 1)), None, None];
        let r = match_recipe(grid).expect("one log must make planks");
        assert_eq!(r.result, (OAK_PLANKS, 4));
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [None; GRID_SIZE]);
        let mut grid = [Some((OAK_LOG, 5)), None, None, None];
        let r = match_recipe(grid).expect("stacked log still crafts");
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [Some((OAK_LOG, 4)), None, None, None]);

        // 木棍 = 竖列 2 木板（左右列都行，平移口径）。
        let grid = [Some((OAK_PLANKS, 1)), None, Some((OAK_PLANKS, 1)), None];
        assert_eq!(match_recipe(grid).unwrap().result, (STICK, 4));
        let grid = [None, Some((OAK_PLANKS, 1)), None, Some((OAK_PLANKS, 1))];
        assert_eq!(match_recipe(grid).unwrap().name, "stick");
        // 横摆 2 木板是压力板，不是木棍（问题 1 的核心区分）。
        let grid = [Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), None, None];
        assert_eq!(match_recipe(grid).unwrap().name, "oak_pressure_plate");
        // 单格堆 2 木板：木棍/压力板都不成立；命中的是木按钮（1 木板 shapeless）。
        assert_eq!(
            match_recipe([Some((OAK_PLANKS, 2)), None, None, None]).unwrap().name,
            "oak_button"
        );
        // 对角线也不是任何配方。
        assert!(match_recipe([Some((OAK_PLANKS, 1)), None, None, Some((OAK_PLANKS, 1))]).is_none());

        // 火把 = 煤上木棍下（左列）；只扣每格 1 个，多的留下。
        let mut grid = [Some((COAL, 3)), None, Some((STICK, 5)), None];
        let r = match_recipe(grid).expect("torch column");
        assert_eq!(r.result, (TORCH, 4));
        assert!(consume_grid(&mut grid, r));
        assert_eq!(grid, [Some((COAL, 2)), None, Some((STICK, 4)), None]);
        // 火把横摆/倒置不成立。
        assert!(match_recipe([Some((COAL, 1)), Some((STICK, 1)), None, None]).is_none());
        assert!(match_recipe([Some((STICK, 1)), None, Some((COAL, 1)), None]).is_none());

        // 工作台 = 2×2 铺满；缺一格不成立。
        let mut grid =
            [Some((OAK_PLANKS, 2)), Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 3))];
        let r = match_recipe(grid).expect("crafting table");
        assert_eq!(r.name, "crafting_table");
        assert!(consume_grid(&mut grid, r));
        assert_eq!(
            grid,
            [Some((OAK_PLANKS, 1)), None, None, Some((OAK_PLANKS, 2))]
        );
        assert!(match_recipe([Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1)), None]).is_none());

        // 书 = shapeless（3 纸 + 皮革，占满 4 格，顺序无关）。
        let mut grid = [Some((ITEM_PAPER, 2)), Some((ITEM_PAPER, 1)), Some((ITEM_PAPER, 7)), Some((ITEM_LEATHER, 1))];
        let r = match_recipe(grid).expect("book");
        assert_eq!(r.result, (ITEM_BOOK, 1));
        assert!(consume_grid(&mut grid, r));
        assert_eq!(
            grid,
            [Some((ITEM_PAPER, 1)), None, Some((ITEM_PAPER, 6)), None]
        );

        // TAG 混合：橡木 + 云杉木板竖列也能出木棍。
        let grid = [Some((OAK_PLANKS, 1)), None, Some((ITEM_SPRUCE_PLANKS, 1)), None];
        assert_eq!(match_recipe(grid).unwrap().name, "stick");
    }

    /// 格外有货就不中（原版口径）：多的不是数量，而是位置/格数。
    #[test]
    fn extra_or_missing_items_never_match() {
        assert!(match_recipe([None; GRID_SIZE]).is_none());
        // 面包要 3 格小麦横排：单格 3 个、两格都不成立（每格只提供 1 单位）。
        assert!(match_recipe([Some((WHEAT, 3)), None, None, None]).is_none());
        assert!(match_recipe([Some((WHEAT, 1)), Some((WHEAT, 1)), None, None]).is_none());
        // 多一个无关物品 → 原木配方不成立（避免白送木板）。
        assert!(match_recipe([Some((OAK_LOG, 1)), Some((STICK, 1)), None, None]).is_none());
        // 竖列木棍格外多一块 → 木棍和压力板都不成立。
        assert!(match_recipe([Some((OAK_PLANKS, 1)), None, Some((OAK_PLANKS, 1)), Some((OAK_PLANKS, 1))]).is_none());
        // 火把列旁边多一块煤 → 不成立。
        assert!(match_recipe([Some((COAL, 1)), Some((COAL, 1)), Some((STICK, 1)), None]).is_none());
    }

    // ---- P41：扩充配方的回归测试（下面 4 个旧测试保持原样） ----

    /// P60：3×3 同一张表走形状口径（行序 idx = y*3+x）。
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
        // P60：2×2 单格 9 小麦不再命中（shapeless 要 9 个占用格，原版口径）。
        let mut g4 = [None; GRID_SIZE];
        g4[0] = Some((WHEAT, 9));
        assert!(match_recipe(g4).is_none(), "single stacked slot is not 9 units");

        // 门 = 两列 3 木板（问题 2 的核心：6 块散放不出门）。
        let mut door = [None; GRID9_SIZE];
        for i in [0, 1, 3, 4, 6, 7] {
            door[i] = Some((OAK_PLANKS, 1));
        }
        assert_eq!(match_recipe_9(door).unwrap().name, "oak_door");
        // 同样 6 块摆成两整行 → 活板门（3×2），不是门。
        let mut trap = [None; GRID9_SIZE];
        for i in 0..6 {
            trap[i] = Some((OAK_PLANKS, 1));
        }
        assert_eq!(match_recipe_9(trap).unwrap().name, "oak_trapdoor");
        // 6 块散放（对角 + 边）→ 什么都不出。
        let mut scatter = [None; GRID9_SIZE];
        for i in [0, 2, 3, 5, 6, 8] {
            scatter[i] = Some((OAK_PLANKS, 1));
        }
        assert!(match_recipe_9(scatter).is_none(), "scattered planks must not craft a door");
        // 台阶 = 1+2+3 台阶形（含镜像）；3+3 两列是门不是台阶。
        let mut stairs = [None; GRID9_SIZE];
        for i in [0, 3, 4, 6, 7, 8] {
            stairs[i] = Some((OAK_PLANKS, 1));
        }
        assert_eq!(match_recipe_9(stairs).unwrap().name, "oak_stairs");
        let mut stairs_m = [None; GRID9_SIZE];
        for i in [2, 4, 5, 6, 7, 8] {
            stairs_m[i] = Some((OAK_PLANKS, 1));
        }
        assert_eq!(match_recipe_9(stairs_m).unwrap().name, "oak_stairs");
        // 铁镐 = 顶行 3 铁 + 中下两棍（tag 料，混合其他木棍不行；格外多一块就不中）。
        let mut pick = [None; GRID9_SIZE];
        for i in [0, 1, 2] {
            pick[i] = Some((ITEM_IRON_INGOT, 1));
        }
        pick[4] = Some((STICK, 1));
        pick[7] = Some((STICK, 1));
        let r = match_recipe_9(pick).expect("iron pickaxe shape");
        assert_eq!(r.name, "iron_pickaxe");
        assert!(consume_grid9(&mut pick, r));
        assert!(pick.iter().all(|s| s.is_none()));
        // 镐形太大，2×2 放不下（原版一致：工具须去工作台）。
        let small = [Some((ITEM_IRON_INGOT, 1)), Some((ITEM_IRON_INGOT, 1)), Some((STICK, 1)), Some((STICK, 1))];
        assert!(match_recipe(small).is_none(), "pickaxe does not fit 2x2");
    }

    /// 每条配方的每个输入/输出 ID 都必须可在生成表中解析；
    /// 同时锁定若干关键 ID 的实测值，防 codegen 漂移。
    /// P60 加形状 sanity：shaped 有去边非空 cells（≤3×3），shapeless cells 为空且单位 ≤9。
    #[test]
    fn p41_all_recipe_ids_resolve_in_generated_table() {
        let mut seen = std::collections::BTreeSet::new();
        // P59 全量表：1185 条 vanilla（data/777/full/recipes 生成，scripts/gen_recipes.py 复核）。
        assert_eq!(RECIPES.len(), 1185, "P60 配方总数（vanilla 26.3 全量，tag 原生，带形状）");
        for recipe in RECIPES {
            assert!(!recipe.needs.is_empty(), "empty needs in {}", recipe.name);
            if recipe.shaped {
                assert!(!recipe.cells.is_empty(), "shaped {} has no cells", recipe.name);
                for (x, y, _) in recipe.cells {
                    assert!(*x < 3 && *y < 3, "{} cell out of 3x3", recipe.name);
                }
            } else {
                assert!(recipe.cells.is_empty(), "shapeless {} must not carry cells", recipe.name);
                let units: i32 = recipe.needs.iter().map(|(_, c)| *c).sum();
                assert!(units <= 9, "shapeless {} needs {units} units > 9 slots", recipe.name);
            }
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

    /// P60 形状审计：每条配方造一个"教科书"格子（origin 放置，tag 取首成员；
    /// shapeless 取前 k 格按单位顺序），断言命中自己；若被别条偷家，必须是锁定的
    /// 已知表序赢家（vanilla 同形冲突同样按配方序），且重复两次一致。
    #[test]
    fn p60_canonical_grids_match_their_own_recipe_or_known_steal() {
        fn concrete(id: i32) -> i32 {
            if id >= 0 {
                id
            } else {
                tag_members(id).first().copied().unwrap_or(id)
            }
        }
        // 已知偷家（grid 属于 A 却命中 B）：逐条核对过 vanilla 真值，表序赢家。
        // 当前为空：跑出来若有新增，必须先核对形状/材料再锁定，禁止直接加。
        let known_steals: &[(&str, &str)] = &[];
        let mut found: Vec<(&str, &str)> = Vec::new();
        for recipe in RECIPES {
            let mut grid = [None; GRID9_SIZE];
            if recipe.shaped {
                for (x, y, need) in recipe.cells {
                    grid[*y as usize * 3 + *x as usize] = Some((concrete(*need), 1));
                }
            } else {
                let mut i = 0;
                for (id, n) in recipe.needs {
                    for _ in 0..*n {
                        grid[i] = Some((concrete(*id), 1));
                        i += 1;
                    }
                }
            }
            let a = match_recipe_9(grid).map(|r| r.name);
            let b = match_recipe_9(grid).map(|r| r.name);
            assert_eq!(a, b, "match must be deterministic for {}", recipe.name);
            match a {
                Some(winner) if winner != recipe.name => {
                    found.push((recipe.name, winner));
                }
                None => panic!("canonical grid of {} matches nothing", recipe.name),
                _ => {}
            }
        }
        assert_eq!(found, known_steals, "unexpected steals: {found:?}");
    }

    /// P60 代表性配方：3×3 教科书摆法命中、扣料每格 1 个；2×2 只放得下小形状。
    #[test]
    fn p41_representative_new_recipes_match_and_consume() {
        // 箱子 = 3×3 木框（中空）；扣完 8 格各剩 1 个（每格只扣 1）。
        let mut grid = [None; GRID9_SIZE];
        for i in [0, 1, 2, 3, 5, 6, 7, 8] {
            grid[i] = Some((OAK_PLANKS, 2));
        }
        let r = match_recipe_9(grid).expect("chest ring");
        assert_eq!(r.name, "chest");
        assert_eq!(r.result, (ITEM_CHEST, 1));
        assert!(consume_grid9(&mut grid, r));
        assert_eq!(
            grid,
            [
                Some((OAK_PLANKS, 1)),
                Some((OAK_PLANKS, 1)),
                Some((OAK_PLANKS, 1)),
                Some((OAK_PLANKS, 1)),
                None,
                Some((OAK_PLANKS, 1)),
                Some((OAK_PLANKS, 1)),
                Some((OAK_PLANKS, 1)),
                Some((OAK_PLANKS, 1)),
            ],
            "each ring slot loses exactly one"
        );

        // 红床 = 顶行 3 羊毛 + 次行 3 木板（3×2，非 2×2 口径）。
        let mut grid = [None; GRID9_SIZE];
        for i in [0, 1, 2] {
            grid[i] = Some((ITEM_RED_WOOL, 1));
        }
        for i in [3, 4, 5] {
            grid[i] = Some((OAK_PLANKS, 1));
        }
        let r = match_recipe_9(grid).expect("red bed");
        assert_eq!(r.name, "red_bed");
        assert_eq!(r.result, (ITEM_RED_BED, 1));
        assert!(consume_grid9(&mut grid, r));
        assert!(grid.iter().all(|s| s.is_none()));

        // 2×2 放得下：单格甘蔗是糖（1 甘蔗 shapeless），不是纸；
        // 纸要 3 格宽横排，2×2 放不下。
        assert_eq!(
            match_recipe([Some((ITEM_SUGAR_CANE, 3)), None, None, None]).unwrap().name,
            "sugar_from_sugar_cane"
        );
        let row = [Some((ITEM_SUGAR_CANE, 1)), Some((ITEM_SUGAR_CANE, 1)), None, None];
        assert!(match_recipe(row).is_none(), "paper needs a 3-wide row");
        let mut row9 = [None; GRID9_SIZE];
        row9[3] = Some((ITEM_SUGAR_CANE, 4));
        row9[4] = Some((ITEM_SUGAR_CANE, 1));
        row9[5] = Some((ITEM_SUGAR_CANE, 2));
        assert_eq!(match_recipe_9(row9).unwrap().name, "paper");
        // 格外多放一根 → 纸不成立。
        row9[0] = Some((ITEM_SUGAR_CANE, 1));
        assert!(match_recipe_9(row9).is_none());

        // 头盔 U 形 vs 矿车 n 形：同 5 铁，不同形状，各归各位。
        let mut helmet = [None; GRID9_SIZE];
        for i in [0, 1, 2, 3, 5] {
            helmet[i] = Some((ITEM_IRON_INGOT, 1));
        }
        assert_eq!(match_recipe_9(helmet).unwrap().name, "iron_helmet");
        let mut cart = [None; GRID9_SIZE];
        for i in [0, 2, 3, 4, 5] {
            cart[i] = Some((ITEM_IRON_INGOT, 1));
        }
        assert_eq!(match_recipe_9(cart).unwrap().name, "minecart");
    }

    /// P60 确定性：同输入永远同输出（表序首中），重复调用不变。
    #[test]
    fn p41_ambiguous_families_are_deterministic() {
        // 门 2 列 vs 台阶形：同 6 木板不同形状，各归各位，重复一致。
        let door = [
            Some((OAK_PLANKS, 1)),
            Some((OAK_PLANKS, 1)),
            None,
            Some((OAK_PLANKS, 1)),
            Some((OAK_PLANKS, 1)),
            None,
            Some((OAK_PLANKS, 1)),
            Some((OAK_PLANKS, 1)),
            None,
        ];
        assert_eq!(match_recipe_9(door).unwrap().name, "oak_door");
        assert_eq!(match_recipe_9(door).unwrap().name, "oak_door", "must be repeatable");
        // 头盔 vs 矿车同上。
        let helmet = [
            Some((ITEM_IRON_INGOT, 1)),
            Some((ITEM_IRON_INGOT, 1)),
            Some((ITEM_IRON_INGOT, 1)),
            Some((ITEM_IRON_INGOT, 1)),
            None,
            Some((ITEM_IRON_INGOT, 1)),
            None,
            None,
            None,
        ];
        assert_eq!(match_recipe_9(helmet).unwrap().name, "iron_helmet");
        assert_eq!(match_recipe_9(helmet).unwrap().name, "iron_helmet", "must be repeatable");
    }

    /// P56/P60：2×2 与 3×3 共用同一张配方表（同一 recipe / 同一 ID），不得分化。
    #[test]
    fn p56_both_grids_share_one_recipe_table() {
        // 火把列：2×2 左列与 3×3 左列命中同一条配方、同一结果。
        let g4 = [Some((COAL, 1)), None, Some((STICK, 1)), None];
        let mut g9 = [None; GRID9_SIZE];
        g9[3] = Some((COAL, 1));
        g9[6] = Some((STICK, 1));
        let r4 = match_recipe(g4).expect("2×2 torch");
        let r9 = match_recipe_9(g9).expect("3×3 torch");
        assert_eq!(r4.name, "torch");
        assert_eq!(r9.name, r4.name);
        assert_eq!(r4.result, r9.result);
        assert_eq!(r4.needs, r9.needs);
        // 原木→木板：单格在两种网格里都命中同一条。
        let r4 = match_recipe([Some((OAK_LOG, 1)), None, None, None]).expect("2x2 planks");
        let mut g9 = [None; GRID9_SIZE];
        g9[8] = Some((OAK_LOG, 1));
        let r9 = match_recipe_9(g9).expect("3x3 planks");
        assert_eq!(r4.name, r9.name);
    }
}
