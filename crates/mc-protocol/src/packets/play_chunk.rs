//! play_chunk: SetCenter + ChunkData真布局 + Unload，只兼容26.3/777，ID已按P4A对照表核对777
//! ChunkData真布局来源：
//! - 包字段顺序：https://minecraft.wiki/w/Java_Edition_protocol/Packets#Chunk_Data_and_Update_Light
//!   （Clientbound `level_chunk_with_light` Play 0x2E：Chunk X Int + Chunk Z Int
//!   + Heightmaps PrefixedArray<Heightmap> + Data PrefixedArray<Byte>
//!   + Block Entities PrefixedArray + Light LightData）
//! - Heightmap/Section/Paletted容器：https://minecraft.wiki/w/Java_Edition_protocol/Chunk_format
//!   （Heightmap结构 Type VarInt + Data LongArray；Chunk Section结构 Block count Short
//!   + Fluid count Short + Block states Paletted + Biomes Paletted；
//!   Paletted Container BPE u8 + Palette + Data Array，BPE=0 单值时 Value VarInt
//!   + 空数据数组；1.21.5+ Data Array长度不再发送，由BPE推算，BPE=0即空）。
//! - Light掩码：https://minecraft.wiki/w/Java_Edition_protocol/Data_types#Type:Light_Data
//!   （Sky/Block Mask BitSet + Empty Sky/Block Mask BitSet + Sky/Block Light arrays；
//!   BitSet = VarInt long数 + Long数组，全0最小可接受）。
use crate::packets::codec::{encode_packet, read_i32_be, write_i32_be, write_i64_be, write_u16_be};
use crate::{read_varint, write_varint, ProtocolError};

/// Set Center Chunk 包 ID（Play CB `0x60` set_chunk_cache_center）。已按P4A对照表核对777（旧值0x4C实为recipe_book_remove）。
pub const PLAY_SET_CENTER_CHUNK_ID: i32 = 0x60;
/// Chunk Data 包 ID（Play CB `0x2E` level_chunk_with_light）。已按P4A对照表核对777（旧值0x27实为game_event）。
pub const PLAY_CHUNK_DATA_ID: i32 = 0x2E;
/// Unload Chunk 包 ID（Play CB `0x26` forget_level_chunk）。777 里旧 KeepAlive 占的 0x26
/// 实际就是该包（见 packet-map-777.md §4.1），P4b 曾误把 KeepAlive S2C 记成它，已改 0x2D。
pub const PLAY_UNLOAD_CHUNK_ID: i32 = 0x26;

/// Heightmap类型：MOTION_BLOCKING = 4（Chunk_format#Heightmap结构表：WORLD_SURFACE=1，MOTION_BLOCKING=4）。
pub const HEIGHTMAP_MOTION_BLOCKING_ID: i32 = 4;
/// 空MOTION_BLOCKING的long数：256列、BPE=9（ceil(log2(384+1))）、每long 7项、ceil(256/7)=37。
/// 来源：Chunk_format#Heightmap结构（BPE算法 + Data Array打包规则）。
pub const HEIGHTMAP_EMPTY_LONGS: usize = 37;

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
/// 保留仅为兼容旧脚手架（mc-server/play.rs占位log仍用它）；真机ChunkData高度图
/// 不再用它，改用 [`empty_heightmaps_bytes`]（777结构化Heightmaps，见下）。
pub fn minimal_empty_nbt_bytes() -> Vec<u8> {
    vec![0x0A, 0x00, 0x00, 0x00]
}

