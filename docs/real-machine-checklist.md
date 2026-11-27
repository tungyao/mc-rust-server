# 真机复验清单（Java 26.3 客户端 + 本机服务端）

> 日期：2026-11-27；对应 main `862c69e`（`cargo test --workspace` **588 passed / 0 failed**）。
> 本轮新增 §13（P51/P52/P53/P54/P55/P56），请连同 §1/§5/§9/§11 一起验；§12 的 UUID 说明已更新。
> 本清单是**人工验证**用；仓库里的脚本 e2e 只验证服务端状态/包流，不能替代真机观察。
> 原则：每项都记录「配置 → 操作 → 期望日志/包/画面 → 截图」，失败先看 §12 的定位表。

---

## 0. 通用准备（每次复验前）

- [ ] 构建：`cargo build -p mc-server`，用 `.\target\debug\mc-server.exe`，工作目录含 `server.toml`。
- [ ] 客户端：**Java 25 runtime `java-runtime-epsilon`**，必须加 `-XX:StackShadowPages=32`，否则 26.3 会偶发 `jvm.dll 0xc0000005`（与本服无关）。
- [ ] 启动参数骨架（HANDOFF §2）：

```powershell
$java = 'C:\Users\qwe\AppData\Local\Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime\java-runtime-epsilon\windows-x64\java-runtime-epsilon\bin\java.exe'
$cp   = Get-Content 'C:\Users\qwe\AppData\Local\Temp\mc263_cp.txt' -Raw
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

- [ ] **隔离存档**：每项测试用独立 `world_dir` / 独立 seed，或先备份 `world/`；避免旧 `state-<seed>.redb`、`player-*.dat` 干扰。
- [ ] 客户端成功标志：进世界后无 `Timed out while waiting for the client to load chunks`、无 `DecoderException`；离线模式预期只有 authlib/Realms 401。
- [ ] 服务端成功标志：`enter_play ... view_radius=10 view_chunks=441` → `play join chunks sent ... count=441 elapsed_ms=...` → `play join sent ... view_sent=441` → `play keepalive echo` → `play move ... forwarded_to_ecs=true` → 跨区块 `play view move ... add=21 remove=21 sent_total=441`；断线同 tick `disconnect → despawn player → player removal complete online=0`。
- [ ] 默认 seed `123456789` 出生 `(369,73,381)`、`on_ground=true`；`seed=42` 出生 `(369,75,381)` 南向约 40 格有沙岸/池塘；`seed=-987654321` 出生 `(369,67,381)` 东侧约 25 格水域。
- [ ] 记录每次的 `play join chunks sent ... elapsed_ms`，用于 §11 性能红线。

---

## 1. 登录 / 区块 / 视距 / 压缩（P13/P15/P16.1）【必验】

- [ ] `view_distance=10`、默认 seed、`compression_threshold=-1`：441 块进服，无超时/Decoder，站在 y=73。截图：出生视野。
- [ ] 按住 W 跨区块：服务端多次 `play view move ... radius=10 add=21 remove=21 sent_total=441`，地形/树连续无空洞；按住 W 约 8 秒测节流（80ms/6 次合并为一批）。
- [ ] `view_distance=4`：`view_chunks=81`、跨区块 `add=9 remove=9`；`view_distance=2`：25 块。两者均正常进服。
- [ ] `compression_threshold=256`：441 块压缩进服无 DecoderException；Join 耗时记录（HANDOFF 历史 1464–1916ms）。
- [ ] 断线同一 tick `online=0`；立即重连只有 1 个实体；同进程缓存命中重连 `elapsed_ms` 应 <100ms（历史 72ms）。
- [ ] Join 附近日志中 `ready to join` 仅 1 条、`chunk data (...)` 脚手架 0 条、`tick lag` 0 条。

## 2. v1 地形 / 水 / 树 / biome（P16）【必验】

- [ ] 默认 seed：草地丘陵 + 大量 oak，树干站草上、树冠跨 chunk 连续；视距内无水面（正常）。
- [ ] `seed=42`：出生草甸+树，向南到沙岸 `(371.9,64,422.1)` 蓝色水面、沙岸贴合水面、无黑块/空洞；截图：出生 / 沙滩 / 树下池塘 / 跨 chunk 水面。
- [ ] `seed=-987654321`：东侧水域更大，树不悬空/不穿水。
- [ ] 地下洞穴可挖到；已知简化：已点亮 section 内洞穴偏亮、chunk 边界 biome 硬切（不算 bug）。

## 3. 方块交互与存档（P17）【必验】

- [ ] 挖脚下草：C2S `Player Action 0x29` START(0) 只 Ack 并出现裂纹动画，STOP(3) 才破坏 → 回 `BlockUpdate(air)+Ack(seq)`；松开鼠标 ABORT(2) 不破坏。
- [ ] 点下方泥土顶放石头：`Use Item On 0x42` → `BlockUpdate(stone)+Ack`；手持不同物品对应不同方块；空手/不可放物品只 Ack 不落子。
- [ ] 超距 / 出视距 / 非法 face / y 越界：应回滚（重发目标现状）+Ack，客户端不残留幽灵块。
- [ ] 圆石放置：手持圆石右键应落圆石且不消失、挖后返还圆石（P22 补丁）。
- [ ] 走远 11 区块再走回：重发的 spawn chunk 仍是编辑后的方块（缓存失效 + override 重编）。
- [ ] 断线当 tick `world saved ... on_disconnect=true`；杀进程重启，重进目标方块仍在。

## 4. 掉落 / 背包 / 手持 / Q 扔（P18/P19/P28）【必验】

- [ ] 挖草掉 dirt：先 `SpawnEntity(id=1000)+SetEntityData`，0.5s 后 `TakeItemEntity(collector=1)+RemoveEntities+全量 Content`；物品进背包槽且可见。
- [ ] `Set Held Item 0x36`（Short）切手持；放置按手持映射扣 1（单槽 `SetContainerSlot`）。
- [ ] 掉落物抛物线/落地：初速上抛不再原地蹦跶；落在挖掘点附近；挖掉掉落物下方方块时平滑落到新支撑面（P18.3）。
- [ ] 拾取延迟：挖掘掉落 10 ticks、手扔 40 ticks（约 2s）后才可拾取。
- [ ] Q 键：空手 Q 只 Ack 不断连；手持 Q 在眼位前方约 0.15 格生成、飞向视角方向（yaw/pitch 由 `PosRot 0x1F`/`Rot 0x20` 决定），轻掷落点约 1 格、站立可自捡；整组 Q（status 4）扔完整栈、单个 Q（status 5）扔 1 个。
- [ ] 背包开屏按 Q（ClickContainer THROW）：光标为空才生效，button 0=1 个 / 1=整栈，全量 Content + 刷盘；光标非空无操作。
- [ ] `rejoin_syncs_selected_held_slot`：选第 3 格 → 断线 → 重进仍选中第 3 格（S2C `0x6B=3`），Q 扔/放置不会用错格。
- [ ] 重启后背包内容（含拾取物）与手持选中槽都在。

## 5. 多人同屏（P21/P43/P47）【必验，双开客户端】

- [ ] A、B 同时在线：B 收到 A 的 `add_entity 0x01`（type=`minecraft:player`=159，唯一对外 id），A 也看到 B；**不给自己的 id=1 发 Spawn**。
- [ ] A 走动/转头（Pos/PosRot/Rot）：B 收到 `move_entity_pos 54`/`pos_rot 55`/`rot 57`，位置连续；出视距 Remove、回视距重新 Spawn。
- [ ] A 挖/放方块：B 收到 `BlockUpdate 0x08`，但**B 不应收到 Ack**（Ack 只回操作者）。
- [ ] A 扔东西/拾取：B 收到同 entity id 的 `Spawn+Data`、`TakeItem 0x7F + Remove`，不重复。
- [ ] A 断线：B 收到 `RemoveEntities 0x4E`，`online` 减 1；A 重连不残留幽灵实体。
- [ ] A 挖块时 B 看到裂纹动画 `Block Destruction 0x05`（START=0 / STOP=255）。
- [ ] 已知限制（不算失败）：玩家没有名字/皮肤/ tab 条目（`player_info_update 0x47` 未实现）；无挖掘音效。

## 6. 农业 / 种田闭环（P22/P26/P27/P33/P37/P39）【必验】

- [ ] 小麦：手持 wheat_seeds 对草/泥土/耕地顶面右键 → 落 0 阶段（6867）、扣 1 种子、BlockUpdate 广播；60s/阶段，耕地减半；成熟（6874）挖掉掉小麦 + 种子，拾取入库。
- [ ] 胡萝卜/马铃薯/甜菜：同样种下→生长→收获；马铃薯成熟有 1/100 毒马铃薯；甜菜成熟掉 beetroot + 种子可再种。
- [ ] 地狱疣：手持 nether_wart 只能种在**灵魂沙**，草/泥土/耕地右键无效（无落子）；4 阶段成熟掉 1~3。
- [ ] 西瓜/南瓜梗：种下→8 阶段→成熟邻格生成真实瓜/南瓜方块；挖瓜掉瓜片/南瓜；每株只结一次（MVP）。
- [ ] 甜浆果丛/火把花/猪笼草/可可：各自 age 推进；可可只能贴 jungle_log 侧面，挖掉支撑停止生长/自动摘除；猪笼草上下两格挖任一半连带另一半。
- [ ] 甘蔗/竹子/仙人掌：竖向堆叠（3/6/3 格上限），仙人掌只种沙；挖基部连根清空并逐段掉落。
- [ ] 其它 7 种树苗（桦/云杉/丛林/金合欢/黑橡木/樱花/红树）：种下约 6 分钟后长成对应材质小树，log/leaves 颜色正确；只做小树形（已知简化）。
- [ ] 骨粉：手持 bone_meal 右键作物推进 1~2 阶段并钳制成熟，成熟后右键不扣；右键树苗立即长成；服务端同步跟踪不跳变。
- [ ] 锄头耕地：任意锄头右键草/泥土顶面变 farmland（不扣、无耐久）；耕地生长加速；挖耕地掉泥土；挖土壤连带其上方作物一起 pop。
- [ ] 树叶 decay：砍原木后周围 3 格内孤叶约 5s 消失并掉落（1/200 苹果）；有原木支撑的树叶不消失。
- [ ] **跨重启计时（P39）**：种下/树苗/树叶观察后设 `generator_version=1` 正常存档，杀进程重启（不立即重进），等剩余时间后重进，阶段应已推进/树已长出/叶已 decay；而不是从 0 重计时。
- [ ] `world_storage=legacy` 模式下仍按旧行为从重进时刻重计时（回退验证）。

## 7. 经验球（P31）【必验】

- [ ] 挖煤矿/铁矿：除物品掉落外生成 `SpawnEntity` type=`minecraft:experience_orb`=50 + `SetEntityData index 8 VarInt`；拾取发 `TakeItemEntity(count=1)+Remove` 并收到 `SetExperience 0x69`。
- [ ] 经验条/等级随拾取变化，原版曲线（1→7、16→352、30→1395）。
- [ ] 断线/拾取后重进，经验值仍在（玩家存档 v2/v3 + redb players）。
- [ ] 已知简化（不算失败）：一矿一球不合并；无击杀/熔炼经验；无 7.25 格吸引。

## 8. 后台管理（P34/P35/P47）【必验】

- [ ] 打开 `http://127.0.0.1:25566/`：Vue 3 + Naive UI 暗色面板，5 个卡片正常渲染；**断网后仍能打开**（完全离线、零外部请求）；`node admin-ui/scripts/check-offline.mjs` 通过。
- [ ] `/api/online`：conn_id、用户名、地址、位置、在线秒数、`gamemode`、`tool_enchant`；人数与实际一致。
- [ ] `/api/world`：seed/online/override_count/dirty_chunks/drop_tick/world_dir/view_distance；`POST /api/save` 脏时 `saved:true`，不脏时 `saved:false`。
- [ ] `/api/items`：1658 项物品 + 中文名/分类；搜索/分类筛选、点击选中、`POST /api/give` 发放后在线玩家背包立即收到（无需重进）。
- [ ] `POST /api/kick`：目标客户端 TCP 断开、`online` 同 tick 减 1；`/api/inventory` 显示 46 槽与中文明细。
- [ ] 新端点：`GET /api/tools`（档位/可挖表）、`POST /api/tool-enchant`、`GET /api/recipes`（283 条）、`GET /api/craft9`（会话快照）、`POST /api/gamemode`（切创造/生存）——面板卡片与 API 联动生效。
- [ ] `POST /api/broadcast` 仍返回 501，面板只显示说明、不发包（故意）。

