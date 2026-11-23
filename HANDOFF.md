# 交接文档：Rust 自研 MC 服务器（只兼容 Java 26.3 / protocol 777 / data 5023）

> 日期：2026-11-23；分支 `main`；本轮 P13（基线 `c3bd992`，代码提交 `02d31ee`）
> 目标：原版 26.3 客户端能完成 Handshake → Login → Configuration → Play，并渲染本服自己生成的世界。

## 1. 一句话现状

**原版 26.3 客户端已经能进 Play 并看到本服 `mc-world` 生成的平原世界**。P13 之后：

- `compression_threshold` 已从 `server.toml` / `NetConfig` 透传到 `handle_login`；默认 `-1` 保持无压缩原行为，`256` 时 Login/Configuration/Play 全链路走压缩帧，真机验证无 `DecoderException`、无 30s 超时。
- C2S 移动包 0x1E–0x21 不再只 log：经 crossbeam `PlayEvent` 通道投递 ECS，`MovementQueue → movement_system` 更新 per-player `Position`，KeepAlive 回显刷新 `PlayerKeepAlive.last_seen`。
- 玩家跨区块时 net 线程动态补发缺失的 radius=4 区块、`forget_level_chunk(0x26)` 卸载越界区块；已发送集合去重，ChunkBatchStart/Finished 成对且 `batch_size` 等于本批块数。
- 真机以压缩模式从出生点走到约 250 格之外（跨 17+ 次区块中心变化），全程 `y=65.0, on_ground=true`，地形持续存在、无重复/错位区块、无超时。

上一阶段（P11/P12）的 Configuration 卡点也已解决：根因不是 dimension NBT，而是 `minecraft:block` 标签缺少 `minecraft:infiniburn_overworld`。现在 Configuration 直接回放原版 26.3 抓包的全量注册表与 Update Tags，保证条目 ID 与标签自洽。

## 2. 怎么跑

### 服务端

```powershell
cd H:\code\rust\paper
cargo build -p mc-server
.\target\debug\mc-server.exe        # 监听 0.0.0.0:25565，离线模式
```

`server.toml` 可改 bind / motd / max_players / view_distance / seed / **compression_threshold**：

```toml
compression_threshold = -1    # 默认：禁用压缩（原行为）
# compression_threshold = 256 # 启用：Login Success 前发 SetCompression(256)
```

压缩阈值语义：`-1` 禁用；`>=0` 启用，原文长度（packet_id VarInt + payload）`>= threshold` 时 zlib，否则压缩帧内 `data_len=0` 明文直通。旧 `server.toml` 没有该键时按 `-1` 补齐。

### 原版客户端（本机直连）

必须使用 Java 25（runtime `java-runtime-epsilon`）。**必须加 `-XX:StackShadowPages=32`**，否则 26.3 在资源加载/进世界时会偶发 `jvm.dll 0xc0000005` 原生崩溃（官方启动器同样加这个参数，与本服无关）。

本机可用的启动参数骨架：

```powershell
$java = 'C:\Users\qwe\AppData\Local\Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime\java-runtime-epsilon\windows-x64\java-runtime-epsilon\bin\java.exe'
$cp   = Get-Content 'C:\Users\qwe\AppData\Local\Temp\mc263_cp.txt' -Raw   # 26.3.json libraries + 26.3.jar
$natives = 'C:\Users\qwe\AppData\Local\Temp\mc263natives'

& $java -Xmx2G -Xms512M -XX:StackShadowPages=32 `
  --enable-native-access=ALL-UNNAMED --add-exports java.base/jdk.internal.misc=ALL-UNNAMED `
  "-Djava.library.path=$natives\java" "-Djna.tmpdir=$natives\jna" `
  "-Dorg.lwjgl.system.SharedLibraryExtractPath=$natives\lwjgl" "-Dio.netty.native.workdir=$natives\netty" `
  -cp $cp net.minecraft.client.main.Main `
  --username ProbePlayer --version 26.3 --gameDir <游戏目录> `
  --assetsDir 'C:\Users\qwe\AppData\Roaming\.minecraft\assets' --assetIndex 34 `
  --uuid 3c65702a413a46d0a9053149979a9273 --accessToken 0 --userType legacy --versionType release `
  --quickPlayMultiplayer 127.0.0.1:25565 --width 1280 --height 720
```

