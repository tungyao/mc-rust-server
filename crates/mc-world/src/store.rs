//! store: P36 世界存储原型 —— 自有格式（redb），**不做 Anvil 兼容**。
//!
//! 前提：`WorldGen` 确定性，地形本身不入盘；只存"与基底不同的方块"（稀疏 override）、
//! 玩家背包/XP、成长计时与一条逻辑编辑日志。磁盘真相源是单个 redb 文件
//! `world/state-<seed>.redb`（每 seed 一个库，延续 P17/P18 的 seed 隔离习惯）。
//!
//! P36.2 起本模块接入生产路径：`main.rs` 启动时打开/迁移本库，`MutableWorld` 把编辑
//! 暂存为 [`BlockEdit`]，`persist.rs` 的 60s/断线/退出触发点用一个写事务把
//! `blocks` + `journal` 落盘；玩家背包/XP 由 `DropState` 经 [`WorldStore::save_player`]
//! 写同一库。协议发包与权威判定逻辑不变，旧格式可通过配置开关回退。
//! 生产落地与迁移分阶段见 `docs/world-storage-plan.md`。
//!
//! 关键取舍：
//! - 选 `redb`（纯 Rust、MVCC、ACID、零拷贝读）而非自造 Anvil / LSM / 列式压缩格式；
//!   单写者 + 多读者并发模型天然匹配"net 线程写、ECS 读"的现状。
//! - 每个 flush 是一个 redb 写事务：崩溃时旧超级块仍有效，不需要 tmp+rename。
//! - `journal` 是逻辑编辑日志（事件溯源），`blocks`/`players`/`timers` 是同一事务内
//!   更新的物化视图；`blocks` 提供 O(log N) 随机读，`journal` 提供重放/审计/未来 undo。

use std::collections::HashMap;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};

use crate::inventory::{PlayerInventory, SLOT_COUNT};
use crate::mutable::WORLD_TOP_Y;
use crate::MIN_Y;

// ---------------------------------------------------------------------------
// schema 常量与表定义
// ---------------------------------------------------------------------------

/// 本原型的 on-disk schema 版本；不兼容变更必须递增并由迁移工具处理。
pub const SCHEMA_VERSION: u32 = 1;
/// 写入 meta 的格式标识，便于未来识别"这个库是哪个实现写的"。
pub const GENERATOR: &str = "mc-world::store redb v1 (P36 prototype)";

const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const BLOCKS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("blocks");
const JOURNAL: TableDefinition<u64, &[u8]> = TableDefinition::new("journal");
const PLAYERS: TableDefinition<&str, &[u8]> = TableDefinition::new("players");
const TIMERS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("timers");

const META_SCHEMA: &str = "schema";
const META_SEED: &str = "seed";
const META_SEQ: &str = "journal_seq";
const META_TICK: &str = "tick_offset";
const META_GENERATOR: &str = "generator";

/// 日志操作码（与 `Op` 一一对应，落盘 u8）。
pub mod op_code {
    pub const BLOCK_SET: u8 = 1;
    pub const BLOCK_CLEAR: u8 = 2;
    pub const PLAYER_SAVE: u8 = 3;
    pub const TIMER_SET: u8 = 4;
    /// P39：删除一条 timer（挖掉/到期/骨粉长树时清理）。
    pub const TIMER_CLEAR: u8 = 5;
}

/// 逻辑编辑类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    BlockSet,
    BlockClear,
    PlayerSave,
    TimerSet,
    /// P39：timer 行被删除，`new=u16::MAX` 表示删除条目。
    TimerClear,
}

impl Op {
    fn code(self) -> u8 {
        match self {
            Op::BlockSet => op_code::BLOCK_SET,
            Op::BlockClear => op_code::BLOCK_CLEAR,
            Op::PlayerSave => op_code::PLAYER_SAVE,
            Op::TimerSet => op_code::TIMER_SET,
            Op::TimerClear => op_code::TIMER_CLEAR,
        }
    }

    fn from_code(c: u8) -> Result<Self, StoreError> {
        match c {
            op_code::BLOCK_SET => Ok(Op::BlockSet),
            op_code::BLOCK_CLEAR => Ok(Op::BlockClear),
            op_code::PLAYER_SAVE => Ok(Op::PlayerSave),
            op_code::TIMER_SET => Ok(Op::TimerSet),
            op_code::TIMER_CLEAR => Ok(Op::TimerClear),
            other => Err(StoreError::BadRecord(format!("unknown journal op {other}"))),
        }
    }
}

/// 一条可重放的编辑记录（33 字节定长值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalRecord {
    pub seq: u64,
    pub op: Op,
    /// 编辑发生时的全局 tick（生长计时恢复用；玩家背包/迁移可为 0）。
    pub tick: u64,
    /// conn_id；系统产生的编辑（ticker / 迁移）为 0。
    pub actor: u64,
    pub pos: (i32, i32, i32),
    /// 旧值（方块 ID / timer kind）；无旧值为 u16::MAX。
    pub old: u16,
    /// 新值（方块 ID / timer stage）；`BlockClear` 用 u16::MAX 表示删除条目。
    pub new: u16,
}

/// 成长计时种类：与 §5.18–20 的作物/树苗/树叶观察一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimerKind {
    Crop = 1,
    Sapling = 2,
    Leaf = 3,
}

impl TimerKind {
    fn code(self) -> u8 {
        self as u8
    }

    fn from_code(c: u8) -> Result<Self, StoreError> {
        match c {
            1 => Ok(TimerKind::Crop),
            2 => Ok(TimerKind::Sapling),
            3 => Ok(TimerKind::Leaf),
            other => Err(StoreError::BadRecord(format!("unknown timer kind {other}"))),
        }
    }
}

/// 一条持久化的成长计时（10 字节定长值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerRecord {
    pub kind: TimerKind,
    pub pos: (i32, i32, i32),
    /// 种下/登记时的全局 tick。配合 meta 的 `tick_offset` 跨重启续算。
    pub planted_tick: u64,
    pub stage: u8,
    /// 作物规格 ID（未来按 `CropSpec` 查表；原型直接存编号）。
    pub spec: u8,
}

/// 一条待刷盘的方块编辑（内存热层 → redb 的桥）。
///
/// `old`/`new` 采用 override 口径：`u16::MAX` = 没有 override（回落到确定性基底）。
/// `new == u16::MAX` 表示设回基底值，刷盘时记 `BlockClear`；否则记 `BlockSet`。
/// `actor` 是 conn_id（系统/ticker/未标注的调用方为 0），`tick` 是全局物理 tick。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEdit {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub old: u16,
    pub new: u16,
    pub actor: u64,
    pub tick: u64,
}

/// 一次性迁移报告（写 `world/migration-<seed>.json`，失败只影响诊断）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub seed: i64,
    /// 本次启动是否真的执行了迁移（以 redb 库新建为唯一判据）。
    pub ran: bool,
    /// 迁入的 P17W 方块条数。
    pub blocks: usize,
    /// 迁入的 P18I 玩家用户名。
    pub players: Vec<String>,
    /// 已改名为 `.bak` 的旧文件数。
    pub renamed: usize,
    /// redb 已存在但旧文件仍在（按设计忽略，供调用方 warn）。
    pub legacy_ignored: usize,
    /// Unix 秒；仅诊断用，不引 chrono。
    pub timestamp: u64,
}

