# 世界存储方案（P36）：redb 自有格式，不做 Anvil 兼容

> 分支 `p36-world-storage`，基于 `af9da5c`；owner=subagent-storage。
> 状态：**设计已定 + 最小原型已实现**。原型未接入生产路径，现有存档格式与协议行为不变。

## 0. 决策摘要（TL;DR）

1. **不做 Anvil**：不读写 `r.<rx>.<rz>.mca`，不做区块 NBT/Region 扇区/原版工具互操作。用户明确要求"直接研究新的世界存储格式"；原版互操作留作未来独立任务（见 §7）。
2. **磁盘真相源 = `redb`**（纯 Rust 嵌入式 KV，COW B-tree、MVCC、ACID、零拷贝读），每 seed 一个文件 `world/state-<seed>.redb`，延续 P17/P18 的 seed 隔离习惯。
3. **内存热层保持不变**：`MutableWorld { seed, base: WorldGen, RwLock<Inner{overrides,dirty}> }` 仍是 net 线程与 ECS 共享的权威视图；redb 是它的持久化后端。这一条是"比 Anvil 更强起点"的关键：**worldgen 确定性 → 地形不入盘，只存编辑差异**。
4. **物化表 + 逻辑日志同事务**：`blocks`/`players`/`timers` 是随用随读的物化视图，`journal` 是可重放的逻辑编辑日志；两者在**同一个 redb 写事务**中更新，天然一致，崩溃时旧超级块仍有效。
5. **成长计时纳入存档**（§5.18–20 的 TODO）：`timers` 表 + meta 的 `tick_offset`，让作物/树苗/树叶观察跨重启续算，而不是从 0 重计时。
6. **原型实测**（`#[test]` 计时，普通 test，无 criterion）：10,000 条编辑一次事务提交 **debug 1.34s / release 73.6ms**，全量重放 **debug 46.8ms / release 1.37ms**。

---

## 1. 背景与约束

### 1.1 现状盘点

| 模块 | 现状 | 问题 |
|---|---|---|
| `mc-world/src/mutable.rs` | `P17W` 稀疏档 `overrides-<seed>.dat`：magic+version+seed+count+14B 条目（x/y/z i32be + id u16be），全量排序写 tmp+rename | 每次刷盘**全量重写**整个文件；只有一个写者；无事务跨玩家数据；文件数随 seed/玩家增长 |
| `mc-world/src/inventory.rs` | `P18I` v1/v2 `player-<user>-<seed>.dat`，v2 尾部 +4B 总经验（§5.10/§5.26） | 与方块档不在同一事务，背包/方块可能落盘到不同时间点；键用用户名（离线 uuid 每次随机，见 §5.10） |
| `mc-server/src/persist.rs` | 60s 周期、断线、退出时刷脏 chunk | 触发时机合理，可直接复用为 redb 的 commit 时机 |
| `mc-network/src/drop_state.rs` | 全服唯一 50ms ticker；作物 `planted_tick`、树苗 `planted_tick`、树叶 `since` **只在内存**（§5.18–20 明确未持久化，重启重计时/清空观察） | 重启后生长进度丢失，是"新格式可以顺手补上"的能力缺口 |
| `mc-world/src/save.rs` | Anvil Region 头/扇区占位壳（compression=3 未压缩，payload 非 NBT） | 与自有稀疏档无关，生产路径不用；本任务不动它 |

### 1.2 必须满足的约束

- **协议零变化**：不改 `main.rs`/`persist.rs`/`drop_state.rs`/net 包行为；现有 358 个测试一字不改且全绿。
- **确定性基底**：地形、树、矿、biome 由 `WorldGen(seed)` 完全决定；磁盘只需保存"与基底不同的方块"，且设回基底值时删除条目（保持稀疏，P17 已有语义）。
- **崩溃安全**：断线/进程被杀后，方块、背包、XP 不能出现半写状态。
- **seed 隔离**：换 seed 不混档（旧实现靠文件名，新实现文件名 + meta 双保险）。
- **可迁移**：一次性读 P17W/P18I 写入新库；旧文件保留可回退。

### 1.3 规模估计

- 真人游玩数小时：override 通常 **几十到几千条**；极端建筑玩家 **10⁵ 量级**。
- 背包 46 槽（实际使用 37）、玩家数个位到几十。
- Journal：玩家编辑 + ticker 批量编辑 + 背包刷盘；按 10⁵ 条计划。当前量级下，任何成熟 KV 都够用，**复杂度/正确性比极限吞吐更重要**。

