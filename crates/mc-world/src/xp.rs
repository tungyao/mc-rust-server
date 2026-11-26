//! xp: 经验球实体 + 经验值/等级换算的纯逻辑（服务端权威，无 IO，只兼容 26.3/777）。
//!
//! - 实体池 [`XpOrbEntities`] 与物品实体池 [`crate::ItemEntities`] **相互独立**，
//!   但共用调用方（`DropState::alloc_id`）的全局自增 entity_id，故不会与物品/玩家冲突。
//! - 物理口径沿用 P18 掉落物：拾取延迟 10 ticks、拾取半径 1.5 格、重力 0.04/
//!   阻尼 0.98、6000 ticks despawn、支撑面静息/被挖空重算、切比雪夫视距过滤。
//!   物理只由全服唯一 50ms ticker 推进（P18.1 R1：连接侧绝不 tick）。
//! - 经验球不合并（原版同值会合并，这里最小实现只做「一矿一球」；见 HANDOFF/提交说明）。
//! - 等级曲线用原版公式（见 wiki Experience#Leveling up）：`xp_for_level` 是达到该
//!   等级所需的**总经验**，`level_for_xp` 为其整数逆，`xp_progress` 是当前等级进度 0..1。

use crate::{items, MIN_Y};
use mc_core::ChunkPos;

// ---------------------------------------------------------------------------
// 经验值 ↔ 等级（原版 26.3 曲线；整数运算，避免浮点误差累积）。
// ---------------------------------------------------------------------------

/// 达到 `level` 所需的总经验值（`level=0 → 0`，`1 → 7`，`16 → 352`，`30 → 1395`）。
/// 超过 i32 上限时钳制到 `i32::MAX`（与 SetExperience 的 VarInt 字段一致）。
pub fn xp_for_level(level: i32) -> i32 {
    let l = level.max(0) as i64;
    let total = if l <= 16 {
        l * l + 6 * l
    } else if l <= 31 {
        (5 * l * l - 81 * l + 720) / 2
    } else {
        (9 * l * l - 325 * l + 4440) / 2
    };
    total.clamp(0, i32::MAX as i64) as i32
}

/// 总经验值对应的等级（整数逆；`xp_for_level(level) <= total < xp_for_level(level+1)`）。
pub fn level_for_xp(total: i32) -> i32 {
    let total = total.max(0);
    let mut level = 0i32;
    // 原版上限约 2.4 亿总经验（再高 bar 消失），循环次数有限；用 i64 防止边界溢出。
    while xp_for_level(level + 1) <= total && level < 100_000 {
        level += 1;
    }
    level
}

