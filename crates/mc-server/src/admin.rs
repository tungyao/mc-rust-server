//! admin: P25 后台管理面板 HTTP 服务，只兼容 26.3/777。
//!
//! 挂法（计划 §5.1）：独立 `std::thread`（名 `mc-admin`）+ 阻塞式 `std::net::TcpListener`，
//! 与 25565 游戏端口完全分离，默认只听回环（`server.toml` 的 `admin_bind`）。
//! 只支持 `GET/POST` + 路径 + `Content-Length` + JSON 体；无鉴权（风险见计划 §6.1）。
//!
//! 并发守则（计划 §5.3/§6.2）：管理线程只碰三把锁的短临界区（内存拷贝 + 组包），
//! 持锁期间不碰网络/磁盘；`hub.snapshot` 先快照、丢锁后再 `unicast`；
//! `save_to_dir` 调用时调用方不持任何锁。
//!
//! M3 范围（计划 §6.4 fallback）：`clientbound.play/disconnect` 与 `system_chat` 在
//! `crates/mc-protocol` 尚无 S2C 编码器（包 ID 虽在数据表里，包体 NBT 无抓包真值），
//! 不许猜字节——
//! - 踢人走"admin 指令通道"备选：`BroadcastHub::kick` 经 outbox 发踢人信号，
//!   `play_loop` 下个节拍主动退出，走与断线一致的清理路径（无需踢出包）；
//! - 广播（系统消息）无可靠编码器，本轮只返回 501 + TODO，不发包。
//! TODO(P25-followup): 抓包核对 777 `system_chat` 包体后实现 `/api/broadcast` 真发包。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use serde::Deserialize;

use crate::admin_items;

/// P52：`/api/world` 只读展示的 socket 归属（启动期由 main 设置一次）。
/// 用 OnceLock 而非 AdminState 新字段：保持既有测试的结构体字面量与 API 形状不变。
static SOCKET_MODE: std::sync::OnceLock<mc_network::SocketMode> = std::sync::OnceLock::new();

/// main 在完成配置解析后调用；重复调用只保留第一次（启动期唯一）。
pub fn set_socket_mode(mode: mc_network::SocketMode) {
    let _ = SOCKET_MODE.set(mode);
}

fn socket_mode() -> mc_network::SocketMode {
    SOCKET_MODE.get().copied().unwrap_or(mc_network::SocketMode::Net)
}

/// 管理线程共享状态（与游戏线程共享的 `Arc`，`main.rs` 组装）。
#[derive(Debug, Clone)]
pub struct AdminState {
    pub hub: Arc<mc_network::BroadcastHub>,
    pub drops: Arc<mc_network::DropState>,
    pub mutable: Arc<mc_world::MutableWorld>,
    pub seed: i64,
    pub world_dir: PathBuf,
    pub view_distance: i32,
}

/// 发物品数量上限：单次最多 10 组（64×10，按 64/栈自动拆多槽）。
pub const GIVE_MAX_COUNT: i32 = 640;

/// HTTP 响应（`route` 纯构造，`serve` 只负责写出）。
pub struct AdminResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl AdminResponse {
    fn json(status: u16, value: serde_json::Value) -> Self {
        Self { status, content_type: "application/json", body: value.to_string().into_bytes() }
    }

    fn err(status: u16, msg: impl Into<String>) -> Self {
        Self::json(status, serde_json::json!({"ok": false, "error": msg.into()}))
    }

    /// 已序列化的缓存体直接透传（P29 `/api/items` 约几十 KB，只生成一次）。
    fn raw(status: u16, body: Vec<u8>) -> Self {
        Self { status, content_type: "application/json", body }
    }

    fn html(body: &'static str) -> Self {
        Self { status: 200, content_type: "text/html; charset=utf-8", body: body.as_bytes().to_vec() }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "Error",
    }
}

fn write_response(stream: &mut TcpStream, resp: &AdminResponse) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        resp.status,
        reason(resp.status),
        resp.content_type,
        resp.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&resp.body);
    let _ = stream.flush();
}

/// 解析后的最小 HTTP 请求（只取路由/查询/体）。
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: Vec<u8>,
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// 纯函数：解析请求头（首行 + `Content-Length`），返回 (方法, 路径, 查询串, 体长)。
fn parse_head(head: &str) -> Result<(String, String, String, usize), String> {
    let mut lines = head.split("\r\n");
    let first = lines.next().ok_or("empty request")?;
    let mut parts = first.split_whitespace();
    let method = parts.next().ok_or("no method")?.to_string();
    let target = parts.next().ok_or("no target")?.to_string();
    if parts.next().is_none() {
        return Err("no version".to_string());
    }
    if method != "GET" && method != "POST" {
        return Err(format!("unsupported method: {method}"));
    }
    let (path, query) = match target.find('?') {
        Some(i) => (target[..i].to_string(), target[i + 1..].to_string()),
        None => (target, String::new()),
    };
    let mut content_len = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some(v) = line.split_once(':') {
            if v.0.trim().eq_ignore_ascii_case("content-length") {
                content_len = v.1.trim().parse().map_err(|_| "bad content-length")?;
            }
        }
    }
    Ok((method, path, query, content_len))
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        if buf.len() > 65536 {
            return Err("head too large".to_string());
        }
        let n = stream.read(&mut tmp).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Err("eof".to_string());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(hend) = find_head_end(&buf) {
            let head = std::str::from_utf8(&buf[..hend]).map_err(|_| "bad utf8".to_string())?;
            let (method, path, query, content_len) = parse_head(head)?;
            if content_len > 1024 * 1024 {
                return Err("body too large".to_string());
            }
            while buf.len() < hend + content_len {
                let n = stream.read(&mut tmp).map_err(|e| format!("read body: {e}"))?;
                if n == 0 {
                    return Err("eof in body".to_string());
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > hend + 1024 * 1024 {
                    return Err("body too large".to_string());
                }
            }
            let body = buf[hend..hend + content_len].to_vec();
            return Ok(HttpRequest { method, path, query, body });
        }
    }
}

/// 最小 query/百分号解码（用户名只含 `[A-Za-z0-9_]`，解码是兜底）。
fn pct_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        if b[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        if k == key {
            Some(pct_decode(v))
        } else {
            None
        }
    })
}

/// 由 `username`（精确匹配，大小写敏感）/ `conn_id` / `uuid`（P51，大小写无关）
/// 解析在线连接。三者按 uuid → username → conn_id 优先级，旧字段一个不删。
/// 先快照后丢锁（计划 §6.2），返回 `(conn_id, username)`。
fn resolve_conn(
    state: &AdminState,
    username: Option<&str>,
    conn_id: Option<u64>,
    uuid: Option<&str>,
) -> Result<(u64, String), String> {
    let snapshot_name = |c: u64| {
        state
            .hub
            .snapshot()
            .into_iter()
            .find(|e| e.conn_id == c)
            .map(|e| e.username)
            .unwrap_or_default()
    };
    if let Some(u) = uuid {
        if u.trim().is_empty() {
            return Err("uuid must not be empty".to_string());
        }
        match state.hub.find_conn_by_uuid(u) {
            Some(c) => Ok((c, snapshot_name(c))),
            None => Err(format!("offline uuid: {u}")),
        }
    } else if let Some(name) = username {
        if name.is_empty() {
            return Err("username must not be empty".to_string());
        }
        match state.hub.find_conn_by_name(name) {
            Some(c) => Ok((c, name.to_string())),
            None => Err(format!("offline: {name}")),
        }
    } else if let Some(c) = conn_id {
        if !state.hub.contains(c) {
            return Err(format!("offline conn {c}"));
        }
        Ok((c, snapshot_name(c)))
    } else {
        Err("uuid, username or conn_id required".to_string())
    }
}

