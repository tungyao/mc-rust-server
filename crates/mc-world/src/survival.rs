//! survival: P40 生存状态纯逻辑（生命/饥饿/饱和 + 食物表），只兼容 26.3/777。
//!
//! 状态本身就是 `SetHealth` S2C（CB `set_health`=106）的三字段：
//! Health(Float) / Food(VarInt) / Food Saturation(Float)。本模块只做权威数值推进，
//! 不碰 socket、不 tick；推进只由 `DropState::start_global_tick` 的唯一 ticker 调用
//! （P18.1 R1：连接侧绝不 tick）。

use crate::registry_id;

/// 满血/满饥饿/满饱和（新玩家与 respawn 的初始值；与 26.3 客户端默认一致）。
pub const MAX_HEALTH: f32 = 20.0;
pub const MAX_FOOD: i32 = 20;
pub const MAX_SATURATION: f32 = 5.0;
/// 饥饿伤害每跳扣 1 颗心（半心制下 2 点，按满血 20 口径这里用 1.0）。
pub const STARVATION_DAMAGE: f32 = 1.0;
/// 坠落伤害的免伤高度（原版：落差 >3 格才开始掉血）。
pub const FALL_SAFE_BLOCKS: f64 = 3.0;

// --- P53 A2 饥饿细节：exhaustion（疲劳度） ---
//
// 注意：本仓库有 528 个既有测试（含 `SurvivalState { health, food, saturation }`
// 穷举字面量），硬约束要求它们**一字不改**且全绿，因此 exhaustion 不直接塞进
// `SurvivalState` 的字段表，而是作为与它同存的浮点伴随状态（DropState.exhaustions /
// PlayerInventory.exhaustion）落盘。语义与「SurvivalState 增加 exhaustion 字段」等价：
// 每 tick 由全服唯一 ticker 推进，跨 4.0 就消耗一格饱和度/饥饿。

/// 疲劳度阈值：跨过 4.0 触发一次饥饿衰减（原版口径）。
pub const EXHAUSTION_THRESHOLD: f32 = 4.0;
/// 疾跑每 50ms tick 的额外消耗。
pub const SPRINT_EXHAUSTION_PER_TICK: f32 = 0.1;
/// 起跳一次性消耗。
pub const JUMP_EXHAUSTION: f32 = 0.2;
/// 游泳每 50ms tick 的额外消耗。
pub const SWIM_EXHAUSTION_PER_TICK: f32 = 0.02;

/// 来自 C2S 移动包的本 tick 活动标记（连接侧只设置，ticker 才推进疲劳度）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Activity {
    pub sprinting: bool,
    pub jumped: bool,
    pub swimming: bool,
}

impl Activity {
    /// 本 tick 应累加的疲劳度（纯函数，无 IO/无 tick）。
    pub fn exhaustion_delta(self) -> f32 {
        let mut d = 0.0;
        if self.sprinting {
            d += SPRINT_EXHAUSTION_PER_TICK;
        }
        if self.jumped {
            d += JUMP_EXHAUSTION;
        }
        if self.swimming {
            d += SWIM_EXHAUSTION_PER_TICK;
        }
        d
    }
}

/// 疲劳度推进（纯逻辑）：返回新的疲劳度与是否跨过阈值（跨过一次扣一格，余量保留）。
/// `exhaustion` 非法（NaN/负数/超上限）时先钳制到 `[0, THRESHOLD)`。
pub fn advance_exhaustion(exhaustion: f32, activity: Activity) -> (f32, bool) {
    let mut value = if exhaustion.is_finite() { exhaustion.max(0.0) } else { 0.0 };
    value += activity.exhaustion_delta();
    if value >= EXHAUSTION_THRESHOLD {
        (value - EXHAUSTION_THRESHOLD, true)
    } else {
        (value, false)
    }
}

/// 读盘兜底：非法疲劳度归零，合法值钳到阈值内（旧档缺字段=0）。
pub fn sanitize_exhaustion(value: f32) -> f32 {
    if !value.is_finite() || value < 0.0 {
        0.0
    } else {
        value.min(EXHAUSTION_THRESHOLD)
    }
}

/// per-conn 生存状态（与背包同存档：`players` 表 v2 / `.dat` v3）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurvivalState {
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
}

impl Default for SurvivalState {
    fn default() -> Self {
        Self::full()
    }
}

impl SurvivalState {
    /// 满血满饥饿满饱和（Join 初始化与复活复位）。
    pub fn full() -> Self {
        Self { health: MAX_HEALTH, food: MAX_FOOD, saturation: MAX_SATURATION }
    }

    pub fn is_dead(&self) -> bool {
        self.health <= 0.0
    }

