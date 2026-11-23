//! mc-core: 基础类型，不依赖 bevy/tokio
//! 只兼容 26.3 (protocol 777, data 5023)，拒绝其他版本
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockPos(pub i32, pub i32, pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkPos(pub i32, pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec3d(pub f64, pub f64, pub f64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerProfile {
    pub uuid: Uuid,
    pub username: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    Status,
    Login,
    Play,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid data: {0}")]
    InvalidData(String),
    #[error("unsupported protocol {got}, only 777 (26.3) supported")]
    UnsupportedProtocol { got: i32 },
}

// ---- 只兼容 26.3 ----
pub const TICK_MS: u64 = 50;
pub const PROTOCOL_VERSION: i32 = 777;
pub const MC_VERSION: &str = "26.3";
pub const DATA_VERSION: i32 = 5023;

pub fn check_protocol(got: i32) -> Result<(), CoreError> {
    if got == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(CoreError::UnsupportedProtocol { got })
    }
}
