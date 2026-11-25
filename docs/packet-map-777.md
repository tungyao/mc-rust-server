# Packet ID 对照表（26.3 / 协议 777）

> 只核对 ID，不改代码。P4a 第一步：先把包号对齐 777，再谈玩法。
> 版本锁定见 `VERSION.md`：MC 26.3 / PROTOCOL 777 / DATA 5023。
> 本表“现用常量”取自 `crates/mc-protocol/src/packets/*.rs`（分支 `feat/p4a-packet-verify` 现状）。

来源（取数时间 2026-11-23）：

- 主来源：[minecraft.wiki — Java Edition protocol/Packets](https://minecraft.wiki/w/Java_Edition_protocol/Packets)（页首声明 `currently 777 in Minecraft 26.3`，且 `Note: The packet IDs listed here are valid only for the protocol version documented on this page, currently 777`）
- 版本号：[minecraft.wiki — Protocol version numbers](https://minecraft.wiki/w/Minecraft_Wiki:Projects/wiki.vg_merge/Protocol_version_numbers)（777 = 26.3）
- 漂移参考（minecraft-data 尚未收录 26.2/26.3，master 只有到 26.1=775）：[`data/pc/26.1/protocol.json`](https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data/pc/26.1/protocol.json) / [`data/pc/26.1/version.json`](https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data/pc/26.1/version.json)（`{"version":775,"minecraftVersion":"26.1"}`，对照可见 Play 包整体 +1 漂移）
- 旧 wiki.vg 已下线（见 [sunset 说明](https://tkte.ch/articles/2024/11/11/sunsetting) / [ProtocolLib #3321](https://github.com/dmulloy2/ProtocolLib/issues/3321)），以 minecraft.wiki 为准。

图例：✅ 对（可直接用） / <span style="color:red">❌ 错（必须改，正确值见同行）</span> / ⬜ 未实现（代码无此常量，需新增）

## 1. Handshake / Status（全对）

| 现用常量（文件） | 现用值 | 真实 ID（777） | 对/错 | 来源链接 |
|---|---|---|---|---|
| `HANDSHAKE_PACKET_ID`（`packets/handshake.rs:6`）| `0x00` SB `intention` | `0x00` SB `intention` | ✅ 对 | [Handshake](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Handshake) |
| `STATUS_REQUEST_PACKET_ID`（`packets/status.rs:6`）| `0x00` SB `status_request` | `0x00` SB `status_request` | ✅ 对 | [List of packets — Status](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets) |
| `STATUS_RESPONSE_PACKET_ID`（`packets/status.rs:8`）| `0x00` CB `status_response` | `0x00` CB `status_response` | ✅ 对 | 同上 |
| `PING_PACKET_ID`（`packets/status.rs:10`）| `0x01` SB `ping_request` | `0x01` SB `ping_request` | ✅ 对 | 同上 |
| `PONG_PACKET_ID`（`packets/status.rs:12`）| `0x01` CB `pong_response` | `0x01` CB `pong_response` | ✅ 对 | 同上 |

## 2. Login（全对）

| 现用常量（文件） | 现用值 | 真实 ID（777） | 对/错 | 来源链接 |
|---|---|---|---|---|
| `LOGIN_START_PACKET_ID`（`packets/login.rs:7`）| `0x00` SB `hello` | `0x00` SB `hello` (Login Start) | ✅ 对 | [List of packets — Login](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets) |
| `ENCRYPTION_REQUEST_PACKET_ID`（`packets/login.rs:11`）| `0x01` CB `hello` | `0x01` CB `hello` (Encryption Request) | ✅ 对 | 同上 |
| `ENCRYPTION_RESPONSE_PACKET_ID`（`packets/login.rs:12`）| `0x01` SB `key` | `0x01` SB `key` (Encryption Response) | ✅ 对 | 同上 |
| `LOGIN_SUCCESS_PACKET_ID`（`packets/login.rs:8`）| `0x02` CB `login_finished` | `0x02` CB `login_finished` (Login Success) | ✅ 对 | 同上 |
| `LOGIN_ACKNOWLEDGED_PACKET_ID`（`packets/login.rs:9`）| `0x03` SB `login_acknowledged` | `0x03` SB `login_acknowledged` | ✅ 对 | 同上 |
| `SET_COMPRESSION_PACKET_ID`（`packets/login.rs:10`）| `0x03` CB `login_compression` | `0x03` CB `login_compression` (Set Compression) | ✅ 对 | 同上 |

## 3. Configuration（代码缺失，需新增；真实 ID 如下）

| 现用常量（文件） | 现用值 | 真实 ID（777） | 对/错 | 来源链接 |
|---|---|---|---|---|
| （无常量，未实现 Finish Configuration CB） | — | `0x03` CB `finish_configuration` | ⬜ 未实现，正确值 `0x03` | [List of packets — Configuration](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets) |
| （无常量，未实现 Acknowledge Finish Configuration SB） | — | `0x03` SB `finish_configuration` | ⬜ 未实现，正确值 `0x03` | 同上 |
| （无常量，未实现 Registry Data CB） | — | `0x07` CB `registry_data` | ⬜ 未实现，正确值 `0x07` | 同上 |
| （无常量，未实现 Known Packs CB/SB） | — | CB `0x0F` `select_known_packs` / SB `0x07` `select_known_packs` | ⬜ 未实现，正确值 CB `0x0F` / SB `0x07` | 同上 |

注：26.1 的 `protocol.json` 里 Configuration CB `registry_data` 也是 `0x07`、`finish_configuration` 也是 `0x03`，此阶段未漂移；Known Packs 在 26.1 是 CB `0x0E`，777 变为 CB `0x0F`（中间插入了 `update_tags 0x0D` 顺延），SB 仍为 `0x07`。见 [`26.1/protocol.json`](https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data/pc/26.1/protocol.json)。

## 4. Play（6 个全错，需改）

| 现用常量（文件） | 现用值 | 真实 ID（777） | 对/错 | 来源链接 |
|---|---|---|---|---|
| `PLAY_LOGIN_PLAY_ID`（`packets/play_login.rs:6`，JoinGame/Login Play CB） | `0x2B` | `0x32` CB `login` | <span style="color:red">❌ 错，正确值 `0x32`（十进制 50）</span> | [Play clientbound — Login (play)](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Login_\(play\)) / [List](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets) |
| `PLAY_SET_CENTER_CHUNK_ID`（`packets/play_chunk.rs:6`，Set Center Chunk CB） | `0x4C` | `0x60` CB `set_chunk_cache_center` | <span style="color:red">❌ 错，正确值 `0x60`（十进制 96）</span> | [Play clientbound — Set Center Chunk](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Set_Center_Chunk) / [List](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets) |
| `PLAY_CHUNK_DATA_ID`（`packets/play_chunk.rs:8`，Chunk Data and Update Light CB） | `0x27` | `0x2E` CB `level_chunk_with_light` | <span style="color:red">❌ 错，正确值 `0x2E`（十进制 46）</span> | [Play clientbound — Chunk Data and Update Light](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Chunk_Data_and_Update_Light) / [List](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets) |
| `PLAY_PLAYER_POSITION_SYNC_ID`（`packets/play_sync.rs:6`，Synchronize Player Position CB，官方名 `player_position`） | `0x3E` | `0x49` CB `player_position` | <span style="color:red">❌ 错，正确值 `0x49`（十进制 73）</span> | [Play clientbound — Synchronize Player Position](https://minecraft.wiki/w/Java_Edition_protocol/Packets#Synchronize_Player_Position) / [List](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets) |
| `PLAY_KEEPALIVE_S2C_ID`（`packets/play_sync.rs:8`，Keep Alive CB） | `0x26` | `0x2D` CB `keep_alive` | <span style="color:red">❌ 错，正确值 `0x2D`（十进制 45）</span> | [List of packets — Play](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets)（CB `45 \| 0x2D \| Keep Alive (clientbound) \| keep_alive`） |
| `PLAY_KEEPALIVE_C2S_ID`（`packets/play_sync.rs:9`，Keep Alive SB） | `0x1A` | `0x1C` SB `keep_alive` | <span style="color:red">❌ 错，正确值 `0x1C`（十进制 28）</span> | [List](https://minecraft.wiki/w/Java_Edition_protocol/Packets#List_of_packets)（SB `28 \| 0x1C \| Keep Alive (serverbound) \| keep_alive`） |

### 4.1 错值在 777 里实际是什么（避免“错得悄无声息”）

| 错值 | 在 777 Play 里的真正含义 | 后果 |
|---|---|---|
| `0x2B` | CB `hurt_animation`（`0x2B Hurt Animation`） | 发 Join 会被客户端当作 Hurt Animation 解析 |
| `0x4C` | CB `recipe_book_remove`（`0x4C Recipe Book Remove`）| 发 SetCenter 会被当作配方书删除 |
| `0x27` | CB `game_event`（`0x27 Game Event`） | 发 ChunkData 会被当作 Game Event 解析，直接断线/崩包解析 |
| `0x3E` | CB `ping`（`0x3E Ping`） | 发 PositionSync 会被当作 Ping |
| `0x26` | CB `forget_level_chunk` 即 Unload Chunk（`0x26 Unload Chunk`）| 发 KeepAlive 会让客户端卸载区块 |
| `0x1A`（SB） | SB `interact`（`0x1A Interact`）| 把 KeepAlive 回包判成 Interact 会导致超时踢人 |

26.1 侧证（[`26.1/protocol.json`](https://raw.githubusercontent.com/PrismarineJS/minecraft-data/master/data/pc/26.1/protocol.json)）：`keep_alive CB 0x2C / map_chunk 0x2D / login 0x31 / position 0x48 / update_view_position(SetCenter) 0x5E / keep_alive SB 0x1C`。777 相对 26.1 整体 +1（SetCenter 因中间插入包变为 `0x60`），说明现用值连 26.1 也对不上，是更老版本残留。

> 勘误（P13）：`0x26 forget_level_chunk` 的包体在 26.3 是 **单个 packed i64 ChunkPos**
> （`x & 0xffffffff | ((z & 0xffffffff) << 32)` 的 big-endian Long），不是旧版
> `Chunk X Int + Chunk Z Int`。依据：26.3.jar
> `ClientboundForgetLevelChunkPacket` 调 `FriendlyByteBuf.writeChunkPos`，
> 后者写 `ChunkPos.pack()` 的 Long（`javap -c` 反编译确认）。P13 真机曾因 x/z
> 交换误卸玩家脚下方块导致掉穿世界，修正后走 250+ 格 `y=65` 稳定。

## 5. 结论（给 P4b 的改单，不在本文件改代码）

- 对（11 个）：Handshake `0x00`；Status 4 个（`0x00/0x00/0x01/0x01`）；Login 6 个（Start `0x00` / EncReq `0x01` / EncResp `0x01` / Success `0x02` / Ack `0x03` / SetCompression `0x03`）。不动。
- 错（6 个，全在 Play）：`PLAY_LOGIN_PLAY_ID 0x2B→0x32`、`PLAY_SET_CENTER_CHUNK_ID 0x4C→0x60`、`PLAY_CHUNK_DATA_ID 0x27→0x2E`、`PLAY_PLAYER_POSITION_SYNC_ID 0x3E→0x49`、`PLAY_KEEPALIVE_S2C_ID 0x26→0x2D`、`PLAY_KEEPALIVE_C2S_ID 0x1A→0x1C`。另 `mc-server/src/play.rs:44,68` 注释里的 `0x2B/0x26/0x1A` 同步改。
- 缺（4 组，Configuration）：Finish Configuration CB `0x03` / Ack SB `0x03` / Registry Data CB `0x07` / Known Packs CB `0x0F` + SB `0x07`。P4b 新增常量+最小握手（Known Packs 双向 + Registry Data + Finish）时引入。
- 本文件只做对照，不改任何 `.rs`；`cargo check/test` 未跑（按任务要求不跑，但未动代码故不破坏）。

> 勘误（P10）：Update Tags CB 在777是 **0x0E**（0x0D是feature_flags，26.1残留）。见config.rs常量注释三方证据。

## 6. Play 挖掘/放置（P17 新增，均已按 777 核对）

> 来源：本页 Play 包表（clientbound `block_update` 8/`block_changed_ack` 4/`section_blocks_update` 86；
> serverbound `player_action` 41/`use_item_on` 66/`use_item` 67/`punch` 46/`set_carried_item` 54/`set_creative_mode_slot` 57）。
> 三方证据：26.1 `protocol.json` 对应项（`block_update` CB 0x07、`player_action` SB 0x28、
> `use_item_on` SB 0x41、`use_item` SB 0x42）+1 漂移恰好对上 777，互相印证。

| 方向 | 包 | 777 ID | 线格式（本服实现） |
|---|---|---|---|
| C2S | Player Action（挖/取消/完成/扔） | `0x29` | Status VarInt + Position + Face Byte + Sequence VarInt；26.3 jar ordinal：0=开始/1=换朝向（忽略）/2=取消(ABORT)/3=完成(STOP)/**4=扔整组(DROP_ALL_ITEMS，P19)/5=扔1个(DROP_ITEM，P19，原版Q)**/6=释放/7=换手/8=穿刺（无世界影响）。旧文档曾误写 1=取消/2=完成，导致松开鼠标即破坏（P18.2 修） |
| C2S | Use Item On（放置） | `0x42` | Hand VarInt + Position + Face VarInt + 3×Float + 2×Bool + Sequence VarInt |
| C2S | Use Item（空手使用） | `0x43` | Hand VarInt + Sequence VarInt + Yaw/Pitch Float（只 ack） |
| C2S | Punch（挥拳） | `0x2E` | 空包体，只忽略 |
| S2C | Block Update（单方块） | `0x08` | Position + blockstate VarInt（全局 palette ID） |
| S2C | Acknowledge Block Change | `0x04` | Sequence VarInt（确认客户端预测；拒绝时先发 BlockUpdate 回滚） |
| S2C | Update Section Blocks | `0x56` | P17 未用（单方块用 0x08 已足够；跨 section 批量留待多人/大编辑） |
| C2S | Set Held Item / Set Creative Mode Slot | `0x36` / `0x39` | P17 暂不解析；P18 起 `0x36` 按 Short 解析切手持（见 §7），`0x39` 仍忽略（Slot 的 data component 需注册表驱动解码，见 HANDOFF §5） |

## 7. Play 掉落物/背包（P18 新增，均已按 777 核对）

> 来源：本页 Play 包表（clientbound `add_entity` 1/`set_entity_data` 101/`remove_entities` 78/
> `entity_position_sync` 35/`move_entity_pos` 54/`take_item_entity` 127/
> `container_set_content` 18/`container_set_slot` 20；serverbound `set_carried_item` 54/
> `container_click` 18）。
> 注：`0x36` 在 S2C 是 `move_entity_pos`（Update Entity Position），在 C2S 是
> `set_carried_item`（Set Held Item）——方向不同，包体完全不同，不要混。
> C2S SetHeld 是 **Short BE**（0..=8），S2C 同名 `set_held_slot`（0x6B）是 VarInt。
> `0x12` 同理：S2C=`container_set_content`，C2S=`container_click`。

| 方向 | 包 | 777 ID | 线格式（本服实现） |
|---|---|---|---|
| S2C | Spawn Entity（掉落物，type=`minecraft:item`=72） | `0x01` | VarInt entity_id + UUID(16B) + VarInt type(72) + 3×Double + LpVec3 + 3×Angle + VarInt data(0) |
| S2C | Set Entity Data（物品栈元数据） | `0x65` | VarInt entity_id + [0x08 + VarInt 7 + Slot] + 0xFF |
| S2C | Remove Entities（despawn/拾取） | `0x4E` | Prefixed Array VarInt |
| S2C | Teleport Entity（`entity_position_sync`，线性路径） | `0x23` | VarInt id + VarInt path(0) + 3×Double + Float yaw + Float pitch + Bool on_ground |
| S2C | Update Entity Position（`move_entity_pos`，step=0） | `0x36` | VarInt id + VarInt props(bit0 on_ground，高位 step 数=0) + 3×Short（差量×4096） |
| S2C | Pickup Item（拾取动画） | `0x7F` | 3×VarInt（collected/collector=LoginPlay entity_id 1/count） |
| S2C | Set Container Content（背包全量，window 0=46 槽） | `0x12` | VarInt window(0) + VarInt state + Prefixed Array Slot + Slot carried(空) |
| S2C | Set Container Slot（背包单槽） | `0x14` | VarInt window(0) + VarInt state + Short slot + Slot |
| C2S | Set Held Item（切手持） | `0x36` | Short BE 0..=8（越界/截断忽略不断线） |
| C2S | Click Container / Set Creative Mode Slot | `0x12` / `0x39` | P18.1 起 `0x12` 解析头字段并服务端权威应用（PICKUP 拿起/放下/合并/交换、SHIFT 整搬、数字键交换；**P19 起 THROW=4 背包开屏按 Q：button 0 扔 1 个、1 扔整栈，要求光标为空否则无操作**；changed/carried 按 jar 线格式跳过不校验，点后必全量同步回滚分叉）；`0x39` 仍忽略 |

Slot 线格式（普通物品）：count VarInt（0=空，后无字段）；否则 item_id VarInt +
add数 VarInt(0) + remove数 VarInt(0)。带 component 的解码报错（避免错位整条流）。
LpVec3 见 Data types#LpVec3（零向量编 1 字节 0x00；wiki (1,0,-1) 等样本向量有单测断言）。
Entity metadata：index u8(8) + type VarInt(7=Slot) + Slot + 0xFF（Item 实体的物品栏）。
`minecraft:item` 在 EntityType 注册表 = **72**；物品 ID（26.3 客户端 jar
`BuiltInRegistries.ITEM` 实测，`C:\Users\qwe\AppData\Local\Temp\ItemIds.java`，未提交）：

| 物品 | item ID | 物品 | item ID |
|---|---|---|---|
| stone | 1 | dirt | 55 |
| cobblestone | 62 | oak_sapling | 77 |
| sand | 88 | oak_log | 163 |
| oak_leaves | 215 | coal | 1010 |
| stick | 1060 | raw_iron | 1017 |

entity：`minecraft:item`=72，`minecraft:experience_orb`=50，`minecraft:player`=159；
ITEM 注册表共 1658 项，ENTITY_TYPE 共 161 项。
