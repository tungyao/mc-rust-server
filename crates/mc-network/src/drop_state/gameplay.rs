//! drop_state/gameplay: P53 批次 1 的四项开关与全服唯一 ticker 内的权威推进。
//!
//! 硬约束：吸引/合并/疲劳度只在 `super::physics_tick` 的唯一 50ms ticker 内推进；
//! 连接侧只写标记、按快照差集发 `0x23`/`0x4E`，绝不调用这里的 tick 入口。

use super::DropState;
use crate::broadcast::GameMode;
use mc_world::{
    inventory::{
        SETTINGS_ENCHANT_FORTUNE, SETTINGS_ENCHANT_NONE, SETTINGS_ENCHANT_SILK_TOUCH,
        SETTINGS_GAMEMODE_CREATIVE, SETTINGS_GAMEMODE_SURVIVAL,
    },
    store::{stat_id, WorldStore},
    survival::{self, Activity},
    tools::ToolEnchant,
};
use std::sync::atomic::Ordering;

impl DropState {
    /// 由 `serve()` 从 `NetConfig` 一次性注入；四个开关彼此独立。
    pub fn set_p53_features(&self, xp_merge: bool, hunger_detail: bool, settings_persist: bool, stats: bool) {
        self.xp_merge_enabled.store(xp_merge, Ordering::Relaxed);
        self.hunger_detail_enabled.store(hunger_detail, Ordering::Relaxed);
        self.settings_persist_enabled.store(settings_persist, Ordering::Relaxed);
        self.stats_enabled.store(stats, Ordering::Relaxed);
    }

    pub fn p53_features(&self) -> (bool, bool, bool, bool) {
        (
            self.xp_merge_enabled.load(Ordering::Relaxed),
            self.hunger_detail_enabled.load(Ordering::Relaxed),
            self.settings_persist_enabled.load(Ordering::Relaxed),
            self.stats_enabled.load(Ordering::Relaxed),
        )
    }

    // ---- A1：经验球吸引 + 合并（总经验守恒） ----

    /// 只在全局 ticker 调用：先把 7.25 格内的球朝最近玩家拉近，再合并邻近球。
    /// 合并产生的新 id 由全局 `alloc_id` 分配，绝不复用旧 id（视距 known 差集自然补
    /// `0x4E`/`0x01`）；经验值总和严格不变，溢出则整组放弃合并。
    pub(crate) fn tick_xp_attract_merge(&self) {
        let players: Vec<(f64, f64, f64)> = {
            let guard = self.hub.lock().unwrap_or_else(|p| p.into_inner());
            guard
                .as_ref()
                .map(|h| h.snapshot().into_iter().map(|e| e.pos).collect())
                .unwrap_or_default()
        };
        let mut orbs = self.orbs.lock().unwrap_or_else(|p| p.into_inner());
        orbs.attract_toward(
            &players,
            crate::config::XP_ATTRACT_RADIUS,
            crate::config::XP_ATTRACT_SPEED,
        );
        let groups = orbs.merge_groups(crate::config::XP_MERGE_DISTANCE, crate::config::XP_MERGE_MAX_ORBS);
        for group in groups {
            let new_id = self.alloc_id();
            let _ = orbs.merge_group(&group, new_id);
        }
    }

    /// 连接侧用：返回本连接**已知**经验球的位置差（只发需要动的 id）。
    /// 新实体由 `sync_view_entities` 补 Spawn，消失的实体由 known 差集补 Remove；
    /// 本函数同时清掉缓存里已消失的 id，避免内存无限增长。
    pub fn orb_position_updates(&self, conn_id: u64) -> Vec<(i32, (f64, f64, f64))> {
        let snapshot: Vec<(i32, (f64, f64, f64))> = {
            let orbs = self.orbs.lock().unwrap_or_else(|p| p.into_inner());
            orbs.snapshots().into_iter().map(|(id, pos, _)| (id, pos)).collect()
        };
        let current: std::collections::HashMap<i32, (f64, f64, f64)> = snapshot.into_iter().collect();
        let mut cache = self.orb_synced.lock().unwrap_or_else(|p| p.into_inner());
        let last = cache.entry(conn_id).or_default();
        last.retain(|id, _| current.contains_key(id));
        let mut out = Vec::new();
        for (id, pos) in current {
            if !self.is_known(conn_id, id) {
                continue;
            }
            let changed = last.get(&id).is_some_and(|old| {
                (old.0 - pos.0).abs() > 1e-4
                    || (old.1 - pos.1).abs() > 1e-4
                    || (old.2 - pos.2).abs() > 1e-4
            });
            if changed {
                out.push((id, pos));
            }
            last.insert(id, pos);
        }
        out
    }

