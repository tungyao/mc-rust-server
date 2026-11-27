# socket→ECS 立项评估与设计（p50-socket-ecs / C1）

> 分支 `p50-socket-ecs`（基于 main `f97194d`），owner=`subagent-socket-ecs`，日期 2026-11-27。
>
> **本轮范围：立项评估 + 方案设计，不写生产代码、不改协议、不改前端、不跑 cargo。**
> 唯一交付物是本文件；不修改 `tasks/`、`HANDOFF.md`、`MEMORY.md`，不动任何 `.rs` / `admin-ui/`。
>
> 范围锚点：`docs/gameplay-gaps-plan.md` §3.3 C1、§4 批次 4（前置：批次 0–3 全绿 +
> 用户批准"新增全局实体系统 / 改 socket 归属 / 接受大版本改造"；C1 本身是灰度项）。
> 本文件**只聚焦 C1**，不重述 gameplay-gaps 的 A/B 类清单，也不重述 `docs/worldgen-plan.md`
> 的地形方案。
>
> 版本锁定：`VERSION.md` → MC 26.3 / protocol 777 / data_version 5023 / `data/777/`。
> 包 ID 全部来自 `data/777/packets.json` 生成表，本文只引用已验证 ID，不猜字节。
>
> 用户硬约束（继承 gameplay-gaps §0）：任何后续玩法实现必须与后台管理联动；
> 因此本文每个阶段都写明 `/api/world` 的只读 `socket_mode` 字段与面板显示。

---

## 0. 结论摘要（TL;DR）

1. **本质矛盾只有一条**：socket 写权在 net 线程，ECS 没有写通道；服务端权威判定
   （`decide_dig`/`decide_place`）必须与"给操作者写 `BlockUpdate` + `Ack`"严格同序，
   所以判定被有意留在 net 线程（HANDOFF §5.1），ECS 只通过共享 `Arc<MutableWorld>`
   读同一世界并刷盘。
2. **现有绕法已经是可工作的稳态**：`BroadcastHub` 把"给别人发包"变成 per-player 无界队列，
   `PlayEvent` 把"给 ECS 上报"变成单向 channel；两者都不改 socket 所有权，因此
   P21/P43/P47 的广播全部长在 hub 上，Play 包流逐字节稳定（`play_stream.rs` 19 项 e2e）。
3. **推荐方案 C（混合）**：net 线程只保留 accept + 帧编解码 + 真实 socket 写，
   ECS 拥有 play loop 的权威判定与"per-player 有序写队列的生产权"。
   它把"必须同线程"精确化为"必须是同一个玩家的有序队列的唯一生产者"，
   既解锁 C2/C3/C4/B15，又不必把 441 块 Join 编码塞进 Bevy 的 50ms Update。
4. **不恢复 P16.1 已删除的全量区块生成脚手架**：继续用 `ChunkCache::bodies`（rayon 并行
   生成/编码）和 `ChunkView`，ECS 侧不引入 `pending_chunks`/`CHUNKS_PER_TICK`。
5. **最大风险是包序与 Ack 归属，不是 I/O 性能**：Join 的 8 段固定包序、
   "视距 flush 先于方块/掉落包"、"Ack 只回操作者本人且不合并"三条一旦破坏，
   全部真机基线和 `play_stream.rs` 同时失效。
6. **C1 不是可选项的替代品**：若不批准，C2 生物 AI / C3 载具 / C4 红石 / B15 命令
   只能停在调研；A 类与大部分 B 类（不依赖实体/权威跨线程判定）可继续。

---

## 1. 现状与痛点

### 1.1 线程与所有权现状（代码锚点）

| 关注点 | 现状 | 代码位置 |
|---|---|---|
| accept / 连接身份 | `serve()` 单 accept 循环，`AtomicU64` 单调分配 `conn_id`，`tokio::spawn` 每连接一个任务 | `mc-network/src/lib.rs:103-114`（`next_conn_id` 见 `:50-55`） |
| 网络运行时 | 独立 tokio multi-thread runtime（`mc-net` 线程），与 Bevy 主线程完全分离 | `mc-server/src/net_thread.rs:6-43` |
| socket 写权 | **每连接 `play_loop` 独占 `&mut TcpStream`**；Join 序列、KeepAlive、动态区块、方块回包、掉包同步全在这里 `write_packet_any` | `login_route/login.rs:576-637`、`login_route/mod.rs:83-`（`play_loop`） |
| 广播写权 | 别人的包不直接写 socket，进 `BroadcastHub::HubConn.tx`（`crossbeam::unbounded`），本连接 `drain_outbox` 在 50ms 节拍排空 | `broadcast.rs:66-77,110-118,335-366`；`login_route/mod.rs:43-59,153` |
| ECS 侧 | 只有 `PlayScaffold` 诊断（`pending_login`/`joined`/`keepalive_seq`/`view_center`/`last_view_radius`/`last_view_chunks`），`keepalive_system` 只编码自检不发包 | `mc-server/src/play.rs:19-33,85-102` |
| ECS 收到的信息 | `PlayEvent::{Movement,KeepAlive,Disconnect,BlockEdit}`，均带 `{addr, conn_id}`；`BlockEdit` 是**判定后**的日志钩子 | `mc-network/src/play_events.rs`；`mc-server/src/movement.rs:151-205` |
| 权威状态 | `Arc<MutableWorld>`（override + dirty + `set_if` CAS）、`Arc<DropState>`（物品/背包/XP）、`Arc<ChunkCache>` 被 net 与 ECS 共享，ECS 只刷盘 | `mc-server/src/main.rs:101-140`、`persist.rs` |
| 全局物理 ticker | `serve()` 启动时**各一次** `DropState::start_global_tick`（50ms，独立 `std::thread`）、V2 的 `FluidState::start_global_tick` | `mc-network/src/lib.rs:71-102`；HANDOFF §5.13 R1 |
| 主循环 | Bevy `Update` 9 个 system `.chain()`，`mc_core::TICK_MS`(50) 一次 `app.update()`，超时打 `tick lag` | `mc-server/src/main.rs:164-182,201-227` |

