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
use crate::packets::codec::{encode_packet, read_i32_be, read_i64_be, write_i32_be, write_i64_be, write_u16_be};
use crate::{read_varint, write_varint, ProtocolError};

/// Set Center Chunk 包 ID（Play CB `0x60` set_chunk_cache_center）。已按P4A对照表核对777（旧值0x4C实为recipe_book_remove）。
pub const PLAY_SET_CENTER_CHUNK_ID: i32 = 0x60;
/// Chunk Data 包 ID（Play CB `0x2E` level_chunk_with_light）。已按P4A对照表核对777（旧值0x27实为game_event）。
pub const PLAY_CHUNK_DATA_ID: i32 = 0x2E;
/// Unload Chunk 包 ID（Play CB `0x26` forget_level_chunk）。777 里旧 KeepAlive 占的 0x26
/// 实际就是该包（见 packet-map-777.md §4.1），P4b 曾误把 KeepAlive S2C 记成它，已改 0x2D。
pub const PLAY_UNLOAD_CHUNK_ID: i32 = 0x26;
/// Chunk Batch Start / Finished（Play CB 0x0C / 0x0B）：26.3 客户端用批次结束
/// 判断“首包区块已下完”，不发这两个包会一直停在“加载地形中…”。
pub const PLAY_CHUNK_BATCH_START_ID: i32 = 0x0C;
pub const PLAY_CHUNK_BATCH_FINISHED_ID: i32 = 0x0B;
/// Game Event（Play CB 0x27）：26.3 客户端只有收到
/// `LEVEL_CHUNKS_LOAD_START`（type=13）才把 LevelLoadTracker 从 WaitingForServer
/// 切到 WaitingForPlayerChunk；缺失则永远停在“加载地形中…”。
pub const PLAY_GAME_EVENT_ID: i32 = 0x27;
pub const GAME_EVENT_LEVEL_CHUNKS_LOAD_START: u8 = 13;
/// 原版 26.3 `worldgen/biome` 网络注册表中 `minecraft:plains` 的 ID
/// （空 KnownPacks 抓包：67 项按 key 排序，plains=41）。区块 palette 用这个 ID。
pub const PLAINS_BIOME_ID_777: i32 = 41;

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

/// Unload Chunk（CB `0x26` forget_level_chunk）：**单个 packed i64 ChunkPos**。
/// 26.3 客户端 `ClientboundForgetLevelChunkPacket` 用 `FriendlyByteBuf.read/writeChunkPos`，
/// 即 `ChunkPos.pack() = (x & 0xffffffff) | ((z & 0xffffffff) << 32)` 的 big-endian Long；
/// 不是旧版的 Chunk X Int + Chunk Z Int（已用 26.3.jar javap 反编译确认）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnloadChunk {
    pub chunk_x: i32,
    pub chunk_z: i32,
}

impl UnloadChunk {
    pub fn packed_pos(&self) -> i64 {
        ((self.chunk_x as u32 as u64) | ((self.chunk_z as u32 as u64) << 32)) as i64
    }

    pub fn from_packed_pos(packed: i64) -> Self {
        let u = packed as u64;
        Self { chunk_x: u as u32 as i32, chunk_z: (u >> 32) as u32 as i32 }
    }

    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_i64_be(self.packed_pos(), out);
    }

    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (packed, n) = read_i64_be(buf)?;
        Ok((Self::from_packed_pos(packed), n))
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
/// block传全局blockstate ID（air=0），biome传全局biome ID（plains=41）。
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
    encode_single_valued_paletted_container(PLAINS_BIOME_ID_777, out); // biome：原版 plains=41
}

/// 均匀节真布局8B（单值palette）：block_count + fluid_count + block单值 + biome单值。
/// `block_id`为全局blockstate ID（air=0/stone=1/grass_block=9/dirt=10，
/// 与 26.3 `reports/blocks.json` 对齐），`biome_id`为全局biome ID（plains=41），
/// `block_count`为非空气数（均匀非空节4096）。
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

/// 一个 section 的方块数（16×16×16）。
pub const SECTION_VOLUME: usize = 4096;

