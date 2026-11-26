//! login: PlayOptions + login handshake / config / join sequence (moved from login_route.rs, no logic change).
//! Shared small helpers (compression-aware IO, event emit, user_tag, chunk batches) reused by sibling modules.
use super::drop_sync::{send_experience, send_full_inventory};
use super::play_loop;
use crate::broadcast::{BroadcastHub, QueuedPacket};
use crate::chunk_cache::{encode_chunk_data_body, encode_chunk_data_body_mutable, ChunkCache};
use crate::chunk_view::{clamp_view_radius, view_chunk_count, ChunkView, PLAY_VIEW_RADIUS};
use crate::drop_state::DropState;
use crate::fluid_sync::FluidState;
use crate::framing::{read_packet, read_packet_framed, to_io_invalid, write_packet, write_packet_framed};
use crate::play_events::PlayEvent;
use crossbeam_channel::Sender;
use mc_protocol::packets;
use mc_world::{MutableWorld, WorldGen, DEFAULT_SEED};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use std::io;
use crate::NewConnection;
use std::time::Instant;

/// Play 期一次性参数：压缩阈值、seed、有效视距、连接身份、事件通道、区块缓存。
/// 旧入口（`handle_login` / `handle_login_configured` / `enter_play`）保留并默认
/// radius=[`PLAY_VIEW_RADIUS`]、无缓存，委托到带 options 的新实现。
#[derive(Debug, Clone)]
pub struct PlayOptions {
    /// 压缩阈值：-1 禁用；>=0 启用。
    pub threshold: i32,
    /// 世界 seed（WorldGen / LoginPlay seed）。
    pub seed: i64,
    /// 有效视距半径（构造时 clamp 到 2..=10）。
    pub view_radius: i32,
    /// accept 时分配的单连接身份，用于 ECS 实体键与 Disconnect 去重。
    pub conn_id: u64,
    /// Play 事件通道（移动/KeepAlive/Disconnect）；None 时不投递。
    pub events: Option<Sender<PlayEvent>>,
    /// 共享区块包体缓存；seed 不匹配时会被忽略并 warn。
    pub chunk_cache: Option<Arc<ChunkCache>>,
    /// 服务端权威可变世界（P17 挖掘/放置）；None 时方块交互包只忽略不断线。
    pub mutable_world: Option<Arc<MutableWorld>>,
    /// 掉落物实体 + 背包共享状态（P18）；None 时挖/放不掉落不同步（旧行为）。
    pub drops: Option<Arc<DropState>>,
    /// 多人广播 hub（P21）；None 时只回包给操作者本人（旧行为）。
    pub hub: Option<Arc<BroadcastHub>>,
    /// P46 流体状态（V2 生效）；方块编辑后由 hub 排队广播/缓存失效交给全服 ticker。
    pub fluid: Option<Arc<FluidState>>,
    /// P43 玩家实体互见开关：仅生产入口/显式 e2e 打开；默认关闭保持旧包流。
    pub player_visibility: bool,
}

impl PlayOptions {
    pub fn new(threshold: i32, seed: i64, view_radius: i32) -> Self {
        Self {
            threshold,
            seed,
            view_radius: clamp_view_radius(view_radius),
            conn_id: crate::next_conn_id(),
            events: None,
            chunk_cache: None,
            mutable_world: None,
            drops: None,
            hub: None,
            fluid: None,
            player_visibility: false,
        }
    }

    pub fn with_conn_id(mut self, conn_id: u64) -> Self {
        self.conn_id = conn_id;
        self
    }

    pub fn with_events(mut self, tx: Sender<PlayEvent>) -> Self {
        self.events = Some(tx);
        self
    }

    pub fn with_optional_events(mut self, events: Option<Sender<PlayEvent>>) -> Self {
        self.events = events;
        self
    }

    pub fn with_chunk_cache(mut self, cache: Arc<ChunkCache>) -> Self {
        self.chunk_cache = Some(cache);
        self
    }

    pub fn with_mutable_world(mut self, world: Arc<MutableWorld>) -> Self {
        self.mutable_world = Some(world);
        self
    }

    pub fn with_drops(mut self, drops: Arc<DropState>) -> Self {
        self.drops = Some(drops);
        self
    }

    pub fn with_hub(mut self, hub: Arc<BroadcastHub>) -> Self {
        self.hub = Some(hub);
        self
    }

    /// P46：注入流体状态（方块编辑时排队传播任务）。
    pub fn with_fluid(mut self, fluid: Arc<FluidState>) -> Self {
        self.fluid = Some(fluid);
        self
    }

    /// P43：打开玩家实体互见（需同时带 `hub` 才真正发包）。
    pub fn with_player_visibility(mut self, enabled: bool) -> Self {
        self.player_visibility = enabled;
        self
    }
}

