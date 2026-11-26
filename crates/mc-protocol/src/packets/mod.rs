//! packets: 26.3/777 包定义，按阶段拆分，每文件<=200行
//! - codec: 基础编解码 + 组帧
//! - handshake/status/login/config: 各阶段包
//! - play_*: Play包（ID已按P4A对照表核对777）
//! 兼容：`packets::X` 路径不变（全部 pub use 重导出）。
mod block_tags_777;
pub mod codec;
pub mod config;
pub mod handshake;
pub mod login;
pub mod vanilla_config_777;
// nbt.rs 在 src/ 顶层（Registry/区块多处共用，不专属于 packets 阶段）；
// 这里用 #[path] 接进来，保持 `packets::nbt` / `packets::NbtTag` 路径不变。
#[path = "../nbt.rs"]
pub mod nbt;
pub mod play_block;
pub mod play_chunk;
pub mod play_entity;
pub mod play_login;
pub mod play_sync;
pub mod status;

// ---------------------------------------------------------------------------
// 编译期 ID 查表（P2-A 做法，与 mc-world 的 registry_id 一致）：
// mc_registry::packet_id / entity_id 内部用 format! 做短名补全，不是 const fn；
// 这里直接在 build.rs 从 data/777/*.json 生成的静态表上做等价的 const 线性查找，
// 使包 ID / 实体类型 ID 既是编译期常量，又随数据 codegen 自动漂移。
// 名称缺失时编译期 panic，不允许静默回退到硬编码值。
// ---------------------------------------------------------------------------

/// 字节级 const 字符串比较（`str == str` 依赖的 `PartialEq` 不是 const fn）。
const fn str_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `mc_registry::packet_id_in(state, name)` 的 const 版本。
/// `state` 形如 `"clientbound.play"` / `"serverbound.login"`。
pub(crate) const fn packet_id(state: &str, name: &str) -> i32 {
    let table = mc_registry::PACKETS;
    let mut i = 0;
    while i < table.len() {
        let (s, n, id) = table[i];
        if str_eq(s, state) && str_eq(n, name) {
            return id;
        }
        i += 1;
    }
    panic!("mc-registry generated PACKETS table is missing a required packet");
}

/// `mc_registry::entity_id(name)` 的 const 版本（此处只使用 `minecraft:` 全名）。
pub(crate) const fn entity_id(name: &str) -> i32 {
    let table = mc_registry::ENTITIES;
    let mut i = 0;
    while i < table.len() {
        let (id, n) = table[i];
        if str_eq(n, name) {
            return id as i32;
        }
        i += 1;
    }
    panic!("mc-registry generated ENTITIES table is missing a required entity");
}

pub use codec::*;
pub use config::*;
pub use handshake::*;
pub use login::*;
pub use nbt::*;
pub use play_block::*;
pub use play_chunk::*;
pub use play_entity::*;
pub use play_login::*;
pub use play_sync::*;
pub use status::*;

