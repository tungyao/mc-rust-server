# 交接文档：Rust 自研 MC 服务器（只兼容 Java 26.3 / protocol 777 / data 5023）

> 更新日期：2026-11-26；分支 `main` @ `bc86694`（P30 任务库 M1）。
> `tasks/` + `tasks/events.jsonl` 已是任务真相源（见 `tasks/README.md`），本文件保留能力/验证记录。
> P19 Q 扔、P20 `login_route` 拆目录、P21 多人广播、P22 种植、P25 管理面板、P26 树苗长树、
> P27 种田闭环、P28 Q 扔按视角飞 + 三个补丁（拾取延迟/弹道/选中槽同步）均已合入 main；
> 但除 P18 早期真机外，上述新增功能均**未做原版客户端真机复验**（见 §5 各条 TODO）。
> `p31-xp-orbs` 经验球已合入（`2563ebe`，见 §5.26），`p33-multi-crop-planting`
> 多作物种植与 `p37-all-crops` **全部作物可种 + 作物模块拆分**均已合入（见 §5.24）；
> 新世界存储格式已完成**方案设计 + 原型 + 生产核心 + 增长计时持久化**
> （`p36-world-storage` / `f24a0bb`、`p38-store-core`、`p39-timer-persist`，redb，
> 明确不做 Anvil 兼容，**默认 `world_storage = "redb"`**，见 §5.27）；
> `p40-survival` 合成 + 生存基础已合入（见 §5.28）；`p42-worldgen-research`
> 已完成地图生成器调研（见 §5.29，用户已定：v1 冻结 + v2 新 seed、允许改协议 helper、
> 流动水 + 游泳、单 chunk 小遗迹）；`p41-all-recipes` 283 条配方与 `p43-player-visible`
> 玩家实体互见均已合入（见 §5.30）；`p44-worldgen-p1` v1 冻结 + v2 低风险视觉已合入
> （见 §5.29）；`p45-worldgen-p2` v2 地形/多 biome/洞穴/光照与 `p46-worldgen-p3`
> v2 流动水/游泳/block light/遗迹均已合入（见 §5.29）；地图生成三阶段全部落地；
> `p47-gameplay-admin` 工具等级/3×3 工作台/游戏模式/挖掘动画 + 后台联动已合入（见 §5.31）；
> **真机复验仍未做**；用户决策：`p32` 保持 blocked、v2 较慢可接受（不强制回退 v1）、
> 身份采用 **UUID 稳定化 + 存档迁移**（任务 `p51-uuid-identity` 排 socket 完成后派发）；
> `p50` 立项评估已完成，`p52-socket-ecs-b` 方案 B 全量切换已合入（见 §5.34），
> **真机对比待用户执行**；
> 后台管理面板已用 Vue 3 + Naive UI 重写并**完全离线化**合入（`p34-admin-vue` / `53f1016`、
> `p35-admin-offline` / `48e14bd`，见 §5.25）；当前无进行中的派发任务。

## 1. 一句话现状

**当前 main = P18 基线 + P19–P28 + P30（详见 §5.16–23）：掉落看得见、捡得起来、进背包看得见，
另有 Q 扔（按视角）、多人广播、种植/种田闭环与管理面板。** 以下为 P18 基线描述：挖方块按掉落表生成物品实体
（AddEntity+SetEntityData，Spawn 带真实初速），重力下落停在支撑面（客户端预测 +
全服唯一 50ms tick；已移除每连接每 tick 的 TeleportEntitySync 覆盖），靠近 <1.5 格
（挖掘掉落 delay 10 ticks / 手扔 40 ticks 后）拾取（TakeItem 动画+Remove+入库+全量背包同步）；
C2S SetHeldItem 切手持，放置按手持映射扣 1（单槽同步），空手/不可放置物品只 Ack、不落子
（P18.3，已无石头回退）。背包存档 `world/player-<user>-<seed>.dat`（用户名键、seed 隔离），
断线/拾取/放置即刷；重启重进石头与背包都在。原版 26.3 真机进服正常
（Join 441 块 304ms 缓存命中 Rendertime 无 DecoderException，站稳 y=73 on_ground；
真人游玩挖 17/放 12 块，放置精确扣 12 石头，无超时/tick lag）。

P16.1 的能力保持：ECS 只做登录/视距诊断（`last_view_radius`/`last_view_chunks`），
Join 附近无 tick lag（每次登录 1 条 `ready to join`）。真机默认 seed：
无压缩 441 块缓存命中 **38ms**（首启约 442ms 量级，见 P16.1）、
`compression_threshold=256` 下 441 块 **1464ms**，均 0 条 `tick lag`。
客户端无 `Timed out while waiting for the client to load chunks`、无 `DecoderException`，
`on_ground=true`；断线同一 tick `online=0`。

P16 的能力保持：世界有水（water=89，海平面 63）、沙岸、oak 树、地下洞穴/煤铁、每 chunk 单一 biome（ocean/beach/forest/plains）；出生点半径 16 内平滑抬升到干燥草地（≥66），默认 seed 出生 y=73 不变；初始 441 块用 `ChunkCache::bodies` rayon 并行生成+编码，缓存命中重连 72ms。

P13 的能力保持：`compression_threshold` 配置化（默认 -1，256 已真机）、C2S 移动经 `PlayEvent` 进 ECS、跨区块动态补/卸区块、0x26 packed ChunkPos 修正。

上一阶段（P11/P12）的 Configuration 卡点也已解决：根因不是 dimension NBT，而是 `minecraft:block` 标签缺少 `minecraft:infiniburn_overworld`。现在 Configuration 直接回放原版 26.3 抓包的全量注册表与 Update Tags，保证条目 ID 与标签自洽。

## 2. 怎么跑

### 服务端

```powershell
cd H:\code\rust\paper
cargo build -p mc-server
.\target\debug\mc-server.exe        # 监听 0.0.0.0:25565，离线模式
```

`server.toml` 可改 bind / motd / max_players / view_distance / **seed** / **compression_threshold** / **world_dir**：

```toml
seed = 123456789              # 默认；决定 WorldGen 地形与出生点 y（x/z 固定 369/381）
# seed = 42                   # 水景演示：出生 (369,75,381)，向南约 40 格有沙岸/池塘
# seed = -987654321           # 出生 (369,67,381)，东侧约 25 格有更大水域
compression_threshold = -1    # 默认：禁用压缩（原行为）
# compression_threshold = 256 # 启用：Login Success 前发 SetCompression(256)
world_dir = "world"           # P17/P36：存档目录；redb 模式为 state-<seed>.redb，旧 toml 缺键按此补齐
world_storage = "redb"        # P38：新格式（默认）；"legacy" 回退到 overrides/player .dat，旧 toml 缺键补 redb
generator_version = 1         # P44：1=v1 永久冻结（默认，老 seed 逐字节不变）；2=v2 仅新 seed/显式开启
socket_mode = "ecs"           # P52：默认全量 ECS 持有 socket；"net" 为紧急回退（启动期锁定，旧 toml 缺键补 ecs）
```

P16 的 seed 示例：默认 seed 视野（radius 10）内只有树林/丘陵、没有水面（P14 高度分布下 h<63 的列约 0.2%，且不在出生视距内）；`seed=42` 出生点朝南约 40 格是沙岸+池塘，`seed=-987654321` 出生点东侧约 25 格水域更大，两者都有大量 oak 树，真机截图见 §4。

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

## 3. 当前实现（P17 之后）

### 视距（P15）

- 唯一有效视距计算：`mc_network::chunk_view::clamp_view_radius(i32) -> i32`，合法范围 **2..=10**，越界 clamp 并 `warn!`；`view_chunk_count(r)` 返回 `(2r+1)^2`。`ServerConfig::effective_view_distance()`、`NetConfig::effective_view_radius()` 都调用它。
- `ChunkView::new(center, radius)` + `radius()`；`plan` 用实例 radius 调 `mc_world::view_chunks`，不再用固定常量。`PLAY_VIEW_RADIUS=4` / `PLAY_VIEW_CHUNK_COUNT=81` 只保留为旧入口默认值。
- 透传：`ServerConfig.view_distance` → `NetConfig.with_view_distance` → `handshake_route` 构造 `PlayOptions { threshold, seed, view_radius, conn_id, events, chunk_cache }` → `handle_login_with_options` → `enter_play_with_options` → `ChunkView`。旧入口 `handle_login` / `handle_login_configured` / `enter_play` / `enter_play_with_events` 保留，默认 radius=4、无缓存，委托到新实现。
- `LoginPlay.view_distance` 与 `simulation_distance` 都等于实际有效半径，`seed` 同步为 `WorldGen.seed`；初始批 `debug_assert` 按 radius 计算，不再写死 81。
- ECS `mc-server/src/play.rs` 只做登录/视距诊断：`login_join_system` 用 `cfg.view_distance` 的 clamp 值写 `PlayScaffold.last_view_radius` / `last_view_chunks` / `view_center`，每个登录打 1 条 `ready to join`；P16.1 已删除 `pending_chunks`/`sent_chunks`/`CHUNKS_PER_TICK` 与每 tick 的 WorldGen+ChunkData 编码/逐块日志，真发送仍在 net 线程。

### ECS Play 脚手架（P16.1）

- `PlayScaffold` 只保留：`pending_login: Vec<NewConnection>`、`joined`、`keepalive_seq`、`view_center`，以及 P16.1 新增的 `last_view_radius` / `last_view_chunks`（最近一次登录的有效视距与 `(2r+1)^2` 块数；0 表示还没登录）。`spawn_center_chunk()` 保留为测试/诊断入口。
- `login_join_system`：只在新登录时 drain `pending_login`，用与 net 线程相同的 `mc_network::clamp_view_radius(cfg.view_distance)` + `view_chunk_count(radius)` 计算诊断，设置 `view_center = Some(world.spawn_chunk())`，每个登录打一条 `ready to join #N addr=... conn_id=... seed=... spawn=(...) center=(...) view_radius=R view_chunks=N (join 已由 net 线程 enter_play 真下发; TODO socket→ECS 时改用 per-player 写通道)`。
- 删除：`CHUNKS_PER_TICK`、`pending_chunks`、`sent_chunks`、每 tick `WorldGen::generate`/`network_bytes`/`heightmaps_bytes`/`light_bytes`/`encode_play_chunk_data_packet` 与逐块 `chunk data (...) sent=... pending=...` info。空队列/重复 tick 不再产生任何区块工作。
- `keepalive_system` 保持原样（只计数 + 编码自检 + log）。
- 测试：两个登录 drain、view_distance 2/4/10/11 → 25/81/441/441、空队列诊断全 0、连续 10 tick 不改变 joined/视距诊断、`scaffold_encode_does_not_panic` 单块编码。
- TODO(socket→ECS)：socket 仍在 net 线程。未来若交给 ECS，必须用 **per-player 写通道 + `ChunkView`** 真发 Join/Chunk/PositionSync；不要恢复 P16.1 删除的全量生成脚手架。

### 区块缓存（P15）

- `mc-network/src/chunk_cache.rs`：`ChunkCache::new(seed, capacity)`，共享 `Arc<ChunkCache>`，内部 `Mutex<HashMap<ChunkPos, Arc<Vec<u8>>>>` + FIFO `VecDeque<ChunkPos>`。
- 缓存的是**已编码 ChunkData 0x2E 包体**（P16 约 19KB sections + ~33KB 光照 ≈ 52KB/块），不是 `GeneratedChunk`（约 196KB/块）；默认容量 `DEFAULT_CHUNK_CACHE_CAPACITY=1024`，覆盖 radius=10 的 441 块并留移动余量。
- `body(pos)` 命中返回同一 `Arc`（不重新生成/编码），未命中在锁外生成后插入；容量满按 FIFO 淘汰；命中/未命中计数在 `ChunkCacheStats`，不逐块打 info。
- P16 新增 `bodies(&[ChunkPos]) -> Vec<Arc<Vec<u8>>>`：保持输入顺序，命中直接复用，未命中用 rayon（mc-network 新增 workspace 依赖）并行生成+编码；`send_chunk_batch` 走它，初始 441 块真机 **496–558ms**、同进程缓存命中重连 **72ms**。逐个 `body()` 语义未变，旧入口无缓存时仍顺序生成。
- `NetConfig.chunk_cache: Option<Arc<ChunkCache>>` + `.with_chunk_cache()`；`main.rs` 用 `cfg.seed` 建缓存注入。`enter_play` 在 cache seed 与 server seed 不一致时 `warn!` 并回退直接生成，绝不串地形。Join、跨区块补发都优先走 cache。
- P17：`ChunkCache::with_mutable(Arc<MutableWorld>)` 绑定可变世界后编码走基底 + override；`invalidate(pos)` 丢弃编辑 chunk，下次发送重编。单测：失效后重编与基底不同且含新方块、未绑定时与纯基底逐字节一致。
- 单测：命中字节与直接编码逐字节一致且复用 `Arc`、`bodies` 顺序/字节/重复命中、FIFO 淘汰最旧、不同区块/不同 seed 不串数据、容量 0 仍能生成。

### 方块交互（P17：C2S 挖掘/放置 + S2C BlockUpdate/Ack）

- 包 ID（777，已按 `docs/packet-map-777.md` §6 核对 + 26.1 protocol.json +1 漂移印证）：
  C2S Player Action `0x29` / Use Item On `0x42` / Use Item `0x43` / Punch `0x2E`（空包体忽略）；
  S2C Block Update `0x08`（Position + blockstate VarInt）/ Acknowledge Block Change `0x04`（Sequence VarInt）。
  `Set Held Item 0x36` 已按 Short 解析切手持（P18.3）；`Set Creative Mode Slot 0x39` 仍忽略不断线
  （Slot 的 data component 需注册表驱动解码，误解析会错位整条 TCP 流）。
- 判定在 **net 线程**（`login_route::decide_dig`/`decide_place`，`play_loop` 内即时执行）：
  socket 所有权在 net 线程、ECS 没有写通道，权威判定必须和 socket 写在同一线程完成；
  判定规则仍是服务端规则（非客户端 claims）。ECS 通过共享 `Arc<MutableWorld>` 看到同一世界，
  只负责刷盘（见下）。`PlayEvent::BlockEdit` 投递给 ECS 只做日志/未来多人钩子，
  脏标记已在 `MutableWorld::set` 内记录。
