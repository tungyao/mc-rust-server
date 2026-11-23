//! config: NetConfig + 常量，只兼容26.3/777
use std::sync::{Arc, atomic::AtomicUsize};

/// Status 回包默认配置；`NetConfig::default()` 的回退值。
pub const STATUS_MAX_PLAYERS: usize = 100;
pub const STATUS_MOTD: &str = "Rust MC 26.3 (777) self-made";
/// 默认视距：只透传（P5C 从 ServerConfig 注入），不管移动视距。
pub const DEFAULT_VIEW_DISTANCE: i32 = 10;

/// 网络配置：由 mc-server 从 `ServerConfig` 透传。
#[derive(Debug, Clone)]
pub struct NetConfig {
    pub max_players: usize,
    pub motd: String,
    pub online_count: Arc<AtomicUsize>,
    pub view_distance: i32,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            max_players: STATUS_MAX_PLAYERS,
            motd: STATUS_MOTD.into(),
            online_count: Arc::new(AtomicUsize::new(0)),
            view_distance: DEFAULT_VIEW_DISTANCE,
        }
    }
}

impl NetConfig {
    /// 旧三参构造保留（兼容旧调用点），view_distance 取默认；
    /// 需要透传视距时链式 `.with_view_distance(v)` 或用 `new_with_view_distance`。
    pub fn new(max_players: usize, motd: String, online_count: Arc<AtomicUsize>) -> Self {
        Self { max_players, motd, online_count, view_distance: DEFAULT_VIEW_DISTANCE }
    }

    pub fn new_with_view_distance(
        max_players: usize,
        motd: String,
        online_count: Arc<AtomicUsize>,
        view_distance: i32,
    ) -> Self {
        Self { max_players, motd, online_count, view_distance }
    }

    pub fn with_view_distance(mut self, view_distance: i32) -> Self {
        self.view_distance = view_distance;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_new_still_compiles_with_default_view() {
        let cfg = NetConfig::new(42, "x".into(), Arc::new(AtomicUsize::new(0)));
        assert_eq!(cfg.view_distance, DEFAULT_VIEW_DISTANCE);
        assert_eq!(cfg.max_players, 42);
    }

    #[test]
    fn view_distance_passthrough() {
        let cfg = NetConfig::new(100, "m".into(), Arc::new(AtomicUsize::new(0))).with_view_distance(12);
        assert_eq!(cfg.view_distance, 12);
        let full = NetConfig::new_with_view_distance(100, "m".into(), Arc::new(AtomicUsize::new(0)), 6);
        assert_eq!(full.view_distance, 6);
        assert_eq!(NetConfig::default().view_distance, DEFAULT_VIEW_DISTANCE);
    }
}