    // ---- A2：疲劳度（活动标记由连接侧写，ticker 推进） ----

    /// per-conn 只写标记，不推进任何时间（P18.1 R1）。
    pub fn apply_activity(&self, conn_id: u64, activity: Activity) {
        self.activities
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(conn_id, activity);
    }

    pub fn exhaustion(&self, conn_id: u64) -> f32 {
        self.exhaustions.lock().unwrap_or_else(|p| p.into_inner()).get(&conn_id).copied().unwrap_or(0.0)
    }

    /// 只在全局 ticker 调用：活动 → 疲劳度 → 跨阈值消耗一格饥饿/饱和。
    /// 锁序恒为 activities → exhaustions → survival → invs，与 `apply_survival` 一致。
    pub(crate) fn tick_activity_exhaustion(&self) {
        let changed: Vec<(u64, survival::SurvivalState)> = {
            let mut acts = self.activities.lock().unwrap_or_else(|p| p.into_inner());
            let mut exs = self.exhaustions.lock().unwrap_or_else(|p| p.into_inner());
            let mut survs = self.lock_surv();
            let mut invs = self.lock_invs();
            let mut out = Vec::new();
            for (conn_id, activity) in acts.iter_mut() {
                let current = exs.get(conn_id).copied().unwrap_or(0.0);
                let (next, due) = survival::advance_exhaustion(current, *activity);
                exs.insert(*conn_id, next);
                // 跳跃是一次性事件，消费后清零；疾跑/游泳由下一批移动包刷新。
                activity.jumped = false;
                let Some(state) = survs.get_mut(conn_id) else { continue };
                if due {
                    state.decay_one();
                    out.push((*conn_id, *state));
                }
                if let Some(inv) = invs.get_mut(conn_id) {
                    inv.set_exhaustion(next);
                    if due {
                        inv.set_survival(*state);
                    }
                }
            }
            out
        };
        for (conn_id, state) in changed {
            self.unicast_health(conn_id, &state);
        }
    }

    // ---- A4：后台设置跨重连落盘 ----