## 9. 生存 / 合成 / 工具 / 模式（P40/P41/P47）【必验】

- [ ] 新玩家满血 20 / 饥饿 20 / 饱和 5，`SetHealth 0x6A` 在需要时同步；长时间挂机饥饿下降、归零后扣血。
- [ ] 手持食物右键扣 1 并恢复饥饿/饱和；坠落 >3 格按每格 1 点扣血；死亡时背包全槽掉落、清空、满状态复活并回到出生点（用 `PlayerPositionSync 0x49`）。
- [ ] 已知简化（不算失败）：无完整 Respawn 0x54 死亡界面包、无饥饿动画/进食音效。
- [ ] 2×2 合成：槽 1–4 放料、0 出结果；试原木→木板、木板→木棍、3 小麦→面包、煤+木棍→火把；结果槽点击扣料并进光标/背包，全量 Content 同步。
- [ ] 283 条配方抽样：铁镐、箱子、床、各树种木板/工具/盔甲；输入相同但槽分布不同的歧义配方（如木棍/压力板）应按槽型得到确定结果。
- [ ] 3×3 工作台：放置 crafting_table 后右键开会话；SHIFT 结果槽从背包补料连续合成；会话仅内存（重进丢失是已知 MVP）。
- [ ] 工具等级：木镐挖铁矿/钻石矿应破坏但零掉落；铁镐正常掉；时运额外掉落；精准采集掉方块自身物品（无对应物品回退旧表）。后台 `/api/tool-enchant` 切换可实时生效。
- [ ] 游戏模式：后台切创造后 START 即破坏且零掉落、放置不扣手持；切回生存恢复扣物品；飞行/创造槽 0x39 未实现（不算失败）。

