# 交接文档：Rust 自研 MC 服务器（只兼容 Java 26.3 / protocol 777 / data 5023）

> 日期：2026-11-23；分支 `main`；本轮 P15（基线 `12bdaa4`；P13 `02d31ee`/`0aaf8ed`；P14 `1e47e69`）
> 目标：原版 26.3 客户端能完成 Handshake → Login → Configuration → Play，并渲染本服自己生成的世界。

## 1. 一句话现状

**原版 26.3 客户端已经能进 Play，并渲染由 `server.toml` seed 驱动的丘陵世界；P15 起 `view_distance` 真正决定下发视距（2..=10），连接断开在下一 tick 清理，快速重连按 `conn_id` 保留唯一实体**。P15 之后：

- `ServerConfig.view_distance` 经唯一函数 `mc_network::clamp_view_radius` 限制到 **2..=10**，再透传到 `LoginPlay.view_distance` / `simulation_distance`、初始区块批、跨区块补发和 ECS 自检；不再是“配置 10、实际固定 radius=4”。
- 初始 Join 一批发送 `(2r+1)^2` 块（r=10 → 441，r=4 → 81，r=2 → 25）；跨区块仍一批补/卸边界列（r=10 单轴 `add=21 remove=21`）。
- 新增共享 `ChunkCache`（已编码 `ChunkData 0x2E` 包体 `Vec<u8>`，FIFO 1024 块）：同一 seed 的连接/重连/跨区块优先命中缓存，无缓存时回退 `WorldGen::generate`。
- accept 时分配单调 `conn_id`，`NewConnection` 与 `PlayEvent::{Movement,KeepAlive,Disconnect}` 全带该 id；ECS 玩家实体有 `PlayerConnId`。连接结束在所有退出路径恰好发一次 `Disconnect`，与 15s `keepalive_timeout_system` 汇入唯一删除点 `player_removal_system`，online 只减一次；同 addr 快速重连时旧连接的迟到 Disconnect 不会误删新实体。

P14 的能力保持：`ServerConfig.seed` 真正生效，`mc-world::WorldGen` 用世界坐标 3 层平滑 value noise 生成地表 y=58..94；每 chunk 真实 `MOTION_BLOCKING` 高度图、逐列草/3 土/石、最低地表 section 向上天空光；默认 seed 出生 y=73，seed 987654321 y=83。

P13 的能力保持：`compression_threshold` 配置化（默认 -1，256 已真机）、C2S 移动经 `PlayEvent` 进 ECS、跨区块动态补/卸区块、0x26 packed ChunkPos 修正。

上一阶段（P11/P12）的 Configuration 卡点也已解决：根因不是 dimension NBT，而是 `minecraft:block` 标签缺少 `minecraft:infiniburn_overworld`。现在 Configuration 直接回放原版 26.3 抓包的全量注册表与 Update Tags，保证条目 ID 与标签自洽。

## 2. 怎么跑

### 服务端

```powershell
cd H:\code\rust\paper
cargo build -p mc-server
.\target\debug\mc-server.exe        # 监听 0.0.0.0:25565，离线模式
```

`server.toml` 可改 bind / motd / max_players / view_distance / **seed** / **compression_threshold**：

```toml
seed = 123456789              # 默认；决定 WorldGen 地形与出生点 y（x/z 固定 369/381）
# seed = 987654321            # 改 seed 后地形轮廓与出生 y 不同
compression_threshold = -1    # 默认：禁用压缩（原行为）
# compression_threshold = 256 # 启用：Login Success 前发 SetCompression(256)
```

压缩阈值语义：`-1` 禁用；`>=0` 启用，原文长度（packet_id VarInt + payload）`>= threshold` 时 zlib，否则压缩帧内 `data_len=0` 明文直通。旧 `server.toml` 没有该键时按 `-1` 补齐。

视距语义（P15）：`view_distance` 是切比雪夫区块半径，**有效范围 2..=10**；越界由 `clamp_view_radius` clamp 并 `warn!`（1→2、11→10）。它同时决定 `LoginPlay.view_distance`/`simulation_distance`、初始批 `(2r+1)^2` 块、跨区块补发边界和 ECS `pending_chunks` 自检数量。默认 10（441 块）；旧文档里的“固定 radius=4/81 块”只作为旧入口默认值保留。

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

