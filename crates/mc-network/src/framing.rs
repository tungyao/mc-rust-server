//! framing: VarInt帧读写 + 超时/上限，只兼容26.3
use mc_protocol::read_varint;
use std::io;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 单次读包超时：防半开连接长期占任务。
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// 单包上限 2 MiB：防恶意 length 导致 OOM。
pub const MAX_PACKET_LEN: usize = 2_097_152;

/// 读一帧：`VarInt(length) + VarInt(packet_id) + payload`。
pub async fn read_packet(stream: &mut TcpStream) -> io::Result<(i32, Vec<u8>)> {
    match tokio::time::timeout(READ_TIMEOUT, read_packet_inner(stream)).await {
        Ok(r) => r,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "read packet timed out")),
    }
}

async fn read_packet_inner(stream: &mut TcpStream) -> io::Result<(i32, Vec<u8>)> {
    let len = read_varint_async(stream).await?;
    if len < 0 { return Err(to_io_invalid(format!("negative packet length {len}"))); }
    if len as usize > MAX_PACKET_LEN { return Err(to_io_invalid(format!("packet too large: {len}"))); }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    let (id, n) = read_varint(&buf).map_err(to_io_invalid)?;
    Ok((id, buf[n..].to_vec()))
}

async fn read_varint_async(stream: &mut TcpStream) -> io::Result<i32> {
    let mut num = 0i32;
    let mut shift = 0u32;
    for _ in 0..5 {
        let mut b = [0u8; 1];
        stream.read_exact(&mut b).await?;
        num |= ((b[0] & 0x7F) as i32) << shift;
        if b[0] & 0x80 == 0 { return Ok(num); }
        shift += 7;
    }
    Err(to_io_invalid("varint too big"))
}

/// 写一帧（组帧复用 `packets::encode_packet`）。
pub async fn write_packet(stream: &mut TcpStream, packet_id: i32, payload: &[u8]) -> io::Result<()> {
    let mut buf = Vec::new();
    mc_protocol::packets::encode_packet(packet_id, payload, &mut buf);
    stream.write_all(&buf).await
}

pub fn to_io_invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}
