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
    let (slots, carried, state) = drops
        .with_inventory(opts.conn_id, |inv| {
            (inv.to_protocol_slots(), inv.cursor_slot(), inv.next_state())
        })
        .unwrap_or_else(|| {
            (vec![packets::Slot::empty(); packets::PLAYER_INVENTORY_SLOT_COUNT], packets::Slot::empty(), 1)
        });
    let mut body = Vec::new();
    packets::SetContainerContent {
        window_id: packets::PLAYER_INVENTORY_WINDOW_ID,
        state_id: state,
        slots,
        carried,
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, &body, opts.threshold)
        .await
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

/// 挖掘掉落生成：每种 drop 一个实体（同位合并由 ItemEntities 处理）。
pub(crate) async fn spawn_drops(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    world: &MutableWorld,
    outcome: &BlockEditOutcome,
) -> io::Result<()> {
    if outcome.drops.is_empty() || outcome.changes.is_empty() {
        return Ok(());
    }
    let (bx, by, bz) = {
        let c = &outcome.changes[0];
        (c.x, c.y, c.z)
    };
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
    Ok(())
}

/// 只回 Ack（无世界影响的操作：空手 Q/空手放/UseItem；对齐客户端序列预测）。
pub(crate) async fn send_ack_only(stream: &mut TcpStream, threshold: i32, sequence: i32) -> io::Result<()> {
    let mut ack = Vec::new();
    mc_protocol::write_varint(sequence, &mut ack);
    write_packet_any(stream, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, &ack, threshold).await
}

/// Q 扔物生成（P19）：在玩家脚下生成掉落实体，复用挖掘掉落的 Spawn/Data 同步路径
///（同物同 chunk 半径 1 格合并；`touched` 的其它实体只补发 Data，未知连接补 Spawn）。
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
    item_id: i32,
    count: i32,
) -> io::Result<()> {
    if count <= 0 {
        return Ok(());
    }
    let id = drops.alloc_id();
    let (bx, by, bz) = (feet.0.floor() as i32, feet.1.floor() as i32, feet.2.floor() as i32);
    let chunk = mc_world::chunk_of_block(bx, bz);
    let rest = world.drop_rest_y(bx, bz, by) as f64;
    let touched = {
        let mut items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        match items.spawn(id, item_id, count, feet.0, feet.1, feet.2, rest) {
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
/// 空手/无世界只回 Ack（无变化不断连）。成功时脚下生成掉落实体 +
/// 全量背包同步 + 刷盘，最后恒回 Ack 对齐客户端预测。
pub(crate) async fn handle_throw_action(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    conn_user: &str,
    mutable: Option<&MutableWorld>,
    feet: (f64, f64, f64),
    act: &packets::PlayerAction,
) -> io::Result<()> {
    let all = act.status == packets::PLAYER_ACTION_DROP_ALL_ITEMS;
    let (drops, world) = match (opts.drops.as_ref(), mutable) {
        (Some(d), Some(w)) => (d, w),
        _ => return send_ack_only(stream, opts.threshold, act.sequence).await,
    };
    let thrown = drops.with_inventory(opts.conn_id, |inv| inv.throw_held(all)).flatten();
    if let Some((item_id, count)) = thrown {
        spawn_thrown_items(stream, opts, drops, world, feet, item_id, count).await?;
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
    // despawn 清理放在拾取之后：全局 ticker 移除的过期实体、以及其它连接刚捡走的
    // 实体，本连接都能在同一 tick 按 known 差集补发 Remove（拾取自己发的已 unmark，
    // 不会重复）。
    let known = drops.known_ids(opts.conn_id);
    let gone: Vec<i32> = {
        let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        known.into_iter().filter(|id| items.get(*id).is_none()).collect()
    };
    send_remove(stream, opts, drops, &gone).await?;
    Ok(())
}

/// 视距变化时补/卸实体：出视距的发 Remove，进视距的补发 Spawn。
pub(crate) async fn sync_view_entities(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    view: &ChunkView,
) -> io::Result<()> {
    let snap: Vec<(i32, mc_core::ChunkPos)> = {
        let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        items.ids().iter().filter_map(|id| items.get(*id).map(|e| (*id, e.chunk))).collect()
    };
    let mut gone = Vec::new();
    for (id, chunk) in &snap {
        if !drop_in_view(view.center(), view.radius(), *chunk) && drops.is_known(opts.conn_id, *id) {
            gone.push(*id);
        }
    }
    send_remove(stream, opts, drops, &gone).await?;
    for (id, chunk) in &snap {
        if drop_in_view(view.center(), view.radius(), *chunk) && !drops.is_known(opts.conn_id, *id) {
            send_spawn(stream, opts, drops, *id).await?;
        }
    }
    Ok(())
}
