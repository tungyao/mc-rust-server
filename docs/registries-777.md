# 777 同步注册表全量清单（只兼容 26.3/777）

> 状态：调研文档，不动代码。现状：`default_configuration_registries()` 只发 2 包
> （`dimension_type/overworld` + `worldgen/biome/plains`），真机走到 Finish 后 ~250ms 断开。
> 本文档给出 777 要求的全部 30 个同步注册表 + 每表最小有效 NBT + 按“缺了必踢”排序的实现清单。
> 主来源：<https://minecraft.wiki/w/Java_Edition_protocol/Registries>（2026-11-07 修订，含 26.x 新注册表）。
> 包 ID 依据：`docs/packet-map-777.md`（Registry Data CB `0x07`；Update Tags CB `0x0D` 见该文档 :46 注）。

## 0. 结论先行（给实现看的 P0）

真机在 Finish 后断开，最可疑的三个缺口（按可能性排序）：

1. **现有 2 包的 NBT 是按旧版本 codec 手拼的，26.3 校验必挂。**
   - `dimension_type` 当前 NBT 含 25w45a 已删除字段（`ultrawarm/natural/bed_works/respawn_anchor_works/piglin_safe/has_raids/effects`），
     缺 26.3 必需字段（`has_ender_dragon_fight/skybox/cardinal_light/has_fixed_time` 等）。见 §2.1。
   - `biome/plains` 当前 NBT 含 25w42a 已删除字段（`sky_color/fog_color/water_fog_color`），
     缺必需字段（`carvers` + `features`）。见 §2.2。
   - Mojang codec 对缺失必需字段是 error → 客户端解析失败 → Finish 后断开（时序上正好 ~250ms）。
2. **缺 `damage_type` 注册表**：Login(play) 要求 27 个指定 damage type，缺失则拒收 Login(play)。见 §2.4。
3. **缺 Update Tags（Configuration CB `0x0D`），且从未发送过**（`login_route.rs` 无 tags 逻辑）。
   以下 tag 缺失则拒收 Finish Configuration：`pattern_item/*`（10 项）、`axe/hoe/shovel`（block_transformer）、
   `bypasses_shield/is_explosion/is_fire`（damage_type）。见 §4。

另外 `monster_spawn_light_level` 的疑问已有答案：**Int 和对象两种都合法**
（wiki 原文 `[Int][NBT Compound / JSON Object] An integer or int provider`，见 §2.1），
当前 `Int 0` 形式可保留，vanilla overworld 用的是 0–7 均匀分布（对象形式）。

策略建议：P0 注册表 **inline 真实 NBT**（不依赖客户端 known packs，可兼容回空 C2S known packs 的客户端）；
Optional 注册表可先发空条目包（`count=0`）占位，后续再补（见 §5 清单 P2）。

## 1. 777 全部 30 个同步注册表

“要求”列取值 = wiki `List of synchronized registries` 表 + 各 `Requirements` 小节原文。
`Optional` = 缺失可进游戏；`Non-empty` = 至少 1 条目；点名条目 = 缺指定条目则踢（注明踢点）。

