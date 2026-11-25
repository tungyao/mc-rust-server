//! view_sync: cross-chunk view throttle + SetCenter / Unload / Batch (moved from login_route.rs view section, no logic change).
use super::drop_sync::sync_view_entities;
use super::login::{send_chunk_batch, send_unload_chunks, write_packet_any, PlayOptions};
use crate::chunk_cache::ChunkCache;
use crate::chunk_view::ChunkView;
use mc_protocol::packets;
use mc_world::{MutableWorld, WorldGen};
use tokio::net::TcpStream;
use tracing::info;
use std::io;
use std::time::{Duration, Instant};

/// 跨区块视距更新的节流窗口：连续跨区块时只记录最新目标中心，窗口到期（或跨越次数
/// 达到上限）才一次性发 SetCenterChunk + UnloadChunk + ChunkBatch。避免走远时每个边界
/// 都立刻吐出一批区块包，把后面的掉落 Spawn 挤在长队列尾部。
const VIEW_MOVE_WINDOW: Duration = Duration::from_millis(80);
/// 窗口内累计中心变化次数上限：脚本/传送等极快移动提前 flush，防止地形长时间滞后。
const VIEW_MOVE_MAX_CROSSINGS: usize = 6;

/// 待发送的视距更新：只记最新目标中心，**不提前改 `ChunkView`**——`sent` 集合必须
/// 始终等于客户端实际已收到的区块，flush 时一次 `plan(final_center)` 得到净增量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingViewMove {
    center: mc_core::ChunkPos,
    first_at: Instant,
    crossings: usize,
}

impl PendingViewMove {
    fn should_flush(&self) -> bool {
        self.first_at.elapsed() >= VIEW_MOVE_WINDOW || self.crossings >= VIEW_MOVE_MAX_CROSSINGS
    }
}

/// 记录一次位置更新对应的目标区块中心；走回原中心直接丢弃待发送更新（客户端视距未变）。
pub(crate) fn queue_view_move(pending: &mut Option<PendingViewMove>, current: mc_core::ChunkPos, x: f64, z: f64) {
    let Some(center) = ChunkView::chunk_of_block(x, z) else { return };
    if center == current {
        pending.take();
        return;
    }
    match pending.as_mut() {
        Some(p) if p.center == center => {}
        Some(p) => {
            p.center = center;
            p.crossings += 1;
        }
        None => {
            *pending = Some(PendingViewMove { center, first_at: Instant::now(), crossings: 1 });
        }
    }
}

/// 玩家跨区块：SetCenterChunk → 卸载越界区块 → 一批补发缺失区块（batch_size = add.len）。
/// 由节流窗口在 flush 时调用，传入的是最后一次移动的目标中心；`view.plan` 只在此时执行，
/// 因此 add/remove 是「旧完整视距 → 新完整视距」的净差，客户端不会缺邻居 section，
/// `ChunkView.sent` 也与真正发出的包严格一致。
async fn maybe_move_view(
    stream: &mut TcpStream,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
    view: &mut ChunkView,
    world: &WorldGen,
    center: mc_core::ChunkPos,
    cache: Option<&ChunkCache>,
    mutable: Option<&MutableWorld>,
) -> io::Result<()> {
    if center == view.center() {
        return Ok(());
    }
    let threshold = opts.threshold;
    let mut body = Vec::new();
    packets::SetCenterChunk { chunk_x: center.0, chunk_z: center.1 }.encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_CENTER_CHUNK_ID, &body, threshold).await?;
    let update = view.plan(center);
    send_unload_chunks(stream, &update.remove, threshold).await?;
    send_chunk_batch(stream, world, &update.add, threshold, cache, mutable).await?;
    info!(
        "play view move uuid={} name={} conn_id={} center=({},{}) radius={} add={} remove={} sent_total={}",
        profile.uuid,
        profile.username,
        opts.conn_id,
        center.0,
        center.1,
        view.radius(),
        update.add.len(),
        update.remove.len(),
        view.sent_count()
    );
    Ok(())
}

/// 把待发送视距更新一次性发出，并把掉落物实体同步到新中心（实体包必须排在区块包之后，
/// 否则客户端还没加载对应区块就收到 Spawn）。
async fn flush_view_move(
    stream: &mut TcpStream,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
    view: &mut ChunkView,
    world: &WorldGen,
    cache: Option<&ChunkCache>,
    mutable: Option<&MutableWorld>,
    center: mc_core::ChunkPos,
) -> io::Result<()> {
    maybe_move_view(stream, profile, opts, view, world, center, cache, mutable).await?;
    if let Some(ds) = &opts.drops {
        sync_view_entities(stream, opts, ds, view).await?;
    }
    Ok(())
}

/// 视距节流入口：窗口到期/跨越次数到上限才真正发包；`force=true` 用于挖掘/放置前
/// 强制刷新，保证权威判定的中心与客户端已加载区块一致，且 Spawn 排在区块包之后。
pub(crate) async fn maybe_flush_view(
    stream: &mut TcpStream,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
    view: &mut ChunkView,
    world: &WorldGen,
    cache: Option<&ChunkCache>,
    mutable: Option<&MutableWorld>,
    pending_view: &mut Option<PendingViewMove>,
    force: bool,
) -> io::Result<()> {
    let Some(pending) = *pending_view else { return Ok(()) };
    if !force && !pending.should_flush() {
        return Ok(());
    }
    *pending_view = None;
    flush_view_move(stream, profile, opts, view, world, cache, mutable, pending.center).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_view_move_coalesces_crossings_and_clears_on_return() {
        let current = mc_core::ChunkPos(0, 0);
        let mut pending = None;
        // 同中心不入队；连续跨 3 格只保留最终中心并累计次数。
        queue_view_move(&mut pending, current, 8.0, 8.0);
        assert!(pending.is_none());
        for cx in [1, 2, 3] {
            queue_view_move(&mut pending, current, (cx * 16 + 1) as f64, 8.0);
        }
        let p = pending.unwrap();
        assert_eq!(p.center, mc_core::ChunkPos(3, 0));
        assert_eq!(p.crossings, 3);
        assert!(!p.should_flush(), "fresh window below cap must wait");
        // 走回原中心 → 待发送更新作废（客户端视距没变，无需任何包）。
        queue_view_move(&mut pending, current, 8.0, 8.0);
        assert!(pending.is_none());
    }

    #[test]
    fn pending_view_move_flushes_on_window_expiry_or_crossing_cap() {
        let cap = PendingViewMove {
            center: mc_core::ChunkPos(1, 0),
            first_at: Instant::now(),
            crossings: VIEW_MOVE_MAX_CROSSINGS,
        };
        assert!(cap.should_flush(), "crossing cap must force a flush");
        let expired = PendingViewMove {
            center: mc_core::ChunkPos(1, 0),
            first_at: Instant::now() - VIEW_MOVE_WINDOW - Duration::from_millis(1),
            crossings: 1,
        };
        assert!(expired.should_flush(), "elapsed window must flush");
    }
}
