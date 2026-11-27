//! survival_sync: P40 SetHealth 发包与坠落伤害接线（只兼容 26.3/777）。
//!
//! `set_health` S2C ID=106（0x6A）由 `data/777/packets.json` 经 mc-registry 生成表查得；
//! 包体按 minecraft.wiki Play#Set_Health 已验证口径：Health Float / Food VarInt /
//! Food Saturation Float。
//!
//! TODO（明确未做）：不发 CB Respawn（0x54 / 十进制 84）。该包体需 dimension_type
//! 注册表 ID + hashed seed + 死亡地点等十余字段，本轮未做原版抓包验证，按任务约束
//! 宁可不猜；死亡闭环改用已验证的 PlayerPositionSync 0x49 + SetHealth + 全量 Content。
//! 客户端死亡界面/Respawn 按钮的真实体验留待后续抓包后补齐。

use tokio::io::{AsyncRead, AsyncWrite};
use super::login::{write_packet_any, PlayOptions};
use crate::drop_state::DropState;
use mc_protocol::packets;
use mc_world::SurvivalState;
use std::io;
use tracing::debug;

/// 组 SetHealth 包体（纯函数；发包线程/全局 ticker 共用）。
pub fn build_set_health_body(s: &SurvivalState) -> Vec<u8> {
    let mut body = Vec::new();
    packets::SetHealth { health: s.health, food: s.food, saturation: s.saturation }
        .encode_body(&mut body);
    body
}

/// 把当前生存状态同步给本连接（Join 读盘非满值 / 受伤 / 进食 / 复活）。
pub(crate) async fn send_set_health<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    opts: &PlayOptions,
    drops: &DropState,
    conn_id: u64,
) -> io::Result<()> {
    let Some(state) = drops.survival_state(conn_id) else { return Ok(()) };
    let body = build_set_health_body(&state);
    write_packet_any(stream, packets::PLAY_SET_HEALTH_S2C_ID, &body, opts.threshold).await
}

/// 组 PlayerPositionSync 包体（复活回出生点，已验证 0x49/61B 口径，与 Join 同构）。
pub fn build_respawn_position_body(spawn: (i32, i32, i32), teleport_id: i32) -> Vec<u8> {
    let mut body = Vec::new();
    packets::PlayerPositionSync {
        teleport_id,
        x: spawn.0 as f64 + 0.5,
        y: spawn.1 as f64,
        z: spawn.2 as f64 + 0.5,
        delta_x: 0.0,
        delta_y: 0.0,
        delta_z: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        relatives: 0,
    }
    .encode_body(&mut body);
    body
}

/// 移动包落地检测 → 权威扣血 + 本连接 SetHealth 同步（连接侧只做判定，不推进 tick）。
/// 返回是否实际扣血（死亡由下一个全局 ticker tick 统一处理掉落/复活）。
pub(crate) async fn apply_fall_and_sync<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    opts: &PlayOptions,
    drops: &DropState,
    blocks: f64,
) -> io::Result<bool> {
    let Some(after) = drops.apply_fall(opts.conn_id, blocks) else { return Ok(false) };
    let dmg = (mc_world::survival::MAX_HEALTH - after.health).max(0.0);
    if dmg <= 0.0 {
        return Ok(false);
    }
    send_set_health(stream, opts, drops, opts.conn_id).await?;
    debug!("play conn_id={} fall damage applied (drop={blocks:.2}, health={:.1})", opts.conn_id, after.health);
    Ok(true)
}

/// 手持食物右键：权威确认是食物 → 扣 1 个 + 补饥饿/饱和 + SetHealth + 单槽同步 + 刷盘。
/// 返回 `Ok(true)` 表示真的吃了；`Ok(false)` = 不是食物/空手（调用方照常只 ack）。
pub(crate) async fn eat_held<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    opts: &PlayOptions,
    drops: &DropState,
) -> io::Result<bool> {
    let Some(food) = drops
        .with_inventory(opts.conn_id, |inv| {
            inv.held().and_then(|(id, _)| mc_world::food_for_item(id))
        })
        .flatten()
    else {
        return Ok(false);
    };
    let consumed = drops
        .with_inventory(opts.conn_id, |inv| inv.consume_held())
        .flatten();
    if consumed.is_none() {
        return Ok(false);
    }
    drops.apply_food(opts.conn_id, food.0, food.1);
    send_set_health(stream, opts, drops, opts.conn_id).await?;
    let (idx, slot, state) = drops
        .with_inventory(opts.conn_id, |inv| {
            let idx = inv.selected_slot_index();
            (idx, inv.protocol_slot(idx), inv.next_state())
        })
        .expect("inventory was present above");
    let mut body = Vec::new();
    packets::SetContainerSlot {
        window_id: packets::PLAYER_INVENTORY_WINDOW_ID,
        state_id: state,
        slot: idx as i16,
        data: slot,
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_CONTAINER_SLOT_S2C_ID, &body, opts.threshold).await?;
    if let Some(tag) = drops.player_tag(opts.conn_id) {
        drops.save_inventory(opts.conn_id, &tag);
    }
    Ok(true)
}
