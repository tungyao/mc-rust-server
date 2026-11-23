//! mc-network: tokio 监听 + 包收发，解包后推 channel 给 ECS
//!
//! 只兼容 26.3 (protocol 777)：每个连接先读 Handshake 包，用
//! [`mc_core::check_protocol`] 校验版本；`next_state == 1` 走 Status 流程
//! （Request → Response → Ping → Pong，走完即关，不计数）；`next_state == 2`
//! 走 Login 离线流程（Start → Success → Acknowledged → [`enter_play`]），
//! 不做正版验证/加密（P3 再做）。只有 Login Ack 成功才算一次真实连接，
//! 上报点见 [`notify_login_success`]（任务E：在线数只认这里）。
use crossbeam_channel::Sender;
use mc_protocol::packets::{
    self, Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID, PING_PACKET_ID,
    STATUS_REQUEST_PACKET_ID, STATUS_RESPONSE_PACKET_ID,
};
use mc_protocol::read_varint;
use std::io;
use std::net::SocketAddr;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct NewConnection(pub std::net::SocketAddr);

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Status 回包默认配置；`NetConfig::default()` 的回退值，正式路径走 `NetConfig` 透传。
pub const STATUS_MAX_PLAYERS: usize = 100;
pub const STATUS_MOTD: &str = "Rust MC 26.3 (777) self-made";
/// 单次读包超时：防半开连接长期占任务。
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// 单包上限 2 MiB：防恶意 length 导致 OOM（Handshake/Status 正常只有几十字节）。
const MAX_PACKET_LEN: usize = 2_097_152;

/// 网络配置：由 mc-server 启动时从 `ServerConfig` 透传。
/// - `max_players` / `motd` 用于 Status Response JSON；
/// - `online_count` 与 ECS 侧 `PlayersOnline` 共享同一 `Arc`，
///   网络线程只读（`load`），计数由 ECS 侧 `try_recv` 排空时 `fetch_add`。
#[derive(Debug, Clone)]
pub struct NetConfig {
    pub max_players: usize,
    pub motd: String,
    pub online_count: Arc<AtomicUsize>,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            max_players: STATUS_MAX_PLAYERS,
            motd: STATUS_MOTD.to_string(),
            online_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl NetConfig {
    pub fn new(max_players: usize, motd: String, online_count: Arc<AtomicUsize>) -> Self {
        Self {
            max_players,
            motd,
            online_count,
        }
    }
}

pub async fn serve(
    listener: TcpListener,
    tx: Sender<NewConnection>,
    config: NetConfig,
) -> Result<(), NetError> {
    info!("listening on {:?}", listener.local_addr()?);
    loop {
        let (socket, addr) = listener.accept().await?;
        info!("accept {addr}");
        // accept 时不计数：Status ping 不算在线，只有 Login 握手成功才 push。
        // tx/config clone 进连接任务，accept loop 永不阻塞。
        let tx = tx.clone();
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, addr, tx, config).await {
                debug!("connection {addr} closed: {e}");
            }
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    tx: Sender<NewConnection>,
    config: NetConfig,
) -> io::Result<()> {
    // 1. 首包必须是 Handshake
    let (packet_id, payload) = read_packet(&mut stream).await?;
    if packet_id != HANDSHAKE_PACKET_ID {
        warn!("{addr} expected handshake 0x00, got {packet_id:#04x}, disconnect");
        return Ok(());
    }
    let (hs, _) = Handshake::decode_body(&payload).map_err(to_io_invalid)?;

    // 2. 只认 26.3/777，其他版本直接断开
    if let Err(e) = mc_core::check_protocol(hs.protocol_version) {
        warn!(
            "{addr} unsupported protocol {} ({e}), disconnect",
            hs.protocol_version
        );
        return Ok(());
    }

    // 3. 按 next_state 分流
    match hs.next_state {
        HandshakeNextState::Status => {
            debug!("{addr} status handshake (server={}:{})", hs.server_address, hs.server_port);
            // Status 走完不计数、不 push。
            handle_status(&mut stream, addr, &config).await?;
        }
        HandshakeNextState::Login => {
            debug!("{addr} login handshake (server={}:{})", hs.server_address, hs.server_port);
            // socket 所有权移交给 Login 流程（与 Status 的 &mut 借用区分开）；
            // Login 成功后由 enter_play 接管，Status 走完即关。
            handle_login(stream, addr, &tx).await?;
        }
    }
    Ok(())
}

/// Status 流程：Request → Response → (可选) Ping → Pong，随后由客户端断开。
async fn handle_status(stream: &mut TcpStream, addr: SocketAddr, config: &NetConfig) -> io::Result<()> {
    let (id, payload) = read_packet(stream).await?;
    if id != STATUS_REQUEST_PACKET_ID {
        warn!("{addr} status: expected request 0x00, got {id:#04x}, disconnect");
        return Ok(());
    }
    if !payload.is_empty() {
        debug!("{addr} status: request has {} trailing bytes, ignored", payload.len());
    }
    packets::decode_status_request(&payload).map_err(to_io_invalid)?;

    let json = packets::status_json(
        config.max_players,
        config.online_count.load(Ordering::SeqCst),
        &config.motd,
    );
    let mut payload = Vec::new();
    packets::encode_string(&json, &mut payload);
    write_packet(stream, STATUS_RESPONSE_PACKET_ID, &payload).await?;
    debug!("{addr} status response sent ({} json bytes)", json.len());

    // 部分客户端只取 server list 不 ping：对端直接关闭时当正常结束（EOF），不报错。
    match read_packet(stream).await {
        Ok((PING_PACKET_ID, p)) => {
            let v = packets::decode_ping_request(&p).map_err(to_io_invalid)?;
            let mut body = Vec::new();
            packets::write_i64_be(v, &mut body);
            write_packet(stream, mc_protocol::packets::PONG_PACKET_ID, &body).await?;
            debug!("{addr} pong sent ({v})");
        }
        Ok((other, _)) => {
            warn!("{addr} status: expected ping 0x01, got {other:#04x}, disconnect");
        }
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            debug!("{addr} closed after status response");
        }
        Err(e) => return Err(e),
    }
    Ok(())
}