/// 当前等级的进度条比例 0.0..=1.0（SetExperience 的 Float 字段）。
pub fn xp_progress(total: i32) -> f32 {
    let total = total.max(0);
    let level = level_for_xp(total);
    let floor = xp_for_level(level);
    let ceil = xp_for_level(level + 1);
    if ceil <= floor {
        return 1.0;
    }
    let p = (total - floor) as f32 / (ceil - floor) as f32;
    p.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// 经验球实体（独立实体池；物理口径复用 `items` 常量）。
// ---------------------------------------------------------------------------

/// 单个经验球实体。`amount` 是拾取时给予的经验值（`SetEntityData` index 8 / VarInt(1)）。
#[derive(Debug, Clone, PartialEq)]
pub struct XpOrb {
    pub id: i32,
    pub uuid: u128,
    pub amount: i32,
    pub pos: (f64, f64, f64),
    pub vel: (f64, f64, f64),
    pub age: u32,
    pub pickup_delay: u32,
    pub rest_y: f64,
    /// 支撑方块坐标 `(x, rest_y - 1, z)`；被挖空时重算静息高度并恢复下落。
    pub support: Option<(i32, i32, i32)>,
    pub on_ground: bool,
    pub chunk: ChunkPos,
    /// 本 tick 是否移动过（同步用，发送后由调用方清零）。
    pub moved: bool,
}

fn chunk_of_pos(x: f64, z: f64) -> ChunkPos {
    ChunkPos((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16))
}

fn dist(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    let (dx, dy, dz) = (a.0 - b.0, a.1 - b.1, a.2 - b.2);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// 经验球实体集合（单世界实例；多连接共享，entity_id 全局唯一由调用方保证）。
#[derive(Debug, Default)]
pub struct XpOrbEntities {
    orbs: Vec<XpOrb>,
}

impl XpOrbEntities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.orbs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.orbs.is_empty()
    }

    pub fn get(&self, id: i32) -> Option<&XpOrb> {
        self.orbs.iter().find(|o| o.id == id)
    }

    pub fn ids(&self) -> Vec<i32> {
        self.orbs.iter().map(|o| o.id).collect()
    }

    /// 生成一个经验球（`id` 必须全局唯一，重复返回 Err 且不落子）。
    /// 初速/延迟/落点与物品实体同口径（确定性 hash 水平扰动 ±0.1，vy=0.25）。
    pub fn spawn(
        &mut self,
        id: i32,
        amount: i32,
        x: f64,
        y: f64,
        z: f64,
        rest_y: f64,
    ) -> Result<(), &'static str> {
        if amount <= 0 {
            return Err("orb amount must be positive");
        }
        if self.orbs.iter().any(|o| o.id == id) {
            return Err("duplicate entity id");
        }
        let h = (id as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let hx = ((h >> 16) as f64 % 200.0) / 1000.0 - 0.1;
        let hz = ((h >> 32) as f64 % 200.0) / 1000.0 - 0.1;
        let sy = rest_y as i32 - 1;
        let support = if sy >= MIN_Y {
            Some((x.floor() as i32, sy, z.floor() as i32))
        } else {
            None
        };
        self.orbs.push(XpOrb {
            id,
            uuid: items::uuid_for_entity(id),
            amount,
            pos: (x, y, z),
            vel: (hx, 0.25, hz),
            age: 0,
            pickup_delay: items::PICKUP_DELAY_TICKS,
            rest_y,
            support,
            on_ground: false,
            chunk: chunk_of_pos(x, z),
            moved: true,
        });
        Ok(())
    }

    /// 物理 tick（只由全服唯一 ticker 调用）：下落/落地/despawn，返回 (moved, dead)。
    pub fn tick(&mut self) -> (Vec<i32>, Vec<i32>) {
        let mut moved = Vec::new();
        let mut dead = Vec::new();
        for o in self.orbs.iter_mut() {
            o.age += 1;
            if o.age >= items::DESPAWN_TICKS {
                dead.push(o.id);
                continue;
            }
            if o.pickup_delay > 0 {
                o.pickup_delay -= 1;
            }
            if !o.on_ground {
                o.vel.1 -= items::GRAVITY;
                o.vel.0 *= items::DRAG;
                o.vel.1 *= items::DRAG;
                o.vel.2 *= items::DRAG;
                o.pos.0 += o.vel.0;
                o.pos.1 += o.vel.1;
                o.pos.2 += o.vel.2;
                o.chunk = chunk_of_pos(o.pos.0, o.pos.2);
                if o.pos.1 <= o.rest_y {
                    o.pos.1 = o.rest_y;
                    o.vel = (0.0, 0.0, 0.0);
                    o.on_ground = true;
                }
                o.moved = true;
            }
            if o.moved {
                moved.push(o.id);
            }
        }
        self.orbs.retain(|o| o.age < items::DESPAWN_TICKS);
        for o in self.orbs.iter_mut() {
            o.moved = false;
        }
        (moved, dead)
    }

    /// 静息在支撑方块上、但支撑已被挖空的实体（与物品实体同口径）。
    pub fn find_unsupported(
        &self,
        is_present: impl Fn((i32, i32, i32)) -> bool,
    ) -> Vec<(i32, (i32, i32, i32))> {
        self.orbs
            .iter()
            .filter(|o| o.on_ground)
            .filter_map(|o| o.support.map(|s| (o.id, s)))
            .filter(|(_, s)| !is_present(*s))
            .collect()
    }

    /// 支撑被挖空：解除静息，按新的静息高度继续下落（保留原支撑列 x/z）。
    pub fn resume_fall(&mut self, id: i32, new_rest_y: f64) -> bool {
        let Some(o) = self.orbs.iter_mut().find(|o| o.id == id) else { return false };
        o.rest_y = new_rest_y;
        o.on_ground = false;
        o.moved = true;
        let sy = new_rest_y as i32 - 1;
        o.support = if sy >= MIN_Y {
            let (sx, _, sz) = o
                .support
                .unwrap_or_else(|| (o.pos.0.floor() as i32, sy, o.pos.2.floor() as i32));
            Some((sx, sy, sz))
        } else {
            None
        };
        true
    }

    /// 移除实体（拾取/despawn 发包后调用）。
    pub fn remove(&mut self, id: i32) -> Option<XpOrb> {
        self.orbs.iter().position(|o| o.id == id).map(|i| self.orbs.remove(i))
    }

    /// 可拾取查询：delay 到期且与玩家脚位置距离 <1.5 的实体 id。
    pub fn pickable(&self, player_feet: (f64, f64, f64)) -> Vec<i32> {
        self.orbs
            .iter()
            .filter(|o| o.pickup_delay == 0 && dist(o.pos, player_feet) < items::PICKUP_RADIUS)
            .map(|o| o.id)
            .collect()
    }

    /// 视距过滤（切比雪夫半径，复用 ChunkView 语义）。
    pub fn in_view(&self, center: ChunkPos, radius: i32) -> Vec<i32> {
        self.orbs
            .iter()
            .filter(|o| {
                (o.chunk.0 - center.0).abs() <= radius && (o.chunk.1 - center.1).abs() <= radius
            })
            .map(|o| o.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xp_level_curve_matches_vanilla_table() {
        // wiki Experience#Leveling up 的抽样点（总经验）。
        for (level, total) in
            [(0, 0), (1, 7), (2, 16), (7, 91), (15, 315), (16, 352), (17, 394), (30, 1395), (32, 1628)]
        {
            assert_eq!(xp_for_level(level), total, "level {level}");
            assert_eq!(level_for_xp(total), level, "total {total}");
        }
        assert_eq!(xp_for_level(31), 1507);
        // 区间内任意值都归到正确等级（只前进不回退）。
        for level in 0..=40 {
            let a = xp_for_level(level);
            let b = xp_for_level(level + 1);
            for total in a..b {
                assert_eq!(level_for_xp(total), level, "total {total} in [{a},{b})");
            }
        }
        // 进度条：等级起点 0、上限 1、中间单调。
        assert_eq!(xp_progress(0), 0.0);
        assert!(xp_progress(1) > 0.0 && xp_progress(1) < 1.0);
        assert_eq!(xp_progress(7), 0.0, "刚好升到 1 级，bar 归零");
        // 同一等级内单调递增；升级瞬间归零是预期（不是全程单调）。
        for level in 0..=5 {
            let a = xp_for_level(level);
            let b = xp_for_level(level + 1);
            let mut prev = -1.0;
            for total in a..b {
                let p = xp_progress(total);
                assert!(p >= prev, "progress must be monotonic within level {level} at {total}");
                prev = p;
            }
        }
    }

    fn spawn_one(orbs: &mut XpOrbEntities, id: i32, amount: i32) {
        orbs.spawn(id, amount, 0.5, 70.0, 0.5, 69.0).unwrap();
    }

    #[test]
    fn orb_entity_ids_unique_and_id_derives_uuid() {
        let mut orbs = XpOrbEntities::new();
        spawn_one(&mut orbs, 1001, 3);
        assert!(orbs.spawn(1001, 3, 0.5, 70.0, 0.5, 69.0).is_err());
        assert!(orbs.spawn(1002, 0, 0.5, 70.0, 0.5, 69.0).is_err(), "zero amount rejected");
        assert_eq!(orbs.len(), 1);
        assert_eq!(orbs.get(1001).unwrap().uuid, items::uuid_for_entity(1001));
    }

    #[test]
    fn orb_falls_rests_and_despires_like_items() {
        let mut orbs = XpOrbEntities::new();
        spawn_one(&mut orbs, 1001, 7);
        for _ in 0..200 {
            orbs.tick();
        }
        let o = orbs.get(1001).unwrap();
        assert!(o.on_ground);
        assert_eq!(o.pos.1, 69.0);
        assert_eq!(o.vel, (0.0, 0.0, 0.0));
        for _ in 0..(items::DESPAWN_TICKS - 200 - 1) {
            let (_, dead) = orbs.tick();
            assert!(dead.is_empty());
        }
        let (_, dead) = orbs.tick();
        assert_eq!(dead, vec![1001]);
        assert!(orbs.is_empty());
    }

    #[test]
    fn orb_pickup_delay_and_radius_follow_p18_caliber() {
        let mut orbs = XpOrbEntities::new();
        orbs.spawn(1001, 1, 0.5, 69.0, 0.5, 69.0).unwrap();
        assert!(orbs.pickable((0.5, 69.0, 0.5)).is_empty(), "delay not over");
        for _ in 0..items::PICKUP_DELAY_TICKS {
            orbs.tick();
        }
        let p = orbs.get(1001).unwrap().pos;
        assert_eq!(orbs.pickable(p), vec![1001]);
        let dx = items::PICKUP_RADIUS + 0.01;
        assert!(orbs.pickable((p.0 + dx, p.1, p.2)).is_empty());
    }

    #[test]
    fn orb_resumes_fall_when_support_is_dug() {
        let mut orbs = XpOrbEntities::new();
        spawn_one(&mut orbs, 1001, 3);
        for _ in 0..200 {
            orbs.tick();
        }
        assert_eq!(orbs.get(1001).unwrap().support, Some((0, 68, 0)));
        assert_eq!(orbs.find_unsupported(|_| false), vec![(1001, (0, 68, 0))]);
        assert!(orbs.find_unsupported(|_| true).is_empty());
        assert!(orbs.resume_fall(1001, 67.0));
        let o = orbs.get(1001).unwrap();
        assert_eq!(o.support, Some((0, 66, 0)));
        assert_eq!(o.rest_y, 67.0);
        assert!(!o.on_ground);
    }

    #[test]
    fn orb_in_view_filters_by_chunk_radius() {
        let mut orbs = XpOrbEntities::new();
        spawn_one(&mut orbs, 1001, 1); // chunk (0,0)
        orbs.spawn(1002, 1, 100.5, 70.0, 100.5, 69.0).unwrap(); // chunk (6,6)
        assert_eq!(orbs.in_view(ChunkPos(0, 0), 2), vec![1001]);
        let mut both = orbs.in_view(ChunkPos(3, 3), 3);
        both.sort();
        assert_eq!(both, vec![1001, 1002]);
    }
}
