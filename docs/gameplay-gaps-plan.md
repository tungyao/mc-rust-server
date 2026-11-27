# 玩法缺口调研与实施路线（p49-gameplay-gaps）

> 分支 `p49-gameplay-gaps`（基于 main `064cbb0`），owner=`subagent-gap-research`，日期 2026-11-27。
>
> **本轮范围：调研 + 方案设计，不写生产代码、不改协议、不跑 cargo。** 唯一交付物是本文件。
> 不修改 `tasks/`、`HANDOFF.md`、`MEMORY.md`；不改任何 `.rs` / 前端源码 / `data/777/`。
>
> 版本锁定：`VERSION.md` → MC 26.3 / protocol 777 / data_version 5023 / `data/777/`。
> 包 ID 唯一数据源：`data/777/packets.json`（`clientbound.play` / `serverbound.play` 生成表）；
> 本文出现的每个 ID 都从该表核对过，并与 `docs/packet-map-777.md` §6–§8 交叉验证。
>
> **用户硬约束：任何后续玩法实现都必须与后台管理联动。** 因此本文每个缺口都写明
> 「后台 API 路径 + 面板卡片/控件 + 联动三件套」；只有服务端逻辑、没有后台可视/可控的
> 改动不视为完成。

---

## 0. 结论摘要（TL;DR）

1. **剩余缺口不是"基础生存没做"，而是"社交/身份/反馈/持久化"四块。**
   P18–P47 已把挖放、掉落、背包、Q 扔、全作物、经验球、redb、合成 283 条、
   工具等级、3×3 工作台、游戏模式、挖掘动画、多人互见、worldgen 三阶段全部落地。
   把它们当缺口重复做是最大的浪费（§1.1 逐条列了现状边界）。
2. **真正的 Top 5**：①聊天（`/api/broadcast` 仍是 501）②`player_info_update 0x47` 名字/皮肤/tab
   ③完整 `Respawn 0x54` 死亡界面 ④飞行/创造槽（`player_abilities 0x41` / `set_creative_mode_slot 0x39`）
   ⑤声音/粒子反馈（`sound 0x77` / `level_particles 0x30`）。详见 §2.2 与 §4。
3. **按"是否要抓包"分成三类是本项目最有用的分类**：A 类无需抓包即可做（纯服务端/已验证包）；
   B 类必须真机抓 26.3 包或反编译 jar 后才能动（不猜字节是仓库铁律）；
   C 类需要更大架构改造（socket→ECS、per-player 写通道、实体系统）才能做。
4. **真机复验是前置门禁，不是可选项。** `docs/real-machine-checklist.md` 12 组全部待用户执行；
   尤其 P44–P47（v2 worldgen 多 biome/非均匀 light、`open_screen`/飞行/0x47/0x54 未做）
   和 P38/P39 redb 计时续算、P40 生存/死亡。在未验证基础上叠加新功能会放大定位难度（§1.2）。
5. **批次建议 0+4**：批次 0 = 真机复验门禁（不写代码）；批次 1 = A 类无需抓包收尾；
   批次 2 = 抓包冲刺 + 社交/身份三件套；批次 3 = 创造/容器/反馈；批次 4 = C 类立项评估。
   每批独立交付、独立回退；B 类一律"先抓包 → 再实现 → 再真机"。

---

## 1. 现状边界

### 1.1 已实现能力（**一律不作为缺口**，来源 HANDOFF §5 最新状态）

| 能力 | HANDOFF 位置 | 真机清单组 | 备注（避免误判为缺口） |
|---|---|---|---|
| 登录/配置/视距 2..=10、压缩阈值、441 块单批 Join、动态补卸区块（80ms 节流）、断线同 tick 清理 | §3 / §5.1–5.3 / §5.7 | §1 | `0x32/0x60/0x2E/0x49/0x2D/0x1C` 已按 777 修正；`0x26` 是 packed i64 |
| 挖/放服务端权威判定、`BlockUpdate 0x08` + `Ack 0x04`、超距/出视距回滚、手持映射（含圆石）、空手只 Ack | §3 / §5.5 / §5.22 | §3 | C2S `0x29/0x42/0x43/0x2E`；START 只 ack、STOP 才破坏 |
| 掉落物/背包 46 槽、拾取延迟 10/40、合并/重力/支撑重算、`TakeItem 0x7F`、全量/单槽同步、存档 | §3 / §5.10–5.13 | §4 | 全服唯一 50ms ticker（R1 已修）；背包键是用户名 |
| Q 扔 + 背包开屏 Q（status 4/5、THROW=4）、按视角飞（`0x1F`/`0x20` 已接）、选中槽重进同步 `0x6B` | §5.16 / §5.21 / §5.23 | §4 | 轻掷 0.06 是有意口径，非缺口 |
| 多人方块/掉落广播 `BroadcastHub`、玩家实体互见 P43（独立 id、known 集、移动 54/55/57、断线 Remove） | §5.17 / §5.30 | §5 | **挖掘动画 `0x05` 已由 P47 实现**（§5.31）；HANDOFF §5.9/§5.17 旧文字"0x05 仍缺"已过时 |
| 全作物：小麦/胡萝卜/马铃薯/甜菜/地狱疣/瓜/南瓜/浆果/火把花/猪笼草/可可/甘蔗/竹子/仙人掌/9 种树苗、耕地/骨粉/树叶 decay、跨重启续算 | §5.18–5.20 / §5.24 / §5.27 | §6 | P33/P37 已合并，模块 `crops/{mod,growth,drops}.rs`；不要重列 |
| 经验球 P31（type=50、曲线、拾取 `0x69`、存档 v2/v3、redb `players`） | §5.26 | §7 | 合并/7.25 吸引才是缺口 |
| 新存储 redb（默认）：`blocks`/`journal`/`players`/`timers`/`meta`，迁移、60s+断线刷盘、generator 版本守卫、计时续算 | §5.27 | §10 | `world_storage="redb"` 默认；legacy 仍可回退 |
| 2×2 合成 + **283 条配方** + 3×3 工作台会话（SHIFT 连续合成）+ 生存（血量/饥饿/饱和/坠落/死亡掉落/复活） | §5.28 / §5.30 / §5.31 | §8 / §9 | 锄头配方已随 P41 的 35 工具补齐；骨粉/苹果仍无配方（走 `/api/give`），这是配方数据缺口不是合成系统缺口 |
| 工具等级/时运/精准采集、游戏模式（后台切换）、工具/配方/工作台/模式后台 API 与面板卡片 | §5.31 | §8 / §9 | 用连接级开关，不做 NBT 附魔/耐久/速度 |
| 后台面板 Vue 3 + Naive UI 单文件、**完全离线**（零外网请求）、5s/2s 轮询、`check-offline.mjs` | §5.25 | §8 | 7 个 API + P47 5 个新端点均在 `admin.rs::route()` |
| worldgen v1 冻结 + v2 三阶段（矿/材质/树种 → 侵蚀河流/洞穴/多 biome/光照 → 流动水/游泳/block light/遗迹） | §5.29（P44/P45/P46） | §11 | **另见 `docs/worldgen-plan.md`，本文件不重复其方案** |

