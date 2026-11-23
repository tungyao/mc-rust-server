//! movement: Position + MovementQueue + KeepAlive超时，只兼容26.3/777
//! P5a只做移动，不管视距和存档。超界/超速只log不踢，P6再接反作弊。
//! Position字段与C2S移动包（`packets::play_sync::MovePlayer*`）对齐：
//! x/y/z/yaw/pitch/on_ground。
use bevy_ecs::prelude::*;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::resources::{PlayEventReceiver, PlayersOnline};

/// 世界边界（原版静默clamp值）：x/z ±30M，y ±20M。超界只warn不踢。
pub const WORLD_BORDER_XZ: f64 = 30_000_000.0;
pub const WORLD_BORDER_Y: f64 = 20_000_000.0;
/// 单次更新超速阈值（P5a宽松占位，只warn不踢，P6接反作弊）。
pub const MAX_MOVE_PER_UPDATE: f64 = 100.0;
/// KeepAlive超时：>15s无回包踢出并减online计数。
pub const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(15);

/// 服务端玩家位置（与C2S移动包字段对齐）。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl Default for Position {
    fn default() -> Self {
        let s = mc_world::SPAWN_POS;
        Self {
            x: s.0 as f64 + 0.5,
            y: s.1 as f64,
            z: s.2 as f64 + 0.5,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
        }
    }
}

/// 玩家KeepAlive活跃度：net线程收到C2S KeepAlive回显/移动包后经 `PlayEvent` 通道进ECS，
/// 由 `movement_ingest_system` 刷新 `last_seen`；在线数只在 timeout 系统减一次。
#[derive(Component, Debug, Clone)]
pub struct PlayerKeepAlive {
    pub last_seen: Instant,
}

impl Default for PlayerKeepAlive {
    fn default() -> Self {
        Self { last_seen: Instant::now() }
    }
}

/// 网络连接地址 → ECS 玩家实体的映射键（net 线程只带 addr 投递事件）。
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerAddr(pub SocketAddr);

/// Tick待应用的移动更新（net线程经channel投递后压入这里，P5a先由测试/占位压入）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MovementUpdate {
    Pos { x: f64, y: f64, z: f64, on_ground: bool },
    PosRot { x: f64, y: f64, z: f64, yaw: f32, pitch: f32, on_ground: bool },
    Rot { yaw: f32, pitch: f32, on_ground: bool },
    Flags { on_ground: bool },
}

impl From<mc_protocol::packets::C2SMovement> for MovementUpdate {
    fn from(m: mc_protocol::packets::C2SMovement) -> Self {
        match m {
            mc_protocol::packets::C2SMovement::Pos(v) => Self::Pos {
                x: v.x,
                y: v.y,
                z: v.z,
                on_ground: v.on_ground,
            },
            mc_protocol::packets::C2SMovement::PosRot(v) => Self::PosRot {
                x: v.x,
                y: v.y,
                z: v.z,
                yaw: v.yaw,
                pitch: v.pitch,
                on_ground: v.on_ground,
            },
            mc_protocol::packets::C2SMovement::Rot(v) => Self::Rot {
                yaw: v.yaw,
                pitch: v.pitch,
                on_ground: v.on_ground,
            },
            mc_protocol::packets::C2SMovement::Flags(v) => Self::Flags {
                on_ground: v.on_ground,
            },
        }
    }
}

/// Tick待应用队列：`(Entity, MovementUpdate)`。net侧 `PlayEvent::Movement` 经 channel
/// 由 `movement_ingest_system` 按 addr 找到实体后压入，`movement_system` 排空并更新 Position。
#[derive(Resource, Default, Debug)]
pub struct MovementQueue(pub Vec<(Entity, MovementUpdate)>);

