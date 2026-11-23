//! nbt: 自研小 NBT 编解码（Named Binary Tag），只兼容 26.3/777，思路参考 valence_nbt。
//! 背景：真机卡在 Registry，`has_data=false` 客户端不认，必须发真 NBT，故先落小编码器
//! （本任务只加编解码 + 接线；Registry 侧改 `data: Some(真NBT)` 是下一步，不在本文件）。
//! 线格式（minecraft.wiki NBT / wiki.vg）：
//! - Tag id：End 0 / Byte 1 / Short 2 / Int 3 / Long 4 / Float 5 / Double 6 /
//!   ByteArray 7 / String 8 / List 9 / Compound 10 / IntArray 11 / LongArray 12。
//! - String = u16be 字节长 + UTF-8 字节（标识符类 ASCII 与改良 UTF-8 二进制一致；
//!   超大 BMP 外字符按普通 UTF-8 处理，与香草 Java 端读 UTF 有差异风险，注册表场景无此字符）。
//! - List = 元素类型 u8 + i32be 个数 + 连续 payload（无 tag、无名，要求同构）。
//! - Compound = 循环（tag id u8 + 名 String + payload），以 TAG_End(0x00) 结尾。
//! - `write_named("", tag)` = 磁盘 NBT 形式（含根 id + 名）；空 compound 即 `0A 00 00 00`。
//! - `write_payload` = 网络 NBT 形式（无名 payload；Registry Data 的 Data 段直接拼这个，
//!   注意 777 registry_data 的 Data 段就是裸 NBT、本库不写 VarInt 长度前缀）。

use std::fmt;

/// Tag id 常量，按规范从 0 到 12 连续编号。
pub const TAG_END: u8 = 0;
pub const TAG_BYTE: u8 = 1;
pub const TAG_SHORT: u8 = 2;
pub const TAG_INT: u8 = 3;
pub const TAG_LONG: u8 = 4;
pub const TAG_FLOAT: u8 = 5;
pub const TAG_DOUBLE: u8 = 6;
pub const TAG_BYTE_ARRAY: u8 = 7;
pub const TAG_STRING: u8 = 8;
pub const TAG_LIST: u8 = 9;
pub const TAG_COMPOUND: u8 = 10;
pub const TAG_INT_ARRAY: u8 = 11;
pub const TAG_LONG_ARRAY: u8 = 12;

/// 递归解码深度上限（香草 512；防恶意包栈溢出）。
pub const MAX_DEPTH: usize = 512;

/// NBT 编解码错误（独立于 [`crate::ProtocolError`]，本任务不改 lib.rs/codec.rs；
/// 调用方可按需把 `UnexpectedEof` 等映射过去）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NbtError {
    UnexpectedEof,
    InvalidTag(u8),
    InvalidUtf8,
    InvalidLength(i32),
    ListTypeMismatch { expected: u8, found: u8 },
    TooDeep,
    /// 具名根不允许 TAG_End（End 无名、无 payload，无法做 `id + 名 + 体`）。
    EmptyRoot,
}

impl fmt::Display for NbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "nbt: unexpected eof"),
            Self::InvalidTag(id) => write!(f, "nbt: invalid tag id {id}"),
            Self::InvalidUtf8 => write!(f, "nbt: invalid utf-8"),
            Self::InvalidLength(n) => write!(f, "nbt: invalid length {n}"),
            Self::ListTypeMismatch { expected, found } => {
                write!(f, "nbt: list type mismatch expected {expected} found {found}")
            }
            Self::TooDeep => write!(f, "nbt: too deep"),
            Self::EmptyRoot => write!(f, "nbt: named root must not be TAG_End"),
        }
    }
}

impl std::error::Error for NbtError {}

/// 有序 Compound（Vec 保持插入序；同名 `insert` 覆盖，维持 NBT 名唯一不变式）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NbtCompound {
    entries: Vec<(String, NbtTag)>,
}

