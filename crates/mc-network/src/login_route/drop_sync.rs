//! drop_sync: drop spawn / Spawn / Data / Remove + inventory sync + pickup tick (moved from login_route.rs P18/P19 section, no logic change).
use super::block_edit::BlockEditOutcome;
use super::login::{write_packet_any, PlayOptions};
use crate::chunk_view::ChunkView;
use crate::drop_state::{DropState, PLAYER_ENTITY_ID};
use mc_protocol::packets;
use mc_world::MutableWorld;
use tokio::net::TcpStream;
use tracing::debug;
use std::io;

// ---------------------------------------------------------------------------
// P18 掉落物 + 背包（服务端权威，net 线程即时同步；ECS 无写通道，见模块文档）。
//
// - 生成：decide_dig 的 drops → alloc id → drop_rest_y 落点 → ItemEntities::spawn
//   → Spawn(0x01，带初速)+SetEntityData(0x65) 发给本连接；客户端自行模拟弹跳/静止。
// - 下落：全服唯一 ticker（`DropState::start_global_tick`，50ms）跑 ItemEntities::tick()
//   与支撑重算/age/despawn，与在线连接数无关；连接侧每 50ms 只做拾取 + 视距补/卸 +
//   despawn 清理 known。服务端只维护权威位置用于拾取，不发 Teleport 与客户端物理打架。
//   despawn/出视距发 Remove(0x4E)，进视距补发 Spawn。
// - 拾取：delay 到期 + 与脚位置 <1.5 → TakeItem(0x7F)+Remove，入库，全量 Content 同步。
// - P31 经验球：独立实体池 + 同一全局 ticker 物理；挖矿掉落生成，拾取后 TakeItem
//   (XP 球 count=1)+Remove + SetExperience(0x69) + XP 存进背包同文件（v2 存档）。
// - 背包：Join 全量 Content；放置成功 consume_held + 单槽 Slot 同步；C2S 0x36 切手持。
// - Q 扔物（P19）：C2S PlayerAction(0x29) status 4/5（jar `Action` DROP_ALL/DROP_ITEM）
//   从手持槽拿走整栈/1 个，脚下生成掉落实体，全量 Content 同步 + 刷盘，恒回 Ack；
//   空手只回 Ack（无变化不断连）。背包打开时按 Q 走 C2S ContainerClick(0x12) THROW
//  （`ContainerInput` id=4：button 0=1 个，1=整栈；光标非空时原版无操作）。
// - ClickContainer(0x12) PICKUP/SWAP/QUICK_MOVE/THROW 服务端权威应用；
//   CreativeSlot(0x39) 仍忽略（Slot 的 data component 需注册表驱动解码，
//   误解析会错位整条流）。
//
// TODO(真机验证，见 HANDOFF §5.11)：本轮四处改动需原版 26.3 客户端真机确认：
//   1) Spawn 带初速 → 掉落物弹跳/落地是否与原版一致；
//   2) 删除每 tick Teleport → 是否不再原地蹦跶、能否正常拾取；
//   3) 删除 spawn_drops 视距门禁 → 走远挖地表方块是否稳定掉落；
//   4) START_DIG 只 ack → 破坏进度动画是否正常完成并触发 FINISH_DIG。
// ---------------------------------------------------------------------------

/// 实体的视距判定（切比雪夫，与 ChunkView 同口径）。
fn drop_in_view(center: mc_core::ChunkPos, radius: i32, chunk: mc_core::ChunkPos) -> bool {
    (chunk.0 - center.0).abs() <= radius && (chunk.1 - center.1).abs() <= radius
}

pub(crate) async fn send_spawn(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    id: i32,
) -> io::Result<()> {
    let snap = {
        let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        items.get(id).map(|e| (e.id, e.uuid, e.pos, e.vel, e.item_id, e.count))
    };
    let Some((id, uuid, (x, y, z), vel, item_id, count)) = snap else { return Ok(()) };
    let mut body = Vec::new();
    packets::SpawnItemEntity {
        entity_id: id,
        uuid,
        x,
        y,
        z,
        // 把服务端初速下发，让客户端用自己的掉落物物理模拟抛物线/落地弹跳/静止。
        // 之前固定发零速度 + 每 50ms Teleport 覆盖，会和客户端物理打架导致原地狂蹦。
        vel: packets::LpVec3 { x: vel.0, y: vel.1, z: vel.2 },
        pitch: 0,
        yaw: 0,
        head_yaw: 0,
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SPAWN_ENTITY_S2C_ID, &body, opts.threshold).await?;
    let mut body = Vec::new();
    packets::ItemEntityData {
        entity_id: id,
        slot: packets::Slot { count, item_id },
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_ENTITY_DATA_S2C_ID, &body, opts.threshold).await?;
    drops.mark_known(opts.conn_id, id);
    Ok(())
}