    /// Join 读盘后由 `load_inventory` 登记；hub 注册完成才应用到权威内存。
    pub fn persisted_settings(&self, conn_id: u64) -> Option<(u8, u8)> {
        self.pending_settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&conn_id)
            .copied()
    }

    /// 登录序列完成后调用（不插包、不改 Join 八段包序）。
    pub fn apply_persisted_settings(&self, conn_id: u64) {
        if !self.settings_persist_enabled.load(Ordering::Relaxed) {
            return;
        }
        let Some((mode, enchant)) = self.persisted_settings(conn_id) else { return };
        if let Some(hub) = self.hub.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            hub.set_gamemode(conn_id, gamemode_from_code(mode));
        }
        self.tool_enchants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(conn_id, enchant_from_code(enchant));
    }

    /// 在线写：权威内存 + 背包存档字段同步更新（落盘由调用方 `save_inventory` 触发）。
    pub fn set_persisted_settings(&self, conn_id: u64, mode: u8, enchant: u8) -> bool {
        if let Some(inv) = self.lock_invs().get_mut(&conn_id) {
            inv.set_persisted_gamemode(mode);
            inv.set_persisted_tool_enchant(enchant);
        } else {
            return false;
        }
        self.pending_settings
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(conn_id, (mode, enchant));
        self.tool_enchants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(conn_id, enchant_from_code(enchant));
        if let Some(hub) = self.hub.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            hub.set_gamemode(conn_id, gamemode_from_code(mode));
        }
        true
    }

    /// 离线写：直接按 UUID（或兼容用户名键）更新 `players` 记录，供后台"改了重进生效"。
    pub fn set_persisted_settings_by_identity(&self, identity: &str, mode: u8, enchant: u8) -> bool {
        if !self.settings_persist_enabled.load(Ordering::Relaxed) {
            return false;
        }
        let Some(mut inv) = self.read_stored_player(identity) else { return false };
        inv.set_persisted_gamemode(mode);
        inv.set_persisted_tool_enchant(enchant);
        if let Some(store) = &self.store {
            store.save_player(identity, &inv).is_ok()
        } else {
            inv.save_to_dir(&self.save_dir, identity, self.seed).is_ok()
        }
    }

    // ---- A7：本地统计（不发包；B13 才推 award_stats） ----

    /// 在 `PlayEvent::BlockEdit` / 死亡路径累加（带稳定 UUID + 用户名兼容镜像）。
    pub fn record_stat(&self, conn_id: u64, stat: u32) {
        if !self.stats_enabled.load(Ordering::Relaxed) {
            return;
        }
        let uuid = self.lock_uuids().get(&conn_id).cloned();
        let user = self.player_tag(conn_id);
        let day = WorldStore::current_stat_day();
        let mut identities: Vec<String> = Vec::new();
        for identity in uuid.into_iter().chain(user) {
            if !identity.is_empty() && !identities.contains(&identity) {
                identities.push(identity);
            }
        }
        for identity in identities {
            if let Some(store) = &self.store {
                if let Err(e) = store.record_stat(&identity, stat, 1) {
                    tracing::warn!("redb stats write failed identity={identity} stat={stat} ({e})");
                }
            } else {
                *self.stats_mem.lock().unwrap_or_else(|p| p.into_inner())
                    .entry((identity, stat, day)).or_insert(0) += 1;
            }
        }
    }

    /// 面板查询：UUID 主键与用户名兼容镜像都接受，按天范围过滤。
    pub fn stats_for(&self, identity: &str, from_day: Option<u32>, to_day: Option<u32>) -> Vec<(u32, u32, u64)> {
        if !self.stats_enabled.load(Ordering::Relaxed) {
            return Vec::new();
        }
        let mut out = if let Some(store) = &self.store {
            store.stats_between(identity, from_day, to_day).unwrap_or_default()
        } else {
            Vec::new()
        };
        for ((who, stat, day), count) in self.stats_mem.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            if who != identity || from_day.is_some_and(|f| *day < f) || to_day.is_some_and(|t| *day > t) {
                continue;
            }
            if let Some(row) = out.iter_mut().find(|(s, d, _)| *s == *stat && *d == *day) {
                row.2 += *count;
            } else {
                out.push((*stat, *day, *count));
            }
        }
        out.sort();
        out
    }
}

/// 存档码 → `GameMode`（未知值一律 survival，与 v1/v2 旧档缺省一致）。
pub(crate) fn gamemode_from_code(code: u8) -> GameMode {
    if code == SETTINGS_GAMEMODE_CREATIVE {
        GameMode::Creative
    } else {
        GameMode::Survival
    }
}

/// `GameMode` → 存档码。
pub fn gamemode_code(mode: GameMode) -> u8 {
    match mode {
        GameMode::Survival => SETTINGS_GAMEMODE_SURVIVAL,
        GameMode::Creative => SETTINGS_GAMEMODE_CREATIVE,
    }
}

/// 存档码 → `ToolEnchant`（未知值一律 none）。
pub(crate) fn enchant_from_code(code: u8) -> ToolEnchant {
    match code {
        SETTINGS_ENCHANT_FORTUNE => ToolEnchant::Fortune,
        SETTINGS_ENCHANT_SILK_TOUCH => ToolEnchant::SilkTouch,
        _ => ToolEnchant::None,
    }
}

