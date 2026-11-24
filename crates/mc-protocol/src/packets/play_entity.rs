//! play_entity: 物品掉落实体 + 背包同步包，只兼容 26.3/777。
//!
//! ID 来源：minecraft.wiki Play 包表（777，实测见 `docs/packet-map-777.md` §7）：
//! - S2C `add_entity` 0x01：SpawnEntity（掉落物用，type=`minecraft:item`=72）
//! - S2C `set_entity_data` 0x65：SetEntityData（物品栈元数据 index 8 type Slot(7)）
//! - S2C `remove_entities` 0x4E：RemoveEntities（despawn/拾取）
//! - S2C `entity_position_sync` 0x23：TeleportEntity 线性路径（掉落/下落同步用绝对坐标）
//! - S2C `move_entity_pos` 0x36：UpdateEntityPosition 线性差量（实现+单测，运行时优先用 0x23）
//! - S2C `take_item_entity` 0x7F：PickupItem 拾取动画（collector=LoginPlay entity_id=1）
//! - S2C `container_set_content` 0x12 / `container_set_slot` 0x14：背包同步（window 0）
//! - C2S `set_carried_item` 0x36：SetHeldItem（**Short** 0..=8，与 S2C 0x6B VarInt 不同）
//! - C2S `container_click` 0x12：ClickContainer（Hashed Slot 复杂，本轮只忽略不断线，见 §7）
//!
//! P18 范围：掉落看得见（Spawn+Metadata）、下落同步（0x23 Teleport）、拾取动画（0x7F
//! 可选但实现）、背包同步（Content/Slot）。窗口整理（ClickContainer 解析）留后续。

use crate::packets::codec::{
    encode_packet, read_bool, read_f32_be, read_f64_be, write_bool, write_f32_be, write_f64_be,
};
use crate::{read_varint, write_varint, ProtocolError};

/// S2C Spawn Entity（掉落物）：`add_entity`。
pub const PLAY_SPAWN_ENTITY_S2C_ID: i32 = 0x01;
/// S2C Set Entity Metadata：`set_entity_data`。
pub const PLAY_SET_ENTITY_DATA_S2C_ID: i32 = 0x65;
/// S2C Remove Entities：`remove_entities`。
pub const PLAY_REMOVE_ENTITIES_S2C_ID: i32 = 0x4E;
/// S2C Teleport Entity：`entity_position_sync`（线性路径，绝对坐标同步）。
pub const PLAY_TELEPORT_ENTITY_S2C_ID: i32 = 0x23;
/// S2C Update Entity Position：`move_entity_pos`（线性差量 step=0）。
pub const PLAY_UPDATE_ENTITY_POS_S2C_ID: i32 = 0x36;
/// S2C Pickup Item：`take_item_entity`（拾取动画，不删实体不加背包，需另发 Remove+背包）。
pub const PLAY_PICKUP_ITEM_S2C_ID: i32 = 0x7F;
/// S2C Set Container Content：`container_set_content`（window 0=玩家背包）。
pub const PLAY_SET_CONTAINER_CONTENT_S2C_ID: i32 = 0x12;
/// S2C Set Container Slot：`container_set_slot`（window 0=玩家背包）。
pub const PLAY_SET_CONTAINER_SLOT_S2C_ID: i32 = 0x14;
/// C2S Set Held Item / Click Container 的 ID 见 `play_block`
///（`PLAY_SET_HELD_ITEM_C2S_ID` 0x36 / `PLAY_CLICK_CONTAINER_C2S_ID` 0x12）：
/// C2S 0x36 是 Short（本文件 `decode_set_held_item_c2s`），注意 S2C 同名包是 VarInt。

/// `minecraft:item` 在 EntityType 注册表中的位置（26.3 客户端 jar 实测，见 HANDOFF）。
pub const ITEM_ENTITY_TYPE_ID: i32 = 72;

/// 玩家背包窗口 ID（window 0）。
pub const PLAYER_INVENTORY_WINDOW_ID: i32 = 0;
/// 玩家背包槽位数（container_set_content 的 Slot 数组长度，见 Inventory 窗口表）。
pub const PLAYER_INVENTORY_SLOT_COUNT: usize = 46;

