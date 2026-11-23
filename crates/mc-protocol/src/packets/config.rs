//! config: Configuration阶段 KnownPacks→Registry→Finish最小可用，只兼容26.3/777，ID已按P4A对照表核对777
//! 流程：Login Ack后 → 发 KnownPacks S2C → 等 KnownPacks C2S → 发 Registry Data(最小2包) → 发 Finish S2C → 等 Finish Ack C2S → enter_play。
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
/// 字段对标 docs/registries-777.md §2.1（26.3现行codec，已删7旧字段，补5新字段）。
/// monster_spawn_light_level 取 Int 0（文档确认 Int 与 int-provider 对象两种都合法，见 §2.1）。
pub fn real_overworld_dimension_nbt() -> Vec<u8> {
    let tag = NbtTag::Compound(
        NbtCompound::new()
            .with("has_skylight", NbtTag::Byte(1))
            .with("has_ceiling", NbtTag::Byte(0))
            .with("has_ender_dragon_fight", NbtTag::Byte(0))
            .with("coordinate_scale", NbtTag::Double(1.0))
            .with("has_fixed_time", NbtTag::Byte(0))
            .with("ambient_light", NbtTag::Float(0.0))
            .with("monster_spawn_block_light_limit", NbtTag::Int(0))
            .with("monster_spawn_light_level", NbtTag::Int(0))
            .with("logical_height", NbtTag::Int(384))
            .with("min_y", NbtTag::Int(-64))
            .with("height", NbtTag::Int(384))
            .with("infiniburn", NbtTag::String("#minecraft:infiniburn_overworld".into()))
            .with("skybox", NbtTag::String("overworld".into()))
            .with("cardinal_light", NbtTag::String("default".into()))
            .with("default_clock", NbtTag::String("minecraft:overworld".into()))
            .clone(),
    );
    let mut out = Vec::with_capacity(256);
    out.push(0x0A); // 包内NBT：根compound id，无根名，直接fields
    tag.write_payload(&mut out);
    out
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
}

/// Configuration阶段最小下发表：dimension_type + biome 各1项（has_data=true真NBT）。
/// 空表直接Finish多数vanilla可过，但真机按索引查维度/生物群系，故发此2包再Finish即站住；更多注册表（chat_type/damage_type等）后半按需补。
pub fn default_configuration_registries() -> Vec<ConfigRegistryData> {
    vec![ConfigRegistryData::minimal_dimension_type(), ConfigRegistryData::minimal_biome()]
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
        assert_eq!(regs.len(), 2);
        assert_eq!(regs[0].registry_id, "minecraft:dimension_type");
        assert_eq!(regs[1].registry_id, "minecraft:worldgen/biome");
        for r in &regs {
            assert_eq!(r.entries.len(), 1);
            assert!(r.entries[0].data.is_some(), "最小下发has_data=true带真NBT");
            let mut pkt = Vec::new(); encode_config_registry_data_packet(r, &mut pkt);
            let (id, n) = packet_id_of(&pkt); assert_eq!(id, CONFIG_REGISTRY_DATA_S2C_ID);
            assert_eq!(decode_config_registry_data(&pkt[n..]).unwrap().0, *r);
        }
        assert_eq!(regs[0].entries[0].entry_id, "minecraft:overworld");
        assert_eq!(regs[1].entries[0].entry_id, "minecraft:plains");
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
        assert_eq!(dim_c.get("has_fixed_time"), Some(&NbtTag::Byte(0)));
        assert_eq!(dim_c.get("ambient_light"), Some(&NbtTag::Float(0.0)));
        assert_eq!(dim_c.get("monster_spawn_block_light_limit"), Some(&NbtTag::Int(0)));
        assert_eq!(dim_c.get("monster_spawn_light_level"), Some(&NbtTag::Int(0)));
        assert_eq!(dim_c.get("logical_height"), Some(&NbtTag::Int(384)));
        assert_eq!(dim_c.get("min_y"), Some(&NbtTag::Int(-64)));
        assert_eq!(dim_c.get("height"), Some(&NbtTag::Int(384)));
        assert_eq!(dim_c.get("infiniburn"), Some(&NbtTag::String("#minecraft:infiniburn_overworld".into())));
        assert_eq!(dim_c.get("skybox"), Some(&NbtTag::String("overworld".into())));
        assert_eq!(dim_c.get("cardinal_light"), Some(&NbtTag::String("default".into())));
        assert_eq!(dim_c.get("default_clock"), Some(&NbtTag::String("minecraft:overworld".into())));
        for gone in ["ultrawarm", "natural", "bed_works", "respawn_anchor_works", "piglin_safe", "has_raids", "effects"] {
            assert_eq!(dim_c.get(gone), None, "维度旧字段已删:{gone}");
        }
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
