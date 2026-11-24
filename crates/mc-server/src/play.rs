//! Play 脚手架：Login 队列 → "ready to join" 诊断 + KeepAlive 计数。
//! 包 ID 取 `packets::PLAY_*` 现有常量，已按 P4A 对照表核对 777。
//! 真 Join/Chunk/动态视距仍由 net 线程 `enter_play` + 共享 `ChunkCache` 完成；
//! ECS 只维护登录/视距诊断，不再生成或编码任何区块（P16.1 删除 P5b 编码自检）。
//! TODO(socket→ECS)：socket 句柄仍在 net 线程。未来若把 socket 交给 ECS，
//! 应使用 **per-player 写通道 + `mc_network::ChunkView`** 真发 Join/Chunk/PositionSync，
//! 不要把本轮删掉的“每 tick 全量生成/编码 1 块”脚手架恢复回来。
use bevy_ecs::prelude::*;
use tracing::info;

use crate::config::ServerConfig;
use crate::TickCounter;

/// Play 脚手架状态：net 线程交来的登录排队 + 最近一次登录的视距诊断。
/// P13：`systems::handle_new_connections` 已为每个登录 spawn
/// `PlayerAddr/PlayerConnId/Position/PlayerKeepAlive`，`PlayEvent` 经 channel 进 ECS。
/// P16.1：删除 `pending_chunks`/`sent_chunks`/`CHUNKS_PER_TICK` 与每 tick 的
/// WorldGen+ChunkData 编码/日志；真实发送早已在 net 线程完成，这段代码只浪费 CPU/日志。
#[derive(Resource, Default, Debug)]
pub struct PlayScaffold {
    /// channel 收到的 NewConnection 按到达顺序排队（addr + conn_id）。
    pub pending_login: Vec<mc_network::NewConnection>,
    /// 已 log "ready to join" 的数量。
    pub joined: u64,
    /// KeepAlive 自增序号（S2C payload）。
    pub keepalive_seq: i64,
    /// 当前视距中心（spawn 区块）；多玩家同中心时去重参考，未来按 Player 拆分。
    pub view_center: Option<mc_core::ChunkPos>,
    /// 最近一次登录使用的有效视距半径（clamp 2..=10）；0 表示还没有登录。
    pub last_view_radius: i32,
    /// 最近一次登录的 `(2r+1)^2` 块数；0 表示还没有登录。
    pub last_view_chunks: usize,
}

/// 出生点所在区块（视距中心）：x/z 固定 369/381 → chunk `(23,23)`，y 由 WorldGen 决定。
/// 保留为测试/诊断入口；`login_join_system` 用 `WorldGen::spawn_chunk()` 取得同一值。
#[allow(dead_code)]
pub fn spawn_center_chunk() -> mc_core::ChunkPos {
    mc_core::ChunkPos(
        mc_world::SPAWN_BLOCK_X.div_euclid(16),
        mc_world::SPAWN_BLOCK_Z.div_euclid(16),
    )
}

