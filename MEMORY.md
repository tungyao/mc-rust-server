# 线性记忆（唯一真相源，多agent实时追加）
# 协议：每个agent开工/完工都追加一行，格式：
# - [YYYY-MM-DD HH:MM UTC] agent=<名> task=<任务> status=<start/done/blocked> detail=<一句话> files=<改了啥>
# 用 `flock MEMORY.lock -c "echo ... >> MEMORY.md"` 追加，避免并发撕裂。

- [2026-11-23 01:20 UTC] agent=main task=init status=done detail=P0-P2骨架+26.3锁死 files=Cargo.toml/README/ARCHITECTURE
- [2026-11-23 01:22 UTC] agent=main task=split-1 status=done detail=packets 1172->7文件/network 301->6文件/server 188->5文件，每文件<=200 files=packets/*,network/*,server/*
- [2026-11-23 02:00 UTC] agent=main task=P3-merge status=done detail=A压缩+B真下发+C世界数据合完 41测试全过 files=login/framing/login_route/play/systems/registry/world
- [2026-11-23 02:10 UTC] agent=main task=P4A-merge status=done detail=包表核对11对6错4缺 已合docs files=docs/packet-map-777.md
- [2026-11-23 02:10 UTC] agent=main task=P4A-merge status=done detail=包表核对11对6错4缺 已合docs files=docs/packet-map-777.md
- [2026-11-23 03:00 UTC] agent=p4b task=config-stage status=start detail=Configuration骨架+Finish往返 files=packets/config.rs,packets/mod.rs,network/login_route.rs
- [2026-11-23 04:00 UTC] agent=p4b task=id-fix status=start detail=按P4A表改6个Play ID+Config常量 files=packets/play_*.rs,packets/config.rs,tests
- [2026-11-23 04:30 UTC] agent=p4b task=id-fix status=done detail=6个Play ID+4个Config ID按P4A表改对，41测试全过 files=packets/play_*.rs,packets/config.rs,packets/mod.rs,server/play.rs,tests/login_flow.rs,tests/online_count.rs,network/login_route注释
- [2026-11-23 05:00 UTC] agent=main task=P5BC-merge status=done detail=P5B视距81块限流+P5C toml+Anvil占位合完 files=world/play/config
- [2026-11-23 05:00 UTC] agent=p5a task=movement status=start detail=C2S 0x1E-0x21+ movement.rs + play_loop last_seen files=packets/play_sync.rs,server/movement.rs,network/login_route.rs
- [2026-11-23 05:30 UTC] agent=p5a task=movement status=done detail=C2S 0x1E-0x21解码+ movement tick/15s超时踢减online + play_loop last_seen/move-log 52测试全过 files=packets/play_sync.rs(397),server/movement.rs(330),network/login_route.rs(296),server/main.rs接线
- [2026-11-23 06:00 UTC] agent=p6a task=registry-data status=start detail=KnownPacks+Registry最小下发+config_finish按序 files=packets/config.rs,network/login_route
- [2026-11-23 06:30 UTC] agent=p6a task=registry-data status=done detail=KnownPacks单vanilla+Registry最小2包+config_finish按序5步，52测试全过 files=packets/config.rs(269),network/login_route.rs(369),tests/login_flow.rs+online_count.rs
- [2026-11-23 06:00 UTC] agent=p6b task=real-chunk status=start detail=play_chunk真布局+world真实palette files=play_chunk.rs/world-lib
- [2026-11-23 06:30 UTC] agent=p6b task=real-chunk status=done detail=777真布局：结构化heightmap+fluid+无data_len+空光照，39测试全过 files=play_chunk.rs(369)/world-lib(259)
- [2026-11-23 07:00 UTC] agent=main task=login-finished-fix status=done detail=26.2加SessionID致777缺16字节，加session_id=nil，30+7全过 files=packets/login.rs
- [2026-11-23 07:20 UTC] agent=main task=config-tolerant status=done detail=穿插包忽略+15s超时，真机reset大概率是ClientInformation插队 files=network/login_route.rs
- [2026-11-23 07:30 UTC] agent=p7b task=real-registry status=start detail=真dimension/biome NBT+minimal带数据 files=packets/config.rs
- [2026-11-23 07:45 UTC] agent=p7b task=real-registry status=done detail=真overworld/plains NBT+minimal带数据+6单测全过 files=packets/config.rs(374)
- [2026-11-23 08:00 UTC] agent=main task=registry-wirefix status=done detail=wiki证伪：Data是PrefixedOptional裸NBT，去VarInt长+去根名00 00，35+24全过 files=packets/config.rs
- [2026-11-23 08:30 UTC] agent=p8 task=full-registry-docs status=start detail=777全量同步注册表文档+清单，不动代码 files=docs/registries-777.md
- [2026-11-23 08:50 UTC] agent=p8 task=full-registry-docs status=done detail=30表+最小NBT+实现清单约1440行，不动代码未跑cargo files=docs/registries-777.md
- [2026-11-23 09:00 UTC] agent=p9 task=registry-nbt-rewrite status=start detail=手拼NBT切nbt.rs builder按777§2.1/§2.2 files=crates/mc-protocol/src/packets/config.rs
- [2026-11-23 09:10 UTC] agent=p9 task=registry-nbt-rewrite status=done detail=builder重写2 NBT+删手拼helper+新字段断言，mc-protocol 35全过 files=crates/mc-protocol/src/packets/config.rs(392行)
- [2026-11-23 09:30 UTC] agent=p10 task=timeline-worldclock-tags status=start detail=dimension重写+timeline/world_clock注册表+UpdateTags files=config.rs,login_route.rs
- [2026-11-23 10:10 UTC] agent=p10 task=timeline-worldclock-tags status=done detail=4包+tags+0x0E勘误，mc-protocol 38+mc-network全过 files=config.rs(810)data/*.json(637)login_route.rs(406)
- [2026-11-23 08:10 UTC] agent=main task=dim-offset-fix status=done detail=offset改Float删3多余字段，向原版JSON对齐 files=config.rs
- [2026-11-23 12:30 UTC] agent=main task=P11-vanilla-registry-replay status=done detail=真机Configuration根因是block tag缺infiniburn_overworld；改为回放原版26.3全量32注册表+15注册表19662项UpdateTags；修LoginPlay/PositionSync 777布局；加KeepAlive/GameEvent/ChunkBatch files=config.rs,vanilla_config_777.rs,block_tags_777.rs,play_login.rs,play_sync.rs,play_chunk.rs,login_route.rs
- [2026-11-23 12:45 UTC] agent=main task=P12-own-world-chunks status=done detail=区块真实blockstate ID(air0/stone1/grass9/dirt10)+间接palette+真实MOTION_BLOCKING高度图+天空光BitSet(byte-array)=真机进Play、地形可见、无30s超时；一次同批81块满足hasAllNeighbors；删除原版区块抓包脚手架 files=world/lib.rs,play_chunk.rs,login_route.rs,play.rs
- [2026-11-23 12:46 UTC] agent=main task=P12-client-crash-note status=done detail=26.3客户端偶发jvm.dll 0xc0000005需-XX:StackShadowPages=32（原版启动器同样加），非服务端问题；真机坐标y65 on_ground、KeepAlive稳定
- [2026-11-23 13:20 UTC] agent=p13 task=compression+ecs-movement+dynamic-chunks status=done detail=压缩阈值配置化(server.toml/NetConfig默认-1，256真机进Play无Decoder/超时)；0x1E-0x21经PlayEvent进ECS MovementQueue、KeepAlive刷新last_seen且online单点减；跨区块动态补/卸radius4区块(batch_size=add.len)；修0x26为packed i64 ChunkPos(真机走250+格跨17+区块y65不超时)；21 test target/112 tests全过 files=mc-server(config/systems/movement/resources/main/play),mc-network(config/handshake/login_route/chunk_view/play_events/framing/lib/tests/play_stream),mc-protocol(play_chunk),server.toml,HANDOFF.md
- [2026-11-23 13:53 UTC] agent=p14 task=seed-worldgen status=done detail=新增WorldGen(3层平滑value noise，地表y58..94，草/3土/石/空气，逐列MOTION_BLOCKING，最低地表section向上天空光)；seed从ServerConfig→NetConfig.with_seed→handle_login_configured→enter_play_with_events→WorldGen全透传，ECS/play用同一ServerConfig.seed；出生x/z369/381，y=地表+1(默认73，seed987654321=83)；真机两seed均无Decoder/30s超时、地形明显不同；123 tests全过 files=mc-world(worldgen/lib),mc-protocol(play_chunk),mc-network(config/handshake/login_route/tests),mc-server(config/main/movement/play/systems),HANDOFF.md
- [2026-11-23 14:35 UTC] agent=p15 task=view-distance-cache-connection-lifecycle status=done detail=view_distance真正生效(clamp 2..=10→LoginPlay/初始批/跨区块/ECS；r10=441块764ms、r4=81块140ms、r2=25块)；ChunkCache(已编码0x2E包体FIFO 1024共享)；accept conn_id+Disconnect全退出路径一次+唯一player_removal_system与timeout去重(断线<1ms online=0，重连单实体)；真机r10跨区块add21/remove21、r4 add9/remove9、threshold256+r10 441块1359ms无Decoder/超时，142 tests全过 files=mc-network(chunk_view/chunk_cache/config/lib/handshake_route/login_route/play_events/tests),mc-server(config/main/movement/play/systems),HANDOFF.md