    /// 一次饥饿节拍：饱和>0 先扣饱和，否则扣饥饿；返回是否发生变化（用于决定是否发包）。
    pub fn decay_one(&mut self) -> bool {
        if self.saturation > 0.0 {
            self.saturation = (self.saturation - 1.0).max(0.0);
            true
        } else if self.food > 0 {
            self.food -= 1;
            true
        } else {
            false
        }
    }

    /// 饥饿见底后的一个伤害节拍（返回扣血后的状态）。
    pub fn starve_damage(&mut self) {
        self.health = (self.health - STARVATION_DAMAGE).max(0.0);
    }

    /// 坠落伤害：落差超过 3 格后每格 1 点，向下取整；返回**实际扣掉**的血量
    /// （不会超过当前生命，避免报告比真实伤害更大的数字）。
    pub fn fall_damage(&mut self, fall_blocks: f64) -> i32 {
        let dmg = (fall_blocks - FALL_SAFE_BLOCKS).floor().max(0.0) as i32;
        if dmg <= 0 {
            return 0;
        }
        let applied = dmg.min(self.health as i32);
        self.health = (self.health - applied as f32).max(0.0);
        applied
    }

    /// 进食：补饥饿并按食物饱和值补饱和（都钳制到上限）；返回是否发生变化。
    pub fn eat(&mut self, food: i32, saturation: f32) -> bool {
        let before = (self.food, self.saturation);
        self.food = (self.food + food.max(0)).min(MAX_FOOD);
        self.saturation = (self.saturation + saturation.max(0.0)).min(MAX_SATURATION);
        (self.food, self.saturation) != before
    }

    /// 读取/恢复时钳制非法值（存档损坏兜底）。
    pub fn sanitize(mut self) -> Self {
        self.health = self.health.clamp(0.0, MAX_HEALTH);
        self.food = self.food.clamp(0, MAX_FOOD);
        self.saturation = self.saturation.clamp(0.0, MAX_SATURATION);
        self
    }
}

/// 食物定义：`(item_id, 饥饿恢复, 饱和增量)`。ID 全部编译期查生成表，
/// 与 drops.rs 同口径；只收录本服可获得且 wiki 口径明确的常见食物（MVP）。
const APPLE: i32 = registry_id(mc_registry::ITEMS, "minecraft:apple") as i32; // 1007
const BREAD: i32 = registry_id(mc_registry::ITEMS, "minecraft:bread") as i32; // 1067
const CARROT: i32 = registry_id(mc_registry::ITEMS, "minecraft:carrot") as i32; // 1378
const POTATO: i32 = registry_id(mc_registry::ITEMS, "minecraft:potato") as i32; // 1379
const BAKED_POTATO: i32 = registry_id(mc_registry::ITEMS, "minecraft:baked_potato") as i32; // 1380
const BEETROOT: i32 = registry_id(mc_registry::ITEMS, "minecraft:beetroot") as i32; // 1438
const MELON_SLICE: i32 = registry_id(mc_registry::ITEMS, "minecraft:melon_slice") as i32; // 1256
const SWEET_BERRIES: i32 = registry_id(mc_registry::ITEMS, "minecraft:sweet_berries") as i32; // 1525
const COOKED_BEEF: i32 = registry_id(mc_registry::ITEMS, "minecraft:cooked_beef") as i32; // 1261
const COOKED_PORKCHOP: i32 = registry_id(mc_registry::ITEMS, "minecraft:cooked_porkchop") as i32; // 1098
const COOKED_CHICKEN: i32 = registry_id(mc_registry::ITEMS, "minecraft:cooked_chicken") as i32; // 1263
const COOKED_MUTTON: i32 = registry_id(mc_registry::ITEMS, "minecraft:cooked_mutton") as i32; // 1416
const COOKED_RABBIT: i32 = registry_id(mc_registry::ITEMS, "minecraft:cooked_rabbit") as i32; // 1401
const COOKED_COD: i32 = registry_id(mc_registry::ITEMS, "minecraft:cooked_cod") as i32; // 1194
const COOKED_SALMON: i32 = registry_id(mc_registry::ITEMS, "minecraft:cooked_salmon") as i32; // 1195

