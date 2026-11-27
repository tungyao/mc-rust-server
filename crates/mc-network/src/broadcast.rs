//! broadcast: per-player 写通道 hub（P21 多人同服可见），只兼容 26.3/777。
//!
//! 设计（HANDOFF §5.1 的 per-player 写通道方向）：socket 仍留在 net 线程、每个
//! `play_loop` 独占自己的 `TcpStream`、ECS 依然不写 socket。新增一个全服共享的
//! `BroadcastHub`（`conn_id → 发送端 + 位置/视距快照`）：
//!
//! - A 的权威判定线程（仍是 A 自己的 `play_loop`）把 `BlockUpdate` / 掉落
//!   `Spawn`+`Data` / `Remove` / `TakeItem` 按原始包体投递给**视距内其它连接**的
//!   队列；各连接的 `play_loop` 在 tick 间隙排空自己队列写入自己 socket。
//! - `Ack` 只回操作者本人，绝不广播（客户端序列预测是 per-connection 的）。
//! - 视距用**接收方**的 `last_pos` + `radius`（切比雪夫，含 `ChunkView` 同口径）；
//!   跨区块移动后 `update_pos` 即时更新，广播集自然跟随，无需等节流 flush。
//! - 锁粒度：单把 `RwLock<HashMap>` 只做注册/注销/位置更新与接收集快照；
//!   广播先在读锁内拷贝目标发送端，丢锁后再 `try_send`（无界队列，永不阻塞
//!   判定线程）；发送失败（对端已走）只跳过，真正的清理在 `enter_play_with_options`
//!   退出路径与 `Disconnect` 唯一删除点汇合（`unregister`），不留幽灵通道。
//!
//! 不同步（MVP 外，留 TODO）：挖掘动画 `Set Block Destroy Stage 0x05`（P43 仍未做）。
//!
//! P43 玩家实体互见：每个注册连接可额外登记一个“对外玩家实体”（`entity_id` 为
//! 独立分配的高位唯一 id，自己客户端仍是 LoginPlay 的 1）。hub 同时保存
//! `conn_id → 玩家实体快照` 与 `viewer_conn_id → 已 Spawn 给该视者的玩家 id 集`，
//! `player_visibility` 模块据此做 Spawn / 位置 / Remove 的视距同步；开关由
//! `PlayOptions::player_visibility` 控制，旧入口默认关闭（旧包流逐字节不变）。

use mc_core::ChunkPos;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Instant;

/// 排队给其它连接 `play_loop` 代写的原始 S2C 包（包 ID + 未压缩包体；
///
/// 接收方用自己的 `threshold` 经 `write_packet_any` 写出）。
#[derive(Debug, Clone)]
pub struct QueuedPacket {
    pub id: i32,
    pub body: Vec<u8>,
}

/// P47 游戏模式：默认生存；创造模式即时破坏不扣物品、放置不消耗手持。
/// 飞行未做（Player Abilities 0x41 包体未抓包验证，不猜字节，见 HANDOFF）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
}

impl GameMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Creative => "creative",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "survival" | "survive" | "0" => Some(Self::Survival),
            "creative" | "creativity" | "1" => Some(Self::Creative),
            _ => None,
        }
    }
}

/// P54 A3：合并队列的键 = (chunk_x, chunk_z, section_y, x, y, z)；
/// 只在同 chunk 同 section 的**同一格**上合并 `BlockUpdate`，绝不合 Ack/其它包。
type MergeKey = (i32, i32, i16, i32, i32, i32);

#[derive(Default)]
struct PendingOutbox {
    queue: VecDeque<QueuedPacket>,
    /// MergeKey → queue 下标（替换后包序位置保持不变）。
    index: HashMap<MergeKey, usize>,
}

struct HubConn {
    tx: crossbeam_channel::Sender<QueuedPacket>,
    /// P54 A3：默认空；只有 `throttle` 打开时 `broadcast` 才写这里。
    pending: Mutex<PendingOutbox>,
    radius: i32,
    gamemode: GameMode,
    pos: (f64, f64, f64),
    /// P25 管理面板：Join 完成时的用户名（大小写精确匹配即显示名）。
    username: String,
    /// P51：稳定身份 UUID 字符串（离线用户名派生；旧 `register` 入口为空串）。
    uuid: String,
    /// P25 管理面板：对端地址（`SocketAddr` 字符串形式，仅展示/诊断用）。
    addr: String,
    /// P25 管理面板：注册时刻（`snapshot` 由此算在线时长）。
    since: Instant,
}

/// P25 管理面板在线条目（`snapshot` 快照的一行；`pos` 为 hub 记录的最近位置）。
#[derive(Debug, Clone)]
pub struct OnlineEntry {
    pub conn_id: u64,
    pub username: String,
    /// P51：稳定 UUID（规范小写字符串）；旧注册入口为空串。
    pub uuid: String,
    pub addr: String,
    pub pos: (f64, f64, f64),
    pub radius: i32,
    /// P47 新增字段：旧字段不变，只增 gamemode。
    pub gamemode: GameMode,
    /// 注册至今的秒数（快照时刻计算）。
    pub online_secs: u64,
}