## 10. 新存储 redb（P36/P38/P39）【必验】

- [ ] `world_storage="redb"`（默认）启动：`redb world opened seed=... / world loaded storage=Redb`，`world/state-<seed>.redb` 生成。
- [ ] 挖/放后等 60s 或断线，重开进程与客户端：方块编辑仍在；`journal` 可重放（单测已覆盖，真机看状态即可）。
- [ ] 迁移：把旧 `overrides-<seed>.dat` / `player-*-<seed>.dat` 放进 `world_dir`，用 redb 启动 → 生成 `migration-<seed>.json`、旧档改名 `.bak`、数据可读；再启动不应重复迁移。
- [ ] 回退：改 `server.toml` 为 `world_storage="legacy"`、把 `.bak` 改回原名 → 旧档可读。
- [ ] `generator_version` 与 redb `meta.generator` 校验：v2 库用配置 1 启动应直接退出并报 `generator version mismatch`（不静默混用）。

## 11. worldgen v2（P44/P45/P46）【高风险，必验】

- [ ] **默认 v1 回归**：保持 `generator_version=1`、默认 seed，出生 y=73、3×3 干燥草地、水/树/biome 与基线截图一致；v1 应逐字节冻结。
- [ ] **v2 视觉**：全新 seed + 干净 `world_dir` + `generator_version=2`，截图出生视野：多树种（oak/birch/spruce/dark_oak）、草花、沙岸 gravel/sandstone/clay 过渡、高地 coarse_dirt/podzol/雪、新矿种类。
- [ ] v2 地形：河流连续、不穿出生保护圈；山脊/侵蚀形态可见；4×4 多 biome 边界与 chunk 接缝无明显硬切/空洞。
- [ ] v2 洞穴：形状不再是 4 格方块台阶，仍保 8 格地表壳；洞穴内光照不过亮（逐列天空光）。
- [ ] v2 流动水：出生圈外挖开水边容器，8 格内逐级灌水；堵源后下游退 air；挖开/堵源操作者收到 Ack，旁人收到 `BlockUpdate 0x08`；断线/重连/重启后 water level 仍在。
- [ ] v2 游泳：水中不触发坠落伤害；脚陷入支撑时位置修正不抽搐；另一玩家看到水中位置 y 变化。（动画/浮力手感差异记录即可）
- [ ] v2 block light：火把/灯笼/岩浆附近亮度正确，chunk 接缝无突变；遗迹内灯笼照明可见。
- [ ] v2 单 chunk 小遗迹：约 6% chunk 出现石砖小遗迹、5×5 足迹不跨 chunk、避出生圈；截图。
- [ ] **性能红线 ⚠️**：`generator_version=2`、r=10、441 块无压缩 Join 的 `elapsed_ms` **≤700ms**（本地估算约 1060ms，可能超标），包体 ≤~60KB；若超标记录实际值，回退到 v1 或降视距。
- [ ] `player_info_update 0x47` 名字/皮肤、`Respawn 0x54`、聊天仍缺，不计入失败；
      `open_screen` 工作台 GUI 已实现但默认关（`workbench_gui=false`），见 §13。

