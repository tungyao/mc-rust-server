//! codec: 基础编解码 (String/u16/i64/i32/f64/f32/bool + 组帧)，只兼容26.3
use crate::{read_varint, write_varint, ProtocolError};

pub fn encode_string(s: &str, out: &mut Vec<u8>) {
    write_varint(s.len() as i32, out);
    out.extend_from_slice(s.as_bytes());
}

pub fn decode_string(buf: &[u8]) -> Result<(String, usize), ProtocolError> {
    let (len, n) = read_varint(buf)?;
    let start = n;
    let end = start + len as usize;
    if buf.len() < end {
        return Err(ProtocolError::UnexpectedEof);
    }
    let s = String::from_utf8_lossy(&buf[start..end]).into_owned();
    Ok((s, end))
}

pub fn write_u16_be(v: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

pub fn read_u16_be(buf: &[u8]) -> Result<(u16, usize), ProtocolError> {
    if buf.len() < 2 {
        return Err(ProtocolError::UnexpectedEof);
    }
    Ok((u16::from_be_bytes([buf[0], buf[1]]), 2))
}

pub fn write_i64_be(v: i64, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

pub fn read_i64_be(buf: &[u8]) -> Result<(i64, usize), ProtocolError> {
    if buf.len() < 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[..8]);
    Ok((i64::from_be_bytes(b), 8))
}

/// 把 `packet_id + payload` 打成完整线包：`VarInt(length) + VarInt(id) + payload`。
pub fn encode_packet(packet_id: i32, payload: &[u8], out: &mut Vec<u8>) {
    let mut body = Vec::with_capacity(payload.len() + 5);
    write_varint(packet_id, &mut body);
    body.extend_from_slice(payload);
    write_varint(body.len() as i32, out);
    out.extend_from_slice(&body);
}

/// 从去掉 length 后的包体读 packet id，返回 `(packet_id, id占字节数)`，
/// 之后 `&buf[n..]` 即 payload。
pub fn decode_packet_id(buf: &[u8]) -> Result<(i32, usize), ProtocolError> {
    read_varint(buf)
}

/// Status Request（serverbound `0x00`，无字段）打包成完整线包。

pub fn write_i32_be(v: i32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

pub fn read_i32_be(buf: &[u8]) -> Result<(i32, usize), ProtocolError> {
    if buf.len() < 4 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[..4]);
    Ok((i32::from_be_bytes(b), 4))
}

pub fn write_f64_be(v: f64, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

pub fn read_f64_be(buf: &[u8]) -> Result<(f64, usize), ProtocolError> {
    if buf.len() < 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[..8]);
    Ok((f64::from_be_bytes(b), 8))
}

pub fn write_f32_be(v: f32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

pub fn read_f32_be(buf: &[u8]) -> Result<(f32, usize), ProtocolError> {
    if buf.len() < 4 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[..4]);
    Ok((f32::from_be_bytes(b), 4))
}

pub fn write_bool(v: bool, out: &mut Vec<u8>) {
    out.push(if v { 1 } else { 0 });
}

pub fn read_bool(buf: &[u8]) -> Result<(bool, usize), ProtocolError> {
    if buf.is_empty() {
        return Err(ProtocolError::UnexpectedEof);
    }
    match buf[0] {
        0 => Ok((false, 1)),
        1 => Ok((true, 1)),
        _ => Err(ProtocolError::UnexpectedEof),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codec_string_roundtrip() {
        for s in ["", "localhost", "mc.例子.test"] {
            let mut b = Vec::new();
            encode_string(s, &mut b);
            let (got, used) = decode_string(&b).unwrap();
            assert_eq!(got, s);
            assert_eq!(used, b.len());
        }
    }
    #[test]
    fn codec_ints_roundtrip() {
        let mut b = Vec::new();
        write_u16_be(25565, &mut b);
        assert_eq!(read_u16_be(&b).unwrap().0, 25565);
        let mut b = Vec::new();
        write_i64_be(-5, &mut b);
        assert_eq!(read_i64_be(&b).unwrap().0, -5);
    }
}
