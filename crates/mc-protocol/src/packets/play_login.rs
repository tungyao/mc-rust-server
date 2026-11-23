//! play_login: Join Game (Login Play 0x2B)，只兼容26.3/777，ID待抓包核对
use crate::packets::codec::{decode_string, encode_packet, encode_string, read_bool, read_i32_be, read_i64_be, write_bool, write_i32_be, write_i64_be};
use crate::{read_varint, write_varint, ProtocolError};

/// Login Play（Join Game）包 ID（Play clientbound `0x2B` 左右）。待26.3抓包核对。
pub const PLAY_LOGIN_PLAY_ID: i32 = 0x2B;

pub struct LoginPlay {
    pub entity_id: i32,
    pub hardcore: bool,
    pub dimension_names: Vec<String>,
    pub max_players: i32,
    pub view_distance: i32,
    pub simulation_distance: i32,
    pub reduced_debug: bool,
    pub enable_respawn_screen: bool,
    pub do_limited_crafting: bool,
    pub dimension_type: i32,
    pub dimension_name: String,
    pub seed: i64,
    pub gamemode: u8,
    pub previous_gamemode: i8,
    pub is_debug: bool,
    pub is_flat: bool,
    pub sea_level: i32,
    pub enforce_secure_chat: bool,
}

impl Default for LoginPlay {
    fn default() -> Self {
        Self {
            entity_id: 1,
            hardcore: false,
            dimension_names: vec!["minecraft:overworld".to_string()],
            max_players: 100,
            view_distance: 10,
            simulation_distance: 10,
            reduced_debug: false,
            enable_respawn_screen: true,
            do_limited_crafting: false,
            dimension_type: 0,
            dimension_name: "minecraft:overworld".to_string(),
            seed: 123456789,
            gamemode: 0,
            previous_gamemode: -1,
            is_debug: false,
            is_flat: false,
            sea_level: 63,
            enforce_secure_chat: false,
        }
    }
}

impl LoginPlay {
    /// 编码包体（不含 length + packet id），字段顺序对标 wiki.vg。
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_i32_be(self.entity_id, out);
        write_bool(self.hardcore, out);
        write_varint(self.dimension_names.len() as i32, out);
        for d in &self.dimension_names {
            encode_string(d, out);
        }
        write_varint(self.max_players, out);
        write_varint(self.view_distance, out);
        write_varint(self.simulation_distance, out);
        write_bool(self.reduced_debug, out);
        write_bool(self.enable_respawn_screen, out);
        write_bool(self.do_limited_crafting, out);
        write_varint(self.dimension_type, out);
        encode_string(&self.dimension_name, out);
        write_i64_be(self.seed, out);
        out.push(self.gamemode);
        out.push(self.previous_gamemode as u8);
        write_bool(self.is_debug, out);
        write_bool(self.is_flat, out);
        write_varint(self.sea_level, out);
        write_bool(self.enforce_secure_chat, out);
        // NOTE: 死亡位置/传送门冷却等 777 字段 P3 补齐，待26.3抓包核对。
    }

    /// 解码包体，返回 `(包, 消费字节数)`。
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (entity_id, n) = read_i32_be(&buf[off..])?;
        off += n;
        let (hardcore, n) = read_bool(&buf[off..])?;
        off += n;
        let (dim_count, n) = read_varint(&buf[off..])?;
        off += n;
        let mut dimension_names = Vec::new();
        for _ in 0..dim_count.max(0) {
            let (s, n) = decode_string(&buf[off..])?;
            off += n;
            dimension_names.push(s);
        }
        let (max_players, n) = read_varint(&buf[off..])?;
        off += n;
        let (view_distance, n) = read_varint(&buf[off..])?;
        off += n;
        let (simulation_distance, n) = read_varint(&buf[off..])?;
        off += n;
        let (reduced_debug, n) = read_bool(&buf[off..])?;
        off += n;
        let (enable_respawn_screen, n) = read_bool(&buf[off..])?;
        off += n;
        let (do_limited_crafting, n) = read_bool(&buf[off..])?;
        off += n;
        let (dimension_type, n) = read_varint(&buf[off..])?;
        off += n;
        let (dimension_name, n) = decode_string(&buf[off..])?;
        off += n;
        let (seed, n) = read_i64_be(&buf[off..])?;
        off += n;
        if buf.len() < off + 2 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let gamemode = buf[off];
        let previous_gamemode = buf[off + 1] as i8;
        off += 2;
        let (is_debug, n) = read_bool(&buf[off..])?;
        off += n;
        let (is_flat, n) = read_bool(&buf[off..])?;
        off += n;
        let (sea_level, n) = read_varint(&buf[off..])?;
        off += n;
        let (enforce_secure_chat, n) = read_bool(&buf[off..])?;
        off += n;
        Ok((
            Self {
                entity_id,
                hardcore,
                dimension_names,
                max_players,
                view_distance,
                simulation_distance,
                reduced_debug,
                enable_respawn_screen,
                do_limited_crafting,
                dimension_type,
                dimension_name,
                seed,
                gamemode,
                previous_gamemode,
                is_debug,
                is_flat,
                sea_level,
                enforce_secure_chat,
            },
            off,
        ))
    }
}

/// Login Play 打成完整线包。
pub fn encode_login_play_packet(v: &LoginPlay, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_LOGIN_PLAY_ID, &body, out);
}

/// Set Center Chunk：客户端视距中心（均为 VarInt）。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_varint;
    use crate::packets::codec::decode_packet_id;
    #[test]
    fn play_login_play_roundtrip() {
        let v = LoginPlay::default();
        let mut pkt = Vec::new(); encode_login_play_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_LOGIN_PLAY_ID);
        let (got, _) = LoginPlay::decode_body(&pkt[n+m..]).unwrap();
        assert_eq!(got.entity_id, v.entity_id);
    }
}
