//! block_edit: server-authoritative dig/place decide + apply (moved from login_route.rs P17 section, no logic change).
use super::login::{emit_play_event, write_packet_any, PlayOptions};
use crate::chunk_cache::ChunkCache;
use crate::play_events::PlayEvent;
use mc_protocol::packets;
use mc_world::MutableWorld;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::info;
use std::io;

// ---------------------------------------------------------------------------
// P17 方块编辑（服务端权威，net 线程即时判定 + 回包）。
//
// 约束：socket 所有权在 net 线程，ECS 没有写通道，因此权威判定必须和 socket
// 写在同一线程完成（判定规则仍是服务端规则：距离/越界/可挖/可放，客户端预测
// 只做参考，非法一律回滚重发）。ECS 通过 `MutableWorld` 共享看到同一份世界，
// 并负责定时/断线/退出刷盘（见 mc-server persist）。
// ---------------------------------------------------------------------------

/// 单方块变更：`new_id == old_id` 表示回滚重发（未落子）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockChange {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub old_id: u16,
    pub new_id: u16,
}

/// 权威判定结果：ack 序列号 + 要下发的 BlockUpdate 列表（空 = 只 ack）+
/// P18 掉落（仅挖掘成功时非空，调用方负责生成物品实体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEditOutcome {
    pub sequence: i32,
    pub changes: Vec<BlockChange>,
    pub drops: Vec<mc_world::Drop>,
}

pub(crate) fn eye_of(pos: (f64, f64, f64)) -> (f64, f64, f64) {
    (pos.0, pos.1 + mc_world::EYE_HEIGHT, pos.2)
}

fn eye_block_dist(eye: (f64, f64, f64), x: i32, y: i32, z: i32) -> f64 {
    let dx = eye.0 - (x as f64 + 0.5);
    let dy = eye.1 - (y as f64 + 0.5);
    let dz = eye.2 - (z as f64 + 0.5);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// 目标 chunk 是否在玩家视距内（切比雪夫）。
fn in_view(center: mc_core::ChunkPos, radius: i32, x: i32, z: i32) -> bool {
    let c = mc_world::chunk_of_block(x, z);
    (c.0 - center.0).abs() <= radius && (c.1 - center.1).abs() <= radius
}

fn rollback(world: &MutableWorld, x: i32, y: i32, z: i32) -> BlockChange {
    let cur = world.get(x, y, z);
    BlockChange { x, y, z, old_id: cur, new_id: cur }
}

/// 挖掘权威判定（Player Action 0x29；status 按 26.3 jar `Action` ordinal）。
/// - status 0（START 开始挖掘）：只校验并 ack，**不破坏方块**。原版生存模式下客户端会播放
///   破坏进度动画，进度满后才发 status 3；在这里即时破坏会导致“点一下就没”（与原版不一致）。
/// - status 3（STOP 完成挖掘）：校验通过后破坏方块并按掉落表带出 drops。
/// - status 2（ABORT 取消）：只 ack。status 1（CHANGE 朝向）：直接忽略（客户端不走预测）。
///   status 4+（丢弃/吃食/换手）：无世界影响，直接忽略。
/// - 非法（越界/超距/出视距/空气/水）：回滚重发现状；`world` 为 None 时忽略不断线。
/// - 完成挖掘用 `MutableWorld::set_if` 原子 CAS：两连接同时挖同一格只有一方成功并
///   生成掉落，另一方拿到回滚（现状）且 `drops` 为空。
/// 判定内已落子（`set_if`），调用方负责：缓存失效 + 发 BlockUpdate/Ack + 投递事件。
pub fn decide_dig(
    world: Option<&MutableWorld>,
    eye: (f64, f64, f64),
    center: mc_core::ChunkPos,
    radius: i32,
    act: &packets::PlayerAction,
) -> Option<BlockEditOutcome> {
    let world = world?;
    match act.status {
        packets::PLAYER_ACTION_START_DIG | packets::PLAYER_ACTION_FINISH_DIG => {
            let (x, y, z) = (act.x, act.y, act.z);
            let bad_range = !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&y);
            if bad_range
                || !in_view(center, radius, x, z)
                || eye_block_dist(eye, x, y, z) > mc_world::MAX_REACH
            {
                return Some(BlockEditOutcome {
                    sequence: act.sequence,
                    changes: vec![rollback(world, x, y, z)],
                    drops: Vec::new(),
                });
            }
            let cur = world.get(x, y, z);
            if cur == mc_world::AIR_BLOCK_ID || cur == mc_world::WATER_BLOCK_ID {
                return Some(BlockEditOutcome {
                    sequence: act.sequence,
                    changes: vec![BlockChange { x, y, z, old_id: cur, new_id: cur }],
                    drops: Vec::new(),
                });
            }
            // 开始挖掘：目标合法，只 ack；让客户端自己播放破坏进度动画。
            if act.status == packets::PLAYER_ACTION_START_DIG {
                return Some(BlockEditOutcome {
                    sequence: act.sequence,
                    changes: Vec::new(),
                    drops: Vec::new(),
                });
            }
            // 完成挖掘：原子条件写。另一个连接已挖掉/改掉此格时 CAS 失败，
            // 只回现状（不生成第二份掉落），避免 N 连接并发挖一格刷出 N 份物品。
            match world.set_if(x, y, z, cur, mc_world::AIR_BLOCK_ID) {
                Some(old) => {
                    let drops = mc_world::drops_for_block(old, x, y, z, world.seed());
                    Some(BlockEditOutcome {
                        sequence: act.sequence,
                        changes: vec![BlockChange {
                            x,
                            y,
                            z,
                            old_id: old,
                            new_id: mc_world::AIR_BLOCK_ID,
                        }],
                        drops,
                    })
                }
                None => Some(BlockEditOutcome {
                    sequence: act.sequence,
                    changes: vec![rollback(world, x, y, z)],
                    drops: Vec::new(),
                }),
            }
        }
        packets::PLAYER_ACTION_CANCEL_DIG => {
            Some(BlockEditOutcome { sequence: act.sequence, changes: Vec::new(), drops: Vec::new() })
        }
        _ => None,
    }
}

