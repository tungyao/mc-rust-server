//! mc-protocol: 纯编解码，不碰网络/ECS，可独立 fuzz/bench
//! 对标 Valence valence_protocol + FerrumC net/crates/codec

pub mod varint;
pub mod packets;

pub use varint::{read_varint, write_varint};

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("unexpected eof")]
    UnexpectedEof,
    #[error("varint too big")]
    VarIntTooBig,
    #[error("invalid packet id {0}")]
    InvalidPacketId(i32),
    #[error("invalid handshake next state {0}")]
    InvalidNextState(i32),
}
