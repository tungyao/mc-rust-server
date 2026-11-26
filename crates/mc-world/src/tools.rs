//! tools: P47 工具档位 / 时运 / 精准采集的纯逻辑（26.3/777，无随机状态）。
//!
//! 所有方块/物品 ID 都从 `mc_registry` 由 `data/777/*.json` 生成的静态表编译期查得，
//! 运行时诊断（后台 `/api/tools`）也只查同一张表，绝不手写/猜 ID。
//!
//! **MVP 边界**：本服没有挖掘计时模型，因此不引入耐久、挖掘速度、工具消耗；
//! 只做「手持档位够不够 → 有没有掉落」+「时运额外掉落」+「精准采集掉自身物品」。
//! 附魔不是从 Slot 的 data component 解码（那需要注册表驱动解码，猜字节会错位 TCP 流），
//! 而是由网络层按连接维护的 `ToolEnchant` 开关（管理面板可设，默认无）传入。

use crate::{drops::hash_pos, registry_id, Drop};

/// 档位序号：徒手 0，木 1，石 2，铜 3，铁 4，钻石 5，下界合金 6（26.3 铜在石/铁之间）。
pub const TIER_HAND: u8 = 0;
pub const TIER_WOOD: u8 = 1;
pub const TIER_STONE: u8 = 2;
pub const TIER_COPPER: u8 = 3;
pub const TIER_IRON: u8 = 4;
pub const TIER_DIAMOND: u8 = 5;
pub const TIER_NETHERITE: u8 = 6;

/// 对外展示的档位名（与序号一一对应，后台 `/api/tools` 直接返回）。
pub const TIER_NAMES: [&str; 7] = ["hand", "wood", "stone", "copper", "iron", "diamond", "netherite"];

/// 附魔开关（服务端权威的连接态，不是 Slot NBT）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolEnchant {
    #[default]
    None,
    Fortune,
    SilkTouch,
}

impl ToolEnchant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fortune => "fortune",
            Self::SilkTouch => "silk_touch",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "fortune" => Some(Self::Fortune),
            "silk" | "silk_touch" | "silktouch" => Some(Self::SilkTouch),
            _ => None,
        }
    }
}

/// 镐子表：物品 ID 与档位都来自生成表（`data/777/items.json`）。
#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    pub item_id: i32,
    pub tier: u8,
    pub name: &'static str,
}

macro_rules! pickaxe {
    ($name:literal, $tier:expr) => {
        ToolSpec {
            item_id: registry_id(mc_registry::ITEMS, concat!("minecraft:", $name)) as i32,
            tier: $tier,
            name: $name,
        }
    };
}

pub const PICKAXES: &[ToolSpec] = &[
    pickaxe!("wooden_pickaxe", TIER_WOOD),
    pickaxe!("stone_pickaxe", TIER_STONE),
    pickaxe!("copper_pickaxe", TIER_COPPER),
    pickaxe!("iron_pickaxe", TIER_IRON),
    pickaxe!("diamond_pickaxe", TIER_DIAMOND),
    pickaxe!("netherite_pickaxe", TIER_NETHERITE),
];

/// 手持物品 → 镐子档位；非镐子（含空手）返回 `TIER_HAND`。
pub fn pickaxe_tier(item_id: Option<i32>) -> u8 {
    let Some(id) = item_id else { return TIER_HAND };
    PICKAXES.iter().find(|t| t.item_id == id).map(|t| t.tier).unwrap_or(TIER_HAND)
}

/// 一条挖矿规则：方块 → 最低档位 + 基础掉落物品 + 是否吃时运。
#[derive(Debug, Clone, Copy)]
pub struct MiningRule {
    pub block_id: u16,
    pub block_name: &'static str,
    pub tier: u8,
    pub item_id: i32,
    pub fortune: bool,
}

macro_rules! ore {
    ($block:literal, $tier:expr, $item:literal, $fortune:expr) => {
        MiningRule {
            block_id: registry_id(mc_registry::BLOCKS, concat!("minecraft:", $block)) as u16,
            block_name: concat!("minecraft:", $block),
            tier: $tier,
            item_id: registry_id(mc_registry::ITEMS, concat!("minecraft:", $item)) as i32,
            fortune: $fortune,
        }
    };
}

