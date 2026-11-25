//! admin_items: P29 管理面板物品可视化——中英对照 + 分类，只兼容 26.3/777。
//!
//! 中文名来源：人工翻译、非原版 assets（Mojang 官方语言文件未随服务端分发）。
//! 对照表只覆盖常用物品（本服实际产出的全部 + 面板可发放的常用项，共 84 项）；
//! 非常用物品不清退（全量 1658 项仍可搜），回退显示注册名、分类归“其他”。

use std::sync::OnceLock;

/// （短名，中文名，分类）：短名即注册名去 `minecraft:` 前缀。
/// 分类仅用于管理面板过滤：方块 / 农作物 / 材料 / 食物 / 工具 / 功能。
const ITEM_ZH: &[(&str, &str, &str)] = &[
    // 方块（含本服掉落：石头/泥土/圆石/沙子/原木/树叶/树苗 + 可放置常用块）。
    ("stone", "石头", "方块"),
    ("dirt", "泥土", "方块"),
    ("cobblestone", "圆石", "方块"),
    ("sand", "沙子", "方块"),
    ("grass_block", "草方块", "方块"),
    ("oak_log", "橡木原木", "方块"),
    ("oak_leaves", "橡树树叶", "方块"),
    ("oak_sapling", "橡树树苗", "方块"),
    ("oak_planks", "橡木木板", "方块"),
    ("torch", "火把", "方块"),
    ("crafting_table", "工作台", "方块"),
    ("furnace", "熔炉", "方块"),
    ("chest", "箱子", "方块"),
    ("glass", "玻璃", "方块"),
    ("ladder", "梯子", "方块"),
    ("cobweb", "蜘蛛网", "方块"),
    ("vine", "藤蔓", "方块"),
    ("lily_pad", "睡莲", "方块"),
    ("cactus", "仙人掌", "方块"),
    // 农作物（含本服产出：小麦/小麦种子 + 可种植常用种子）。
    ("wheat", "小麦", "农作物"),
    ("wheat_seeds", "小麦种子", "农作物"),
    ("carrot", "胡萝卜", "农作物"),
    ("potato", "马铃薯", "农作物"),
    ("sugar_cane", "甘蔗", "农作物"),
    ("melon_seeds", "西瓜种子", "农作物"),
    ("pumpkin_seeds", "南瓜种子", "农作物"),
    ("beetroot_seeds", "甜菜种子", "农作物"),
    ("torchflower_seeds", "火炬花种子", "农作物"),
    // 材料（含本服产出：煤炭/粗铁/木棍/骨粉原料 + 常见矿物材料）。
    ("coal", "煤炭", "材料"),
    ("charcoal", "木炭", "材料"),
    ("raw_iron", "粗铁", "材料"),
    ("iron_ingot", "铁锭", "材料"),
    ("iron_nugget", "铁粒", "材料"),
    ("stick", "木棍", "材料"),
    ("bone", "骨头", "材料"),
    ("bone_meal", "骨粉", "材料"),
    ("diamond", "钻石", "材料"),
    ("emerald", "绿宝石", "材料"),
    ("gold_ingot", "金锭", "材料"),
    ("lapis_lazuli", "青金石", "材料"),
    ("redstone", "红石粉", "材料"),
    ("quartz", "石英", "材料"),
    ("flint", "燧石", "材料"),
    ("clay_ball", "黏土球", "材料"),
    ("brick", "红砖", "材料"),
    ("leather", "皮革", "材料"),
    ("feather", "羽毛", "材料"),
    ("string", "线", "材料"),
    ("sugar", "糖", "材料"),
    ("paper", "纸", "材料"),
    ("book", "书", "材料"),
    // 食物（含任务点名的苹果 + 常见可发放食物）。
    ("apple", "苹果", "食物"),
    ("bread", "面包", "食物"),
    ("baked_potato", "烤马铃薯", "食物"),
    ("melon_slice", "西瓜片", "食物"),
    ("cookie", "曲奇", "食物"),
    ("egg", "鸡蛋", "食物"),
    ("snowball", "雪球", "食物"),
    ("mushroom_stew", "蘑菇煲", "食物"),
    ("bowl", "碗", "食物"),
    // 工具（含任务点名的木/石/铁锄 + 同系列常用工具武器）。
    ("wooden_hoe", "木锄", "工具"),
    ("stone_hoe", "石锄", "工具"),
    ("iron_hoe", "铁锄", "工具"),
    ("wooden_pickaxe", "木镐", "工具"),
    ("stone_pickaxe", "石镐", "工具"),
    ("iron_pickaxe", "铁镐", "工具"),
    ("wooden_axe", "木斧", "工具"),
    ("stone_axe", "石斧", "工具"),
    ("iron_axe", "铁斧", "工具"),
    ("wooden_shovel", "木锹", "工具"),
    ("stone_shovel", "石锹", "工具"),
    ("iron_shovel", "铁锹", "工具"),
    ("wooden_sword", "木剑", "工具"),
    ("stone_sword", "石剑", "工具"),
    ("iron_sword", "铁剑", "工具"),
    ("bow", "弓", "工具"),
    ("arrow", "箭", "工具"),
    ("shears", "剪刀", "工具"),
    // 功能（容器/载具/杂项）。
    ("fishing_rod", "钓鱼竿", "功能"),
    ("bucket", "空桶", "功能"),
    ("water_bucket", "水桶", "功能"),
    ("glass_bottle", "玻璃瓶", "功能"),
    ("oak_boat", "橡木船", "功能"),
    ("carrot_on_a_stick", "胡萝卜钓竿", "功能"),
];

