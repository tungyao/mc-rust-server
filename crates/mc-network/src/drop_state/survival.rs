//! drop_state/survival: P40 生存状态的全局 ticker 推进与死亡闭环（只兼容 26.3/777）。
//!
//! 权威状态只在 `super::DropState::physics_tick` 的唯一 ticker 内推进；连接侧只发
//! SetHealth/背包/位置同步包（P18.1 R1：连接侧绝不 tick，避免多连接倍速）。
//! 死亡时按既有掉落路径生成 ItemEntity 并经 BroadcastHub fan-out，清空背包、
//! 恢复满状态、位置回 `WorldGen::spawn_pos`（PlayerPositionSync 0x49 + SetHealth）。

use super::DropState;
use crate::login_route::drop_sync::build_full_inventory_body;
use crate::login_route::survival_sync::{build_respawn_position_body, build_set_health_body};
use mc_protocol::packets;
use mc_world::{MutableWorld, SurvivalState};
use std::sync::atomic::Ordering;

impl DropState {
    fn lock_surv(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<u64, SurvivalState>> {
        self.survival.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 只读快照（Join 发包/测试）。
    pub fn survival_state(&self, conn_id: u64) -> Option<SurvivalState> {
        self.lock_surv().get(&conn_id).copied()
    }

    /// 原地修改生存状态，并同步写回背包（背包是存档真相源，断线/周期刷盘自动带上）。
    /// 返回修改后的快照。
    pub fn apply_survival<R>(
        &self,
        conn_id: u64,
        f: impl FnOnce(&mut SurvivalState) -> R,
    ) -> Option<(SurvivalState, R)> {
        let mut survs = self.lock_surv();
        let state = survs.get_mut(&conn_id)?;
        let out = f(state);
        let snapshot = *state;
        drop(survs);
        // 锁序恒为 survival → inventory（load_inventory 同序），避免死锁。
        if let Some(inv) = self.lock_invs().get_mut(&conn_id) {
            inv.set_survival(snapshot);
        }
        Some((snapshot, out))
    }

    /// 坠落伤害（连接侧落地判定调用）；返回扣血后的状态快照。
    pub fn apply_fall(&self, conn_id: u64, blocks: f64) -> Option<SurvivalState> {
        let (after, _) = self.apply_survival(conn_id, |s| {
            s.fall_damage(blocks);
        })?;
        Some(after)
    }

    /// 进食：调用方负责先扣手持食物 1 个，这里只加饥饿/饱和。
    pub fn apply_food(&self, conn_id: u64, food: i32, saturation: f32) -> Option<SurvivalState> {
        let (after, _) = self.apply_survival(conn_id, |s| {
            s.eat(food, saturation);
        })?;
        Some(after)
    }

    /// 测试/配置：饥饿每 N 个全局 tick 扣一格（生产 400=20s）。
    pub fn set_hunger_ticks(&self, n: u64) {
        self.hunger_ticks.store(n.max(1), Ordering::Relaxed);
    }

    /// 测试/配置：饥饿见底后每 N 个全局 tick 扣 1 点血（生产 200=10s）。
    pub fn set_starvation_ticks(&self, n: u64) {
        self.starvation_ticks.store(n.max(1), Ordering::Relaxed);
    }

    pub fn hunger_ticks(&self) -> u64 {
        self.hunger_ticks.load(Ordering::Relaxed)
    }

    pub fn starvation_ticks(&self) -> u64 {
        self.starvation_ticks.load(Ordering::Relaxed)
    }

    /// 全服唯一 ticker 内调用：按全局 tick 模节拍推进饥饿与饿死伤害；
    /// 非满状态经 hub 发 SetHealth；死亡走 `handle_death` 闭环。
    pub(crate) fn tick_survival(&self, world: &MutableWorld) {
        let now = self.tick_count.load(Ordering::Relaxed);
        let hunger_due = now % self.hunger_ticks().max(1) == 0;
        let starve_due = now % self.starvation_ticks().max(1) == 0;
        // 注意：死亡检测必须每 tick 都跑（否则伤害恰好落在两个饥饿节拍之间时，
        // 尸体要等到下一个节拍才处理，甚至可能一直卡住）；只有衰减/扣血按节拍。
        let mut changed: Vec<(u64, SurvivalState)> = Vec::new();
        let mut dead: Vec<u64> = Vec::new();
        {
            let mut survs = self.lock_surv();
            let mut invs = self.lock_invs();
            for (conn_id, state) in survs.iter_mut() {
                let mut dirty = false;
                if hunger_due && state.decay_one() {
                    dirty = true;
                }
                if starve_due && state.food == 0 && !state.is_dead() {
                    state.starve_damage();
                    dirty = true;
                }
                if dirty {
                    if let Some(inv) = invs.get_mut(conn_id) {
                        inv.set_survival(*state);
                    }
                    changed.push((*conn_id, *state));
                }
                if state.is_dead() {
                    dead.push(*conn_id);
                }
            }
        }
        for (conn_id, state) in changed {
            self.unicast_health(conn_id, &state);
        }
        for conn_id in dead {
            self.handle_death(conn_id, world);
        }
    }

    /// 经广播 hub 给单连接排队一个 SetHealth（ticker 内无 socket）。
    fn unicast_health(&self, conn_id: u64, state: &SurvivalState) {
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(hub) = hub.as_ref() {
            hub.unicast(conn_id, packets::PLAY_SET_HEALTH_S2C_ID, build_set_health_body(state));
        }
    }

    /// 死亡闭环：背包全部内容按既有掉落路径生成实体并广播；清空背包；
    /// 满状态复活；回出生点（PlayerPositionSync 0x49 + SetHealth + 全量 Content）。
    fn handle_death(&self, conn_id: u64, world: &MutableWorld) {
        let pos = self.death_position(conn_id, world);
        let (items, tag) = {
            let mut invs = self.lock_invs();
            let Some(inv) = invs.get_mut(&conn_id) else { return };
            let items: Vec<(i32, i32)> = (0..mc_world::inventory::SLOT_COUNT)
                .filter_map(|i| inv.get(i))
                .collect();
            inv.clear_all();
            inv.set_survival(SurvivalState::full());
            (items, self.lock_tags().get(&conn_id).cloned())
        };
        if let Some(state) = self.apply_survival(conn_id, |s| *s = SurvivalState::full()) {
            self.unicast_health(conn_id, &state.0);
        }
        if let Some(tag) = tag {
            let invs = self.lock_invs();
            if let Some(inv) = invs.get(&conn_id) {
                self.persist_inventory(&tag, inv);
            }
        }
        let spawn = world.spawn_pos();
        let teleport_id = self.alloc_id();
        let chunk = mc_world::chunk_of_block(spawn.0, spawn.2);
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner()).clone();
        for (item_id, count) in items {
            let id = self.alloc_id();
            let rest = world.drop_rest_y(spawn.0, spawn.2, spawn.1) as f64;
            let spawned = {
                let mut entities = self.items.lock().unwrap_or_else(|p| p.into_inner());
                entities
                    .spawn(
                        id,
                        item_id,
                        count,
                        pos.0 + 0.5,
                        pos.1,
                        pos.2 + 0.5,
                        rest,
                    )
                    .is_ok()
            };
            if !spawned {
                continue;
            }
            let mut spawn_body = Vec::new();
            packets::SpawnItemEntity {
                entity_id: id,
                uuid: mc_world::items::uuid_for_entity(id),
                x: pos.0 + 0.5,
                y: pos.1,
                z: pos.2 + 0.5,
                vel: packets::LpVec3::zero(),
                pitch: 0,
                yaw: 0,
                head_yaw: 0,
            }
            .encode_body(&mut spawn_body);
            let mut data_body = Vec::new();
            packets::ItemEntityData {
                entity_id: id,
                slot: packets::Slot { count, item_id },
            }
            .encode_body(&mut data_body);
            if let Some(hub) = hub.as_ref() {
                hub.unicast(conn_id, packets::PLAY_SPAWN_ENTITY_S2C_ID, spawn_body.clone());
                hub.unicast(conn_id, packets::PLAY_SET_ENTITY_DATA_S2C_ID, data_body.clone());
                for viewer in hub.recipients_in_view(conn_id, chunk) {
                    hub.unicast(viewer, packets::PLAY_SPAWN_ENTITY_S2C_ID, spawn_body.clone());
                    hub.unicast(viewer, packets::PLAY_SET_ENTITY_DATA_S2C_ID, data_body.clone());
                    self.mark_known(viewer, id);
                }
            }
            self.mark_known(conn_id, id);
        }
        if let Some(hub) = hub.as_ref() {
            if let Some(body) = build_full_inventory_body(self, conn_id) {
                hub.unicast(conn_id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, body);
            }
            hub.unicast(
                conn_id,
                packets::PLAY_PLAYER_POSITION_SYNC_ID,
                build_respawn_position_body((spawn.0, spawn.1, spawn.2), teleport_id),
            );
            hub.update_pos(conn_id, (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5));
        }
    }

    /// 死亡掉落位置优先用 hub 记录的最近位置，没注册时回出生点。
    fn death_position(&self, conn_id: u64, world: &MutableWorld) -> (f64, f64, f64) {
        let hub = self.hub.lock().unwrap_or_else(|p| p.into_inner());
        let pos = hub
            .as_ref()
            .and_then(|h| h.snapshot().into_iter().find(|e| e.conn_id == conn_id).map(|e| e.pos));
        pos.unwrap_or_else(|| {
            let s = world.spawn_pos();
            (s.0 as f64 + 0.5, s.1 as f64, s.2 as f64 + 0.5)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drop_state::DropState;
    use mc_world::survival::{MAX_FOOD, MAX_HEALTH, MAX_SATURATION};
    use std::sync::Arc;

    fn ds() -> DropState {
        DropState::new(std::env::temp_dir().join(format!("mc-p40-surv-{}", std::process::id())), 7)
    }

    #[test]
    fn hunger_ticker_decays_and_starves_only_on_global_tick() {
        let state = ds();
        state.set_hunger_ticks(1);
        state.set_starvation_ticks(1);
        state.lock_surv().insert(3, SurvivalState { health: 20.0, food: 0, saturation: 0.0 });
        // 不调用 ticker 时状态不变（连接侧绝不 tick）。
        assert_eq!(state.survival_state(3).unwrap().health, 20.0);
        // 直接验证 tick 主体（单测无 socket：hub 为 None，只改权威状态）。
        let world = Arc::new(MutableWorld::new(7));
        state.tick_survival(&world);
        let after = state.survival_state(3).unwrap();
        assert_eq!(after.health, 19.0);
        assert_eq!(after.food, 0);
        assert_eq!(after.saturation, 0.0);
    }

    #[test]
    fn death_clears_inventory_drops_items_and_respawns_full() {
        let state = ds();
        let world = Arc::new(MutableWorld::new(7));
        state.load_inventory(9, "dead-player");
        state.with_inventory(9, |inv| {
            inv.set_slot(36, Some((mc_world::drops::ITEM_STONE, 5)));
        });
        state.apply_survival(9, |s| {
            *s = SurvivalState { health: 0.0, food: 3, saturation: 1.0 };
        });
        state.tick_survival(&world);
        assert!(state.with_inventory(9, |inv| inv.is_empty()).unwrap_or(false));
        let after = state.survival_state(9).unwrap();
        assert_eq!((after.health, after.food, after.saturation), (MAX_HEALTH, MAX_FOOD, MAX_SATURATION));
        let drops = state.items.lock().unwrap();
        let ids = drops.ids();
        assert_eq!(ids.len(), 1, "inventory contents must become item entities");
        let e = drops.get(ids[0]).unwrap();
        assert_eq!(e.item_id, mc_world::drops::ITEM_STONE);
        assert_eq!(e.count, 5);
    }
}
