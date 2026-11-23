# Rust MC Server (自研)

目标：Java版生存服，对标 Paper，集 Valence / FerrumC / Pumpkin / SteelMC 之所长。
- 只跟最新版 (1.21.8 起步，后续跟 26.x)，不做多版本兼容
- Bevy ECS + 分 tick (同步玩法 / 异步世界)
- Rust 原生 Plugin + WASM 沙箱

## Workspace

- mc-core: 基础类型 Pos/ChunkPos/Text/Tick/Error
- mc-protocol: VarInt/NBT/包编解码 + codegen (学 Valence)
- mc-registry: 方块/物品/生物群系/维度 (build.rs 生成)
- mc-network: tokio + 加密/压缩 + Status/Login/Play (学 FerrumC 线程模型)
- mc-world: Chunk(三种截面) + 光照/流体 + Anvil + K/V (学 FerrumC/SteelMC)
- mc-commands: 命令图 (学 FerrumC Brigadier-like)
- mc-plugins: 插件 ABI + Wasmtime (Rust trait 先行, WASM 后上)
- mc-server: 组装 Bevy App + EventLoop + Systems (学 Valence Plugin制)

## Tick 模型

[Net线程 Tokio] --crossbeam--> [Main线程 Bevy World 50ms] --rayon--> [Worker池: 生成/光照/IO]
同步: 移动/战斗/交互/命令/AI  异步: 区块生成、光照、包编码、磁盘IO

## 路线图
- P0 骨架: workspace + mc-server 能跑 0.0.0.0:25565 + Status
- P1 Limbo: Login/加密/压缩/进服/移动/聊天/KeepAlive
- P2 世界: 视距发送 + Anvil读写 + 简单地形
- P3 生存: 背包/血量/饥饿/重生/TabList
- P4 扩展: 命令图 + Rust Plugin + WASM + RCON/Query