/// 放置权威判定（Use Item On 0x42）：目标 = 被点方块 + face 偏移。
/// 要求：被点方块是实心（非空气/水）、目标可替换（空气/水）、y/视距/距离合法；
/// 否则回滚重发目标现状。`place_id` 由服务端选择（P17 固定默认材料）。
pub fn decide_place(
    world: Option<&MutableWorld>,
    eye: (f64, f64, f64),
    center: mc_core::ChunkPos,
    radius: i32,
    act: &packets::UseItemOn,
    place_id: u16,
) -> Option<BlockEditOutcome> {
    let world = world?;
    let face = i8::try_from(act.face).ok().and_then(packets::face_offset);
    let Some((dx, dy, dz)) = face else {
        return Some(BlockEditOutcome { sequence: act.sequence, changes: Vec::new(), drops: Vec::new() });
    };
    let (tx, ty, tz) = (act.x + dx, act.y + dy, act.z + dz);
    if !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&act.y)
        || !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&ty)
        || !in_view(center, radius, tx, tz)
        || eye_block_dist(eye, act.x, act.y, act.z) > mc_world::MAX_REACH
        || !mc_world::is_placeable(place_id)
    {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![rollback(world, tx, ty, tz)],
            drops: Vec::new(),
        });
    }
    let clicked = world.get(act.x, act.y, act.z);
    if clicked == mc_world::AIR_BLOCK_ID || clicked == mc_world::WATER_BLOCK_ID {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![rollback(world, tx, ty, tz)],
            drops: Vec::new(),
        });
    }
    let cur = world.get(tx, ty, tz);
    if !mc_world::is_replaceable(cur) {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![BlockChange { x: tx, y: ty, z: tz, old_id: cur, new_id: cur }],
            drops: Vec::new(),
        });
    }
    let old = world.set(tx, ty, tz, place_id);
    Some(BlockEditOutcome {
        sequence: act.sequence,
        changes: vec![BlockChange { x: tx, y: ty, z: tz, old_id: old, new_id: place_id }],
        drops: Vec::new(),
    })
}