## 12. 失败定位速查

| 现象 | 先看哪里 |
|---|---|
| 卡在“加载地形中…”/30s 超时 | 服务端 `play join chunks sent` 是否发全；客户端是否缺 `GameEvent 13`/`ChunkBatchStart/Finished`；视距 clamp 2..=10 |
| `DecoderException` / 连接重置 | 服务端是否有猜包/改包体；v2 的 biome 64 项容器与非均匀 light 是高风险点，先回退 `generator_version=1` 对比 |
| 方块操作后客户端幽灵块/数不对 | `BlockUpdate` 是否回滚、Ack seq 是否对、手持物品映射（`Set Held Item` Short vs S2C VarInt）、圆石映射 |
| 掉落看不到/捡不到 | Spawn/Data 是否发、50ms 全局 ticker 是否跑、拾取延迟（10/40 ticks）、是否出视距、双开时 known 集 |
| 背包/经验重进丢失 | P51 起主键是稳定 UUID（`player-<uuid>-<seed>.dat` / redb UUID key），username 只是兼容镜像；检查 `.bak` 迁移报告 `uuid-migration-<seed>.json` |
| 种植后不生长/重启从头 | 全局 ticker 是否跑；`timers` 表与 `tick_offset` 是否恢复；legacy 模式本来就重计时 |
| 别人看不到我/看到幽灵玩家 | P43 `player_visibility` 是否开启、对外 id 是否唯一、视距进出 `RemoveEntities` 是否发 |
| 面板打不开/空白 | `admin_page.html` 是否被重新构建、`check-offline.mjs`、浏览器控制台是否有 CDN 请求（应为零） |
| Join 明显变慢 | 对比 v1 同 seed 的 `elapsed_ms`；v2 地形/洞穴/多 biome 开销；是否压缩阈值开启 |
| redb 启动退出 | `meta.generator` 与 `generator_version` 是否一致；旧库无 `|worldgen=` 后缀按 v1 解释 |