/// 777真高度图：PrefixedArray<Heightmap>，单项 MOTION_BLOCKING 空数组。
/// 布局：count VarInt(1) + type VarInt(4) + long数 VarInt(37) + 37×i64be(0)。
/// 来源：Packets#Chunk_Data_and_Update_Light（Heightmaps字段）+
/// Chunk_format#Heightmap结构（MOTION_BLOCKING=4；缺失则客户端按最小值初始化，
/// 此处给37零即全列最小高度，真机可站）。
pub fn empty_heightmaps_bytes() -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + HEIGHTMAP_EMPTY_LONGS * 8);
    write_varint(1, &mut out); // 1个heightmap
    write_varint(HEIGHTMAP_MOTION_BLOCKING_ID, &mut out); // MOTION_BLOCKING
    write_varint(HEIGHTMAP_EMPTY_LONGS as i32, &mut out); // 37 longs
    for _ in 0..HEIGHTMAP_EMPTY_LONGS {
        write_i64_be(0, &mut out);
    }
    out
}

/// 单值Paletted容器（BPE=0）：BPE u8(0) + Value VarInt。
/// 来源：Chunk_format#Paletted_Container结构 + #Single_valued
/// （BPE=0时Data Array为空；1.21.5+长度不再发送，BPE=0即空，不再跟VarInt 0）。
/// block传全局blockstate ID（air=0），biome传全局biome ID（plains=0）。
pub fn encode_single_valued_paletted_container(value: i32, out: &mut Vec<u8>) {
    out.push(0x00); // Bits Per Entry = 0（单值）
    write_varint(value, out); // Value：全局palette ID
    // 1.21.5+：Data Array长度不发送，BPE=0表示空数组，此处无后续字节。
}

/// 单节空section真布局8B（777/1.21.5+）：block_count u16be(0) + fluid_count u16be(0)
/// + block单值容器(2B: BPE0 + air 0) + biome单值容器(2B: BPE0 + plains 0)。
/// 来源：Chunk_format#Chunk_Section结构（Block count Short + Fluid count Short
/// + Block states Paletted + Biomes Paletted）。旧版8B是 2+3+3
/// （无fluid、有data_len VarInt 0）；新版8B是 2+2+2+2（有fluid、无data_len），
/// 总长同为8B但内部按spec重排，特此注释。
pub fn encode_empty_section_placeholder(out: &mut Vec<u8>) {
    write_u16_be(0, out); // block_count = 0（非空气方块数）
    write_u16_be(0, out); // fluid_count = 0（777 Section含流体计数）
    encode_single_valued_paletted_container(0, out); // block：air（全局blockstate 0）
    encode_single_valued_paletted_container(0, out); // biome：plains（全局biome 0）
}

/// 均匀节真布局8B（单值palette）：block_count + fluid_count + block单值 + biome单值。
/// `block_id`为全局blockstate ID占位（air=0/stone=1等，待registry对齐），
/// `biome_id`为全局biome ID（plains=0），`block_count`为非空气数（均匀非空节4096）。
pub fn encode_uniform_section_single(
    block_id: i32,
    biome_id: i32,
    block_count: u16,
    fluid_count: u16,
    out: &mut Vec<u8>,
) {
    write_u16_be(block_count, out);
    write_u16_be(fluid_count, out);
    encode_single_valued_paletted_container(block_id, out);
    encode_single_valued_paletted_container(biome_id, out);
}

/// 最小光照数据（全0+空数组，最小可接受）：4×BitSet空 + Sky/Block数组数0。
/// 布局：sky_mask VarInt(0) + block_mask VarInt(0) + empty_sky VarInt(0)
/// + empty_block VarInt(0) + sky数组数 VarInt(0) + block数组数 VarInt(0) = 6B。
/// 来源：Data_types#Light_Data（Sky/Block Mask + Empty Masks + arrays；
/// BitSet = VarInt long数 + Long数组，0即空）。全0表示无光照数据，
/// 客户端渲染偏暗但不掉线；全1（empty掩码置位）同样合法，此处取最小全0。
pub fn encode_empty_light_data(out: &mut Vec<u8>) {
    write_varint(0, out); // sky light mask：空BitSet
    write_varint(0, out); // block light mask：空BitSet
    write_varint(0, out); // empty sky light mask：空BitSet
    write_varint(0, out); // empty block light mask：空BitSet
    write_varint(0, out); // sky light数组数：0
    write_varint(0, out); // block light数组数：0
}