客户端日志成功标志：`Connecting to 127.0.0.1, 25565` 后**没有** `Timed out while waiting for the client to load chunks`、没有 `DecoderException`，直接进入世界。服务端日志成功标志：`enter_play ... view_radius=10 view_chunks=441` → `play join chunks sent ... count=441 ... elapsed_ms=...` → `play join sent ... view_sent=441` → `play keepalive echo` → `play move ... forwarded_to_ecs=true` → 跨区块时 `play view move ... radius=10 add=21 remove=21 sent_total=441`；断开时同一 tick 内 `disconnect ... conn_id=N` → `despawn player conn_id=N` → `player removal complete removed=1 online=0`。

## 3. 当前实现（P15 之后）

### 视距（P15）

- 唯一有效视距计算：`mc_network::chunk_view::clamp_view_radius(i32) -> i32`，合法范围 **2..=10**，越界 clamp 并 `warn!`；`view_chunk_count(r)` 返回 `(2r+1)^2`。`ServerConfig::effective_view_distance()`、`NetConfig::effective_view_radius()` 都调用它。
- `ChunkView::new(center, radius)` + `radius()`；`plan` 用实例 radius 调 `mc_world::view_chunks`，不再用固定常量。`PLAY_VIEW_RADIUS=4` / `PLAY_VIEW_CHUNK_COUNT=81` 只保留为旧入口默认值。
- 透传：`ServerConfig.view_distance` → `NetConfig.with_view_distance` → `handshake_route` 构造 `PlayOptions { threshold, seed, view_radius, conn_id, events, chunk_cache }` → `handle_login_with_options` → `enter_play_with_options` → `ChunkView`。旧入口 `handle_login` / `handle_login_configured` / `enter_play` / `enter_play_with_events` 保留，默认 radius=4、无缓存，委托到新实现。
- `LoginPlay.view_distance` 与 `simulation_distance` 都等于实际有效半径，`seed` 同步为 `WorldGen.seed`；初始批 `debug_assert` 按 radius 计算，不再写死 81。
- ECS `mc-server/src/play.rs` 的编码自检/日志使用 `cfg.effective_view_distance()`，`pending_chunks` 入队 `(2r+1)^2` 块；`CHUNKS_PER_TICK=1` 仍只影响自检，真发送在 net 线程。

### 区块缓存（P15）

- `mc-network/src/chunk_cache.rs`：`ChunkCache::new(seed, capacity)`，共享 `Arc<ChunkCache>`，内部 `Mutex<HashMap<ChunkPos, Arc<Vec<u8>>>>` + FIFO `VecDeque<ChunkPos>`。
- 缓存的是**已编码 ChunkData 0x2E 包体**（约 30–40KB/块），不是 `GeneratedChunk`（约 196KB/块）；默认容量 `DEFAULT_CHUNK_CACHE_CAPACITY=1024`，覆盖 radius=10 的 441 块并留移动余量。
- `body(pos)` 命中返回同一 `Arc`（不重新生成/编码），未命中在锁外生成后插入；容量满按 FIFO 淘汰；命中/未命中计数在 `ChunkCacheStats`，不逐块打 info。
- `NetConfig.chunk_cache: Option<Arc<ChunkCache>>` + `.with_chunk_cache()`；`main.rs` 用 `cfg.seed` 建缓存注入。`enter_play` 在 cache seed 与 server seed 不一致时 `warn!` 并回退直接生成，绝不串地形。Join、跨区块补发都优先走 cache。
- 单测：命中字节与直接编码逐字节一致且复用 `Arc`、FIFO 淘汰最旧、不同区块/不同 seed 不串数据、容量 0 仍能生成。

### 连接生命周期（P15）