### 1.2 已实现但**待真机复验**（不是缺口，但禁止在未验基础上叠加）

`docs/real-machine-checklist.md`（对应 main `76f645a`，509 passed）列出 12 组人工验证，全部尚未由用户执行：

- §1 登录/视距/压缩/断线；§2 v1 地形水树 biome；§3 挖放存档；§4 掉落/背包/Q 扔/选中槽；
- §5 多人同屏（A/B 双开，含 P47 `0x05` 裂纹动画）；§6 农业全作物 + P39 跨重启计时；
- §7 经验球；§8 后台 P34/P35/P47；§9 生存/合成/工具/模式；§10 redb；§11 v2 worldgen；§12 失败定位表。

**高风险门禁（必须在动 B/C 类前完成）**：

1. v2 441 块无压缩 Join `elapsed_ms` ≤700ms（本地估算 1060ms，**可能超标**）；
2. v2 64 项 biome 容器 / 非均匀 sky+block light 无 vanilla 抓包对照（`DecoderException` 风险）；
3. P40 死亡复活路径（当前是 `0x49` + `SetHealth` 自研流程，不是 `Respawn 0x54`）；
4. P47 `open_screen` 未发、飞行未做、`0x39` 未解析、`0x47` 未发；
5. redb `timers` 跨重启续算与 v2 `meta.generator` 校验。

### 1.3 本文件不涉及的范围

- **地形/worldgen**：三阶段已落地，后续任何地形改动走 `docs/worldgen-plan.md` 的版本化流程
  （v1 冻结、v2 仅新 seed、cache 全失效），本文件只引用不重述。
- **存档格式选型**：已定 redb（不做 Anvil 兼容），见 `docs/world-storage-plan.md`。
- **协议表维护**：`docs/packet-map-777.md` 是 ID 权威；本文只引用 ID，不改协议代码/数据。

### 1.4 真正剩余缺口（总览）

归入 §2 三类表：A 类 8 项（无需抓包）、B 类 15 项（必须抓包/数据）、C 类 5 项（架构改造）。
其中 C 类含"socket→ECS"前置，它是 B 类中涉及实体/载具/AI 的隐藏依赖。

---

## 2. 缺口清单表

### 2.0 类别图例

- **A｜无需抓包即可做**：包 ID/布局已在 `data/777/packets.json` + 已验证实现中，或纯服务端逻辑/纯内存。
- **B｜必须真机抓 777 包后才能做**：仓库铁律"bitmask/NBT/组件布局没有抓包验证就不猜字节"。
  来源：26.3 客户端 jar 反编译 + minecraft.wiki 26.3 章节 + 真机抓包对照，三者取交集。
- **C｜需要更大架构改造**：socket→ECS、per-player 写通道、实体/AI/物理系统、全服 ticking 区域。

工作量为单人日量级估计（小 ≤1d / 中 1–3d / 大 >3d 或需立项）。

### 2.1 A 类：无需抓包即可做

| ID | 缺口 | 玩家价值 | 依赖 | 工作量 | 风险 | 前置任务 | 后台联动（简述） |
|---|---|---|---|---|---|---|---|
| A1 | 经验球合并 + 7.25 格吸引（§5.26 剩余） | 中：地面不再一堆球，捡取手感接近原版 | 纯 `mc-world::xp` 池逻辑；复用 `0x01/0x65/0x7F/0x69`，无新包 | 小 | 低：吸引会改变拾取时序，需 e2e 覆盖 | 真机清单 §7 先确认渲染/拾取正常 | `/api/online` 加 `xp` 列；面板在线表 + 「经验」卡 `POST /api/xp` |
| A2 | 饥饿细节：疾跑/跳跃/游泳额外消耗、食物饱和差异（§5.28 剩余） | 中：生存更真实 | 纯 `mc-world::survival` + 全局 ticker；无新包 | 小 | 低：数值平衡需真机体感 | 清单 §9 生存组先过 | `/api/online` 加 `health/food/saturation`；面板「生存状态」卡 |
| A3 | 大视距广播风暴限流/合并（§5.17 剩余） | 中：多人高频挖放不卡顿 | 纯 `BroadcastHub` 队列：每连接按 tick 合并 `BlockUpdate`/`section_blocks_update 0x56` | 中 | 中：合并会改变包序，需保留 Ack 只回操作者 | 清单 §5 双开先确认当前包流 | `/api/world` 加 `broadcast_queue/dropped`；面板世界状态卡加指标 |
| A4 | 后台设置跨重连持久化（gamemode / tool_enchant） | 中：后台切了模式，重进却回生存，运维困惑 | `HubConn`/`DropState::tool_enchants` 只在内存；需 redb `players` 新列（schema v2→v3 兼容读） | 小 | 低：旧档 v1/v2 读兼容已有先例 | redb 清单 §10 先过 | `/api/online` 增 `persisted_*`；「在线玩家」卡下拉写入即落盘 |
| A5 | journal 裁剪/运维 + net 调用点 actor 标注（§5.27 剩余） | 低（运维向）：防止库无限膨胀、定位谁改了方块 | 纯 `mc-world::store`；`set_with_context` API 已有，当前 actor=0 | 小 | 低 | 无（可与 A 类并行） | `/api/world` 加 `journal_seq/size`；`POST /api/journal-trim`；面板「存档运维」卡 |
| A6 | 时间/天气（昼夜 + 晴/雨/雷暴） | 中高：世界"活"起来，夜间光照有对比 | `set_time 0x73`（wiki 已验证）、`game_event 0x27` 天气类型值需真机确认；**不抓包可以先做服务端状态 + 后台显示** | 小 | 低-中：天气事件 type 值若猜错客户端无效果（不断线） | 清单 §2/§11 先确认天空光现状 | `GET/POST /api/time`、`GET/POST /api/weather`；面板「世界状态」卡加时间滑块/天气下拉 |
| A7 | 统计本地累积 + 后台展示（不发 `award_stats`） | 低-中：后台能看到挖掘/放置/死亡计数 | 纯 `redb` 新表 `stats` + 全局 ticker 累加；不涉及客户端包 | 小 | 低 | 无 | `GET /api/stats`；面板「统计」卡（只读） |
| A8 | 工作台会话/容器内容**纯服务端**跨重连保留 | 中：3×3 会话重进丢失的已知 MVP 限制 | 复用 `0x12/0x14` 容器包；新增 redb `block_entities` 表；**不上 `0x06` 就不需抓包** | 中 | 中：会话与背包槽映射要一致（0/1–9/10–45） | 清单 §9 工作台会话先过 | `/api/craft9` 增 `persisted/owner`；面板「工作台会话」卡加"已落盘"标识 |

### 2.2 B 类：必须真机抓 777 包后才能做