---

## 2. 候选方案评估

### 2.1 总览

| 方案 | 机制 | 适合本项目的点 | 不适合的点 | 结论 |
|---|---|---|---|---|
| **A. `redb` 嵌入式 KV** | 纯 Rust COW B-tree、MVCC、单写多读、ACID、零拷贝 `&[u8]` 读 | 单文件、事务跨方块/背包/计时；崩溃安全；读 O(log N)；并发模型与"net 写/ECS 读"天然契合；无需自造文件格式 | 单写者（但本项目本来单写）；大 value (>3GiB) 无关；journal 无限增长需自己裁剪 | **选定为主存储** |
| **B. Append-only edit journal / 事件溯源（独立方案）** | 追加日志 + 周期快照；读靠回放或内存投影 | 天然有 undo/审计/成长起源；写极快；崩溃安全（追加 + CRC） | 单独作真相源时，随机读方块要回放全历史或维护索引；快照/裁剪复杂；背包这类"覆盖写"语义要包装成事件；多人读并发要自己管 | **选为 redb 内的逻辑层**（journal 表），不作为独立引擎 |
| **C. LSM 分段 + compaction** | 多段有序文件 + 后台归并 | 高写入吞吐、批量写友好 | 多文件管理、compaction 写放大、读放大（bloom/多层查找）；我们的写入量极小（60s 一次批量 flush），完全用不上；迁移/备份要处理一堆段文件 | **不选**（收益远小于复杂度） |
| **D. 列式块存储 + zstd + `rkyv` mmap 零拷贝** | 按 chunk 列存 blockstate，zstd 压缩，`rkyv` mmap 零反序列化 | 读整 chunk 快；大块压缩率高；mmap 零拷贝诱人 | 我们的编辑是**单方块随机写**：每次写要读改写整个块，反而放大；2–12B 的 key/value 压缩无收益；`rkyv` schema 变更需严格版本管理且引入 unsafe/mmap 生命周期问题；地形不入盘后"列"根本没有大块可压 | **不选**（为不存在的大块数据付复杂度） |
| **E. per-chunk `RwLock`/`DashMap` 热层** | 按 chunk 分片锁，降低写锁竞争 | 当前单一 `RwLock<Inner>` 在多连接高频编辑时可能成为热点 | 它只是**内存并发结构**，不是持久化格式；断电即丢；不能单独解决存档、迁移、跨数据事务 | **作为未来热层优化**（阶段 5），不是存储方案 |

### 2.2 为什么 Anvil 也不合适（重申用户前提）

Anvil 的设计目标是"**保存完整 chunk 的 NBT**"，配套 32×32 Region 文件、8KB 头、扇区分配与回收、zlib 压缩、Heightmaps、block_entities。对本项目：

- 地形确定性可重新生成 → 存整 chunk 是**纯冗余**（441 块视野约 20MB 网络量级，磁盘同理）；
- 方块编辑是稀疏差异（14B/条）→ Anvil 每次编辑要读改写整个 chunk/Region，写放大 3 个数量级；
- 背包/XP 与方块编辑**无法在同一事务**落盘（Anvil 没有跨文件事务）；
- Region 扇区回收、NBT 编解码、压缩类型都是我们不需要的复杂度；
- 当前 `save.rs` 的 Anvil 占位壳正是这种复杂度的残留（且未生产使用）。

结论：**新格式从零设计，Anvil 不进入方案集**。

### 2.3 评分（5 分制，越高越好）

| 维度 | A redb | B journal | C LSM | D 列式+rkyv | E DashMap |
|---|---|---|---|---|---|
| 随机读方块 | 5 | 2 | 3 | 4 | 5（仅内存） |
| 批量写 / 低写放大 | 4 | 5 | 3 | 1 | 5（仅内存） |
| 崩溃安全 | 5 | 4 | 4 | 3 | 0 |
| 并发（单写多读） | 5 | 2 | 3 | 4 | 5 |
| 跨数据事务（方块+背包） | 5 | 2 | 3 | 2 | 0 |
| 实现/维护复杂度 | 4 | 3 | 2 | 1 | 4 |
| 与确定性基底协同（只存差异） | 5 | 5 | 5 | 1 | 5 |
| 迁移成本 | 4 | 3 | 2 | 1 | 0 |
| **合计** | **37** | **26** | **25** | **18** | **24** |

---

## 3. 选定架构

### 3.1 分层