impl NbtCompound {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }
    /// 链式 `&mut` 插入（同名覆盖旧值，返回 self 方便 `.insert(..).insert(..)`）。
    pub fn insert(&mut self, name: impl Into<String>, tag: NbtTag) -> &mut Self {
        let name = name.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == name) {
            slot.1 = tag;
        } else {
            self.entries.push((name, tag));
        }
        self
    }
    /// 消费式 builder：`NbtCompound::new().with("a", ...).with("b", ...)`。
    pub fn with(mut self, name: impl Into<String>, tag: NbtTag) -> Self {
        self.insert(name, tag);
        self
    }
    pub fn get(&self, name: &str) -> Option<&NbtTag> {
        self.entries.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &NbtTag)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

/// 同构 List：元素类型 + 元素（无 tag、无名裸 payload 序列）。
#[derive(Debug, Clone, PartialEq)]
pub struct NbtList {
    elem: u8,
    items: Vec<NbtTag>,
}

impl NbtList {
    /// 空 list，元素类型按惯例记 TAG_End(0)，编码为 `00 00000000`。
    pub fn empty() -> Self {
        Self { elem: TAG_END, items: Vec::new() }
    }
    /// 显式元素类型构造；逐项校验同构，错配报 `ListTypeMismatch`，非法 id 报 `InvalidTag`。
    pub fn new(elem: u8, items: Vec<NbtTag>) -> Result<Self, NbtError> {
        if elem > TAG_LONG_ARRAY {
            return Err(NbtError::InvalidTag(elem));
        }
        if elem == TAG_END && !items.is_empty() {
            return Err(NbtError::InvalidTag(elem));
        }
        for it in &items {
            if it.id() != elem {
                return Err(NbtError::ListTypeMismatch { expected: elem, found: it.id() });
            }
        }
        Ok(Self { elem, items })
    }
    /// 从元素推断类型（空推断为 `empty()`）；混装报 `ListTypeMismatch`。
    pub fn from_tags(items: Vec<NbtTag>) -> Result<Self, NbtError> {
        match items.first() {
            None => Ok(Self::empty()),
            Some(first) => Self::new(first.id(), items),
        }
    }
    pub fn elem_type(&self) -> u8 {
        self.elem
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &NbtTag> {
        self.items.iter()
    }
}

/// NBT 值（End 只作 List 空类型/Compound 终止符，不设变体）。
#[derive(Debug, Clone, PartialEq)]
pub enum NbtTag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    /// 线上与有符号字节数组二进制一致，此处用 u8 方便拼 buffer。
    ByteArray(Vec<u8>),
    String(String),
    List(NbtList),
    Compound(NbtCompound),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl NbtTag {
    pub fn id(&self) -> u8 {
        match self {
            Self::Byte(_) => TAG_BYTE,
            Self::Short(_) => TAG_SHORT,
            Self::Int(_) => TAG_INT,
            Self::Long(_) => TAG_LONG,
            Self::Float(_) => TAG_FLOAT,
            Self::Double(_) => TAG_DOUBLE,
            Self::ByteArray(_) => TAG_BYTE_ARRAY,
            Self::String(_) => TAG_STRING,
            Self::List(_) => TAG_LIST,
            Self::Compound(_) => TAG_COMPOUND,
            Self::IntArray(_) => TAG_INT_ARRAY,
            Self::LongArray(_) => TAG_LONG_ARRAY,
        }
    }

    /// 快捷构造 compound：`NbtTag::compound(vec![("name", NbtTag::String("x".into()))])`。
    pub fn compound(entries: Vec<(&str, NbtTag)>) -> Self {
        let mut c = NbtCompound::new();
        for (k, v) in entries {
            c.insert(k, v);
        }
        Self::Compound(c)
    }

    /// 写无名 payload（网络 NBT 形式；根 compound 只写条目 + `0x00`，不写 `0A`/根名）。
    /// 字符串/数组超长时截断到类型上限（`debug_assert` 会喊；注册表静态串不会触发）。
    pub fn write_payload(&self, out: &mut Vec<u8>) {
        match self {
            Self::Byte(v) => out.push(*v as u8),
            Self::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
            Self::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
            Self::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
            Self::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
            Self::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
            Self::ByteArray(v) => {
                debug_assert!(v.len() <= i32::MAX as usize);
                out.extend_from_slice(&(v.len() as i32).to_be_bytes());
                out.extend_from_slice(v);
            }
            Self::String(s) => write_nbt_string(s, out),
            Self::List(l) => {
                out.push(l.elem);
                debug_assert!(l.items.len() <= i32::MAX as usize);
                out.extend_from_slice(&(l.items.len() as i32).to_be_bytes());
                for it in &l.items {
                    it.write_payload(out);
                }
            }
            Self::Compound(c) => {
                for (k, v) in &c.entries {
                    out.push(v.id());
                    write_nbt_string(k, out);
                    v.write_payload(out);
                }
                out.push(TAG_END);
            }
            Self::IntArray(v) => {
                debug_assert!(v.len() <= i32::MAX as usize);
                out.extend_from_slice(&(v.len() as i32).to_be_bytes());
                for x in v {
                    out.extend_from_slice(&x.to_be_bytes());
                }
            }
            Self::LongArray(v) => {
                debug_assert!(v.len() <= i32::MAX as usize);
                out.extend_from_slice(&(v.len() as i32).to_be_bytes());
                for x in v {
                    out.extend_from_slice(&x.to_be_bytes());
                }
            }
        }
    }

    /// 写具名 tag（磁盘 NBT 形式：`id u8 + 名 String + payload`）。
    pub fn write_named(&self, name: &str, out: &mut Vec<u8>) {
        out.push(self.id());
        write_nbt_string(name, out);
        self.write_payload(out);
    }
}

/// 写无名 payload（等价 `tag.write_payload(out)`，Registry Data 的 Data 段直接拼这个）。
pub fn write_nbt_payload(tag: &NbtTag, out: &mut Vec<u8>) {
    tag.write_payload(out);
}

/// 写具名 tag（等价 `tag.write_named(name, out)`；`write_nbt_named("", 空compound)` = `0A 00 00 00`）。
pub fn write_nbt_named(name: &str, tag: &NbtTag, out: &mut Vec<u8>) {
    tag.write_named(name, out);
}

/// 读具名 tag，返回 `((名, 值), 消费字节数)`；首字节 TAG_End 报 `EmptyRoot`。
pub fn read_nbt_named(buf: &[u8]) -> Result<((String, NbtTag), usize), NbtError> {
    let first = *buf.first().ok_or(NbtError::UnexpectedEof)?;
    if first == TAG_END {
        return Err(NbtError::EmptyRoot);
    }
    if first > TAG_LONG_ARRAY {
        return Err(NbtError::InvalidTag(first));
    }
    let (name, n1) = read_nbt_string(&buf[1..])?;
    let (tag, n2) = read_payload(first, &buf[1 + n1..], 0)?;
    Ok(((name, tag), 1 + n1 + n2))
}

/// 读无名 payload（调用方已从外部得知 tag id；Registry Data 侧根固定传 `TAG_COMPOUND`）。
pub fn read_nbt_payload(tag_id: u8, buf: &[u8]) -> Result<(NbtTag, usize), NbtError> {
    read_payload(tag_id, buf, 0)
}

/// 读根 compound 无名 payload 的便捷口（Registry Data Data 段用）。
pub fn read_nbt_compound_payload(buf: &[u8]) -> Result<(NbtCompound, usize), NbtError> {
    match read_payload(TAG_COMPOUND, buf, 0)? {
        (NbtTag::Compound(c), n) => Ok((c, n)),
        _ => unreachable!("compound payload always decodes to Compound"),
    }
}

fn write_nbt_string(s: &str, out: &mut Vec<u8>) {
    debug_assert!(s.len() <= u16::MAX as usize, "nbt string too long");
    let n = s.len().min(u16::MAX as usize) as u16;
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&s.as_bytes()[..n as usize]);
}

