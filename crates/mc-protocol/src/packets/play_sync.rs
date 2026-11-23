//! play_sync: PositionSync(S2C) + KeepAlive + C2S移动包，只兼容26.3/777，ID已按P4A对照表+777 SB表核对
//! C2S移动包ID（777 SB）：0x1E move_player_pos / 0x1F move_player_pos_rot /
//! 0x20 move_player_rot / 0x21 move_player_status_only（Play serverbound表）。
//! 注意：此前猜测0x1B/0x1C是错的——777里SB 0x1C是KeepAlive回显（与C2S KeepAlive 0x1C冲突故不可复用），
//! SB 0x1A是Interact、0x1B是Jigsaw Generate，移动包另排在0x1E-0x21。见docs/packet-map-777.md §4与
//! minecraft.wiki Play serverbound表（777）：30/0x1E Set Player Position等4行。
use crate::packets::codec::{encode_packet, read_f32_be, read_f64_be, read_i32_be, read_i64_be, write_f32_be, write_f64_be, write_i32_be, write_i64_be};
use crate::{read_varint, write_varint, ProtocolError};

/// Player Position Sync 包 ID（Play CB `0x49` player_position）。已按P4A对照表核对777（旧值0x3E实为ping）。
pub const PLAY_PLAYER_POSITION_SYNC_ID: i32 = 0x49;
/// KeepAlive S2C/C2S 包 ID（Play CB `0x2D` / SB `0x1C` keep_alive）。已按P4A对照表核对777（旧值0x26实为unload_chunk/0x1A实为interact）。
pub const PLAY_KEEPALIVE_S2C_ID: i32 = 0x2D;
pub const PLAY_KEEPALIVE_C2S_ID: i32 = 0x1C;

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerPositionSync {
    /// 26.3 `ClientboundPlayerPositionPacket(id, change, relatives)`：id 在前。
    pub teleport_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// PositionMoveRotation.deltaMovement（3×Double，普通传送为 0）。
    pub delta_x: f64,
    pub delta_y: f64,
    pub delta_z: f64,
    pub yaw: f32,
    pub pitch: f32,
    /// `Relative.SET_STREAM_CODEC`：Int 位掩码（0=全部绝对坐标）。
    pub relatives: i32,
}

