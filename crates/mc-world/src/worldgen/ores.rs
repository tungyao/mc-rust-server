//! P44 v2 矿物：按高度带补齐 copper/gold/redstone/diamond/emerald/lapis。
//!
//! 纯函数：随机性只来自 `(seed, x, y, z, salt)` 的 `hash01`，不读邻居 chunk、
//! 不依赖生成顺序，因此 rayon 并行与 `ChunkCache` 缓存都安全。
//!
//! v1 的 coal/iron 逻辑原封不动留在 `mod.rs::ore_at`，本模块只在 v2 调用。
//! 所有方块 ID 由 `crate::registry_id` 从 `data/777/blocks.json` 查表得到。

use super::noise::hash01;
use super::ORE_SALT;

/// 一条矿物高度带：`per_mille` 是选中方块（预筛后）的千分率。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OreBand {
    pub id: u16,
    pub min_y: i32,
    pub max_y: i32,
    pub per_mille: u32,
}

/// 1/8 坐标预筛（沿用 v1 技巧），阈值在选中方块内放大 8 倍以保持总体概率。
const ORE_SELECT_ONE_IN: u32 = 8;

pub(super) const COAL: OreBand = OreBand {
    id: crate::COAL_ORE_BLOCK_ID,
    min_y: 0,
    max_y: 80,
    per_mille: 12,
};
pub(super) const COPPER: OreBand = OreBand {
    id: crate::COPPER_ORE_BLOCK_ID,
    min_y: -16,
    max_y: 48,
    per_mille: 8,
};
pub(super) const IRON: OreBand = OreBand {
    id: crate::IRON_ORE_BLOCK_ID,
    min_y: -40,
    max_y: 56,
    per_mille: 10,
};
pub(super) const GOLD: OreBand = OreBand {
    id: crate::GOLD_ORE_BLOCK_ID,
    min_y: -64,
    max_y: -8,
    per_mille: 6,
};
pub(super) const REDSTONE: OreBand = OreBand {
    id: crate::REDSTONE_ORE_BLOCK_ID,
    min_y: -64,
    max_y: 16,
    per_mille: 8,
};
pub(super) const LAPIS: OreBand = OreBand {
    id: crate::LAPIS_ORE_BLOCK_ID,
    min_y: -64,
    max_y: -24,
    per_mille: 4,
};
pub(super) const DIAMOND: OreBand = OreBand {
    id: crate::DIAMOND_ORE_BLOCK_ID,
    min_y: -64,
    max_y: -48,
    per_mille: 4,
};
/// 绿宝石用窄带 + 低概率近似山地矿（P1 没有 biome 子格，按全图稀有点缀）。
pub(super) const EMERALD: OreBand = OreBand {
    id: crate::EMERALD_ORE_BLOCK_ID,
    min_y: -16,
    max_y: 32,
    per_mille: 2,
};

/// 判定顺序固定，保证同 seed 同方块结果唯一。
pub(super) const BANDS: [OreBand; 8] =
    [COAL, COPPER, IRON, GOLD, REDSTONE, LAPIS, DIAMOND, EMERALD];

/// v2 矿：只对 stone 调用。返回 `Some(id)` 表示该石头应替换为矿。
pub(super) fn ore_at(seed: i64, x: i32, y: i32, z: i32) -> Option<u16> {
    let select = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA77)
        ^ (z as u32).wrapping_mul(0xC2B2_AE3D);
    if select & 7 != 0 {
        return None;
    }
    let roll = (hash01(seed, x as i64, y as i64, z as i64, ORE_SALT) * 1000.0) as u32;
    let mut cumulative = 0u32;
    for band in BANDS {
        if y < band.min_y || y > band.max_y {
            continue;
        }
        cumulative += band.per_mille * ORE_SELECT_ONE_IN;
        if roll < cumulative {
            return Some(band.id);
        }
    }
    None
}

/// 按方块 ID 反查高度带（测试断言用）。
#[cfg(test)]
pub(super) fn band_for_id(id: u16) -> Option<&'static OreBand> {
    BANDS.iter().find(|band| band.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_are_ordered_and_bounded() {
        for band in BANDS {
            assert!(band.min_y <= band.max_y, "{} bad band", band.name());
            assert!(band.min_y >= crate::MIN_Y, "{} below world", band.name());
            assert!(band.max_y <= crate::worldgen::MAX_SURFACE_Y, "{} above surface", band.name());
            assert!(band.per_mille > 0 && band.per_mille < 1000);
            assert_eq!(band.id, mc_registry::block_id(band.name()).unwrap() as u16);
        }
        let unique: std::collections::HashSet<u16> = BANDS.iter().map(|b| b.id).collect();
        assert_eq!(unique.len(), BANDS.len(), "ore IDs must be unique");
    }

    #[test]
    fn v2_ore_is_deterministic_and_respects_bands() {
        for (x, y, z) in [
            (0, 0, 0),
            (-15, -50, 300),
            (1_800_000, 20, -900_000),
            (369, -63, 381),
        ] {
            assert_eq!(ore_at(42, x, y, z), ore_at(42, x, y, z));
            if let Some(id) = ore_at(42, x, y, z) {
                let band = band_for_id(id).unwrap();
                assert!((band.min_y..=band.max_y).contains(&y));
            }
        }
    }

    impl OreBand {
        fn name(&self) -> &'static str {
            match self.id {
                crate::COAL_ORE_BLOCK_ID => "minecraft:coal_ore",
                crate::COPPER_ORE_BLOCK_ID => "minecraft:copper_ore",
                crate::IRON_ORE_BLOCK_ID => "minecraft:iron_ore",
                crate::GOLD_ORE_BLOCK_ID => "minecraft:gold_ore",
                crate::REDSTONE_ORE_BLOCK_ID => "minecraft:redstone_ore",
                crate::LAPIS_ORE_BLOCK_ID => "minecraft:lapis_ore",
                crate::DIAMOND_ORE_BLOCK_ID => "minecraft:diamond_ore",
                crate::EMERALD_ORE_BLOCK_ID => "minecraft:emerald_ore",
                _ => "unknown ore",
            }
        }
    }
}