impl MigrationReport {
    fn skipped(seed: i64, legacy_ignored: usize) -> Self {
        Self {
            seed,
            ran: false,
            blocks: 0,
            players: Vec::new(),
            renamed: 0,
            legacy_ignored,
            timestamp: now_secs(),
        }
    }

    /// 手写极简 JSON：mc-world 不依赖 serde_json，字段全是标量/字符串数组。
    fn to_json(&self) -> String {
        // 用户名是 Minecraft 名（ASCII）；过滤掉会破坏 JSON 的字符即可，不引 serde_json。
        let players = self
            .players
            .iter()
            .map(|p| {
                let safe: String = p
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                    .collect();
                format!("\"{safe}\"")
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"seed\":{},\"ran\":{},\"blocks\":{},\"players\":[{}],\"renamed\":{},\"legacy_ignored\":{},\"timestamp\":{}}}",
            self.seed, self.ran, self.blocks, players, self.renamed, self.legacy_ignored, self.timestamp
        )
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 错误
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("redb: {0}")]
    Redb(#[from] redb::Error),
    #[error("legacy save corrupt: {0}")]
    Legacy(String),
    #[error("schema version {0} unsupported (expected {1})")]
    Version(u32, u32),
    #[error("seed mismatch: file={0} requested={1}")]
    Seed(i64, i64),
    #[error("bad record: {0}")]
    BadRecord(String),
}

macro_rules! impl_redb_from {
    ($($t:ty),+ $(,)?) => {
        $(
            impl From<$t> for StoreError {
                fn from(e: $t) -> Self {
                    StoreError::Redb(redb::Error::from(e))
                }
            }
        )+
    };
}

impl_redb_from!(
    redb::DatabaseError,
    redb::TableError,
    redb::StorageError,
    redb::TransactionError,
    redb::CommitError,
);

// ---------------------------------------------------------------------------
// 字节编码工具（定长、BE，避免引入 bincode/postcard 等额外依赖）
// ---------------------------------------------------------------------------

fn take<const N: usize>(buf: &[u8], off: usize) -> Result<[u8; N], StoreError> {
    buf.get(off..off + N)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| StoreError::BadRecord(format!("record truncated at {off} (need {N} bytes)")))
}

fn block_key(x: i32, y: i32, z: i32) -> [u8; 12] {
    let mut k = [0u8; 12];
    k[0..4].copy_from_slice(&x.to_be_bytes());
    k[4..8].copy_from_slice(&y.to_be_bytes());
    k[8..12].copy_from_slice(&z.to_be_bytes());
    k
}

fn timer_key(kind: TimerKind, pos: (i32, i32, i32)) -> [u8; 13] {
    let mut k = [0u8; 13];
    k[0] = kind.code();
    k[1..5].copy_from_slice(&pos.0.to_be_bytes());
    k[5..9].copy_from_slice(&pos.1.to_be_bytes());
    k[9..13].copy_from_slice(&pos.2.to_be_bytes());
    k
}

fn encode_journal(r: &JournalRecord) -> Vec<u8> {
    let mut b = Vec::with_capacity(33);
    b.push(r.op.code());
    b.extend_from_slice(&r.tick.to_be_bytes());
    b.extend_from_slice(&r.actor.to_be_bytes());
    b.extend_from_slice(&r.pos.0.to_be_bytes());
    b.extend_from_slice(&r.pos.1.to_be_bytes());
    b.extend_from_slice(&r.pos.2.to_be_bytes());
    b.extend_from_slice(&r.old.to_be_bytes());
    b.extend_from_slice(&r.new.to_be_bytes());
    b
}

fn decode_journal(seq: u64, b: &[u8]) -> Result<JournalRecord, StoreError> {
    if b.len() != 33 {
        return Err(StoreError::BadRecord(format!("journal value len {} != 33", b.len())));
    }
    Ok(JournalRecord {
        seq,
        op: Op::from_code(b[0])?,
        tick: u64::from_be_bytes(take::<8>(b, 1)?),
        actor: u64::from_be_bytes(take::<8>(b, 9)?),
        pos: (
            i32::from_be_bytes(take::<4>(b, 17)?),
            i32::from_be_bytes(take::<4>(b, 21)?),
            i32::from_be_bytes(take::<4>(b, 25)?),
        ),
        old: u16::from_be_bytes(take::<2>(b, 29)?),
        new: u16::from_be_bytes(take::<2>(b, 31)?),
    })
}

/// 玩家记录：`version u8 | selected u8 | state i32 | xp i32 | slot_count u8`
/// + 每个非空槽 `slot u8 | item i32 | count i32`（9B）。
fn encode_player(inv: &PlayerInventory) -> Vec<u8> {
    let mut b = Vec::with_capacity(11 + SLOT_COUNT * 9);
    b.push(1);
    b.push(inv.selected());
    b.extend_from_slice(&inv.state_id().to_be_bytes());
    b.extend_from_slice(&inv.total_xp().to_be_bytes());
    let entries: Vec<(usize, i32, i32)> = (0..SLOT_COUNT)
        .filter_map(|i| inv.get(i).map(|(id, c)| (i, id, c)))
        .collect();
    b.push(entries.len() as u8);
    for (slot, id, count) in entries {
        b.push(slot as u8);
        b.extend_from_slice(&id.to_be_bytes());
        b.extend_from_slice(&count.to_be_bytes());
    }
    b
}

fn decode_player(b: &[u8]) -> Result<PlayerInventory, StoreError> {
    if b.len() < 11 || b[0] != 1 {
        return Err(StoreError::BadRecord("bad player record header".into()));
    }
    let selected = b[1];
    let state = i32::from_be_bytes(take::<4>(b, 2)?);
    let xp = i32::from_be_bytes(take::<4>(b, 6)?);
    let count = b[10] as usize;
    if count > SLOT_COUNT {
        return Err(StoreError::BadRecord(format!("too many player slots {count}")));
    }
    let mut inv = PlayerInventory::new();
    inv.set_selected(selected);
    inv.set_state_id(state);
    inv.set_total_xp(xp);
    let mut off = 11;
    for _ in 0..count {
        let slot = b[off] as usize;
        let id = i32::from_be_bytes(take::<4>(b, off + 1)?);
        let n = i32::from_be_bytes(take::<4>(b, off + 5)?);
        off += 9;
        if slot >= SLOT_COUNT || id <= 0 || n <= 0 {
            return Err(StoreError::BadRecord(format!("invalid player slot {slot}/{id}/{n}")));
        }
        inv.set_slot(slot, Some((id, n)));
    }
    if off != b.len() {
        return Err(StoreError::BadRecord("trailing bytes in player record".into()));
    }
    Ok(inv)
}

fn encode_timer(t: &TimerRecord) -> Vec<u8> {
    let mut b = Vec::with_capacity(10);
    b.extend_from_slice(&t.planted_tick.to_be_bytes());
    b.push(t.stage);
    b.push(t.spec);
    b
}

fn decode_timer(kind: TimerKind, pos: (i32, i32, i32), b: &[u8]) -> Result<TimerRecord, StoreError> {
    if b.len() != 10 {
        return Err(StoreError::BadRecord(format!("timer value len {} != 10", b.len())));
    }
    Ok(TimerRecord {
        kind,
        pos,
        planted_tick: u64::from_be_bytes(take::<8>(b, 0)?),
        stage: b[8],
        spec: b[9],
    })
}

// ---------------------------------------------------------------------------
// meta 读写
// ---------------------------------------------------------------------------

fn meta_get(tx: &WriteTransaction, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
    let t = tx.open_table(META)?;
    let out = t.get(key)?.map(|g| g.value().to_vec());
    Ok(out)
}

fn meta_u64(tx: &WriteTransaction, key: &str) -> Result<Option<u64>, StoreError> {
    match meta_get(tx, key)? {
        Some(b) => Ok(Some(u64::from_be_bytes(take::<8>(&b, 0)?))),
        None => Ok(None),
    }
}

fn meta_put(tx: &WriteTransaction, key: &str, value: &[u8]) -> Result<(), StoreError> {
    let mut t = tx.open_table(META)?;
    t.insert(key, value)?;
    Ok(())
}

/// 只读 meta（不占用唯一写者）。
fn meta_u64_read(db: &Database, key: &str) -> Result<Option<u64>, StoreError> {
    let tx = db.begin_read()?;
    let t = tx.open_table(META)?;
    let out = t.get(key)?.map(|g| g.value().to_vec());
    drop(t);
    match out {
        Some(b) => Ok(Some(u64::from_be_bytes(take::<8>(&b, 0)?))),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// WorldStore
// ---------------------------------------------------------------------------

/// 一个 seed 的存储库。`Database` 内部自带文件锁与 MVCC：`WorldStore` 可 `Arc` 共享，
/// 同一时刻只有一个写事务，读事务并发无锁。
#[derive(Debug)]
pub struct WorldStore {
    db: Database,
    seed: i64,
    path: PathBuf,
}

impl WorldStore {
    /// 库文件路径：`dir/state-<seed>.redb`（seed 隔离，与旧档同目录但不同名）。
    pub fn store_path(dir: &Path, seed: i64) -> PathBuf {
        dir.join(format!("state-{seed}.redb"))
    }

    /// 打开（不存在则创建并初始化 schema）。生产接入时只在启动调用一次。
    pub fn open(dir: &Path, seed: i64) -> Result<Self, StoreError> {
        Self::open_at(Self::store_path(dir, seed), seed)
    }

    /// 直接从路径打开；测试用它验证 seed/schema 防串档。
    pub fn open_at(path: PathBuf, seed: i64) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let exists = path.exists();
        let db = if exists { Database::open(&path)? } else { Database::create(&path)? };
        if !exists {
            let tx = db.begin_write()?;
            // open_table 即建表；显式打开保证文件布局稳定。
            let _ = tx.open_table(META)?;
            let _ = tx.open_table(BLOCKS)?;
            let _ = tx.open_table(JOURNAL)?;
            let _ = tx.open_table(PLAYERS)?;
            let _ = tx.open_table(TIMERS)?;
            let v = SCHEMA_VERSION.to_be_bytes();
            meta_put(&tx, META_SCHEMA, v.as_slice())?;
            let s = seed.to_be_bytes();
            meta_put(&tx, META_SEED, s.as_slice())?;
            let zero = 0u64.to_be_bytes();
            meta_put(&tx, META_SEQ, zero.as_slice())?;
            meta_put(&tx, META_TICK, zero.as_slice())?;
            meta_put(&tx, META_GENERATOR, GENERATOR.as_bytes())?;
            tx.commit()?;
        } else {
            let tx = db.begin_write()?;
            let schema = match meta_get(&tx, META_SCHEMA)? {
                Some(b) => u32::from_be_bytes(take::<4>(&b, 0)?),
                None => return Err(StoreError::Legacy("meta.schema missing".into())),
            };
            if schema != SCHEMA_VERSION {
                return Err(StoreError::Version(schema, SCHEMA_VERSION));
            }
            let file_seed = match meta_get(&tx, META_SEED)? {
                Some(b) => i64::from_be_bytes(take::<8>(&b, 0)?),
                None => return Err(StoreError::Legacy("meta.seed missing".into())),
            };
            if file_seed != seed {
                return Err(StoreError::Seed(file_seed, seed));
            }
            // 只读校验，不提交任何变更。
            drop(tx);
        }
        Ok(Self { db, seed, path })
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 当前全局 tick 偏移：重启时用它初始化 `DropState::tick_count`，使持久化的
    /// `planted_tick` 与 `now.saturating_sub(planted)` 仍然自洽（§5.18–20）。
    pub fn tick_offset(&self) -> Result<u64, StoreError> {
        Ok(meta_u64_read(&self.db, META_TICK)?.unwrap_or(0))
    }

    pub fn set_tick_offset(&self, tick: u64) -> Result<(), StoreError> {
        self.with_write(|tx, _| {
            let b = tick.to_be_bytes();
            meta_put(tx, META_TICK, b.as_slice())
        })
    }

    /// P36.2 生产入口：打开（必要时先迁移）并保存迁移报告。
    ///
    /// 幂等判据只有一条：`state-<seed>.redb` 在打开前是否存在。新建库时扫
    /// `overrides-<seed>.dat` 与 `player-*-<seed>.dat`，按文档事务迁入，写
    /// `migration-<seed>.json`，旧文件改名 `.bak` 保留（不删除）。任一步失败会
    /// 回滚新建的库文件，下次启动重试；已存在库时只报告遗留旧档数量，不读取。
    pub fn open_with_migration(dir: &Path, seed: i64) -> Result<(Self, MigrationReport), StoreError> {
        let path = Self::store_path(dir, seed);
        let existed = path.exists();
        let store = Self::open(dir, seed)?;
        if existed {
            let ignored = legacy_files(dir, seed).len();
            return Ok((store, MigrationReport::skipped(seed, ignored)));
        }
        let result = (|| {
            let mut report = MigrationReport {
                seed,
                ran: true,
                blocks: 0,
                players: Vec::new(),
                renamed: 0,
                legacy_ignored: 0,
                timestamp: now_secs(),
            };
            let mut to_rename = Vec::new();
            // 方块档：一个事务内写全部 blocks + journal(BlockSet)。
            let p17 = crate::mutable::MutableWorld::save_path(dir, seed);
            if p17.exists() {
                report.blocks = store.migrate_legacy_p17(dir, seed)?;
                to_rename.push(p17);
            }
            // 背包档：扫描真实文件名（用户名键），v1/v2 由 load_from_dir 兼容。
            for (tag, p18) in legacy_players(dir, seed) {
                if store.migrate_legacy_p18(dir, &tag, seed)? {
                    report.players.push(tag);
                    to_rename.push(p18);
                }
            }
            // 全部数据事务成功后才改名旧档；改名失败则回滚改名 + 删除新库，下次重试。
            let mut done = Vec::new();
            for path in &to_rename {
                if rename_backup(path).is_ok() {
                    done.push(path);
                }
            }
            if done.len() != to_rename.len() {
                for path in &done {
                    let _ = std::fs::rename(backup_path(path), path);
                }
                return Err(StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "renaming legacy saves to .bak failed",
                )));
            }
            report.renamed = done.len();
            // 报告写失败只影响诊断，不影响数据。
            let report_path = dir.join(format!("migration-{seed}.json"));
            if let Some(parent) = report_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::write(report_path, report.to_json());
            Ok(report)
        })();
        match result {
            Ok(report) => Ok((store, report)),
            Err(e) => {
                // 迁移失败：删掉刚建的库，保留全部旧档，下次启动重跑（幂等）。
                drop(store);
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
        }
    }

    /// 一个写事务内提交一批方块编辑：`blocks` 物化视图 + `journal(old→new, actor, tick)`。
    /// 调用方（内存热层）只在成功后清 dirty，失败可原样重试。
    pub fn flush_blocks(&self, edits: &[BlockEdit]) -> Result<usize, StoreError> {
        if edits.is_empty() {
            return Ok(0);
        }
        self.with_write(|tx, seq| {
            for e in edits {
                let op = if e.new == u16::MAX { Op::BlockClear } else { Op::BlockSet };
                Self::put_block(tx, seq, e.x, e.y, e.z, e.new, op, e.tick, e.actor)?;
            }
            Ok(edits.len())
        })
    }

    /// 统一写入口：所有修改都在一个 redb 写事务内完成，日志序号同事务递增。
    fn with_write<R>(
        &self,
        f: impl FnOnce(&WriteTransaction, &mut u64) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let tx = self.db.begin_write()?;
        let mut seq = meta_u64(&tx, META_SEQ)?.unwrap_or(0);
        let out = f(&tx, &mut seq)?;
        let b = seq.to_be_bytes();
        meta_put(&tx, META_SEQ, b.as_slice())?;
        tx.commit()?;
        Ok(out)
    }

    /// 在已有事务内写一条方块记录 + 一条日志（迁移/批量刷盘复用）。
    fn put_block(
        tx: &WriteTransaction,
        seq: &mut u64,
        x: i32,
        y: i32,
        z: i32,
        id: u16,
        op: Op,
        tick: u64,
        actor: u64,
    ) -> Result<(), StoreError> {
        let key = block_key(x, y, z);
        let mut blocks = tx.open_table(BLOCKS)?;
        let old = blocks
            .get(key.as_slice())?
            .map(|g| u16::from_be_bytes(g.value().try_into().unwrap()))
            .unwrap_or(u16::MAX);
        if op == Op::BlockClear {
            blocks.remove(key.as_slice())?;
        } else {
            let v = id.to_be_bytes();
            blocks.insert(key.as_slice(), v.as_slice())?;
        }
        drop(blocks);
        let rec = JournalRecord {
            seq: *seq,
            op,
            tick,
            actor,
            pos: (x, y, z),
            old,
            new: if op == Op::BlockClear { u16::MAX } else { id },
        };
        append_journal(tx, seq, &rec)
    }

    /// 读单方块（只查稀疏 override 表；未覆盖的方块由调用方回落到确定性基底）。
    /// y 越界返回 None（与 `MutableWorld::get` 的空气口径区分）。
    pub fn block(&self, x: i32, y: i32, z: i32) -> Result<Option<u16>, StoreError> {
        if !(MIN_Y..=WORLD_TOP_Y).contains(&y) {
            return Ok(None);
        }
        let tx = self.db.begin_read()?;
        let t = tx.open_table(BLOCKS)?;
        let key = block_key(x, y, z);
        Ok(t.get(key.as_slice())?
            .map(|g| u16::from_be_bytes(g.value().try_into().unwrap())))
    }

    /// 写一条方块编辑（`tick=0, actor=0` 的便捷入口）。
    pub fn set_block(&self, x: i32, y: i32, z: i32, id: u16) -> Result<(), StoreError> {
        self.set_block_with(x, y, z, id, 0, 0)
    }

    /// 带 tick/actor 的写（生产接入时由 net 线程传 conn_id，ticker 传 0）。
    pub fn set_block_with(
        &self,
        x: i32,
        y: i32,
        z: i32,
        id: u16,
        tick: u64,
        actor: u64,
    ) -> Result<(), StoreError> {
        if !(MIN_Y..=WORLD_TOP_Y).contains(&y) {
            return Ok(());
        }
        self.with_write(|tx, seq| {
            Self::put_block(tx, seq, x, y, z, id, Op::BlockSet, tick, actor)
        })
    }

    /// 删除 override 条目（设回基底值时调用，保持稀疏）。
    pub fn clear_block(&self, x: i32, y: i32, z: i32) -> Result<(), StoreError> {
        self.with_write(|tx, seq| {
            Self::put_block(tx, seq, x, y, z, 0, Op::BlockClear, 0, 0)
        })
    }

    /// 写玩家背包 + XP（一个事务内更新 `players` 与 `journal`）。
    pub fn save_player(&self, tag: &str, inv: &PlayerInventory) -> Result<(), StoreError> {
        let bytes = encode_player(inv);
        self.with_write(|tx, seq| {
            {
                let mut t = tx.open_table(PLAYERS)?;
                t.insert(tag, bytes.as_slice())?;
            }
            let tick = meta_u64(tx, META_TICK)?.unwrap_or(0);
            let rec = JournalRecord {
                seq: *seq,
                op: Op::PlayerSave,
                tick,
                actor: 0,
                pos: (0, 0, 0),
                old: 0,
                new: 0,
            };
            append_journal(tx, seq, &rec)
        })
    }

    /// 读玩家背包 + XP；无记录返回 None（生产层据此给新背包开局石头）。
    pub fn load_player(&self, tag: &str) -> Result<Option<PlayerInventory>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(PLAYERS)?;
        match t.get(tag)? {
            Some(g) => Ok(Some(decode_player(g.value())?)),
            None => Ok(None),
        }
    }

    /// 写成长计时（作物/树苗/树叶观察），让 §5.18–20 的跨重启计时可恢复。
    pub fn set_timer(
        &self,
        kind: TimerKind,
        pos: (i32, i32, i32),
        planted_tick: u64,
        stage: u8,
        spec: u8,
    ) -> Result<(), StoreError> {
        let key = timer_key(kind, pos);
        let value = encode_timer(&TimerRecord { kind, pos, planted_tick, stage, spec });
        self.with_write(|tx, seq| {
            {
                let mut t = tx.open_table(TIMERS)?;
                t.insert(key.as_slice(), value.as_slice())?;
            }
            let rec = JournalRecord {
                seq: *seq,
                op: Op::TimerSet,
                tick: planted_tick,
                actor: 0,
                pos,
                old: kind.code() as u16,
                new: stage as u16,
            };
            append_journal(tx, seq, &rec)
        })
    }

    pub fn timer(&self, kind: TimerKind, pos: (i32, i32, i32)) -> Result<Option<TimerRecord>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(TIMERS)?;
        let key = timer_key(kind, pos);
        let stored_kind = TimerKind::from_code(key[0])?;
        debug_assert_eq!(stored_kind, kind);
        match t.get(key.as_slice())? {
            Some(g) => Ok(Some(decode_timer(kind, pos, g.value())?)),
            None => Ok(None),
        }
    }