/// `ceil(log2(n))`（n>=2）；用于 palette 的 BPE。
fn ceil_log2(n: usize) -> u8 {
    let mut bits = 0u8;
    let mut v = n - 1;
    while v > 0 {
        bits += 1;
        v >>= 1;
    }
    bits
}

/// 间接/直接 palette 的 section 编码（777，1.21.5+ 无 Data Array 长度前缀）。
/// `block_states` 长度必须为 4096，按 `y*256 + z*16 + x` 排列（section 内局部坐标）。
/// - palette<=256：间接 palette，BPE=max(4, ceil(log2(palette_len)))；
/// - palette>256：直接全局 palette，BPE=15，不写 palette 列表；
/// - 单值 palette 退回单值容器（长度 0）。
/// Data Array 采用 1.16+ 的“跨 long 不拆分”打包：每个 long 放 floor(64/BPE) 项，
/// 低位在前；长度由客户端按 4096/BPE 推算，不再发送 VarInt 长度。
pub fn encode_block_section(block_states: &[i32], biome_id: i32, out: &mut Vec<u8>) {
    assert_eq!(block_states.len(), SECTION_VOLUME, "section must contain 4096 block states");
    let block_count = block_states.iter().filter(|&&b| b != 0).count() as u16;
    write_u16_be(block_count, out); // 非空气方块数
    write_u16_be(0, out); // fluid_count：当前世界无流体

    // 建立局部 palette（保持首次出现顺序），同时把 4096 项映射为 palette 索引。
    let mut palette: Vec<i32> = Vec::new();
    let mut indices = [0u16; SECTION_VOLUME];
    for (i, &state) in block_states.iter().enumerate() {
        let idx = match palette.iter().position(|&p| p == state) {
            Some(idx) => idx,
            None => {
                palette.push(state);
                palette.len() - 1
            }
        };
        indices[i] = idx as u16;
    }

    if palette.len() == 1 {
        encode_single_valued_paletted_container(palette[0], out);
    } else {
        let direct = palette.len() > 256;
        let bpe = if direct { 15 } else { ceil_log2(palette.len()).max(4) };
        out.push(bpe);
        if !direct {
            write_varint(palette.len() as i32, out);
            for &state in &palette {
                write_varint(state, out);
            }
        }
        let per_long = 64 / bpe as usize;
        let long_count = SECTION_VOLUME.div_ceil(per_long);
        for li in 0..long_count {
            let mut word: u64 = 0;
            for k in 0..per_long {
                let idx = li * per_long + k;
                if idx >= SECTION_VOLUME {
                    break;
                }
                let value = if direct { block_states[idx] as u64 } else { indices[idx] as u64 };
                word |= value << (k * bpe as usize);
            }
            write_i64_be(word as i64, out);
        }
    }
    encode_single_valued_paletted_container(biome_id, out);
}

/// 真实 MOTION_BLOCKING 高度图（777 结构化 Heightmaps，非 NBT）：
/// count(1) + type(4) + long数(37) + 37×i64be。
/// `height` 是相对 `min_y` 的地表高度（最高非空气方块 y + 1 - min_y）。
/// 256 列同值（当前平原地形），BPE=9、每 long 7 项，`ceil(256/7)=37` longs。
pub fn motion_blocking_heightmap_uniform(height: i32) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + HEIGHTMAP_EMPTY_LONGS * 8);
    write_varint(1, &mut out); // 1 个 heightmap
    write_varint(HEIGHTMAP_MOTION_BLOCKING_ID, &mut out); // MOTION_BLOCKING
    write_varint(HEIGHTMAP_EMPTY_LONGS as i32, &mut out); // 37 longs
    let bpe = 9usize;
    let per_long = 64 / bpe; // 7
    for li in 0..HEIGHTMAP_EMPTY_LONGS {
        let mut word: u64 = 0;
        for k in 0..per_long {
            if li * per_long + k >= 256 {
                break;
            }
            word |= (height as u64) << (k * bpe);
        }
        write_i64_be(word as i64, &mut out);
    }
    out
}

