//! login_route: Login离线流程 Start→[SetCompression]→Success→Ack→Config(Finish→Ack)→enter_play(真下发)
//! Play 期：KeepAlive + C2S 移动包；移动包经 crossbeam channel 投递 ECS（PlayEvent），
//! 同时 net 线程自身按玩家跨区块动态补发有效视距区块并卸载越界区块。
//! 视距来自 `ServerConfig.view_distance`（`clamp_view_radius` 限制 2..=10），
//! 区块包体优先走共享 `ChunkCache`；连接结束在所有退出路径恰好投递一次 Disconnect。
pub mod block_edit;
pub mod drop_sync;
pub mod login;
pub mod view_sync;

pub use self::block_edit::{decide_dig, decide_place, decide_plant, BlockChange, BlockEditOutcome};
pub use self::login::{config_finish, enter_play, enter_play_with_events, enter_play_with_options, handle_login, handle_login_configured, handle_login_with_options, handle_login_with_threshold, notify_login_success, PlayOptions};

use self::block_edit::{apply_block_outcome, eye_of};
use self::drop_sync::{handle_throw_action, send_full_inventory, send_held_slot, spawn_drops, spawn_thrown_items, tick_drops};
use self::login::{emit_play_event, read_packet_any, user_tag, write_packet_any};
use self::view_sync::{maybe_flush_view, queue_view_move, PendingViewMove};
use crate::broadcast::QueuedPacket;
use crate::chunk_cache::ChunkCache;
use crate::chunk_view::ChunkView;
use crate::play_events::PlayEvent;
use mc_protocol::packets;
use mc_world::{MutableWorld, WorldGen};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use std::io;

/// P21：排空本连接的广播队列（其它玩家投递的 BlockUpdate/Spawn/Data/Remove/TakeItem），
///
/// 在 tick 间隙写入自己的 socket；队列为空直接返回，写失败则断开并走统一清理路径。
async fn drain_outbox(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    outbox: &Option<crossbeam_channel::Receiver<QueuedPacket>>,
) -> io::Result<()> {
    let Some(rx) = outbox else { return Ok(()) };
    while let Ok(pkt) = rx.try_recv() {
        write_packet_any(stream, pkt.id, &pkt.body, opts.threshold).await?;
    }
    Ok(())
}