/// 物品名（`minecraft:xxx` 或短名）或数字 id → 数字 id；未知拒绝（不静默回退）。
fn resolve_item(s: &str) -> Result<i32, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("item required".to_string());
    }
    if let Ok(n) = t.parse::<i32>() {
        if n > 0 && mc_registry::item_name(n as u32).is_some() {
            return Ok(n);
        }
        return Err(format!("unknown item: {s}"));
    }
    match mc_registry::item_id(t) {
        Some(id) => Ok(id as i32),
        None => Err(format!("unknown item: {s}")),
    }
}

#[derive(Debug, Deserialize)]
struct GiveReq {
    username: Option<String>,
    conn_id: Option<u64>,
    /// P51：稳定 UUID（与 username/conn_id 二选一，优先）。
    uuid: Option<String>,
    item: Option<String>,
    /// P29 新增：数字 id 直发（与 `item` 二选一，`item` 优先；旧字段不动）。
    item_id: Option<i32>,
    count: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct KickReq {
    username: Option<String>,
    conn_id: Option<u64>,
    uuid: Option<String>,
}

/// P47：工具附魔开关（`none` / `fortune` / `silk_touch`），按连接设置，仅当前会话。
/// P47 游戏模式切换请求（username 或 conn_id + survival|creative）。
#[derive(Debug, Deserialize)]
struct GamemodeReq {
    username: Option<String>,
    conn_id: Option<u64>,
    uuid: Option<String>,
    gamemode: Option<String>,
}

/// P53 A4：把模式同时写入内存与背包存档字段（设置落盘由 `save_inventory` 完成）。
fn persist_gamemode(state: &AdminState, conn: u64, user: &str, mode: mc_network::broadcast::GameMode) {
    if !state.drops.p53_features().2 {
        return;
    }
    let code = mc_network::gamemode_code(mode);
    if state.drops.set_persisted_settings(conn, code, state.drops.with_inventory(conn, |inv| inv.persisted_tool_enchant()).unwrap_or(0)) {
        state.drops.save_inventory(conn, user);
    }
}

fn handle_gamemode(state: &AdminState, body: &[u8]) -> AdminResponse {
    let req: GamemodeReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return AdminResponse::err(400, "bad json"),
    };
    let mode = match req.gamemode.as_deref() {
        Some(s) => match mc_network::broadcast::GameMode::parse(s) {
            Some(v) => v,
            None => return AdminResponse::err(400, "gamemode must be survival|creative"),
        },
        None => return AdminResponse::err(400, "gamemode required"),
    };
    let (conn, user) = match resolve_conn(state, req.username.as_deref(), req.conn_id, req.uuid.as_deref()) {
        Ok(v) => v,
        Err(e) => return AdminResponse::err(400, e),
    };
    if !state.hub.set_gamemode(conn, mode) {
        return AdminResponse::err(400, format!("offline conn {conn}"));
    }
    persist_gamemode(state, conn, &user, mode);
    AdminResponse::json(
        200,
        serde_json::json!({"ok": true, "conn_id": conn, "username": user, "gamemode": mode.as_str(), "persisted": state.drops.p53_features().2}),
    )
}

#[derive(Debug, Deserialize)]
struct ToolEnchantReq {
    username: Option<String>,
    conn_id: Option<u64>,
    uuid: Option<String>,
    enchant: Option<String>,
}

/// P47 工具档位/可挖方块表：所有 ID 直接来自 `mc_registry` 生成表，
/// 与挖掘判定用的 `mc_world::tools` 常量同源，不让面板自己猜数字。
fn handle_tools() -> AdminResponse {
    use mc_world::tools::{MINING_RULES, PICKAXES, TIER_NAMES};
    let tiers: Vec<serde_json::Value> = TIER_NAMES
        .iter()
        .enumerate()
        .map(|(tier, name)| {
            let tool = PICKAXES.iter().find(|p| p.tier as usize == tier);
            serde_json::json!({
                "tier": tier,
                "name": name,
                "item_id": tool.map(|p| p.item_id),
                "item": tool.map(|p| p.name),
            })
        })
        .collect();
    let tools: Vec<serde_json::Value> = PICKAXES
        .iter()
        .map(|p| serde_json::json!({ "name": p.name, "item": format!("minecraft:{}", p.name), "item_id": p.item_id, "tier": p.tier }))
        .collect();
    let blocks: Vec<serde_json::Value> = MINING_RULES
        .iter()
        .map(|r| {
            serde_json::json!({
                "block": r.block_name,
                "block_id": r.block_id,
                "tier": r.tier,
                "tier_name": TIER_NAMES[r.tier as usize],
                "item_id": r.item_id,
                "item": mc_registry::item_name(r.item_id as u32),
                "fortune": r.fortune,
            })
        })
        .collect();
    AdminResponse::json(
        200,
        serde_json::json!({
            "ok": true,
            "tiers": tiers,
            "tools": tools,
            "blocks": blocks,
            "note": "P47 MVP: no durability / no mining speed; enchant is per-conn server flag",
        }),
    )
}

fn handle_tool_enchant(state: &AdminState, body: &[u8]) -> AdminResponse {
    let req: ToolEnchantReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return AdminResponse::err(400, "bad json"),
    };
    let name = match req.enchant.as_deref() {
        Some(s) => match mc_world::tools::ToolEnchant::parse(s) {
            Some(v) => v,
            None => return AdminResponse::err(400, "enchant must be none|fortune|silk_touch"),
        },
        None => return AdminResponse::err(400, "enchant required"),
    };
    let (conn, user) = match resolve_conn(state, req.username.as_deref(), req.conn_id, req.uuid.as_deref()) {
        Ok(v) => v,
        Err(e) => return AdminResponse::err(400, e),
    };
    state.drops.set_tool_enchant(conn, name);
    if state.drops.p53_features().2 {
        let mode = state.drops.with_inventory(conn, |inv| inv.persisted_gamemode()).unwrap_or(0);
        if state.drops.set_persisted_settings(conn, mode, mc_network::enchant_code(name)) {
            state.drops.save_inventory(conn, &user);
        }
    }
    AdminResponse::json(
        200,
        serde_json::json!({"ok": true, "conn_id": conn, "username": user, "enchant": name.as_str(), "persisted": state.drops.p53_features().2}),
    )
}

