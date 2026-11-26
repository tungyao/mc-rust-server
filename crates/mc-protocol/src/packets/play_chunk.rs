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

// 以下包 ID 全部编译期从 `mc-registry` 由 data/777/packets.json 生成的 PACKETS 表派生。

/// Set Center Chunk 包 ID（Play CB `0x60` set_chunk_cache_center）。已按P4A对照表核对777（旧值0x4C实为recipe_book_remove）。
pub const PLAY_SET_CENTER_CHUNK_ID: i32 = super::packet_id("clientbound.play", "set_chunk_cache_center");
/// Chunk Data 包 ID（Play CB `0x2E` level_chunk_with_light）。已按P4A对照表核对777（旧值0x27实为game_event）。
pub const PLAY_CHUNK_DATA_ID: i32 = super::packet_id("clientbound.play", "level_chunk_with_light");
/// Unload Chunk 包 ID（Play CB `0x26` forget_level_chunk）。777 里旧 KeepAlive 占的 0x26
/// 实际就是该包（见 packet-map-777.md §4.1），P4b 曾误把 KeepAlive S2C 记成它，已改 0x2D。
pub const PLAY_UNLOAD_CHUNK_ID: i32 = super::packet_id("clientbound.play", "forget_level_chunk");
/// Chunk Batch Start / Finished（Play CB 0x0C / 0x0B）：26.3 客户端用批次结束
/// 判断“首包区块已下完”，不发这两个包会一直停在“加载地形中…”。
pub const PLAY_CHUNK_BATCH_START_ID: i32 = super::packet_id("clientbound.play", "chunk_batch_start");
pub const PLAY_CHUNK_BATCH_FINISHED_ID: i32 = super::packet_id("clientbound.play", "chunk_batch_finished");
/// Game Event（Play CB 0x27）：26.3 客户端只有收到
/// `LEVEL_CHUNKS_LOAD_START`（type=13）才把 LevelLoadTracker 从 WaitingForServer
/// 切到 WaitingForPlayerChunk；缺失则永远停在“加载地形中…”。
pub const PLAY_GAME_EVENT_ID: i32 = super::packet_id("clientbound.play", "game_event");
pub const GAME_EVENT_LEVEL_CHUNKS_LOAD_START: u8 = 13;
/// 原版 26.3 `worldgen/biome` 网络注册表中 `minecraft:plains` 的 ID
/// （空 KnownPacks 抓包：67 项按 key 排序，plains=41）。区块 palette 用这个 ID。
/// 以下 biome ID 均从仓库内 `registry_00.bin`（`minecraft:worldgen/biome`，67 项，
/// 顺序即客户端网络 ID）解析确认，P16 起按 chunk 选用：
/// plains=41 / forest=22 / beach=3 / ocean=36（river=42、birch_forest=4 等备用）。
pub const PLAINS_BIOME_ID_777: i32 = 41;
pub const FOREST_BIOME_ID_777: i32 = 22;
pub const BEACH_BIOME_ID_777: i32 = 3;
pub const OCEAN_BIOME_ID_777: i32 = 36;

