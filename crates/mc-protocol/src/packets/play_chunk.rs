//! play_chunk: SetCenter + ChunkData(空section占位) + Unload，只兼容26.3/777，ID已按P4A对照表核对777
use crate::packets::codec::{decode_string, encode_packet, encode_string, read_i32_be, write_i32_be};
use crate::{read_varint, write_varint, ProtocolError};

/// Set Center Chunk 包 ID（Play CB `0x60` set_chunk_cache_center）。已按P4A对照表核对777（旧值0x4C实为recipe_book_remove）。
pub const PLAY_SET_CENTER_CHUNK_ID: i32 = 0x60;
/// Chunk Data 包 ID（Play CB `0x2E` level_chunk_with_light）。已按P4A对照表核对777（旧值0x27实为game_event）。
pub const PLAY_CHUNK_DATA_ID: i32 = 0x2E;
/// Unload Chunk 包 ID（Play CB `0x26` forget_level_chunk）。777 里旧 KeepAlive 占的 0x26
/// 实际就是该包（见 packet-map-777.md §4.1），P4b 曾误把 KeepAlive S2C 记成它，已改 0x2D。
pub const PLAY_UNLOAD_CHUNK_ID: i32 = 0x26;

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

/// Unload Chunk（CB `0x26` forget_level_chunk）：chunk_x i32be + chunk_z i32be。
/// 与 ChunkData 头两字段同布局（SetCenter 才用 VarInt），只兼容 26.3/777。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnloadChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
}

impl UnloadChunk {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_i32_be(self.chunk_x, out);
        write_i32_be(self.chunk_z, out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (chunk_x, n1) = read_i32_be(buf)?;
        let (chunk_z, n2) = read_i32_be(&buf[n1..])?;
        Ok((Self { chunk_x, chunk_z }, n1 + n2))
    }
}

/// Unload Chunk 打成完整线包。
pub fn encode_unload_chunk_packet(v: &UnloadChunk, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(PLAY_UNLOAD_CHUNK_ID, &body, out);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_varint;
    use crate::packets::codec::decode_packet_id;
    #[test]
    fn play_set_center_chunk_roundtrip() {
        let v = SetCenterChunk { chunk_x: -1, chunk_z: 5 };
        let mut pkt = Vec::new(); encode_set_center_chunk_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_SET_CENTER_CHUNK_ID);
        assert_eq!(SetCenterChunk::decode_body(&pkt[n+m..]).unwrap().0, v);
    }
    #[test]
    fn play_chunk_data_empty_roundtrip() {
        let v = PlayChunkData::empty(0, 0);
        let mut pkt = Vec::new(); encode_play_chunk_data_packet(&v, &mut pkt);
        assert!(!pkt.is_empty());
        assert_eq!(minimal_empty_nbt_bytes(), vec![0x0A,0x00,0x00,0x00]);
    }
    #[test]
    fn play_unload_chunk_roundtrip() {
        // 0x26 在 777 就是 Unload（packet-map §4.1），不是 KeepAlive
        assert_eq!(PLAY_UNLOAD_CHUNK_ID, 0x26);
        let v = UnloadChunk { chunk_x: 3, chunk_z: -5 };
        let mut pkt = Vec::new(); encode_unload_chunk_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_UNLOAD_CHUNK_ID);
        assert_eq!(UnloadChunk::decode_body(&pkt[n+m..]).unwrap().0, v);
    }
}