- `serve` accept 时用 `AtomicU64` 分配单调 `conn_id`；`NewConnection { addr, conn_id }` 同步到 ECS。`PlayerConnId(u64)` 是跨事件实体键，`PlayerAddr` 只用于日志。
- `PlayEvent::{Movement, KeepAlive, Disconnect}` 都带 `{ addr, conn_id }`；`enter_play_with_options` 在**所有退出路径**（正常 EOF、读错误、写失败、KeepAlive 超时、Join 序列失败）统一发一次 `Disconnect`，join 失败也不会在 ECS 留下永久实体。
- ECS `movement_ingest_system` 收到 Disconnect 只登记 `PendingPlayerRemoval`（conn_id 去重）；`keepalive_timeout_system` 也只登记。唯一删除点 `player_removal_system` 按 conn_id 找实体 → despawn → online 饱和减一次。未找到的 conn_id 保留约 1s 重试，覆盖 NewConnection/Disconnect 跨 channel 观察顺序的极窄竞态；conn_id 永不复用，旧连接的迟到 Disconnect 不会误删同 addr 的新实体。
- `handle_new_connections` 对重复 conn_id 幂等（已有实体 + 同 tick seen 集合），online 只在 spawn 时 `fetch_add` 一次。

### 地形生成（P14）

- `crates/mc-world/src/worldgen.rs`：`WorldGen { seed }`、`GeneratedChunk { pos, sections, heights }`。
- 算法：世界坐标 (x,z) 的 3 层平滑 value noise（scale 128/48/24，幅度 20/10/6），基高 58，地表 y 严格落在 58..94；相邻列最多差 3（单测保证）。
- `height_at(x,z)` 直接用世界坐标，chunk 边界连续；负坐标/远坐标用 `div_euclid`/wrapping hash，不溢出。
- 每列：`y == h` 为 `grass_block(9)`，`h-1..h-3` 为 `dirt(10)`，`y <= h-4` 为 `stone(1)` 到 `MIN_Y(-64)`，其余 `air(0)`；24 个 section，fluid_count 恒为 0。
- `GeneratedChunk::heightmaps_bytes`：按该 chunk 的实际 256 列 `h+1-MIN_Y` 打包（BPE=9、37 longs），不再有统一高度。
- `GeneratedChunk::light_bytes`：取本 chunk 最低地表 `min_h`，从 `(min_h-MIN_Y)/16` section 向上发 15 级天空光；地下无数组（0）。
- `WorldGen::spawn_pos`：x=369、z=381 固定，y = `height_at(369,381)+1`；默认 seed y=73，seed 987654321 y=83。
- seed 透传：`ServerConfig.seed` → `NetConfig.with_seed()` → `PlayOptions.seed` → `handle_login_with_options` → `enter_play_with_options` → `WorldGen`；旧入口 `handle_login_configured(... seed ...)` / `enter_play_with_events` 仍委托同一实现。ECS 侧 `handle_new_connections`/`Position::at_spawn(cfg.seed)`、`play::login_join_system` 也读同一个 `ServerConfig.seed` 资源。
- 旧 `Chunk/chunk_for/chunk_network_bytes/chunk_*_bytes` 保留为 save/load 占位与旧测试，Play 网络路径不再使用。

### 压缩路径（P13）

- `mc-server/config.rs`：`ServerConfig.compression_threshold`，默认 `DEFAULT_COMPRESSION_THRESHOLD = -1`；旧 toml 缺键按默认补齐（单测覆盖）。
- `mc-network/config.rs`：`NetConfig.compression_threshold` + `.with_compression_threshold()`；`NetConfig::new` 仍默认 `-1`。
- `handshake_route` 用 `PlayOptions` 透传 `config.compression_threshold` / `config.seed` / `config.effective_view_radius()` / `config.play_events` / `config.chunk_cache`；`handle_login` / `handle_login_with_threshold` / `handle_login_configured` 旧入口保留，委托到同一实现。
- `framing.rs`：压缩帧边界单测（`raw.len() >= threshold` 才压缩；等于阈值必须压缩；`threshold=0` 连空 payload 也压缩；截断/长度不符报错）。
- 真机 `threshold=256`：SetCompression 明文帧 → Login Success 起压缩帧 → Configuration 32 注册表/Update Tags → Play 全部视距区块均压缩，客户端正常进世界（P15 radius=10/441 块再次真机，见 §4）。

### Play 登录序列（`mc-network/src/login_route.rs`）

按顺序发送：