pub(crate) async fn send_data(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    id: i32,
) -> io::Result<()> {
    let snap = {
        let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        items.get(id).map(|e| (e.id, e.item_id, e.count))
    };
    let Some((id, item_id, count)) = snap else { return Ok(()) };
    let mut body = Vec::new();
    packets::ItemEntityData {
        entity_id: id,
        slot: packets::Slot { count, item_id },
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_ENTITY_DATA_S2C_ID, &body, opts.threshold).await
}

pub(crate) async fn send_remove(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    ids: &[i32],
) -> io::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut body = Vec::new();
    packets::RemoveEntities { ids: ids.to_vec() }.encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_REMOVE_ENTITIES_S2C_ID, &body, opts.threshold).await?;
    for id in ids {
        drops.unmark_known(opts.conn_id, *id);
    }
    Ok(())
}

// ---- P31 经验球（复用 add_entity/SetData/Remove/TakeItem 已验证包体，ID 全部查生成表） ----

/// 经验球生成：Spawn(0x01，type=50) + SetData(0x65，index 8/VarInt(1)=经验值)。
pub(crate) async fn send_orb_spawn(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    id: i32,
) -> io::Result<()> {
    let snap = {
        let orbs = drops.orbs.lock().unwrap_or_else(|p| p.into_inner());
        orbs.get(id).map(|o| (o.id, o.uuid, o.pos, o.vel, o.amount))
    };
    let Some((id, uuid, (x, y, z), vel, amount)) = snap else { return Ok(()) };
    let mut body = Vec::new();
    packets::SpawnExperienceOrb {
        entity_id: id,
        uuid,
        x,
        y,
        z,
        vel: packets::LpVec3 { x: vel.0, y: vel.1, z: vel.2 },
        pitch: 0,
        yaw: 0,
        head_yaw: 0,
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SPAWN_ENTITY_S2C_ID, &body, opts.threshold).await?;
    let mut body = Vec::new();
    packets::XpOrbEntityData { entity_id: id, amount }.encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_ENTITY_DATA_S2C_ID, &body, opts.threshold).await?;
    drops.mark_known(opts.conn_id, id);
    Ok(())
}

/// P31 经验/等级同步（Join 与拾取后）：Float 进度 + VarInt 等级 + VarInt 总经验。
pub(crate) async fn send_experience(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
) -> io::Result<()> {
    let Some((total, level, progress)) = drops.with_inventory(opts.conn_id, |inv| {
        (inv.total_xp(), inv.xp_level(), inv.xp_progress())
    }) else {
        return Ok(());
    };
    let mut body = Vec::new();
    packets::SetExperience { progress, level, total }.encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_EXPERIENCE_S2C_ID, &body, opts.threshold).await
}

/// P21/P31：向视距内其它连接同步单个经验球（未知→Spawn+Data，已知→Data）。
pub(crate) fn broadcast_orb_to_viewers(
    opts: &PlayOptions,
    drops: &DropState,
    chunk: mc_core::ChunkPos,
    id: i32,
) {
    let Some(hub) = &opts.hub else { return };
    let viewers = hub.recipients_in_view(opts.conn_id, chunk);
    if viewers.is_empty() {
        return;
    }
    let snap = {
        let orbs = drops.orbs.lock().unwrap_or_else(|p| p.into_inner());
        orbs.get(id).map(|o| (o.id, o.uuid, o.pos, o.vel, o.amount))
    };
    let Some((id, uuid, (x, y, z), vel, amount)) = snap else { return };
    let mut dbody = Vec::new();
    packets::XpOrbEntityData { entity_id: id, amount }.encode_body(&mut dbody);
    for viewer in viewers {
        if drops.is_known(viewer, id) {
            hub.unicast(viewer, packets::PLAY_SET_ENTITY_DATA_S2C_ID, dbody.clone());
            continue;
        }
        let mut sbody = Vec::new();
        packets::SpawnExperienceOrb {
            entity_id: id,
            uuid,
            x,
            y,
            z,
            vel: packets::LpVec3 { x: vel.0, y: vel.1, z: vel.2 },
            pitch: 0,
            yaw: 0,
            head_yaw: 0,
        }
        .encode_body(&mut sbody);
        hub.unicast(viewer, packets::PLAY_SPAWN_ENTITY_S2C_ID, sbody);
        hub.unicast(viewer, packets::PLAY_SET_ENTITY_DATA_S2C_ID, dbody.clone());
        drops.mark_known(viewer, id);
    }
}