/// 排空 net 线程投递的 Play 事件（IO-free）：
/// - Movement：addr → Entity，转换 `MovementUpdate` 后压入 MovementQueue（下个 system 应用）；
/// - KeepAlive：刷新 PlayerKeepAlive.last_seen（在线数仍只在 timeout 系统减）。
pub fn movement_ingest_system(
    events: Res<PlayEventReceiver>,
    mut queue: ResMut<MovementQueue>,
    addr_query: Query<(Entity, &PlayerAddr)>,
    mut keepalive_query: Query<&mut PlayerKeepAlive>,
) {
    if events.0.is_empty() {
        return;
    }
    let mut by_addr = std::collections::HashMap::new();
    for (ent, addr) in addr_query.iter() {
        by_addr.insert(addr.0, ent);
    }
    while let Ok(ev) = events.0.try_recv() {
        match ev {
            mc_network::PlayEvent::Movement { addr, movement } => match by_addr.get(&addr) {
                Some(&ent) => {
                    queue.0.push((ent, MovementUpdate::from(movement)));
                    // 移动包同样证明连接存活：26.3 客户端即使站立也约 1s 发一次，
                    // 用它刷新可避免 “15s 心跳周期 == 15s 超时” 的临界竞态；
                    // net 线程仍按 KeepAlive 回显严格执行 15s 踢出。
                    if let Ok(mut ka) = keepalive_query.get_mut(ent) {
                        ka.last_seen = Instant::now();
                    }
                }
                None => debug!("movement from unknown addr {addr} ignored"),
            },
            mc_network::PlayEvent::KeepAlive { addr } => match by_addr.get(&addr) {
                Some(&ent) => {
                    if let Ok(mut ka) = keepalive_query.get_mut(ent) {
                        ka.last_seen = Instant::now();
                    }
                }
                None => debug!("keepalive from unknown addr {addr} ignored"),
            },
        }
    }
}

