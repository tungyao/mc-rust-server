//! mc-registry: 方块/物品/实体/包 ID 注册表
//! 只兼容 26.3 (protocol 777, data 5023)。
//!
//! 全部 ID 由 `build.rs` 从 `data/manifest.json` + `data/777/*.json` 编译期生成，
//! 来源与校验见 `data/777/SOURCE.md`。旧的 216 项连续占位表已废弃。
use std::collections::HashMap;

// ---- 26.3 版本常量透传（以 mc-core 为唯一来源） ----
pub use mc_core::{DATA_VERSION, MC_VERSION, PROTOCOL_VERSION};
/// 别名：兼容 `PROTOCOL` / `DATA` 短名断言。
pub const PROTOCOL: i32 = mc_core::PROTOCOL_VERSION;
/// 别名：兼容 `DATA` 短名断言。
pub const DATA: i32 = mc_core::DATA_VERSION;

// ---- 编译期生成的真实 26.3 注册表 ----
mod generated {
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

/// 26.3 方块表：`&[(blockstate_id, namespaced_name)]`，由 build.rs 从 JSON 生成。
pub use generated::{
    BLOCKS, BLOCK_STATES, by_id as block_by_id, by_name, ENTITIES, ITEMS, PACKETS,
};

/// P37：按方块名 + 属性组合索引取真实 blockstate ID（区间起点 + 索引）。
/// 索引来自 `data/777/full/blocks.json` 的状态数组顺序，运行时查询供测试/诊断；
/// 生产代码用 `mc_world::state_index_id` 的编译期版本。
pub fn block_state_id(name: &str, index: u16) -> Option<u32> {
    BLOCK_STATES
        .iter()
        .find(|(_, n, count)| *n == name && index < *count)
        .map(|(first, _, _)| first + u32::from(index))
}

#[derive(Debug, Clone)]
pub struct BlockDef {
    pub id: u32,
    pub name: &'static str,
}

/// 旧 API（兼容保留）：仍返回最初 3 个条目，ID 为真实 blockstate ID
/// （air=0 / stone=1 / grass_block=9，由生成表提供）。
/// 新代码请使用 [`block_id`] / [`block_name`] 查询真实 blockstate ID。
pub fn vanilla_blocks() -> HashMap<u32, BlockDef> {
    let mut m = HashMap::new();
    m.insert(0, BlockDef { id: 0, name: "minecraft:air" });
    m.insert(1, BlockDef { id: 1, name: "minecraft:stone" });
    m.insert(9, BlockDef { id: 9, name: "minecraft:grass_block" });
    m
}

/// 按数字 id 查命名空间名，如 `block_name(0) == Some("minecraft:air")`。
pub fn block_name(id: u32) -> Option<&'static str> {
    BLOCKS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

/// 按名查数字 id，同时接受 `minecraft:stone` 与 `stone` 两种写法。
pub fn block_id(name: &str) -> Option<u32> {
    if let Some(id) = by_name(name) {
        return Some(id);
    }
    if !name.contains(':') {
        let full = format!("minecraft:{name}");
        if let Some(id) = by_name(&full) {
            return Some(id);
        }
    }
    None
}

/// 通用查找：同时接受 `minecraft:dirt` 与 `dirt`。
fn lookup_id(table: &[(u32, &'static str)], name: &str) -> Option<u32> {
    if let Some((id, _)) = table.iter().find(|(_, n)| *n == name) {
        return Some(*id);
    }
    if !name.contains(':') {
        let full = format!("minecraft:{name}");
        if let Some((id, _)) = table.iter().find(|(_, n)| *n == full.as_str()) {
            return Some(*id);
        }
    }
    None
}

/// 按名查物品 ID（`minecraft:dirt` / `dirt` 均可）。
pub fn item_id(name: &str) -> Option<u32> {
    lookup_id(ITEMS, name)
}

/// 按物品 ID 查名字。
pub fn item_name(id: u32) -> Option<&'static str> {
    ITEMS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

/// 按名查实体类型 ID（`minecraft:zombie` / `zombie` 均可）。
pub fn entity_id(name: &str) -> Option<u32> {
    lookup_id(ENTITIES, name)
}

/// 按实体类型 ID 查名字。
pub fn entity_name(id: u32) -> Option<&'static str> {
    ENTITIES.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

/// 按 `side` + `state` + 包名查包 ID，例如
/// `packet_id("clientbound", "play", "block_update") == Some(8)`。
pub fn packet_id(side: &str, state: &str, name: &str) -> Option<i32> {
    let key = format!("{side}.{state}");
    PACKETS
        .iter()
        .find(|(s, n, _)| *s == key.as_str() && *n == name)
        .map(|(_, _, id)| *id)
}

/// 与 [`packet_id`] 相同，但 state 直接写 `"clientbound.play"` 形式。
pub fn packet_id_in(state: &str, name: &str) -> Option<i32> {
    PACKETS
        .iter()
        .find(|(s, n, _)| *s == state && *n == name)
        .map(|(_, _, id)| *id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_name_zero_is_air() {
        assert_eq!(block_name(0), Some("minecraft:air"));
    }

    #[test]
    fn block_id_stone_exists() {
        assert!(block_id("minecraft:stone").is_some());
        // 短名写法同样可用
        assert_eq!(block_id("stone"), block_id("minecraft:stone"));
    }

    #[test]
    fn version_consts_match_26_3() {
        assert_eq!(MC_VERSION, "26.3");
        assert_eq!(PROTOCOL_VERSION, 777);
        assert_eq!(DATA_VERSION, 5023);
        // 短名别名一致
        assert_eq!(PROTOCOL, 777);
        assert_eq!(DATA, 5023);
    }

    #[test]
    fn generated_table_has_real_blockstate_ids() {
        assert!(BLOCKS.len() >= 1000, "want full 26.3 table, got {}", BLOCKS.len());
        // 任务点名的常见方块必须存在，且 ID 是真实 blockstate ID（非连续占位号）。
        for (name, id) in [
            ("minecraft:air", 0),
            ("minecraft:stone", 1),
            ("minecraft:grass_block", 9),
            ("minecraft:dirt", 10),
            ("minecraft:water", 89),
            ("minecraft:sand", 121),
            ("minecraft:coal_ore", 136),
            ("minecraft:iron_ore", 134),
            ("minecraft:oak_log", 140),
            ("minecraft:oak_leaves", 294),
        ] {
            assert_eq!(block_id(name), Some(id), "wrong blockstate ID for {name}");
            assert_eq!(by_name(name), Some(id));
            assert_eq!(block_by_id(id), Some(name));
            assert_eq!(block_name(id), Some(name));
        }
        for name in [
            "minecraft:deepslate",
            "minecraft:white_wool",
            "minecraft:redstone_lamp",
        ] {
            assert!(block_id(name).is_some(), "missing {name}");
        }
        // 不再是 0..N 的连续假 ID：grass_block 应为 9 而不是 2。
        assert_ne!(BLOCKS.get(2).map(|(id, _)| *id), Some(9));
    }

    #[test]
    fn p37_block_state_index_matches_26_3() {
        assert_eq!(block_state_id("minecraft:nether_wart", 3), Some(11184));
        assert_eq!(block_state_id("minecraft:cocoa", 11), Some(11226));
        assert_eq!(block_state_id("minecraft:bamboo", 11), Some(18647));
        assert_eq!(block_state_id("minecraft:pitcher_crop", 1), Some(16781));
        assert_eq!(block_state_id("minecraft:wheat", 8), None);
    }

    #[test]
    fn item_id_matches_26_3() {
        assert_eq!(item_id("minecraft:stone"), Some(1));
        assert_eq!(item_id("minecraft:dirt"), Some(55));
        assert_eq!(item_id("dirt"), Some(55));
        assert_eq!(item_name(55), Some("minecraft:dirt"));
        assert_eq!(ITEMS.len(), 1658);
    }

    #[test]
    fn entity_id_matches_26_3() {
        assert_eq!(entity_id("minecraft:player"), Some(159));
        assert_eq!(entity_id("zombie"), Some(154));
        assert_eq!(entity_name(159), Some("minecraft:player"));
        assert_eq!(ENTITIES.len(), 161);
    }

    #[test]
    fn packet_id_matches_26_3() {
        assert_eq!(packet_id("clientbound", "play", "block_update"), Some(8));
        assert_eq!(packet_id("clientbound", "play", "add_entity"), Some(1));
        assert_eq!(packet_id("clientbound", "play", "level_chunk_with_light"), Some(46));
        assert_eq!(packet_id("serverbound", "play", "player_action"), Some(41));
        assert_eq!(packet_id("serverbound", "play", "use_item_on"), Some(66));
        assert_eq!(packet_id_in("clientbound.play", "take_item_entity"), Some(127));
        assert!(!PACKETS.is_empty());
    }
}