/// P45 v2 用到的其余 biome 网络 ID，同样来自 `registry_00.bin` 的 67 项（下标即 ID）。
/// 每项都由 `decode_config_registry_data` 的 `entries` 顺序核对过，见
/// `play_chunk.rs::tests::extra_biome_ids_match_vanilla_registry`。
pub const RIVER_BIOME_ID_777: i32 = 42;
pub const BIRCH_FOREST_BIOME_ID_777: i32 = 4;
pub const SNOWY_PLAINS_BIOME_ID_777: i32 = 47;
pub const SNOWY_TAIGA_BIOME_ID_777: i32 = 49;
pub const TAIGA_BIOME_ID_777: i32 = 57;
pub const MEADOW_BIOME_ID_777: i32 = 33;
pub const DESERT_BIOME_ID_777: i32 = 15;
pub const SAVANNA_BIOME_ID_777: i32 = 43;
pub const WARM_OCEAN_BIOME_ID_777: i32 = 60;
pub const COLD_OCEAN_BIOME_ID_777: i32 = 6;
pub const FROZEN_OCEAN_BIOME_ID_777: i32 = 23;
pub const FROZEN_RIVER_BIOME_ID_777: i32 = 25;
pub const SNOWY_BEACH_BIOME_ID_777: i32 = 46;
pub const STONY_SHORE_BIOME_ID_777: i32 = 53;
pub const SWAMP_BIOME_ID_777: i32 = 56;
pub const MANGROVE_SWAMP_BIOME_ID_777: i32 = 32;
pub const DARK_FOREST_BIOME_ID_777: i32 = 9;
pub const FLOWER_FOREST_BIOME_ID_777: i32 = 21;
pub const CHERRY_GROVE_BIOME_ID_777: i32 = 5;
pub const GROVE_BIOME_ID_777: i32 = 26;
pub const DEEP_DARK_BIOME_ID_777: i32 = 11;
pub const DRIPSTONE_CAVES_BIOME_ID_777: i32 = 16;
pub const LUSH_CAVES_BIOME_ID_777: i32 = 31;
pub const MUSHROOM_FIELDS_BIOME_ID_777: i32 = 34;

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
    encode_empty_section_with_biome(PLAINS_BIOME_ID_777, out);
}

/// 均匀空（air）section：block_count=0、fluid_count=0、block 单值 air、biome 单值。
/// P16 起 WorldGen 按 chunk 的真实 biome 调用；旧 [`encode_empty_section_placeholder`]
/// 等价于 `biome_id = plains(41)`。
pub fn encode_empty_section_with_biome(biome_id: i32, out: &mut Vec<u8>) {
    write_u16_be(0, out); // block_count = 0（非空气方块数）
    write_u16_be(0, out); // fluid_count = 0（777 Section含流体计数）
    encode_single_valued_paletted_container(0, out); // block：air（全局blockstate 0）
    encode_single_valued_paletted_container(biome_id, out); // biome：调用方指定
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
    encode_block_section_with_fluid(block_states, biome_id, 0, out);
}

/// 带真实 fluid_count 的混合/单值 section 编码（P16 水）。
/// `fluid_count` 是该 section 内 water 方块数（0..=4096），由调用方统计；
/// `block_count` 内部按“非空气（!=0）”统计，水也算非空气。
/// palette 自适应：单值 → 单值容器；<=256 → 间接 palette（BPE=max(4,ceil(log2))）；
/// >256 → direct（BPE=15，不写 palette 列表）。
pub fn encode_block_section_with_fluid(
    block_states: &[i32],
    biome_id: i32,
    fluid_count: u16,
    out: &mut Vec<u8>,
) {
    encode_block_states_with_fluid(block_states, fluid_count, out);
    encode_single_valued_paletted_container(biome_id, out);
}

/// 只写 block_count/fluid_count/block states 容器（不写 biome）。
/// 从 v1 的 `encode_block_section_with_fluid` 原样抽出，字节完全一致；
/// v2 的多 biome 容器在它之后追加，v1 路径仍走老函数。
fn encode_block_states_with_fluid(block_states: &[i32], fluid_count: u16, out: &mut Vec<u8>) {
    assert_eq!(block_states.len(), SECTION_VOLUME, "section must contain 4096 block states");
    let block_count = block_states.iter().filter(|&&b| b != 0).count() as u16;
    write_u16_be(block_count, out); // 非空气方块数（含水）
    write_u16_be(fluid_count, out); // 流体方块数（水）

    // 建立局部 palette（保持首次出现顺序），同时把 4096 项映射为 palette 索引。
    // 线性查重在 debug 构建下比 HashMap 更快（palette 通常只有几项），顺序也可复现。
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
}