客户端日志成功标志：`Connecting to 127.0.0.1, 25565` 后**没有** `Timed out while waiting for the client to load chunks`、没有 `DecoderException`，直接进入世界。服务端日志成功标志：`enter_play ... threshold=...` → `play join sent ... view_sent=81` → `play keepalive echo` → `play move ... forwarded_to_ecs=true` → 跨区块时 `play view move ... add=9 remove=9 sent_total=81`。

## 3. 当前实现（P13 之后）

### 压缩路径（P13）

- `mc-server/config.rs`：`ServerConfig.compression_threshold`，默认 `DEFAULT_COMPRESSION_THRESHOLD = -1`；旧 toml 缺键按默认补齐（单测覆盖）。
- `mc-network/config.rs`：`NetConfig.compression_threshold` + `.with_compression_threshold()`；`NetConfig::new` 仍默认 `-1`。
- `handshake_route` 调 `handle_login_configured(..., config.compression_threshold, config.play_events)`；`handle_login` / `handle_login_with_threshold` 旧入口保留，委托到同一实现。
- `framing.rs`：压缩帧边界单测（`raw.len() >= threshold` 才压缩；等于阈值必须压缩；`threshold=0` 连空 payload 也压缩；截断/长度不符报错）。
- 真机 `threshold=256`：SetCompression 明文帧 → Login Success 起压缩帧 → Configuration 32 注册表/Update Tags → Play 81 区块均压缩，客户端正常进世界。

### Play 登录序列（`mc-network/src/login_route.rs`）

按顺序发送：

1. `LoginPlay`（0x32，777 布局）
2. `SetCenterChunk`（0x60）中心 = `SPAWN_POS` 所在区块 (23,23)
3. `GameEvent`（0x27 / type 13 `LEVEL_CHUNKS_LOAD_START`）
4. `ChunkBatchStart`（0x0C）
5. **81 个 `ChunkData`（0x2E）**，同一批（`ChunkView.plan` 初始返回中心优先的 81 块）
6. `ChunkBatchFinished`（0x0B，batch_size=81）
7. `PlayerPositionSync`（0x49，teleport id 在前）
8. `KeepAlive`（0x2D）

之后 `play_loop` 每 15 秒主动发 KeepAlive，处理 C2S KeepAlive 回显、移动包 0x1E–0x21。

**为什么必须发 81 块（至少 3×3）**：客户端 `SectionUpdateTracker.hasAllNeighbors` 要求玩家所在 section 的 4 个正向 + 4 个斜向邻居区块都存在，否则玩家 section 永远不会编译，30 秒后客户端打印超时并强制放行。只发 1 块时即使格式完全正确也会超时。

### C2S 移动包 → ECS（P13）

- `mc-network/src/play_events.rs`：`PlayEvent::Movement { addr, movement }` / `PlayEvent::KeepAlive { addr }`，经 `NetConfig.play_events: Option<Sender<PlayEvent>>` 从 net 线程投递。
- `play_loop` 解码 0x1E–0x21 后：先处理跨区块（见下），再 `try_send` 事件；日志带 `forwarded_to_ecs=true`。
- `mc-server/systems.rs::handle_new_connections`：每登录成功 `spawn((PlayerAddr(addr), Position::default(), PlayerKeepAlive::default()))`，在线数仍只在此 `fetch_add` 一次。
- `mc-server/movement.rs::movement_ingest_system`：排空 `PlayEventReceiver`，按 addr 找实体 → `MovementQueue.push((entity, MovementUpdate))`；KeepAlive 回显与移动包都刷新 `last_seen`（26.3 客户端站立时也约 1s 发一次移动包，避免 15s 心跳周期与 15s 超时的临界竞态）。
- `movement_system` 排空队列并更新 `Position`；`keepalive_timeout_system` 仍是 online 唯一的递减点（饱和减，超时只 despawn 一次）。
- 测试：`mc-network/tests/play_stream.rs` 证明包真的进入 `PlayEvent` 通道；`movement.rs` 单测证明事件进入 MovementQueue、Position 更新、last_seen 刷新且 online 不变；`systems.rs` 单测证明一次 NewConnection 只 spawn 一个实体/只加一次 online。

### 动态区块加载/卸载（P13）

