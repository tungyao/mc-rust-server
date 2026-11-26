//! player_visibility: P43 多人玩家实体互见（只兼容 26.3/777）。
//!
//! 设计：每个连接仍用 LoginPlay 的 `entity_id=1` 作为自己的实体（客户端本地
//! 玩家不能由服务端 Spawn，wiki §Spawn Entity 明确禁止）；对**其它**连接则
//! 用一个高位全局唯一 id 生成 `minecraft:player`(159) 实体。所有包经 P21
//! `BroadcastHub` 的 per-conn 队列写给视者，不写操作者自己的 socket：
//! - Join：`sync_all` 让新连接看到视距内旧玩家、旧连接看到新玩家，per-viewer
//!   known 集去重（Duplicate Spawn 会被客户端替换实体，必须避免）。
//! - 移动：0x1E/0x1F → `move_entity_pos`(54)/`move_entity_pos_rot`(55)；
//!   0x20/0x21 → `move_entity_rot`(57)。差量超 i16 时回退绝对坐标
//!   `entity_position_sync`(35)，不猜字节、不做 Ack。
//! - 视距进出：每个连接自己的 50ms 节拍 `sync_viewer` 补 Spawn / 补 Remove。
//! - 断线：`broadcast_leave` 在 `hub.unregister` 之前发 RemoveEntities(78)。
//!
//! 包体来源：`crates/mc-protocol/src/packets/play_entity.rs`（wiki 26.3/777
//! §Spawn Entity / §Object data / §Update Entity Position[ and Rotation] /
//! §Update Entity Rotation），包 ID 与实体类型全部来自 `data/777` 生成表。

use crate::broadcast::{chunk_of_pos, chunk_in_view, BroadcastHub, PlayerEntityState};
use mc_protocol::packets;

/// 对外玩家实体 id 起点：高位独立计数器，永不与 `DropState::alloc_id`（1000 起）
/// 或客户端自身 id=1 冲突；理论上限 2^31，远超现实连接数。
pub const FIRST_PLAYER_ENTITY_ID: i32 = 1_000_000_000;
static NEXT_PLAYER_ENTITY_ID: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(FIRST_PLAYER_ENTITY_ID);

