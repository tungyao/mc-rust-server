//! config: ServerConfig，只兼容26.3/777
//! P5C：server.toml 落盘（bind/motd/max_players/view_distance/seed），缺文件自动生成默认。
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 默认视距（格数/区块半径），与 Play LoginPlay view_distance=10 对齐；只管透传，不管移动视距。
pub const DEFAULT_VIEW_DISTANCE: i32 = 10;
/// 默认种子，与 `mc_world::DEFAULT_SEED` 一致（123456789）。
pub const DEFAULT_SEED: i64 = mc_world::DEFAULT_SEED;
/// 默认压缩阈值：-1 = 禁用（原行为，不发 SetCompression，全链路明文帧）。
/// >=0 = Login Success 前发 SetCompression(threshold)，Login/Configuration/Play 后续包全走压缩帧。
pub const DEFAULT_COMPRESSION_THRESHOLD: i32 = -1;
/// 默认存档目录：可变世界 override 落盘位置（`overrides-<seed>.dat`）。
pub const DEFAULT_WORLD_DIR: &str = "world";
/// 默认管理面板监听地址：只听回环（无鉴权下的唯一缓解，禁止改为 0.0.0.0，
/// 除非前置反向代理做鉴权）。
pub const DEFAULT_ADMIN_BIND: &str = "127.0.0.1:25566";

/// 世界存储后端：`redb` = 新格式（`world/state-<seed>.redb`），`legacy` = 旧稀疏档。
/// 注意：这是独立的 toml 键，不塞进 `ServerConfig`——后者有旧测试用穷举结构体字面量，
/// 加字段会破坏"365 测试一字不改"的硬约束。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldStorage {
    Redb,
    Legacy,
}

impl Default for WorldStorage {
    fn default() -> Self {
        Self::Redb
    }
}

impl std::str::FromStr for WorldStorage {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "redb" => Ok(Self::Redb),
            "legacy" => Ok(Self::Legacy),
            other => Err(format!("unknown world_storage {other:?} (expected redb|legacy)")),
        }
    }
}

/// 旧 `server.toml` 缺 `world_storage` 键时补一行 `world_storage = "redb"`（默认新格式）。
/// 写失败只影响诊断：读取仍按默认 redb 进行。
fn ensure_world_storage_key(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    if text.contains("world_storage") {
        return;
    }
    let mut updated = text;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str("\n# P38 world storage backend: \"redb\" (default) or \"legacy\"\nworld_storage = \"redb\"\n");
    let _ = std::fs::write(path, updated);
}

/// P52 socket 归属默认：**ecs 全量切换**；`socket_mode = "net"` 为紧急回退。
/// 与 `world_storage`/`generator_version` 一样作为独立 toml 键读取，避免给穷举的
/// `ServerConfig` 字面量加字段（旧测试一字不改）。
pub const DEFAULT_SOCKET_MODE: mc_network::SocketMode = mc_network::SocketMode::Ecs;

/// 旧 `server.toml` 缺 `socket_mode` 键时补一行（生产默认 ecs）。
fn ensure_socket_mode_key(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    if text.contains("socket_mode") {
        return;
    }
    let mut updated = text;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str("\n# P52 socket ownership: \"ecs\" (default, full ECS I/O) or \"net\" (emergency fallback)\nsocket_mode = \"ecs\"\n");
    let _ = std::fs::write(path, updated);
}

/// 读取 socket 归属开关：缺键按默认 ecs 补齐到文件；非法值警告后回退 ecs。
/// 启动期锁定，不做运行时热切（避免同进程两套写路径导致 Ack 双生产者）。
pub fn load_socket_mode(path: impl AsRef<Path>) -> mc_network::SocketMode {
    let path = path.as_ref();
    let Ok(text) = std::fs::read_to_string(path) else { return DEFAULT_SOCKET_MODE };
    ensure_socket_mode_key(path);
    let value = match toml::from_str::<toml::Value>(&text) {
        Ok(v) => v.get("socket_mode").and_then(|x| x.as_str()).map(str::to_string),
        Err(_) => None,
    };
    match value {
        Some(v) => match v.parse::<mc_network::SocketMode>() {
            Ok(mode) => mode,
            Err(e) => {
                eprintln!("warn: {}: {e}, defaulting to ecs", path.display());
                DEFAULT_SOCKET_MODE
            }
        },
        None => DEFAULT_SOCKET_MODE,
    }
}