### 1.2 为什么权威判定必须与 socket 写同线程

这不是性能选择，是**协议语义 + 竞态**共同决定的：

1. **Ack 是 per-connection 的客户端序列预测**。`Acknowledge Block Change 0x04` 携带 C2S 的
   sequence，客户端用它决定预测方块是"确认"还是"回滚"。仓库铁律（HANDOFF §5.17、
   gameplay-gaps §5.1）：**Ack 永远只回操作者本人，绝不广播、绝不合并**；广播给旁人会让
   旁人用自己的 sequence 解释这个数字，直接错位。
2. **BlockUpdate 与 Ack 必须在同一条流上严格相邻**。现在 `apply_block_outcome` 的顺序是
   `cache.invalidate` → `fluid.on_block_change` → `hub.broadcast(BlockUpdate)` →
   `write(BlockUpdate)` → `write(Ack(seq))`（`block_edit.rs:359-411`）。若判定在 A 线程、
   写在 B 线程，B 线程队列里可能已排入"另一个玩家的广播"或"视距 flush 的 Unload/Chunk"，
   Ack 就会被推迟到错误位置；客户端先收到别人的包再收到自己的 Ack，预测窗口错乱。
3. **判定依赖 socket 侧的即时位置快照**。`last_pos`/`last_look` 由本连接 C2S 移动包更新
   （`mod.rs:99-110`），超距/出视距回滚与 Q 扔方向都读它；把判定挪走就必须把这份状态
   也跨线程同步，否则判定用的是过期位置。
4. **判定前必须强制 flush 视距**。跨区块移动只记 `PendingViewMove`（80ms / 6 次合并），
   挖掘/放置前以 `force=true` 调 `maybe_flush_view`（`mod.rs:330,463`），保证
   `SetCenterChunk/UnloadChunk/ChunkBatch` 先于 `Spawn/BlockUpdate`；否则客户端在还没加载
   的区块里收到实体/方块更新。这条顺序也不允许跨线程竞态。
5. **并发同挖一格靠 `MutableWorld::set_if` CAS**（`block_edit.rs:138`），两连接判定线程各自
   CAS 才能做到"只出一份掉落"（HANDOFF §5.13 R1）。并发语义已在 net 线程模型下验证过。

结论：**"判定与写同线程"当前等价于"判定与写同序、且写方是唯一生产者"**。C1 的全部设计
都是围绕"如何在把判定移到 ECS 后继续维持这条不变量"。

### 1.3 `BroadcastHub` + `PlayEvent` 绕开的核心限制

- `BroadcastHub` 解决的是**"A 的判定线程要给 B 的 socket 写包"**：它不把 `TcpStream` 交给 A，
  而是给每个连接一个 `Sender<QueuedPacket>`，A 只在读锁内拷贝目标发送端、丢锁后 `try_send`
  无界队列（`broadcast.rs:13-16,355-366`），B 自己在 tick 间隙 `drain_outbox`。这样 A 永不
  阻塞在 B 的 socket 上，也保住了"单写者"不变量。
- `PlayEvent` 解决的是**"net 线程要通知 ECS 但 ECS 不能回写"**：单向 channel，
  `Connection lifecycle`（`Disconnect` 去重）、`Movement`、`KeepAlive`、`BlockEdit`(日志)
  都走它；online 只在 ECS 的 `handle_new_connections` 加一次、`player_removal_system` 减一次。
- **核心限制没被绕开，只是被容忍**：`BroadcastHub` 只能搬"已经决定好、只改别人画面"的包，
  `PlayEvent` 只能上报。任何需要**ECS 做出权威决定并立刻对某个玩家回包**的玩法
  （AI 命中扣血、载具位置、命令执行结果）仍然必须绕回 net 线程的 `play_loop`，
  否则就得新建一条临时 channel + 临时队列，正是 C 类任务反复踩的坑。
- 两者都是有意的稳态设计（HANDOFF §5.1），**不是待修 bug**；C1 是"在保留两者优点的基础上
  扩展 ECS 写权"，不是"推翻广播架构"。

### 1.4 P21/P43/P47 已经在 hub 上做了什么（现状边界，勿当缺口重做）

| 任务 | 已落在 hub 上的能力 | 位置 |
|---|---|---|
| P21 | `BlockUpdate 0x08`、掉落 `Spawn 0x01`/`SetData 0x65`/`Remove 0x4E`/`TakeItem 0x7F` 按原始包体 fan-out 给视距内其它连接；接收方 `known` 集逐对端去重；`Ack` 只回操作者 | `broadcast.rs:1-24,355-366`；`login_route/mod.rs:43-59` |
| P43 | `PlayerEntityState`（独立高位 entity id / uuid / pos / yaw / pitch）+ per-viewer 玩家 known 集；Join 双向 Spawn、0x1E/0x1F→`move_entity_pos(54)/pos_rot(55)`、0x20/0x21→`move_entity_rot(57)`、超 i16 回退 `entity_position_sync(35)`、断线 `RemoveEntities 0x4E` 后 unregister | `player_visibility.rs`；`broadcast.rs:99-118,243-289` |
| P47 | 挖掘动画 `BlockDestruction 0x05`（START=0 / STOP=255）经 `queue_destroy_stage` → `hub.broadcast`，操作者只收 Ack；工具档位/时运/精准采集仍是 net 线程判定（基于共享 `MutableWorld`）；3×3 工作台会话仍 per-conn 内存 | `login_route/mod.rs:61-81`；`block_edit.rs:95-`；`login_route/craft9.rs` |