fn read_nbt_string(buf: &[u8]) -> Result<(String, usize), NbtError> {
    if buf.len() < 2 {
        return Err(NbtError::UnexpectedEof);
    }
    let n = u16::from_be_bytes([buf[0], buf[1]]) as usize;
    if buf.len() < 2 + n {
        return Err(NbtError::UnexpectedEof);
    }
    let s = std::str::from_utf8(&buf[2..2 + n]).map_err(|_| NbtError::InvalidUtf8)?;
    Ok((s.to_owned(), 2 + n))
}

fn read_i32_be(buf: &[u8]) -> Result<(i32, usize), NbtError> {
    if buf.len() < 4 {
        return Err(NbtError::UnexpectedEof);
    }
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[..4]);
    Ok((i32::from_be_bytes(b), 4))
}

fn read_payload(tag_id: u8, buf: &[u8], depth: usize) -> Result<(NbtTag, usize), NbtError> {
    if depth > MAX_DEPTH {
        return Err(NbtError::TooDeep);
    }
    match tag_id {
        TAG_END => Err(NbtError::InvalidTag(TAG_END)),
        TAG_BYTE => match buf.first() {
            Some(&b) => Ok((NbtTag::Byte(b as i8), 1)),
            None => Err(NbtError::UnexpectedEof),
        },
        TAG_SHORT => {
            if buf.len() < 2 {
                return Err(NbtError::UnexpectedEof);
            }
            Ok((NbtTag::Short(i16::from_be_bytes([buf[0], buf[1]])), 2))
        }
        TAG_INT => {
            let (v, n) = read_i32_be(buf)?;
            Ok((NbtTag::Int(v), n))
        }
        TAG_LONG => {
            if buf.len() < 8 {
                return Err(NbtError::UnexpectedEof);
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[..8]);
            Ok((NbtTag::Long(i64::from_be_bytes(b)), 8))
        }
        TAG_FLOAT => {
            if buf.len() < 4 {
                return Err(NbtError::UnexpectedEof);
            }
            let mut b = [0u8; 4];
            b.copy_from_slice(&buf[..4]);
            Ok((NbtTag::Float(f32::from_be_bytes(b)), 4))
        }
        TAG_DOUBLE => {
            if buf.len() < 8 {
                return Err(NbtError::UnexpectedEof);
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[..8]);
            Ok((NbtTag::Double(f64::from_be_bytes(b)), 8))
        }
        TAG_BYTE_ARRAY => {
            let (len, n) = read_i32_be(buf)?;
            if len < 0 {
                return Err(NbtError::InvalidLength(len));
            }
            let (len, n) = (len as usize, n);
            if buf.len() < n + len {
                return Err(NbtError::UnexpectedEof);
            }
            Ok((NbtTag::ByteArray(buf[n..n + len].to_vec()), n + len))
        }
        TAG_STRING => {
            let (s, n) = read_nbt_string(buf)?;
            Ok((NbtTag::String(s), n))
        }
        TAG_LIST => {
            let elem = *buf.first().ok_or(NbtError::UnexpectedEof)?;
            if elem > TAG_LONG_ARRAY {
                return Err(NbtError::InvalidTag(elem));
            }
            let (len, mut off) = read_i32_be(&buf[1..]).map(|(v, n)| (v, n + 1))?;
            if len < 0 {
                return Err(NbtError::InvalidLength(len));
            }
            if elem == TAG_END {
                if len != 0 {
                    return Err(NbtError::InvalidTag(TAG_END));
                }
                return Ok((NbtTag::List(NbtList::empty()), off));
            }
            let mut items = Vec::with_capacity(len as usize);
            for _ in 0..len {
                let (it, k) = read_payload(elem, &buf[off..], depth + 1)?;
                off += k;
                items.push(it);
            }
            Ok((NbtTag::List(NbtList { elem, items }), off))
        }
        TAG_COMPOUND => {
            let mut c = NbtCompound::new();
            let mut off = 0;
            loop {
                let id = *buf.get(off).ok_or(NbtError::UnexpectedEof)?;
                off += 1;
                if id == TAG_END {
                    break;
                }
                if id > TAG_LONG_ARRAY {
                    return Err(NbtError::InvalidTag(id));
                }
                let (name, n1) = read_nbt_string(&buf[off..])?;
                off += n1;
                let (tag, n2) = read_payload(id, &buf[off..], depth + 1)?;
                off += n2;
                c.insert(name, tag);
            }
            Ok((NbtTag::Compound(c), off))
        }
        TAG_INT_ARRAY => {
            let (len, mut off) = read_i32_be(buf)?;
            if len < 0 {
                return Err(NbtError::InvalidLength(len));
            }
            let mut v = Vec::with_capacity(len as usize);
            for _ in 0..len {
                let (x, _) = read_i32_be(&buf[off..])?;
                off += 4;
                v.push(x);
            }
            Ok((NbtTag::IntArray(v), off))
        }
        TAG_LONG_ARRAY => {
            let (len, mut off) = read_i32_be(buf)?;
            if len < 0 {
                return Err(NbtError::InvalidLength(len));
            }
            let mut v = Vec::with_capacity(len as usize);
            for _ in 0..len {
                if buf.len() < off + 8 {
                    return Err(NbtError::UnexpectedEof);
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&buf[off..off + 8]);
                off += 8;
                v.push(i64::from_be_bytes(b));
            }
            Ok((NbtTag::LongArray(v), off))
        }
        other => Err(NbtError::InvalidTag(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named_roundtrip(name: &str, tag: &NbtTag) -> Vec<u8> {
        let mut b = Vec::new();
        write_nbt_named(name, tag, &mut b);
        let ((got_name, got_tag), used) = read_nbt_named(&b).unwrap();
        assert_eq!(got_name, name);
        assert_eq!(&got_tag, tag);
        assert_eq!(used, b.len(), "具名编解码必须字节对齐消费");
        b
    }

    fn payload_roundtrip(tag: &NbtTag) -> Vec<u8> {
        let mut b = Vec::new();
        write_nbt_payload(tag, &mut b);
        let (got, used) = read_nbt_payload(tag.id(), &b).unwrap();
        assert_eq!(&got, tag);
        assert_eq!(used, b.len(), "无名编解码必须字节对齐消费");
        b
    }

    #[test]
    fn empty_compound_known_bytes() {
        // 已知字节：空 compound 具名根（名 ""）= 0A 00 00 00（id + u16空名 + TAG_End）。
        let tag = NbtTag::Compound(NbtCompound::new());
        let mut b = Vec::new();
        write_nbt_named("", &tag, &mut b);
        assert_eq!(b, vec![0x0A, 0x00, 0x00, 0x00]);
        let ((name, back), used) = read_nbt_named(&b).unwrap();
        assert_eq!(name, "");
        assert_eq!(back, tag);
        assert_eq!(used, b.len());
        // 网络 NBT（无名 payload）空 compound 只有 1 个 TAG_End。
        let mut p = Vec::new();
        write_nbt_payload(&tag, &mut p);
        assert_eq!(p, vec![0x00]);
        let (c, used) = read_nbt_compound_payload(&p).unwrap();
        assert!(c.is_empty());
        assert_eq!(used, 1);
    }

    #[test]
    fn string_entry_known_bytes_roundtrip() {
        // {name:"x"} 根名 "" 的逐字节展开：
        // 0A | 00 00(根名) | 08 00 04 'name' 00 01 'x' | 00(End)。
        let tag = NbtTag::compound(vec![("name", NbtTag::String("x".into()))]);
        let b = named_roundtrip("", &tag);
        assert_eq!(
            b,
            vec![
                0x0A, 0x00, 0x00, 0x08, 0x00, 0x04, b'n', b'a', b'm', b'e', 0x00, 0x01, b'x',
                0x00,
            ]
        );
        // 多字节 UTF-8 按字节计数："例子" = 6 字节 → 00 06。
        let uni = NbtTag::compound(vec![("k", NbtTag::String("例子".into()))]);
        let ub = named_roundtrip("", &uni);
        assert!(ub.windows(2).any(|w| w == [0x00, 0x06]), "string 长按 UTF-8 字节数");
        // 无名 payload 形式也往返一致（Registry Data Data 段就拼这个）。
        payload_roundtrip(&tag);
    }

    #[test]
    fn nested_list_compound_roundtrip() {
        // 覆盖全部 12 种 tag + 嵌套 list<compound>/compound<list>/空 list。
        let inner = NbtTag::compound(vec![
            ("b", NbtTag::Byte(-5)),
            ("s", NbtTag::Short(-300)),
            ("i", NbtTag::Int(123456)),
            ("l", NbtTag::Long(-9_000_000_000)),
            ("f", NbtTag::Float(1.5)),
            ("d", NbtTag::Double(-2.25)),
            ("ba", NbtTag::ByteArray(vec![0, 255, 1])),
            ("str", NbtTag::String("minecraft:overworld".into())),
            ("ia", NbtTag::IntArray(vec![1, -2, i32::MAX])),
            ("la", NbtTag::LongArray(vec![0, i64::MIN, i64::MAX])),
        ]);
        let list_of_compound = NbtTag::List(
            NbtList::from_tags(vec![
                inner.clone(),
                NbtTag::compound(vec![("a", NbtTag::Byte(1))]),
            ])
            .unwrap(),
        );
        let root = NbtTag::compound(vec![
            ("ints", NbtTag::List(NbtList::new(TAG_INT, vec![NbtTag::Int(1), NbtTag::Int(2)]).unwrap())),
            ("objs", list_of_compound),
            ("empty", NbtTag::List(NbtList::empty())),
            ("nested", NbtTag::compound(vec![("deep", NbtTag::List(
                NbtList::from_tags(vec![NbtTag::String("x".into())]).unwrap(),
            ))])),
        ]);
        let b = named_roundtrip("root", &root);
        // 具名根首字节必为 Compound id，且名 "root" 占 00 04。
        assert_eq!(&b[..7], &[TAG_COMPOUND, 0x00, 0x04, b'r', b'o', b'o', b't']);
        assert_eq!(*b.last().unwrap(), TAG_END);
        payload_roundtrip(&root);
        // List payload 结构抽查：[Int 1,2] = 03(元素类型) 00000002(个数) 00000001 00000002。
        let li = NbtTag::List(NbtList::new(TAG_INT, vec![NbtTag::Int(1), NbtTag::Int(2)]).unwrap());
        let mut lb = Vec::new();
        write_nbt_payload(&li, &mut lb);
        assert_eq!(lb, vec![0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02]);
        let (back, used) = read_nbt_payload(TAG_LIST, &lb).unwrap();
        assert_eq!(back, li);
        assert_eq!(used, lb.len());
    }

    #[test]
    fn decode_rejects_malformed() {
        // 截断：空 compound 少 TAG_End。
        assert_eq!(read_nbt_named(&[0x0A, 0x00, 0x00]), Err(NbtError::UnexpectedEof));
        // 非法 tag id（13 未定义）。
        assert_eq!(read_nbt_named(&[0x0D, 0x00, 0x00]), Err(NbtError::InvalidTag(13)));
        // 具名根 TAG_End 无名无体，直接拒。
        assert_eq!(read_nbt_named(&[0x00]), Err(NbtError::EmptyRoot));
        // List 混装构造期即拒。
        assert!(matches!(
            NbtList::new(TAG_INT, vec![NbtTag::Byte(1)]),
            Err(NbtError::ListTypeMismatch { expected: 3, found: 1 })
        ));
        // 非法 UTF-8 字符串。
        assert_eq!(
            read_nbt_named(&[0x08, 0x00, 0x00, 0x00, 0x01, 0xFF, 0x00]),
            Err(NbtError::InvalidUtf8)
        );
        // 负长度 ByteArray。
        assert_eq!(
            read_nbt_payload(TAG_BYTE_ARRAY, &[0xFF, 0xFF, 0xFF, 0xFF]),
            Err(NbtError::InvalidLength(-1))
        );
    }
}