/// Slot 空（count=0，后无字段）。
pub const SLOT_EMPTY_COUNT: i32 = 0;

fn read_short_be(buf: &[u8]) -> Result<(i16, usize), ProtocolError> {
    if buf.len() < 2 {
        return Err(ProtocolError::UnexpectedEof);
    }
    Ok((i16::from_be_bytes([buf[0], buf[1]]), 2))
}

fn write_short_be(v: i16, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn read_uuid(buf: &[u8]) -> Result<(u128, usize), ProtocolError> {
    if buf.len() < 16 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(&buf[..16]);
    Ok((u128::from_be_bytes(b), 16))
}

fn write_uuid(v: u128, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_be_bytes());
}

// ---------------------------------------------------------------------------
// Slot（背包/实体元数据）：只支持空与无 component 的普通物品。
// 线格式：count VarInt；count=0 时结束；否则 item_id VarInt + add数 VarInt +
// remove数 VarInt（普通物品均为 0）。带 component 的物品解码时报错，避免错位整条流。
// ---------------------------------------------------------------------------

/// 普通物品栈（无 data component）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub count: i32,
    pub item_id: i32,
}

impl Slot {
    pub fn empty() -> Self {
        Self { count: 0, item_id: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.count <= 0
    }

    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.count, out);
        if self.count <= 0 {
            return;
        }
        write_varint(self.item_id, out);
        write_varint(0, out);
        write_varint(0, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (count, n) = read_varint(&buf[off..])?;
        off += n;
        if count <= 0 {
            return Ok((Self::empty(), off));
        }
        let (item_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (add, n) = read_varint(&buf[off..])?;
        off += n;
        let (remove, n) = read_varint(&buf[off..])?;
        off += n;
        if add != 0 || remove != 0 {
            return Err(ProtocolError::UnexpectedEof);
        }
        Ok((Self { count, item_id }, off))
    }
}

// ---------------------------------------------------------------------------
// LpVec3（Spawn/速度用，见 Data types#LpVec3）：0 向量编 1 字节 0x00。
// ---------------------------------------------------------------------------

/// 低精度速度向量（与 wiki 伪代码同构，大小端按“前 2 字节小端、后 4 字节大端”）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LpVec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl LpVec3 {
    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0, z: 0.0 }
    }

    const MAX_Q: f64 = 32766.0;

    fn pack(v: f64) -> u64 {
        ((v * 0.5 + 0.5) * Self::MAX_Q).round() as u64
    }

    fn unpack(v: u64) -> f64 {
        ((v & 32767) as f64).min(Self::MAX_Q) * 2.0 / Self::MAX_Q - 1.0
    }

    pub fn encode_body(&self, out: &mut Vec<u8>) {
        let m = self.x.abs().max(self.y.abs()).max(self.z.abs());
        if !m.is_finite() || m < 1.0 / Self::MAX_Q {
            out.push(0);
            return;
        }
        let scale = m.ceil() as u64;
        let need_cont = (scale & 0x3) != scale;
        let packed_scale = if need_cont { (scale & 0x3) | 0x04 } else { scale };
        let px = Self::pack(self.x / scale as f64) << 3;
        let py = Self::pack(self.y / scale as f64) << 18;
        let pz = Self::pack(self.z / scale as f64) << 33;
        let packed = pz | py | px | packed_scale;
        out.push(packed as u8);
        out.push((packed >> 8) as u8);
        out.extend_from_slice(&((packed >> 16) as u32).to_be_bytes());
        if need_cont {
            write_varint((scale >> 2) as i32, out);
        }
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        if buf.is_empty() {
            return Err(ProtocolError::UnexpectedEof);
        }
        if buf[0] == 0 {
            return Ok((Self::zero(), 1));
        }
        if buf.len() < 6 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let b1 = buf[0] as u64;
        let b2 = buf[1] as u64;
        let mut w = [0u8; 4];
        w.copy_from_slice(&buf[2..6]);
        let hi = u32::from_be_bytes(w) as u64;
        let packed = (hi << 16) | (b2 << 8) | b1;
        let mut scale = b1 & 0x03;
        let mut off = 6;
        if (b1 & 0x04) != 0 {
            let (extra, n) = read_varint(&buf[off..])?;
            off += n;
            scale |= (extra as u64) << 2;
        }
        let s = scale as f64;
        Ok((
            Self {
                x: Self::unpack(packed >> 3) * s,
                y: Self::unpack(packed >> 18) * s,
                z: Self::unpack(packed >> 33) * s,
            },
            off,
        ))
    }
}