1. `LoginPlay`（0x32，777 布局；`view_distance=simulation_distance=有效半径`，`seed=WorldGen.seed`）
2. `SetCenterChunk`（0x60）中心 = `WorldGen::spawn_chunk()` = chunk (23,23)
3. `GameEvent`（0x27 / type 13 `LEVEL_CHUNKS_LOAD_START`）
4. `ChunkBatchStart`（0x0C）
5. **`(2r+1)^2` 个 `ChunkData`（0x2E）**，同一批（`ChunkView.plan` 初始返回中心优先的全部区块，内容由 WorldGen 生成或 ChunkCache 命中；r=10 → 441，r=4 → 81，r=2 → 25）
6. `ChunkBatchFinished`（0x0B，batch_size = 该批块数）
7. `PlayerPositionSync`（0x49，teleport id 在前，y = `WorldGen::spawn_pos().1`；必须在全部区块之后）
8. `KeepAlive`（0x2D）

之后 `play_loop` 每 15 秒主动发 KeepAlive，处理 C2S KeepAlive 回显、移动包 0x1E–0x21。

**初始批策略（P15 结论）**：仍允许同一批发送全部 `(2r+1)^2` 块，不拆 64 块/50ms。真机 debug 实测：r=10 无压缩 441 块 **764ms**，`threshold=256` 压缩 441 块 **1359ms**；r=4 81 块 **140ms**，均远低于 5s 且客户端无超时/Decode 错误，因此保留单批以避免额外的批次边界与 PositionSync 顺序复杂度。

**为什么至少 3×3**：客户端 `SectionUpdateTracker.hasAllNeighbors` 要求玩家所在 section 的 4 个正向 + 4 个斜向邻居区块都存在，否则玩家 section 永远不会编译，30 秒后客户端打印超时并强制放行。因此有效视距下限 clamp 到 2（正好 5×5）。

### C2S 移动包 → ECS（P13/P15）

- `mc-network/src/play_events.rs`：`PlayEvent::Movement { addr, conn_id, movement }` / `KeepAlive { addr, conn_id }` / `Disconnect { addr, conn_id }`，经 `NetConfig.play_events: Option<Sender<PlayEvent>>` 从 net 线程投递。
- `play_loop` 解码 0x1E–0x21 后：先处理跨区块（见下），再 `try_send` 事件；日志带 `forwarded_to_ecs=true`。
- `mc-server/systems.rs::handle_new_connections`：每登录成功 `spawn((PlayerAddr(addr), PlayerConnId(conn_id), Position::at_spawn(cfg.seed), PlayerKeepAlive::default()))`，在线数只在此 `fetch_add` 一次；重复 conn_id 幂等。`Position::default()` 也使用默认 seed 的 WorldGen 出生 y，不再写死 67。
- `mc-server/movement.rs::movement_ingest_system`：排空 `PlayEventReceiver`，按 conn_id 找实体 → `MovementQueue.push((entity, MovementUpdate))`；KeepAlive 回显与移动包都刷新 `last_seen`（26.3 客户端站立时也约 1s 发一次移动包，避免 15s 心跳周期与 15s 超时的临界竞态）；Disconnect 登记到 `PendingPlayerRemoval`。
- `movement_system` 排空队列并更新 `Position`；`keepalive_timeout_system` 与 Disconnect 都只登记删除；`player_removal_system` 是 online 唯一递减点（按 conn_id 去重，饱和减）。
- 测试：`mc-network/tests/play_stream.rs` 证明包真的进入 `PlayEvent` 通道；`movement.rs` 单测证明事件进入 MovementQueue、Position 更新、last_seen 刷新且 online 不变，以及 Disconnect→timeout / timeout→迟到 Disconnect 都只减一次、旧 conn_id 不误删新实体；`systems.rs` 单测证明一次 NewConnection 只 spawn 一个实体/只加一次 online、重复 conn_id 幂等。

### 动态区块加载/卸载（P13/P15）

- `mc-network/src/chunk_view.rs`：纯逻辑 `ChunkView` 维护 `center + radius + sent: HashSet<ChunkPos>`。
  - 初始 `plan(center)` 返回半径 r 的全部 `(2r+1)^2` 块（中心优先）；
  - 跨区块 `plan(new_center)` 返回 `add = 新视距 - 已发送`（`view_chunks` 中心优先顺序）和 `remove = 已发送 - 新视距`（按 x,z 排序）；内部集合同步更新，避免重复发送。
  - 单轴跨区块 add/remove = `2r+1`（r=4 → 9，r=10 → 21，r=2 → 5）；斜跨 = `4r+1`（r=4 → 17），单测覆盖 radius 2/4/10。