/// P21：向事件区块视距内的其它连接同步单个掉落实体（Spawn 或 Data）。
///
/// - 接收方未知 → `Spawn` + `Data` 并 `mark_known`（与 `send_spawn` 同序）；
/// - 接收方已知（合并堆叠）→ 只补 `Data`。
/// `Ack`/背包同步不受影响（仍只发操作者本人）。
pub(crate) fn broadcast_entity_to_viewers(
    opts: &PlayOptions,
    drops: &DropState,
    chunk: mc_core::ChunkPos,
    id: i32,
) {
    let Some(hub) = &opts.hub else { return };
    let viewers = hub.recipients_in_view(opts.conn_id, chunk);
    if viewers.is_empty() {
        return;
    }
    let snap = {
        let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        items.get(id).map(|e| (e.id, e.uuid, e.pos, e.vel, e.item_id, e.count))
    };
    let Some((id, uuid, (x, y, z), vel, item_id, count)) = snap else { return };
    let mut dbody = Vec::new();
    packets::ItemEntityData {
        entity_id: id,
        slot: packets::Slot { count, item_id },
    }
    .encode_body(&mut dbody);
    for viewer in viewers {
        if drops.is_known(viewer, id) {
            hub.unicast(viewer, packets::PLAY_SET_ENTITY_DATA_S2C_ID, dbody.clone());
            continue;
        }
        let mut sbody = Vec::new();
        packets::SpawnItemEntity {
            entity_id: id,
            uuid,
            x,
            y,
            z,
            vel: packets::LpVec3 { x: vel.0, y: vel.1, z: vel.2 },
            pitch: 0,
            yaw: 0,
            head_yaw: 0,
        }
        .encode_body(&mut sbody);
        hub.unicast(viewer, packets::PLAY_SPAWN_ENTITY_S2C_ID, sbody);
        hub.unicast(viewer, packets::PLAY_SET_ENTITY_DATA_S2C_ID, dbody.clone());
        drops.mark_known(viewer, id);
    }
}

/// P21：向视距内其它连接广播 `RemoveEntities`（拾取即时同步；despawn/出视距仍走
/// 各连接自己的 `tick_drops` 差集兜底）。逐个接收方 `unmark_known`，避免接收方
/// tick 重复补发。
pub(crate) fn broadcast_remove_to_viewers(
    opts: &PlayOptions,
    drops: &DropState,
    chunk: mc_core::ChunkPos,
    ids: &[i32],
) {
    let Some(hub) = &opts.hub else { return };
    if ids.is_empty() {
        return;
    }
    let viewers = hub.recipients_in_view(opts.conn_id, chunk);
    if viewers.is_empty() {
        return;
    }
    let mut body = Vec::new();
    packets::RemoveEntities { ids: ids.to_vec() }.encode_body(&mut body);
    for viewer in viewers {
        hub.unicast(viewer, packets::PLAY_REMOVE_ENTITIES_S2C_ID, body.clone());
        for id in ids {
            drops.unmark_known(viewer, *id);
        }
    }
}

/// P21：向视距内其它连接广播拾取动画（`collector` 与操作者看到的一致，恒为 1）。
pub(crate) fn broadcast_takeitem_to_viewers(
    opts: &PlayOptions,
    chunk: mc_core::ChunkPos,
    entity_id: i32,
    count: i32,
) {
    let Some(hub) = &opts.hub else { return };
    let mut body = Vec::new();
    packets::TakeItemEntity {
        collected_id: entity_id,
        collector_id: PLAYER_ENTITY_ID,
        count,
    }
    .encode_body(&mut body);
    hub.broadcast(opts.conn_id, chunk, packets::PLAY_PICKUP_ITEM_S2C_ID, &body);
}

/// 全量背包同步（Join/拾取后）。
pub(crate) async fn send_full_inventory(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
) -> io::Result<()> {
    let body = build_full_inventory_body(drops, opts.conn_id).unwrap_or_else(|| {
        let mut body = Vec::new();
        packets::SetContainerContent {
            window_id: packets::PLAYER_INVENTORY_WINDOW_ID,
            state_id: 1,
            slots: vec![packets::Slot::empty(); packets::PLAYER_INVENTORY_SLOT_COUNT],
            carried: packets::Slot::empty(),
        }
        .encode_body(&mut body);
        body
    });
    write_packet_any(stream, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, &body, opts.threshold)
        .await
}

/// P25 管理面板：组全量背包同步包体（纯函数，不碰 socket、不落盘）。
/// 返回 `None` = 该 conn 已无背包（已下线/从未 Join）；调用方（管理线程）据此
/// 判定 delivered。背包锁临界区内一次拷贝 + 组包，`next_state` 单调递增。
pub fn build_full_inventory_body(drops: &DropState, conn_id: u64) -> Option<Vec<u8>> {
    let (slots, carried, state) = drops.with_inventory(conn_id, |inv| {
        (inv.to_protocol_slots(), inv.cursor_slot(), inv.next_state())
    })?;
    let mut body = Vec::new();
    packets::SetContainerContent {
        window_id: packets::PLAYER_INVENTORY_WINDOW_ID,
        state_id: state,
        slots,
        carried,
    }
    .encode_body(&mut body);
    Some(body)
}

