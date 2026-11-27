//! P54 批次 1 基础设施的 redb 扩展（A5 journal 运维 / A6 时间天气 / A8 工作台会话）。
//! 只增表/增 meta 键：SCHEMA_VERSION 不变，P53 `stats` 表与 v4 存档布局逐字节不变。
use super::*;

/// P54 A8：一个逻辑容器的 10 槽快照（服务端纯落盘，不发 `0x06`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraftSnapshot {
    /// 稳定 UUID（存档主键口径，与 players/stats 一致）。
    pub owner: String,
    pub pos: (i32, i32, i32),
    /// Unix 秒；后台 `last_saved` 展示用。
    pub last_saved: u64,
    /// 槽 0 结果 + 1..=9 合成格（背包部分仍在 players 表）。
    pub grid: [Option<(i32, i32)>; 10],
}

/// 逻辑容器键：与 `MutableWorld` 坐标无关，按稳定 UUID + 容器坐标定位。
pub fn craft_container_key(owner: &str, pos: (i32, i32, i32)) -> String {
    format!("craft9|{owner}|{},{},{}", pos.0, pos.1, pos.2)
}

fn encode_craft_snapshot(s: &CraftSnapshot) -> Vec<u8> {
    let mut out = Vec::new();
    let owner = s.owner.as_bytes();
    out.extend_from_slice(&(owner.len() as u16).to_be_bytes());
    out.extend_from_slice(owner);
    for v in [s.pos.0, s.pos.1, s.pos.2] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&s.last_saved.to_be_bytes());
    for stack in s.grid {
        match stack {
            Some((id, count)) => {
                out.push(1);
                out.extend_from_slice(&id.to_be_bytes());
                out.extend_from_slice(&count.to_be_bytes());
            }
            None => out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0]),
        }
    }
    out
}

fn decode_craft_snapshot(bytes: &[u8]) -> Result<CraftSnapshot, StoreError> {
    let mut off = 0usize;
    let mut take = |n: usize| -> Result<&[u8], StoreError> {
        let end = off.checked_add(n).ok_or_else(|| StoreError::BadRecord("craft offset overflow".into()))?;
        let slice = bytes.get(off..end).ok_or_else(|| StoreError::BadRecord("craft snapshot truncated".into()))?;
        off = end;
        Ok(slice)
    };
    let owner_len = u16::from_be_bytes(take(2)?.try_into().unwrap()) as usize;
    let owner = String::from_utf8(take(owner_len)?.to_vec())
        .map_err(|_| StoreError::BadRecord("craft owner not utf8".into()))?;
    let mut pos = [0i32; 3];
    for p in &mut pos {
        *p = i32::from_be_bytes(take(4)?.try_into().unwrap());
    }
    let last_saved = u64::from_be_bytes(take(8)?.try_into().unwrap());
    let mut grid = [None; 10];
    for slot in &mut grid {
        let flag = take(1)?[0];
        let id = i32::from_be_bytes(take(4)?.try_into().unwrap());
        let count = i32::from_be_bytes(take(4)?.try_into().unwrap());
        if flag == 1 {
            *slot = Some((id, count));
        }
    }
    Ok(CraftSnapshot { owner, pos: (pos[0], pos[1], pos[2]), last_saved, grid })
}

impl WorldStore {
    // --- P54 A5：journal 运维（裁剪窗口 + 可观测统计） ---

    /// 只读统计：`(当前 seq, 保留条数, 最老 seq, 近似字节数)`。
    pub fn journal_stats(&self) -> Result<(u64, u64, u64, u64), StoreError> {
        let seq = self.journal_len()?;
        let tx = self.db.begin_read()?;
        let Ok(t) = tx.open_table(JOURNAL) else { return Ok((seq, 0, seq, 0)) };
        let mut count = 0u64;
        let mut oldest = seq;
        let mut bytes = 0u64;
        for item in t.iter()? {
            let (k, v) = item?;
            if count == 0 {
                oldest = k.value();
            }
            count += 1;
            bytes += 8 + v.value().len() as u64;
        }
        Ok((seq, count, oldest, bytes))
    }