| # | 注册表名 | 要求 | 最小条目 | NBT 字段来源 |
|---|----------|------|----------|--------------|
| 1 | `minecraft:dimension_type` | 技术上可选，但 Login(play) 的 Dimension Type 字段必填 → 实际至少 1 项 | `minecraft:overworld` | §2.1 / <https://minecraft.wiki/w/Dimension_type> |
| 2 | `minecraft:worldgen/biome` | 必须含 `minecraft:plains`（否则拒收 Login(play)，作未加载区块默认） | `minecraft:plains` | §2.2 / <https://minecraft.wiki/w/Biome_definition_(Java_Edition)> |
| 3 | `minecraft:chat_type` | Optional（聊天/Disguised Chat/Player Chat 包引用） | 7 项全发（`chat` 等，防玩家发消息时查不到） | §2.3 / <https://minecraft.wiki/w/Chat_type> |
| 4 | `minecraft:damage_type` | 27 项点名（否则拒收 Login(play)）+ 3 个 tag（否则拒收 Finish，见 §4） | 27 项全发 | §2.4 / <https://minecraft.wiki/w/Damage_type> |
| 5 | `minecraft:painting_variant` | Non-empty | 1 项（`minecraft:kebab`，16×16） | <https://minecraft.wiki/w/Painting_variant_definition> |
| 6 | `minecraft:wolf_variant` | Non-empty | 1 项（`minecraft:pale`） | <https://minecraft.wiki/w/Mob_variant_definitions>（Wolf 节） |
| 7 | `minecraft:wolf_sound_variant` | Non-empty | 1 项（`minecraft:classic`，待实现时按链接核对名） | 同上（Wolf sounds 节） |
| 8 | `minecraft:cat_variant` | Non-empty | 1 项（`minecraft:tabby`，待核对） | 同上（Cat 节） |
| 9 | `minecraft:cat_sound_variant` | Non-empty | 1 项（待核对） | 同上（Cat sounds 节） |
| 10 | `minecraft:cow_variant` | Non-empty | 1 项（`minecraft:temperate`，待核对） | 同上（Cow 节） |
| 11 | `minecraft:cow_sound_variant` | Non-empty | 1 项（待核对） | 同上（Cow sounds 节） |
| 12 | `minecraft:pig_variant` | Non-empty | 1 项（`minecraft:temperate`，待核对） | 同上（Pig 节） |
| 13 | `minecraft:pig_sound_variant` | Non-empty | 1 项（待核对） | 同上（Pig sounds 节） |
| 14 | `minecraft:chicken_variant` | 点名 3 项（`cold/temperate/warm`，否则拒收 Finish） | 3 项全发 | 同上（Chicken 节）+ Registries 页 Chicken variant requirements |
| 15 | `minecraft:chicken_sound_variant` | Non-empty | 1 项（待核对） | 同上（Chicken sounds 节） |
| 16 | `minecraft:frog_variant` | Non-empty | 1 项（`minecraft:temperate`，待核对） | 同上（Frog 节） |
| 17 | `minecraft:zombie_nautilus_variant` | Non-empty | 1 项（待核对） | 同上（Zombie Nautilus 节） |
| 18 | `minecraft:banner_pattern` | 条目不限，但 10 个 `pattern_item/*` tag 必需（否则拒收 Finish，见 §4） | 1 项 + 10 个 tag | <https://minecraft.wiki/w/Banner_pattern_definition> |
| 19 | `minecraft:block_transformer` | 条目不限，但 `axe/hoe/shovel` 3 个 tag 必需（否则拒收 Finish） | 1 项（⚠️ 新注册表，格式页待细读） + 3 个 tag | <https://minecraft.wiki/w/Block_transformer_definition> |
| 20 | `minecraft:instrument` | 必须含 `minecraft:ponder_goat_horn`（山羊角默认组件，否则拒收 Finish） | 1 项即它 | <https://minecraft.wiki/w/Instrument_definition> |
| 21 | `minecraft:jukebox_song` | 22 项点名（物品组件默认值，否则拒收 Finish） | 22 项全发 | <https://minecraft.wiki/w/Jukebox_song_definition> |
| 22 | `minecraft:trim_material` | 10 项点名（否则拒收 Finish） | 10 项全发 | <https://minecraft.wiki/w/Armor_trim_definition>（Trim material 节） |
| 23 | `minecraft:trim_pattern` | Optional | 1 项（如 `minecraft:coast`，待核对） | 同上（Trim pattern 节） |
| 24 | `minecraft:sulfur_cube_archetype` | Non-empty（⚠️ 26.x 新怪物相关） | 1 项（待核对） | <https://minecraft.wiki/w/Sulfur_cube_archetype_definition> |
| 25 | `minecraft:dialog` | Optional | 空包或 1 项 | <https://minecraft.wiki/w/Dialog>（Dialog format 节） |
| 26 | `minecraft:enchantment` | Optional | 空包占位（魔咒内容大，后续按需） | <https://minecraft.wiki/w/Enchantment_definition> |
| 27 | `minecraft:test_environment` | Optional | 空包占位 | <https://minecraft.wiki/w/Test_environment_definition> |
| 28 | `minecraft:test_instance` | Optional | 空包占位 | <https://minecraft.wiki/w/Test_instance_definition> |
| 29 | `minecraft:timeline` | Optional（⚠️ 26.x 新：环境属性时间线） | 空包占位 | <https://minecraft.wiki/w/Timeline> |
| 30 | `minecraft:world_clock` | Optional（⚠️ 26.x 新：`/time` 默认时钟，dimension 引 `default_clock`） | 空包占位 | <https://minecraft.wiki/w/World_clock> |

