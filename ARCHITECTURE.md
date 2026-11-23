# 架构设计：集百家之所长

选择（用户已定）: Java生存服 / Bevy ECS分tick / Rust+WASM插件 / 只跟最新版

## 1. 为什么这样选

| 需求 | 抄谁 | 做法 |
|---|---|---|
| 框架可扩展 | Valence | 一切皆 Bevy Plugin: ClientPlugin/MovementPlugin/ChunkViewPlugin... Client 是 Entity |
| 高性能多线程 | FerrumC | TCP acceptor 独立线程+Tokio runtime, crossbeam-channel 进 Main World, 自研 NBT/Anvil 以后上 SIMD |
| 原版对齐 | Pumpkin/SteelMC | registry/build.rs 从 minecraft-data 生成方块/物品表; 世界生成做 parity 测试 |
| 易用 | Paper | server.toml + RCON/Query + 以后 Web 后台 |

## 2. 线程模型

- Net线程: tokio TcpListener, 处理握手/状态/登录/AES/CFB + zlib, 解包后推 Event 到 ECS
- Main线程: bevy_ecs World, 固定 50ms tick, 只跑同步系统
- Worker池: rayon, 跑 chunk生成/光照/存档压缩/包编码

绝对不要在 tick 里做 IO。IO 全部走 channel + rayon。

## 3. Crate 边界

- mc-protocol 不依赖 bevy, 纯编解码，可独立 fuzz/bench
- mc-world 不依赖网络，可独立做 parity 测试
- mc-server 只做组装和调度，不写具体玩法逻辑

## 4. 数据驱动

- 协议包定义 + 注册表全部走 codegen，不要手写 700+ 包
- 数据源: PrismarineJS minecraft-data 或 Valence extractor (Fabric mod 抽 JSON)
- 版本升级 = 换数据文件 + 重新 cargo build

## 5. 插件

- Phase1: Rust Plugin = Bevy Plugin, 静态链接，最快
- Phase2: WASM (wasmtime), 事件总线: PlayerJoin/Chat/Tick/BlockBreak, 权限隔离+热重载
- 不做 Java/Paper 兼容 (FerrumC FAQ 已证伪: 反射+动态类加载在 Rust 不可行)

## 6. 下一步实现顺序

1. mc-core + mc-protocol VarInt/NBT/ping 包 (可独立测试)
2. mc-network status 监听 (mc-server 能在 MC 列表里被看到)
3. login + play + 空世界 + 移动回显
4. chunk 视距 + Anvil 读
5. 生存 + 命令 + 插件