这些广播**不随 C1 重写**：C1 只改变"谁决定发包内容、包进哪个队列"，不改变 hub 的
视距快照、known 集去重、Ack 只回本人的语义。

---

## 2. 候选方案（≥3 个）

### 2.0 共同约束与判据（所有方案逐项打分）

以下 8 条是硬判据，任一不满足即不可选：

1. **Join 包序逐字节不变**：`LoginPlay 0x32` → `SetCenterChunk 0x60` → `GameEvent 0x27`
   (type=13 `LEVEL_CHUNKS_LOAD_START`) → `ChunkBatchStart 0x0C` → `(2r+1)²` 个
   `ChunkData 0x2E` → `ChunkBatchFinished 0x0B`(batch_size=N) → `PlayerPositionSync 0x49`
   → `KeepAlive 0x2D`（`login.rs:576-637`；HANDOFF §3）。
2. **Ack 顺序**：`BlockUpdate` 先、`Ack(seq)` 后，同一玩家流上相邻；Ack 不广播、不合并。
3. **视距 flush 先于方块/掉落 Spawn、且 `ChunkView.sent` 与实发包严格一致**
   （`view_sync.rs:20-131`）。
4. **50ms tick 预算**：`app.update()` 超过 50ms 会打 `tick lag`（`main.rs:225`）；
   P16.1 删掉 ECS 每 tick 区块编码自检正是因为造成 73–79ms lag。441 块生成/编码
   实测 442–558ms（无压缩）/ 1709ms（threshold=256），**绝不能放进 ECS Update**。
5. **唯一物理 ticker（P18.1 R1）**：掉落/XP/作物/树苗/decay/流体只在 `serve()` 启动的
   一个 ticker 推进；连接侧 `tick_drops` 只做拾取/视距同步，绝不 `items.tick()`。
6. **共享权威状态不变**：`MutableWorld::set_if` CAS 保留；admin 线程仍只通过 hub 队列
   （`ADMIN_KICK_PACKET_ID=-1` 信号模式）间接影响连接。
7. **逐字节回归**：`crates/mc-network/tests/play_stream.rs` 19 项 e2e（Join/视距/跨区块
   合并/挖放 Ack/掉落拾取/XP/busy 连接/双连接单 ticker/手持/Q 扔/重进选中槽/远挖/
   掉落出视距回返）一个字不改、全绿。
8. **灰度可回退**：`socket_mode=net|ecs` 启动时配置，默认 `net`；关闭后包流逐字节回退基线。

### 2.1 方案 A：维持 net 线程持 socket + ECS 只做只读状态/事件（**不动架构**）

- **改造文件**：无生产代码改动（仅 `docs/` 与本评估）。最多给 `NetConfig`/`PlayOptions`
  加一个恒为 `net` 的占位字段以便灰度 API 有值可读。
- **线程/所有权**：`serve()` accept → 每连接 tokio 任务独占 `TcpStream`；ECS 只读共享
  `Arc<MutableWorld>` + 收 `PlayEvent`，不持写通道。
- **锁模型**：现状：`BroadcastHub` 三把 `RwLock<HashMap>`（conns/players/player_known），
  `ChunkCache` `Mutex<HashMap>` + FIFO，`MutableWorld` `RwLock<Inner>`；跨线程只传
  channel/快照，不持锁跨调用。
- **Ack 顺序**：现状不变（net 线程 `apply_block_outcome` 内 BlockUpdate→Ack 相邻）。
- **Join 包序**：现状不变，逐字节等于基线。
- **50ms tick 预算**：现状不变，Join 编码在 net 侧 rayon（`ChunkCache::bodies`），
  ECS Update 无区块工作；`tick lag` 0 条。
- **唯一 ticker 兼容**：完全兼容（ticker 本来就在 `serve()`）。
- **崩溃/背压**：现状：无界 crossbeam 队列 + `try_send` 永不阻塞；写失败/`Disconnect`
  统一在 `enter_play_with_options` 退出路径 `unregister` + 投一次 `Disconnect`。
- **优点**：零风险、零回归、可立即交付；**缺点**：C2/C3/C4/B15 继续被阻塞，
  AI 伤害/载具/命令的回包仍需各自造临时通道，技术债继续累积。

### 2.2 方案 B：完全把 socket I/O 移进 ECS + per-player 写通道/异步任务

- **改造文件（预计）**：`mc-network/src/lib.rs`（`serve` 不再 spawn 连接任务；改为向 ECS
  注册 writer/reader）、`config.rs`/`handshake_route.rs`/`login_route/login.rs`（`NetConfig`
  与 `PlayOptions` 换成 writer 句柄而非 `TcpStream`）、`login_route/mod.rs`（`play_loop`
  拆成"ECS 决策 system + 只读 reader 任务 + 有序 writer 任务"）、`broadcast.rs`（写入口
  从"S2C 原始包"改为"ECS 产出的 OutboundIntent"）、`mc-server/src/{main,systems,play,
  movement,persist,net_thread,admin}.rs`（新增 Outbound 组件/队列资源、system 顺序、
  `/api/world` 字段）。