注：wiki 表共 30 行（含 `sulfur_cube_archetype`、`timeline`、`world_clock`、`block_transformer`
及 12 个 mob variant/sound 注册表）。另有 requirements 小节点名的 `damage_type` 27 项、
`jukebox_song` 22 项、`trim_material` 10 项、`chicken_variant` 3 项已分别列入上表。

## 2. 重点注册表最小有效 NBT（已逐字段核对 wiki）

NBT 类型缩写：Byte=1, Int=3, Float=5, Double=6, String=8, List=9, Compound=10。
JSON→NBT 映射按 Registries 页说明（data pack JSON 结构即 NBT 结构）。

### 2.1 `dimension_type` / `minecraft:overworld`（P0，第 0 项）

来源：<https://minecraft.wiki/w/Dimension_type>（含 26.1/26.2 修订：`effects` 删除、
`natural/ultrawarm/bed_works/respawn_anchor_works/piglin_safe/has_raids` 迁移至 environment attributes、
`fixed_time`→`has_fixed_time`、`infiniburn` 可为 tag/单个 ID/ID 列表）。

**`monster_spawn_light_level` 定论：Int 或 int-provider 对象均可。**
wiki 原文：`[Int][NBT Compound / JSON Object] monster_spawn_light_level: An integer or int provider
from 0 to 15`。当前代码 `Int 0` 形式合法；vanilla overworld 默认表写 `0-7`
（即 uniform int provider，min 0 max 7）。建议实现时照抄对象形式以贴近 vanilla，
校验层面两者都过。

26.3 overworld 最小有效 JSON（→ NBT，按 wiki Defaults 表 + JSON format 节）：

```json
{
  "has_skylight": true,
  "has_ceiling": false,
  "has_ender_dragon_fight": false,
  "coordinate_scale": 1.0,
  "has_fixed_time": false,
  "ambient_light": 0.0,
  "monster_spawn_block_light_limit": 0,
  "monster_spawn_light_level": { "type": "minecraft:uniform", "value": { "min_inclusive": 0, "max_inclusive": 7 } },
  "logical_height": 384,
  "min_y": -64,
  "height": 384,
  "infiniburn": "#minecraft:infiniburn_overworld",
  "skybox": "overworld",
  "cardinal_light": "default",
  "default_clock": "minecraft:overworld"
}
```

字段说明（必填除非标“可选”）：`coordinate_scale` Double(0.00001–30000000)；
`has_skylight/has_ceiling`（`has_ceiling=true` 会关天气、改重生/刷怪位置计算）；
`ambient_light` Float；`monster_spawn_block_light_limit` Int(0–15)；
`logical_height` Int（≤ height）；`min_y` Int（-2032–2031 且为 16 倍数）；
`height` Int（16–4064 且为 16 倍数，`min_y+height-1 ≤ 2031`）；
`infiniburn`（tag 带 `#` 前缀，或单个 ID，或 ID 列表）；
`attributes` 可选（environment attribute map）；`timelines` 可选（ID/tag/数组）。
`has_ender_dragon_fight`（26.1-snap6 新增，无“可选”标注，按必填处理）。
`default_clock` 为 26.1 新增 String ID（overworld 默认值见 Defaults 表）。

**当前代码偏差清单**（`config.rs: real_overworld_dimension_nbt`）：发送了 7 个已删除字段
（`ultrawarm/natural/bed_works/respawn_anchor_works/piglin_safe/has_raids/effects`），
缺 4 个现行字段（`has_ender_dragon_fight/has_fixed_time/skybox/cardinal_light`，
另建议补 `default_clock`）。其中 `effects` 删除（25w45a）、`natural` 等迁移 attributes（25w42a/25w45a）、
`fixed_time` 更名（25w45a），三处均有 History 节为证。