/// 最小光照数据（全0+空数组，最小可接受）：4×BitSet空 + Sky/Block数组数0。
/// 布局：sky_mask VarInt(0) + block_mask VarInt(0) + empty_sky VarInt(0)
/// + empty_block VarInt(0) + sky数组数 VarInt(0) + block数组数 VarInt(0) = 6B。
/// 来源：Data_types#Light_Data（Sky/Block Mask + Empty Masks + arrays）。
/// 26.3 的 BitSet 是 VarInt 字节数 + `BitSet.toByteArray()`（空集就是 0），
/// 全0表示无光照数据；世界侧用 [`encode_sky_light_data`] 发真实天空光。
pub fn encode_empty_light_data(out: &mut Vec<u8>) {
    write_varint(0, out); // sky light mask：空BitSet
    write_varint(0, out); // block light mask：空BitSet
    write_varint(0, out); // empty sky light mask：空BitSet
    write_varint(0, out); // empty block light mask：空BitSet
    write_varint(0, out); // sky light数组数：0
    write_varint(0, out); // block light数组数：0
}

/// 平原地形的天空光照：给 `mask` 中每个 section 发一份 2048B 的 15 级 nibble，
/// block light 全空。`mask` 每个 bit 对应一个 16 格 section（bit0 = y=-64..-49）。
/// 注意：26.3 的 BitSet 线格式是 `FriendlyByteBuf.writeByteArray(BitSet.toByteArray())`
/// = VarInt 字节数 + 小端 bit 字节，不是旧版的 “long 数量 + long 数组”（已按客户端
/// `ByteBufCodecs$15` 反编译确认）。没有数组的 section 客户端按 0 处理。
pub fn encode_sky_light_data(mask: u64, sky_level: u8, out: &mut Vec<u8>) {
    let level = sky_level & 0x0f;
    let packed = level | (level << 4);
    if mask == 0 {
        encode_empty_light_data(out);
        return;
    }
    let mut bits = mask.to_le_bytes().to_vec();
    while bits.last() == Some(&0) {
        bits.pop();
    }
    write_varint(bits.len() as i32, out); // sky mask：BitSet 字节数组
    out.extend_from_slice(&bits);
    write_varint(0, out); // block mask
    write_varint(0, out); // empty sky mask
    write_varint(0, out); // empty block mask
    let count = mask.count_ones();
    write_varint(count as i32, out); // sky light 数组数
    for _ in 0..count {
        write_varint(2048, out);
        out.resize(out.len() + 2048, packed);
    }
    write_varint(0, out); // block light 数组数
}

/// 读一个BitSet并跳过（返回消费字节数）：26.3 是 VarInt 字节数 + 小端 bit 字节
/// （`FriendlyByteBuf.writeByteArray(BitSet.toByteArray())`），不是 long 数组。
fn skip_bitset(buf: &[u8]) -> Result<usize, ProtocolError> {
    let (nbytes, n) = read_varint(buf)?;
    if nbytes < 0 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let need = nbytes as usize;
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
    /// 24节串接（每节8B起，见 [`encode_empty_section_placeholder`]）。
    pub section_bytes: Vec<u8>,
    /// Light Data 原始字节（天空/方块光照掩码 + 数组）。世界侧用
    /// [`encode_sky_light_data`] 填平原地表天空光；空 chunk 用 6B 空光照。
    pub light_bytes: Vec<u8>,
}