/// Login 成功队列排空 + 单条 "ready to join" 诊断日志。
/// 调用链（main.rs）：`handle_new_connections`（收 NewConnection + spawn ECS 实体）
/// → `movement_ingest_system`（PlayEvent）→ 本系统 → `movement_system`。
/// 与 net 线程使用同一个 `clamp_view_radius` / `view_chunk_count`，只记录有效视距，
/// 不调用 `WorldGen::generate`、不编码 ChunkData、不逐块打 info。
/// TODO(socket→ECS)：未来 socket 进 ECS 后，在此用 per-player 写通道 + `ChunkView`
/// 真发 Join/Chunk/PositionSync；不要恢复全量生成脚手架。
pub fn login_join_system(cfg: Res<ServerConfig>, mut play: ResMut<PlayScaffold>) {
    if play.pending_login.is_empty() {
        return;
    }
    // 与 net 线程 / ECS 使用同一个 ServerConfig.seed 和同一套有效视距 clamp。
    let world = mc_world::WorldGen::new(cfg.seed);
    let radius = mc_network::clamp_view_radius(cfg.view_distance);
    let chunk_count = mc_network::view_chunk_count(radius);
    let center = world.spawn_chunk();
    let spawn = world.spawn_pos();
    play.view_center = Some(center);
    play.last_view_radius = radius;
    play.last_view_chunks = chunk_count;
    let batch = std::mem::take(&mut play.pending_login);
    for conn in batch {
        play.joined += 1;
        info!(
            "ready to join #{} addr={} conn_id={} seed={} spawn=({},{},{}) center=({},{}) view_radius={} view_chunks={} (join 已由 net 线程 enter_play 真下发; TODO socket→ECS 时改用 per-player 写通道)",
            play.joined,
            conn.addr,
            conn.conn_id,
            cfg.seed,
            spawn.0,
            spawn.1,
            spawn.2,
            center.0,
            center.1,
            radius,
            chunk_count
        );
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

    fn scaffold_with(conns: &[(u16, u64)]) -> PlayScaffold {
        PlayScaffold {
            pending_login: conns
                .iter()
                .map(|&(port, conn_id)| mc_network::NewConnection {
                    addr: format!("127.0.0.1:{port}").parse().unwrap(),
                    conn_id,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn schedule() -> Schedule {
        let mut sched = Schedule::default();
        sched.add_systems(login_join_system);
        sched
    }

    #[test]
    fn pending_login_is_a_real_queue() {
        // 真实队列：两个不同 conn_id 入队 → 一次 drain 触发两次 join，不丢身份。
        let mut world = World::new();
        world.insert_resource(ServerConfig { view_distance: 2, ..Default::default() });
        world.insert_resource(scaffold_with(&[(10001, 1), (10002, 2)]));
        let mut sched = schedule();
        sched.run(&mut world);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.joined, 2);
        assert!(play.pending_login.is_empty());
        assert_eq!(play.view_center, Some(spawn_center_chunk()));
        assert_eq!(play.last_view_radius, 2);
        assert_eq!(play.last_view_chunks, 25);
        assert_eq!(play.last_view_chunks, mc_network::view_chunk_count(2));
    }

    #[test]
    fn view_distance_drives_ecs_view_diagnostics() {
        for (view_distance, radius, chunks) in
            [(2, 2, 25), (4, 4, 81), (10, 10, 441), (11, 10, 441)]
        {
            let mut world = World::new();
            world.insert_resource(ServerConfig { view_distance, ..Default::default() });
            world.insert_resource(scaffold_with(&[(21001, 3)]));
            let mut sched = schedule();
            sched.run(&mut world);
            let play = world.get_resource::<PlayScaffold>().unwrap();
            assert_eq!(play.joined, 1);
            assert_eq!(play.view_center, Some(spawn_center_chunk()));
            assert_eq!(play.last_view_radius, radius, "view_distance={view_distance}");
            assert_eq!(
                play.last_view_chunks, chunks,
                "view_distance={view_distance} must report (2r+1)^2 chunks"
            );
        }
    }

    #[test]
    fn empty_queue_reports_no_join() {
        let mut world = World::new();
        world.insert_resource(ServerConfig::default());
        world.insert_resource(PlayScaffold::default());
        let mut sched = schedule();
        sched.run(&mut world);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.joined, 0);
        assert_eq!(play.view_center, None);
        assert_eq!(play.last_view_radius, 0);
        assert_eq!(play.last_view_chunks, 0);
        assert!(play.pending_login.is_empty());
    }

    #[test]
    fn repeated_ticks_do_no_chunk_work() {
        // 登录后连续 10 tick：joined/视距诊断保持不变，ECS 不再生成/编码任何区块。
        let mut world = World::new();
        world.insert_resource(ServerConfig { view_distance: 10, ..Default::default() });
        world.insert_resource(scaffold_with(&[(20001, 4)]));
        let mut sched = schedule();
        sched.run(&mut world);
        let center = spawn_center_chunk();
        for _ in 0..10 {
            sched.run(&mut world);
            let play = world.get_resource::<PlayScaffold>().unwrap();
            assert_eq!(play.joined, 1);
            assert_eq!(play.view_center, Some(center));
            assert_eq!(play.last_view_radius, 10);
            assert_eq!(play.last_view_chunks, 441);
            assert!(play.pending_login.is_empty());
        }
        // 新登录只增加计数并刷新同一份诊断，不产生其它工作。
        {
            let mut play = world.get_resource_mut::<PlayScaffold>().unwrap();
            play.pending_login.push(mc_network::NewConnection {
                addr: "127.0.0.1:20002".parse().unwrap(),
                conn_id: 5,
            });
        }
        sched.run(&mut world);
        let play = world.get_resource::<PlayScaffold>().unwrap();
        assert_eq!(play.joined, 2);
        assert_eq!(play.last_view_radius, 10);
        assert_eq!(play.last_view_chunks, 441);
    }

    #[test]
    fn scaffold_encode_does_not_panic() {
        // Join Game + 中心 chunk + 生成区块 + 出生同步 + KeepAlive 单块编码自检
        let login = mc_protocol::packets::LoginPlay::default();
        let mut buf = Vec::new();
        mc_protocol::packets::encode_login_play_packet(&login, &mut buf);
        assert!(!buf.is_empty());

        let center = mc_protocol::packets::SetCenterChunk { chunk_x: 0, chunk_z: 0 };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_set_center_chunk_packet(&center, &mut buf);
        assert!(!buf.is_empty());

        let gen = mc_world::WorldGen::new(mc_world::DEFAULT_SEED);
        let chunk = gen.generate(mc_core::ChunkPos(0, 0));
        let data = mc_protocol::packets::PlayChunkData {
            chunk_x: 0,
            chunk_z: 0,
            heightmaps_nbt: chunk.heightmaps_bytes(),
            section_bytes: chunk.network_bytes(),
            light_bytes: chunk.light_bytes(),
        };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_play_chunk_data_packet(&data, &mut buf);
        assert!(!buf.is_empty());

        // Unload 编码自检（0x26 forget_level_chunk，不是 KeepAlive）
        let mut buf = Vec::new();
        mc_protocol::packets::encode_unload_chunk_packet(
            &mc_protocol::packets::UnloadChunk { chunk_x: 0, chunk_z: 0 },
            &mut buf,
        );
        assert!(!buf.is_empty());

        let spawn = gen.spawn_pos();
        let sync = mc_protocol::packets::PlayerPositionSync {
            teleport_id: 1,
            x: spawn.0 as f64 + 0.5,
            y: spawn.1 as f64,
            z: spawn.2 as f64 + 0.5,
            delta_x: 0.0,
            delta_y: 0.0,
            delta_z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            relatives: 0,
        };
        let mut buf = Vec::new();
        mc_protocol::packets::encode_player_position_sync_packet(&sync, &mut buf);
        assert!(!buf.is_empty());

        let mut buf = Vec::new();
        mc_protocol::packets::encode_keepalive_s2c_packet(1, &mut buf);
        assert!(!buf.is_empty());
    }
}