/// 需要镐子的方块/矿石表；其余方块（土/沙/木/叶/作物…）徒手可挖，沿用旧掉落表。
/// 深层矿石与普通矿石同档位（26.3 沿用同一材料门槛），只分别列条目便于后台查表。
pub const MINING_RULES: &[MiningRule] = &[
    ore!("stone", TIER_WOOD, "cobblestone", false),
    ore!("coal_ore", TIER_WOOD, "coal", true),
    ore!("deepslate_coal_ore", TIER_WOOD, "coal", true),
    ore!("iron_ore", TIER_STONE, "raw_iron", true),
    ore!("deepslate_iron_ore", TIER_STONE, "raw_iron", true),
    ore!("copper_ore", TIER_STONE, "raw_copper", true),
    ore!("deepslate_copper_ore", TIER_STONE, "raw_copper", true),
    ore!("diamond_ore", TIER_IRON, "diamond", true),
    ore!("deepslate_diamond_ore", TIER_IRON, "diamond", true),
    ore!("ancient_debris", TIER_DIAMOND, "netherite_scrap", false),
    ore!("obsidian", TIER_NETHERITE, "obsidian", false),
];

/// 按方块 ID 查挖矿规则（blockstate ID 来自生成表，多状态方块只列默认态）。
pub fn mining_rule(block_id: u16) -> Option<&'static MiningRule> {
    MINING_RULES.iter().find(|r| r.block_id == block_id)
}

/// 档位是否够挖该方块（不在表里 = 徒手可挖，恒允许）。
pub fn can_mine(block_id: u16, tier: u8) -> bool {
    mining_rule(block_id).map(|r| tier >= r.tier).unwrap_or(true)
}

/// 精准采集：按 blockstate 的注册名查同名物品（`data/777/blocks.json` + `items.json`），
/// 没有对应物品（例如某些多状态方块）时返回 `None`，调用方回退原掉落表。
pub fn silk_item_for_block(block_id: u16) -> Option<i32> {
    // 空气/水不是可挖方块，即便注册表里有 `minecraft:air` 物品也不能掉。
    if block_id == crate::AIR_BLOCK_ID || block_id == crate::WATER_BLOCK_ID {
        return None;
    }
    let name = mc_registry::block_name(block_id as u32)?;
    match mc_registry::item_id(name) {
        Some(id) if id > 0 => Some(id as i32),
        _ => None,
    }
}

/// 时运额外掉落：确定性 hash（`world seed ^ FORTUNE_SALT` + 坐标），不用全局随机、
/// 不扰动原版草方块种子/树叶分支的 hash。矿石本体 1 个，额外 0~2 个（3/8 概率 +1，
/// 1/8 概率 +2），与原版 fortune I 的量级一致；只在吃时运的规则上生效。
pub fn fortune_extra(seed: i64, x: i32, y: i32, z: i32) -> i32 {
    match hash_pos(seed ^ 0x464f5254554e45, x, y, z) % 8 {
        0 | 1 | 2 | 3 => 1,
        4 => 2,
        _ => 0,
    }
}

/// 工具口径的方块掉落：先查档位（不够 = 空掉落），再查精准采集（有对应物品则掉自身），
/// 再查时运（矿石额外数量），最后回退 [`crate::drops_for_block`]（旧表逐字节不变）。
pub fn drops_for_block_with_tool(
    block_id: u16,
    x: i32,
    y: i32,
    z: i32,
    seed: i64,
    held_item: Option<i32>,
    enchant: ToolEnchant,
) -> Vec<Drop> {
    let tier = pickaxe_tier(held_item);
    if !can_mine(block_id, tier) {
        return Vec::new();
    }
    if enchant == ToolEnchant::SilkTouch {
        if let Some(item_id) = silk_item_for_block(block_id) {
            return vec![Drop { item_id, count: 1 }];
        }
    }
    let mut out = crate::drops_for_block(block_id, x, y, z, seed);
    if enchant == ToolEnchant::Fortune {
        if let Some(rule) = mining_rule(block_id) {
            if rule.fortune {
                if let Some(drop) = out.iter_mut().find(|d| d.item_id == rule.item_id) {
                    drop.count += fortune_extra(seed, x, y, z);
                }
            }
        }
    }
    out
}

