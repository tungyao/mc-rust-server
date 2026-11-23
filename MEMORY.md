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
