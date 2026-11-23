//! mc-registry: 方块/物品/生物群系注册表
//! 只兼容 26.3 (protocol 777, data 5023)。
//! 方块表由 build.rs 在编译期从硬编码占位表生成（离线可构建），
//! P2 再切 build.rs 从 minecraft-data 生成全量表。
use std::collections::HashMap;

// ---- 26.3 版本常量透传（以 mc-core 为唯一来源） ----
pub use mc_core::{DATA_VERSION, MC_VERSION, PROTOCOL_VERSION};
/// 别名：兼容 `PROTOCOL` / `DATA` 短名断言。
pub const PROTOCOL: i32 = mc_core::PROTOCOL_VERSION;
/// 别名：兼容 `DATA` 短名断言。
pub const DATA: i32 = mc_core::DATA_VERSION;

// ---- 编译期生成的 26.3 方块表占位 ----
mod generated {
    include!(concat!(env!("OUT_DIR"), "/blocks_generated.rs"));
}

/// 26.3 方块表：`&[(block_id, namespaced_name)]`，由 build.rs 生成。
pub use generated::{BLOCKS, by_name};

#[derive(Debug, Clone)]
pub struct BlockDef {
    pub id: u32,
    pub name: &'static str,
}

/// 旧 API（兼容保留）：只返回最初 3 个条目。
pub fn vanilla_blocks() -> HashMap<u32, BlockDef> {
    let mut m = HashMap::new();
    m.insert(0, BlockDef { id: 0, name: "minecraft:air" });
    m.insert(1, BlockDef { id: 1, name: "minecraft:stone" });
    m.insert(2, BlockDef { id: 2, name: "minecraft:grass_block" });
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
    fn generated_table_has_30_entries() {
        assert_eq!(BLOCKS.len(), 30);
        // 任务点名的几个常见方块必须存在
        for name in [
            "minecraft:air",
            "minecraft:stone",
            "minecraft:grass_block",
            "minecraft:dirt",
            "minecraft:oak_log",
        ] {
            assert!(by_name(name).is_some(), "missing {name}");
            assert!(block_id(name).is_some(), "missing {name}");
        }
    }
}