// ---------------------------------------------------------------------------
// Spawn Entity（掉落物）：entity_id + uuid + type(72) + xyz + LpVec3 + 角度 + data(0)。
// ---------------------------------------------------------------------------

/// 掉落物生成（S2C 0x01，`minecraft:item`，data=0 无额外 Object data）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnItemEntity {
    pub entity_id: i32,
    pub uuid: u128,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub vel: LpVec3,
    pub pitch: u8,
    pub yaw: u8,
    pub head_yaw: u8,
}

impl SpawnItemEntity {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.entity_id, out);
        write_uuid(self.uuid, out);
        write_varint(ITEM_ENTITY_TYPE_ID, out);
        write_f64_be(self.x, out);
        write_f64_be(self.y, out);
        write_f64_be(self.z, out);
        self.vel.encode_body(out);
        out.push(self.pitch);
        out.push(self.yaw);
        out.push(self.head_yaw);
        write_varint(0, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (entity_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (uuid, n) = read_uuid(&buf[off..])?;
        off += n;
        let (typ, n) = read_varint(&buf[off..])?;
        off += n;
        if typ != ITEM_ENTITY_TYPE_ID {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (x, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (y, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (z, n) = read_f64_be(&buf[off..])?;
        off += n;
        let (vel, n) = LpVec3::decode_body(&buf[off..])?;
        off += n;
        if buf.len() < off + 3 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (pitch, yaw, head_yaw) = (buf[off], buf[off + 1], buf[off + 2]);
        off += 3;
        let (data, n) = read_varint(&buf[off..])?;
        off += n;
        if data != 0 {
            return Err(ProtocolError::UnexpectedEof);
        }
        Ok((Self { entity_id, uuid, x, y, z, vel, pitch, yaw, head_yaw }, off))
    }
}

pub fn encode_spawn_item_packet(v: &SpawnItemEntity, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_SPAWN_ENTITY_S2C_ID, &body, out);
}

// ---------------------------------------------------------------------------
// Set Entity Data：entity_id + [index 8 u8, type 7 varint, Slot] + 0xFF。
// ---------------------------------------------------------------------------

/// 掉落物物品栈元数据（S2C 0x65，index 8 / type Slot(7)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemEntityData {
    pub entity_id: i32,
    pub slot: Slot,
}

impl ItemEntityData {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.entity_id, out);
        out.push(8);
        write_varint(7, out);
        self.slot.encode_body(out);
        out.push(0xFF);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (entity_id, n) = read_varint(&buf[off..])?;
        off += n;
        if buf.len() < off + 1 || buf[off] != 8 {
            return Err(ProtocolError::UnexpectedEof);
        }
        off += 1;
        let (typ, n) = read_varint(&buf[off..])?;
        off += n;
        if typ != 7 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (slot, n) = Slot::decode_body(&buf[off..])?;
        off += n;
        if buf.len() <= off || buf[off] != 0xFF {
            return Err(ProtocolError::UnexpectedEof);
        }
        off += 1;
        Ok((Self { entity_id, slot }, off))
    }
}

pub fn encode_set_entity_data_packet(v: &ItemEntityData, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_SET_ENTITY_DATA_S2C_ID, &body, out);
}

// ---------------------------------------------------------------------------
// Remove Entities：Prefixed Array VarInt。
// ---------------------------------------------------------------------------

/// 实体销毁（S2C 0x4E，despawn/拾取）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveEntities {
    pub ids: Vec<i32>,
}