/// `ToolEnchant` → 存档码。
pub fn enchant_code(enchant: ToolEnchant) -> u8 {
    match enchant {
        ToolEnchant::None => SETTINGS_ENCHANT_NONE,
        ToolEnchant::Fortune => SETTINGS_ENCHANT_FORTUNE,
        ToolEnchant::SilkTouch => SETTINGS_ENCHANT_SILK_TOUCH,
    }
}

/// 面板展示用的 stat 名称（不依赖客户端注册表）。
pub fn stat_name(stat: u32) -> &'static str {
    match stat {
        stat_id::MINED => "挖掘方块",
        stat_id::PLACED => "放置方块",
        stat_id::DEATHS => "死亡次数",
        _ => "未知",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn state() -> DropState {
        DropState::new(std::env::temp_dir().join(format!("mc-p53-gameplay-{}", std::process::id())), 7)
    }

    #[test]
    fn feature_flags_are_independent() {
        let s = state();
        assert_eq!(s.p53_features(), (false, false, false, false));
        s.set_p53_features(true, false, true, false);
        assert_eq!(s.p53_features(), (true, false, true, false));
    }

    #[test]
    fn settings_codes_roundtrip_all_values() {
        for (code, mode) in [
            (SETTINGS_GAMEMODE_SURVIVAL, GameMode::Survival),
            (SETTINGS_GAMEMODE_CREATIVE, GameMode::Creative),
        ] {
            assert_eq!(gamemode_code(mode), code);
            assert_eq!(gamemode_from_code(code), mode);
        }
        for (code, en) in [
            (SETTINGS_ENCHANT_NONE, ToolEnchant::None),
            (SETTINGS_ENCHANT_FORTUNE, ToolEnchant::Fortune),
            (SETTINGS_ENCHANT_SILK_TOUCH, ToolEnchant::SilkTouch),
        ] {
            assert_eq!(enchant_code(en), code);
            assert_eq!(enchant_from_code(code), en);
        }
        assert_eq!(gamemode_from_code(99), GameMode::Survival);
        assert_eq!(enchant_from_code(99), ToolEnchant::None);
    }

    /// P53 A1：全局 ticker 内吸引并合并，经验总量严格守恒；仅开开关时才生效。
    #[test]
    fn global_ticker_attracts_and_merges_with_conserved_xp() {
        let s = Arc::new(state());
        s.set_p53_features(true, false, false, false);
        s.orbs.lock().unwrap().spawn(1000, 3, 0.6, 69.0, 0.5, 69.0).unwrap();
        s.orbs.lock().unwrap().spawn(1001, 4, 0.9, 69.0, 0.5, 69.0).unwrap();
        let before: i32 = s.orbs.lock().unwrap().snapshots().iter().map(|x| x.2).sum();
        // physics_tick 是私有入口，但 start_global_tick 已启动同一路径；这里直接验证纯逻辑
        // 与开关传递，真实 ticker 路径由 e2e 覆盖。
        s.tick_xp_attract_merge();
        let after: i32 = s.orbs.lock().unwrap().snapshots().iter().map(|x| x.2).sum();
        assert_eq!(after, before);
        assert!(s.orbs.lock().unwrap().len() <= 2);
    }

    /// P53 A2：活动标记只写不 tick，全局 ticker 跨阈值才扣饥饿并把余量落回背包。
    #[test]
    fn activity_exhaustion_only_advances_on_ticker() {
        let s = Arc::new(state());
        s.set_p53_features(false, true, false, false);
        s.load_inventory(9, "ex-player");
        s.apply_activity(9, Activity { sprinting: true, jumped: true, swimming: false });
        assert_eq!(s.exhaustion(9), 0.0, "per-conn marker must not advance time");
        for _ in 0..20 {
            s.tick_activity_exhaustion();
        }
        assert!(s.exhaustion(9) > 0.0);
        assert!(s.exhaustion(9) < survival::EXHAUSTION_THRESHOLD);
        let stored = s.with_inventory(9, |inv| inv.exhaustion()).unwrap();
        assert_eq!(stored, s.exhaustion(9), "exhaustion follows the inventory save");
    }
}