/// P25 管理面板踢人信号：不是真实包 ID（真实包 ID 均为非负 varint），
/// `play_loop` 在 `drain_outbox` 看到它即主动退出走统一清理路径
/// （`unload`/`unregister`/`Disconnect`，与断线完全一致）。
/// 这就是计划 §6.4 的备选"admin 指令通道"：不断线猜包、不手写踢出包体。
pub const ADMIN_KICK_PACKET_ID: i32 = -1;

/// P54 A3：每连接合并队列默认上限（纯服务端背压参数）。
pub const DEFAULT_BROADCAST_MAX_QUEUE: usize = 256;

/// P43：一个连接对外的玩家实体快照（位置/朝向随 C2S 移动包更新）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerEntityState {
    /// 对外唯一实体 id（不等于 1，也不与掉落/经验球 id 冲突）。
    pub entity_id: i32,
    pub uuid: u128,
    pub pos: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
}

/// 全服共享的广播 hub（`Arc` 进 `NetConfig`/`PlayOptions`）。
pub struct BroadcastHub {
    conns: RwLock<HashMap<u64, HubConn>>,
    /// P54 A3：限流开关（默认关，改包序有风险，等真机 §5 双开包流确认）。
    throttle: AtomicBool,
    /// 每个接收连接最多排多少个包；满后**新包**丢弃（BlockUpdate 合并不占新槽）。
    max_queue: AtomicUsize,
    queued: AtomicU64,
    dropped: AtomicU64,
    merged_blocks: AtomicU64,
    /// P43：`owner_conn_id → 该玩家对外的实体快照`；只在开关打开时登记。
    players: RwLock<HashMap<u64, PlayerEntityState>>,
    /// P43：`viewer_conn_id → 已发给该视者的玩家实体 id`（独立的 known 集，
    /// 绝不复用 `DropState::known`，后者只跟踪 item/orb，差集兜底会误删玩家）。
    player_known: RwLock<HashMap<u64, HashSet<i32>>>,
}

impl std::fmt::Debug for BroadcastHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BroadcastHub").field("len", &self.len()).finish()
    }
}

impl BroadcastHub {
    pub fn new() -> Self {
        Self {
            conns: RwLock::new(HashMap::new()),
            throttle: AtomicBool::new(false),
            max_queue: AtomicUsize::new(DEFAULT_BROADCAST_MAX_QUEUE),
            queued: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            merged_blocks: AtomicU64::new(0),
            players: RwLock::new(HashMap::new()),
            player_known: RwLock::new(HashMap::new()),
        }
    }

    /// P54 A3：默认合并队列上限（纯服务端参数，可由 admin 调整）。
    pub const DEFAULT_MAX_QUEUE: usize = 256;

    /// 当前在线连接数（测试/诊断用）。
    pub fn len(&self) -> usize {
        self.conns.read().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub fn contains(&self, conn_id: u64) -> bool {
        self.conns.read().unwrap_or_else(|p| p.into_inner()).contains_key(&conn_id)
    }

    /// 视者自己的位置快照与半径（玩家实体视距判定用）。
    pub fn conn_pos(&self, conn_id: u64) -> Option<((f64, f64, f64), i32)> {
        self.conns.read().unwrap_or_else(|p| p.into_inner())
            .get(&conn_id).map(|c| (c.pos, c.radius))
    }

    /// 注册连接的写队列（Join 序列完成后、进 `play_loop` 前调用，保证接收方
    /// 已持有完整视距区块，广播包不会排在区块包前面）。
    pub fn register(
        &self,
        conn_id: u64,
        tx: crossbeam_channel::Sender<QueuedPacket>,
        radius: i32,
        pos: (f64, f64, f64),
    ) {
        self.register_with_name(conn_id, tx, radius, pos, String::new(), String::new());
    }

    /// P25：带用户名/地址的注册（Play Join 完成后调用；旧 `register` 保留给旧单测）。
    /// 无 UUID 信息，快照的 `uuid` 为空串（旧调用方语义不变）。
    pub fn register_with_name(
        &self,
        conn_id: u64,
        tx: crossbeam_channel::Sender<QueuedPacket>,
        radius: i32,
        pos: (f64, f64, f64),
        username: String,
        addr: String,
    ) {
        self.register_with_uuid(conn_id, tx, radius, pos, username, addr, String::new());
    }

    /// P51：带稳定 UUID 的注册（生产入口使用；用户名仍保留展示/兼容）。
    pub fn register_with_uuid(
        &self,
        conn_id: u64,
        tx: crossbeam_channel::Sender<QueuedPacket>,
        radius: i32,
        pos: (f64, f64, f64),
        username: String,
        addr: String,
        uuid: String,
    ) {
        self.conns
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                conn_id,
                HubConn {
                    tx,
                    pending: Mutex::new(PendingOutbox::default()),
                    radius,
                    pos,
                    gamemode: GameMode::Survival,
                    username,
                    uuid,
                    addr,
                    since: Instant::now(),
                },
            );
    }