impl PlayChunkData {
    /// 空chunk：结构化空MOTION_BLOCKING + 24节空section + 空光照。
    pub fn empty(chunk_x: i32, chunk_z: i32) -> Self {
        let mut section_bytes = Vec::new();
        for _ in 0..24 {
            encode_empty_section_placeholder(&mut section_bytes);
        }
        let mut light_bytes = Vec::new();
        encode_empty_light_data(&mut light_bytes);
        Self {
            chunk_x,
            chunk_z,
            heightmaps_nbt: empty_heightmaps_bytes(),
            section_bytes,
            light_bytes,
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
        // Light Data：世界侧给真实天空光，空 chunk 给 6B 空光照。
        out.extend_from_slice(&self.light_bytes);
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
        let light_start = off;
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
        Ok((Self { chunk_x, chunk_z, heightmaps_nbt, section_bytes, light_bytes: buf[light_start..off].to_vec() }, off))
    }
}

/// Chunk Batch Start 打成完整线包（空包体）。
pub fn encode_chunk_batch_start_packet(out: &mut Vec<u8>) {
    encode_packet(PLAY_CHUNK_BATCH_START_ID, &[], out);
}
/// Game Event `LEVEL_CHUNKS_LOAD_START` 包体：Event u8 + Float param(0)。
pub fn encode_game_event_level_chunks_load_start(out: &mut Vec<u8>) {
    out.push(GAME_EVENT_LEVEL_CHUNKS_LOAD_START);
    out.extend_from_slice(&0.0f32.to_be_bytes());
}
/// Chunk Batch Finished 打成完整线包：VarInt batch_size（本批区块数）。
pub fn encode_chunk_batch_finished_packet(batch_size: i32, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_varint(batch_size, &mut body);
    encode_packet(PLAY_CHUNK_BATCH_FINISHED_ID, &body, out);
}

/// Chunk Data 打成完整线包。
pub fn encode_play_chunk_data_packet(v: &PlayChunkData, out: &mut Vec<u8>) {    let mut body = Vec::new();
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
    fn plains_biome_id_matches_vanilla_registry() {
        let body = include_bytes!("data/vanilla_config_777/registry_00.bin");
        let (reg, _) = super::super::decode_config_registry_data(body).unwrap();
        assert_eq!(reg.registry_id, "minecraft:worldgen/biome");
        let idx = reg.entries.iter().position(|e| e.entry_id == "minecraft:plains").unwrap();
        assert_eq!(idx as i32, PLAINS_BIOME_ID_777);
    }
    #[test]
    fn play_chunk_data_empty_roundtrip() {        let v = PlayChunkData::empty(0, 0);
        // ID不动
        assert_eq!(PLAY_CHUNK_DATA_ID, 0x2E);
        assert_eq!(PLAY_SET_CENTER_CHUNK_ID, 0x60);
        // 旧空NBT保留兼容（play.rs占位仍用它），真高度图走结构化
        assert_eq!(minimal_empty_nbt_bytes(), vec![0x0A,0x00,0x00,0x00]);
        // 真高度图：1 + MOTION_BLOCKING(4) + 37 + 37×8零
        assert_eq!(&v.heightmaps_nbt[..3], &[0x01, 0x04, 0x25]);
        assert_eq!(v.heightmaps_nbt.len(), 3 + HEIGHTMAP_EMPTY_LONGS * 8);
        // 每节真8B：00 00(blk) 00 00(fluid) 00 00(blk单值air) 00 29(biome单值plains=41)
        assert_eq!(v.section_bytes.len(), 24 * 8);
        assert_eq!(&v.section_bytes[..8], &[0,0,0,0,0,0,0,41]);
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
    fn block_section_indirect_palette_layout() {
        // 2 层石头 + 2 层泥土：间接 palette BPE=4，palette=[stone=1,dirt=10]，
        // 4096 项占 256 longs（无 data_len 前缀）。
        let mut states = [0i32; SECTION_VOLUME];
        states[..2048].fill(1);
        states[2048..].fill(10);
        let mut b = Vec::new();
        encode_block_section(&states, PLAINS_BIOME_ID_777, &mut b);
        assert_eq!(b.len(), 2058);
        assert_eq!(&b[..4], &[0x10, 0x00, 0x00, 0x00], "block_count=4096, fluid=0");
        assert_eq!(b[4], 4, "BPE=4");
        assert_eq!(b[5], 2, "palette len=2");
        assert_eq!(&b[6..8], &[1, 10], "palette=[stone,dirt]");
        // 第一项 stone(1)，第 2048 项 dirt(10)，最后一项 dirt；末尾 2B 是 biome 单值 plains=41。
        assert_eq!(&b[b.len() - 2..], &[0x00, 0x29]);
        let data = &b[8..b.len() - 2];
        assert_eq!(data.len(), 256 * 8);
        let word0 = u64::from_be_bytes(data[..8].try_into().unwrap());
        let word128 = u64::from_be_bytes(data[128 * 8..129 * 8].try_into().unwrap());
        let word255 = u64::from_be_bytes(data[255 * 8..256 * 8].try_into().unwrap());
        // Data Array 存的是 palette 索引（不是 blockstate ID）：前 2048 项=0(stone)，后 2048 项=1(dirt)。
        assert_eq!(word0 & 0xf, 0);
        assert_eq!((word0 >> 4) & 0xf, 0);
        assert_eq!(word128 & 0xf, 1, "entry 2048 is palette index 1 (dirt)");
        assert_eq!((word255 >> 60) & 0xf, 1, "entry 4095 is palette index 1 (dirt)");
        // 单值 section 仍走 8B 单值容器。
        let stone = [1i32; SECTION_VOLUME];
        let mut s = Vec::new();
        encode_block_section(&stone, PLAINS_BIOME_ID_777, &mut s);
        assert_eq!(s, vec![0x10,0x00,0x00,0x00,0x00,0x01,0x00,0x29]);
    }
    #[test]
    fn sky_light_data_layout() {
        // section 8..=23 共 16 个 section，天空光 15：BitSet 字节 [0x00,0xFF,0xFF]。
        let mask: u64 = 0x00FF_FF00;
        let mut b = Vec::new();
        encode_sky_light_data(mask, 15, &mut b);
        assert_eq!(b.len(), 1 + 3 + 3 + 1 + 16 * (2 + 2048) + 1);
        assert_eq!(b[0], 3, "sky mask 3 bytes");
        assert_eq!(&b[1..4], &[0x00, 0xFF, 0xFF]);
        assert_eq!(&b[4..7], &[0, 0, 0], "block/empty masks");
        assert_eq!(b[7], 16, "sky array count");
        assert_eq!(&b[8..10], &[0x80, 0x10], "array len 2048 VarInt");
        assert!(b[10..10 + 2048].iter().all(|&x| x == 0xFF));
        assert_eq!(*b.last().unwrap(), 0, "block array count");
        // mask=0 退回 6B 空光照。
        let mut e = Vec::new();
        encode_sky_light_data(0, 15, &mut e);
        assert_eq!(e, vec![0, 0, 0, 0, 0, 0]);
    }
    #[test]
    fn motion_blocking_heightmap_uniform_layout() {
        let h = motion_blocking_heightmap_uniform(129);
        assert_eq!(&h[..3], &[0x01, 0x04, 0x25]);
        assert_eq!(h.len(), 3 + 37 * 8);
        let word0 = u64::from_be_bytes(h[3..11].try_into().unwrap());
        for k in 0..7 {
            assert_eq!((word0 >> (k * 9)) & 0x1ff, 129);
        }
        let last = u64::from_be_bytes(h[3 + 36 * 8..].try_into().unwrap());
        for k in 0..4 {
            assert_eq!((last >> (k * 9)) & 0x1ff, 129);
        }
        assert_eq!(last >> 36, 0);
    }
    #[test]
    fn play_unload_chunk_is_packed_long() {
        // 0x26 在 777 就是 Unload（packet-map §4.1），不是 KeepAlive。
        assert_eq!(PLAY_UNLOAD_CHUNK_ID, 0x26);
        let v = UnloadChunk { chunk_x: 3, chunk_z: -5 };
        // x 在低 32 位、z 在高 32 位，big-endian Long。
        assert_eq!(v.packed_pos(), 0xFFFF_FFFB_0000_0003u64 as i64);
        let mut body = Vec::new();
        v.encode_body(&mut body);
        assert_eq!(body, vec![0xFF, 0xFF, 0xFF, 0xFB, 0x00, 0x00, 0x00, 0x03]);
        let (back, used) = UnloadChunk::decode_body(&body).unwrap();
        assert_eq!((back, used), (v, 8));
        let mut pkt = Vec::new();
        encode_unload_chunk_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, PLAY_UNLOAD_CHUNK_ID);
        assert_eq!(UnloadChunk::decode_body(&pkt[n + m..]).unwrap().0, v);
    }
}