async fn play_loop(
    stream: &mut TcpStream,
    addr: SocketAddr,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
    mut seq: i64,
    mut view: ChunkView,
    world: &WorldGen,
    cache: Option<&ChunkCache>,
    mutable: Option<&MutableWorld>,
    outbox: Option<crossbeam_channel::Receiver<QueuedPacket>>,
) -> io::Result<()> {
    let threshold = opts.threshold;
    // net 线程的 last_seen 只认 KeepAlive 回显（移动包不刷新，避免客户端只发移动不回心跳）；
    // ECS 侧的 PlayerKeepAlive.last_seen 另由 movement_ingest_system 按移动包刷新，用于离线清理。
    let mut last_seen = Instant::now();
    // P17 方块判定用的玩家位置（C2S 移动包直接更新；初始为出生点）。
    // 与 ECS Position 同源（同一批移动包），net 线程先看到，判定距离用它足够。
    let spawn = world.spawn_pos();
    let mut last_pos = (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5);
    // 客户端在 Play 里每 tick 都发 0x0D，read_packet 不会超时；必须按时间主动发 KeepAlive，
    // 否则客户端 30s 收不到 0x2D 会 “连接超时” 断开（2026-11-23 真机实测）。
    let mut last_keepalive = Instant::now();
    let conn_user = user_tag(profile);
    // P18 掉落同步节拍：按墙钟每 50ms 跑一次连接侧拾取/视距同步（物理在全局 ticker）。
    // 注意不能只放在读超时分支——原版客户端每 tick 都发移动包（静止也有 StatusOnly），
    // 包流不断时 50ms 超时永不触发，tick 会被饿死：pickup_delay 减不下去，
    // 掉落物看得见但永远捡不起来（P18 回归：busy 连接拾取测试）。
    let mut last_drop_tick = Instant::now();
    // 跨区块节流：移动包只更新目标中心，窗口到期/次数上限再合并发包（见 PendingViewMove）。
    let mut pending_view: Option<PendingViewMove> = None;
    loop {
        // P18 掉落 tick：50ms 读超时即跑一次物理/拾取（不改变 10s 半开超时语义）。
        let next = match tokio::time::timeout(
            Duration::from_millis(50),
            read_packet_any(stream, threshold),
        )
        .await
        {
            Err(_) => {
                // 读超时也是节流窗口的节拍：玩家停止移动后仍要 flush 最后一次跨区块更新。
                if let Err(e) = maybe_flush_view(
                    stream, profile, opts, &mut view, world, cache, mutable, &mut pending_view, false,
                )
                .await
                {
                    debug!("play uuid={} view flush failed ({e}), closing", profile.uuid);
                    break;
                }
                if let Some(ds) = &opts.drops {
                    if let Err(e) = tick_drops(stream, opts, ds, &conn_user, last_pos, &view).await {
                        debug!("play uuid={} drop sync failed ({e}), closing", profile.uuid);
                        break;
                    }
                }
                if let Err(e) = drain_outbox(stream, opts, &outbox).await {
                    debug!("play uuid={} outbox drain failed ({e}), closing", profile.uuid);
                    break;
                }
                last_drop_tick = Instant::now();
                continue;
            }
            Ok(r) => r,
        };
        if last_keepalive.elapsed() >= Duration::from_secs(15) {
            seq += 1;
            let mut body = Vec::new();
            packets::KeepAliveS2C(seq).encode_body(&mut body);
            if let Err(e) = write_packet_any(stream, packets::PLAY_KEEPALIVE_S2C_ID, &body, threshold).await {
                debug!("play uuid={} keepalive send failed ({e}), closing", profile.uuid);
                break;
            }
            last_keepalive = Instant::now();
            debug!("play keepalive send uuid={} seq={seq}", profile.uuid);
        }
        match next {
            Ok((id, payload)) if id == packets::PLAY_KEEPALIVE_C2S_ID => {
                match packets::decode_keepalive_c2s(&payload) {
                    Ok(echo) => {
                        last_seen = Instant::now();
                        info!("play keepalive echo uuid={} name={} id={echo}", profile.uuid, profile.username);
                        // ECS 用它刷新 PlayerKeepAlive.last_seen；在线数仍只在 ECS 删除点减。
                        emit_play_event(&opts.events, PlayEvent::KeepAlive { addr, conn_id: opts.conn_id });
                    }
                    Err(e) => debug!("play uuid={} bad keepalive: {e}", profile.uuid),
                }
            }
            Ok((id, payload))
                if id == packets::PLAY_MOVE_POS_C2S_ID
                    || id == packets::PLAY_MOVE_POSROT_C2S_ID
                    || id == packets::PLAY_MOVE_ROT_C2S_ID
                    || id == packets::PLAY_MOVE_FLAGS_C2S_ID =>
            {
                match packets::decode_c2s_movement(id, &payload) {
                    Ok(m) => {
                        info!(
                            "play move uuid={} name={} id={:#04x} m={:?} forwarded_to_ecs={}",
                            profile.uuid, profile.username, id, m, opts.events.is_some()
                        );
                        // 跨区块只记目标中心，真正发包交给节流窗口（窗口到期或次数上限）。
                        // 这样连续跨区块会合并成一次 SetCenterChunk/Unload/ChunkBatch，
                        // 掉落 Spawn 不再排在几十个边界批次后面。
                        match m {
                            packets::C2SMovement::Pos(p) => {
                                last_pos = (p.x, p.y, p.z);
                                queue_view_move(&mut pending_view, view.center(), p.x, p.z);
                            }
                            packets::C2SMovement::PosRot(p) => {
                                last_pos = (p.x, p.y, p.z);
                                queue_view_move(&mut pending_view, view.center(), p.x, p.z);
                            }
                            _ => {}
                        }
                        // P21：广播集用接收方 last_pos，跨区块后自然跟随。
                        if let Some(hub) = &opts.hub {
                            hub.update_pos(opts.conn_id, last_pos);
                        }
                        emit_play_event(
                            &opts.events,
                            PlayEvent::Movement { addr, conn_id: opts.conn_id, movement: m },
                        );
                    }
                    Err(e) => debug!("play uuid={} bad movement id={id:#04x}: {e}", profile.uuid),
                }
            }
            Ok((id, payload)) if id == packets::PLAY_PLAYER_ACTION_C2S_ID => {
                // 挖掘：服务端权威判定 + BlockUpdate/Ack 回包；仅 STOP(3) 真落子
                // （START(0) 只 ack，让客户端播放原版破坏进度动画；ABORT(2) 只 ack 不落子，
                // 之前把 1/2 当取消/完成，松开鼠标即破坏；CHANGE(1) 直接忽略）。
                // 成功掉落同步生成物品实体（Spawn+Data，含初速，客户端自行模拟弹跳）。
                // Q 扔物（P19）：DROP_ITEM(5) 扔 1 个手持，DROP_ALL_ITEMS(4) 扔整组；
                // 空手只回 Ack（无变化不断连）。
                // 判定前强制 flush：保证 view.center() 已跟上玩家（出视距误回滚），
                // 也保证区块包先于 BlockUpdate/Spawn 到达客户端。
                if let Err(e) = maybe_flush_view(
                    stream, profile, opts, &mut view, world, cache, mutable, &mut pending_view, true,
                )
                .await
                {
                    debug!("play uuid={} view flush before dig failed ({e}), closing", profile.uuid);
                    break;
                }
                match packets::PlayerAction::decode_body(&payload) {
                    Ok((act, _)) => {
                        if act.status == packets::PLAYER_ACTION_DROP_ITEM
                            || act.status == packets::PLAYER_ACTION_DROP_ALL_ITEMS
                        {
                            if let Err(e) =
                                handle_throw_action(stream, opts, &conn_user, mutable, last_pos, &act)
                                    .await
                            {
                                debug!("play uuid={} throw failed ({e}), closing", profile.uuid);
                                break;
                            }
                        } else if let Some(outcome) = decide_dig(
                            mutable,
                            eye_of(last_pos),
                            view.center(),
                            view.radius(),
                            &act,
                        ) {
                            if let Some(world) = mutable {
                                if let Err(e) =
                                    apply_block_outcome(stream, addr, profile, opts, cache, &outcome).await
                                {
                                    debug!("play uuid={} block outcome failed ({e}), closing", profile.uuid);
                                    break;
                                }
                                if let Some(ds) = &opts.drops {
                                    if let Err(e) =
                                        spawn_drops(stream, opts, ds, world, &outcome).await
                                    {
                                        debug!("play uuid={} drop spawn failed ({e}), closing", profile.uuid);
                                        break;
                                    }
                                    // P22：挖掉小麦即摘除生长跟踪（ticker 懒摘除是兜底）。
                                    // P26：挖苗/挖土同理（所在土被挖则其上方树苗取消生长）。
                                    for c in &outcome.changes {
                                        if c.new_id == mc_world::AIR_BLOCK_ID
                                            && mc_world::wheat_age(c.old_id).is_some()
                                        {
                                            ds.uproot_crop(c.x, c.y, c.z);
                                        }
                                        if c.new_id == mc_world::AIR_BLOCK_ID
                                            && c.old_id == mc_world::OAK_SAPLING_BLOCK_ID
                                        {
                                            ds.uproot_sapling(c.x, c.y, c.z);
                                        }
                                        if c.new_id == mc_world::AIR_BLOCK_ID
                                            && (c.old_id == mc_world::GRASS_BLOCK_ID
                                                || c.old_id == mc_world::DIRT_BLOCK_ID)
                                            && world.get(c.x, c.y + 1, c.z)
                                                == mc_world::OAK_SAPLING_BLOCK_ID
                                        {
                                            ds.uproot_sapling(c.x, c.y + 1, c.z);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => debug!("play uuid={} bad player_action: {e}", profile.uuid),
                }
            }
            Ok((id, payload)) if id == packets::PLAY_USE_ITEM_ON_C2S_ID => {
                // P17 放置：目标 = 被点方块 + face 偏移；非法回滚重发。
                // P18：手持映射决定材料；空手/无方块映射（如圆石/煤）只 ack 不落子，成功后扣 1 + 单槽同步。
                // 种子/树苗走上面的种植专属路径，不经过这里。
                // 与挖掘同理：判定前强制 flush，避免节流中的旧中心导致合法放置被回滚。
                if let Err(e) = maybe_flush_view(
                    stream, profile, opts, &mut view, world, cache, mutable, &mut pending_view, true,
                )
                .await
                {
                    debug!("play uuid={} view flush before place failed ({e}), closing", profile.uuid);
                    break;
                }
                match packets::UseItemOn::decode_body(&payload) {
                    Ok((act, _)) => {
                        // P22/P23：手持小麦种子或树苗对土壤顶面右键 → 种下（专属路径，不走方块映射）。
                        // 树苗复用小麦的种植判定（仅 plant_id 不同），但不登记生长。
                        let held_plant: Option<u16> = opts
                            .drops
                            .as_ref()
                            .and_then(|ds| ds.with_inventory(opts.conn_id, |inv| inv.held()).flatten())
                            .and_then(|(item_id, _)| {
                                if item_id == mc_world::drops::ITEM_WHEAT_SEEDS {
                                    Some(mc_world::WHEAT_CROP_BASE_ID)
                                } else if item_id == mc_world::drops::ITEM_OAK_SAPLING {
                                    Some(mc_world::OAK_SAPLING_BLOCK_ID)
                                } else {
                                    None
                                }
                            });
                        if let Some(plant_id) = held_plant {
                            if let Some(outcome) = decide_plant(
                                mutable,
                                eye_of(last_pos),
                                view.center(),
                                view.radius(),
                                &act,
                                plant_id,
                            ) {
                                if mutable.is_some() {
                                    let planted = outcome.changes.iter().any(|c| {
                                        c.new_id == plant_id && c.old_id != c.new_id
                                    });
                                    if let Err(e) = apply_block_outcome(
                                        stream, addr, profile, opts, cache, &outcome,
                                    )
                                    .await
                                    {
                                        debug!("play uuid={} plant outcome failed ({e}), closing", profile.uuid);
                                        break;
                                    }
                                    if planted {
                                        if let Some(ds) = &opts.drops {
                                            // 小麦登记多阶段生长；树苗登记单次长树（P26）。
                                            if plant_id == mc_world::WHEAT_CROP_BASE_ID {
                                                if let Some(c) = outcome.changes.first() {
                                                    ds.plant_crop(c.x, c.y, c.z);
                                                }
                                            } else if plant_id == mc_world::OAK_SAPLING_BLOCK_ID {
                                                if let Some(c) = outcome.changes.first() {
                                                    ds.plant_sapling(c.x, c.y, c.z);
                                                }
                                            }
                                            let consumed = ds.with_inventory(opts.conn_id, |inv| {
                                                inv.consume_held()
                                            });
                                            if consumed.flatten().is_some() {
                                                if let Err(e) = send_held_slot(stream, opts, ds).await
                                                {
                                                    debug!("play uuid={} plant held sync failed ({e}), closing", profile.uuid);
                                                    break;
                                                }
                                                ds.save_inventory(opts.conn_id, &conn_user);
                                            }
                                        }
                                    }
                                }
                            }
                            continue;
                        }
                        // 手持映射决定材料；空手/无方块映射（如圆石/煤）原版不放任何方块，
                        // 只 ack 对齐客户端预测——之前回退放石头是 P17 兼容，已按反馈去掉。
                        let place_id: Option<u16> = opts
                            .drops
                            .as_ref()
                            .and_then(|ds| ds.with_inventory(opts.conn_id, |inv| inv.held()).flatten())
                            .and_then(|(item_id, _)| mc_world::block_for_item(item_id));
                        let Some(place_id) = place_id else {
                            let mut ack = Vec::new();
                            mc_protocol::write_varint(act.sequence, &mut ack);
                            if let Err(e) = write_packet_any(
                                stream,
                                packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID,
                                &ack,
                                threshold,
                            )
                            .await
                            {
                                debug!("play uuid={} place noop ack failed ({e}), closing", profile.uuid);
                                break;
                            }
                            continue;
                        };
                        if let Some(outcome) = decide_place(
                            mutable,
                            eye_of(last_pos),
                            view.center(),
                            view.radius(),
                            &act,
                            place_id,
                        ) {
                            if let Some(_world) = mutable {
                                let placed = outcome
                                    .changes
                                    .iter()
                                    .any(|c| c.new_id == place_id && c.old_id != c.new_id);
                                if let Err(e) =
                                    apply_block_outcome(stream, addr, profile, opts, cache, &outcome).await
                                {
                                    debug!("play uuid={} block outcome failed ({e}), closing", profile.uuid);
                                    break;
                                }
                                if placed {
                                    if let Some(ds) = &opts.drops {
                                        let consumed = ds.with_inventory(opts.conn_id, |inv| {
                                            inv.consume_held()
                                        });
                                        if consumed.flatten().is_some() {
                                            if let Err(e) = send_held_slot(stream, opts, ds).await {
                                                debug!("play uuid={} held sync failed ({e}), closing", profile.uuid);
                                                break;
                                            }
                                            ds.save_inventory(opts.conn_id, &conn_user);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => debug!("play uuid={} bad use_item_on: {e}", profile.uuid),
                }
            }
            Ok((id, payload)) if id == packets::PLAY_USE_ITEM_C2S_ID => {
                // 空手使用：无世界影响，只 ack 序列号让客户端预测对齐。
                match packets::UseItem::decode_body(&payload) {
                    Ok((act, _)) => {
                        let mut ack = Vec::new();
                        mc_protocol::write_varint(act.sequence, &mut ack);
                        if let Err(e) =
                            write_packet_any(stream, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, &ack, threshold).await
                        {
                            debug!("play uuid={} use_item ack failed ({e}), closing", profile.uuid);
                            break;
                        }
                    }
                    Err(e) => debug!("play uuid={} bad use_item: {e}", profile.uuid),
                }
            }
            Ok((id, payload)) if id == packets::PLAY_SET_HELD_ITEM_C2S_ID => {
                // P18 切手持：Short BE 0..=8 → 背包 selected；越界/截断只忽略不断线。
                match packets::decode_set_held_item_c2s(&payload) {
                    Ok((slot, _)) => {
                        if let Some(ds) = &opts.drops {
                            ds.with_inventory(opts.conn_id, |inv| inv.set_selected(slot));
                            debug!("play uuid={} held slot={slot}", profile.uuid);
                        }
                    }
                    Err(e) => debug!("play uuid={} bad set_held_item: {e}", profile.uuid),
                }
            }
            Ok((id, _)) if id == packets::PLAY_PUNCH_C2S_ID
                || id == packets::PLAY_SET_CREATIVE_SLOT_C2S_ID =>
            {
                // 挥拳/创造背包点选：P18 只忽略不断线（Slot 的 data component 需注册表驱动解码，
                // 误解析会错位整条 TCP 流），只忽略不断线已由单测覆盖。
                debug!("play uuid={} c2s id={id:#04x} ignored (no state change)", profile.uuid);
            }
            Ok((id, payload)) if id == packets::PLAY_CLICK_CONTAINER_C2S_ID => {
                // P18.1 集装箱点击：服务端权威应用（拿起/放下/交换/Shift 搬运/数字键交换），
                // 之后必发全量同步——客户端预测与服务端不一致时回滚，永远不分叉。
                // P19：THROW（背包打开时按 Q）从点击槽扔（button 0=1 个，1=整栈；
                // 光标非空时原版无操作，只全量同步回滚预测）。
                match packets::decode_container_click(&payload) {
                    Ok((click, _)) => {
                        let mut thrown = None;
                        if click.container_id != packets::PLAYER_INVENTORY_WINDOW_ID {
                            debug!(
                                "play uuid={} click non-player window {} ignored",
                                profile.uuid, click.container_id
                            );
                        } else if let Some(ds) = &opts.drops {
                            if click.click_type == packets::CLICK_THROW {
                                thrown = ds
                                    .with_inventory(opts.conn_id, |inv| {
                                        inv.throw_from_slot(click.slot, click.button)
                                    })
                                    .flatten();
                            } else {
                                ds.with_inventory(opts.conn_id, |inv| {
                                    inv.apply_click(click.slot, click.button, click.click_type)
                                });
                            }
                        }
                        if let Some((item_id, count)) = thrown {
                            if let (Some(ds), Some(world)) = (opts.drops.as_ref(), mutable) {
                                if let Err(e) = spawn_thrown_items(
                                    stream, opts, ds, world, last_pos, item_id, count,
                                )
                                .await
                                {
                                    debug!("play uuid={} throw spawn failed ({e}), closing", profile.uuid);
                                    break;
                                }
                            }
                        }
                        if let Some(ds) = &opts.drops {
                            if let Err(e) = send_full_inventory(stream, opts, ds).await {
                                debug!("play uuid={} click sync failed ({e}), closing", profile.uuid);
                                break;
                            }
                            ds.save_inventory(opts.conn_id, &conn_user);
                        }
                    }
                    Err(e) => debug!("play uuid={} bad container click: {e}", profile.uuid),
                }
            }
            Ok((id, payload)) => {
                debug!("play uuid={} c2s id={id:#04x} len={} (ignored)", profile.uuid, payload.len());
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                if last_seen.elapsed() > Duration::from_secs(15) {
                    warn!("play uuid={} keepalive timeout >15s without echo, kicking (disconnect event -> ECS)", profile.uuid);
                    // 退出循环后由 enter_play_with_options 统一发 Disconnect；
                    // ECS 的删除点会与 15s timeout 去重，online 只减一次。
                    break;
                }
                seq += 1;
                let mut body = Vec::new();
                packets::KeepAliveS2C(seq).encode_body(&mut body);
                if let Err(e) = write_packet_any(stream, packets::PLAY_KEEPALIVE_S2C_ID, &body, threshold).await {
                    debug!("play uuid={} keepalive resend failed ({e}), closing", profile.uuid);
                    break;
                }
                last_keepalive = Instant::now();
            }
            Err(e) => {
                debug!("play uuid={} closed ({e})", profile.uuid);
                break;
            }
        }
        // 节流窗口由每个收到的包也检查一次（读超时分支是玩家停住不动时的兜底节拍）。
        // 放在掉落 tick 之前，保证区块包先于同 tick 的实体包到达。
        if let Err(e) = maybe_flush_view(
            stream, profile, opts, &mut view, world, cache, mutable, &mut pending_view, false,
        )
        .await
        {
            debug!("play uuid={} view flush failed ({e}), closing", profile.uuid);
            break;
        }
        // 包流不断时也按 50ms 节拍跑连接侧同步（拾取/视距），防止读超时分支饿死。
        // 物理 tick 不在这里：全服唯一 ticker 独立运行，不随连接数翻倍。
        if last_drop_tick.elapsed() >= Duration::from_millis(50) {
            if let Some(ds) = &opts.drops {
                if let Err(e) = tick_drops(stream, opts, ds, &conn_user, last_pos, &view).await {
                    debug!("play uuid={} drop sync failed ({e}), closing", profile.uuid);
                    break;
                }
            }
            last_drop_tick = Instant::now();
        }
        // P21：同节拍排空广播队列（其它玩家的编辑/掉落），再回读循环。
        if let Err(e) = drain_outbox(stream, opts, &outbox).await {
            debug!("play uuid={} outbox drain failed ({e}), closing", profile.uuid);
            break;
        }
    }
    Ok(())
}