| ID | 缺口 | 玩家价值 | 依赖（抓包/数据） | 工作量 | 风险 | 前置任务 | 后台联动（简述） |
|---|---|---|---|---|---|---|---|
| B1 | 聊天：C2S `chat 0x09` / `chat_command 0x07`；S2C `player_chat 0x42` / `system_chat 0x7c`（p32 blocked） | **高**：社交基础，且 `/api/broadcast` 当前 501 无法真广播 | **必抓**：聊天包 signed message/UUID/timestamp 布局、玩家名组件；`system_chat` 的样式组件 NBT | 中 | 中：猜字节会错位 TCP 流；离线模式签名策略要定 | 抓包 → 清单 §1 基线再跑 | **实现 `POST /api/broadcast`（去 501）**；面板「广播」卡从说明改为输入框+发送；可选 `GET /api/chatlog` |
| B2 | `player_info_update 0x47` 名字/皮肤/tab + `player_info_remove 0x46` / `tab_list 0x7d` | **高**：多人能认出谁是谁，G 列表正常（§5.30 明确"无抓包不猜 bitmask"） | **必抓**：0x47 的 bitmask 各动作顺序、属性字段（name/skin/textures signature）；皮肤需 Mojang 会话或离线默认 | 中 | 中：bitmask 顺序错会导致 tab 条目缺失/客户端异常 | 抓包；清单 §5 双开基线 | `/api/online` 加 `display_name/skin/tab_listed`；面板在线表增列 |
| B3 | 完整 `Respawn 0x54` 死亡界面（当前是自研 `0x49`+`SetHealth` 复活） | **高**：死亡/重生界面、维度切换语义正确 | **必抓**：`0x54` 十余字段（dimension_type、hashed seed、gamemode、death location、portal cooldown 等） | 中 | 中：字段顺序错直接断线；需与登录期注册表 ID 自洽 | 抓包；清单 §9 死亡流程先记录现状 | `/api/online` 加 `deaths/last_death_at`；面板「生存状态」卡加死亡记录 |
| B4 | 飞行 / `player_abilities 0x41`（CB+SB） | 中高：创造模式核心操作 | **必抓/反编译**：0x41 的 flags bitmask 与飞行速度字段（§5.31"包体未验证"） | 小-中 | 中：flags 位错会复制出错误能力（如无敌/瞬移） | 抓包；清单 §9 创造模式先过 | `/api/online` 加 `flying/abilities`；面板在线表加「飞行」开关 → `POST /api/flight` |
| B5 | `set_creative_mode_slot 0x39`（C2S）创造槽写入 | 中：创造模式真正可用（当前只 `/api/give` 进生存背包） | **必抓 + data component codec**：Slot 带 component 需注册表驱动解码（§5.5"误解析错位整条 TCP 流"） | 中 | **高**：解错直接整条流错位断线 | 抓包 + 组件解码护栏（失败忽略不断线） | 复用 `/api/give` 走 0x39 下发；面板「发放物品」卡加"写入创造槽"勾选项 |
| B6 | `open_screen 0x3c` 客户端 GUI（工作台/箱子真界面） | 中高：3×3 会话当前无 GUI，只能靠背包界面猜 | **必抓/数据**：26.3 无 menu type 生成表，P47 明确"不猜字节未发" | 中 | 中 | 抓包；清单 §9 工作台会话先过 | `/api/craft9` 加 `screen_open`；面板「工作台会话」卡加"打开 GUI"按钮 |
| B7 | 工具耐久 / 挖掘速度（§5.31 明确未做） | 中：生存工具闭环 | **必抓 + 组件**：耐久要 item data component（客户端耐久条）；速度需 `update_attributes 0x86`（mining_speed） | 大 | 高：组件编码 + 属性包布局双重风险，且会动背包 Slot 线格式 | 抓包 + B5 组件方案先定 | `/api/tools` 加 durability；面板「工具等级」卡加耐久条 + `POST /api/repair` |
| B8 | 声音/粒子反馈：`sound 0x77` / `level_particles 0x30` / `block_event 0x07` | 中高："挖/放/吃/拾取有反馈"，观感提升最大 | **必抓/数据**：sound/particle 注册表 ID 不在简化 `data/777/*.json`；布局需 wiki+抓包 | 中 | 中：ID 错只是静默无声，不断线 | 抓包；清单 §3/§4 先过 | `POST /api/effect-demo`；面板「反馈测试」卡（选音效/粒子→对在线玩家发） |
| B9 | 方块实体网络同步 `block_entity_data 0x06` + 箱子/熔炉容器 | 中高：箱子能存东西、打开有界面 | **必抓**：0x06 的 NBT/block entity type 布局；配合 A8 的 `block_entities` 表 | 中-大 | 中-高：NBT 编码错会断线 | 抓包 + A8 先落盘 | `GET /api/block-entities` + `GET /api/inventory?container=`；面板「容器快照」卡 |
| B10 | 熔炼（熔炉 block entity + 配方 + 燃料/进度） | 中高：铁矿→铁锭、食物烹饪（P41 未覆盖熔炼产物） | **必抓 + 数据**：熔炉 NBT、smelting recipe 表不在 283 条合成配方里；XP 发放可复用 `0x69` | 大 | 高：块实体 + tick + 存档 + 配方数据四线并行 | B9 + A8 先做；抓包 | `GET /api/furnaces`、`POST /api/furnace-set`；面板「熔炼」卡 |
| B11 | 真附魔（item data component / 附魔台 GUI） | 中：当前只有连接级 none/fortune/silk_touch 服务端开关，非真实物品附魔 | **必抓 + 组件**：附魔 NBT/components、附魔台菜单 | 大 | 高：改背包 Slot 线格式影响所有背包链路 | B5 组件方案 + 抓包 | `/api/enchant` 替换/并存 `tool-enchant`；面板工具卡加附魔下拉 |
| B12 | 药水/状态效果：`update_mob_effect 0x87` / `remove_mob_effect 0x4f` | 中：药水、速度/夜视等 | **必抓 + 组件**：effect 布局 + potion contents 组件 | 大 | 高 | B5 组件方案 + 抓包 | `GET /api/effects`、`POST /api/effect`；面板「状态效果」卡 |
| B13 | 成就/统计**发包**：`award_stats 0x03` / `update_advancements 0x85` | 低-中：A7 的本地计数推到客户端 | **必抓/数据**：stat 注册表 ID、advancement 定义数据不在 `data/777` | 中 | 中 | A7 先落盘；抓包 | `/api/stats`、`/api/advancements`；面板「统计/成就」卡加"推送客户端" |
| B14 | 交易：`merchant_offers 0x35` / C2S `select_trade 0x34` | 低-中：缺村民体系，价值依赖 B15/C2 | **必抓**：offers NBT；村民实体 AI（C 类） | 大 | 高 | C2 生物实体先做；抓包 | `GET /api/trades`、`POST /api/trade-open`；面板「交易」卡 |
| B15 | 命令体系：`commands 0x10` / `chat_command 0x07` / `command_suggestions 0x0f` | 中：运维/玩家命令入口 | **必抓 + 桥接**：brigadier 树 NBT；命令执行要跨 net 线程进权威状态（C 类） | 大 | 高 | socket→ECS 或 admin 指令通道；抓包 | `POST /api/command`（admin 独立通道，不借 chat 伪造）；面板「命令」卡 |

### 2.3 C 类：需要更大架构改造（socket→ECS 等）

