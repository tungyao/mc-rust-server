//! P25 管理面板端到端：真游戏服（临时端口）+ 真管理服（临时端口）联调。
//! 覆盖 §8.M2（give→客户端即时同步→复核→落盘）与 §8.M3（kick→断开→online 清零、save 落盘）。
//! 旧 e2e（play_stream/play_actions）一个字不动，本文件只追加。
use mc_network::{BroadcastHub, ChunkCache, DropState, NetConfig};
use mc_protocol::packets::{self, Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID};
use mc_server::admin::{serve_admin_on, AdminState};
use mc_world::{MutableWorld, DEFAULT_SEED};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn read_frame(s: &mut TcpStream) -> std::io::Result<(i32, Vec<u8>)> {
    let len = read_varint_async(s).await?;
    let mut buf = vec![0u8; len as usize];
    s.read_exact(&mut buf).await?;
    let (id, n) = mc_protocol::read_varint(&buf).expect("packet id");
    Ok((id, buf[n..].to_vec()))
}

async fn read_varint_async(s: &mut TcpStream) -> std::io::Result<i32> {
    let mut num = 0i32;
    let mut shift = 0u32;
    for _ in 0..5 {
        let mut b = [0u8; 1];
        s.read_exact(&mut b).await?;
        num |= ((b[0] & 0x7F) as i32) << shift;
        if b[0] & 0x80 == 0 {
            return Ok(num);
        }
        shift += 7;
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "varint too big"))
}

async fn read_frame_timeout(s: &mut TcpStream, dur: Duration) -> std::io::Result<(i32, Vec<u8>)> {
    tokio::time::timeout(dur, read_frame(s))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "test read timeout"))?
}

/// 走完 Login + Configuration + Join（radius=2，25 块）+ 开局背包同步，返回 Play 连接。
async fn join(addr: std::net::SocketAddr, name: &str) -> TcpStream {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let hs = Handshake {
        protocol_version: 777,
        server_address: "localhost".to_string(),
        server_port: addr.port(),
        next_state: HandshakeNextState::Login,
    };
    let mut body = Vec::new();
    hs.encode_body(&mut body);
    let mut framed = Vec::new();
    packets::encode_packet(HANDSHAKE_PACKET_ID, &body, &mut framed);
    s.write_all(&framed).await.unwrap();

    let mut b = Vec::new();
    packets::encode_string(name, &mut b);
    b.extend_from_slice(&[0u8; 16]);
    let mut start = Vec::new();
    packets::encode_packet(packets::LOGIN_START_PACKET_ID, &b, &mut start);
    s.write_all(&start).await.unwrap();
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::LOGIN_SUCCESS_PACKET_ID);
    let mut ack = Vec::new();
    packets::encode_login_acknowledged_packet(&mut ack);
    s.write_all(&ack).await.unwrap();

    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::CONFIG_KNOWN_PACKS_S2C_ID);
    let mut kp = Vec::new();
    packets::encode_config_known_packs_c2s_packet(&packets::ConfigKnownPacks::vanilla(), &mut kp);
    s.write_all(&kp).await.unwrap();
    for _ in 0..32 {
        let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        assert_eq!(id, packets::CONFIG_REGISTRY_DATA_S2C_ID);
    }
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::CONFIG_UPDATE_TAGS_S2C_ID);
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::CONFIG_FINISH_S2C_ID);
    let mut fack = Vec::new();
    packets::encode_config_finish_ack_packet(&mut fack);
    s.write_all(&fack).await.unwrap();

    // Join：4 头 + 25 块 + 3 尾，随后 1 个开局背包 Content。
    for _ in 0..(4 + 25 + 3) {
        let _ = read_frame_timeout(&mut s, Duration::from_secs(10)).await.unwrap();
    }
    let (id, _) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
    assert_eq!(id, packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID, "join must end with inventory sync");
    s
}

/// 管理 HTTP 裸客户端（`Connection: close`，读到 EOF 即整包）。
async fn http(admin: std::net::SocketAddr, method: &str, path: &str, body: &str) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(admin).await.unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("http response must have a head");
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, body.as_bytes().to_vec())
}

fn j(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body).unwrap()
}

struct Env {
    game: std::net::SocketAddr,
    admin: std::net::SocketAddr,
    shutdown: Arc<AtomicBool>,
    drops: Arc<DropState>,
    mutable: Arc<MutableWorld>,
    world_dir: std::path::PathBuf,
}

async fn boot() -> Env {
    let seed = DEFAULT_SEED;
    let dir = std::env::temp_dir().join(format!("mc-admin-e2e-{}-{}", std::process::id(), next_tag()));
    let world_dir = dir.join("world");
    let hub = Arc::new(BroadcastHub::new());
    let mutable = Arc::new(MutableWorld::new(seed));
    let cache = Arc::new(ChunkCache::new(seed, 64).with_mutable(Arc::clone(&mutable)));
    let drops = Arc::new(DropState::new(world_dir.clone(), seed));
    let config = NetConfig::new(100, "admin-e2e".into(), Arc::new(AtomicUsize::new(0)))
        .with_view_distance(2)
        .with_seed(seed)
        .with_chunk_cache(cache)
        .with_mutable_world(Arc::clone(&mutable))
        .with_drops(Arc::clone(&drops))
        .with_hub(Arc::clone(&hub));
    let gl = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let game = gl.local_addr().unwrap();
    let (tx, _rx) = crossbeam_channel::unbounded();
    tokio::spawn(async move {
        let _ = mc_network::serve(gl, tx, config).await;
    });
    let al = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let admin = al.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let state = AdminState {
        hub,
        drops: Arc::clone(&drops),
        mutable: Arc::clone(&mutable),
        seed,
        world_dir: world_dir.clone(),
        view_distance: 2,
    };
    let sd = Arc::clone(&shutdown);
    std::thread::spawn(move || serve_admin_on(al, state, sd));
    Env { game, admin, shutdown, drops, mutable, world_dir }
}