fn finite_or_warn(x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> bool {
    if !x.is_finite() || !y.is_finite() || !z.is_finite() || !yaw.is_finite() || !pitch.is_finite() {
        warn!("movement ignored: non-finite x={x} y={y} z={z} yaw={yaw} pitch={pitch} (P6 kick, P5a only log)");
        return false;
    }
    true
}

/// 单条更新应用到Position：超界/超速只warn不踢（P6再做反作弊）。
/// 返回是否应用（NaN/无限直接忽略，其余clamp后应用）。
pub fn apply_movement_update(pos: &mut Position, upd: &MovementUpdate) -> bool {
    match *upd {
        MovementUpdate::Pos { x, y, z, on_ground } => {
            if !finite_or_warn(x, y, z, pos.yaw, pos.pitch) {
                return false;
            }
            check_bounds_and_speed(pos.x, pos.y, pos.z, x, y, z);
            let (cx, cy, cz) = clamp_to_border(x, y, z);
            pos.x = cx;
            pos.y = cy;
            pos.z = cz;
            pos.on_ground = on_ground;
            true
        }
        MovementUpdate::PosRot { x, y, z, yaw, pitch, on_ground } => {
            if !finite_or_warn(x, y, z, yaw, pitch) {
                return false;
            }
            check_bounds_and_speed(pos.x, pos.y, pos.z, x, y, z);
            let (cx, cy, cz) = clamp_to_border(x, y, z);
            pos.x = cx;
            pos.y = cy;
            pos.z = cz;
            pos.yaw = yaw;
            pos.pitch = pitch;
            pos.on_ground = on_ground;
            true
        }
        MovementUpdate::Rot { yaw, pitch, on_ground } => {
            if !finite_or_warn(pos.x, pos.y, pos.z, yaw, pitch) {
                return false;
            }
            pos.yaw = yaw;
            pos.pitch = pitch;
            pos.on_ground = on_ground;
            true
        }
        MovementUpdate::Flags { on_ground } => {
            pos.on_ground = on_ground;
            true
        }
    }
}

fn clamp_to_border(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let cx = x.clamp(-WORLD_BORDER_XZ, WORLD_BORDER_XZ);
    let cy = y.clamp(-WORLD_BORDER_Y, WORLD_BORDER_Y);
    let cz = z.clamp(-WORLD_BORDER_XZ, WORLD_BORDER_XZ);
    if cx != x || cy != y || cz != z {
        warn!("movement out of bounds x={x} y={y} z={z} clamped to ({cx},{cy},{cz}) (P5a only log, P6 kick)");
    }
    (cx, cy, cz)
}

fn check_bounds_and_speed(px: f64, py: f64, pz: f64, x: f64, y: f64, z: f64) {
    let dx = x - px;
    let dy = y - py;
    let dz = z - pz;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
    if dist > MAX_MOVE_PER_UPDATE {
        warn!("movement too fast dist={dist:.1} from ({px:.1},{py:.1},{pz:.1}) to ({x:.1},{y:.1},{z:.1}) (P5a only log, P6 anticheat)");
    }
}

/// Tick排空MovementQueue并更新Position（IO-free，只碰ECS）。
pub fn movement_system(mut queue: ResMut<MovementQueue>, mut query: Query<&mut Position>) {
    if queue.0.is_empty() {
        return;
    }
    let batch = std::mem::take(&mut queue.0);
    for (ent, upd) in batch {
        match query.get_mut(ent) {
            Ok(mut pos) => {
                if apply_movement_update(&mut pos, &upd) {
                    debug!(
                        "movement applied entity={ent:?} pos=({:.2},{:.2},{:.2}) yaw={:.1} on_ground={}",
                        pos.x, pos.y, pos.z, pos.yaw, pos.on_ground
                    );
                }
            }
            Err(_) => {
                warn!("movement for missing entity {ent:?} ignored");
            }
        }
    }
}

/// KeepAlive超时踢出：last_seen超过15s即despawn并减online计数（饱和减防underflow）。
pub fn keepalive_timeout_system(
    mut commands: Commands,
    online: Res<PlayersOnline>,
    query: Query<(Entity, &PlayerKeepAlive)>,
) {
    let now = Instant::now();
    for (ent, ka) in query.iter() {
        if now.duration_since(ka.last_seen) > KEEPALIVE_TIMEOUT {
            info!("keepalive timeout >15s, despawn {ent:?}");
            commands.entity(ent).despawn();
            let n = online.0.load(Ordering::SeqCst);
            if n > 0 {
                online.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn spawn_player(world: &mut World, pos: Position, last_seen: Instant) -> Entity {
        world.spawn((pos, PlayerKeepAlive { last_seen })).id()
    }

    #[test]
    fn tick_applies_queued_movement() {
        let mut world = World::new();
        world.insert_resource(MovementQueue::default());
        let ent = spawn_player(&mut world, Position::default(), Instant::now());
        world
            .get_resource_mut::<MovementQueue>()
            .unwrap()
            .0
            .push((ent, MovementUpdate::PosRot {
                x: 10.5,
                y: 81.0,
                z: 10.5,
                yaw: 90.0,
                pitch: 0.0,
                on_ground: true,
            }));
        let mut sched = Schedule::default();
        sched.add_systems(movement_system);
        sched.run(&mut world);
        let pos = world.get::<Position>(ent).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (10.5, 81.0, 10.5));
        assert_eq!(pos.yaw, 90.0);
        assert!(pos.on_ground);
        assert!(world.get_resource::<MovementQueue>().unwrap().0.is_empty());
    }

    #[test]
    fn out_of_bounds_and_speed_only_log_not_kick() {
        let mut world = World::new();
        world.insert_resource(MovementQueue::default());
        let ent = spawn_player(&mut world, Position::default(), Instant::now());
        world
            .get_resource_mut::<MovementQueue>()
            .unwrap()
            .0
            .push((ent, MovementUpdate::Pos {
                x: 100_000_000.0,
                y: 80.0,
                z: 0.0,
                on_ground: false,
            }));
        let mut sched = Schedule::default();
        sched.add_systems(movement_system);
        sched.run(&mut world);
        // 实体仍在（不踢），坐标被clamp到边界
        let pos = world.get::<Position>(ent).unwrap();
        assert_eq!(pos.x, WORLD_BORDER_XZ);
        // 非法NaN直接忽略但也不踢
        world
            .get_resource_mut::<MovementQueue>()
            .unwrap()
            .0
            .push((ent, MovementUpdate::Pos {
                x: f64::NAN,
                y: 80.0,
                z: 0.0,
                on_ground: false,
            }));
        sched.run(&mut world);
        assert!(world.get::<Position>(ent).is_some());
    }

    #[test]
    fn keepalive_timeout_despawns_and_decrements_online() {
        let mut world = World::new();
        let online = PlayersOnline(Arc::new(std::sync::atomic::AtomicUsize::new(1)));
        world.insert_resource(online);
        let old = spawn_player(
            &mut world,
            Position::default(),
            Instant::now() - Duration::from_secs(16),
        );
        let fresh = spawn_player(&mut world, Position::default(), Instant::now());
        let mut sched = Schedule::default();
        sched.add_systems(keepalive_timeout_system);
        sched.run(&mut world);
        assert!(world.get_entity(old).is_none(), "timed out entity must be despawned");
        assert!(world.get_entity(fresh).is_some(), "fresh entity stays");
        assert_eq!(
            world.get_resource::<PlayersOnline>().unwrap().0.load(Ordering::SeqCst),
            0
        );
        // 再跑一次不underflow
        sched.run(&mut world);
        assert_eq!(
            world.get_resource::<PlayersOnline>().unwrap().0.load(Ordering::SeqCst),
            0
        );
    }

    #[test]
    fn c2s_movement_converts_to_update() {
        let m = mc_protocol::packets::C2SMovement::PosRot(
            mc_protocol::packets::MovePlayerPosRot {
                x: 1.0,
                y: 2.0,
                z: 3.0,
                yaw: 10.0,
                pitch: 20.0,
                on_ground: true,
                horizontal_collision: false,
            },
        );
        let u = MovementUpdate::from(m);
        assert_eq!(
            u,
            MovementUpdate::PosRot { x: 1.0, y: 2.0, z: 3.0, yaw: 10.0, pitch: 20.0, on_ground: true }
        );
    }

    #[test]
    fn play_event_movement_reaches_queue_and_updates_position() {
        use bevy_ecs::schedule::IntoSystemConfigs;
        let addr: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        let mut world = World::new();
        let online = PlayersOnline(Arc::new(std::sync::atomic::AtomicUsize::new(1)));
        world.insert_resource(online);
        world.insert_resource(MovementQueue::default());
        let (tx, rx) = crossbeam_channel::unbounded();
        world.insert_resource(PlayEventReceiver(rx));
        let ent = world
            .spawn((PlayerAddr(addr), Position::default(), PlayerKeepAlive::default()))
            .id();
        // 先只发移动包：它也必须刷新 last_seen（26.3 站立时也约 1s 一包），
        // 否则 15s KeepAlive 周期与 15s 超时会在临界点误踢。
        world.get_mut::<PlayerKeepAlive>(ent).unwrap().last_seen =
            Instant::now() - Duration::from_secs(16);
        tx.send(mc_network::PlayEvent::Movement {
            addr,
            movement: mc_protocol::packets::C2SMovement::Pos(
                mc_protocol::packets::MovePlayerPos {
                    x: 384.5,
                    y: 65.0,
                    z: 381.5,
                    on_ground: true,
                    horizontal_collision: false,
                },
            ),
        })
        .unwrap();
        let mut sched = Schedule::default();
        sched.add_systems((movement_ingest_system, movement_system, keepalive_timeout_system).chain());
        sched.run(&mut world);
        let pos = world.get::<Position>(ent).unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (384.5, 65.0, 381.5));
        assert!(pos.on_ground);
        assert!(world.get_resource::<MovementQueue>().unwrap().0.is_empty());
        assert!(world.get_entity(ent).is_some(), "movement event must refresh last_seen");
        assert_eq!(
            world.get_resource::<PlayersOnline>().unwrap().0.load(Ordering::SeqCst),
            1,
            "movement/keepalive events must not change online count"
        );
        // 再验证 KeepAlive 回显同样刷新；两条路径都不重复减 online。
        world.get_mut::<PlayerKeepAlive>(ent).unwrap().last_seen =
            Instant::now() - Duration::from_secs(16);
        tx.send(mc_network::PlayEvent::KeepAlive { addr }).unwrap();
        sched.run(&mut world);
        assert!(world.get_entity(ent).is_some(), "keepalive event must refresh last_seen");
        assert_eq!(
            world.get_resource::<PlayersOnline>().unwrap().0.load(Ordering::SeqCst),
            1
        );
    }

    #[test]
    fn play_event_from_unknown_addr_is_ignored() {
        let mut world = World::new();
        world.insert_resource(MovementQueue::default());
        let (tx, rx) = crossbeam_channel::unbounded();
        world.insert_resource(PlayEventReceiver(rx));
        let known: SocketAddr = "127.0.0.1:40001".parse().unwrap();
        let unknown: SocketAddr = "127.0.0.1:40002".parse().unwrap();
        let ent = world
            .spawn((PlayerAddr(known), Position::default(), PlayerKeepAlive::default()))
            .id();
        tx.send(mc_network::PlayEvent::Movement {
            addr: unknown,
            movement: mc_protocol::packets::C2SMovement::Flags(
                mc_protocol::packets::MovePlayerFlags { on_ground: true, horizontal_collision: false },
            ),
        })
        .unwrap();
        let mut sched = Schedule::default();
        sched.add_systems(movement_ingest_system);
        sched.run(&mut world);
        assert!(world.get_resource::<MovementQueue>().unwrap().0.is_empty());
        assert!(!world.get::<Position>(ent).unwrap().on_ground);
    }
}
