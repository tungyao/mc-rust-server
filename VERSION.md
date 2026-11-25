# 版本策略：只兼容 26.3

- MC_VERSION = 26.3
- PROTOCOL_VERSION = 777
- DATA_VERSION = 5023
- Pack: resource 97.1 / data 121.0 (仅记录，不校验)

行为：
- Handshake 里 protocol != 777 直接断开，Status 里 version 固定写 26.3/777
- 不做 ViaVersion / 多版本兼容，不存旧数据升级逻辑
- 升级策略：等 26.4 出了再整体切数据文件 + codegen，不保留向后兼容

## 版本 ↔ 协议 ↔ 数据目录映射

| MC 版本 | protocol | data_version | 数据目录 | 数据来源 |
|---|---|---|---|---|
| 26.3 | 777 | 5023 | `data/777/` | Pumpkin 26.3 Extractor `assets/*.json`（URL / SHA 见 `data/777/SOURCE.md`） |

当前仅支持上表这一行。升级到新版本时追加一行，并把 `data/manifest.json`、
`crates/mc-core/src/lib.rs` 的版本常量一起切过去；**整体切换，不保留旧版本兼容**。

数据源说明与可复现的升级步骤（下载校验 → 精简表 → manifest → codegen → 手改包体布局 →
fixture 回归 → T1–T13 真机清单）见 `docs/data-driven.md`；
计划原文见 `docs/data-driven-plan.md`。
