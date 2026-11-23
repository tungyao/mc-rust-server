//! config: Configuration阶段 KnownPacks→Registry→Finish最小可用，只兼容26.3/777，ID已按P4A对照表核对777
//! 流程：Login Ack后 → 发 KnownPacks S2C → 等 KnownPacks C2S → 发 Registry Data(4包) → 发 Update Tags → 发 Finish S2C → 等 Finish Ack C2S → enter_play。
//! 字段来源：minecraft.wiki 777 Configuration `select_known_packs`(count VarInt + namespace/id/version均为String)
//! 与 `registry_data`(registry_id Identifier + count VarInt + entries[entry_id Identifier + has_data Bool + 可选NBT])。
use crate::packets::codec::{decode_string, encode_packet, encode_string};
use crate::{read_varint, write_varint, ProtocolError};

/// Finish Configuration CB 包 ID（Configuration `0x03` finish_configuration）。已按P4A对照表核对777。
pub const CONFIG_FINISH_S2C_ID: i32 = 0x03;
/// Finish Configuration Ack SB 包 ID（Configuration `0x03` finish_configuration）。已按P4A对照表核对777。
pub const CONFIG_FINISH_ACK_C2S_ID: i32 = 0x03;
/// Registry Data CB 包 ID（Configuration `0x07` registry_data）。已按P4A对照表核对777（旧占位0x05作废）。
pub const CONFIG_REGISTRY_DATA_S2C_ID: i32 = 0x07;
/// Known Packs CB 包 ID（Configuration `0x0F` select_known_packs）。已按P4A对照表核对777（旧占位0x0E是26.1值，777因update_tags插入顺延）。
pub const CONFIG_KNOWN_PACKS_S2C_ID: i32 = 0x0F;
/// Known Packs SB 包 ID（Configuration `0x07` select_known_packs）。已按P4A对照表核对777。
pub const CONFIG_KNOWN_PACKS_C2S_ID: i32 = 0x07;

/// 是否跳过 Registry 下发：P6a起为false（发最小registry再Finish，真机才站得住）。
/// 空registry直接Finish在多数vanilla上可过，但Play Login按索引查dimension_type，缺表会黑屏/被踢，故至少发2包。
pub fn config_registry_skipped() -> bool { false }

/// Finish Configuration S2C 空包（服务端发，encode 供发送用；decode 供单测/模拟客户端用）。
pub fn encode_config_finish_s2c_packet(out: &mut Vec<u8>) {
    encode_packet(CONFIG_FINISH_S2C_ID, &[], out);
}
/// Finish S2C 解码：空包，忽略尾部字节。
pub fn decode_config_finish_s2c(_payload: &[u8]) -> Result<(), ProtocolError> { Ok(()) }

/// Finish Ack C2S 空包（客户端回，encode 供单测/模拟客户端用；服务端主要做 decode）。
pub fn encode_config_finish_ack_packet(out: &mut Vec<u8>) {
    encode_packet(CONFIG_FINISH_ACK_C2S_ID, &[], out);
}
/// Finish Ack 解码：空包，忽略尾部字节。
pub fn decode_config_finish_ack(_payload: &[u8]) -> Result<(), ProtocolError> { Ok(()) }

// Registry Data：registry_id + 条目表。ID已按P4A对照表核对（CB 0x07）。
// 线格式来源 minecraft.wiki 777 `registry_data`：registry_id(Identifier/String) + VarInt条数
// + 每项 entry_id(Identifier/String) + has_data(Bool) + 有数据时NBT。
// data语义：Some内即完整未压缩NBT字节（包内NBT：0x0A + fields + 0x00，无根名、无长度前缀）。
use super::nbt::{NbtCompound, NbtList, NbtTag};

/// overworld dimension_type 真NBT（未压缩包内格式：0x0A + fields + 0x00，无根名、无长度前缀）。
/// 字段对标 docs/registries-777.md §2.1 + 真机数据 dimension_overworld.json（26.3现行codec，已删7旧字段）。
/// monster_spawn_light_level 为 int-provider 对象 {type, min_inclusive, max_inclusive}（Int 与对象两种都合法，真数据用对象）；
/// attributes 整棵照抄真数据；timelines 字符串 "#minecraft:in_overworld"；infiniburn 保持 "#minecraft:infiniburn_overworld"。
pub fn real_overworld_dimension_nbt() -> Vec<u8> {
    let tag = NbtTag::Compound(
        NbtCompound::new()
            .with("has_skylight", NbtTag::Byte(1))
            .with("has_ceiling", NbtTag::Byte(0))
            .with("has_ender_dragon_fight", NbtTag::Byte(0))
            .with("coordinate_scale", NbtTag::Double(1.0))
            .with("ambient_light", NbtTag::Float(0.0))
            .with("monster_spawn_block_light_limit", NbtTag::Int(0))
            .with("monster_spawn_light_level", NbtTag::compound(vec![
                ("type", NbtTag::String("minecraft:uniform".into())),
                ("min_inclusive", NbtTag::Int(0)),
                ("max_inclusive", NbtTag::Int(7)),
            ]))
            .with("logical_height", NbtTag::Int(384))
            .with("min_y", NbtTag::Int(-64))
            .with("height", NbtTag::Int(384))
            .with("infiniburn", NbtTag::String("#minecraft:infiniburn_overworld".into()))
            .with("default_clock", NbtTag::String("minecraft:overworld".into()))
            .with("attributes", real_overworld_attributes_nbt())
            .with("timelines", NbtTag::String("#minecraft:in_overworld".into()))
            .clone(),
    );
    let mut out = Vec::with_capacity(512);
    out.push(0x0A); // 包内NBT：根compound id，无根名，直接fields
    tag.write_payload(&mut out);
    out
}