/// Login 离线流程（无加密/无正版验证）：Handshake(Login) → Login Start →
/// Login Success → Login Acknowledged → [`enter_play`]。
/// 与 Status 严格区分：Status 走完即关且不计数；Login 只有等到 Ack
/// 才算一次真实连接（上报点见 [`notify_login_success`]）。
async fn handle_login(mut stream: TcpStream, addr: SocketAddr, tx: &Sender<NewConnection>) -> io::Result<()> {
    // 1. Login Start（serverbound `0x00`：String name + UUID）。
    let (id, payload) = read_packet(&mut stream).await?;
    if id != packets::LOGIN_START_PACKET_ID {
        warn!("{addr} login: expected start 0x00, got {id:#04x}, disconnect");
        return Ok(());
    }
    let (start, _) = packets::LoginStart::decode_body(&payload).map_err(to_io_invalid)?;
    // 用户名基本校验：空名/超长（原版上限 16 字符）直接断开。
    if start.name.is_empty() || start.name.chars().count() > 16 {
        warn!("{addr} login: bad username {:?}, disconnect", start.name);
        return Ok(());
    }

    // 2. 离线 UUID（不发 Encryption Request `0x01`，不做正版验证；
    // P3 在此插入 Mojang 会话校验 + 共享密钥协商）。
    // 压缩未启用：Set Compression(`0x03`) 阈值 -1 占位，不发送；
    // P3 启用时阈值 `>= 0`，在 Success 之前发送，见 [`packets::encode_set_compression_packet`]。
    debug_assert_eq!(packets::COMPRESSION_DISABLED_THRESHOLD, -1);
    let uuid = packets::offline_uuid(&start.name);
    let profile = mc_core::PlayerProfile { uuid, username: start.name.clone() };

    // 3. Login Success（clientbound `0x02`：UUID + name）。
    let mut body = Vec::new();
    packets::LoginSuccess { uuid, username: profile.username.clone() }.encode_body(&mut body);
    write_packet(&mut stream, packets::LOGIN_SUCCESS_PACKET_ID, &body).await?;

    // 4. 等 Login Acknowledged（serverbound `0x03` 空包）；错包/EOF/超时都直接断开。
    match read_packet(&mut stream).await {
        Ok((packets::LOGIN_ACKNOWLEDGED_PACKET_ID, p)) => {
            packets::decode_login_acknowledged(&p).map_err(to_io_invalid)?;
        }
        Ok((other, _)) => {
            warn!("{addr} login: expected acknowledged 0x03, got {other:#04x}, disconnect");
            return Ok(());
        }
        Err(e) => {
            debug!("{addr} login: no acknowledged ({e}), disconnect");
            return Ok(());
        }
    }

    info!("login ok uuid={} name={}", uuid, profile.username);
    // 任务E调用点：只有走到这里（Login Ack 成功）才算一次真实连接，
    // Status/半开/握手失败的连接不经过这里。
    notify_login_success(tx, addr, &profile);

    // 5. socket 所有权移交给 Play（与 Status 的“走完即关”区分开）。
    enter_play(stream, profile).await?;
    Ok(())
}