/// 短名 → 中文名（表外返回 `None`，调用方回退注册名）。
pub fn item_zh(short: &str) -> Option<&'static str> {
    ITEM_ZH.iter().find(|(n, _, _)| *n == short).map(|(_, zh, _)| *zh)
}

/// 短名 → 分类（表外 → “其他”）。
pub fn item_category(short: &str) -> &'static str {
    ITEM_ZH.iter().find(|(n, _, _)| *n == short).map(|(_, _, c)| *c).unwrap_or("其他")
}

/// 清单条目（`GET /api/items` 的数组元素；`name_zh` 表外时回退注册名）。
#[derive(Debug, Clone)]
pub struct ItemEntry {
    pub id: u32,
    pub name: &'static str,
    pub name_zh: &'static str,
    pub category: &'static str,
}

fn short_of(name: &'static str) -> &'static str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

static CATALOG: OnceLock<Vec<ItemEntry>> = OnceLock::new();

/// 全量清单（`mc_registry::ITEMS` 1658 项，第一次调用时生成并缓存）。
pub fn items_catalog() -> &'static [ItemEntry] {
    CATALOG
        .get_or_init(|| {
            mc_registry::ITEMS
                .iter()
                .map(|(id, name)| {
                    let (name_zh, category) = match ITEM_ZH.iter().find(|(n, _, _)| *n == short_of(name)) {
                        Some((_, zh, c)) => (*zh, *c),
                        None => (*name, "其他"),
                    };
                    ItemEntry { id: *id, name, name_zh, category }
                })
                .collect()
        })
        .as_slice()
}

static CATALOG_JSON: OnceLock<Vec<u8>> = OnceLock::new();

/// 缓存的 `/api/items` 响应体（约几十 KB，只序列化一次；在线轮询不走这里）。
pub fn items_catalog_json() -> &'static [u8] {
    CATALOG_JSON
        .get_or_init(|| {
            let items: Vec<serde_json::Value> = items_catalog()
                .iter()
                .map(|e| serde_json::json!({"id": e.id, "name": e.name, "name_zh": e.name_zh, "category": e.category}))
                .collect();
            serde_json::json!({"ok": true, "count": items.len(), "items": items}).to_string().into_bytes()
        })
        .as_slice()
}

/// 数字 id → （注册名，中文名，分类）；未知 id 返回 `None`。
pub fn item_info_by_id(id: u32) -> Option<(&'static str, &'static str, &'static str)> {
    let name = mc_registry::item_name(id)?;
    let short = short_of(name);
    match item_zh(short) {
        Some(zh) => Some((name, zh, item_category(short))),
        None => Some((name, name, "其他")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_covers_all_server_drops_and_giveables() {
        // 本服实际产出（drops.rs 全部 + 开局石头 + 旧面板 datalist）必须有中文名。
        for short in [
            "stone", "dirt", "cobblestone", "sand", "oak_log", "oak_sapling", "coal", "stick",
            "raw_iron", "wheat_seeds", "wheat",
        ] {
            assert!(item_zh(short).is_some(), "{short} must have a Chinese name");
        }
        // 任务点名：苹果 / 骨粉 / 锄头。
        assert_eq!(item_zh("apple"), Some("苹果"));
        assert_eq!(item_zh("bone_meal"), Some("骨粉"));
        assert_eq!(item_zh("wooden_hoe"), Some("木锄"));
        assert_eq!(item_zh("stone_hoe"), Some("石锄"));
        assert_eq!(item_zh("iron_hoe"), Some("铁锄"));
        assert_eq!(ITEM_ZH.len(), 84, "keep the table at 84 entries (add/remove deliberately)");
    }

    #[test]
    fn table_has_no_dup_names_and_all_names_are_real() {
        let mut seen = std::collections::HashSet::new();
        for (short, _, _) in ITEM_ZH {
            assert!(seen.insert(*short), "duplicate entry: {short}");
            assert!(
                mc_registry::item_id(short).is_some(),
                "{short} must exist in data/777/items.json"
            );
        }
    }

    #[test]
    fn unknown_item_falls_back_to_registry_name() {
        // 表外物品：中文名回退注册名，分类归“其他”。
        let (name, zh, cat) = item_info_by_id(mc_registry::item_id("minecraft:diamond_sword").unwrap()).unwrap();
        assert_eq!(name, "minecraft:diamond_sword");
        assert_eq!(zh, "minecraft:diamond_sword");
        assert_eq!(cat, "其他");
        assert!(item_info_by_id(999_999).is_none());
    }
}
