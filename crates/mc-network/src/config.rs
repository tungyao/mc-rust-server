//! config: NetConfig + 常量，只兼容26.3/777
use std::sync::{Arc, atomic::AtomicUsize};

/// Status 回包默认配置；`NetConfig::default()` 的回退值。
pub const STATUS_MAX_PLAYERS: usize = 100;
pub const STATUS_MOTD: &str = "Rust MC 26.3 (777) self-made";

/// 网络配置：由 mc-server 从 `ServerConfig` 透传。
#[derive(Debug, Clone)]
pub struct NetConfig {
    pub max_players: usize,
    pub motd: String,
    pub online_count: Arc<AtomicUsize>,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self { max_players: STATUS_MAX_PLAYERS, motd: STATUS_MOTD.into(), online_count: Arc::new(AtomicUsize::new(0)) }
    }
}

impl NetConfig {
    pub fn new(max_players: usize, motd: String, online_count: Arc<AtomicUsize>) -> Self {
        Self { max_players, motd, online_count }
    }
}
