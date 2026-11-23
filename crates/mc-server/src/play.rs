//! Play 脚手架：Login 占位 → "ready to join" + 每秒 KeepAlive 计数
//! 不真发包（只 log），TODO 接 socket 写。包 ID 待26.3抓包核对。
use bevy_ecs::prelude::*;
use tracing::info;

use crate::TickCounter;

/// Play 脚手架状态（内存占位，P3 接真实 Player 实体/socket 句柄）。
#[derive(Resource, Default, Debug)]
pub struct PlayScaffold {
    /// channel 收到的 NewConnection 尚未走完 Login 占位的数量。
    pub pending_login: u64,
    /// 已 log "ready to join" 的数量。
    pub joined: u64,
    /// KeepAlive 自增序号（S2C payload）。
    pub keepalive_seq: i64,
}

/// Login 成功占位：`handle_new_connections` 把 NewConnection 计数搬到这里后，
/// 本系统逐个 log "ready to join"，模拟 Login Play → Chunk → 出生点流程入口。
/// P3 在此接真实 Login 成功事件 + Join Game/Chunk Data 下发。
pub fn login_join_system(mut play: ResMut<PlayScaffold>) {
    if play.pending_login == 0 {
        return;
    }
    let n = play.pending_login;
    play.pending_login = 0;
    for _ in 0..n {
        play.joined += 1;
        // 脚手架自检：出生点 chunk 能取到网络字节（只算长度，不发送）。
        let spawn = mc_world::SPAWN_POS;
        let chunk = mc_world::chunk_for(mc_core::ChunkPos(
            spawn.0.div_euclid(16),
            spawn.2.div_euclid(16),
        ));
        let net_bytes = mc_world::chunk_network_bytes(&chunk);
        info!(
            "ready to join #{} spawn=({},{},{}) chunk_bytes={} (login placeholder; TODO: Join Game 0x2B + Chunk Data + Position Sync)",
            play.joined,
            spawn.0,
            spawn.1,
            spawn.2,
            net_bytes.len()
        );
        // TODO(P3): Login 成功后下发 Join Game / Set Center Chunk / Chunk Data /
        // Player Position Sync，包 ID 待26.3抓包核对。
    }
}

/// 每秒（20 tick）一次 KeepAlive 计数：只 log 不真发，留 TODO 接 socket 写。
/// 用 TickCounter 判断整秒，避免在 tick 里做 IO。
pub fn keepalive_system(tick: Res<TickCounter>, mut play: ResMut<PlayScaffold>) {
    if tick.0 == 0 || tick.0 % 20 != 0 {
        return;
    }
    play.keepalive_seq += 1;
    // 脚手架 encode 自检：确保包能编码，但不发送。
    let mut buf = Vec::new();
    mc_protocol::packets::encode_keepalive_s2c_packet(play.keepalive_seq, &mut buf);
    info!(
        "keepalive seq={} tick={} bytes={} (TODO: 接socket写, s2c 0x26/c2s 0x1A 待26.3抓包核对)",
        play.keepalive_seq,
        tick.0,
        buf.len()
    );
    // TODO(P3): 遍历在线 Player 的 socket 写 KeepAlive S2C，并处理 C2S 超时踢出。
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_encode_does_not_panic() {
        // Join Game + 中心 chunk + 空 chunk + 出生同步 + KeepAlive 全走一遍 encode
        let login = mc_protocol::packets::LoginPlay::default();
        let mut buf = Vec::new();
        mc_protocol::packets::encode_login_play_packet(&login, &mut buf);
        assert!(!buf.is_empty());

        let center = mc_protocol::packets::SetCenterChunk { chunk_x: 0, chunk_z: 0 };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_set_center_chunk_packet(&center, &mut buf);
        assert!(!buf.is_empty());

        let chunk = mc_world::chunk_for(mc_core::ChunkPos(0, 0));
        let section_bytes = mc_world::chunk_network_bytes(&chunk);
        let data = mc_protocol::packets::PlayChunkData {
            chunk_x: 0,
            chunk_z: 0,
            heightmaps_nbt: mc_protocol::packets::minimal_empty_nbt_bytes(),
            section_bytes,
        };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_play_chunk_data_packet(&data, &mut buf);
        assert!(!buf.is_empty());

        let spawn = mc_world::SPAWN_POS;
        let sync = mc_protocol::packets::PlayerPositionSync {
            x: spawn.0 as f64 + 0.5,
            y: spawn.1 as f64,
            z: spawn.2 as f64 + 0.5,
            yaw: 0.0,
            pitch: 0.0,
            flags: 0,
            teleport_id: 1,
        };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_player_position_sync_packet(&sync, &mut buf);
        assert!(!buf.is_empty());

        let mut buf = Vec::new();
        mc_protocol::packets::encode_keepalive_s2c_packet(1, &mut buf);
        assert!(!buf.is_empty());
    }
}