/// overworld attributes 整棵（照抄真机 dimension_overworld.json，类型按 wiki Environment_attribute 页取 NBT 类型）：
/// bool→Byte；整数→Int；字符串（颜色/sound/规则）→String；cloud_height 按 wiki [Float] 取 Float；
/// mood.offset 取 Float(2.0)：原版JSON字面即2.0，若codec是Int原版自己先炸，故必为浮点类；wiki的[Int]存疑。
fn real_overworld_attributes_nbt() -> NbtTag {
    NbtTag::compound(vec![
        ("minecraft:audio/ambient_sounds", NbtTag::compound(vec![
            ("mood", NbtTag::compound(vec![
                ("block_search_extent", NbtTag::Int(8)),
                ("offset", NbtTag::Float(2.0)),
                ("sound", NbtTag::String("minecraft:ambient.cave".into())),
                ("tick_delay", NbtTag::Int(6000)),
            ])),
        ])),
        ("minecraft:audio/background_music", NbtTag::compound(vec![
            ("creative", NbtTag::compound(vec![
                ("max_delay", NbtTag::Int(24000)),
                ("min_delay", NbtTag::Int(12000)),
                ("sound", NbtTag::String("minecraft:music.creative".into())),
            ])),
            ("default", NbtTag::compound(vec![
                ("max_delay", NbtTag::Int(24000)),
                ("min_delay", NbtTag::Int(12000)),
                ("sound", NbtTag::String("minecraft:music.game".into())),
            ])),
        ])),
        ("minecraft:gameplay/bed_rule", NbtTag::compound(vec![
            ("can_set_spawn", NbtTag::String("always".into())),
            ("can_sleep", NbtTag::String("when_dark".into())),
            ("error_message", NbtTag::compound(vec![
                ("translate", NbtTag::String("block.minecraft.bed.no_sleep".into())),
            ])),
        ])),
        ("minecraft:gameplay/nether_portal_spawns_piglin", NbtTag::Byte(1)),
        ("minecraft:gameplay/respawn_anchor_works", NbtTag::Byte(0)),
        ("minecraft:gameplay/straw_bed_rule", NbtTag::compound(vec![
            ("can_set_spawn", NbtTag::String("never".into())),
            ("can_sleep", NbtTag::String("when_dark".into())),
            ("destroy_on_leave", NbtTag::Byte(1)),
            ("error_message", NbtTag::compound(vec![
                ("translate", NbtTag::String("block.minecraft.bed.no_sleep".into())),
            ])),
        ])),
        ("minecraft:visual/ambient_light_color", NbtTag::String("#0a0a0a".into())),
        ("minecraft:visual/cloud_color", NbtTag::String("#ccffffff".into())),
        ("minecraft:visual/cloud_height", NbtTag::Float(192.33)),
        ("minecraft:visual/fog_color", NbtTag::String("#c0d8ff".into())),
        ("minecraft:visual/sky_color", NbtTag::String("#78a7ff".into())),
    ])
}

/// plains biome 真NBT（未压缩包内格式：0x0A + fields + 0x00，无根名、无长度前缀）。
/// 字段对标 docs/registries-777.md §2.2：删3旧色值字段，补 temperature_modifier + carvers/features 空list。
pub fn real_plains_biome_nbt() -> Vec<u8> {
    let effects = NbtTag::compound(vec![("water_color", NbtTag::Int(4_159_204))]);
    let tag = NbtTag::Compound(
        NbtCompound::new()
            .with("has_precipitation", NbtTag::Byte(1))
            .with("temperature", NbtTag::Float(0.8))
            .with("temperature_modifier", NbtTag::String("none".into()))
            .with("downfall", NbtTag::Float(0.4))
            .with("effects", effects)
            .with("carvers", NbtTag::List(NbtList::empty()))
            .with("features", NbtTag::List(NbtList::empty()))
            .clone(),
    );
    let mut out = Vec::with_capacity(128);
    out.push(0x0A); // 包内NBT：根compound id，无根名，直接fields
    tag.write_payload(&mut out);
    out
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRegistryEntry { pub entry_id: String, pub data: Option<Vec<u8>> }

impl ConfigRegistryEntry {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.entry_id, out);
        match &self.data {
            // Prefixed Optional NBT：Bool(1) + 裸NBT（无长度前缀，NBT自定界）
            Some(d) => { out.push(1); out.extend_from_slice(d); }
            None => out.push(0),
        }
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (entry_id, n1) = decode_string(buf)?;
        if buf.len() <= n1 { return Err(ProtocolError::UnexpectedEof); }
        let (data, off) = if buf[n1] != 0 {
            // 包内NBT：首字节须为 0x0A compound，后跟 fields + 0x00，用 nbt reader 定界
            let rest = &buf[n1 + 1..];
            if rest.first() != Some(&0x0A) { return Err(ProtocolError::UnexpectedEof); }
            let (_c, n) = super::nbt::read_nbt_compound_payload(&rest[1..]).map_err(|_| ProtocolError::UnexpectedEof)?;
            (Some(buf[n1 + 1..n1 + 1 + 1 + n].to_vec()), n1 + 1 + 1 + n)
        } else {
            (None, n1 + 1)
        };
        Ok((Self { entry_id, data }, off))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRegistryData { pub registry_id: String, pub entries: Vec<ConfigRegistryEntry> }

impl ConfigRegistryData {
    /// 空 registry 构造（仅单测用；生产走 minimal_*，不再跳过）。
    pub fn empty(registry_id: impl Into<String>) -> Self {
        Self { registry_id: registry_id.into(), entries: Vec::new() }
    }
    /// 最小 dimension_type：1项 overworld 且 has_data=true（真NBT）。
    /// 来源：minecraft.wiki 777 `registry_data`；vanilla注册表名 `minecraft:dimension_type`，
    /// Play Login按索引查维度（本服LoginPlay dimension_type=0即此表第0项），缺表真机黑屏/被踢。
    pub fn minimal_dimension_type() -> Self {
        Self {
            registry_id: "minecraft:dimension_type".into(),
            entries: vec![ConfigRegistryEntry { entry_id: "minecraft:overworld".into(), data: Some(real_overworld_dimension_nbt()) }],
        }
    }
    /// 最小 biome：1项 plains 且 has_data=true（真NBT）。
    /// 来源同上；vanilla生物群系注册表名 `minecraft:worldgen/biome`，客户端强制校验时缺表会踢。
    pub fn minimal_biome() -> Self {
        Self {
            registry_id: "minecraft:worldgen/biome".into(),
            entries: vec![ConfigRegistryEntry { entry_id: "minecraft:plains".into(), data: Some(real_plains_biome_nbt()) }],
        }
    }
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.registry_id, out);
        write_varint(self.entries.len() as i32, out);
        for e in &self.entries { e.encode_body(out); }
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (registry_id, mut off) = decode_string(buf)?;
        let (n, m) = read_varint(&buf[off..])?; off += m;
        let mut entries = Vec::new();
        for _ in 0..n.max(0) {
            let (e, k) = ConfigRegistryEntry::decode_body(&buf[off..])?; off += k; entries.push(e);
        }
        Ok((Self { registry_id, entries }, off))
    }
    /// timeline 注册表：4 条目固定顺序 [day, early_game, moon, villager_schedule]（ID 0..3，供 Update Tags 引用）。
    /// NBT 按真机 data/timeline_*.json 经 json_value_to_nbt 忠实转换；clock/world_clock 引用保持字符串。
    pub fn minimal_timeline() -> Self {
        Self {
            registry_id: "minecraft:timeline".into(),
            entries: vec![
                ConfigRegistryEntry { entry_id: "minecraft:day".into(), data: Some(real_day_timeline_nbt()) },
                ConfigRegistryEntry { entry_id: "minecraft:early_game".into(), data: Some(real_early_game_timeline_nbt()) },
                ConfigRegistryEntry { entry_id: "minecraft:moon".into(), data: Some(real_moon_timeline_nbt()) },
                ConfigRegistryEntry { entry_id: "minecraft:villager_schedule".into(), data: Some(real_villager_schedule_timeline_nbt()) },
            ],
        }
    }
    /// world_clock 注册表：2 条目 [overworld, the_end]，空 compound（真数据 overworld.json/the_end.json 均为空对象）。
    /// dimension 的 default_clock="minecraft:overworld" 即指向此表第 0 项，缺它真机报缺 overworld 值。
    pub fn minimal_world_clock() -> Self {
        Self {
            registry_id: "minecraft:world_clock".into(),
            entries: vec![
                ConfigRegistryEntry { entry_id: "minecraft:overworld".into(), data: Some(empty_compound_packet_nbt()) },
                ConfigRegistryEntry { entry_id: "minecraft:the_end".into(), data: Some(empty_compound_packet_nbt()) },
            ],
        }
    }
}