impl PlayerPositionSync {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.teleport_id, out);
        write_f64_be(self.x, out);
        write_f64_be(self.y, out);
        write_f64_be(self.z, out);
        write_f64_be(self.delta_x, out);
        write_f64_be(self.delta_y, out);
        write_f64_be(self.delta_z, out);
        write_f32_be(self.yaw, out);
        write_f32_be(self.pitch, out);
        write_i32_be(self.relatives, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (teleport_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (x, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (y, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (z, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (delta_x, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (delta_y, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (delta_z, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (yaw, n) = read_f32_be(&buf[off..])?;
        off += n;
        let (pitch, n) = read_f32_be(&buf[off..])?;
        off += n;
        let (relatives, n) = read_i32_be(&buf[off..])?;
        off += n;
        Ok((Self { teleport_id, x, y, z, delta_x, delta_y, delta_z, yaw, pitch, relatives }, off))
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

// ---------------------------------------------------------------------------
// C2S 移动包（777 SB 0x1E-0x21）。线格式（26.3）：
// - 0x1E Pos: f64 x/y/z + u8 flags
// - 0x1F PosRot: f64 x/y/z + f32 yaw/pitch + u8 flags
// - 0x20 Rot: f32 yaw/pitch + u8 flags
// - 0x21 StatusOnly: u8 flags
// flags: bit0 on_ground，bit1 horizontal_collision（26.3把旧版独立bool并进flags字节）。
// 字段名与服务端 `mc-server/src/movement.rs::Position` 对齐：x/y/z/yaw/pitch/on_ground。
// ---------------------------------------------------------------------------

/// Set Player Position C2S 包 ID（Play SB `0x1E` move_player_pos，777）。
pub const PLAY_MOVE_POS_C2S_ID: i32 = 0x1E;
/// Set Player Position and Rotation C2S 包 ID（Play SB `0x1F` move_player_pos_rot，777）。
pub const PLAY_MOVE_POSROT_C2S_ID: i32 = 0x1F;
/// Set Player Rotation C2S 包 ID（Play SB `0x20` move_player_rot，777）。
pub const PLAY_MOVE_ROT_C2S_ID: i32 = 0x20;
/// Set Player Movement Flags C2S 包 ID（Play SB `0x21` move_player_status_only，777）。
pub const PLAY_MOVE_FLAGS_C2S_ID: i32 = 0x21;

/// 移动 flags 打包/解包：bit0 on_ground，bit1 horizontal_collision。
pub fn pack_move_flags(on_ground: bool, horizontal_collision: bool) -> u8 {
    (u8::from(on_ground)) | (u8::from(horizontal_collision) << 1)
}

pub fn unpack_move_flags(flags: u8) -> (bool, bool) {
    (flags & 0x01 != 0, flags & 0x02 != 0)
}

/// C2S Set Player Position（0x1E）：只有位置 + flags。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovePlayerPos {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub on_ground: bool,
    pub horizontal_collision: bool,
}

impl MovePlayerPos {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_f64_be(self.x, out);
        write_f64_be(self.y, out);
        write_f64_be(self.z, out);
        out.push(pack_move_flags(self.on_ground, self.horizontal_collision));
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if buf.len() != 24 + 1 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (x, mut n) = read_f64_be(&buf[0..])?;
        let mut off = n;
        let (y, m) = read_f64_be(&buf[off..])?;
        off += m;
        let (z, m) = read_f64_be(&buf[off..])?;
        off += m;
        let flags = buf[off];
        off += 1;
        let (on_ground, horizontal_collision) = unpack_move_flags(flags);
        let _ = n;
        n = off;
        Ok((Self { x, y, z, on_ground, horizontal_collision }, n))
    }
}

/// C2S Set Player Position and Rotation（0x1F）：位置 + 旋转 + flags。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovePlayerPosRot {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
}

impl MovePlayerPosRot {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_f64_be(self.x, out);
        write_f64_be(self.y, out);
        write_f64_be(self.z, out);
        write_f32_be(self.yaw, out);
        write_f32_be(self.pitch, out);
        out.push(pack_move_flags(self.on_ground, self.horizontal_collision));
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if buf.len() != 24 + 8 + 1 {
            return Err(ProtocolError::UnexpectedEof);
        }
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
        let flags = buf[off];
        off += 1;
        let (on_ground, horizontal_collision) = unpack_move_flags(flags);
        Ok((Self { x, y, z, yaw, pitch, on_ground, horizontal_collision }, off))
    }
}

/// C2S Set Player Rotation（0x20）：只有旋转 + flags。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovePlayerRot {
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
}

impl MovePlayerRot {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_f32_be(self.yaw, out);
        write_f32_be(self.pitch, out);
        out.push(pack_move_flags(self.on_ground, self.horizontal_collision));
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if buf.len() != 8 + 1 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (yaw, mut off) = read_f32_be(&buf[0..])?;
        let (pitch, n) = read_f32_be(&buf[off..])?;
        off += n;
        let flags = buf[off];
        off += 1;
        let (on_ground, horizontal_collision) = unpack_move_flags(flags);
        Ok((Self { yaw, pitch, on_ground, horizontal_collision }, off))
    }
}

/// C2S Set Player Movement Flags（0x21，status_only）：只有 flags。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovePlayerFlags {
    pub on_ground: bool,
    pub horizontal_collision: bool,
}

impl MovePlayerFlags {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        out.push(pack_move_flags(self.on_ground, self.horizontal_collision));
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if buf.len() != 1 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (on_ground, horizontal_collision) = unpack_move_flags(buf[0]);
        Ok((Self { on_ground, horizontal_collision }, 1))
    }
}

/// C2S 移动包统一分发：按包 ID 解出对应结构。非移动包 ID 返回 InvalidPacketId。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum C2SMovement {
    Pos(MovePlayerPos),
    PosRot(MovePlayerPosRot),
    Rot(MovePlayerRot),
    Flags(MovePlayerFlags),
}

pub fn decode_c2s_movement(packet_id: i32, payload: &[u8]) -> Result<C2SMovement, ProtocolError> {
    match packet_id {
        PLAY_MOVE_POS_C2S_ID => Ok(C2SMovement::Pos(MovePlayerPos::decode_body(payload)?.0)),
        PLAY_MOVE_POSROT_C2S_ID => Ok(C2SMovement::PosRot(MovePlayerPosRot::decode_body(payload)?.0)),
        PLAY_MOVE_ROT_C2S_ID => Ok(C2SMovement::Rot(MovePlayerRot::decode_body(payload)?.0)),
        PLAY_MOVE_FLAGS_C2S_ID => Ok(C2SMovement::Flags(MovePlayerFlags::decode_body(payload)?.0)),
        other => Err(ProtocolError::InvalidPacketId(other)),
    }
}

/// C2S 移动包打成完整线包（供测试模拟客户端）。
pub fn encode_move_pos_packet(v: &MovePlayerPos, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_MOVE_POS_C2S_ID, &body, out);
}

pub fn encode_move_posrot_packet(v: &MovePlayerPosRot, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_MOVE_POSROT_C2S_ID, &body, out);
}

pub fn encode_move_rot_packet(v: &MovePlayerRot, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_MOVE_ROT_C2S_ID, &body, out);
}