- 规则（生存风格，26.3 jar ordinal）：`Player Action` status 0(START) → 只 ack（客户端播破坏进度动画）；status 3(STOP，进度满后）→ 即时破坏（落 air）；status 2(ABORT，松开鼠标）→ 只 ack 不落子；status 1(CHANGE 朝向）→ 直接忽略；
  status 4/5 为 Q 扔整组/单个（P19；按视角飞见 §5.21），其余 status 无世界影响直接忽略。
  `Use Item On` 目标 = 被点方块 + face 偏移，
  被点须实心（非空气/水）、目标须可替换（空气/水）、y∈[-64,319]、目标 chunk 在视距内、
  眼到被点中心 ≤6.5（`MAX_REACH`，原版约 6 + 0.5 网络余量）→ 按手持物品映射落方块（P18.3）
  或走种植/耕地/骨粉专属路径（P22/P27）；空手/不可放置物品只 Ack 不落子，不回退石头；
  否则回滚（重发目标现状 + ack）。`Use Item 0x43` 只 ack。
- 落子后同 socket 即时发 `BlockUpdate`（变更/回滚都发，保证操作者视角立即一致）再发 `Ack(seq)`；
  失效对应 chunk 缓存。`play_loop` 跟踪 `last_pos`（移动包更新，初值出生点），`eye = feet + 1.62`。
- `world` 为 None（旧入口/单测无世界）时交互包只忽略不断线——旧行为与 Play 链路不受影响。
- 单测：挖草/完成都破坏、挖空气·超距·出视距·y越界回滚且不加条目、ABORT(2)/CHANGE(1) 不落子、4+/无世界忽略；
  点泥土顶放置成功、目标被占/点空气/非法face/超距回滚；socket 端到端（见 §4）。
- P18：`BlockEditOutcome` 新增 `drops: Vec<Drop>`（仅挖掘成功时非空，回滚/cancel 为空；
  `world.seed()` 内部取 seed 算树叶概率）。`decide_place` 签名不变，材料由调用方按手持选定。

### 掉落/实体/背包（P18：生存第一棒）

- 包 ID（777，已按 `docs/packet-map-777.md` §7 核对，来源 minecraft.wiki Play 包表 +
  26.3 客户端 jar 实测）：
  S2C SpawnEntity `0x01`（type=`minecraft:item`=72，data=0）/ SetEntityData `0x65`
  （index 8/type Slot(7)）/ RemoveEntities `0x4E` / TeleportEntity `0x23`（线性路径，
  掉落同步用它，不用差量基准）/ UpdateEntityPos `0x36`（step=0，实现+单测，运行时
  优先用 0x23）/ TakeItemEntity `0x7F`（collector=LoginPlay entity_id 1）/
  SetContainerContent `0x12`（window 0 全量 46 槽）/ SetContainerSlot `0x14` /
  S2C SetHeldSlot `0x6B`（VarInt；Join 时 selected≠0 同步，P28.3）；
  C2S SetHeldItem `0x36`（**Short** 0..=8，与 S2C `0x6B` VarInt 不同）/ ClickContainer `0x12`
  （P18.1 已解码并服务端权威处理 PICKUP/SWAP/QUICK_MOVE，P19 追加 THROW，点后全量同步）。
- 物品 ID（`BuiltInRegistries.ITEM` 实测，工具放 Temp 未提交）：
  stone 1 / dirt 55 / cobblestone 62 / oak_sapling 77 / sand 88 / oak_log 163 /
  coal 1010 / stick 1060 / raw_iron 1017；entity：item 72 / exp orb 50 / player 159。
- 掉落表（`mc-world/src/drops.rs`，确定性、无随机状态；工具等级/精准采集留 TODO，
  限制见 §5）：
  grass/dirt→dirt(55)，stone→cobblestone(62)，sand→sand(88)，oak_log→oak_log(163)，
  oak_leaves→1/8 sapling(77)另 1/8 stick(1060)（world+pos hash），coal_ore→coal(1010)，
  iron_ore→raw_iron(1017)，其余无掉落。
- 物品实体（`mc-world/src/items.rs` 纯逻辑 + `mc-network/src/drop_state.rs` 共享状态；
  net 线程权威，ECS 无写通道，沿用 P17 模式）：
  挖掘掉落 pickup_delay=10 ticks、手扔 40 ticks（P28.1）；挖掘掉落初速上抛 0.25+hash
  水平扰动，手扔为眼位前方 + 朝向×0.06 + 上抛 0.1（P28.2 轻掷保证可自捡）；
  重力 0.04/阻尼 0.98、落点 `support_top_y`（支撑被挖空则重算，P18.3）；
  同物品同 chunk 半径 1 格合并（上限 64，超出留新实体）；
  6000 ticks despawn；delay 到期 + 与脚位置 <1.5 拾取；
  entity_id 全局自增（1000 起，LoginPlay entity_id=1 不冲突，永不复用）。
- 同步：生成 Spawn+Data（视距内才发）；每 50ms tick（读超时即跑，不改变 10s 半开语义）
  移动发 Teleport(0x23)，despawn/出视距发 Remove；进视距补发；拾取发 TakeItem+Remove。
  跨区块按 ChunkView 半径判定。P21 起方块/掉落包经 `BroadcastHub` fan-out 给视距内其它连接
  （Ack 只回操作者本人），各连接按对端维护 known 集去重；玩家实体互见已由 §5.30 P43 补齐，挖掘动画 0x05 仍缺。
- 背包（`mc-world/src/inventory.rs`，46 槽：9–35 主/36–44 快捷栏/0–8 合成盔甲未用；
  selected 0..=8 ↔ 36+selected）：
  Join 读盘+全量 Content（selected≠0 另发 S2C 0x6B，P28.3）；拾取 add_item
  （同物合并→快捷栏优先→主包，上限 64，禁入合成/盔甲槽，P18.1）后全量同步；
  放置按手持映射（stone/dirt/sand/log/cobblestone 等）选材料、成功 consume_held 扣 1
  + 单槽同步；空手/不可放置物品只 Ack 不扣（P18.3）。
  C2S ClickContainer PICKUP/SWAP/QUICK_MOVE 服务端权威判定 + 点后全量同步（P18.1）；
  Q/背包 Q 扔走 `throw_held`/`throw_from_slot`（P19）+ 按视角飞（P28）。
  新背包开局送 64 石头（hotbar 槽 36，后续生存模式替换）。
  存档 `world/player-<user>-<seed>.dat`（P18I 魔数/版本/selected/state/条目，tmp+rename；
  **键用用户名**：离线 `offline_uuid` 每次随机 v4，uuid 键重连永远 miss，见 inventory 文档；
  在线模式落地后切回 uuid）。
  刷盘：拾取/放置/断线即刷（net 线程，ECS 不经手）；`unload_inventory` 断线保存+释状态。

### 可变世界与存档（P17：MutableWorld + 自有稀疏格式）

- `crates/mc-world/src/mutable.rs::MutableWorld { seed, base: WorldGen, RwLock<Inner> }`：
  `Inner { overrides: (x,y,z)→u16, dirty: HashSet<ChunkPos> }`，只存与基底不同的方块
  （设回基底值删条目，保持稀疏）；读 = override 优先；`set` 记 dirty。
- `generate_chunk(pos)` = 基底整 chunk + 逐条贴 override + 只重算被触碰列的
  `heights`（地形集合：石头/土/草/沙/矿）/`motion_heights`（最高非空气，含水/树叶/原木，
  与 P16 基底规则一致）；其余列沿用基底值。编码/高度图/光照走与纯基底同一路径，
  override 为空时与纯 WorldGen 逐字节一致（单测）。
- `ChunkCache` 绑定后 Join/跨区块补发自动含编辑；`save/load` 占位（旧 `Chunk` 格式）未动。
- 存档 `world/overrides-<seed>.dat`：魔数 `P17W` + 版本 1 + seed + 条数 + 条目
  （x/y/z i32be + id u16be，按坐标排序），tmp 写 + rename 原子替换。seed 在文件名隔离，
  换 seed 用新文件、旧文件保留并 warn；文件内 seed 不符报错；`world_dir` 默认 `world/`。
- 触发：`persist::autosave_system` 每 1200 tick（60s）且脏时刷 / `PendingPlayerRemoval` 非空
  （断线/超时登记）且脏时立即刷（**必须排在 `player_removal_system` 之前**，否则 pending
  已被 drain 而永远看不到断线）；`main` 退出时全量刷。启动 `load_from_dir`（缺文件=空世界），
  出生点若被 override 埋住则向上抬到空气（`MutableWorld::spawn_pos`，ECS 建实体用它）。
- 单测：读写/sparse 删除/dirty、挖顶 motion 下降+填回与基底一致、塔放 motion 上升且高度图含新值、
  y 越界 no-op、存盘 round-trip、seed 隔离、坏文件报错、出生点抬升、allowlist。
  与 Anvil 的差距见 §5.4。

### 连接生命周期（P15）

- `serve` accept 时用 `AtomicU64` 分配单调 `conn_id`；`NewConnection { addr, conn_id }` 同步到 ECS。`PlayerConnId(u64)` 是跨事件实体键，`PlayerAddr` 只用于日志。
- `PlayEvent::{Movement, KeepAlive, Disconnect}` 都带 `{ addr, conn_id }`；`enter_play_with_options` 在**所有退出路径**（正常 EOF、读错误、写失败、KeepAlive 超时、Join 序列失败）统一发一次 `Disconnect`，join 失败也不会在 ECS 留下永久实体。
- ECS `movement_ingest_system` 收到 Disconnect 只登记 `PendingPlayerRemoval`（conn_id 去重）；`keepalive_timeout_system` 也只登记。唯一删除点 `player_removal_system` 按 conn_id 找实体 → despawn → online 饱和减一次。未找到的 conn_id 保留约 1s 重试，覆盖 NewConnection/Disconnect 跨 channel 观察顺序的极窄竞态；conn_id 永不复用，旧连接的迟到 Disconnect 不会误删同 addr 的新实体。
- `handle_new_connections` 对重复 conn_id 幂等（已有实体 + 同 tick seen 集合），online 只在 spawn 时 `fetch_add` 一次。

### 地形生成（P14 噪声 + P16 水/沙/树/洞穴/矿/biome）

- `crates/mc-world/src/worldgen/{mod,noise,encode,tests}.rs`（对外 API 仍是 `mc_world::WorldGen` / `GeneratedChunk`，`worldgen.rs` 拆成子模块，`src` 每文件 ≤2000 行）。
- 高度：世界坐标 (x,z) 的 3 层平滑 value noise（scale 128/48/24，幅度 20/10/6），基高 58，地表 y 严格落在 58..94；相邻列最多差 3（单测覆盖）。`height_at` 直接用世界坐标，chunk 边界连续；负坐标/远坐标用 `div_euclid`/wrapping hash，不溢出。
- 出生点保护：x=369、z=381 固定；半径 16 内把低于 `SPAWN_DRY_SURFACE_Y=66` 的列按 smootherstep 抬到 66（半径 4.5 内平台），只抬不降，因此相邻列仍 ≤3。`spawn_pos().1 = height_at(369,381)+1`：默认 seed 73、987654321=83、42=75、-987654321=67，全部 > SEA_LEVEL+1，3×3 是草方块、干燥、无树/洞穴。
- 分层：`SEA_LEVEL=63`。h > 65 → 顶层 `grass_block(9)` + 3 层 `dirt(10)` + `stone(1)` 到 `MIN_Y(-64)`；h ≤ 65 → 顶层 4 格 `sand(121)` + stone；h < 63 → h+1..=63 填 `water(89)`。
- 洞穴：3D value noise（scale 24，按 4×4×4 格采样，同格 64 方块一起判断）在 `MIN_Y+9 ..= h-9` 只挖 `stone`；地表 8 格壳（h-7..=h）完整，洞穴不会与水体连通。
- 矿：剩余 stone 按确定性 hash 替换 `coal_ore(136)`（y 0..80，约 1%）/`iron_ore(134)`（y -40..56，约 0.8%）；只替换 stone，不用 copper_ore。
- 树：草方块、h>65、4 邻域高差 ≤1 的列，按 biome 密度生成 oak（forest 1/10、plains 1/40；beach/ocean 0）：trunk 4..6 格 `oak_log(140)`；树冠 top-2/top-1 两层 5×5 去角、top 层 3×3、top+1 十字 `oak_leaves(294)`；leaves 只覆盖 air/leaves，trunk 可覆盖 leaves。每 chunk 扫描 `base-2..=base+17` 候选树，只把落在本 chunk 的方块写入 → 跨 chunk 树干/树冠连续；出生点 7 格内不生成树原点（保证最近树叶在 4 格外），水下/沙滩/海底不长树。
- biome：每 chunk 单一值，所有 section（含 air/水）的 biome 容器都用 `GeneratedChunk.biome_id`。规则：chunk 内 4×4 采样，水样本 ≥7（>0.4）→ `ocean(36)`；沙样本 ≥7 → `beach(3)`；否则低频森林噪声（scale 96）>0.55 → `forest(22)`，其余 `plains(41)`；出生 chunk 若算成 ocean/beach 强制 forest/plains。不做 4×4×4 多 biome，chunk 边界颜色可能略硬。
- ID 表（来源：26.3 客户端 jar `SharedConstants.tryDetectVersion(); Bootstrap.bootStrap(); Block.getId(block.defaultBlockState())`，air/stone/grass/dirt 与 `reports/blocks.json` 一致；biome ID 由仓库 `crates/mc-protocol/src/packets/data/vanilla_config_777/registry_00.bin` 的 `minecraft:worldgen/biome` 67 项顺序解析确认）：

  | 方块 | ID | 用途 | | biome | 网络 ID |
  |---|---|---|---|---|---|
  | air | 0 | 空气 | | plains | 41 |
  | stone | 1 | 地下/洞穴壳 | | forest | 22 |
  | grass_block | 9 | 干燥地表 | | beach | 3 |
  | dirt | 10 | 草下 3 层 | | ocean | 36 |
  | water | 89 | 海平面 63 以下 | | river | 42（未用） |
  | sand | 121 | h≤65 顶层 4 格 | | birch_forest | 4（未用） |
  | oak_log | 140 | 树干（axis=y） | | snowy_plains | 47（未用） |
  | oak_leaves | 294 | 树冠 | | meadow | 33（未用） |
  | coal_ore | 136 | y 0..80 | | | |
  | iron_ore | 134 | y -40..56 | | | |

- 编码：`GeneratedChunk::network_bytes` 每节统计 `block_count`（非 air，含水）与 `fluid_count`（water 数）；均匀 air 用 `encode_empty_section_with_biome`；均匀水/石用 `encode_uniform_section_single(...)`（全水 4096/4096）；混合节用 `encode_block_section_with_fluid`，palette 自适应 BPE（>256 仍走 direct）。旧签名 `encode_empty_section_placeholder` / `encode_block_section` 保留（fluid=0/plains），其它代码不受影响。
- 高度图：`GeneratedChunk.motion_heights[256]` = 每列最高阻挡方块（水/树/草/沙/石）+1-MIN_Y，`heightmaps_bytes` 用它；`heights` 仍表示草/沙地形顶（spawn/测试/天空光起点）。
- `light_bytes`：取本 chunk 最低地表 `min_h`，从 `(min_h-MIN_Y)/16` section 向上发 15 级天空光；地下无数组（0）。水下 15 级可接受；已点亮 section 内的洞穴也会偏亮（已知简化）。
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

**初始批策略（P15 结论，P16 复测）**：仍允许同一批发送全部 `(2r+1)^2` 块，不拆 64 块/50ms。P16 起 `ChunkCache::bodies` 用 rayon 并行生成+编码初始批（`send_chunk_batch`），真机 debug 实测：r=10 无压缩 441 块 **496–558ms**，`threshold=256` 压缩 441 块 **1916ms**，重连缓存命中 **72ms**；r=4 81 块保持旧行为。均远低于 5s 且客户端无超时/Decode 错误，因此保留单批以避免额外的批次边界与 PositionSync 顺序复杂度。

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

- 真实 blockstate ID（对齐 `reports/blocks.json` + 26.3 客户端 jar `Block.getId`）：`air=0`、`stone=1`、`grass_block=9`（snowy=false）、`dirt=10`、`water=89`、`sand=121`、`oak_log=140`、`oak_leaves=294`、`coal_ore=136`、`iron_ore=134`。
- P16 起 Play 区块由 `WorldGen::generate` 生成：草/沙顶层 + 3 层土/沙 + 石头到 MIN_Y，水/树/洞穴/矿，24 个 section（y=-64..319），每节 `block_count`（非 air，含水）与 `fluid_count`（water 数）。
- 均匀 section 用单值 palette（air 节带 chunk biome；全水节 4096/4096）；混合 section 用**间接 palette**（BPE=max(4,ceil(log2(palette)))；P16 palette 约 4–10 项；>256 仍走 direct BPE=15），4096 项 Data Array，1.16+ 不跨 long。
- `MOTION_BLOCKING` 高度图：`mc_protocol::packets::motion_blocking_heightmap(&motion_heights)` 打包每列最高阻挡方块（水/树/草/沙/石）+1-(-64)，BPE=9、每 long 7 项、37 longs；旧 `motion_blocking_heightmap_uniform` 保留兼容。
- 天空光：26.3 的 BitSet 线格式是 **`VarInt 字节数 + 小端 bit 字节`**（`FriendlyByteBuf.writeByteArray(BitSet.toByteArray())`），不是旧版 `long 数量 + long 数组`。P16 仍从本 chunk 最低地表 section 到 23 发 2048B 全 15 级 nibble，地下按 0；水/树不改变起点。
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

### P16 地形真机（水/树/biome/压缩/重连）

- **默认 seed `123456789`**：`enter_play ... seed=123456789 spawn=(369,73,381)`；`play join chunks sent ... count=441 ... elapsed_ms=558`；客户端无 `Timed out while waiting for the client to load chunks`、无 `DecoderException`；玩家站在地表 `y=73.0, on_ground=true`。截图 `logs/p16-default-forest.png`：草地 + 大量 oak（树干站在草上、树冠连续），默认视距内没有水面（与 §2 说明一致）。
- **水景 seed `42`**：`enter_play ... seed=42 spawn=(369,75,381)`；441 块 **496ms**。按住 W 从 spawn 向南走到 `(371.9,64.0,422.1)`（y=64、`on_ground=true`），服务端记录 `view move center=(23,24)/(23,25)/(22,25)/(22,26)/(23,26)/(24,26) radius=10 add=21 remove=21 sent_total=441`。截图：
  - `logs/p16-water42-spawn.png`：出生点开阔草甸 + 树木，远处可见沙岸。
  - `logs/p16-water42-beach.png`：**水面为正常蓝色**（非黑块/空洞）、完整沙岸、周围森林；玩家站在沙上 `on_ground=true`。
  - `logs/p16-water42-view2.png` / `logs/p16-water42-crosschunk.png`：树冠下的池塘与跨 chunk 后的水面/沙岸连续，无断块。
- **缓存命中重连**：强制关闭客户端后服务端同一 tick `disconnect conn_id=1` → `despawn player conn_id=1` → `player removal complete removed=1 online=0`；重启客户端 `conn_id=2`，`play join chunks sent ... count=441 ... elapsed_ms=72`（共享缓存命中），只有 1 个实体。
- **压缩 `threshold=256` + radius 10**：seed 42，`play join chunks sent ... count=441 ... elapsed_ms=1916`；客户端正常进世界，移动包 `y=75.0, on_ground=true`，无 `DecoderException`、无区块加载超时。
- 客户端日志只有离线模式预期的 authlib/Realms 401；P16 时初始 Join 附近有 `tick lag: 73–79ms > 50ms` 的 warn（ECS 每 tick 编码自检 + 并行生成争 CPU），P16.1 已删除该自检（见下）。
- 真机确认水不是“黑块/空洞”：可见蓝色水面、水面高度统一为 63、沙岸贴合水面；树没有悬空/穿水，跨 chunk 树冠连续。

### P16.1 真机（删除 ECS 区块自检）

- **默认 seed `123456789`、view_distance=10、threshold=-1**：日志中 `ready to join` **1 条**、`chunk data (...)` 脚手架 **0 条**、`tick lag` **0 条**；`play join chunks sent ... count=441 ... elapsed_ms=442`、`play join sent ... view_sent=441`；移动包 `y=73.0, on_ground=true`；客户端无 `Timed out while waiting for the client to load chunks`、无 `DecoderException`。
- 跨区块：按住 W 从 spawn 向西走到 `x≈359.9`，`play view move ... center=(22,23) radius=10 add=21 remove=21 sent_total=441`，地形/树连续无超时；断线同一 tick `player removal complete removed=1 online=0`。
- **compression_threshold=256 + radius 10（默认 seed）**：`ready to join` 1 条、脚手架 0 条、`tick lag` 0 条；`play join chunks sent ... count=441 ... elapsed_ms=1709`；客户端正常进世界、`y=73.0, on_ground=true`，断线 `online=0`。
- 对比 P16：同样 441 块无压缩 496–558ms → 442ms，压缩 1916ms → 1709ms；tick lag 从 73–79ms（压缩曾 315ms）降为 0 条，日志从 441 行/登录降为 1 行/登录。


### P17 真机（单人挖/放 + 服务端权威 + 重启不丢；脚本客户端直连真服务端）

- **默认 seed `123456789`、view_distance=10、threshold=-1**（真实 `mc-server` 二进制，脚本客户端走完整握手/配置/Play）：
  Join 441 块 → 移动到出生点 → 挖脚下草方块（`Player Action 0x29 finish seq=11`）→ 服务端回 `BlockUpdate(air) + Ack(11)`；
  点下方泥土顶面放石头（`Use Item On 0x42 seq=12`）→ 回 `BlockUpdate(stone=1) + Ack(12)`。
  服务端日志 `play block edit ... pos=(369,72,381) 9->0 seq=11` / `0->1 seq=12`。
- **走远再走回**：向南 11 区块（spawn chunk 被卸载）再北返，每次跨区块 `add=21 remove=21`；
  回到视距后重发的 spawn chunk 解码得目标方块仍是石头（缓存失效 + override 重编路径正常）。
- **重启不丢**：断线当 tick `world saved ... (periodic=false on_disconnect=true)`，存档
  `world/overrides-123456789.dat` 31B（1 条目）；杀进程重启 → 启动日志
  `world loaded dir=world seed=123456789 overrides=1 dirty=0` → 只读重进解码出生 chunk，
  目标仍是石头(1)。
- **compression_threshold=256**：同样全流程通过；`play join chunks sent ... count=441 ... elapsed_ms=1464`；
  BlockUpdate/Ack/跨区块/断线刷盘均正常，无解码错误。
- 原版 26.3 客户端冒烟：**未完成**。本环境两次启动均停在 atlas 加载后、无连接尝试
  （服务端无 accept，客户端 stdout 只有离线 401/Realms 噪声前即停滞；与服务端无关——同期脚本客户端
  正常进服）。Join 路径与 P16.1 逐字节一致（只新增 C2S 分支与可选 mutable 编码路径，无交互时行为不变），
  且脚本客户端已对真服务端验证完整握手/配置/Play/压缩全流程。**P18 已补上**（见下）。

### P18 真机（掉落+拾取+背包；原版 26.3 客户端 + 脚本客户端真服务端）

- **脚本客户端真服务端**（`mc-server` 二进制，默认 seed，view_distance=10，threshold=-1）：
  Join **441 块 330ms**（P17 的 383ms 量级；P18 Join 多 1 个 Content 包，开销可忽略）；
  挖脚下草 → `BlockUpdate(air)+Ack` + `SpawnEntity(id=1000)+SetEntityData(dirt×1)`；
  0.5s 后（delay 10 ticks）`TakeItemEntity(collector=1)` + `RemoveEntities` +
  全量 `SetContainerContent`（dirt 进槽 9）；切手持 1→0；点泥土顶放石头 →
  `BlockUpdate(stone)+Ack` + `SetContainerSlot(36→63)`。
  断线当 tick 存盘（`world saved ... on_disconnect=true`），`player-Smoke18-*.dat` 30B；
  杀进程重启 → `world loaded ... overrides=1`，重进 Join Content 即 63 石头+1 泥土，
  世界石头仍在。压缩链路未变（P17 已覆盖），本轮 e2e 跑明文。
- **原版 26.3 客户端**（首次进服成功，P17 曾卡 atlas）：
  `enter_play ... view_chunks=441` → `play join chunks sent ... count=441 elapsed_ms=304`
  → `play join sent ... view_sent=441`；客户端渲染 chunk（Chunk Sections 扩容日志），
  无 `DecoderException`、无 `Timed out while waiting for the client to load chunks`；
  站稳 `y=73.0, on_ground=true`，移动包约 1s 一包、KeepAlive 正常；**P18 Join 新增的
  Content 包被原版接受**（进 Play 链路未破）。
  真人游玩约 3 分钟：挖 17 块（草/泥土 9/10→0）+ 放 12 块石头（0→1），服务端精确扣
  12 石头（64→52，`player-ProbePlayer-*.dat` 21B），不断线、无报错、无 tick lag。
  掉落物拾取的“可见动画”由脚本 e2e 覆盖（真人未做砍树/跟随拾取，留手动复验）。
- 客户端日志只有离线模式预期的 authlib/Realms 401，无区块加载超时/解码错误。

### 动态区块真机（P13）

- 出生后按住 W 约 8 秒（期间合成按键释放异常，实际持续移动更久），玩家从 (369.5,65,381.5) 走到约 (145.5,65,686.3)，跨 chunk 中心 17+ 次。
- 服务端每次 `view move` 均 `add=9 remove=9 sent_total=81`；无 `DecoderException`、无 30s 超时。
- 玩家全程 `y=65.0, on_ground=true`，未再出现 P13 首次真机时的“走远后掉穿世界”（该问题由 0x26 x/z 交换导致，已修复）。
- 菜单打开后空闲 20s+：KeepAlive echo 持续（id=10..12），无 ECS keepalive timeout。

### 本地检查

- `cargo test --workspace`：全部通过（2026-11-27 实跑 **516 passed / 0 failed**；
  P18 当时 215，之后 P18.1–P52 又追加单测与 play_stream / play_actions / play_crops /
  play_all_crops / timer_persist / play_survival / play_recipes / player_visibility /
  fluid / play_tools / socket_ecs_e2e / admin_e2e 集成测试）。
- redb 生产核心真机进程冒烟（2026-11-26）：临时 `server.toml` 设 `world_storage="redb"`、
  独立端口与 world_dir，启动后日志 `redb world opened seed=... / world loaded storage=Redb`
  → `listening` → ticker/keepalive 正常，目录下生成 `state-<seed>.redb` 与 `migration-<seed>.json`，
  无 panic/退出。
- `cargo check --workspace --tests`：0 error。
- `scripts/check_budget.sh`（Windows 用 `C:\Program Files\Git\bin\bash.exe`）：通过（src ≤2000 行软约束）。

## 5. 已知限制 / 下一步

1. **socket 仍未进 ECS（P17 有意为之）**：Play 的 Join/Chunk/动态区块/方块回包实际仍由 net 线程发送；
   权威判定（`decide_dig`/`decide_place`）与 socket 写必须在同一线程完成，因为 ECS 没有写通道。
   ECS 通过共享 `Arc<MutableWorld>` 看到同一世界并负责刷盘；`PlayEvent::BlockEdit` 只做日志/未来多人钩子。
   未来 socket→ECS 时必须用 per-player 写通道 + `ChunkView`，不要恢复已删除的全量生成脚手架。
2. **在线数/连接语义**：ECS 玩家实体已有 `PlayerConnId`，Disconnect 在下一 tick 清理，timeout 与 Disconnect 汇入唯一删除点；同 addr 快速重连由 conn_id 区分。仍未按 UUID 去重（同 UUID 多开仍会有多个实体），也未有登录阶段踢掉旧连接的策略。
3. **初始 441 块仍是单批**：P17 真机 debug 下缓存命中 38ms（无压缩）、1464ms（threshold=256 新进程），未触发 5s 阈值，因此没有拆 64 块 + 50ms。若未来弱机器/更大视距（clamp 上限 10 已封顶）或高延迟网络出现客户端超时，再把 `send_join_sequence` 改为每批 64 块、批间 sleep（每批 BatchStart/Finished 成对，PositionSync 最后）。
4. **存档是自有稀疏格式，不是 Anvil**：`world/overrides-<seed>.dat` 只存与基底不同的方块
   （魔数 `P17W`/版本/seed/条数/条目 x,y,z,id），无 Region 文件头、无 Chunk NBT（Sections blockstates/biomes、
   Heightmaps、block_entities）、无压缩、无旧扇区回收。换 seed 靠文件名隔离（旧文件保留）。
   下轮补真 Anvil 时：读 `r.<rx>.<rz>.mca`（Region 头 + zlib NBT），写 Sections 的 block_states/biomes
   palette + MOTION_BLOCKING 高度图 + 空 block_entities；自有格式文件届时做一次性迁移或废弃。
   `save/load.rs` 的旧 `Chunk` 占位格式未动，与本存档无关。
5. **放置按手持映射**：`Set Held Item 0x36` 已按 Short 解析切手持
   （`Set Creative Mode Slot 0x39` 仍忽略，data component 需注册表驱动解码）；
   放置材料 = 手持物品映射（stone/dirt/sand/oak_log/cobblestone→对应方块，
   树苗/小麦种子走种植专属路径）+ 成功扣 1，
   空手/无方块映射（如煤/铁/木棍/小麦）只 ack 不落子（P18.3 去掉了 P17 石头回退；
   圆石映射缺失曾导致客户端预测幽灵块，后已补齐，见 §5.22）。
   新背包开局送 64 石头（后续生存模式/创造背包替换）。
   创造/生存都按即时破坏处理（生存按住挖会触发两次：start 已破坏，
   finish 命中空气回滚重发空气，无副作用）。
6. **跨区块仍是即时整批**：单轴 `2r+1` 块、斜跨最多 `4r+1` 块一次写完，没有每 tick pending 队列/发送限流；r=10 时单次最多 41 块，真机未出现写阻塞或超时。
6. **地形/水/树仍是最小实现**：
   - 水是静态 source 方块（water=89，level 0），没有流向/水位更新/游泳物理；`fluid_count` 只统计 water；默认 seed 视距内没有水面（P14 高度分布均值 ~76，只有约 0.2% 列 <63），水景需要 `seed=42` 或 `-987654321`。
   - 洞穴按 4×4×4 格采样（省 CPU），形状偏方块化；**P17 起玩家可挖到洞穴**，已点亮 section 内的洞穴仍会得到 15 级天空光（偏亮；挖掘后重算光照起点只看最低地形高度，未做泛光）。
   - 每 chunk 单一 biome，chunk 边界可能有草色/树叶色硬切；出生 chunk 强制 forest/plains。没有 4×4×4 多 biome、雪原/河流/结构。
   - 树是确定性 oak 小树（无 sapling/树叶 decay/随机高度分布），出生点 4 格内无树叶；树密度 forest ~1/10、plains ~1/40，beach/ocean 不长树。
   - 沙滩/水下顶层 4 格沙是硬编码（h≤65），陡坡会露出石头/沙层侧面；没有 gravel/sandstone 过渡。
   - 矿物只有 coal/iron，只在 stone 中替换；没有铜/金/红石/钻石，也没有矿洞光照。
7. **压缩只有阈值可配**：zlib level 固定 6；没有压缩统计/限流；`threshold=0` 会让所有包压缩（仅单测覆盖，未做真机）。
8. ~~`mc-registry` 的 216 项占位表仍是旧数据~~ **已过时，见第 14 条**：数据驱动 P0–P4
   已用 `data/777/*.json` 生成 1286 方块 / 1658 物品 / 161 实体 / 全量包 ID 表，
   真实 ID 单一数据源，假 ID 已全部替换。
9. ~~多人同步缺失~~ **方块/掉落同步已由第 17 条 P21 `BroadcastHub` 取代**：
   BlockUpdate / Spawn / Data / Remove / TakeItem 会 fan-out 给视距内其它连接（Ack 只回操作者）。
   **仍缺**：挖掘动画 `Set Block Destroy Stage 0x05`（玩家实体互见已由 §5.30 P43 补齐）、
   大视距广播风暴限流、两人同屏真机复验。
10. **P18 剩余（生存完整版）**：~~窗口点击整理（ClickContainer 只忽略，整理留后续）~~ **已过时：P18.1 已实现
    PICKUP/SWAP/QUICK_MOVE（仅整箱 SHIFT 整理未做）**；
   工具等级/精准采集（掉落无视工具，TODO）；经验球（entity 50，已由 §5.26 实现）；
   创造/生存模式切换（全按即时破坏+开局石头）；背包键为用户名（离线 uuid 随机，
   在线模式落地后切回 uuid，见 inventory 文档）；实体 UUID 为 id 派生（非随机，
   客户端仅 warn，不影响）；掉落落地高度重算已实现（`ItemEntity.support` 记录支撑坐标，
   `tick_drops` 每 tick 检测悬空并重算 `rest_y`）。
11. **P18.2 掉落修复待真机测试**：本轮修了四处（见 §3 掉落/实体/背包）：
    - Spawn 带真实初速、删除每 50ms `TeleportEntitySync`(0x23) → 验证不再原地蹦跶、
      抛物线/落地与原版一致、拾取仍正常；
    - `spawn_drops` 删除 `drop_in_view` 门禁 + `drop_rest_y` 从被挖方块向下找支撑
      → 验证走远挖地表/树下/地下方块都稳定掉落且落在挖掘点附近；
    - `decide_dig` 的 START_DIG(0) 只 ack、仅 STOP(3) 破坏（旧代码把 ABORT(2) 当完成，松开即碎；已按 jar 枚举修正 1/2/3）
      → 验证原版破坏进度动画正常完成并触发 FINISH_DIG（脚本 e2e 已覆盖，真机未验）。
    脚本端到端已全部通过（含模拟原版逐步走远→START→FINISH 全流程）；**原版 26.3 真机复验待做**。
    另：走远会瞬时补发大量区块（radius=10 跨 23 区块约 1035 包/~20MB），掉落 Spawn 排在
    后面，若真机感觉“走远后没掉落”需先确认是延迟还是丢失；必要时给跨区块批次加节流。
    **已实现跨区块批次节流（`login_route.rs`）**：移动包只把目标中心记进
    `PendingViewMove`（走回原中心直接作废），80ms 窗口到期或累计 6 次跨越才一次性发
    SetCenterChunk + UnloadChunk + ChunkBatch；`ChunkView::plan` 只在 flush 时调用，
    add/remove 是「旧完整视距 → 新完整视距」的净差，`sent` 与实际发包严格一致，
    客户端始终持有完整 `(2r+1)²` 视距（满足 3×3 邻居要求）。挖掘/放置前强制 flush，
    避免旧中心误判并保证 Spawn/BlockUpdate 排在区块包之后；50ms 读超时兜底保证玩家
    停住后仍会 flush。新增单测 + e2e（连续跨 3 区块合并为一批），`cargo test --workspace`
    与 `cargo check --workspace --tests` 全绿；真机感知延迟仍待复验。
12. **P18.3 落地高度重算**：`ItemEntity` 新增 `support: Option<(x,y,z)>`（支撑方块坐标），
    `ItemEntities::find_unsupported` / `resume_fall` 纯逻辑 + `tick_drops` 接入世界查询；
    支撑被挖空则重算 `rest_y` 并恢复下落。单测 + e2e 覆盖（挖掉掉落物脚下泥土验证重算）。
    TODO(真机)：挖掉掉落物下方方块时观察是否平滑落到新支撑面。
13. **R1 多连接重复 tick（已修）**：多连接共享 `Arc<DropState>` 曾导致每连接都调
    `items.tick()` → N 连接 = N 倍速物理。现由 `serve()` 启动**唯一**独立 `std::thread`
    ticker（50ms，`DropState::start_global_tick`），`physics_tick()` 在同一把 items 写锁内
    做支撑重算 + `items.tick()` + `tick_count++`；各连接 `tick_drops` 只做拾取/视距同步，
    绝不 tick。`MutableWorld::set_if` CAS 让 `decide_dig` 原子化，两连接同挖一格只一份掉落。
    测试：`drop_state` 单测（tick_count/支撑）、`set_if` CAS 单测、并发挖掘单测、
    `two_connections_share_one_global_drop_physics_tick` 双连接 e2e（700ms 内 tick 增量 ≈14
    而非 28，拾取延迟不被加速）。剩余：ticker 线程无显式停止句柄（进程生命周期），
    仅配置 drops 不配 mutable_world 不启动（main/测试均同时配置）。
    另附待真机测试清单 T1–T13（初速/弹跳/拾取/START-FINISH/走远/树下/换列/合并/双开/
    视距回返）见本轮审查报告；脚本 e2e 只验服务端状态，不能替代真机观察。
14. **数据驱动 P0/P1（已完成，见 `docs/data-driven-plan.md`）**：`data/manifest.json` +
    `data/777/{packets,blocks,items,entities,properties}.json`（精简表）+ `data/777/full/` 原资产
    （来源/SHA 见 `SOURCE.md`，共 ~9.4MB）；`mc-registry/build.rs` 从 JSON 生成 1286 方块 /
    1658 物品 / 161 实体 / 全量包 ID 表，**替换 216 项连续假 ID**；29 个真机实测 ID
    构建期断言 + 6 个集成校验测试。新增 `item_id`/`entity_id`/`packet_id` API，旧 API 不变。
    P2/P3 已完成：`mc-world` 的 10 方块 / 9 物品 ID、`mc-protocol` 的全部包 ID 与
    `ITEM_ENTITY_TYPE_ID` 均改为编译期从生成表查得（各自 const `registry_id` / `packet_id`
    线性查找，缺失即编译期 panic，不静默回退）；`0x23` 常量改名
    `PLAY_ENTITY_POSITION_SYNC_S2C_ID`（兼容别名已删、全仓库引用已更新）；36 个抓包 bin
    迁至 `data/777/{vanilla_config,login_play.bin,player_position_sync.bin,update_tags.bin}`，
    `include_bytes!` 改走 workspace 相对路径。抓包 fixture 逐字节测试全部通过。
    真实 ID 现在只有一份数据源（`data/777/*.json`）；新增方块/物品只需改 JSON 并重编译。
    **P4 已完成（2026-11-25）**：升级流程已文档化到 `docs/data-driven.md`（下载+SHA 校验 →
    精简表取 `default_state_id` → 更新 manifest → `cargo build` 重跑 codegen → 手改 `mc-protocol`
    包体布局 → fixture 回归：LoginPlay/PositionSync 逐字节、UpdateTags 抓包解码比对），
    版本 ↔ protocol ↔ data_version ↔ 数据目录映射见 `VERSION.md`，
    `ARCHITECTURE.md` §4 已改为「已实现」现状。**待真机 T1–T13 仍未做**（清单索引见本文件 §5 第 13 条，
    原始审查报告未入仓库；脚本 e2e 只验服务端状态，不能替代客户端观察）。
15. **架构加固（真机不可测期间完成）**：
    - `data/manifest.json` 新增 `vanilla_config` 有序索引（32 个 registry bin + update_tags），
      `mc-protocol/build.rs` 编译期据此生成两个公开符号（`VANILLA_CONFIG_REGISTRY_BODIES_777` /
      `VANILLA_CONFIG_UPDATE_TAGS_BODY_777`，名字/类型/顺序不变），源码不再写死文件名与数量；
      条目数≠32 / 重名 / 路径穿越 / 文件缺失均编译期失败。`vanilla_blocks()` 的 grass_block
      历史假值 2 修正为真实 9（无调用方）。
    - `DropState::stop_global_tick()`：每代独立 `Arc<AtomicBool>` 停止标志（start/stop 共用锁），
      stop 后 ticker 至多再等一个节拍即退出且不多跑一次 tick；restart 后 `tick_count` 不重置、
      从原计数继续。生产 `serve()` 不调用停止，仅供生命周期管理/测试。单测验证 start→stop→restart。
    - R4/R7 补服务端可验证回归护栏（**非真机验证**）：`items.rs` 单测证明水平漂移跨列后
      `resume_fall` 的 support x/z 保持原列；`play_stream.rs` e2e 证明掉落物出视距收到 Remove、
      回返后收到**同 entity_id 的 Spawn**（不是只发位置包）。三处均注明 T1–T13 仍需人工确认。
    - 真机不可测期间未改发包行为；上述断言全部是服务端状态级/包流级验证。
16. **P19 Q 扔物（已合入 main，未真机）**：`play_block.rs` 新增
    `PLAYER_ACTION_DROP_ALL_ITEMS=4`/`DROP_ITEM=5`（与 jar ordinal 一致，单测断言）；
    `play_entity.rs` 新增 `CLICK_THROW=4`；`inventory.rs` 新增 `throw_held`/`throw_from_slot`
   （含单测：单扔/整扔/清空、button 语义、光标非空无操作、越界拒绝）；
    `login_route/{mod.rs,drop_sync.rs}` 新增 `spawn_thrown_items`/`handle_throw_action` + 两处接线
    （PlayerAction 4/5 恒回 Ack；ContainerClick THROW 全量同步 + 刷盘）。
    `play_stream.rs` 新增 3 个 e2e（单扔→整扔、空手只 ack、容器 THROW 含光标无操作）。
    遗留 TODO：原版真机复验（Q/背包 Q 手感、扔出物抛物线/落地/拾取、空手 Q 不断连）；
    创造模式/副手/死亡掉落未做。扔出方向已按视角偏置见第 21 条 P28（本条当时的“落在脚下”已过时）。
17. **P21 多人同服可见（已合入 main，未真机）**：新增全服共享 `BroadcastHub`
    （`crates/mc-network/src/broadcast.rs`，`conn_id → 发送端 + 位置/视距快照`）；
    A 的权威判定线程把 `BlockUpdate` / 掉落 `Spawn`+`Data` / `Remove` / `TakeItem`
    按原始包体投递给视距内其它连接的无界队列，各连接 `play_loop` 在 tick 间隙
    （50ms 读超时分支 + 包处理节拍）排空自己队列写入自己 socket。
    - 范围：方块挖/放 `BlockUpdate` + `Ack`（`Ack` 只回操作者本人，客户端序列预测
      是 per-connection 的，广播 `Ack` 会错位对端预测）；掉落 `Spawn`/`Data`/
      `Remove`/`TakeItem`；拾取后即时向视距内其它连接补 `TakeItem` + `Remove`
      （despawn/出视距仍走各连接 `tick_drops` 差集兜底）。
      不同步：挖掘动画 `Set Block Destroy Stage 0x05`；玩家实体互相可见
      （已有 `PlayerConnId` 实体但无位置广播，保持不动）。
    - 并发：`decide_dig` 的 `set_if` CAS 语义不变；锁粒度最小——单把
      `RwLock<HashMap>` 只做注册/注销/位置更新与接收集快照，广播先在读锁内拷贝
      发送端、丢锁后再 `try_send`（无界队列，永不阻塞判定线程）；`update_pos`
      只改本连接条目。断开时 `enter_play_with_options` 退出路径与 `Disconnect`
      同点 `unregister`（注册在 Join 序列完成后，保证接收方已持有完整视距），不留幽灵通道。
      视距用接收方 `last_pos` + `radius`（切比雪夫，与 `ChunkView` 同口径），
      每个移动包后更新，跨区块移动后广播集自然跟随，无需等节流 flush。
    - 接收方 `known` 集按对端逐个维护（`Spawn` 前 mark、`Remove` 后 unmark），
      避免接收方 tick 重复补发/重复清理；与接收方自发包的竞态窗口内最多产生
      无害重复（客户端忽略未知实体 Remove），e2e 断言均容忍重复包、但对 `Ack`
      严格零容忍。`hub=None`（旧入口/旧单测）行为与旧版逐字节一致。
    - 测试：`broadcast.rs` 单测（视距过滤/位置更新移动广播集/注销清通道）+
      `play_stream.rs` 新增 3 个 e2e（A 挖/放 B 收 `BlockUpdate` 且无 `Ack`；
      A 扔 B 收同 entity `Spawn`、A 拾取 B 收 `TakeItem`+`Remove`；
      断开一端 hub 条目清除、另一端照常挖放）。旧 20 项一个字未动，全绿。
    - 遗留 TODO：玩家实体互见已由 §5.30 P43 补齐；挖掘动画 0x05；
      大视距（radius=10）广播风暴限流/合并（当前每变更即时广播，高频挖放 +
      多人同视距时包量线性增长）；真机两人同屏复验。
18. **P22 种子种植（已合入 main，未真机）**：手持 `wheat_seeds`（物品 1065，
    `data/777/items.json`）对草方块/泥土**顶面**（face=1）右键 → 上方空气格落
    0 阶段小麦（blockstate **6867**，`data/777/blocks.json` 的 `wheat.default_state_id`；
    age N = 6867+N，`data/777/full/blocks.json` 的 wheat `states` 数组按 age 升序
    `id: 6867..=6874`，实现只从 JSON 取 base，偏移即数据顺序本身；单测断言
    base=6867 与逐 age 映射防漂移）→ 扣 1 种子 + 单槽同步 + 刷盘 → `BlockUpdate`
    经 P21 hub fan-out（种植路径复用 `apply_block_outcome`）。
    生长走**全服唯一 50ms ticker**（`DropState::physics_tick` 内 `tick_crops`，
    与掉落物理同节拍；连接侧 `tick_drops` 绝不碰作物，P18.1 R1 不重犯）：
    阶段 = `(now - planted) / 1200`（**固定 60s/阶段**，满 7 阶段约 7 分钟成熟），
    到阶段即 `MutableWorld::set`（override+dirty+刷盘）+ 缓存失效 + hub 广播
    `BlockUpdate`（`except=u64::MAX`，视距内全体都收）；方块已不是预期旧阶段
    （被挖/被改）则静默摘除。挖小麦走通用挖掘 + 掉落表新增两臂：
    成熟（age=7）掉小麦（1066）×1 + 种子 1~2（hash 最低位决定），未熟只掉 1 种子；
    拾取/背包链路复用 P18。挖掉时挖掘路径显式 `uproot_crop`，ticker 懒摘除是兜底；
    `serve` 在 world+drops+hub/cache 齐备时注入 hub/cache 并 `adopt_existing_crops`
    把存量小麦重新纳入跟踪（**从重启时刻重计时**，与 Anvil 的差距见下）。
    - **与原版差异（有意 MVP，后续再补，均已记 TODO）**：无需耕地（直接种在草/泥土上，
      无 farmland 方块、无锄头）；无水源/光照加速；固定时长生长（原版随机 tick）；
      ~~重启后存量作物重计时（不持久化 planted）~~ **已由 §5.27 P39 修正（redb 模式续算，
      legacy 模式仍重计时）**；小麦是非地形弱方块——`heights`
      （地形顶）不变、天空光起点不变，但 `motion_heights` 随麦顶 +1（与水/树叶同口径，
      高度图含麦顶，客户端渲染/高度一致）；`drop_rest_y` 把小麦当支撑（掉落物可停在麦顶）。
    - 测试：`drops` 单测（成熟/未熟掉落、两分支覆盖、种子不可放置）+ `lib` 单测
      （base/逐 age/钳制）+ `block_edit` 单测（顶面种下/motion+1/6 种拒绝）+
      `drop_state` 单测（ticker 推进/摘除/重启认领）+ `play_stream.rs` 新增 1 个 e2e
      （种下→BlockUpdate 6867→扣种子；真实生长 6868..=6874；挖成熟→掉小麦+种子→
      拾取入库；阶段间隔经 `set_stage_ticks(10)` 调小，生产 1200 不变）。
      旧 23 项一个字未动，全绿。遗留 TODO：原版真机复验（种植手感/生长可见/收割拾取）；
      水源/光照加速、作物被水破坏、生长跨重启精确计时（持久化 planted）；
       耕地+锄头+骨粉与挖土连带已由第 20 条 P27 补齐（本条当时 TODO 已过时）。
19. **P26 树苗长成橡树（已合入 main，未真机）**：`mc-world::sapling_oak_blocks`
    （trunk 在前、leaves 在后，复刻 `worldgen::write_tree` 冠形：top-2/top-1 去角 5×5、
    top 3×3、top+1 十字；干高固定 4 = 世界生成 4~6 的缩小版下限，57 格）+
    `DropState::tick_saplings`（全服唯一 50ms ticker 内，连接侧绝不推进）+
    `plant_sapling`/`uproot_sapling`/`sapling_count`/`set_sapling_ticks`/
    `adopt_existing_saplings`（`serve` 在 `adopt_existing_crops` 后调用）。
    种下经 `decide_plant` 同路径（P23 已有），落子成功即 `plant_sapling` 登记；
    到期（生产 `SAPLING_GROW_TICKS=7200` ≈ 6min，5~10min 区间内）树苗格恒变原木、
    其余 56 格只在空气时放（被占跳过，不覆盖玩家建筑），多格变更逐格
    `MutableWorld::set` + 受影响 chunk 失效 + hub 广播（`except=u64::MAX`，全体视距内都收）。
    树苗被挖/所在土被挖即 `uproot_sapling`（挖掘路径显式摘除 + ticker 懒摘除兜底，
    土壤检查是与小麦不同的新增条件）；生长跨重启与小麦同口径（`adopt_existing_saplings`
    从重启动时刻重计时，不持久化 planted）。
    - **跟踪结构二选一**：另起 `saplings: (x,y,z)→planted_tick` 同构 map，不复用 `crops`。
      作物是多阶段 age 推进（`planted/stage` + `stage_ticks`），树苗是单次到期替换为多格树
      （`planted` + `sapling_ticks`）；计时、落子（单格 vs 57 格广播）、摘除条件
      （树苗多查土壤）语义完全不同，共用一表只会把两种间隔/阶段耦合在一起。
    - **与原版差异（有意 MVP）**：固定时长生长（原版随机 tick + 生长阶段）；
      无光照/空间校验（原版要亮度≥9 + 上方净空/横向空间，夜间不长、遮挡不长，
      本轮只要土壤是草/泥土、目标格空气就放，被占只跳过该格）；
      无树叶 decay/苹果掉落（树叶只走通用挖掘掉落表）；无骨粉催熟
       （均为 P26 当时状态；P27 已补骨粉与 decay，见第 20 条）；
      生长 planted 已由 §5.27 P39 持久化；大树/白桦/变种仍不做（P37 已补其它树种材质，
      但仍是 P26 小树形）。
    - 测试：`lib` 单测（形状确定性/57 格/去角/十字/平移）+ `drop_state` 单测
      （到期长树/被占跳过/挖苗取消/挖土取消/重启认领）+ `play_actions.rs` 新增 1 个 e2e
      （种树苗→BlockUpdate 30→扣苗；调小 `set_sapling_ticks(10)` 后出现 log 140 +
      leaves 294；挖树干→BlockUpdate 0 + Ack + Spawn/Data 掉 log 163）。
      旧 25 项（play_stream 17 + play_actions 8）一个字未动，全绿。
      遗留 TODO：原版真机复验（种植手感/生长可见/砍树拾取）；光照/净空校验；
      大树/多变种；树叶 decay/苹果；生长跨重启精确计时。
20. **P27 种田闭环（已合入 main，未真机）**：锄头耕地 + 耕地加速 + 骨粉催熟 + 树叶 decay。
    - **获得途径（P40 已有 2×2 玩家合成，但锄头/骨粉/苹果无配方，仍走管理面板 `/api/give` 领取；
      本条当时的“无合成系统”已过时，见 §5.28；
      `resolve_item` 本就接受短名/全名，无需改 admin 代码；
      e2e `admin_give_farming_kit_hoe_bonemeal_apple` 覆盖）**：
      锄头（7 系任一：wooden/stone/copper/golden/iron/diamond/netherite_hoe，
      `is_hoe_item`）→ 耕地；骨粉（bone_meal 1215）→ 催熟；
      苹果（apple 1007）→ 仅 decay 掉落，不可领取种植。种子仍是草方块 1/8 掉落。
    - **耕地**：手持任意锄头对草/泥土顶面右键 → 被点格变耕地（`FARMLAND_BLOCK_ID=6875`，
      `data/777/blocks.json` 的 `farmland.default_state_id`，只用干燥默认态），
      `decide_till`（face=1/土壤/y/视距/距离与种植同口径，上方有作物也不拦）；
      不扣锄头（无耐久系统）。种/挖走 P21 广播三步（`set` + 缓存失效 + hub）。
      耕地挖掉落泥土×1（`drops_for_block` 新增臂，旧表不动）；耕地计入地形高度
      （`is_terrain` += farmland，耕后高度图/天空光不变）。
    - **耕地加速**：`decide_plant` 土壤检查改为 `is_crop_soil`（草/泥土/耕地）；
      `tick_crops` 每拍现查脚下土壤——仍是耕地 interval 减半（`stage_ticks/2`），
      否则普通速度（只前进不回退；耕地被挖/被换后自动回落）。树苗土壤检查同步放宽。
    - **骨粉**：手持骨粉右键小麦本身（任意 face）→ `decide_bonemeal` 按
      `bonemeal_advance`（确定性 1~2，同位点恒定）推进并钳制到成熟，扣 1，
      `apply_bonemeal_crop` 同步跟踪（ticker 从此继续，不跳变）；
      已熟 → Noop 重发 + Ack（不扣）；右键树苗 → `grow_sapling_now` 立即长成
      （与 ticker 到期同函数：土壤须草/泥土/耕地，P26 形状复用，扣 1；
      土壤不行则重发 + Ack、不扣）；点其它方块 → Noop。
    - **树叶 decay**：原木被挖时 `schedule_leaf_decay_around` 登记周围 3³ 格内树叶
      （`or_insert` 不续命）；`tick_leaves` 随全局 ticker 推进（连接侧绝不碰）：
      已非树叶/3 格内有原木 → 取消观察；登记满 `LEAF_DECAY_TICKS=100`（约 5s）
      → 设空气 + `leaf_decay_drops`（徒手表 + 独立 hash 1/200 苹果）生成实体
      + 缓存失效 + hub 广播（掉落靠各连接 `tick_drops` 视距补发自动同步，
      ticker 内无 socket）。自然树天然有支撑，无需全世界扫描（与 Anvil 的差距外，
      这是本轮最大简化，见下）。
    - **挖掘连带（P27 顺手补的 P22/P26 悬空洞）**：土壤（草/泥土/耕地）被挖，
      其上方小麦/树苗一起 pop（BlockChange 追加 + 掉落 + 摘除跟踪，广播一次下发）；
      原木被挖登记 decay 观察。之前版本挖土只摘跟踪、作物块悬空。
    - **与原版差异（有意 MVP，均记 TODO）**：固定时长生长（原版随机 tick）；
      无水源/光照/水分加速（耕地只有干燥一态，无踩回、无水合）；
      锄头/骨粉无合成配方（P40 配方表未含）、无掉落来源（管理面板发）；骨粉对草地无长草/长花效果；
      decay 是到原木直接距离 ≤3（原版树叶 distance 属性逐格传播 7 格）且延迟固定 5s；
      decay 苹果率 1/200（原版橡树叶一致量级）但徒手挖树叶不掉苹果（旧单测锁定）；
      ~~无饥饿/进食~~ **已由 P40 实现（§5.28）**；~~生长 planted 与 decay scheduled 均不持久化~~
      **已由 §5.27 P39 修正（redb 模式跨重启续算）**；树苗可种在耕地上（原版不行，P27 简化）。
    - 测试：`drops` 单测（ID/hoe 表/耕地掉泥/骨粉 1~2 双分支/decay 苹果臂存在且前缀=徒手表）+
      `lib` 单测（farmland=6875/土壤集合/半径 3）+ `mutable` 单测（耕后高度图不动）+
      `block_edit` 单测（耕地/骨粉小麦推进钳制/成熟 noop/树苗路由/拒绝臂）+
      `drop_state` 单测（耕地双倍速+换土回落不回退/骨粉同步/decay 孤叶消失+有支撑保留+
      掉落成实体）+ `play_actions.rs` 追加 3 个 e2e（耕种加速对照/骨粉催熟+催树/
      砍树干后孤叶 decay 广播+掉落）+ `admin_e2e.rs` 追加 1 个（give 三件套）。
      旧 25 项（play_stream 17 + play_actions 8）与旧 admin e2e 一字未动，全绿。
      遗留 TODO：原版真机复验（耕地手感/催熟手感/decay 可见性/苹果拾取）；
      踩耕地还原、水合加速、骨粉长草、decay 跨重启持久化。
21. **P28 Q 扔按视角飞（已合入 main，未真机）**：
    - **旋转包结论**：777 旋转包一直存在且包体已知——C2S `move_player_pos_rot`(0x1F) 与
      `move_player_rot`(0x20)，ID 来自 `data/777/packets.json` 生成表（`packet_id("serverbound.play",
      "move_player_pos_rot"/"move_player_rot")`，编译期断言 `0x1F`/`0x20`，
      见 `play_sync.rs::c2s_movement_ids_are_0x1e_to_0x21_not_keepalive` 单测；与
      `docs/packet-map-777.md` §4 的 minecraft.wiki Play serverbound 表 4 行一致：
      0x1E Pos / 0x1F PosRot / 0x20 Rot / 0x21 StatusOnly）。之前不是包未知，
      而是 `play_loop` 把 `Rot` 归入 `_` 忽略、只用 `Pos`/`PosRot` 更新 `last_pos`，
      yaw/pitch 从未存过——所以 P19 扔出物只能掉在脚下。本轮把该分支接上，
      无需抓包，不走"按最后移动方向"降级。
    - **改动**：`play_loop` 新增 `last_look: (f32, f32)`（PosRot/Rot 更新，Pos/Flags 保持，
      初始 `(0,0)` 平视 +Z；断线即丢，随连接生命期，无需清理）；`spawn_thrown_items`/
      `handle_throw_action` 加 `look` 参（两处接线：PlayerAction Q 与 ContainerClick THROW）。
      生成点 = 眼高（feet + `EYE_HEIGHT` 1.62）+ 朝向×0.15 格（P28 初版 0.5 格是从脸前
      半空"弹"出来，真机反馈手感不对；原版就是从眼位扔出，P28.2 收回头上）；
      初速 = 朝向×0.06 + 纵向基线 0.1（初版 0.25 上扬太陡，同原因降到 0.1）。
      拾取/合并/广播/数量语义全不动（`spawn_with_velocity` 新实体才带定向初速，
      被合并吸收的不改既有速度；空手 Q/光标非空 THROW 仍只回包不落子）。
      手扔拾取延迟 40 ticks（P28.1，原版 q-drop 口径；挖掘掉落仍 10）。
    - **与原版差异（有意）**：原版 Q 扔初速约 0.3（眼位前方、飞数格），本服取 0.06 是因为
      半径 1.5——0.3 会飞出自捡半径，导致旧 e2e
      `broadcast_thrown_drop_spawns_for_viewer_and_pickup_removes`（A 原地不动等拾取）
      失败。轻掷落点在面前 ~1 格，站立 2s（手扔延迟）后可自捡；方向性由 0.15 格前置 + 非零初速体现。
      TODO：自扔暂不可捡（对齐原版手感）后，可把 `THROW_SPEED` 恢复到 0.3 量级；
      空手 Q、背包开屏 Q（button 0/1、光标非空无操作）数量语义未动；真机复验扔手感/抛物线/落点。
    - 测试：`drop_sync` 单测（四正向 + 垂直 + 45° 边界、`look_dir` 单位长；平扔初速口径；
      400 ticks 物理仿真落点仍在自捡半径内）+ `items` 单测（显式初速保留/合并语义；
      手扔 40 拍延迟回归）+
      `play_actions.rs` 只追加 1 个 e2e（PosRot yaw=-90 → Spawn x+0.15/眼高、vel≈(0.06,0.1,0)）。
      旧 26 项（play_stream 17 + play_actions 9）一个字未动，全绿。

22. **圆石放置映射补齐**：`block_for_item` 与 `PLACEABLE_BLOCK_IDS` 双双漏了圆石，
    手持圆石右键只 ack（延续 P18.3 规则），客户端却预测落块+扣数，
    随后被服务端纠正——看起来放下一瞬间消失、数还少了（管理页默认发放即圆石，必现）。
    补 `ITEM_COBBLESTONE→COBBLESTONE_BLOCK_ID(14)` + 白名单 + 挖返还；
    回归 e2e `placed_cobblestone_stays_and_dig_returns_cobble`。
    现存可获得方块映射已审计齐全（不可放的煤/铁/木棍/小麦保持只 ack，符合原版）。

23. **Join 选中槽同步（S2C `set_held_slot` 0x6B）**：服务端 `selected` 随存档恢复，
    但客户端重进默认选中 0 格、之前从不同步——造成你看着选了圆石（1 格），
    服务端却从钻石格（4 格）扔，Q 扔错格/放错材料。现 Join 时 selected≠0 则发
    S2C 0x6B（VarInt 0..=8；ID 见 `data/777/packets.json` 生成表，
    C2S Short vs S2C VarInt 区别见 jar 注释；新背包 selected=0 不发包，零包序影响）。
    回归 e2e `rejoin_syncs_selected_held_slot`（切 3 格→断线→重进收到 0x6B=3）。
    真机待验：重进后选中与下线前一致。

24. **P33 多作物种植（已完成并合入，`p33-multi-crop`）**：新增 `mc-world/src/crops.rs`
    通用作物表 `CropSpec`（种子 item / 方块 base / max_age / 每阶段 tick / 掉落口径），
    ID 全部编译期从 `data/777` 查表（单测断言防漂移）。新增三种作物：
    **胡萝卜**（carrot 1378，12402–12409，8 阶段，成熟 1~2 + 1/4 再 +1）、
    **马铃薯**（potato 1379，12410–12417，8 阶段，成熟 1~2 + 1/100 毒马铃薯 1381）、
    **甜菜**（beetroot_seeds 1439，16792–16795，4 阶段，成熟 beetroot 1438 + 种子 1~2）。
    全部复用原链路：`decide_plant` → BlockUpdate → 全服唯一 ticker 生长 → 骨粉钳制 →
    挖土连带 pop → 掉落/广播/拾取；耕地减半加速同样生效。
    小麦/树苗/骨粉/树叶 decay/Q 扔/后台/经验球行为不变：`wheat_stage_id`/`wheat_age`
    与旧签名改为委托通用表，`BonemealAction::Wheat` 保留兼容旧测试。
    地狱疣当时未做（需 soul_sand，扩公共土壤集合会影响小麦/树苗）——**已由 P37 用独立
    `Soil::SoulSand` 解决，见下**。
    测试：347 个旧测试一字未改，新增 8 单测 + 3 e2e（`play_crops.rs`），共
    **358 passed / 0 failed**；`p27_bonemeal_advances_wheat_and_grows_sapling`
    在并行负载下偶发竞态失败（未改测试，隔离 8/8 通过），继续观察。

    后续 **P37 全作物 + 模块拆分（`p37-all-crops` / `66ed05f`）** 在此之上补齐：
    - 作物：地狱疣（soul_sand 独立土壤）、西瓜/南瓜梗（成熟邻格落真实瓜/南瓜）、
      甜浆果丛、火把花、猪笼草（上下两格）、可可豆（jungle_log 侧面 face 映射）、
      甘蔗/竹子/仙人掌（新增 Vertical 竖向生长）、其它 7 种树苗（各自真实 log/leaves）。
      新增 `mc-registry` 生成表 `BLOCK_STATES` / `block_state_id(name,index)` 以解析多属性
      blockstate（如可可 age×facing、竹子 age×leaves×stage），ID 全部查表。
    - 模块拆分：`mc-world/src/crops/{mod,growth,drops}.rs`（规格/生长模型/掉落）+
      `mc-network/src/login_route/crops.rs`（路由/种植/骨粉/挖掘副作用）；旧公开 API
      保留为薄 shim，358 个 P33 之前旧测试一字未改。
    - 与原版差异（有意 MVP）：固定 1200 拍/阶段；瓜每株只结一次且邻格任意可替换；
      火把花/猪笼草成熟只掉花物品；甜浆果幼丛也掉 1 颗；树苗统一 P26 小树形只换材质；
      decay 仍为原木距离 ≤3 简化。新增 5 个 e2e（`play_all_crops.rs`），合计见 §4。
25. **后台管理面板 Vue + Naive UI 重写 + 完全离线化（已完成并合入：`p34-admin-vue` / `53f1016`，
    `p35-admin-offline` / `48e14bd`）**：`crates/mc-server/src/admin_page.html` 从 208 行原生 JS
    改为 1901 行**单文件自包含** Vue 3.5.13 + Naive UI 2.41.0 构建产物（JS/CSS 全内联，
    559,059 B，sha256 `b2047a5b…b552`），仍 `include_str!` + `GET /`，**运行时零外网请求**。
    - 源码在 `admin-ui/`（Vite 5.4.21 + vite-plugin-singlefile 2.3.4；
      `vue` 别名指向 `vue.esm-bundler.js` 以编译字符串模板并与 Naive UI 共用实例）；
      `package-lock.json` 已提交，`npm ci && npm run build` 可复现同一产物；
      `node_modules/`、`dist/` 已忽略。构建脚本 `postbuild.mjs` 做离线断言并复制产物。
      注：产物中 Vue/Naive UI 的 XML 命名空间等纯字符串 URL 被转义为 `\/`，运行时取值不变，
      因此文件里没有任何 `http(s)://` 字面量。
    - 后端 `admin.rs` 800 行 / `admin_items.rs` 215 行零改动，7 个 API 与 3 个 admin_e2e
      一字未动；broadcast 仍 501 stub，页面只显示说明不发包。世界状态/存档、在线表/踢人、
      1658 物品筛选发放、46 槽背包、5s/2s 轮询全部保留。
    - 验证：`node admin-ui/scripts/check-offline.mjs` 通过（无外链/importmap/动态 import），
      Playwright 无头 Chromium 实测外部请求 0、控制台错误 0、7 个 API 均被调用；
      合并后 `cargo test --workspace` **347 passed / 0 failed**，`check_budget.sh` 绿。

26. **P31 经验球（已合入 main，`2563ebe`）**：`mc-world/src/xp.rs` 新增独立 `XpOrbEntities`
    池（与物品池分离、共用全局 entity_id；物理口径沿用 P18：10 tick 延迟、1.5 格拾取、
    重力 0.04/阻尼 0.98、6000 tick despawn、支撑重算），等级曲线用原版公式。
    `drops.rs` 确定性矿物 XP：煤矿 hash 0~2、铁矿 1，其它方块无 XP（铁矿 XP 是本服口径，
    原版需熔炼才给，代码已注释）。包 ID/实体 ID 全部查 `data/777` 生成表
    （`set_experience=105`/0x69、experience_orb=50）；`data/777/packets.json` 没有
    `spawn_experience_orb` 条目，故复用已验证的 `add_entity 0x01`（type=50/data=0），
    元数据 `SetEntityData 0x65` index 8 / VarInt(1) / 经验值，拾取 `TakeItemEntity 0x7F`
    count=1 + Remove + `SetExperience`（Float 进度 + VarInt 等级 + VarInt 总经验）。
    物理只在全服唯一 50ms ticker 推进（连接侧绝不 tick，P18.1 R1 不重犯），
    广播走 BroadcastHub（Ack 仍只回操作者）。XP 存进 `player-<user>-<seed>.dat`
    v2（尾部 +4B），v1 旧文件兼容读取、XP 缺省 0；拾取/断线即刷，Join 时 XP>0 补发 0x69。
    测试：新增 `xp.rs` 单测、XP 掉落表、`play_entity` 包往返、存档 roundtrip/v1 兼容、
    `play_stream` e2e（挖铁矿→Spawn→拾取→SetExperience→重进 XP 仍在）；
    `cargo test --workspace` **347 passed / 0 failed**（重复多轮全绿；agent 首次并行跑曾
    偶发一次 `p27_bonemeal_advances_wheat_and_grows_sapling` 失败，疑既有骨粉/ticker 竞态，
    后续多轮未复现，需继续观察）。
    未做：经验球合并、原版 7.25 格吸引、击杀/熔炼 XP、真机复验（客户端渲染大小/拾取动画）。

27. **P36 新世界存储格式：方案 + 独立原型（已合入 `f24a0bb`，尚未接生产路径）**：
    用户明确指示**不做 Anvil 兼容/导出**，直接研究新格式。设计见 `docs/world-storage-plan.md`：
    选**纯 Rust `redb`**（MVCC/事务/零拷贝，单写多读），每 seed 一个
    `world/state-<seed>.redb` 作为磁盘真相源；保留 `MutableWorld` 稀疏内存热层，
    worldgen 确定性使地形不入盘，只存编辑差异。Schema v1：`meta`（schema/seed/
    journal_seq/tick_offset/generator）、`blocks`（12B 坐标→2B blockstate）、
    `journal`（u64 seq→33B 操作）、`players`（46 槽+selected+state_id+XP）、
    `timers`（作物/树苗/树叶增长计时，计划补 §5.18–20 的跨重启续算缺口）。
    同一 redb 事务写多表 → 崩溃安全；迁移一次性读 `P17W`/`P18I` 写入 redb，
    旧档改名 `.bak` 保留回退；分阶段：原型 → 生产替换（带 legacy/redb 开关）
    → 计时持久化 → journal 裁剪 → 热层优化。明确不做的：Anvil 读写/导出、
    LSM 列式等（文档有评分对比）。
    原型 `crates/mc-world/src/store.rs` **不接生产路径**（不改 main/persist/协议，
    旧存档格式不变），闭环覆盖开关库、方块编辑、玩家背包+XP、journal 重放、
    旧档迁移、schema/seed 守卫；7 个新单测，release 10k 编辑提交 73.6ms / 重放 1.37ms。
    依赖 `redb = "=2.6.4"`（仓库忽略 `Cargo.lock`，故锁确切版本防上游漂移）。
    当前 `cargo test --workspace` **365 passed / 0 failed**。
    **后续 P38 生产核心已合入**：`world_storage = "redb" | "legacy"`（**默认 redb**，
    `server.toml` 显式写入、旧 toml 缺键自动补；`ServerConfig` 结构体未加字段以免破坏
    穷举字面量的旧测试）；`main.rs` 启动按 seed 打开/迁移 `state-<seed>.redb`，
    `MutableWorld::with_store` 注入 blocks 热层，编辑在内存排队，
    `autosave_system`（60s/断线/退出）用**一个 redb 写事务**刷 `blocks` + `journal` 并清 dirty，
    失败保留重试；`DropState::with_store` 把背包/XP 读写与 `save_all_inventories` 切到
    `players` 表 + journal，旧构造函数 `new()` 仍是 legacy 以兼容全部旧测试。
    迁移仅在库不存在时执行：读 `P17W`/`P18I`（含 v1 XP=0）→ 事务写入 → 写
    `migration-<seed>.json` → 旧档改名 `.bak`；失败回滚，库已存在时旧档只告警不读。
    协议包与权威判定零变化。真机进程冒烟见 §4。
    **P39 增长计时持久化已合入**：redb 模式登记时把 `kind+pos+planted_tick+stage+spec`
    写入 `timers`，同一事务更新 `meta.tick_offset`；重启 `tick_count = tick_offset`，
    `now - planted_tick` 按原进度继续。P33 作物 / P37 全部生长模型（Age/Tree/Vertical/
    Side/Fruit）、树苗、树叶观察都恢复；阶段推进/骨粉/挖除/到期分别写或删 timer
    （新增 `delete_timer` + `TimerClear` journal，`set_timers` 批量写），IO 在释放内存锁后做。
    legacy（`store=None`）仍从 0 计时、行为逐字节不变。新增 `drop_state/timer_io.rs`
    与 `timer_spec.rs`（spec 字节 = 来源表 tag + 索引，bit7 记 Fruit 已落果）。
    `timer_persist.rs` 6 个真实 redb + ticker 集成测试，合计见 §4。
    **仍待**：journal 裁剪/运维、net 调用点逐连接 `actor` 标注（已有 `set_with_context`
    API，当前 actor=0）、每个 tick 都写 tick_offset（目前仅登记/推进/60s 刷新）。

28. **P40 2×2 合成 + 生存基础（已合入）**：`mc-world/src/crafting.rs` 无序精确匹配配方表，
    物品 ID 全部编译期查 `data/777/items.json` 生成表：原木→4 木板、2 木板→4 木棍、
    4 木板→工作台、3 木板+2 木棍→木镐、3 圆石+2 木棍→石镐、3 小麦→面包、
    煤+木棍→4 火把。**P41 已把配方扩充到 283 条（原 7 条逐字节保留 + 276 条），见 §5.30。**
    背包槽 1–4 为合成格、0 为结果槽（服务端权威判定 + 扣料 + 全量
    Content 同步），9–45 的 PICKUP/SWAP/QUICK_MOVE/THROW 语义不变；合成格随背包/redb
    持久化。`mc-world/src/survival.rs` + `drop_state/survival.rs`：per-conn
    health/food/saturation（初始 20/20/5.0），饱和优先衰减、饥饿归零周期扣血、
    落差 >3 格每格 1 点坠落伤害，**全部只在全服唯一 50ms ticker 推进**（连接侧只判定/发包）；
    死亡 = 背包全槽按现有掉落路径生成 ItemEntity 并广播 → 清空背包 → 满状态复活 →
    `PlayerPositionSync 0x49` + SetHealth + 全量 Content 回出生点。手持食物右键扣 1 补饥饿。
    协议：S2C `set_health` ID 查生成表为 106（0x6A），包体 Float+VarInt+Float 按 wiki
    已验证口径；Join 新玩家用客户端默认满值故不发包（保持旧包序），读盘非满值必发。
    存档：`.dat` v3 = v2 + 12B 生存字段；redb `players` v2 = v1 + 生存字段，v1/v2 旧档可读。
    **未做**：3×3 工作台/block entity；CB Respawn 0x54（包体需 dimension_type/hashed seed
    等十余字段且无抓包验证，不猜字节，死亡界面留后续）；锄头/骨粉/苹果无配方。
    新增 21 项测试（含 `play_survival.rs` 5 个 e2e），合计见 §4。

29. **P42 地图生成器调研（已完成，仅文档）**：`docs/worldgen-plan.md`（346 行）盘点现状
    （3 层 value noise 缓丘、4×4×4 方块洞穴、每 chunk 单 biome、oak 单树种、静态水、
    coal/iron 均匀矿、天空光从最低地表向上全 15 级），并给出改进方案与路线图。
    核心约束：**不能原地改变同一 seed 的基底**——`overrides` / `state-<seed>.redb`
    只存差异，基底一变旧编辑语义就漂移；建议生成器带版本号（`meta.generator` 已预留），
    v1 冻结、v2 仅新 seed 启用。ChunkData 0x2E 的多 biome 需新增 64 项 biome 容器编码器，
    非均匀光照需改 light helper；缓存的是已编码包体，基底变更必须全失效。
    路线：**P1 低风险视觉**（矿种/高度带、地表材质过渡、多树种与草花）→
    **P2 结构性地貌**（侵蚀/山脊/河流、细洞穴、4×4 多 biome、逐列天空光）→
    **P3 流体与结构**（流动水、block light、单 chunk 小遗迹）。
    性能红线：441 块无压缩 Join ≤700ms（基线 496–558ms）、包体 ≤60KB、
    默认 seed 出生 y=73 / 3×3 干燥回归全绿；洞穴禁止逐方块 3D noise（会放大约 30×）。
    待用户决策：v1 冻结 vs 原地升级、是否允许改 `mc-protocol` 编码 helper、
    静态水 vs 流动水、结构范围、出生圈外是否强制水景、性能红线 700/600ms、
    是否先抓 vanilla ChunkData 再进 P2。本轮未改任何 Rust 代码/协议，未跑 cargo。
    **用户决策（2026-11-26）**：v1 冻结 + v2 仅新 seed；允许改 `mc-protocol`
    增加 biome 容器/非均匀 light helper；水体做**流动水 + 游泳**；结构做**单 chunk 小遗迹**。
    其余（出生圈外是否强制水景、性能红线 700/600ms、是否先抓 vanilla ChunkData）
    按文档默认值执行。
    **P1 已实现并合入（`p44-worldgen-p1`）**：`GeneratorVersion::{V1,V2}` + 独立 toml 键
    `generator_version`（默认 1，老 seed/20 个旧 worldgen 测试逐字节不变、golden FNV 锁定 4 块）；
    v2 仅显式开启，redb `meta.generator` 追加 `|worldgen=<1|2>` 启动校验（不符直接退出），
    `ChunkCache::set_version` 全量失效。v2 内容：新矿高度带（copper/gold/redstone/diamond/
    emerald/lapis）、地表过渡（gravel/sandstone/clay/coarse_dirt/podzol/snow/ice）、
    多树种（oak/birch/spruce/dark_oak，复用 P37 真实 ID）、草花与水岸植被；
    拆 `worldgen/{ores,features}.rs`，`mutable` 白名单同步。高度场/biome 容器/光照/协议
    包体均零改动。新增 16 个测试（旧 440 个一字未改），合计见 §4。
    **P2 已实现并合入（`p45-worldgen-p2`，仅 v2）**：`worldgen/{terrain,caves,biome,light}.rs`
    - 地形：高频侵蚀（`ridge²` 山脊抬升 y>68）+ 河流下切至 `SEA_LEVEL-2`，出生半径 20 内
      权重为 0（覆盖出生 chunk，半径 16 河道权重 0），默认 seed 出生 y=73/3×3 干燥保持 v1。
    - 洞穴：4×4×4 格点（约 1000 次 3D noise/块）三线性插值到 2×2×2 子格 + 2D 噪声带隧道；
      逐方块无 3D noise；8 格壳/stone-only/MIN_Y+9 下限不变。
    - 矿脉/deepslate：8×8×8 粗格 hash 门控 + 矿脉高值区概率翻倍；`y<-16` 深板岩、
      `y<=-62` 深板岩矿（真实 ID 查表）。
    - 多 biome：每 section 一个 **64 项（4×4×4）biome 容器**，16 个水平子格按
      温度/湿度/高度/河流场取 river/birch/snowy/meadow/taiga/deep_dark 等，出生圈锁 forest/plains。
    - 逐列天空光：按列最高不透明块起点，树叶/水各 -2，仅非零 section 发 2048B nibble。
    - `mc-protocol` 新增 `encode_biome_paletted_container`（BPE 0 单值 / 1–3 间接 / 7 direct）
      与 `encode_nonuniform_sky_light_data` helper，线格式不变；布局按 minecraft.wiki 26.3
      已验证口径 + 逐字节回归测试，**本环境无原版客户端、未抓包，需真机复验**。
    - 隔离：v1 仍走原 `network_bytes()`/`light_bytes()` 逐字节不变，old golden FNV 与
      456 个旧测试一字未改；合并后 471 passed。
    - **性能风险**：本地 debug rayon 估算 441 块 v1 685ms / v2 **1060ms**（约 1.55×），
      包体 54KB/块；真机红线为无压缩 ≤700ms，v2 是否达标待真机复测（v1 仍是默认，不受影响）。
    **P3 已实现并合入（`p46-worldgen-p3`，仅 v2）**：
    - 流动水：`minecraft:water` 16 个 state（89..104）全部经 `state_index_id` 查表；
      level0=源、1..8=流水，只向 air 扩散每格 +1、最大 8 格，失 inflow 退 air；
      **全服唯一 50ms ticker**（`fluid_sync.rs`，`serve()` 启动一次，V1 不启动），
      每拍 ≤64 任务/≤48 落子，写 `MutableWorld` + cache 失效 + `BlockUpdate 0x08`
      经 hub 广播（Ack 仍只回操作者）。无压力均衡/双源造源/跨 chunk 水流。
    - 游泳：查脚/脚上/眼格判水中，豁免坠落伤害，脚陷入支撑时用
      `entity_position_sync 0x23` 只给本人做最小修正；水中位置经 P43 互见广播。
      无原版游泳动画/浮力手感/水下视野。
    - block light：torch/wall_torch=14、soul_torch=10、lantern/sea_lantern/glowstone/
      lava=15（真实 ID 与亮度查表），chunk 内 BFS 每格 -1；`mc-protocol` 新增
      `encode_light_data` 合并 sky/block mask+数组（wiki 26.3 布局 + 逐字节测试，
      **未抓包需真机复验**）。玩家编辑后不重算光照（沿用老缺口）。
    - 遗迹：每 chunk 6% hash，5×5 足迹全在单 chunk、25 列干燥落差≤1、避出生圈，
      石砖地基+环墙+柱+灯笼；无 jigsaw/loot/block entity，纯函数适配 rayon/缓存。
    - 隔离：全部仅 V2；V1 golden FNV 与 471 个旧测试一字未改；合并后 490 passed。
    - 真机待验：挖墙灌水/堵源退水、重连状态、双连接 BlockUpdate/Ack、block light
      接缝、游泳手感、441 块 Join ≤700ms（P2 本地 v2 估算 1060ms 已超红线，需实测）。

30. **P41 全配方 + P43 玩家实体互见（均已合入）**：
    - **P41 配方补齐**：`crafting.rs` 从 7 条扩到 **283 条**（原 7 条逐字节保留）；
      144 木材类（10 树种原木/剥离原木→木板、木板→木棍/工作台/箱子/台阶/楼梯/门/
      活板门/告示牌/栅栏/栅栏门/船/压力板/按钮、桶）、35 工具（7 系 × 镐斧铲锄剑）、
      16 盔甲、48 羊毛/床/地毯、40 杂项。ID 全部编译期查表。44 组输入多重集相同的
      冲突以**槽分布指纹**（按 (id,count) 排序的占用槽列表）确定性拆分，未命中时按
      表序回退，不用随机/哈希序。未覆盖：熔炼/锻造专属产物（锭、木炭、玻璃、
      下界合金盔甲）、需 >4 种输入或工作台专属 3×3 形状的配方。`play_recipes.rs`
      2 个 e2e（铁镐、箱子+床）。
    - **P43 玩家实体互见**：`player_visibility.rs` 独立全局 id 分配器（从
      1_000_000_000 起，自己仍用 LoginPlay 的 1，不与掉落 1000+ 冲突）；BroadcastHub
      存 `PlayerEntityState` 与**独立的 per-viewer 玩家 known 集**（不复用 item/orb known）。
      Join 双向 Spawn（`add_entity 0x01`，type=`minecraft:player`=159，data=0）；
      移动 C2S 0x1E/0x1F→`move_entity_pos(54)`/`move_entity_pos_rot(55)`、
      0x20/0x21→`move_entity_rot(57)`，超 i16 差量回退 `entity_position_sync(35)`；
      50ms 节拍视距进出补发/移除，断线先广播 `RemoveEntities 0x4E` 再 unregister。
      Ack 只回操作者本人。功能为 opt-in（`with_player_visibility(true)`，生产入口已开）。
      **未做**：`player_info_update 0x47` 名字/tab/皮肤（bitmask 无抓包不猜字节）、
      挖掘动画 `0x05`。ID/类型查生成表，布局按 wiki 26.3 已验证口径，记录于
      `docs/packet-map-777.md` §8。新增双连接 e2e（互见 Spawn / 移动 / Remove / 零 Ack）。
    合计见 §4。

31. **P47 剩余玩法 + 后台联动（已合入，`p47-gameplay-admin`）**：
    - **工具等级 / 时运 / 精准采集**：`mc-world/src/tools.rs` 6 档镐（木/石/铜/铁/钻石/
      下界合金）与矿/方块门槛表，ID 全部查生成表；时运 = seed+坐标 hash 额外 0~2 个，
      精准采集按 blockstate 注册名找同名物品（无则回退旧掉落）；档位不够仍破坏但零掉落。
      不做耐久/挖掘速度/附魔 NBT（用连接级服务端开关）。`decide_dig_with_tool` 由
      `NetConfig.tool_rules` opt-in（默认关保旧包流，生产入口开）。
    - **3×3 工作台 + 整箱整理**：`login_route/craft9.rs` 复用 283 条配方引擎，
      右键放置 crafting_table 开连接级会话（0 结果 / 1–9 格 / 10–45 背包），
      SHIFT 结果槽从背包补料连续合成；会话仅内存无 block entity 存档；
      `open_screen` 因 26.3 无 menu type 生成表、不猜字节而未发。
    - **游戏模式**：hub per-conn survival（默认）/creative；创造 START 即破坏零掉落、
      放置不扣手持。飞行未做（Player Abilities 0x41 包体未验证），
      `Set Creative Mode Slot 0x39` 仍不解析。
    - **挖掘动画 0x05**：ID 查生成表、包体按 wiki 26.3（VarInt 实体 id + packed i64
      BlockPos + u8 stage 0..9 / 255 取消）；START 发 0、STOP/创造破坏发 255 给视距内连接，
      操作者只收 Ack；由 `NetConfig.destroy_stage` opt-in（生产入口开）。
    - **后台联动**：新增 `GET /api/tools`（档位/可挖表）、`POST /api/tool-enchant`
      （none|fortune|silk_touch）、`GET /api/recipes`（283 条）、`GET /api/craft9`（会话快照）、
      `POST /api/gamemode`；`/api/online` 增 `gamemode`/`tool_enchant` 字段（旧字段保留）。
      Vue `admin-ui/src/Dashboard.js` 加工具等级/配方浏览/工作台会话卡片与在线模式下拉，
      `npm ci && npm run build` 重建离线单文件 `admin_page.html`，
      `check-offline.mjs` 零外链通过；旧 3 个 admin_e2e 未改，新增 1 个。
    - **附带修复**：`drop_state.rs` 2060 行触发 2000 行软告警，把测试模块拆到
      `drop_state/tests.rs`（旧测试一字未改），budget 恢复零 WARN。
    - 旧 490 个测试一字不改；合并后 **509 passed / 0 failed**，budget 与离线检查绿。
    - 未做：耐久/挖掘速度、飞行、创造槽包、真实 open_screen GUI、工作台存档、
      完整 Respawn 0x54、名字/皮肤 0x47、聊天广播（p32 blocked）、生物 AI/战斗/命令/声音。

32. **真机复验清单（2026-11-27）**：`docs/real-machine-checklist.md` 按 12 组列出人工验证步骤
    （通用准备 / 登录视距压缩 / v1 地形 / 方块交互 / 掉落背包 Q 扔 / 多人 /
    农业 / 经验球 / 后台 / 生存合成工具 / redb 存档 / v2 世界生成），每项含配置、操作、
    期望日志与包、截图点；末尾附失败定位速查表与记录表。**真机验证由用户在 Windows 客户端执行**，
    仓库内脚本 e2e 不能替代。重点风险：v2 441 块 Join 本地估算 1060ms 超 700ms 红线、
    v2 biome 容器/非均匀 light 无 vanilla 抓包对照、P47 `open_screen`/飞行/0x47/0x54 未做。

33. **P49 剩余玩法缺口调研（已完成，仅文档）**：`docs/gameplay-gaps-plan.md` 把剩余 28 项缺口
    分 A 类（无需抓包：经验球合并/吸引、饥饿细节、广播限流、后台设置持久化、journal 运维、
    时间/天气、统计计数、工作台会话落盘）、B 类（必须抓 777 包：聊天 `0x09`/`0x42`/`0x7c`、
    `player_info_update 0x47`、`Respawn 0x54`、飞行 `0x41`、创造槽 `0x39`、声音/粒子）、
    C 类（需 socket→ECS 大改：AI/红石/载具/命令）。每项含包 ID 来源、状态归属、
    tick 归属（必须全服唯一 ticker）、存档位置与**具体后台 API/面板联动点**。
    推荐：批次 0 = 真机复验门禁（不写代码）→ 批次 1 = A 类 8 项 → 批次 2 = 社交三件套
    → 批次 3 = 创造/容器/反馈 → 批次 4 = 大架构评估。明确不建议做与 12 个待决策点见文档 §5。
    **用户决策（2026-11-27）**：下一批先做**批次 4 socket→ECS 立项评估**（`p50-socket-ecs`，
    仅调研设计）；`p32` 聊天保持 blocked；v2 真机若超 700ms **接受较慢**、不强制回退 v1
    （v1 仍是默认、v2 仍 opt-in）；身份方案选 **UUID 稳定化 + 玩家存档/后台迁移**
    （`p51-uuid-identity`，排队待派发，会动 `player-<user>-<seed>.dat` 键与后台按用户名查找）。

34. **P50 socket→ECS 立项评估（已完成，仅文档）**：`docs/socket-ecs-plan.md`（449 行）推荐
    **方案 C（混合）**：net 线程只保留 accept + 帧编解码 + 真实 socket 写，ECS 拥有 play loop
    权威判定与「每个玩家有序写队列的唯一生产者」；把“判定必须与 socket 写同线程”精确化为
    “必须与 Ack 同序、且无第三方插队”，不把 441 块 Join 编码塞进 Bevy 50ms Update，
    且不恢复 P16.1 删除的全量区块生成脚手架（继续 `ChunkCache::bodies` + `ChunkView`）。
    灰度 `socket_mode=net|ecs`（默认 net、重启切换、按进程锁定），阶段：
    P0 只读观测（`/api/world.socket_mode`）→ P1 Outbound 通道抽取（纯重构）→ P2 移动/心跳
    ECS 化 → P3 方块判定 ECS 化 → P4 视距/掉落 ECS 化 → P5 全量切换与清理；
    每阶段独立提交/回退/验收（含 `play_stream.rs` 逐字节基线与真机清单 §1/§3/§5/§8）。
    **最大风险**：包序与 Ack 归属（`BlockUpdate → Ack(seq)` 必须同玩家流相邻、Ack 绝不进 hub；
    Join 八段包序 + 视距 flush 先于方块/掉落包），判定移到 ECS 最多多 1 个 50ms tick 的 Ack 延迟。
    待用户决策：批准 C1 立项、选 C 还是 B、灰度/背压策略、真机门禁、先做 P0+P1 还是直推 P5、
    C2/C3/C4/B15 排期与抓包归属。
    **用户决策（2026-11-27）**：**批准 C1 立项，选方案 B（socket I/O 全移 ECS）**，
    **直接 P5 全量切换**（不经过 P0–P4 中间阶段），背压采用**无限队列**；
    `p51-uuid-identity` 等 socket→ECS 完成后再派发（不并行）。实现任务见 `p52-socket-ecs-b`。
    已明确告知用户：跳过灰度阶段、全量切换与无限队列是本轮最高风险项，必须靠
    `play_stream.rs` 逐字节基线与真机清单兜底。
    **P52 实现已合入（`p52-socket-ecs-b` / `72436e0`）**：`mc-network/src/socket_ecs.rs`
    实现 `SocketEcsRuntime`；`serve()` 在 ecs 模式只 accept + 分配 conn_id 并交接，
    Bevy `Update` 的非阻塞 `socket_ecs_system` 只 `try_recv + register`；每连接三个
    ECS 持有的任务：reader（字节→Inbound）、writer（**唯一写者**，FIFO 排空 Outbound→socket）、
    logic（复用既有登录/配置/Play 权威路径，是本人 Outbound 的唯一生产者）。
    tokio runtime 由 `main.rs` 创建并交给 ECS 资源；`socket_mode` 启动期锁定，
    生产默认 `ecs`、`net` 为紧急回退；`/api/world` 只读暴露 `socket_mode`。
    三条红线：Join 八段与全部 ChunkData 逐字节不变（新 e2e 断言 ECS/net 两模式相等）、
    `BlockUpdate→Ack(seq)` 同玩家流相邻且 Ack 绝不入 hub、视距 flush 先于方块/掉落包；
    全服唯一 50ms 物理 ticker 仍在 `serve()` 启动一次，未恢复 P16.1 脚手架。
    按用户选择用**无限写队列**（不丢装饰包、不踢慢客户端），风险是慢客户端无界增长/OOM；
    监控点 `stats()`（accepted/registered/retired/queued_bytes_total）与 `active_connections()`，
    见 `docs/socket-ecs-p52.md`。旧 509 测试一字未改；合并后 516 passed / 0 failed。
    进程冒烟（临时端口 + `socket_mode` 缺键自动补 ecs）：日志 `socket_mode=ecs → redb world
    opened → listening → global drop physics ticker started → keepalive`，无 panic。
    **真机待验**：清单 §1/§3/§5/§8 的 ECS vs net 对比、慢连接 RSS、tick lag=0。

## 6. 关键文件

```
crates/mc-protocol/src/packets/
  config.rs                  Configuration 包编解码/断言
  vanilla_config_777.rs      32 个原版注册表包体 + Update Tags 常量
  data/vanilla_config_777/   抓包二进制（33 个文件，约 216KB）
  block_tags_777.rs          300 个 block tag / 14825 项的回归数据
  play_chunk.rs              SetCenter/ChunkData/GameEvent/Batch/Unload（0x26 packed i64）
  play_block.rs              挖掘/放置 C2S（0x29/0x42/0x43/0x2E）+ BlockUpdate 0x08/Ack 0x04 + Position pack/unpack + P18 ClickContainer 0x12 常量 + P19 DROP_ALL/ITEM=4/5
  play_entity.rs             P18 掉落/背包包：Spawn 0x01/SetData 0x65/Remove 0x4E/Teleport 0x23/PosDelta 0x36/TakeItem 0x7F/Content 0x12/SlotPkt 0x14 + Slot/LpVec3/元数据编解码 + C2S SetHeld Short 解析 + P19 CLICK_THROW=4 + P28.3 S2C set_held_slot 0x6B
  play_login.rs              LoginPlay 777 布局 + 抓包往返测试
  play_sync.rs               PlayerPositionSync/KeepAlive/C2S 移动包
  data/*_777.bin             LoginPlay / PositionSync / UpdateTags 抓包
crates/mc-network/src/
  login_route/               Play 接线目录：login.rs（登录/Join序列）+ mod.rs（play_loop）+ block_edit.rs（decide_dig/decide_place + BlockUpdate/Ack）+ crops.rs（P37 作物路由）+ craft9.rs（P47 3×3 工作台会话）+ drop_sync.rs（掉落/50ms tick/拾取/背包同步 + P19 Q扔）+ view_sync.rs（视距节流/flush）
  socket_ecs.rs              P52 ECS socket 运行时（reader/writer/logic 三任务 + 无限 Outbound + stats）
  broadcast.rs               P21 per-player 写通道 hub（conn_id → 队列 + 位置/视距快照，BlockUpdate/Spawn/Data/Remove/TakeItem 视距广播）+ P43 玩家实体状态/known
  player_visibility.rs       P43 对外玩家 id 分配、Join/移动/视距/断线广播
  fluid_sync.rs              P46 v2 全服唯一流体 ticker + BlockUpdate 广播
  drop_state.rs / drop_state/{timer_io,survival}.rs / timer_spec.rs   DropState：ItemEntities + 背包 + 视距集 + 存档 + P39 timer IO + P40 生存 ticker
  login_route/survival_sync.rs SetHealth/位置同步/进食发包
  tests/timer_persist.rs      P39 真实 redb + ticker 跨重启续算集成测试
  tests/play_survival.rs      P40 合成/饥饿/死亡掉落/重生 e2e
  tests/play_recipes.rs       P41 新配方放料→结果→扣料→同步 e2e
  tests/player_visibility.rs  P43 双连接玩家互见 e2e
  tests/fluid.rs              P46 流体存档/广播/游泳互见 e2e
  tests/socket_ecs_e2e.rs     P52 ECS/net Join 逐字节、Ack 相邻无广播、断线清理 e2e
  chunk_view.rs              clamp_view_radius/view_chunk_count、per-connection 视距集合/去重/跨区块 add/remove 规划
  chunk_cache.rs             共享已编码 ChunkData 0x2E 包体缓存（Mutex+HashMap+FIFO，默认 1024；`bodies` rayon 并行批量；P17 `with_mutable` + `invalidate`）
  play_events.rs             net 线程 → ECS 的移动/KeepAlive/Disconnect/BlockEdit 事件（conn_id 实体键）
  config.rs                  NetConfig（视距/压缩阈值/seed/Play 事件通道/ChunkCache/MutableWorld/DropState）
  framing.rs                 VarInt 帧 + zlib 压缩帧 + 阈值边界
crates/mc-network/tests/play_stream.rs   移动事件/压缩配置/seed 透传/视距 2/4 初始块与跨区块批次/P17 挖放端到端/P18 挖→掉落→拾取→背包→放置扣减→存档端到端/P19 Q 扔（单扔/整扔/空手 ack/容器 THROW）集成测试
crates/mc-world/src/
  mutable.rs                 MutableWorld（基底 + 稀疏 override + dirty）+ 自有稀疏存档读/写 + 出生点抬升 + P18 support_top_y
  drops.rs                   P18 确定性掉落表 + item ID 常量 + 手持→方块映射（作物掉落委托 crops/drops.rs）
  crops/{mod,growth,drops}.rs P33/P37 通用作物规格表 + 各生长模型（Age/Tree/Vertical/Side/Fruit）+ 掉落规则
  items.rs                   P18 物品实体纯逻辑（生成/合并/重力/拾取/despawn/视距过滤）
  inventory.rs               P18 背包（46 槽/手持/合并/扣减）+ `player-<user>-<seed>.dat` 存档 + P19 throw_held/throw_from_slot
  tools.rs                   P47 工具档位/矿门槛/时运/精准采集纯逻辑
  crafting.rs                P40/P41/P47 2×2 283 条配方 + 3×3 `match_recipe_9`/`consume_grid9`
  survival.rs                P40 生存状态（health/food/saturation）与食物表纯逻辑
  store.rs                   P36/P38/P39 redb 存储（生产核心 + 计时）
  worldgen/
    mod.rs                   WorldGen + GeneratorVersion(v1 冻结/v2 显式)、高度/出生保护/biome/洞穴/树、跨 chunk 扫描
    ores.rs / features.rs    P44 v2 矿种高度带 / 地表材质·多树种·草花水岸植被
    terrain.rs / caves.rs / biome.rs / light.rs   P45 v2 侵蚀河流 / 细洞穴 / 4×4×4 多 biome / 逐列天空光
    ruins.rs                 P46 v2 单 chunk 确定性小遗迹
    fluid.rs                 P46 v2 水体 state/流动规则/游泳纯判定
    noise.rs                 2D/3D 平滑 value noise + 确定性 hash
    encode.rs                GeneratedChunk（sections/heights/motion_heights/biome_id）+ network/heightmap/light 编码
    tests.rs                 水/沙岸/树/biome/洞穴/矿/高度图/编码/4 seed 出生点测试
crates/mc-world/src/lib.rs             SPAWN_BLOCK_X/Z、方块 ID 常量、PLACEABLE allowlist、旧 Chunk/save 占位、palette/heightmap/light
crates/mc-server/src/
  config.rs                  ServerConfig.seed/compression_threshold/view_distance/world_dir + effective_view_distance()
  main.rs                    P18 DropState 创建 + NetConfig 注入（world_dir 同目录、seed 隔离）
  systems.rs                 NewConnection(addr,conn_id) → ECS 玩家实体（SharedWorld 出生点优先，重复 conn_id 幂等）
  movement.rs                PlayEvent 排空 + MovementQueue/Position/KeepAlive + PendingPlayerRemoval 唯一删除点（+BlockEdit 日志臂）
  play.rs                    ECS PlayScaffold：登录队列 + `last_view_radius/chunks` 诊断 + KeepAlive 计数（P16.1 无区块生成/编码；socket 未接入）
  persist.rs                 autosave_system（60s 周期/断线即刷；必须排在 player_removal 之前）+ SharedWorld/WorldSaveDir
  admin.rs / admin_items.rs / admin_page.html   管理面板后端（含 P47 tools/recipes/craft9/gamemode 端点）+ 离线 Vue 单文件产物（源码 admin-ui/src/Dashboard.js）
docs/packet-map-777.md                 777 包 ID 对照表（权威；§6 挖掘/放置 9 个 ID；§7 P18 掉落/背包 10 个 ID + 物品/实体 ID 表）
docs/registries-777.md                 注册表清单
docs/world-storage-plan.md             P36 新世界存储格式方案（redb，不做 Anvil 兼容）
docs/worldgen-plan.md                  P42 地图生成器调研与分阶段路线图（未改代码）
docs/real-machine-checklist.md         真机复验清单（按功能分组，含配置/按键/日志/截图/失败定位）
docs/gameplay-gaps-plan.md             P49 剩余玩法缺口调研（A/B/C 分类 + 后台联动 + 批次路线）
docs/socket-ecs-plan.md                P50 socket→ECS 立项评估（方案 C 混合 + 灰度 + P0–P5）
docs/socket-ecs-p52.md                 P52 方案 B 实际线程模型/风险/回退/真机待验
```

## 7. 测试命令

```bash
cargo check --workspace --tests
cargo test --workspace
bash scripts/check_budget.sh     # Windows 可用 C:\Program Files\Git\bin\bash.exe
```

## 8. Git 状态 / 提交注意

- 当前分支 `main` @ `bc86694`（P30 任务库 M1）；P19–P28 均已合入并 push（P19 `bd42e93`、P20 `5962e89`、P21 `bb898d5`、P22 `f10207b`、P25 `a07b79d`、P26 `4fbf7aa`、P27 `8161068`、P28 `535939b` + 补丁 `c97e34a`/`93620e8`/`adcc449`），历史提交见 `git log` / `tasks/events.jsonl`。
- 历史：P18 代码 `cf821df`（协议）/`9109cd9`（掉落表+实体+背包）/`34af0cd`（net 接线+e2e）、文档 `f693d73`（基线 `7a0dca7`）；P17 代码 `c5c57af`/`db8f666`/`1fc0a45`/`4de7bbe`、文档 `3744455`/`7a0dca7`（基线 `97c241b`）；P16.1 代码 `561cc7e`、文档 `fc91291`；P16 代码 `2d8c89d`、文档 `4b4cb08`/`b83a096`（基线 `ceec052`）；P15 代码 `7f44b20`、文档 `0df9b40`（基线 `12bdaa4`）；P14 `1e47e69`/`12bdaa4`（基线 `c3bd992`）；P13 `02d31ee`/`0aaf8ed`。
- P18 注意：`DropState` 进 `NetConfig`/`PlayOptions`（`handshake_route` 透传）；`decide_*` 仍在 net 线程（ECS 无写通道，实体/背包同步同理，见 §5.1）；50ms tick 是读超时驱动，不改变 10s 半开语义；背包键用用户名（离线 uuid 随机，见 §5.10）；临时 Java 实测工具/冒烟 ps1/测试世界只放 Temp、不提交；冒烟服 `C:\Users\qwe\AppData\Local\Temp\p18smoke` 不在仓库。
- P17 注意：`autosave_system` 必须排在 `player_removal_system` 之前（断线登记靠 `PendingPlayerRemoval` 非空触发，唯一删除点会 drain 它）；`decide_*` 在 net 线程做判定是刻意设计（ECS 无写通道），见 §5.1；脚本客户端 `live_edit.py` 只放 Temp、不提交。
- P16.1 注意：ECS `PlayScaffold` 已不再生成/编码区块；`ready to join` 每次登录 1 条，`chunk data (...)` 脚手架日志和 `tick lag` 已消失。未来 socket→ECS 时用 per-player 写通道 + `ChunkView`，不要恢复全量生成脚手架。
- P16 注意：`crates/mc-world/src/worldgen.rs` 已拆成 `worldgen/{mod,noise,encode,tests}.rs`；新增 `rayon` 到 `mc-network`（workspace 依赖，`Cargo.lock` 无版本变化）；临时 seed 搜索程序/截图均在 `logs/`/Temp，未提交。
- 不要提交：`scripts/check_budget.sh` 的纯行尾/文件 mode 变化、`Cargo.lock`（如无依赖变更）、`logs/`、`target/`、临时日志。
- `HANDOFF.md`、`MEMORY.md` 按仓库习惯更新。
- 旧的 `.gitignore` / `desk_log/conn.log` 删除是历史工作区噪声，已从 HEAD 恢复，不要误提交。