- **线程/所有权**：`TcpStream` 归 ECS 侧管理的 per-player 异步任务（由 Bevy 启动 system
  在 tokio runtime 上 spawn，句柄作为 ECS 组件存在）；读任务把 C2S 包解码后投入 ECS
  的 Inbound 队列，写任务 drain 本玩家 Outbound 队列并负责 VarInt/压缩帧编码。
  **注意**：Bevy system 本身不能 `await`，所谓"socket 进 ECS"只能是"ECS 拥有任务句柄和
  队列"，不是把 `read/write` 塞进 system。
- **锁模型**：每个玩家一个**有界** mpsc（或 `crossbeam` 有界队列）作为唯一写入口；
  全局 hub 仍保留视距快照/known 集（广播决策可留在 ECS），`MutableWorld`/`DropState`
  仍 `Arc` + 锁；需新增"Outbound 队列满"的背压信号回 ECS。
- **Ack 顺序**：同一 ECS system 内先 enqueue BlockUpdate 再 enqueue Ack，单 writer 任务
  顺序 drain → 相邻保证；广播包（别人）不进本人的 writer 队列之前先经 hub。
- **Join 包序**：由 ECS 的 `join_system` 按同一 8 段顺序 enqueue；**但 441 块生成/编码
  必须 offload 到 rayon/blocking task**（否则 50ms tick 爆掉），因而 Join 从"一次顺序
  写"变成"异步生成 + 有序 enqueue"，需要新的"生成完成再放行后续包"的门闩；
  这是包序风险最高的改造点。
- **50ms tick 预算**：高风险。Join 编码、每 50ms 掉包视距同步、KeepAlive 编码若落在
  Update 内，很容易复现 P16.1 的 `tick lag`。必须严格把重活放在 IO/rayon 任务，
  system 只做轻量 enqueue。
- **唯一 ticker 兼容**：ticker 可从 `serve()` 移到 ECS 启动 system，但**必须只启动一次**，
  且不得因连接数或 `app.update()` 多次而倍增；一旦写成"每玩家实体一个 tick system"
  就复现 R1 N 倍速，是最危险的回归点。建议 ticker 仍留在网络/IO 线程，ECS 只读其状态。
- **崩溃/背压**：有界队列必须定义丢包策略：可丢的是 `0x05` 动画、玩家位置差量等
  装饰包；**绝不可丢** Ack / BlockUpdate / `0x49` PositionSync / Unload / KeepAlive /
  ChunkBatch 边界；writer panic → 发 `Disconnect` 信号给 ECS，仍走唯一删除点。
- **优点**：ECS 真正拥有完整权威闭环，C2/C3/C4 最自然；
  **缺点**：改造面最大、Join 异步门闩+50ms 预算+ticker 迁移三处高危，回归成本最高。

### 2.3 方案 C（混合，推荐）：net 线程只做 accept/帧编解码，ECS 拥有 play loop 权威判定 + per-player mpsc 写

- **改造文件（预计）**：
  - `mc-network/src/lib.rs`：`serve()` 保留 accept + 唯一 ticker；连接任务交给"帧层"
    （读帧 → Inbound 队列，写帧 ← Outbound 队列），不再持有权威判定逻辑。
  - `config.rs` / `handshake_route.rs` / `login_route/login.rs`：
    `NetConfig`/`PlayOptions` 增加 `socket_mode` 与 per-player 写通道句柄
    （`OutboundSender`），`PlayEvent` 扩展为携带原始 C2S 意图（见下）。
  - `login_route/mod.rs`：`play_loop` 瘦身为"读帧 + 事件投递 + drain Outbound"；
    `decide_dig`/`decide_place`/`apply_block_outcome` 从 net 线程移到 ECS system。
  - `broadcast.rs`：接口不变（`QueuedPacket{id,body}` + hub 快照），只是入队方从 net
    线程改成 ECS system； Ack 仍只 unicast 给操作者。
  - `mc-server/src/{main,systems,play,movement,persist,admin}.rs`：新增
    `OutboundWriter`/`PlayerView(ChunkView)` 组件、`SocketMode` 资源、
    `authority_*_system` 的编排、`/api/world` 只读字段。
- **线程/所有权**：
  - net 线程（IO 层）：`TcpStream` + VarInt/压缩帧编解码；**不判定**；
    每连接一个有序 `Outbound` 队列（mpsc/crossbeam），单一 writer drain。
  - ECS（权威层）：C2S 原始包（含 sequence）经 `PlayEvent::BlockAction`/`Movement` 进 ECS；
    `authority_block_system` 调 `decide_*`（保留 `set_if` CAS）+ `apply_block_outcome`，
    **产生** BlockUpdate 与 Ack 并 enqueue 到该玩家的 Outbound；广播仍走 hub。
  - 关键不变量：每个玩家的 Outbound 只有 ECS 一个生产者写 Ack；IO 层只搬运字节，
    不插入任何包。因此"同线程"被替换为"同一有序队列的唯一生产者"，Ack 不变量成立。
- **锁模型**：与现状相同的一套 `Arc` + `RwLock`/`Mutex`；不新增跨线程长锁。
  `PlayEvent` 单向 + Outbound 单向，两个方向都不持锁等待对端。
- **Ack 顺序**：ECS system 内 `BlockUpdate` → `Ack(seq)` 相邻 enqueue；
  视距 flush 的 `SetCenter/Unload/ChunkBatch` 也由同一 system 先 enqueue；
  广播包通过 hub 进**接收方** Outbound，绝不与发送方 Ack 混流。
