# 数据驱动化改造计划（借鉴 / 复制 Pumpkin 的数据与 codegen，协议可随之变更）

> 状态：**计划中**（2026-11-25）。用户已明确：**许可证无所谓，可以直接复制代码**。
> 因此策略从"只借鉴思路"升级为"复制数据资产 + 复制/移植 codegen 思路"，但**游戏逻辑仍以自研为主**
> （Pumpkin 没有世界生成，物品/方块行为绑定它自己的 Bevy 架构，搬过来也不能直接用）。
> 边界：codegen 解决 **ID / 注册表数据**；**包体布局变化仍需手改 `mc-protocol`**（Pumpkin 自己的
> porting 流程第 5 步也是 "Compiler Fixes: Update packet layouts, codecs..."），无法纯数据化。

## 0. 已验证的事实（2026-11-25 下载 Pumpkin master 实测）

数据源：`https://raw.githubusercontent.com/Pumpkin-MC/Pumpkin/master/assets/*.json`（临时下载到
`/tmp/opencode/pumpkin-*.json`，未入仓库）。这些是 Pumpkin 的 26.3 Extractor 从真实游戏提取的数据。

| 资产 | 大小 | 内容 | 与本项目关系 |
|---|---|---|---|
| `packets.json` | 5.5KB | `{"version":777, clientbound/serverbound: {state: {name: id}}}` | 包 ID 全量真值 |
| `blocks.json` | 8.2MB | 1286 方块：`id` / `default_state_id` / `states[]` / hardness / properties | blockstate ID 真值 |
| `items.json` | 1MB | 按名字索引：`{"stone":{"id":1,"components":{...}},...}` | item ID + 组件真值 |
| `entities.json` | 240KB | 实体类型 / 元数据 | entity ID 真值 |
| `properties.json` | 14KB | 方块属性枚举（facing / powered…） | 属性变体真值 |
| `tracked_data.json` | 209KB | 实体 tracked data | 元数据索引真值 |

**逐项对照结论（全部一致，可直接替换硬编码）**：
- 包 ID：S2C `add_entity=0x01`、`block_changed_ack=0x04`、`block_update=0x08`、
  `chunk_batch_start/finished=0x0C/0x0B`、`level_chunk_with_light=0x2E`、`forget_level_chunk=0x26`、
  `set_chunk_cache_center=0x60`、`player_position=0x49`、`entity_position_sync=0x23`、
  `set_entity_data=0x65`、`move_entity_pos=0x36`、`remove_entities=0x4E`、`take_item_entity=0x7F`、
  `container_set_content/slot=0x12/0x14`；C2S `player_action=0x29`、`use_item_on=0x42`、
  `use_item=0x43`、`punch=0x2E`、`set_carried_item=0x36`（我们叫 SetHeldItem）、
  `container_click=0x12`、`move_*=0x1E/0x1F/0x20/0x21`、`keep_alive=0x1C` —— 全部吻合。
  （注：Pumpkin 的 `teleport_entity=0x80` 是另一个包；我们 `0x23` 实为 `entity_position_sync`，
  包体线性路径正确，只是常量名历史遗留叫 Teleport，可顺手改名。）
- 方块 `default_state_id`：air 0 / stone 1 / grass **9** / dirt **10** / water **89** / sand **121** /
  oak_log **140** / oak_leaves **294** / coal_ore **136** / iron_ore **134** —— 与 HANDOFF §3 实测一致。
- 物品 ID：stone 1 / dirt 55 / cobblestone 62 / sapling 77 / sand 88 / oak_log 163 / coal 1010 /
  stick 1060 / raw_iron 1017 —— 全部一致。

## 1. 现状痛点

| 现状 | 问题 |
|---|---|
| `mc-registry` 216 项方块表是 **build.rs 生成的连续占位 ID** | 不是真实 26.3 ID；HANDOFF §5.8 标为旧数据 |
| `mc-world/src/lib.rs` 硬编码 10 个 blockstate ID、`drops.rs` 9 个 item ID、`play_entity.rs` 3 个 entity ID | 真机实测值散落多处，升级时逐个手改、易漏 |
| 包 ID 常量散落在 `play_block.rs` / `play_entity.rs` / `play_chunk.rs` / `play_sync.rs` | 版本升级时逐文件审计 |
| 33 个抓包 bin 已是数据，但路径写死 `vanilla_config_777/` | 换版本要改路径和源码里的数组 |

## 2. 分阶段计划

### P0 — 数据资产入库（~0.5 天）
- 建 `data/777/`（协议版本目录），把 Pumpkin 的 5 个 JSON **复制入库**（用户已同意复制）：
  `packets.json` / `blocks.json` / `items.json` / `entities.json` / `properties.json`。
  - 8.2MB 的 `blocks.json` 若嫌大，可用脚本裁剪为「name → default_state_id」精简表（~50KB），
    保留完整版作为升级时的真值来源（可放 `data/777/full/` 或按 .gitignore 排除）。
