//! 26.3 (777) 专用包定义，不做多版本兼容
//! 后续走 build.rs + minecraft-data/777 codegen，现在手写 Status/Handshake 打通 P0
//!
//! 字段布局对标 wiki.vg protocol 777（Status 阶段）：
//! - Handshaking serverbound `0x00`: `VarInt protocol` + `String address` + `u16 port` + `VarInt next_state`
//! - Status serverbound `0x00` (Status Request): 无字段
//! - Status clientbound `0x00` (Status Response): `String json`
//! - Status serverbound `0x01` (Ping Request) / clientbound `0x01` (Pong): `i64 payload` (big-endian)
//!
//! 约定：`*_body` 编解码的是去掉 length + packet id 后的 payload；
//! 线包组帧统一走 [`encode_packet`]（`VarInt(length) + VarInt(id) + payload`）。
use crate::{read_varint, write_varint, ProtocolError};
use uuid::Uuid;

/// Handshake 包 ID（Handshaking state，serverbound `0x00`）。
pub const HANDSHAKE_PACKET_ID: i32 = 0x00;
/// Status Request 包 ID（Status state，serverbound `0x00`）。
pub const STATUS_REQUEST_PACKET_ID: i32 = 0x00;
/// Status Response 包 ID（Status state，clientbound `0x00`）。
pub const STATUS_RESPONSE_PACKET_ID: i32 = 0x00;
/// Ping Request 包 ID（Status state，serverbound `0x01`）。
pub const PING_PACKET_ID: i32 = 0x01;
/// Pong 包 ID（Status state，clientbound `0x01`）。
pub const PONG_PACKET_ID: i32 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeNextState {
    Status = 1,
    Login = 2,
}

impl HandshakeNextState {
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    pub fn from_i32(v: i32) -> Result<Self, ProtocolError> {
        match v {
            1 => Ok(HandshakeNextState::Status),
            2 => Ok(HandshakeNextState::Login),
            other => Err(ProtocolError::InvalidNextState(other)),
        }
    }
}

impl TryFrom<i32> for HandshakeNextState {
    type Error = ProtocolError;

    fn try_from(v: i32) -> Result<Self, Self::Error> {
        Self::from_i32(v)
    }
}

/// Handshake 包体（wiki.vg: Handshaking serverbound `0x00`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: HandshakeNextState,
}

impl Handshake {
    /// 编码包体字段（不含 length + packet id）。
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.protocol_version, out);
        encode_string(&self.server_address, out);
        write_u16_be(self.server_port, out);
        write_varint(self.next_state.as_i32(), out);
    }

    /// 解码包体（已去掉 packet id 的 payload），返回 `(包, 消费字节数)`。
    /// 尾部多余字节会被忽略，调用方可用消费字节数做严格校验。
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (protocol_version, n1) = read_varint(buf)?;
        let (server_address, n2) = decode_string(&buf[n1..])?;
        let off = n1 + n2;
        let (server_port, _) = read_u16_be(&buf[off..])?;
        let (next_raw, n3) = read_varint(&buf[off + 2..])?;
        let next_state = HandshakeNextState::from_i32(next_raw)?;
        Ok((
            Self {
                protocol_version,
                server_address,
                server_port,
                next_state,
            },
            off + 2 + n3,
        ))
    }
}

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
pub fn encode_status_request_packet(out: &mut Vec<u8>) {
    encode_packet(STATUS_REQUEST_PACKET_ID, &[], out);
}

/// Status Request 解码：按 wiki 无字段，尾部字节直接忽略（兼容性优先）。
pub fn decode_status_request(_payload: &[u8]) -> Result<(), ProtocolError> {
    Ok(())
}

/// Status Response（clientbound `0x00`，单个 String JSON）打包成完整线包。
pub fn encode_status_response_packet(json: &str, out: &mut Vec<u8>) {
    let mut payload = Vec::new();
    encode_string(json, &mut payload);
    encode_packet(STATUS_RESPONSE_PACKET_ID, &payload, out);
}

/// Status Response 解码，返回 `(json, 消费字节数)`。
pub fn decode_status_response(payload: &[u8]) -> Result<(String, usize), ProtocolError> {
    decode_string(payload)
}