/// 空 compound 包内 NBT（0x0A + 0x00；world_clock 真数据即空对象）。
fn empty_compound_packet_nbt() -> Vec<u8> {
    let tag = NbtTag::Compound(NbtCompound::new());
    let mut out = Vec::with_capacity(2);
    out.push(0x0A);
    tag.write_payload(&mut out);
    out
}

// timeline 真数据（vanilla data pack JSON，逐字节拷自真机 /tmp/p10-ref/，构建时 include 进二进制）。
const TIMELINE_DAY_JSON: &str = include_str!("data/timeline_day.json");
const TIMELINE_EARLY_GAME_JSON: &str = include_str!("data/timeline_early_game.json");
const TIMELINE_MOON_JSON: &str = include_str!("data/timeline_moon.json");
const TIMELINE_VILLAGER_SCHEDULE_JSON: &str = include_str!("data/timeline_villager_schedule.json");

/// data pack JSON → NBT 通用转换（timeline 注册表用）。
/// 类型规则（minecraft.wiki Timeline / Environment attribute 页）：
/// bool→Byte，整数→Int，小数→Float，字符串→String，数组→同构 List，对象→Compound；
/// 唯独 `cubic_bezier` 数组元素按 wiki 明确 [Double] 取 Double（其余小数属性均为 [Float]）。
/// JSON null 在真数据中不存在，遇到即跳过（对象键）/丢弃（数组元素），不静默编造值。
fn json_value_to_nbt(key: &str, v: &serde_json::Value) -> Option<NbtTag> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(NbtTag::Byte(*b as i8)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                debug_assert!(i >= i32::MIN as i64 && i <= i32::MAX as i64, "timeline int 超出 i32");
                Some(NbtTag::Int(i as i32))
            } else if let Some(u) = n.as_u64() {
                debug_assert!(u <= i32::MAX as u64, "timeline int 超出 i32");
                Some(NbtTag::Int(u as i32))
            } else {
                n.as_f64().map(|f| {
                    if key == "cubic_bezier" { NbtTag::Double(f) } else { NbtTag::Float(f as f32) }
                })
            }
        }
        serde_json::Value::String(s) => Some(NbtTag::String(s.clone())),
        serde_json::Value::Array(items) => {
            let mut tags = Vec::with_capacity(items.len());
            for it in items {
                // 数组元素自身无键名：cubic_bezier 数组内元素透传提示，保证 List(Double) 同构。
                let k = if key == "cubic_bezier" { "cubic_bezier" } else { "" };
                if let Some(t) = json_value_to_nbt(k, it) { tags.push(t); }
            }
            Some(NbtTag::List(NbtList::from_tags(tags).expect("timeline JSON 数组必须同构")))
        }
        serde_json::Value::Object(map) => {
            let mut c = NbtCompound::new();
            for (k, val) in map {
                if let Some(t) = json_value_to_nbt(k, val) { c.insert(k.clone(), t); }
            }
            Some(NbtTag::Compound(c))
        }
    }
}

/// 内嵌 timeline 真数据 JSON → 包内 NBT（0x0A + fields + 0x00）。
fn timeline_json_to_packet_nbt(json: &str) -> Vec<u8> {
    let v: serde_json::Value = serde_json::from_str(json).expect("内嵌 timeline 真数据必须合法 JSON");
    let tag = json_value_to_nbt("", &v).expect("timeline 根必须为对象");
    let mut out = Vec::with_capacity(2048);
    out.push(0x0A); // 包内NBT：根compound id，无根名，直接fields
    tag.write_payload(&mut out);
    out
}

/// day timeline 真 NBT（period 24000，clock `minecraft:overworld`，18 track，含 time_markers）。
pub fn real_day_timeline_nbt() -> Vec<u8> { timeline_json_to_packet_nbt(TIMELINE_DAY_JSON) }
/// early_game timeline 真 NBT（无 period，clock `minecraft:overworld`，单 track）。
pub fn real_early_game_timeline_nbt() -> Vec<u8> { timeline_json_to_packet_nbt(TIMELINE_EARLY_GAME_JSON) }
/// moon timeline 真 NBT（period 192000，clock `minecraft:overworld`，2 track）。
pub fn real_moon_timeline_nbt() -> Vec<u8> { timeline_json_to_packet_nbt(TIMELINE_MOON_JSON) }
/// villager_schedule timeline 真 NBT（period 24000，clock `minecraft:overworld`，2 track）。
pub fn real_villager_schedule_timeline_nbt() -> Vec<u8> { timeline_json_to_packet_nbt(TIMELINE_VILLAGER_SCHEDULE_JSON) }

/// Update Tags CB 包 ID（Configuration `update_tags`，tag 数据永不走 known packs，必须全量发送）。
/// 取值勘误：repo 旧文档记 0x0D（实为 26.1 值）；26.1 protocol.json 里 tags 已在 0x0D，
/// 26.2/26.3 新增 post_effects(0x0A) 使其后全体 +1，live wiki 777 明示 update_tags=0x0E、known_packs=0x0F（与本分支 0x0F 自洽）；
/// 0x0D 在 777 是 feature_flags，误用会撞包。26.4-snap 侧证 Update Tags 亦为 0x0E。
pub const CONFIG_UPDATE_TAGS_S2C_ID: i32 = 0x0E;

/// Update Tags 单 tag：tag 名（Identifier，不带 `#` 前缀）+ 条目数字 ID 数组（VarInt 数组，顺序即发送序）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUpdateTag { pub name: String, pub entries: Vec<i32> }

impl ConfigUpdateTag {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.name, out);
        write_varint(self.entries.len() as i32, out);
        for e in &self.entries { write_varint(*e, out); }
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (name, mut off) = decode_string(buf)?;
        let (n, m) = read_varint(&buf[off..])?; off += m;
        let mut entries = Vec::with_capacity(n.max(0) as usize);
        for _ in 0..n.max(0) {
            let (e, k) = read_varint(&buf[off..])?; off += k; entries.push(e);
        }
        Ok((Self { name, entries }, off))
    }
}

