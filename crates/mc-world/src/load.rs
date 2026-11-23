//! load: Region 头表查找 + chunk 列占位读，只兼容26.3/777
//!
//! 缺文件 / 空表项 → `Ok(None)`（首启/未生成）；帧格式见 save.rs。
//! TODO(P6)：compression==2 时走 zlib 解压 + NBT 解析；payload 换真实反序列化。
use crate::save::{COMPRESSION_NONE, WorldIoError, decode_entry, header_entry_index, local_coords, region_path};
use crate::{Chunk, SectionKind};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// chunk 列占位解码（`save::encode_chunk_column` 的逆操作）。
/// P6 换 NBT 反序列化；长度/标签非法时报 CorruptHeader。
pub fn decode_chunk_column(pos: mc_core::ChunkPos, bytes: &[u8]) -> Result<Chunk, WorldIoError> {
    let (n, rest) = bytes.split_first().ok_or_else(|| WorldIoError::CorruptHeader("empty column".into()))?;
    let mut sections = Vec::with_capacity(*n as usize);
    let mut cur = rest;
    for _ in 0..*n {
        let (tag, tail) = cur.split_first().ok_or_else(|| WorldIoError::CorruptHeader("truncated section".into()))?;
        match tag {
            0x00 => {
                if tail.len() < 2 {
                    return Err(WorldIoError::CorruptHeader("truncated uniform id".into()));
                }
                sections.push(SectionKind::Uniform(u16::from_be_bytes([tail[0], tail[1]])));
                cur = &tail[2..];
            }
            0x01 => {
                sections.push(SectionKind::Paletted);
                cur = tail;
            }
            0x02 => {
                sections.push(SectionKind::Direct);
                cur = tail;
            }
            other => return Err(WorldIoError::CorruptHeader(format!("unknown section tag {other:#04x}"))),
        }
    }
    Ok(Chunk { pos, sections })
}

/// 读一列：`Ok(Some(chunk))` 命中 / `Ok(None)` 未生成（占位）/ `Err` 文件损坏。
pub fn load_chunk(dir: &Path, pos: mc_core::ChunkPos) -> Result<Option<Chunk>, WorldIoError> {
    let path = region_path(dir, pos.0, pos.1);
    let mut f = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(WorldIoError::Io(e)),
    };
    let len = f.metadata()?.len();
    if len < crate::save::HEADER_BYTES as u64 {
        return Err(WorldIoError::CorruptHeader(format!("{} shorter than 8KB header", path.display())));
    }
    let (lx, lz) = local_coords(pos.0, pos.1);
    let idx = header_entry_index(lx, lz);
    f.seek(SeekFrom::Start((idx * 4) as u64))?;
    let mut entry = [0u8; 4];
    f.read_exact(&mut entry)?;
    let (sector, count) = decode_entry(u32::from_be_bytes(entry));
    if sector == 0 || count == 0 {
        return Ok(None);
    }
    f.seek(SeekFrom::Start(sector as u64 * crate::save::SECTOR_BYTES as u64))?;
    let mut len_b = [0u8; 4];
    f.read_exact(&mut len_b)?;
    let frame_len = u32::from_be_bytes(len_b) as usize;
    if frame_len < 1 || frame_len as u64 > count as u64 * crate::save::SECTOR_BYTES as u64 {
        return Err(WorldIoError::CorruptHeader(format!("bad frame len {frame_len}")));
    }
    let mut comp = [0u8; 1];
    f.read_exact(&mut comp)?;
    if comp[0] != COMPRESSION_NONE {
        return Err(WorldIoError::UnsupportedCompression(comp[0]));
    }
    let mut payload = vec![0u8; frame_len - 1];
    f.read_exact(&mut payload)?;
    Ok(Some(decode_chunk_column(pos, &payload)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::save_chunk;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::env::temp_dir().join(format!("mc-region-{}-{}-{tag}-{n}", std::process::id(), line!()))
    }

    #[test]
    fn missing_file_and_empty_entry_are_none() {
        let dir = tmp_dir("missing");
        let _ = std::fs::remove_dir_all(&dir);
        // region 文件根本不存在
        assert!(load_chunk(&dir, mc_core::ChunkPos(0, 0)).unwrap().is_none());
        // 文件存在但表项为空（只写 8KB 空头）
        std::fs::create_dir_all(&dir).unwrap();
        let path = crate::save::region_path(&dir, 0, 0);
        std::fs::write(&path, vec![0u8; crate::save::HEADER_BYTES]).unwrap();
        assert!(load_chunk(&dir, mc_core::ChunkPos(0, 0)).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tmp_dir("roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        for (x, z) in [(0, 0), (33, -1), (-33, -33)] {
            let pos = mc_core::ChunkPos(x, z);
            let chunk = crate::chunk_for(pos);
            save_chunk(&dir, &chunk).unwrap();
            let back = load_chunk(&dir, pos).unwrap().expect("column must exist");
            assert_eq!(back.pos, pos);
            assert_eq!(back.sections, chunk.sections, "roundtrip mismatch at {x},{z}");
        }
        // 未写的列仍是 None（同文件内空表项）
        assert!(load_chunk(&dir, mc_core::ChunkPos(1, 1)).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_payload_errors() {
        assert!(decode_chunk_column(mc_core::ChunkPos(0, 0), &[]).is_err());
        assert!(decode_chunk_column(mc_core::ChunkPos(0, 0), &[1, 0x09]).is_err());
        assert!(decode_chunk_column(mc_core::ChunkPos(0, 0), &[1, 0x00, 0x01]).is_err());
        // compression != 3 的帧报 UnsupportedCompression（P6 再支持 2）
        let dir = tmp_dir("badcomp");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pos = mc_core::ChunkPos(0, 0);
        let path = crate::save::region_path(&dir, pos.0, pos.1);
        let mut raw = vec![0u8; crate::save::HEADER_BYTES + crate::save::SECTOR_BYTES];
        raw[0..4].copy_from_slice(&crate::save::encode_entry(2, 1).to_be_bytes());
        let off = crate::save::SECTOR_BYTES * 2;
        raw[off..off + 4].copy_from_slice(&2u32.to_be_bytes());
        raw[off + 4] = 2; // zlib：占位实现拒绝
        raw[off + 5] = 0;
        std::fs::write(&path, &raw).unwrap();
        assert!(matches!(
            load_chunk(&dir, pos),
            Err(WorldIoError::UnsupportedCompression(2))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
