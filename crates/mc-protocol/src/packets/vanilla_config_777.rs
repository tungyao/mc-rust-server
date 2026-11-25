//! 26.3/777 原版 Configuration 全量 registry_data 包体（32 个同步注册表，按原版顺序）。
//! 来源：本机 26.3 vanilla server，`select_known_packs` 回空包，抓取服务端实际下发的
//! 0x07 registry_data 包体（见 HANDOFF §9）。条目顺序即客户端注册表 ID 顺序，
//! 因此 Update Tags 的 VarInt ID 与之严格自洽；不要单独替换其中某个包。
//! `update_tags.bin` 是同一次抓取的 0x0E 包体（15 个注册表、19662 项）。
//!
//! 用法：Configuration 阶段按本数组顺序逐包发送，最后发 update_tags，再 Finish。
//! 这样客户端拿到的注册表 = 原版服务端（空 KnownPacks）下发的完整集合，
//! 不再依赖逐个手补 dimension/biome/damage_type 等最小条目。
//! 重新生成：见 HANDOFF.md §9 的抓包步骤；只读数据，不要手改。
//!
//! 包体索引由 `data/manifest.json` 提供，`build.rs` 编译期生成到 `$OUT_DIR`；
//! 本模块只负责引入生成结果，两个公开符号的名字/类型/顺序保持不变。

include!(concat!(env!("OUT_DIR"), "/vanilla_config_generated.rs"));
