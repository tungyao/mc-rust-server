//! config: NetConfig + 常量，只兼容26.3/777
use super::broadcast::BroadcastHub;
use super::chunk_cache::ChunkCache;
use super::chunk_view::clamp_view_radius;
use super::drop_state::DropState;
use super::fluid_sync::FluidState;
use super::play_events::PlayEvent;
use super::socket_ecs::{AcceptedSocket, SocketMode};
use crossbeam_channel::Sender;
use mc_world::{MutableWorld, world_time::WorldTimeState};
use std::sync::{
    Arc,
    atomic::AtomicUsize,
};

/// Status 回包默认配置；`NetConfig::default()` 的回退值。
pub const STATUS_MAX_PLAYERS: usize = 100;
pub const STATUS_MOTD: &str = "Rust MC 26.3 (777) self-made";
/// 默认视距：透传到 Play（有效值再经 `clamp_view_radius` 限制到 2..=10）。
pub const DEFAULT_VIEW_DISTANCE: i32 = 10;
/// 默认压缩阈值：-1=禁用（原行为）。
pub const DEFAULT_COMPRESSION_THRESHOLD: i32 = -1;

/// P53 A1：原版经验球吸引半径（7.25 格）与合并上限（每组最多 16 个球）。
pub const XP_ATTRACT_RADIUS: f64 = mc_world::xp::XP_ATTRACT_RADIUS;
pub const XP_MERGE_MAX_ORBS: usize = mc_world::xp::XP_MERGE_MAX_ORBS;
pub const XP_MERGE_DISTANCE: f64 = mc_world::xp::XP_MERGE_DISTANCE;
pub const XP_ATTRACT_SPEED: f64 = mc_world::xp::XP_ATTRACT_SPEED;
/// P53 A2：每 tick 疲劳推进阈值（跨 4.0 扣一格饥饿，余量保留）。
pub const EXHAUSTION_THRESHOLD: f32 = mc_world::survival::EXHAUSTION_THRESHOLD;

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
    /// 多人广播 hub（P21）；None 时只回包给操作者本人（旧行为）。
    pub hub: Option<Arc<BroadcastHub>>,
    /// P46 流体状态（全服唯一 50ms ticker 在 `serve` 启动，仅 V2 生效）。
    pub fluid: Option<Arc<FluidState>>,
    /// P43 玩家实体互见开关；默认 false 保持既有 e2e 包流，生产入口显式打开。
    pub player_visibility: bool,
    /// P47 挖掘动画 0x05 广播开关；默认 false 保持既有 e2e 包流逐字节不变，生产入口显式打开。
    pub destroy_stage: bool,
    /// P47 工具档位/时运/精准采集开关；默认 false 保持旧掉落口径，生产入口显式打开。
    pub tool_rules: bool,
    /// P52 socket 归属：库默认 net（历史逐字节基线/单测入口），生产入口由 ServerConfig 设为 ecs。
    pub socket_mode: SocketMode,
    /// ECS 模式下的 accept 交接通道；由 `SocketEcsRuntime::incoming_sender()` 注入。
    pub socket_accept: Option<Sender<AcceptedSocket>>,
    /// P53 A1：经验球 7.25 格吸引 + 邻近合并（库默认关保持旧 e2e 包流，生产 server.toml 默认开）。
    pub xp_merge: bool,
    /// P53 A2：疾跑/跳跃/游泳疲劳度细节（库默认关，生产默认开）。
    pub hunger_detail: bool,
    /// P53 A4：后台设置（gamemode/tool_enchant）跨重连落盘（库默认关，生产默认开）。
    pub settings_persist: bool,
    /// P53 A7：本地统计累积（库默认关，生产默认开；不发包）。
    pub stats: bool,
    // --- P54 批次 1 基础设施：四项独立开关 ---
    /// A3：广播限流/合并（**库/生产默认关**：改包序有风险，等真机 §5 双开包流确认）。
    pub broadcast_throttle: bool,
    /// A5：journal 按 seq 裁剪（默认开；裁剪只在后台/60s autosave，不占 50ms 关键路径）。
    pub journal_trim: bool,
    /// A5：journal 保留窗口（条数；默认 10000）。
    pub journal_keep: u64,
    /// A6：全局时间/天气状态（时间恒推进；天气只有服务端状态，从不发包）。
    pub world_time: Arc<WorldTimeState>,
    /// A8：工作台会话跨重连落盘（恒开，仅在 redb 模式下真正写表）。
    pub craft_session_persist: bool,
    // P55 B6：工作台 GUI 写死常开（基本功能）。菜单恒为 12
    //（`CRAFTING_MENU_TYPE_ID`，真机已校准标题为无名 tag）。
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
            hub: None,
            fluid: None,
            player_visibility: false,
            destroy_stage: false,
            tool_rules: false,
            socket_mode: SocketMode::Net,
            socket_accept: None,
            xp_merge: false,
            hunger_detail: false,
            settings_persist: false,
            stats: false,
            broadcast_throttle: false,
            journal_trim: true,
            journal_keep: DEFAULT_JOURNAL_KEEP,
            world_time: Arc::new(WorldTimeState::default()),
            craft_session_persist: true,
        }
    }
}

