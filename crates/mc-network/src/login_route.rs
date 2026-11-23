//! login_route: Login离线流程 Start→[SetCompression]→Success→Ack→Config(Finish→Ack)→enter_play(真下发)
//! Play 期：KeepAlive + C2S 移动包；移动包经 crossbeam channel 投递 ECS（PlayEvent），
//! 同时 net 线程自身按玩家跨区块动态补发有效视距区块并卸载越界区块。
//! 视距来自 `ServerConfig.view_distance`（`clamp_view_radius` 限制 2..=10），
//! 区块包体优先走共享 `ChunkCache`；连接结束在所有退出路径恰好投递一次 Disconnect。
use super::chunk_cache::{encode_chunk_data_body, ChunkCache};
use super::chunk_view::{clamp_view_radius, view_chunk_count, ChunkView, PLAY_VIEW_RADIUS};
use super::framing::{read_packet, read_packet_framed, to_io_invalid, write_packet, write_packet_framed};
use super::play_events::PlayEvent;
use crossbeam_channel::Sender;
use mc_protocol::packets;
use mc_world::{WorldGen, DEFAULT_SEED};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use std::io;

use super::NewConnection;

use std::time::{Duration, Instant};

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
    let keepalive_id = send_join_sequence(stream, profile, opts, &mut view, &world, cache).await?;
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
    play_loop(stream, addr, profile, opts, keepalive_id, view, &world, cache).await
}

/// 压缩感知写包：threshold>=0 走压缩帧，否则普通帧。
async fn write_packet_any(stream: &mut TcpStream, packet_id: i32, payload: &[u8], threshold: i32) -> io::Result<()> {
    if packets::compression_enabled(threshold) {
        write_packet_framed(stream, packet_id, payload, threshold).await
    } else {
        write_packet(stream, packet_id, payload).await
    }
}

/// 单块 ChunkData 包体：优先共享缓存（已编码 Vec<u8>），无缓存时直接生成+编码。
fn play_chunk_body(
    world: &WorldGen,
    pos: mc_core::ChunkPos,
    cache: Option<&ChunkCache>,
) -> Arc<Vec<u8>> {
    match cache {
        Some(cache) => cache.body(pos),
        None => Arc::new(encode_chunk_data_body(world, pos)),
    }
}

/// 发一批 ChunkData：ChunkBatchStart → N×ChunkData → ChunkBatchFinished(batch_size=N)。
/// 空批次不发（客户端 ChunkBatchSizeCalculator 只接受正 batch_size）。
async fn send_chunk_batch(
    stream: &mut TcpStream,
    world: &WorldGen,
    add: &[mc_core::ChunkPos],
    threshold: i32,
    cache: Option<&ChunkCache>,
) -> io::Result<()> {
    if add.is_empty() {
        return Ok(());
    }
    write_packet_any(stream, packets::PLAY_CHUNK_BATCH_START_ID, &[], threshold).await?;
    for pos in add {
        let body = play_chunk_body(world, *pos, cache);
        write_packet_any(stream, packets::PLAY_CHUNK_DATA_ID, body.as_slice(), threshold).await?;
    }
    let mut body = Vec::new();
    mc_protocol::write_varint(add.len() as i32, &mut body);
    write_packet_any(stream, packets::PLAY_CHUNK_BATCH_FINISHED_ID, &body, threshold).await
}

