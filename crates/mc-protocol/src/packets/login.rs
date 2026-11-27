//! login: Start/Success(含26.3属性表)/Ack + 压缩开关 + 加密占位，只兼容26.3/777
use crate::packets::codec::{decode_string, encode_packet, encode_string};
use crate::{read_varint, write_varint, ProtocolError};
use uuid::Uuid;

// 包ID: Start 0x00(SB hello) / Success 0x02(CB login_finished) / Ack 0x03(SB login_acknowledged)
// / SetCompression 0x03(CB login_compression) / EncReq 0x01(CB hello) / EncResp 0x01(SB key)
// 全部编译期从 `mc-registry` 由 data/777/packets.json 生成的 PACKETS 表派生。
pub const LOGIN_START_PACKET_ID: i32 = super::packet_id("serverbound.login", "hello");
pub const LOGIN_SUCCESS_PACKET_ID: i32 = super::packet_id("clientbound.login", "login_finished");
pub const LOGIN_ACKNOWLEDGED_PACKET_ID: i32 = super::packet_id("serverbound.login", "login_acknowledged");
pub const SET_COMPRESSION_PACKET_ID: i32 = super::packet_id("clientbound.login", "login_compression");
pub const ENCRYPTION_REQUEST_PACKET_ID: i32 = super::packet_id("clientbound.login", "hello");
pub const ENCRYPTION_RESPONSE_PACKET_ID: i32 = super::packet_id("serverbound.login", "key");

/// 压缩阈值语义：-1=禁用(不发SetCompression)；>=0=启用(Success前先发，后续包走zlib帧)。
pub const COMPRESSION_DISABLED_THRESHOLD: i32 = -1;
/// 阈值>=0即启用压缩（framing 压缩帧判定与 login_route 发送判定共用）。
pub fn compression_enabled(threshold: i32) -> bool { threshold >= 0 }

/// 离线模式 UUID（历史遗留）：服务端随机分配 v4，忽略客户端所带。
/// **只保留给旧单测/旧工具**，身份链路已改用 [`stable_offline_uuid`]。
pub fn offline_uuid(_name: &str) -> Uuid { Uuid::new_v4() }

/// P51：原版口径的离线 UUID（稳定身份）。
///
/// 原版 `UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF_8))`：
/// 对 **裸** `OfflinePlayer:<name>` 字节取 MD5（不是 `Uuid::new_v3` 的
/// namespace+name），再把 digest[6] 设为版本 3、digest[8] 设为 RFC4122 variant。
/// 因此同一用户名每次登录得到同一个 UUID，不同用户名不同；不信任客户端带的 UUID。
pub fn stable_offline_uuid(name: &str) -> Uuid {
    let mut digest = md5(format!("OfflinePlayer:{name}").as_bytes());
    digest[6] = (digest[6] & 0x0f) | 0x30; // version 3
    digest[8] = (digest[8] & 0x3f) | 0x80; // variant 10xx
    Uuid::from_bytes(digest)
}

/// 存档键是否已是 UUID（迁移据此区分 username 键与 uuid 键）。
pub fn is_uuid_tag(tag: &str) -> bool {
    Uuid::parse_str(tag).is_ok()
}

/// 自包含 MD5（原版离线 UUID 算法要求裸摘要；`uuid` crate 的 v3 只能算 namespace+name）。
/// 避免引入新依赖（仓库忽略 Cargo.lock，靠自包含实现消除版本漂移）。
fn md5(input: &[u8]) -> [u8; 16] {
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let mut msg = input.to_vec();
    let bit_len = (msg.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    let mut h = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        let [h0, h1, h2, h3] = h;
        let (mut a, mut b, mut c, mut d) = (h0, h1, h2, h3);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let old_a = a;
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                old_a
                    .wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(S[i]),
            );
        }
        h = [
            h0.wrapping_add(a),
            h1.wrapping_add(b),
            h2.wrapping_add(c),
            h3.wrapping_add(d),
        ];
    }
    let mut out = [0u8; 16];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// UUID 线包格式：16字节 big-endian。
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
    pub fn encode_body(&self, out: &mut Vec<u8>) { encode_string(&self.name, out); write_uuid(self.uuid, out); }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (name, n1) = decode_string(buf)?; let (uuid, n2) = read_uuid(&buf[n1..])?;
        Ok((Self { name, uuid }, n1 + n2))
    }
}