/// 默认压缩禁用（阈值 -1，原行为）。
pub async fn handle_login(stream: TcpStream, addr: SocketAddr, tx: &Sender<NewConnection>) -> io::Result<()> {
    handle_login_configured(
        stream,
        addr,
        tx,
        packets::COMPRESSION_DISABLED_THRESHOLD,
        DEFAULT_SEED,
        None,
    )
    .await
}

/// 阈值可配：threshold>=0 时 Success 前先发 SetCompression，其后走压缩帧。
pub async fn handle_login_with_threshold(stream: TcpStream, addr: SocketAddr, tx: &Sender<NewConnection>, threshold: i32) -> io::Result<()> {
    handle_login_configured(stream, addr, tx, threshold, DEFAULT_SEED, None).await
}

/// 旧登录入口（无缓存/默认 radius=4）：保留给旧测试与调用点，委托到 options 实现。
pub async fn handle_login_configured(
    stream: TcpStream,
    addr: SocketAddr,
    tx: &Sender<NewConnection>,
    threshold: i32,
    seed: i64,
    events: Option<Sender<PlayEvent>>,
) -> io::Result<()> {
    let opts = PlayOptions::new(threshold, seed, PLAY_VIEW_RADIUS).with_optional_events(events);
    handle_login_with_options(stream, addr, tx, opts).await
}

/// 完整登录入口（Serve 从 NetConfig 透传 PlayOptions：压缩阈值 + seed + 有效视距 +
/// conn_id + Play 事件通道 + 共享区块缓存）。
pub async fn handle_login_with_options(
    mut stream: TcpStream,
    addr: SocketAddr,
    tx: &Sender<NewConnection>,
    opts: PlayOptions,
) -> io::Result<()> {
    let (id, payload) = read_packet(&mut stream).await?;
    if id != packets::LOGIN_START_PACKET_ID {
        warn!("{addr} login: expected start 0x00, got {id:#04x}, disconnect");
        return Ok(());
    }
    let (start, _) = packets::LoginStart::decode_body(&payload).map_err(to_io_invalid)?;
    if start.name.is_empty() || start.name.chars().count() > 16 {
        warn!("{addr} login: bad username {:?}, disconnect", start.name);
        return Ok(());
    }
    let threshold = opts.threshold;
    let compressed = packets::compression_enabled(threshold);
    if compressed {
        debug!("{addr} login: enable compression threshold={threshold}");
        let mut sc = Vec::new();
        packets::encode_set_compression_packet(threshold, &mut sc);
        use tokio::io::AsyncWriteExt;
        stream.write_all(&sc).await?;
    }
    let uuid = packets::offline_uuid(&start.name);
    let profile = mc_core::PlayerProfile { uuid, username: start.name.clone() };
    let mut body = Vec::new();
    packets::LoginSuccess::offline(uuid, profile.username.clone()).encode_body(&mut body);
    if compressed {
        write_packet_framed(&mut stream, packets::LOGIN_SUCCESS_PACKET_ID, &body, threshold).await?;
    } else {
        write_packet(&mut stream, packets::LOGIN_SUCCESS_PACKET_ID, &body).await?;
    }
    let ack = if compressed { read_packet_framed(&mut stream, threshold).await } else { read_packet(&mut stream).await };
    match ack {
        Ok((packets::LOGIN_ACKNOWLEDGED_PACKET_ID, p)) => {
            packets::decode_login_acknowledged(&p).map_err(to_io_invalid)?;
        }
        Ok((other, _)) => {
            warn!("{addr} login: expected ack 0x03, got {other:#04x}, disconnect");
            return Ok(());
        }
        Err(e) => {
            debug!("{addr} login: no ack ({e}), disconnect");
            return Ok(());
        }
    }
    info!("login ok uuid={} name={} conn_id={}", uuid, profile.username, opts.conn_id);
    if let Err(e) = config_finish(&mut stream, threshold).await {
        warn!("{addr} config: finish failed ({e}), disconnect");
        return Ok(());
    }
    notify_login_success(tx, addr, opts.conn_id, &profile);
    // `enter_play_with_options` 在所有退出路径（含 Join 序列失败）恰好投递一次
    // Disconnect；因此这里不能在 Err 分支再发一次，否则 ECS 会双减 online。
    enter_play_with_options(stream, addr, profile, opts).await
}