fn handle_online(state: &AdminState) -> AdminResponse {
    // 读锁内只拷贝快照（`snapshot` 内部处理），丢锁后组 JSON。
    let mut entries = state.hub.snapshot();
    entries.sort_by_key(|e| e.conn_id);
    let online: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            // P53：背包内的 XP/生存/疲劳/落盘设置一次拷贝，锁外组 JSON。
            let (xp, surv, exhaustion, pgm, pen) = state
                .drops
                .with_inventory(e.conn_id, |inv| {
                    (
                        inv.total_xp(),
                        inv.survival(),
                        inv.exhaustion(),
                        inv.persisted_gamemode(),
                        inv.persisted_tool_enchant(),
                    )
                })
                .unwrap_or((0, mc_world::SurvivalState::full(), 0.0, 0, 0));
            serde_json::json!({
                "conn_id": e.conn_id,
                "username": e.username,
                // P51：旧字段一个不删，只增稳定 UUID。
                "uuid": e.uuid,
                "addr": e.addr,
                "pos": [e.pos.0, e.pos.1, e.pos.2],
                "online_secs": e.online_secs,
                "gamemode": e.gamemode.as_str(),
                "tool_enchant": state.drops.tool_enchant(e.conn_id).as_str(),
                // P53 A1/A2/A4：后台联动新增列（旧字段保留）。
                "xp": xp,
                "health": surv.health,
                "food": surv.food,
                "saturation": surv.saturation,
                "exhaustion": exhaustion,
                "persisted_gamemode": if pgm == 1 { "creative" } else { "survival" },
                "persisted_tool_enchant": match pen { 1 => "fortune", 2 => "silk_touch", _ => "none" },
                "settings_persist": state.drops.p53_features().2,
            })
        })
        .collect();
    AdminResponse::json(
        200,
        serde_json::json!({"ok": true, "online": online, "count": online.len()}),
    )
}

fn handle_world(state: &AdminState) -> AdminResponse {
    let (xp_merge, hunger_detail, settings_persist, stats) = state.drops.p53_features();
    AdminResponse::json(
        200,
        serde_json::json!({
            "ok": true,
            "seed": state.seed,
            "online": state.hub.len(),
            "override_count": state.mutable.override_count(),
            "dirty_chunks": state.mutable.dirty_len(),
            "drop_tick": state.drops.tick_count(),
            "world_dir": state.world_dir.display().to_string(),
            "view_distance": state.view_distance,
            // P52：只读，不新增 POST；面板世界状态卡 <pre> 自动显示。
            "socket_mode": socket_mode().as_str(),
            // P53：四项开关只读展示（写入只在 server.toml，启动期锁定）。
            "p53_features": {
                "xp_merge": xp_merge, "hunger_detail": hunger_detail,
                "settings_persist": settings_persist, "stats": stats,
            },
        }),
    )
}

fn handle_give(state: &AdminState, body: &[u8]) -> AdminResponse {
    let req: GiveReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return AdminResponse::err(400, "bad json"),
    };
    let count = req.count.unwrap_or(1);
    if !(1..=GIVE_MAX_COUNT).contains(&count) {
        return AdminResponse::err(400, format!("count must be 1..={GIVE_MAX_COUNT}"));
    }
    // P29：`item`（名/数字串）优先，缺省时可用新增的 `item_id` 数字字段；响应形状不变。
    let item_id = match (req.item.as_deref(), req.item_id) {
        (Some(s), _) => match resolve_item(s) {
            Ok(id) => id,
            Err(e) => return AdminResponse::err(400, e),
        },
        (None, Some(n)) => {
            if n > 0 && mc_registry::item_name(n as u32).is_some() {
                n
            } else {
                return AdminResponse::err(400, format!("unknown item: {n}"));
            }
        }
        (None, None) => return AdminResponse::err(400, "item required"),
    };
    let (conn, user) = match resolve_conn(state, req.username.as_deref(), req.conn_id, req.uuid.as_deref()) {
        Ok(v) => v,
        Err(e) => return AdminResponse::err(400, e),
    };
    // 计划 §5.3：背包锁内只做 `add_item`（结果带出）；落盘在锁外；
    // 组包纯函数；最后 unicast。永不丢物品：写入+落盘先生效，投递失败只影响即时显示。
    let leftover = match state.drops.with_inventory(conn, |inv| inv.add_item(item_id, count)) {
        Some(l) => l,
        None => return AdminResponse::err(400, format!("offline conn {conn}")),
    };
    state.drops.save_inventory(conn, &user);
    let delivered = match mc_network::login_route::drop_sync::build_full_inventory_body(
        &state.drops,
        conn,
    ) {
        Some(body) => {
            state.hub.unicast(conn, mc_protocol::packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, body)
        }
        None => false,
    };
    AdminResponse::json(
        200,
        serde_json::json!({"ok": true, "leftover": leftover, "delivered": delivered}),
    )
}

fn handle_inventory(state: &AdminState, query: &str) -> AdminResponse {
    let username = query_param(query, "username");
    let conn_id = query_param(query, "conn_id").and_then(|s| s.parse::<u64>().ok());
    let uuid = query_param(query, "uuid");
    let (conn, user) = match resolve_conn(state, username.as_deref(), conn_id, uuid.as_deref()) {
        Ok(v) => v,
        Err(e) => return AdminResponse::err(400, e),
    };
    let Some((selected, slots)) =
        state.drops.with_inventory(conn, |inv| (inv.selected(), inv.to_protocol_slots()))
    else {
        return AdminResponse::err(400, format!("offline conn {conn}"));
    };
    let filled: Vec<[i64; 3]> = slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            if s.is_empty() {
                None
            } else {
                Some([i as i64, s.item_id as i64, s.count as i64])
            }
        })
        .collect();
    // P29 新增：格子中文明细（`slots` 旧字段原样保留，只增不改）。
    let detail: Vec<serde_json::Value> = filled
        .iter()
        .map(|s| {
            let id = s[1] as u32;
            let (name, zh, cat) = admin_items::item_info_by_id(id)
                .map(|(n, z, c)| (n.to_string(), z.to_string(), c.to_string()))
                .unwrap_or((format!("unknown:{id}"), format!("unknown:{id}"), "其他".to_string()));
            serde_json::json!({
                "slot": s[0], "id": s[1], "count": s[2],
                "name": name, "name_zh": zh, "category": cat,
            })
        })
        .collect();
    AdminResponse::json(
        200,
        serde_json::json!({
            "ok": true,
            "username": user,
            "conn_id": conn,
            "selected": selected,
            "slots": filled,
            "slots_detail": detail,
        }),
    )
}

fn handle_kick(state: &AdminState, body: &[u8]) -> AdminResponse {
    let req: KickReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return AdminResponse::err(400, "bad json"),
    };
    let (conn, user) = match resolve_conn(state, req.username.as_deref(), req.conn_id, req.uuid.as_deref()) {
        Ok(v) => v,
        Err(e) => return AdminResponse::err(400, e),
    };
    if state.hub.kick(conn) {
        AdminResponse::json(200, serde_json::json!({"ok": true, "kicked": true, "conn_id": conn, "username": user}))
    } else {
        AdminResponse::err(400, format!("offline conn {conn}"))
    }
}

fn handle_save(state: &AdminState) -> AdminResponse {
    // 沿用 autosave 语义：无脏 chunk 不碰盘。
    if state.mutable.dirty_len() == 0 {
        return AdminResponse::json(
            200,
            serde_json::json!({"ok": true, "saved": false, "overrides": state.mutable.override_count()}),
        );
    }
    match state.mutable.save_to_dir(&state.world_dir) {
        Ok(n) => AdminResponse::json(200, serde_json::json!({"ok": true, "saved": true, "overrides": n})),
        Err(e) => AdminResponse::err(500, format!("save failed: {e}")),
    }
}

/// P29 物品清单（全量 1658 项，服务端一次生成缓存，页面按需取一次）。
fn handle_items() -> AdminResponse {
    AdminResponse::raw(200, admin_items::items_catalog_json().to_vec())
}

