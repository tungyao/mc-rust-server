//! config: NetConfig + 常量，只兼容26.3/777
use super::play_events::PlayEvent;
use crossbeam_channel::Sender;
use std::sync::{Arc, atomic::AtomicUsize};

/// Status 回包默认配置；`NetConfig::default()` 的回退值。
pub const STATUS_MAX_PLAYERS: usize = 100;
pub const STATUS_MOTD: &str = "Rust MC 26.3 (777) self-made";
/// 默认视距：只透传（P5C 从 ServerConfig 注入），不管移动视距。
pub const DEFAULT_VIEW_DISTANCE: i32 = 10;
/// 默认压缩阈值：-1=禁用（原行为）。
pub const DEFAULT_COMPRESSION_THRESHOLD: i32 = -1;

/// 网络配置：由 mc-server 从 `ServerConfig` 透传。
#[derive(Debug, Clone)]
pub struct NetConfig {
    pub max_players: usize,
    pub motd: String,
    pub online_count: Arc<AtomicUsize>,
    pub view_distance: i32,
    /// 世界 seed：决定 terrain/高度图/出生点 y，与 `ServerConfig.seed` 一致。
    pub seed: i64,
    /// 压缩阈值：-1 禁用；>=0 启用（Login Success 前发 SetCompression）。
    pub compression_threshold: i32,
    /// Play 期事件通道（移动/KeepAlive）→ ECS；None 时只记录日志（单测/旧调用点）。
    pub play_events: Option<Sender<PlayEvent>>,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            max_players: STATUS_MAX_PLAYERS,
            motd: STATUS_MOTD.into(),
            online_count: Arc::new(AtomicUsize::new(0)),
            view_distance: DEFAULT_VIEW_DISTANCE,
            seed: mc_world::DEFAULT_SEED,
            compression_threshold: DEFAULT_COMPRESSION_THRESHOLD,
            play_events: None,
        }
    }
}

impl NetConfig {
    /// 旧三参构造保留（兼容旧调用点），view_distance/compression 取默认；
    /// 需要透传时链式 `.with_view_distance(v)` / `.with_compression_threshold(t)`。
    pub fn new(max_players: usize, motd: String, online_count: Arc<AtomicUsize>) -> Self {
        Self {
            max_players,
            motd,
            online_count,
            view_distance: DEFAULT_VIEW_DISTANCE,
            seed: mc_world::DEFAULT_SEED,
            compression_threshold: DEFAULT_COMPRESSION_THRESHOLD,
            play_events: None,
        }
    }

    pub fn new_with_view_distance(
        max_players: usize,
        motd: String,
        online_count: Arc<AtomicUsize>,
        view_distance: i32,
    ) -> Self {
        Self { view_distance, ..Self::new(max_players, motd, online_count) }
    }

    pub fn with_view_distance(mut self, view_distance: i32) -> Self {
        self.view_distance = view_distance;
        self
    }

    pub fn with_seed(mut self, seed: i64) -> Self {
        self.seed = seed;
        self
    }

    pub fn with_compression_threshold(mut self, threshold: i32) -> Self {
        self.compression_threshold = threshold;
        self
    }

    pub fn with_play_events(mut self, tx: Sender<PlayEvent>) -> Self {
        self.play_events = Some(tx);
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
        assert_eq!(cfg.seed, mc_world::DEFAULT_SEED);
        assert_eq!(cfg.compression_threshold, DEFAULT_COMPRESSION_THRESHOLD);
        assert!(cfg.play_events.is_none());
        assert_eq!(cfg.max_players, 42);
    }

    #[test]
    fn seed_and_view_distance_passthrough() {
        let cfg = NetConfig::new(100, "m".into(), Arc::new(AtomicUsize::new(0)))
            .with_view_distance(12)
            .with_seed(987654321);
        assert_eq!(cfg.view_distance, 12);
        assert_eq!(cfg.seed, 987654321);
        let full = NetConfig::new_with_view_distance(100, "m".into(), Arc::new(AtomicUsize::new(0)), 6);
        assert_eq!(full.view_distance, 6);
        assert_eq!(full.seed, mc_world::DEFAULT_SEED);
        assert_eq!(NetConfig::default().view_distance, DEFAULT_VIEW_DISTANCE);
    }

    #[test]
    fn compression_threshold_and_event_channel_passthrough() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let cfg = NetConfig::default()
            .with_compression_threshold(256)
            .with_play_events(tx);
        assert_eq!(cfg.compression_threshold, 256);
        cfg.play_events.as_ref().unwrap().send(PlayEvent::KeepAlive {
            addr: "127.0.0.1:1".parse().unwrap(),
        }).unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            PlayEvent::KeepAlive { addr: "127.0.0.1:1".parse().unwrap() }
        );
        assert_eq!(NetConfig::default().compression_threshold, -1);
    }
}