/// P22 种植判定（手持 `wheat_seeds` 对草方块/泥土**顶面**右键）；
/// P23 起树苗（`oak_sapling`）复用同一路径（`plant_id` 区分）：
/// 要求：face=顶面(+Y)，被点是草/泥土，目标（被点上方 1 格）是空气、
/// y/视距/距离合法；否则回滚重发目标现状（非法 face 只 ack）。
/// 成功时目标落 `plant_id`（小麦传 `WHEAT_CROP_BASE_ID`，树苗传 `OAK_SAPLING_BLOCK_ID`）。
/// 判定内已落子，调用方负责：缓存失效 + 发 BlockUpdate/Ack + 生长登记（仅小麦）+
/// 扣手持 + 投递事件（复用 `apply_block_outcome`，走 P21 广播）。
pub fn decide_plant(
    world: Option<&MutableWorld>,
    eye: (f64, f64, f64),
    center: mc_core::ChunkPos,
    radius: i32,
    act: &packets::UseItemOn,
    plant_id: u16,
) -> Option<BlockEditOutcome> {
    let world = world?;
    // 只要顶面：face=1(+Y)；其它 face（含非法）只 ack，不改世界。
    if act.face != 1 {
        return Some(BlockEditOutcome { sequence: act.sequence, changes: Vec::new(), drops: Vec::new() });
    }
    let (tx, ty, tz) = (act.x, act.y + 1, act.z);
    if !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&act.y)
        || !(mc_world::MIN_Y..=mc_world::WORLD_TOP_Y).contains(&ty)
        || !in_view(center, radius, tx, tz)
        || eye_block_dist(eye, act.x, act.y, act.z) > mc_world::MAX_REACH
    {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![rollback(world, tx, ty, tz)],
            drops: Vec::new(),
        });
    }
    // 被点必须是土壤（草方块/泥土；耕地/锄头不在 P22 范围，见 HANDOFF §5）。
    let clicked = world.get(act.x, act.y, act.z);
    if clicked != mc_world::GRASS_BLOCK_ID && clicked != mc_world::DIRT_BLOCK_ID {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![rollback(world, tx, ty, tz)],
            drops: Vec::new(),
        });
    }
    // 目标必须是空气格（水面/被占都不种）。
    let cur = world.get(tx, ty, tz);
    if cur != mc_world::AIR_BLOCK_ID {
        return Some(BlockEditOutcome {
            sequence: act.sequence,
            changes: vec![BlockChange { x: tx, y: ty, z: tz, old_id: cur, new_id: cur }],
            drops: Vec::new(),
        });
    }
    let old = world.set(tx, ty, tz, plant_id);
    Some(BlockEditOutcome {
        sequence: act.sequence,
        changes: vec![BlockChange { x: tx, y: ty, z: tz, old_id: old, new_id: plant_id }],
        drops: Vec::new(),
    })
}

