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
//! - C2S `container_click` 0x12：ClickContainer（PICKUP/SWAP/QUICK_MOVE/THROW 已实现；
//!   changedSlots/carried 按 jar 实测线格式跳过、不做 hash 校验，见 §7）
//!
//! P18 范围：掉落看得见（Spawn+Metadata）、下落同步（0x23 Teleport）、拾取动画（0x7F
//! 可选但实现）、背包同步（Content/Slot）。窗口整理（ClickContainer 解析）留后续。

use crate::packets::codec::{
    encode_packet, read_bool, read_f32_be, read_f64_be, write_bool, write_f32_be, write_f64_be,
};
use crate::{read_varint, write_varint, ProtocolError};

// 以下包 ID 全部编译期从 `mc-registry` 由 data/777/packets.json 生成的 PACKETS 表派生。

/// S2C Spawn Entity（掉落物）：`add_entity`。
pub const PLAY_SPAWN_ENTITY_S2C_ID: i32 = super::packet_id("clientbound.play", "add_entity");
/// S2C Set Entity Metadata：`set_entity_data`。
pub const PLAY_SET_ENTITY_DATA_S2C_ID: i32 = super::packet_id("clientbound.play", "set_entity_data");
/// S2C Remove Entities：`remove_entities`。
pub const PLAY_REMOVE_ENTITIES_S2C_ID: i32 = super::packet_id("clientbound.play", "remove_entities");
/// S2C Entity Position Sync：`entity_position_sync`（线性路径，绝对坐标同步）。
/// 0x23 历史上被误称为 TeleportEntity（Pumpkin 的 `teleport_entity` 是另一个包 0x80）；
/// 包体不变，仅名字改正。
pub const PLAY_ENTITY_POSITION_SYNC_S2C_ID: i32 = super::packet_id("clientbound.play", "entity_position_sync");
/// S2C Update Entity Position：`move_entity_pos`（线性差量 step=0）。
pub const PLAY_UPDATE_ENTITY_POS_S2C_ID: i32 = super::packet_id("clientbound.play", "move_entity_pos");
/// S2C Pickup Item：`take_item_entity`（拾取动画，不删实体不加背包，需另发 Remove+背包）。
pub const PLAY_PICKUP_ITEM_S2C_ID: i32 = super::packet_id("clientbound.play", "take_item_entity");
/// S2C Set Container Content：`container_set_content`（window 0=玩家背包）。
pub const PLAY_SET_CONTAINER_CONTENT_S2C_ID: i32 = super::packet_id("clientbound.play", "container_set_content");
/// S2C Set Container Slot：`container_set_slot`（window 0=玩家背包）。
pub const PLAY_SET_CONTAINER_SLOT_S2C_ID: i32 = super::packet_id("clientbound.play", "container_set_slot");
/// C2S Set Held Item / Click Container 的 ID 见 `play_block`
///（`PLAY_SET_HELD_ITEM_C2S_ID` 0x36 / `PLAY_CLICK_CONTAINER_C2S_ID` 0x12）：
/// C2S 0x36 是 Short（本文件 `decode_set_held_item_c2s`），注意 S2C 同名包是 VarInt。
/// S2C Set Held Slot：`set_held_slot`（clientbound.play=107=0x6B，生成表断言锁定）——
/// 服务端→客户端同步选中快捷栏（VarInt 0..=8）；Join 时必发一次，对齐客户端默认选 0。
pub const PLAY_SET_HELD_SLOT_S2C_ID: i32 = super::packet_id("clientbound.play", "set_held_slot");
/// S2C Set Experience：`set_experience`（clientbound.play 105 = 0x69，777 生成表查得）。
pub const PLAY_SET_EXPERIENCE_S2C_ID: i32 = super::packet_id("clientbound.play", "set_experience");

/// S2C Set Held Slot 包体：VarInt 选中槽 0..=8（越界由调用方保证不发送）。
pub fn encode_set_held_slot_s2c(slot: u8) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(slot as i32, &mut out);
    out
}

/// `minecraft:item` 在 EntityType 注册表中的位置，编译期从 `mc-registry` 由
/// data/777/entities.json 生成的 ENTITIES 表派生（26.3 实测值 72）。
pub const ITEM_ENTITY_TYPE_ID: i32 = super::entity_id("minecraft:item");

/// P31 `minecraft:experience_orb` 在 EntityType 注册表中的位置，编译期从 `mc-registry`
/// 由 data/777/entities.json 生成的 ENTITIES 表派生（26.3 实测值 50）。
pub const EXPERIENCE_ORB_ENTITY_TYPE_ID: i32 = super::entity_id("minecraft:experience_orb");

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
// P31 经验球生成：复用 add_entity 包体（wiki §Spawn Entity 对全部实体通用），
// 只把 type 换成 50（minecraft:experience_orb），data 仍为 0。
// data/777/packets.json 没有 spawn_experience_orb 的 ID，故不猜新包，保守复用已验证包。
// ---------------------------------------------------------------------------

