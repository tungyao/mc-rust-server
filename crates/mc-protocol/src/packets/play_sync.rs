//! play_sync: PositionSync + KeepAlive，只兼容26.3/777，ID已按P4A对照表核对777
use crate::packets::codec::{encode_packet, read_bool, read_f32_be, read_f64_be, read_i32_be, read_i64_be, write_bool, write_f32_be, write_f64_be, write_i32_be, write_i64_be};
use crate::{read_varint, write_varint, ProtocolError};

/// Player Position Sync 包 ID（Play CB `0x49` player_position）。已按P4A对照表核对777（旧值0x3E实为ping）。
pub const PLAY_PLAYER_POSITION_SYNC_ID: i32 = 0x49;
/// KeepAlive S2C/C2S 包 ID（Play CB `0x2D` / SB `0x1C` keep_alive）。已按P4A对照表核对777（旧值0x26实为unload_chunk/0x1A实为interact）。
pub const PLAY_KEEPALIVE_S2C_ID: i32 = 0x2D;
pub const PLAY_KEEPALIVE_C2S_ID: i32 = 0x1C;

pub struct PlayerPositionSync {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub flags: i8,
    pub teleport_id: i32,
}

impl PlayerPositionSync {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_f64_be(self.x, out);
        write_f64_be(self.y, out);
        write_f64_be(self.z, out);
        write_f32_be(self.yaw, out);
        write_f32_be(self.pitch, out);
        out.push(self.flags as u8);
        write_varint(self.teleport_id, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (x, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (y, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (z, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (yaw, n) = read_f32_be(&buf[off..])?;
        off += n;
        let (pitch, n) = read_f32_be(&buf[off..])?;
        off += n;
        if buf.len() < off + 1 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let flags = buf[off] as i8;
        off += 1;
        let (teleport_id, n) = read_varint(&buf[off..])?;
        off += n;
        Ok((Self { x, y, z, yaw, pitch, flags, teleport_id }, off))
    }
}

/// Player Position Sync 打成完整线包。
pub fn encode_player_position_sync_packet(v: &PlayerPositionSync, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_PLAYER_POSITION_SYNC_ID, &body, out);
}

/// KeepAlive S2C：单个 i64 big-endian。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAliveS2C(pub i64);

impl KeepAliveS2C {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_i64_be(self.0, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if buf.len() < 8 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (v, n) = read_i64_be(buf)?;
        Ok((Self(v), n))
    }
}

/// KeepAlive C2S：单个 i64 big-endian（客户端回显）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAliveC2S(pub i64);

impl KeepAliveC2S {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_i64_be(self.0, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if buf.len() < 8 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (v, n) = read_i64_be(buf)?;
        Ok((Self(v), n))
    }
}

/// KeepAlive S2C 打成完整线包（Play CB `0x2D`，已按P4A对照表核对777）。
pub fn encode_keepalive_s2c_packet(keepalive_id: i64, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    KeepAliveS2C(keepalive_id).encode_body(&mut body);
    encode_packet(PLAY_KEEPALIVE_S2C_ID, &body, out);
}

/// KeepAlive C2S 解码（Play SB `0x1C`，已按P4A对照表核对777），要求 payload 恰好 8 字节。
pub fn decode_keepalive_c2s(payload: &[u8]) -> Result<i64, ProtocolError> {
    if payload.len() != 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let (v, _) = read_i64_be(payload)?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_varint;
    use crate::packets::codec::decode_packet_id;
    #[test]
    fn play_position_sync_roundtrip() {
        let v = PlayerPositionSync { x: 8.5, y: 80.0, z: 8.5, yaw: 0.0, pitch: 0.0, flags: 0, teleport_id: 1 };
        let mut pkt = Vec::new(); encode_player_position_sync_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_PLAYER_POSITION_SYNC_ID);
        assert_eq!(PlayerPositionSync::decode_body(&pkt[n+m..]).unwrap().0.x, v.x);
    }
    #[test]
    fn play_keepalive_roundtrip() {
        let mut b = Vec::new(); encode_keepalive_s2c_packet(7, &mut b);
        let (_, n) = read_varint(&b).unwrap();
        let (id, m) = decode_packet_id(&b[n..]).unwrap();
        assert_eq!(id, PLAY_KEEPALIVE_S2C_ID);
        assert_eq!(decode_keepalive_c2s(&b[n+m..]).unwrap(), 7);
    }
}