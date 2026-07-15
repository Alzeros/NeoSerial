use serde::{Deserialize, Serialize};

/// 行尾模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum LineEnding {
    Cr,
    Lf,
    Crlf,
    None,
}

impl LineEnding {
    /// 将行尾字节追加到 out。
    pub fn append_bytes(&self, out: &mut Vec<u8>) {
        match self {
            LineEnding::Cr => out.push(b'\r'),
            LineEnding::Lf => out.push(b'\n'),
            LineEnding::Crlf => out.extend_from_slice(b"\r\n"),
            LineEnding::None => {}
        }
    }
}

/// ASCII 字符串转字节。
pub fn ascii_to_bytes(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// 十六进制字符串转字节。接受空格分隔的两位十六进制对（大小写不敏感），
/// 允许任意空白分隔。非法输入返回 Err(message)。
pub fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err("hex 长度必须为偶数".to_string());
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    let bytes = cleaned.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_digit(bytes[i]).ok_or_else(|| format!("非法 hex 字符: '{}'", bytes[i] as char))?;
        let lo = hex_digit(bytes[i + 1]).ok_or_else(|| format!("非法 hex 字符: '{}'", bytes[i + 1] as char))?;
        out.push(hi * 16 + lo);
        i += 2;
    }
    Ok(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 字节转 ASCII 显示字符串。可打印 ASCII（0x20-0x7E）原样，其余替换为 '.'。
pub fn bytes_to_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if (0x20..=0x7e).contains(&b) { b as char } else { '.' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_ending_append() {
        let mut v = vec![b'A'];
        LineEnding::Cr.append_bytes(&mut v);
        assert_eq!(v, b"A\r");

        let mut v = vec![b'A'];
        LineEnding::Crlf.append_bytes(&mut v);
        assert_eq!(v, b"A\r\n");

        let mut v = vec![b'A'];
        LineEnding::None.append_bytes(&mut v);
        assert_eq!(v, b"A");
    }

    #[test]
    fn test_hex_basic() {
        assert_eq!(hex_to_bytes("41 54").unwrap(), b"AT");
        assert_eq!(hex_to_bytes("5A A5 01 02").unwrap(), vec![0x5a, 0xa5, 0x01, 0x02]);
    }

    #[test]
    fn test_hex_lowercase_and_nospace() {
        assert_eq!(hex_to_bytes("4154").unwrap(), b"AT");
        assert_eq!(hex_to_bytes("5aa5").unwrap(), vec![0x5a, 0xa5]);
    }

    #[test]
    fn test_hex_empty() {
        assert_eq!(hex_to_bytes("").unwrap(), Vec::<u8>::new());
        assert_eq!(hex_to_bytes("   ").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn test_hex_odd_length_err() {
        assert!(hex_to_bytes("415").is_err());
    }

    #[test]
    fn test_hex_illegal_char_err() {
        assert!(hex_to_bytes("5Z").is_err());
        assert!(hex_to_bytes("GG").is_err());
    }

    #[test]
    fn test_bytes_to_ascii_printable() {
        assert_eq!(bytes_to_ascii(b"AT+CSQ"), "AT+CSQ");
    }

    #[test]
    fn test_bytes_to_ascii_nonprintable() {
        assert_eq!(bytes_to_ascii(b"A\x01B\x7f"), "A.B.");
        assert_eq!(bytes_to_ascii(b"\r\n"), "..");
    }

    #[test]
    fn test_ascii_to_bytes() {
        assert_eq!(ascii_to_bytes("AT"), b"AT");
    }

    #[test]
    fn test_line_ending_serde() {
        let le = LineEnding::Crlf;
        let json = serde_json::to_string(&le).unwrap();
        assert_eq!(json, "\"Crlf\"");
        let back: LineEnding = serde_json::from_str(&json).unwrap();
        assert_eq!(back, LineEnding::Crlf);
    }
}
