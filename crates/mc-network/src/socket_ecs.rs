//! socket_ecs: 方案 B —— socket I/O 所有权归 ECS。
//!
//! 线程/所有权模型（与 `docs/socket-ecs-plan.md` §2.2 一致）：
//! - `serve()` 在 ECS 模式下**只 accept**，把 `TcpStream` 交给本模块的连接登记通道，
//!   不再 `tokio::spawn` 每连接任务；登记与任务句柄由 ECS 的 `SocketEcsRuntime` 资源拥有。
//! - Bevy system 每 tick 调 `drain_accept()`（只做 try_recv + spawn，不阻塞 50ms Update）。
//! - 每个连接有三个 ECS 拥有的任务：reader（非阻塞读帧字节 → Inbound 队列）、
//!   writer（**唯一**从 Outbound 队列 drain 并写真实 socket）、logic（登录/配置/Play 权威判定，
//!   把要发的包编码进该玩家唯一的 Outbound 队列）。
//! - 写队列用**无限队列**（用户 P52 明确选择）：不丢装饰包、不踢慢客户端。
//!   **风险**：慢客户端可使 `out_tx` 队列无界增长，最终 OOM；监控点是
//!   `queued_bytes_total` / `active_connections` / `retired_connections` 计数与
//!   `mc-server` 的 `socket_ecs` 日志。生产真机必须观察慢连接内存增长；本版本不做踢人。
//! - 包序：logic 是本人 Outbound 的唯一生产者，writer 单任务 FIFO drain；
//!   广播包经 `BroadcastHub` 进**接收方**队列，Ack 从不入 hub。Join 八段包序、视距 flush
//!   先于方块/掉落、BlockUpdate→Ack 相邻沿用既有编码路径，逐字节不变。
//! - 全局 50ms 物理 ticker 仍只在 `serve()` 启动一次；本模块绝不调用 `items.tick()`。

use crate::config::NetConfig;
use crate::handshake_route::handle_connection;
use crate::{NewConnection, NetError};
use crossbeam_channel::{Receiver as CbReceiver, Sender as CbSender};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::{AbortHandle, JoinHandle};
use tracing::{debug, info};

/// 启动期锁定的 socket 归属；不做运行时热切（避免同进程两套写路径导致 Ack 双生产者）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketMode {
    /// 紧急回退：旧 net 线程每连接任务持 socket，逐字节等于历史基线。
    Net,
    /// 生产默认：socket I/O 全归 ECS 运行时（本模块）。
    Ecs,
}

impl SocketMode {
    pub const fn as_str(self) -> &'static str {
        match self { Self::Net => "net", Self::Ecs => "ecs" }
    }
}

impl Default for SocketMode {
    fn default() -> Self { Self::Net }
}

impl std::str::FromStr for SocketMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "net" => Ok(Self::Net),
            "ecs" => Ok(Self::Ecs),
            other => Err(format!("unknown socket_mode {other:?} (expected net|ecs)")),
        }
    }
}

/// accept 后交给 ECS 登记的连接（连接身份在 accept 处单调分配，不复用）。
#[derive(Debug)]
pub struct AcceptedSocket {
    pub addr: SocketAddr,
    pub conn_id: u64,
    pub stream: TcpStream,
}

/// ECS 侧的虚拟 socket：对上层登录/Play 编码逻辑表现为普通异步字节流，
/// 实际读写都经过 ECS 拥有的 Inbound/Outbound 队列。
pub struct EcsSocket {
    inbound: UnboundedReceiver<Vec<u8>>,
    outbound: UnboundedSender<Vec<u8>>,
    cur: Vec<u8>,
    off: usize,
    queued_bytes: Arc<AtomicU64>,
}

impl AsyncRead for EcsSocket {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        loop {
            let this = &mut *self;
            if this.off < this.cur.len() {
                let n = std::cmp::min(buf.remaining(), this.cur.len() - this.off);
                let off = this.off;
                buf.put_slice(&this.cur[off..off + n]);
                this.off += n;
                return Poll::Ready(Ok(()));
            }
            this.cur.clear();
            this.off = 0;
            match Pin::new(&mut this.inbound).poll_recv(cx) {
                Poll::Ready(Some(bytes)) => {
                    this.cur = bytes;
                }
                Poll::Ready(None) => return Poll::Ready(Ok(())), // EOF：上层按断开处理
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl AsyncWrite for EcsSocket {
    fn poll_write(mut self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        // 无限队列：永不阻塞、永不丢包、永不踢慢客户端（风险见模块注释）。
        this.queued_bytes.fetch_add(buf.len() as u64, Ordering::Relaxed);
        match this.outbound.send(buf.to_vec()) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "ecs writer closed"))),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // writer 在 Outbound channel 关闭后自然 shutdown 真实 socket。
        Poll::Ready(Ok(()))
    }
}

struct ConnHandles {
    #[allow(dead_code)]
    tasks: Vec<AbortHandle>,
    #[allow(dead_code)]
    outbound: UnboundedSender<Vec<u8>>,
}

/// Bevy Resource：ECS 拥有全部连接登记、任务句柄与写队列生产权。
pub struct SocketEcsRuntime {
    incoming_rx: CbReceiver<AcceptedSocket>,
    incoming_tx: CbSender<AcceptedSocket>,
    handle: tokio::runtime::Handle,
    config: NetConfig,
    new_tx: crossbeam_channel::Sender<NewConnection>,
    conns: Arc<Mutex<HashMap<u64, ConnHandles>>>,
    accepted: AtomicU64,
    registered: AtomicU64,
    retired: Arc<AtomicU64>,
    queued_bytes_total: Arc<AtomicU64>,
}