/// 经验球生成（S2C 0x01，`minecraft:experience_orb`，data=0）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnExperienceOrb {
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

impl SpawnExperienceOrb {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.entity_id, out);
        write_uuid(self.uuid, out);
        write_varint(EXPERIENCE_ORB_ENTITY_TYPE_ID, out);
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
        if typ != EXPERIENCE_ORB_ENTITY_TYPE_ID {
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

pub fn encode_spawn_experience_orb_packet(v: &SpawnExperienceOrb, out: &mut Vec<u8>) {
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
// P31 经验球元数据（S2C 0x65）：index 8 / type VarInt(1) / 经验值 VarInt + 0xFF。
// wiki Entity metadata §Experience Orb：index 8 = VarInt(1) = 「orb 拾取时给的经验」。
// ---------------------------------------------------------------------------

/// 经验球的经验值元数据（决定客户端渲染大小与球内数值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XpOrbEntityData {
    pub entity_id: i32,
    pub amount: i32,
}

impl XpOrbEntityData {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.entity_id, out);
        out.push(8);
        write_varint(1, out); // EntityDataSerializers.INT
        write_varint(self.amount, out);
        out.push(0xFF);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (entity_id, n) = read_varint(&buf[off..])?;
        off += n;
        if buf.len() <= off || buf[off] != 8 {
            return Err(ProtocolError::UnexpectedEof);
        }
        off += 1;
        let (typ, n) = read_varint(&buf[off..])?;
        off += n;
        if typ != 1 {
            return Err(ProtocolError::UnexpectedEof);
        }
        let (amount, n) = read_varint(&buf[off..])?;
        off += n;
        if buf.len() <= off || buf[off] != 0xFF {
            return Err(ProtocolError::UnexpectedEof);
        }
        off += 1;
        Ok((Self { entity_id, amount }, off))
    }
}

pub fn encode_xp_orb_data_packet(v: &XpOrbEntityData, out: &mut Vec<u8>) {
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
    encode_packet(PLAY_ENTITY_POSITION_SYNC_S2C_ID, &body, out);
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
// P31 Set Experience（S2C 0x69）：Float 进度 + VarInt 等级 + VarInt 总经验。
// 来源：wiki §Set Experience（ID 105/0x69）+ data/777/packets.json set_experience。
// ---------------------------------------------------------------------------

/// 经验/等级同步包体（客户端经验条与等级显示）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetExperience {
    /// 当前等级内进度 0.0..=1.0。
    pub progress: f32,
    pub level: i32,
    /// 总经验值（wiki Total Experience）。
    pub total: i32,
}

impl SetExperience {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_f32_be(self.progress, out);
        write_varint(self.level, out);
        write_varint(self.total, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (progress, n) = read_f32_be(&buf[off..])?;
        off += n;
        let (level, n) = read_varint(&buf[off..])?;
        off += n;
        let (total, n) = read_varint(&buf[off..])?;
        off += n;
        Ok((Self { progress, level, total }, off))
    }
}

pub fn encode_set_experience_packet(v: &SetExperience, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_SET_EXPERIENCE_S2C_ID, &body, out);
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

// ---------------------------------------------------------------------------
// C2S Container Click（0x12）：只解析 container/state/slot/button/click_type，
// changedSlots + carriedItem 按 jar 实测 777 线格式跳过（不校验 hash）：
// container VarInt + state VarInt + slot ShortBE + button Byte + clickType VarInt(0..=6)
// + changedSlots(VarInt 条数 + [ShortBE 槽 + hashed 栈]*) + carried(hashed 栈)；
// hashed 栈 = Bool(1B) + [VarInt 物品 + VarInt 数量 + patchmap[
//   added(VarInt 条数 + [VarInt 组件 + I32BE hash]*) + removed(VarInt 条数 + VarInt 组件*)]]。
// 计数设上限防恶意包撑内存；clickType>6 拒绝（777 只有 0..=6）。
// ---------------------------------------------------------------------------

/// C2S 点击类型（ContainerInput ordinal，jar 实测 PICKUP=0..PICKUP_ALL=6）。
pub const CLICK_PICKUP: u8 = 0;
/// Shift+点击整栈搬运。
pub const CLICK_QUICK_MOVE: u8 = 1;
/// 数字键：点击槽 <-> 快捷栏 button 槽交换。
pub const CLICK_SWAP: u8 = 2;
/// 背包打开时按 Q：扔点击槽里的东西（jar `ContainerInput::THROW` id=4；
/// button=0 扔 1 个，button=1 扔整栈；要求光标为空，否则原版无操作）。
pub const CLICK_THROW: u8 = 4;

