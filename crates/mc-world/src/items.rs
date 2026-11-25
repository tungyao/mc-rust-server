//! items: 服务端权威物品实体（掉落物），纯逻辑无 IO，只兼容 26.3/777。
//!
//! - 生成：pickup_delay=10 ticks；初速上抛 + hash 确定性水平扰动；重力下落到
//!   生成时算好的 `rest_y`（支撑面顶，调用方由 `MutableWorld::support_top_y` 给）。
//! - 同物品同 chunk 半径 1 格内合并（count 相加，上限 64，超出部分留新实体）。
//! - 6000 ticks（5 分钟）despawn；pickup_delay 到期且与玩家距离 <1.5 才可捡。
//! - entity_id 由调用方分配（全局自增，与玩家 LoginPlay entity_id=1 不冲突，
//!   本模块只断言唯一，不分配）。

use crate::MIN_Y;
use mc_core::ChunkPos;

/// 拾取保护 ticks（10）。
pub const PICKUP_DELAY_TICKS: u32 = 10;
/// despawn ticks（6000=5 分钟）。
pub const DESPAWN_TICKS: u32 = 6000;
/// 拾取半径（与玩家脚位置 3D 距离）。
pub const PICKUP_RADIUS: f64 = 1.5;
/// 合并半径（同物品同 chunk 内 3D 距离）。
pub const MERGE_RADIUS: f64 = 1.0;
/// 单栈上限。
pub const MAX_STACK: i32 = 64;
/// 重力/阻尼（每 tick）：vy -= G，再整体 *= DRAG。
pub const GRAVITY: f64 = 0.04;
pub const DRAG: f64 = 0.98;

/// 单个掉落物实体。
#[derive(Debug, Clone, PartialEq)]
pub struct ItemEntity {
    pub id: i32,
    pub uuid: u128,
    pub item_id: i32,
    pub count: i32,
    pub pos: (f64, f64, f64),
    pub vel: (f64, f64, f64),
    pub age: u32,
    pub pickup_delay: u32,
    pub rest_y: f64,
    /// 支撑方块坐标：`(x, rest_y - 1, z)`。被挖空时重算静息高度并恢复下落。
    /// 全空列（rest_y == MIN_Y）为 None。
    pub support: Option<(i32, i32, i32)>,
    pub on_ground: bool,
    pub chunk: ChunkPos,
    /// 本 tick 是否移动过（同步用，发送后由调用方清零）。
    pub moved: bool,
}

/// 由 entity id 派生确定性 UUID（高位混合，避免与其它实体重 UUID）。
pub fn uuid_for_entity(id: i32) -> u128 {
    let hi = (id as u64).wrapping_mul(0x9e3779b97f4a7c15);
    let lo = (id as u64).wrapping_mul(0xbf58476d1ce4e5b9) ^ 0x4a7c159e3779b97f;
    ((hi as u128) << 64) | lo as u128
}

fn chunk_of_pos(x: f64, z: f64) -> ChunkPos {
    ChunkPos((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16))
}

