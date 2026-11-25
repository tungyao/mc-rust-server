# 任务库（agent 真相源，替代手写 MEMORY/HANDOFF 追踪）

协议：不要直接手改 JSON，用 `python scripts/task.py <cmd>`，它做原子写 + 双写事件日志。

- `tasks/*.json`：一任务一文件，git 可 diff/可审。`todo → doing → done|blocked`，只有 `claim` 能拿 `todo` 的活。
- `tasks/events.jsonl`：结构化事件流（`ts/agent/task/status/detail/files`），机器可查。旧 `MEMORY.md` 已一次性迁移进来，之后只追加，不手改。
- `MEMORY.md`：过渡期双写保留（人类可读），新 agent 只读 `tasks/` + `events.jsonl`。

常用：

```powershell
python scripts/task.py list
python scripts/task.py list --status todo
python scripts/task.py show p30-xp-orbs
python scripts/task.py new --id p30-xp-orbs --title "XP orbs" --priority p1
python scripts/task.py claim p30-xp-orbs --owner main
python scripts/task.py done p30-xp-orbs --detail "orb entity + pickup" --files "mc-world/items.rs"
python scripts/task.py block p30-xp-orbs --detail "等 777 TakeItem 抓包"
python scripts/task.py log --task p30-xp-orbs --limit 20
python scripts/task.py migrate  # 重跑 MEMORY.md -> events.jsonl（幂等，按行去重）
```
