# P52：方案 B（socket I/O 全量进 ECS）实现说明

> 分支 `p52-socket-ecs-b`，基于 main `bfdff8f`。设计依据 `docs/socket-ecs-plan.md` §2.2/§3/§4/§6。
> 本文记录实现事实、回退方式与待真机验证项；`HANDOFF.md`/`MEMORY.md`/`tasks/` 由主会话维护，本分支未改。

## 1. 线程 / 所有权模型

- `serve()` 在 `socket_mode=ecs` 下**只做 accept + 单调 conn_id 分配**，把 `TcpStream` 通过
  `crossbeam` 通道交给 ECS；**不再**为连接 `tokio::spawn` 任务。
- Bevy `Update` 链新增 `systems::socket_ecs_system`（在 `handle_new_connections` 之前），
  每 tick 只 `try_recv` + `register`：不阻塞 50ms 预算、不做 socket IO。
- `mc_network::SocketEcsRuntime`（Bevy 侧包成 `resources::SocketEcs`）是唯一连接登记中心，
  持有每个连接的三个任务句柄：
  1. **reader**：非阻塞读真实 socket 的原始字节 → Inbound 队列（只搬运，不判定）；
  2. **writer**：本连接**唯一**写者，FIFO 排空 Outbound 队列并写真实 socket；
  3. **logic**：复用既有（已逐字节验证）的 `handshake_route::handle_connection` →
     登录 / 配置 / Play 权威路径，把要发的包编码进本玩家 Outbound 队列。
- Bevy system 本身不 `await`；"socket 进 ECS" 按计划 §2.2 的定义是
  **ECS 拥有任务句柄、队列与写权生产权**，而非把 `read/write` 塞进 system。
- `mc-net` tokio runtime 由 `main` 创建，`Handle` 交给 ECS 资源；`net_thread` 只跑 accept 与 Ctrl-C。
- 全局 50ms 物理 ticker（drop/fluid/crop/timer）**仍只在 `serve()` 启动一次**；连接侧绝不 tick，
  未恢复 P16.1 删除的 `pending_chunks`/`CHUNKS_PER_TICK`/每 tick WorldGen。

## 2. 三条红线的保证

1. **Join 八段包序 / `(2r+1)²` ChunkData 逐字节不变**：`send_join_sequence` 原样复用，
   只是 `TcpStream` 泛型化为 `AsyncRead + AsyncWrite`；writer 单任务 FIFO 保证顺序。
   `socket_ecs_e2e::ecs_join_eight_segments_are_byte_exact_and_match_net_mode` 同时断言
   段序与 ECS/net 两模式逐字节相等。
2. **BlockUpdate → Ack(seq) 相邻、Ack 不广播**：logic 是本人 Outbound 的唯一生产者，
   同一路径内先 `BlockUpdate` 再 `Ack`；hub 只 fan-out 方块/掉落给视距内**其它**连接，
   Ack 从不进 hub。视距 flush（SetCenter/Unload/ChunkBatch）的 `force=true` 门闩原样保留，
   仍在方块/掉落包之前。e2e 双连接断言 A 相邻 Ack、B 收 BlockUpdate 且零 Ack。
3. **唯一 50ms ticker**：ticker 归属未迁移；连接数不产生额外 ticker 或 tick 行为。

## 3. 回退开关与后台联动

- `server.toml` 的 `socket_mode = "net" | "ecs"`，**生产默认 `"ecs"`**（旧 toml 缺键自动补写），
  启动期锁定、不做运行时热切（避免同进程双写路径/Ack 双生产者）。
- `net` 模式走原 `play_loop`，逐字节等于历史基线（现有 509 测试全部在这个模式下保持一致）。
- `GET /api/world` 新增**只读** `"socket_mode"` 字段；旧字段与 `/api/online`、所有 POST 不变。
  面板「世界状态」卡原本就是 `<pre>{{ worldText }}</pre>`，字段自动可见，无需改前端产物。

## 4. 写队列风险与监控点（用户明确选择无限队列）

- 选择：**不丢装饰包、不踢慢客户端**，每个连接一个 `tokio::mpsc::unbounded_channel`。
- **风险**：慢客户端读取慢于服务端产出时 Outbound 队列**无界增长**，最终可能 OOM；
  同时 socket 写可能在 writer 任务里阻塞（只拖慢该连接，不阻塞 Bevy Update）。
- **监控点**：`SocketEcsRuntime::stats()` 的 `accepted`/`registered`/`retired`/`queued_bytes_total`
  与 `active_connections()`；日志关键字 `socket_ecs register` / `socket_ecs logic ... closed`。
  真机压测需观察慢连接下 RSS 增长；若不可接受，后续应改为有界 + 明确优先级丢包策略（需用户再批准）。

## 5. 真机待验（脚本 e2e 不能替代）

- `docs/real-machine-checklist.md` §1（登录/视距/压缩/断线）、§3（方块交互/Ack）、
  §5（多人同屏含 0x05）、§8（面板 `/api/world`）在默认 ecs 模式下各跑一遍；
  再以 `socket_mode = "net"` 重启做同样对比。
- 重点观察：`tick lag` 必须为 0；Join `elapsed_ms` 与 net 模式对照；
  多人场景 B 端零 Ack；慢客户端内存增长。