// ---------------------------------------------------------------------------
// P45 v2：64 项 biome 容器 + 非均匀天空光。线格式与 v1 完全相同，只是新 helper。
// ---------------------------------------------------------------------------

/// biome Paletted Container 的条目数：每 section 4×4×4 = 64 个 biome 单元。
pub const BIOME_CONTAINER_ENTRIES: usize = 64;
/// 原版 67 项 biome 注册表的 direct BPE = ceil(log2(67)) = 7。
/// 来源：wiki Chunk_format#Palette_formats（biomes 的 direct BPE 按注册表大小运行时计算）。
const BIOME_GLOBAL_BITS: u8 = 7;
/// biome 间接 palette 的最大条目数（>8 走 direct，wiki 同节 + minecraftdocs.dev ladder）。
const BIOME_MAX_INDIRECT: usize = 8;

/// 64 项 biome paletted container（777/1.21.5+，无 Data Array 长度前缀）。
/// - 1 项：BPE=0 + Value VarInt（无 data array）；
/// - 2..=8 项：BPE=max(1,ceil(log2(n)))，VarInt palette 长度 + VarInt 各项；
/// - >8 项：BPE=7 direct（全局 biome ID 直接打包，不写 palette 列表）。
///
/// 布局依据：minecraft.wiki `Java_Edition_protocol/Chunk_format`
/// 「Chunk Section structure / Biomes = 64 entries (4×4×4)」「Palette formats」
/// （biomes indirect 1–3 bit / direct 7 bit）、
/// 「Data Array format」（1.21.5+ 长度不发送、不跨 long、低位在前）。
/// 真机 vanilla 包体对照待补（本环境无原版客户端）。
pub fn encode_biome_paletted_container(biomes: &[i32; BIOME_CONTAINER_ENTRIES], out: &mut Vec<u8>) {
    let mut palette: Vec<i32> = Vec::new();
    for &b in biomes {
        if !palette.contains(&b) {
            palette.push(b);
        }
    }
    if palette.len() == 1 {
        encode_single_valued_paletted_container(palette[0], out);
        return;
    }
    let (bpe, direct) = if palette.len() <= BIOME_MAX_INDIRECT {
        (ceil_log2(palette.len()).max(1), false)
    } else {
        (BIOME_GLOBAL_BITS, true)
    };
    out.push(bpe);
    if !direct {
        write_varint(palette.len() as i32, out);
        for &b in &palette {
            write_varint(b, out);
        }
    }
    let per_long = 64 / bpe as usize;
    let longs = BIOME_CONTAINER_ENTRIES.div_ceil(per_long);
    for li in 0..longs {
        let mut word: u64 = 0;
        for k in 0..per_long {
            let idx = li * per_long + k;
            if idx >= BIOME_CONTAINER_ENTRIES {
                break;
            }
            let value = if direct {
                biomes[idx] as u64
            } else {
                palette.iter().position(|&p| p == biomes[idx]).unwrap() as u64
            };
            word |= value << (k * bpe as usize);
        }
        write_i64_be(word as i64, out);
    }
}

/// 均匀 air section + 多 biome 容器（v2 空节路径）。
pub fn encode_empty_section_with_biomes(biomes: &[i32; BIOME_CONTAINER_ENTRIES], out: &mut Vec<u8>) {
    write_u16_be(0, out);
    write_u16_be(0, out);
    encode_single_valued_paletted_container(0, out);
    encode_biome_paletted_container(biomes, out);
}

/// 均匀方块 section + 多 biome 容器（v2 全水/全石节路径）。
pub fn encode_uniform_section_single_with_biomes(
    block_id: i32,
    block_count: u16,
    fluid_count: u16,
    biomes: &[i32; BIOME_CONTAINER_ENTRIES],
    out: &mut Vec<u8>,
) {
    write_u16_be(block_count, out);
    write_u16_be(fluid_count, out);
    encode_single_valued_paletted_container(block_id, out);
    encode_biome_paletted_container(biomes, out);
}