| ID | 缺口 | 玩家价值 | 依赖（架构） | 工作量 | 风险 | 前置任务 | 后台联动（简述） |
|---|---|---|---|---|---|---|---|
| C1 | **socket→ECS**（per-player 写通道 + `ChunkView`） | 基础设施：很多玩法（实体/AI/命令）的前提 | 把 net 线程的 socket 写权移入 ECS；`decide_*` 权威判定与写通道同线程；**不要恢复 P16.1 删除的全量区块生成脚手架** | 大 | **高**：动 Join 包序会破全部真机基线 | 清单 §1 全绿；A/B 类先交付 | `/api/world` 加 `socket_mode=net|ecs`；面板「世界状态」卡只读展示 |
| C2 | 生物 AI + 战斗（怪物/动物、攻击 `0x01`、`damage_event 0x19`、`hurt_animation 0x2b`、`animate 0x02`） | 高：生存核心体验缺口 | C1 + 实体系统（位置/血量/AI 状态）+ 全局实体 ticker + AI 行为数据（不在 `data/777`） | 大（立项） | 高 | C1；抓包；B8 音效可先铺 | `GET /api/mobs`、`POST /api/mob-spawn`、`POST /api/mob-kill`；面板「实体」卡 |
| C3 | 载具（船/矿车）：`move_vehicle 0x3a` / `set_passengers 0x6d` / C2S `move_vehicle 0x22` / `paddle_boat 0x23` | 低-中：依赖水/轨道场景 | C1 + 物理/乘客关系 + 实体状态持久化 | 大（立项） | 高 | C2 实体系统；抓包 | `GET /api/vehicles`、`POST /api/vehicle-spawn`；面板「载具」卡 |
| C4 | 红石（信号传播、活塞/门/灯、ticking area） | 中：工程玩家刚需，但生态链很长 | C1 + 全服 ticking 区域 + `block_entities` 表 + B9 `0x06` + `block_event 0x07` | 大（立项） | 高：tick 风暴会影响 50ms 红线 | B9/A8 + 抓包 | `GET /api/redstone`、`POST /api/redstone-toggle`；面板「红石」卡 |
| C5 | 村庄/完整结构/战利品表 | 低（本阶段）：worldgen-plan 已明确不做 | jigsaw/loot NBT 不在 `data/777`；跨 chunk 生成 + 容器存档 | ≥5 人日 | 高 | `docs/worldgen-plan.md` §3.7/§6 决策 | 不建议本轮做；无联动点 |

---

## 3. 每个缺口的实现思路

### 3.0 统一约定（所有缺口都遵守）

**包 ID / 布局来源**：一律从 `data/777/packets.json` 编译期查表（模式：`packet_id("clientbound.play","xxx")`），
不手写新数字；字段顺序取 minecraft.wiki 26.3 对应章节 + 26.3 jar `javap` 反编译 + 真机抓包三者交叉。
**没有抓包验证的 bitmask / NBT / data component 一律不猜、不发**（§5.30/§5.31 铁律）。
特别注意方向同名不同义：`0x36` S2C=`move_entity_pos` / C2S=`set_carried_item`；
`0x39` S2C=`move_entity_rot` / C2S=`set_creative_mode_slot`；`0x12` S2C=`container_set_content` / C2S=`container_click`。

**状态放哪**：

- **per-conn（内存，断线即丢）**：gamemode / tool_enchant / 飞行 flags / 广播 known 集 / 工作台会话 /
  `last_pos`+`last_look` / 客户端序列。现有位置：`BroadcastHub::HubConn`（`broadcast.rs:66-77`）、
  `DropState::tool_enchants`（`drop_state.rs:148`）、`login_route::craft9` 会话表。
  需要跨重连的字段才落 `players`（A4）。
- **全局唯一（内存 + ticker）**：掉落/XP/作物/树苗/decay/生存/流体/实体 AI —— 必须挂在
  `serve()` 启动的**唯一** 50ms ticker（`DropState::start_global_tick` / `physics_tick`，§5.13 R1）。
  各连接的 `tick_drops` **只做拾取/视距同步，绝不推进时间**；N 连接推进 N 倍速是已修过的 R1，不得重犯。
  新增 tick 驱动功能（A2/A6/C2/C4）复用同一个 `tick_count`，redb 模式从 `meta.tick_offset` 恢复。
- **实体/世界**：方块编辑走 `MutableWorld` 稀疏 override + `ChunkCache::invalidate(pos)`；
  实体（生物/载具）需要新增全局实体池（C 类），不能塞进 per-conn。

**存档位置**：

| 数据 | 位置 | 迁移要求 |
|---|---|---|
| 背包 46 槽 / selected / health/food/sat / XP | redb `players`（schema v1→v2→v3 兼容读；legacy `.dat` v1/v2/v3） | 新字段只追加旧档补默认值 |
| 方块编辑 | redb `blocks` + `journal`（seed 隔离，generator 版本守卫） | v1 冻结，不自动迁移基底 |
| 生长/树苗/decay/fluid 计时 | redb `timers`（`kind+pos+planted_tick+stage+spec`） | 新计时种类加 tag，不改旧格式 |
| 工作台/箱子/熔炉内容 | redb **新表 `block_entities`**（按 packed pos 键） | A8 先建表，B9/B10 复用 |
| 统计/成就/效果/实体 | redb **新表**（`stats`/`effects`/`entities`） | 均需 schema 版本位，旧库缺省为空 |
| 时间/天气 | `meta` 新字段或 `world_state` 表 | 缺省 = 白天/晴，保持旧包流不变 |

**后台联动三件套（缺一不算完成）**：

1. `crates/mc-server/src/main.rs`：构造 `AdminState`（当前字段 `hub/drops/mutable/seed/world_dir/view_distance`）
   增加新状态的 `Arc`；admin 线程独立端口 25566，**不得在 `play_loop` 热路径持锁等待后台**。
2. `crates/mc-server/src/admin.rs::route()`：`match (method, path)` 新增加 GET/POST 分支；
   写操作通过 `BroadcastHub` 队列/channel 投递（借鉴 `ADMIN_KICK_PACKET_ID=-1` 的信号模式），
   不手写未知包；返回 JSON 沿用 `{ ok, error }` 约定。
3. `admin-ui/src/Dashboard.js`：新增 `<n-card>` + `jget/jpost` 控件；`npm ci && npm run build` 重建
   `crates/mc-server/src/admin_page.html` 离线单文件，`node admin-ui/scripts/check-offline.mjs` 必须零外链。
4. 回归：1 个 admin_e2e（新端点）+ 旧 3 个 admin_e2e 一字不改。

**回退通则**：每个能力一个 `NetConfig::with_xxx(bool)` opt-in（已有 `with_player_visibility` /
`with_destroy_stage` / `with_tool_rules` / `with_fluid` 先例），生产入口开、旧 e2e 默认关；
关掉后包流逐字节回退到基线。new redb 表只增不删，旧库打开时缺表视为空。

---

### 3.1 A 类逐项方案

**A1 经验球合并 + 7.25 吸引**
- 包：`0x01`(type=50) / `0x65`(index 8 VarInt) / `0x7F` / `0x69`，全部已验证，无新包。
- 状态：`XpOrbEntities` 全局池（`xp.rs`），吸引半径与合并上限做 `NetConfig` 常量。
- tick：唯一 50ms ticker；吸引只在全局 tick 做，per-conn 只发 `0x23/0x4E/0x7F`。
- 存档：`players` XP 字段不变（合并只改实体池，不改总数）。
- 后台：`/api/online` 加 `xp`；面板在线表「经验」列 + 「经验发放」卡 `POST /api/xp`。