```
net 线程（decide_dig / decide_place / plant / till，set_if CAS）
全服唯一 ticker（作物/树苗/树叶推进，DropState::physics_tick）
        │ set() / set_if()，RwLock 内微秒级；不碰磁盘
        ▼
┌─────────────────────────────────────────────────┐
│ MutableWorld（内存热层，保持现状）                │
│  seed + WorldGen 确定性基底                       │
│  RwLock<Inner { overrides: (x,y,z)->u16,          │
│               dirty: HashSet<ChunkPos> }>        │
└─────────────────────────────────────────────────┘
        │ flush：60s / 断线 / 退出（复用 persist.rs 触发点）
        │ 一个 redb WriteTransaction 内同时写：
        │   blocks + journal + players + timers + meta
        ▼
┌─────────────────────────────────────────────────┐
│ WorldStore（redb::Database，可 Arc 共享）         │
│  meta / blocks / journal / players / timers      │
└─────────────────────────────────────────────────┘
        ▼
world/state-<seed>.redb   （单文件，COW B-tree，双超级块 + 校验）
```

启动时反向：`WorldStore::open` → 读 `meta`/`blocks`/`players`/`timers` → 注入 `MutableWorld` 与 `DropState`（含 `tick_offset` 与 timer 认领）。**地形仍由 `WorldGen(seed)` 重算，不读盘**。

### 3.2 On-disk schema（精确到字节）

所有整数大端（BE），定长键/值；`&[u8]` 直读，无 serde 版本耦合。

**表清单**

| 表 | Key 类型 | Value 类型 | 用途 |
|---|---|---|---|
| `meta` | `&str` | `&[u8]` | schema/seed/日志头/tick 偏移/生成器标识 |
| `blocks` | `&[u8]`（12B） | `&[u8]`（2B） | 稀疏方块 override（当前物化状态） |
| `journal` | `u64`（seq） | `&[u8]`（33B） | 逻辑编辑日志（重放/审计/未来 undo） |
| `players` | `&str`（user tag） | `&[u8]`（变长） | 46 槽背包 + selected + state_id + 总 XP |
| `timers` | `&[u8]`（13B） | `&[u8]`（10B） | 作物/树苗/树叶成长计时 |

**meta 条目**

| key | value | 说明 |
|---|---|---|
| `schema` | `u32` = `1` | 不兼容变更递增；打开时校验，不符报 `Version` |
| `seed` | `i64` | 与文件名双重防串档（复制文件到别的 seed 名也会报错） |
| `journal_seq` | `u64` | 下一条日志序号；与日志同事务递增 |
| `tick_offset` | `u64` | 重启时恢复 `DropState::tick_count`，保证 `now - planted_tick` 自洽 |
| `generator` | ASCII | `mc-world::store redb v1 (P36 prototype)`，便于未来识别 |

**blocks**

```
key   = x:i32be(4) || y:i32be(4) || z:i32be(4)       共 12B
value = blockstate_id:u16be                          共  2B
```
- 只存**与确定性基底不同**的方块；`air(0)` 是合法 value（挖掉石头后仍需覆盖基底）。
- 设回基底值时 `remove(key)`，并由 journal 记一条 `BlockClear`。

**journal**（33B 定长）

```
op:u8 || tick:u64be || actor:u64be || x:i32be || y:i32be || z:i32be || old:u16be || new:u16be
```
| op | 值 | 语义 |
|---|---|---|
| `BlockSet` | 1 | `new` = 新 blockstate；`old` = 旧值（`u16::MAX` = 原来无 override） |
| `BlockClear` | 2 | 删除 override；`new = u16::MAX` |
| `PlayerSave` | 3 | 背包+XP 覆盖写；`pos=(0,0,0)`，实际数据在 `players` 表 |
| `TimerSet` | 4 | `tick` = planted tick；`old` = kind；`new` = stage |

- `actor` = conn_id（玩家编辑），系统/ticker/迁移为 0。
- 重放：`BlockSet` 插入 `new`，`BlockClear` 删除；其它 op 跳过。原型提供 `replay()` 与 `replay_blocks()`，后者用于自检 `blocks` 物化视图与日志一致。

**players**

```
version:u8 (=1) || selected:u8 || state_id:i32be || total_xp:i32be || slot_count:u8
|| 每个非空槽：slot:u8 || item_id:i32be || count:i32be   （9B/槽）
```
- key 仍是**稳定用户名**（§5.10：离线 uuid 每次随机，不能作键）；在线模式落地后换 uuid 是一个显式迁移项。
- v1/v2 旧档 XP 语义在迁移时由 `PlayerInventory::load_from_dir` 兼容（v1 无尾字段 → XP=0）。