impl SocketEcsRuntime {
    /// 由 mc-server 在 Bevy App 组装时创建；`handle` 是 mc-net tokio runtime 的句柄，
    /// 所有连接任务由 ECS 在该 runtime 上 spawn（Bevy system 自身不 await、不做 IO）。
    pub fn new(
        handle: tokio::runtime::Handle,
        config: NetConfig,
        new_tx: crossbeam_channel::Sender<NewConnection>,
    ) -> Self {
        let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded();
        Self {
            incoming_rx,
            incoming_tx,
            handle,
            config,
            new_tx,
            conns: Arc::new(Mutex::new(HashMap::new())),
            accepted: AtomicU64::new(0),
            registered: AtomicU64::new(0),
            retired: Arc::new(AtomicU64::new(0)),
            queued_bytes_total: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 交给 `serve()` 的 accept 通道；`NetConfig::with_socket_accept` 透传。
    pub fn incoming_sender(&self) -> CbSender<AcceptedSocket> {
        self.incoming_tx.clone()
    }

    /// 只读诊断计数（供日志/真机对照；面板通过 `/api/world` 只读展示模式）。
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (
            self.accepted.load(Ordering::Relaxed),
            self.registered.load(Ordering::Relaxed),
            self.retired.load(Ordering::Relaxed),
            self.queued_bytes_total.load(Ordering::Relaxed),
        )
    }

    pub fn active_connections(&self) -> usize {
        self.conns.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Bevy Update 内的非阻塞登记：只 try_recv + spawn，绝不等待 socket。
    pub fn drain_accept(&self) {
        while let Ok(accepted) = self.incoming_rx.try_recv() {
            self.accepted.fetch_add(1, Ordering::Relaxed);
            self.register(accepted);
        }
    }

    /// 注册单个连接：reader / writer / logic 三个任务句柄保存在 ECS 资源中。
    pub fn register(&self, accepted: AcceptedSocket) {
        let AcceptedSocket { stream, addr, conn_id } = accepted;
        if self.conns.lock().map(|m| m.contains_key(&conn_id)).unwrap_or(false) {
            debug!("socket_ecs duplicate conn_id={conn_id} ignored");
            return;
        }
        let (in_tx, in_rx) = unbounded_channel::<Vec<u8>>();
        let (out_tx, out_rx) = unbounded_channel::<Vec<u8>>();
        let (read_half, write_half) = stream.into_split();

        // reader：只搬运字节，不判定、不发包；EOF/错误时 drop sender 让上层读 EOF。
        let reader: JoinHandle<()> = self.handle.spawn(async move {
            let mut read_half = read_half;
            let mut buf = [0u8; 16 * 1024];
            loop {
                match read_half.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if in_tx.send(buf[..n].to_vec()).is_err() { break; }
                    }
                }
            }
        });

        // writer：本连接唯一写者，FIFO 排空无限队列；channel 关闭才 shutdown。
        let writer: JoinHandle<()> = self.handle.spawn(async move {
            let mut write_half = write_half;
            let mut rx = out_rx;
            while let Some(bytes) = rx.recv().await {
                if write_half.write_all(&bytes).await.is_err() { break; }
            }
            let _ = write_half.shutdown().await;
        });

        // logic：复用既有（已逐字节验证）登录/配置/Play 权威路径，唯一 Outbound 生产者。
        let socket = EcsSocket {
            inbound: in_rx,
            outbound: out_tx.clone(),
            cur: Vec::new(),
            off: 0,
            queued_bytes: Arc::clone(&self.queued_bytes_total),
        };
        let config = self.config.clone();
        let new_tx = self.new_tx.clone();
        let logic: JoinHandle<()> = self.handle.spawn(async move {
            if let Err(e) = handle_connection(socket, addr, conn_id, new_tx, config).await {
                debug!("socket_ecs logic conn_id={conn_id} closed: {e}");
            }
        });

        let tasks = vec![
            reader.abort_handle(), writer.abort_handle(), logic.abort_handle(),
        ];
        // 清理任务只负责移除句柄；Disconnect/unregister 仍由 enter_play_with_options
        // 的统一退出路径投递（唯一删除点 player_removal_system 减 online）。
        let conns = Arc::clone(&self.conns);
        let retired = Arc::clone(&self.retired);
        let _reaper: JoinHandle<()> = self.handle.spawn(async move {
            let _ = logic.await;
            if let Ok(mut m) = conns.lock() { m.remove(&conn_id); }
            retired.fetch_add(1, Ordering::Relaxed);
        });
        if let Ok(mut m) = self.conns.lock() {
            m.insert(conn_id, ConnHandles { tasks, outbound: out_tx });
        }
        self.registered.fetch_add(1, Ordering::Relaxed);
        info!("socket_ecs register addr={addr} conn_id={conn_id} (ecs writer, unbounded outbound)");
    }

}

/// 把 `AcceptedSocket` 交给 ECS 运行时（供 `serve()` 在 ecs 模式下调用）。
pub fn handoff_to_ecs(tx: &CbSender<AcceptedSocket>, accepted: AcceptedSocket) -> Result<(), NetError> {
    tx.send(accepted).map_err(|e| NetError::EcsHandoff(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_mode_str_and_parse() {
        assert_eq!(SocketMode::Net.as_str(), "net");
        assert_eq!(SocketMode::Ecs.as_str(), "ecs");
        assert_eq!("ecs".parse::<SocketMode>().unwrap(), SocketMode::Ecs);
        assert_eq!("NET".parse::<SocketMode>().unwrap(), SocketMode::Net);
        assert!("bogus".parse::<SocketMode>().is_err());
        assert_eq!(SocketMode::default(), SocketMode::Net, "library default stays net for byte-exact tests");
    }
}
