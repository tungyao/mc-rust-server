# 线性记忆（唯一真相源，多agent实时追加）
# 协议：每个agent开工/完工都追加一行，格式：
# - [YYYY-MM-DD HH:MM UTC] agent=<名> task=<任务> status=<start/done/blocked> detail=<一句话> files=<改了啥>
# 用 `flock MEMORY.lock -c "echo ... >> MEMORY.md"` 追加，避免并发撕裂。

- [2026-11-23 01:20 UTC] agent=main task=init status=done detail=P0-P2骨架+26.3锁死 files=Cargo.toml/README/ARCHITECTURE
- [2026-11-23 01:22 UTC] agent=main task=split-1 status=done detail=packets 1172->7文件/network 301->6文件/server 188->5文件，每文件<=200 files=packets/*,network/*,server/*