impl RemoveEntities {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.ids.len() as i32, out);
        for id in &self.ids {
            write_varint(*id, out);
        }
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (len, n) = read_varint(&buf[off..])?;
        off += n;
        if len < 0 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let mut ids = Vec::with_capacity(len as usize);
        for _ in 0..len {
            let (id, n) = read_varint(&buf[off..])?;
            off += n;
            ids.push(id);
        }
        Ok((Self { ids }, off))
    }
}

pub fn encode_remove_entities_packet(v: &RemoveEntities, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_REMOVE_ENTITIES_S2C_ID, &body, out);
}

// ---------------------------------------------------------------------------
// Teleport Entity（entity_position_sync 0x23，线性路径）：entity + path=0 + xyz +
// yaw/pitch f32 + on_ground。运行时掉落同步用它（绝对坐标，不依赖差量基准）。
// ---------------------------------------------------------------------------

/// 实体传送/位置同步（S2C 0x23，线性路径 path=0）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeleportEntitySync {
    pub entity_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl TeleportEntitySync {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.entity_id, out);
        write_varint(0, out);
        write_f64_be(self.x, out);
        write_f64_be(self.y, out);
        write_f64_be(self.z, out);
        write_f32_be(self.yaw, out);
        write_f32_be(self.pitch, out);
        write_bool(self.on_ground, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (entity_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (path, n) = read_varint(&buf[off..])?;
        off += n;
        if path != 0 {
            return Err(ProtocolError::UnexpectedEof);
        }
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
        let (on_ground, n) = read_bool(&buf[off..])?;
        off += n;
        Ok((Self { entity_id, x, y, z, yaw, pitch, on_ground }, off))
    }
}

pub fn encode_teleport_entity_packet(v: &TeleportEntitySync, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_TELEPORT_ENTITY_S2C_ID, &body, out);
}

// ---------------------------------------------------------------------------
// Update Entity Position（move_entity_pos 0x36，step=0 线性差量）：
// properties bit0=on_ground，高位 step 数（本实现只发 0，即单组 Short 差量）。
// ---------------------------------------------------------------------------

/// 实体位置差量（S2C 0x36，step=0；差量=(新-旧)*4096，四舍五入 clamp 到 i16）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateEntityPos {
    pub entity_id: i32,
    pub on_ground: bool,
    pub dx: i16,
    pub dy: i16,
    pub dz: i16,
}

impl UpdateEntityPos {
    /// 由新旧绝对坐标算差量（与客户端 fixed-point 12 位小数一致）。
    pub fn delta(old: f64, new: f64) -> i16 {
        ((new - old) * 4096.0).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16
    }

    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.entity_id, out);
        write_varint(if self.on_ground { 1 } else { 0 }, out);
        write_short_be(self.dx, out);
        write_short_be(self.dy, out);
        write_short_be(self.dz, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (entity_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (props, n) = read_varint(&buf[off..])?;
        off += n;
        if props >> 1 != 0 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (dx, n) = read_short_be(&buf[off..])?;
        off += n;
        let (dy, n) = read_short_be(&buf[off..])?;
        off += n;
        let (dz, n) = read_short_be(&buf[off..])?;
        off += n;
        Ok((Self { entity_id, on_ground: props & 1 != 0, dx, dy, dz }, off))
    }
}

pub fn encode_update_entity_pos_packet(v: &UpdateEntityPos, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_UPDATE_ENTITY_POS_S2C_ID, &body, out);
}

// ---------------------------------------------------------------------------
// Pickup Item（take_item_entity 0x7F）：纯拾取动画，不删实体不加背包。
// ---------------------------------------------------------------------------

/// 拾取动画（S2C 0x7F；collector 取 LoginPlay entity_id=1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakeItemEntity {
    pub collected_id: i32,
    pub collector_id: i32,
    pub count: i32,
}

impl TakeItemEntity {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.collected_id, out);
        write_varint(self.collector_id, out);
        write_varint(self.count, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (collected_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (collector_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (count, n) = read_varint(&buf[off..])?;
        off += n;
        Ok((Self { collected_id, collector_id, count }, off))
    }
}

pub fn encode_take_item_packet(v: &TakeItemEntity, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_PICKUP_ITEM_S2C_ID, &body, out);
}

