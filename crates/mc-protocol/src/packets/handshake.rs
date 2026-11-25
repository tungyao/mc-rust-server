//! handshake: Handshaking serverbound 0x00，只兼容26.3/777
use crate::packets::codec::{decode_string, encode_string, read_u16_be, write_u16_be};
use crate::{read_varint, write_varint, ProtocolError};

/// Handshake 包 ID（Handshaking state，serverbound `intention` = `0x00`）。
/// 编译期从 `mc-registry` 生成的 PACKETS 表派生。
pub const HANDSHAKE_PACKET_ID: i32 = super::packet_id("serverbound.handshake", "intention");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeNextState {
    Status = 1,
    Login = 2,
}

impl HandshakeNextState {
    pub fn as_i32(self) -> i32 { self as i32 }
    pub fn from_i32(v: i32) -> Result<Self, ProtocolError> {
        match v { 1 => Ok(Self::Status), 2 => Ok(Self::Login), other => Err(ProtocolError::InvalidNextState(other)) }
    }
}

impl TryFrom<i32> for HandshakeNextState {
    type Error = ProtocolError;
    fn try_from(v: i32) -> Result<Self, Self::Error> { Self::from_i32(v) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handshake {
    pub protocol_version: i32,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: HandshakeNextState,
}

impl Handshake {
    /// 编码包体字段（不含 length + packet id）。
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_varint(self.protocol_version, out);
        encode_string(&self.server_address, out);
        write_u16_be(self.server_port, out);
        write_varint(self.next_state.as_i32(), out);
    }

    /// 解码包体（已去掉 packet id 的 payload），返回 `(包, 消费字节数)`。
    /// 尾部多余字节会被忽略，调用方可用消费字节数做严格校验。
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (protocol_version, n1) = read_varint(buf)?;
        let (server_address, n2) = decode_string(&buf[n1..])?;
        let off = n1 + n2;
        let (server_port, _) = read_u16_be(&buf[off..])?;
        let (next_raw, n3) = read_varint(&buf[off + 2..])?;
        let next_state = HandshakeNextState::from_i32(next_raw)?;
        Ok((
            Self {
                protocol_version,
                server_address,
                server_port,
                next_state,
            },
            off + 2 + n3,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_varint, write_varint};
    use crate::packets::codec::encode_packet;
    use crate::packets::codec::decode_packet_id;
    #[test]
    fn handshake_roundtrip() {
        for h in [
            Handshake { protocol_version: 777, server_address: "localhost".into(), server_port: 25565, next_state: HandshakeNextState::Status },
            Handshake { protocol_version: 777, server_address: "mc.例子.test".into(), server_port: 0, next_state: HandshakeNextState::Login },
        ] {
            let mut body = Vec::new();
            h.encode_body(&mut body);
            let (got, used) = Handshake::decode_body(&body).unwrap();
            assert_eq!(got, h); assert_eq!(used, body.len());
            let mut framed = Vec::new();
            encode_packet(HANDSHAKE_PACKET_ID, &body, &mut framed);
            let (len, n) = read_varint(&framed).unwrap();
            assert_eq!(len as usize, framed.len() - n);
            let (id, m) = decode_packet_id(&framed[n..]).unwrap();
            assert_eq!(id, HANDSHAKE_PACKET_ID);
            assert_eq!(Handshake::decode_body(&framed[n+m..]).unwrap().0, h);
        }
    }
    #[test]
    fn handshake_rejects_bad_next_state() {
        let mut body = Vec::new();
        write_varint(777, &mut body);
        encode_string("localhost", &mut body);
        write_u16_be(25565, &mut body);
        write_varint(3, &mut body);
        match Handshake::decode_body(&body) {
            Err(ProtocolError::InvalidNextState(3)) => {}
            other => panic!("got {other:?}"),
        }
    }
    #[test]
    fn handshake_rejects_truncated() {
        let h = Handshake { protocol_version: 777, server_address: "localhost".into(), server_port: 25565, next_state: HandshakeNextState::Status };
        let mut body = Vec::new(); h.encode_body(&mut body);
        for cut in [1, body.len()-1] { assert!(Handshake::decode_body(&body[..cut]).is_err()); }
    }
}