/// P53 批次 1：四项玩法开关（生产默认全开，server.toml 可单独关）。
/// 与 `world_storage`/`socket_mode` 一样走独立 toml 键，绝不给穷举字面量的
/// `ServerConfig` 加字段（旧 528 测试一字不改的硬约束）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureToggles {
    pub xp_merge: bool,
    pub hunger_detail: bool,
    pub settings_persist: bool,
    pub stats: bool,
    /// P54 A3：默认关（改包序有风险，等真机双开包流确认）。
    pub broadcast_throttle: bool,
    /// P54 A5：默认开。
    pub journal_trim: bool,
    pub journal_keep: u64,
    /// P54 A6：时间默认推进；天气广播默认关（0x27 type 未确认，当前不发包）。
    pub weather_broadcast: bool,
    /// P54 A8：默认开。
    pub craft_session_persist: bool,
}

impl Default for FeatureToggles {
    fn default() -> Self {
        Self {
            xp_merge: true, hunger_detail: true, settings_persist: true, stats: true,
            broadcast_throttle: false, journal_trim: true, journal_keep: 10_000,
            weather_broadcast: false, craft_session_persist: true,
        }
    }
}

/// 缺键时补一行 `key = <default>`，返回当前值；非法值警告后回退默认。
fn load_bool_key(path: &Path, key: &str, default: bool, comment: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else { return default };
    let value = toml::from_str::<toml::Value>(&text)
        .ok()
        .and_then(|v| v.get(key).and_then(|x| x.as_bool()));
    if value.is_none() && !text.contains(key) {
        let mut updated = text;
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&format!("\n# {comment}\n{key} = {default}\n"));
        let _ = std::fs::write(path, updated);
    }
    value.unwrap_or(default)
}

/// 读取四项 P53 开关：旧 server.toml 缺键时逐项补默认值（全开）。
pub fn load_feature_toggles(path: impl AsRef<Path>) -> FeatureToggles {
    let path = path.as_ref();
    FeatureToggles {
        xp_merge: load_bool_key(path, "xp_merge", true, "P53 A1: XP orb attraction/merge (default true)"),
        hunger_detail: load_bool_key(path, "hunger_detail", true, "P53 A2: sprint/jump/swim exhaustion (default true)"),
        settings_persist: load_bool_key(path, "settings_persist", true, "P53 A4: persist gamemode/tool_enchant across reconnect (default true)"),
        stats: load_bool_key(path, "stats", true, "P53 A7: local stats accumulation (default true, no packets)"),
        // P54：A3 默认关；A5/A6 时间/A8 默认开；天气广播默认关。
        broadcast_throttle: load_bool_key(path, "broadcast_throttle", false, "P54 A3: broadcast merge/throttle (default false, packet order risk)"),
        journal_trim: load_bool_key(path, "journal_trim", true, "P54 A5: journal seq window trim (default true)"),
        journal_keep: load_u64_key(path, "journal_keep", 10_000, "P54 A5: journal retained record count"),
        weather_broadcast: load_bool_key(path, "weather_broadcast", false, "P54 A6: weather game_event (default false; 0x27 type unconfirmed, not sent yet)"),
        craft_session_persist: load_bool_key(path, "craft_session_persist", true, "P54 A8: workbench session redb persistence (default true)"),
    }
}

/// P55 B6：工作台 GUI 总开关，独立 toml 键（不塞进 `FeatureToggles`/`ServerConfig`，
/// 保住 P53/P54 的穷举字面量测试）。**默认 false**：包体菜单 ID 是 wiki 推导值，
/// 未抓包校准，开启后才可能断线；后台 `/api/workbench-gui` 可运行时热切。
pub const DEFAULT_WORKBENCH_GUI: bool = false;

/// P55 B6：菜单类型 ID 的配置覆盖键；wiki 推导值 12，**待抓包校准**。
pub const DEFAULT_WORKBENCH_MENU_ID: i32 = mc_protocol::packets::CRAFTING_MENU_TYPE_ID;

