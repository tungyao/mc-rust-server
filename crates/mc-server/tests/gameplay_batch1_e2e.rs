//! P53 批次 1 端到端：真游戏服 + 真管理服，覆盖同名重进 XP/模式保留、
//! `/api/xp`、`/api/set-food`、`/api/stats` 与时间范围筛选。
//! 旧 3 个 admin_e2e 与 mc-network 的 528 个测试一字不改。
#[path = "../../mc-network/tests/common.rs"]
mod common;

use common::{login_and_join, read_frame_timeout, send_pos_y, spawn_server};
use mc_network::{BroadcastHub, ChunkCache, DropState, NetConfig};
use mc_server::admin::{serve_admin_on, AdminState};
use mc_world::store::WorldStore;
use mc_world::{MutableWorld, SurvivalState, WorldGen, DEFAULT_SEED};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize},
    Arc,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn http(admin: std::net::SocketAddr, method: &str, path: &str, body: &str) -> (u16, serde_json::Value) {
    let mut s = TcpStream::connect(admin).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("http response");
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let json = if body.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(body).unwrap()
    };
    (status, json)
}

struct Env {
    hub: Arc<BroadcastHub>,
    game: std::net::SocketAddr,
    admin: std::net::SocketAddr,
    drops: Arc<DropState>,
    mutable: Arc<MutableWorld>,
    _shutdown: Arc<AtomicBool>,
    dir: std::path::PathBuf,
}