**A2 饥饿细节**
- 包：`SetHealth 0x6a`（已验证），无新包。
- 状态：per-conn `SurvivalState`（`survival.rs`，已随背包落盘），新增 exhaustion 浮点字段。
- tick：唯一 50ms ticker；`play_loop` 从移动包推代价 sprint/jump 标记给对应 per-conn 状态。
- 存档：`players` 追加 exhaustion 字段（v3/v4 兼容读）。
- 后台：`/api/online` 加 `health/food/saturation/exhaustion`，面板「生存状态」卡只读展示 + 调试按钮
  `POST /api/set-food`（便于复验）。

**A3 广播风暴限流**
- 包：复用 `block_update 0x08`；高频多格变更再考虑 `section_blocks_update 0x56`（已查表，未用过，需 wire format 对照 wiki）。
- 状态：`BroadcastHub` per-conn `VecDeque<QueuedPacket>`；按 tick 合并同 chunk 同 section 的更新。
- tick：在 `play_loop` 的 50ms 读超时分支 `drain_outbox`，**Ack `0x04` 永远直连操作者不合并/不广播**。
- 存档：无。
- 后台：`/api/world` 加 `broadcast_queue/broadcast_dropped/merged_blocks`，面板世界状态卡加三个统计量 + 合并阈值滑杆 `POST /api/broadcast-throttle`（纯服务端参数，无包风险）。

**A4 后台设置跨重连持久化**
- 包：无新包；`0x6A`/`0x41`（飞行另见 B4）在 Join 时按落盘状态补发。
- 状态：`HubConn.gamemode` / `DropState::tool_enchants` 为权威内存，登录时以 `players` 值为初值。
- tick：无；纯登录/后台事件。
- 存档：`players` 加 `gamemode`/`tool_enchant` 列（v2→v3 兼容读，旧档默认 survival/none）。
- 后台：现有 `/api/gamemode`、`/api/tool-enchant` 写入即落盘；`/api/online` 增 `persisted= true`。
  面板在线表下拉保持不变，提示改为"重连后仍生效"。

**A5 journal 裁剪/运维**
- 包：无。
- 状态：`WorldStore` 内；`set_with_context` 已提供，net 调用点补 actor（conn_id）。
- tick：裁剪走后台触发/60s autosave，不占 50ms ticker 关键路径。
- 存档：`journal` 表按 seq 保留窗口，裁剪前确认 `blocks` 已落盘。
- 后台：`/api/world` 加 `journal_seq/journal_bytes/oldest_kept`；`POST /api/journal-trim`；面板「存档运维」卡。

**A6 时间/天气**
- 包：`set_time 0x73`（wiki 26.3：`Long + Bool`）、`game_event 0x27`（天气事件 type 值**真机确认后再发**，
  当前只用了 13=LEVEL_CHUNKS_LOAD_START）；夜间天空光已由 v2 逐列实现，无需新包。
- 状态：全局 `WorldTimeState { tick, weather, weather_until }`（Arc 注入 NetConfig + AdminState）。
- tick：唯一 50ms ticker 推进 tick；天气到期在 ticker 内切换并 hub 广播 `0x27`。
- 存档：`meta` 加 `world_time/weather`（缺省白天/晴）；天气随机种子从 seed 派生。
- 后台：`GET/POST /api/time`（支持"设为白天/夜晚/自定义 tick"）、`GET/POST /api/weather`；
  面板「世界状态」卡加时间滑块 + 晴/雨/雷暴下拉。

**A7 统计本地累积**
- 包：无（A/B 分离：本项只服务端计数，B13 才推送 `award_stats`）。
- 状态：per-conn 计数随 `players` 落盘；不做全局排行。
- tick：挖掘/放置/死亡事件在已有 `PlayEvent::BlockEdit` / 死亡路径累加，不需新 ticker。
- 存档：redb 新表 `stats`（key = username+stat_id，value = u64）。
- 后台：`GET /api/stats?username=`；面板「统计」卡（表格只读）+ 时间范围筛选。

**A8 工作台会话纯服务端落盘**
- 包：复用 `container_set_content 0x12` / `container_set_slot 0x14`（window id 待 B6 GUI 时统一分配）。
- 状态：per-conn 会话表（`craft9.rs` 内存）在断开时写盘，重连 Join 时恢复；**不引入 `0x06` 就无需抓包**。
- tick：无。
- 存档：redb 新表 `block_entities`（key = 逻辑容器 id 或 packed pos，value = 10 槽快照）。
- 后台：`/api/craft9` 增 `persisted/owner/last_saved`；面板「工作台会话」卡加"已落盘/恢复"状态与"清空会话"按钮。

---

### 3.2 B 类逐项方案（**先抓包，再实现**）

> 共同前置：建立一个 `data/777/captures/` 抓包目录与 wiki/jar 三方对照记录（仿 `packet-map-777.md` 体例），
> 每个 B 项补齐后才允许写对应 `mc-protocol` 布局；所有新包先 opt-in 关，真机验证后再在生产入口打开。

**B1 聊天**
- 包：C2S `chat 0x09` / `chat_command 0x07`；S2C `player_chat 0x42` / `system_chat 0x7c`。
  `data/777/packets.json` 已确认 ID；**布局必抓**（signed message body / UUID / timestamp / component NBT）。
- 状态：per-conn 聊天节流计数（内存）；聊天日志环形缓冲（全局内存，可选落盘）。
- tick：无；net 线程收到 `0x09` 后经 hub fan-out 给视距内或全服（按设计决定），不走 ECS ticker。
- 存档：可选 `chatlog` 表（默认关）。
- 后台：**把 `POST /api/broadcast` 从 501 变为真发 `system_chat 0x7c`**；面板「广播」卡改输入框+发送按钮+
  最近 10 条日志；`GET /api/chatlog`（可关）。

**B2 `player_info_update 0x47` 名字/皮肤/tab**
- 包：`0x47`（update）/ `0x46`（remove）/ `tab_list 0x7d`（ID 已查表）；**bitmask 与动作顺序必抓**。
- 状态：per-conn 身份快照（`BroadcastHub::PlayerEntityState` 已存 uuid/pos/yaw/pitch，扩 `name/skin_hash`）。
- tick：Join 时发 ADD_PLAYER + 移动/换装时发 UPDATE；断线发 REMOVE；与 P43 的 `0x01/0x4E` 同节拍。
- 存档：皮肤纹理签名不落盘（离线模式用默认 skin）；名字即登录用户名。
- 后台：`/api/online` 加 `display_name/skin_source/tab_listed`；面板在线表增列，并加"刷新 tab"按钮
  `POST /api/tab-refresh`（触发重发 `0x47`）。

**B3 完整 Respawn 0x54**
- 包：`respawn 0x54`（ID 已查表）；十余字段布局**必抓**；与 P40 现有 `0x49`+`SetHealth` 复活路径并存，
  由 `NetConfig::with_full_respawn(bool)` 切换。