/// 后台 `/api/tools` 的档位行（名字 + 物品 ID）。
pub fn tool_catalog() -> Vec<ToolSpec> {
    PICKAXES.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_come_from_generated_tables() {
        for (name, expected_id, spec) in [
            ("minecraft:wooden_pickaxe", 1027, PICKAXES[0]),
            ("minecraft:stone_pickaxe", 1037, PICKAXES[1]),
            ("minecraft:copper_pickaxe", 1032, PICKAXES[2]),
            ("minecraft:iron_pickaxe", 1047, PICKAXES[3]),
            ("minecraft:diamond_pickaxe", 1052, PICKAXES[4]),
            ("minecraft:netherite_pickaxe", 1057, PICKAXES[5]),
        ] {
            assert_eq!(spec.item_id, expected_id, "wrong id for {name}");
            assert_eq!(mc_registry::item_id(name), Some(expected_id as u32));
        }
        assert_eq!(mc_registry::block_id("minecraft:coal_ore"), Some(136));
        assert_eq!(mc_registry::block_id("minecraft:diamond_ore"), Some(6863));
        assert_eq!(mc_registry::block_id("minecraft:ancient_debris"), Some(25176));
        assert_eq!(mc_registry::block_id("minecraft:obsidian"), Some(4925));
    }

    #[test]
    fn tier_gate_blocks_low_tier_drops() {
        let wood = PICKAXES[0].item_id;
        let stone = PICKAXES[1].item_id;
        let iron = PICKAXES[3].item_id;
        let diamond = PICKAXES[4].item_id;
        let netherite = PICKAXES[5].item_id;
        let coal = mc_registry::block_id("minecraft:coal_ore").unwrap() as u16;
        let iron_ore = mc_registry::block_id("minecraft:iron_ore").unwrap() as u16;
        let dia = mc_registry::block_id("minecraft:diamond_ore").unwrap() as u16;
        let debris = mc_registry::block_id("minecraft:ancient_debris").unwrap() as u16;
        let obsidian = mc_registry::block_id("minecraft:obsidian").unwrap() as u16;
        assert_eq!(pickaxe_tier(None), TIER_HAND);
        assert_eq!(pickaxe_tier(Some(wood)), TIER_WOOD);
        assert_eq!(pickaxe_tier(Some(diamond)), TIER_DIAMOND);
        assert!(can_mine(coal, TIER_WOOD));
        assert!(!can_mine(iron_ore, TIER_WOOD));
        assert!(can_mine(iron_ore, TIER_STONE));
        assert!(!can_mine(dia, TIER_STONE));
        assert!(can_mine(dia, TIER_IRON));
        assert!(!can_mine(debris, TIER_IRON));
        assert!(can_mine(debris, TIER_DIAMOND));
        assert!(!can_mine(obsidian, TIER_DIAMOND));
        assert!(can_mine(obsidian, TIER_NETHERITE));
        assert_eq!(
            drops_for_block_with_tool(iron_ore, 0, 10, 0, 1, Some(wood), ToolEnchant::None),
            Vec::new(),
            "low tier still breaks but drops nothing"
        );
        assert_eq!(
            drops_for_block_with_tool(iron_ore, 0, 10, 0, 1, Some(stone), ToolEnchant::None),
            vec![Drop { item_id: mc_registry::item_id("minecraft:raw_iron").unwrap() as i32, count: 1 }]
        );
        let _ = (iron, netherite);
    }

    #[test]
    fn fortune_is_deterministic_and_only_for_ores() {
        let coal = mc_registry::block_id("minecraft:coal_ore").unwrap() as u16;
        let wood = PICKAXES[0].item_id;
        let mut saw_extra = false;
        let mut saw_plain = false;
        for x in 0..64 {
            let d = drops_for_block_with_tool(coal, x, 40, 0, 777, Some(wood), ToolEnchant::Fortune);
            assert_eq!(d.len(), 1);
            assert!(d[0].count >= 1 && d[0].count <= 3, "base 1 + 0..2 bonus");
            assert_eq!(
                d,
                drops_for_block_with_tool(coal, x, 40, 0, 777, Some(wood), ToolEnchant::Fortune),
                "same seed/pos must be identical"
            );
            if d[0].count > 1 { saw_extra = true } else { saw_plain = true }
        }
        assert!(saw_extra && saw_plain, "both fortune branches must exist");
        let stone = crate::STONE_BLOCK_ID;
        assert_eq!(
            drops_for_block_with_tool(stone, 0, 60, 0, 777, Some(wood), ToolEnchant::Fortune),
            crate::drops_for_block(stone, 0, 60, 0, 777),
            "non-ore must not be affected by fortune"
        );
    }

    #[test]
    fn silk_touch_drops_the_block_item_or_falls_back() {
        let coal = mc_registry::block_id("minecraft:coal_ore").unwrap() as u16;
        let wood = PICKAXES[0].item_id;
        assert_eq!(
            drops_for_block_with_tool(coal, 0, 40, 0, 777, Some(wood), ToolEnchant::SilkTouch),
            vec![Drop { item_id: mc_registry::item_id("minecraft:coal_ore").unwrap() as i32, count: 1 }]
        );
        // 无对应物品的方块（例如空气）回退旧表（空掉落）。
        assert!(drops_for_block_with_tool(
            crate::AIR_BLOCK_ID, 0, 64, 0, 777, Some(wood), ToolEnchant::SilkTouch
        ).is_empty());
        assert_eq!(ToolEnchant::parse("silk_touch"), Some(ToolEnchant::SilkTouch));
        assert_eq!(ToolEnchant::parse("fortune"), Some(ToolEnchant::Fortune));
        assert_eq!(ToolEnchant::None.as_str(), "none");
    }
}
