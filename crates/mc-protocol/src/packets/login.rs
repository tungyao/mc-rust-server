//! login: Login Start/Success/Ack + 压缩占位，只兼容26.3/777（离线模式）
use crate::packets::codec::{decode_string, encode_packet, encode_string};
use crate::{read_varint, write_varint, ProtocolError};
use uuid::Uuid;

/// Login Start 包 ID（Login serverbound `0x00`）。
pub const LOGIN_START_PACKET_ID: i32 = 0x00;
/// Login Success 包 ID（Login clientbound `0x02`）。
pub const LOGIN_SUCCESS_PACKET_ID: i32 = 0x02;
/// Login Acknowledged 包 ID（Login serverbound `0x03`，空包）。
pub const LOGIN_ACKNOWLEDGED_PACKET_ID: i32 = 0x03;
/// Set Compression 包 ID（Login clientbound `0x03`，与 Ack 同值但方向相反）。
pub const SET_COMPRESSION_PACKET_ID: i32 = 0x03;
/// Encryption Request 包 ID（Login clientbound `0x01`，P3 实现正版验证时再用）。
pub const ENCRYPTION_REQUEST_PACKET_ID: i32 = 0x01;

/// 压缩禁用阈值占位：`-1` 表示不启用压缩、不发送 Set Compression。
pub const COMPRESSION_DISABLED_THRESHOLD: i32 = -1;

/// 离线模式 UUID：随机 v4。
pub fn offline_uuid(_name: &str) -> Uuid { Uuid::new_v4() }

/// UUID 按线包格式读写：16 字节 big-endian。
pub fn write_uuid(v: Uuid, out: &mut Vec<u8>) { out.extend_from_slice(v.as_bytes()); }
pub fn read_uuid(buf: &[u8]) -> Result<(Uuid, usize), ProtocolError> {
    if buf.len() < 16 { return Err(ProtocolError::UnexpectedEof); }
    let mut b = [0u8; 16]; b.copy_from_slice(&buf[..16]);
    Ok((Uuid::from_bytes(b), 16))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    pub name: String,
    pub uuid: Uuid,
}

impl LoginStart {
    /// 编码包体（不含 length + packet id）。
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.name, out);
        write_uuid(self.uuid, out);
    }

    /// 解码包体，返回 `(包, 消费字节数)`。尾部多余字节忽略。
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (name, n1) = decode_string(buf)?;
        let (uuid, n2) = read_uuid(&buf[n1..])?;
        Ok((Self { name, uuid }, n1 + n2))
    }
}

/// Login Start 打成完整线包（服务端主要做 decode，encode 供单测/模拟客户端用）。
pub fn encode_login_start_packet(v: &LoginStart, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(LOGIN_START_PACKET_ID, &body, out);
}

/// Login Start 解码（去掉 packet id 后的 payload）。
pub fn decode_login_start(payload: &[u8]) -> Result<(LoginStart, usize), ProtocolError> {
    LoginStart::decode_body(payload)
}

/// Login Success（clientbound `0x02`）：`UUID` + `String username`。
/// NOTE: 777 原版尾部还有 `VarInt 属性数 + 属性表`；离线占位只发 UUID+name，
/// P3 按 26.3 抓包补齐属性表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess {
    pub uuid: Uuid,
    pub username: String,
}

impl LoginSuccess {
    /// 编码包体（不含 length + packet id）。
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_uuid(self.uuid, out);
        encode_string(&self.username, out);
    }

    /// 解码包体，返回 `(包, 消费字节数)`。尾部多余字节（P3 的属性表）忽略。
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (uuid, n1) = read_uuid(buf)?;
        let (username, n2) = decode_string(&buf[n1..])?;
        Ok((Self { uuid, username }, n1 + n2))
    }
}

/// Login Success 打成完整线包。
pub fn encode_login_success_packet(v: &LoginSuccess, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    v.encode_body(&mut body);
    encode_packet(LOGIN_SUCCESS_PACKET_ID, &body, out);
}

/// Login Success 解码（去掉 packet id 后的 payload）。
pub fn decode_login_success(payload: &[u8]) -> Result<(LoginSuccess, usize), ProtocolError> {
    LoginSuccess::decode_body(payload)
}

