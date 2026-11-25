//! play_block: 挖掘/放置 C2S + 方块更新 S2C，只兼容 26.3/777。
//!
//! ID 来源：minecraft.wiki Play 包表（777，见 `docs/packet-map-777.md` §6）：
//! - SB `player_action` 0x29：Status VarInt + Position + Face Byte + Sequence VarInt
//! - SB `use_item_on` 0x42：Hand VarInt + Position + Face VarInt + 3×Float + 2×Bool + Sequence VarInt
//! - SB `use_item` 0x43：Hand VarInt + Sequence VarInt + Yaw/Pitch Float
//! - SB `punch` 0x2E：空包体（手臂挥动动画，无状态变化）
//! - CB `block_update` 0x08：Position + blockstate VarInt（全局 palette ID）
//! - CB `block_changed_ack` 0x04：Sequence VarInt（确认客户端预测，拒绝时先发 BlockUpdate 回滚）
//!
//! P17 只用以上包：创造风格即时破坏/放置；`Set Held Item`(0x36) 与
//! `Set Creative Mode Slot`(0x39) 暂不解析（Slot 格式的 data component 需要
//! 注册表驱动解码，误解析会错位整条 TCP 流），放置统一用服务端默认材料。
use crate::packets::codec::{
    encode_packet, read_bool, read_f32_be, read_i64_be, write_bool, write_f32_be, write_i64_be,
};
use crate::{read_varint, write_varint, ProtocolError};

// 以下包 ID 全部编译期从 `mc-registry` 由 data/777/packets.json 生成的 PACKETS 表派生。

/// C2S Player Action（挖方块/取消/完成/丢弃等）包 ID（777 SB `0x29`）。
pub const PLAY_PLAYER_ACTION_C2S_ID: i32 = super::packet_id("serverbound.play", "player_action");
/// C2S Use Item On（对方块使用/放置）包 ID（777 SB `0x42`）。
pub const PLAY_USE_ITEM_ON_C2S_ID: i32 = super::packet_id("serverbound.play", "use_item_on");
/// C2S Use Item（空手使用）包 ID（777 SB `0x43`）。
pub const PLAY_USE_ITEM_C2S_ID: i32 = super::packet_id("serverbound.play", "use_item");
/// C2S Punch（挥拳动画）包 ID（777 SB `0x2E`）：空包体，只忽略。
pub const PLAY_PUNCH_C2S_ID: i32 = super::packet_id("serverbound.play", "punch");
/// C2S Click Container：`container_click`（Hashed Slot 复杂，本轮只忽略不断线，见模块文档）。
pub const PLAY_CLICK_CONTAINER_C2S_ID: i32 = super::packet_id("serverbound.play", "container_click");
/// C2S Set Held Item（切换手持槽位）包 ID（777 SB `0x36`）：P17 暂不解析，只忽略。
pub const PLAY_SET_HELD_ITEM_C2S_ID: i32 = super::packet_id("serverbound.play", "set_carried_item");
/// C2S Set Creative Mode Slot（创造模式点物品）包 ID（777 SB `0x39`）：P17 暂不解析，只忽略。
pub const PLAY_SET_CREATIVE_SLOT_C2S_ID: i32 = super::packet_id("serverbound.play", "set_creative_mode_slot");
/// S2C Block Update（单方块变更）包 ID（777 CB `0x08`）。
pub const PLAY_BLOCK_UPDATE_S2C_ID: i32 = super::packet_id("clientbound.play", "block_update");
/// S2C Acknowledge Block Change（确认方块变更序列号）包 ID（777 CB `0x04`）。
pub const PLAY_BLOCK_CHANGED_ACK_S2C_ID: i32 = super::packet_id("clientbound.play", "block_changed_ack");

/// Player Action Status：开始挖掘（创造模式下客户端只发这个，视为已完成）。
pub const PLAYER_ACTION_START_DIG: i32 = 0;
/// Player Action Status：取消挖掘。
pub const PLAYER_ACTION_CANCEL_DIG: i32 = 1;
/// Player Action Status：完成挖掘（生存模式按住挖完）。
pub const PLAYER_ACTION_FINISH_DIG: i32 = 2;

/// 方块坐标 pack：26 位 x + 26 位 z + 12 位 y（见 wiki Data types/Position）。
/// `((x & 0x3FFFFFF) << 38) | ((z & 0x3FFFFFF) << 12) | (y & 0xFFF)`。
pub fn pack_block_pos(x: i32, y: i32, z: i32) -> u64 {
    ((x as u64 & 0x3FF_FFFF) << 38) | ((z as u64 & 0x3FF_FFFF) << 12) | (y as u64 & 0xFFF)
}