/// Ping Request（serverbound `0x01`，i64 big-endian）打包成完整线包。
pub fn encode_ping_request_packet(payload: i64, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_i64_be(payload, &mut body);
    encode_packet(PING_PACKET_ID, &body, out);
}

/// Ping Request 解码，要求 payload 恰好 8 字节。
pub fn decode_ping_request(payload: &[u8]) -> Result<i64, ProtocolError> {
    if payload.len() != 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let (v, _) = read_i64_be(payload)?;
    Ok(v)
}

/// Pong（clientbound `0x01`，i64 big-endian，原样回显 Ping 的 payload）打包成完整线包。
pub fn encode_pong_packet(payload: i64, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_i64_be(payload, &mut body);
    encode_packet(PONG_PACKET_ID, &body, out);
}

/// Pong 解码，要求 payload 恰好 8 字节。
pub fn decode_pong_payload(payload: &[u8]) -> Result<i64, ProtocolError> {
    if payload.len() != 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let (v, _) = read_i64_be(payload)?;
    Ok(v)
}

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

// ---------------------------------------------------------------------------
// Play 阶段最小包脚手架（任务F：能进服不掉线，只求能 encode）
// 对标 wiki.vg protocol 777 Play 阶段。包 ID 均为按 wiki.vg 推测值，
// 待26.3抓包核对（若抓包 ID 不同，只改下面常量即可，encode 逻辑不变）。
// 当前为简化版：只编码客户端进服必需字段，死亡位置/传送门冷却等
// 777 新增字段 P3 补齐。不改 Handshake/Status 逻辑。
// ---------------------------------------------------------------------------

/// Login Play（Join Game）包 ID（Play clientbound `0x2B` 左右）。待26.3抓包核对。
pub const PLAY_LOGIN_PLAY_ID: i32 = 0x2B;
/// Set Center Chunk 包 ID（Play clientbound）。待26.3抓包核对。
pub const PLAY_SET_CENTER_CHUNK_ID: i32 = 0x4C;
/// Chunk Data and Update Light 包 ID（Play clientbound）。待26.3抓包核对。
pub const PLAY_CHUNK_DATA_ID: i32 = 0x27;
/// Synchronize Player Position（Player Position Sync）包 ID（Play clientbound）。待26.3抓包核对。
pub const PLAY_PLAYER_POSITION_SYNC_ID: i32 = 0x3E;
/// KeepAlive S2C 包 ID（Play clientbound `0x26` 左右）。待26.3抓包核对。
pub const PLAY_KEEPALIVE_S2C_ID: i32 = 0x26;
/// KeepAlive C2S 包 ID（Play serverbound `0x1A` 左右）。待26.3抓包核对。
pub const PLAY_KEEPALIVE_C2S_ID: i32 = 0x1A;

/// Login Play（Join Game）简化版：覆盖进服必需字段。
/// wiki.vg 完整字段含死亡位置/传送门冷却/海平面/安全聊天等，P3 补齐。
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCenterChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
}

impl SetCenterChunk {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.chunk_x, out);
        write_varint(self.chunk_z, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (chunk_x, n1) = read_varint(buf)?;
        let (chunk_z, n2) = read_varint(&buf[n1..])?;
        Ok((Self { chunk_x, chunk_z }, n1 + n2))
    }
}

/// Set Center Chunk 打成完整线包。
pub fn encode_set_center_chunk_packet(v: &SetCenterChunk, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_SET_CENTER_CHUNK_ID, &body, out);
}

/// 最小空 NBT（空 Compound）：`0A 00 00 00`
/// = TAG_Compound(0x0A) + name_len u16be(0) + TAG_End(0x00)。
/// 用于空 chunk 高度图占位，P3 填真实高度图/光照。
pub fn minimal_empty_nbt_bytes() -> Vec<u8> {
    vec![0x0A, 0x00, 0x00, 0x00]
}

/// 单节空 section 占位字节（先返回空 paletted section 字节，P3 填真实高度图/光照）。
/// 布局（简化版，待26.3抓包核对）：block_count i16be(0) + block单值调色板(3B)
/// + biome单值调色板(3B) = 8B/节。
pub fn encode_empty_section_placeholder(out: &mut Vec<u8>) {
    // block_count = 0 (非空方块数)
    out.extend_from_slice(&[0x00, 0x00]);
    // block states: bits_per_entry=0(单值) + palette air(0) + data_len(0)
    out.extend_from_slice(&[0x00, 0x00, 0x00]);
    // biomes: bits_per_entry=0(单值) + palette plains(0) + data_len(0)
    out.extend_from_slice(&[0x00, 0x00, 0x00]);
}