/// 任务E钩子：Login 成功后的唯一真实连接上报点。
/// 有界 channel 满时 best-effort 丢弃，只记 debug，不阻塞连接任务；
/// 实际在线数由 ECS 侧排空 channel 时 `fetch_add`，网络侧不直接改计数。
pub fn notify_login_success(tx: &Sender<NewConnection>, addr: SocketAddr, profile: &mc_core::PlayerProfile) {
    if let Err(e) = tx.try_send(NewConnection(addr)) {
        debug!("login success notify dropped for {addr} ({e}): {profile:?}");
    }
}

/// 进入 Play 的钩子：拿走 socket 所有权。P2 在此实现 Configuration → Play；
/// 当前占位实现只记 log 后关闭连接（`drop`），调用形状已与 Status 区分开。
pub async fn enter_play(stream: TcpStream, profile: mc_core::PlayerProfile) -> io::Result<()> {
    info!(
        "enter_play uuid={} name={} (P2 TODO: real play pipeline)",
        profile.uuid, profile.username
    );
    drop(stream);
    Ok(())
}

/// 读一帧：`VarInt(length) + VarInt(packet_id) + payload`，返回 `(packet_id, payload)`。
async fn read_packet(stream: &mut TcpStream) -> io::Result<(i32, Vec<u8>)> {
    let res = tokio::time::timeout(READ_TIMEOUT, read_packet_inner(stream)).await;
    match res {
        Ok(r) => r,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "read packet timed out")),
    }
}

async fn read_packet_inner(stream: &mut TcpStream) -> io::Result<(i32, Vec<u8>)> {
    let len = read_varint_async(stream).await?;
    if len < 0 {
        return Err(to_io_invalid(format!("negative packet length {len}")));
    }
    if len as usize > MAX_PACKET_LEN {
        return Err(to_io_invalid(format!("packet too large: {len} bytes")));
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    let (id, n) = read_varint(&buf).map_err(to_io_invalid)?;
    Ok((id, buf[n..].to_vec()))
}

async fn read_varint_async(stream: &mut TcpStream) -> io::Result<i32> {
    let mut num = 0i32;
    let mut shift = 0u32;
    for _ in 0..5 {
        let mut b = [0u8; 1];
        stream.read_exact(&mut b).await?;
        num |= ((b[0] & 0x7F) as i32) << shift;
        if b[0] & 0x80 == 0 {
            return Ok(num);
        }
        shift += 7;
    }
    Err(to_io_invalid("varint too big"))
}

/// 写一帧（组帧复用 [`mc_protocol::packets::encode_packet`]，与单测同一份代码）。
async fn write_packet(stream: &mut TcpStream, packet_id: i32, payload: &[u8]) -> io::Result<()> {
    let mut buf = Vec::new();
    packets::encode_packet(packet_id, payload, &mut buf);
    stream.write_all(&buf).await
}

fn to_io_invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}