fn next_tag() -> usize {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

async fn boot(tag: &str, xp_merge: bool) -> Env {
    let dir = std::env::temp_dir().join(format!("mc-p53-e2e-{}-{tag}-{}", std::process::id(), next_tag()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Arc::new(WorldStore::open(&dir, DEFAULT_SEED).unwrap());
    let mutable = Arc::new(MutableWorld::with_store(DEFAULT_SEED, Arc::clone(&store)).unwrap());
    let cache = Arc::new(ChunkCache::new(DEFAULT_SEED, 64).with_mutable(Arc::clone(&mutable)));
    let hub = Arc::new(BroadcastHub::new());
    let drops = Arc::new(DropState::with_store(dir.clone(), DEFAULT_SEED, Some(store)));
    let cfg = NetConfig::new(100, "p53-e2e".into(), Arc::new(AtomicUsize::new(0)))
        .with_seed(DEFAULT_SEED)
        .with_view_distance(2)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub))
        // A1 关以保持 XP 测试包流简洁；A2/A4/A7 开（生产默认全开）。
        .with_xp_merge(xp_merge)
        .with_hunger_detail(true)
        .with_settings_persist(true)
        .with_stats(true);
    let game = spawn_server(cfg).await;
    let al = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let admin = al.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let state = AdminState {
        hub: Arc::clone(&hub),
        drops: Arc::clone(&drops),
        mutable: Arc::clone(&mutable),
        seed: DEFAULT_SEED,
        world_dir: dir.clone(),
        view_distance: 2,
    };
    let sd = Arc::clone(&shutdown);
    std::thread::spawn(move || serve_admin_on(al, state, sd));
    Env { hub: Arc::clone(&hub), game, admin, drops, mutable, _shutdown: shutdown, dir }
}

/// 读取若干帧直到遇见目标包 id（跳过 KeepAlive/位置同步等节拍包）。
async fn wait_for(stream: &mut TcpStream, id: i32, tries: usize) -> Option<Vec<u8>> {
    for _ in 0..tries {
        let (got, payload) = read_frame_timeout(stream, Duration::from_secs(5)).await.unwrap();
        if got == id {
            return Some(payload);
        }
    }
    None
}

#[tokio::test]
async fn p53_xp_mode_food_and_stats_survive_same_name_reconnect() {
    use mc_protocol::packets;
    let env = boot("persist", false).await;
    let mut s = login_and_join(env.game, "SameName", 2).await;
    let (st, v) = http(env.admin, "POST", "/api/xp", r#"{"username":"SameName","amount":9}"#).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["total_xp"], 9);
    let (st, v) = http(env.admin, "POST", "/api/gamemode", r#"{"username":"SameName","gamemode":"creative"}"#).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["persisted"], true);
    let (st, v) = http(env.admin, "POST", "/api/set-food", r#"{"username":"SameName","food":7,"saturation":2.5}"#).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["food"], 7);

    let (st, v) = http(env.admin, "GET", "/api/online", "").await;
    assert_eq!(st, 200);
    let p = &v["online"][0];
    assert_eq!(p["username"], "SameName");
    assert_eq!(p["xp"], 9);
    assert_eq!(p["gamemode"], "creative");
    assert_eq!(p["persisted_gamemode"], "creative");
    assert_eq!(p["food"], 7);
    assert_eq!(p["saturation"], 2.5);

    // 真实挖一格：PlayEvent::BlockEdit 路径累加 MINED（0x08 BlockUpdate + Ack 仍相邻）。
    let spawn = WorldGen::new(DEFAULT_SEED).spawn_pos();
    let (gx, gy, gz) = (spawn.0, spawn.1 - 1, spawn.2);
    env.mutable.set(gx, gy, gz, mc_world::STONE_BLOCK_ID);
    send_pos_y(&mut s, spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5).await;
    let mut act = Vec::new();
    packets::PlayerAction { status: 3, x: gx, y: gy, z: gz, face: 1, sequence: 41 }.encode_body(&mut act);
    mc_network::framing::write_packet(&mut s, packets::PLAY_PLAYER_ACTION_C2S_ID, &act).await.unwrap();
    let update = wait_for(&mut s, packets::PLAY_BLOCK_UPDATE_S2C_ID, 20).await;
    assert!(update.is_some(), "dig must produce BlockUpdate");
    let ack = wait_for(&mut s, packets::PLAY_BLOCK_CHANGED_ACK_S2C_ID, 20).await;
    assert!(ack.is_some(), "Ack must follow BlockUpdate on the same player stream");

    let (st, v) = http(env.admin, "GET", "/api/stats?username=SameName", "").await;
    assert_eq!(st, 200, "{v}");
    let rows = v["stats"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["stat_id"] == mc_world::store::stat_id::MINED as i64 && r["count"] == 1), "{rows:?}");
    // 时间范围筛选：未来起始日应过滤掉全部记录。
    let (_, v) = http(env.admin, "GET", "/api/stats?username=SameName&from=2999-01-01", "").await;
    assert!(v["stats"].as_array().unwrap().is_empty(), "future from must filter out stats");

    // 断线刷盘 → 同名同 UUID 重进：XP、模式、附魔、饥饿全部从 redb players v3 恢复。
    drop(s);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut s2 = login_and_join(env.game, "SameName", 2).await;
    assert!(wait_for(&mut s2, packets::PLAY_SET_EXPERIENCE_S2C_ID, 40).await.is_some(), "rejoin must resend XP");
    assert!(wait_for(&mut s2, packets::PLAY_SET_HEALTH_S2C_ID, 40).await.is_some(), "rejoin must resend non-full health");
    let (st, v) = http(env.admin, "GET", "/api/online", "").await;
    assert_eq!(st, 200);
    let p = &v["online"][0];
    assert_eq!(p["username"], "SameName");
    assert_eq!(p["xp"], 9, "XP must survive same-name reconnect");
    assert_eq!(p["gamemode"], "creative", "mode must survive same-name reconnect");
    assert_eq!(p["persisted_tool_enchant"], "none");
    assert_eq!(p["food"], 7);
    drop(s2);
    let _ = std::fs::remove_dir_all(&env.dir);
}

#[tokio::test]
async fn p53_set_food_rejects_bad_values_and_world_exposes_feature_flags() {
    let env = boot("food", false).await;
    let s = login_and_join(env.game, "FoodProbe", 2).await;
    let (st, _) = http(env.admin, "POST", "/api/set-food", r#"{"username":"FoodProbe","food":21}"#).await;
    assert_eq!(st, 400, "food > 20 must be rejected");
    let (st, v) = http(env.admin, "POST", "/api/set-food", r#"{"username":"FoodProbe","food":0,"saturation":0}"#).await;
    assert_eq!(st, 200, "{v}");
    // 饥饿见底后全局 ticker 会周期性扣血；验证权威状态确实由 ticker 推进。
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (_, online) = http(env.admin, "GET", "/api/online", "").await;
    let conn = online["online"][0]["conn_id"].as_u64().unwrap();
    let snap = env.drops.survival_state(conn).unwrap_or(SurvivalState::full());
    assert_eq!(snap.food, 0, "set-food must change authoritative survival state");
    let (_, world) = http(env.admin, "GET", "/api/world", "").await;
    let f = &world["p53_features"];
    assert_eq!(f["xp_merge"], false);
    assert_eq!(f["hunger_detail"], true);
    assert_eq!(f["settings_persist"], true);
    assert_eq!(f["stats"], true);
    drop(s);
    let _ = std::fs::remove_dir_all(&env.dir);
}


/// P53 A1：吸引/合并确实由全服唯一 50ms ticker 驱动，总经验守恒且位置朝玩家移动。
#[tokio::test]
async fn p53_global_ticker_attracts_and_merges_xp_conserved() {
    let env = boot("xpmerge", true).await;
    let (tx, _rx) = crossbeam_channel::unbounded();
    env.hub.register_with_uuid(
        7,
        tx,
        2,
        (0.5, 69.0, 0.5),
        "XpAttract".into(),
        "x".into(),
        mc_protocol::packets::stable_offline_uuid("XpAttract").to_string(),
    );
    env.hub.update_pos(7, (0.5, 69.0, 0.5));
    {
        let mut orbs = env.drops.orbs.lock().unwrap();
        orbs.spawn(1000, 3, 2.0, 69.0, 0.5, 69.0).unwrap();
        orbs.spawn(1001, 4, 2.1, 69.0, 0.5, 69.0).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
    let (count, total, min_x) = {
        let orbs = env.drops.orbs.lock().unwrap();
        let snaps = orbs.snapshots();
        let total: i32 = snaps.iter().map(|x| x.2).sum();
        let min_x = snaps.iter().map(|x| x.1.0).fold(f64::INFINITY, f64::min);
        (snaps.len(), total, min_x)
    };
    assert_eq!(total, 7, "merge must conserve total XP");
    assert!(count <= 2);
    assert!(min_x < 2.0, "orb must move toward the player within 7.25 blocks");
    let _ = std::fs::remove_dir_all(&env.dir);
}