/// Chunk Data and Update Light（空 chunk 简化版）。
/// 真实协议含高度图 NBT(无长度前缀) + section 数据 + block entity 数组
/// + 天空/方块光照掩码；此处高度图/section 均用 VarInt 长度前缀包裹以便
/// 单测自 delimit，P3 按抓包改成真实布局。待26.3抓包核对。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayChunkData {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub heightmaps_nbt: Vec<u8>,
    pub section_bytes: Vec<u8>,
}

impl PlayChunkData {
    /// 空 chunk 构造：最小 NBT + 24 节空 section。
    pub fn empty(chunk_x: i32, chunk_z: i32) -> Self {
        let mut section_bytes = Vec::new();
        for _ in 0..24 {
            encode_empty_section_placeholder(&mut section_bytes);
        }
        Self {
            chunk_x,
            chunk_z,
            heightmaps_nbt: minimal_empty_nbt_bytes(),
            section_bytes,
        }
    }

    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_i32_be(self.chunk_x, out);
        write_i32_be(self.chunk_z, out);
        write_varint(self.heightmaps_nbt.len() as i32, out);
        out.extend_from_slice(&self.heightmaps_nbt);
        write_varint(self.section_bytes.len() as i32, out);
        out.extend_from_slice(&self.section_bytes);
        // 空 chunk：block entity 数量 0
        write_varint(0, out);
        // NOTE: 天空/方块光照掩码 P3 补齐，待26.3抓包核对。
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (chunk_x, n) = read_i32_be(&buf[off..])?;
        off += n;
        let (chunk_z, n) = read_i32_be(&buf[off..])?;
        off += n;
        let (nbt_len, n) = read_varint(&buf[off..])?;
        off += n;
        let nbt_len = nbt_len.max(0) as usize;
        if buf.len() < off + nbt_len {
            return Err(ProtocolError::UnexpectedEof);
        }
        let heightmaps_nbt = buf[off..off + nbt_len].to_vec();
        off += nbt_len;
        let (sec_len, n) = read_varint(&buf[off..])?;
        off += n;
        let sec_len = sec_len.max(0) as usize;
        if buf.len() < off + sec_len {
            return Err(ProtocolError::UnexpectedEof);
        }
        let section_bytes = buf[off..off + sec_len].to_vec();
        off += sec_len;
        let (_, n) = read_varint(&buf[off..])?; // block entity count，空 chunk 期望 0
        off += n;
        Ok((Self { chunk_x, chunk_z, heightmaps_nbt, section_bytes }, off))
    }
}

/// Chunk Data 打成完整线包。
pub fn encode_play_chunk_data_packet(v: &PlayChunkData, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_CHUNK_DATA_ID, &body, out);
}

/// Synchronize Player Position（Player Position Sync）。
/// wiki.vg: Double x/y/z + Float yaw/pitch + Byte flags + VarInt teleport_id。
#[derive(Debug, Clone, Copy, PartialEq)]
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

/// KeepAlive S2C 打成完整线包（包 ID 待26.3抓包核对）。
pub fn encode_keepalive_s2c_packet(keepalive_id: i64, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    KeepAliveS2C(keepalive_id).encode_body(&mut body);
    encode_packet(PLAY_KEEPALIVE_S2C_ID, &body, out);
}

/// KeepAlive C2S 解码（包 ID 待26.3抓包核对），要求 payload 恰好 8 字节。
pub fn decode_keepalive_c2s(payload: &[u8]) -> Result<i64, ProtocolError> {
    if payload.len() != 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let (v, _) = read_i64_be(payload)?;
    Ok(v)
}

/// 26.3 Status Response JSON (protocol 777 固定)
pub fn status_json(max_players: usize, online: usize, motd: &str) -> String {
    format!(
        r#"{{"version":{{"name":"{}","protocol":{}}},"players":{{"max":{},"online":{}}},"description":{{"text":"{}"}}}}"#,
        mc_core::MC_VERSION,
        mc_core::PROTOCOL_VERSION,
        max_players,
        online,
        motd
    )
}

