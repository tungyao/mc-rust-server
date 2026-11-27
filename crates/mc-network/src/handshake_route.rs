//! handshake_route: 首包Handshake + 777校验 + 分流 Status/Login
use tokio::io::{AsyncRead, AsyncWrite};
use super::config::NetConfig;
use super::framing::{read_packet, to_io_invalid};
use super::login_route::{handle_login_with_options, PlayOptions};
use super::status_route::handle_status;
use super::NewConnection;
use crossbeam_channel::Sender;
use mc_protocol::packets::{Handshake, HandshakeNextState, HANDSHAKE_PACKET_ID};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{debug, warn};

/// 首包必须是 Handshake 0x00；只认 26.3/777；Status用&mut，Login拿走所有权。
/// `conn_id` 来自 accept，作为 ECS 实体键一路透传到 PlayEvent。
pub async fn handle_connection<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    addr: SocketAddr,
    conn_id: u64,
    tx: Sender<NewConnection>,
    config: NetConfig,
) -> io::Result<()> {
    let (packet_id, payload) = read_packet(&mut stream).await?;
    if packet_id != HANDSHAKE_PACKET_ID {
        warn!("{addr} expected handshake 0x00, got {packet_id:#04x}, disconnect");
        return Ok(());
    }
    let (hs, _) = Handshake::decode_body(&payload).map_err(to_io_invalid)?;
    if let Err(e) = mc_core::check_protocol(hs.protocol_version) {
        warn!("{addr} unsupported protocol {} ({e}), disconnect", hs.protocol_version);
        return Ok(());
    }
    match hs.next_state {
        HandshakeNextState::Status => {
            debug!("{addr} status handshake (server={}:{})", hs.server_address, hs.server_port);
            handle_status(&mut stream, addr, &config).await?;
        }
        HandshakeNextState::Login => {
            debug!("{addr} conn_id={conn_id} login handshake (server={}:{})", hs.server_address, hs.server_port);
            let opts = PlayOptions {
                threshold: config.compression_threshold,
                seed: config.seed,
                // 唯一有效视距计算：与 LoginPlay/初始区块流/ECS 自检一致。
                view_radius: config.effective_view_radius(),
                conn_id,
                events: config.play_events.clone(),
                chunk_cache: config.chunk_cache.clone(),
                mutable_world: config.mutable_world.clone(),
                drops: config.drops.clone(),
                hub: config.hub.clone(),
                fluid: config.fluid.clone(),
                player_visibility: config.player_visibility,
                destroy_stage: config.destroy_stage,
                tool_rules: config.tool_rules,
                xp_merge: config.xp_merge,
                hunger_detail: config.hunger_detail,
                settings_persist: config.settings_persist,
                stats: config.stats,
                craft_session_persist: config.craft_session_persist,
                workbench_gui: Arc::clone(&config.workbench_gui),
                workbench_menu_id: config.workbench_menu_id,
            };
            handle_login_with_options(stream, addr, &tx, opts).await?;
        }
    }
    Ok(())
}
