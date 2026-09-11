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

/// 中文标点转半角。中文输入法下顺手打出的 ？，：等,模组一概不认——发送是纯透传,
/// ？走的是 UTF-8 的 EF BC 9F 三个字节,而模组只认半角 ?(0x3F)。这些符号出现在
/// AT 指令的骨架里基本都是打错了,所以转掉。
///
/// **引号内原样不动**:AT+CMGS="你好,世界" 这类引号里是用户要发出去的内容,
/// 里面的中文标点是正文不是语法,改了就是改用户的数据。
/// 引号本身(半角 " 与全角 ＂“”)都转成 " 并参与配对,所以 AT+X=1,“IP” 这种
/// 连引号都打成全角的也能救回来。引号没闭合时,其后一律按"引号内"处理(宁可不改)。
pub fn punct_to_halfwidth(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_quote = false;
    for ch in s.chars() {
        if matches!(ch, '"' | '\u{FF02}' | '\u{201C}' | '\u{201D}') {
            out.push('"');
            in_quote = !in_quote;
        } else if in_quote {
            out.push(ch);
        } else {
            out.push(halfwidth_char(ch));
        }
    }
    out
}

/// 单字符的全角→半角映射。未收录的字符原样返回。
fn halfwidth_char(ch: char) -> char {
    match ch {
        // 全角 ASCII 区(U+FF01-U+FF5E)与半角一一对应,差 0xFEE0。
        // 覆盖 ？！，；：（）＝＋－＊／＜＞＃＆％＠ 以及全角数字/字母(ＡＴ→AT)
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
        '\u{3000}' => ' ', // 全角空格
        '。' => '.',
        '、' => ',',
        '‘' | '’' => '\'',
        _ => ch,
    }
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

/// 字节转 ASCII 显示字符串。仅保留可打印 ASCII（0x20-0x7E），其余直接丢弃。
/// 行尾 CR/LF 等控制字符不会出现在日志区，视觉更干净。
/// 串口数据可能截断多字节 UTF-8 序列（如中文），用 from_utf8_lossy 保证不 panic。
pub fn bytes_to_ascii(bytes: &[u8]) -> String {
    let filtered: Vec<u8> = bytes
        .iter()
        .filter(|&&b| (0x20..=0x7e).contains(&b))
        .copied()
        .collect();
    String::from_utf8_lossy(&filtered).into_owned()
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
        // 控制字符/高位字节直接丢弃，不再用 '.' 占位
        assert_eq!(bytes_to_ascii(b"A\x01B\x7f"), "AB");
        assert_eq!(bytes_to_ascii(b"\r\n"), "");
        assert_eq!(bytes_to_ascii(b"AT\r\n"), "AT");
    }

    #[test]
    fn test_ascii_to_bytes() {
        assert_eq!(ascii_to_bytes("AT"), b"AT");
    }

    /// 起因:中文输入法打出的 ？ 透传过去是 UTF-8 三字节,模组只认 0x3F。
    #[test]
    fn test_punct_to_halfwidth_fixes_at_syntax() {
        assert_eq!(punct_to_halfwidth("AT+CSQ？"), "AT+CSQ?");
        assert_eq!(punct_to_halfwidth("AT+CGDCONT＝1，2"), "AT+CGDCONT=1,2");
        // 全角字母数字(输入法全角模式)一并转
        assert_eq!(punct_to_halfwidth("ＡＴ＋ＣＳＱ"), "AT+CSQ");
        // 全角空格、句号、顿号
        assert_eq!(punct_to_halfwidth("AT　A。B、C"), "AT A.B,C");
        // 纯半角原样通过
        assert_eq!(punct_to_halfwidth("AT+QMTCONN=1,\"cli\""), "AT+QMTCONN=1,\"cli\"");
    }

    /// 引号里是要发出去的正文,中文标点属于内容,不能动。
    #[test]
    fn test_punct_to_halfwidth_keeps_quoted_content() {
        assert_eq!(
            punct_to_halfwidth("AT+CMGS=\"你好,世界?\""),
            "AT+CMGS=\"你好,世界?\""
        );
        // 引号外照转,引号内照留
        assert_eq!(
            punct_to_halfwidth("AT+CMGS＝\"喂,在吗?\"，1"),
            "AT+CMGS=\"喂,在吗?\",1"
        );
    }

    /// 全角引号自己也转,并且参与配对——否则 1,“IP” 里的内容会被当成引号外。
    #[test]
    fn test_punct_to_halfwidth_fullwidth_quotes_pair_up() {
        assert_eq!(
            punct_to_halfwidth("AT+CGDCONT=1，“IP”，“cmnet”"),
            "AT+CGDCONT=1,\"IP\",\"cmnet\""
        );
    }

    /// 引号没闭合:其后一律按引号内处理,宁可少改也不改坏半条指令。
    #[test]
    fn test_punct_to_halfwidth_unclosed_quote_stops_converting() {
        assert_eq!(punct_to_halfwidth("AT+CMGS=\"你好，"), "AT+CMGS=\"你好，");
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
