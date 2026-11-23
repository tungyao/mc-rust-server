use crate::ProtocolError;

pub fn read_varint(buf: &[u8]) -> Result<(i32, usize), ProtocolError> {
    let mut num = 0i32;
    let mut shift = 0u32;
    for (i, &b) in buf.iter().enumerate().take(5) {
        num |= ((b & 0x7F) as i32) << shift;
        if b & 0x80 == 0 {
            return Ok((num, i + 1));
        }
        shift += 7;
    }
    Err(ProtocolError::VarIntTooBig)
}

pub fn write_varint(value: i32, out: &mut Vec<u8>) {
    // 按 wiki.vg 用 u32 逻辑右移，否则负数算术右移永不为 0 会死循环。
    let mut u = value as u32;
    loop {
        let mut temp = (u & 0x7F) as u8;
        u >>= 7;
        if u != 0 {
            temp |= 0x80;
        }
        out.push(temp);
        if u == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        for v in [0, 1, 127, 128, 255, 2147483647] {
            let mut b = Vec::new();
            write_varint(v, &mut b);
            let (r, _) = read_varint(&b).unwrap();
            assert_eq!(v, r);
        }
    }
}
