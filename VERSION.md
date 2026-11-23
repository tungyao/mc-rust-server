# 版本策略：只兼容 26.3

- MC_VERSION = 26.3
- PROTOCOL_VERSION = 777
- DATA_VERSION = 5023
- Pack: resource 97.1 / data 121.0 (仅记录，不校验)

行为：
- Handshake 里 protocol != 777 直接断开，Status 里 version 固定写 26.3/777
- 不做 ViaVersion / 多版本兼容，不存旧数据升级逻辑
- 升级策略：等 26.4 出了再整体切数据文件 + codegen，不保留向后兼容