    /// P39 生产入口：一批 timer 更新 + 当前全局 tick 基线，在**同一个事务**落盘。
    ///
    /// `tick_offset` 是 `DropState::tick_count`（全服唯一物理 ticker），重启后
    /// `now = tick_offset`，于是 `now - planted_tick` 继续按原进度推进（§3.6）。
    /// 每条更新仍记一条 `TimerSet` journal，`tick` 保持 planted tick 口径不变。
    pub fn set_timers(&self, timers: &[TimerRecord], tick_offset: u64) -> Result<usize, StoreError> {
        if timers.is_empty() {
            return Ok(0);
        }
        self.with_write(|tx, seq| {
            {
                let mut t = tx.open_table(TIMERS)?;
                for rec in timers {
                    let key = timer_key(rec.kind, rec.pos);
                    let value = encode_timer(rec);
                    t.insert(key.as_slice(), value.as_slice())?;
                }
            }
            let b = tick_offset.to_be_bytes();
            meta_put(tx, META_TICK, b.as_slice())?;
            for rec in timers {
                append_journal(
                    tx,
                    seq,
                    &JournalRecord {
                        seq: *seq,
                        op: Op::TimerSet,
                        tick: rec.planted_tick,
                        actor: 0,
                        pos: rec.pos,
                        old: rec.kind.code() as u16,
                        new: rec.stage as u16,
                    },
                )?;
            }
            Ok(timers.len())
        })
    }