pub fn encode_move_flags_packet(v: &MovePlayerFlags, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_MOVE_FLAGS_C2S_ID, &body, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_varint;
    use crate::packets::codec::decode_packet_id;
    #[test]
    fn play_position_sync_roundtrip() {
        let v = PlayerPositionSync {
            teleport_id: 1,
            x: 8.5, y: 80.0, z: 8.5,
            delta_x: 0.0, delta_y: 0.0, delta_z: 0.0,
            yaw: 0.0, pitch: 0.0, relatives: 0,
        };
        let mut pkt = Vec::new(); encode_player_position_sync_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_PLAYER_POSITION_SYNC_ID);
        let (got, used) = PlayerPositionSync::decode_body(&pkt[n+m..]).unwrap();
        assert_eq!(got, v);
        assert_eq!(used, pkt.len() - n - m);
    }

    /// 26.3 原版服务端实测 player_position 0x49 包体（61B）：
    /// VarInt teleportId + PositionMoveRotation(3×pos, 3×delta, yaw, pitch) + Int relative mask。
    #[test]
    fn play_position_sync_matches_vanilla_777_capture() {
        let body = include_bytes!("data/player_position_sync_777.bin");
        let (got, used) = PlayerPositionSync::decode_body(body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got.teleport_id, 1);
        assert_eq!(got.delta_x, 0.0);
        assert_eq!(got.delta_y, 0.0);
        assert_eq!(got.delta_z, 0.0);
        assert_eq!(got.yaw, 0.0);
        assert_eq!(got.pitch, 0.0);
        assert_eq!(got.relatives, 0);
        assert!(got.x > 0.0 && got.y > 0.0 && got.z > 0.0);
        let mut re = Vec::new();
        got.encode_body(&mut re);
        assert_eq!(re, body, "player_position 编码必须与原版抓包逐字节一致");
    }
    #[test]
    fn play_keepalive_roundtrip() {
        let mut b = Vec::new(); encode_keepalive_s2c_packet(7, &mut b);
        let (_, n) = read_varint(&b).unwrap();
        let (id, m) = decode_packet_id(&b[n..]).unwrap();
        assert_eq!(id, PLAY_KEEPALIVE_S2C_ID);
        assert_eq!(decode_keepalive_c2s(&b[n+m..]).unwrap(), 7);
    }
    #[test]
    fn c2s_movement_ids_are_0x1e_to_0x21_not_keepalive() {
        // 777确认：SB 0x1C是KeepAlive，不可复用；移动包是0x1E-0x21。
        assert_eq!(PLAY_KEEPALIVE_C2S_ID, 0x1C);
        assert_eq!(PLAY_MOVE_POS_C2S_ID, 0x1E);
        assert_eq!(PLAY_MOVE_POSROT_C2S_ID, 0x1F);
        assert_eq!(PLAY_MOVE_ROT_C2S_ID, 0x20);
        assert_eq!(PLAY_MOVE_FLAGS_C2S_ID, 0x21);
    }
    #[test]
    fn c2s_movement_roundtrip() {
        let pos = MovePlayerPos { x: 8.5, y: 80.0, z: -3.25, on_ground: true, horizontal_collision: false };
        let mut pkt = Vec::new(); encode_move_pos_packet(&pos, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_MOVE_POS_C2S_ID);
        assert_eq!(decode_c2s_movement(id, &pkt[n+m..]).unwrap(), C2SMovement::Pos(pos));

        let pr = MovePlayerPosRot { x: 1.0, y: 2.0, z: 3.0, yaw: 90.0, pitch: -45.0, on_ground: false, horizontal_collision: true };
        let mut pkt = Vec::new(); encode_move_posrot_packet(&pr, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(decode_c2s_movement(id, &pkt[n+m..]).unwrap(), C2SMovement::PosRot(pr));

        let rot = MovePlayerRot { yaw: 180.0, pitch: 0.0, on_ground: true, horizontal_collision: true };
        let mut pkt = Vec::new(); encode_move_rot_packet(&rot, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(decode_c2s_movement(id, &pkt[n+m..]).unwrap(), C2SMovement::Rot(rot));

        let fl = MovePlayerFlags { on_ground: false, horizontal_collision: false };
        let mut pkt = Vec::new(); encode_move_flags_packet(&fl, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(decode_c2s_movement(id, &pkt[n+m..]).unwrap(), C2SMovement::Flags(fl));
    }
    #[test]
    fn c2s_movement_rejects_bad_len_and_id() {
        assert!(MovePlayerPos::decode_body(&[0u8; 10]).is_err());
        assert!(MovePlayerRot::decode_body(&[0u8; 3]).is_err());
        assert!(MovePlayerFlags::decode_body(&[]).is_err());
        assert!(matches!(
            decode_c2s_movement(PLAY_KEEPALIVE_C2S_ID, &[0u8; 8]),
            Err(crate::ProtocolError::InvalidPacketId(_))
        ));
    }
    #[test]
    fn move_flags_pack_unpack() {
        assert_eq!(pack_move_flags(true, false), 0x01);
        assert_eq!(pack_move_flags(false, true), 0x02);
        assert_eq!(unpack_move_flags(0x03), (true, true));
        assert_eq!(unpack_move_flags(0x00), (false, false));
    }
}