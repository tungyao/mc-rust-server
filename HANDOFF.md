# 交接文档：Rust 自研 MC 服务器（只兼容 Java 26.3 / protocol 777 / data 5023）

> 日期：2026-11-23；分支 `main` @ `bc3c8f0`（工作区有未提交的 P11/P12 改动）
> 目标：原版 26.3 客户端能完成 Handshake → Login → Configuration → Play，并渲染本服自己生成的世界。

## 1. 一句话现状

**原版 26.3 客户端已经能进 Play 并看到本服 `mc-world` 生成的平原世界**：出生点 (369,67,381)、chunk (23,23)，一次同批发送视距 4 的 81 个区块；客户端约 5 秒内离开“加载地形中…”，落到 y=65 的草方块上，KeepAlive 稳定，**不再出现 30 秒超时**。

上一阶段（P11）的 Configuration 卡点也已解决：根因不是 dimension NBT，而是 `minecraft:block` 标签缺少 `minecraft:infiniburn_overworld`。现在 Configuration 直接回放原版 26.3 抓包的全量注册表与 Update Tags，保证条目 ID 与标签自洽。

## 2. 怎么跑

### 服务端

```powershell
cd H:\code\rust\paper
cargo build -p mc-server
.\target\debug\mc-server.exe        # 监听 0.0.0.0:25565，离线模式，无压缩（threshold=-1）
```

`server.toml` 可改 bind / motd / max_players / view_distance / seed。

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

客户端日志成功标志：`Connecting to 127.0.0.1, 25565` 后**没有** `Timed out while waiting for the client to load chunks`，直接进入世界。服务端日志成功标志：`enter_play` → `play join sent` → `play keepalive echo` → `play move ... y: 65.0, on_ground: true`。

## 3. 当前实现（P11/P12 之后）

### Configuration

- 回放原版 26.3 空 KnownPacks 抓包的 **32 个同步注册表**（`vanilla_config_777.rs` + `data/vanilla_config_777/registry_00..31.bin`），不再手写最小注册表。
- 回放同一次抓包的 **全量 Update Tags**（15 个注册表、19662 项，`data/update_tags_777.bin`）；`block_tags_777.rs` 保留完整 300 个 block tag 及回归测试。
- 因此 `minecraft:block` 条目 ID 与标签 ID 全部来自原版，`infiniburn_overworld` 等缺失标签问题彻底消失。
- `packets/config.rs` 仍保留 registry/update tags 的编解码与断言测试，但发送路径以抓包字节为准。

### Play 登录序列（`mc-network/src/login_route.rs`）

按顺序发送：

1. `LoginPlay`（0x32，777 布局：previous gamemode optional、last death optional、portal cooldown、sea level、online mode 等）
2. `SetCenterChunk`（0x60）中心 = `SPAWN_POS` 所在区块 (23,23)
3. `GameEvent`（0x27 / type 13 `LEVEL_CHUNKS_LOAD_START`）
4. `ChunkBatchStart`（0x0C）
5. **81 个 `ChunkData`（0x2E）**，同一批
6. `ChunkBatchFinished`（0x0B，batch_size=81）
7. `PlayerPositionSync`（0x49，teleport id 在前，777 布局）
8. `KeepAlive`（0x2D）

之后 `play_loop` 每 15 秒主动发 KeepAlive，并处理 C2S KeepAlive 回显、移动包 0x1E–0x21。

**为什么必须发 81 块（至少 3×3）**：客户端 `SectionUpdateTracker.hasAllNeighbors` 要求玩家所在 section 的 4 个正向 + 4 个斜向邻居区块都存在，否则玩家 section 永远不会编译，30 秒后客户端打印超时并强制放行。只发 1 块时即使格式完全正确也会超时。

### 区块数据（`mc-world` + `play_chunk.rs`）