/// A5 默认 journal 保留窗口（条）。
pub const DEFAULT_JOURNAL_KEEP: u64 = 10_000;

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
            hub: None,
            fluid: None,
            player_visibility: false,
            destroy_stage: false,
            tool_rules: false,
            socket_mode: SocketMode::Net,
            socket_accept: None,
            xp_merge: false,
            hunger_detail: false,
            settings_persist: false,
            stats: false,
            broadcast_throttle: false,
            journal_trim: true,
            journal_keep: DEFAULT_JOURNAL_KEEP,
            world_time: Arc::new(WorldTimeState::default()),
            craft_session_persist: true,
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

    pub fn with_hub(mut self, hub: Arc<BroadcastHub>) -> Self {
        self.hub = Some(hub);
        self
    }

    /// P46：注入流体状态（`serve` 会为 V2 世界启动全服唯一 ticker）。
    pub fn with_fluid(mut self, fluid: Arc<FluidState>) -> Self {
        self.fluid = Some(fluid);
        self
    }

    /// P43：开启玩家实体互见（需同时 `.with_hub(...)`）。
    pub fn with_player_visibility(mut self, enabled: bool) -> Self {
        self.player_visibility = enabled;
        self
    }

    /// P47：开启挖掘进度广播（需同时 `.with_hub(...)`）；默认关闭保持旧包流。
    pub fn with_destroy_stage(mut self, enabled: bool) -> Self {
        self.destroy_stage = enabled;
        self
    }

    /// P47：开启工具等级/时运/精准采集判定；默认关闭保持旧掉落测试逐字节不变。
    pub fn with_tool_rules(mut self, enabled: bool) -> Self {
        self.tool_rules = enabled;
        self
    }

    /// P52：启动期锁定 socket 归属；不做运行时热切（Ack 生产者必须唯一）。
    pub fn with_socket_mode(mut self, mode: SocketMode) -> Self {
        self.socket_mode = mode;
        self
    }

    /// P52：注入 ECS 运行时的 accept 交接通道（仅 ecs 模式需要）。
    pub fn with_socket_accept(mut self, tx: Sender<AcceptedSocket>) -> Self {
        self.socket_accept = Some(tx);
        self
    }

    /// P53 A1：开启经验球 7.25 格吸引 + 合并（只走全服唯一 ticker）。
    pub fn with_xp_merge(mut self, enabled: bool) -> Self {
        self.xp_merge = enabled;
        self
    }

    /// P53 A2：开启疾跑/跳跃/游泳疲劳度细节。
    pub fn with_hunger_detail(mut self, enabled: bool) -> Self {
        self.hunger_detail = enabled;
        self
    }

    /// P53 A4：开启后台设置跨重连落盘/恢复。
    pub fn with_settings_persist(mut self, enabled: bool) -> Self {
        self.settings_persist = enabled;
        self
    }

    /// P53 A7：开启本地统计累积（仅服务端，不推 `award_stats`）。
    pub fn with_stats(mut self, enabled: bool) -> Self {
        self.stats = enabled;
        self
    }

    /// P54 A3：广播限流（默认关）。需要同时 `.with_hub(...)` 才真正生效。
    pub fn with_broadcast_throttle(mut self, enabled: bool) -> Self {
        self.broadcast_throttle = enabled;
        self
    }

    /// P54 A5：journal 裁剪开关（默认开）。
    pub fn with_journal_trim(mut self, enabled: bool) -> Self {
        self.journal_trim = enabled;
        self
    }

    pub fn with_journal_keep(mut self, keep: u64) -> Self {
        self.journal_keep = keep;
        self
    }

    /// P54 A6：注入全局时间/天气状态（默认新建一份；admin 与 net 共享同一 Arc）。
    pub fn with_world_time(mut self, time: Arc<WorldTimeState>) -> Self {
        self.world_time = time;
        self
    }

    /// P54 A8：工作台会话落盘（恒开，仅 redb 模式有实际存档）。
    pub fn with_craft_session_persist(mut self, enabled: bool) -> Self {
        self.craft_session_persist = enabled;
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
        assert!(!cfg.player_visibility, "P43 must be opt-in to preserve legacy packet streams");
        assert_eq!(cfg.max_players, 42);
    }

    #[test]
    fn player_visibility_passthrough() {
        let cfg = NetConfig::default().with_player_visibility(true);
        assert!(cfg.player_visibility);
        assert!(!NetConfig::new(1, "x".into(), Arc::new(AtomicUsize::new(0))).player_visibility);
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

    /// P53：四个新开关库默认全关（旧 e2e 包流逐字节不变），可独立打开。
    #[test]
    fn p53_feature_switches_are_opt_in() {
        let base = NetConfig::default();
        assert!(!base.xp_merge && !base.hunger_detail && !base.settings_persist && !base.stats);
        let cfg = NetConfig::default()
            .with_xp_merge(true)
            .with_hunger_detail(true)
            .with_settings_persist(true)
            .with_stats(true);
        assert!(cfg.xp_merge && cfg.hunger_detail && cfg.settings_persist && cfg.stats);
        assert_eq!(XP_ATTRACT_RADIUS, 7.25);
        assert_eq!(XP_MERGE_MAX_ORBS, 16);
        assert_eq!(EXHAUSTION_THRESHOLD, 4.0);
    }

    /// P54：A3 默认关（唯一可配项）；journal/工作台落盘恒开、天气恒不广播。
    #[test]
    fn p54_feature_switches_have_independent_defaults() {
        let base = NetConfig::default();
        assert!(!base.broadcast_throttle, "A3 changes packet order → opt-in only");
        assert!(base.journal_trim && base.craft_session_persist);
        assert_eq!(base.journal_keep, DEFAULT_JOURNAL_KEEP);
        assert_eq!(base.world_time.tick(), mc_world::world_time::DEFAULT_DAY_TICK);
        let cfg = base
            .with_broadcast_throttle(true)
            .with_journal_trim(false)
            .with_journal_keep(7)
            .with_craft_session_persist(false);
        assert!(cfg.broadcast_throttle && !cfg.journal_trim && !cfg.craft_session_persist);
        assert_eq!(cfg.journal_keep, 7);
    }

    /// P55 B6：工作台 GUI 已写死常开——右键恒发 0x3c + 0x12，菜单恒 12。
    /// 本测试只锁定菜单 ID 常量（真机已校准标题为无名 tag）。
    #[test]
    fn p55_workbench_gui_is_opt_in_and_runtime_toggle_shared() {
        assert_eq!(mc_protocol::packets::CRAFTING_MENU_TYPE_ID, 12);
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
