//! save: Anvil Region 文件头 + chunk 列占位写，只兼容26.3/777
//!
//! 已实现：region 文件定位（32x32 分区、`r.{rx}.{rz}.mca`、8KB 头：4KB offsets + 4KB timestamps）。
//! 占位：chunk 列按 `[len u32 BE][compression u8][payload]` 落盘，compression 固定 3（uncompressed）。
//! TODO(P6)：payload 填真实 NBT（压缩类型切 2=zlib，参考 Anvil 规范）；覆盖写旧扇区回收。
use crate::{Chunk, SectionKind};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Region 边长：32x32 chunk 列。
pub const REGION_SIZE: i32 = 32;
/// 扇区字节数；文件头固定占 2 扇区（8KB：offsets 4KB + timestamps 4KB）。
pub const SECTOR_BYTES: usize = 4096;
pub const HEADER_BYTES: usize = SECTOR_BYTES * 2;
pub const HEADER_ENTRY_COUNT: usize = 32 * 32;
/// Anvil 压缩类型占位：3 = uncompressed。P6 切 2（zlib）并填 NBT。
pub const COMPRESSION_NONE: u8 = 3;

/// 存档 IO 错误占位。
#[derive(Debug, thiserror::Error)]
pub enum WorldIoError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupt region header: {0}")]
    CorruptHeader(String),
    #[error("unsupported compression {0} (placeholder only reads 3=uncompressed; P6 fills NBT+zlib)")]
    UnsupportedCompression(u8),
}

/// 区块 → region 坐标（floor 除法，负坐标向 -inf 取整）。
pub fn region_coords(chunk_x: i32, chunk_z: i32) -> (i32, i32) {
    (chunk_x.div_euclid(REGION_SIZE), chunk_z.div_euclid(REGION_SIZE))
}

/// 区块 → region 内局部坐标（0..32）。
pub fn local_coords(chunk_x: i32, chunk_z: i32) -> (i32, i32) {
    (chunk_x.rem_euclid(REGION_SIZE), chunk_z.rem_euclid(REGION_SIZE))
}

/// region 文件名：`r.{rx}.{rz}.mca`。
pub fn region_filename(region_x: i32, region_z: i32) -> String {
    format!("r.{region_x}.{region_z}.mca")
}

/// 区块在存档目录下的 region 文件路径。
pub fn region_path(dir: &Path, chunk_x: i32, chunk_z: i32) -> PathBuf {
    let (rx, rz) = region_coords(chunk_x, chunk_z);
    dir.join(region_filename(rx, rz))
}

/// 局部坐标 → 头表项下标（Anvil 顺序：x + z*32）。
pub fn header_entry_index(local_x: i32, local_z: i32) -> usize {
    debug_assert!((0..REGION_SIZE).contains(&local_x) && (0..REGION_SIZE).contains(&local_z));
    (local_x + local_z * REGION_SIZE) as usize
}

/// 头表项 u32 BE →（起始扇区，扇区数）。空表项为 0。
pub fn decode_entry(entry: u32) -> (u32, u8) {
    ((entry >> 8), (entry & 0xFF) as u8)
}

/// （起始扇区，扇区数）→ 头表项 u32 BE。
pub fn encode_entry(sector: u32, count: u8) -> u32 {
    (sector << 8) | count as u32
}

/// chunk 列占位编码：`[section数 u8][逐节：kind u8 + Uniform时 id u16 BE]`。
/// P6 用真实 NBT 替换整个 payload（压缩类型同步切 2）。
pub fn encode_chunk_column(chunk: &Chunk) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + chunk.sections.len() * 3);
    out.push(chunk.sections.len() as u8);
    for kind in &chunk.sections {
        match *kind {
            SectionKind::Uniform(id) => {
                out.push(0x00);
                out.extend_from_slice(&id.to_be_bytes());
            }
            SectionKind::Paletted => out.push(0x01),
            SectionKind::Direct => out.push(0x02),
        }
    }
    out
}

