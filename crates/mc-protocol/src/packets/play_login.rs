//! play_login: Join Game (Login Play 0x32)，只兼容26.3/777，ID已按P4A对照表核对777
use crate::packets::codec::{decode_string, encode_packet, encode_string, read_bool, read_i32_be, read_i64_be, write_bool, write_i32_be, write_i64_be};
use crate::{read_varint, write_varint, ProtocolError};

/// Login Play（Join Game）包 ID（Play CB `0x32` login）。已按P4A对照表核对777（旧值0x2B实为hurt_animation）。
/// 编译期从 `mc-registry` 生成的 PACKETS 表派生。
pub const PLAY_LOGIN_PLAY_ID: i32 = super::packet_id("clientbound.play", "login");

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// `GameType.OPTIONAL_STREAM_CODEC`：Bool + 可选 VarInt，不是裸 i8。
    pub previous_gamemode: Option<u8>,
    pub is_debug: bool,
    pub is_flat: bool,
    /// `GlobalPos.STREAM_CODEC.optional`：Bool +（维度 Identifier + BlockPos packed long）。
    pub last_death_location: Option<(String, i64)>,
    pub portal_cooldown: i32,
    pub sea_level: i32,
    /// 26.3 新增：CommonPlayerSpawnInfo 之后的 onlineMode。
    pub online_mode: bool,
    pub enforce_secure_chat: bool,
}

impl Default for LoginPlay {
    fn default() -> Self {
        Self {
            entity_id: 1,
            hardcore: false,
            dimension_names: vec![
                "minecraft:overworld".to_string(),
                "minecraft:the_nether".to_string(),
                "minecraft:the_end".to_string(),
            ],
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
            previous_gamemode: None,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: 63,
            online_mode: false,
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
        write_varint(self.gamemode as i32, out);
        // GameType.OPTIONAL_STREAM_CODEC: Bool + 可选 VarInt
        match self.previous_gamemode {
            None => out.push(0),
            Some(g) => { out.push(1); write_varint(g as i32, out); }
        }
        write_bool(self.is_debug, out);
        write_bool(self.is_flat, out);
        // GlobalPos.STREAM_CODEC.optional: Bool +（维度 + packed BlockPos long）
        match &self.last_death_location {
            None => out.push(0),
            Some((dim, pos)) => {
                out.push(1);
                encode_string(dim, out);
                write_i64_be(*pos, out);
            }
        }
        write_varint(self.portal_cooldown, out);
        write_varint(self.sea_level, out);
        write_bool(self.online_mode, out);
        write_bool(self.enforce_secure_chat, out);
        // NOTE: 26.3 抓包核对（cap_play/000_32.bin）：字段到 enforcesSecureChat 为止，无旧版 portal cooldown 之外的尾字段。
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
        let (gamemode, n) = read_varint(&buf[off..])?;
        off += n;
        let (has_previous, n) = read_bool(&buf[off..])?;
        off += n;
        let previous_gamemode = if has_previous {
            let (g, n) = read_varint(&buf[off..])?;
            off += n;
            Some(g as u8)
        } else {
            None
        };
        let (is_debug, n) = read_bool(&buf[off..])?;
        off += n;
        let (is_flat, n) = read_bool(&buf[off..])?;
        off += n;
        let (has_death, n) = read_bool(&buf[off..])?;
        off += n;
        let last_death_location = if has_death {
            let (dim, n) = decode_string(&buf[off..])?;
            off += n;
            let (pos, n) = read_i64_be(&buf[off..])?;
            off += n;
            Some((dim, pos))
        } else {
            None
        };
        let (portal_cooldown, n) = read_varint(&buf[off..])?;
        off += n;
        let (sea_level, n) = read_varint(&buf[off..])?;
        off += n;
        let (online_mode, n) = read_bool(&buf[off..])?;
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
                gamemode: gamemode as u8,
                previous_gamemode,
                is_debug,
                is_flat,
                last_death_location,
                portal_cooldown,
                sea_level,
                online_mode,
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
        let (got, used) = LoginPlay::decode_body(&pkt[n+m..]).unwrap();
        assert_eq!(got, v);
        assert_eq!(used, pkt.len() - n - m, "encode/decode 必须消费完全部字节");
    }

    /// 26.3 原版服务端空 KnownPacks 实测 Login(play) 0x32 包体（109B）。
    /// 用来钉死 26.3 的字段顺序：previousGameType 是 optional、GlobalPos optional、
    /// portal_cooldown/sea_level 两个 VarInt，以及末尾 onlineMode/enforcesSecureChat。
    #[test]
    fn play_login_matches_vanilla_777_capture() {
        let body = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/777/login_play.bin"));
        let (got, used) = LoginPlay::decode_body(body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got.entity_id, 27);
        assert_eq!(got.dimension_names, vec!["minecraft:overworld", "minecraft:the_nether", "minecraft:the_end"]);
        assert_eq!(got.max_players, 10);
        assert_eq!(got.dimension_type, 0);
        assert_eq!(got.dimension_name, "minecraft:overworld");
        assert_eq!(got.seed, 6_850_048_404_533_824_663);
        assert_eq!(got.previous_gamemode, None);
        assert_eq!(got.last_death_location, None);
        assert_eq!(got.portal_cooldown, 0);
        assert_eq!(got.sea_level, 63);
        assert!(!got.online_mode);
        assert!(!got.enforce_secure_chat);
        let mut re = Vec::new();
        got.encode_body(&mut re);
        assert_eq!(re, body, "Login(play) 编码必须与原版抓包逐字节一致");
    }
}