/// Configuration阶段最小可用：按序 KnownPacks S2C→KnownPacks C2S→Registry Data(4包)→Update Tags→Finish S2C→等 Finish Ack C2S。
/// 包 ID 已按P4A对照表核对777（CB 0x0F/0x07/0x0E/0x03，SB 0x07/0x03）；字段见 packets::config（来源 minecraft.wiki 777）。
/// Update Tags（CB 0x0E，见 config.rs 勘误注释，旧文档 0x0D 是 26.1 值）在 Finish 前发送：
/// tag 永不走 known packs，timeline 4 tag 缺失则 dimension 引用的 `#minecraft:in_overworld` 悬空，真机拒收 Finish。
/// 容忍度：真机在 Configuration 会穿插 ClientInformation(0x00)/PluginMessage/Pong/KeepAlive 等，
/// 这些与进服无关，先 debug 忽略，只认 KnownPacks 回包与 Finish Ack；规定步数内收不齐才 Err。
/// threshold 透传（压缩开启后 Configuration 包同样走压缩帧）。
pub async fn config_finish(stream: &mut TcpStream, threshold: i32) -> io::Result<()> {
    use tokio::time::{timeout, Duration};
    let compressed = packets::compression_enabled(threshold);
    // 1. Known Packs S2C（单vanilla包）。
    let known = packets::ConfigKnownPacks::vanilla();
    let mut body = Vec::new();
    known.encode_body(&mut body);
    debug!("config known packs send id={:#04x} n={}", packets::CONFIG_KNOWN_PACKS_S2C_ID, known.packs.len());
    if compressed {
        write_packet_framed(stream, packets::CONFIG_KNOWN_PACKS_S2C_ID, &body, threshold).await?;
    } else {
        write_packet(stream, packets::CONFIG_KNOWN_PACKS_S2C_ID, &body).await?;
    }
    // 2. 等 Known Packs C2S：中间穿插包全部忽略，最多等 32 个包/整体 15s。
    let mut got_known = false;
    for _ in 0..32 {
        let next = timeout(Duration::from_secs(15), async {
            if compressed { read_packet_framed(stream, threshold).await } else { read_packet(stream).await }
        })
        .await
        .map_err(|_| to_io_invalid("config known packs wait timed out"))??;
        let (id, payload) = next;
        if id == packets::CONFIG_KNOWN_PACKS_C2S_ID {
            let reply = packets::decode_config_known_packs_c2s(&payload).map_err(to_io_invalid)?.0;
            debug!("config known packs ack n={}", reply.packs.len());
            got_known = true;
            break;
        }
        debug!("config: ignore interleaved id={id:#04x} len={} while waiting known packs", payload.len());
    }
    if !got_known {
        return Err(to_io_invalid("config known packs response missing"));
    }
    // 3. Registry Data：原版 26.3 全量 32 个同步注册表包体（空 KnownPacks 抓包，条目ID与 Update Tags 自洽）。
    for (name, body) in packets::vanilla_config_777::VANILLA_CONFIG_REGISTRY_BODIES_777 {
        debug!("config registry send {} bytes={}", name, body.len());
        if compressed {
            write_packet_framed(stream, packets::CONFIG_REGISTRY_DATA_S2C_ID, body, threshold).await?;
        } else {
            write_packet(stream, packets::CONFIG_REGISTRY_DATA_S2C_ID, body).await?;
        }
    }
    // 4. Update Tags：同一次抓包的全量 15 个注册表包体（19662 项），
    //    含 block 的 300 个 tag（`#minecraft:infiniburn_overworld` 等）。
    {
        let body = packets::vanilla_config_777::VANILLA_CONFIG_UPDATE_TAGS_BODY_777;
        debug!("config update tags send bytes={}", body.len());
        if compressed {
            write_packet_framed(stream, packets::CONFIG_UPDATE_TAGS_S2C_ID, body, threshold).await?;
        } else {
            write_packet(stream, packets::CONFIG_UPDATE_TAGS_S2C_ID, body).await?;
        }
    }
    // 5. Finish S2C → 等 Finish Ack C2S，中间穿插包同样忽略。
    debug!("config finish send id={:#04x}", packets::CONFIG_FINISH_S2C_ID);
    if compressed {
        write_packet_framed(stream, packets::CONFIG_FINISH_S2C_ID, &[], threshold).await?;
    } else {
        write_packet(stream, packets::CONFIG_FINISH_S2C_ID, &[]).await?;
    }
    for _ in 0..32 {
        let next = timeout(Duration::from_secs(15), async {
            if compressed { read_packet_framed(stream, threshold).await } else { read_packet(stream).await }
        })
        .await
        .map_err(|_| to_io_invalid("config finish ack wait timed out"))??;
        let (id, payload) = next;
        if id == packets::CONFIG_FINISH_ACK_C2S_ID {
            packets::decode_config_finish_ack(&payload).map_err(to_io_invalid)?;
            debug!("config finish ack ok");
            return Ok(());
        }
        debug!("config: ignore interleaved id={id:#04x} len={} while waiting finish ack", payload.len());
    }
    Err(to_io_invalid("config finish ack missing"))
}

/// Login 成功后的唯一真实连接上报点（在线数只认这里）。
pub fn notify_login_success(
    tx: &Sender<NewConnection>,
    addr: SocketAddr,
    conn_id: u64,
    profile: &mc_core::PlayerProfile,
) {
    if let Err(e) = tx.try_send(NewConnection { addr, conn_id }) {
        debug!("login success notify dropped for {addr} conn_id={conn_id} ({e}): {profile:?}");
    }
}