/// Login Acknowledged（serverbound `0x03`）打成完整空包（供单测/模拟客户端用）。
pub fn encode_login_acknowledged_packet(out: &mut Vec<u8>) {
    encode_packet(LOGIN_ACKNOWLEDGED_PACKET_ID, &[], out);
}

/// Login Acknowledged 解码：按 wiki 无字段，尾部字节直接忽略（兼容性优先，
/// 与 [`decode_status_request`] 同策略）。
pub fn decode_login_acknowledged(_payload: &[u8]) -> Result<(), ProtocolError> {
    Ok(())
}

/// Set Compression（Login clientbound `0x03`，`VarInt threshold`）打成完整线包。
/// 占位函数：当前离线流程阈值固定 [`COMPRESSION_DISABLED_THRESHOLD`]（`-1`），
/// 不发送；P3 启用压缩时在 Login Success 之前发送阈值 `>= 0` 的包。
pub fn encode_set_compression_packet(threshold: i32, out: &mut Vec<u8>) {
    let mut body = Vec::new();
    write_varint(threshold, &mut body);
    encode_packet(SET_COMPRESSION_PACKET_ID, &body, out);
}

/// Set Compression 解码，返回阈值。
pub fn decode_set_compression(payload: &[u8]) -> Result<i32, ProtocolError> {
    let (threshold, _) = read_varint(payload)?;
    Ok(threshold)
}

// P3 预留：Encryption Request（Login clientbound `0x01`）=
// `String server_id` + `VarInt len + public_key` + `VarInt len + verify_token`。
// 离线模式跳过整个加密握手：不发送 Request，不处理 serverbound `0x01`
// Encryption Response；做正版验证时在此处插入 Mojang 会话校验+共享密钥协商。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_varint;
    use crate::packets::codec::decode_packet_id;
    #[test]
    fn login_start_roundtrip() {
        let v = LoginStart { name: "Steve".into(), uuid: Uuid::new_v4() };
        let mut pkt = Vec::new(); encode_login_start_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, LOGIN_START_PACKET_ID);
        let (got, _) = decode_login_start(&pkt[n+m..]).unwrap();
        assert_eq!(got, v);
    }
    #[test]
    fn login_start_rejects_truncated() {
        let v = LoginStart { name: "Steve".into(), uuid: Uuid::new_v4() };
        let mut body = Vec::new(); v.encode_body(&mut body);
        assert!(decode_login_start(&body[..2]).is_err());
    }
    #[test]
    fn login_success_roundtrip() {
        let v = LoginSuccess { uuid: Uuid::new_v4(), username: "Alex".into() };
        let mut pkt = Vec::new(); encode_login_success_packet(&v, &mut pkt);
        let (_, n) = read_varint(&pkt).unwrap();
        let (id, m) = decode_packet_id(&pkt[n..]).unwrap();
        assert_eq!(id, LOGIN_SUCCESS_PACKET_ID);
        let (got, _) = decode_login_success(&pkt[n+m..]).unwrap();
        assert_eq!(got, v);
    }
    #[test]
    fn login_ack_is_empty() {
        let mut b = Vec::new(); encode_login_acknowledged_packet(&mut b);
        let (_, n) = read_varint(&b).unwrap();
        let (id, m) = decode_packet_id(&b[n..]).unwrap();
        assert_eq!(id, LOGIN_ACKNOWLEDGED_PACKET_ID);
        decode_login_acknowledged(&b[n+m..]).unwrap();
    }
    #[test]
    fn compression_disabled_default() {
        assert_eq!(COMPRESSION_DISABLED_THRESHOLD, -1);
        let mut b = Vec::new(); encode_set_compression_packet(-1, &mut b);
        assert_eq!(decode_set_compression(&b[read_varint(&b).unwrap().1+1..]).unwrap(), -1);
        let _ = write_varint(0, &mut Vec::new());
    }
    #[test]
    fn offline_uuid_is_random_v4() {
        assert_ne!(offline_uuid("a"), offline_uuid("a"));
    }
}
