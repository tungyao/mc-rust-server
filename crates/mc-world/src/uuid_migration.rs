//! uuid_migration: P51 玩家存档从用户名键迁移到稳定 UUID 键。
//!
//! 背景：离线 uuid 过去每次登录随机 v4（见 `mc_protocol::packets::login`），
//! 背包只能按用户名键存（`player-<user>-<seed>.dat` / redb `players` 用户名 key）。
//! 现在离线身份改成 `OfflinePlayer:<name>` 派生的稳定 v3 UUID，启动时一次性把
//! 旧键复制成 UUID 键，旧文件改名 `.bak` 保留（可手工回退）；redb 旧键**保留**
//! 作为兼容兜底，新键不存在时仍可按用户名键读。
//!
//! 幂等：UUID 键已存在就跳过；报告文件保留历史记录。失败时回滚本次改名的文件
//! 并删掉新写的 UUID 文件（redb 复制是非破坏性的，旧键始终在）。
//! 存档格式/版本不变（v1/v2/v3 继续可读），XP / 背包 / 生存三字段 / 合成格按
//! 原始字节或原记录复制，不重新编码语义。

use std::path::{Path, PathBuf};

use mc_protocol::packets::{is_uuid_tag, stable_offline_uuid};
use serde::{Deserialize, Serialize};

use crate::inventory::PlayerInventory;
use crate::store::{StoreError, WorldStore};

/// `uuid-migration-<seed>.json`：一次/累计迁移的诊断记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UuidMigrationReport {
    pub seed: i64,
    /// 本次启动是否做了任何迁移（历史报告被继承时保留 true）。
    pub ran: bool,
    /// 已改名为 `.bak` 并写出 UUID 键的用户名文件标签。
    pub files: Vec<String>,
    /// 已复制成 UUID 键的 redb `players` 用户名键（旧键保留）。
    pub redb_keys: Vec<String>,
    /// UUID 键已存在、无需迁移的用户名文件/redb 键数（幂等计数）。
    pub skipped: usize,
    /// Unix 秒；仅诊断用，不引 chrono。
    pub timestamp: u64,
}

impl UuidMigrationReport {
    fn new(seed: i64) -> Self {
        Self {
            seed,
            ran: false,
            files: Vec::new(),
            redb_keys: Vec::new(),
            skipped: 0,
            timestamp: now_secs(),
        }
    }
}