- **Join 包序**：**Join 序列仍由 net 线程生成并首帧投递**（`send_join_sequence` 原样复用，
  8 段字节不变），ECS 只在 `login_join_system` 收到 `handle_new_connections` 的
  `NewConnection` 后建 `PlayerView`/`OutboundWriter` 组件；Join 完成后才允许 hub 注册
  （现有 `register_with_name` 时机不变）。这避开了方案 B 的异步门闩风险。
  后续 C2/C3/C4 的实体/红石包在 Join 之后由 ECS 通过同一 Outbound 发。
- **50ms tick 预算**：Join 生成/编码仍在 net 侧 rayon（`ChunkCache::bodies`）；
  ECS system 只做"判定 + enqueue"，单玩家单包级别是微秒量级；
  保持 `pending_chunks`/`CHUNKS_PER_TICK` 不恢复，`tick lag` 应维持 0。
- **唯一 ticker 兼容**：**ticker 继续留在 `serve()`**，不随 socket 归属迁移；
  ECS 只通过 `Arc<DropState>` 读/改状态，绝不调用 `items.tick()`；
  `tick_drops` 仍是每连接拾取/视距同步，不推进时间。
- **崩溃/背压**：Outbound 有界 + 明确优先级：控制包（Ack/BlockUpdate/PositionSync/
  Unload/ChunkBatch/KeepAlive）不丢；装饰包（0x05 动画、玩家位置差量）可丢；
  持续超限视为慢客户端 → 由 ECS 发 `Disconnect`/踢人信号走统一清理；
  writer 失败只触发一次 `unregister` + 一次 online 递减（沿用 `player_removal_system`
  唯一删除点）。
- **优点**：改动面中等、Join 包序零风险、ticker 不迁移、解锁 C2/C3/C4/B15；
  **缺点**：判定比现状多一跳 channel（Ack 延迟最多一个 50ms tick），
  需要新增"Outbound 生产者唯一"的不变量测试。

### 2.4 方案对比

| 判据 | A 维持现状 | B 全进 ECS | C 混合（推荐） |
|---|---|---|---|
| Join 包序逐字节 | ✅ 原样 | ⚠️ 需异步门闩 | ✅ 原样 |
| Ack 同序/只回本人 | ✅ | ✅ 单 writer | ✅ 唯一生产者 |
| 50ms 预算 / tick lag | ✅ 0 条 | ⚠️ 高危 | ✅ 重活仍在 net 侧 |
| 唯一 ticker（R1） | ✅ | ⚠️ 易倍增 | ✅ 不迁移 |
| 解锁 AI/载具/红石/命令 | ❌ | ✅ | ✅ |
| 改造文件数 | 0 | 12+ | 8–10 |
| 灰度回退成本 | 无 | 高 | 低（开关切回 net） |
| 回归风险 | 无 | 高 | 中 |

---

## 3. 推荐方案与理由

**推荐方案 C**。理由：

1. **它精确修复真正的限制，而不是搬动无关的东西**。痛点是"权威判定拿不到有序写通道"，
   不是"帧编解码必须在 ECS"。C 把 net 线程保留为 dumb framing/writer，ECS 拿到的是
   "有序写队列的生产权"，足够让 AI/载具/红石/命令闭环，又不必重写 Join。
2. **不恢复 P16.1 删除的全量区块生成脚手架**。明确禁止：
   `pending_chunks`、`sent_chunks`、`CHUNKS_PER_TICK`、每 tick
   `WorldGen::generate`/`network_bytes`/`heightmaps_bytes`/`light_bytes`/
   `encode_play_chunk_data_packet` 与逐块 info 日志（HANDOFF §3「ECS Play 脚手架」、
   §8 P16.1 注意）。继续依赖：
   - `ChunkCache::bodies`（rayon 批量生成+编码，命中复用同一 `Arc<Vec<u8>>`；
     `chunk_cache.rs` HANDOFF §3 区块缓存）；
   - `ChunkView::plan`（初始 `(2r+1)²`、跨区块 add/remove 净差、`sent` 与实发包一致；
     `chunk_view.rs`）；
   - 80ms / 6 次跨区块节流（`view_sync.rs`）。
3. **保留 `ChunkCache::bodies` 与 `ChunkView` 是包序不变的前提**：二者已经保证了
   "中心优先的整批区块 + 正确 Batch 边界 + 去重"，ECS 侧重写这些只会引入
   重复/漏块/邻居 section 缺失（30 秒超时）。
4. **客户端逐字节回归 `play_stream.rs` 不能破**：19 项 e2e 一字不改、全绿；
   任何阶段若导致 Join/跨区块/挖放 Ack/掉落/XP/busy 连接/双连接单 ticker
   其中一项红，立即回退该阶段开关，不允许"改测试对齐新行为"。
5. **真机门禁继承**：`docs/real-machine-checklist.md` §1（登录/视距/压缩/断线）、
   §5（多人同屏含 P47 0x05）、§8（面板 `/api/world`）在灰度前后各跑一遍；
   §11 v2 性能红线与 C1 无关，req 不动（若批次 4 前置未绿则不进）。
6. **与后台联动**：`admin.rs::handle_world` 增加只读 `socket_mode` 字段；
   `admin-ui/src/Dashboard.js` 的「世界状态」卡已用 `<pre>{{ worldText }}</pre>` 显示
   `/api/world` 的 JSON，天然展示该字段，无需新增卡片；**只读，不新增 POST 端点**，
   灰度切换走 `server.toml` 启动配置，避免后台线程直接改连接状态。

---

## 4. 迁移路线

### 4.1 灰度开关