### 2.2 `worldgen/biome` / `minecraft:plains`（P0，第 0 项）

来源：<https://minecraft.wiki/w/Biome_definition_(Java_Edition)>。
plains 全字段最小有效 JSON：

```json
{
  "has_precipitation": true,
  "temperature": 0.8,
  "temperature_modifier": "none",
  "downfall": 0.4,
  "effects": { "water_color": 4159204 },
  "carvers": [],
  "features": []
}
```

字段说明：`temperature` Float（控草地/树叶色、雨雪判定）；`downfall` Float；
`effects.water_color` 必填（常规值 4159204；25w44a 起也接受 hex 字符串或 RGB 数组）；
`foliage_color/dry_foliage_color/grass_color/grass_color_modifier` 可选（缺省按 temperature/downfall 推导）；
`attributes` 可选；`carvers` 必填可空（tag 带 `#` 或 ID 数组）；`features` 为生成阶段列表，可空数组。
plains 的 temperature 0.8 / downfall 0.4 沿用现值（与 vanilla 一致）。

**当前代码偏差清单**（`config.rs: real_plains_biome_nbt`）：发送了 3 个 25w42a 已删除字段
（`sky_color/fog_color/water_fog_color`），缺 2 个必填字段（`carvers/features`）。
另注意 26.3-snap4 删除了 `creature_spawn_probability/spawners/spawn_costs`
（迁至 environment attributes）——当前代码本就没发这三个，无需动作。

### 2.3 `chat_type`（P0，7 项全发）

来源：<https://minecraft.wiki/w/Chat_type>。单条目格式（`style` 可选）：

```json
{
  "chat": { "translation_key": "chat.type.text", "parameters": ["sender", "content"] },
  "narration": { "translation_key": "chat.type.text.narrator", "parameters": ["sender", "content"] }
}
```

7 个 vanilla 条目名（List of chat types 节，`msg_command` 已拆分为 in/out，
`team_msg_command` 已拆分为 in/out，老名不再存在）：
`chat` / `emote_command` / `msg_command_incoming` / `msg_command_outgoing` /
`say_command` / `team_msg_command_incoming` / `team_msg_command_outgoing`。
各条目 translation_key/parameters 按 Chat_type 页“Default chat format”列照抄
（如 `chat` 为 `<%s> %s` 配 `["sender","content"]`）；narration 同页“Default narration”列。
要求列为 Optional，但玩家一发消息（Player Chat Message 包引用序号）就查表，缺项即错位，故 7 项全发。

### 2.4 `damage_type`（P0，27 项全发）

来源：<https://minecraft.wiki/w/Damage_type> + Registries 页 Damage type requirements。
单条目最小有效 JSON（箭头示例为证，`effects/death_message_type` 可省，有默认值）：

```json
{ "message_id": "generic", "exhaustion": 0.0, "scaling": "never" }
```

- `message_id` String（死亡消息键 `death.attack.<message_id>` 的一部分）；
- `exhaustion` Float（饥饿消耗，`generic` vanilla 值为 0.0，箭头示例 0.1）；
- `scaling` 枚举：`never` / `always` / `when_caused_by_living_non_player`。
  占位值对“能不能进服”无影响（只影响难度伤害缩放与死亡文本），实现时先统一
  `exhaustion: 0.0, scaling: "never"`，后续有空再按 vanilla 逐项对齐。

27 个 Login(play) 点名条目（Registries 页原文，顺序即建议发送顺序）：
`cactus/campfire/cramming/dragon_breath/drown/dry_out/ender_pearl/fall/fly_into_wall/freeze/`
`generic/generic_kill/hot_floor/in_fire/in_wall/lava/lightning_bolt/magic/on_fire/`
`out_of_world/outside_border/spear/stalagmite/starve/sweet_berry_bush/wither`。
（注意含 26.2 新增 `spear` 与 `sulfur_cube_hot` 世代相近的新项；`sulfur_cube_hot` 不在 27 项内，
随 `sulfur_cube_archetype` 另行处理。）