---

## 13. 本轮新增：身份 / ECS / 批次1 / GUI 合成（**新，必验**）

> 前置：用独立 seed 与 `world_dir`；`cargo build -p mc-server` 后运行。以下开关都在 `server.toml`。

**P52 socket→ECS（最高风险）**
- [ ] 启动日志 `socket_mode=ecs → redb opened → listening → global drop physics ticker started → keepalive`，无 panic。
- [ ] 改 `socket_mode="net"` 重启做 A/B：同 seed 同操作的 Join 八段/ChunkData、方块 `BlockUpdate→Ack` 包流应一致；
      后台 `/api/world.socket_mode` 只读显示正确。
- [ ] 双开：A 挖/放方块，A 收到的 `Ack(seq)` 紧邻 `BlockUpdate`，B 只收 `BlockUpdate`、零 Ack；视距切换先于方块包。
- [ ] 慢连接（限速/暂停客户端收发 30s）：观察服务端 RSS 是否持续上涨（**无限写队列**风险）、
      `SocketEcsRuntime::stats()` 的 queued_bytes、其它玩家 tick 是否正常；恢复后队列排空。
- [ ] 客户端无 `tick lag`，服务端 KeepAlive 稳定。

**P51 稳定 UUID**
- [ ] 同名玩家重进：背包/XP/生存状态保留；后台 `/api/online` 的 `uuid` 每次登录一致（离线 `OfflinePlayer:<name>` v3）。
- [ ] `/api/give`、`/api/inventory`、`/api/kick`、`/api/gamemode`、`/api/tool-enchant` 用 `uuid` 定位均生效，旧 username 仍可用。
- [ ] 迁移报告 `uuid-migration-<seed>.json` 存在、旧 `.dat` 变 `.bak`、重进不重复迁移（幂等）。