/// Login Start 完整线包（服务端主要做 decode，encode 供单测/模拟客户端用）。
pub fn encode_login_start_packet(v: &LoginStart, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body); encode_packet(LOGIN_START_PACKET_ID, &body, out);
}
pub fn decode_login_start(payload: &[u8]) -> Result<(LoginStart, usize), ProtocolError> { LoginStart::decode_body(payload) }

/// Success 属性项（26.3抓包：name + value + 签名标志 + 可选 signature）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginProperty { pub name: String, pub value: String, pub signature: Option<String> }
impl LoginProperty {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.name, out); encode_string(&self.value, out);
        match &self.signature { Some(s) => { out.push(1); encode_string(s, out); } None => out.push(0), }
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (name, n1) = decode_string(buf)?; let (value, n2) = decode_string(&buf[n1..])?;
        let mut off = n1 + n2; if buf.len() <= off { return Err(ProtocolError::UnexpectedEof); }
        let sig = if buf[off] != 0 { let (s, n) = decode_string(&buf[off + 1..])?; off += 1 + n; Some(s) } else { off += 1; None };
        Ok((Self { name, value, signature: sig }, off))
    }
}

/// Login Success（CB 0x02 login_finished）：GameProfile(UUID + name + 属性表) + Session ID UUID。
/// 26.2(776)+ 新增尾部 Session ID UUID 字段（见 Protocol History），777 必发，缺则客户端
/// `DecoderException: Failed to decode login_finished`。离线模式发 `Uuid::nil()` 占位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSuccess { pub uuid: Uuid, pub username: String, pub properties: Vec<LoginProperty>, pub session_id: Uuid }
impl LoginSuccess {
    /// 离线快捷构造：属性表为空，session 占位 nil。
    pub fn offline(uuid: Uuid, username: String) -> Self { Self { uuid, username, properties: Vec::new(), session_id: Uuid::nil() } }
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        write_uuid(self.uuid, out); encode_string(&self.username, out);
        write_varint(self.properties.len() as i32, out);
        for p in &self.properties { p.encode_body(out); }
        write_uuid(self.session_id, out);
    }
    /// 解码兼容旧包：尾部缺属性数/session 时视为零属性 + nil session（离线占位包直接通过）。
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (uuid, n1) = read_uuid(buf)?; let (username, n2) = decode_string(&buf[n1..])?;
        let mut off = n1 + n2; let mut properties = Vec::new();
        if off < buf.len() {
            let (n, m) = read_varint(&buf[off..])?; off += m;
            for _ in 0..n { let (p, k) = LoginProperty::decode_body(&buf[off..])?; off += k; properties.push(p); }
        }
        let mut session_id = Uuid::nil();
        if buf.len() >= off + 16 {
            let (s, _) = read_uuid(&buf[off..])?; session_id = s; off += 16;
        }
        Ok((Self { uuid, username, properties, session_id }, off))
    }
}

/// Login Success 完整线包。
pub fn encode_login_success_packet(v: &LoginSuccess, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body); encode_packet(LOGIN_SUCCESS_PACKET_ID, &body, out);
}
pub fn decode_login_success(payload: &[u8]) -> Result<(LoginSuccess, usize), ProtocolError> { LoginSuccess::decode_body(payload) }

/// Login Acknowledged（SB 0x03）空包（供单测/模拟客户端用）；解码忽略尾部字节。
pub fn encode_login_acknowledged_packet(out: &mut Vec<u8>) { encode_packet(LOGIN_ACKNOWLEDGED_PACKET_ID, &[], out); }
pub fn decode_login_acknowledged(_payload: &[u8]) -> Result<(), ProtocolError> { Ok(()) }

