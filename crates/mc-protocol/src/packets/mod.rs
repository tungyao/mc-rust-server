//! packets: 26.3/777 包定义，按阶段拆分，每文件<=200行
//! - codec: 基础编解码 + 组帧
//! - handshake/status/login/config: 各阶段包
//! - play_*: Play包（ID已按P4A对照表核对777）
//! 兼容：`packets::X` 路径不变（全部 pub use 重导出）。
mod block_tags_777;
pub mod codec;
pub mod config;
pub mod handshake;
pub mod login;
pub mod vanilla_config_777;
// nbt.rs 在 src/ 顶层（Registry/区块多处共用，不专属于 packets 阶段）；
// 这里用 #[path] 接进来，保持 `packets::nbt` / `packets::NbtTag` 路径不变。
#[path = "../nbt.rs"]
pub mod nbt;
pub mod play_block;
pub mod play_chunk;
pub mod play_login;
pub mod play_sync;
pub mod status;

pub use codec::*;
pub use config::*;
pub use handshake::*;
pub use login::*;
pub use nbt::*;
pub use play_block::*;
pub use play_chunk::*;
pub use play_login::*;
pub use play_sync::*;
pub use status::*;