- 状态：per-conn 死亡上下文（death dim/pos/角度）在死亡时生成，发完即清。
- tick：死亡事件在唯一 ticker 判定（坠落/饥饿/战斗伤害统一入口），发包在 net 线程。
- 存档：`players` 的 health/food/sat 已存在；新增 `deaths` 计数与 `last_death` 位置字段。
- 后台：`/api/online` 加 `deaths/last_death_at`；面板「生存状态」卡加死亡次数/最近死亡坐标 + "测试死亡"按钮（调试开关）。

**B4 飞行 `player_abilities 0x41`**
- 包：CB/SB `0x41`（ID 已查表）；flags bitmask + fly speed **必抓/反编译**。
- 状态：per-conn `HubConn.abilities`（与 gamemode 联动）；SB 回包在 net 线程更新内存。
- tick：无；切换即时发包。
- 存档：随 A4 的 `players.gamemode` 一并落 `abilities` 字段。
- 后台：`/api/online` 加 `flying/fly_speed`；`POST /api/flight {username,on}`；面板在线表加飞行开关。

**B5 创造槽 `set_creative_mode_slot 0x39`**
- 包：C2S `0x39`（ID 已查表）；**Slot data component 解码是硬门槛**（当前 `0x12` 遇 component 直接报错防错位）。
  先实现"带 component 时安全跳过字段而非断流"，再解析创造槽写入。
- 状态：per-conn 9 格创造槽（`DropState` 新增 map），与生存背包隔离。
- tick：无；C2S 即时写入，Join 时按落盘状态补发 S2C 容器包。
- 存档：`players` 增 `creative_slots`（可选，默认关）。
- 后台：`/api/give` 增 `target=creative|survival`；面板「发放物品」卡加目标下拉。

**B6 `open_screen 0x3c` GUI**
- 包：`0x3c`（ID 已查表）；menu type 与附加 NBT **必抓**（P47 因无生成表未发）。
- 状态：per-conn `open_container` id，与 A8 会话/容器内容绑定。
- tick：无。
- 存档：A8/B9 提供内容，GUI 本身不落盘。
- 后台：`/api/craft9` 加 `screen_open`；`POST /api/screen-open`；面板「工作台会话」卡加"打开 GUI"按钮。

**B7 工具耐久/挖掘速度**
- 包：Slot data component（耐久条）+ `update_attributes 0x86`（mining speed）；**两者布局必抓**。
- 状态：per-conn 背包槽组件（随 `players` 落盘）；服务端持有耐久表，客户端靠 component 显示。
- tick：挖掘 START→STOP 的进度按服务端速度计时（仍只在全局 ticker 判定，别在 per-conn 各自算）。
- 存档：`players` 背包条目扩组件字节（v4 兼容读）。
- 后台：`/api/tools` 加 `durability/mining_speed/tier`；面板「工具等级」卡加耐久条 + `POST /api/repair`。

**B8 声音/粒子**
- 包：`sound 0x77` / `sound_entity 0x76` / `level_particles 0x30` / `block_event 0x07`（ID 已查表）。
  sound/particle registry ID 需从 26.3 jar 抽取成表（不在简化 `data/777/*.json`），**布局必抓**。
- 状态：无持久状态；事件触发即 hub 广播给视距内连接。
- tick：由已有挖掘/放置/拾取/进食事件触发；周期性环境音（若有）走全局 ticker。
- 存档：无。
- 后台：`POST /api/effect-demo {sound|particle,pos,username}`；面板「反馈测试」卡。

**B9 方块实体 `block_entity_data 0x06` + 箱子**
- 包：`0x06`（ID 已查表）NBT 布局**必抓**；容器内容复用 `0x12/0x14`。
- 状态：per-conn 打开会话（指向全局 `block_entities` 池）；内容为全局实体状态。
- tick：无（箱子无 tick）；熔炉在 B10 才需要 tick。
- 存档：A8 建的 `block_entities` 表。
- 后台：`GET /api/block-entities?pos=`；面板「容器快照」卡（按坐标查 46/27 槽）。

**B10 熔炼**
- 包：B9 的 `0x06` + `container_set_data 0x13`（进度条）+ `0x12/0x14`；熔炼配方数据需从 26.3 jar/数据生成器导出（不在 283 条里）。
- 状态：全局 `FurnaceState` 池（pos→输入/燃料/输出/进度/经验），per-conn 只持有"正在看哪个熔炉"。
- tick：**唯一 50ms ticker**推进所有熔炉，完成发 `0x13` + XP（`0x69`，复用 P31 曲线）；连接侧绝不 tick。
- 存档：`block_entities` 熔炉条目 + `timers` 进度；XP 给玩家仍写 `players`。
- 后台：`GET /api/furnaces`、`POST /api/furnace-set {pos,slot,item}`；面板「熔炼」卡（进度条 + 投料按钮）。

**B11 真附魔**
- 包：Slot component（附魔列表）+ 附魔台 `open_screen 0x3c`（B6）；布局必抓。
- 状态：`players` 背包条目组件字节；服务端 `tools.rs` 的 `ToolEnchant` 升级为从组件读，保留连接级开关作调试回退。
- tick：无。
- 存档：`players` 组件字段（v4 兼容读）。
- 后台：`POST /api/enchant {username,slot,enchant,level}`；面板「工具等级」卡加附魔下拉，旧 `tool-enchant` 端点标记"调试覆盖"。

**B12 药水/状态效果**
- 包：`update_mob_effect 0x87` / `remove_mob_effect 0x4f`（ID 已查表）；potion contents 组件 + effect 布局必抓。
- 状态：per-conn 效果表（剩余 tick/等级），随 `players` 落盘。
- tick：效果倒计时挂**唯一 50ms ticker**；到期发 `0x4f`。
- 存档：`players` 增 `effects` 字段或 redb `effects` 表。
- 后台：`GET /api/effects`、`POST /api/effect {username,effect,duration,level}` / `POST /api/effect-clear`；
  面板「状态效果」卡（列表 + 添加/清除按钮）。

**B13 统计/成就发包**
- 包：`award_stats 0x03` / `update_advancements 0x85`（ID 已查表）；stat ID 与 advancement 定义数据需导出/抓包。
- 状态：A7 的 `stats` 表为真相源，Join 时推送增量。
- tick：无（事件驱动）。
- 存档：`stats` 表 + 新 `advancements` 表。
- 后台：`GET /api/stats`、`GET /api/advancements`；面板「统计/成就」卡加"推送客户端"按钮。

**B14 交易**
- 包：`merchant_offers 0x35` / C2S `select_trade 0x34`（ID 已查表）；offers NBT 必抓。
- 状态：per-conn 打开的村民会话（引用 C2 实体）；村民库存全局。
- tick：交易次数上限随全局 ticker 窗口重置。
- 存档：`block_entities`/新 `villagers` 表。
- 后台：`GET /api/trades`、`POST /api/trade-open`；面板「交易」卡（只读 offers + 强制刷新）。

**B15 命令体系**
- 包：`commands 0x10` / `chat_command 0x07` / `command_suggestions 0x0f`（ID 已查表）；brigadier 树 NBT 必抓。
- 状态：命令注册表全局只读；执行上下文 per-conn。
- tick：命令执行通过 channel 进权威线程（与 admin `ADMIN_KICK_PACKET_ID` 同模式），**不直接改 socket**。
- 存档：无（命令审计可选 `chatlog`）。
- 后台：`POST /api/command`（admin 独立通道，不经聊天包伪造，避免签名/日志污染）；面板「命令」卡（输入框 + 返回输出）。