/// 方块坐标 unpack：i64 算术右移保留符号（负坐标正确）。
pub fn unpack_block_pos(v: u64) -> (i32, i32, i32) {
    let s = v as i64;
    ((s >> 38) as i32, ((s << 52) >> 52) as i32, ((s << 26) >> 38) as i32)
}

fn read_block_pos(buf: &[u8]) -> Result<((i32, i32, i32), usize), ProtocolError> {
    let (v, n) = read_i64_be(buf)?;
    Ok((unpack_block_pos(v as u64), n))
}

fn write_block_pos(x: i32, y: i32, z: i32, out: &mut Vec<u8>) {
    write_i64_be(pack_block_pos(x, y, z) as i64, out);
}

/// Face → 被点击方块的邻居偏移（wiki Player Action 表：0:-Y 1:+Y 2:-Z 3:+Z 4:-X 5:+X）。
/// 非法 face 返回 None（调用方只 ack，不改世界）。
pub fn face_offset(face: i8) -> Option<(i32, i32, i32)> {
    match face {
        0 => Some((0, -1, 0)),
        1 => Some((0, 1, 0)),
        2 => Some((0, 0, -1)),
        3 => Some((0, 0, 1)),
        4 => Some((-1, 0, 0)),
        5 => Some((1, 0, 0)),
        _ => None,
    }
}

/// C2S Player Action（0x29）：Status + 方块坐标 + Face(i8) + Sequence。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerAction {
    pub status: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub face: i8,
    pub sequence: i32,
}

impl PlayerAction {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.status, out);
        write_block_pos(self.x, self.y, self.z, out);
        out.push(self.face as u8);
        write_varint(self.sequence, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (status, n) = read_varint(&buf[off..])?;
        off += n;
        let ((x, y, z), n) = read_block_pos(&buf[off..])?;
        off += n;
        if buf.len() <= off {
            return Err(ProtocolError::UnexpectedEof);
        }
        let face = buf[off] as i8;
        off += 1;
        let (sequence, n) = read_varint(&buf[off..])?;
        off += n;
        Ok((Self { status, x, y, z, face, sequence }, off))
    }
}

/// C2S Use Item On（0x42）：Hand + 方块坐标 + Face(VarInt) + 3×Float + 2×Bool + Sequence。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseItemOn {
    pub hand: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub face: i32,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub cursor_z: f32,
    pub inside_block: bool,
    pub world_border_hit: bool,
    pub sequence: i32,
}

impl UseItemOn {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.hand, out);
        write_block_pos(self.x, self.y, self.z, out);
        write_varint(self.face, out);
        write_f32_be(self.cursor_x, out);
        write_f32_be(self.cursor_y, out);
        write_f32_be(self.cursor_z, out);
        write_bool(self.inside_block, out);
        write_bool(self.world_border_hit, out);
        write_varint(self.sequence, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (hand, n) = read_varint(&buf[off..])?;
        off += n;
        let ((x, y, z), n) = read_block_pos(&buf[off..])?;
        off += n;
        let (face, n) = read_varint(&buf[off..])?;
        off += n;
        let (cursor_x, n) = read_f32_be(&buf[off..])?;
        off += n;
        let (cursor_y, n) = read_f32_be(&buf[off..])?;
        off += n;
        let (cursor_z, n) = read_f32_be(&buf[off..])?;
        off += n;
        let (inside_block, n) = read_bool(&buf[off..])?;
        off += n;
        let (world_border_hit, n) = read_bool(&buf[off..])?;
        off += n;
        let (sequence, n) = read_varint(&buf[off..])?;
        off += n;
        Ok((
            Self {
                hand,
                x,
                y,
                z,
                face,
                cursor_x,
                cursor_y,
                cursor_z,
                inside_block,
                world_border_hit,
                sequence,
            },
            off,
        ))
    }
}

/// C2S Use Item（0x43）：Hand + Sequence + Yaw + Pitch（无方块目标，只 ack）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseItem {
    pub hand: i32,
    pub sequence: i32,
    pub yaw: f32,
    pub pitch: f32,
}

impl UseItem {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.hand, out);
        write_varint(self.sequence, out);
        write_f32_be(self.yaw, out);
        write_f32_be(self.pitch, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (hand, n) = read_varint(&buf[off..])?;
        off += n;
        let (sequence, n) = read_varint(&buf[off..])?;
        off += n;
        let (yaw, n) = read_f32_be(&buf[off..])?;
        off += n;
        let (pitch, n) = read_f32_be(&buf[off..])?;
        off += n;
        Ok((Self { hand, sequence, yaw, pitch }, off))
    }
}

/// S2C Block Update（0x08）包体：方块坐标 + 新 blockstate ID（全局 palette）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockUpdate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block_id: i32,
}