// ---------------------------------------------------------------------------
// Login 阶段（离线模式，任务D）：只兼容 777，不做正版验证/加密。
// 对标 wiki.vg protocol 777 Login 阶段：
// - serverbound `0x00` Login Start: `String name` + `UUID`
// - clientbound `0x02` Login Success: `UUID` + `String username`
// - serverbound `0x03` Login Acknowledged: 空包（客户端确认后进 Configuration）
// 压缩/加密：本任务不启用。Set Compression（clientbound `0x03`）只提供编解码
// 函数占位，不发送（阈值 -1 = 禁用）；Encryption Request/Response 留注释，
// P3 做正版验证时再实现。不改 Handshake/Status/Play 逻辑。
// ---------------------------------------------------------------------------

/// Login Start 包 ID（Login serverbound `0x00`）。
pub const LOGIN_START_PACKET_ID: i32 = 0x00;
/// Login Success 包 ID（Login clientbound `0x02`）。
pub const LOGIN_SUCCESS_PACKET_ID: i32 = 0x02;
/// Login Acknowledged 包 ID（Login serverbound `0x03`，空包）。
pub const LOGIN_ACKNOWLEDGED_PACKET_ID: i32 = 0x03;
/// Set Compression 包 ID（Login clientbound `0x03`，与 Ack 同值但方向相反）。
pub const SET_COMPRESSION_PACKET_ID: i32 = 0x03;
/// Encryption Request 包 ID（Login clientbound `0x01`，P3 实现正版验证时再用）。
pub const ENCRYPTION_REQUEST_PACKET_ID: i32 = 0x01;

/// 压缩禁用阈值占位：`-1` 表示不启用压缩、不发送 Set Compression；
/// P3 启用压缩时改为实际阈值（`>= 0`）并在 Login Success 之前发送。
pub const COMPRESSION_DISABLED_THRESHOLD: i32 = -1;

/// 离线模式 UUID：随机 v4（不做正版验证，不使用 `OfflinePlayer:<name>` 固定算法）。
/// P3 如需与单机逻辑一致，可换成 `Uuid::new_v5(NAMESPACE_DNS, name)`/v3
///（需要给 uuid 开 `v5`/`v3` feature）。
pub fn offline_uuid(_name: &str) -> Uuid {
    Uuid::new_v4()
}

/// UUID 按线包格式读写：16 字节 big-endian（`Uuid::as_bytes` 即该布局）。
pub fn write_uuid(v: Uuid, out: &mut Vec<u8>) {
    out.extend_from_slice(v.as_bytes());
}

/// 读 16 字节 UUID，不足 16 字节报 `UnexpectedEof`。
pub fn read_uuid(buf: &[u8]) -> Result<(Uuid, usize), ProtocolError> {
    if buf.len() < 16 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(&buf[..16]);
    Ok((Uuid::from_bytes(b), 16))
}

/// Login Start（serverbound `0x00`）：`String name` + `UUID`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    pub name: String,
    pub uuid: Uuid,
}

impl LoginStart {
    /// 编码包体（不含 length + packet id）。
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.name, out);
        write_uuid(self.uuid, out);
    }

    /// 解码包体，返回 `(包, 消费字节数)`。尾部多余字节忽略。
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (name, n1) = decode_string(buf)?;
        let (uuid, n2) = read_uuid(&buf[n1..])?;
        Ok((Self { name, uuid }, n1 + n2))
    }
}

/// Login Start 打成完整线包（服务端主要做 decode，encode 供单测/模拟客户端用）。
pub fn encode_login_start_packet(v: &LoginStart, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(LOGIN_START_PACKET_ID, &body, out);
}

/// Login Start 解码（去掉 packet id 后的 payload）。
pub fn decode_login_start(payload: &[u8]) -> Result<(LoginStart, usize), ProtocolError> {
    LoginStart::decode_body(payload)
}

/// Login Success（clientbound `0x02`）：`UUID` + `String username`。
/// NOTE: 777 原版尾部还有 `VarInt 属性数 + 属性表`；离线占位只发 UUID+name，
/// P3 按 26.3 抓包补齐属性表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess {
    pub uuid: Uuid,
    pub username: String,
}