#[cfg(test)]
mod registry_id_tests {
    /// 所有包 ID 常量必须与 `mc-registry` 由 data/777/packets.json 生成的表一致（防漂移）。
    #[test]
    fn packet_id_constants_match_generated_table() {
        let cases: &[(i32, &str, &str)] = &[
            (super::HANDSHAKE_PACKET_ID, "serverbound.handshake", "intention"),
            (super::LOGIN_START_PACKET_ID, "serverbound.login", "hello"),
            (super::LOGIN_SUCCESS_PACKET_ID, "clientbound.login", "login_finished"),
            (super::LOGIN_ACKNOWLEDGED_PACKET_ID, "serverbound.login", "login_acknowledged"),
            (super::SET_COMPRESSION_PACKET_ID, "clientbound.login", "login_compression"),
            (super::ENCRYPTION_REQUEST_PACKET_ID, "clientbound.login", "hello"),
            (super::ENCRYPTION_RESPONSE_PACKET_ID, "serverbound.login", "key"),
            (super::STATUS_REQUEST_PACKET_ID, "serverbound.status", "status_request"),
            (super::STATUS_RESPONSE_PACKET_ID, "clientbound.status", "status_response"),
            (super::PING_PACKET_ID, "serverbound.status", "ping_request"),
            (super::PONG_PACKET_ID, "clientbound.status", "pong_response"),
            (super::CONFIG_FINISH_S2C_ID, "clientbound.config", "finish_configuration"),
            (super::CONFIG_FINISH_ACK_C2S_ID, "serverbound.config", "finish_configuration"),
            (super::CONFIG_REGISTRY_DATA_S2C_ID, "clientbound.config", "registry_data"),
            (super::CONFIG_KNOWN_PACKS_S2C_ID, "clientbound.config", "select_known_packs"),
            (super::CONFIG_KNOWN_PACKS_C2S_ID, "serverbound.config", "select_known_packs"),
            (super::CONFIG_UPDATE_TAGS_S2C_ID, "clientbound.config", "update_tags"),
            (super::PLAY_PLAYER_ACTION_C2S_ID, "serverbound.play", "player_action"),
            (super::PLAY_USE_ITEM_ON_C2S_ID, "serverbound.play", "use_item_on"),
            (super::PLAY_USE_ITEM_C2S_ID, "serverbound.play", "use_item"),
            (super::PLAY_PUNCH_C2S_ID, "serverbound.play", "punch"),
            (super::PLAY_CLICK_CONTAINER_C2S_ID, "serverbound.play", "container_click"),
            (super::PLAY_SET_HELD_ITEM_C2S_ID, "serverbound.play", "set_carried_item"),
            (super::PLAY_SET_CREATIVE_SLOT_C2S_ID, "serverbound.play", "set_creative_mode_slot"),
            (super::PLAY_BLOCK_UPDATE_S2C_ID, "clientbound.play", "block_update"),
            (super::PLAY_BLOCK_CHANGED_ACK_S2C_ID, "clientbound.play", "block_changed_ack"),
            (super::PLAY_SET_CENTER_CHUNK_ID, "clientbound.play", "set_chunk_cache_center"),
            (super::PLAY_CHUNK_DATA_ID, "clientbound.play", "level_chunk_with_light"),
            (super::PLAY_UNLOAD_CHUNK_ID, "clientbound.play", "forget_level_chunk"),
            (super::PLAY_CHUNK_BATCH_START_ID, "clientbound.play", "chunk_batch_start"),
            (super::PLAY_CHUNK_BATCH_FINISHED_ID, "clientbound.play", "chunk_batch_finished"),
            (super::PLAY_GAME_EVENT_ID, "clientbound.play", "game_event"),
            (super::PLAY_SPAWN_ENTITY_S2C_ID, "clientbound.play", "add_entity"),
            (super::PLAY_SET_ENTITY_DATA_S2C_ID, "clientbound.play", "set_entity_data"),
            (super::PLAY_REMOVE_ENTITIES_S2C_ID, "clientbound.play", "remove_entities"),
            (super::PLAY_ENTITY_POSITION_SYNC_S2C_ID, "clientbound.play", "entity_position_sync"),
            (super::PLAY_UPDATE_ENTITY_POS_S2C_ID, "clientbound.play", "move_entity_pos"),
            (super::PLAY_PICKUP_ITEM_S2C_ID, "clientbound.play", "take_item_entity"),
            (super::PLAY_SET_CONTAINER_CONTENT_S2C_ID, "clientbound.play", "container_set_content"),
            (super::PLAY_SET_CONTAINER_SLOT_S2C_ID, "clientbound.play", "container_set_slot"),
            (super::PLAY_LOGIN_PLAY_ID, "clientbound.play", "login"),
            (super::PLAY_PLAYER_POSITION_SYNC_ID, "clientbound.play", "player_position"),
            (super::PLAY_KEEPALIVE_S2C_ID, "clientbound.play", "keep_alive"),
            (super::PLAY_KEEPALIVE_C2S_ID, "serverbound.play", "keep_alive"),
            (super::PLAY_MOVE_POS_C2S_ID, "serverbound.play", "move_player_pos"),
            (super::PLAY_MOVE_POSROT_C2S_ID, "serverbound.play", "move_player_pos_rot"),
            (super::PLAY_MOVE_ROT_C2S_ID, "serverbound.play", "move_player_rot"),
            (super::PLAY_MOVE_FLAGS_C2S_ID, "serverbound.play", "move_player_status_only"),
            (super::PLAY_SET_HEALTH_S2C_ID, "clientbound.play", "set_health"),
        ];
        for &(constant, state, name) in cases {
            assert_eq!(
                mc_registry::packet_id_in(state, name),
                Some(constant),
                "packet ID drift: {state}/{name}"
            );
        }
    }

    #[test]
    fn entity_position_sync_id_is_0x23() {
        assert_eq!(super::PLAY_ENTITY_POSITION_SYNC_S2C_ID, 0x23);
    }

    #[test]
    fn item_entity_type_id_matches_generated_table() {
        assert_eq!(super::ITEM_ENTITY_TYPE_ID, 72);
        assert_eq!(
            mc_registry::entity_id("minecraft:item"),
            Some(super::ITEM_ENTITY_TYPE_ID as u32)
        );
    }
}