impl BlockUpdate {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_block_pos(self.x, self.y, self.z, out);
        write_varint(self.block_id, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let ((x, y, z), n) = read_block_pos(buf)?;
        let (block_id, m) = read_varint(&buf[n..])?;
        Ok((Self { x, y, z, block_id }, n + m))
    }
}

/// Block Update 打成完整线包。
pub fn encode_block_update_packet(v: &BlockUpdate, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_BLOCK_UPDATE_S2C_ID, &body, out);
}

/// Acknowledge Block Change（0x04）打成完整线包：只有 Sequence VarInt。
pub fn encode_block_changed_ack_packet(sequence: i32, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_varint(sequence, &mut body);
    encode_packet(PLAY_BLOCK_CHANGED_ACK_S2C_ID, &body, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::codec::decode_packet_id;
    use crate::read_varint;

    #[test]
    fn block_interaction_packet_ids_match_777() {
        assert_eq!(PLAY_PLAYER_ACTION_C2S_ID, 0x29);
        assert_eq!(PLAY_USE_ITEM_ON_C2S_ID, 0x42);
        assert_eq!(PLAY_USE_ITEM_C2S_ID, 0x43);
        assert_eq!(PLAY_PUNCH_C2S_ID, 0x2E);
        assert_eq!(PLAY_BLOCK_UPDATE_S2C_ID, 0x08);
        assert_eq!(PLAY_BLOCK_CHANGED_ACK_S2C_ID, 0x04);
    }

    #[test]
    fn block_pos_pack_unpack_roundtrip_with_negatives() {
        for (x, y, z) in [
            (0, 0, 0),
            (369, 73, 381),
            (-1, -64, -1),
            (-30000000, 320, 30000000),
            (30000000, -2048, -30000000),
            (33_554_431, 2047, -33_554_432),
        ] {
            assert_eq!(unpack_block_pos(pack_block_pos(x, y, z)), (x, y, z), "pos=({x},{y},{z})");
        }
        // 已知向量：(1,64,-1) 的高低位布局
        let v = pack_block_pos(1, 64, -1);
        assert_eq!(v, (1u64 << 38) | (0x3FF_FFFFu64 << 12) | 64);
    }

    #[test]
    fn face_offsets_cover_six_sides() {
        assert_eq!(face_offset(0), Some((0, -1, 0)));
        assert_eq!(face_offset(1), Some((0, 1, 0)));
        assert_eq!(face_offset(2), Some((0, 0, -1)));
        assert_eq!(face_offset(3), Some((0, 0, 1)));
        assert_eq!(face_offset(4), Some((-1, 0, 0)));
        assert_eq!(face_offset(5), Some((1, 0, 0)));
        assert_eq!(face_offset(6), None);
        assert_eq!(face_offset(-1), None);
    }

    #[test]
    fn player_action_roundtrip() {
        let v = PlayerAction { status: 2, x: 369, y: 72, z: 381, face: 1, sequence: 7 };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = PlayerAction::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        // 截断必须报错（Face/Sequence 缺失）
        assert!(PlayerAction::decode_body(&body[..body.len() - 2]).is_err());
        assert!(PlayerAction::decode_body(&[]).is_err());
    }

    #[test]
    fn use_item_on_roundtrip() {
        let v = UseItemOn {
            hand: 0,
            x: 369,
            y: 72,
            z: 381,
            face: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.25,
            inside_block: false,
            world_border_hit: false,
            sequence: 9,
        };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = UseItemOn::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        assert!(UseItemOn::decode_body(&body[..5]).is_err());
    }

    #[test]
    fn use_item_roundtrip() {
        let v = UseItem { hand: 1, sequence: 3, yaw: 90.0, pitch: -10.0 };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = UseItem::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        assert!(UseItem::decode_body(&[0x00]).is_err());
    }

    #[test]
    fn block_update_and_ack_wire_packets() {
        let u = BlockUpdate { x: 369, y: 72, z: 381, block_id: 0 };
        let mut pkt = Vec::new();
        encode_block_update_packet(&u, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_BLOCK_UPDATE_S2C_ID);
        assert_eq!(BlockUpdate::decode_body(&pkt[n + m..]).unwrap().0, u);

        let mut ack = Vec::new();
        encode_block_changed_ack_packet(7, &mut ack);
        let (_, n) = read_varint(&ack).unwrap();
        let (id, m) = decode_packet_id(&ack[n..]).unwrap();
        assert_eq!(id, PLAY_BLOCK_CHANGED_ACK_S2C_ID);
        assert_eq!(read_varint(&ack[n + m..]).unwrap().0, 7);
    }

    #[test]
    fn dig_start_status_is_zero_and_cancel_is_one() {
        assert_eq!(PLAYER_ACTION_START_DIG, 0);
        assert_eq!(PLAYER_ACTION_CANCEL_DIG, 1);
        assert_eq!(PLAYER_ACTION_FINISH_DIG, 2);
    }
}