- `play_loop` 从移动包位置算区块（`floor + div_euclid(16)`，负坐标正确），中心变化时：
  1. `SetCenterChunk`（0x60，VarInt x/z）
  2. 逐个 `UnloadChunk`（0x26）越界区块
  3. `ChunkBatchStart` → N×`ChunkData`（优先 ChunkCache）→ `ChunkBatchFinished(batch_size=N)`；N=add.len，空批次不发。
  - P15 保留“跨区块一批立即发送”，未引入每 tick pending 队列/阻塞 sleep；r=10 单轴 21 块，斜跨最多 41 块。已发送集合语义由 `ChunkView` 单测覆盖（快速移动不重复/不漏块）。
- 0x26 线格式为 **packed i64 ChunkPos**：`x & 0xffffffff | ((z & 0xffffffff) << 32)` 的 big-endian Long（26.3 `ClientboundForgetLevelChunkPacket` 用 `FriendlyByteBuf.writeChunkPos`；已用 26.3.jar `javap` 反编译确认）。旧实现写 `Chunk X Int + Chunk Z Int`，真机会把 x/z 交换、误卸玩家脚下的区块（P13 真机复现并修复）。
- P13 真机（旧平原地形）：压缩模式从 (369,381) 走到约 (145,686)，服务端记录 17+ 次 `view move`（各 add=9/remove=9），玩家始终 `y=65.0, on_ground=true`；20s+ 空闲后 KeepAlive echo 持续、无 ECS timeout。

### 区块数据（`mc-world` + `play_chunk.rs`）

- 真实 blockstate ID（对齐 `reports/blocks.json`）：`air=0`、`stone=1`、`grass_block=9`（snowy=false）、`dirt=10`。
- P14 起 Play 区块由 `WorldGen::generate` 生成：每列草/3 层泥土/石头到 MIN_Y，24 个 section（y=-64..319）。
- 均匀 section 用单值 palette；混合 section 用**间接 palette**（BPE=4，palette 最多 4 项，4096 项 Data Array，1.16+ 不跨 long）。
- `MOTION_BLOCKING` 高度图：`mc_protocol::packets::motion_blocking_heightmap(&[i32;256])` 按实际列高打包，值 = 地表 y + 1 - (-64)，BPE=9、每 long 7 项、37 longs；旧 `motion_blocking_heightmap_uniform` 保留兼容。
- 天空光：26.3 的 BitSet 线格式是 **`VarInt 字节数 + 小端 bit 字节`**（`FriendlyByteBuf.writeByteArray(BitSet.toByteArray())`），不是旧版 `long 数量 + long 数组`。P14 从本 chunk 最低地表 section 到 23 发 2048B 全 15 级 nibble，地下按 0。
- 旧的“原版 (23,23) 区块抓包”调试脚手架已删除；旧 `chunk_for` 平面实现保留给 save/load 占位测试。

### 其它已修

- `LoginPlay` 和 `PlayerPositionSync` 按 777 真布局修正，并用原版抓包 fixture 做逐字节往返测试。
- KeepAlive 周期发送，真机连接可长期保持。
- GameEvent + Chunk Batch 是 26.3 离开“加载地形中…”的必要条件。

## 4. 验证结果（2026-11-23 真机 / 本地）

### Seed 真机（P14）

- 默认 seed `123456789`：服务端 `enter_play ... seed=123456789 spawn=(369,73,381)`、ECS `spawn_y=73.0`、`PositionSync y=73.0`；客户端无 `Timed out while waiting for the client to load chunks`、无 `DecoderException`，截图可见明显丘陵/台阶地形。
- seed `987654321`：服务端 `enter_play ... seed=987654321 spawn=(369,83,381)`、ECS `spawn_y=83.0`、客户端移动包 `y: 83.0, on_ground: true`；地形轮廓与默认 seed 不同，同样无超时/解码错误。
- 同一 seed 的确定性由单测覆盖：`same_seed_same_chunk_is_identical`（同 chunk 逐 section/高度相等）、`net_config_seed_drives_spawn_and_chunk_terrain`（同 seed 的 PositionSync/ChunkData 与 WorldGen 逐字节一致）。