/// 读工作台 GUI 开关（旧 server.toml 缺键时补 `workbench_gui = false`）。
pub fn load_workbench_gui(path: impl AsRef<Path>) -> bool {
    load_bool_key(
        path.as_ref(),
        "workbench_gui",
        DEFAULT_WORKBENCH_GUI,
        "P55 B6: workbench GUI (open_screen 0x3c), menu id wiki-derived, capture required before enabling",
    )
}

/// 读菜单类型 ID（缺键补 12；非法/越界值警告后回退 12）。
pub fn load_workbench_menu_id(path: impl AsRef<Path>) -> i32 {
    let path = path.as_ref();
    let Ok(text) = std::fs::read_to_string(path) else { return DEFAULT_WORKBENCH_MENU_ID };
    let value = toml::from_str::<toml::Value>(&text)
        .ok()
        .and_then(|v| v.get("workbench_menu_id").and_then(|x| x.as_integer()))
        .and_then(|n| i32::try_from(n).ok())
        .filter(|n| *n >= 0);

    if value.is_none() && !text.contains("workbench_menu_id") {
        let mut updated = text;
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&format!(
            "\n# P55 B6: menu type id for crafting GUI (wiki-derived 12, capture-calibrate before use)\nworkbench_menu_id = {}\n",
            DEFAULT_WORKBENCH_MENU_ID
        ));
        let _ = std::fs::write(path, updated);
    }
    value.unwrap_or(DEFAULT_WORKBENCH_MENU_ID)
}

/// 数值 toml 键：缺键补默认值；非法值警告后回退默认。
fn load_u64_key(path: &Path, key: &str, default: u64, comment: &str) -> u64 {
    let Ok(text) = std::fs::read_to_string(path) else { return default };
    let value = toml::from_str::<toml::Value>(&text)
        .ok()
        .and_then(|v| v.get(key).and_then(|x| x.as_integer()))
        .and_then(|n| u64::try_from(n).ok());
    if value.is_none() && !text.contains(key) {
        let mut updated = text;
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&format!("\n# {comment}\n{key} = {default}\n"));
        let _ = std::fs::write(path, updated);
    }
    value.unwrap_or(default)
}

/// 生成器版本默认值：v1=legacy 永久冻结；v2 只对新 seed / 显式配置启用。
pub const DEFAULT_GENERATOR_VERSION: u32 = 1;

/// 旧 `server.toml` 缺 `generator_version` 键时补一行（默认 1，老 seed 不受影响）。
fn ensure_generator_version_key(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    if text.contains("generator_version") {
        return;
    }
    let mut updated = text;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str("\n# P44 world generator: 1=legacy (frozen default), 2=new-seed visuals\ngenerator_version = 1\n");
    let _ = std::fs::write(path, updated);
}

/// 读取生成器版本（独立 toml 键，避免给穷举的 `ServerConfig` 字面量加字段）。
/// 只有 1/2 合法；非法值警告后回退 v1。
pub fn load_generator_version(path: impl AsRef<Path>) -> u32 {
    let path = path.as_ref();
    let Ok(text) = std::fs::read_to_string(path) else { return DEFAULT_GENERATOR_VERSION };
    ensure_generator_version_key(path);
    let value = match toml::from_str::<toml::Value>(&text) {
        Ok(v) => v.get("generator_version").and_then(|x| x.as_integer()),
        Err(_) => None,
    };
    match value {
        Some(1) => 1,
        Some(2) => 2,
        Some(other) => {
            eprintln!(
                "warn: {}: generator_version {other} unsupported (expected 1|2), defaulting to 1",
                path.display()
            );
            DEFAULT_GENERATOR_VERSION
        }
        None => DEFAULT_GENERATOR_VERSION,
    }
}

