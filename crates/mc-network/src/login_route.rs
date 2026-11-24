//! login_route: Login离线流程 Start→[SetCompression]→Success→Ack→Config(Finish→Ack)→enter_play(真下发)
//! Play 期：KeepAlive + C2S 移动包；移动包经 crossbeam channel 投递 ECS（PlayEvent），
//! 同时 net 线程自身按玩家跨区块动态补发有效视距区块并卸载越界区块。
//! 视距来自 `ServerConfig.view_distance`（`clamp_view_radius` 限制 2..=10），
//! 区块包体优先走共享 `ChunkCache`；连接结束在所有退出路径恰好投递一次 Disconnect。
use super::chunk_cache::{encode_chunk_data_body, encode_chunk_data_body_mutable, ChunkCache};
use super::chunk_view::{clamp_view_radius, view_chunk_count, ChunkView, PLAY_VIEW_RADIUS};
use super::drop_state::{DropState, PLAYER_ENTITY_ID};
use super::framing::{read_packet, read_packet_framed, to_io_invalid, write_packet, write_packet_framed};
use super::play_events::PlayEvent;
use crossbeam_channel::Sender;
use mc_protocol::packets;
use mc_world::{MutableWorld, WorldGen, DEFAULT_SEED};
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
    /// 服务端权威可变世界（P17 挖掘/放置）；None 时方块交互包只忽略不断线。
    pub mutable_world: Option<Arc<MutableWorld>>,
    /// 掉落物实体 + 背包共享状态（P18）；None 时挖/放不掉落不同步（旧行为）。
    pub drops: Option<Arc<DropState>>,
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
    play_loop(stream, addr, profile, opts, keepalive_id, view, &world, cache, mutable).await
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
async fn send_chunk_batch(
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

fn eye_of(pos: (f64, f64, f64)) -> (f64, f64, f64) {
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

/// 挖掘权威判定（Player Action 0x29）。
/// - status 0/2（开始/完成）：创造风格即时破坏；非法（越界/超距/空气/水）回滚。
/// - status 1（取消）：只 ack。- status 3..=7（丢弃/吃食/换手）：无世界影响，直接忽略。
/// - `world` 为 None（旧入口/单测无世界）：忽略不断线。
/// 判定内已落子（`set`），调用方负责：缓存失效 + 发 BlockUpdate/Ack + 投递事件。
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
            let old = world.set(x, y, z, mc_world::AIR_BLOCK_ID);
            let drops = mc_world::drops_for_block(old, x, y, z, world.seed());
            Some(BlockEditOutcome {
                sequence: act.sequence,
                changes: vec![BlockChange { x, y, z, old_id: old, new_id: mc_world::AIR_BLOCK_ID }],
                drops,
            })
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

/// 落子后动作：缓存失效 → 发 BlockUpdate → 发 Ack → 投递 BlockEdit 事件。
async fn apply_block_outcome(
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

// ---------------------------------------------------------------------------
// P18 掉落物 + 背包（服务端权威，net 线程即时同步；ECS 无写通道，见模块文档）。
//
// - 生成：decide_dig 的 drops → alloc id → support_top_y 落点 → ItemEntities::spawn
//   → Spawn(0x01)+SetEntityData(0x65) 发给本连接（视距内才发）。
// - 下落：play_loop 每 50ms tick 一次（read 超时即 tick，不改变 10s 半开超时语义），
//   移动过的实体发 Teleport(0x23)；despawn/出视距发 Remove(0x4E)。
// - 拾取：delay 到期 + 与脚位置 <1.5 → TakeItem(0x7F)+Remove，入库，全量 Content 同步。
// - 背包：Join 全量 Content；放置成功 consume_held + 单槽 Slot 同步；C2S 0x36 切手持。
// - ClickContainer(0x12)/CreativeSlot(0x39) 只忽略（Hashed Slot 复杂，留后续）。
// ---------------------------------------------------------------------------

/// 实体的视距判定（切比雪夫，与 ChunkView 同口径）。
fn drop_in_view(center: mc_core::ChunkPos, radius: i32, chunk: mc_core::ChunkPos) -> bool {
    (chunk.0 - center.0).abs() <= radius && (chunk.1 - center.1).abs() <= radius
}

/// 存档键：稳定用户名（离线 uuid 随机，见 inventory 文档）。
fn user_tag(profile: &mc_core::PlayerProfile) -> String {
    profile.username.clone()
}

async fn send_spawn(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    id: i32,
) -> io::Result<()> {
    let snap = {
        let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        items.get(id).map(|e| (e.id, e.uuid, e.pos, e.item_id, e.count))
    };
    let Some((id, uuid, (x, y, z), item_id, count)) = snap else { return Ok(()) };
    let mut body = Vec::new();
    packets::SpawnItemEntity {
        entity_id: id,
        uuid,
        x,
        y,
        z,
        vel: packets::LpVec3::zero(),
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

async fn send_data(
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

async fn send_teleport(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    id: i32,
    pos: (f64, f64, f64),
    on_ground: bool,
) -> io::Result<()> {
    let mut body = Vec::new();
    packets::TeleportEntitySync {
        entity_id: id,
        x: pos.0,
        y: pos.1,
        z: pos.2,
        yaw: 0.0,
        pitch: 0.0,
        on_ground,
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_TELEPORT_ENTITY_S2C_ID, &body, opts.threshold).await
}

async fn send_remove(
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

/// 全量背包同步（Join/拾取后）。
async fn send_full_inventory(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
) -> io::Result<()> {
    let (slots, state) = drops
        .with_inventory(opts.conn_id, |inv| (inv.to_protocol_slots(), inv.next_state()))
        .unwrap_or_else(|| (vec![packets::Slot::empty(); packets::PLAYER_INVENTORY_SLOT_COUNT], 1));
    let mut body = Vec::new();
    packets::SetContainerContent {
        window_id: packets::PLAYER_INVENTORY_WINDOW_ID,
        state_id: state,
        slots,
        carried: packets::Slot::empty(),
    }
    .encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, &body, opts.threshold)
        .await
}

/// 单槽同步（放置消耗后）。
async fn send_held_slot(
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
async fn spawn_drops(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    world: &MutableWorld,
    outcome: &BlockEditOutcome,
    view: &ChunkView,
) -> io::Result<()> {
    if outcome.drops.is_empty() || outcome.changes.is_empty() {
        return Ok(());
    }
    let (bx, by, bz) = {
        let c = &outcome.changes[0];
        (c.x, c.y, c.z)
    };
    for d in &outcome.drops {
        let id = drops.alloc_id();
        let rest = world.support_top_y(bx, bz) as f64;
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
            let in_view_now = {
                let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
                items.get(tid).map(|e| drop_in_view(view.center(), view.radius(), e.chunk))
            };
            let Some(in_view_now) = in_view_now else { continue };
            if !in_view_now {
                continue;
            }
            if tid == id {
                send_spawn(stream, opts, drops, tid).await?;
            } else if drops.is_known(opts.conn_id, tid) {
                send_data(stream, opts, drops, tid).await?;
            } else {
                send_spawn(stream, opts, drops, tid).await?;
            }
        }
    }
    Ok(())
}

/// 每 50ms tick：下落同步 + despawn + 拾取 + 入库同步。
async fn tick_drops(
    stream: &mut TcpStream,
    opts: &PlayOptions,
    drops: &DropState,
    conn_user: &str,
    last_pos: (f64, f64, f64),
    view: &ChunkView,
) -> io::Result<()> {
    let (moved, dead) = {
        let mut items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        items.tick()
    };
    // 移动同步：视距内发 Teleport（未见过则补发 Spawn）；known 但已出视距发 Remove。
    for id in moved {
        let snap = {
            let items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
            items.get(id).map(|e| (e.pos, e.on_ground, e.chunk))
        };
        let Some((pos, on_ground, chunk)) = snap else { continue };
        if drop_in_view(view.center(), view.radius(), chunk) {
            if !drops.is_known(opts.conn_id, id) {
                send_spawn(stream, opts, drops, id).await?;
            } else {
                send_teleport(stream, opts, id, pos, on_ground).await?;
            }
        } else if drops.is_known(opts.conn_id, id) {
            send_remove(stream, opts, drops, &[id]).await?;
        }
    }
    // despawn：known 的发 Remove。
    {
        let gone: Vec<i32> = dead.into_iter().filter(|id| drops.is_known(opts.conn_id, *id)).collect();
        send_remove(stream, opts, drops, &gone).await?;
    }
    // 拾取：delay 到期 + 距离 <1.5 → TakeItem+Remove，入库，全量同步 + 刷盘。
    let picked: Vec<(i32, i32, i32)> = {
        let mut items = drops.items.lock().unwrap_or_else(|p| p.into_inner());
        let ids = items.pickable(last_pos);
        let mut out = Vec::new();
        for id in ids {
            if let Some(e) = items.remove(id) {
                out.push((id, e.item_id, e.count));
            }
        }
        out
    };
    if picked.is_empty() {
        return Ok(());
    }
    for (id, _, count) in &picked {
        let mut body = Vec::new();
        packets::TakeItemEntity { collected_id: *id, collector_id: PLAYER_ENTITY_ID, count: *count }
            .encode_body(&mut body);
        write_packet_any(stream, packets::PLAY_PICKUP_ITEM_S2C_ID, &body, opts.threshold).await?;
    }
    let gone: Vec<i32> = picked.iter().map(|(id, _, _)| *id).collect();
    send_remove(stream, opts, drops, &gone).await?;
    for (_, item_id, count) in picked {
        drops.with_inventory(opts.conn_id, |inv| {
            inv.add_item(item_id, count);
        });
    }
    send_full_inventory(stream, opts, drops).await?;
    drops.save_inventory(opts.conn_id, conn_user);
    Ok(())
}

/// 视距变化时补/卸实体：出视距的发 Remove，进视距的补发 Spawn。
async fn sync_view_entities(
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
    mutable: Option<&MutableWorld>,
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
    // P18 掉落 tick 节拍：按墙钟每 50ms 跑一次物理/拾取。
    // 注意不能只放在读超时分支——原版客户端每 tick 都发移动包（静止也有 StatusOnly），
    // 包流不断时 50ms 超时永不触发，tick 会被饿死：pickup_delay 减不下去，
    // 掉落物看得见但永远捡不起来（P18 回归：busy 连接拾取测试）。
    let mut last_drop_tick = Instant::now();
    loop {
        // P18 掉落 tick：50ms 读超时即跑一次物理/拾取（不改变 10s 半开超时语义）。
        let next = match tokio::time::timeout(
            Duration::from_millis(50),
            read_packet_any(stream, threshold),
        )
        .await
        {
            Err(_) => {
                if let (Some(ds), Some(_)) = (&opts.drops, mutable) {
                    if let Err(e) = tick_drops(stream, opts, ds, &conn_user, last_pos, &view).await {
                        debug!("play uuid={} drop tick failed ({e}), closing", profile.uuid);
                        break;
                    }
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
                        match m {
                            packets::C2SMovement::Pos(p) => {
                                last_pos = (p.x, p.y, p.z);
                                if let Err(e) = maybe_move_view(stream, profile, opts, &mut view, world, p.x, p.z, cache, mutable).await {
                                    debug!("play uuid={} view move failed ({e}), closing", profile.uuid);
                                    break;
                                }
                                if let Some(ds) = &opts.drops {
                                    if let Err(e) = sync_view_entities(stream, opts, ds, &view).await {
                                        debug!("play uuid={} view entity sync failed ({e}), closing", profile.uuid);
                                        break;
                                    }
                                }
                            }
                            packets::C2SMovement::PosRot(p) => {
                                last_pos = (p.x, p.y, p.z);
                                if let Err(e) = maybe_move_view(stream, profile, opts, &mut view, world, p.x, p.z, cache, mutable).await {
                                    debug!("play uuid={} view move failed ({e}), closing", profile.uuid);
                                    break;
                                }
                                if let Some(ds) = &opts.drops {
                                    if let Err(e) = sync_view_entities(stream, opts, ds, &view).await {
                                        debug!("play uuid={} view entity sync failed ({e}), closing", profile.uuid);
                                        break;
                                    }
                                }
                            }
                            _ => {}
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
                // P17 挖掘：服务端权威判定 + 即时落子 + BlockUpdate/Ack 回包。
                // P18：成功掉落同步生成物品实体（视距内 Spawn+Data）。
                match packets::PlayerAction::decode_body(&payload) {
                    Ok((act, _)) => {
                        if let Some(outcome) = decide_dig(
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
                                        spawn_drops(stream, opts, ds, world, &outcome, &view).await
                                    {
                                        debug!("play uuid={} drop spawn failed ({e}), closing", profile.uuid);
                                        break;
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
                // P18：手持映射决定材料（未知/空手回退石头），成功后扣 1 + 单槽同步。
                match packets::UseItemOn::decode_body(&payload) {
                    Ok((act, _)) => {
                        let place_id = opts
                            .drops
                            .as_ref()
                            .and_then(|ds| ds.with_inventory(opts.conn_id, |inv| inv.held()).flatten())
                            .and_then(|(item_id, _)| mc_world::block_for_item(item_id))
                            .unwrap_or(mc_world::DEFAULT_PLACE_BLOCK_ID);
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
                || id == packets::PLAY_CLICK_CONTAINER_C2S_ID
                || id == packets::PLAY_SET_CREATIVE_SLOT_C2S_ID =>
            {
                // 挥拳/容器点击/创造背包点选：P18 只忽略不断线（ClickContainer 的
                // Hashed Slot 复杂，整理功能留后续；不断线已由单测覆盖）。
                debug!("play uuid={} c2s id={id:#04x} ignored (no state change)", profile.uuid);
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
        // 包流不断时也按 50ms 节拍跑掉落 tick，防止读超时分支饿死。
        if last_drop_tick.elapsed() >= Duration::from_millis(50) {
            if let (Some(ds), Some(_)) = (&opts.drops, mutable) {
                if let Err(e) = tick_drops(stream, opts, ds, &conn_user, last_pos, &view).await {
                    debug!("play uuid={} drop tick failed ({e}), closing", profile.uuid);
                    break;
                }
            }
            last_drop_tick = Instant::now();
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
    fn dig_start_breaks_grass_and_finish_breaks_too() {
        for status in [packets::PLAYER_ACTION_START_DIG, packets::PLAYER_ACTION_FINISH_DIG] {
            let (world, feet, center) = dig_fixture();
            let (x, y, z) = grass_top();
            let act = packets::PlayerAction { status, x, y, z, face: 1, sequence: 5 };
            let out = decide_dig(Some(&world), eye_of(feet), center, 2, &act).unwrap();
            assert_eq!(out.sequence, 5);
            assert_eq!(
                out.changes,
                vec![BlockChange { x, y, z, old_id: mc_world::GRASS_BLOCK_ID, new_id: 0 }]
            );
            assert_eq!(world.get(x, y, z), 0, "decide must apply the edit");
            assert_eq!(world.dirty_len(), 1);
            // P18：挖草带出 dirt 掉落（item 55×1）。
            assert_eq!(
                out.drops,
                vec![mc_world::Drop { item_id: mc_world::drops::ITEM_DIRT, count: 1 }]
            );
        }
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
            &packets::PlayerAction { status: 2, x, y, z, face: 1, sequence: 1 },
        )
        .unwrap();
        assert!(air_out.drops.is_empty(), "rollback must not drop");
        // cancel 只 ack，无掉落。
        let cancel = decide_dig(
            Some(&world),
            eye,
            center,
            2,
            &packets::PlayerAction { status: 1, x, y, z, face: 0, sequence: 8 },
        )
        .unwrap();
        assert!(cancel.drops.is_empty());
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
            &packets::PlayerAction { status: 2, x, y, z, face: 1, sequence: 1 },
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
            &packets::PlayerAction { status: 2, x: x + 100, y, z, face: 1, sequence: 2 },
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
            &packets::PlayerAction { status: 1, x, y, z, face: 0, sequence: 8 },
        )
        .unwrap();
        assert!(cancel.changes.is_empty(), "cancel must only ack");
        assert_eq!(cancel.sequence, 8);
        // 丢弃/吃食/换手：无世界影响，直接忽略（不 ack，避免误确认旧序列）。
        for status in [3, 4, 5, 6, 7, 99] {
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
                &packets::PlayerAction { status: 2, x, y, z, face: 1, sequence: 1 },
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
}
