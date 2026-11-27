//! P54 后台联动 e2e：真实 redb + 真 HTTP 管理端口，覆盖时间/天气 API、
//! 广播限流参数、journal 裁剪、工作台会话持久化与清空。旧 admin_e2e 一字不改。
use mc_network::{BroadcastHub, DropState, NetConfig};
use mc_server::admin::{serve_admin_on, set_admin_extras, AdminState};
use mc_world::store::WorldStore;
use mc_world::world_time::WorldTimeState;
use mc_world::MutableWorld;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn http(admin: std::net::SocketAddr, method: &str, path: &str, body: &str) -> serde_json::Value {
    let mut s = TcpStream::connect(admin).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("http head");
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    assert_eq!(status, 200, "unexpected status {status}: {body}");
    serde_json::from_str(body).unwrap()
}

fn tmp_dir() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("mc-infra-admin-e2e-{}-{n}", std::process::id()))
}

#[tokio::test]
async fn p54_admin_apis_and_craft_session_roundtrip() {
    let dir = tmp_dir();
    let _ = std::fs::remove_dir_all(&dir);
    let seed = 777;
    let store = Arc::new(WorldStore::open(&dir, seed).unwrap());
    let world_time = Arc::new(WorldTimeState::from_store(Some(store.as_ref()), seed));
    // OnceLock：整个测试二进制只注入一次（与 main 启动期同口径）。
    set_admin_extras(Some(Arc::clone(&store)), Arc::clone(&world_time), true, 4, true);

    let hub = Arc::new(BroadcastHub::new());
    let mutable = Arc::new(MutableWorld::new(seed));
    let drops = Arc::new(DropState::with_store(dir.clone(), seed, Some(Arc::clone(&store))));
    drops.set_craft_session_persist(true);
    let _cfg = NetConfig::new(1, "p54-admin".into(), Arc::new(AtomicUsize::new(0))).with_hub(Arc::clone(&hub));
    let state = AdminState {
        hub: Arc::clone(&hub),
        drops: Arc::clone(&drops),
        mutable: Arc::clone(&mutable),
        seed,
        world_dir: dir.clone(),
        view_distance: 2,
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let sd = Arc::clone(&shutdown);
    std::thread::spawn(move || serve_admin_on(listener, state, sd));

    // A6 时间/天气：GET/POST 全链路 + meta 持久化（不发 0x27）。
    let w = http(addr, "GET", "/api/world", "").await;
    assert_eq!(w["ok"], true);
    assert_eq!(w["world_time"], 1000);
    assert_eq!(w["weather"], "clear");
    assert_eq!(w["broadcast_throttle"], false);
    assert_eq!(w["journal_trim"], true);
    assert_eq!(w["journal_keep"], 4);
    let t = http(addr, "POST", "/api/time", r#"{"preset":"night"}"#).await;
    assert_eq!(t["tick"], 13_000);
    assert_eq!(store.world_time().unwrap(), Some(13_000));
    let t = http(addr, "GET", "/api/time", "").await;
    assert_eq!(t["tick"], 13_000);
    assert_eq!(t["is_day"], false);
    let r = http(addr, "POST", "/api/weather", r#"{"weather":"rain","duration_ticks":12}"#).await;
    assert_eq!(r["weather"], "rain");
    assert!(r["weather_until"].as_i64().unwrap() > 0);
    assert_eq!(store.weather().unwrap(), Some(1));
    let r = http(addr, "GET", "/api/weather", "").await;
    assert_eq!(r["weather"], "rain");
    assert_eq!(r["weather_broadcast"], false);
    assert!(r["note"].as_str().unwrap().contains("no packet"));

    // A3/A5：纯服务端参数与裁剪端点。
    let r = http(addr, "POST", "/api/broadcast-throttle", r#"{"enabled":true,"max_queue":8}"#).await;
    assert_eq!(r["enabled"], true);
    assert_eq!(r["max_queue"], 8);
    assert_eq!(hub.throttle_enabled(), true);
    let r = http(addr, "POST", "/api/journal-trim", "{}").await;
    assert_eq!(r["ok"], true);
    assert_eq!(r["journal_seq"], 0);
    http(addr, "POST", "/api/broadcast-throttle", r#"{"enabled":false}"#).await;

    // A8：断开写盘 → 重进恢复 → API 展示 persisted/owner/last_saved → 清空。
    let uuid = "550e8400-e29b-41d4-a716-446655440000";
    hub.register_with_uuid(
        9, crossbeam_channel::unbounded().0, 2, (0.0, 64.0, 0.0),
        "Crafter".into(), "127.0.0.1:1".into(), uuid.into(),
    );
    drops.load_inventory_with_uuid(9, "Crafter", uuid);
    drops.open_craft9(9, 3, 70, 4);
    drops.with_craft9(9, |s| s.grid[2] = Some((7, 5)));
    drops.unload_inventory(9, "Crafter");
    // 重连恢复后 API 快照带落盘标记。
    hub.register_with_uuid(
        10, crossbeam_channel::unbounded().0, 2, (0.0, 64.0, 0.0),
        "Crafter".into(), "127.0.0.1:2".into(), uuid.into(),
    );
    drops.load_inventory_with_uuid(10, "Crafter", uuid);
    assert_eq!(drops.craft9_session(10).unwrap().grid[2], Some((7, 5)));
    let c = http(addr, "GET", "/api/craft9", "").await;
    assert_eq!(c["persist_enabled"], true);
    let s = &c["sessions"][0];
    assert_eq!(s["owner"], uuid);
    assert_eq!(s["persisted"], true);
    assert!(s["last_saved"].as_u64().unwrap() > 0);
    let r = http(addr, "POST", "/api/craft9-clear", &format!(r#"{{"uuid":"{uuid}"}}"#)).await;
    assert_eq!(r["removed"], true);
    assert_eq!(drops.craft9_persisted_info(uuid).0, false);

    shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _ = std::fs::remove_dir_all(&dir);
}
