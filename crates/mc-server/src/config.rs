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
