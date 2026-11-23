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
