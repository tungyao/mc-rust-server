//! status: Status Request/Response + Ping/Pong + status_json，只兼容26.3/777
use crate::packets::codec::{decode_string, encode_packet, encode_string, read_i64_be, write_i64_be};
use crate::ProtocolError;

/// Status Request 包 ID（Status serverbound `0x00`）。
pub const STATUS_REQUEST_PACKET_ID: i32 = 0x00;
/// Status Response 包 ID（Status clientbound `0x00`）。
pub const STATUS_RESPONSE_PACKET_ID: i32 = 0x00;
/// Ping Request 包 ID（Status serverbound `0x01`）。
pub const PING_PACKET_ID: i32 = 0x01;
/// Pong 包 ID（Status clientbound `0x01`）。
pub const PONG_PACKET_ID: i32 = 0x01;

pub fn encode_status_request_packet(out: &mut Vec<u8>) {
    encode_packet(STATUS_REQUEST_PACKET_ID, &[], out);
}

/// Status Request 解码：按 wiki 无字段，尾部字节直接忽略（兼容性优先）。
pub fn decode_status_request(_payload: &[u8]) -> Result<(), ProtocolError> {
    Ok(())
}

/// Status Response（clientbound `0x00`，单个 String JSON）打包成完整线包。
pub fn encode_status_response_packet(json: &str, out: &mut Vec<u8>) {
    let mut payload = Vec::new();
    encode_string(json, &mut payload);
    encode_packet(STATUS_RESPONSE_PACKET_ID, &payload, out);
}

/// Status Response 解码，返回 `(json, 消费字节数)`。
pub fn decode_status_response(payload: &[u8]) -> Result<(String, usize), ProtocolError> {
    decode_string(payload)
}

/// Ping Request（serverbound `0x01`，i64 big-endian）打包成完整线包。
pub fn encode_ping_request_packet(payload: i64, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_i64_be(payload, &mut body);
    encode_packet(PING_PACKET_ID, &body, out);
}

/// Ping Request 解码，要求 payload 恰好 8 字节。
pub fn decode_ping_request(payload: &[u8]) -> Result<i64, ProtocolError> {
    if payload.len() != 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let (v, _) = read_i64_be(payload)?;
    Ok(v)
}

/// Pong（clientbound `0x01`，i64 big-endian，原样回显 Ping 的 payload）打包成完整线包。
pub fn encode_pong_packet(payload: i64, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_i64_be(payload, &mut body);
    encode_packet(PONG_PACKET_ID, &body, out);
}

/// Pong 解码，要求 payload 恰好 8 字节。
pub fn decode_pong_payload(payload: &[u8]) -> Result<i64, ProtocolError> {
    if payload.len() != 8 {
        return Err(ProtocolError::UnexpectedEof);
    }
    let (v, _) = read_i64_be(payload)?;
    Ok(v)
}

pub fn status_json(max_players: usize, online: usize, motd: &str) -> String {
    format!(
        r#"{{"version":{{"name":"{}","protocol":{}}},"players":{{"max":{},"online":{}}},"description":{{"text":"{}"}}}}"#,
        mc_core::MC_VERSION,
        mc_core::PROTOCOL_VERSION,
        max_players,
        online,
        motd
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_varint, write_varint};
    use crate::packets::codec::decode_packet_id;
    #[test]
    fn status_json_has_version() {
        let j = status_json(100, 0, "motd");
        assert!(j.contains("26.3")); assert!(j.contains("777"));
    }
    #[test]
    fn status_response_roundtrip() {
        let json = status_json(10, 1, "hi");
        let mut pkt = Vec::new();
        encode_status_response_packet(&json, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, STATUS_RESPONSE_PACKET_ID);
        assert_eq!(decode_status_response(&pkt[n+m..]).unwrap().0, json);
        let _ = write_varint(0, &mut Vec::new());
    }
    #[test]
    fn ping_pong_roundtrip() {
        for v in [0i64, 12345, -1, i64::MIN, i64::MAX] {
            let mut a = Vec::new(); encode_ping_request_packet(v, &mut a);
            let (_, n) = read_varint(&a).unwrap();
            let (id, m) = decode_packet_id(&a[n..]).unwrap();
            assert_eq!(id, PING_PACKET_ID);
            assert_eq!(decode_ping_request(&a[n+m..]).unwrap(), v);
            let mut b = Vec::new(); encode_pong_packet(v, &mut b);
            let (_, n2) = read_varint(&b).unwrap();
            let (id2, m2) = decode_packet_id(&b[n2..]).unwrap();
            assert_eq!(id2, PONG_PACKET_ID);
            assert_eq!(decode_pong_payload(&b[n2+m2..]).unwrap(), v);
        }
    }
}