impl LoginSuccess {
    /// 编码包体（不含 length + packet id）。
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_uuid(self.uuid, out);
        encode_string(&self.username, out);
    }

    /// 解码包体，返回 `(包, 消费字节数)`。尾部多余字节（P3 的属性表）忽略。
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (uuid, n1) = read_uuid(buf)?;
        let (username, n2) = decode_string(&buf[n1..])?;
        Ok((Self { uuid, username }, n1 + n2))
    }
}

/// Login Success 打成完整线包。
pub fn encode_login_success_packet(v: &LoginSuccess, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(LOGIN_SUCCESS_PACKET_ID, &body, out);
}

/// Login Success 解码（去掉 packet id 后的 payload）。
pub fn decode_login_success(payload: &[u8]) -> Result<(LoginSuccess, usize), ProtocolError> {
    LoginSuccess::decode_body(payload)
}

/// Login Acknowledged（serverbound `0x03`）打成完整空包（供单测/模拟客户端用）。
pub fn encode_login_acknowledged_packet(out: &mut Vec<u8>) {
    encode_packet(LOGIN_ACKNOWLEDGED_PACKET_ID, &[], out);
}

/// Login Acknowledged 解码：按 wiki 无字段，尾部字节直接忽略（兼容性优先，
/// 与 [`decode_status_request`] 同策略）。
pub fn decode_login_acknowledged(_payload: &[u8]) -> Result<(), ProtocolError> {
    Ok(())
}

/// Set Compression（Login clientbound `0x03`，`VarInt threshold`）打成完整线包。
/// 占位函数：当前离线流程阈值固定 [`COMPRESSION_DISABLED_THRESHOLD`]（`-1`），
/// 不发送；P3 启用压缩时在 Login Success 之前发送阈值 `>= 0` 的包。
pub fn encode_set_compression_packet(threshold: i32, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_varint(threshold, &mut body);
    encode_packet(SET_COMPRESSION_PACKET_ID, &body, out);
}

/// Set Compression 解码，返回阈值。
pub fn decode_set_compression(payload: &[u8]) -> Result<i32, ProtocolError> {
    let (threshold, _) = read_varint(payload)?;
    Ok(threshold)
}