**timers**（补 §5.18–20 的缺口）

```
key   = kind:u8 || x:i32be || y:i32be || z:i32be     共 13B
value = planted_tick:u64be || stage:u8 || spec:u8    共 10B
```
| kind | 值 | 对应现状 |
|---|---|---|
| Crop | 1 | `DropState.crops`（多阶段 age，§5.18/§5.24/§5.33） |
| Sapling | 2 | `DropState.saplings`（单次到期长树，§5.19） |
| Leaf | 3 | `DropState.pending_leaves`（失支撑 decay 观察，§5.20） |

- `spec` 存作物规格编号（未来按 `CropSpec` 表查），树苗/树叶填 0。

### 3.3 崩溃安全与 durability

- 每次 flush 是**一个 redb `WriteTransaction`**：COW B-tree 更新页，提交时写两个超级块 + 校验和。进程在提交中途被杀 → 旧超级块完整，回到上一次成功 flush 的状态；不会出现"方块写了、背包没写"。
- `Durability::Immediate`（redb 默认）：`commit()` 返回即持久化。玩家背包/XP、迁移、重要方块编辑都用这个级别。可选在**周期性方块 flush** 上用 `Eventual` 换取更低延迟，但会放宽丢失窗口到一次 flush 周期 —— 本方案**统一用 Immediate**，60s 一次 fsync 的代价可忽略。
- 不再需要 P17/P18 的 tmp+rename（那是单文件全量重写时代的做法）。
- 打开时校验 `schema` 与 `seed`；坏 meta 明确报错，不静默当空世界。

### 3.4 并发模型

- redb 原生：一个 `begin_write()` + 任意并发 `begin_read()`（MVCC），读不阻塞写、写不阻塞读。
- **阶段 1（本原型 / 生产替换推荐）**：保留 `Arc<MutableWorld>` 的 `RwLock` 作为内存热层；net 线程 `set/set_if` 只在内存，ECS `autosave_system` 在 60s/断线/退出时把 `dirty` 批量写进一个事务。提交成功后清 dirty；失败则保留 dirty 下次重试。这样**协议/线程模型零变化**。
- **阶段 2（可选写穿）**：每条编辑直接一个 redb 事务，`MutableWorld` 变读缓存。优点：崩溃丢失窗口 = 一条编辑；缺点：每个实体编辑一次 commit。原型批量数据可作为上限参考：release 10k 条同事务 73.6ms；逐条提交预计 0.1–1ms/条，玩家手动编辑完全可接受，但 **ticker 必须按 tick 批量**（同 tick 几十格作物阶段变更合成一个事务）。本阶段不在本任务范围。
- **`DashMap` 分片热层**（方案 E）：只有实测单把 `RwLock` 成为热点才做；按 `ChunkPos` 分片，redb 层不变。

### 3.5 快照 / 压缩 / 去重

- **去重 = 与确定性基底比较**：`set` 时若 `id == base_value` 则删 override（P17 已有），跨 chunk/玩家天然无重复。地形永不入盘。
- **压缩 = 不启用**：key 12B、value 2–33B，zstd 对这种小值没有收益且破坏 `&[u8]` 零拷贝读。若未来存大 blob（整 chunk、皮肤等），可加按 value 压缩的独立表，与当前 schema 正交。
- **快照 = 不需要周期性全量快照**：redb 的 COW/MVCC 本身就是一致快照；`blocks`/`players`/`timers` 是物化视图，`journal` 是增量历史。若 journal 增长过大，未来可加 `snapshot` 表保存"裁剪点之前的物化状态"并删除旧 journal（阶段 4）；原型不裁剪，`journal_len()` 可观测。
- **空闲页**：redb 自动复用；`WorldStore::compact()` 暴露 `Database::compact()`，低峰手动调用即可。

### 3.6 成长计时如何持久化（§5.18–20）