/// Update Tags 单注册表：registry 名 + 其下 tags 数组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigTaggedRegistry { pub registry_id: String, pub tags: Vec<ConfigUpdateTag> }

impl ConfigTaggedRegistry {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.registry_id, out);
        write_varint(self.tags.len() as i32, out);
        for t in &self.tags { t.encode_body(out); }
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (registry_id, mut off) = decode_string(buf)?;
        let (n, m) = read_varint(&buf[off..])?; off += m;
        let mut tags = Vec::with_capacity(n.max(0) as usize);
        for _ in 0..n.max(0) {
            let (t, k) = ConfigUpdateTag::decode_body(&buf[off..])?; off += k; tags.push(t);
        }
        Ok((Self { registry_id, tags }, off))
    }
}

/// Update Tags 整包：Tagged Registries 数组（registry String + tags[name + VarInt 数组 entryID]）。
/// 线格式来源 minecraft.wiki 777 `update_tags`（Configuration CB 0x0E；Play CB 0x89 同结构，本分支只发 Configuration）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUpdateTags { pub registries: Vec<ConfigTaggedRegistry> }

impl ConfigUpdateTags {
    /// timeline 注册表 4 个 tag（P10）。
    /// 条目 ID 依 minimal_timeline 固定顺序（day=0, early_game=1, moon=2, villager_schedule=3）：
    /// in_overworld 真数据 [#universal, day, moon, early_game] 展平为 [3,0,2,1]；universal/in_end/in_nether 真数据均归约到 [3]。
    pub fn timeline_registry() -> ConfigTaggedRegistry {
        ConfigTaggedRegistry {
            registry_id: "minecraft:timeline".into(),
            tags: vec![
                ConfigUpdateTag { name: "minecraft:in_overworld".into(), entries: vec![3, 0, 2, 1] },
                ConfigUpdateTag { name: "minecraft:universal".into(), entries: vec![3] },
                ConfigUpdateTag { name: "minecraft:in_end".into(), entries: vec![3] },
                ConfigUpdateTag { name: "minecraft:in_nether".into(), entries: vec![3] },
            ],
        }
    }
    /// block 注册表 tag 全量（P11）：dimension_type 的 `infiniburn` 是 TagKey，
    /// 客户端只用网络下发的 tag（本地数据包 tag 不会补），缺了就在 Finish 阶段报
    /// `Missing tag: 'minecraft:infiniburn_overworld' in 'minecraft:block'`。
    /// 数据来自 26.3 原版 `data/minecraft/tags/block/**`（递归展开 `#tag` 引用、去重），
    /// block 注册表 ID 来自 data generator 的 `reports/registries.json` protocol_id；
    /// 与 26.3 原版服务端空 KnownPacks 实测下发的 block tag（300 个、14825 项）逐项一致。
    /// 只发 block 与 timeline 两个注册表：其余注册表本服未下发条目，避免 tag 指向不存在的 ID。
    pub fn block_registry() -> ConfigTaggedRegistry {
        ConfigTaggedRegistry {
            registry_id: "minecraft:block".into(),
            tags: super::block_tags_777::BLOCK_TAGS_777
                .iter()
                .map(|(name, ids)| ConfigUpdateTag {
                    name: (*name).into(),
                    entries: ids.iter().map(|&id| id as i32).collect(),
                })
                .collect(),
        }
    }
    /// Configuration Update Tags 最小集：block（infiniburn）+ timeline（in_*）两个注册表。
    pub fn minimal_tags() -> Self {
        Self { registries: vec![Self::block_registry(), Self::timeline_registry()] }
    }
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.registries.len() as i32, out);
        for r in &self.registries { r.encode_body(out); }
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (n, mut off) = read_varint(buf)?;
        let mut registries = Vec::with_capacity(n.max(0) as usize);
        for _ in 0..n.max(0) {
            let (r, k) = ConfigTaggedRegistry::decode_body(&buf[off..])?; off += k; registries.push(r);
        }
        Ok((Self { registries }, off))
    }
}

/// Update Tags 完整线包。
pub fn encode_config_update_tags_packet(v: &ConfigUpdateTags, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body);
    encode_packet(CONFIG_UPDATE_TAGS_S2C_ID, &body, out);
}
pub fn decode_config_update_tags(p: &[u8]) -> Result<(ConfigUpdateTags, usize), ProtocolError> {
    ConfigUpdateTags::decode_body(p)
}

/// Configuration阶段最小下发表：dimension_type + biome + timeline + world_clock 各包（真NBT，has_data=true）。
/// 真机按索引查维度/生物群系（本服 Play Login dimension_type=0、区块 palette 用 biome 0）；
/// timeline 缺表则 dimension 引用的 `#minecraft:in_overworld` 悬空、world_clock 缺表则 default_clock 悬空，
/// 故 4 包 + Update Tags 后再 Finish（更多注册表后半按需补）。
pub fn default_configuration_registries() -> Vec<ConfigRegistryData> {
    vec![
        ConfigRegistryData::minimal_dimension_type(),
        ConfigRegistryData::minimal_biome(),
        ConfigRegistryData::minimal_timeline(),
        ConfigRegistryData::minimal_world_clock(),
    ]
}

/// Registry Data 完整线包。
pub fn encode_config_registry_data_packet(v: &ConfigRegistryData, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body);
    encode_packet(CONFIG_REGISTRY_DATA_S2C_ID, &body, out);
}
pub fn decode_config_registry_data(p: &[u8]) -> Result<(ConfigRegistryData, usize), ProtocolError> {
    ConfigRegistryData::decode_body(p)
}

// Known Packs：pack 列表（namespace/id/version 均为 String）。
// 线格式来源 minecraft.wiki 777 `select_known_packs`（CB 0x0F / SB 0x07）：VarInt条数 + 每项3个String。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigKnownPack { pub namespace: String, pub id: String, pub version: String }

impl ConfigKnownPack {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.namespace, out); encode_string(&self.id, out); encode_string(&self.version, out);
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (namespace, n1) = decode_string(buf)?;
        let (id, n2) = decode_string(&buf[n1..])?;
        let (version, n3) = decode_string(&buf[n1 + n2..])?;
        Ok((Self { namespace, id, version }, n1 + n2 + n3))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigKnownPacks { pub packs: Vec<ConfigKnownPack> }

impl ConfigKnownPacks {
    /// 空列表（客户端可接受；服务端S2C二选一，默认走 vanilla() 更贴近真服）。
    pub fn empty() -> Self { Self { packs: Vec::new() } }
    /// 单vanilla包：minecraft/core，version取MC版本26.3（见VERSION.md）。
    /// wiki 777字段同上；vanilla固定此包，客户端仅记录不强校验版本，空列表亦可，真服一般下发此单包。
    pub fn vanilla() -> Self {
        Self { packs: vec![ConfigKnownPack { namespace: "minecraft".into(), id: "core".into(), version: "26.3".into() }] }
    }
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.packs.len() as i32, out);
        for p in &self.packs { p.encode_body(out); }
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (n, mut off) = read_varint(buf)?;
        let mut packs = Vec::new();
        for _ in 0..n.max(0) {
            let (p, k) = ConfigKnownPack::decode_body(&buf[off..])?; off += k; packs.push(p);
        }
        Ok((Self { packs }, off))
    }
}