/// 逐个发 UnloadChunk（0x26 forget_level_chunk）。
async fn send_unload_chunks(stream: &mut TcpStream, remove: &[mc_core::ChunkPos], threshold: i32) -> io::Result<()> {
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
    send_chunk_batch(stream, world, &update.add, threshold, cache).await?;
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
async fn read_packet_any(stream: &mut TcpStream, threshold: i32) -> io::Result<(i32, Vec<u8>)> {
    if packets::compression_enabled(threshold) {
        read_packet_framed(stream, threshold).await
    } else {
        read_packet(stream).await
    }
}

/// 向 ECS 投递 Play 事件：不阻塞 net 线程；通道满/关闭只 debug（主循环用无界通道）。
fn emit_play_event(events: &Option<Sender<PlayEvent>>, ev: PlayEvent) {
    if let Some(tx) = events {
        if let Err(e) = tx.try_send(ev) {
            debug!("play event dropped ({e})");
        }
    }
}

/// 玩家跨区块：SetCenterChunk → 卸载越界区块 → 一批补发缺失区块（batch_size = add.len）。
/// 单轴跨区块 add/remove = 2r+1（r=10 → 21），斜跨最多 4r+1（41）；不引入额外阻塞限流。
async fn maybe_move_view(
    stream: &mut TcpStream,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
    view: &mut ChunkView,
    world: &WorldGen,
    x: f64,
    z: f64,
    cache: Option<&ChunkCache>,
) -> io::Result<()> {
    let Some(center) = ChunkView::chunk_of_block(x, z) else {
        return Ok(());
    };
    if center == view.center() {
        return Ok(());
    }
    let threshold = opts.threshold;
    let mut body = Vec::new();
    packets::SetCenterChunk { chunk_x: center.0, chunk_z: center.1 }.encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_CENTER_CHUNK_ID, &body, threshold).await?;
    let update = view.plan(center);
    send_unload_chunks(stream, &update.remove, threshold).await?;
    send_chunk_batch(stream, world, &update.add, threshold, cache).await?;
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

async fn play_loop(
    stream: &mut TcpStream,
    addr: SocketAddr,
    profile: &mc_core::PlayerProfile,
    opts: &PlayOptions,
    mut seq: i64,
    mut view: ChunkView,
    world: &WorldGen,
    cache: Option<&ChunkCache>,
) -> io::Result<()> {
    let threshold = opts.threshold;
    // net 线程的 last_seen 只认 KeepAlive 回显（移动包不刷新，避免客户端只发移动不回心跳）；
    // ECS 侧的 PlayerKeepAlive.last_seen 另由 movement_ingest_system 按移动包刷新，用于离线清理。
    let mut last_seen = Instant::now();
    // 客户端在 Play 里每 tick 都发 0x0D，read_packet 不会超时；必须按时间主动发 KeepAlive，
    // 否则客户端 30s 收不到 0x2D 会 “连接超时” 断开（2026-11-23 真机实测）。
    let mut last_keepalive = Instant::now();
    loop {
        let next = read_packet_any(stream, threshold).await;
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
                        let pos = match m {
                            packets::C2SMovement::Pos(p) => Some((p.x, p.z)),
                            packets::C2SMovement::PosRot(p) => Some((p.x, p.z)),
                            _ => None,
                        };
                        if let Some((x, z)) = pos {
                            if let Err(e) = maybe_move_view(stream, profile, opts, &mut view, world, x, z, cache).await {
                                debug!("play uuid={} view move failed ({e}), closing", profile.uuid);
                                break;
                            }
                        }
                        emit_play_event(
                            &opts.events,
                            PlayEvent::Movement { addr, conn_id: opts.conn_id, movement: m },
                        );
                    }
                    Err(e) => debug!("play uuid={} bad movement id={id:#04x}: {e}", profile.uuid),
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
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
            let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
            assert_eq!(id, packets::CONFIG_REGISTRY_DATA_S2C_ID);
            assert_eq!(&payload, want);
        }
        // 4. Update Tags 1包（CB 0x0E：原版全量 15 个注册表）。
        let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::CONFIG_UPDATE_TAGS_S2C_ID);
        assert_eq!(payload, packets::vanilla_config_777::VANILLA_CONFIG_UPDATE_TAGS_BODY_777);
        // 5. Finish S2C → 回 Finish Ack。
        let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
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
        let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        super::super::framing::write_packet(&mut c, 0x7F, &[]).await.unwrap();
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
        let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        let mut kp = Vec::new();
        packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kp);
        use tokio::io::AsyncWriteExt;
        c.write_all(&kp).await.unwrap();
        for _ in 0..5 {
            let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        }
        super::super::framing::write_packet(&mut c, 0x7F, &[]).await.unwrap();
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
            let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        }
        // 回一个 keepalive echo，服务端只log不断开；随后客户端断开，服务端正常退出
        let mut b = Vec::new();
        packets::KeepAliveC2S(1).encode_body(&mut b);
        super::super::framing::write_packet(&mut c, packets::PLAY_KEEPALIVE_C2S_ID, &b).await.unwrap();
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
            let _ = super::super::framing::read_packet(&mut c).await.unwrap();
        }
        // 依次发 0x1E/0x1F/0x20/0x21，服务端转发 ECS（本测试无通道，只log）且不断开；
        // 坐标保持在出生区块，避免触发跨区块动态补发。
        let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
        let x = spawn.0 as f64 + 0.5;
        let z = spawn.2 as f64 + 0.5;
        let mut body = Vec::new();
        packets::MovePlayerPos { x, y: 80.0, z, on_ground: true, horizontal_collision: false }
            .encode_body(&mut body);
        super::super::framing::write_packet(&mut c, packets::PLAY_MOVE_POS_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerPosRot { x, y: 80.0, z, yaw: 90.0, pitch: 0.0, on_ground: true, horizontal_collision: false }
            .encode_body(&mut body);
        super::super::framing::write_packet(&mut c, packets::PLAY_MOVE_POSROT_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerRot { yaw: 180.0, pitch: 0.0, on_ground: false, horizontal_collision: false }
            .encode_body(&mut body);
        super::super::framing::write_packet(&mut c, packets::PLAY_MOVE_ROT_C2S_ID, &body).await.unwrap();
        body.clear();
        packets::MovePlayerFlags { on_ground: true, horizontal_collision: false }.encode_body(&mut body);
        super::super::framing::write_packet(&mut c, packets::PLAY_MOVE_FLAGS_C2S_ID, &body).await.unwrap();
        // 300ms内不应EOF（连接仍开着）
        match tokio::time::timeout(std::time::Duration::from_millis(300), super::super::framing::read_packet(&mut c)).await {
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
        let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
        assert_eq!(id, packets::PLAY_LOGIN_PLAY_ID);
        let (login, _) = packets::LoginPlay::decode_body(&payload).unwrap();
        assert_eq!(login.view_distance, 2, "LoginPlay view_distance must match effective radius");
        assert_eq!(login.simulation_distance, 2);
        let mut chunks = 0;
        let mut finished = None;
        for _ in 0..(3 + 25 + 3) {
            let (id, payload) = super::super::framing::read_packet(&mut c).await.unwrap();
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