/// P47 配方浏览：全部 283 条，输入/输出均带注册名 + ID + 分类；
/// ID 来自同一张生成表，面板只展示、不自行拼数字。
fn handle_recipes() -> AdminResponse {
    let recipes: Vec<serde_json::Value> = mc_world::crafting::RECIPES
        .iter()
        .map(|r| {
            let needs: Vec<serde_json::Value> = r
                .needs
                .iter()
                .map(|(id, count)| {
                    serde_json::json!({
                        "item_id": id,
                        "name": mc_registry::item_name(*id as u32),
                        "count": count,
                    })
                })
                .collect();
            let category = admin_items::item_info_by_id(r.result.0 as u32)
                .map(|(_, _, c)| c.to_string())
                .unwrap_or_else(|| "其他".to_string());
            serde_json::json!({
                "name": r.name,
                "needs": needs,
                "result": { "item_id": r.result.0, "count": r.result.1,
                            "name": mc_registry::item_name(r.result.0 as u32) },
                "category": category,
                "grids": [2, 3],
            })
        })
        .collect();
    AdminResponse::json(
        200,
        serde_json::json!({
            "ok": true,
            "count": recipes.len(),
            "recipes": recipes,
            "note": "P47: 2×2 player grid and 3×3 workbench share this table; no shaped matching",
        }),
    )
}

/// P47 工作台会话快照（仅内存会话，断线即丢；无 block entity 存档 MVP）。
fn handle_craft9(state: &AdminState) -> AdminResponse {
    let sessions: Vec<serde_json::Value> = state
        .drops
        .craft9_sessions()
        .into_iter()
        .map(|s| {
            let grid: Vec<serde_json::Value> = s
                .grid
                .into_iter()
                .enumerate()
                .filter_map(|(i, stack)| {
                    stack.map(|(id, count)| {
                        serde_json::json!({
                            "slot": i + 1,
                            "item_id": id,
                            "name": mc_registry::item_name(id as u32),
                            "count": count,
                        })
                    })
                })
                .collect();
            serde_json::json!({
                "pos": [s.x, s.y, s.z],
                "grid": grid,
            })
        })
        .collect();
    AdminResponse::json(200, serde_json::json!({"ok": true, "sessions": sessions}))
}

// --- P53 A1/A2/A7：经验发放 / 生存调试 / 本地统计 ---

#[derive(Debug, Deserialize)]
struct PlayerReq {
    username: Option<String>,
    conn_id: Option<u64>,
    uuid: Option<String>,
    amount: Option<i32>,
    food: Option<i32>,
    saturation: Option<f32>,
    health: Option<f32>,
}

/// `YYYY-MM-DD` → 距 epoch 的天数（纯算法，无 chrono 依赖；非法返回 None）。
pub fn date_to_stat_day(s: &str) -> Option<u32> {
    let mut parts = s.split('-');
    let (y, m, d) = (parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<i64>().ok()?);
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant civil_from_days 逆运算（1970-01-01 = 0）。
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    u32::try_from(era * 146_097 + doe - 719_468).ok()
}

fn handle_xp(state: &AdminState, body: &[u8]) -> AdminResponse {
    let req: PlayerReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return AdminResponse::err(400, "bad json"),
    };
    let amount = req.amount.unwrap_or(0);
    if !(1..=i32::MAX).contains(&amount) {
        return AdminResponse::err(400, "amount must be positive");
    }
    let (conn, user) = match resolve_conn(state, req.username.as_deref(), req.conn_id, req.uuid.as_deref()) {
        Ok(v) => v,
        Err(e) => return AdminResponse::err(400, e),
    };
    let total = match state.drops.with_inventory(conn, |inv| inv.add_experience(amount)) {
        Some(v) => v,
        None => return AdminResponse::err(400, format!("offline conn {conn}")),
    };
    state.drops.save_inventory(conn, &user);
    let delivered = state.drops.with_inventory(conn, |inv| {
        (inv.xp_level(), inv.xp_progress())
    }).map(|(level, progress)| {
        let mut body = Vec::new();
        mc_protocol::packets::SetExperience { progress, level, total }.encode_body(&mut body);
        state.hub.unicast(conn, mc_protocol::packets::PLAY_SET_EXPERIENCE_S2C_ID, body)
    }).unwrap_or(false);
    AdminResponse::json(200, serde_json::json!({"ok": true, "total_xp": total, "delivered": delivered}))
}

fn handle_set_food(state: &AdminState, body: &[u8]) -> AdminResponse {
    let req: PlayerReq = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return AdminResponse::err(400, "bad json"),
    };
    let food = match req.food {
        Some(v) if (0..=mc_world::survival::MAX_FOOD).contains(&v) => v,
        _ => return AdminResponse::err(400, "food must be 0..=20"),
    };
    let (conn, user) = match resolve_conn(state, req.username.as_deref(), req.conn_id, req.uuid.as_deref()) {
        Ok(v) => v,
        Err(e) => return AdminResponse::err(400, e),
    };
    let after = match state.drops.apply_survival(conn, |s| {
        s.food = food;
        if let Some(sat) = req.saturation {
            s.saturation = sat.clamp(0.0, mc_world::survival::MAX_SATURATION);
        }
        if let Some(hp) = req.health {
            s.health = hp.clamp(0.0, mc_world::survival::MAX_HEALTH);
        }
    }) {
        Some((s, _)) => s,
        None => return AdminResponse::err(400, format!("offline conn {conn}")),
    };
    state.drops.save_inventory(conn, &user);
    let body = mc_network::login_route::survival_sync::build_set_health_body(&after);
    let delivered = state.hub.unicast(conn, mc_protocol::packets::PLAY_SET_HEALTH_S2C_ID, body);
    AdminResponse::json(200, serde_json::json!({"ok": true, "food": after.food, "delivered": delivered}))
}

fn handle_stats(state: &AdminState, query: &str) -> AdminResponse {
    let username = query_param(query, "username");
    let uuid = query_param(query, "uuid");
    let identity = uuid.as_deref().or(username.as_deref());
    let Some(identity) = identity.filter(|s| !s.is_empty()) else {
        return AdminResponse::err(400, "username or uuid required");
    };
    let from = query_param(query, "from").and_then(|s| date_to_stat_day(&s));
    let to = query_param(query, "to").and_then(|s| date_to_stat_day(&s));
    let rows: Vec<serde_json::Value> = state
        .drops
        .stats_for(identity, from, to)
        .into_iter()
        .map(|(stat, day, count)| serde_json::json!({
            "stat_id": stat, "stat": mc_network::stat_name(stat),
            "day": day, "count": count,
        }))
        .collect();
    AdminResponse::json(200, serde_json::json!({
        "ok": true, "identity": identity, "stats": rows,
        "note": "P53 A7: server-local only; award_stats (B13) not sent",
    }))
}

/// 路由（纯构造响应；M1 只读：`/` + `/api/online` + `/api/world`）。
pub fn route(req: &HttpRequest, state: &AdminState) -> AdminResponse {
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => AdminResponse::html(include_str!("admin_page.html")),
        ("GET", "/api/online") => handle_online(state),
        ("GET", "/api/world") => handle_world(state),
        ("GET", "/api/items") => handle_items(),
        ("GET", "/api/tools") => handle_tools(),
        ("GET", "/api/recipes") => handle_recipes(),
        ("GET", "/api/craft9") => handle_craft9(state),
        ("GET", "/api/inventory") => handle_inventory(state, &req.query),
        ("GET", "/api/stats") => handle_stats(state, &req.query),
        ("POST", "/api/xp") => handle_xp(state, &req.body),
        ("POST", "/api/set-food") => handle_set_food(state, &req.body),
        ("POST", "/api/give") => handle_give(state, &req.body),
        ("POST", "/api/tool-enchant") => handle_tool_enchant(state, &req.body),
        ("POST", "/api/gamemode") => handle_gamemode(state, &req.body),
        ("POST", "/api/kick") => handle_kick(state, &req.body),
        ("POST", "/api/save") => handle_save(state),
        ("POST", "/api/broadcast") => AdminResponse::err(
            501,
            "broadcast TODO: no verified system_chat S2C encoder for 777, refusing to guess bytes",
        ),
        _ => AdminResponse::err(404, "not found"),
    }
}

