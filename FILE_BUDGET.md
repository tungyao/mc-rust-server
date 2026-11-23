# 文件大小预算（硬性）
- `crates/*/src/**/*.rs`：<=200行（含注释/空行，按 `wc -l` 计）
- `crates/*/tests/*.rs`：<=250行
- 超了必须拆：按阶段/职责拆成 `mod`（参考 `packets/` 拆分：codec/handshake/status/login/play_*）
- 检查：`bash scripts/check_budget.sh`（CI/提交前必跑，agent完工自查）

例外：`build.rs` 生成代码不计，但手写部分<=200行。