/// 进入 Play：拿住 socket，按序真下发后进循环。threshold透传，压缩开启后Play包走压缩帧。
/// 旧签名保留（无 ECS 事件通道/默认 seed/视距 radius=4）；Serve 走 [`enter_play_with_options`]。
pub async fn enter_play(stream: TcpStream, addr: SocketAddr, profile: mc_core::PlayerProfile, threshold: i32) -> io::Result<()> {
    enter_play_with_options(stream, addr, profile, PlayOptions::new(threshold, DEFAULT_SEED, PLAY_VIEW_RADIUS)).await
}

/// 旧入口（带 seed + Play 事件通道，默认 radius=4、无缓存）。
pub async fn enter_play_with_events(
    stream: TcpStream,
    addr: SocketAddr,
    profile: mc_core::PlayerProfile,
    threshold: i32,
    seed: i64,
    events: Option<Sender<PlayEvent>>,
) -> io::Result<()> {
    enter_play_with_options(
        stream,
        addr,
        profile,
        PlayOptions::new(threshold, seed, PLAY_VIEW_RADIUS).with_optional_events(events),
    )
    .await
}

/// 进入 Play（完整 options）：Join 序列 + Play 循环；退出时恰好投递一次 Disconnect。
pub async fn enter_play_with_options(
    mut stream: TcpStream,
    addr: SocketAddr,
    profile: mc_core::PlayerProfile,
    opts: PlayOptions,
) -> io::Result<()> {
    let conn_id = opts.conn_id;
    let events = opts.events.clone();
    let result = enter_play_inner(&mut stream, addr, &profile, &opts).await;
    // P18 背包刷盘：断线时落盘 + 释放连接状态（seed 隔离，缺文件下次按新背包开局）。
    if let Some(drops) = &opts.drops {
        drops.unload_inventory(conn_id, &user_tag(&profile));
    }
    // P21 广播通道与 Disconnect 同点清理：覆盖所有退出路径，不留幽灵通道。
    if let Some(hub) = &opts.hub {
        // P43：先把本玩家从视者视野移除（RemoveEntities + unmark），再注销通道；
        // 顺序反了会拿不到对外实体 id、留下幽灵玩家实体。
        if opts.player_visibility {
            crate::player_visibility::broadcast_leave(hub, conn_id);
        }
        hub.unregister(conn_id);
    }
    // 正常 EOF / 读错误 / 写失败 / KeepAlive 超时 / Join 序列失败全部走这里，
    // 只发一次 Disconnect。ECS 按 conn_id 立即 despawn，与 15s timeout 去重。
    emit_play_event(&events, PlayEvent::Disconnect { addr, conn_id });
    result
}

async fn enter_play_inner(
    stream: &mut TcpStream,
    addr: SocketAddr,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
) -> io::Result<()> {
    let world = WorldGen::new(opts.seed);
    let radius = clamp_view_radius(opts.view_radius);
    let cache = match opts.chunk_cache.as_deref() {
        Some(c) if c.seed() == opts.seed => Some(c),
        Some(c) => {
            warn!(
                "chunk cache seed {} != server seed {}, falling back to direct WorldGen",
                c.seed(), opts.seed
            );
            None
        }
        None => None,
    };
    let spawn = world.spawn_pos();
    info!(
        "enter_play uuid={} name={} conn_id={} threshold={} seed={} view_radius={} view_chunks={} cache={} spawn=({},{},{})",
        profile.uuid,
        profile.username,
        opts.conn_id,
        opts.threshold,
        opts.seed,
        radius,
        view_chunk_count(radius),
        cache.map(|c| c.capacity()).unwrap_or(0),
        spawn.0,
        spawn.1,
        spawn.2
    );
    // 出生点与中心区块由 WorldGen 按 seed 计算（x/z 固定 369/381 → chunk 23,23）。
    let center = world.spawn_chunk();
    let mut view = ChunkView::new(center, radius);
    let mutable = opts.mutable_world.as_deref();
    let keepalive_id =
        send_join_sequence(stream, profile, opts, &mut view, &world, cache, mutable).await?;
    info!(
        "play join sent uuid={} name={} conn_id={} keepalive={} view_sent={} view_radius={} seed={} spawn=({},{},{})",
        profile.uuid,
        profile.username,
        opts.conn_id,
        keepalive_id,
        view.sent_count(),
        radius,
        opts.seed,
        spawn.0,
        spawn.1,
        spawn.2
    );
    // P18 背包 Join 同步（读盘/开局 + 全量 Content），失败不阻断进服。
    send_join_inventory(stream, profile, opts).await;
    // P21 广播队列：Join 序列完成后注册，保证接收方已持有完整视距区块，
    // 广播包不会排在区块包前面；初始位置取出生点，移动包随后更新。
    // P25：带上用户名/地址，管理面板 `/api/online` 直接读快照。
    let outbox = opts.hub.as_ref().map(|hub| {
        let (tx, rx) = crossbeam_channel::unbounded::<QueuedPacket>();
        hub.register_with_name(
            opts.conn_id,
            tx,
            radius,
            (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5),
            profile.username.clone(),
            addr.to_string(),
        );
        if opts.player_visibility {
            // P43：对外实体 id 与客户端自身 id=1 严格分离；自己永远不 Spawn。
            let state = crate::PlayerEntityState {
                entity_id: crate::player_visibility::next_player_entity_id(),
                uuid: profile.uuid.as_u128(),
                pos: (spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5),
                yaw: 0.0,
                pitch: 0.0,
            };
            hub.enable_player(opts.conn_id, state);
            // 刚进服：给视距内旧连接发自己的 Spawn，并让自己看到视距内旧玩家；
            // per-viewer known 集保证双方都只发一次（cycle 兜底见 play_loop 节拍）。
            crate::player_visibility::spawn_to_viewers(hub, opts.conn_id, &state);
            crate::player_visibility::sync_viewer(hub, opts.conn_id);
        }
        rx
    });
    play_loop(stream, addr, profile, opts, keepalive_id, view, &world, cache, mutable, outbox).await
}

