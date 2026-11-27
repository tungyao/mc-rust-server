//! world_time: A6 全局时间/天气服务端状态（P54 batch1-infra）。
//!
//! 口径：
//! - `tick` 是原版 `set_time 0x73` 的 long tick（24000 = 一天），由**全服唯一**
//!   50ms 物理 ticker 推进（挂在 `DropState::physics_tick` 里，绝不随连接数倍增）。
//! - `weather` / `weather_until` 只在服务端推进并落盘；天气 `game_event 0x27` 的
//!   type 值真机未确认，**本实现不猜字节、不发包**（即使 `weather_broadcast` 开关
//!   打开也只更新服务端状态与后台显示，待抓包确认后再补广播）。
//! - 时间/天气缺省 = 白天(1000 tick)/晴；旧存档缺 `meta.world_time`/`meta.weather`
//!   时按缺省读，不触碰 P53 的 v4 存档/stats 表。
//! - 本轮选择「手动切换 + 可选 duration 到期回晴」，不做 seed 派生的随机昼夜循环：
//!   自动循环会改变现有真机截图/存档基线，且 §5.2 Q6 尚未给出用户决策。

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

/// 原版一天 tick 数（set_time 口径）。
pub const TICKS_PER_DAY: i64 = 24_000;
/// 缺省白天时刻（客户端默认 1000 = 日出后）。
pub const DEFAULT_DAY_TICK: i64 = 1000;
/// 缺省夜晚时刻。
pub const DEFAULT_NIGHT_TICK: i64 = 13_000;

/// 天气（服务端状态；数值只用于本地存档/API，不是网络包 type 值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Weather {
    #[default]
    Clear,
    Rain,
    Thunder,
}

impl Weather {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Rain => "rain",
            Self::Thunder => "thunder",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "clear" | "sunny" | "晴" | "0" => Some(Self::Clear),
            "rain" | "rainy" | "雨" | "1" => Some(Self::Rain),
            "thunder" | "thunderstorm" | "雷暴" | "2" => Some(Self::Thunder),
            _ => None,
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::Clear => 0,
            Self::Rain => 1,
            Self::Thunder => 2,
        }
    }

    /// 存档/后台整数值 → 天气（未知值按晴，保持旧档兼容）。
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::Rain,
            2 => Self::Thunder,
            _ => Self::Clear,
        }
    }
}

/// 全局时间/天气状态：一个进程一份，`Arc` 进 `NetConfig` 与 admin 注入口。
#[derive(Debug)]
pub struct WorldTimeState {
    tick: AtomicI64,
    weather: AtomicU64,
    ///  absolut long tick；到达后天气回晴（0 = 永久）。
    weather_until: AtomicI64,
    /// 天气只有服务端状态（不发包：0x27 type 真机未确认），无广播开关。
    /// 是否自动推进 tick（生产开；旧测试入口关可保持逐字节包流）。
    advance: AtomicBool,
}

impl Default for WorldTimeState {
    fn default() -> Self {
        Self {
            tick: AtomicI64::new(DEFAULT_DAY_TICK),
            weather: AtomicU64::new(Weather::Clear.code() as u64),
            weather_until: AtomicI64::new(0),
            advance: AtomicBool::new(true),
        }
    }
}

impl WorldTimeState {
    /// 从存档恢复（缺键/损坏按缺省白天/晴）。种子不参与循环：本轮只手动切换。
    pub fn from_store(store: Option<&crate::store::WorldStore>, seed: i64) -> Self {
        let me = Self::default();
        let Some(store) = store else { return me };
        if let Some(v) = store.world_time().ok().flatten() {
            me.tick.store(v.rem_euclid(TICKS_PER_DAY), Ordering::Relaxed);
        }
        if let Some(v) = store.weather().ok().flatten() {
            me.weather.store(Weather::from_code(v).code() as u64, Ordering::Relaxed);
        }
        let _ = seed; // 保留参数以便未来 seed 派生；本轮明确不使用。
        me
    }

    pub fn tick(&self) -> i64 {
        self.tick.load(Ordering::Relaxed)
    }

    pub fn set_tick(&self, tick: i64) {
        self.tick.store(tick.rem_euclid(TICKS_PER_DAY), Ordering::Relaxed);
    }

    /// 唯一 50ms ticker 调用：推进 long tick，并在到期时把天气切回晴。
    /// 返回 `true` 表示天气发生了切换（调用方可记录日志；当前不发包）。
    pub fn advance_tick(&self) -> bool {
        if !self.advance.load(Ordering::Relaxed) {
            return false;
        }
        self.tick.fetch_add(1, Ordering::Relaxed);
        let t = self.tick.load(Ordering::Relaxed);
        if t >= TICKS_PER_DAY {
            self.tick.store(t.rem_euclid(TICKS_PER_DAY), Ordering::Relaxed);
        }
        let until = self.weather_until.load(Ordering::Relaxed);
        if until > 0 && self.tick.load(Ordering::Relaxed) == until {
            self.weather.store(Weather::Clear.code() as u64, Ordering::Relaxed);
            self.weather_until.store(0, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn set_advance(&self, enabled: bool) {
        self.advance.store(enabled, Ordering::Relaxed);
    }

    pub fn weather(&self) -> Weather {
        Weather::from_code(self.weather.load(Ordering::Relaxed) as u8)
    }

    /// 设置天气；`duration_ticks=Some(n)` 表示 n 个 long tick 后回晴（0 = 永久）。
    pub fn set_weather(&self, weather: Weather, duration_ticks: Option<i64>) {
        self.weather.store(weather.code() as u64, Ordering::Relaxed);
        let until = match duration_ticks {
            Some(n) if n > 0 && n < TICKS_PER_DAY => {
                (self.tick.load(Ordering::Relaxed) + n).rem_euclid(TICKS_PER_DAY)
            }
            _ => 0,
        };
        self.weather_until.store(until, Ordering::Relaxed);
    }

    pub fn weather_until(&self) -> i64 {
        self.weather_until.load(Ordering::Relaxed)
    }

    /// 存档快照（由 ticker  periodic 或 API 显式调用写 meta）。
    pub fn snapshot(&self) -> (i64, u8) {
        (self.tick.load(Ordering::Relaxed), self.weather().code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_daylight_and_clear() {
        let state = WorldTimeState::default();
        assert_eq!(state.tick(), DEFAULT_DAY_TICK);
        assert_eq!(state.weather(), Weather::Clear);
        assert_eq!(Weather::parse("thunderstorm"), Some(Weather::Thunder));
    }

    #[test]
    fn ticker_wraps_day_and_expires_weather() {
        let state = WorldTimeState::default();
        state.set_tick(TICKS_PER_DAY - 1);
        state.set_weather(Weather::Rain, Some(2));
        assert!(!state.advance_tick(), "tick 0 crosses day, weather not yet expired");
        assert_eq!(state.tick(), 0);
        assert_eq!(state.weather(), Weather::Rain);
        assert!(state.advance_tick(), "weather must expire inside the unique ticker");
        assert_eq!(state.weather(), Weather::Clear);
        assert_eq!(state.weather_until(), 0);
    }

    #[test]
    fn manual_advance_switch_does_not_change_state() {
        let state = WorldTimeState::default();
        state.set_advance(false);
        let t = state.tick();
        assert!(!state.advance_tick());
        assert_eq!(state.tick(), t);
    }
}
