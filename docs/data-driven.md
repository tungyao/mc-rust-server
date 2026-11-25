# 数据驱动：如何升级到新 MC 版本

> 本文是 `docs/data-driven-plan.md` P4 的交付物，描述**已实现**的升级入口，供照着走一遍。
> 项目策略是**只跟最新版、整体切换**，不做多版本兼容（见 `VERSION.md`）；codegen 只解决 ID，
> **包体布局仍需手改 `mc-protocol`**，游戏逻辑（`WorldGen` / `drops` / `inventory`）仍自研。

## 1. 数据源：Pumpkin Extractor JSON

真实 ID 的唯一数据源是 Pumpkin-MC/Pumpkin `assets/*.json`——由 **Pumpkin 26.3 Extractor**
从真实游戏提取的注册表数据（当前入库日期 2026-11-25）。上游 URL 与 **SHA-256 记录位置**是
`data/777/SOURCE.md`，升级时在新版本目录里照它的格式新建 `SOURCE.md`。

| 上游文件（`assets/`） | 本地入库 | 用途 / 处理方式 |
|---|---|---|
| `packets.json` | `data/777/packets.json` | 包 ID 全量真值，原样复制（5,564 B） |
| `blocks.json` | `data/777/blocks.json` + `data/777/full/blocks.json` | 方块/blockstate ID 真值，精简表 + 完整上游原文件 |
| `items.json` | `data/777/items.json` + `data/777/full/items.json` | 物品 ID 真值，精简表 + 完整上游原文件 |
| `entities.json` | `data/777/entities.json` + `data/777/full/entities.json` | 实体类型 ID 真值，精简表 + 完整上游原文件 |
| `properties.json` | `data/777/properties.json` | 方块属性枚举，原样复制（14,126 B）；当前**不参与 codegen**，但 build 时校验其存在且为非空数组 |

上游基址：`https://raw.githubusercontent.com/Pumpkin-MC/Pumpkin/master/assets/`。
`master` 会移动，所以必须**记录下载时的 URL 与 SHA**；SHA 以 `full/` 下的上游原文件为准。

⚠️ **blockstate ID 必须取 `default_state_id`**，不是 `states[0].id`，也不是方块种类序号 `id`。
实测：grass_block `id=8` / `default_state_id=9`，water 37→89、sand 39→121、oak_log 51→140、
oak_leaves 94→294。`crates/mc-registry/build.rs` 按此字段读取，取错会导致整个世界方块错位。

## 2. 目录约定与 manifest

数据按协议版本隔离：`data/<protocol>/`（当前 `data/777/`）。版本选择由
`data/manifest.json` 一处完成，目前内容：

| 字段 | 当前值 | 含义 |
|---|---|---|
| `version` | `"26.3"` | MC 版本号（字符串），用于 Handshake/Status 与版本识别 |
| `protocol` | `777` | 协议号；同时是数据目录名 `data/777/` |
| `data_version` | `5023` | 存档/NBT 的 data version |
| `data_dir` | `"777"` | 相对 `data/` 的版本目录名，必须是简单相对目录（build.rs 拒绝 `.` / `..` / 空段） |
| `vanilla_config` | `{"registries":[…],"update_tags":"update_tags.bin"}` | 抓包索引：`registries` 是 32 项**有序**数组（每项 `name` + `file`），`update_tags` 是 UpdateTags bin 文件名；`mc-protocol/build.rs` 据此生成 Configuration 阶段包体索引 |

目录内容约定（`data/777/` 为样板）：

- `blocks.json` / `items.json` / `entities.json` / `packets.json` / `properties.json`：codegen 读取的精简表/原样数据；
- `full/`：上游原始 JSON，用于重新生成精简表或查完整字段（方块属性、状态变体、物品组件等）；
- `SOURCE.md`：来源 URL、SHA-256、精简规则、ID 校验清单；
- `login_play.bin` / `player_position_sync.bin` / `update_tags.bin`：真机抓包 fixture；
- `vanilla_config/registry_00.bin` … `registry_31.bin` + `update_tags.bin`：Configuration 阶段 32 个注册表包体。