/// 分配一个全服唯一、永不复用的对外玩家实体 id。
pub fn next_player_entity_id() -> i32 {
    NEXT_PLAYER_ENTITY_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn spawn_body(state: &PlayerEntityState) -> Vec<u8> {
    let mut body = Vec::new();
    packets::SpawnPlayerEntity {
        entity_id: state.entity_id,
        uuid: state.uuid,
        x: state.pos.0,
        y: state.pos.1,
        z: state.pos.2,
        vel: packets::LpVec3::zero(),
        pitch: packets::angle_byte(state.pitch),
        yaw: packets::angle_byte(state.yaw),
        head_yaw: packets::angle_byte(state.yaw),
    }
    .encode_body(&mut body);
    body
}

/// Join 注册后：把该玩家 Spawn 给视距内的其它连接（每个视者只发一次）。
pub fn spawn_to_viewers(hub: &BroadcastHub, owner: u64, state: &PlayerEntityState) {
    let body = spawn_body(state);
    for viewer in hub.recipients_in_view(owner, chunk_of_pos(state.pos)) {
        if hub.mark_player_known(viewer, state.entity_id) {
            hub.unicast(viewer, packets::PLAY_SPAWN_ENTITY_S2C_ID, body.clone());
        }
    }
}

fn in_view_of(viewer_pos: (f64, f64, f64), radius: i32, state: &PlayerEntityState) -> bool {
    chunk_in_view(chunk_of_pos(viewer_pos), radius, chunk_of_pos(state.pos))
}

/// 单连接视距同步：视者已知但已出视距 → Remove（并 unmark）；
/// 视距内但未知 → Spawn（mark）。与 item/orb 的 `DropState.known` 完全隔离。
pub fn sync_viewer(hub: &BroadcastHub, viewer: u64) {
    let Some((viewer_pos, radius)) = hub.conn_pos(viewer) else { return };
    let states = hub.player_states();
    let mut by_id = std::collections::HashMap::new();
    for (_, state) in &states {
        by_id.insert(state.entity_id, *state);
    }

    let gone: Vec<i32> = hub.player_known_ids(viewer)
        .into_iter()
        .filter(|id| by_id.get(id).map(|s| !in_view_of(viewer_pos, radius, s)).unwrap_or(true))
        .collect();
    if !gone.is_empty() {
        let mut body = Vec::new();
        packets::RemoveEntities { ids: gone.clone() }.encode_body(&mut body);
        hub.unicast(viewer, packets::PLAY_REMOVE_ENTITIES_S2C_ID, body);
        for id in gone {
            hub.unmark_player_known(viewer, id);
        }
    }
    for (owner, state) in states {
        if owner == viewer || !in_view_of(viewer_pos, radius, &state) {
            continue;
        }
        if hub.mark_player_known(viewer, state.entity_id) {
            hub.unicast(viewer, packets::PLAY_SPAWN_ENTITY_S2C_ID, spawn_body(&state));
        }
    }
}

/// Join / 周期性兜底：对所有连接跑一遍视距同步（幂等，known 集去重）。
pub fn sync_all(hub: &BroadcastHub) {
    for viewer in hub.conn_ids() {
        sync_viewer(hub, viewer);
    }
}

fn delta_fits(old: f64, new: f64) -> bool {
    ((new - old) * 4096.0).round().abs() <= i16::MAX as f64
}

fn teleport_body(state: &PlayerEntityState, yaw: f32, pitch: f32, on_ground: bool) -> Vec<u8> {
    let mut body = Vec::new();
    packets::TeleportEntitySync {
        entity_id: state.entity_id,
        x: state.pos.0,
        y: state.pos.1,
        z: state.pos.2,
        yaw,
        pitch,
        on_ground,
    }
    .encode_body(&mut body);
    body
}

/// 移动包 → 视者位置/朝向包。只发给**已 Spawn 过该玩家**的视者；
/// 还没见过的视者由 `sync_viewer` 用当前位置补 Spawn。Ack 绝不走这里。
pub fn broadcast_movement(
    hub: &BroadcastHub,
    conn_id: u64,
    movement: packets::C2SMovement,
    on_ground: bool,
) {
    let Some(state) = hub.player_state(conn_id) else { return };
    let (new_pos, new_yaw, new_pitch) = match movement {
        packets::C2SMovement::Pos(p) => ((p.x, p.y, p.z), state.yaw, state.pitch),
        packets::C2SMovement::PosRot(p) => ((p.x, p.y, p.z), p.yaw, p.pitch),
        packets::C2SMovement::Rot(p) => (state.pos, p.yaw, p.pitch),
        packets::C2SMovement::Flags(_) => (state.pos, state.yaw, state.pitch),
    };

    let dx = packets::UpdateEntityPos::delta(state.pos.0, new_pos.0);
    let dy = packets::UpdateEntityPos::delta(state.pos.1, new_pos.1);
    let dz = packets::UpdateEntityPos::delta(state.pos.2, new_pos.2);
    let fits = delta_fits(state.pos.0, new_pos.0)
        && delta_fits(state.pos.1, new_pos.1)
        && delta_fits(state.pos.2, new_pos.2);
    let (packet_id, body) = match movement {
        packets::C2SMovement::Pos(_) => {
            if fits {
                let mut body = Vec::new();
                packets::UpdateEntityPos { entity_id: state.entity_id, on_ground, dx, dy, dz }
                    .encode_body(&mut body);
                (packets::PLAY_UPDATE_ENTITY_POS_S2C_ID, body)
            } else {
                (packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID, teleport_body(&state, new_yaw, new_pitch, on_ground))
            }
        }
        packets::C2SMovement::PosRot(_) => {
            if fits {
                let mut body = Vec::new();
                packets::UpdateEntityPosRot {
                    entity_id: state.entity_id,
                    on_ground,
                    dx,
                    dy,
                    dz,
                    yaw: packets::angle_byte(new_yaw),
                    pitch: packets::angle_byte(new_pitch),
                }
                .encode_body(&mut body);
                (packets::PLAY_UPDATE_ENTITY_POS_ROT_S2C_ID, body)
            } else {
                (packets::PLAY_ENTITY_POSITION_SYNC_S2C_ID, teleport_body(&state, new_yaw, new_pitch, on_ground))
            }
        }
        packets::C2SMovement::Rot(_) | packets::C2SMovement::Flags(_) => {
            let mut body = Vec::new();
            packets::UpdateEntityRot {
                entity_id: state.entity_id,
                on_ground,
                yaw: packets::angle_byte(new_yaw),
                pitch: packets::angle_byte(new_pitch),
            }
            .encode_body(&mut body);
            (packets::PLAY_UPDATE_ENTITY_ROT_S2C_ID, body)
        }
    };

    for viewer in hub.recipients_in_view(conn_id, chunk_of_pos(new_pos)) {
        if hub.is_player_known(viewer, state.entity_id) {
            hub.unicast(viewer, packet_id, body.clone());
        }
    }
    hub.update_player_pose(conn_id, new_pos, new_yaw, new_pitch);
}

/// 断线清理：在 `hub.unregister` 之前调用，给所有已 Spawn 过该玩家的视者发
/// RemoveEntities(0x4E)，并逐个 unmark（与 item/orb 的广播口径一致）。
pub fn broadcast_leave(hub: &BroadcastHub, conn_id: u64) {
    let Some(state) = hub.player_state(conn_id) else { return };
    let mut body = Vec::new();
    packets::RemoveEntities { ids: vec![state.entity_id] }.encode_body(&mut body);
    for viewer in hub.conn_ids() {
        if viewer == conn_id {
            continue;
        }
        if hub.is_player_known(viewer, state.entity_id) {
            hub.unicast(viewer, packets::PLAY_REMOVE_ENTITIES_S2C_ID, body.clone());
            hub.unmark_player_known(viewer, state.entity_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::Receiver;

    fn hub_pair() -> (BroadcastHub, Receiver<crate::QueuedPacket>, Receiver<crate::QueuedPacket>) {
        let hub = BroadcastHub::new();
        let (atx, arx) = crossbeam_channel::unbounded();
        let (btx, brx) = crossbeam_channel::unbounded();
        hub.register(1, atx, 2, (0.0, 64.0, 0.0));
        hub.register(2, btx, 2, (1.0, 64.0, 0.0));
        (hub, arx, brx)
    }

    fn enable(hub: &BroadcastHub, conn: u64, id: i32, pos: (f64, f64, f64)) {
        hub.enable_player(conn, PlayerEntityState {
            entity_id: id,
            uuid: id as u128,
            pos,
            yaw: 0.0,
            pitch: 0.0,
        });
    }

    #[test]
    fn player_ids_are_unique() {
        let a = next_player_entity_id();
        let b = next_player_entity_id();
        assert!(a >= FIRST_PLAYER_ENTITY_ID);
        assert!(b > a, "external player ids must never repeat");
        assert_ne!(a, 1);
        assert_ne!(a, crate::PLAYER_ENTITY_ID);
        assert!(a >= 1_000_000_000, "must never collide with drop ids (>=1000)");
    }

    #[test]
    fn join_spawns_once_and_moves_only_to_known_viewers() {
        let (hub, arx, brx) = hub_pair();
        let a = 1_000_010_000;
        let b = 1_000_010_001;
        enable(&hub, 1, a, (0.0, 64.0, 0.0));
        enable(&hub, 2, b, (1.0, 64.0, 0.0));
        sync_all(&hub);
        let sa = arx.try_recv().unwrap();
        assert_eq!(sa.id, packets::PLAY_SPAWN_ENTITY_S2C_ID);
        let sp = packets::SpawnPlayerEntity::decode_body(&sa.body).unwrap().0;
        assert_eq!(sp.entity_id, b);
        let sb = brx.try_recv().unwrap();
        assert_eq!(sb.id, packets::PLAY_SPAWN_ENTITY_S2C_ID);
        assert_eq!(packets::SpawnPlayerEntity::decode_body(&sb.body).unwrap().0.entity_id, a);
        // 再同步一次：known 集去重，无重复 Spawn。
        sync_all(&hub);
        assert!(arx.try_recv().is_err());
        assert!(brx.try_recv().is_err());

        broadcast_movement(
            &hub,
            2,
            packets::C2SMovement::Pos(packets::MovePlayerPos {
                x: 1.5, y: 64.0, z: 0.0, on_ground: true, horizontal_collision: false,
            }),
            true,
        );
        let mv = arx.try_recv().unwrap();
        assert_eq!(mv.id, packets::PLAY_UPDATE_ENTITY_POS_S2C_ID);
        let pos = packets::UpdateEntityPos::decode_body(&mv.body).unwrap().0;
        assert_eq!(pos.entity_id, b);
        assert_eq!(pos.dx, 2048);
        assert!(brx.try_recv().is_err(), "movement must not go back to the actor");
    }

    #[test]
    fn leave_sends_remove_only_to_viewers_that_know_the_player() {
        let (hub, arx, brx) = hub_pair();
        let a = 1_000_020_000;
        enable(&hub, 1, a, (0.0, 64.0, 0.0));
        // 视者 2 在视距内、3 在视距外（100 格外）。
        let (ctx, crx) = crossbeam_channel::unbounded();
        hub.register(3, ctx, 2, (100.0, 64.0, 100.0));
        sync_all(&hub);
        assert!(brx.try_recv().is_ok());
        assert!(crx.try_recv().is_err());
        broadcast_leave(&hub, 1);
        let rm = brx.try_recv().unwrap();
        assert_eq!(rm.id, packets::PLAY_REMOVE_ENTITIES_S2C_ID);
        assert_eq!(packets::RemoveEntities::decode_body(&rm.body).unwrap().0.ids, vec![a]);
        assert!(arx.try_recv().is_err(), "leaving player must not be notified about itself");
        assert!(crx.try_recv().is_err());
        hub.unregister(1);
        assert!(hub.player_state(1).is_none());
        assert!(!hub.is_player_known(2, a));
    }

    #[test]
    fn view_exit_and_reentry_send_remove_then_respawn() {
        let (hub, _arx, brx) = hub_pair();
        let a = 1_000_030_000;
        enable(&hub, 1, a, (0.0, 64.0, 0.0));
        sync_all(&hub);
        assert!(brx.try_recv().is_ok());
        // A 走出视者 2 的半径（>2 区块）。
        hub.update_player_pose(1, (100.0, 64.0, 100.0), 0.0, 0.0);
        sync_viewer(&hub, 2);
        let rm = brx.try_recv().unwrap();
        assert_eq!(rm.id, packets::PLAY_REMOVE_ENTITIES_S2C_ID);
        assert_eq!(packets::RemoveEntities::decode_body(&rm.body).unwrap().0.ids, vec![a]);
        assert!(!hub.is_player_known(2, a));
        // 走回来补 Spawn。
        hub.update_player_pose(1, (1.0, 64.0, 0.0), 0.0, 0.0);
        sync_viewer(&hub, 2);
        let sp = brx.try_recv().unwrap();
        assert_eq!(sp.id, packets::PLAY_SPAWN_ENTITY_S2C_ID);
        assert_eq!(packets::SpawnPlayerEntity::decode_body(&sp.body).unwrap().0.entity_id, a);
    }
}