    /// P39：删除一条 timer（挖掉/取消/到期完成），并记一条 `TimerClear` journal。
    /// 返回 `true` 表示本来有这一行；`tick` 沿用当前 tick_offset 基线。
    pub fn delete_timer(&self, kind: TimerKind, pos: (i32, i32, i32)) -> Result<bool, StoreError> {
        self.with_write(|tx, seq| {
            let key = timer_key(kind, pos);
            let removed = {
                let mut t = tx.open_table(TIMERS)?;
                let was_present = t.remove(key.as_slice())?.is_some();
                was_present
            };
            if removed {
                let tick = meta_u64(tx, META_TICK)?.unwrap_or(0);
                append_journal(
                    tx,
                    seq,
                    &JournalRecord {
                        seq: *seq,
                        op: Op::TimerClear,
                        tick,
                        actor: 0,
                        pos,
                        old: kind.code() as u16,
                        new: u16::MAX,
                    },
                )?;
            }
            Ok(removed)
        })
    }

    /// P39：启动认领用——列出当前全部 timer（`DropState::with_store` 一次读入内存）。
    pub fn timers(&self) -> Result<Vec<TimerRecord>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(TIMERS)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (k, v) = item?;
            let kb = k.value();
            let kind = TimerKind::from_code(kb[0])?;
            let pos = (
                i32::from_be_bytes(take::<4>(kb, 1)?),
                i32::from_be_bytes(take::<4>(kb, 5)?),
                i32::from_be_bytes(take::<4>(kb, 9)?),
            );
            out.push(decode_timer(kind, pos, v.value())?);
        }
        Ok(out)
    }

    /// 日志条数（= 下一个 seq）。
    pub fn journal_len(&self) -> Result<u64, StoreError> {
        Ok(meta_u64_read(&self.db, META_SEQ)?.unwrap_or(0))
    }

    /// 按 seq 升序列出全部编辑（事件溯源回放入口）。
    pub fn replay(&self) -> Result<Vec<JournalRecord>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(JOURNAL)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (k, v) = item?;
            out.push(decode_journal(k.value(), v.value())?);
        }
        Ok(out)
    }

    /// 只回放方块编辑，得到"当前 override 集合"；用于自检 `blocks` 物化视图。
    /// `BlockClear` 的 `new=u16::MAX` 表示删除条目。
    pub fn replay_blocks(&self) -> Result<HashMap<(i32, i32, i32), u16>, StoreError> {
        let mut out = HashMap::new();
        for rec in self.replay()? {
            match rec.op {
                Op::BlockSet => {
                    out.insert(rec.pos, rec.new);
                }
                Op::BlockClear => {
                    out.remove(&rec.pos);
                }
                Op::PlayerSave | Op::TimerSet | Op::TimerClear => {}
            }
        }
        Ok(out)
    }

    /// 列出当前所有方块 override（诊断/迁移校验用）。
    pub fn blocks(&self) -> Result<Vec<((i32, i32, i32), u16)>, StoreError> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(BLOCKS)?;
        let mut out = Vec::new();
        for item in t.iter()? {
            let (k, v) = item?;
            let kb = k.value();
            let pos = (
                i32::from_be_bytes(take::<4>(kb, 0)?),
                i32::from_be_bytes(take::<4>(kb, 4)?),
                i32::from_be_bytes(take::<4>(kb, 8)?),
            );
            out.push((pos, u16::from_be_bytes(v.value().try_into().unwrap())));
        }
        Ok(out)
    }

    /// 一次性迁移旧 P17W 方块档（在单个事务内写入所有记录，避免逐条提交）。
    /// 旧文件保持原样，由调用方决定改名 `.bak` 或保留（见设计文档迁移策略）。
    pub fn migrate_legacy_p17(&self, dir: &Path, seed: i64) -> Result<usize, StoreError> {
        let entries = parse_p17(&crate::mutable::MutableWorld::save_path(dir, seed), seed)?;
        self.with_write(|tx, seq| {
            let tick = meta_u64(tx, META_TICK)?.unwrap_or(0);
            for ((x, y, z), id) in &entries {
                Self::put_block(tx, seq, *x, *y, *z, *id, Op::BlockSet, tick, 0)?;
            }
            Ok(entries.len())
        })
    }

    /// 一次性迁移旧 P18I 背包/XP 档（v1/v2 都由 `load_from_dir` 兼容读取）。
    pub fn migrate_legacy_p18(&self, dir: &Path, tag: &str, seed: i64) -> Result<bool, StoreError> {
        let path = PlayerInventory::save_path(dir, tag, seed);
        if !path.exists() {
            return Ok(false);
        }
        let (inv, is_new) = PlayerInventory::load_from_dir(dir, tag, seed)
            .map_err(|e| StoreError::Legacy(format!("{}: {e}", path.display())))?;
        if is_new {
            return Ok(false);
        }
        self.save_player(tag, &inv)?;
        Ok(true)
    }

    /// 触发 redb 空闲页回收（可选维护操作；生产可在低峰期调用）。
    pub fn compact(&mut self) -> Result<bool, redb::CompactionError> {
        self.db.compact()
    }
}