/// C2S ContainerClick（0x12）解析结果（changedSlots/carried 已跳过不返回）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerClick {
    pub container_id: i32,
    pub state_id: i32,
    pub slot: i16,
    pub button: i8,
    pub click_type: u8,
}

fn skip_varint(buf: &[u8], off: &mut usize) -> Result<(), ProtocolError> {
    let (_, n) = read_varint(&buf[*off..])?;
    *off += n;
    Ok(())
}

fn skip_bytes(buf: &[u8], off: &mut usize, n: usize) -> Result<(), ProtocolError> {
    if buf.len() < *off + n {
        return Err(ProtocolError::UnexpectedEof);
    }
    *off += n;
    Ok(())
}

/// 跳过一个 HashedPatchMap：added(条数 + [组件 VarInt + I32 hash]*) + removed(条数 + 组件 VarInt*)。
fn skip_patchmap(buf: &[u8], off: &mut usize) -> Result<(), ProtocolError> {
    let (added, n) = read_varint(&buf[*off..])?;
    if !(0..=256).contains(&added) {
        return Err(ProtocolError::UnexpectedEof);
    }
    *off += n;
    for _ in 0..added {
        skip_varint(buf, off)?;
        skip_bytes(buf, off, 4)?;
    }
    let (removed, n) = read_varint(&buf[*off..])?;
    if !(0..=256).contains(&removed) {
        return Err(ProtocolError::UnexpectedEof);
    }
    *off += n;
    for _ in 0..removed {
        skip_varint(buf, off)?;
    }
    Ok(())
}

/// 跳过一个 HashedStack：Bool(1B) + [物品 VarInt + 数量 VarInt + patchmap]。
fn skip_hashed_stack(buf: &[u8], off: &mut usize) -> Result<(), ProtocolError> {
    let (present, n) = read_bool(&buf[*off..])?;
    *off += n;
    if !present {
        return Ok(());
    }
    skip_varint(buf, off)?;
    skip_varint(buf, off)?;
    skip_patchmap(buf, off)
}