// P3 预留：Encryption Request（Login clientbound `0x01`）=
// `String server_id` + `VarInt len + public_key` + `VarInt len + verify_token`。
// 离线模式跳过整个加密握手：不发送 Request，不处理 serverbound `0x01`
// Encryption Response；做正版验证时在此处插入 Mojang 会话校验+共享密钥协商。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_roundtrip() {
        let cases = [
            Handshake {
                protocol_version: 777,
                server_address: "localhost".to_string(),
                server_port: 25565,
                next_state: HandshakeNextState::Status,
            },
            Handshake {
                protocol_version: 777,
                server_address: "127.0.0.1".to_string(),
                server_port: 19132,
                next_state: HandshakeNextState::Login,
            },
            // 空地址 + unicode 地址（按字节计长）
            Handshake {
                protocol_version: 777,
                server_address: "mc.例子.test".to_string(),
                server_port: 0,
                next_state: HandshakeNextState::Status,
            },
        ];
        for h in cases {
            // body 层 roundtrip
            let mut body = Vec::new();
            h.encode_body(&mut body);
            let (got, used) = Handshake::decode_body(&body).unwrap();
            assert_eq!(got, h);
            assert_eq!(used, body.len());

            // 组帧后 roundtrip：length + id + body
            let mut framed = Vec::new();
            encode_packet(HANDSHAKE_PACKET_ID, &body, &mut framed);
            let (len, n) = read_varint(&framed).unwrap();
            assert_eq!(len as usize, framed.len() - n);
            let (id, m) = decode_packet_id(&framed[n..]).unwrap();
            assert_eq!(id, HANDSHAKE_PACKET_ID);
            let (got2, used2) = Handshake::decode_body(&framed[n + m..]).unwrap();
            assert_eq!(got2, h);
            assert_eq!(n + m + used2, framed.len());
        }
    }

    #[test]
    fn handshake_rejects_bad_next_state() {
        let mut body = Vec::new();
        write_varint(777, &mut body);
        encode_string("localhost", &mut body);
        write_u16_be(25565, &mut body);
        write_varint(3, &mut body); // 777 下只有 1/2 合法
        match Handshake::decode_body(&body) {
            Err(ProtocolError::InvalidNextState(3)) => {}
            other => panic!("expected InvalidNextState(3), got {other:?}"),
        }
    }

    #[test]
    fn handshake_rejects_truncated() {
        let h = Handshake {
            protocol_version: 777,
            server_address: "localhost".to_string(),
            server_port: 25565,
            next_state: HandshakeNextState::Status,
        };
        let mut body = Vec::new();
        h.encode_body(&mut body);
        for cut in [1, body.len() - 1] {
            assert!(
                Handshake::decode_body(&body[..cut]).is_err(),
                "truncated to {cut} bytes should fail"
            );
        }
    }

    #[test]
    fn status_json_has_version() {
        let j = status_json(100, 0, "test motd");
        assert!(j.contains("26.3"), "missing 26.3 in {j}");
        assert!(j.contains("777"), "missing 777 in {j}");
        assert!(j.contains("test motd"), "missing motd in {j}");
    }

    #[test]
    fn status_response_roundtrip() {
        let json = status_json(100, 3, "hello");
        let mut framed = Vec::new();
        encode_status_response_packet(&json, &mut framed);
        let (len, n) = read_varint(&framed).unwrap();
        assert_eq!(len as usize, framed.len() - n);
        let (id, m) = decode_packet_id(&framed[n..]).unwrap();
        assert_eq!(id, STATUS_RESPONSE_PACKET_ID);
        let (back, used) = decode_status_response(&framed[n + m..]).unwrap();
        assert_eq!(back, json);
        assert_eq!(n + m + used, framed.len());

        // request 是空包
        let mut req = Vec::new();
        encode_status_request_packet(&mut req);
        let (_, n) = read_varint(&req).unwrap();
        let (id, m) = decode_packet_id(&req[n..]).unwrap();
        assert_eq!(id, STATUS_REQUEST_PACKET_ID);
        decode_status_request(&req[n + m..]).unwrap();
    }

    #[test]
    fn ping_pong_roundtrip() {
        for v in [0i64, 1, -1, 123_456_789, i64::MIN, i64::MAX] {
            let mut ping = Vec::new();
            encode_ping_request_packet(v, &mut ping);
            let (_, n) = read_varint(&ping).unwrap();
            let (id, m) = decode_packet_id(&ping[n..]).unwrap();
            assert_eq!(id, PING_PACKET_ID);
            assert_eq!(decode_ping_request(&ping[n + m..]).unwrap(), v);

            let mut pong = Vec::new();
            encode_pong_packet(v, &mut pong);
            let (_, n) = read_varint(&pong).unwrap();
            let (id, m) = decode_packet_id(&pong[n..]).unwrap();
            assert_eq!(id, PONG_PACKET_ID);
            assert_eq!(decode_pong_payload(&pong[n + m..]).unwrap(), v);
        }
    }

    #[test]
    fn play_login_play_roundtrip() {
        let v = LoginPlay::default();
        let mut body = Vec::new();
        v.encode_body(&mut body);
        assert!(!body.is_empty(), "login play body must not be empty");
        let (back, used) = LoginPlay::decode_body(&body).unwrap();
        assert_eq!(back, v);
        assert_eq!(used, body.len());

        let mut framed = Vec::new();
        encode_login_play_packet(&v, &mut framed);
        let (len, n) = read_varint(&framed).unwrap();
        assert_eq!(len as usize, framed.len() - n);
        let (id, m) = decode_packet_id(&framed[n..]).unwrap();
        assert_eq!(id, PLAY_LOGIN_PLAY_ID);
        let (back2, used2) = LoginPlay::decode_body(&framed[n + m..]).unwrap();
        assert_eq!(back2, v);
        assert_eq!(n + m + used2, framed.len());
    }

    #[test]
    fn play_set_center_chunk_roundtrip() {
        for (x, z) in [(0, 0), (8, -3), (-100, 200), (i32::MAX, i32::MIN)] {
            let v = SetCenterChunk { chunk_x: x, chunk_z: z };
            let mut body = Vec::new();
            v.encode_body(&mut body);
            let (back, used) = SetCenterChunk::decode_body(&body).unwrap();
            assert_eq!(back, v);
            assert_eq!(used, body.len());

            let mut framed = Vec::new();
            encode_set_center_chunk_packet(&v, &mut framed);
            let (len, n) = read_varint(&framed).unwrap();
            assert_eq!(len as usize, framed.len() - n);
            let (id, m) = decode_packet_id(&framed[n..]).unwrap();
            assert_eq!(id, PLAY_SET_CENTER_CHUNK_ID);
            assert_eq!(framed.len(), n + m + body.len());
        }
    }

    #[test]
    fn play_chunk_data_empty_roundtrip() {
        // 最小 NBT 形状断言
        assert_eq!(minimal_empty_nbt_bytes(), vec![0x0A, 0x00, 0x00, 0x00]);
        let mut sec = Vec::new();
        encode_empty_section_placeholder(&mut sec);
        assert_eq!(sec.len(), 8, "empty section placeholder must be 8 bytes");

        let v = PlayChunkData::empty(0, 0);
        assert_eq!(v.section_bytes.len(), 24 * 8);
        let mut body = Vec::new();
        v.encode_body(&mut body);
        assert!(!body.is_empty());
        let (back, used) = PlayChunkData::decode_body(&body).unwrap();
        assert_eq!(back, v);
        assert_eq!(used, body.len());

        let mut framed = Vec::new();
        encode_play_chunk_data_packet(&v, &mut framed);
        let (len, n) = read_varint(&framed).unwrap();
        assert_eq!(len as usize, framed.len() - n);
        let (id, m) = decode_packet_id(&framed[n..]).unwrap();
        assert_eq!(id, PLAY_CHUNK_DATA_ID);
    }

    #[test]
    fn play_player_position_sync_roundtrip() {
        let v = PlayerPositionSync {
            x: 8.5,
            y: 80.0,
            z: 8.5,
            yaw: 0.0,
            pitch: 0.0,
            flags: 0,
            teleport_id: 1,
        };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        // 8*3 + 4*2 + 1 + varint(1) = 34
        assert_eq!(body.len(), 34);
        let (back, used) = PlayerPositionSync::decode_body(&body).unwrap();
        assert_eq!(back, v);
        assert_eq!(used, body.len());

        let mut framed = Vec::new();
        encode_player_position_sync_packet(&v, &mut framed);
        let (len, n) = read_varint(&framed).unwrap();
        assert_eq!(len as usize, framed.len() - n);
        let (id, m) = decode_packet_id(&framed[n..]).unwrap();
        assert_eq!(id, PLAY_PLAYER_POSITION_SYNC_ID);
    }

    #[test]
    fn play_keepalive_roundtrip() {
        for v in [0i64, 1, -1, 12345, i64::MAX, i64::MIN] {
            let mut body = Vec::new();
            KeepAliveS2C(v).encode_body(&mut body);
            assert_eq!(body.len(), 8);
            let (back, used) = KeepAliveS2C::decode_body(&body).unwrap();
            assert_eq!(back.0, v);
            assert_eq!(used, 8);

            let mut framed = Vec::new();
            encode_keepalive_s2c_packet(v, &mut framed);
            let (len, n) = read_varint(&framed).unwrap();
            assert_eq!(len as usize, framed.len() - n);
            let (id, m) = decode_packet_id(&framed[n..]).unwrap();
            assert_eq!(id, PLAY_KEEPALIVE_S2C_ID);
            assert_eq!(decode_keepalive_c2s(&framed[n + m..]).unwrap(), v);
        }
    }

    #[test]
    fn login_start_roundtrip() {
        let cases = [
            ("Steve", Uuid::from_bytes([1u8; 16])),
            ("Notch", Uuid::nil()),
            ("玩家123456789012", Uuid::max()),
        ];
        for (name, uuid) in cases {
            let v = LoginStart { name: name.to_string(), uuid };
            // body 层 roundtrip
            let mut body = Vec::new();
            v.encode_body(&mut body);
            let (back, used) = LoginStart::decode_body(&body).unwrap();
            assert_eq!(back, v);
            assert_eq!(used, body.len());
            // free 函数解码一致
            let (back2, used2) = decode_login_start(&body).unwrap();
            assert_eq!(back2, v);
            assert_eq!(used2, body.len());

            // 组帧后 roundtrip：length + id + body
            let mut framed = Vec::new();
            encode_login_start_packet(&v, &mut framed);
            let (len, n) = read_varint(&framed).unwrap();
            assert_eq!(len as usize, framed.len() - n);
            let (id, m) = decode_packet_id(&framed[n..]).unwrap();
            assert_eq!(id, LOGIN_START_PACKET_ID);
            let (back3, used3) = LoginStart::decode_body(&framed[n + m..]).unwrap();
            assert_eq!(back3, v);
            assert_eq!(n + m + used3, framed.len());
        }
    }

    #[test]
    fn login_start_rejects_truncated() {
        let v = LoginStart { name: "Steve".to_string(), uuid: Uuid::from_bytes([7u8; 16]) };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        // 名字 VarInt 不全 / UUID 少于 16 字节都应失败
        for cut in [1, 2, 6, 7, body.len() - 1] {
            assert!(
                LoginStart::decode_body(&body[..cut]).is_err(),
                "truncated to {cut} bytes should fail"
            );
        }
    }

    #[test]
    fn login_success_roundtrip() {
        for (username, uuid) in [
            ("Steve".to_string(), Uuid::from_bytes([2u8; 16])),
            ("a".to_string(), Uuid::nil()),
            ("十六字符名字xxxxab".to_string(), Uuid::max()),
        ] {
            let v = LoginSuccess { uuid, username };
            // body 层 roundtrip
            let mut body = Vec::new();
            v.encode_body(&mut body);
            let (back, used) = LoginSuccess::decode_body(&body).unwrap();
            assert_eq!(back, v);
            assert_eq!(used, body.len());

            // 组帧后：encode 可 decode
            let mut framed = Vec::new();
            encode_login_success_packet(&v, &mut framed);
            let (len, n) = read_varint(&framed).unwrap();
            assert_eq!(len as usize, framed.len() - n);
            let (id, m) = decode_packet_id(&framed[n..]).unwrap();
            assert_eq!(id, LOGIN_SUCCESS_PACKET_ID);
            let (back2, used2) = decode_login_success(&framed[n + m..]).unwrap();
            assert_eq!(back2, v);
            assert_eq!(n + m + used2, framed.len());
        }
    }

    #[test]
    fn login_acknowledged_is_empty_packet() {
        let mut framed = Vec::new();
        encode_login_acknowledged_packet(&mut framed);
        let (len, n) = read_varint(&framed).unwrap();
        assert_eq!(len as usize, framed.len() - n);
        let (id, m) = decode_packet_id(&framed[n..]).unwrap();
        assert_eq!(id, LOGIN_ACKNOWLEDGED_PACKET_ID);
        assert!(framed[n + m..].is_empty(), "ack payload must be empty");
        decode_login_acknowledged(&framed[n + m..]).unwrap();
    }

    #[test]
    fn set_compression_roundtrip_and_disabled_default() {
        // 禁用占位：阈值 -1
        assert_eq!(COMPRESSION_DISABLED_THRESHOLD, -1);
        for threshold in [COMPRESSION_DISABLED_THRESHOLD, 0, 256, 1_048_576] {
            let mut framed = Vec::new();
            encode_set_compression_packet(threshold, &mut framed);
            let (len, n) = read_varint(&framed).unwrap();
            assert_eq!(len as usize, framed.len() - n);
            let (id, m) = decode_packet_id(&framed[n..]).unwrap();
            assert_eq!(id, SET_COMPRESSION_PACKET_ID);
            assert_eq!(decode_set_compression(&framed[n + m..]).unwrap(), threshold);
        }
    }

    #[test]
    fn offline_uuid_is_random_v4() {
        // 离线 UUID 走 v4 随机：两次不同版本位均为 4，且与名字无关（各调一次即可）。
        let a = offline_uuid("Steve");
        let b = offline_uuid("Steve");
        assert_ne!(a, b, "offline uuid must be random per login");
        assert_eq!(a.get_version_num(), 4);
        assert_eq!(b.get_version_num(), 4);
    }
}