- 真实 blockstate ID（对齐 `reports/blocks.json`）：`air=0`、`stone=1`、`grass_block=9`（snowy=false）、`dirt=10`。
- 平原分层：y≤60 石头，61–63 泥土，64 草方块，其余空气；24 个 section，对应 y=-64..319。
- 均匀 section 用单值 palette；混合 section 用**间接 palette**（BPE=4，palette `[stone,dirt]` / `[grass,air]`，4096 项 Data Array，1.16+ 不跨 long）。
- `MOTION_BLOCKING` 高度图：草方块 y=64 → 值 65-(-64)=129，BPE=9、每 long 7 项、37 longs。
- 天空光：26.3 的 BitSet 线格式是 **`VarInt 字节数 + 小端 bit 字节`**（`FriendlyByteBuf.writeByteArray(BitSet.toByteArray())`），不是旧版 `long 数量 + long 数组`。给 section 8..23 发 2048B 全 15 级 nibble，地下按 0。真机截图确认地表全亮。
- 旧的“原版 (23,23) 区块抓包”调试脚手架已删除（`vanilla_chunk_23_23_body()` 与 42KB fixture 不再存在）。

### 其它已修

- `LoginPlay` 和 `PlayerPositionSync` 按 777 真布局修正，并用原版抓包 fixture 做逐字节往返测试。
- KeepAlive 周期发送，真机连接可长期保持。
- GameEvent + Chunk Batch 是 26.3 离开“加载地形中…”的必要条件。

## 4. 验证结果（2026-11-23 真机）

- 客户端进入 Play，无 `DecoderException`、无 `level_chunk_with_light` 解码失败、无 30s 超时。
- 服务端持续收到移动包，玩家 `y=65.0, on_ground: true`。
- 画面：平原草方块、天空/云正常、地表受光；HUD 正常。
- 客户端 KeepAlive 回显稳定，服务端每 15s 主动发包。
- `cargo test --workspace`：20 个 test target 全过；`cargo check --workspace --tests` 0 error；`scripts/check_budget.sh` 通过（src ≤2000 行软约束）。

## 5. 已知限制 / 下一步

1. **压缩路径仍未用真机端到端验证**：当前 `handle_login` 固定 `COMPRESSION_DISABLED_THRESHOLD`。帧编解码单测已覆盖，若要接真实压缩，需把 threshold 配置化后重跑真机。
2. **socket 尚未交进 ECS**：Play 的 Join/Chunk 实际由 net 线程发送；`mc-server/src/play.rs` 仍只做编码自检、视距队列和日志，`TODO(P5c)` 保留。
3. **移动包尚未转发 ECS**：`play_loop` 只解析并记录，未写入 `MovementQueue`。
4. **区块只发初始 81 块**：玩家跨区块后没有动态加载/卸载，也没有把区块数据写入 Anvil 存档；视距外仍由客户端保留。
5. **地形是固定平原**：`chunk_for` 与 x/z 无关，尚未按 seed 生成高度/群系；区块发送也未按玩家 per-player 拆分。
6. **每块带 32KB 天空光 + 4KB section**：81 块约 2.9MB，未压缩；后续可只在必要时发光照/裁剪 section，或启用压缩。
7. `mc-registry` 的 216 项占位表仍是旧数据；区块直接用 `mc-world` 常量，未走该表。

## 6. 关键文件

```
crates/mc-protocol/src/packets/
  config.rs                  Configuration 包编解码/断言
  vanilla_config_777.rs      32 个原版注册表包体 + Update Tags 常量
  data/vanilla_config_777/   抓包二进制（33 个文件，约 216KB）
  block_tags_777.rs          300 个 block tag / 14825 项的回归数据
  play_chunk.rs              SetCenter/ChunkData/GameEvent/Batch + palette/heightmap/light 编码
  play_login.rs              LoginPlay 777 布局 + 抓包往返测试
  play_sync.rs               PlayerPositionSync 777 布局 + 抓包往返测试
  data/*_777.bin             LoginPlay / PositionSync / UpdateTags 抓包
crates/mc-network/src/login_route.rs   Configuration/Play 发送序列、81 块、KeepAlive 循环
crates/mc-world/src/lib.rs             SPAWN_POS、chunk_for、palette/heightmap/light
crates/mc-server/src/play.rs           ECS 视距队列/日志（socket 未接入）
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

- 当前分支 `main`，最近提交 `bc3c8f0`；P11/P12 改动尚未提交。
- 不要提交：`scripts/check_budget.sh` 的纯行尾变化、`Cargo.lock`（如无依赖变更）、`logs/`、`target/`、临时日志。
- `HANDOFF.md`、`MEMORY.md` 按仓库习惯更新；`HANDOFF.md` 是未跟踪文件。
- 旧的 `.gitignore` / `desk_log/conn.log` 删除是历史工作区噪声，已从 HEAD 恢复，不要误提交。