/// 读一个BitSet并跳过（返回消费字节数）：VarInt long数 + long数×8B。
fn skip_bitset(buf: &[u8]) -> Result<usize, ProtocolError> {
    let (nlongs, n) = read_varint(buf)?;
    if nlongs < 0 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let need = nlongs as usize * 8;
    if buf.len() < n + need {
        return Err(ProtocolError::UnexpectedEof);
    }
    Ok(n + need)
}

/// Chunk Data and Update Light真布局（777）。
/// 字段顺序（Packets#Chunk_Data_and_Update_Light）：
/// Chunk X Int + Chunk Z Int + Heightmaps（结构化，无长度前缀，自带count自定界）
/// + Data（VarInt size + section字节） + Block Entities（VarInt count，空=0，
///  每项 PackedXZ u8 + Y Short + Type VarInt + Data NBT） + Light（LightData 6B最小）。
/// 注意：Heightmaps不再是NBT、无VarInt长度前缀；Section含fluid_count、
/// 单值容器无data_len（1.21.5+）；包ID 0x2E不动。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayChunkData {
    pub chunk_x: i32,
    pub chunk_z: i32,
    /// 高度图原始字节（777结构化Heightmaps，非NBT；字段名带nbt仅为兼容旧脚手架构造，
    /// 实际内容见 [`empty_heightmaps_bytes`]）。
    pub heightmaps_nbt: Vec<u8>,
    /// 24节串接（每节8B真布局，见 [`encode_empty_section_placeholder`]）。
    pub section_bytes: Vec<u8>,
}

