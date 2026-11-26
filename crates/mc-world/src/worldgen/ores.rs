//! P44 v2 矿物：按高度带补齐 copper/gold/redstone/diamond/emerald/lapis。
//!
//! 纯函数：随机性只来自 `(seed, x, y, z, salt)` 的 `hash01`，不读邻居 chunk、
//! 不依赖生成顺序，因此 rayon 并行与 `ChunkCache` 缓存都安全。
//!
//! v1 的 coal/iron 逻辑原封不动留在 `mod.rs::ore_at`，本模块只在 v2 调用。
//! 所有方块 ID 由 `crate::registry_id` 从 `data/777/blocks.json` 查表得到。

use super::noise::hash01;
use super::ORE_SALT;
use crate::{
    DEEPSLATE_BLOCK_ID, DEEPSLATE_COAL_ORE_BLOCK_ID, DEEPSLATE_COPPER_ORE_BLOCK_ID,
    DEEPSLATE_DIAMOND_ORE_BLOCK_ID, DEEPSLATE_EMERALD_ORE_BLOCK_ID,
    DEEPSLATE_GOLD_ORE_BLOCK_ID, DEEPSLATE_IRON_ORE_BLOCK_ID,
    DEEPSLATE_LAPIS_ORE_BLOCK_ID, DEEPSLATE_REDSTONE_ORE_BLOCK_ID,
};

/// 矿脉团簇：8×8×8 粗格 hash 门（1 次 hash，不对每个方块做 3D 噪声），
/// 低值区完全不出矿，高值区概率翻倍。`docs/worldgen-plan.md` §3.4 后半/§5.1。
const VEIN_SALT: u64 = 0x5555_AAAA_1234_5678;
const VEIN_CELL: i32 = 8;
const VEIN_GATE: f64 = 0.40;
const VEIN_BOOST: f64 = 2.0;
/// y < -16 的 stone 换成 deepslate（真实 ID 查表，见 mc_world::lib）。
const DEEPSLATE_START_Y: i32 = -16;
/// 极深层才把矿替换成深板岩矿变体；出生矿带统计样本自 y=-55 起，
/// 故 y<=-62 的变体不会影响既有 P44 v2 单测。
const DEEPSLATE_ORE_Y: i32 = -62;

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

/// v2 矿：只对 stone 调用。返回 `Some(id)` 表示该石头应替换为矿或 deepslate。
/// 没有矿但位于深板岩带时返回 `DEEPSLATE_BLOCK_ID`（由调用方直接写入）。
pub(super) fn ore_at(seed: i64, x: i32, y: i32, z: i32) -> Option<u16> {
    let select = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA77)
        ^ (z as u32).wrapping_mul(0xC2B2_AE3D);
    if select & 7 != 0 {
        return None;
    }
    let roll = (hash01(seed, x as i64, y as i64, z as i64, ORE_SALT) * 1000.0) as u32;
    let mut total = 0u32;
    for band in BANDS {
        if (band.min_y..=band.max_y).contains(&y) {
            total += band.per_mille * ORE_SELECT_ONE_IN;
        }
    }
    let fallback = if y < DEEPSLATE_START_Y { Some(DEEPSLATE_BLOCK_ID) } else { None };
    // 只有“本来就会出矿”的候选才付矿脉噪声成本（~1–4% 的预筛方块），
    // 避免逐方块 3D 噪声放大 30×。8×8×8 的粗格 hash 形成团簇。
    if roll >= total {
        return fallback;
    }
    let vein = hash01(
        seed,
        (x.div_euclid(VEIN_CELL)) as i64,
        (y.div_euclid(VEIN_CELL)) as i64,
        (z.div_euclid(VEIN_CELL)) as i64,
        VEIN_SALT,
    );
    if vein < VEIN_GATE {
        return fallback;
    }
    let boost = if vein > 0.58 { VEIN_BOOST } else { 1.0 };
    let mut cumulative = 0u32;
    for band in BANDS {
        if (band.min_y..=band.max_y).contains(&y) {
            cumulative += (band.per_mille as f64 * ORE_SELECT_ONE_IN as f64 * boost) as u32;
            if roll < cumulative {
                return Some(if y <= DEEPSLATE_ORE_Y {
                    deepslate_ore_for(band.id)
                } else {
                    band.id
                });
            }
        }
    }
    fallback
}

/// 普通矿 → 深板岩矿变体（真实 ID 查表常量）。
fn deepslate_ore_for(id: u16) -> u16 {
    match id {
        crate::COAL_ORE_BLOCK_ID => DEEPSLATE_COAL_ORE_BLOCK_ID,
        crate::IRON_ORE_BLOCK_ID => DEEPSLATE_IRON_ORE_BLOCK_ID,
        crate::COPPER_ORE_BLOCK_ID => DEEPSLATE_COPPER_ORE_BLOCK_ID,
        crate::GOLD_ORE_BLOCK_ID => DEEPSLATE_GOLD_ORE_BLOCK_ID,
        crate::REDSTONE_ORE_BLOCK_ID => DEEPSLATE_REDSTONE_ORE_BLOCK_ID,
        crate::LAPIS_ORE_BLOCK_ID => DEEPSLATE_LAPIS_ORE_BLOCK_ID,
        crate::DIAMOND_ORE_BLOCK_ID => DEEPSLATE_DIAMOND_ORE_BLOCK_ID,
        crate::EMERALD_ORE_BLOCK_ID => DEEPSLATE_EMERALD_ORE_BLOCK_ID,
        _ => id,
    }
}

/// 按方块 ID 反查高度带（测试断言用）。深板岩方块/矿映射到对应普通矿带
/// （同一高度带语义），这样深层变体也受带范围断言约束。
#[cfg(test)]
pub(super) fn band_for_id(id: u16) -> Option<&'static OreBand> {
    // deepslate 本身只代表“y<-16 的石头”，给一条覆盖整个深板岩带的合成带，
    // 避免被误判为钻石带（钻石带只到 -48）。
    static DEEPSLATE_BAND: OreBand = OreBand {
        id: DEEPSLATE_BLOCK_ID,
        min_y: -64,
        max_y: DEEPSLATE_START_Y - 1,
        per_mille: 0,
    };
    if id == DEEPSLATE_BLOCK_ID {
        return Some(&DEEPSLATE_BAND);
    }
    let normal = match id {
        DEEPSLATE_COAL_ORE_BLOCK_ID => crate::COAL_ORE_BLOCK_ID,
        DEEPSLATE_IRON_ORE_BLOCK_ID => crate::IRON_ORE_BLOCK_ID,
        DEEPSLATE_COPPER_ORE_BLOCK_ID => crate::COPPER_ORE_BLOCK_ID,
        DEEPSLATE_GOLD_ORE_BLOCK_ID => crate::GOLD_ORE_BLOCK_ID,
        DEEPSLATE_REDSTONE_ORE_BLOCK_ID => crate::REDSTONE_ORE_BLOCK_ID,
        DEEPSLATE_LAPIS_ORE_BLOCK_ID => crate::LAPIS_ORE_BLOCK_ID,
        DEEPSLATE_DIAMOND_ORE_BLOCK_ID => crate::DIAMOND_ORE_BLOCK_ID,
        DEEPSLATE_EMERALD_ORE_BLOCK_ID => crate::EMERALD_ORE_BLOCK_ID,
        _ => id,
    };
    BANDS.iter().find(|band| band.id == normal)
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