/// SetCompression（CB 0x03，VarInt threshold）完整线包；阈值>=0时在 Success 之前发送。
pub fn encode_set_compression_packet(threshold: i32, out: &mut Vec<u8>) {
    let mut body = Vec::new(); write_varint(threshold, &mut body); encode_packet(SET_COMPRESSION_PACKET_ID, &body, out);
}
pub fn decode_set_compression(payload: &[u8]) -> Result<i32, ProtocolError> { Ok(read_varint(payload)?.0) }

/// VarInt长度前缀字节串读写（加密包的 key/token/secret 共用）。
fn write_bytes(b: &[u8], out: &mut Vec<u8>) { write_varint(b.len() as i32, out); out.extend_from_slice(b); }
fn read_bytes(buf: &[u8]) -> Result<(&[u8], usize), ProtocolError> {
    let (len, n) = read_varint(buf)?; if len < 0 { return Err(ProtocolError::UnexpectedEof); }
    let e = n + len as usize; if buf.len() < e { return Err(ProtocolError::UnexpectedEof); }
    Ok((&buf[n..e], e))
}

// P3后半：Encryption Request（CB 0x01）= server_id + pubkey + verify_token。离线不发送，
// 占位只留 encode/decode；正版验证+Mojang会话校验+共享密钥协商后半再接，不在此实现加密。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionRequest { pub server_id: String, pub public_key: Vec<u8>, pub verify_token: Vec<u8> }
impl EncryptionRequest {
    pub fn encode_body(&self, out: &mut Vec<u8>) {
        encode_string(&self.server_id, out); write_bytes(&self.public_key, out); write_bytes(&self.verify_token, out);
    }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (server_id, n1) = decode_string(buf)?; let (pk, n2) = read_bytes(&buf[n1..])?;
        let (tok, n3) = read_bytes(&buf[n1 + n2..])?;
        Ok((Self { server_id, public_key: pk.to_vec(), verify_token: tok.to_vec() }, n1 + n2 + n3))
    }
}
pub fn encode_encryption_request_packet(v: &EncryptionRequest, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body); encode_packet(ENCRYPTION_REQUEST_PACKET_ID, &body, out);
}
pub fn decode_encryption_request(p: &[u8]) -> Result<(EncryptionRequest, usize), ProtocolError> { EncryptionRequest::decode_body(p) }

