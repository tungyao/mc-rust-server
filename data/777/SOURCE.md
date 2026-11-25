# data/777 数据来源（MC 26.3 / protocol 777 / data_version 5023）

本目录存放 **Pumpkin 26.3 Extractor** 从真实游戏提取的注册表数据。
获取日期：2026-11-25。版权归原作者，按 MIT 许可证使用（见 `docs/data-driven-plan.md` 中的授权说明）。

## 来源 URL

所有文件来自同一仓库的 `assets/` 目录：

| 本地文件 | 上游 URL | 处理方式 |
|---|---|---|
| `packets.json` | <https://raw.githubusercontent.com/Pumpkin-MC/Pumpkin/master/assets/packets.json> | 原样复制（5,564 B） |
| `blocks.json` | <https://raw.githubusercontent.com/Pumpkin-MC/Pumpkin/master/assets/blocks.json> | 精简（见下），完整版在 `full/blocks.json` |
| `items.json` | <https://raw.githubusercontent.com/Pumpkin-MC/Pumpkin/master/assets/items.json> | 精简（见下），完整版在 `full/items.json` |
| `entities.json` | <https://raw.githubusercontent.com/Pumpkin-MC/Pumpkin/master/assets/entities.json> | 精简（见下），完整版在 `full/entities.json` |
| `properties.json` | <https://raw.githubusercontent.com/Pumpkin-MC/Pumpkin/master/assets/properties.json> | 原样复制（14,126 B） |

### SHA-256（上游原文件，与 `full/` 下文件一致）

```
4714470b70eafe9ff34b67f498723c1cd7ba281d99a20c7c4415708497b2a9df  blocks.json
f1208d35e03f7b0c45fc17ddeeb8966535856c9758277da5ccb90a5ef4b0847d  items.json
714ce6a924cc5d097f569ebcab091e7202a5108f8503cc72d3004c3f9a58f981  entities.json
e7b64e92e0c37829e12f5b66f9b7a4afe5599ebdc6f53a767f00be28dcc481f5  packets.json
5bf3eb420fc556b9ab4190387cb3e0e43503b69801dd169a93df1d295ea68471  properties.json
```

## 精简规则

`crates/mc-registry/build.rs` 只读取本目录的精简表；`full/` 保留上游原始 JSON，
用于未来升级时重新生成精简表或查询完整字段（方块属性、状态变体、物品组件等）。

- `blocks.json`：`{"blocks":[{"name","id","default_state_id","hardness"}, ...]}`，1286 项。
  ⚠️ **blockstate ID 必须取 `default_state_id`，不是 `states[0].id`，也不是 `id`**
  （后者只是方块种类的序号）。实测：grass_block `id=8` 但 `default_state_id=9`；
  water 37→89、sand 39→121、oak_log 51→140、oak_leaves 94→294。
- `items.json`：`{"<name>":{"id":N}, ...}`，1658 项（丢弃 `components` 组件表）。
- `entities.json`：`{"<name>":{"id":N}, ...}`，161 项（丢弃其他实体元数据）。
- `packets.json` / `properties.json`：未精简，原样入库。

## ID 校验（P0 回归护栏）

由 `crates/mc-registry/tests/data_validation.rs` 与 `build.rs` 中的断言逐项对照
`HANDOFF.md` / `docs/data-driven-plan.md` 中的真机实测值：

- blocks `default_state_id`：air 0 / stone 1 / grass_block 9 / dirt 10 / water 89 /
  sand 121 / oak_log 140 / oak_leaves 294 / coal_ore 136 / iron_ore 134
- items：stone 1 / dirt 55 / cobblestone 62 / oak_sapling 77 / sand 88 / oak_log 163 /
  coal 1010 / stick 1060 / raw_iron 1017
- packets：CB play `add_entity`=1 / `block_changed_ack`=4 / `block_update`=8 /
  `level_chunk_with_light`=46 / `entity_position_sync`=35 / `set_entity_data`=101 /
  `take_item_entity`=127；SB play `player_action`=41 / `use_item_on`=66 / `move_player_pos`=30

## 重新获取

```bash
BASE=https://raw.githubusercontent.com/Pumpkin-MC/Pumpkin/master/assets
for n in packets blocks items entities properties; do
  curl -fsSL -o /tmp/pumpkin-$n.json $BASE/$n.json
done
```

然后用与上述精简规则相同的脚本覆盖 `data/777/*.json`，`cargo build` 会自动重跑 codegen
（`build.rs` 已对 `data/` 目录声明 `rerun-if-changed`）。