/// 落子后动作：缓存失效 → 发 BlockUpdate → 发 Ack → 投递 BlockEdit 事件。
/// P21：每个 BlockUpdate 同步广播给视距内其它连接（`Ack` 仍只回操作者本人，
/// 客户端序列预测是 per-connection 的）；无 hub 时行为与旧版逐字节一致。
pub(crate) async fn apply_block_outcome(
    stream: &mut TcpStream,
    addr: SocketAddr,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
    cache: Option<&ChunkCache>,
    outcome: &BlockEditOutcome,
) -> io::Result<()> {
    for c in &outcome.changes {
        if let Some(cache) = cache {
            cache.invalidate(mc_world::chunk_of_block(c.x, c.z));
        }
        if c.new_id != c.old_id {
            info!(
                "play block edit uuid={} name={} conn_id={} pos=({},{},{}) {}->{} seq={}",
                profile.uuid, profile.username, opts.conn_id, c.x, c.y, c.z, c.old_id, c.new_id,
                outcome.sequence
            );
        }
        let mut body = Vec::new();
        packets::BlockUpdate { x: c.x, y: c.y, z: c.z, block_id: c.new_id as i32 }
            .encode_body(&mut body);
        if let Some(hub) = &opts.hub {
            hub.broadcast(
                opts.conn_id,
                mc_world::chunk_of_block(c.x, c.z),
                packets::PLAY_BLOCK_UPDATE_S2C_ID,
                &body,
            );
        }
        write_packet_any(stream, packets::PLAY_BLOCK_UPDATE_S2C_ID, &body, opts.threshold).await?;
        emit_play_event(
            &opts.events,
            PlayEvent::BlockEdit {
                addr,
                conn_id: opts.conn_id,
                x: c.x,
                y: c.y,
                z: c.z,
                old_id: c.old_id,
                new_id: c.new_id,
            },
        );
    }
    let mut ack = Vec::new();
    mc_protocol::write_varint(outcome.sequence, &mut ack);
    write_packet_any(stream, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, &ack, opts.threshold).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_world::{WorldGen, DEFAULT_SEED};
    use std::sync::Arc;

    /// P17 判定单测共用夹具：默认 seed 出生点 (369,h,381)，脚站在地表草上方。
    fn dig_fixture() -> (MutableWorld, (f64, f64, f64), mc_core::ChunkPos) {
        let world = MutableWorld::new(DEFAULT_SEED);
        let s = WorldGen::new(DEFAULT_SEED).spawn_pos();
        let feet = (s.0 as f64 + 0.5, s.1 as f64, s.2 as f64 + 0.5);
        (world, feet, mc_core::ChunkPos(s.0.div_euclid(16), s.2.div_euclid(16)))
    }

    fn grass_top() -> (i32, i32, i32) {
        let s = WorldGen::new(DEFAULT_SEED).spawn_pos();
        (s.0, s.1 - 1, s.2)
    }

    #[test]
    fn dig_start_only_acks_and_finish_breaks_grass() {
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);

        // 开始挖掘：目标合法，只 ack，不破坏、不掉落（让客户端播放破坏进度动画）。
        let start = packets::PlayerAction {
            status: packets::PLAYER_ACTION_START_DIG, x, y, z, face: 1, sequence: 5,
        };
        let out = decide_dig(Some(&world), eye, center, 2, &start).unwrap();
        assert_eq!(out.sequence, 5);
        assert!(out.changes.is_empty(), "START_DIG must not break the block");
        assert!(out.drops.is_empty(), "START_DIG must not drop items");
        assert_eq!(world.get(x, y, z), mc_world::GRASS_BLOCK_ID);
        assert_eq!(world.dirty_len(), 0);

        // 完成挖掘：真正破坏并带出 dirt 掉落（item 55×1）。
        let finish = packets::PlayerAction {
            status: packets::PLAYER_ACTION_FINISH_DIG, x, y, z, face: 1, sequence: 6,
        };
        let out = decide_dig(Some(&world), eye, center, 2, &finish).unwrap();
        assert_eq!(out.sequence, 6);
        assert_eq!(
            out.changes,
            vec![BlockChange { x, y, z, old_id: mc_world::GRASS_BLOCK_ID, new_id: 0 }]
        );
        assert_eq!(world.get(x, y, z), 0, "decide must apply the edit");
        assert_eq!(world.dirty_len(), 1);
        assert_eq!(
            out.drops,
            vec![mc_world::Drop { item_id: mc_world::drops::ITEM_DIRT, count: 1 }]
        );
    }

    #[test]
    fn concurrent_finish_dig_on_one_block_drops_once() {
        // R1：两连接同时挖同一格，set_if 的 CAS 只允许一方成功，不能刷出双份掉落。
        let world = Arc::new(MutableWorld::new(DEFAULT_SEED));
        let s = WorldGen::new(DEFAULT_SEED).spawn_pos();
        let (x, y, z) = (s.0, s.1 - 1, s.2);
        let feet = (s.0 as f64 + 0.5, s.1 as f64, s.2 as f64 + 0.5);
        let center = mc_world::chunk_of_block(x, z);
        let act = packets::PlayerAction {
            status: packets::PLAYER_ACTION_FINISH_DIG, x, y, z, face: 1, sequence: 7,
        };
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let world = Arc::clone(&world);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    decide_dig(Some(&world), eye_of(feet), center, 2, &act).unwrap()
                })
            })
            .collect();
        let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            outcomes.iter().filter(|o| !o.drops.is_empty()).count(),
            1,
            "two concurrent FINISH_DIG must not produce two drops: {outcomes:?}"
        );
        assert_eq!(world.get(x, y, z), mc_world::AIR_BLOCK_ID, "block must be dug exactly once");
    }

    #[test]
    fn dig_drops_follow_block_and_rollback_has_none() {
        let (world, feet, center) = dig_fixture();
        let eye = eye_of(feet);
        // 回滚（挖空气）无掉落。
        let (x, y, z) = grass_top();
        world.set(x, y, z, 0);
        let air_out = decide_dig(
            Some(&world),
            eye,
            center,
            2,
            &packets::PlayerAction {
                status: packets::PLAYER_ACTION_FINISH_DIG, x, y, z, face: 1, sequence: 1,
            },
        )
        .unwrap();
        assert!(air_out.drops.is_empty(), "rollback must not drop");
        // cancel(ABORT=2) 只 ack，无掉落。
        let cancel = decide_dig(
            Some(&world),
            eye,
            center,
            2,
            &packets::PlayerAction {
                status: packets::PLAYER_ACTION_CANCEL_DIG, x, y, z, face: 0, sequence: 8,
            },
        )
        .unwrap();
        assert!(cancel.drops.is_empty());
    }

    #[test]
    fn abort_and_direction_change_never_break() {
        // P18.2 回归：ABORT(2) 是松开鼠标，不是完成——绝不能落子；
        // CHANGE(1) 直接忽略（客户端不走预测，不用 ack）。
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);
        let abort = packets::PlayerAction {
            status: packets::PLAYER_ACTION_CANCEL_DIG, x, y, z, face: 1, sequence: 9,
        };
        let out = decide_dig(Some(&world), eye, center, 2, &abort).unwrap();
        assert!(out.changes.is_empty(), "ABORT must not break the block");
        assert!(out.drops.is_empty());
        assert_eq!(world.get(x, y, z), mc_world::GRASS_BLOCK_ID);
        let change = packets::PlayerAction {
            status: packets::PLAYER_ACTION_CHANGE_DIRECTION, x, y, z, face: 1, sequence: 10,
        };
        assert!(
            decide_dig(Some(&world), eye, center, 2, &change).is_none(),
            "CHANGE direction must be ignored without ack"
        );
        assert_eq!(world.get(x, y, z), mc_world::GRASS_BLOCK_ID);
    }

    #[test]
    fn dig_rejects_air_water_far_out_of_view_and_bad_y() {
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);
        // 挖空气：回滚重发空气，不落子。
        world.set(x, y, z, 0);
        let air_out = decide_dig(
            Some(&world),
            eye,
            center,
            2,
            &packets::PlayerAction {
                    status: packets::PLAYER_ACTION_FINISH_DIG, x, y, z, face: 1, sequence: 1,
                },
        )
        .unwrap();
        assert_eq!(air_out.changes.len(), 1);
        assert_eq!(air_out.changes[0].new_id, 0);
        assert_eq!(world.override_count(), 1, "rollback must not add entries");
        // 超距（出生点 100 格外）：回滚，世界不变。
        let far = decide_dig(
            Some(&world),
            eye,
            center,
            2,
            &packets::PlayerAction {
                status: packets::PLAYER_ACTION_FINISH_DIG, x: x + 100, y, z, face: 1, sequence: 2,
            },
        )
        .unwrap();
        assert_eq!(far.changes[0].old_id, far.changes[0].new_id);
        assert_eq!(world.override_count(), 1);
        // 出视距（视距 2，目标 chunk 差 10）：回滚。
        let oov = decide_dig(
            Some(&world),
            eye,
            center,
            2,
            &packets::PlayerAction { status: 0, x: x + 160, y, z, face: 1, sequence: 3 },
        )
        .unwrap();
        assert_eq!(oov.changes[0].old_id, oov.changes[0].new_id);
        // y 越界：回滚为空气。
        let bad_y = decide_dig(
            Some(&world),
            eye,
            center,
            2,
            &packets::PlayerAction { status: 0, x, y: 400, z, face: 1, sequence: 4 },
        )
        .unwrap();
        assert_eq!(bad_y.changes[0].new_id, 0);
    }

    #[test]
    fn dig_cancel_acks_drop_statuses_and_missing_world_ignored() {
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);
        let cancel = decide_dig(
            Some(&world),
            eye,
            center,
            2,
            &packets::PlayerAction {
                status: packets::PLAYER_ACTION_CANCEL_DIG, x, y, z, face: 0, sequence: 8,
            },
        )
        .unwrap();
        assert!(cancel.changes.is_empty(), "cancel must only ack");
        assert_eq!(cancel.sequence, 8);
        // 丢弃/吃食/换手：无世界影响，直接忽略（不 ack，避免误确认旧序列）。
        for status in [4, 5, 6, 7, 8, 99] {
            assert!(
                decide_dig(
                    Some(&world),
                    eye,
                    center,
                    2,
                    &packets::PlayerAction { status, x, y, z, face: 0, sequence: 0 },
                )
                .is_none(),
                "status={status} must be ignored"
            );
        }
        // 无世界（旧入口）：不断线，只忽略。
        assert!(
            decide_dig(
                None,
                eye,
                center,
                2,
                &packets::PlayerAction {
                    status: packets::PLAYER_ACTION_FINISH_DIG, x, y, z, face: 1, sequence: 1,
                },
            )
            .is_none()
        );
    }

    #[test]
    fn place_on_dirt_top_succeeds_and_occupied_rolls_back() {
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);
        // 先挖掉草，露出泥土。
        world.set(x, y, z, 0);
        // 点泥土顶面放石头。
        let act = packets::UseItemOn {
            hand: 0,
            x,
            y: y - 1,
            z,
            face: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 12,
        };
        let out = decide_place(Some(&world), eye, center, 2, &act, mc_world::STONE_BLOCK_ID).unwrap();
        assert_eq!(out.sequence, 12);
        assert_eq!(
            out.changes,
            vec![BlockChange { x, y, z, old_id: 0, new_id: mc_world::STONE_BLOCK_ID }]
        );
        assert_eq!(world.get(x, y, z), mc_world::STONE_BLOCK_ID);
        // 同一位置再放：目标已被占据 → 回滚重发石头，不新增条目。
        let before = world.override_count();
        let again =
            decide_place(Some(&world), eye, center, 2, &act, mc_world::STONE_BLOCK_ID).unwrap();
        assert_eq!(again.changes[0].old_id, mc_world::STONE_BLOCK_ID);
        assert_eq!(again.changes[0].new_id, mc_world::STONE_BLOCK_ID);
        assert_eq!(world.override_count(), before);
    }

    #[test]
    fn place_rejects_air_click_bad_face_and_out_of_reach() {
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);
        let base = |face| packets::UseItemOn {
            hand: 0,
            x,
            y,
            z,
            face,
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 20,
        };
        // 点空气放：回滚（目标现状重发）。先把被点方块挖空。
        world.set(x, y, z, 0);
        let against_air = decide_place(
            Some(&world),
            eye,
            center,
            2,
            &packets::UseItemOn { y: y + 5, sequence: 21, ..base(1) },
            mc_world::STONE_BLOCK_ID,
        )
        .unwrap();
        assert_eq!(against_air.changes[0].old_id, against_air.changes[0].new_id);
        let _ = against_air;
        // 非法 face：只 ack。
        let bad_face =
            decide_place(Some(&world), eye, center, 2, &base(9), mc_world::STONE_BLOCK_ID).unwrap();
        assert!(bad_face.changes.is_empty());
        // 超距眼睛 100 格：回滚。
        let far_eye = (eye.0 + 100.0, eye.1, eye.2);
        let far = decide_place(Some(&world), far_eye, center, 2, &base(1), mc_world::STONE_BLOCK_ID)
            .unwrap();
        assert_eq!(far.changes[0].old_id, far.changes[0].new_id);
        // 无世界：忽略。
        assert!(decide_place(None, eye, center, 2, &base(1), mc_world::STONE_BLOCK_ID).is_none());
    }

    #[test]
    fn plant_seeds_on_grass_top_grows_stage_zero() {
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);
        // 点草方块顶面：上方空气落 0 阶段小麦。
        let act = packets::UseItemOn {
            hand: 0,
            x,
            y,
            z,
            face: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 31,
        };
        let out = decide_plant(Some(&world), eye, center, 2, &act, mc_world::WHEAT_CROP_BASE_ID).unwrap();
        assert_eq!(out.sequence, 31);
        assert_eq!(
            out.changes,
            vec![BlockChange { x, y: y + 1, z, old_id: 0, new_id: mc_world::WHEAT_CROP_BASE_ID }]
        );
        assert!(out.drops.is_empty());
        assert_eq!(world.get(x, y + 1, z), mc_world::WHEAT_CROP_BASE_ID);
        // 小麦是非地形弱方块：terrain 高度不变，motion 顶随之 +1（与水/树叶同口径）。
        let col = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
        let gen = world.generate_chunk(mc_world::chunk_of_block(x, z));
        assert_eq!(gen.heights[col], y, "wheat must not move the terrain height");
        assert_eq!(gen.motion_heights[col], y + 2 - mc_world::MIN_Y, "motion follows the wheat top");
    }

    #[test]
    fn plant_sapling_on_grass_top_lands_oak_sapling() {
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);
        // 树苗复用种植路径：点草方块顶面，上方空气落树苗方块（30）。
        let act = packets::UseItemOn {
            hand: 0,
            x,
            y,
            z,
            face: 1,
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 32,
        };
        let out =
            decide_plant(Some(&world), eye, center, 2, &act, mc_world::OAK_SAPLING_BLOCK_ID).unwrap();
        assert_eq!(out.sequence, 32);
        assert_eq!(
            out.changes,
            vec![BlockChange { x, y: y + 1, z, old_id: 0, new_id: mc_world::OAK_SAPLING_BLOCK_ID }]
        );
        assert_eq!(world.get(x, y + 1, z), mc_world::OAK_SAPLING_BLOCK_ID);
        // 挖掉种下的树苗：原样返还树苗物品（可再种，不断链）。
        let drops =
            mc_world::drops_for_block(mc_world::OAK_SAPLING_BLOCK_ID, x, y + 1, z, world.seed());
        assert_eq!(
            drops,
            vec![mc_world::Drop { item_id: mc_world::drops::ITEM_OAK_SAPLING, count: 1 }]
        );
    }

    #[test]
    fn plant_rejects_non_top_face_non_soil_occupied_and_bad_range() {
        let (world, feet, center) = dig_fixture();
        let (x, y, z) = grass_top();
        let eye = eye_of(feet);
        let base = |face| packets::UseItemOn {
            hand: 0,
            x,
            y,
            z,
            face,
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_z: 0.5,
            inside_block: false,
            world_border_hit: false,
            sequence: 40,
        };
        // 侧面/底面：只 ack，不落子。
        for face in [0, 2, 3, 4, 5, 9] {
            let out = decide_plant(Some(&world), eye, center, 2, &base(face), mc_world::WHEAT_CROP_BASE_ID).unwrap();
            assert!(out.changes.is_empty(), "face={face} must only ack");
            assert_eq!(world.get(x, y + 1, z), mc_world::AIR_BLOCK_ID);
        }
        // 被点是石头：回滚目标现状。
        world.set(x, y, z, mc_world::STONE_BLOCK_ID);
        let out = decide_plant(Some(&world), eye, center, 2, &base(1), mc_world::WHEAT_CROP_BASE_ID).unwrap();
        assert_eq!(out.changes[0].old_id, out.changes[0].new_id);
        assert_eq!(world.get(x, y + 1, z), mc_world::AIR_BLOCK_ID);
        world.set(x, y, z, mc_world::GRASS_BLOCK_ID);
        // 目标被占：回滚重发。
        world.set(x, y + 1, z, mc_world::STONE_BLOCK_ID);
        let before = world.override_count();
        let out = decide_plant(Some(&world), eye, center, 2, &base(1), mc_world::WHEAT_CROP_BASE_ID).unwrap();
        assert_eq!(out.changes[0].new_id, mc_world::STONE_BLOCK_ID);
        assert_eq!(world.override_count(), before);
        world.set(x, y + 1, z, mc_world::AIR_BLOCK_ID);
        // 泥土同样可种。
        world.set(x, y, z, mc_world::DIRT_BLOCK_ID);
        let out = decide_plant(Some(&world), eye, center, 2, &base(1), mc_world::WHEAT_CROP_BASE_ID).unwrap();
        assert_eq!(out.changes[0].new_id, mc_world::WHEAT_CROP_BASE_ID);
        // 超距/出视距/无世界。
        let far_eye = (eye.0 + 100.0, eye.1, eye.2);
        let out = decide_plant(Some(&world), far_eye, center, 2, &base(1), mc_world::WHEAT_CROP_BASE_ID).unwrap();
        assert_eq!(out.changes[0].old_id, out.changes[0].new_id);
        assert!(
            decide_plant(None, eye, center, 2, &base(1), mc_world::WHEAT_CROP_BASE_ID).is_none(),
            "missing world must be ignored"
        );
    }
}