### 2.5 其余注册表最小 NBT 速查（置信度标注，实现时按链接复核）

- ✅ 已核对（本节上文）：dimension_type / biome / chat_type / damage_type。
- 🔶 通用已知形状、待实现时按链接复核（均为小对象，单条目即可过 Non-empty 校验）：
  - `painting_variant`: `{asset,width,height}`（如 `kebab` 16×16），源 <https://minecraft.wiki/w/Painting_variant_definition>。
  - `instrument`: `{sound_event,use_duration,range,description}`，必含 `ponder_goat_horn`，
    源 <https://minecraft.wiki/w/Instrument_definition>。
  - `jukebox_song` 22 项：`11/13/5/blocks/bounce/cat/chirp/creator/creator_music_box/far/`
    `lava_chicken/mall/mellohi/otherside/pigstep/precipice/relic/stal/strad/tears/wait/ward`，
    形如 `{sound_event,description,length_in_seconds,comparator_output}`，
    源 <https://minecraft.wiki/w/Jukebox_song_definition>。
  - `trim_material` 10 项：`amethyst/copper/diamond/emerald/gold/iron/lapis/netherite/quartz/redstone`
    （注：wiki 点名 10 项无 `resin`；`resin` 是否存在待实现时以 client.jar 为准），
    形如 `{asset,description}`，源 <https://minecraft.wiki/w/Armor_trim_definition>。
  - `trim_pattern`（Optional）：形如 `{asset,description,decal}`，同上链接。
  - `banner_pattern`：形如 `{asset,translation_key}`，源 <https://minecraft.wiki/w/Banner_pattern_definition>。
  - 12 个 mob variant/sound：Cat/Chicken/Cow/Frog/Pig/Wolf 各 `{asset}` 类小对象 + 同名 sound variant，
    `chicken_variant` 必须含 `cold/temperate/warm` 3 项，
    源 <https://minecraft.wiki/w/Mob_variant_definitions>。
  - `dialog`（Optional）：源 <https://minecraft.wiki/w/Dialog>。
  - `enchantment`（Optional）：源 <https://minecraft.wiki/w/Enchantment_definition>。
  - `test_environment` / `test_instance`（Optional）：
    源 <https://minecraft.wiki/w/Test_environment_definition> /
    <https://minecraft.wiki/w/Test_instance_definition>。
  - 🔴 待细读（26.x 新注册表，格式不熟，实现前必须读完链接页）：
    `block_transformer`（<https://minecraft.wiki/w/Block_transformer_definition>）、
    `sulfur_cube_archetype`（<https://minecraft.wiki/w/Sulfur_cube_archetype_definition>）、
    `timeline`（<https://minecraft.wiki/w/Timeline>）、
    `world_clock`（<https://minecraft.wiki/w/World_clock>）。

## 3. 条目顺序约束（实现必须遵守）

Entries 数组顺序即数字 ID（从 0 递增），Login(play)/Respawn/Chunk 包按序号引用，
服务端与客户端必须一致。当前 Play Login 发 `dimension_type=0`、
区块 palette 用 biome 0 → `overworld` 必须是 dimension 包第 0 项、
`plains` 必须是 biome 包第 0 项。damage_type 等其余注册表目前无包引用其序号，
顺序只要与 27 项名单一致即可，建议按 §2.4 所列 wiki 顺序发送。

## 4. Update Tags（与注册表同 P0，Configuration CB `0x0D`）

来源：Registries 页 Known packs negotiation（tag 永不走 known packs，必须全量发送）
+ 四个 requirements 小节。当前服务端从未发送 Update Tags 包，属 Finish 被踢的高嫌疑项。

最小 tag 集合（tag 名 → 所含条目，tag 类型按包格式 `tag_name + VarInt count + 条目ID列表`）：

- `minecraft:pattern_item/bordure_indented/creeper/field_masoned/flow/flower/globe/guster/mojang/piglin/skull`（10 个 tag，各含对应 banner_pattern 条目）。
- `minecraft:axe` / `minecraft:hoe` / `minecraft:shovel`（3 个 block_transformer tag）。
- `minecraft:bypasses_shield` / `minecraft:is_explosion` / `minecraft:is_fire`（3 个 damage_type tag，
  成员划分见 <https://minecraft.wiki/w/Damage_type_tag_(Java_Edition)>，实现时照抄）。