- `server.toml` 新键 `socket_mode = "net" | "ecs"`，**默认 `"net"`**（旧 toml 缺键按默认补，
  与 `compression_threshold`/`world_storage`/`generator_version` 的兼容先例一致）。
- 透传链：`ServerConfig` → `NetConfig::with_socket_mode()` → `handshake_route::PlayOptions`
  → `enter_play_with_options`；同时进 `AdminState` 供 `/api/world` 只读展示。
- 语义：
  - `net`（默认）：走现状 `play_loop`，`socket_mode` 字段返回 `"net"`，包流与基线逐字节一致；
  - `ecs`：启用方案 C，`socket_mode` 字段返回 `"ecs"`；
  - **开关只影响连接内发包路径，不影响物理 ticker、存档、协议包定义**。
- 原子性：切换靠重启进程（启动期配置），不做运行时热切；避免"同一进程内两套写路径"
  带来的 Ack 生产者不唯一。若未来要热切，必须按连接粒度在 Join 时决定并锁定。

### 4.2 阶段拆分（每阶段可独立回退）

| 阶段 | 内容（方案 C 子集） | 回退方式 | 验收（单测 / e2e / 真机清单） |
|---|---|---|---|
| P0 只读观测 | 加 `SocketMode` 资源 + `/api/world.socket_mode` 只读字段（恒 `"net"`）+ 面板 `<pre>` 显示；不改任何发包路径 | 删字段/关 UI，零包流影响 | `admin_e2e` 旧 3 项一字不改 + 新 1 项断言字段存在且只读；`check-offline.mjs` 零外链；清单 §8 |
| P1 通道抽取 | 把现状 `play_loop` 的"写"抽成 per-player Outbound 队列 + 单 writer drain（`socket_mode=net` 下行为逐字节不变，仅内部重构） | 开关恒 `net`，回退提交即可 | `play_stream.rs` 19 项全绿，`player_visibility.rs`/`fluid.rs`/`play_actions.rs` 全绿；清单 §1/§5 |
| P2 移动/心跳权威 ECS 化 | C2S `Movement`/`KeepAlive` 意图先经 ECS 更新 `Position`/`last_seen` 再回写（位置差量可丢，Ack 类不动） | `socket_mode=net` 恢复现状 | `movement.rs` 单测 + `play_stream::movement_and_keepalive_packets_reach_play_event_channel` + 清单 §1 移动/断线 |
| P3 方块判定 ECS 化 | `decide_*`/`apply_block_outcome` 移到 ECS system；保留 `set_if` CAS、view 强制 flush 先于 BlockUpdate、Ack 相邻；hub 广播入口改为 ECS | 单独 opt-in `socket_mode` 按连接回退 | `play_stream::dig_and_place_roundtrip_with_blockupdate_ack_and_persistence` + `play_actions.rs` + 清单 §3、§5（B 不收 Ack） |
| P4 视距/掉落同步 ECS 化 | `ChunkView`/`PendingViewMove`/`tick_drops` 的"决定发包"移到 ECS，**生成/编码仍在 net 侧 `ChunkCache::bodies`** | 回退 P4 开关 | `view_distance_two_streams...`/`crossing_chunk_streams...`/`consecutive_chunk_crossings...` + `drop_leaves_view_then_respawns_on_return` + `two_connections_share_one_global_drop_physics_tick`（**ticker 仍单实例**）+ 清单 §1、§4 |
| P5 全量切换与清理 | 生产入口 `socket_mode=ecs`；删除 net 侧残留判定分支；文档更新（由主会话维护 `HANDOFF.md`） | 改回 `socket_mode=net` 重启 | 全套 e2e + `cargo check --workspace --tests` + `check_budget.sh` + 清单 §1–§10（重点 §1/§3/§5/§8） |

- 每阶段**独立提交、独立灰度、独立回退**；不跨阶段共享未验证的新包/新状态。
- 每阶段必须同时通过：相关单测 + 对应 `play_stream`/`play_actions`/`admin_e2e` +
  真机清单对应组的**切换前后对比**；任何一项红即停在该阶段。
- 回退通则沿用 gameplay-gaps §3.0：每个能力一个 opt-in，关掉后包流逐字节回退基线。

### 4.3 需要抓包或额外数据的前置

- **C1 本身不需要新抓包**：不新增包 ID、不改包体，Join/Chunk/BlockUpdate/Ack 全部已验证。
- **但必须补齐的观测数据**（否则无法判定灰度是否达标）：
  1. `play join chunks sent ... elapsed_ms` 在 `socket_mode=net` 与 `ecs` 下的对照
     （默认 seed、r=10、threshold=-1 与 256；基线 442–558ms / 1709ms）；
  2. `tick lag` 计数在两种模式下必须都为 0（P16.1 基线）；
  3. `admin /api/world` 的 `socket_mode` 快照与进程实际模式一致；
  4. 双开 A/B：B 收 `BlockUpdate` 但**零 Ack**、0x05 动画仅发给视距内（清单 §5）；
  5. 慢客户端/背压注入：有界队列满时的丢包计数与连接存活时间（用于定阈值）。
- **批次 4 硬前置**（gameplay-gaps §4）：批次 0–3 全绿 + 用户批准；清单 §11 v2 性能红线
  若未过，先修 worldgen 问题（走 `docs/worldgen-plan.md`）再进 C1，避免两个变量叠加。
- C2/C3/C4/B15 各自所需的 AI/载具/红石/brigadier 抓包是**它们自己的前置**，不属于 C1；
  C1 只提供"ECS 可权威回包"的基础设施。

---

## 5. 对后续玩法的影响

### 5.1 为什么 AI / 红石 / 载具 / 命令依赖 C1