1. 种下/登记时写 `timers` 一条（kind + pos + planted_tick + stage + spec）。
2. 每次 flush 把当前 `tick_count` 写进 `meta.tick_offset`。
3. 启动时 `DropState.tick_count = tick_offset`（从断点续算，而非从 0 开始）；`adopt_existing_crops/saplings` 改为从 `timers` 读取**原 planted_tick 与 stage**，树叶观察同样从 `timers` 恢复。
4. 语义升级：从"重启重计时"变为"跨重启继续生长/继续 decay 观察"。**P39 已接入生产**：
   `DropState::with_store` 启动时读 `tick_offset` 与全部 `timers` 并恢复内存跟踪，
   `adopt_existing_crops/plants/saplings` 优先用原 planted/stage，树叶观察在构造时重建；
   legacy 模式（`store=None`）仍按旧口径重计时。
5. `spec` 字节仍是 10B 值里的 1B：低 4bit 为规格表索引，高 4bit 为来源表 tag
   （P33 作物 / P37 Age / Tree / Vertical / Side），bit7 是 Fruit 是否已落果标志；
   删除 timer 记 `TimerClear` journal（`op=5`），`tick` 用当前 `tick_offset`。

### 3.7 复杂度与体积评估

| 操作 | 复杂度 | 原型实测 / 说明 |
|---|---|---|
| 读一个方块 | O(log N) B-tree | 1 个 read txn + 1 次 lookup；未命中即回落 WorldGen |
| 写一条方块编辑 + 日志 | O(log N)，同事务 2 次 insert | 单条提交 |
| 批量 flush M 条 | O(M log N)，**一个事务** | release 10k 条 **73.6ms**（debug 1.34s）；重放 10k 条 release **1.37ms**（debug 46.8ms） |
| 玩家读/写 | O(log P) | 变长 value，46 槽上限 413B |
| timer 读/写 | O(log T) | 定长 10B |
| 全量重放 | O(J) | J = journal 条数；顺序按 u64 key，天然有序 |
| 体积 | ~12+2+33 = 47B 逻辑/编辑 + redb 页/元数据开销 | 1M override 约 35–60MB（旧 P17W 约 14MB）；journal 另计。以当前规模完全可接受 |

**目标**：release 10k 编辑 flush <100ms（实测 73.6ms）；重放 <5ms（实测 1.37ms）；启动加载 1M override <500ms；DB 体积 <100MB @1M override。

---

## 4. 与 `P17W` / `P18I` 的迁移方案

### 4.1 一次性迁移流程

```
启动：
  if world/state-<seed>.redb 不存在:
      WorldStore::open() 建库（schema=1, seed, seq=0, tick=0）
      if world/overrides-<seed>.dat 存在:
          解析 P17W（magic/version/seed/count + 14B 条目）
          一个事务内写 blocks + journal(BlockSet)；返回迁移条数
      for 每个已知 player tag（或扫描 player-*-<seed>.dat 文件名）:
          解析 P18I v1/v2（PlayerInventory::load_from_dir 兼容）
          写 players + journal(PlayerSave)
      写 migration 报告（条目数、玩家数、时间戳）到 world/migration-<seed>.json
      旧文件**改名保留**为 overrides-<seed>.dat.bak / player-...dat.bak（不删）
  else:
      WorldStore::open() 校验 schema/seed
      旧文件若仍存在 → warn"已迁移，忽略旧档"，不读取
```

### 4.2 关键细节

- **旧文件保留**：改名 `.bak` 而非删除，回退时把 `.bak` 改回原名并切回旧代码即可（见 §6）。
- **幂等/防重入**：以 `state-<seed>.redb` 是否存在为唯一判据；迁移失败（库未建成）下次仍会重试；迁移成功但报告写失败只影响诊断，不影响数据。
- **一致性**：方块与背包迁移分别是一个事务；若中途崩溃，下次重跑迁移是幂等覆盖写（`insert` 覆盖同 key），journal 可能多几条重复 `BlockSet`，但物化状态一致（重复插入同值无害）。生产阶段 2 可加 `meta.migrated_from` 标记避免重复记日志。
- **seed 隔离**：旧档文件名带 seed、新库文件名带 seed、库内 meta 也有 seed；三处一致才读。
- **XP v1 兼容**：`PlayerInventory::load_from_dir` 已处理 v1 无尾字段（XP=0），迁移直接复用，不改旧格式。
- **验证**：迁移后对比 `blocks().len()` 与 P17W 条目数（排除越界/基底自愈项）、`load_player(tag).total_xp()` 与旧档一致；原型单测用真实 `MutableWorld::save_to_dir` / `PlayerInventory::save_to_dir` 生成旧档再迁移（见 `store.rs::migrates_legacy_p17_and_p18_files`）。

### 4.3 原型已实现的部分

