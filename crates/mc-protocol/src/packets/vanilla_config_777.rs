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

/// 32 个 registry_data 包体，顺序与原版 SYNCHRONIZED_REGISTRIES 一致。
pub static VANILLA_CONFIG_REGISTRY_BODIES_777: &[(&str, &[u8])] = &[
    ("minecraft:worldgen/biome", include_bytes!("data/vanilla_config_777/registry_00.bin")),
    ("minecraft:chat_type", include_bytes!("data/vanilla_config_777/registry_01.bin")),
    ("minecraft:trim_pattern", include_bytes!("data/vanilla_config_777/registry_02.bin")),
    ("minecraft:trim_material", include_bytes!("data/vanilla_config_777/registry_03.bin")),
    ("minecraft:wolf_variant", include_bytes!("data/vanilla_config_777/registry_04.bin")),
    ("minecraft:wolf_sound_variant", include_bytes!("data/vanilla_config_777/registry_05.bin")),
    ("minecraft:pig_variant", include_bytes!("data/vanilla_config_777/registry_06.bin")),
    ("minecraft:pig_sound_variant", include_bytes!("data/vanilla_config_777/registry_07.bin")),
    ("minecraft:frog_variant", include_bytes!("data/vanilla_config_777/registry_08.bin")),
    ("minecraft:cat_variant", include_bytes!("data/vanilla_config_777/registry_09.bin")),
    ("minecraft:cat_sound_variant", include_bytes!("data/vanilla_config_777/registry_10.bin")),
    ("minecraft:cow_sound_variant", include_bytes!("data/vanilla_config_777/registry_11.bin")),
    ("minecraft:cow_variant", include_bytes!("data/vanilla_config_777/registry_12.bin")),
    ("minecraft:chicken_sound_variant", include_bytes!("data/vanilla_config_777/registry_13.bin")),
    ("minecraft:chicken_variant", include_bytes!("data/vanilla_config_777/registry_14.bin")),
    ("minecraft:zombie_nautilus_variant", include_bytes!("data/vanilla_config_777/registry_15.bin")),
    ("minecraft:painting_variant", include_bytes!("data/vanilla_config_777/registry_16.bin")),
    ("minecraft:sulfur_cube_archetype", include_bytes!("data/vanilla_config_777/registry_17.bin")),
    ("minecraft:dimension_type", include_bytes!("data/vanilla_config_777/registry_18.bin")),
    ("minecraft:damage_type", include_bytes!("data/vanilla_config_777/registry_19.bin")),
    ("minecraft:banner_pattern", include_bytes!("data/vanilla_config_777/registry_20.bin")),
    ("minecraft:enchantment", include_bytes!("data/vanilla_config_777/registry_21.bin")),
    ("minecraft:jukebox_song", include_bytes!("data/vanilla_config_777/registry_22.bin")),
    ("minecraft:instrument", include_bytes!("data/vanilla_config_777/registry_23.bin")),
    ("minecraft:test_environment", include_bytes!("data/vanilla_config_777/registry_24.bin")),
    ("minecraft:test_instance", include_bytes!("data/vanilla_config_777/registry_25.bin")),
    ("minecraft:dialog", include_bytes!("data/vanilla_config_777/registry_26.bin")),
    ("minecraft:world_clock", include_bytes!("data/vanilla_config_777/registry_27.bin")),
    ("minecraft:timeline", include_bytes!("data/vanilla_config_777/registry_28.bin")),
    ("minecraft:decorated_pot_pattern", include_bytes!("data/vanilla_config_777/registry_29.bin")),
    ("minecraft:block_transformer", include_bytes!("data/vanilla_config_777/registry_30.bin")),
    ("minecraft:worldgen/block_state_provider", include_bytes!("data/vanilla_config_777/registry_31.bin")),
];

/// 同一次抓取的 Update Tags 0x0E 包体（15 个注册表）。
pub static VANILLA_CONFIG_UPDATE_TAGS_BODY_777: &[u8] =
    include_bytes!("data/vanilla_config_777/update_tags.bin");