/// 单槽同步（放置消耗后）。
pub(crate) async fn send_held_slot(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
) -> io::Result<()> {
    let Some((idx, slot, state)) = drops.with_inventory(opts.conn_id, |inv| {
        let idx = inv.selected_slot_index();
        (idx, inv.protocol_slot(idx), inv.next_state())
    }) else {
        return Ok(());
    };
    let mut body = Vec::new();
    packets::SetContainerSlot {
        window_id: packets::PLAYER_INVENTORY_WINDOW_ID,
        state_id: state,
        slot: idx as i16,
        data: slot,
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_CONTAINER_SLOT_S2C_ID, &body, opts.threshold).await
}

/// 挖掘掉落生成：每种 drop 一个物品实体（同位合并由 ItemEntities 处理），
/// 同一次挖掘按方块类型额外生成经验球（P31，共用挖掘路径与 fan-out）。
pub(crate) async fn spawn_drops(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    world: &MutableWorld,
    outcome: &BlockEditOutcome,
) -> io::Result<()> {
    if outcome.changes.is_empty() {
        return Ok(());
    }
    let (bx, by, bz, old_id) = {
        let c = &outcome.changes[0];
        (c.x, c.y, c.z, c.old_id)
    };
    // XP 由被破坏方块的 old_id 查确定性表（与物品掉落同源，CAS 失败/回滚时 old=air→空）。
    let xp_orbs = mc_world::drops::xp_orbs_for_block(old_id, bx, by, bz, world.seed());
    if outcome.drops.is_empty() && xp_orbs.is_empty() {
        return Ok(());
    }
    let chunk = mc_world::chunk_of_block(bx, bz);
    for d in &outcome.drops {
        let id = drops.alloc_id();
        // 以被破坏方块为起点向下找支撑面：树冠/水面/洞穴顶不再把掉落物抬到高处，
        // 挖掘地下方块时掉落物留在挖掘点附近（用户反馈“超出范围啥也没有”的根因）。
        let rest = world.drop_rest_y(bx, bz, by) as f64;
        let touched = {
            let mut items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
            match items.spawn(
                id,
                d.item_id,
                d.count,
                bx as f64 + 0.5,
                by as f64 + 0.5,
                bz as f64 + 0.5,
                rest,
            ) {
                Ok(t) => t,
                Err(e) => {
                    debug!("drop spawn failed id={id} ({e})");
                    continue;
                }
            }
        };
        for tid in touched {
            // 被破坏方块一定在挖掘半径内、且必然处于视距内，无需额外视距门禁；
            // 合并命中的其它实体同步当前堆叠数量即可。
            if tid == id {
                send_spawn(stream, opts, drops, tid).await?;
            } else if drops.is_known(opts.conn_id, tid) {
                send_data(stream, opts, drops, tid).await?;
            } else {
                send_spawn(stream, opts, drops, tid).await?;
            }
            // P21：同视距其它连接实时看到掉落（Spawn/Data + known 维护）。
            broadcast_entity_to_viewers(opts, drops, chunk, tid);
        }
    }
    // P31：每个经验值一个球（同物合并未做，见模块取舍），与物品掉落同点/同支撑/同广播。
    for amount in xp_orbs {
        let id = drops.alloc_id();
        let rest = world.drop_rest_y(bx, bz, by) as f64;
        let spawned = {
            let mut orbs = drops.orbs.lock().unwrap_or_else(|p| p.into_inner());
            orbs.spawn(
                id,
                amount,
                bx as f64 + 0.5,
                by as f64 + 0.5,
                bz as f64 + 0.5,
                rest,
            )
            .is_ok()
        };
        if !spawned {
            debug!("xp orb spawn failed id={id}");
            continue;
        }
        send_orb_spawn(stream, opts, drops, id).await?;
        broadcast_orb_to_viewers(opts, drops, chunk, id);
    }
    Ok(())
}

/// 只回 Ack（无世界影响的操作：空手 Q/空手放/UseItem；对齐客户端序列预测）。
pub(crate) async fn send_ack_only(stream: &mut TcpStream, threshold: i32, sequence: i32) -> io::Result<()> {
    let mut ack = Vec::new();
    mc_protocol::write_varint(sequence, &mut ack);
    write_packet_any(stream, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, &ack, threshold).await
}

/// P28 朝向扔物：yaw/pitch（度，原版约定：yaw=0 朝 +Z、yaw=-90 朝 +X、
/// pitch=-90 抬头、+90 低头）→ 单位方向向量。原版 `Vec3.directionFromRotation`
/// 同构（`x=-sin(yaw)*cos(pitch), y=-sin(pitch), z=cos(yaw)*cos(pitch)`），
/// 纯函数，单测覆盖边界值。
pub(crate) fn look_dir(yaw_deg: f32, pitch_deg: f32) -> (f64, f64, f64) {
    let yaw = (yaw_deg as f64).to_radians();
    let pitch = (pitch_deg as f64).to_radians();
    let cp = pitch.cos();
    (-yaw.sin() * cp, -pitch.sin(), yaw.cos() * cp)
}

