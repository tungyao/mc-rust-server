//! mc-network: tokio 监听 + 包收发，只兼容26.3/777
//! 模块上限200行：config/framing/handshake_route/status_route/login_route
pub mod broadcast;
pub mod chunk_cache;
pub mod chunk_view;
pub mod config;
pub mod drop_state;
pub mod framing;
pub mod handshake_route;
pub mod login_route;
pub mod play_events;
pub mod status_route;

pub use broadcast::{OnlineEntry, ADMIN_KICK_PACKET_ID, BroadcastHub, QueuedPacket};
pub use chunk_cache::{
    encode_chunk_data_body, encode_chunk_data_body_mutable, ChunkCache, ChunkCacheStats,
    DEFAULT_CHUNK_CACHE_CAPACITY,
};
pub use chunk_view::{
    clamp_view_radius, view_chunk_count, ChunkView, ChunkViewUpdate, MAX_VIEW_RADIUS,
    MIN_VIEW_RADIUS, PLAY_VIEW_CHUNK_COUNT, PLAY_VIEW_RADIUS,
};
pub use config::{NetConfig, STATUS_MAX_PLAYERS, STATUS_MOTD};
pub use drop_state::{DropState, FIRST_DROP_ENTITY_ID, PLAYER_ENTITY_ID, STARTER_STONE_COUNT};
pub use framing::{MAX_PACKET_LEN, READ_TIMEOUT};
pub use login_route::{
    enter_play, enter_play_with_events, enter_play_with_options, handle_login_with_options,
    notify_login_success, PlayOptions,
};
pub use play_events::PlayEvent;

use crossbeam_channel::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::TcpListener;
use tracing::{debug, info};

/// accept 时分配的单连接身份：ECS 实体键，避免仅用 addr 时旧断线误删重连新实体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewConnection {
    pub addr: std::net::SocketAddr,
    pub conn_id: u64,
}

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

/// 全局单调连接 id（serve / 旧登录入口共用；0 保留为“未知”）。
pub fn next_conn_id() -> u64 {
    NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// 监听入口：全服唯一掉落物理 ticker 在此启动一次（启动逻辑保持不变）。
/// `DropState::stop_global_tick` 是给测试/生命周期管理用的显式停止句柄，生产入口不调用它。
/// 相关回归均为服务端状态级验证；真机手感 T1–T13 仍需人工确认。
pub async fn serve(listener: TcpListener, tx: Sender<NewConnection>, config: NetConfig) -> Result<(), NetError> {
    info!("listening on {:?}", listener.local_addr()?);
    // P18 掉落物理：全服唯一 ticker 在这里启动一次（50ms）。各连接的 play_loop
    // 只做拾取/视距同步，绝不调用 ItemEntities::tick()，否则 N 连接 = N 倍速物理。
    // 独立 std::thread，不依赖 tokio runtime；DropState::new 在单测里依然无副作用。
    if let (Some(drops), Some(world)) = (&config.drops, &config.mutable_world) {
        // P22 生长广播/缓存失效：hub 与 cache 齐备时注入 DropState，生长变更才能
        // fan-out 到视距内连接并使 chunk 缓存失效；缺谁都不影响落子本身。
        if let Some(hub) = &config.hub {
            drops.set_hub(Arc::clone(hub));
        }
        if let Some(cache) = &config.chunk_cache {
            drops.set_cache(Arc::clone(cache));
        }
        // P22：重启后把存量小麦重新纳入生长（内存跟踪丢失，override 还在）。
        drops.adopt_existing_crops(world);
        if drops.start_global_tick(Arc::clone(world)) {
            info!("global drop physics ticker started ({}ms)", drop_state::DROP_TICK_MS);
        } else {
            debug!("global drop physics ticker already running");
        }
    }
    loop {
        let (socket, addr) = listener.accept().await?;
        let conn_id = next_conn_id();
        info!("accept {addr} conn_id={conn_id}");
        let tx = tx.clone();
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(e) = handshake_route::handle_connection(socket, addr, conn_id, tx, config).await {
                debug!("connection {addr} conn_id={conn_id} closed: {e}");
            }
        });
    }
}