- `mc-network/src/chunk_view.rs`：纯逻辑 `ChunkView` 维护 `center + sent: HashSet<ChunkPos>`。
  - 初始 `plan(center)` 返回 radius=4 的 81 块（中心优先）；
  - 跨区块 `plan(new_center)` 返回 `add = 新视距 - 已发送`（`view_chunks` 中心优先顺序）和 `remove = 已发送 - 新视距`（按 x,z 排序）；内部集合同步更新，避免重复发送。
  - 十字跨区块 add/remove 各 9；斜跨 17（9+8），单测覆盖。
- `play_loop` 从移动包位置算区块（`floor + div_euclid(16)`，负坐标正确），中心变化时：
  1. `SetCenterChunk`（0x60，VarInt x/z）
  2. 逐个 `UnloadChunk`（0x26）越界区块
  3. `ChunkBatchStart` → N×`ChunkData` → `ChunkBatchFinished(batch_size=N)`；N=add.len，空批次不发。
- 0x26 线格式本轮修正为 **packed i64 ChunkPos**：`x & 0xffffffff | ((z & 0xffffffff) << 32)` 的 big-endian Long（26.3 `ClientboundForgetLevelChunkPacket` 用 `FriendlyByteBuf.writeChunkPos`；已用 26.3.jar `javap` 反编译确认）。旧实现写 `Chunk X Int + Chunk Z Int`，真机会把 x/z 交换、误卸玩家脚下的区块（本轮真机复现并修复）。
- 真机：压缩模式从 (369,381) 走到约 (145,686)，服务端记录 17+ 次 `view move`（各 add=9/remove=9），玩家始终 `y=65.0, on_ground=true`；20s+ 空闲后 KeepAlive echo 持续、无 ECS timeout。

### 区块数据（`mc-world` + `play_chunk.rs`）

- 真实 blockstate ID（对齐 `reports/blocks.json`）：`air=0`、`stone=1`、`grass_block=9`（snowy=false）、`dirt=10`。
- 平原分层：y≤60 石头，61–63 泥土，64 草方块，其余空气；24 个 section，对应 y=-64..319。
- 均匀 section 用单值 palette；混合 section 用**间接 palette**（BPE=4，palette `[stone,dirt]` / `[grass,air]`，4096 项 Data Array，1.16+ 不跨 long）。
- `MOTION_BLOCKING` 高度图：草方块 y=64 → 值 65-(-64)=129，BPE=9、每 long 7 项、37 longs。
- 天空光：26.3 的 BitSet 线格式是 **`VarInt 字节数 + 小端 bit 字节`**（`FriendlyByteBuf.writeByteArray(BitSet.toByteArray())`），不是旧版 `long 数量 + long 数组`。给 section 8..23 发 2048B 全 15 级 nibble，地下按 0。
- 旧的“原版 (23,23) 区块抓包”调试脚手架已删除。

### 其它已修

- `LoginPlay` 和 `PlayerPositionSync` 按 777 真布局修正，并用原版抓包 fixture 做逐字节往返测试。
- KeepAlive 周期发送，真机连接可长期保持。
- GameEvent + Chunk Batch 是 26.3 离开“加载地形中…”的必要条件。

## 4. 验证结果（2026-11-23 真机 / 本地）

### 压缩真机（threshold=256）

- 服务端日志：`enter_play ... threshold=256`、`play join sent ... view_sent=81`。
- 客户端无 `DecoderException`、无 `Timed out while waiting for the client to load chunks`、无连接超时；进入世界并渲染地形。
- KeepAlive 回显 id=1..12 连续；移动/跨区块日志正常。

### 动态区块真机

- 出生后按住 W 约 8 秒（期间合成按键释放异常，实际持续移动更久），玩家从 (369.5,65,381.5) 走到约 (145.5,65,686.3)，跨 chunk 中心 17+ 次。
- 服务端每次 `view move` 均 `add=9 remove=9 sent_total=81`；无 `DecoderException`、无 30s 超时。
- 玩家全程 `y=65.0, on_ground=true`，未再出现 P13 首次真机时的“走远后掉穿世界”（该问题由 0x26 x/z 交换导致，已修复）。
- 菜单打开后空闲 20s+：KeepAlive echo 持续（id=10..12），无 ECS keepalive timeout。

### 本地检查

- `cargo test --workspace`：21 个 test target 全过（112 个测试）。
- `cargo check --workspace --tests`：0 error。
- `scripts/check_budget.sh`（Windows 用 `C:\Program Files\Git\bin\bash.exe`）：通过（src ≤2000 行软约束）。

## 5. 已知限制 / 下一步