- 建议同包附带：`minecraft:infiniburn_overworld` 等被 dimension 引用的 block tag
  （dimension 用 `#` 引用，缺 tag 时客户端解析维度即错；成员见
  <https://minecraft.wiki/w/Block_tag_(Java_Edition)>）。

## 5. 实现清单（按“缺了必踢”排序，只估行数不动代码）

NBT 构造沿用 `config.rs` 现有手拼风格（`nbt_byte/nbt_int/nbt_float/nbt_double/nbt_string` + 新增
`nbt_list/nbt_compound` 小助手；P7A 的 nbt.rs encoder 切过去时再统一替换）。

| 优先级 | 任务 | 内容 | 估行数（含单测） |
|--------|------|------|------------------|
| P0-1 | 重写 overworld dimension NBT | 按 §2.1 字段：删 7 个旧字段，加 `has_ender_dragon_fight/has_fixed_time/skybox/cardinal_light/default_clock`，`monster_spawn_light_level` 改 uniform 对象（或保留 Int 0，单测断言两种形式） | ~150（builder ~90 + test ~60） |
| P0-2 | 重写 plains biome NBT | 按 §2.2：删 3 个旧色值字段，加 `carvers: []` + `features: []` | ~90（builder ~50 + test ~40） |
| P0-3 | `damage_type` 27 项全量 | 表驱动 27 个 `{message_id, exhaustion: 0.0, scaling: "never"}` + Login(play) 点名断言单测 | ~150（表 ~90 + 编解码复用现有 + test ~60） |
| P0-4 | Update Tags 包 | Configuration CB `0x0D` 编解码 + §4 最小 tag 集（16+ 个 tag）+ Finish 前发送接线 | ~180（codec ~80 + 表 ~60 + test/接线 ~40） |
| P0-5 | `chat_type` 7 项 | 表驱动 7 条目 + 单测 | ~110（表 ~60 + test ~50） |
| P0-6 | 发送顺序修正 | Registry Data（30 包）→ Update Tags → Finish S2C；`overworld/plains` 各自第 0 项断言 | ~40（接线 + 单测） |
| P1-1 | Non-empty 类补齐 | painting(1) + wolf/cat/cow/pig/frog/chicken/zombie_nautilus variant(7) + 6 sound variant(各1) + instrument(ponder_goat_horn) + block_transformer(1+3tag 已在 P0-4) + sulfur_cube(1)，单条目小 NBT | ~350（~14 表 × ~25） |
| P1-2 | 点名类补齐 | jukebox 22 项 + trim_material 10 项 + trim_pattern 1 项 + chicken_variant 3 项 | ~250（表驱动为主） |
| P2 | Optional 占位 | dialog/enchantment/test_environment/test_instance/timeline/world_clock 发空条目包（`count=0`）或单最小条目；enchantment 全量延后 | ~120（6 包 × ~20） |

合计约 1440 行新增（含测试），分 3 个 PR 做：PR1 = P0-1+P0-2（只改两个 NBT 函数，真机验证是否还踢）→
PR2 = P0-3+P0-4+P0-5+P0-6（进服门票补齐）→ PR3 = P1+P2（全量 30 包）。

## 6. 待真机验证的开放问题

1. P0-1+P0-2 修正后真机是否仍在 Finish 后断开（若不断开，则病因即旧 NBT 校验失败）。
2. Optional 注册表发空条目包（`count=0`）是否被 vanilla 接受，还是必须至少 1 项
   （Non-empty 类已按至少 1 项处理；Optional 类待 PR3 时真机验证）。
3. `trim_material` 是否需补 `resin`（wiki 点名 10 项，实现时以 client.jar `data/minecraft/trim_material/` 为准）。
4. mob variant 各 sound 注册表的确切条目名（实现时以 client.jar `data/minecraft/<name>/` 文件名为准，
   jar 内 data pack 即权威来源，见 Registries 页 Obtaining vanilla registry data 节）。
