//! config: NetConfig + 常量，只兼容26.3/777
use super::chunk_cache::ChunkCache;
use super::chunk_view::clamp_view_radius;
use super::drop_state::DropState;
use super::play_events::PlayEvent;
use crossbeam_channel::Sender;
use mc_world::MutableWorld;
use std::sync::{Arc, atomic::AtomicUsize};

/// Status 回包默认配置；`NetConfig::default()` 的回退值。
pub const STATUS_MAX_PLAYERS: usize = 100;
pub const STATUS_MOTD: &str = "Rust MC 26.3 (777) self-made";
/// 默认视距：透传到 Play（有效值再经 `clamp_view_radius` 限制到 2..=10）。
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
    /// Play 期事件通道（移动/KeepAlive/Disconnect）→ ECS；None 时只记录日志（单测/旧调用点）。
    pub play_events: Option<Sender<PlayEvent>>,
    /// 多连接共享的已编码 ChunkData 包体缓存；None 时每块直接 WorldGen 生成。
    pub chunk_cache: Option<Arc<ChunkCache>>,
    /// 服务端权威可变世界（P17 挖掘/放置）；None 时忽略方块交互包（旧行为）。
    pub mutable_world: Option<Arc<MutableWorld>>,
    /// 掉落物实体 + 背包共享状态（P18）；None 时挖/放不掉落不同步（旧行为）。
    pub drops: Option<Arc<DropState>>,
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
            chunk_cache: None,
            mutable_world: None,
            drops: None,
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
            chunk_cache: None,
            mutable_world: None,
            drops: None,
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

    /// 实际生效的视距半径：`view_distance` clamp 到 2..=10。
    pub fn effective_view_radius(&self) -> i32 {
        clamp_view_radius(self.view_distance)
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

    pub fn with_chunk_cache(mut self, cache: Arc<ChunkCache>) -> Self {
        self.chunk_cache = Some(cache);
        self
    }

    pub fn with_mutable_world(mut self, world: Arc<MutableWorld>) -> Self {
        self.mutable_world = Some(world);
        self
    }

    pub fn with_drops(mut self, drops: Arc<DropState>) -> Self {
        self.drops = Some(drops);
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
        assert!(cfg.chunk_cache.is_none());
        assert!(cfg.mutable_world.is_none());
        assert_eq!(cfg.max_players, 42);
    }

    #[test]
    fn seed_and_view_distance_passthrough() {
        let cfg = NetConfig::new(100, "m".into(), Arc::new(AtomicUsize::new(0)))
            .with_view_distance(12)
            .with_seed(987654321);
        assert_eq!(cfg.view_distance, 12, "raw config value is preserved");
        assert_eq!(cfg.effective_view_radius(), 10, "effective radius is clamped");
        assert_eq!(cfg.seed, 987654321);
        let full = NetConfig::new_with_view_distance(100, "m".into(), Arc::new(AtomicUsize::new(0)), 6);
        assert_eq!(full.view_distance, 6);
        assert_eq!(full.effective_view_radius(), 6);
        assert_eq!(full.seed, mc_world::DEFAULT_SEED);
        assert_eq!(NetConfig::default().view_distance, DEFAULT_VIEW_DISTANCE);
    }

    #[test]
    fn effective_view_radius_clamps_boundaries() {
        let mk = |v| NetConfig::default().with_view_distance(v).effective_view_radius();
        assert_eq!(mk(-5), 2);
        assert_eq!(mk(1), 2);
        assert_eq!(mk(2), 2);
        assert_eq!(mk(10), 10);
        assert_eq!(mk(11), 10);
        assert_eq!(mk(i32::MAX), 10);
    }

    #[test]
    fn compression_threshold_and_event_channel_passthrough() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let cfg = NetConfig::default()
            .with_compression_threshold(256)
            .with_play_events(tx)
            .with_chunk_cache(Arc::new(ChunkCache::new(42, 8)));
        assert_eq!(cfg.compression_threshold, 256);
        cfg.play_events.as_ref().unwrap().send(PlayEvent::KeepAlive {
            addr: "127.0.0.1:1".parse().unwrap(),
            conn_id: 7,
        }).unwrap();
        assert_eq!(
            rx.try_recv().unwrap(),
            PlayEvent::KeepAlive { addr: "127.0.0.1:1".parse().unwrap(), conn_id: 7 }
        );
        assert_eq!(NetConfig::default().compression_threshold, -1);
        assert_eq!(cfg.chunk_cache.as_ref().unwrap().seed(), 42);
        assert_eq!(cfg.chunk_cache.as_ref().unwrap().capacity(), 8);
    }
}
