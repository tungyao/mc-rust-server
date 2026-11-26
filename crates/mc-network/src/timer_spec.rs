//! P39：`timers` 表 `spec` 字节的编号映射（§3.2 保持 10B 定长值不变）。
//!
//! 布局：低 4bit = 规格表内索引，高 4bit = 来源表 tag；bit7 另外当
//! 「瓜果已落过」标志（只用于 P37 age 模型的 Fruit）。不碰任何方块 ID/阶段/
//! 掉落逻辑，只把已有的静态规格映射成 u8，便于跨重启还原 `PlantKind`。
//!
//! | tag | 值 | 表 |
//! |---|---|---|
//! | P33 | 0x00 | `crops::CROPS`（小麦/胡萝卜/马铃薯/甜菜） |
//! | P37 age | 0x10 | `crops::AGE_CROPS`（含 Fruit 瓜梗/南瓜梗） |
//! | P37 tree | 0x20 | `crops::TREE_SPECS`（橡树走 P26 旧路径，spec=0） |
//! | P37 vertical | 0x30 | `crops::VERTICAL_SPECS` |
//! | P37 side | 0x40 | `crops::COCOA` |

use mc_world::crops::{
    AGE_CROPS, AgeCropSpec, CROPS, CropSpec, TREE_SPECS, TreeSpec, VERTICAL_SPECS, VerticalSpec,
};
use mc_world::store::{TimerKind, TimerRecord};

const P33: u8 = 0x00;
const AGE: u8 = 0x10;
const TREE: u8 = 0x20;
const VERTICAL: u8 = 0x30;
const SIDE: u8 = 0x40;
const FRUIT_DONE: u8 = 0x80;

fn index_of<T>(table: &[T], name: &str, get_name: fn(&T) -> &'static str) -> Option<u8> {
    table.iter().position(|s| get_name(s) == name).map(|i| i as u8)
}

pub(crate) fn p33(crop: &CropSpec) -> u8 {
    P33 | index_of(&CROPS, crop.name, |s| s.name).unwrap_or(0)
}

pub(crate) fn age(spec: &AgeCropSpec, fruit_done: bool) -> u8 {
    AGE | index_of(&AGE_CROPS, spec.name, |s| s.name).unwrap_or(0)
        | if fruit_done { FRUIT_DONE } else { 0 }
}

pub(crate) fn tree(spec: &TreeSpec) -> u8 {
    TREE | index_of(&TREE_SPECS, spec.name, |s| s.name).unwrap_or(0)
}

pub(crate) fn vertical(spec: &VerticalSpec) -> u8 {
    VERTICAL | index_of(&VERTICAL_SPECS, spec.name, |s| s.name).unwrap_or(0)
}

pub(crate) fn side() -> u8 {
    SIDE
}

/// 还原意图：每条 timer 决定放进哪张内存表、以哪个规格继续计时。
pub(crate) enum Restored {
    /// P33 作物（`DropState.crops`）。
    P33Crop(&'static CropSpec),
    /// P37 age 模型（`DropState.plants`，含 Fruit 是否已结算）。
    Age(&'static AgeCropSpec, bool),
    /// P37 树苗（`DropState.plants`）。
    Tree(&'static TreeSpec),
    /// P37 竖向堆叠（`DropState.plants`）。
    Vertical(&'static VerticalSpec),
    /// P37 可可（`DropState.plants`）。
    Side(&'static mc_world::crops::SideSpec),
    /// P26 橡树苗（`DropState.saplings`）；spec=0。
    OakSapling,
    /// P27 树叶观察（启动时直接进 `DropState.pending_leaves`）。
    Leaf,
}

impl Restored {
    pub(crate) fn from_record(rec: &TimerRecord) -> Option<Self> {
        let idx = (rec.spec & 0x0f) as usize;
        // bit7 是 Fruit 标志，不属于来源表 tag，先剥掉。
        match (rec.kind, (rec.spec & 0x7f) & 0xf0) {
            (TimerKind::Crop, P33) => CROPS.get(idx).map(Self::P33Crop),
            (TimerKind::Crop, AGE) => AGE_CROPS
                .get(idx)
                .map(|s| Self::Age(s, rec.spec & FRUIT_DONE != 0)),
            (TimerKind::Sapling, TREE) => TREE_SPECS.get(idx).map(Self::Tree),
            (TimerKind::Crop, VERTICAL) => VERTICAL_SPECS.get(idx).map(Self::Vertical),
            (TimerKind::Crop, SIDE) => Some(Self::Side(&mc_world::crops::COCOA)),
            (TimerKind::Sapling, 0) => Some(Self::OakSapling),
            (TimerKind::Leaf, _) => Some(Self::Leaf),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_byte_roundtrips_all_plant_models() {
        let records = [
            TimerRecord {
                kind: TimerKind::Crop,
                pos: (0, 0, 0),
                planted_tick: 0,
                stage: 0,
                spec: p33(&CROPS[2]),
            },
            TimerRecord {
                kind: TimerKind::Crop,
                pos: (0, 0, 0),
                planted_tick: 0,
                stage: 0,
                spec: age(&AGE_CROPS[1], true),
            },
            TimerRecord {
                kind: TimerKind::Sapling,
                pos: (0, 0, 0),
                planted_tick: 0,
                stage: 0,
                spec: tree(&TREE_SPECS[3]),
            },
            TimerRecord {
                kind: TimerKind::Crop,
                pos: (0, 0, 0),
                planted_tick: 0,
                stage: 0,
                spec: vertical(&VERTICAL_SPECS[2]),
            },
            TimerRecord {
                kind: TimerKind::Crop,
                pos: (0, 0, 0),
                planted_tick: 0,
                stage: 0,
                spec: side(),
            },
            TimerRecord {
                kind: TimerKind::Sapling,
                pos: (0, 0, 0),
                planted_tick: 0,
                stage: 0,
                spec: 0,
            },
            TimerRecord {
                kind: TimerKind::Leaf,
                pos: (0, 0, 0),
                planted_tick: 0,
                stage: 0,
                spec: 0,
            },
        ];
        assert!(matches!(Restored::from_record(&records[0]), Some(Restored::P33Crop(c)) if c.name == CROPS[2].name));
        assert!(matches!(Restored::from_record(&records[1]), Some(Restored::Age(c, true)) if c.name == AGE_CROPS[1].name));
        assert!(matches!(Restored::from_record(&records[2]), Some(Restored::Tree(c)) if c.name == TREE_SPECS[3].name));
        assert!(matches!(Restored::from_record(&records[3]), Some(Restored::Vertical(c)) if c.name == VERTICAL_SPECS[2].name));
        assert!(matches!(Restored::from_record(&records[4]), Some(Restored::Side(_))));
        assert!(matches!(Restored::from_record(&records[5]), Some(Restored::OakSapling)));
        assert!(matches!(Restored::from_record(&records[6]), Some(Restored::Leaf)));
    }
}