### 压缩真机（threshold=256）

- P13（radius=4 固定时代）：服务端日志 `enter_play ... threshold=256`、`play join sent ... view_sent=81`；客户端无 `DecoderException`、无 `Timed out while waiting for the client to load chunks`、无连接超时；进入世界并渲染地形；KeepAlive 回显 id=1..12 连续。
- P15 复测：`threshold=256` + `view_distance=10`，441 块全部压缩，`play join chunks sent ... count=441 ... elapsed_ms=1359`，客户端无 `DecoderException`/超时，收到 keepalive echo 正常进 Play。

### 视距 / 缓存 / 断线真机（P15）

- **view_distance=10（默认）**：服务端 `enter_play ... view_radius=10 view_chunks=441 cache=1024`；`play join chunks sent ... count=441 ... elapsed_ms=764`；`play join sent ... view_sent=441`。客户端无 `Timed out while waiting for the client to load chunks`、无 `DecoderException`。按住 W 从 spawn (369.5,73,381.5) 向南走到 z≈407.4，服务端记录 `play view move ... center=(23,24) radius=10 add=21 remove=21 sent_total=441`、随后 `(23,25)` 同样 21/21；移动包 `y=73.0, on_ground=true`，地形连续。
- **view_distance=4**：服务端 `view_radius=4 view_chunks=81`；`play join chunks sent ... count=81 ... elapsed_ms=140`；跨 chunk 记录 `play view move ... radius=4 add=9 remove=9 sent_total=81`；客户端同样无超时/解码错误，证明配置真的生效（不再是固定 81/9）。
- **断线**：强制结束客户端后，同一 tick（日志时间差 <1ms）出现 `disconnect ... conn_id=1 -> queue ECS removal` → `despawn player conn_id=1` → `player removal complete removed=1 online=0`，不再等 15s。立即重连：`accept ... conn_id=2` → `new connection ... conn_id=2 (online=1) entity=Entity { index: 0, generation: 2 }` → `play join sent ... view_sent=441`，只有 1 个实体、online=1。
- 客户端日志只有离线模式预期的 authlib/Realms 401 错误，没有区块加载超时或 `DecoderException`。

### 动态区块真机（P13）

- 出生后按住 W 约 8 秒（期间合成按键释放异常，实际持续移动更久），玩家从 (369.5,65,381.5) 走到约 (145.5,65,686.3)，跨 chunk 中心 17+ 次。
- 服务端每次 `view move` 均 `add=9 remove=9 sent_total=81`；无 `DecoderException`、无 30s 超时。
- 玩家全程 `y=65.0, on_ground=true`，未再出现 P13 首次真机时的“走远后掉穿世界”（该问题由 0x26 x/z 交换导致，已修复）。
- 菜单打开后空闲 20s+：KeepAlive echo 持续（id=10..12），无 ECS keepalive timeout。

### 本地检查

- `cargo test --workspace`：全部通过（142 个测试）。
- `cargo check --workspace --tests`：0 error。
- `scripts/check_budget.sh`（Windows 用 `C:\Program Files\Git\bin\bash.exe`）：通过（src ≤2000 行软约束；最大 `mc-network/src/login_route.rs` 约 732 行）。

## 5. 已知限制 / 下一步