/// P18 Join 背包同步：读盘（缺文件=新背包+石头开局）→ 全量 Content。
/// 失败只记 debug，不阻断 Join（背包下次拾取/重进再同步）。
async fn send_join_inventory(
    stream: &mut TcpStream,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
) {
    let Some(drops) = &opts.drops else { return };
    drops.load_inventory(opts.conn_id, &user_tag(profile));
    if let Err(e) = send_full_inventory(stream, opts, drops).await {
        debug!("play uuid={} join inventory send failed ({e})", profile.uuid);
    }
    // P31 Join 经验同步：XP>0 才发（新玩家 XP=0 与客户端默认一致，保持既有包序不变）。
    let has_xp = drops.with_inventory(opts.conn_id, |inv| inv.total_xp() > 0).unwrap_or(false);
    if has_xp {
        if let Err(e) = send_experience(stream, opts, drops).await {
            debug!("play uuid={} join experience send failed ({e})", profile.uuid);
        }
    }
    // P28.3 Join 选中同步：读盘恢复的 selected 经 S2C 0x6B 发给客户端。
    // 客户端重进默认选中 0 格，不发会造成"你看着选了 A，服务端按 B 出手"
    //（Q 扔错格/放错材料），且之后不再有纠正机会。
    // 只在非零时发送：新背包 selected=0 与客户端默认一致，无需多一个包
    //（所有既有 e2e 都是新背包，零包序影响）；切过槽又重进的才需要纠正。
    let selected = drops.with_inventory(opts.conn_id, |inv| inv.selected());
    if let Some(sel) = selected.filter(|s| *s != 0) {
        let body = packets::encode_set_held_slot_s2c(sel);
        if let Err(e) =
            write_packet_any(stream, packets::PLAY_SET_HELD_SLOT_S2C_ID, &body, opts.threshold)
                .await
        {
            debug!("play uuid={} join held sync failed ({e})", profile.uuid);
        }
    }
    // P40 Join 生存同步：新玩家默认满血/满饥饿/饱和=5 与客户端默认完全一致，
    // 发一个相同的 SetHealth 只会改变既有包序（403 个既有 e2e 一字不改必须全绿），
    // 故只在读盘得到的生存状态非满值时发送；非满值的重进/进食/受伤必发。
    let needs_health = drops
        .survival_state(opts.conn_id)
        .map(|s| s != mc_world::SurvivalState::full())
        .unwrap_or(false);
    if needs_health {
        if let Err(e) = super::survival_sync::send_set_health(stream, opts, drops, opts.conn_id).await {
            debug!("play uuid={} join health sync failed ({e})", profile.uuid);
        }
    }
}

/// 压缩感知写包：threshold>=0 走压缩帧，否则普通帧。
pub(crate) async fn write_packet_any(stream: &mut TcpStream, packet_id: i32, payload: &[u8], threshold: i32) -> io::Result<()> {
    if packets::compression_enabled(threshold) {
        write_packet_framed(stream, packet_id, payload, threshold).await
    } else {
        write_packet(stream, packet_id, payload).await
    }
}

/// 单块 ChunkData 包体：优先共享缓存（已编码 Vec<u8>），无缓存时直接生成+编码。
/// 缓存未绑定 MutableWorld 但 PlayOptions 带了可变世界时，走 override 路径单块生成。
fn play_chunk_body(
    world: &WorldGen,
    pos: mc_core::ChunkPos,
    cache: Option<&ChunkCache>,
    mutable: Option<&MutableWorld>,
) -> Arc<Vec<u8>> {
    match cache {
        Some(cache) => cache.body(pos),
        None => match mutable {
            Some(m) => Arc::new(encode_chunk_data_body_mutable(m, pos)),
            None => Arc::new(encode_chunk_data_body(world, pos)),
        },
    }
}

