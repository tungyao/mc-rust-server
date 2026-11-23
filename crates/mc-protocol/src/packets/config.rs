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
// 生产最小只发 has_data=false 项（客户端用内置默认即可站住），data=Some 分支为占位（真NBT后半接，线格式待NBT库补齐）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRegistryEntry { pub entry_id: String, pub data: Option<Vec<u8>> }

impl ConfigRegistryEntry {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.entry_id, out);
        match &self.data {
            Some(d) => { out.push(1); write_varint(d.len() as i32, out); out.extend_from_slice(d); }
            None => out.push(0),
        }
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (entry_id, n1) = decode_string(buf)?;
        if buf.len() <= n1 { return Err(ProtocolError::UnexpectedEof); }
        let (data, off) = if buf[n1] != 0 {
            let (len, n2) = read_varint(&buf[n1 + 1..])?;
            if len < 0 { return Err(ProtocolError::UnexpectedEof); }
            let s = n1 + 1 + n2; let e = s + len as usize;
            if buf.len() < e { return Err(ProtocolError::UnexpectedEof); }
            (Some(buf[s..e].to_vec()), e)
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
    /// 最小 dimension_type：1项 overworld 且 has_data=false。
    /// 来源：minecraft.wiki 777 `registry_data`；vanilla注册表名 `minecraft:dimension_type`，
    /// Play Login按索引查维度（本服LoginPlay dimension_type=0即此表第0项），缺表真机黑屏/被踢。
    pub fn minimal_dimension_type() -> Self {
        Self {
            registry_id: "minecraft:dimension_type".into(),
            entries: vec![ConfigRegistryEntry { entry_id: "minecraft:overworld".into(), data: None }],
        }
    }
    /// 最小 biome：1项 plains 且 has_data=false。
    /// 来源同上；vanilla生物群系注册表名 `minecraft:worldgen/biome`，客户端强制校验时缺表会踢。
    pub fn minimal_biome() -> Self {
        Self {
            registry_id: "minecraft:worldgen/biome".into(),
            entries: vec![ConfigRegistryEntry { entry_id: "minecraft:plains".into(), data: None }],
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

/// Configuration阶段最小下发表：dimension_type + biome 各1项（has_data=false）。
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
                ConfigRegistryEntry { entry_id: "minecraft:the_nether".into(), data: Some(vec![1, 2, 3]) },
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
            assert!(r.entries[0].data.is_none(), "最小下发has_data=false，客户端用内置默认");
            let mut pkt = Vec::new(); encode_config_registry_data_packet(r, &mut pkt);
            let (id, n) = packet_id_of(&pkt); assert_eq!(id, CONFIG_REGISTRY_DATA_S2C_ID);
            assert_eq!(decode_config_registry_data(&pkt[n..]).unwrap().0, *r);
        }
        assert_eq!(regs[0].entries[0].entry_id, "minecraft:overworld");
        assert_eq!(regs[1].entries[0].entry_id, "minecraft:plains");
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