/// chunk 列持久化入口（占位）：定位 region 文件，头表追加写一列，返回 Ok。
/// P6 填 NBT 时只换 `encode_chunk_column` + 压缩字节，定位逻辑不变。
pub fn save_chunk(dir: &Path, chunk: &Chunk) -> Result<(), WorldIoError> {
    std::fs::create_dir_all(dir)?;
    let (lx, lz) = local_coords(chunk.pos.0, chunk.pos.1);
    let idx = header_entry_index(lx, lz);
    let path = region_path(dir, chunk.pos.0, chunk.pos.1);

    let payload = encode_chunk_column(chunk);
    // Anvil 列帧：[len = 1 + payload][compression][payload]，按扇区对齐追加。
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.extend_from_slice(&((1 + payload.len()) as u32).to_be_bytes());
    frame.push(COMPRESSION_NONE);
    frame.extend_from_slice(&payload);
    let sectors = ((frame.len() + SECTOR_BYTES - 1) / SECTOR_BYTES) as u8;
    frame.resize(sectors as usize * SECTOR_BYTES, 0);

    let mut f = OpenOptions::new().read(true).write(true).create(true).open(&path)?;
    let len = f.metadata()?.len();
    if len < HEADER_BYTES as u64 {
        // 新文件：补齐 8KB 空头。
        f.set_len(HEADER_BYTES as u64)?;
    }
    // 追加到文件尾（覆盖写只更新表项指向，旧扇区暂不回收：P6 再做）。
    let end = f.metadata()?.len();
    debug_assert!(end % SECTOR_BYTES as u64 == 0);
    let sector = (end / SECTOR_BYTES as u64) as u32;
    f.seek(SeekFrom::Start(end))?;
    f.write_all(&frame)?;

    let mut entry = [0u8; 4];
    entry.copy_from_slice(&encode_entry(sector, sectors).to_be_bytes());
    f.seek(SeekFrom::Start((idx * 4) as u64))?;
    f.write_all(&entry)?;
    // timestamp 表同步（秒级占位）。
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    f.seek(SeekFrom::Start((SECTOR_BYTES + idx * 4) as u64))?;
    f.write_all(&now.to_be_bytes())?;
    f.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_location_math() {
        assert_eq!(region_coords(0, 0), (0, 0));
        assert_eq!(region_coords(31, 31), (0, 0));
        assert_eq!(region_coords(32, 0), (1, 0));
        // 负坐标向 -inf：-1 仍属 region -1
        assert_eq!(region_coords(-1, -1), (-1, -1));
        assert_eq!(region_coords(-32, -33), (-1, -2));
        assert_eq!(local_coords(-1, -1), (31, 31));
        assert_eq!(local_coords(33, -33), (1, 31));
        assert_eq!(region_filename(1, -2), "r.1.-2.mca");
        assert_eq!(header_entry_index(0, 0), 0);
        assert_eq!(header_entry_index(31, 31), 1023);
        assert_eq!(header_entry_index(5, 3), 5 + 3 * 32);
        // 表项编解码往返
        let e = encode_entry(7, 2);
        assert_eq!(decode_entry(e), (7, 2));
        assert_eq!(decode_entry(0), (0, 0));
    }

    #[test]
    fn save_chunk_roundtrip_file_layout() {
        let dir = std::env::temp_dir().join(format!("mc-region-{}-save", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let chunk = crate::chunk_for(mc_core::ChunkPos(33, -1));
        save_chunk(&dir, &chunk).unwrap();

        // 文件定位 + 头表项非空 + 数据扇区对齐
        let path = region_path(&dir, 33, -1);
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "r.1.-1.mca");
        let raw = std::fs::read(&path).unwrap();
        assert!(raw.len() >= HEADER_BYTES + SECTOR_BYTES);
        assert_eq!(raw.len() % SECTOR_BYTES, 0);
        let (lx, lz) = local_coords(33, -1);
        let idx = header_entry_index(lx, lz);
        let entry = u32::from_be_bytes(raw[idx * 4..idx * 4 + 4].try_into().unwrap());
        let (sector, count) = decode_entry(entry);
        assert_eq!(sector, 2, "首列追加在头后 sector 2");
        assert!(count >= 1);
        let off = sector as usize * SECTOR_BYTES;
        let len = u32::from_be_bytes(raw[off..off + 4].try_into().unwrap()) as usize;
        assert_eq!(raw[off + 4], COMPRESSION_NONE);
        assert_eq!(len, 1 + raw[off + 5..off + 4 + len].len());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