/// 发一批 ChunkData：ChunkBatchStart → N×ChunkData → ChunkBatchFinished(batch_size=N)。
/// 空批次不发（客户端 ChunkBatchSizeCalculator 只接受正 batch_size）。
/// 命中共享缓存时用 `ChunkCache::bodies` 并行生成缺失区块（初始 441 块的主要开销），
/// 无缓存旧入口仍逐个生成。
pub(crate) async fn send_chunk_batch(
    stream: &mut TcpStream,
    world: &WorldGen,
    add: &[mc_core::ChunkPos],
    threshold: i32,
    cache: Option<&ChunkCache>,
    mutable: Option<&MutableWorld>,
) -> io::Result<()> {
    if add.is_empty() {
        return Ok(());
    }
    let bodies: Vec<Arc<Vec<u8>>> = match cache {
        Some(cache) => cache.bodies(add),
        None => add.iter().map(|&pos| play_chunk_body(world, pos, None, mutable)).collect(),
    };
    write_packet_any(stream, packets::PLAY_CHUNK_BATCH_START_ID, &[], threshold).await?;
    for body in &bodies {
        write_packet_any(stream, packets::PLAY_CHUNK_DATA_ID, body.as_slice(), threshold).await?;
    }
    let mut body = Vec::new();
    mc_protocol::write_varint(add.len() as i32, &mut body);
    write_packet_any(stream, packets::PLAY_CHUNK_BATCH_FINISHED_ID, &body, threshold).await
}

/// 逐个发 UnloadChunk（0x26 forget_level_chunk）。
pub(crate) async fn send_unload_chunks(stream: &mut TcpStream, remove: &[mc_core::ChunkPos], threshold: i32) -> io::Result<()> {
    for pos in remove {
        let mut body = Vec::new();
        packets::UnloadChunk { chunk_x: pos.0, chunk_z: pos.1 }.encode_body(&mut body);
        write_packet_any(stream, packets::PLAY_UNLOAD_CHUNK_ID, &body, threshold).await?;
    }
    Ok(())
}

async fn send_join_sequence(
    stream: &mut TcpStream,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
    view: &mut ChunkView,
    world: &WorldGen,
    cache: Option<&ChunkCache>,
    mutable: Option<&MutableWorld>,
) -> io::Result<i64> {
    let threshold = opts.threshold;
    let radius = view.radius();
    // LoginPlay 的 view_distance/simulation_distance 必须等于实际有效半径（不再是固定 10）。
    let mut body = Vec::new();
    packets::LoginPlay {
        view_distance: radius,
        simulation_distance: radius,
        seed: world.seed,
        ..Default::default()
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_LOGIN_PLAY_ID, &body, threshold).await?;
    debug!("play {} login play sent view_distance={radius}", profile.username);
    let spawn = world.spawn_pos();
    let center = world.spawn_chunk();
    body.clear();
    packets::SetCenterChunk { chunk_x: center.0, chunk_z: center.1 }.encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_CENTER_CHUNK_ID, &body, threshold).await?;
    // 26.3 客户端要 Game Event LEVEL_CHUNKS_LOAD_START(type=13) 才离开 WaitingForServer。
    body.clear();
    packets::encode_game_event_level_chunks_load_start(&mut body);
    write_packet_any(stream, packets::PLAY_GAME_EVENT_ID, &body, threshold).await?;
    // 26.3 必须把区块包夹在 chunk_batch_start(0x0C) / chunk_batch_finished(0x0B) 之间，
    // 否则客户端一直停在“加载地形中…”。关键：SectionUpdateTracker.hasAllNeighbors 要求
    // 中心 section 的 4 正+4 斜邻居区块都存在，只发 1 块时 player section 永远不会编译，
    // 30s 后客户端超时放行。因此初始一批发送全部 `(2r+1)^2` 块（r=4 → 81，r=10 → 441），
    // 与 mc-server 视距队列 / ChunkView 一致。
    let update = view.plan(center);
    debug_assert_eq!(update.add.len(), view_chunk_count(radius));
    let started = Instant::now();
    send_chunk_batch(stream, world, &update.add, threshold, cache, mutable).await?;
    info!(
        "play join chunks sent uuid={} name={} conn_id={} count={} view_radius={} elapsed_ms={}",
        profile.uuid,
        profile.username,
        opts.conn_id,
        update.add.len(),
        radius,
        started.elapsed().as_millis()
    );
    body.clear();
    packets::PlayerPositionSync {
        teleport_id: 1,
        x: spawn.0 as f64 + 0.5, y: spawn.1 as f64, z: spawn.2 as f64 + 0.5,
        delta_x: 0.0, delta_y: 0.0, delta_z: 0.0,
        yaw: 0.0, pitch: 0.0, relatives: 0,
    }.encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_PLAYER_POSITION_SYNC_ID, &body, threshold).await?;
    let keepalive_id = 1i64;
    body.clear();
    packets::KeepAliveS2C(keepalive_id).encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_KEEPALIVE_S2C_ID, &body, threshold).await?;
    Ok(keepalive_id)
}