/// 混合 section + 多 biome 容器（v2 主路径）。
pub fn encode_block_section_with_fluid_and_biomes(
    block_states: &[i32],
    fluid_count: u16,
    biomes: &[i32; BIOME_CONTAINER_ENTRIES],
    out: &mut Vec<u8>,
) {
    encode_block_states_with_fluid(block_states, fluid_count, out);
    encode_biome_paletted_container(biomes, out);
}

/// 真实 MOTION_BLOCKING 高度图（777 结构化 Heightmaps，非 NBT）：
/// count(1) + type(4) + long数(37) + 37×i64be。
/// `columns[z*16+x]` 是该列相对 `min_y` 的高度值（最高非空气方块 y + 1 - min_y）。
/// BPE=9、每 long 7 项，`ceil(256/7)=37` longs；值超过 9 位会被截断，调用方需保证
/// 0..=511（主世界 -64..319 相对高度最大 384）。
pub fn motion_blocking_heightmap(columns: &[i32; 256]) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + HEIGHTMAP_EMPTY_LONGS * 8);
    write_varint(1, &mut out); // 1 个 heightmap
    write_varint(HEIGHTMAP_MOTION_BLOCKING_ID, &mut out); // MOTION_BLOCKING
    write_varint(HEIGHTMAP_EMPTY_LONGS as i32, &mut out); // 37 longs
    let bpe = 9usize;
    let per_long = 64 / bpe; // 7
    for li in 0..HEIGHTMAP_EMPTY_LONGS {
        let mut word: u64 = 0;
        for k in 0..per_long {
            let idx = li * per_long + k;
            if idx >= 256 {
                break;
            }
            word |= ((columns[idx] as u64) & 0x1ff) << (k * bpe);
        }
        write_i64_be(word as i64, &mut out);
    }
    out
}