/// 读取存储后端开关（缺键按默认 redb 补齐到文件）。非法值警告后回退默认。
pub fn load_world_storage(path: impl AsRef<Path>) -> WorldStorage {
    let path = path.as_ref();
    let Ok(text) = std::fs::read_to_string(path) else { return WorldStorage::default() };
    ensure_world_storage_key(path);
    let value = match toml::from_str::<toml::Value>(&text) {
        Ok(v) => v.get("world_storage").and_then(|x| x.as_str()).map(str::to_string),
        Err(_) => None,
    };
    match value {
        Some(v) => match v.parse::<WorldStorage>() {
            Ok(mode) => mode,
            Err(e) => {
                eprintln!("warn: {}: {e}, defaulting to redb", path.display());
                WorldStorage::default()
            }
        },
        None => WorldStorage::default(),
    }
}

#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind: String,
    pub motd: String,
    pub max_players: usize,
    pub view_distance: i32,
    pub seed: i64,
    /// 压缩阈值（字节）：-1 禁用；>=0 启用。旧 server.toml 缺该键时按默认 -1 补齐。
    pub compression_threshold: i32,
    /// 存档目录：P17 override 落盘位置；旧 toml 缺键时按默认 `world` 补齐。
    pub world_dir: String,
    /// 管理面板监听地址：默认只听回环；旧 toml 缺键时按默认补齐。
    pub admin_bind: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:25565".to_string(),
            motd: "Rust MC 26.3 (777) self-made".to_string(),
            max_players: 100,
            view_distance: DEFAULT_VIEW_DISTANCE,
            seed: DEFAULT_SEED,
            compression_threshold: DEFAULT_COMPRESSION_THRESHOLD,
            world_dir: DEFAULT_WORLD_DIR.to_string(),
            admin_bind: DEFAULT_ADMIN_BIND.to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse {path}: {msg}")]
    Parse { path: String, msg: String },
}

impl ServerConfig {
    /// 实际生效的视距半径：`view_distance` clamp 到 2..=10（与 net 线程/LoginPlay 一致）。
    pub fn effective_view_distance(&self) -> i32 {
        mc_network::clamp_view_radius(self.view_distance)
    }