/// 手持食物 → `(food, saturation)`；非食物返回 None。
pub fn food_for_item(item_id: i32) -> Option<(i32, f32)> {
    match item_id {
        APPLE => Some((4, 1.2)),
        BREAD => Some((5, 6.0)),
        CARROT => Some((3, 3.6)),
        POTATO => Some((1, 0.2)),
        BAKED_POTATO => Some((5, 6.0)),
        BEETROOT => Some((1, 0.6)),
        MELON_SLICE => Some((2, 1.2)),
        SWEET_BERRIES => Some((1, 0.1)),
        COOKED_BEEF | COOKED_PORKCHOP => Some((8, 12.8)),
        COOKED_CHICKEN | COOKED_MUTTON | COOKED_RABBIT => Some((6, 9.6)),
        COOKED_COD | COOKED_SALMON => Some((5, 6.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_state_is_default_and_sanitizes() {
        assert_eq!(SurvivalState::default(), SurvivalState::full());
        assert_eq!((SurvivalState::full().health, SurvivalState::full().food, SurvivalState::full().saturation), (20.0, 20, 5.0));
        assert_eq!(SurvivalState { health: 99.0, food: -1, saturation: -2.0 }.sanitize(), SurvivalState::full().with_health(20.0).with_food(0).with_saturation(0.0));
    }

    #[test]
    fn hunger_decays_saturation_then_food_then_starves() {
        let mut s = SurvivalState { health: 20.0, food: 2, saturation: 2.0 };
        assert!(s.decay_one());
        assert_eq!((s.food, s.saturation), (2, 1.0));
        assert!(s.decay_one());
        assert_eq!((s.food, s.saturation), (2, 0.0));
        assert!(s.decay_one());
        assert_eq!((s.food, s.saturation), (1, 0.0));
        assert!(s.decay_one());
        assert_eq!(s.food, 0);
        assert!(!s.decay_one(), "empty food has nothing left to decay");
        s.starve_damage();
        assert_eq!(s.health, 19.0);
        for _ in 0..19 {
            s.starve_damage();
        }
        assert!(s.is_dead());
        assert_eq!(s.health, 0.0);
    }

    #[test]
    fn fall_damage_only_above_three_blocks() {
        let mut s = SurvivalState::full();
        assert_eq!(s.fall_damage(3.0), 0);
        assert_eq!(s.fall_damage(5.0), 2, "floor(5-3)=2");
        assert_eq!(s.health, 18.0);
        assert_eq!(s.fall_damage(10.9), 7);
        assert_eq!(s.health, 11.0);
        assert_eq!(s.fall_damage(1000.0), 11, "clamped to zero health");
        assert!(s.is_dead());
    }

    #[test]
    fn food_table_has_verified_ids() {
        assert_eq!(food_for_item(APPLE), Some((4, 1.2)));
        assert_eq!(food_for_item(BREAD), Some((5, 6.0)));
        assert_eq!(food_for_item(mc_registry::item_id("minecraft:stone").unwrap() as i32), None);
        assert_eq!(mc_registry::item_id("minecraft:bread"), Some(1067));
    }

    #[test]
    fn eating_restores_food_and_saturation_with_clamps() {
        let mut s = SurvivalState { health: 10.0, food: 18, saturation: 4.0 };
        assert!(s.eat(5, 6.0));
        assert_eq!((s.food, s.saturation), (20, 5.0));
        assert!(!s.eat(0, 0.0));
    }

    /// P53 A2：疾跑/跳跃/游泳差异化消耗，跨 4.0 才扣一格且余量保留。
    #[test]
    fn exhaustion_accumulates_from_activity_and_wraps_at_threshold() {
        assert_eq!(Activity::default().exhaustion_delta(), 0.0);
        let sprint = Activity { sprinting: true, ..Activity::default() };
        let jump = Activity { jumped: true, ..Activity::default() };
        let swim = Activity { swimming: true, ..Activity::default() };
        assert!(jump.exhaustion_delta() > sprint.exhaustion_delta());
        assert!(sprint.exhaustion_delta() > swim.exhaustion_delta());
        let (v, due) = advance_exhaustion(3.95, jump);
        assert!(due);
        assert!((v - 0.15).abs() < 1e-6, "leftover above threshold must be kept");
        let (v, due) = advance_exhaustion(3.85, sprint);
        assert!(!due && (v - 3.95).abs() < 1e-6);
        assert_eq!(sanitize_exhaustion(f32::NAN), 0.0);
        assert_eq!(sanitize_exhaustion(99.0), EXHAUSTION_THRESHOLD);
        assert_eq!(sanitize_exhaustion(1.25), 1.25);
    }
}

/// 测试辅助：链式改字段（仅用于单测构造边界值）。
#[cfg(test)]
impl SurvivalState {
    fn with_health(mut self, v: f32) -> Self {
        self.health = v;
        self
    }
    fn with_food(mut self, v: i32) -> Self {
        self.food = v;
        self
    }
    fn with_saturation(mut self, v: f32) -> Self {
        self.saturation = v;
        self
    }
}