fn handle_one(stream: &mut TcpStream, state: &AdminState) {
    let resp = match read_http_request(stream) {
        Ok(req) => route(&req, state),
        Err(e) => AdminResponse::err(400, e),
    };
    write_response(stream, &resp);
}

/// 管理监听循环（`main.rs` 起独立线程跑这个；`shutdown` 由主循环/Ctrl-C 置位）。
/// `bind` 失败只记错返回（故障隔离：绝不影响 25565 游戏端口）。
pub fn serve_admin(bind: &str, state: AdminState, shutdown: Arc<AtomicBool>) {
    let listener = match TcpListener::bind(bind) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("admin bind {bind} failed ({e}), panel disabled, game server keeps running");
            return;
        }
    };
    tracing::info!("admin panel listening on {}", listener.local_addr().map(|a| a.to_string()).unwrap_or_default());
    serve_admin_on(listener, state, shutdown);
}

/// 已绑定 listener 上的服务循环（测试用 `:0` 临时端口时直接调这个）。
pub fn serve_admin_on(listener: TcpListener, state: AdminState, shutdown: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                handle_one(&mut stream, &state);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                tracing::warn!("admin accept failed ({e})");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn test_state() -> AdminState {
        // 每个用例独立存档目录：背包落盘按 (dir,user,seed) 隔离，并行/复跑不串味。
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let base = std::env::temp_dir().join(format!("mc-admin-ut-{}-{n}", std::process::id()));
        AdminState {
            hub: Arc::new(mc_network::BroadcastHub::new()),
            drops: Arc::new(mc_network::DropState::new(base.clone(), 777)),
            mutable: Arc::new(mc_world::MutableWorld::new(777)),
            seed: 777,
            world_dir: base.join("world"),
            view_distance: 2,
        }
    }

    fn get(path: &str) -> HttpRequest {
        let (path, query) = match path.find('?') {
            Some(i) => (path[..i].to_string(), path[i + 1..].to_string()),
            None => (path.to_string(), String::new()),
        };
        HttpRequest { method: "GET".into(), path, query, body: Vec::new() }
    }

    fn post(path: &str, body: &str) -> HttpRequest {
        HttpRequest { method: "POST".into(), path: path.into(), query: String::new(), body: body.as_bytes().to_vec() }
    }

    fn body_json(resp: &AdminResponse) -> serde_json::Value {
        serde_json::from_slice(&resp.body).unwrap()
    }

    #[test]
    fn parse_head_routes_and_reads_content_length() {
        let (m, p, q, n) =
            parse_head("POST /api/give?x=1 HTTP/1.1\r\nContent-Length: 12\r\n").unwrap();
        assert_eq!((m.as_str(), p.as_str(), q.as_str(), n), ("POST", "/api/give", "x=1", 12));
        assert!(parse_head("DELETE / HTTP/1.1\r\n").is_err(), "only GET/POST are supported");
        assert!(parse_head("GET").is_err());
    }

    #[test]
    fn unknown_paths_404_and_bad_json_400() {
        let st = test_state();
        assert_eq!(route(&get("/nope"), &st).status, 404);
        assert_eq!(route(&post("/api/give", "{oops"), &st).status, 400);
        assert_eq!(route(&post("/api/kick", ""), &st).status, 400);
        // M1 只读端点：空服也 200。
        let w = route(&get("/api/world"), &st);
        assert_eq!(w.status, 200);
        let v = body_json(&w);
        assert_eq!(v["ok"], true);
        assert_eq!(v["seed"], 777);
        assert_eq!(v["online"], 0);
        let o = route(&get("/api/online"), &st);
        assert_eq!(body_json(&o)["count"], 0);
        let index = route(&get("/"), &st);
        assert_eq!(index.status, 200);
        assert!(index.body.windows(6).any(|w| w == b"fetch("), "page must drive the API with fetch");
    }

    #[test]
    fn world_endpoint_exposes_readonly_socket_mode() {
        let st = test_state();
        let w = route(&get("/api/world"), &st);
        let v = body_json(&w);
        let mode = v["socket_mode"].as_str().expect("socket_mode must be a read-only string");
        assert!(matches!(mode, "net" | "ecs"), "mode must be a known value: {mode}");
        // 旧字段全部保留，API 形状不破坏。
        assert_eq!(v["view_distance"], 2);
        assert_eq!(v["seed"], 777);
        assert_eq!(route(&post("/api/world", ""), &st).status, 404, "no write endpoint added");
    }

    #[test]
    fn give_rejects_unknown_item_bad_count_and_offline() {
        let st = test_state();
        // 未知物品 400（名称与数字 id 都要拦）。
        let r = route(&post("/api/give", r#"{"username":"Nobody","item":"minecraft:not_a_thing","count":1}"#), &st);
        assert_eq!(r.status, 400);
        let r = route(&post("/api/give", r#"{"username":"Nobody","item":"999999","count":1}"#), &st);
        assert_eq!(r.status, 400);
        assert_eq!(body_json(&r)["ok"], false);
        // 数量越界 400。
        for bad in ["0", "-3", "641"] {
            let r = route(
                &post("/api/give", &format!(r#"{{"username":"Nobody","item":"stone","count":{bad}}}"#)),
                &st,
            );
            assert_eq!(r.status, 400, "count={bad} must be rejected");
        }
        // 缺物品 400；离线用户 400（用户名精确匹配，大小写敏感）。
        let r = route(&post("/api/give", r#"{"username":"Nobody","count":1}"#), &st);
        assert_eq!(r.status, 400);
        st.hub.register_with_name(
            5,
            crossbeam_channel::unbounded().0,
            2,
            (0.0, 64.0, 0.0),
            "Somebody".into(),
            "127.0.0.1:1".into(),
        );
        let r = route(&post("/api/give", r#"{"username":"somebody","item":"stone","count":1}"#), &st);
        assert_eq!(r.status, 400, "name match must be case-sensitive");
        // kick/broadcast/save 的离线与降级语义。
        let r = route(&post("/api/kick", r#"{"username":"Nobody"}"#), &st);
        assert_eq!(r.status, 400);
        let r = route(&post("/api/broadcast", r#"{"text":"hi"}"#), &st);
        assert_eq!(r.status, 501, "broadcast has no verified encoder, must stay disabled");
        let r = route(&get("/api/inventory?username=Nobody"), &st);
        assert_eq!(r.status, 400);
    }

    #[test]
    fn give_writes_save_and_delivers_to_online_conn() {
        let st = test_state();
        let (tx, rx) = crossbeam_channel::unbounded();
        st.hub.register_with_name(11, tx, 2, (0.0, 64.0, 0.0), "Giver".into(), "127.0.0.1:2".into());
        st.drops.load_inventory(11, "Giver");
        // 短名与 minecraft: 全名都要能用；count 缺省=1。
        let r = route(&post("/api/give", r#"{"username":"Giver","item":"cobblestone"}"#), &st);
        assert_eq!(r.status, 200);
        let v = body_json(&r);
        assert_eq!(v, serde_json::json!({"ok": true, "leftover": 0, "delivered": true}));
        // unicast 出去的正是 Content 包（id 对、包体可解、含刚发的物）。
        let pkt = rx.try_recv().expect("give must unicast a sync packet");
        assert_eq!(pkt.id, mc_protocol::packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID);
        let (content, _) = mc_protocol::packets::SetContainerContent::decode_body(&pkt.body).unwrap();
        let cobble = mc_registry::item_id("minecraft:cobblestone").unwrap() as i32;
        assert!(content.slots.iter().any(|s| s.item_id == cobble && s.count == 1));
        // 复核端点看到同一终态；落盘文件存在（永不丢物品：掉线重进也在）。
        let r = route(&get("/api/inventory?username=Giver"), &st);
        assert_eq!(r.status, 200);
        let inv = body_json(&r);
        assert_eq!(inv["conn_id"], 11);
        assert!(inv["slots"].as_array().unwrap().iter().any(|s| s[1] == cobble && s[2] == 1));
        let dir = st.drops.save_dir();
        assert!(mc_world::PlayerInventory::save_path(&dir, "Giver", 777).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn give_reports_leftover_when_full_and_undelivered_when_gone() {
        let st = test_state();
        let stone = mc_registry::item_id("minecraft:stone").unwrap() as i32;
        // 背包可填充槽（27 主 + 9 快捷 + 1 副手）中空的 36 格全部塞满石头
        //（槽 36 已有开局石头，故只填 36 次；0-8 合成格 add_item 永不碰）。
        st.drops.load_inventory(21, "Full");
        st.drops.with_inventory(21, |inv| {
            for _ in 0..36 {
                assert_eq!(inv.add_item(stone, 64), 0);
            }
        });
        let (tx, rx) = crossbeam_channel::unbounded();
        st.hub.register_with_name(21, tx, 2, (0.0, 64.0, 0.0), "Full".into(), "127.0.0.1:3".into());
        // 满包：64 个 dirt 一个都装不下，leftover 如实返回 64，但包仍投递（全量同步）。
        let r = route(
            &post("/api/give", r#"{"username":"Full","item":"minecraft:dirt","count":64}"#),
            &st,
        );
        assert_eq!(r.status, 200);
        let v = body_json(&r);
        assert_eq!(v["leftover"], 64);
        assert_eq!(v["delivered"], true);
        assert!(rx.try_recv().is_ok());
        // 对端已走（receiver 丢了）：写入+落盘仍生效，delivered=false。
        drop(rx);
        st.hub.register_with_name(
            22,
            crossbeam_channel::unbounded().0,
            2,
            (0.0, 64.0, 0.0),
            "Gone".into(),
            "127.0.0.1:4".into(),
        );
        st.drops.load_inventory(22, "Gone");
        let (tx2, rx2) = crossbeam_channel::unbounded::<mc_network::QueuedPacket>();
        drop(rx2);
        st.hub.unregister(22);
        st.hub.register_with_name(22, tx2, 2, (0.0, 64.0, 0.0), "Gone".into(), "127.0.0.1:4".into());
        let r = route(&post("/api/give", r#"{"username":"Gone","item":"stone","count":5}"#), &st);
        assert_eq!(r.status, 200);
        let v = body_json(&r);
        assert_eq!(v["leftover"], 0);
        assert_eq!(v["delivered"], false, "dead outbox must report undelivered");
        // 但背包终态可查（永不丢物品）。
        let inv = route(&get("/api/inventory?username=Gone"), &st);
        let stone_total: i64 = body_json(&inv)["slots"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s[1] == stone as i64)
            .map(|s| s[2].as_i64().unwrap())
            .sum();
        assert!(stone_total >= 5 + 64, "starter stone + given stone must persist, got {stone_total}");
        let _ = std::fs::remove_dir_all(st.drops.save_dir());
    }

    #[test]
    fn kick_removes_online_entry_and_save_respects_dirty() {
        let st = test_state();
        let (tx, rx) = crossbeam_channel::unbounded();
        st.hub.register_with_name(31, tx, 2, (0.0, 64.0, 0.0), "KickMe".into(), "127.0.0.1:5".into());
        assert_eq!(body_json(&route(&get("/api/online"), &st))["count"], 1);
        let r = route(&post("/api/kick", r#"{"username":"KickMe"}"#), &st);
        assert_eq!(r.status, 200);
        assert_eq!(body_json(&r)["kicked"], true);
        let pkt = rx.try_recv().unwrap();
        assert_eq!(pkt.id, mc_network::ADMIN_KICK_PACKET_ID);
        // 干净世界：save 不碰盘。
        let dir = st.world_dir.clone();
        let _ = std::fs::remove_dir_all(&dir);
        let r = route(&post("/api/save", ""), &st);
        assert_eq!(r.status, 200);
        let v = body_json(&r);
        assert_eq!(v["saved"], false);
        assert!(!mc_world::MutableWorld::save_path(&dir, 777).exists());
        // 脏世界：找一格与石头不同的位置落子（基底恰为石头时会保稀疏删条目），
        // save 落盘且 overrides 对得上。
        let y = (60..300)
            .find(|&y| st.mutable.get(1, y, 1) != mc_world::STONE_BLOCK_ID)
            .expect("must find a non-stone cell");
        st.mutable.set(1, y, 1, mc_world::STONE_BLOCK_ID);
        let r = route(&post("/api/save", ""), &st);
        assert_eq!(body_json(&r), serde_json::json!({"ok": true, "saved": true, "overrides": 1}));
        assert!(mc_world::MutableWorld::save_path(&dir, 777).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn items_catalog_json_shape_and_cache() {
        // 形状：ok/count/items，条目含 id/注册名/中文名/分类，全量 1658 不清退。
        let st = test_state();
        let r = route(&get("/api/items"), &st);
        assert_eq!(r.status, 200);
        let v = body_json(&r);
        assert_eq!(v["ok"], true);
        assert_eq!(v["count"], 1658);
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 1658);
        for it in items {
            assert!(it["id"].is_u64(), "missing id: {it}");
            assert!(it["name"].is_string(), "missing name: {it}");
            assert!(it["name_zh"].is_string(), "missing name_zh: {it}");
            assert!(it["category"].is_string(), "missing category: {it}");
        }
        let stone = items.iter().find(|e| e["name"] == "minecraft:stone").expect("stone in catalog");
        assert_eq!(stone["name_zh"], "石头");
        assert_eq!(stone["category"], "方块");
        // 表外回退：注册名原样、分类“其他”。
        let sword =
            items.iter().find(|e| e["name"] == "minecraft:diamond_sword").expect("sword in catalog");
        assert_eq!(sword["name_zh"], "minecraft:diamond_sword");
        assert_eq!(sword["category"], "其他");
        // 缓存：两次同一指针内容一致（OnceLock 只生成一次）。
        let a = crate::admin_items::items_catalog_json().as_ptr();
        let b = route(&get("/api/items"), &st).body.len();
        assert_eq!(b, crate::admin_items::items_catalog_json().len());
        assert_eq!(a, crate::admin_items::items_catalog_json().as_ptr());
    }

    #[test]
    fn gamemode_endpoint_defaults_survival_and_switches_per_conn() {
        let st = test_state();
        assert_eq!(route(&post("/api/gamemode", r#"{"username":"Nobody","gamemode":"creative"}"#), &st).status, 400);
        assert_eq!(route(&post("/api/gamemode", r#"{"username":"Nobody","gamemode":"spectator"}"#), &st).status, 400);
        let (tx, _rx) = crossbeam_channel::unbounded();
        st.hub.register_with_name(61, tx, 2, (0.0, 64.0, 0.0), "ModeUser".into(), "x".into());
        let online = body_json(&route(&get("/api/online"), &st));
        assert_eq!(online["online"][0]["gamemode"], "survival");
        let r = route(&post("/api/gamemode", r#"{"username":"ModeUser","gamemode":"creative"}"#), &st);
        assert_eq!(body_json(&r)["gamemode"], "creative");
        assert_eq!(body_json(&route(&get("/api/online"), &st))["online"][0]["gamemode"], "creative");
        let r = route(&post("/api/gamemode", r#"{"conn_id":61,"gamemode":"survival"}"#), &st);
        assert_eq!(r.status, 200);
        assert_eq!(body_json(&route(&get("/api/online"), &st))["online"][0]["gamemode"], "survival");
    }

    #[test]
    fn recipes_catalog_exposes_all_recipes_and_craft9_sessions() {
        let st = test_state();
        let r = route(&get("/api/recipes"), &st);
        assert_eq!(r.status, 200);
        let v = body_json(&r);
        assert_eq!(v["count"], mc_world::crafting::RECIPES.len() as i64);
        assert_eq!(v["count"], 283);
        let recipes = v["recipes"].as_array().unwrap();
        let hay = recipes.iter().find(|x| x["name"] == "hay_block").unwrap();
        assert_eq!(hay["result"]["item_id"], mc_registry::item_id("minecraft:hay_block").unwrap() as i64);
        assert_eq!(hay["needs"][0]["item_id"], mc_registry::item_id("minecraft:wheat").unwrap() as i64);
        assert!(hay["needs"][0]["name"].is_string());
        assert!(hay["category"].is_string());
        let sessions = body_json(&route(&get("/api/craft9"), &st));
        assert_eq!(sessions["sessions"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn tools_catalog_is_table_driven_and_enchant_endpoint_sets_conn_state() {
        let st = test_state();
        let r = route(&get("/api/tools"), &st);
        assert_eq!(r.status, 200);
        let v = body_json(&r);
        assert_eq!(v["ok"], true);
        assert_eq!(v["tiers"].as_array().unwrap().len(), 7);
        let tools = v["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 6);
        let wood = tools.iter().find(|t| t["name"] == "wooden_pickaxe").unwrap();
        assert_eq!(wood["item_id"], mc_registry::item_id("minecraft:wooden_pickaxe").unwrap() as i64);
        assert_eq!(wood["tier"], 1);
        let blocks = v["blocks"].as_array().unwrap();
        let dia = blocks.iter().find(|b| b["block"] == "minecraft:diamond_ore").unwrap();
        assert_eq!(dia["block_id"], mc_registry::block_id("minecraft:diamond_ore").unwrap() as i64);
        assert_eq!(dia["tier"], 4);
        assert_eq!(dia["fortune"], true);
        // 离线/坏附魔 400；在线设置生效（默认无附魔）。
        assert_eq!(route(&post("/api/tool-enchant", r#"{"username":"Nobody","enchant":"fortune"}"#), &st).status, 400);
        assert_eq!(route(&post("/api/tool-enchant", r#"{"username":"Nobody","enchant":"nope"}"#), &st).status, 400);
        let (tx, _rx) = crossbeam_channel::unbounded();
        st.hub.register_with_name(51, tx, 2, (0.0, 64.0, 0.0), "Miner".into(), "x".into());
        assert_eq!(st.drops.tool_enchant(51), mc_world::tools::ToolEnchant::None);
        let r = route(&post("/api/tool-enchant", r#"{"username":"Miner","enchant":"silk_touch"}"#), &st);
        assert_eq!(body_json(&r)["enchant"], "silk_touch");
        assert_eq!(st.drops.tool_enchant(51), mc_world::tools::ToolEnchant::SilkTouch);
    }

    #[test]
    fn give_accepts_numeric_item_id_and_inventory_adds_detail() {
        let st = test_state();
        let (tx, _rx) = crossbeam_channel::unbounded();
        st.hub.register_with_name(41, tx, 2, (0.0, 64.0, 0.0), "ItemUser".into(), "127.0.0.1:6".into());
        st.drops.load_inventory(41, "ItemUser");
        // P29 新增：数字 item_id 直发；旧响应形状精确不变（只增请求字段）。
        let dirt = mc_registry::item_id("minecraft:dirt").unwrap() as i32;
        let r = route(&post("/api/give", &format!(r#"{{"username":"ItemUser","item_id":{dirt},"count":5}}"#)), &st);
        assert_eq!(r.status, 200);
        assert_eq!(body_json(&r), serde_json::json!({"ok": true, "leftover": 0, "delivered": true}));
        // 数字 id 越界仍 400；item 与 item_id 双缺仍 400。
        let r = route(&post("/api/give", r#"{"username":"ItemUser","item_id":999999,"count":1}"#), &st);
        assert_eq!(r.status, 400);
        let r = route(&post("/api/give", r#"{"username":"ItemUser","count":1}"#), &st);
        assert_eq!(r.status, 400);
        // 复核：slots 旧字段原样 + slots_detail 中文明细。
        let inv = route(&get("/api/inventory?username=ItemUser"), &st);
        assert_eq!(inv.status, 200);
        let v = body_json(&inv);
        assert!(v["slots"].as_array().unwrap().iter().any(|s| s[1] == dirt as i64 && s[2] == 5));
        let det = v["slots_detail"].as_array().unwrap();
        let d = det.iter().find(|e| e["id"] == dirt as i64).expect("dirt in detail");
        assert_eq!(d["name"], "minecraft:dirt");
        assert_eq!(d["name_zh"], "泥土");
        assert_eq!(d["category"], "方块");
        let _ = std::fs::remove_dir_all(st.drops.save_dir());
    }

    /// P51：所有按玩家定位的端点都接受 uuid；旧 username/conn_id 字段继续可用。
    #[test]
    fn endpoints_resolve_player_by_uuid() {
        let st = test_state();
        let name = "UuidUser";
        let uuid = mc_protocol::packets::stable_offline_uuid(name).to_string();
        let (tx, rx) = crossbeam_channel::unbounded();
        st.hub.register_with_uuid(
            71,
            tx,
            2,
            (0.0, 64.0, 0.0),
            name.into(),
            "uuid-addr".into(),
            uuid.clone(),
        );
        st.drops.load_inventory(71, name);
        // /api/online 增 uuid 字段，旧字段不删。
        let online = body_json(&route(&get("/api/online"), &st));
        assert_eq!(online["online"][0]["uuid"], uuid);
        assert_eq!(online["online"][0]["username"], name);
        // give by uuid（大小写/无连字符归一）→ 客户端收到全量背包。
        let compact = uuid.replace('-', "").to_uppercase();
        let r = route(
            &post("/api/give", &format!(r#"{{"uuid":"{compact}","item":"stone","count":3}}"#)),
            &st,
        );
        assert_eq!(r.status, 200);
        assert!(rx.try_recv().is_ok());
        // inventory / gamemode / tool-enchant / kick 都按 uuid 定位。
        let inv = body_json(&route(&get(&format!("/api/inventory?uuid={uuid}")), &st));
        assert_eq!(inv["conn_id"], 71);
        assert_eq!(inv["username"], name);
        let r = route(&post("/api/gamemode", &format!(r#"{{"uuid":"{uuid}","gamemode":"creative"}}"#)), &st);
        assert_eq!(r.status, 200);
        assert_eq!(body_json(&r)["username"], name);
        let r = route(
            &post("/api/tool-enchant", &format!(r#"{{"uuid":"{uuid}","enchant":"fortune"}}"#)),
            &st,
        );
        assert_eq!(r.status, 200);
        assert_eq!(st.drops.tool_enchant(71), mc_world::tools::ToolEnchant::Fortune);
        let (ktx, krx) = crossbeam_channel::unbounded();
        st.hub.register_with_uuid(
            72,
            ktx,
            2,
            (0.0, 64.0, 0.0),
            "KickByUuid".into(),
            "x".into(),
            mc_protocol::packets::stable_offline_uuid("KickByUuid").to_string(),
        );
        let kick_uuid = mc_protocol::packets::stable_offline_uuid("KickByUuid").to_string();
        let r = route(&post("/api/kick", &format!(r#"{{"uuid":"{kick_uuid}"}}"#)), &st);
        assert_eq!(r.status, 200);
        assert_eq!(body_json(&r)["kicked"], true);
        assert_eq!(krx.try_recv().unwrap().id, mc_network::ADMIN_KICK_PACKET_ID);
        // 未知 uuid 400；旧用户名/conn_id 仍生效。
        assert_eq!(route(&post("/api/kick", r#"{"uuid":"00000000-0000-0000-0000-000000000000"}"#), &st).status, 400);
        assert_eq!(route(&post("/api/gamemode", r#"{"username":"UuidUser","gamemode":"survival"}"#), &st).status, 200);
        let _ = std::fs::remove_dir_all(st.drops.save_dir());
    }

    /// P53：日期 → 天数映射（epoch=0，月份边界正确）。
    #[test]
    fn date_to_stat_day_maps_epoch_and_modern_dates() {
        assert_eq!(date_to_stat_day("1970-01-01"), Some(0));
        assert_eq!(date_to_stat_day("1970-01-02"), Some(1));
        assert_eq!(date_to_stat_day("2000-01-01"), Some(10957));
        assert_eq!(date_to_stat_day("2026-11-27"), Some(WorldStoreDay::day(2026, 11, 27)));
        assert_eq!(date_to_stat_day("2026-13-01"), None);
        assert_eq!(date_to_stat_day("bad"), None);
    }

    // 参考值独立算一次，避免测试与被测函数共用同一实现。
    struct WorldStoreDay;
    impl WorldStoreDay {
        fn day(y: i64, m: i64, d: i64) -> u32 {
            let mut total = 0i64;
            for year in 1970..y {
                total += if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 366 } else { 365 };
            }
            let days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            for month in 1..m {
                total += days[(month - 1) as usize];
                if month == 2 && y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                    total += 1;
                }
            }
            (total + d - 1) as u32
        }
    }

    /// P53 A1/A2/A7：`/api/xp`、`/api/online`、`/api/set-food`、`/api/stats` 全链路。
    #[test]
    fn p53_admin_endpoints_report_and_mutate_state() {
        let st = test_state();
        st.drops.set_p53_features(true, true, false, true);
        let uuid = mc_protocol::packets::stable_offline_uuid("P53Admin").to_string();
        let (tx, rx) = crossbeam_channel::unbounded();
        st.hub.register_with_uuid(81, tx, 2, (0.0, 64.0, 0.0), "P53Admin".into(), "x".into(), uuid.clone());
        st.drops.load_inventory_with_uuid(81, "P53Admin", &uuid);
        st.drops.set_player_identity(81, &uuid);

        let r = route(&post("/api/xp", r#"{"username":"P53Admin","amount":9}"#), &st);
        assert_eq!(r.status, 200);
        assert_eq!(body_json(&r)["total_xp"], 9);
        assert_eq!(rx.try_recv().unwrap().id, mc_protocol::packets::PLAY_SET_EXPERIENCE_S2C_ID);
        let r = route(&post("/api/set-food", r#"{"username":"P53Admin","food":7,"saturation":2.5}"#), &st);
        assert_eq!(r.status, 200);
        assert_eq!(rx.try_recv().unwrap().id, mc_protocol::packets::PLAY_SET_HEALTH_S2C_ID);
        let online = body_json(&route(&get("/api/online"), &st));
        let p = &online["online"][0];
        assert_eq!(p["xp"], 9);
        assert_eq!(p["food"], 7);
        assert_eq!(p["saturation"], 2.5);

        // 统计只本地累积，不发 awrard_stats（队列里没有 0x03）。
        st.drops.record_stat(81, mc_world::store::stat_id::MINED);
        st.drops.record_stat(81, mc_world::store::stat_id::DEATHS);
        let r = route(&get("/api/stats?uuid="), &st);
        assert_eq!(r.status, 400, "empty uuid is rejected");
        let r = route(&get(&format!("/api/stats?uuid={uuid}")), &st);
        assert_eq!(r.status, 200);
        let stats = body_json(&r)["stats"].as_array().unwrap().clone();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0]["stat_id"], mc_world::store::stat_id::MINED as i64);
        assert_eq!(stats[0]["count"], 1);
        assert!(rx.try_recv().is_err(), "A7 must not send packets");
        let _ = std::fs::remove_dir_all(st.drops.save_dir());
    }

    /// P53 A4：后台切模式/附魔后落盘，换 conn 同名/同 UUID 重进仍生效。
    #[test]
    fn p53_settings_persist_across_reconnect() {
        let st = test_state();
        st.drops.set_p53_features(false, false, true, false);
        st.drops.set_hub(Arc::clone(&st.hub)); // serve() 在生产启动期注入
        let uuid = mc_protocol::packets::stable_offline_uuid("Reconnect").to_string();
        st.hub.register_with_uuid(91, crossbeam_channel::unbounded().0, 2, (0.0, 64.0, 0.0), "Reconnect".into(), "x".into(), uuid.clone());
        st.drops.load_inventory_with_uuid(91, "Reconnect", &uuid);
        assert_eq!(route(&post("/api/gamemode", r#"{"username":"Reconnect","gamemode":"creative"}"#), &st).status, 200);
        assert_eq!(route(&post("/api/tool-enchant", r#"{"username":"Reconnect","enchant":"silk_touch"}"#), &st).status, 200);
        let disk = st.drops.with_inventory(91, |inv| (inv.persisted_gamemode(), inv.persisted_tool_enchant())).unwrap();
        assert_eq!(disk, (1, 2));
        st.drops.unload_inventory(91, "Reconnect");
        st.hub.unregister(91);
        // 同名 + 同 UUID 重进：以落盘值作为权威内存初值。
        st.hub.register_with_uuid(92, crossbeam_channel::unbounded().0, 2, (0.0, 64.0, 0.0), "Reconnect".into(), "x".into(), uuid.clone());
        st.drops.load_inventory_with_uuid(92, "Reconnect", &uuid);
        st.drops.apply_persisted_settings(92);
        assert_eq!(st.hub.gamemode(92), mc_network::broadcast::GameMode::Creative);
        assert_eq!(st.drops.tool_enchant(92), mc_world::tools::ToolEnchant::SilkTouch);
        let p = &body_json(&route(&get("/api/online"), &st))["online"][0];
        assert_eq!(p["persisted_gamemode"], "creative");
        assert_eq!(p["persisted_tool_enchant"], "silk_touch");
        let _ = std::fs::remove_dir_all(st.drops.save_dir());
    }
}