fn dist(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    let (dx, dy, dz) = (a.0 - b.0, a.1 - b.1, a.2 - b.2);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// 物品实体集合（单世界实例；多连接共享，entity_id 全局唯一由调用方保证）。
#[derive(Debug, Default)]
pub struct ItemEntities {
    entities: Vec<ItemEntity>,
}

impl ItemEntities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn get(&self, id: i32) -> Option<&ItemEntity> {
        self.entities.iter().find(|e| e.id == id)
    }

    pub fn ids(&self) -> Vec<i32> {
        self.entities.iter().map(|e| e.id).collect()
    }

    /// 生成掉落物（`id` 必须全局唯一，重复则返回 Err 且不落子）。
    /// 先尝试同物品同 chunk 半径 1 格内合并（上限 64），合满后剩余部分落新实体。
    /// 初速：vy=0.25，水平 hash 扰动 ±0.1；rest_y 为支撑面顶 y（ feet 高度）。
    pub fn spawn(
        &mut self,
        id: i32,
        item_id: i32,
        count: i32,
        x: f64,
        y: f64,
        z: f64,
        rest_y: f64,
    ) -> Result<Vec<i32>, &'static str> {
        // 确定性水平扰动（hash id，避免全局随机）。
        let h = (id as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let hx = ((h >> 16) as f64 % 200.0) / 1000.0 - 0.1;
        let hz = ((h >> 32) as f64 % 200.0) / 1000.0 - 0.1;
        self.spawn_with_velocity(id, item_id, count, x, y, z, rest_y, (hx, 0.25, hz))
    }

    /// 带显式初速的生成（P28 扔物按视角定向；挖掘掉落走上面的 `spawn` 默认初速）。
    /// 合并语义与 `spawn` 完全一致（同物品同 chunk 半径 1 格、64 上限）；
    /// `vel` 只用于新落子实体，被合并吸收的数量不改变既有实体速度。
    pub fn spawn_with_velocity(
        &mut self,
        id: i32,
        item_id: i32,
        mut count: i32,
        x: f64,
        y: f64,
        z: f64,
        rest_y: f64,
        vel: (f64, f64, f64),
    ) -> Result<Vec<i32>, &'static str> {
        if count <= 0 {
            return Err("count must be positive");
        }
        if self.entities.iter().any(|e| e.id == id) {
            return Err("duplicate entity id");
        }
        let mut touched = Vec::new();
        let chunk = chunk_of_pos(x, z);
        // 合并：同物品同 chunk 半径 1 格内，未满栈的按顺序填。
        for e in self.entities.iter_mut() {
            if count <= 0 {
                break;
            }
            if e.item_id == item_id
                && e.chunk == chunk
                && dist(e.pos, (x, y, z)) <= MERGE_RADIUS
                && e.count < MAX_STACK
            {
                let room = MAX_STACK - e.count;
                let take = room.min(count);
                e.count += take;
                count -= take;
                e.moved = true;
                touched.push(e.id);
            }
        }
        if count > 0 {
            let sy = rest_y as i32 - 1;
            let support = if sy >= MIN_Y {
                Some((x.floor() as i32, sy, z.floor() as i32))
            } else {
                None
            };
            self.entities.push(ItemEntity {
                id,
                uuid: uuid_for_entity(id),
                item_id,
                count,
                pos: (x, y, z),
                vel,
                age: 0,
                pickup_delay: PICKUP_DELAY_TICKS,
                rest_y,
                support,
                on_ground: false,
                chunk,
                moved: true,
            });
            touched.push(id);
        }
        Ok(touched)
    }

    /// 物理 tick（每 50ms 一次）：下落/落地；返回 (moved_ids, despawned_ids)。
    /// despawn 的直接移除，调用方负责发 RemoveEntities。
    pub fn tick(&mut self) -> (Vec<i32>, Vec<i32>) {
        let mut moved = Vec::new();
        let mut dead = Vec::new();
        for e in self.entities.iter_mut() {
            e.age += 1;
            if e.age >= DESPAWN_TICKS {
                dead.push(e.id);
                continue;
            }
            if e.pickup_delay > 0 {
                e.pickup_delay -= 1;
            }
            if !e.on_ground {
                e.vel.1 -= GRAVITY;
                e.vel.0 *= DRAG;
                e.vel.1 *= DRAG;
                e.vel.2 *= DRAG;
                e.pos.0 += e.vel.0;
                e.pos.1 += e.vel.1;
                e.pos.2 += e.vel.2;
                e.chunk = chunk_of_pos(e.pos.0, e.pos.2);
                if e.pos.1 <= e.rest_y {
                    e.pos.1 = e.rest_y;
                    e.vel = (0.0, 0.0, 0.0);
                    e.on_ground = true;
                }
                e.moved = true;
            }
            if e.moved {
                moved.push(e.id);
            }
        }
        self.entities.retain(|e| e.age < DESPAWN_TICKS);
        for e in self.entities.iter_mut() {
            e.moved = false;
        }
        (moved, dead)
    }

    /// 找出「静息在支撑方块上、但支撑已被挖空」的实体。
    /// `is_present` 由调用方注入世界查询（本模块无 IO）；返回 `(entity_id, 支撑坐标)`。
    pub fn find_unsupported(
        &self,
        is_present: impl Fn((i32, i32, i32)) -> bool,
    ) -> Vec<(i32, (i32, i32, i32))> {
        self.entities
            .iter()
            .filter(|e| e.on_ground)
            .filter_map(|e| e.support.map(|s| (e.id, s)))
            .filter(|(_, s)| !is_present(*s))
            .collect()
    }

    /// 支撑被挖空：解除静止，改用新的静息高度继续下落。
    /// `new_rest_y` 由调用方用世界查询算出（`MutableWorld::drop_rest_y`）。
    pub fn resume_fall(&mut self, id: i32, new_rest_y: f64) -> bool {
        let Some(e) = self.entities.iter_mut().find(|e| e.id == id) else { return false };
        e.rest_y = new_rest_y;
        e.on_ground = false;
        e.moved = true;
        let sy = new_rest_y as i32 - 1;
        e.support = if sy >= MIN_Y {
            // 保留原支撑列的 x/z：掉落物的水平漂移不能改变“下方是哪一列”。
            let (sx, _, sz) = e.support.unwrap_or_else(|| {
                (e.pos.0.floor() as i32, sy, e.pos.2.floor() as i32)
            });
            Some((sx, sy, sz))
        } else {
            None
        };
        true
    }

    /// 移除指定实体（拾取/despawn 发包后调用），返回被删实体。
    pub fn remove(&mut self, id: i32) -> Option<ItemEntity> {
        self.entities.iter().position(|e| e.id == id).map(|i| self.entities.remove(i))
    }

    /// 可拾取查询：delay 到期且与玩家脚位置距离 <1.5 的实体 id。
    pub fn pickable(&self, player_feet: (f64, f64, f64)) -> Vec<i32> {
        self.entities
            .iter()
            .filter(|e| e.pickup_delay == 0 && dist(e.pos, player_feet) < PICKUP_RADIUS)
            .map(|e| e.id)
            .collect()
    }

    /// 视距过滤（切比雪夫半径，复用 ChunkView 语义）。
    pub fn in_view(&self, center: ChunkPos, radius: i32) -> Vec<i32> {
        self.entities
            .iter()
            .filter(|e| {
                (e.chunk.0 - center.0).abs() <= radius && (e.chunk.1 - center.1).abs() <= radius
            })
            .map(|e| e.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_one(es: &mut ItemEntities, id: i32, x: f64, y: f64, z: f64, rest: f64) {
        es.spawn(id, 55, 1, x, y, z, rest).unwrap();
    }

    #[test]
    fn entity_ids_must_be_unique() {
        let mut es = ItemEntities::new();
        spawn_one(&mut es, 1001, 0.5, 70.0, 0.5, 69.0);
        assert!(es.spawn(1001, 55, 1, 0.5, 70.0, 0.5, 69.0).is_err());
        assert_eq!(es.len(), 1);
        assert_ne!(uuid_for_entity(1001), uuid_for_entity(1002));
    }

    #[test]
    fn falling_lands_on_rest_height() {
        let mut es = ItemEntities::new();
        spawn_one(&mut es, 1001, 0.5, 70.0, 0.5, 69.0);
        for _ in 0..200 {
            es.tick();
        }
        let e = es.get(1001).unwrap();
        assert!(e.on_ground);
        assert_eq!(e.pos.1, 69.0);
        assert_eq!(e.vel, (0.0, 0.0, 0.0));
    }

    #[test]
    fn merge_same_item_within_radius_caps_at_64() {
        let mut es = ItemEntities::new();
        es.spawn(1001, 55, 60, 0.5, 70.0, 0.5, 69.0).unwrap();
        // 同位置同物品 +10 → 合并到 64，剩余 6 落新实体。
        let touched = es.spawn(1002, 55, 10, 0.5, 70.0, 0.5, 69.0).unwrap();
        assert_eq!(es.get(1001).unwrap().count, 64);
        assert_eq!(es.get(1002).unwrap().count, 6);
        assert!(touched.contains(&1001) && touched.contains(&1002));
        // 不同物品不合并。
        es.spawn(1003, 62, 1, 0.5, 70.0, 0.5, 69.0).unwrap();
        assert_eq!(es.len(), 3);
        // 远处同物品不合并（>1 格）。
        es.spawn(1004, 55, 1, 10.5, 70.0, 10.5, 69.0).unwrap();
        assert_eq!(es.len(), 4);
    }

    #[test]
    fn despawn_after_6000_ticks() {
        let mut es = ItemEntities::new();
        spawn_one(&mut es, 1001, 0.5, 70.0, 0.5, 69.0);
        for _ in 0..5999 {
            let (_, dead) = es.tick();
            assert!(dead.is_empty());
        }
        assert_eq!(es.len(), 1);
        let (_, dead) = es.tick();
        assert_eq!(dead, vec![1001]);
        assert!(es.is_empty());
    }

    #[test]
    fn pickup_delay_blocks_early_pickup() {
        let mut es = ItemEntities::new();
        spawn_one(&mut es, 1001, 0.5, 69.0, 0.5, 69.0);
        // 刚生成（delay=10）：即使站在上面也不可捡。
        assert!(es.pickable((0.5, 69.0, 0.5)).is_empty());
        for _ in 0..10 {
            es.tick();
        }
        // delay 到期：实体当前位置可捡（初速会带它轻微位移，用实际位置断言）。
        let p = es.get(1001).unwrap().pos;
        assert_eq!(es.pickable(p), vec![1001]);
        // 远处不可捡。
        assert!(es.pickable((10.5, 69.0, 10.5)).is_empty());
        // 1.5 边界：1.49 可捡，1.51 不可（相对实际位置）。
        assert_eq!(es.pickable((p.0 + 1.49, p.1, p.2)), vec![1001]);
        assert!(es.pickable((p.0 + 1.51, p.1, p.2)).is_empty());
    }

    #[test]
    fn unsupported_drop_resumes_fall_to_new_rest_height() {
        let mut es = ItemEntities::new();
        // 掉落物静止在 y=69（支撑方块 (0,68,0)）。
        spawn_one(&mut es, 1001, 0.5, 70.0, 0.5, 69.0);
        for _ in 0..200 {
            es.tick();
        }
        let e = es.get(1001).unwrap();
        assert!(e.on_ground);
        assert_eq!(e.support, Some((0, 68, 0)));
        assert_eq!(e.rest_y, 69.0);

        // 支撑还在 → 无需重算。
        assert!(es.find_unsupported(|_| true).is_empty());
        // 支撑被挖 → 返回该实体，重算后恢复下落。
        let unsupported = es.find_unsupported(|_| false);
        assert_eq!(unsupported, vec![(1001, (0, 68, 0))]);
        assert!(es.resume_fall(1001, 67.0));
        let e = es.get(1001).unwrap();
        assert_eq!(e.rest_y, 67.0);
        assert!(!e.on_ground);
        assert_eq!(e.support, Some((0, 66, 0)));
        for _ in 0..200 {
            es.tick();
        }
        let e = es.get(1001).unwrap();
        assert!(e.on_ground);
        assert_eq!(e.pos.1, 67.0);
    }

    /// P28：显式初速只用于新落子实体（合并吸收不改变既有速度）。
    #[test]
    fn spawn_with_velocity_keeps_explicit_vel_and_merge_semantics() {
        let mut es = ItemEntities::new();
        es.spawn(1001, 55, 60, 0.5, 70.0, 0.5, 69.0).unwrap();
        let before = es.get(1001).unwrap().vel;
        // 同位同物 +10：合并进 1001（64 上限，剩余 6 落新实体 1002 带显式初速）。
        let touched =
            es.spawn_with_velocity(1002, 55, 10, 0.5, 70.0, 0.5, 69.0, (0.0, 0.25, 0.06))
                .unwrap();
        assert_eq!(es.get(1001).unwrap().count, 64);
        assert_eq!(es.get(1001).unwrap().vel, before, "merge must not touch existing velocity");
        assert_eq!(es.get(1002).unwrap().count, 6);
        assert_eq!(es.get(1002).unwrap().vel, (0.0, 0.25, 0.06));
        assert!(touched.contains(&1001) && touched.contains(&1002));
    }

    #[test]
    fn in_view_filters_by_chunk_radius() {
        let mut es = ItemEntities::new();
        spawn_one(&mut es, 1001, 0.5, 70.0, 0.5, 69.0); // chunk (0,0)
        spawn_one(&mut es, 1002, 100.5, 70.0, 100.5, 69.0); // chunk (6,6)
        assert_eq!(es.in_view(ChunkPos(0, 0), 2), vec![1001]);
        let mut both = es.in_view(ChunkPos(3, 3), 3);
        both.sort();
        assert_eq!(both, vec![1001, 1002]);
    }

    /// R4 回归（服务端状态级验证，真机手感 T1–T13 仍需人工确认）：
    /// 水平漂移跨到相邻列后，支撑被挖空只能用「生成时的原列」重算，
    /// 不能用漂移后的列；`chunk` 随位置更新，但 `support` 的 x/z 不跟着漂。
    #[test]
    fn resume_fall_keeps_original_support_column_after_horizontal_drift() {
        let mut es = ItemEntities::new();
        // 原列 (0,0)，支撑方块 (0,68,0)。从高处落下 + 明显水平速度，保证跨列。
        spawn_one(&mut es, 1001, 0.5, 100.0, 0.5, 69.0);
        let original_support = es.get(1001).unwrap().support;
        assert_eq!(original_support, Some((0, 68, 0)));
        {
            let e = es.entities.iter_mut().find(|e| e.id == 1001).unwrap();
            e.vel = (0.5, 0.25, 0.4);
        }
        let mut landed = false;
        for _ in 0..300 {
            es.tick();
            if es.get(1001).unwrap().on_ground {
                landed = true;
                break;
            }
        }
        assert!(landed, "drop must land before its support is dug");
        let e = es.get(1001).unwrap();
        let drifted_col = (e.pos.0.floor() as i32, e.pos.2.floor() as i32);
        assert_ne!(drifted_col, (0, 0), "horizontal velocity must cross into a neighbouring column");
        assert_eq!(e.support, original_support, "resting support stays on the original column");

        // 原列支撑被挖空 → resume_fall 只更新 y，x/z 必须仍是原列。
        assert!(es.find_unsupported(|_| false).contains(&(1001, original_support.unwrap())));
        assert!(es.resume_fall(1001, 67.0));
        let e = es.get(1001).unwrap();
        assert_eq!(e.support, Some((0, 66, 0)), "x/z must not follow the drifted column");
        assert_eq!(e.rest_y, 67.0, "rest_y must follow the new support height");
        assert!(!e.on_ground, "support loss must resume falling");
    }
}