| 后续任务 | 依赖 C1 的具体环节 |
|---|---|
| C2 生物 AI + 战斗 | 命中判定在 ECS 全局 ticker 做出后，必须立刻给**受击玩家的 socket** 发 `damage_event 0x19`/`hurt_animation 0x2b`，并给攻击者回 `animate 0x02`；现在 ECS 没有写通道，只能绕回 net 线程造临时通道；实体位置/血量在 ECS 池，判定与写跨线程会重演 Ack 错位问题 |
| C3 载具 | C2S `move_vehicle 0x22`/`paddle_boat 0x23` 的权威结果 `move_vehicle 0x3a`/`set_passengers 0x6d` 要发给同乘者与视距内玩家；乘客关系在 ECS，写入口不在，无法保证"物理 tick → 广播"同序 |
| C4 红石 | 全服 ticking 区域在 ECS ticker 里传播信号，同 tick 合并后 `BlockUpdate 0x08`/`block_event 0x07` 要按合并结果写给视距内；现在 ticker 只能调 hub，无法给操作者回 Ack（红石操作者也是玩家）；高频更新还要有界队列背压，正是 C1 的 Outbound 模型 |
| B15 命令 | 命令执行上下文在 ECS，结果必须回给发起玩家（且 admin 的 `/api/command` 走独立通道不能借 chat 伪造）；需要"ECS 决定 → per-player 有序写"的标准入口，否则每条命令都要新建一次性 channel |

共同点：它们都需要**"ECS 在 50ms tick 内做出权威判定，并按玩家有序回包"**。
方案 A 让它们永远停在调研；方案 C 提供唯一入口，后续只需新增包内容而不是新增通道。

### 5.2 即使不做 C1 也能继续的玩法（A / B 类）

- **A 类全部**（经验球合并/吸引、饥饿细节、广播限流、后台设置持久化、journal 运维、
  时间/天气、统计计数、工作台会话落盘）：纯服务端状态或已验证包，落 `BroadcastHub`/
  全局 ticker/redb，不碰 socket 归属。
- **B 类中不依赖 ECS 权威回包的**：聊天（B1，net 线程 fan-out）、`player_info_update 0x47`
  名字/皮肤（B2，Join/事件时由 hub 发）、完整 `Respawn 0x54`（B3，P40 死亡路径在 net 线程）、
  飞行（B4）、创造槽（B5）、`open_screen`（B6）、声音/粒子（B8）、方块实体/箱子（B9）、
  熔炼（B10，ticker + hub 广播）、真附魔（B11）、药水（B12）、统计/成就发包（B13）
  都可继续，只要它们的发包仍在 net 线程或 hub 上。
- **明确继续被阻塞的**：C2/C3/C4/B14 交易（依赖 C2 村民 AI）/B15 命令，
  以及任何"AI/物理在 ECS 判定且需即时回包"的组合。
- 依据：gameplay-gaps §2.2/§2.3 的分类表；C1 是这些 C 类的隐藏前置，不是 A/B 的前置。

---

## 6. 风险与决策点

### 6.1 技术风险

| 风险 | 表现 / 后果 | 缓解 |
|---|---|---|
| 写阻塞 | 有界 Outbound 满时 writer/data 线程阻塞，反向拖慢 ECS 50ms tick，全服 lag | 优先级分层（控制包不丢、装饰包可丢）；`try_send` + 慢客户端超时踢出；预演注入测试定阈值 |
| 包序错乱 | Ack 被广播/别人的包插队；`ChunkBatchStart/Finished` 被拆开；`PositionSync` 跑到 ChunkData 前面 | "唯一生产者 + 单 writer"不变量；Join 序列保持 net 线程原样；`play_stream.rs` 逐字节断言不动 |
| Ack 归属 | 把 Ack 塞进 hub 广播（gameplay-gaps §5.1 明令禁止）→ 旁人 sequence 错位 | Ack 始终只 `unicast` 操作者；新增 e2e 断言"B 收 BlockUpdate 零 Ack" |
| 并发判定（CAS） | 两连接/AI ticker/admin 同改一格产生双份掉落 | 保留 `MutableWorld::set_if` CAS；ECS 单线程 system 内也不改成 read-then-write |
| 连接生命周期 | writer 失败、超时、admin kick、Join 失败各自清理 → online 减多次或留幽灵通道 | 沿用唯一删除点 `player_removal_system`；`conn_id` 单调不复用；所有退出路径汇入一次 `Disconnect` + `unregister` |
| 全局 ticker 倍增 | 写成"每玩家一个 tick system"复现 R1 N 倍速 | ticker 留在 `serve()`，不随 socket 迁移；`two_connections_share_one_global_drop_physics_tick` 必须持续绿 |
| 50ms 预算 | Join 441 块编码或视距 flush 落进 Update → `tick lag` | 重活仍在 net 侧 `ChunkCache::bodies`/rayon；`pending_chunks` 不恢复 |
| 灰度双路径 | 同进程内 net 与 ecs 两套写路径并存导致 Ack 双生产者 | 按进程启动配置锁定；按连接 Join 时决定并固定；不做运行时热切 |
| Ack 延迟 | 判定移到 ECS 后最多多一个 50ms tick，手感变化 | P3 真机 §3 对比 START/STOP 与 Ack 时序；若不可接受，允许"同 tick 内立即 enqueue + 非阻塞 flush"优化 |

### 6.2 管理面板联动（用户硬约束）

- `admin.rs::handle_world()` 的 JSON 增加**只读** `"socket_mode": "net"|"ecs"`
  （`AdminState` 增加对应字段，从启动配置拷贝）；不改 `/api/online`，不新增 POST。
