//! config: ServerConfig，只兼容26.3/777
use bevy_ecs::prelude::*;

#[derive(Debug, Clone, Resource)]
pub struct ServerConfig {
    pub bind: String,
    pub motd: String,
    pub max_players: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:25565".to_string(),
            motd: "Rust MC 26.3 (777) self-made".to_string(),
            max_players: 100,
        }
    }
}