`WorldStore::migrate_legacy_p17` / `migrate_legacy_p18` 已能：解析真实旧档 → 写入 redb → 重开仍在 → journal 记录数正确。生产接线（扫描玩家列表、写报告、改名 `.bak`）留阶段 2。

---

## 5. 分阶段实施计划

| 阶段 | 内容 | 产出 | 风险 |
|---|---|---|---|
| **P36.1（本任务，已完成）** | `mc-world/src/store.rs` 原型：redb 打开/关闭、方块读写、背包+XP 读写、journal 重放、timers、旧档迁移、schema/seed 守卫；7 个单测；文档 | 独立模块，不接生产；358 旧测试不动 | 低（零侵入） |
| **P36.2** | 生产替换：`persist.rs::autosave_system` 改调 `WorldStore` flush（同 60s/断线/退出触发点）；`main.rs` 启动时建/开库并迁移；`drop_state.rs` 的背包读写改走 store | 现有玩法不变、存档换成 redb；旧档保留 | 中（需 e2e 全绿 + 真机冒烟）；用配置开关可一键回退旧档 |
| **P36.3（P39 已完成）** | 计时持久化：`timers` 表接入 `plant_crop`/`plant_sapling`/`schedule_leaf_decay_around`；启动时恢复 `tick_offset` 与原 stage/planted | 作物/树苗/decay 跨重启续算；新增 8 个单测/e2e，403 测试全绿 | 中（语义变化，需 e2e 验"重启后继续生长"） |
| **P36.4** | 运维工具：journal 查询/回放 CLI 或管理页 API；`snapshot` 表 + journal 裁剪；compact 调度 | 可审计、可 undo、可控体积 | 低 |
| **P36.5** | 可选优化：写穿事务（阶段 2 并发模型）、`DashMap` 分片热层、批量 tick 落盘 | 更低丢失窗口/更低锁竞争 | 中（需压测确认收益） |

---

## 6. 风险与回退

| 风险 | 缓解 |
|---|---|
| redb 文件损坏 | 事务原子 + 双超级块；启动时 schema/seed 校验；备份 `state-<seed>.redb`（可复制单文件）；旧 `.bak` 仍在 |
| 迁移丢数据 | 旧文件只改名不删除；迁移报告记录数量；迁移前后断言条数/XP；失败可重跑 |
| 新格式 bug 导致线上异常 | 阶段 2 加 `storage_backend = "legacy" \| "redb"` 配置开关（默认 legacy，灰度切 redb）；回退只改配置 + 恢复 `.bak` 文件名 |
| journal 无限增长 | `journal_len()` 监控；阶段 4 做 snapshot + 裁剪；当前量级（10⁵ 条）下体积可忽略 |
| ticker 与 flush 的竞态 | `timers`/`journal` 与方块在同一事务；ticker 只在内存更新，flush 时统一快照；锁顺序维持现状（items → orbs，连接侧不同时持两把锁） |
| schema 未来变更 | `schema` 版本号 + `generator` 标识；不兼容变更必须写迁移器，不静默读写 |
| 真机未验 | 本轮不接生产，协议路径逐字节不变；阶段 2 需脚本 e2e + 原版 26.3 冒烟（§4 验证流程） |

---

## 7. 明确不做 / 未来独立任务

- **不做 Anvil 导出/兼容**：不写 `r.<rx>.<rz>.mca`、不解码 NBT、不生成 Region 扇区、不做原版地图工具互操作。这是用户明确指示。
- 若未来需要原版工具互操作（导入地图、用 MCA 编辑器查看），作为**独立任务**评估：届时做单向导出器（redb → Anvil）即可，不反向引入 Anvil 作为真相源。
- 本轮不改：`tasks/`、`HANDOFF.md`、`MEMORY.md`（主会话统一更新）；不改 `save.rs` 的 Anvil 占位壳；不改协议/生产存档路径。

---

## 8. 验证记录（本分支实跑）

- `cargo check --workspace --tests`：0 error、0 warning。
- `cargo test -p mc-world store::`：7 passed / 0 failed（含关闭重开、背包+XP、重放一致、计时、旧档迁移、seed/schema 守卫、10k 批量计时）。
- 全量 `cargo test --workspace` / `bash scripts/check_budget.sh`：见提交前复验（应为 358 + 7 = 365 passed / 0 failed）。
- 计时（`#[test]` 打印，无 criterion）：
  - debug：10k 编辑提交 1.3356s，重放 46.8ms；
  - release：10k 编辑提交 73.6ms，重放 1.37ms。