1. **socket 仍未进 ECS**：Play 的 Join/Chunk/动态区块实际仍由 net 线程发送；`PlayScaffold.pending_chunks` 只做编码自检/日志。P13 已把移动/KeepAlive 事件送进 ECS，但 socket 所有权/写通道的拆分会影响当前已验证链路，故保留 `TODO(P13/socket→ECS)`（`mc-server/src/play.rs`、`systems.rs`）。
2. **在线数语义**：ECS 现在真的有 Player 实体，15s 超时会 despawn 并减 online（单点、饱和减）。net 线程断开时不会立刻通知 ECS，因此正常退出后 online 最多滞后 15s 才减；同一源 addr 快速重连且旧实体未超时可能短暂出现两个实体。多玩家/重连要后续按 UUID 去重。
3. **动态区块是固定 radius=4**：与初始 81 块一致，未读 `view_distance`；每次跨区块立即整批发送 9/17 块，未做每 tick 限流；net 线程 own 每连接状态，未做多玩家共享区块缓存。
4. **区块数据仍按需临时生成**：没有 Anvil 存档、没有按 seed 的高度/群系；所有区块同一水平面。动态加载/卸载只影响网络发送集合，不落盘。
5. **地形固定平原**：`chunk_for` 与 x/z 无关，尚未按 seed 生成高度/群系；区块发送也未按玩家 per-player 拆分（当前每个连接独立维护自己的 `ChunkView`，但区块内容共享同一生成函数）。
6. **压缩只有阈值可配**：zlib level 固定 6；没有压缩统计/限流；`threshold=0` 会让所有包压缩（仅单测覆盖，未做真机）。
7. `mc-registry` 的 216 项占位表仍是旧数据；区块直接用 `mc-world` 常量，未走该表。

## 6. 关键文件

```
crates/mc-protocol/src/packets/
  config.rs                  Configuration 包编解码/断言
  vanilla_config_777.rs      32 个原版注册表包体 + Update Tags 常量
  data/vanilla_config_777/   抓包二进制（33 个文件，约 216KB）
  block_tags_777.rs          300 个 block tag / 14825 项的回归数据
  play_chunk.rs              SetCenter/ChunkData/GameEvent/Batch/Unload（0x26 packed i64）
  play_login.rs              LoginPlay 777 布局 + 抓包往返测试
  play_sync.rs               PlayerPositionSync/KeepAlive/C2S 移动包
  data/*_777.bin             LoginPlay / PositionSync / UpdateTags 抓包
crates/mc-network/src/
  login_route.rs             Configuration/Play 发送序列、81 块、KeepAlive、动态区块、移动转发
  chunk_view.rs              per-connection 视距集合/去重/跨区块 add/remove 规划
  play_events.rs             net 线程 → ECS 的移动/KeepAlive 事件
  config.rs                  NetConfig（压缩阈值 + Play 事件通道）
  framing.rs                 VarInt 帧 + zlib 压缩帧 + 阈值边界
crates/mc-network/tests/play_stream.rs   移动事件/压缩配置/跨区块批次集成测试
crates/mc-world/src/lib.rs             SPAWN_POS、chunk_for、palette/heightmap/light
crates/mc-server/src/
  config.rs                  ServerConfig.compression_threshold
  systems.rs                 NewConnection → ECS 玩家实体
  movement.rs                PlayEvent 排空 + MovementQueue/Position/KeepAlive
  play.rs                    ECS 视距编码自检/日志（socket 未接入）
docs/packet-map-777.md                 777 包 ID 对照表（权威）
docs/registries-777.md                 注册表清单
```

## 7. 测试命令

```bash
cargo check --workspace --tests
cargo test --workspace
bash scripts/check_budget.sh     # Windows 可用 C:\Program Files\Git\bin\bash.exe
```

## 8. Git 状态 / 提交注意

- 当前分支 `main`；P13 基线 `c3bd992`，代码提交 `02d31ee`；文档/MEMORY 为后续 `docs:` 提交。
- 不要提交：`scripts/check_budget.sh` 的纯行尾/文件 mode 变化、`Cargo.lock`（如无依赖变更）、`logs/`、`target/`、临时日志。
- `HANDOFF.md`、`MEMORY.md` 按仓库习惯更新。
- 旧的 `.gitignore` / `desk_log/conn.log` 删除是历史工作区噪声，已从 HEAD 恢复，不要误提交。