    /// P25：在线快照（读锁内只做内存拷贝，丢锁后算时长；调用方拿到快照后再
    /// `unicast`/`broadcast`，绝不在持 hub 读锁时调它们，见计划 §6.2）。
    pub fn snapshot(&self) -> Vec<OnlineEntry> {
        let now = Instant::now();
        self.conns
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|(id, c)| OnlineEntry {
                conn_id: *id,
                username: c.username.clone(),
                uuid: c.uuid.clone(),
                addr: c.addr.clone(),
                pos: c.pos,
                radius: c.radius,
                gamemode: c.gamemode,
                online_secs: now.duration_since(c.since).as_secs(),
            })
            .collect()
    }

    /// P25：按用户名精确匹配（大小写敏感）找 conn_id；重名/离线返回 `None`。
    /// 离线模式同名即同一身份：先后上线只会留一个 hub 条目（旧连接退出即注销），
    /// 同时在线的同名连接取 conn_id 最大者（最新注册）。
    pub fn find_conn_by_name(&self, username: &str) -> Option<u64> {
        self.conns
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|(_, c)| c.username == username)
            .map(|(id, _)| *id)
            .max()
    }

    /// P51：按稳定 UUID 查在线连接（大小写/连字符归一，按 UUID 值比较）。
    /// 旧注册入口的 UUID 为空串，不会被误匹配。同时在线同一身份取最新 conn_id。
    pub fn find_conn_by_uuid(&self, uuid: &str) -> Option<u64> {
        let wanted = uuid::Uuid::parse_str(uuid.trim()).ok()?;
        self.conns
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|(_, c)| uuid::Uuid::parse_str(&c.uuid).ok() == Some(wanted))
            .map(|(id, _)| *id)
            .max()
    }

    /// 注销连接（`enter_play_with_options` 退出路径，与 `Disconnect` 投递同点，
    /// 覆盖所有退出分支；重复注销无害）。
    pub fn unregister(&self, conn_id: u64) {
        self.conns.write().unwrap_or_else(|p| p.into_inner()).remove(&conn_id);
        // P43：与该连接绑定的玩家实体/known 一并清理（先摘掉其它视者 known 里的
        // 残留 id，调用方在此之前已广播 RemoveEntities，这里只做状态收尾）。
        // 先放掉 players 写锁再拿 player_known 写锁（同 `enable_player` 的锁序，避免嵌套）。
        let removed = self.players.write().unwrap_or_else(|p| p.into_inner()).remove(&conn_id);
        if let Some(state) = removed {
            for ids in self.player_known.write().unwrap_or_else(|p| p.into_inner()).values_mut() {
                ids.remove(&state.entity_id);
            }
        }
        self.player_known.write().unwrap_or_else(|p| p.into_inner()).remove(&conn_id);
    }

    // -----------------------------------------------------------------------
    // P43 玩家实体互见：登记/快照/per-viewer known 集。
    // -----------------------------------------------------------------------

    /// 登记一个连接的对外玩家实体（Join 完成后、视距区块已下发时调用）。
    pub fn enable_player(&self, conn_id: u64, state: PlayerEntityState) {
        self.players.write().unwrap_or_else(|p| p.into_inner()).insert(conn_id, state);
        self.player_known.write().unwrap_or_else(|p| p.into_inner()).entry(conn_id).or_default();
    }

    pub fn player_state(&self, conn_id: u64) -> Option<PlayerEntityState> {
        self.players.read().unwrap_or_else(|p| p.into_inner()).get(&conn_id).copied()
    }

    /// 全部在线玩家快照（视者同步用；读锁内只做 Copy 集合，丢锁后再发包）。
    pub fn player_states(&self) -> Vec<(u64, PlayerEntityState)> {
        self.players.read().unwrap_or_else(|p| p.into_inner()).iter().map(|(&id, &s)| (id, s)).collect()
    }

    /// 移动包后更新本连接的对外玩家姿态（不改视距条目，避免死锁）。
    pub fn update_player_pose(&self, conn_id: u64, pos: (f64, f64, f64), yaw: f32, pitch: f32) {
        if let Some(s) = self.players.write().unwrap_or_else(|p| p.into_inner()).get_mut(&conn_id) {
            s.pos = pos;
            s.yaw = yaw;
            s.pitch = pitch;
        }
    }

    pub fn is_player_known(&self, viewer: u64, entity_id: i32) -> bool {
        self.player_known.read().unwrap_or_else(|p| p.into_inner())
            .get(&viewer).map(|s| s.contains(&entity_id)).unwrap_or(false)
    }

    /// 首次发给该视者时返回 `true`（去重：重复 Join/同 tick 补发只发一次 Spawn）。
    pub fn mark_player_known(&self, viewer: u64, entity_id: i32) -> bool {
        self.player_known.write().unwrap_or_else(|p| p.into_inner())
            .entry(viewer).or_default().insert(entity_id)
    }

    pub fn unmark_player_known(&self, viewer: u64, entity_id: i32) {
        if let Some(ids) = self.player_known.write().unwrap_or_else(|p| p.into_inner()).get_mut(&viewer) {
            ids.remove(&entity_id);
        }
    }

    pub fn player_known_ids(&self, viewer: u64) -> Vec<i32> {
        self.player_known.read().unwrap_or_else(|p| p.into_inner())
            .get(&viewer).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    /// 全部已登记视者的 conn_id（断开广播 Remove 用）。
    pub fn conn_ids(&self) -> Vec<u64> {
        self.conns.read().unwrap_or_else(|p| p.into_inner()).keys().copied().collect()
    }

    /// 更新接收视距快照（每个移动包后调用；只改本连接条目，写锁极短）。
    pub fn update_pos(&self, conn_id: u64, pos: (f64, f64, f64)) {
        if let Some(c) = self.conns.write().unwrap_or_else(|p| p.into_inner()).get_mut(&conn_id) {
            c.pos = pos;
        }
    }

    /// P47：设置本连接游戏模式（管理面板调用）；离线返回 false。
    pub fn set_gamemode(&self, conn_id: u64, mode: GameMode) -> bool {
        if let Some(c) = self.conns.write().unwrap_or_else(|p| p.into_inner()).get_mut(&conn_id) {
            c.gamemode = mode;
            true
        } else {
            false
        }
    }

    /// P47：读取连接游戏模式（无 hub 的旧入口按生存处理，由调用方 `unwrap_or_default`）。
    pub fn gamemode(&self, conn_id: u64) -> GameMode {
        self.conns
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&conn_id)
            .map(|c| c.gamemode)
            .unwrap_or_default()
    }

    /// 事件所在区块视距内的其它连接（快照；调用方再逐个 `unicast` 并维护
    /// `DropState::known`，避免本文件依赖掉落状态）。
    pub fn recipients_in_view(&self, except: u64, chunk: ChunkPos) -> Vec<u64> {
        self.conns
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|(id, c)| {
                **id != except && chunk_in_view(chunk_of_pos(c.pos), c.radius, chunk)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// 向单个连接排队（对端已注销/队列异常只返回 `false`，调用方跳过）。
    pub fn unicast(&self, conn_id: u64, id: i32, body: Vec<u8>) -> bool {
        let tx = self
            .conns
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&conn_id)
            .map(|c| c.tx.clone());
        match tx {
            Some(tx) => tx.try_send(QueuedPacket { id, body }).is_ok(),
            None => false,
        }
    }

    /// P25 踢人：向目标连接排一个踢出信号，其 `play_loop` 在下个节拍（≤50ms）
    /// 看到后主动退出，走与断线完全一致的清理路径。返回 `false` = 已离线。
    pub fn kick(&self, conn_id: u64) -> bool {
        self.unicast(conn_id, ADMIN_KICK_PACKET_ID, Vec::new())
    }

    /// 向事件区块视距内的其它连接各排队一份（`except` 本人不收；`Ack` 不要走这里）。
    ///
    /// P54 A3：默认关——逐包走原 crossbeam 队列，包流逐字节不变；开启后改为
    /// per-conn `VecDeque`，同 tick 内同 chunk 同 section 的 `BlockUpdate` 合并
    /// 为最后一格状态，其它包（尤其 Ack，虽然 Ack 本来就不进 hub）原样排队。
    /// `section_blocks_update 0x56` 的 wire format 未对照确认，**不使用**；只合并 0x08。
    pub fn broadcast(&self, except: u64, chunk: ChunkPos, id: i32, body: &[u8]) {
        if self.throttle_enabled() {
            let targets: Vec<u64> = {
                let guard = self.conns.read().unwrap_or_else(|p| p.into_inner());
                guard
                    .iter()
                    .filter(|(cid, c)| **cid != except && chunk_in_view(chunk_of_pos(c.pos), c.radius, chunk))
                    .map(|(cid, _)| *cid)
                    .collect()
            };
            for cid in targets {
                self.queue_throttled(cid, QueuedPacket { id, body: body.to_vec() });
            }
            return;
        }
        // 读锁内只拷贝发送端，丢锁后再发送：判定线程永不阻塞，锁粒度最小。
        let targets: Vec<crossbeam_channel::Sender<QueuedPacket>> = {
            let guard = self.conns.read().unwrap_or_else(|p| p.into_inner());
            guard
                .iter()
                .filter(|(cid, c)| **cid != except && chunk_in_view(chunk_of_pos(c.pos), c.radius, chunk))
                .map(|(_, c)| c.tx.clone())
                .collect()
        };
        for tx in targets {
            let _ = tx.try_send(QueuedPacket { id, body: body.to_vec() });
        }
    }

    // --- P54 A3：广播限流（默认关；纯服务端参数） ---

    pub fn throttle_enabled(&self) -> bool {
        self.throttle.load(Ordering::Relaxed)
    }

    pub fn set_throttle(&self, enabled: bool) {
        self.throttle.store(enabled, Ordering::Relaxed);
        if !enabled {
            // 关掉时把残留合并队列放回原通道，避免吞包；顺序保持排队顺序。
            let ids = self.conn_ids();
            for cid in ids {
                for pkt in self.drain_throttled(cid) {
                    let _ = self.unicast(cid, pkt.id, pkt.body);
                }
            }
        }
    }

    pub fn max_queue(&self) -> usize {
        self.max_queue.load(Ordering::Relaxed)
    }

    /// 合并队列上限（0 视为 1）。设置后不会丢已排队的包。
    pub fn set_max_queue(&self, n: usize) {
        self.max_queue.store(n.max(1), Ordering::Relaxed);
    }

    pub fn throttle_stats(&self) -> (u64, u64, u64) {
        (
            self.queued.load(Ordering::Relaxed),
            self.dropped.load(Ordering::Relaxed),
            self.merged_blocks.load(Ordering::Relaxed),
        )
    }

    /// 给单个接收连接排一个（可能合并的）包；只处理 `BlockUpdate`。
    fn queue_throttled(&self, conn_id: u64, pkt: QueuedPacket) {
        let key = if pkt.id == mc_protocol::packets::PLAY_BLOCK_UPDATE_S2C_ID {
            mc_protocol::packets::BlockUpdate::decode_body(&pkt.body).ok().map(|(u, _)| {
                (
                    mc_world::chunk_of_block(u.x, u.z).0,
                    mc_world::chunk_of_block(u.x, u.z).1,
                    ((u.y - mc_world::MIN_Y) / 16) as i16,
                    u.x, u.y, u.z,
                )
            })
        } else {
            None
        };
        // 单把读锁只用于定位连接；发包/排空绝不持它（计划 §6.2）。
        let guard = self.conns.read().unwrap_or_else(|p| p.into_inner());
        let Some(conn) = guard.get(&conn_id) else { return };
        let mut q = conn.pending.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(key) = key {
            if let Some(&idx) = q.index.get(&key) {
                // 同 section 同格只保留最后一次状态；不新增队列项。
                if let Some(slot) = q.queue.get_mut(idx) {
                    *slot = pkt;
                }
                self.merged_blocks.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        if q.queue.len() >= self.max_queue() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let next_index = q.queue.len();
        if let Some(key) = key {
            q.index.insert(key, next_index);
        }
        q.queue.push_back(pkt);
        self.queued.fetch_add(1, Ordering::Relaxed);
    }

    /// 由 `play_loop` 的 50ms 节拍排空合并队列（视距 flush 已先于本函数调用）。
    pub fn drain_throttled(&self, conn_id: u64) -> Vec<QueuedPacket> {
        let guard = self.conns.read().unwrap_or_else(|p| p.into_inner());
        let Some(conn) = guard.get(&conn_id) else { return Vec::new() };
        let mut q = conn.pending.lock().unwrap_or_else(|p| p.into_inner());
        let out: Vec<QueuedPacket> = q.queue.drain(..).collect();
        q.index.clear();
        drop(q);
        drop(guard);
        if !out.is_empty() {
            self.queued.fetch_sub(out.len() as u64, Ordering::Relaxed);
        }
        out
    }
}

/// 玩家实体的视距区块判定与方块/掉落同口径（供 player_visibility 复用）。
pub(crate) fn chunk_of_pos(pos: (f64, f64, f64)) -> ChunkPos {
    // 与 `ChunkView::chunk_of_block` 同口径（floor + div_euclid），非有限坐标钳制。
    let bx = pos.0.floor().clamp(-30_000_000.0, 30_000_000.0) as i32;
    let bz = pos.2.floor().clamp(-30_000_000.0, 30_000_000.0) as i32;
    ChunkPos(bx.div_euclid(16), bz.div_euclid(16))
}

pub(crate) fn chunk_in_view(center: ChunkPos, radius: i32, chunk: ChunkPos) -> bool {
    (chunk.0 - center.0).abs() <= radius && (chunk.1 - center.1).abs() <= radius
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub_with(a_pos: (f64, f64, f64), b_pos: (f64, f64, f64)) -> (BroadcastHub, crossbeam_channel::Receiver<QueuedPacket>, crossbeam_channel::Receiver<QueuedPacket>) {
        let hub = BroadcastHub::new();
        let (atx, arx) = crossbeam_channel::unbounded();
        let (btx, brx) = crossbeam_channel::unbounded();
        hub.register(1, atx, 2, a_pos);
        hub.register(2, btx, 2, b_pos);
        (hub, arx, brx)
    }

    #[test]
    fn broadcast_reaches_viewer_in_range_but_not_far_connection() {
        // A(1) 与 B(2) 同在出生区块；事件也在出生区块。
        let (hub, arx, brx) = hub_with((369.5, 73.0, 381.5), (373.5, 73.0, 381.5));
        // 远端 C 在 100 格外（半径 2 视距外）。
        let (ctx, crx) = crossbeam_channel::unbounded();
        hub.register(3, ctx, 2, (600.5, 73.0, 700.5));
        assert_eq!(hub.len(), 3);
        let chunk = ChunkPos(23, 23);
        hub.broadcast(1, chunk, 0x08, &[1, 2, 3]);
        // A（操作者）不收；B 收到；C 收不到。
        assert!(arx.try_recv().is_err());
        let got = brx.try_recv().unwrap();
        assert_eq!((got.id, got.body), (0x08, vec![1, 2, 3]));
        assert!(crx.try_recv().is_err());
    }

    #[test]
    fn update_pos_moves_broadcast_set_and_unregister_clears() {
        let (hub, _arx, brx) = hub_with((369.5, 73.0, 381.5), (373.5, 73.0, 381.5));
        let chunk = ChunkPos(23, 23);
        // B 走远（同 C 位置）后不再收到出生区块广播。
        hub.update_pos(2, (600.5, 73.0, 700.5));
        assert_eq!(hub.recipients_in_view(1, chunk), Vec::<u64>::new());
        // B 走回后恢复。
        hub.update_pos(2, (373.5, 73.0, 381.5));
        assert_eq!(hub.recipients_in_view(1, chunk), vec![2]);
        hub.broadcast(1, chunk, 0x08, &[]);
        assert!(brx.try_recv().is_ok());
        // 注销后幽灵通道不留存：unicast 失败，recipients 为空。
        hub.unregister(2);
        assert!(!hub.contains(2));
        assert!(hub.contains(1));
        assert_eq!(hub.len(), 1);
        assert!(!hub.unicast(2, 0x08, vec![]));
        hub.unregister(2);
    }

    #[test]
    fn snapshot_reports_name_addr_pos_and_uptime() {
        let hub = BroadcastHub::new();
        let (atx, _arx) = crossbeam_channel::unbounded();
        hub.register(1, atx, 2, (369.5, 73.0, 381.5));
        // 旧 register 用户名/地址为空，快照仍可用（位置/时长正常）。
        let snap = hub.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].conn_id, 1);
        assert!(snap[0].username.is_empty());
        assert_eq!(snap[0].pos, (369.5, 73.0, 381.5));
        assert_eq!(snap[0].radius, 2);
    }

    #[test]
    fn register_with_name_snapshot_and_find_by_name() {
        let hub = BroadcastHub::new();
        let (atx, _arx) = crossbeam_channel::unbounded();
        let (btx, _brx) = crossbeam_channel::unbounded();
        hub.register_with_name(
            7,
            atx,
            4,
            (1.5, 64.0, 2.5),
            "Steve".into(),
            "127.0.0.1:50001".into(),
        );
        hub.register_with_name(9, btx, 4, (3.5, 64.0, 4.5), "Alex".into(), "127.0.0.1:50002".into());
        let mut snap = hub.snapshot();
        snap.sort_by_key(|e| e.conn_id);
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].username, "Steve");
        assert_eq!(snap[0].addr, "127.0.0.1:50001");
        assert_eq!(snap[1].username, "Alex");
        // 精确匹配：大小写敏感。
        assert_eq!(hub.find_conn_by_name("Steve"), Some(7));
        assert_eq!(hub.find_conn_by_name("steve"), None);
        assert_eq!(hub.find_conn_by_name("Nobody"), None);
        // 位置更新后快照跟随。
        hub.update_pos(7, (10.5, 70.0, 20.5));
        let e = hub.snapshot().into_iter().find(|e| e.conn_id == 7).unwrap();
        assert_eq!(e.pos, (10.5, 70.0, 20.5));
        // 注销后快照消失、按名查找同步消失（无幽灵在线）。
        hub.unregister(7);
        assert!(hub.snapshot().iter().all(|e| e.conn_id != 7));
        assert_eq!(hub.find_conn_by_name("Steve"), None);
    }

    /// P51：UUID 注册/快照/查找（旧用户名入口仍返回空 uuid）。
    #[test]
    fn register_with_uuid_snapshot_and_find_by_uuid() {
        let hub = BroadcastHub::new();
        let (atx, _arx) = crossbeam_channel::unbounded();
        let (btx, _brx) = crossbeam_channel::unbounded();
        let a_uuid = mc_protocol::packets::stable_offline_uuid("Alpha").to_string();
        let b_uuid = mc_protocol::packets::stable_offline_uuid("Beta").to_string();
        hub.register_with_uuid(
            3, atx, 2, (0.0, 64.0, 0.0), "Alpha".into(), "addr-a".into(), a_uuid.clone(),
        );
        hub.register_with_uuid(
            4, btx, 2, (0.0, 64.0, 0.0), "Beta".into(), "addr-b".into(), b_uuid.clone(),
        );
        let snap = hub.snapshot();
        assert_eq!(snap.iter().find(|e| e.conn_id == 3).unwrap().uuid, a_uuid);
        assert_eq!(snap.iter().find(|e| e.conn_id == 4).unwrap().uuid, b_uuid);
        assert_eq!(hub.find_conn_by_uuid(&a_uuid), Some(3));
        assert_eq!(hub.find_conn_by_uuid(&b_uuid), Some(4));
        assert_eq!(hub.find_conn_by_uuid(&a_uuid.to_uppercase()), Some(3), "UUID 大小写无关");
        assert_eq!(hub.find_conn_by_uuid("not-a-uuid"), None);
        assert_eq!(hub.find_conn_by_uuid(""), None);
        hub.unregister(3);
        assert_eq!(hub.find_conn_by_uuid(&a_uuid), None);
        // 旧入口没有 uuid，不会被查到，但旧字段保持可用。
        let (ctx, _crx) = crossbeam_channel::unbounded();
        hub.register_with_name(5, ctx, 2, (0.0, 64.0, 0.0), "Legacy".into(), "addr-c".into());
        assert!(hub.snapshot().iter().find(|e| e.conn_id == 5).unwrap().uuid.is_empty());
        assert_eq!(hub.find_conn_by_name("Legacy"), Some(5));
    }

    #[test]
    fn gamemode_defaults_survival_and_is_per_conn() {
        let hub = BroadcastHub::new();
        let (atx, _arx) = crossbeam_channel::unbounded();
        let (btx, _brx) = crossbeam_channel::unbounded();
        hub.register_with_name(1, atx, 2, (0.0, 64.0, 0.0), "A".into(), "x".into());
        hub.register_with_name(2, btx, 2, (0.0, 64.0, 0.0), "B".into(), "y".into());
        assert_eq!(hub.gamemode(1), GameMode::Survival);
        assert_eq!(GameMode::parse("creative"), Some(GameMode::Creative));
        assert!(hub.set_gamemode(1, GameMode::Creative));
        assert_eq!(hub.gamemode(1), GameMode::Creative);
        assert_eq!(hub.gamemode(2), GameMode::Survival);
        assert!(!hub.set_gamemode(99, GameMode::Creative));
        let snap = hub.snapshot();
        assert_eq!(snap.iter().find(|e| e.conn_id == 1).unwrap().gamemode, GameMode::Creative);
        assert_eq!(snap.iter().find(|e| e.conn_id == 2).unwrap().gamemode, GameMode::Survival);
    }

    #[test]
    fn kick_signals_only_target_and_fails_when_offline() {
        let hub = BroadcastHub::new();
        let (atx, arx) = crossbeam_channel::unbounded();
        let (btx, brx) = crossbeam_channel::unbounded();
        hub.register_with_name(1, atx, 2, (0.0, 64.0, 0.0), "A".into(), "x".into());
        hub.register_with_name(2, btx, 2, (0.0, 64.0, 0.0), "B".into(), "y".into());
        assert!(hub.kick(1));
        let pkt = arx.try_recv().unwrap();
        assert_eq!(pkt.id, super::ADMIN_KICK_PACKET_ID);
        assert!(brx.try_recv().is_err(), "kick must not disturb other connections");
        assert!(!hub.kick(999), "kick of offline conn must fail");
    }

    #[test]
    fn player_known_set_is_per_viewer_and_dedupes() {
        let hub = BroadcastHub::new();
        let (atx, _arx) = crossbeam_channel::unbounded();
        let (btx, _brx) = crossbeam_channel::unbounded();
        hub.register(1, atx, 2, (0.0, 64.0, 0.0));
        hub.register(2, btx, 2, (4.0, 64.0, 0.0));
        let a = PlayerEntityState { entity_id: 1_000_000_000, uuid: 11, pos: (0.0, 64.0, 0.0), yaw: 0.0, pitch: 0.0 };
        let b = PlayerEntityState { entity_id: 1_000_000_001, uuid: 22, pos: (4.0, 64.0, 0.0), yaw: 0.0, pitch: 0.0 };
        hub.enable_player(1, a);
        hub.enable_player(2, b);
        assert_eq!(hub.player_state(1), Some(a));
        assert!(hub.mark_player_known(2, a.entity_id));
        assert!(!hub.mark_player_known(2, a.entity_id), "second Spawn must be suppressed");
        assert!(!hub.is_player_known(1, a.entity_id), "known sets are per viewer");
        assert_eq!(hub.player_known_ids(2), vec![a.entity_id]);
        hub.unmark_player_known(2, a.entity_id);
        assert!(hub.mark_player_known(2, a.entity_id), "after Remove a re-Spawn is allowed");
        hub.update_player_pose(1, (1.0, 65.0, 2.0), 90.0, -10.0);
        assert_eq!(hub.player_state(1).unwrap().pos, (1.0, 65.0, 2.0));
        assert_eq!(hub.player_state(1).unwrap().yaw, 90.0);
    }

    #[test]
    fn unregister_clears_player_state_and_viewer_known_entries() {
        let hub = BroadcastHub::new();
        let (atx, _arx) = crossbeam_channel::unbounded();
        let (btx, _brx) = crossbeam_channel::unbounded();
        hub.register(1, atx, 2, (0.0, 64.0, 0.0));
        hub.register(2, btx, 2, (4.0, 64.0, 0.0));
        let a = PlayerEntityState { entity_id: 1_000_000_002, uuid: 33, pos: (0.0, 64.0, 0.0), yaw: 0.0, pitch: 0.0 };
        hub.enable_player(1, a);
        assert!(hub.mark_player_known(2, a.entity_id));
        hub.unregister(1);
        assert!(hub.player_state(1).is_none());
        assert!(!hub.is_player_known(2, a.entity_id));
        assert!(hub.player_known_ids(1).is_empty());
        assert!(hub.player_states().is_empty());
    }

    fn block_update_body(x: i32, y: i32, z: i32, id: i32) -> Vec<u8> {
        let mut body = Vec::new();
        mc_protocol::packets::BlockUpdate { x, y, z, block_id: id }.encode_body(&mut body);
        body
    }

    #[test]
    fn throttle_defaults_off_and_leaves_packet_stream_unchanged() {
        let (hub, _arx, brx) = hub_with((0.0, 64.0, 0.0), (1.0, 64.0, 0.0));
        assert!(!hub.throttle_enabled());
        assert_eq!(hub.max_queue(), DEFAULT_BROADCAST_MAX_QUEUE);
        assert_eq!(hub.throttle_stats(), (0, 0, 0));
        hub.broadcast(1, ChunkPos(0, 0), mc_protocol::packets::PLAY_BLOCK_UPDATE_S2C_ID, &[1, 2]);
        let got = brx.try_recv().unwrap();
        assert_eq!((got.id, got.body), (mc_protocol::packets::PLAY_BLOCK_UPDATE_S2C_ID, vec![1, 2]));
        assert!(hub.drain_throttled(2).is_empty(), "no hidden queue when disabled");
    }

    #[test]
    fn throttle_merges_only_block_update_and_acks_stay_direct() {
        let (hub, arx, brx) = hub_with((0.0, 64.0, 0.0), (1.0, 64.0, 0.0));
        hub.set_throttle(true);
        let chunk = ChunkPos(0, 0);
        let id = mc_protocol::packets::PLAY_BLOCK_UPDATE_S2C_ID;
        hub.broadcast(1, chunk, id, &block_update_body(1, 70, 1, 1));
        hub.broadcast(1, chunk, id, &block_update_body(2, 71, 1, 2));
        hub.broadcast(1, chunk, id, &block_update_body(1, 70, 1, 9));
        // 不同包（掉落 Spawn）不合并：排两项。
        hub.broadcast(1, chunk, 0x01, &[7, 7]);
        assert!(brx.try_recv().is_err(), "throttled packets wait for the 50ms drain");
        let (queued, dropped, merged) = hub.throttle_stats();
        assert_eq!((queued, dropped, merged), (3, 0, 1), "only same-position updates merge");
        // Ack 永远直连本人，不进 hub 合并队列（P52 红线）。
        assert!(hub.unicast(1, mc_protocol::packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, vec![3]));
        let ack = arx.try_recv().unwrap();
        assert_eq!(ack.id, mc_protocol::packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID);
        assert_eq!(hub.throttle_stats().2, 1, "Ack must never be merged");
        let drained = hub.drain_throttled(2);
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].id, id);
        assert_eq!(drained[0].body, block_update_body(1, 70, 1, 9), "last write wins");
        assert_eq!(drained[1].id, id);
        assert_eq!(drained[1].body, block_update_body(2, 71, 1, 2), "other position stays");
        assert_eq!(drained[2].id, 0x01);
        assert_eq!(hub.throttle_stats().0, 0);
        assert!(hub.drain_throttled(2).is_empty());
        hub.set_throttle(false);
    }

    #[test]
    fn throttle_drops_new_packets_at_queue_limit() {
        let (hub, _arx, _brx) = hub_with((0.0, 64.0, 0.0), (1.0, 64.0, 0.0));
        hub.set_throttle(true);
        hub.set_max_queue(2);
        hub.broadcast(1, ChunkPos(0, 0), 0x01, &[1]);
        hub.broadcast(1, ChunkPos(0, 0), 0x01, &[2]);
        hub.broadcast(1, ChunkPos(0, 0), 0x01, &[3]);
        assert_eq!(hub.throttle_stats(), (2, 1, 0));
        assert_eq!(hub.drain_throttled(2).len(), 2);
    }
}