/// 解析 C2S ContainerClick（0x12），changedSlots/carried 只跳过。
pub fn decode_container_click(buf: &[u8]) -> Result<(ContainerClick, usize), ProtocolError> {
    let mut off = 0;
    let (container_id, n) = read_varint(&buf[off..])?;
    off += n;
    let (state_id, n) = read_varint(&buf[off..])?;
    off += n;
    let (slot, n) = read_short_be(&buf[off..])?;
    off += n;
    if buf.len() <= off {
        return Err(ProtocolError::UnexpectedEof);
    }
    let button = buf[off] as i8;
    off += 1;
    let (click_type, n) = read_varint(&buf[off..])?;
    off += n;
    if !(0..=6).contains(&click_type) {
        return Err(ProtocolError::UnexpectedEof);
    }
    let (changed, n) = read_varint(&buf[off..])?;
    if !(0..=64).contains(&changed) {
        return Err(ProtocolError::UnexpectedEof);
    }
    off += n;
    for _ in 0..changed {
        let (_, n) = read_short_be(&buf[off..])?;
        off += n;
        skip_hashed_stack(buf, &mut off)?;
    }
    skip_hashed_stack(buf, &mut off)?;
    Ok((ContainerClick { container_id, state_id, slot, button, click_type: click_type as u8 }, off))
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
        assert_eq!(PLAY_ENTITY_POSITION_SYNC_S2C_ID, 0x23);
        assert_eq!(PLAY_UPDATE_ENTITY_POS_S2C_ID, 0x36);
        assert_eq!(PLAY_PICKUP_ITEM_S2C_ID, 0x7F);
        assert_eq!(PLAY_SET_CONTAINER_CONTENT_S2C_ID, 0x12);
        assert_eq!(PLAY_SET_CONTAINER_SLOT_S2C_ID, 0x14);
        assert_eq!(super::super::play_block::PLAY_SET_HELD_ITEM_C2S_ID, 0x36);
        assert_eq!(super::super::play_block::PLAY_CLICK_CONTAINER_C2S_ID, 0x12);
        assert_eq!(PLAY_SET_HELD_SLOT_S2C_ID, 0x6B);
        assert_eq!(ITEM_ENTITY_TYPE_ID, 72);
        // P31：从 data/777 生成表查得，不允许硬编码猜测。
        assert_eq!(PLAY_SET_EXPERIENCE_S2C_ID, 105);
        assert_eq!(EXPERIENCE_ORB_ENTITY_TYPE_ID, 50);
    }

    /// P31：经验球 Spawn/Data 与 SetExperience 的包体往返（保守复用已验证布局）。
    #[test]
    fn p31_xp_packets_roundtrip() {
        let v = SpawnExperienceOrb {
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
        let (got, used) = SpawnExperienceOrb::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        // 物品 Spawn 不能误收经验球（type 校验不同），反之亦然。
        assert!(SpawnItemEntity::decode_body(&body).is_err());
        let mut pkt = Vec::new();
        encode_spawn_experience_orb_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, _) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_SPAWN_ENTITY_S2C_ID);

        let d = XpOrbEntityData { entity_id: 1001, amount: 3 };
        let mut body = Vec::new();
        d.encode_body(&mut body);
        let (got, used) = XpOrbEntityData::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, d);
        assert!(ItemEntityData::decode_body(&body).is_err());
        assert!(XpOrbEntityData::decode_body(&body[..body.len() - 1]).is_err());

        let e = SetExperience { progress: 0.5, level: 1, total: 11 };
        let mut body = Vec::new();
        e.encode_body(&mut body);
        let (got, used) = SetExperience::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, e);
        let mut pkt = Vec::new();
        encode_set_experience_packet(&e, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, _) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_SET_EXPERIENCE_S2C_ID);
    }

    #[test]
    fn set_held_slot_s2c_is_varint_roundtrip() {
        // S2C 0x6B 包体是单个 VarInt（与 C2S 0x36 Short 不同，jar 布局，勿改）。
        for slot in 0..=8u8 {
            let b = encode_set_held_slot_s2c(slot);
            assert_eq!(crate::read_varint(&b).unwrap().0, slot as i32);
        }
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
        assert_eq!(id, PLAY_ENTITY_POSITION_SYNC_S2C_ID);
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
    fn container_click_decode_and_skip() {
        use super::{CLICK_PICKUP, CLICK_SWAP};
        // container=0 varint, state=3 varint, slot=37 short, button=0, type=PICKUP,
        // changedSlots=1 [(slot=37 short, hashed present + item varint + count varint + 空 patchmap)],
        // carried=空(0x00)。changed/carried 只跳过，返回头字段。
        let mut b = Vec::new();
        write_varint(0, &mut b);
        write_varint(3, &mut b);
        b.extend_from_slice(&37i16.to_be_bytes());
        b.push(0);
        write_varint(CLICK_PICKUP as i32, &mut b);
        write_varint(1, &mut b);
        b.extend_from_slice(&37i16.to_be_bytes());
        write_bool(true, &mut b);
        write_varint(55, &mut b);
        write_varint(1, &mut b);
        write_varint(0, &mut b);
        write_varint(0, &mut b);
        write_bool(false, &mut b);
        let (got, used) = decode_container_click(&b).unwrap();
        assert_eq!(used, b.len());
        assert_eq!(
            got,
            ContainerClick { container_id: 0, state_id: 3, slot: 37, button: 0, click_type: CLICK_PICKUP }
        );
        // SWAP 头字段同样解析。
        let mut c = Vec::new();
        write_varint(0, &mut c);
        write_varint(4, &mut c);
        c.extend_from_slice(&9i16.to_be_bytes());
        c.push(2);
        write_varint(CLICK_SWAP as i32, &mut c);
        write_varint(0, &mut c);
        write_bool(false, &mut c);
        let (got2, used2) = decode_container_click(&c).unwrap();
        assert_eq!(used2, c.len());
        assert_eq!(got2.button, 2);
        assert_eq!(got2.click_type, CLICK_SWAP);
        // 非法：clickType 越界 / 截断 / changedSlots 过大。
        let mut bad = c.clone();
        bad[5] = 7;
        assert!(decode_container_click(&bad).is_err());
        assert!(decode_container_click(&b[..b.len() - 1]).is_err());
        let mut many = Vec::new();
        write_varint(0, &mut many);
        write_varint(0, &mut many);
        many.extend_from_slice(&0i16.to_be_bytes());
        many.push(0);
        write_varint(0, &mut many);
        write_varint(99, &mut many);
        assert!(decode_container_click(&many).is_err());
    }

    #[test]
    fn container_click_throw_mode_decodes() {
        // P19：背包打开时按 Q 走 THROW（`ContainerInput` id=4，button 0=1 个/1=整栈）。
        assert_eq!(CLICK_THROW, 4);
        let mut b = Vec::new();
        write_varint(0, &mut b);
        write_varint(9, &mut b);
        b.extend_from_slice(&37i16.to_be_bytes());
        b.push(1);
        write_varint(CLICK_THROW as i32, &mut b);
        write_varint(0, &mut b);
        write_bool(false, &mut b);
        let (got, used) = decode_container_click(&b).unwrap();
        assert_eq!(used, b.len());
        assert_eq!(
            got,
            ContainerClick { container_id: 0, state_id: 9, slot: 37, button: 1, click_type: CLICK_THROW }
        );
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