/// P28 扔物调参（P28.2 修过弹道：生成点收回头上、前置只剩 0.15 格，
/// 上扬基线降到 0.1——之前 0.5+0.25 是从脸前半空"弹"出来再落下，手感不对；
/// 原版 `ServerPlayer.drop` 就是从眼位（eye-0.3，XZ 不前置）扔出）：
/// - 生成点 = 眼高（feet + 1.62）+ 朝向 × 0.15 格；
/// - 初速 = 朝向 × `THROW_SPEED` + 纵向基线 0.1（抬头/低头叠加 `dir.y * SPEED`）。
/// 水平取 0.06（P18.2 挖掘散布 ±0.1 的同数量级下限——轻掷）：约落在面前 1 格，
/// 站立原地 2s（手扔延迟，P28.1）后可自捡（旧 e2e `broadcast_thrown_drop…` 要求；
/// 原版 Q 扔约 0.3 会飞出 1.5 格自捡半径，差异见 HANDOFF P28，待自捡排除后恢复）。
pub(crate) const THROW_FORWARD_DIST: f64 = 0.15;
pub(crate) const THROW_SPEED: f64 = 0.06;
pub(crate) const THROW_UP_BIAS: f64 = 0.1;

/// Q 扔物生成（P19 + P28 朝向 + P28.2 弹道）：复用挖掘掉落的 Spawn/Data 同步路径
///（同物同 chunk 半径 1 格合并；`touched` 的其它实体只补发 Data，未知连接补 Spawn）。
/// P28.2：生成点 = 眼高 + 朝向×0.15 格（基本在头上），初速 = 朝向×`THROW_SPEED` + 纵向基线 0.1；
/// 拾取/合并/广播链路不变，`look` 为扔出瞬间该连接的 (yaw, pitch)。
/// 原版依据：`ServerboundPlayerActionPacket$Action` DROP_ITEM=5（Q，扔 1 个）/
/// DROP_ALL_ITEMS=4（Ctrl+Q，扔整栈），`ServerPlayer.drop` 从手持槽拿走；
/// 背包打开时按 Q 走 ContainerClick THROW（`ContainerInput` id=4，button 0/1），
/// 要求光标为空（`doClick` THROW 分支先查 carried，手持栈时无操作）。
pub(crate) async fn spawn_thrown_items(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    world: &MutableWorld,
    feet: (f64, f64, f64),
    look: (f32, f32),
    item_id: i32,
    count: i32,
) -> io::Result<()> {
    if count <= 0 {
        return Ok(());
    }
    let id = drops.alloc_id();
    // P28：眼高 + 朝向×0.5 格（原版从眼位前方扔出，不再掉在脚下）。
    let dir = look_dir(look.0, look.1);
    let eye = (feet.0, feet.1 + mc_world::EYE_HEIGHT, feet.2);
    let sx = eye.0 + dir.0 * THROW_FORWARD_DIST;
    let sy = eye.1 + dir.1 * THROW_FORWARD_DIST;
    let sz = eye.2 + dir.2 * THROW_FORWARD_DIST;
    // 初速 = 朝向×THROW_SPEED(0.06) + 纵向基线 0.25（P18.2 `spawn` 默认 vy 同值，平扔抛物线一致）。
    let vel =
        (dir.0 * THROW_SPEED, THROW_UP_BIAS + dir.1 * THROW_SPEED, dir.2 * THROW_SPEED);
    let (bx, by, bz) = (sx.floor() as i32, sy.floor() as i32, sz.floor() as i32);
    let chunk = mc_world::chunk_of_block(bx, bz);
    let rest = world.drop_rest_y(bx, bz, by) as f64;
    let touched = {
        let mut items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        match items.spawn_with_velocity(
            id,
            item_id,
            count,
            sx,
            sy,
            sz,
            rest,
            vel,
            mc_world::items::THROWN_PICKUP_DELAY_TICKS,
        ) {
            Ok(t) => t,
            Err(e) => {
                debug!("throw spawn failed id={id} ({e})");
                return Ok(());
            }
        }
    };
    for tid in touched {
        if tid == id {
            send_spawn(stream, opts, drops, tid).await?;
        } else if drops.is_known(opts.conn_id, tid) {
            send_data(stream, opts, drops, tid).await?;
        } else {
            send_spawn(stream, opts, drops, tid).await?;
        }
        // P21：同视距其它连接实时看到扔出的东西。
        broadcast_entity_to_viewers(opts, drops, chunk, tid);
    }
    Ok(())
}