fn append_journal(tx: &WriteTransaction, seq: &mut u64, rec: &JournalRecord) -> Result<(), StoreError> {
    let mut t = tx.open_table(JOURNAL)?;
    let bytes = encode_journal(rec);
    t.insert(*seq, bytes.as_slice())?;
    *seq += 1;
    Ok(())
}

/// `path` → `path.bak`（旧档改名保留，回退时改回原名即可）。
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

fn rename_backup(path: &Path) -> std::io::Result<()> {
    let bak = backup_path(path);
    if bak.exists() {
        let _ = std::fs::remove_file(&bak);
    }
    std::fs::rename(path, bak)
}

/// 扫描 `player-<user>-<seed>.dat`（只认当前 seed；用户名可能含 '-'，取中间全部）。
fn legacy_players(dir: &Path, seed: i64) -> Vec<(String, PathBuf)> {
    let prefix = "player-";
    let suffix = format!("-{seed}.dat");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix(prefix) {
                if let Some(tag) = rest.strip_suffix(&suffix) {
                    if !tag.is_empty() {
                        out.push((tag.to_string(), entry.path()));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 启动时发现库已存在但旧文件仍在：只计数供调用方 warn，绝不读取/覆盖。
fn legacy_files(dir: &Path, seed: i64) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let p17 = crate::mutable::MutableWorld::save_path(dir, seed);
    if p17.exists() {
        out.push(p17);
    }
    out.extend(legacy_players(dir, seed).into_iter().map(|(_, p)| p));
    out
}

/// 解析旧 `overrides-<seed>.dat`（P17W v1）：魔数/版本/seed/条数 + 14B 条目。
/// seed 必须与目标库一致，防止把别的世界迁进来。
fn parse_p17(path: &Path, seed: i64) -> Result<Vec<((i32, i32, i32), u16)>, StoreError> {
    let mut f = File::open(path)?;
    let mut head = [0u8; 17];
    f.read_exact(&mut head)
        .map_err(|e| StoreError::Legacy(format!("{}: header ({e})", path.display())))?;
    if &head[0..4] != b"P17W" {
        return Err(StoreError::Legacy(format!("{}: bad magic", path.display())));
    }
    if head[4] != 1 {
        return Err(StoreError::Legacy(format!("{}: unsupported version {}", path.display(), head[4])));
    }
    let file_seed = i64::from_be_bytes(take::<8>(&head, 5)?);
    if file_seed != seed {
        return Err(StoreError::Legacy(format!(
            "{}: seed {file_seed} != target {seed}",
            path.display()
        )));
    }
    let count = u32::from_be_bytes(take::<4>(&head, 13)?) as usize;
    if count > 16_000_000 {
        return Err(StoreError::Legacy(format!("{}: too many entries {count}", path.display())));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let mut e = [0u8; 14];
        f.read_exact(&mut e)
            .map_err(|err| StoreError::Legacy(format!("{}: entry truncated ({err})", path.display())))?;
        let x = i32::from_be_bytes(take::<4>(&e, 0)?);
        let y = i32::from_be_bytes(take::<4>(&e, 4)?);
        let z = i32::from_be_bytes(take::<4>(&e, 8)?);
        let id = u16::from_be_bytes(take::<2>(&e, 12)?);
        if (MIN_Y..=WORLD_TOP_Y).contains(&y) {
            out.push(((x, y, z), id));
        }
    }
    let mut tail = [0u8; 1];
    if f.read(&mut tail)? != 0 {
        return Err(StoreError::Legacy(format!("{}: trailing bytes", path.display())));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 单测：最小闭环
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AIR_BLOCK_ID, DIRT_BLOCK_ID, STONE_BLOCK_ID};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn tmp(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("mc-p36-store-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn block_edits_survive_close_and_reopen() {
        let dir = tmp("blocks");
        let seed = 42;
        {
            let s = WorldStore::open(&dir, seed).unwrap();
            assert_eq!(s.seed(), seed);
            assert!(s.path().ends_with("state-42.redb"));
            s.set_block(10, 0, 10, STONE_BLOCK_ID).unwrap();
            s.set_block_with(-5, 100, -7, DIRT_BLOCK_ID, 7, 99).unwrap();
            assert_eq!(s.block(10, 0, 10).unwrap(), Some(STONE_BLOCK_ID));
            assert_eq!(s.journal_len().unwrap(), 2);
        }
        let s = WorldStore::open(&dir, seed).unwrap();
        assert_eq!(s.block(10, 0, 10).unwrap(), Some(STONE_BLOCK_ID));
        assert_eq!(s.block(-5, 100, -7).unwrap(), Some(DIRT_BLOCK_ID));
        assert_eq!(s.block(1, 2, 3).unwrap(), None, "unset blocks are not rows");
        let recs = s.replay().unwrap();
        assert_eq!(recs.iter().map(|r| r.seq).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(recs[1].tick, 7);
        assert_eq!(recs[1].actor, 99);
        assert_eq!(recs[1].new, DIRT_BLOCK_ID);
        cleanup(&dir);
    }

    #[test]
    fn inventory_and_xp_roundtrip_after_reopen() {
        let dir = tmp("player");
        {
            let s = WorldStore::open(&dir, 7).unwrap();
            let mut inv = PlayerInventory::new();
            inv.set_selected(2);
            inv.set_slot(9, Some((55, 5)));
            inv.set_slot(37, Some((1, 64)));
            inv.set_total_xp(8);
            inv.next_state();
            s.save_player("alice-tag", &inv).unwrap();
            assert_eq!(s.load_player("alice-tag").unwrap().unwrap().state_id(), inv.state_id());
        }
        let s = WorldStore::open(&dir, 7).unwrap();
        let back = s.load_player("alice-tag").unwrap().unwrap();
        assert_eq!(back.selected(), 2);
        assert_eq!(back.get(9), Some((55, 5)));
        assert_eq!(back.get(37), Some((1, 64)));
        assert_eq!(back.total_xp(), 8);
        assert!(s.load_player("nobody").unwrap().is_none());
        cleanup(&dir);
    }

    #[test]
    fn journal_replay_reconstructs_materialized_blocks() {
        let dir = tmp("replay");
        let s = WorldStore::open(&dir, 1).unwrap();
        s.set_block(3, 70, 4, DIRT_BLOCK_ID).unwrap();
        s.set_block(3, 70, 4, STONE_BLOCK_ID).unwrap();
        s.clear_block(3, 70, 4).unwrap();
        assert_eq!(s.block(3, 70, 4).unwrap(), None);
        let rows: HashMap<_, _> = s.blocks().unwrap().into_iter().collect();
        assert!(rows.is_empty(), "setting back to base removes the override row");
        let recs = s.replay().unwrap();
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[2].op, Op::BlockClear);
        assert_eq!(recs[2].new, u16::MAX);
        assert!(s.replay_blocks().unwrap().is_empty());
        // 新增一条后物化视图与回放结果一致。
        s.set_block(8, 8, 8, STONE_BLOCK_ID).unwrap();
        assert_eq!(s.replay_blocks().unwrap(), s.blocks().unwrap().into_iter().collect());
        cleanup(&dir);
    }

    #[test]
    fn timers_persist_for_growth_resume() {
        let dir = tmp("timers");
        let seed = 3;
        {
            let s = WorldStore::open(&dir, seed).unwrap();
            s.set_tick_offset(400).unwrap();
            s.set_timer(TimerKind::Crop, (5, 71, 5), 120, 3, 0).unwrap();
            s.set_timer(TimerKind::Sapling, (9, 71, 9), 200, 0, 0).unwrap();
            s.set_timer(TimerKind::Leaf, (7, 80, 7), 350, 0, 0).unwrap();
        }
        let s = WorldStore::open(&dir, seed).unwrap();
        assert_eq!(s.tick_offset().unwrap(), 400);
        let crop = s.timer(TimerKind::Crop, (5, 71, 5)).unwrap().unwrap();
        assert_eq!(crop.planted_tick, 120);
        assert_eq!(crop.stage, 3);
        assert_eq!(crop.kind, TimerKind::Crop);
        assert!(s.timer(TimerKind::Sapling, (5, 71, 5)).unwrap().is_none());
        assert!(s.timer(TimerKind::Leaf, (7, 80, 7)).unwrap().is_some());
        assert_eq!(s.replay().unwrap().iter().filter(|r| r.op == Op::TimerSet).count(), 3);
        cleanup(&dir);
    }

    /// P39：批量 timer 写入与删除的 journal/回放口径（tick_offset 同事务更新）。
    #[test]
    fn timer_batch_set_clear_and_tick_offset_roundtrip() {
        let dir = tmp("timer-batch");
        let seed = 9;
        {
            let s = WorldStore::open(&dir, seed).unwrap();
            let a = TimerRecord { kind: TimerKind::Crop, pos: (1, 71, 1), planted_tick: 10, stage: 2, spec: 0 };
            let b = TimerRecord { kind: TimerKind::Sapling, pos: (2, 71, 2), planted_tick: 10, stage: 0, spec: 0 };
            assert_eq!(s.set_timers(&[a, b], 1234).unwrap(), 2);
            assert_eq!(s.tick_offset().unwrap(), 1234);
            assert!(s.delete_timer(TimerKind::Crop, (1, 71, 1)).unwrap());
            assert!(!s.delete_timer(TimerKind::Crop, (1, 71, 1)).unwrap(), "delete is idempotent");
            assert_eq!(s.timers().unwrap().len(), 1);
        }
        let s = WorldStore::open(&dir, seed).unwrap();
        assert_eq!(s.tick_offset().unwrap(), 1234);
        let all = s.timers().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].kind, TimerKind::Sapling);
        assert_eq!(all[0].planted_tick, 10);
        let recs = s.replay().unwrap();
        assert_eq!(recs.len(), 3, "2 TimerSet + 1 TimerClear");
        assert_eq!(recs[0].op, Op::TimerSet);
        assert_eq!(recs[0].tick, 10, "TimerSet tick stays planted tick");
        assert_eq!(recs[2].op, Op::TimerClear);
        assert_eq!(recs[2].tick, 1234, "TimerClear tick uses current offset");
        assert_eq!(recs[2].new, u16::MAX);
        assert!(s.replay_blocks().unwrap().is_empty(), "timers do not alter blocks");
        cleanup(&dir);
    }

    #[test]
    fn migrates_legacy_p17_and_p18_files() {
        let dir = tmp("migrate");
        let seed = 12345;
        // 用生产 API 生成真实旧档（P17W v1 / P18I v2）。
        let legacy_world = crate::MutableWorld::new(seed);
        // y=0 可能落在洞穴里（基底已是空气），选一个与基底不同的值保证生成 override。
        let base = legacy_world.get(10, 0, 10);
        let changed = if base == AIR_BLOCK_ID { STONE_BLOCK_ID } else { AIR_BLOCK_ID };
        let _ = legacy_world.set(10, 0, 10, changed);
        let _ = legacy_world.set(11, 100, 11, STONE_BLOCK_ID);
        assert_eq!(legacy_world.save_to_dir(&dir).unwrap(), 2);

        let mut legacy_inv = PlayerInventory::new();
        legacy_inv.set_selected(1);
        legacy_inv.set_slot(36, Some((1, 63)));
        legacy_inv.set_total_xp(5);
        legacy_inv.save_to_dir(&dir, "bob", seed).unwrap();

        let s = WorldStore::open(&dir, seed).unwrap();
        assert_eq!(s.migrate_legacy_p17(&dir, seed).unwrap(), 2);
        assert!(s.migrate_legacy_p18(&dir, "bob", seed).unwrap());
        assert!(!s.migrate_legacy_p18(&dir, "nobody", seed).unwrap());

        assert_eq!(s.block(10, 0, 10).unwrap(), Some(changed));
        assert_eq!(s.block(11, 100, 11).unwrap(), Some(STONE_BLOCK_ID));
        let back = s.load_player("bob").unwrap().unwrap();
        assert_eq!(back.selected(), 1);
        assert_eq!(back.get(36), Some((1, 63)));
        assert_eq!(back.total_xp(), 5);
        assert_eq!(s.journal_len().unwrap(), 3, "2 block edits + 1 player save");

        drop(s);
        let again = WorldStore::open(&dir, seed).unwrap();
        assert_eq!(again.block(10, 0, 10).unwrap(), Some(changed));
        assert_eq!(again.load_player("bob").unwrap().unwrap().total_xp(), 5);
        assert!(crate::mutable::MutableWorld::save_path(&dir, seed).exists(), "legacy file kept");
        cleanup(&dir);
    }

    #[test]
    fn rejects_wrong_seed_and_schema_version() {
        let dir = tmp("guard");
        let src = {
            let s = WorldStore::open(&dir, 7).unwrap();
            s.set_block(1, 1, 1, STONE_BLOCK_ID).unwrap();
            s.path().to_path_buf()
        };
        let wrong_seed = dir.join("state-99.redb");
        std::fs::copy(&src, &wrong_seed).unwrap();
        assert!(matches!(WorldStore::open_at(wrong_seed, 99), Err(StoreError::Seed(7, 99))));

        let wrong_schema = dir.join("state-100.redb");
        std::fs::copy(&src, &wrong_schema).unwrap();
        {
            let db = Database::open(&wrong_schema).unwrap();
            let tx = db.begin_write().unwrap();
            let v = 999u32.to_be_bytes();
            meta_put(&tx, META_SCHEMA, v.as_slice()).unwrap();
            tx.commit().unwrap();
        }
        assert!(matches!(
            WorldStore::open_at(wrong_schema, 100),
            Err(StoreError::Version(999, 1))
        ));
        cleanup(&dir);
    }

    /// 普通 `#[test]` 计时打印：10k 条方块编辑一次事务写入 + 重放量级参考。
    #[test]
    fn bulk_write_and_replay_smoke() {
        let dir = tmp("bulk");
        let s = WorldStore::open(&dir, 77).unwrap();
        let t0 = std::time::Instant::now();
        s.with_write(|tx, seq| {
            for i in 0..10_000i32 {
                WorldStore::put_block(tx, seq, i, 64, -i, STONE_BLOCK_ID, Op::BlockSet, 0, 0)?;
            }
            Ok(())
        })
        .unwrap();
        let written = t0.elapsed();
        let t1 = std::time::Instant::now();
        let recs = s.replay().unwrap();
        let replayed = t1.elapsed();
        assert_eq!(recs.len(), 10_000);
        assert_eq!(s.blocks().unwrap().len(), 10_000);
        eprintln!("p36 bulk: 10000 edits committed in {written:?}, replay in {replayed:?}");
        cleanup(&dir);
    }

    /// P36.2 生产闭环：内存编辑 + 背包/XP → flush → 重开仍在，journal 带 old→new/actor/tick。
    #[test]
    fn production_flush_survives_reopen_with_actor_and_tick() {
        use std::sync::Arc;
        let dir = tmp("prod");
        let seed = 5150;
        let (x, y, z) = {
            let s = crate::WorldGen::new(seed).spawn_pos();
            (s.0, s.1 - 1, s.2)
        };
        {
            let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
            let world = crate::MutableWorld::with_store(seed, Arc::clone(&store)).unwrap();
            let mut inv = PlayerInventory::new();
            inv.set_slot(36, Some((55, 3)));
            inv.set_total_xp(11);
            inv.set_selected(2);
            // 挖：基底草 → 空气（override 记录 air）；放：空气 → 石头（y=100 基底为空气）。
            assert_ne!(world.get(x, y, z), AIR_BLOCK_ID);
            world.set_with_context(x, y, z, AIR_BLOCK_ID, 7, 31);
            world.set_with_context(10, 100, 10, STONE_BLOCK_ID, 8, 32);
            assert_eq!(world.dirty_len(), 2);
            store.save_player("alice", &inv).unwrap();
            assert_eq!(world.flush_to_store(99).unwrap(), 2, "one transaction for both edits");
            assert_eq!(world.dirty_len(), 0);
            assert_eq!(world.override_count(), 2);
            assert!(world.take_edits().is_empty());
        }
        let reopened = Arc::new(WorldStore::open(&dir, seed).unwrap());
        let world = crate::MutableWorld::with_store(seed, Arc::clone(&reopened)).unwrap();
        assert_eq!(world.get(x, y, z), AIR_BLOCK_ID);
        assert_eq!(world.get(10, 100, 10), STONE_BLOCK_ID);
        assert_eq!(world.dirty_len(), 0);
        let back = reopened.load_player("alice").unwrap().unwrap();
        assert_eq!(back.selected(), 2);
        assert_eq!(back.get(36), Some((55, 3)));
        assert_eq!(back.total_xp(), 11);
        let records: Vec<_> = reopened
            .replay()
            .unwrap()
            .into_iter()
            .filter(|r| r.op != Op::PlayerSave)
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].pos, (x, y, z));
        assert_eq!(records[0].old, u16::MAX, "no override before digging");
        assert_eq!(records[0].new, AIR_BLOCK_ID);
        assert_eq!((records[0].actor, records[0].tick), (7, 31));
        assert_eq!(records[1].pos, (10, 100, 10));
        assert_eq!(records[1].new, STONE_BLOCK_ID);
        assert_eq!((records[1].actor, records[1].tick), (8, 32));
        assert_eq!(reopened.replay_blocks().unwrap(), reopened.blocks().unwrap().into_iter().collect());
        cleanup(&dir);
    }

    /// 一次性迁移的编排：真实旧档 → redb → 旧档改名 .bak → 报告 → 重开不再重跑。
    #[test]
    fn open_with_migration_renames_legacy_files_and_is_idempotent() {
        use std::sync::Arc;
        let dir = tmp("mig-prod");
        let seed = 777;
        let legacy = crate::MutableWorld::new(seed);
        assert_eq!(legacy.set(10, 100, 10, STONE_BLOCK_ID), AIR_BLOCK_ID);
        assert_eq!(legacy.set(11, 100, 11, DIRT_BLOCK_ID), AIR_BLOCK_ID);
        assert_eq!(legacy.save_to_dir(&dir).unwrap(), 2);
        let mut inv = PlayerInventory::new();
        inv.set_total_xp(17);
        inv.set_slot(36, Some((1, 42)));
        inv.save_to_dir(&dir, "carol", seed).unwrap();

        let (store, report) = WorldStore::open_with_migration(&dir, seed).unwrap();
        assert!(report.ran);
        assert_eq!(report.seed, seed);
        assert_eq!(report.blocks, 2);
        assert_eq!(report.players, vec!["carol".to_string()]);
        assert_eq!(report.renamed, 2);
        assert_eq!(report.legacy_ignored, 0);
        assert!(!crate::mutable::MutableWorld::save_path(&dir, seed).exists());
        assert!(backup_path(&crate::mutable::MutableWorld::save_path(&dir, seed)).exists());
        let p18 = PlayerInventory::save_path(&dir, "carol", seed);
        assert!(!p18.exists());
        assert!(backup_path(&p18).exists());
        let manifest = std::fs::read_to_string(dir.join(format!("migration-{seed}.json"))).unwrap();
        assert!(manifest.contains("\"blocks\":2") && manifest.contains("carol"));
        assert_eq!(store.load_player("carol").unwrap().unwrap().total_xp(), 17);
        assert_eq!(store.journal_len().unwrap(), 3, "2 BlockSet + 1 PlayerSave");
        drop(store);

        // 第二次打开：库已存在，迁移不重跑，journal 条数不变。
        let (again, report2) = WorldStore::open_with_migration(&dir, seed).unwrap();
        assert!(!report2.ran);
        assert_eq!(again.journal_len().unwrap(), 3);
        let world = crate::MutableWorld::with_store(seed, Arc::new(again)).unwrap();
        assert_eq!(world.get(10, 100, 10), STONE_BLOCK_ID);
        assert_eq!(world.get(11, 100, 11), DIRT_BLOCK_ID);
        cleanup(&dir);
    }

    /// 库已存在时遗留的旧档只 warn 不读取；seed 不匹配的玩家文件不扫进来。
    #[test]
    fn existing_store_ignores_legacy_files_and_wrong_seed() {
        let dir = tmp("ignore-legacy");
        let seed = 88;
        {
            let s = WorldStore::open(&dir, seed).unwrap();
            s.set_block(1, 100, 1, STONE_BLOCK_ID).unwrap();
        }
        let legacy = crate::MutableWorld::new(seed);
        legacy.set(2, 100, 2, DIRT_BLOCK_ID);
        legacy.save_to_dir(&dir).unwrap();
        let mut inv = PlayerInventory::new();
        inv.set_total_xp(42);
        inv.save_to_dir(&dir, "dave", seed).unwrap();
        inv.save_to_dir(&dir, "other-seed", 999).unwrap();
        let (s, report) = WorldStore::open_with_migration(&dir, seed).unwrap();
        assert!(!report.ran);
        assert_eq!(report.legacy_ignored, 2, "only current-seed files count");
        assert!(s.block(2, 100, 2).unwrap().is_none());
        assert!(s.load_player("dave").unwrap().is_none());
        assert!(crate::mutable::MutableWorld::save_path(&dir, seed).exists(), "legacy file untouched");
        cleanup(&dir);
    }
}