- `admin-ui/src/Dashboard.js` 的「世界状态」卡已用 `worldText` 展示 `/api/world` JSON，
  字段自动可见；**不需要新卡片**，但需确认 `check-offline.mjs` 仍零外链、
  `admin_page.html` 重新构建后 sha 记录更新（构建由主会话/实现阶段执行，本轮不做）。
- 面板只显示、不提供切换控件：灰度切换是 `server.toml` 启动配置，
  后台线程不得直接改连接写路径（HANDOFF §5.1 故障隔离设计）。

### 6.3 工作量估算（单人日，仅评估）

| 项 | 估算 |
|---|---|
| P0 只读观测（含 admin + 面板字段 + e2e） | 0.5 d |
| P1 Outbound 通道抽取（纯重构，逐字节回归） | 1.5–2 d |
| P2 移动/心跳 ECS 化 | 1–1.5 d |
| P3 方块判定 ECS 化（最高危） | 2–3 d |
| P4 视距/掉落 ECS 化（含背压策略） | 2–3 d |
| P5 全量切换 + 真机复验配合 | 1–2 d |
| **合计** | **8–12 人日**（不含后续 C2/C3/C4/B15 各自立项） |

> 估算前提：批次 0–3 已全绿、无并行 worldgen 改动；若真机发现包序回归，
> 定位成本另计（历史上 Ack/0x26 packed i64 类问题曾花数日）。

### 6.4 需要用户批准 / 决策的事项

1. **是否批准 C1 立项本身**：即"改 socket 归属 / 新增全局实体系统 / 接受大版本改造"
   （gameplay-gaps §5.2 第 9 问）。未批准则 C2/C3/C4/B15 停在调研，只做 A/B。
2. **是否接受方案 C 而非方案 B**：C 的判定比现状多一跳 channel（最多一个 50ms tick
   的 Ack 延迟）；若用户要求"零延迟 Ack"且愿付最大改造成本，才选 B。
3. **灰度策略确认**：`socket_mode` 默认 `net`、按进程重启切换、面板只读——是否接受？
   是否需要在 `server.toml` 之外提供运维脚本（本轮不做）。
4. **背压策略**：慢客户端有界队列满时，是否同意"优先丢装饰包、持续超限踢人"？
   还是要求无限队列（写阻塞风险自负）？
5. **真机执行者与时点**：清单 §1/§3/§5/§8 的切换前后对比由谁在 Windows 客户端跑、
   何时跑？未跑完不进 P3+（硬门禁）。
6. **范围边界**：本轮**只交付本文档**；下一轮是否直接按阶段 P0→P5 实现，
   还是先只做 P0+P1（低风险通道抽取）再评估？
7. **C2/C3/C4/B15 的排期**：C1 完成后是否立即启动后续立项，以及它们的抓包
   （AI 行为数据、载具 NBT、红石网络、brigadier 树）由谁负责——这些不在 C1 预算内。
8. **文档归属**：本文件由本任务线维护；`HANDOFF.md`/`MEMORY.md`/`tasks/` 由主会话维护，
   P5 完成时的状态更新需主会话执行（本轮不动）。

---

## 7. 来源索引（本轮只读）

| 文件 / 章节 | 用途 |
|---|---|
| `docs/gameplay-gaps-plan.md` §3.3 C1、§4 批次 4、§5.1/§5.2 | 立项边界、前置、灰度、决策点编号 |
| `HANDOFF.md` §3「ECS Play 脚手架（P16.1）」、§5.1、§5.9/§5.13/§5.17/§5.30/§5.31、§8 | 现状边界、R1、hub 语义、不要恢复脚手架 |
| `crates/mc-network/src/lib.rs:66-115` | `serve()` accept / conn_id / 唯一 ticker |
| `crates/mc-network/src/config.rs:21-49`、`handshake_route.rs:40-55` | `NetConfig`/`PlayOptions` 透传链 |
| `crates/mc-network/src/login_route/login.rs:27-54,376-445,576-637` | `PlayOptions`、Join 8 段包序、hub 注册时机 |
| `crates/mc-network/src/login_route/mod.rs:43-59,83-` | `drain_outbox`、`play_loop`、50ms 节拍、force flush |
| `crates/mc-network/src/login_route/block_edit.rs:359-411` | BlockUpdate→Ack 相邻、`set_if` CAS |
| `crates/mc-network/src/login_route/view_sync.rs:13-131` | 80ms/6 次跨区块节流与 flush 顺序 |
| `crates/mc-network/src/broadcast.rs:1-24,66-118,335-366` | per-player 写通道、视距快照、try_send 语义 |
| `crates/mc-server/src/main.rs:101-182,201-227` | 共享 Arc 注入、Bevy system 编排、50ms 主循环 |
| `crates/mc-server/src/play.rs:19-33,85-102` | `PlayScaffold` 诊断、无写通道的 KeepAlive TODO |
| `crates/mc-server/src/systems.rs:24-63`、`movement.rs:151-205` | NewConnection/PlayEvent/唯一删除点 |
| `crates/mc-server/src/admin.rs:417-431` | `/api/world` JSON（`socket_mode` 落点） |
| `admin-ui/src/Dashboard.js:17,162-165,357` | 「世界状态」卡 `<pre>{{ worldText }}</pre>` 显示 |
| `docs/real-machine-checklist.md` §1/§3/§5/§8/§12 | 灰度前后真机门禁与失败定位 |
| `crates/mc-network/tests/play_stream.rs` | 19 项逐字节/包流回归基线（一字不改） |