// ---------------------------------------------------------------------------
// 背包同步（window 0）：Content=全量 46 槽 + 手持物；Slot=单槽。
// ---------------------------------------------------------------------------

/// 全量背包（S2C 0x12，window 0，46 槽 + 手持空）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetContainerContent {
    pub window_id: i32,
    pub state_id: i32,
    pub slots: Vec<Slot>,
    pub carried: Slot,
}

impl SetContainerContent {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.window_id, out);
        write_varint(self.state_id, out);
        write_varint(self.slots.len() as i32, out);
        for s in &self.slots {
            s.encode_body(out);
        }
        self.carried.encode_body(out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (window_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (state_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (len, n) = read_varint(&buf[off..])?;
        off += n;
        if len < 0 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let mut slots = Vec::with_capacity(len as usize);
        for _ in 0..len {
            let (s, n) = Slot::decode_body(&buf[off..])?;
            off += n;
            slots.push(s);
        }
        let (carried, n) = Slot::decode_body(&buf[off..])?;
        off += n;
        Ok((Self { window_id, state_id, slots, carried }, off))
    }
}

pub fn encode_set_container_content_packet(v: &SetContainerContent, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_SET_CONTAINER_CONTENT_S2C_ID, &body, out);
}

/// 单槽背包（S2C 0x14，window 0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetContainerSlot {
    pub window_id: i32,
    pub state_id: i32,
    pub slot: i16,
    pub data: Slot,
}

impl SetContainerSlot {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.window_id, out);
        write_varint(self.state_id, out);
        write_short_be(self.slot, out);
        self.data.encode_body(out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (window_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (state_id, n) = read_varint(&buf[off..])?;
        off += n;
        let (slot, n) = read_short_be(&buf[off..])?;
        off += n;
        let (data, n) = Slot::decode_body(&buf[off..])?;
        off += n;
        Ok((Self { window_id, state_id, slot, data }, off))
    }
}

pub fn encode_set_container_slot_packet(v: &SetContainerSlot, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_SET_CONTAINER_SLOT_S2C_ID, &body, out);
}

// ---------------------------------------------------------------------------
// C2S Set Held Item（0x36）：Short BE 0..=8，越界返回错误（调用方忽略不断线）。
// ---------------------------------------------------------------------------

