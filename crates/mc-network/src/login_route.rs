//! login_route: Login离线流程 Start→[SetCompression]→Success→Ack→Config(Finish→Ack)→enter_play(真下发)
//! Play 期：KeepAlive + C2S 移动包；移动包经 crossbeam channel 投递 ECS（PlayEvent），
//! 同时 net 线程自身按玩家跨区块动态补发 radius=4 区块并卸载越界区块。
use super::chunk_view::ChunkView;
use super::framing::{read_packet, read_packet_framed, to_io_invalid, write_packet, write_packet_framed};
use super::play_events::PlayEvent;
use crossbeam_channel::Sender;
use mc_protocol::packets;
use mc_world::{WorldGen, DEFAULT_SEED};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use std::io;

use super::NewConnection;

use std::time::{Duration, Instant};

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

/// 完整登录入口：压缩阈值 + 世界 seed + Play 事件通道（Serve 从 NetConfig 透传）。
pub async fn handle_login_configured(
    mut stream: TcpStream,
    addr: SocketAddr,
    tx: &Sender<NewConnection>,
    threshold: i32,
    seed: i64,
    events: Option<Sender<PlayEvent>>,
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
    info!("login ok uuid={} name={}", uuid, profile.username);
    if let Err(e) = config_finish(&mut stream, threshold).await {
        warn!("{addr} config: finish failed ({e}), disconnect");
        return Ok(());
    }
    notify_login_success(tx, addr, &profile);
    enter_play_with_events(stream, addr, profile, threshold, seed, events).await?;
    Ok(())
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
pub fn notify_login_success(tx: &Sender<NewConnection>, addr: SocketAddr, profile: &mc_core::PlayerProfile) {
    if let Err(e) = tx.try_send(NewConnection(addr)) {
        debug!("login success notify dropped for {addr} ({e}): {profile:?}");
    }
}

/// 进入 Play：拿住 socket，按序真下发后进循环。threshold透传，压缩开启后Play包走压缩帧。
/// 旧签名保留（无 ECS 事件通道/默认 seed）；Serve 走 [`enter_play_with_events`]。
pub async fn enter_play(stream: TcpStream, addr: SocketAddr, profile: mc_core::PlayerProfile, threshold: i32) -> io::Result<()> {
    enter_play_with_events(stream, addr, profile, threshold, DEFAULT_SEED, None).await
}

/// 进入 Play（带世界 seed + Play 事件通道）：Join 序列发送后进入循环。
pub async fn enter_play_with_events(
    mut stream: TcpStream,
    addr: SocketAddr,
    profile: mc_core::PlayerProfile,
    threshold: i32,
    seed: i64,
    events: Option<Sender<PlayEvent>>,
) -> io::Result<()> {
    let world = WorldGen::new(seed);
    let spawn = world.spawn_pos();
    info!(
        "enter_play uuid={} name={} threshold={} seed={} spawn=({},{},{})",
        profile.uuid, profile.username, threshold, seed, spawn.0, spawn.1, spawn.2
    );
    // 出生点与中心区块由 WorldGen 按 seed 计算（x/z 固定 369/381 → chunk 23,23）。
    let center = world.spawn_chunk();
    let mut view = ChunkView::new(center);
    let keepalive_id = send_join_sequence(&mut stream, &profile, threshold, &mut view, &world).await?;
    info!(
        "play join sent uuid={} name={} keepalive={} view_sent={} seed={} spawn=({},{},{})",
        profile.uuid,
        profile.username,
        keepalive_id,
        view.sent_count(),
        seed,
        spawn.0,
        spawn.1,
        spawn.2
    );
    play_loop(&mut stream, addr, &profile, keepalive_id, threshold, view, &world, events).await
}

/// 压缩感知写包：threshold>=0 走压缩帧，否则普通帧。
async fn write_packet_any(stream: &mut TcpStream, packet_id: i32, payload: &[u8], threshold: i32) -> io::Result<()> {
    if packets::compression_enabled(threshold) {
        write_packet_framed(stream, packet_id, payload, threshold).await
    } else {
        write_packet(stream, packet_id, payload).await
    }
}

/// 单块 ChunkData 包体（按 seed 生成真实 palette/heightmap/light）。
fn play_chunk_body(world: &WorldGen, pos: mc_core::ChunkPos) -> Vec<u8> {
    let chunk = world.generate(pos);
    let chunk_data = packets::PlayChunkData {
        chunk_x: pos.0,
        chunk_z: pos.1,
        heightmaps_nbt: chunk.heightmaps_bytes(),
        section_bytes: chunk.network_bytes(),
        light_bytes: chunk.light_bytes(),
    };
    let mut body = Vec::new();
    chunk_data.encode_body(&mut body);
    body
}

/// 发一批 ChunkData：ChunkBatchStart → N×ChunkData → ChunkBatchFinished(batch_size=N)。
/// 空批次不发（客户端 ChunkBatchSizeCalculator 只接受正 batch_size）。
async fn send_chunk_batch(
    stream: &mut TcpStream,
    world: &WorldGen,
    add: &[mc_core::ChunkPos],
    threshold: i32,
) -> io::Result<()> {
    if add.is_empty() {
        return Ok(());
    }
    write_packet_any(stream, packets::PLAY_CHUNK_BATCH_START_ID, &[], threshold).await?;
    for pos in add {
        let body = play_chunk_body(world, *pos);
        write_packet_any(stream, packets::PLAY_CHUNK_DATA_ID, &body, threshold).await?;
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
    threshold: i32,
    view: &mut ChunkView,
    world: &WorldGen,
) -> io::Result<i64> {
    let mut body = Vec::new();
    packets::LoginPlay::default().encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_LOGIN_PLAY_ID, &body, threshold).await?;
    debug!("play {} login play sent", profile.username);
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
    // 30s 后客户端超时放行。因此一次性发 radius=4 的 81 块（同一批），
    // 与 mc-server 视距队列 / ChunkView 一致。
    let update = view.plan(center);
    debug_assert_eq!(update.add.len(), super::chunk_view::PLAY_VIEW_CHUNK_COUNT);
    send_chunk_batch(stream, world, &update.add, threshold).await?;
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
async fn maybe_move_view(
    stream: &mut TcpStream,
    profile: &mc_core::PlayerProfile,
    view: &mut ChunkView,
    world: &WorldGen,
    x: f64,
    z: f64,
    threshold: i32,
) -> io::Result<()> {
    let Some(center) = ChunkView::chunk_of_block(x, z) else {
        return Ok(());
    };
    if center == view.center() {
        return Ok(());
    }
    let mut body = Vec::new();
    packets::SetCenterChunk { chunk_x: center.0, chunk_z: center.1 }.encode_body(&mut body);
    write_packet_any(stream, packets::PLAY_SET_CENTER_CHUNK_ID, &body, threshold).await?;
    let update = view.plan(center);
    send_unload_chunks(stream, &update.remove, threshold).await?;
    send_chunk_batch(stream, world, &update.add, threshold).await?;
    info!(
        "play view move uuid={} name={} center=({},{}) add={} remove={} sent_total={}",
        profile.uuid,
        profile.username,
        center.0,
        center.1,
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
    mut seq: i64,
    threshold: i32,
    mut view: ChunkView,
    world: &WorldGen,
    events: Option<Sender<PlayEvent>>,
) -> io::Result<()> {
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
                        // ECS 用它刷新 PlayerKeepAlive.last_seen；在线数仍只在 ECS timeout 系统减。
                        emit_play_event(&events, PlayEvent::KeepAlive { addr });
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
                            profile.uuid, profile.username, id, m, events.is_some()
                        );
                        let pos = match m {
                            packets::C2SMovement::Pos(p) => Some((p.x, p.z)),
                            packets::C2SMovement::PosRot(p) => Some((p.x, p.z)),
                            _ => None,
                        };
                        if let Some((x, z)) = pos {
                            if let Err(e) = maybe_move_view(stream, profile, &mut view, world, x, z, threshold).await {
                                debug!("play uuid={} view move failed ({e}), closing", profile.uuid);
                                break;
                            }
                        }
                        emit_play_event(&events, PlayEvent::Movement { addr, movement: m });
                    }
                    Err(e) => debug!("play uuid={} bad movement id={id:#04x}: {e}", profile.uuid),
                }
            }
            Ok((id, payload)) => {
                debug!("play uuid={} c2s id={id:#04x} len={} (ignored)", profile.uuid, payload.len());
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                if last_seen.elapsed() > Duration::from_secs(15) {
                    warn!("play uuid={} keepalive timeout >15s without echo, kicking (online-- in ECS timeout system)", profile.uuid);
                    // online递减单点放在ECS keepalive_timeout_system，避免net/ECS双减；
                    // net线程只关socket（踢出）。
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
}