pub fn report_path(dir: &Path, seed: i64) -> PathBuf {
    dir.join(format!("uuid-migration-{seed}.json"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// 不覆盖历史 `.bak`（P36 迁移可能已留一份）：冲突时退到 `.bak.1`、`.bak.2`。
fn free_backup_path(path: &Path) -> PathBuf {
    let base = backup_path(path);
    if !base.exists() {
        return base;
    }
    for n in 1..1000 {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".bak.{n}"));
        let candidate = path.with_file_name(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

/// 启动时调用（生产入口一次，redb/legacy 两种后端都可传 `store=None`）。
pub fn migrate_saves(
    dir: &Path,
    seed: i64,
    store: Option<&WorldStore>,
) -> Result<UuidMigrationReport, StoreError> {
    let report_file = report_path(dir, seed);
    let previous: Option<UuidMigrationReport> = std::fs::read(&report_file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let mut report = previous.clone().unwrap_or_else(|| UuidMigrationReport::new(seed));
    let before_files = report.files.len();
    let before_keys = report.redb_keys.len();
    let before_skipped = report.skipped;

    // 已改名/复制过的记录不再重复计数：按值合并去重。
    let push_unique = |list: &mut Vec<String>, value: String| {
        if !list.contains(&value) {
            list.push(value);
        }
    };

    // 1) redb players 表：复制旧 username 记录到 UUID 键，旧键保留做兜底。
    if let Some(store) = store {
        for key in store.player_keys()? {
            if is_uuid_tag(&key) {
                continue;
            }
            let uuid = stable_offline_uuid(&key).to_string();
            if store.copy_player_key(&key, &uuid)? {
                push_unique(&mut report.redb_keys, key);
            } else {
                report.skipped += 1;
            }
        }
    }

    // 2) 稀疏档：UUID 目标不存在时才搬；已存在的 username 文件视为兼容镜像跳过。
    //    done 记录本次改名，任何一步失败都回滚，绝不留下半迁移状态。
    let mut done: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new(); // (旧, bak, uuid)
    let result = (|| -> Result<(), StoreError> {
        for (tag, old) in crate::store::legacy_players(dir, seed) {
            if is_uuid_tag(&tag) {
                continue;
            }
            let uuid = stable_offline_uuid(&tag).to_string();
            let target = PlayerInventory::save_path(dir, &uuid, seed);
            if target.exists() {
                report.skipped += 1;
                continue;
            }
            // 逐字节复制原档（魔数/版本/XP/生存字段/合成格原样保留）。
            let tmp = target.with_extension("tmp");
            std::fs::write(&tmp, std::fs::read(&old)?)?;
            std::fs::rename(&tmp, &target)?;
            let bak = free_backup_path(&old);
            std::fs::rename(&old, &bak)?;
            done.push((old, bak, target));
            push_unique(&mut report.files, tag);
        }
        Ok(())
    })();

    if let Err(e) = result {
        for (old, bak, target) in done.into_iter().rev() {
            let _ = std::fs::remove_file(&target);
            if bak.exists() {
                let _ = std::fs::rename(&bak, &old);
            }
        }
        return Err(e);
    }

    let changed = report.files.len() != before_files
        || report.redb_keys.len() != before_keys
        || report.skipped != before_skipped;
    if !changed {
        return Ok(previous.unwrap_or(report));
    }
    report.ran = true;
    report.timestamp = now_secs();
    if let Some(parent) = report_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_file, serde_json::to_vec(&report).map_err(|e| {
        StoreError::BadRecord(format!("uuid migration report: {e}"))
    })?)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mc-p51-mig-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> PlayerInventory {
        let mut inv = PlayerInventory::new();
        inv.add_item(55, 5);
        inv.set_slot(3, Some((62, 4))); // 合成格随档保留
        inv.set_total_xp(23);
        inv.set_survival(crate::survival::SurvivalState {
            health: 12.5,
            food: 7,
            saturation: 2.0,
        });
        inv
    }

    #[test]
    fn file_migration_preserves_all_fields_and_is_idempotent() {
        let dir = tmp("file");
        let seed = 301;
        let uuid = stable_offline_uuid("OldPlayer").to_string();
        sample().save_to_dir(&dir, "OldPlayer", seed).unwrap();
        let first = migrate_saves(&dir, seed, None).unwrap();
        assert_eq!(first.files, vec!["OldPlayer".to_string()]);
        assert!(first.ran);
        let target = PlayerInventory::save_path(&dir, &uuid, seed);
        assert!(target.exists(), "UUID 主键文件必须生成");
        assert!(!PlayerInventory::save_path(&dir, "OldPlayer", seed).exists());
        assert!(backup_path(&PlayerInventory::save_path(&dir, "OldPlayer", seed)).exists());
        assert!(report_path(&dir, seed).exists());
        let (back, is_new) = PlayerInventory::load_from_dir(&dir, &uuid, seed).unwrap();
        assert!(!is_new);
        assert_eq!(back.total_xp(), 23);
        assert_eq!(back.get(3), Some((62, 4)));
        assert_eq!(back.get(36), Some((55, 5)));
        assert_eq!(back.survival().health, 12.5);
        // 二次启动：UUID 键已存在 → 跳过，报告不变，旧 .bak 保留。
        let bytes = std::fs::read(&target).unwrap();
        let second = migrate_saves(&dir, seed, None).unwrap();
        assert_eq!(second.files, first.files);
        assert_eq!(second.skipped, 0, "旧文件已改名 .bak，没有可跳过的 username 档");
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert!(backup_path(&PlayerInventory::save_path(&dir, "OldPlayer", seed)).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redb_migration_copies_uuid_key_and_keeps_legacy_fallback() {
        let dir = tmp("redb");
        let seed = 302;
        let store = WorldStore::open(&dir, seed).unwrap();
        let inv = sample();
        store.save_player("RedbUser", &inv).unwrap();
        let uuid = stable_offline_uuid("RedbUser").to_string();
        let first = migrate_saves(&dir, seed, Some(&store)).unwrap();
        assert_eq!(first.redb_keys, vec!["RedbUser".to_string()]);
        let migrated = store.load_player(&uuid).unwrap().unwrap();
        assert_eq!(migrated.total_xp(), 23);
        assert_eq!(migrated.get(3), Some((62, 4)));
        assert_eq!(migrated.survival(), inv.survival());
        assert!(store.load_player("RedbUser").unwrap().is_some(), "旧键保留兜底");
        let second = migrate_saves(&dir, seed, Some(&store)).unwrap();
        assert_eq!(second.redb_keys, first.redb_keys);
        assert_eq!(second.skipped, 1);
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_file_migration_rolls_back_renames() {
        let dir = tmp("rollback");
        let seed = 303;
        sample().save_to_dir(&dir, "A", seed).unwrap();
        sample().save_to_dir(&dir, "B", seed).unwrap();
        // 让第二份的临时写入路径不可写（目录占位），触发中途失败。
        let uuid_b = stable_offline_uuid("B").to_string();
        std::fs::create_dir_all(
            PlayerInventory::save_path(&dir, &uuid_b, seed).with_extension("tmp"),
        )
        .unwrap();
        assert!(migrate_saves(&dir, seed, None).is_err());
        // 第一份旧档已恢复，未留下新键；报告没写。
        assert!(PlayerInventory::save_path(&dir, "A", seed).exists());
        let uuid_a = stable_offline_uuid("A").to_string();
        assert!(!PlayerInventory::save_path(&dir, &uuid_a, seed).exists());
        assert!(!report_path(&dir, seed).exists());
        assert!(PlayerInventory::save_path(&dir, "B", seed).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