/// 解析 C2S SetHeldItem（Short；C2S 用 Short，S2C 同名包用 VarInt，不要混）。
pub fn decode_set_held_item_c2s(buf: &[u8]) -> Result<(u8, usize), ProtocolError> {
    let (v, n) = read_short_be(buf)?;
    if !(0..=8).contains(&v) {
        return Err(ProtocolError::UnexpectedEof);
    }
    if buf.len() != n {
        return Err(ProtocolError::UnexpectedEof);
    }
    Ok((v as u8, n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::codec::decode_packet_id;

    #[test]
    fn entity_packet_ids_match_777() {
        assert_eq!(PLAY_SPAWN_ENTITY_S2C_ID, 0x01);
        assert_eq!(PLAY_SET_ENTITY_DATA_S2C_ID, 0x65);
        assert_eq!(PLAY_REMOVE_ENTITIES_S2C_ID, 0x4E);
        assert_eq!(PLAY_TELEPORT_ENTITY_S2C_ID, 0x23);
        assert_eq!(PLAY_UPDATE_ENTITY_POS_S2C_ID, 0x36);
        assert_eq!(PLAY_PICKUP_ITEM_S2C_ID, 0x7F);
        assert_eq!(PLAY_SET_CONTAINER_CONTENT_S2C_ID, 0x12);
        assert_eq!(PLAY_SET_CONTAINER_SLOT_S2C_ID, 0x14);
        assert_eq!(super::super::play_block::PLAY_SET_HELD_ITEM_C2S_ID, 0x36);
        assert_eq!(super::super::play_block::PLAY_CLICK_CONTAINER_C2S_ID, 0x12);
        assert_eq!(ITEM_ENTITY_TYPE_ID, 72);
    }

    #[test]
    fn slot_empty_and_plain_roundtrip() {
        let e = Slot::empty();
        let mut b = Vec::new();
        e.encode_body(&mut b);
        assert_eq!(b, vec![0]);
        assert_eq!(Slot::decode_body(&b).unwrap().0, e);

        let s = Slot { count: 3, item_id: 55 };
        let mut b = Vec::new();
        s.encode_body(&mut b);
        let (got, used) = Slot::decode_body(&b).unwrap();
        assert_eq!(used, b.len());
        assert_eq!(got, s);
        // 带 component 的拒绝（避免错位流）。
        let mut bad = Vec::new();
        write_varint(1, &mut bad);
        write_varint(55, &mut bad);
        write_varint(1, &mut bad);
        write_varint(0, &mut bad);
        assert!(Slot::decode_body(&bad).is_err());
        assert!(Slot::decode_body(&[]).is_err());
    }

    #[test]
    fn lpvec3_zero_and_samples_roundtrip() {
        let mut b = Vec::new();
        LpVec3::zero().encode_body(&mut b);
        assert_eq!(b, vec![0]);
        assert_eq!(LpVec3::decode_body(&b).unwrap().0, LpVec3::zero());

        // wiki 样本：(1,0,-1) 编码断言。
        let mut b = Vec::new();
        LpVec3 { x: 1.0, y: 0.0, z: -1.0 }.encode_body(&mut b);
        assert_eq!(b, vec![0xF1, 0xFF, 0x00, 0x00, 0xFF, 0xFF]);
        let (got, used) = LpVec3::decode_body(&b).unwrap();
        assert_eq!(used, b.len());
        assert!((got.x - 1.0).abs() < 0.01 && got.y.abs() < 0.01 && (got.z + 1.0).abs() < 0.01);

        // 小速度往返误差在量化精度内。
        for v in [LpVec3 { x: 0.1, y: 0.25, z: -0.05 }, LpVec3 { x: 2.0, y: -1.0, z: 0.5 }] {
            let mut b = Vec::new();
            v.encode_body(&mut b);
            let (got, used) = LpVec3::decode_body(&b).unwrap();
            assert_eq!(used, b.len());
            assert!((got.x - v.x).abs() < 0.05, "x {v:?} -> {got:?}");
            assert!((got.y - v.y).abs() < 0.05, "y {v:?} -> {got:?}");
            assert!((got.z - v.z).abs() < 0.05, "z {v:?} -> {got:?}");
        }
        assert!(LpVec3::decode_body(&[]).is_err());
    }

    #[test]
    fn spawn_item_roundtrip_and_wire_id() {
        let v = SpawnItemEntity {
            entity_id: 1001,
            uuid: 0x0102030405060708090a0b0c0d0e0f10,
            x: 369.5,
            y: 73.0,
            z: 381.5,
            vel: LpVec3::zero(),
            pitch: 0,
            yaw: 0,
            head_yaw: 0,
        };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = SpawnItemEntity::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        // type 非 72 拒绝；data 非 0 拒绝（entity_id=1001 占 2 字节 VarInt，type 在 [18]）。
        let mut bad = body.clone();
        bad[18] = 73;
        assert!(SpawnItemEntity::decode_body(&bad).is_err());

        let mut pkt = Vec::new();
        encode_spawn_item_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, _) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_SPAWN_ENTITY_S2C_ID);
    }

    #[test]
    fn entity_data_metadata_roundtrip() {
        let v = ItemEntityData { entity_id: 1001, slot: Slot { count: 1, item_id: 55 } };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = ItemEntityData::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        // 缺 terminator 报错。
        assert!(ItemEntityData::decode_body(&body[..body.len() - 1]).is_err());

        let mut pkt = Vec::new();
        encode_set_entity_data_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, _) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_SET_ENTITY_DATA_S2C_ID);
    }

    #[test]
    fn remove_entities_roundtrip() {
        let v = RemoveEntities { ids: vec![1001, 1002, 300000] };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = RemoveEntities::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        assert!(RemoveEntities::decode_body(&[]).is_err());

        let mut pkt = Vec::new();
        encode_remove_entities_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, _) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_REMOVE_ENTITIES_S2C_ID);
    }

    #[test]
    fn teleport_entity_sync_roundtrip() {
        let v =
            TeleportEntitySync { entity_id: 1001, x: 1.5, y: 70.0, z: -3.25, yaw: 10.0, pitch: -5.0, on_ground: true };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = TeleportEntitySync::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        // stepped 路径拒绝（entity_id=1001 占 2 字节，path 在 [2]）。
        let mut bad = body.clone();
        bad[2] = 1;
        assert!(TeleportEntitySync::decode_body(&bad).is_err());

        let mut pkt = Vec::new();
        encode_teleport_entity_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, _) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_TELEPORT_ENTITY_S2C_ID);
    }

    #[test]
    fn update_entity_pos_delta_and_roundtrip() {
        assert_eq!(UpdateEntityPos::delta(10.0, 10.0), 0);
        assert_eq!(UpdateEntityPos::delta(0.0, 1.0), 4096);
        assert_eq!(UpdateEntityPos::delta(0.0, 0.0002), 1);
        // 超大位移 clamp 到 i16。
        assert_eq!(UpdateEntityPos::delta(0.0, 100.0), i16::MAX);
        assert_eq!(UpdateEntityPos::delta(0.0, -100.0), i16::MIN);

        let v = UpdateEntityPos { entity_id: 7, on_ground: true, dx: 12, dy: -3, dz: 4096 };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = UpdateEntityPos::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        let v2 = UpdateEntityPos { entity_id: 7, on_ground: false, dx: 0, dy: 0, dz: 0 };
        let mut b2 = Vec::new();
        v2.encode_body(&mut b2);
        assert_eq!(UpdateEntityPos::decode_body(&b2).unwrap().0, v2);
        // stepped (properties>>1 != 0) 拒绝。
        let mut bad = b2.clone();
        bad[1] = 3;
        assert!(UpdateEntityPos::decode_body(&bad).is_err());
    }

    #[test]
    fn take_item_roundtrip() {
        let v = TakeItemEntity { collected_id: 1001, collector_id: 1, count: 3 };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = TakeItemEntity::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);

        let mut pkt = Vec::new();
        encode_take_item_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, _) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_PICKUP_ITEM_S2C_ID);
    }

    #[test]
    fn container_content_and_slot_roundtrip() {
        let mut slots = vec![Slot::empty(); PLAYER_INVENTORY_SLOT_COUNT];
        slots[36] = Slot { count: 64, item_id: 1 };
        slots[9] = Slot { count: 3, item_id: 55 };
        let v =
            SetContainerContent { window_id: 0, state_id: 5, slots: slots.clone(), carried: Slot::empty() };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = SetContainerContent::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);

        let mut pkt = Vec::new();
        encode_set_container_content_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, _) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_SET_CONTAINER_CONTENT_S2C_ID);

        let s = SetContainerSlot { window_id: 0, state_id: 6, slot: 36, data: Slot { count: 63, item_id: 1 } };
        let mut b2 = Vec::new();
        s.encode_body(&mut b2);
        let (got2, used2) = SetContainerSlot::decode_body(&b2).unwrap();
        assert_eq!(used2, b2.len());
        assert_eq!(got2, s);

        let mut pkt2 = Vec::new();
        encode_set_container_slot_packet(&s, &mut pkt2);
        let (_, n) = read_varint(&pkt2).unwrap();
        let (id, _) = decode_packet_id(&pkt2[n..]).unwrap();
        assert_eq!(id, PLAY_SET_CONTAINER_SLOT_S2C_ID);
    }

    #[test]
    fn set_held_item_c2s_is_short_not_varint() {
        // 0..=8 合法（Short BE），且整个包体恰 2 字节。
        for slot in 0..=8i16 {
            let mut b = vec![(slot >> 8) as u8, slot as u8];
            assert_eq!(decode_set_held_item_c2s(&b).unwrap().0, slot as u8);
            b.push(0);
            assert!(decode_set_held_item_c2s(&b).is_err(), "trailing byte must fail");
        }
        assert!(decode_set_held_item_c2s(&[0, 9]).is_err());
        assert!(decode_set_held_item_c2s(&[0]).is_err());
        assert!(decode_set_held_item_c2s(&[]).is_err());
    }
}