- 建 `data/manifest.json`：`{ version: "26.3", protocol: 777, data_version: 5023, data_dir: "777" }`。
- 把现有 33 个抓包 bin 纳入 manifest 索引（可暂留原路径，P3 再迁）。
- 加校验测试：上表所有 ID 从 JSON 解析后 == 当前硬编码值（先不改调用方，作为回归护栏）。
- 验收：`data/` 可直接被 build.rs 读取；校验测试全绿。

### P1 — build.rs codegen（`mc-registry`，~1 天）
- 复制/移植 Pumpkin `tools/pumpkin-codegen` 的思路（自己写 Rust build.rs，MIT 无所谓）：
  读 `data/777/*.json` → 生成 `OUT_DIR/generated.rs`。
- 生成 `BLOCKS` / `ITEMS` / `ENTITIES` / `PACKETS` 表 + `by_name` / `by_id`；
  **替换 216 项占位表**为真实 26.3 表。
- API 保持：`block_name` / `block_id` / `vanilla_blocks` 不变；新增 `item_id` / `entity_id` / `packet_id`。
- `cargo:rerun-if-changed=data/` 精确声明。
- 验收：`cargo test -p mc-registry`；`block_id("minecraft:stone")==1`、`item_id("minecraft:dirt")==55`。

### P2 — 消费方接入（~0.5 天）
- `mc-world/src/lib.rs` block ID 常量改为 `mc_registry::block_id(...)` 派生的编译期常量（保留常量名）。
- `mc-world/src/drops.rs` item ID、`mc-protocol/src/packets/play_entity.rs` entity ID 同理。
- 保留硬编码常量作为 `#[cfg(test)]` 对照断言，防止 codegen 漂移。
- 验收：`cargo test --workspace` 全绿；抓包 fixture 逐字节不变。

### P3 — 协议层版本化（~1 天）
- 包 ID 从生成的 `PACKETS` 表派生（或保留常量但从表生成 + 断言）。
- 顺手把 `0x23` 常量由 `PLAY_TELEPORT_ENTITY_S2C_ID` 改名为
  `PLAY_ENTITY_POSITION_SYNC_S2C_ID`（语义正确，包体不变）。
- `vanilla_config_777.rs` 改为按 manifest 索引 bin（文件名不再写死 32 项）。
- 版本切换 = 换 `data/manifest.json` 指向 + 适配布局差异；数据目录按版本隔离。
- 验收：配置阶段抓包对比测试不变；切到同版本副本能编译。

### P4 — 升级流程文档化（~0.5 天）
- `docs/data-driven.md`：从 Pumpkin/Extractor 拉数据 → 更新 `data/777/` → `cargo build` 重跑
  codegen → 补包体布局差异 → 跑 fixture 回归。
- `VERSION.md` 记录版本 ↔ 数据目录映射；HANDOFF §5.8 标记已解决。
- 验收：照文档在测试副本走一遍升级流程（无需真机）。

## 3. 明确不做的事

- **不搬 Pumpkin 的游戏逻辑**：它没有世界生成；方块/物品行为绑定它的 Bevy 架构，
  复制过来也不能直接用。自研 `WorldGen` / `drops` / `inventory` 判定逻辑保持原样。
- **不自动化包体布局**：字段顺序、新增字段仍需按版本手改并有测试覆盖。
- **不追求完整注册表**：P1 用完整 blocks.json 生成表，但游戏逻辑只用到 ~20 种方块。

## 4. 风险与缓解

| 风险 | 缓解 |
|---|---|
| Pumpkin master 数据对应版本可能不是精确 777（release 名 `26.3-26.51`） | P0 校验测试逐项比对 HANDOFF 实测值；不一致则以实测为准并记录差异 |
| 8.2MB JSON 入库体积 | 精简表 + 完整版按需； Manifest 记录来源 SHA |
| codegen 路径/编译期依赖问题 | build.rs 只读本地 data 目录；`cargo:rerun-if-changed` 精确声明 |
| ID 来源替换后 fixture 漂移 | 每阶段跑 fixture 逐字节对比；新增生成值断言 |

## 5. 验收标准（整体）

1. `cargo check --workspace --tests` / `cargo test --workspace` 全绿；
2. 所有抓包 fixture（LoginPlay / PositionSync / Configuration 32 包 + UpdateTags）逐字节不变；
3. `mc-registry` 不再有连续占位假 ID；
4. 新增一块方块/物品只需改 `data/777/*.json` 并重新编译，不改游戏逻辑源码；
5. 文档可复现升级流程。

## 6. 排期与当前进度

- 正在收尾 §5.11：节流子代理已实现（80ms 窗口 / 6 次变化上限，挖掘前强制 flush），
  审查子代理另报 **R1 高危：多连接共享 `DropState` 导致 `items.tick()` 被每连接各调一次**，
  建议先修 R1 再推进数据驱动。
- 之后按 P0 → P1 → P2 → P3 → P4 推进；P0/P1 可合一个提交，P2/P3 各一个，P4 随文档提交。