/// Q 扔手持（P19）：DROP_ITEM=5 扔 1 个，DROP_ALL_ITEMS=4 扔整组；
/// 空手/无世界只回 Ack（无变化不断连）。成功时按 P28 朝向生成掉落实体 +
/// 全量背包同步 + 刷盘，最后恒回 Ack 对齐客户端预测。
pub(crate) async fn handle_throw_action(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    conn_user: &str,
    mutable: Option<&MutableWorld>,
    feet: (f64, f64, f64),
    look: (f32, f32),
    act: &packets::PlayerAction,
) -> io::Result<()> {
    let all = act.status == packets::PLAYER_ACTION_DROP_ALL_ITEMS;
    let (drops, world) = match (opts.drops.as_ref(), mutable) {
        (Some(d), Some(w)) => (d, w),
        _ => return send_ack_only(stream, opts.threshold, act.sequence).await,
    };
    let thrown = drops.with_inventory(opts.conn_id, |inv| inv.throw_held(all)).flatten();
    if let Some((item_id, count)) = thrown {
        spawn_thrown_items(stream, opts, drops, world, feet, look, item_id, count).await?;
        send_full_inventory(stream, opts, drops).await?;
        drops.save_inventory(opts.conn_id, conn_user);
    }
    send_ack_only(stream, opts.threshold, act.sequence).await
}

/// 每 50ms 连接侧同步（**不是**物理 tick）：视距补/卸 + despawn 清理 + 拾取 + 入库同步。
/// 物理（下落/age/despawn/支撑重算）只由全服唯一 ticker 跑，本函数绝不调用 items.tick()。
pub(crate) async fn tick_drops(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    conn_user: &str,
    last_pos: (f64, f64, f64),
    view: &ChunkView,
) -> io::Result<()> {
    // 视距一致性：遍历全量 ids（moved 仅供权威物理内部使用），进视距补 Spawn /
    // 出视距发 Remove。不发 TeleportEntitySync，避免与客户端物理打架。
    sync_view_entities(stream, opts, drops, view).await?;
    // 拾取：delay 到期 + 距离 <1.5 → TakeItem+Remove，入库，全量同步 + 刷盘。
    // items.pickable + items.remove 在同一把锁内，连接间先到先得，天然互斥。
    let picked: Vec<(i32, i32, i32, mc_core::ChunkPos)> = {
        let mut items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        let ids = items.pickable(last_pos);
        let mut out = Vec::new();
        for id in ids {
            if let Some(e) = items.remove(id) {
                out.push((id, e.item_id, e.count, e.chunk));
            }
        }
        out
    };
    if !picked.is_empty() {
        for (id, _, count, _) in &picked {
            let mut body = Vec::new();
            packets::TakeItemEntity {
                collected_id: *id,
                collector_id: PLAYER_ENTITY_ID,
                count: *count,
            }
            .encode_body(&mut body);
            write_packet_any(stream, packets::PLAY_PICKUP_ITEM_S2C_ID, &body, opts.threshold).await?;
        }
        let gone: Vec<i32> = picked.iter().map(|(id, _, _, _)| *id).collect();
        send_remove(stream, opts, drops, &gone).await?;
        // P21：拾取动画 + 移除即时同步给同视距其它连接（逐实体按其所在区块判定）。
        for (id, _, count, chunk) in &picked {
            broadcast_takeitem_to_viewers(opts, *chunk, *id, *count);
            broadcast_remove_to_viewers(opts, drops, *chunk, &[*id]);
        }
        for (_, item_id, count, _) in picked {
            drops.with_inventory(opts.conn_id, |inv| {
                inv.add_item(item_id, count);
            });
        }
        send_full_inventory(stream, opts, drops).await?;
        drops.save_inventory(opts.conn_id, conn_user);
    }
    // P31 拾取经验球：delay 到期 + 距离 <1.5 → TakeItem(count=1，wiki 777 口径)
    // + Remove + 累加 XP + SetExperience + 刷盘。与物品拾取同一把 orbs 锁先到先得。
    let picked_orbs: Vec<(i32, i32, mc_core::ChunkPos)> = {
        let mut orbs = drops.orbs.lock().unwrap_or_else(|p| p.into_inner());
        let ids = orbs.pickable(last_pos);
        let mut out = Vec::new();
        for id in ids {
            if let Some(o) = orbs.remove(id) {
                out.push((id, o.amount, o.chunk));
            }
        }
        out
    };
    if !picked_orbs.is_empty() {
        for (id, _, _) in &picked_orbs {
            let mut body = Vec::new();
            packets::TakeItemEntity {
                collected_id: *id,
                collector_id: PLAYER_ENTITY_ID,
                count: 1, // wiki take_item_entity：XP 球恒为 1
            }
            .encode_body(&mut body);
            write_packet_any(stream, packets::PLAY_PICKUP_ITEM_S2C_ID, &body, opts.threshold).await?;
        }
        let gone: Vec<i32> = picked_orbs.iter().map(|(id, _, _)| *id).collect();
        send_remove(stream, opts, drops, &gone).await?;
        for (id, _, chunk) in &picked_orbs {
            broadcast_takeitem_to_viewers(opts, *chunk, *id, 1);
            broadcast_remove_to_viewers(opts, drops, *chunk, &[*id]);
        }
        let gained: i32 = picked_orbs.iter().map(|(_, amount, _)| *amount).sum();
        drops.with_inventory(opts.conn_id, |inv| {
            inv.add_experience(gained);
        });
        send_experience(stream, opts, drops).await?;
        drops.save_inventory(opts.conn_id, conn_user);
    }
    // despawn 清理放在拾取之后：全局 ticker 移除的过期实体、以及其它连接刚捡走的
    // 实体，本连接都能在同一 tick 按 known 差集补发 Remove（拾取自己发的已 unmark，
    // 不会重复）。id 全局唯一，故两张池都要查（只查 items 会误删仍存在的经验球）。
    let known = drops.known_ids(opts.conn_id);
    let gone: Vec<i32> = {
        let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        let orbs = drops.orbs.lock().unwrap_or_else(|p| p.into_inner());
        known
            .into_iter()
            .filter(|id| items.get(*id).is_none() && orbs.get(*id).is_none())
            .collect()
    };
    send_remove(stream, opts, drops, &gone).await?;
    Ok(())
}

