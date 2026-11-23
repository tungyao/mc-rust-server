//! Play 脚手架：Login 队列 → "ready to join" + 每秒 KeepAlive 计数
//! 包 ID 取 `packets::PLAY_*` 现有常量，已按P4A对照表核对777。
use bevy_ecs::prelude::*;
use std::net::SocketAddr;
use tracing::info;

use crate::TickCounter;

/// Play 脚手架状态：net 线程交来的登录排队，P3 接真实 Player 实体。
/// P3后半 TODO：`NewConnection` 目前只带 addr（profile/socket 句柄留在 net 线程，
/// 真下发走 `mc_network::login_route::enter_play`）；要把 socket 从 net 线程交到 ECS，
/// 需扩展 channel 信封携带 `PlayerProfile` + socket 句柄（或 tokio mpsc 发包通道），
/// 届时本队列升级为 `Vec<PlayerProfile>`，真下发从 net 侧移到 ECS 侧。
#[derive(Resource, Default, Debug)]
pub struct PlayScaffold {
    /// channel 收到的 NewConnection 按到达顺序排队（替代 P2 的 u64 计数器）。
    pub pending_login: Vec<SocketAddr>,
    /// 已 log "ready to join" 的数量。
    pub joined: u64,
    /// KeepAlive 自增序号（S2C payload）。
    pub keepalive_seq: i64,
}

/// Login 成功队列排空：`handle_new_connections` 把 NewConnection 入队后，
/// 本系统逐个触发一次 Join 下发（当前为 log，socket 真发包在 net 线程已做）。
/// TODO(P3后半)：socket 句柄交接进 ECS 后，在此真写 Join Game / Set Center Chunk /
///
/// Chunk Data / Player Position Sync，包 ID 已按P4A对照表核对777。
pub fn login_join_system(mut play: ResMut<PlayScaffold>) {
    if play.pending_login.is_empty() {
        return;
    }
    let batch = std::mem::take(&mut play.pending_login);
    for addr in batch {
        play.joined += 1;
        // 脚手架自检：出生点 chunk 能取到网络字节（只算长度，不发送）。
        let spawn = mc_world::SPAWN_POS;
        let chunk = mc_world::chunk_for(mc_core::ChunkPos(
            spawn.0.div_euclid(16),
            spawn.2.div_euclid(16),
        ));
        let net_bytes = mc_world::chunk_network_bytes(&chunk);
        info!(
            "ready to join #{} addr={} spawn=({},{},{}) chunk_bytes={} (join已由net线程enter_play真下发; TODO socket句柄进ECS后在此真写 Join 0x32 + Chunk + Position Sync)",
            play.joined,
            addr,
            spawn.0,
            spawn.1,
            spawn.2,
            net_bytes.len()
        );
        // TODO(P3后半)：socket 从 net 线程交到 ECS 后，在此真下发 Join Game /
        // Set Center Chunk / Chunk Data / Player Position Sync，包 ID 已按P4A对照表核对777。
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
        "keepalive seq={} tick={} bytes={} (TODO: 接socket写, s2c 0x2D/c2s 0x1C 已按P4A对照表核对777)",
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
    fn pending_login_is_a_real_queue() {
        // 真实队列：两个不同 addr 入队 → 一次 drain 触发两次 join，不丢身份。
        let play = PlayScaffold {
            pending_login: vec![
                "127.0.0.1:10001".parse().unwrap(),
                "127.0.0.1:10002".parse().unwrap(),
            ],
            ..Default::default()
        };
        let mut world = World::new();
        world.insert_resource(play);
        let mut sched = Schedule::default();
        sched.add_systems(login_join_system);
        sched.run(&mut world);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.joined, 2);
        assert!(play.pending_login.is_empty());
        // 空队列再跑一次：joined 不变。
        sched.run(&mut world);
        assert_eq!(world.get_resource::<PlayScaffold>().unwrap().joined, 2);
    }
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