抓包 bin 的索引已归入 `data/manifest.json` 的 `vanilla_config`：`registries` 是 32 个 registry bin
的**有序索引**（每项 `name` + 位于 `<data_dir>/vanilla_config/` 下的 `file`），`update_tags` 指向
同目录的 UpdateTags bin。`crates/mc-protocol/build.rs` 从它生成索引，不再依赖源码里写死的文件名/
数量；文件缺失、条目数不是 32、名字重复都会在编译期报错。

## 3. 升级流程（按序执行）

以升级到假想的新版本 `26.4 / protocol NNN / data_version DDD` 为例：

1. **建目录、下载、校验 SHA**：新建 `data/<NNN>/`，按 `data/777/SOURCE.md` 的 URL 模式拉取
   5 个上游 JSON 原文件到 `data/<NNN>/full/`，逐项核对 SHA-256 并记录到新目录的 `SOURCE.md`
   （含下载日期与上游 URL）。哈希不符就停手——说明上游资源已变或抓错了版本。
2. **生成精简表**（从 `full/` 生成 `data/<NNN>/*.json`，规则见 `data/777/SOURCE.md` §精简规则）：
   - `blocks.json` → `{"blocks":[{"name","id","default_state_id","hardness"}, ...]}`；
     ID 取 `default_state_id`，**不要用 `states[0].id` 或 `id`**；
   - `items.json` → `{"<name>":{"id":N}, ...}`（丢弃 `components`）；
   - `entities.json` → `{"<name>":{"id":N}, ...}`（丢弃其他元数据）；
   - `packets.json` / `properties.json` 原样复制。
   上游名称不带命名空间时，`build.rs` 会自动补 `minecraft:`，精简表不要自己加。
3. **安置抓包 bin**：把新版本真机抓到的 LoginPlay / PositionSync / UpdateTags 与 32 个
   Configuration 注册表包体放进 `data/<NNN>/`，更新 manifest 的 `vanilla_config.registries`
   有序索引与 `vanilla_config.update_tags`（数量/顺序/文件名都可能变；新版本注册表数量变化时
   还要同步改 `mc-protocol/build.rs` 的 `EXPECTED_REGISTRIES`）。旧版本 bin 留在旧目录，不混用。
4. **更新版本开关**：
   - `data/manifest.json` 的 `version` / `protocol` / `data_version` / `data_dir` 四项；
   - `crates/mc-core/src/lib.rs` 的 `MC_VERSION` / `PROTOCOL_VERSION` / `DATA_VERSION`；
   - `crates/mc-registry/build.rs` 的 `EXPECTED_VERSION` / `EXPECTED_PROTOCOL` /
     `EXPECTED_DATA_VERSION`，以及 `validate_expected_ids()` 里的 29 项真机实测断言
     （换成新版本实测值；否则会编译期失败，这正是护栏）；
   - `crates/mc-protocol/build.rs` 的 `EXPECTED_VERSION` / `EXPECTED_PROTOCOL` /
     `EXPECTED_DATA_VERSION`（以及注册表数量，若变化）；
   - `VERSION.md` 的版本映射表与 `docs/registries-777.md` / `docs/packet-map-777.md` 这类
     版本后缀文档（新增对应版本文档或注明仅适用于 777）。
5. **`cargo build` 触发 codegen**：
   - `crates/mc-registry/build.rs` 读 manifest + 精简表 → `$OUT_DIR/generated.rs`
     （`BLOCKS` / `ITEMS` / `ENTITIES` / `PACKETS` 表 + `by_name` / `by_id`），由
     `crates/mc-registry/src/lib.rs` 的 `include!` 引入；
   - `crates/mc-protocol/build.rs` 读 manifest 的 `vanilla_config` → `$OUT_DIR/vanilla_config_generated.rs`
     （32 个 registry 包体与 UpdateTags 的有序 `include_bytes!` 索引），由 `vanilla_config_777.rs` 引入。
   两个 build script 都声明了 `cargo:rerun-if-changed`，改完数据直接重新编译即可。先确认
   `block_id("minecraft:stone")==1` 这类断言通过、表长度合理，再往下做。