    /// 按 seq 保留最近 `keep` 条（旧记录删除）。
    ///
    /// `blocks_flushed` 必须是调用方确认 `blocks` 已落盘的凭据（dirty=0）；
    /// 否则拒绝裁剪，避免日志是唯一尚未物化的编辑时丢数据。只在后台触发，
    /// 绝不占唯一 50ms ticker 的关键路径。
    pub fn trim_journal(&self, keep: u64, blocks_flushed: bool) -> Result<u64, StoreError> {
        if !blocks_flushed {
            return Err(StoreError::BadRecord("journal trim requires flushed blocks".into()));
        }
        let tx = self.db.begin_write()?;
        let seq = meta_u64(&tx, META_SEQ)?.unwrap_or(0);
        let cutoff = seq.saturating_sub(keep);
        let mut t = tx.open_table(JOURNAL)?;
        let mut removed = 0u64;
        let keys: Vec<u64> = t.iter()?
            .filter_map(|item| item.ok())
            .filter(|(k, _)| k.value() < cutoff)
            .map(|(k, _)| k.value())
            .collect();
        for k in keys {
            t.remove(k)?;
            removed += 1;
        }
        drop(t);
        tx.commit()?;
        Ok(removed)
    }

    // --- P54 A6：时间/天气持久化（meta 缺省值由 WorldTimeState 兜底） ---

    pub fn world_time(&self) -> Result<Option<i64>, StoreError> {
        Ok(meta_u64_read(&self.db, META_WORLD_TIME)?.map(|v| v as i64))
    }

    pub fn set_world_time(&self, tick: i64) -> Result<(), StoreError> {
        self.with_write(|tx, _| {
            let b = (tick.max(0) as u64).to_be_bytes();
            meta_put(tx, META_WORLD_TIME, b.as_slice())
        })
    }

    pub fn weather(&self) -> Result<Option<u8>, StoreError> {
        Ok(meta_u64_read(&self.db, META_WEATHER)?.map(|v| v as u8))
    }

    pub fn set_weather(&self, code: u8) -> Result<(), StoreError> {
        self.with_write(|tx, _| {
            let b = (code as u64).to_be_bytes();
            meta_put(tx, META_WEATHER, b.as_slice())
        })
    }

    // --- P54 A8：工作台会话快照（不发 0x06，不碰背包/合成语义） ---

    pub fn save_craft_snapshot(&self, snapshot: &CraftSnapshot) -> Result<(), StoreError> {
        let key = craft_container_key(&snapshot.owner, snapshot.pos);
        let value = encode_craft_snapshot(snapshot);
        self.with_write(|tx, _| {
            let mut t = tx.open_table(BLOCK_ENTITIES)?;
            t.insert(key.as_str(), value.as_slice())?;
            Ok(())
        })
    }

    pub fn load_craft_snapshot(&self, key: &str) -> Result<Option<CraftSnapshot>, StoreError> {
        let tx = self.db.begin_read()?;
        let Ok(t) = tx.open_table(BLOCK_ENTITIES) else { return Ok(None) };
        match t.get(key)? {
            Some(v) => Ok(Some(decode_craft_snapshot(v.value())?)),
            None => Ok(None),
        }
    }

    pub fn delete_craft_snapshot(&self, key: &str) -> Result<bool, StoreError> {
        self.with_write(|tx, _| {
            let mut t = tx.open_table(BLOCK_ENTITIES)?;
            let exists = t.get(key)?.is_some();
            if exists {
                t.remove(key)?;
            }
            Ok(exists)
        })
    }

    /// 全部快照（后台列表/清空用）。
    pub fn craft_snapshots(&self) -> Result<Vec<CraftSnapshot>, StoreError> {
        let tx = self.db.begin_read()?;
        let Ok(t) = tx.open_table(BLOCK_ENTITIES) else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for item in t.iter()? {
            let (_, v) = item?;
            out.push(decode_craft_snapshot(v.value())?);
        }
        Ok(out)
    }
}