/// 视距变化时补/卸实体：出视距的发 Remove，进视距的补发 Spawn。
/// P31：同一 `known` 集同时跟踪物品与经验球（id 全局唯一，不会混淆；按类型发对应 Spawn）。
#[derive(Clone, Copy)]
enum SyncKind {
    Item,
    Orb,
}

pub(crate) async fn sync_view_entities(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    view: &ChunkView,
) -> io::Result<()> {
    let snap: Vec<(i32, mc_core::ChunkPos, SyncKind)> = {
        // 锁顺序恒为 items → orbs（与全局 ticker 一致，避免死锁）。
        let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        let orbs = drops.orbs.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = Vec::new();
        for id in items.ids() {
            if let Some(e) = items.get(id) {
                out.push((id, e.chunk, SyncKind::Item));
            }
        }
        for id in orbs.ids() {
            if let Some(o) = orbs.get(id) {
                out.push((id, o.chunk, SyncKind::Orb));
            }
        }
        out
    };
    let mut gone = Vec::new();
    for (id, chunk, _) in &snap {
        if !drop_in_view(view.center(), view.radius(), *chunk) && drops.is_known(opts.conn_id, *id) {
            gone.push(*id);
        }
    }
    send_remove(stream, opts, drops, &gone).await?;
    for (id, chunk, kind) in &snap {
        if drop_in_view(view.center(), view.radius(), *chunk) && !drops.is_known(opts.conn_id, *id) {
            match kind {
                SyncKind::Item => send_spawn(stream, opts, drops, *id).await?,
                SyncKind::Orb => send_orb_spawn(stream, opts, drops, *id).await?,
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("mc-drop-body-{}-{tag}-{n}", std::process::id()))
    }

    #[test]
    fn full_inventory_body_matches_inventory_state() {
        let dir = unique_dir("inv");
        let drops = DropState::new(dir.clone(), 1);
        // 未 Join 的 conn：无背包，组包返回 None（管理线程据此判 delivered=false）。
        assert!(build_full_inventory_body(&drops, 42).is_none());
        drops.load_inventory(42, "body-test");
        // 新背包开局：hotbar 槽 36 有 64 石头（item 1）。
        let body = build_full_inventory_body(&drops, 42).expect("joined conn must have a body");
        let (content, _) = packets::SetContainerContent::decode_body(&body).unwrap();
        assert_eq!(content.window_id, packets::PLAYER_INVENTORY_WINDOW_ID);
        assert_eq!(content.slots.len(), packets::PLAYER_INVENTORY_SLOT_COUNT);
        assert_eq!(content.slots[36], packets::Slot { count: 64, item_id: 1 });
        assert_eq!(content.carried, packets::Slot::empty());
        // 加 cobble 后重组包：新包体含新增物，且 state_id 单调递增。
        let cobble = mc_world::drops::ITEM_COBBLESTONE;
        drops.with_inventory(42, |inv| inv.add_item(cobble, 65));
        let body2 = build_full_inventory_body(&drops, 42).unwrap();
        let (c2, _) = packets::SetContainerContent::decode_body(&body2).unwrap();
        assert!(c2.state_id > content.state_id, "state_id must be monotonic");
        let total: i32 =
            c2.slots.iter().filter(|s| s.item_id == cobble).map(|s| s.count).sum();
        assert_eq!(total, 65, "added cobble must appear in the sync body");
        drops.unload_inventory(42, "body-test");
        assert!(build_full_inventory_body(&drops, 42).is_none(), "after unload no body");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// P28：yaw/pitch → 方向向量边界值（原版 `directionFromRotation` 同构）。
    #[test]
    fn look_dir_cardinal_and_vertical_boundaries() {
        // 平视四正向（pitch=0）：yaw=0 → +Z，yaw=180 → -Z，yaw=-90 → +X，yaw=90 → -X。
        for (yaw, want) in
            [(0.0f32, (0.0, 0.0, 1.0)), (180.0, (0.0, 0.0, -1.0)), (-90.0, (1.0, 0.0, 0.0)), (90.0, (-1.0, 0.0, 0.0))]
        {
            let d = look_dir(yaw, 0.0);
            assert!(
                (d.0 - want.0).abs() < 1e-6 && (d.1 - want.1).abs() < 1e-6 && (d.2 - want.2).abs() < 1e-6,
                "yaw={yaw} must face {want:?}, got {d:?}"
            );
        }
        // 垂直：pitch=-90 抬头 → +Y，pitch=90 低头 → -Y（yaw 无关）。
        for yaw in [0.0f32, -90.0, 90.0, 180.0] {
            let up = look_dir(yaw, -90.0);
            assert!((up.0).abs() < 1e-6 && (up.2).abs() < 1e-6 && (up.1 - 1.0).abs() < 1e-6,
                "pitch=-90 must face up regardless of yaw={yaw}, got {up:?}");
            let down = look_dir(yaw, 90.0);
            assert!((down.0).abs() < 1e-6 && (down.2).abs() < 1e-6 && (down.1 + 1.0).abs() < 1e-6,
                "pitch=90 must face down regardless of yaw={yaw}, got {down:?}");
        }
        // 45° 俯仰：yaw=0/pitch=-45 → (0, +√2/2, +√2/2)，模长为 1。
        let d = look_dir(0.0, -45.0);
        let s = std::f64::consts::FRAC_1_SQRT_2;
        assert!((d.0).abs() < 1e-6 && (d.1 - s).abs() < 1e-6 && (d.2 - s).abs() < 1e-6,
            "yaw=0/pitch=-45 must split +Y/+Z evenly, got {d:?}");
        let n = (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt();
        assert!((n - 1.0).abs() < 1e-9, "look_dir must be unit length, got {n}");
    }

    /// P28.2：平扔初速口径（朝向×0.06 + 纵向基线 0.1；0.25 会从脸前弹起，手感不对）。
    #[test]
    fn throw_velocity_matches_flat_throw_caliber() {
        let dir = look_dir(-90.0, 0.0);
        let vel = (dir.0 * THROW_SPEED, THROW_UP_BIAS + dir.1 * THROW_SPEED, dir.2 * THROW_SPEED);
        assert!((vel.0 - 0.06).abs() < 1e-9, "flat +X throw must carry +X speed, got {vel:?}");
        assert!((vel.1 - 0.1).abs() < 1e-9, "flat throw uses the gentle 0.1 up bias, got {vel:?}");
        assert!(vel.2.abs() < 1e-9, "flat +X throw must not drift in Z, got {vel:?}");
        assert_eq!(THROW_FORWARD_DIST, 0.15, "spawn offset stays near the head");
    }

    /// P28.2：平扔落点仍在自捡半径内（旧 e2e `broadcast_thrown_drop…` 要求原地自捡）。
    /// 眼位 + 前 0.15 格生成、初速 (0.06, 0.1, 0)，tick 到落地后与脚距离 < 1.5。
    #[test]
    fn flat_throw_lands_within_pickup_reach() {
        let feet = (0.5, 70.0, 0.5);
        let dir = look_dir(0.0, 0.0);
        let sx = feet.0 + dir.0 * THROW_FORWARD_DIST;
        let sy = feet.1 + mc_world::EYE_HEIGHT + dir.1 * THROW_FORWARD_DIST;
        let sz = feet.2 + dir.2 * THROW_FORWARD_DIST;
        let vel =
            (dir.0 * THROW_SPEED, THROW_UP_BIAS + dir.1 * THROW_SPEED, dir.2 * THROW_SPEED);
        let mut es = mc_world::ItemEntities::new();
        es.spawn_with_velocity(
            9001,
            1,
            1,
            sx,
            sy,
            sz,
            feet.1,
            vel,
            mc_world::items::THROWN_PICKUP_DELAY_TICKS,
        )
        .unwrap();
        for _ in 0..400 {
            es.tick();
            if es.get(9001).unwrap().on_ground {
                break;
            }
        }
        let e = es.get(9001).unwrap();
        assert!(e.on_ground, "thrown drop must land, pos={:?}", e.pos);
        let (dx, dy, dz) = (e.pos.0 - feet.0, e.pos.1 - feet.1, e.pos.2 - feet.2);
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        assert!(dist < mc_world::items::PICKUP_RADIUS, "landed drop must be re-pickable, dist={dist}");
    }
}