---

### 3.3 C 类逐项方案（本轮只记录，不实施）

**C1 socket→ECS**
- 包：无新包，但会动 Join/Chunk/PositionSync 的发送位置——必须逐字节回归 `play_stream.rs`。
- 状态：per-player 写通道 + `ChunkView`；权威判定与写通道同线程；ECS 只读共享 `Arc<MutableWorld>`。
- tick：主循环 50ms；全局物理 ticker 保持独立，不得按连接数倍增。
- 存档：无。
- 后台：`/api/world` 加 `socket_mode` 只读字段，面板世界状态卡显示，便于灰度对比。
- 前置：清单 §1 全绿；这是一切实体/AI/命令类玩法的隐藏前置。

**C2 生物 AI + 战斗**
- 包：`add_entity 0x01`（entity type 查 `data/777/entities.json`，161 项）、`set_entity_data 0x65`、
  `animate 0x02`、`hurt_animation 0x2b`、`damage_event 0x19`、`entity_event 0x22`、C2S `attack 0x01`（均需抓包验证布局）。
- 状态：新增**全局实体池**（位置/朝向/血量/AI 状态/target），存 redb `entities` 表；per-viewer known 集复用 P43 思路。
- tick：全局唯一 50ms ticker 推进 AI/寻路/攻击冷却；与 P18.1 R1 同防线，连接侧绝不推进。
- 存档：`entities` 表（pos/hp/type/AI 状态）。
- 后台：`GET /api/mobs`、`POST /api/mob-spawn`、`POST /api/mob-kill`；面板「实体」卡（列表/坐标/血量 + 生成/清除按钮）。

**C3 载具**
- 包：`move_vehicle 0x3a` / `set_passengers 0x6d` / C2S `move_vehicle 0x22` / `paddle_boat 0x23`（需抓包）。
- 状态：全局实体池 + 乘客关系；物理状态放实体池。
- tick：全局 ticker 推进物理，广播 `0x3a` 给视距内。
- 存档：`entities` 表（含 passengers）。
- 后台：`GET /api/vehicles`、`POST /api/vehicle-spawn`；面板「载具」卡。

**C4 红石**
- 包：`block_entity_data 0x06`（B9）/ `block_event 0x07` / `block_update 0x08`。
- 状态：全局红石网络（源/强度/传播队列）+ `block_entities` 表。
- tick：全服 ticking 区域，必须合并同 tick 更新并设上限，避免 50ms 红线被冲垮。
- 存档：`block_entities` + `timers`。
- 后台：`GET /api/redstone`、`POST /api/redstone-toggle`；面板「红石」卡。

**C5 村庄/完整结构**：不建议本轮做（见 §5.1）；无联动点。

---

## 4. 推荐批次

> 原则：**真机门禁 → 无需抓包 → 抓包后社交 → 创造/容器 → 大架构立项**。
> 每批可独立交付、独立回退；批次之间不共享未验证的新包。

### 批次 0：真机复验门禁（**不写生产代码**）

- **做什么**：用户按 `docs/real-machine-checklist.md` §1–§12 跑完，重点记录 v2 Join `elapsed_ms`、
  biome/light 包体解码、P40 死亡、P47 后台/创造/工作台、redb 计时续算。
- **交付**：填写清单末尾记录表；任何失败项先定位修复，不在其上叠加。
- **回退**：无需回退（无代码改动）。
- **门禁判定**：§1/§9/§11 全绿才进批次 2+；B 类抓包可与批次 0/1 **并行**（抓包不改代码）。

### 批次 1：A 类无需抓包收尾（低风险，可与批次 0 并行开发）

- **做什么**：A1 经验球合并/吸引、A2 饥饿细节、A3 广播限流、A4 设置持久化、A5 journal 运维、
  A6 时间/天气（天气事件值待确认，先上时间）、A7 统计计数、A8 工作台会话落盘。
- **玩家/运维价值**：手感 + 运维可观测性立刻提升；后台面板立刻新增 4–5 个可视卡片。
- **工作量**：约 4–6 人日（8 项 × 小~中）。
- **独立交付/回退**：每项一个 `with_xxx(bool)` opt-in；A8 只建 `block_entities` 表不改包流；
  A6 天气若事件值不对则只发 `set_time`，回退关天气开关。
- **验收**：清单 §4/§7/§8/§9 回归 + 每项的 admin_e2e + `check-offline.mjs`。

### 批次 2：抓包冲刺 + 社交/身份三件套（**Top 1–3**）

- **做什么**：一次抓齐 B1/B2/B3 的包体 → 实现聊天（含 `/api/broadcast` 去 501）、
  `player_info_update 0x47` 名字/皮肤/tab、完整 `Respawn 0x54`。
- **玩家价值**：最高——从"能在同一世界挖放"变成"能认人、能聊天、能正常死亡重生"。
- **工作量**：抓包 1–2d + 实现 4–6d。
- **独立交付/回退**：三个能力分别 opt-in；聊天失败只关聊天、`0x47` 失败只关玩家信息包、
  `0x54` 失败退回 P40 的 `0x49`+`SetHealth`。**禁止在三者都未抓包时猜字节开工**。
- **前置**：批次 0 的 §5 多人同屏 + §9 死亡基线；抓包对照表入 `docs/packet-map-777.md`（由主会话决定归属）。

### 批次 3：创造/容器/反馈（中风险，依赖抓包）

- **做什么**：B4 飞行 + B5 创造槽 + B6 `open_screen` + B8 声音/粒子 + B9 方块实体/箱子 + A8 会话接 GUI。
  B7 耐久/速度、B11 附魔、B12 药水视组件方案成熟度决定是否拆到批次 4。
- **玩家价值**：创造模式完整、容器可持久、交互有声音粒子反馈。
- **工作量**：约 6–10 人日。
- **独立交付/回退**：B8 反馈最先做（无声无副作用，关掉即回退）；B5 创造槽必须有"解码失败忽略不断线"护栏；
  B9 依赖 `block_entities` 表（A8 已建）。
- **前置**：B4/B6 抓包；B5 的 data component 解码方案先定；清单 §9 创造模式先过。

### 批次 4：大架构立项评估（**本轮只评估，不排期实现**）

- **做什么**：先评估 C1 socket→ECS 的成本与风险；C2 生物 AI/战斗、C4 红石、C3 载具、
  B10 熔炼、B14 交易、B15 命令作为后续独立任务单独立项。
- **玩家价值**：高（生存/工程玩家核心），但投入产出不确定。
- **独立交付/回退**：C1 本身就是灰度项（`socket_mode`）；C2/C4 需新实体池/红石网络，
  任何一项都必须能在 50ms ticker 上限流并一键关闭。
- **前置**：批次 0–3 全绿 + 用户明确批准"新增全局实体系统 / 改 socket 归属 / 接受大版本改造"。

### 批次依赖图（文字版）

