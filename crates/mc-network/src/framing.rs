//! framing: VarInt帧读写 + 压缩帧(zlib) + 超时/上限，只兼容26.3
use mc_protocol::{read_varint, write_varint};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 单次读包超时：防半开连接长期占任务。
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// 单包上限 2 MiB：防恶意 length 导致 OOM。
pub const MAX_PACKET_LEN: usize = 2_097_152;

/// 读一帧：`VarInt(length) + VarInt(packet_id) + payload`（压缩禁用路径）。
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

/// 写一帧（组帧复用 `packets::encode_packet`，压缩禁用路径）。
pub async fn write_packet(stream: &mut TcpStream, packet_id: i32, payload: &[u8]) -> io::Result<()> {
    let mut buf = Vec::new();
    mc_protocol::packets::encode_packet(packet_id, payload, &mut buf);
    stream.write_all(&buf).await
}

/// 压缩帧纯编码（可单测）：`data_len_varint + data`（不含外层 length）。
/// threshold<0 或原文长<threshold → data_len=0 明文；否则 data 为 zlib(原文)。
/// 26.3 语义：原文 = `packet_id_varint + payload`；data_len = 原文长（0 表未压缩）。
pub fn encode_compressed_frame(packet_id: i32, payload: &[u8], threshold: i32) -> Vec<u8> {
    let mut raw = Vec::with_capacity(payload.len() + 5);
    write_varint(packet_id, &mut raw);
    raw.extend_from_slice(payload);
    let mut frame = Vec::new();
    if threshold >= 0 && raw.len() >= threshold as usize {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        write_varint(raw.len() as i32, &mut frame);
        frame.extend_from_slice(&z);
    } else {
        write_varint(0, &mut frame);
        frame.extend_from_slice(&raw);
    }
    frame
}

/// 压缩帧纯解码（可单测）：输入去掉外层 length 后的帧体 → `(packet_id, payload)`。
pub fn decode_compressed_frame(frame: &[u8]) -> io::Result<(i32, Vec<u8>)> {
    let (data_len, n) = read_varint(frame).map_err(to_io_invalid)?;
    if data_len < 0 { return Err(to_io_invalid(format!("negative data length {data_len}"))); }
    let data = frame.get(n..).ok_or_else(|| to_io_invalid("truncated compressed frame"))?;
    let raw = if data_len == 0 {
        data.to_vec()
    } else {
        let out = miniz_oxide::inflate::decompress_to_vec_zlib(data)
            .map_err(|e| to_io_invalid(format!("zlib decompress: {e:?}")))?;
        if out.len() != data_len as usize { return Err(to_io_invalid(format!("data length mismatch: want {data_len}, got {}", out.len()))); }
        out
    };
    let (id, m) = read_varint(&raw).map_err(to_io_invalid)?;
    Ok((id, raw.get(m..).unwrap_or(&[]).to_vec()))
}

/// 写一帧（压缩感知）：threshold>=0 走压缩帧，否则普通帧。
pub async fn write_packet_framed(stream: &mut TcpStream, packet_id: i32, payload: &[u8], threshold: i32) -> io::Result<()> {
    if threshold < 0 { return write_packet(stream, packet_id, payload).await; }
    let frame = encode_compressed_frame(packet_id, payload, threshold);
    if frame.len() > MAX_PACKET_LEN { return Err(to_io_invalid(format!("compressed frame too large: {}", frame.len()))); }
    let mut buf = Vec::new();
    write_varint(frame.len() as i32, &mut buf);
    buf.extend_from_slice(&frame);
    stream.write_all(&buf).await
}

/// 读一帧（压缩感知）：threshold>=0 按压缩帧解析，否则普通帧。
pub async fn read_packet_framed(stream: &mut TcpStream, threshold: i32) -> io::Result<(i32, Vec<u8>)> {
    if threshold < 0 { return read_packet(stream).await; }
    match tokio::time::timeout(READ_TIMEOUT, read_packet_compressed_inner(stream)).await {
        Ok(r) => r,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "read packet timed out")),
    }
}

async fn read_packet_compressed_inner(stream: &mut TcpStream) -> io::Result<(i32, Vec<u8>)> {
    let len = read_varint_async(stream).await?;
    if len < 0 { return Err(to_io_invalid(format!("negative packet length {len}"))); }
    if len as usize > MAX_PACKET_LEN { return Err(to_io_invalid(format!("packet too large: {len}"))); }
    let mut frame = vec![0u8; len as usize];
    stream.read_exact(&mut frame).await?;
    decode_compressed_frame(&frame)
}

pub fn to_io_invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compressed_frame_small_passthrough() {
        let f = encode_compressed_frame(0x02, b"hi", 256);
        assert_eq!(read_varint(&f).unwrap().0, 0, "below threshold must be data_len=0");
        assert_eq!(decode_compressed_frame(&f).unwrap(), (0x02, b"hi".to_vec()));
    }
    #[test]
    fn compressed_frame_large_zlib_roundtrip() {
        let payload = vec![7u8; 1024];
        let f = encode_compressed_frame(0x02, &payload, 256);
        assert!(read_varint(&f).unwrap().0 > 0, "above threshold must carry data_len");
        assert_eq!(decode_compressed_frame(&f).unwrap(), (0x02, payload));
    }
    #[test]
    fn disabled_threshold_stays_plain() {
        let f = encode_compressed_frame(0x02, b"hi", -1);
        assert_eq!(read_varint(&f).unwrap().0, 0);
        assert_eq!(decode_compressed_frame(&f).unwrap(), (0x02, b"hi".to_vec()));
    }
}