/// 压缩感知读包：threshold>=0 按压缩帧解析，否则普通帧。
pub(crate) async fn read_packet_any(stream: &mut TcpStream, threshold: i32) -> io::Result<(i32, Vec<u8>)> {
    if packets::compression_enabled(threshold) {
        read_packet_framed(stream, threshold).await
    } else {
        read_packet(stream).await
    }
}

/// 向 ECS 投递 Play 事件：不阻塞 net 线程；通道满/关闭只 debug（主循环用无界通道）。
pub(crate) fn emit_play_event(events: &Option<Sender<PlayEvent>>, ev: PlayEvent) {
    if let Some(tx) = events {
        if let Err(e) = tx.try_send(ev) {
            debug!("play event dropped ({e})");
        }
    }
}

pub(crate) fn user_tag(profile: &mc_core::PlayerProfile) -> String {
    profile.username.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn config_finish_sends_known_registry_tags_finish_in_order() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            config_finish(&mut s, packets::COMPRESSION_DISABLED_THRESHOLD).await.unwrap();
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 1. KnownPacks S2C（CB 0x0F，单vanilla包）。
        let (id, payload) = crate::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::CONFIG_KNOWN_PACKS_S2C_ID);
        let packs = packets::decode_config_known_packs_s2c(&payload).unwrap().0;
        assert_eq!(packs.packs.len(), 1);
        assert_eq!(packs.packs[0].namespace, "minecraft");
        // 2. 回 KnownPacks C2S（SB 0x07）。
        let mut kp = Vec::new();
        packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kp);
        use tokio::io::AsyncWriteExt;
        c.write_all(&kp).await.unwrap();
        // 3. Registry Data 32包（CB 0x07：原版全量同步注册表，包体与抓包逐字节一致）。
        for (_name, want) in packets::vanilla_config_777::VANILLA_CONFIG_REGISTRY_BODIES_777 {
            let (id, payload) = crate::framing::read_packet(&mut c).await.unwrap();
            assert_eq!(id, packets::CONFIG_REGISTRY_DATA_S2C_ID);
            assert_eq!(&payload, want);
        }
        // 4. Update Tags 1包（CB 0x0E：原版全量 15 个注册表）。
        let (id, payload) = crate::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::CONFIG_UPDATE_TAGS_S2C_ID);
        assert_eq!(payload, packets::vanilla_config_777::VANILLA_CONFIG_UPDATE_TAGS_BODY_777);
        // 5. Finish S2C → 回 Finish Ack。
        let (id, payload) = crate::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::CONFIG_FINISH_S2C_ID);
        packets::decode_config_finish_s2c(&payload).unwrap();
        let mut ack = Vec::new();
        packets::encode_config_finish_ack_packet(&mut ack);
        c.write_all(&ack).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn config_finish_rejects_wrong_known_packs_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            assert!(config_finish(&mut s, packets::COMPRESSION_DISABLED_THRESHOLD).await.is_err());
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let _ = crate::framing::read_packet(&mut c).await.unwrap();
        crate::framing::write_packet(&mut c, 0x7F, &[]).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn config_finish_rejects_wrong_ack_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            assert!(config_finish(&mut s, packets::COMPRESSION_DISABLED_THRESHOLD).await.is_err());
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 先走完 KnownPacks 往返 + 读掉 4 个 Registry + 1 个 Update Tags，再在 Finish 处错包。
        let _ = crate::framing::read_packet(&mut c).await.unwrap();
        let mut kp = Vec::new();
        packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kp);
        use tokio::io::AsyncWriteExt;
        c.write_all(&kp).await.unwrap();
        for _ in 0..5 {
            let _ = crate::framing::read_packet(&mut c).await.unwrap();
        }
        crate::framing::write_packet(&mut c, 0x7F, &[]).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn enter_play_sends_join_then_reads_keepalive_echo() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let profile = mc_core::PlayerProfile { uuid: packets::offline_uuid("Steve"), username: "Steve".into() };
        let server = tokio::spawn(async move {
            let (s, peer) = listener.accept().await.unwrap();
            enter_play(s, peer, profile, packets::COMPRESSION_DISABLED_THRESHOLD).await.unwrap();
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 读完整 Join 序列：4 包头 + 81 块 ChunkData + BatchFinished/Pos/KeepAlive。
        for _ in 0..88 {
            let _ = crate::framing::read_packet(&mut c).await.unwrap();
        }
        // 回一个 keepalive echo，服务端只log不断开；随后客户端断开，服务端正常退出
        let mut b = Vec::new();
        packets::KeepAliveC2S(1).encode_body(&mut b);
        crate::framing::write_packet(&mut c, packets::PLAY_KEEPALIVE_C2S_ID, &b).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        drop(c);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn enter_play_accepts_movement_packets_without_disconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let profile = mc_core::PlayerProfile { uuid: packets::offline_uuid("Alex"), username: "Alex".into() };
        let server = tokio::spawn(async move {
            let (s, peer) = listener.accept().await.unwrap();
            enter_play(s, peer, profile, packets::COMPRESSION_DISABLED_THRESHOLD).await.unwrap();
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        // 读完整 Join 序列（4 + 81 + 3 包），否则服务端写满 socket 缓冲会阻塞。
        for _ in 0..88 {
            let _ = crate::framing::read_packet(&mut c).await.unwrap();
        }
        // 依次发 0x1E/0x1F/0x20/0x21，服务端转发 ECS（本测试无通道，只log）且不断开；
        // 坐标保持在出生区块，避免触发跨区块动态补发。
        let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
        let x = spawn.0 as f64 + 0.5;
        let z = spawn.2 as f64 + 0.5;
        let mut body = Vec::new();
        packets::MovePlayerPos { x, y: 80.0, z, on_ground: true, horizontal_collision: false }
            .encode_body(&mut body);
        crate::framing::write_packet(&mut c, packets::PLAY_MOVE_POS_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerPosRot { x, y: 80.0, z, yaw: 90.0, pitch: 0.0, on_ground: true, horizontal_collision: false }
            .encode_body(&mut body);
        crate::framing::write_packet(&mut c, packets::PLAY_MOVE_POSROT_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerRot { yaw: 180.0, pitch: 0.0, on_ground: false, horizontal_collision: false }
            .encode_body(&mut body);
        crate::framing::write_packet(&mut c, packets::PLAY_MOVE_ROT_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerFlags { on_ground: true, horizontal_collision: false }.encode_body(&mut body);
        crate::framing::write_packet(&mut c, packets::PLAY_MOVE_FLAGS_C2S_ID, &body).await.unwrap();
        // 300ms内不应EOF（连接仍开着）
        match tokio::time::timeout(std::time::Duration::from_millis(300), crate::framing::read_packet(&mut c)).await {
            Err(_) => {} // 超时=服务端没发新包但连接仍开着，符合预期
            Ok(Ok(_)) => {} // 若收到keepalive重发也算连接正常
            Ok(Err(e)) => panic!("server closed play after movement packets: {e:?}"),
        }
        drop(c);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn join_sequence_uses_options_view_radius_and_disconnect_event() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = crossbeam_channel::unbounded::<PlayEvent>();
        let profile = mc_core::PlayerProfile { uuid: packets::offline_uuid("Radius"), username: "Radius".into() };
        let server = tokio::spawn(async move {
            let (s, peer) = listener.accept().await.unwrap();
            let opts = PlayOptions::new(packets::COMPRESSION_DISABLED_THRESHOLD, DEFAULT_SEED, 2)
                .with_conn_id(77)
                .with_events(tx);
            enter_play_with_options(s, peer, profile, opts).await.unwrap();
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        // LoginPlay + SetCenter + GameEvent + BatchStart + 25 ChunkData + Finished/Pos/KeepAlive.
        let (id, payload) = crate::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::PLAY_LOGIN_PLAY_ID);
        let (login, _) = packets::LoginPlay::decode_body(&payload).unwrap();
        assert_eq!(login.view_distance, 2, "LoginPlay view_distance must match effective radius");
        assert_eq!(login.simulation_distance, 2);
        let mut chunks = 0;
        let mut finished = None;
        for _ in 0..(3 + 25 + 3) {
            let (id, payload) = crate::framing::read_packet(&mut c).await.unwrap();
            match id {
                packets::PLAY_CHUNK_DATA_ID => chunks += 1,
                packets::PLAY_CHUNK_BATCH_FINISHED_ID => finished = Some(payload),
                _ => {}
            }
        }
        assert_eq!(chunks, 25);
        let (batch_size, _) = mc_protocol::read_varint(finished.as_ref().unwrap()).unwrap();
        assert_eq!(batch_size, 25);
        let peer = c.local_addr().unwrap();
        drop(c);
        // 连接结束后恰好一个 Disconnect（带 conn_id；addr 是客户端 peer 地址）。
        let ev = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(ev) = rx.try_recv() {
                    return ev;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("disconnect event");
        match ev {
            PlayEvent::Disconnect { addr: ev_addr, conn_id } => {
                assert_eq!(conn_id, 77);
                assert_eq!(ev_addr, peer, "Disconnect must carry the accepted peer addr");
            }
            other => panic!("expected Disconnect, got {other:?}"),
        }
        server.await.unwrap();
    }
}
