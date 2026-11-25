# 架构设计：集百家之所长

选择（用户已定）: Java生存服 / Bevy ECS分tick / Rust+WASM插件 / 只跟最新版

## 1. 为什么这样选

| 需求 | 抄谁 | 做法 |
|---|---|---|
| 框架可扩展 | Valence | 一切皆 Bevy Plugin: ClientPlugin/MovementPlugin/ChunkViewPlugin... Client 是 Entity |
| 高性能多线程 | FerrumC | TCP acceptor 独立线程+Tokio runtime, crossbeam-channel 进 Main World, 自研 NBT/Anvil 以后上 SIMD |
| 原版对齐 | Pumpkin/SteelMC | registry/build.rs 从 Pumpkin Extractor JSON 生成真实方块/物品 ID 表; 世界生成做 parity 测试 |
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

## 4. 数据驱动（已实现，26.3 / 777）

- 真实 ID **单一数据源**：`data/manifest.json` + `data/<protocol>/*.json`，来自 Pumpkin 26.3
  Extractor 的 `assets/*.json`（来源 URL / SHA 见 `data/777/SOURCE.md`）；不再有连续占位假 ID。
- 流水线：`crates/mc-registry/build.rs` 读精简表 → 生成 `$OUT_DIR/generated.rs`
  （`BLOCKS` / `ITEMS` / `ENTITIES` / `PACKETS` + `by_name` / `by_id` + 29 项真机 ID 构建期断言）
  → `crates/mc-registry/src/lib.rs` 的 `include!` → `mc-world` / `mc-protocol` 编译期查表。
  方块/物品/实体/包 ID 均由表派生，缺失即编译期 panic，不静默回退。
- manifest 选版本（`version` / `protocol` / `data_version` / `data_dir`），版本目录按协议隔离
  （当前 `data/777/`）；`vanilla_config` 有序索引（32 个 registry bin + update_tags）也由
  `mc-protocol/build.rs` 编译期生成。升级 = 换数据文件 + `cargo build` 重跑两个 build script，
  详见 `docs/data-driven.md`。
- **边界**：codegen 只解决 ID，**不生成包体**；字段顺序、新增字段、注册表条目结构等布局差异
  仍需手改 `mc-protocol`。游戏逻辑（`WorldGen` / `drops` / `inventory`）继续自研，不搬 Pumpkin
  的 Bevy 绑定代码。整体切换版本，不做多版本兼容（见 `VERSION.md`）。

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