**批次 1（P53/P54）**
- [ ] `xp_merge=true`：地面经验球 7.25 格内被吸引、邻近合并，总经验不丢；捡取手感正常（§7）。
- [ ] `hunger_detail=true`：疾跑/跳跃/游泳后 `/api/online` 的 `exhaustion` 上升、饱和/饥饿按阈值下降；`POST /api/set-food` 生效。
- [ ] `settings_persist=true`：后台切创造/附魔 → 断线重进仍生效；`/api/online.persisted_*` 正确。
- [ ] `stats=true`：`GET /api/stats?uuid=` 能看到挖掘/放置/死亡计数按天累加，面板「统计」卡显示。
- [ ] `broadcast_throttle` 保持 **false**：双开高频挖放包流无异常；确认后再决定是否开（会改包序）。
- [ ] `GET|POST /api/time` 改白天/夜晚客户端天空光有变化；`GET|POST /api/weather` 只改变面板/服务端状态
      （`0x27` 未确认，**不应有天气包**）；`weather_broadcast=true` 也保持不发包。
- [ ] `POST /api/journal-trim` 在 redb 落盘后裁剪；`/api/world.journal_seq/bytes/oldest_kept` 合理。
- [ ] `craft_session_persist=true`：3×3 会话放原料 → 断线重进 → 服务端会话仍在内存（**客户端 GUI 暂不补发**）。

**P55/P56 GUI 与合成（写死常开，无开关）**
- [ ] 客户端右键工作台（或手持工作台放置）：弹出真 3×3 界面，不断线、无 `DecoderException`；截图。
      若仍断线：把 `debug/disconnect-*-client.txt` 的 `found N bytes extra` 发回来。
      （2026-11-27 已修一次：标题无名 tag，13 字节整包。）
- [ ] 3×3：格内放小麦 → 结果槽显示干草捆；结果 SHIFT 连续合成；背包格 SHIFT 优先补合成格。
- [ ] 2×2（背包按 E 的原生界面）：结果槽不能手放；SHIFT 连续合成产物入包；SWAP 进空快捷栏/副手；格 1–4 SHIFT 入包。
- [ ] 后台面板「工作台会话」卡显示"常开（右键工作台即开）"，无开关无按钮；
      `/api/workbench-gui` 与 `/api/screen-open` 已删除（应 404）。

**P60 vanilla 形状 + 拖拽（用户反馈三件套，必验）**
- [ ] 木棍=竖列 2 木板；横摆 2 木板→压力板；单格堆 2 木板→木按钮（不是木棍）。
- [ ] 门=两列 3 木板；散放 6 木板不出任何东西；两整行 6 块→活板门；1+2+3 台阶形→楼梯。
- [ ] 工具/床/箱子等 3 格宽配方在背包 2×2 里不出结果（须去工作台）。
- [ ] 拖一叠木板划过合成格：按格均分、光标清空，不再瞬间弹回；逐格离散放置同样正常。
- [ ] 双击同种物品收集到光标至满栈；中键/拖拽中键无操作（生存）。
- [ ] 结果 SHIFT 连做：锁定当前配方做到没料为止，不横跳到别的产物。

---

## 复验记录表（请按实际填写）

| 日期 | 服务端 commit | seed / 配置 | 项目 | 结果 | 备注/截图 |
|---|---|---|---|---|---|
| | `862c69e` | | §1 / §11 v2 Join 性能与解码 | | |
| | `862c69e` | | §5 / §13 P52 ECS A/B 与双开 Ack | | |
| | `862c69e` | | §13 P51 UUID + 批次1 + P55/P56 GUI 合成 | | |
| | | | | | |