```
批次0（真机门禁） ─┬─> 批次1（A 类，可并行开发）
                   └─> B 类抓包（并行，不改代码） ─> 批次2（聊天/0x47/0x54）
                                                   └─> 批次3（飞行/创造槽/GUI/音效/容器）
                                                        └─> 批次4（C1 socket→ECS → C2/C3/C4）
```

---

## 5. 明确不建议做的 + 需用户决策的问题

### 5.1 明确不建议做（本轮/可预见范围）

| 项 | 理由 | 替代方案 |
|---|---|---|
| 重复实现 P18–P47 已有能力（挖放/掉落/全作物/经验球/283 配方/工具等级/3×3 工作台/挖掘动画/玩家互见/worldgen 三阶段） | 已完成并合入，重做会破坏既有包流与存档 | 直接按清单 §1.1 真机复验 |
| 村庄 / 完整结构 / jigsaw / loot table（C5） | 缺 jigsaw/loot/block entity NBT 数据，跨 chunk 生成 + 容器存档成本高（≥5 人日，`docs/worldgen-plan.md` §3.7 已判定不做） | 保留 v2 单 chunk 小遗迹，足够观感 |
| 真 Anvil 读写/导出 | 用户已定"不做 Anvil 兼容"，redb 是磁盘真相源（`docs/world-storage-plan.md`） | 保持自有稀疏格式 + redb 迁移 |
| 改同一 seed 的 v1 地形基底 | 会让 `overrides`/`state-<seed>.redb` 的稀疏差异语义漂移（`docs/worldgen-plan.md` §2.2/§5.3） | v1 冻结，v2 仅新 seed |
| 猜 `0x47` bitmask / `0x54` / `0x39` Slot component / `open_screen` menu type 的字节 | 仓库铁律，猜错会断线或"错得悄无声息"；P13/P17 已有惨痛先例 | 先抓包 + jar 反编译 + wiki 三方核对 |
| 用 per-conn ticker 推进掉落/作物/生存/熔炉/AI | P18.1 R1 已修过 N 连接 N 倍速 | 全部挂 `serve()` 唯一 50ms ticker |
| 工具耐久/真附魔/药水（B7/B11/B12）**现在做** | 需改背包 Slot data component 线格式，影响所有背包链路，投入产出比低 | 先保留连接级 `tool-enchant` 开关；等 B5 组件方案稳定 |
| 完整附魔台 GUI / 战斗 / 交易 / 命令 **现在做** | 依赖实体系统 + socket→ECS（C 类），不是"补个包"能解决 | 批次 4 立项 |
| 把广播 Ack `0x04` 也塞进 BroadcastHub | 客户端序列预测是 per-connection 的，广播会错位（§5.17） | Ack 永远只回操作者本人 |
| 后台线程直接同步写 socket / 在 `play_loop` 持等后台锁 | admin 是独立端口 25566，故障隔离是设计目标 | 走 `BroadcastHub` 队列/channel 信号（仿 `ADMIN_KICK_PACKET_ID=-1`） |

### 5.2 需用户决策的问题列表

1. **真机复验谁执行、何时？** 批次 0 是硬门禁。用户是否在 Windows 客户端按清单跑完并提供日志/截图？
   若 v2 Join 超过 700ms 或 biome/light 解码失败，是否接受"回退 v1 默认值、v2 仅实验 seed"？
2. **是否解除 `p32-broadcast-chat` 的 blocked 状态？** 聊天是 Top 1，但涉及内容审核/日志留存策略
   （离线无签名是否允许纯文本广播？是否把 `/api/broadcast` 当作管理员系统消息专用通道？）。
3. **玩家身份方案**：离线模式每次 uuid 随机（§5.10 背包用用户名键）。`0x47` 的名字/皮肤是
   "显示用户名 + 默认离线皮肤"即可，还是要引入 UUID 稳定化/皮肤服务？后者会动存档键策略。
4. **死亡流程是否替换？** 保留 P40 自研 `0x49`+`SetHealth`，还是抓包后切 `Respawn 0x54`？
   两者共存时后台 `/api/gamemode`/调试按钮走哪条？
5. **创造模式范围**：只补飞行 + 创造槽写入（B4/B5），还是连 `open_screen` GUI、耐久/速度一起做？
   后者要动背包组件线格式，风险高。
6. **时间/天气是否需要持久与随机？** 是否默认走 seed 派生的昼夜循环（会影响现有存档/真机截图基线），
   还是仅后台手动切换、重启回白天？
7. **社交包广播范围**：聊天/音效/粒子是全服还是视距内？全服简单但大世界包量高；
   视距内需复用 `BroadcastHub`（已有视距快照）。
8. **`data/777/captures/` 与抓包文档的归属**：抓包对照表写进 `docs/packet-map-777.md`（由主会话维护），
   还是新增独立文档由本任务线维护？（本子代理不改既有文档，需主会话指定。）
9. **是否批准 socket→ECS（C1）立项？** 这是 C2/C3/C4/B15 的隐藏前置；若不批准，
   这些缺口只能停在"调研"状态，后续只做 A/B 类中不依赖实体系统的部分。
10. **管理员设置持久化粒度**：gamemode/tool_enchant 落 `players` 后，是否允许后台修改离线玩家
    （需要新增按用户名的离线档案编辑 API 与权限边界）？
11. **统计/成就是否推客户端**：只做后台可见（A7，无包风险）还是必须 `award_stats`/`update_advancements`
    （B13，要 stat 注册表与成就定义数据）？
12. **是否投入 data component 解码基础设施？** B5/B7/B11/B12 共用，做一次可解四个缺口；
    但会改背包 Slot 线格式，需评估是否值得单独立项。

---

## 6. 调研来源索引

| 文件 | 本文件用途 |
|---|---|
| `HANDOFF.md` §5（尤其 §5.1/5.10/5.13/5.17/5.20/5.26–5.32） | 现状边界、已实现清单、明确未做项、R1/不猜字节铁律 |
| `docs/real-machine-checklist.md` | 12 组复验门禁、高风险项（v2 多 biome/非均匀 light、P47 未做项） |
| `docs/worldgen-plan.md` | 仅用于确认"三阶段已落地、后续不改基底"，不重复方案 |
| `docs/packet-map-777.md` §6–§8 | 已验证包 ID/布局、不猜字节原则、P43/P47 未做项 |
| `data/777/packets.json` | 全部新包 ID 的编译期查表来源（本文逐个核对） |
| `crates/mc-server/src/admin.rs::route()` | 现有 13 个 API 路径（7 GET + 6 POST，含 501 的 `/api/broadcast`）、AdminState 字段 |
| `crates/mc-server/src/main.rs` | ticker/redb/hub/fluid/NetConfig opt-in 的接线位置 |
| `admin-ui/src/Dashboard.js` | 现有 5+卡片与 `jget/jpost` 调用模式，新增卡片的落点 |
| `crates/mc-network/src/broadcast.rs` | `HubConn`（per-conn 状态）、`ADMIN_KICK_PACKET_ID` 信号模式 |
| `crates/mc-network/src/drop_state.rs` | `tool_enchants` per-conn 状态、背包/ XP / timer IO 入口 |
| `crates/mc-world/src/store.rs` | redb `meta/blocks/journal/players/timers` schema 与迁移 |