/// Known Packs S2C 完整线包。
pub fn encode_config_known_packs_s2c_packet(v: &ConfigKnownPacks, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body);
    encode_packet(CONFIG_KNOWN_PACKS_S2C_ID, &body, out);
}
pub fn decode_config_known_packs_s2c(p: &[u8]) -> Result<(ConfigKnownPacks, usize), ProtocolError> {
    ConfigKnownPacks::decode_body(p)
}

/// Known Packs C2S 完整线包（服务端主要做 decode，encode 供单测/模拟客户端用）。
pub fn encode_config_known_packs_c2s_packet(v: &ConfigKnownPacks, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body);
    encode_packet(CONFIG_KNOWN_PACKS_C2S_ID, &body, out);
}
pub fn decode_config_known_packs_c2s(p: &[u8]) -> Result<(ConfigKnownPacks, usize), ProtocolError> {
    ConfigKnownPacks::decode_body(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::codec::decode_packet_id;
    fn packet_id_of(pkt: &[u8]) -> (i32, usize) {
        let (_, n) = read_varint(pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        (id, n + m)
    }
    #[test]
    fn config_finish_roundtrip() {
        let mut s2c = Vec::new(); encode_config_finish_s2c_packet(&mut s2c);
        let (id, n) = packet_id_of(&s2c); assert_eq!(id, CONFIG_FINISH_S2C_ID);
        decode_config_finish_s2c(&s2c[n..]).unwrap();
        let mut ack = Vec::new(); encode_config_finish_ack_packet(&mut ack);
        let (aid, m) = packet_id_of(&ack); assert_eq!(aid, CONFIG_FINISH_ACK_C2S_ID);
        decode_config_finish_ack(&ack[m..]).unwrap();
        assert!(!config_registry_skipped(), "P6a起下发最小registry，不再跳过");
    }
    #[test]
    fn config_ids_match_p4a_map() {
        assert_eq!(CONFIG_FINISH_S2C_ID, 0x03);
        assert_eq!(CONFIG_FINISH_ACK_C2S_ID, 0x03);
        assert_eq!(CONFIG_REGISTRY_DATA_S2C_ID, 0x07);
        assert_eq!(CONFIG_KNOWN_PACKS_S2C_ID, 0x0F);
        assert_eq!(CONFIG_KNOWN_PACKS_C2S_ID, 0x07);
        // Update Tags：777 取 0x0E（26.1 的 0x0D 在 777 是 feature_flags，见常量注释勘误）。
        assert_eq!(CONFIG_UPDATE_TAGS_S2C_ID, 0x0E);
    }
    #[test]
    fn config_registry_placeholder_roundtrip() {
        let v = ConfigRegistryData {
            registry_id: "minecraft:dimension_type".into(),
            entries: vec![
                ConfigRegistryEntry { entry_id: "minecraft:overworld".into(), data: None },
                ConfigRegistryEntry { entry_id: "minecraft:the_nether".into(), data: Some(real_plains_biome_nbt()) },
            ],
        };
        let mut pkt = Vec::new(); encode_config_registry_data_packet(&v, &mut pkt);
        let (id, n) = packet_id_of(&pkt); assert_eq!(id, CONFIG_REGISTRY_DATA_S2C_ID);
        assert_eq!(decode_config_registry_data(&pkt[n..]).unwrap().0, v);
        let empty = ConfigRegistryData::empty("minecraft:dimension_type");
        assert!(empty.entries.is_empty());
        let mut e = Vec::new(); empty.encode_body(&mut e);
        assert_eq!(ConfigRegistryData::decode_body(&e).unwrap().0, empty);
        assert!(ConfigRegistryData::decode_body(&e[..e.len().saturating_sub(1).max(1)]).is_err()
            || ConfigRegistryData::decode_body(&[]).is_err());
    }
    #[test]
    fn config_minimal_registries_stand() {
        let regs = default_configuration_registries();
        assert_eq!(regs.len(), 4);
        assert_eq!(regs[0].registry_id, "minecraft:dimension_type");
        assert_eq!(regs[1].registry_id, "minecraft:worldgen/biome");
        assert_eq!(regs[2].registry_id, "minecraft:timeline");
        assert_eq!(regs[3].registry_id, "minecraft:world_clock");
        for r in &regs {
            assert!(!r.entries.is_empty());
            assert!(r.entries.iter().all(|e| e.data.is_some()), "最小下发has_data=true带真NBT");
            let mut pkt = Vec::new(); encode_config_registry_data_packet(r, &mut pkt);
            let (id, n) = packet_id_of(&pkt); assert_eq!(id, CONFIG_REGISTRY_DATA_S2C_ID);
            assert_eq!(decode_config_registry_data(&pkt[n..]).unwrap().0, *r);
        }
        assert_eq!(regs[0].entries[0].entry_id, "minecraft:overworld");
        assert_eq!(regs[1].entries[0].entry_id, "minecraft:plains");
        // timeline 固定顺序 [day, early_game, moon, villager_schedule]（ID 0..3，供 tags 引用）。
        let tl_ids: Vec<&str> = regs[2].entries.iter().map(|e| e.entry_id.as_str()).collect();
        assert_eq!(tl_ids, ["minecraft:day", "minecraft:early_game", "minecraft:moon", "minecraft:villager_schedule"]);
        // world_clock 固定 [overworld, the_end]。
        let wc_ids: Vec<&str> = regs[3].entries.iter().map(|e| e.entry_id.as_str()).collect();
        assert_eq!(wc_ids, ["minecraft:overworld", "minecraft:the_end"]);
    }
    #[test]
    fn config_real_nbt_shape_and_roundtrip() {
        use super::super::nbt::{NbtTag, TAG_END, read_nbt_compound_payload};
        let dim = real_overworld_dimension_nbt();
        let bio = real_plains_biome_nbt();
        for nbt in [&dim, &bio] {
            assert!(!nbt.is_empty(), "NBT非空");
            assert_eq!(nbt[0], 0x0A, "NBT以TAG_Compound 0x0A开头");
            assert_eq!(nbt[nbt.len() - 1], 0x00, "NBT以TAG_End结尾");
            // 包内格式自定界：0x0A后compound payload恰好消费到尾（无根名、无长度前缀）
            let (_, used) = read_nbt_compound_payload(&nbt[1..]).unwrap();
            assert_eq!(1 + used, nbt.len(), "NBT payload必须字节对齐消费");
        }
        // 维度：§2.1全字段结构化断言（新字段存在，7个已删除旧字段不存在）
        let (dim_c, _) = read_nbt_compound_payload(&dim[1..]).unwrap();
        assert_eq!(dim_c.get("has_skylight"), Some(&NbtTag::Byte(1)));
        assert_eq!(dim_c.get("has_ceiling"), Some(&NbtTag::Byte(0)));
        assert_eq!(dim_c.get("has_ender_dragon_fight"), Some(&NbtTag::Byte(0)));
        assert_eq!(dim_c.get("coordinate_scale"), Some(&NbtTag::Double(1.0)));
        assert_eq!(dim_c.get("ambient_light"), Some(&NbtTag::Float(0.0)));
        assert_eq!(dim_c.get("monster_spawn_block_light_limit"), Some(&NbtTag::Int(0)));
        // monster_spawn_light_level：真数据 int-provider 对象 {type, min_inclusive, max_inclusive}。
        let msl = dim_c.get("monster_spawn_light_level").expect("维度缺monster_spawn_light_level");
        let NbtTag::Compound(msl_c) = msl else { panic!("monster_spawn_light_level须为Compound对象") };
        assert_eq!(msl_c.get("type"), Some(&NbtTag::String("minecraft:uniform".into())));
        assert_eq!(msl_c.get("min_inclusive"), Some(&NbtTag::Int(0)));
        assert_eq!(msl_c.get("max_inclusive"), Some(&NbtTag::Int(7)));
        assert_eq!(dim_c.get("logical_height"), Some(&NbtTag::Int(384)));
        assert_eq!(dim_c.get("min_y"), Some(&NbtTag::Int(-64)));
        assert_eq!(dim_c.get("height"), Some(&NbtTag::Int(384)));
        assert_eq!(dim_c.get("infiniburn"), Some(&NbtTag::String("#minecraft:infiniburn_overworld".into())));
        assert_eq!(dim_c.get("default_clock"), Some(&NbtTag::String("minecraft:overworld".into())));
        for gone in ["ultrawarm", "natural", "bed_works", "respawn_anchor_works", "piglin_safe", "has_raids", "effects", "has_fixed_time", "skybox", "cardinal_light"] {
            assert_eq!(dim_c.get(gone), None, "维度旧字段已删:{gone}");
        }
        // timelines：字符串 tag 引用（维度引 timeline 注册表即靠它）。
        assert_eq!(dim_c.get("timelines"), Some(&NbtTag::String("#minecraft:in_overworld".into())));
        // attributes：整棵存在，类型逐项断言（wiki Environment_attribute 页）。
        let attrs = dim_c.get("attributes").expect("维度缺attributes");
        let NbtTag::Compound(attrs_c) = attrs else { panic!("attributes须为Compound") };
        assert_eq!(attrs_c.len(), 11, "attributes须为真数据11项");
        let mood = attrs_c.get("minecraft:audio/ambient_sounds").expect("缺ambient_sounds");
        let NbtTag::Compound(mood_c) = mood else { panic!("ambient_sounds须为Compound") };
        let NbtTag::Compound(m) = mood_c.get("mood").expect("缺mood") else { panic!("mood须为Compound") };
        assert_eq!(m.get("block_search_extent"), Some(&NbtTag::Int(8)));
        assert_eq!(m.get("offset"), Some(&NbtTag::Float(2.0)), "offset与原版JSON字面2.0一致取Float");
        assert_eq!(m.get("sound"), Some(&NbtTag::String("minecraft:ambient.cave".into())));
        assert_eq!(m.get("tick_delay"), Some(&NbtTag::Int(6000)));
        let NbtTag::Compound(_) = attrs_c.get("minecraft:audio/background_music").expect("缺background_music")
            else { panic!("background_music须为Compound") };
        let NbtTag::Compound(bed) = attrs_c.get("minecraft:gameplay/bed_rule").expect("缺bed_rule")
            else { panic!("bed_rule须为Compound") };
        assert_eq!(bed.get("can_sleep"), Some(&NbtTag::String("when_dark".into())));
        assert_eq!(attrs_c.get("minecraft:gameplay/nether_portal_spawns_piglin"), Some(&NbtTag::Byte(1)));
        assert_eq!(attrs_c.get("minecraft:gameplay/respawn_anchor_works"), Some(&NbtTag::Byte(0)));
        assert_eq!(attrs_c.get("minecraft:visual/ambient_light_color"), Some(&NbtTag::String("#0a0a0a".into())));
        assert_eq!(attrs_c.get("minecraft:visual/cloud_color"), Some(&NbtTag::String("#ccffffff".into())));
        assert_eq!(attrs_c.get("minecraft:visual/cloud_height"), Some(&NbtTag::Float(192.33)));
        assert_eq!(attrs_c.get("minecraft:visual/sky_color"), Some(&NbtTag::String("#78a7ff".into())));
        // plains：§2.2全字段结构化断言（新字段存在，3个已删除旧色值不存在）
        let (bio_c, _) = read_nbt_compound_payload(&bio[1..]).unwrap();
        assert_eq!(bio_c.get("has_precipitation"), Some(&NbtTag::Byte(1)));
        assert_eq!(bio_c.get("temperature"), Some(&NbtTag::Float(0.8)));
        assert_eq!(bio_c.get("temperature_modifier"), Some(&NbtTag::String("none".into())));
        assert_eq!(bio_c.get("downfall"), Some(&NbtTag::Float(0.4)));
        let eff = bio_c.get("effects").expect("plains缺effects");
        let NbtTag::Compound(eff_c) = eff else { panic!("effects须为Compound") };
        assert_eq!(eff_c.get("water_color"), Some(&NbtTag::Int(4_159_204)));
        for gone in ["sky_color", "fog_color", "water_fog_color"] {
            assert_eq!(eff_c.get(gone), None, "biome旧色值已删:{gone}");
        }
        for key in ["carvers", "features"] {
            let l = bio_c.get(key).unwrap_or_else(|| panic!("plains缺{key}"));
            let NbtTag::List(l) = l else { panic!("{key}须为List") };
            assert!(l.is_empty(), "{key}须为空list");
            assert_eq!(l.elem_type(), TAG_END, "{key}空list元素类型按惯例为TAG_End");
        }
        // 往返 decode_body 通过（含包头线包与纯body两层）
        for (reg_id, entry_id, nbt) in [
            ("minecraft:dimension_type", "minecraft:overworld", dim.clone()),
            ("minecraft:worldgen/biome", "minecraft:plains", bio.clone()),
        ] {
            let v = ConfigRegistryData {
                registry_id: reg_id.into(),
                entries: vec![ConfigRegistryEntry { entry_id: entry_id.into(), data: Some(nbt.clone()) }],
            };
            let mut body = Vec::new(); v.encode_body(&mut body);
            assert_eq!(ConfigRegistryData::decode_body(&body).unwrap().0, v);
            let mut pkt = Vec::new(); encode_config_registry_data_packet(&v, &mut pkt);
            let (id, n) = packet_id_of(&pkt); assert_eq!(id, CONFIG_REGISTRY_DATA_S2C_ID);
            assert_eq!(decode_config_registry_data(&pkt[n..]).unwrap().0, v);
            // entry级往返
            let mut eb = Vec::new();
            v.entries[0].encode_body(&mut eb);
            assert_eq!(ConfigRegistryEntry::decode_body(&eb).unwrap().0.data.unwrap(), nbt);
        }
    }
    #[test]
    fn config_timeline_nbt_faithful_and_roundtrip() {
        use super::super::nbt::read_nbt_compound_payload;
        // 4 条目 NBT 非空、包内格式自定界、clock 均为字符串 minecraft:overworld。
        let nbts = [
            ("minecraft:day", real_day_timeline_nbt()),
            ("minecraft:early_game", real_early_game_timeline_nbt()),
            ("minecraft:moon", real_moon_timeline_nbt()),
            ("minecraft:villager_schedule", real_villager_schedule_timeline_nbt()),
        ];
        for (id, nbt) in &nbts {
            assert!(!nbt.is_empty(), "{id} NBT非空");
            assert_eq!(nbt[0], 0x0A, "{id} 以TAG_Compound开头");
            assert_eq!(nbt[nbt.len() - 1], 0x00, "{id} 以TAG_End结尾");
            let (_, used) = read_nbt_compound_payload(&nbt[1..]).unwrap();
            assert_eq!(1 + used, nbt.len(), "{id} payload字节对齐");
            let (c, _) = read_nbt_compound_payload(&nbt[1..]).unwrap();
            assert_eq!(c.get("clock"), Some(&NbtTag::String("minecraft:overworld".into())), "{id} clock须为字符串");
        }
        // day：period 24000 + 18 track + time_markers 混合 Int/Compound。
        let (day_c, _) = read_nbt_compound_payload(&nbts[0].1[1..]).unwrap();
        assert_eq!(day_c.get("period_ticks"), Some(&NbtTag::Int(24000)));
        let NbtTag::Compound(tracks) = day_c.get("tracks").expect("day缺tracks") else { panic!("tracks须为Compound") };
        assert_eq!(tracks.len(), 18, "day须为真数据18 track");
        let NbtTag::Compound(markers) = day_c.get("time_markers").expect("day缺time_markers")
            else { panic!("time_markers须为Compound") };
        assert_eq!(markers.get("minecraft:roll_village_siege"), Some(&NbtTag::Int(18000)), "marker Int形式保留");
        let NbtTag::Compound(noon) = markers.get("minecraft:noon").expect("缺noon marker")
            else { panic!("noon marker须为Compound") };
        assert_eq!(noon.get("ticks"), Some(&NbtTag::Int(6000)));
        assert_eq!(noon.get("show_in_commands"), Some(&NbtTag::Byte(1)));
        // ease 字符串与 cubic_bezier 对象并存；cubic_bezier 元素须为 Double（wiki明确[Double]）。
        let NbtTag::Compound(sun) = tracks.get("minecraft:visual/sun_angle").expect("缺sun_angle track")
            else { panic!("track须为Compound") };
        let NbtTag::Compound(ease) = sun.get("ease").expect("sun_angle缺ease") else { panic!("ease对象须为Compound") };
        let NbtTag::List(cb) = ease.get("cubic_bezier").expect("缺cubic_bezier") else { panic!("cubic_bezier须为List") };
        assert_eq!(cb.len(), 4);
        assert_eq!(cb.elem_type(), super::super::nbt::TAG_DOUBLE, "cubic_bezier须为Double列表");
        let NbtTag::Compound(bees) = tracks.get("minecraft:gameplay/bees_stay_in_hive").expect("缺bees track")
            else { panic!("track须为Compound") };
        assert_eq!(bees.get("modifier"), Some(&NbtTag::String("or".into())), "modifier字符串保留");
        assert_eq!(bees.get("ease"), None, "缺省ease即linear，不发键");
        let NbtTag::Compound(cat) = tracks.get("minecraft:gameplay/cat_waking_up_gift_chance").expect("缺cat track")
            else { panic!("track须为Compound") };
        assert_eq!(cat.get("ease"), Some(&NbtTag::String("constant".into())), "ease字符串形式保留");
        // keyframe value 异构：bool→Byte / float→Float / string→String。
        let NbtTag::List(kf) = bees.get("keyframes").expect("缺keyframes") else { panic!("keyframes须为List") };
        assert_eq!(kf.len(), 2);
        let mut it = kf.iter();
        let NbtTag::Compound(k0) = it.next().unwrap() else { panic!("keyframe须为Compound") };
        assert_eq!(k0.get("ticks"), Some(&NbtTag::Int(12542)));
        assert_eq!(k0.get("value"), Some(&NbtTag::Byte(1)));
        // moon：period 192000 + moon_phase 字符串 value。
        let (moon_c, _) = read_nbt_compound_payload(&nbts[2].1[1..]).unwrap();
        assert_eq!(moon_c.get("period_ticks"), Some(&NbtTag::Int(192000)));
        let NbtTag::Compound(mtracks) = moon_c.get("tracks").expect("moon缺tracks") else { panic!("tracks须为Compound") };
        assert_eq!(mtracks.len(), 2);
        // early_game：无 period（缺省不重复）+ 单 track bool keyframe。
        let (eg_c, _) = read_nbt_compound_payload(&nbts[1].1[1..]).unwrap();
        assert_eq!(eg_c.get("period_ticks"), None, "early_game无period_ticks");
        let NbtTag::Compound(eg_t) = eg_c.get("tracks").expect("early_game缺tracks") else { panic!("tracks须为Compound") };
        assert_eq!(eg_t.len(), 1);
        // villager_schedule：period 24000 + 2 track + 字符串 value。
        let (vs_c, _) = read_nbt_compound_payload(&nbts[3].1[1..]).unwrap();
        assert_eq!(vs_c.get("period_ticks"), Some(&NbtTag::Int(24000)));
        let NbtTag::Compound(vs_t) = vs_c.get("tracks").expect("villager缺tracks") else { panic!("tracks须为Compound") };
        assert_eq!(vs_t.len(), 2);
        // 注册表往返：顺序固定 [day, early_game, moon, villager_schedule]。
        let reg = ConfigRegistryData::minimal_timeline();
        assert_eq!(reg.entries.len(), 4);
        let mut body = Vec::new(); reg.encode_body(&mut body);
        assert_eq!(ConfigRegistryData::decode_body(&body).unwrap().0, reg);
        let mut pkt = Vec::new(); encode_config_registry_data_packet(&reg, &mut pkt);
        let (id, n) = packet_id_of(&pkt); assert_eq!(id, CONFIG_REGISTRY_DATA_S2C_ID);
        assert_eq!(decode_config_registry_data(&pkt[n..]).unwrap().0, reg);
    }
    #[test]
    fn config_world_clock_empty_compound_roundtrip() {
        use super::super::nbt::read_nbt_compound_payload;
        // 空 compound 包内字节：0x0A 0x00。
        assert_eq!(empty_compound_packet_nbt(), vec![0x0A, 0x00]);
        let reg = ConfigRegistryData::minimal_world_clock();
        assert_eq!(reg.registry_id, "minecraft:world_clock");
        assert_eq!(reg.entries.len(), 2);
        for e in &reg.entries {
            let d = e.data.as_ref().expect("world_clock条目须带数据");
            assert_eq!(d, &vec![0x0A, 0x00], "world_clock真数据为空对象");
            let (c, used) = read_nbt_compound_payload(&d[1..]).unwrap();
            assert!(c.is_empty());
            assert_eq!(1 + used, d.len());
        }
        let mut pkt = Vec::new(); encode_config_registry_data_packet(&reg, &mut pkt);
        let (id, n) = packet_id_of(&pkt); assert_eq!(id, CONFIG_REGISTRY_DATA_S2C_ID);
        assert_eq!(decode_config_registry_data(&pkt[n..]).unwrap().0, reg);
    }
    #[test]
    fn config_update_tags_codec_and_minimal_set() {
        // 最小集：block（原版全量 300 tag）+ timeline（4 tag）。
        let v = ConfigUpdateTags::minimal_tags();
        assert_eq!(v.registries.len(), 2);
        assert_eq!(v.registries[0].registry_id, "minecraft:block");
        assert_eq!(v.registries[0].tags.len(), crate::packets::block_tags_777::BLOCK_TAGS_777.len());
        assert_eq!(v.registries[0].tags.len(), 300);
        let infiniburn = |name: &str| -> Vec<i32> {
            v.registries[0].tags.iter().find(|t| t.name == name).expect("infiniburn tag").entries.clone()
        };
        assert_eq!(infiniburn("minecraft:infiniburn_overworld"), vec![334, 729]);
        assert_eq!(infiniburn("minecraft:infiniburn_nether"), vec![334, 729]);
        assert_eq!(infiniburn("minecraft:infiniburn_end"), vec![334, 729, 36]);
        // timeline 注册表 4 tag，ID 依 [day=0, early_game=1, moon=2, villager_schedule=3] 固定顺序。
        assert_eq!(v.registries[1].registry_id, "minecraft:timeline");
        let expect: Vec<(&str, Vec<i32>)> = vec![
            ("minecraft:in_overworld", vec![3, 0, 2, 1]),
            ("minecraft:universal", vec![3]),
            ("minecraft:in_end", vec![3]),
            ("minecraft:in_nether", vec![3]),
        ];
        assert_eq!(v.registries[1].tags.len(), expect.len());
        for (tag, (name, ids)) in v.registries[1].tags.iter().zip(expect.iter()) {
            assert_eq!(&tag.name, name);
            assert_eq!(&tag.entries, ids);
        }
        // body 往返 + 整包往返（包 ID 0x0E）。
        let mut body = Vec::new(); v.encode_body(&mut body);
        assert_eq!(ConfigUpdateTags::decode_body(&body).unwrap().0, v);
        assert!(ConfigUpdateTags::decode_body(&[]).is_err());
        let mut pkt = Vec::new(); encode_config_update_tags_packet(&v, &mut pkt);
        let (id, n) = packet_id_of(&pkt); assert_eq!(id, CONFIG_UPDATE_TAGS_S2C_ID);
        assert_eq!(decode_config_update_tags(&pkt[n..]).unwrap().0, v);
        // 空集往返（不发，单测编解码）。
        let empty = ConfigUpdateTags { registries: Vec::new() };
        let mut eb = Vec::new(); empty.encode_body(&mut eb);
        assert_eq!(ConfigUpdateTags::decode_body(&eb).unwrap().0, empty);
    }
    /// 26.3 原版服务端（空 KnownPacks）实测 update_tags 0x0E 包体（58754B）：
    /// 只比对本服实际下发的 block/timeline 两个注册表，tag 顺序按名字归一化。
    #[test]
    fn config_update_tags_match_vanilla_777_capture() {
        let (cap, used) = decode_config_update_tags(include_bytes!("data/update_tags_777.bin")).unwrap();
        assert_eq!(used, 58_754);
        fn sorted(mut tags: Vec<ConfigUpdateTag>) -> Vec<ConfigUpdateTag> {
            tags.sort_by(|a, b| a.name.cmp(&b.name));
            tags
        }
        let cap_block = cap.registries.iter().find(|r| r.registry_id == "minecraft:block").unwrap();
        assert_eq!(sorted(cap_block.tags.clone()), sorted(ConfigUpdateTags::block_registry().tags));
        let cap_timeline = cap.registries.iter().find(|r| r.registry_id == "minecraft:timeline").unwrap();
        assert_eq!(sorted(cap_timeline.tags.clone()), sorted(ConfigUpdateTags::timeline_registry().tags));
    }
    #[test]
    fn config_known_packs_placeholder_roundtrip() {
        let v = ConfigKnownPacks {
            packs: vec![ConfigKnownPack { namespace: "minecraft".into(), id: "core".into(), version: "1.0".into() }],
        };
        let mut s2c = Vec::new(); encode_config_known_packs_s2c_packet(&v, &mut s2c);
        let (id, n) = packet_id_of(&s2c); assert_eq!(id, CONFIG_KNOWN_PACKS_S2C_ID);
        assert_eq!(decode_config_known_packs_s2c(&s2c[n..]).unwrap().0, v);
        let mut c2s = Vec::new(); encode_config_known_packs_c2s_packet(&v, &mut c2s);
        let (cid, m) = packet_id_of(&c2s); assert_eq!(cid, CONFIG_KNOWN_PACKS_C2S_ID);
        assert_eq!(decode_config_known_packs_c2s(&c2s[m..]).unwrap().0, v);
        let empty = ConfigKnownPacks::empty();
        let mut b = Vec::new(); empty.encode_body(&mut b);
        assert!(ConfigKnownPacks::decode_body(&b).unwrap().0.packs.is_empty());
        // vanilla单包：线包ID与字段与777一致
        let vanilla = ConfigKnownPacks::vanilla();
        assert_eq!(vanilla.packs.len(), 1);
        assert_eq!(vanilla.packs[0].namespace, "minecraft");
        assert_eq!(vanilla.packs[0].id, "core");
        let mut vp = Vec::new(); encode_config_known_packs_s2c_packet(&vanilla, &mut vp);
        let (vid, vn) = packet_id_of(&vp); assert_eq!(vid, CONFIG_KNOWN_PACKS_S2C_ID);
        assert_eq!(decode_config_known_packs_s2c(&vp[vn..]).unwrap().0, vanilla);
    }
}