impl PlayChunkData {
    /// 空chunk：结构化空MOTION_BLOCKING + 24节空section。
    pub fn empty(chunk_x: i32, chunk_z: i32) -> Self {
        let mut section_bytes = Vec::new();
        for _ in 0..24 {
            encode_empty_section_placeholder(&mut section_bytes);
        }
        Self {
            chunk_x,
            chunk_z,
            heightmaps_nbt: empty_heightmaps_bytes(),
            section_bytes,
        }
    }

    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_i32_be(self.chunk_x, out);
        write_i32_be(self.chunk_z, out);
        // Heightmaps结构化数据：无长度前缀，直接追加（首字节即count VarInt自定界）。
        out.extend_from_slice(&self.heightmaps_nbt);
        // Data：VarInt size + section字节。
        write_varint(self.section_bytes.len() as i32, out);
        out.extend_from_slice(&self.section_bytes);
        // 空chunk：block entity数量0（PackedXZ+Y+Type+NBT每项，此处无项）。
        write_varint(0, out);
        // 最小光照：全0掩码+空数组（6B）。
        encode_empty_light_data(out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let mut off = 0;
        let (chunk_x, n) = read_i32_be(&buf[off..])?;
        off += n;
        let (chunk_z, n) = read_i32_be(&buf[off..])?;
        off += n;
        // Heightmaps：结构化（count + 每项 type + long数组），无长度前缀，需逐项解析定界。
        let h_start = off;
        let (hcount, n) = read_varint(&buf[off..])?;
        off += n;
        if hcount < 0 {
            return Err(ProtocolError::UnexpectedEof);
        }
        for _ in 0..hcount {
            let (_, n) = read_varint(&buf[off..])?; // heightmap type
            off += n;
            let (llen, n) = read_varint(&buf[off..])?; // long数
            off += n;
            if llen < 0 {
                return Err(ProtocolError::UnexpectedEof);
            }
            let need = llen as usize * 8;
            if buf.len() < off + need {
                return Err(ProtocolError::UnexpectedEof);
            }
            off += need;
        }
        let heightmaps_nbt = buf[h_start..off].to_vec();
        // Data：VarInt size + 字节。
        let (sec_len, n) = read_varint(&buf[off..])?;
        off += n;
        let sec_len = sec_len.max(0) as usize;
        if buf.len() < off + sec_len {
            return Err(ProtocolError::UnexpectedEof);
        }
        let section_bytes = buf[off..off + sec_len].to_vec();
        off += sec_len;
        // Block entities：最小实现只接受0（非0需NBT跳过，当前脚手架不发送）。
        let (be_count, n) = read_varint(&buf[off..])?;
        off += n;
        if be_count != 0 {
            return Err(ProtocolError::UnexpectedEof);
        }
        // Light：4×BitSet + 2×数组（每数组：VarInt数 + 每项 VarInt len + bytes）。
        for _ in 0..4 {
            let n = skip_bitset(&buf[off..])?;
            off += n;
        }
        for _ in 0..2 {
            let (acount, n) = read_varint(&buf[off..])?;
            off += n;
            if acount < 0 {
                return Err(ProtocolError::UnexpectedEof);
            }
            for _ in 0..acount {
                let (alen, n) = read_varint(&buf[off..])?;
                off += n;
                if alen < 0 {
                    return Err(ProtocolError::UnexpectedEof);
                }
                let alen = alen as usize;
                if buf.len() < off + alen {
                    return Err(ProtocolError::UnexpectedEof);
                }
                off += alen;
            }
        }
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
        // ID不动
        assert_eq!(PLAY_CHUNK_DATA_ID, 0x2E);
        assert_eq!(PLAY_SET_CENTER_CHUNK_ID, 0x60);
        // 旧空NBT保留兼容（play.rs占位仍用它），真高度图走结构化
        assert_eq!(minimal_empty_nbt_bytes(), vec![0x0A,0x00,0x00,0x00]);
        // 真高度图：1 + MOTION_BLOCKING(4) + 37 + 37×8零
        assert_eq!(&v.heightmaps_nbt[..3], &[0x01, 0x04, 0x25]);
        assert_eq!(v.heightmaps_nbt.len(), 3 + HEIGHTMAP_EMPTY_LONGS * 8);
        // 每节真8B：00 00(blk) 00 00(fluid) 00 00(blk单值air) 00 00(biome单值plains)
        assert_eq!(v.section_bytes.len(), 24 * 8);
        assert_eq!(&v.section_bytes[..8], &[0,0,0,0,0,0,0,0]);
        // encode/decode往返（含光照6B），总body = 8 + heightmaps + sizeVar + 192 + 1 + 6
        let mut body = Vec::new(); v.encode_body(&mut body);
        let (got, used) = PlayChunkData::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got, v);
        let mut pkt = Vec::new(); encode_play_chunk_data_packet(&v, &mut pkt);
        assert!(!pkt.is_empty());
    }
    #[test]
    fn play_chunk_data_single_valued_container_layout() {
        // 单值容器：BPE0 + Value，无data_len（1.21.5+）
        let mut b = Vec::new();
        encode_single_valued_paletted_container(0, &mut b);
        assert_eq!(b, vec![0x00, 0x00]);
        let mut b = Vec::new();
        encode_single_valued_paletted_container(1, &mut b);
        assert_eq!(b, vec![0x00, 0x01]);
        // 均匀石头节8B：10 00(4096) 00 00(fluid0) 00 01(air?no,stone1) 00 00(plains)
        let mut s = Vec::new();
        encode_uniform_section_single(1, 0, 4096, 0, &mut s);
        assert_eq!(s, vec![0x10,0x00,0x00,0x00,0x00,0x01,0x00,0x00]);
    }
    #[test]
    fn play_chunk_data_light_is_empty_6bytes() {
        let mut b = Vec::new();
        encode_empty_light_data(&mut b);
        assert_eq!(b, vec![0,0,0,0,0,0]);
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