    /// 读档或建档：文件存在则解析（旧文件缺新键时按 Default 补齐）；
    /// 缺文件则写一份默认 server.toml 再返回默认；解析失败返回 Err，由 main 回退默认+warn。
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let cfg = Self::load_or_create_inner(path)?;
        ensure_world_storage_key(path);
        ensure_generator_version_key(path);
        ensure_socket_mode_key(path);
        Ok(cfg)
    }

    fn load_or_create_inner(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| ConfigError::Parse {
                path: path.display().to_string(),
                msg: e.to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let cfg = Self::default();
                let text = toml::to_string_pretty(&cfg)
                    .map_err(|e| ConfigError::Parse { path: path.display().to_string(), msg: e.to_string() })?;
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, text)?;
                Ok(cfg)
            }
            Err(e) => Err(ConfigError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn tmp_toml() -> std::path::PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("mc-server-cfg-{}-{n}.toml", std::process::id()))
    }

    #[test]
    fn missing_file_creates_default() {
        let p = tmp_toml();
        let _ = std::fs::remove_file(&p);
        let cfg = ServerConfig::load_or_create(&p).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0:25565");
        assert_eq!(cfg.max_players, 100);
        assert_eq!(cfg.view_distance, DEFAULT_VIEW_DISTANCE);
        assert_eq!(cfg.seed, DEFAULT_SEED);
        assert_eq!(cfg.compression_threshold, DEFAULT_COMPRESSION_THRESHOLD, "compression stays disabled by default");
        assert_eq!(cfg.world_dir, DEFAULT_WORLD_DIR);
        assert_eq!(cfg.admin_bind, DEFAULT_ADMIN_BIND, "admin panel listens on loopback by default");
        // 文件已落盘，且内容可再解析
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("view_distance"));
        assert!(text.contains("compression_threshold"));
        assert!(text.contains("admin_bind"), "fresh toml must carry the admin bind");
        let again = ServerConfig::load_or_create(&p).unwrap();
        assert_eq!(again.view_distance, cfg.view_distance);
        assert_eq!(again.seed, cfg.seed);
        assert_eq!(again.compression_threshold, cfg.compression_threshold);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn legacy_file_without_compression_key_defaults_to_disabled() {
        let p = tmp_toml();
        std::fs::write(
            &p,
            "bind = \"127.0.0.1:25565\"\nmotd = \"old\"\nmax_players = 7\nview_distance = 4\nseed = 42\n",
        )
        .unwrap();
        let cfg = ServerConfig::load_or_create(&p).unwrap();
        assert_eq!(cfg.compression_threshold, -1, "old toml must keep old plain-frame behavior");
        assert_eq!(cfg.world_dir, DEFAULT_WORLD_DIR, "old toml saves to default world dir");
        assert_eq!(cfg.admin_bind, DEFAULT_ADMIN_BIND, "old toml gets loopback admin bind");
        assert_eq!(cfg.max_players, 7);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn effective_view_distance_clamps_2_to_10() {
        let mk = |v| ServerConfig { view_distance: v, ..Default::default() }.effective_view_distance();
        assert_eq!(mk(-1), 2);
        assert_eq!(mk(1), 2);
        assert_eq!(mk(2), 2);
        assert_eq!(mk(4), 4);
        assert_eq!(mk(10), 10);
        assert_eq!(mk(11), 10);
        assert_eq!(mk(i32::MAX), 10);
    }

    #[test]
    fn roundtrip_custom_values() {
        let p = tmp_toml();
        let cfg = ServerConfig {
            bind: "127.0.0.1:25566".into(),
            motd: "test".into(),
            max_players: 42,
            view_distance: 12,
            seed: 987654321,
            compression_threshold: 256,
            world_dir: "world-test".into(),
            admin_bind: "127.0.0.1:25567".into(),
        };
        std::fs::write(&p, toml::to_string_pretty(&cfg).unwrap()).unwrap();
        let back = ServerConfig::load_or_create(&p).unwrap();
        assert_eq!(back.bind, "127.0.0.1:25566");
        assert_eq!(back.max_players, 42);
        assert_eq!(back.view_distance, 12);
        assert_eq!(back.seed, 987654321);
        assert_eq!(back.compression_threshold, 256);
        assert_eq!(back.world_dir, "world-test");
        assert_eq!(back.admin_bind, "127.0.0.1:25567");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn bad_toml_returns_err() {
        let p = tmp_toml();
        std::fs::write(&p, "bind = [unclosed\n").unwrap();
        assert!(ServerConfig::load_or_create(&p).is_err());
        let _ = std::fs::remove_file(&p);
    }

    /// P38 开关：缺键按默认 redb 补齐到旧 toml，两种模式都能解析，非法值回退默认。
    #[test]
    fn socket_mode_defaults_to_ecs_and_allows_net_fallback() {
        let p = tmp_toml();
        let _ = std::fs::remove_file(&p);
        std::fs::write(&p, "seed = 42\n").unwrap();
        assert_eq!(load_socket_mode(&p), mc_network::SocketMode::Ecs, "production default is full ECS");
        let filled = std::fs::read_to_string(&p).unwrap();
        assert!(filled.contains("socket_mode = \"ecs\""), "old toml gets the new key");

        std::fs::write(&p, "seed = 42\nsocket_mode = \"net\"\n").unwrap();
        assert_eq!(load_socket_mode(&p), mc_network::SocketMode::Net, "net stays as emergency fallback");
        std::fs::write(&p, "seed = 42\nsocket_mode = \"bogus\"\n").unwrap();
        assert_eq!(load_socket_mode(&p), mc_network::SocketMode::Ecs, "invalid value falls back to ecs");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn world_storage_switch_reads_both_modes_and_fills_missing_key() {
        let p = tmp_toml();
        std::fs::write(&p, "bind = \"127.0.0.1:25565\"\nseed = 42\n").unwrap();
        assert_eq!(load_world_storage(&p), WorldStorage::Redb);
        let filled = std::fs::read_to_string(&p).unwrap();
        assert!(filled.contains("world_storage = \"redb\""), "old toml must gain the key");
        // 缺键的旧 toml 仍能被 ServerConfig 解析（旧测试口径不变）。
        assert_eq!(ServerConfig::load_or_create(&p).unwrap().seed, 42);
        assert_eq!(load_world_storage(&p), WorldStorage::Redb);

        std::fs::write(&p, "seed = 42\nworld_storage = \"legacy\"\n").unwrap();
        assert_eq!(load_world_storage(&p), WorldStorage::Legacy);
        std::fs::write(&p, "seed = 42\nworld_storage = \"redb\"\n").unwrap();
        assert_eq!(load_world_storage(&p), WorldStorage::Redb);
        std::fs::write(&p, "seed = 42\nworld_storage = \"nonsense\"\n").unwrap();
        assert_eq!(load_world_storage(&p), WorldStorage::Redb, "unknown value falls back to default");
        let _ = std::fs::remove_file(&p);
    }

    /// P53：旧 toml 缺键补四项默认开；显式 false 可单独关闭任一项。
    #[test]
    fn p53_feature_toggles_default_on_and_allow_off() {
        let p = tmp_toml();
        let _ = std::fs::remove_file(&p);
        std::fs::write(&p, "seed = 42\n").unwrap();
        assert_eq!(load_feature_toggles(&p), FeatureToggles::default());
        let filled = std::fs::read_to_string(&p).unwrap();
        for key in ["xp_merge", "hunger_detail", "settings_persist", "stats"] {
            assert!(filled.contains(&format!("{key} = true")), "old toml gains {key}");
        }
        std::fs::write(
            &p,
            "seed = 42\nxp_merge = false\nhunger_detail = true\nsettings_persist = false\nstats = true\n",
        )
        .unwrap();
        let toggles = load_feature_toggles(&p);
        assert!(!toggles.xp_merge && toggles.hunger_detail && !toggles.settings_persist && toggles.stats);
        let _ = std::fs::remove_file(&p);
    }

    /// P55：GUI 默认关（未校准字节不能默认发），菜单 ID 默认 12 且可覆盖。
    #[test]
    fn p55_workbench_gui_defaults_off_with_calibration_override() {
        let p = tmp_toml();
        let _ = std::fs::remove_file(&p);
        std::fs::write(&p, "seed = 42\n").unwrap();
        assert!(!load_workbench_gui(&p));
        assert_eq!(load_workbench_menu_id(&p), 12);
        let filled = std::fs::read_to_string(&p).unwrap();
        assert!(filled.contains("workbench_gui = false"));
        assert!(filled.contains("workbench_menu_id = 12"));
        // 旧 ServerConfig 穷举字面量仍能解析（不给它加字段）。
        assert_eq!(ServerConfig::load_or_create(&p).unwrap().seed, 42);

        std::fs::write(&p, "seed = 42\nworkbench_gui = true\nworkbench_menu_id = 7\n").unwrap();
        assert!(load_workbench_gui(&p));
        assert_eq!(load_workbench_menu_id(&p), 7);
        std::fs::write(&p, "seed = 42\nworkbench_menu_id = -3\n").unwrap();
        assert_eq!(load_workbench_menu_id(&p), 12, "negative id falls back to derived default");
        let _ = std::fs::remove_file(&p);
    }

    /// P44 开关：独立 toml 键，缺键补 1，1/2 合法，非法值回退 1。
    #[test]
    fn generator_version_switch_reads_1_and_2_and_fills_missing_key() {
        let p = tmp_toml();
        std::fs::write(&p, "seed = 42\n").unwrap();
        assert_eq!(load_generator_version(&p), 1);
        let filled = std::fs::read_to_string(&p).unwrap();
        assert!(filled.contains("generator_version = 1"), "old toml must gain the key");
        // 旧测试穷举的 ServerConfig 字面量不需要新字段，仍可解析。
        assert_eq!(ServerConfig::load_or_create(&p).unwrap().seed, 42);

        std::fs::write(&p, "seed = 42\ngenerator_version = 2\n").unwrap();
        assert_eq!(load_generator_version(&p), 2);
        std::fs::write(&p, "seed = 42\ngenerator_version = 1\n").unwrap();
        assert_eq!(load_generator_version(&p), 1);
        std::fs::write(&p, "seed = 42\ngenerator_version = 9\n").unwrap();
        assert_eq!(load_generator_version(&p), 1, "unknown value falls back to legacy");
        let _ = std::fs::remove_file(&p);
    }
}