6. **手改包体布局**（codegen 做不到）：生成表只给 ID，**不生成任何结构体、字段顺序或编解码器**。
   按新版本协议逐包核对并修改 `mc-protocol`：新增/删除字段、字段顺序变化、NBT 与注册表条目
   结构变化、Configuration 注册表数量与顺序、包 ID 语义变化（例如 26.2/26.3 插入
   `post_effects` 后 known_packs/update_tags 顺延到 `0x0F` / `0x0E`）都要人工适配
   `config.rs` / `vanilla_config_777.rs` / `block_tags_777.rs` 等（bin 路径已由 build.rs 按
   manifest 生成，不用再手改 `include_bytes!`）。上游对照可用 minecraft.wiki 的版本协议页 +
   新抓包；不要因为 ID 表更新就假设包体兼容。
7. **fixture 回归**：跑 `cargo test --workspace`（或 `cargo check --workspace --tests`），
   重点看三组对照（区分"逐字节"与"解码后比对"）：
   - LoginPlay：`crates/mc-protocol/src/packets/play_login.rs` 的
     `play_login_matches_vanilla_777_capture` ↔ `data/<NNN>/login_play.bin`，解码后重新编码
     与抓包**逐字节相等**；
   - PositionSync：`crates/mc-protocol/src/packets/play_sync.rs` 的
     `play_position_sync_matches_vanilla_777_capture` ↔ `data/<NNN>/player_position_sync.bin`，
     同样**逐字节相等**；
   - Configuration：`config.rs` 的 `config_update_tags_match_vanilla_777_capture` ↔
     `data/<NNN>/update_tags.bin`，校验抓包长度（26.3 为 58,754 B）与 block/timeline 标签
     （按名字排序后比对），**不是整包字节相等**；32 个 `vanilla_config` registry bin 由
     `mc-protocol/build.rs` 按 manifest 检查文件存在/数量/顺序，`vanilla_config_777.rs` 原样
     下发，升级时按需逐包对照新抓包。
   注意区分两种回归：**同一版本内**改动要求现有 fixture 断言不漂移；**跨版本升级**必须用
   新版本抓包替换 fixture 后重新断言，不能拿旧 bin 期待相等。
8. **真机清单**：脚本 e2e 只验服务端状态，不能替代客户端观察。升级后的待做项按
   `HANDOFF.md` §5 第 13 条的 **T1–T13 真机测试清单**（初速/弹跳/拾取/START–FINISH/走远/
   树下/换列/合并/双开/视距回返）逐条走；该条注明原始审查报告未入仓库，当前只保留索引。
   清单未完成前不要写「真机已验证」。

## 4. 已知约束

- **整体切换、不做多版本兼容**：见 `VERSION.md`。Handshake protocol 不符直接断开，Status
  固定写当前版本，不保留旧数据升级逻辑。
- **`vanilla_blocks()` 只有 API 兼容意义**：`crates/mc-registry/src/lib.rs` 里它只列
  air=0 / stone=1 / grass_block=9（早期历史值 grass_block=2 已修正为真实 blockstate ID）。
  新代码一律用 `block_id()` / `block_name()` 系列，别依赖这个精简兼容入口推导 ID。
- **真值只有一份**：`data/<protocol>/*.json`。新增方块/物品只改 JSON 并重新编译，不改游戏
  逻辑源码；但新字段/新包结构仍要手改 `mc-protocol`。
- **版本后缀文件**：`vanilla_config_777.rs`、`block_tags_777.rs` 等名字带 777 的源码文件是
  历史命名；升级时按新版本内容更新，不要只改数据而漏掉它们。