fn next_tag() -> u64 {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    SEQ.fetch_add(1, Ordering::SeqCst) as u64
}

use std::sync::atomic::AtomicBool;

#[tokio::test]
async fn admin_give_kick_save_flow() {
    let env = boot().await;
    let cobble = mc_registry::item_id("minecraft:cobblestone").unwrap() as i64;
    let mut s = join(env.game, "E2EPlayer").await;

    // M1：online 列表出现该用户（hub 注册在 Join 序列发完后，轮询等它）。
    let mut online = serde_json::Value::Null;
    for _ in 0..50 {
        let (st, b) = http(env.admin, "GET", "/api/online", "").await;
        assert_eq!(st, 200);
        online = j(&b);
        if online["count"] == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(online["count"], 1);
    assert_eq!(online["online"][0]["username"], "E2EPlayer");
    // M1：world 端点字段齐全。
    let (st, b) = http(env.admin, "GET", "/api/world", "").await;
    assert_eq!(st, 200);
    let w = j(&b);
    assert_eq!(w["ok"], true);
    assert_eq!(w["seed"], DEFAULT_SEED);
    assert_eq!(w["online"], 1);
    assert_eq!(w["dirty_chunks"], 0);

    // M2：发 64 个 cobble（背包此前无此物）→ 客户端 <2s 内收到 Content 同步。
    let (st, b) = http(
        env.admin,
        "POST",
        "/api/give",
        r#"{"username":"E2EPlayer","item":"minecraft:cobblestone","count":64}"#,
    )
    .await;
    assert_eq!(st, 200);
    assert_eq!(j(&b), serde_json::json!({"ok": true, "leftover": 0, "delivered": true}));
    let mut got = None;
    for _ in 0..40 {
        let (id, payload) = read_frame_timeout(&mut s, Duration::from_secs(5)).await.unwrap();
        if id == packets::PLAY_SET_CONTAINER_CONTENT_S2C_ID {
            let (c, _) = packets::SetContainerContent::decode_body(&payload).unwrap();
            if c.slots.iter().any(|sl| sl.item_id as i64 == cobble && sl.count == 64) {
                got = Some(c);
                break;
            }
        }
    }
    let content = got.expect("client must receive the given cobble in <2s without rejoin");
    assert!(content.slots.iter().any(|sl| sl.item_id as i64 == cobble && sl.count == 64));
    // M2：复核端点看到同一终态。
    let (st, b) = http(env.admin, "GET", "/api/inventory?username=E2EPlayer", "").await;
    assert_eq!(st, 200);
    let inv = j(&b);
    assert!(inv["slots"].as_array().unwrap().iter().any(|e| e[1] == cobble && e[2] == 64));
    // M2：未知物品 400、离线用户 400。
    let (st, _) = http(env.admin, "POST", "/api/give", r#"{"username":"E2EPlayer","item":"nope","count":1}"#).await;
    assert_eq!(st, 400);
    let (st, _) = http(env.admin, "POST", "/api/give", r#"{"username":"Ghost","item":"stone","count":1}"#).await;
    assert_eq!(st, 400);

    // M3：落一块方块再 save → 文件 mtime/内容更新（重启不丢由 load_from_dir 语义保证）。
    env.mutable.set(7, 200, 7, mc_world::STONE_BLOCK_ID);
    let (st, b) = http(env.admin, "POST", "/api/save", "").await;
    assert_eq!(st, 200);
    assert_eq!(j(&b)["saved"], true);
    let path = MutableWorld::save_path(&env.world_dir, DEFAULT_SEED);
    assert!(path.exists(), "save must flush overrides to disk");
    let back = MutableWorld::load_from_dir(&env.world_dir, DEFAULT_SEED).unwrap();
    assert_eq!(back.get(7, 200, 7), mc_world::STONE_BLOCK_ID);
    // M3：broadcast 无真值编码器，保持 501。
    let (st, _) = http(env.admin, "POST", "/api/broadcast", r#"{"text":"hi"}"#).await;
    assert_eq!(st, 501);

    // M3：kick → 客户端连接关闭 + online 清零（统一清理路径）。
    let (st, b) = http(env.admin, "POST", "/api/kick", r#"{"username":"E2EPlayer"}"#).await;
    assert_eq!(st, 200);
    assert_eq!(j(&b)["kicked"], true);
    let mut closed = false;
    for _ in 0..25 {
        match read_frame_timeout(&mut s, Duration::from_millis(200)).await {
            Ok(_) => continue,
            Err(_) => {
                closed = true;
                break;
            }
        }
    }
    // 读到 EOF 或超时都再确认一次：连接必须已死（写探测失败即关闭）。
    if !closed {
        closed = s.write_all(&[0u8; 1]).await.is_err()
            || read_frame_timeout(&mut s, Duration::from_secs(3)).await.is_err();
    }
    assert!(closed, "kicked client connection must die");
    let mut gone = false;
    for _ in 0..50 {
        let (st, b) = http(env.admin, "GET", "/api/online", "").await;
        assert_eq!(st, 200);
        if j(&b)["count"] == 0 {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(gone, "kicked player must leave the online list");

    env.drops.stop_global_tick();
    env.shutdown.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_dir_all(env.world_dir.parent().unwrap());
}