/// 256 列同值的高度图（旧平原测试/兼容入口，内部走 [`motion_blocking_heightmap`]）。
pub fn motion_blocking_heightmap_uniform(height: i32) -> Vec<u8> {
    motion_blocking_heightmap(&[height; 256])
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

/// 非均匀天空光（P45 v2）：`sections` 每项为 `(section_index, 2048B nibble数组)`，
/// nibble 索引按 `y*256 + z*16 + x`，低位在偶索引、高位在奇索引。
///
/// 线与 [`encode_sky_light_data`] 完全相同（wiki `Data_types#Light_Data`）：
/// sky mask BitSet + block mask + empty sky + empty block + sky 数组数/数组 + block 数组数。
/// 只把含非零 nibble 的 section 放进 mask（零节客户端按 0 处理）；
/// BitSet = VarInt 字节数 + 小端 bit 字节（26.3 `FriendlyByteBuf.writeByteArray`）。
/// 数组按 section 索引升序发送，与 mask 的 bit 顺序一致。
/// 真机 vanilla 包体对照待补（本环境无原版客户端）。
pub fn encode_nonuniform_sky_light_data(sections: &[(u32, [u8; 2048])], out: &mut Vec<u8>) {
    if sections.is_empty() {
        encode_empty_light_data(out);
        return;
    }
    let mut mask = 0u64;
    for &(section, _) in sections {
        if section < 64 {
            mask |= 1u64 << section;
        }
    }
    let mut ordered: Vec<&(u32, [u8; 2048])> = sections.iter().collect();
    ordered.sort_by_key(|(section, _)| *section);
    let mut bits = mask.to_le_bytes().to_vec();
    while bits.last() == Some(&0) {
        bits.pop();
    }
    write_varint(bits.len() as i32, out);
    out.extend_from_slice(&bits);
    write_varint(0, out); // block mask
    write_varint(0, out); // empty sky mask
    write_varint(0, out); // empty block mask
    write_varint(ordered.len() as i32, out);
    for (_, nibble) in ordered {
        write_varint(2048, out);
        out.extend_from_slice(nibble);
    }
    write_varint(0, out); // block light 数组数
}

/// P46 v2：天空光 + block light 合并进 LightData（线格式不变，见 wiki `Data_types#Light_Data`）。
/// `sky`/`block` 每项为 `(section_index, 2048B nibble数组)`，nibble 索引
/// `y*256 + z*16 + x`，偶索引低位/奇索引高位。布局与
/// [`encode_nonuniform_sky_light_data`] 相同，只是 block mask/数组不再恒空：
/// sky mask BitSet + block mask BitSet + empty sky + empty block + sky 数组 + block 数组。
/// 空输入仍走 [`encode_empty_light_data`]（6B），与旧版逐字节一致。
/// 真机 vanilla 包体对照待补（本环境无原版客户端）。
pub fn encode_light_data(
    sky: &[(u32, [u8; 2048])],
    block: &[(u32, [u8; 2048])],
    out: &mut Vec<u8>,
) {
    if sky.is_empty() && block.is_empty() {
        encode_empty_light_data(out);
        return;
    }
    write_light_mask(sky, out);
    write_light_mask(block, out);
    write_varint(0, out); // empty sky mask：发送的数组覆盖全部非零节
    write_varint(0, out); // empty block mask
    write_varint(sky.len() as i32, out);
    for (_, nibble) in sky {
        write_varint(2048, out);
        out.extend_from_slice(nibble);
    }
    write_varint(block.len() as i32, out);
    for (_, nibble) in block {
        write_varint(2048, out);
        out.extend_from_slice(nibble);
    }
}

/// 一节列表 → BitSet（VarInt 字节数 + 小端 bit 字节；仅发 <64 的 section）。
fn write_light_mask(sections: &[(u32, [u8; 2048])], out: &mut Vec<u8>) {
    let mut mask = 0u64;
    for &(section, _) in sections {
        if section < 64 {
            mask |= 1u64 << section;
        }
    }
    let mut bits = mask.to_le_bytes().to_vec();
    while bits.last() == Some(&0) {
        bits.pop();
    }
    write_varint(bits.len() as i32, out);
    out.extend_from_slice(&bits);
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
        let body = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/777/vanilla_config/registry_00.bin"));
        let (reg, _) = super::super::decode_config_registry_data(body).unwrap();
        assert_eq!(reg.registry_id, "minecraft:worldgen/biome");
        let idx = |name: &str| {
            reg.entries.iter().position(|e| e.entry_id == name).unwrap() as i32
        };
        assert_eq!(idx("minecraft:plains"), PLAINS_BIOME_ID_777);
        assert_eq!(idx("minecraft:forest"), FOREST_BIOME_ID_777);
        assert_eq!(idx("minecraft:beach"), BEACH_BIOME_ID_777);
        assert_eq!(idx("minecraft:ocean"), OCEAN_BIOME_ID_777);
        assert_eq!(reg.entries.len(), 67);
    }
    #[test]
    fn empty_section_and_uniform_water_carry_biome_and_fluid() {
        // 空 air 节：block/fluid 都是 0，block 单值 air，biome 用真实 biome。
        let mut b = Vec::new();
        encode_empty_section_with_biome(FOREST_BIOME_ID_777, &mut b);
        assert_eq!(b, vec![0, 0, 0, 0, 0, 0, 0, FOREST_BIOME_ID_777 as u8]);
        // 旧入口字节不变（biome=plains）。
        let mut old = Vec::new();
        encode_empty_section_placeholder(&mut old);
        assert_eq!(b.len(), old.len());
        assert_eq!(&old[7], &41);
        // 均匀全水节：block_count=fluid_count=4096，water=89，ocean=36。
        let mut w = Vec::new();
        encode_uniform_section_single(89, OCEAN_BIOME_ID_777, 4096, 4096, &mut w);
        assert_eq!(w, vec![0x10, 0x00, 0x10, 0x00, 0x00, 89, 0x00, OCEAN_BIOME_ID_777 as u8]);
    }
    #[test]
    fn block_section_with_fluid_writes_fluid_count_and_palette() {
        // 前 1000 个 water(89)、其余 stone(1)：fluid_count=1000，palette=[89,1]。
        let mut states = [0i32; SECTION_VOLUME];
        states[..1000].fill(89);
        states[1000..].fill(1);
        let mut b = Vec::new();
        encode_block_section_with_fluid(&states, OCEAN_BIOME_ID_777, 1000, &mut b);
        assert_eq!(&b[..4], &[0x10, 0x00, 0x03, 0xE8], "block_count=4096 fluid_count=1000");
        assert_eq!(b[4], 4, "BPE=4");
        assert_eq!(b[5], 2, "palette len=2");
        assert_eq!(&b[6..8], &[89, 1], "palette=[water,stone]");
        // 末尾 still biome 单值。
        assert_eq!(&b[b.len() - 2..], &[0x00, OCEAN_BIOME_ID_777 as u8]);
        // 旧签名 encode_block_section 等价 fluid_count=0（字节完全一致）。
        let mut old = Vec::new();
        encode_block_section(&states, OCEAN_BIOME_ID_777, &mut old);
        assert_eq!(&old[..4], &[0x10, 0x00, 0x00, 0x00]);
        assert_eq!(&old[4..], &b[4..]);
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
    fn motion_blocking_heightmap_columns_packed() {
        let mut cols = [0i32; 256];
        for (i, c) in cols.iter_mut().enumerate() {
            *c = 80 + (i % 40) as i32; // 80..119，全部在 9 bit 内
        }
        let h = motion_blocking_heightmap(&cols);
        assert_eq!(&h[..3], &[0x01, 0x04, 0x25]);
        assert_eq!(h.len(), 3 + 37 * 8);
        for li in 0..37 {
            let word = u64::from_be_bytes(h[3 + li * 8..3 + li * 8 + 8].try_into().unwrap());
            for k in 0..7 {
                let idx = li * 7 + k;
                if idx >= 256 {
                    assert_eq!((word >> (k * 9)) & 0x1ff, 0, "padding must be zero");
                } else {
                    assert_eq!((word >> (k * 9)) & 0x1ff, cols[idx] as u64, "column {idx}");
                }
            }
        }
        // 均匀版本仍等价于 256 列同值。
        assert_eq!(
            motion_blocking_heightmap_uniform(129),
            motion_blocking_heightmap(&[129; 256])
        );
    }
    #[test]
    fn extra_biome_ids_match_vanilla_registry() {
        let body = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/777/vanilla_config/registry_00.bin"));
        let (reg, _) = super::super::decode_config_registry_data(body).unwrap();
        let idx = |name: &str| reg.entries.iter().position(|e| e.entry_id == format!("minecraft:{name}")).unwrap() as i32;
        for (name, id) in [
            ("river", RIVER_BIOME_ID_777),
            ("birch_forest", BIRCH_FOREST_BIOME_ID_777),
            ("snowy_plains", SNOWY_PLAINS_BIOME_ID_777),
            ("snowy_taiga", SNOWY_TAIGA_BIOME_ID_777),
            ("taiga", TAIGA_BIOME_ID_777),
            ("meadow", MEADOW_BIOME_ID_777),
            ("desert", DESERT_BIOME_ID_777),
            ("savanna", SAVANNA_BIOME_ID_777),
            ("warm_ocean", WARM_OCEAN_BIOME_ID_777),
            ("cold_ocean", COLD_OCEAN_BIOME_ID_777),
            ("frozen_ocean", FROZEN_OCEAN_BIOME_ID_777),
            ("frozen_river", FROZEN_RIVER_BIOME_ID_777),
            ("snowy_beach", SNOWY_BEACH_BIOME_ID_777),
            ("stony_shore", STONY_SHORE_BIOME_ID_777),
            ("swamp", SWAMP_BIOME_ID_777),
            ("mangrove_swamp", MANGROVE_SWAMP_BIOME_ID_777),
            ("dark_forest", DARK_FOREST_BIOME_ID_777),
            ("flower_forest", FLOWER_FOREST_BIOME_ID_777),
            ("cherry_grove", CHERRY_GROVE_BIOME_ID_777),
            ("grove", GROVE_BIOME_ID_777),
            ("deep_dark", DEEP_DARK_BIOME_ID_777),
            ("dripstone_caves", DRIPSTONE_CAVES_BIOME_ID_777),
            ("lush_caves", LUSH_CAVES_BIOME_ID_777),
            ("mushroom_fields", MUSHROOM_FIELDS_BIOME_ID_777),
        ] {
            assert_eq!(idx(name), id, "biome {name}");
        }
        assert_eq!(reg.entries.len(), 67);
    }

    #[test]
    fn biome_paletted_container_byte_layout() {
        // 单值：BPE0 + VarInt(biome)，无 data array。
        let mut b = Vec::new();
        encode_biome_paletted_container(&[PLAINS_BIOME_ID_777; 64], &mut b);
        assert_eq!(b, vec![0, PLAINS_BIOME_ID_777 as u8]);

        // 3 项间接：palette=[forest=22, plains=41, river=42]，BPE=2、2 个 long。
        // 前 16 格 index0、中 16 格 index1、后 32 格 index2。
        let mut biomes = [0i32; 64];
        biomes[..16].fill(FOREST_BIOME_ID_777);
        biomes[16..32].fill(PLAINS_BIOME_ID_777);
        biomes[32..].fill(RIVER_BIOME_ID_777);
        let mut b = Vec::new();
        encode_biome_paletted_container(&biomes, &mut b);
        assert_eq!(&b[..5], &[2, 3, 22, 41, 42], "BPE + palette");
        assert_eq!(b.len(), 5 + 16, "2 longs, no length prefix");
        let w0 = u64::from_be_bytes(b[5..13].try_into().unwrap());
        let w1 = u64::from_be_bytes(b[13..21].try_into().unwrap());
        // index0 在前 16 项；index1 占 bit32..63 的偶数位；word1 全是 index2。
        assert_eq!(w0 & 0xffff_ffff, 0, "first 16 entries are palette[0]");
        assert_eq!(w0 >> 32, 0x5555_5555, "next 16 entries are palette[1]");
        assert_eq!(w1, 0xAAAA_AAAA_AAAA_AAAA, "last 32 entries are palette[2]");

        // 9 项：超过间接上限 8 → direct BPE=7、无 palette、8 个 long（64/9 向上取整）。
        let mut biomes = [0i32; 64];
        for (i, b) in biomes.iter_mut().enumerate() {
            *b = (i % 9) as i32;
        }
        let mut b = Vec::new();
        encode_biome_paletted_container(&biomes, &mut b);
        assert_eq!(b[0], 7, "direct BPE=7");
        assert_eq!(b.len(), 1 + 8 * 8, "8 longs");
        let w0 = u64::from_be_bytes(b[1..9].try_into().unwrap());
        for k in 0..9 {
            assert_eq!((w0 >> (k * 7)) & 0x7f, k as u64, "direct entry {k}");
        }
        // 组合 section：block 单值 air + 多 biome，字节与手工拼接一致。
        let mut combined = Vec::new();
        encode_empty_section_with_biomes(&[FOREST_BIOME_ID_777; 64], &mut combined);
        assert_eq!(&combined[..6], &[0, 0, 0, 0, 0, 0]);
        assert_eq!(&combined[6..], &[0, FOREST_BIOME_ID_777 as u8]);
    }

    #[test]
    fn nonuniform_sky_light_byte_layout() {
        let mut a = [0u8; 2048];
        a.fill(0xff);
        let mut c = [0u8; 2048];
        c[0] = 0x0f; // 第 0 格 15、第 1 格 0
        let sections = [(1u32, a), (3u32, c)];
        let mut b = Vec::new();
        encode_nonuniform_sky_light_data(&sections, &mut b);
        // mask bits 1+3 = 0x0A，小端 1 字节。
        assert_eq!(&b[..2], &[1, 0x0a]);
        assert_eq!(&b[2..5], &[0, 0, 0], "block/empty masks");
        assert_eq!(b[5], 2, "sky array count");
        assert_eq!(&b[6..8], &[0x80, 0x10], "len 2048 VarInt");
        assert!(b[8..8 + 2048].iter().all(|&x| x == 0xff));
        assert_eq!(&b[2056..2058], &[0x80, 0x10]);
        assert_eq!(b[2058], 0x0f);
        assert!(b[2059..2058 + 2048].iter().all(|&x| x == 0));
        assert_eq!(*b.last().unwrap(), 0, "block array count");
        assert_eq!(b.len(), 2 + 3 + 1 + 2 * (2 + 2048) + 1);

        // PlayChunkData 的 decode_body 能完整消费这段 LightData。
        let v = PlayChunkData {
            chunk_x: 0,
            chunk_z: 0,
            heightmaps_nbt: empty_heightmaps_bytes(),
            section_bytes: Vec::new(),
            light_bytes: b.clone(),
        };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = PlayChunkData::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got.light_bytes, b);

        // 空 section 列表退回 6B 空光照。
        let mut e = Vec::new();
        encode_nonuniform_sky_light_data(&[], &mut e);
        assert_eq!(e, vec![0, 0, 0, 0, 0, 0]);
    }

    /// P46：sky + block 合并 LightData 的逐字节回归（布局按 wiki 26.3 已验证口径，
    /// 本环境无原版客户端，需真机复验）。
    #[test]
    fn encode_light_data_combines_sky_and_block_masks_byte_for_byte() {
        let sky = vec![(0u32, [0x11u8; 2048]), (3, [0x22; 2048])];
        let block = vec![(1u32, [0x33u8; 2048])];
        let mut b = Vec::new();
        super::encode_light_data(&sky, &block, &mut b);
        let mut expected = Vec::new();
        write_varint(1, &mut expected);
        expected.push(0b1001); // sky mask sections 0+3
        write_varint(1, &mut expected);
        expected.push(0b0010); // block mask section 1
        write_varint(0, &mut expected); // empty sky
        write_varint(0, &mut expected); // empty block
        write_varint(2, &mut expected);
        for arr in [&[0x11u8; 2048][..], &[0x22u8; 2048][..]] {
            write_varint(2048, &mut expected);
            expected.extend_from_slice(arr);
        }
        write_varint(1, &mut expected);
        write_varint(2048, &mut expected);
        expected.extend_from_slice(&[0x33; 2048]);
        assert_eq!(b, expected, "combined LightData must match hand-built 26.3 layout");

        // PlayChunkData 的 decode_body 能完整消费两种数组。
        let v = PlayChunkData {
            chunk_x: 7,
            chunk_z: -9,
            heightmaps_nbt: empty_heightmaps_bytes(),
            section_bytes: Vec::new(),
            light_bytes: b.clone(),
        };
        let mut body = Vec::new();
        v.encode_body(&mut body);
        let (got, used) = PlayChunkData::decode_body(&body).unwrap();
        assert_eq!(used, body.len());
        assert_eq!(got.light_bytes, b);

        // 空输入与旧版空光照逐字节一致（v1 路径不变）。
        let mut e = Vec::new();
        super::encode_light_data(&[], &[], &mut e);
        assert_eq!(e, vec![0, 0, 0, 0, 0, 0]);
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
