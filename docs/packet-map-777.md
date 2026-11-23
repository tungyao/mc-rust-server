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

## 5. 结论（给 P4b 的改单，不在本文件改代码）

- 对（11 个）：Handshake `0x00`；Status 4 个（`0x00/0x00/0x01/0x01`）；Login 6 个（Start `0x00` / EncReq `0x01` / EncResp `0x01` / Success `0x02` / Ack `0x03` / SetCompression `0x03`）。不动。
- 错（6 个，全在 Play）：`PLAY_LOGIN_PLAY_ID 0x2B→0x32`、`PLAY_SET_CENTER_CHUNK_ID 0x4C→0x60`、`PLAY_CHUNK_DATA_ID 0x27→0x2E`、`PLAY_PLAYER_POSITION_SYNC_ID 0x3E→0x49`、`PLAY_KEEPALIVE_S2C_ID 0x26→0x2D`、`PLAY_KEEPALIVE_C2S_ID 0x1A→0x1C`。另 `mc-server/src/play.rs:44,68` 注释里的 `0x2B/0x26/0x1A` 同步改。
- 缺（4 组，Configuration）：Finish Configuration CB `0x03` / Ack SB `0x03` / Registry Data CB `0x07` / Known Packs CB `0x0F` + SB `0x07`。P4b 新增常量+最小握手（Known Packs 双向 + Registry Data + Finish）时引入。
- 本文件只做对照，不改任何 `.rs`；`cargo check/test` 未跑（按任务要求不跑，但未动代码故不破坏）。
