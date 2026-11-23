//! config: ServerConfig，只兼容26.3/777
//! P5C：server.toml 落盘（bind/motd/max_players/view_distance/seed），缺文件自动生成默认。
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 默认视距（格数/区块半径），与 Play LoginPlay view_distance=10 对齐；只管透传，不管移动视距。
pub const DEFAULT_VIEW_DISTANCE: i32 = 10;
/// 默认种子，与 mc-world 空世界 WORLD_SEED 对齐。
pub const DEFAULT_SEED: i64 = 123456789;

#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind: String,
    pub motd: String,
    pub max_players: usize,
    pub view_distance: i32,
    pub seed: i64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:25565".to_string(),
            motd: "Rust MC 26.3 (777) self-made".to_string(),
            max_players: 100,
            view_distance: DEFAULT_VIEW_DISTANCE,
            seed: DEFAULT_SEED,
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
    /// 读档或建档：文件存在则解析（旧文件缺新键时按 Default 补齐）；
    /// 缺文件则写一份默认 server.toml 再返回默认；解析失败返回 Err，由 main 回退默认+warn。
    pub fn load_or_create(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
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
        // 文件已落盘，且内容可再解析
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("view_distance"));
        let again = ServerConfig::load_or_create(&p).unwrap();
        assert_eq!(again.view_distance, cfg.view_distance);
        assert_eq!(again.seed, cfg.seed);
        let _ = std::fs::remove_file(&p);
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
        };
        std::fs::write(&p, toml::to_string_pretty(&cfg).unwrap()).unwrap();
        let back = ServerConfig::load_or_create(&p).unwrap();
        assert_eq!(back.bind, "127.0.0.1:25566");
        assert_eq!(back.max_players, 42);
        assert_eq!(back.view_distance, 12);
        assert_eq!(back.seed, 987654321);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn bad_toml_returns_err() {
        let p = tmp_toml();
        std::fs::write(&p, "bind = [unclosed\n").unwrap();
        assert!(ServerConfig::load_or_create(&p).is_err());
        let _ = std::fs::remove_file(&p);
    }
}