1. **socket 仍未进 ECS**：Play 的 Join/Chunk/动态区块实际仍由 net 线程发送；`PlayScaffold.pending_chunks` 只做编码自检/日志。P13/P15 已把移动/KeepAlive/Disconnect 事件送进 ECS，但 socket 所有权/写通道的拆分会影响当前已验证链路，故保留 `TODO(P13/socket→ECS)`（`mc-server/src/play.rs`、`systems.rs`）。
2. **在线数/连接语义**：ECS 玩家实体已有 `PlayerConnId`，Disconnect 在下一 tick 清理，timeout 与 Disconnect 汇入唯一删除点；同 addr 快速重连由 conn_id 区分。仍未按 UUID 去重（同 UUID 多开仍会有多个实体），也未有登录阶段踢掉旧连接的策略。
3. **初始 441 块仍是单批**：真机 debug 下 764ms（无压缩）/1359ms（threshold=256），未触发 5s 阈值，因此没有拆 64 块 + 50ms。若未来弱机器/更大视距（clamp 上限 10 已封顶）或高延迟网络出现客户端超时，再把 `send_join_sequence` 改为每批 64 块、批间 sleep（每批 BatchStart/Finished 成对，PositionSync 最后）。
4. **区块缓存只在本进程内**：`ChunkCache` 是已编码包体的内存 FIFO（默认 1024 块，约 30–40MB 上限），进程重启/换 seed 不保留；没有 Anvil 存档，动态加载/卸载只影响网络发送集合，不落盘。`save/load` 仍是旧的 `Chunk` 占位格式。
5. **跨区块仍是即时整批**：单轴 `2r+1` 块、斜跨最多 `4r+1` 块一次写完，没有每 tick pending 队列/发送限流；r=10 时单次最多 41 块，真机未出现写阻塞或超时。
6. **地形只有高度轮廓**：固定 `minecraft:plains` biome，无树/洞穴/水/雪/矿石/结构；seed 只影响 value-noise 高度。高度差 >3 的陡坡会露出石头（符合“草/土/石”分层，但视觉上不是原版 dirt 全覆盖）。出生点只做“地表 + 1”，没有额外安全检查（本地形无水和虚空，满足当前范围）。
7. **压缩只有阈值可配**：zlib level 固定 6；没有压缩统计/限流；`threshold=0` 会让所有包压缩（仅单测覆盖，未做真机）。
8. `mc-registry` 的 216 项占位表仍是旧数据；区块直接用 `mc-world` 常量，未走该表。

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
  login_route.rs             PlayOptions、Configuration/Play 发送序列、`(2r+1)^2` 块、KeepAlive、动态区块、移动/Disconnect 事件
  chunk_view.rs              clamp_view_radius/view_chunk_count、per-connection 视距集合/去重/跨区块 add/remove 规划
  chunk_cache.rs             共享已编码 ChunkData 0x2E 包体缓存（Mutex+HashMap+FIFO，默认 1024）
  play_events.rs             net 线程 → ECS 的移动/KeepAlive/Disconnect 事件（conn_id 实体键）
  config.rs                  NetConfig（视距/压缩阈值/seed/Play 事件通道/ChunkCache）
  framing.rs                 VarInt 帧 + zlib 压缩帧 + 阈值边界
crates/mc-network/tests/play_stream.rs   移动事件/压缩配置/seed 透传/视距 2/4 初始块与跨区块批次集成测试
crates/mc-world/src/worldgen.rs       WorldGen/GeneratedChunk（value noise、逐列高度、heightmap/light）
crates/mc-world/src/lib.rs             SPAWN_BLOCK_X/Z、旧 Chunk/save 占位、palette/heightmap/light
crates/mc-server/src/
  config.rs                  ServerConfig.seed/compression_threshold/view_distance + effective_view_distance()
  systems.rs                 NewConnection(addr,conn_id) → ECS 玩家实体（Position::at_spawn，重复 conn_id 幂等）
  movement.rs                PlayEvent 排空 + MovementQueue/Position/KeepAlive + PendingPlayerRemoval 唯一删除点
  play.rs                    ECS 视距编码自检/日志（cfg 有效视距，socket 未接入）
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

- 当前分支 `main`；P15 代码 `7f44b20`、文档 `0df9b40`（基线 `12bdaa4`）；P14 `1e47e69`/`12bdaa4`（基线 `c3bd992`）；P13 `02d31ee`/`0aaf8ed`。
- 不要提交：`scripts/check_budget.sh` 的纯行尾/文件 mode 变化、`Cargo.lock`（如无依赖变更）、`logs/`、`target/`、临时日志。
- `HANDOFF.md`、`MEMORY.md` 按仓库习惯更新。
- 旧的 `.gitignore` / `desk_log/conn.log` 删除是历史工作区噪声，已从 HEAD 恢复，不要误提交。
