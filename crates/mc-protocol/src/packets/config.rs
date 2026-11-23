//! config: Configuration阶段 Finish/Ack + Registry/KnownPacks占位，只兼容26.3/777，ID已按P4A对照表核对777
//! 流程（当前骨架）：Login Ack 后直接发 Finish S2C → 等 Finish Ack C2S → enter_play。
//! Registry Data / Known Packs 暂不发送（空 registry 直接 Finish），只留 encode/decode 占位，ID已按P4A对照表核对，字段布局后半再接。
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

/// 当前是否跳过 Registry 下发：空 registry 直接 Finish。P4A 接真 registry 后改成 false 并按表下发。
pub fn config_registry_skipped() -> bool { true }

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

// Registry Data 占位：registry_id + 条目表。ID已按P4A对照表核对（CB 0x07），真实NBT/条目布局后半再接，
// 当前骨架不发送（空 registry 直接 Finish），只留 encode/decode 供后半接。
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
    /// 空 registry 构造（当前默认：不下发，直接 Finish）。
    pub fn empty(registry_id: impl Into<String>) -> Self {
        Self { registry_id: registry_id.into(), entries: Vec::new() }
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

/// Registry Data 完整线包（占位，当前不发送；encode/decode 供单测与后半接）。
pub fn encode_config_registry_data_packet(v: &ConfigRegistryData, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body);
    encode_packet(CONFIG_REGISTRY_DATA_S2C_ID, &body, out);
}
pub fn decode_config_registry_data(p: &[u8]) -> Result<(ConfigRegistryData, usize), ProtocolError> {
    ConfigRegistryData::decode_body(p)
}

// Known Packs 占位：pack 列表（namespace/id/version 均为 String）。
// ID已按P4A对照表核对（CB 0x0F / SB 0x07），当前骨架不发送，只留 encode/decode。
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

/// Known Packs S2C 完整线包（占位用）。
pub fn encode_config_known_packs_s2c_packet(v: &ConfigKnownPacks, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body);
    encode_packet(CONFIG_KNOWN_PACKS_S2C_ID, &body, out);
}
pub fn decode_config_known_packs_s2c(p: &[u8]) -> Result<(ConfigKnownPacks, usize), ProtocolError> {
    ConfigKnownPacks::decode_body(p)
}

/// Known Packs C2S 完整线包（占位用，服务端主要做 decode）。
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
        assert!(config_registry_skipped(), "空 registry 应直接 Finish");
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
        let empty = ConfigKnownPacks { packs: Vec::new() };
        let mut b = Vec::new(); empty.encode_body(&mut b);
        assert!(ConfigKnownPacks::decode_body(&b).unwrap().0.packs.is_empty());
    }
}