// P3后半：Encryption Response（SB 0x01）= shared_secret + verify_token。离线不处理，
// 占位只留 encode/decode；解密/密钥协商后半再接。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionResponse { pub shared_secret: Vec<u8>, pub verify_token: Vec<u8> }
impl EncryptionResponse {
    pub fn encode_body(&self, out: &mut Vec<u8>) { write_bytes(&self.shared_secret, out); write_bytes(&self.verify_token, out); }
    pub fn decode_body(buf: &[u8]) -> Result<(Self, usize), ProtocolError> {
        let (sec, n1) = read_bytes(buf)?; let (tok, n2) = read_bytes(&buf[n1..])?;
        Ok((Self { shared_secret: sec.to_vec(), verify_token: tok.to_vec() }, n1 + n2))
    }
}
pub fn encode_encryption_response_packet(v: &EncryptionResponse, out: &mut Vec<u8>) {
    let mut body = Vec::new(); v.encode_body(&mut body); encode_packet(ENCRYPTION_RESPONSE_PACKET_ID, &body, out);
}
pub fn decode_encryption_response(p: &[u8]) -> Result<(EncryptionResponse, usize), ProtocolError> { EncryptionResponse::decode_body(p) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packets::codec::decode_packet_id;
    fn packet_id_of(pkt: &[u8]) -> (i32, usize) { let (_, n) = read_varint(pkt).unwrap(); let (id, m) = decode_packet_id(&pkt[n..]).unwrap(); (id, n + m) }
    #[test]
    fn login_start_roundtrip() {
        let v = LoginStart { name: "Steve".into(), uuid: Uuid::new_v4() };
        let mut pkt = Vec::new(); encode_login_start_packet(&v, &mut pkt);
        let (id, n) = packet_id_of(&pkt); assert_eq!(id, LOGIN_START_PACKET_ID);
        assert_eq!(decode_login_start(&pkt[n..]).unwrap().0, v);
        let mut body = Vec::new(); v.encode_body(&mut body);
        assert!(decode_login_start(&body[..2]).is_err());
    }
    #[test]
    fn login_success_props_roundtrip_and_legacy_compat() {
        let v = LoginSuccess { uuid: Uuid::new_v4(), username: "Alex".into(), session_id: Uuid::nil(), properties: vec![
            LoginProperty { name: "textures".into(), value: "v".into(), signature: Some("s".into()) },
            LoginProperty { name: "x".into(), value: "y".into(), signature: None }] };
        let mut pkt = Vec::new(); encode_login_success_packet(&v, &mut pkt);
        let (id, n) = packet_id_of(&pkt); assert_eq!(id, LOGIN_SUCCESS_PACKET_ID);
        assert_eq!(decode_login_success(&pkt[n..]).unwrap().0, v);
        let mut legacy = Vec::new(); write_uuid(v.uuid, &mut legacy); encode_string(&v.username, &mut legacy);
        let (got, used) = decode_login_success(&legacy).unwrap();
        assert!(got.properties.is_empty() && used == legacy.len() && got.username == "Alex");
    }
    #[test]
    fn login_ack_and_compression_gate() {
        let mut b = Vec::new(); encode_login_acknowledged_packet(&mut b);
        let (id, n) = packet_id_of(&b); assert_eq!(id, LOGIN_ACKNOWLEDGED_PACKET_ID);
        decode_login_acknowledged(&b[n..]).unwrap();
        assert!(!compression_enabled(-1) && compression_enabled(0) && compression_enabled(256));
        let mut c = Vec::new(); encode_set_compression_packet(256, &mut c);
        let (cid, m) = packet_id_of(&c); assert_eq!(cid, SET_COMPRESSION_PACKET_ID);
        assert_eq!(decode_set_compression(&c[m..]).unwrap(), 256);
        assert_eq!(COMPRESSION_DISABLED_THRESHOLD, -1);
    }
    #[test]
    fn encryption_request_response_placeholder_roundtrip() {
        let req = EncryptionRequest { server_id: "".into(), public_key: vec![1, 2, 3], verify_token: vec![4, 5] };
        let mut p = Vec::new(); encode_encryption_request_packet(&req, &mut p);
        let (id, n) = packet_id_of(&p); assert_eq!(id, ENCRYPTION_REQUEST_PACKET_ID);
        assert_eq!(decode_encryption_request(&p[n..]).unwrap().0, req);
        let rsp = EncryptionResponse { shared_secret: vec![9, 9], verify_token: vec![7] };
        let mut q = Vec::new(); encode_encryption_response_packet(&rsp, &mut q);
        let (rid, m) = packet_id_of(&q); assert_eq!(rid, ENCRYPTION_RESPONSE_PACKET_ID);
        assert_eq!(decode_encryption_response(&q[m..]).unwrap().0, rsp);
        assert_ne!(offline_uuid("a"), offline_uuid("a"));
    }

    /// P51：原版 `OfflinePlayer:<name>` 裸 MD5 + v3/variant 位向量（独立工具计算）。
    #[test]
    fn stable_offline_uuid_is_vanilla_deterministic_and_name_scoped() {
        assert_eq!(stable_offline_uuid("Notch").to_string(), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(stable_offline_uuid("Steve").to_string(), "5627dd98-e6be-3c21-b8a8-e92344183641");
        assert_eq!(stable_offline_uuid("Alex").to_string(), "36532b5e-c442-3dbb-a24c-c7e55d0f979a");
        assert_eq!(stable_offline_uuid("Notch"), stable_offline_uuid("Notch"));
        assert_ne!(stable_offline_uuid("Notch"), stable_offline_uuid("notch"));
        assert_eq!(stable_offline_uuid("Notch").get_version_num(), 3);
        assert!(is_uuid_tag("b50ad385-829d-3141-a216-7e7d7539ba7f"));
        assert!(!is_uuid_tag("Steve"));
    }
}
