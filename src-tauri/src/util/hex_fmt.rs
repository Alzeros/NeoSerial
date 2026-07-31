/// 把字节按 16 字节一行格式化为十六进制转储字符串。
/// 每行格式: "41 54 2B 43 46 55 4E 3D  31 2C 31 0D 0A"
/// 两组 8 字节以两空格分隔，不带偏移量前缀和尾部 ASCII 列。
pub fn format_hex_dump(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for chunk in bytes.chunks(16) {
        // 第一组 8 字节
        for j in 0..8 {
            if j < chunk.len() {
                out.push_str(&format!("{:02X} ", chunk[j]));
            } else {
                out.push_str("   ");
            }
        }
        out.push(' '); // 组间额外空格

        // 第二组 8 字节
        for j in 8..16 {
            if j < chunk.len() {
                out.push_str(&format!("{:02X} ", chunk[j]));
            } else {
                out.push_str("   ");
            }
        }
        out.push('\n');
    }
    // 去掉末尾换行
    out.pop();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert_eq!(format_hex_dump(b""), "");
    }

    #[test]
    fn test_single_line_full_8() {
        let s = format_hex_dump(b"AT+CFUN=");
        assert!(s.starts_with("41 54 2B 43 46 55 4E 3D "));
        assert!(!s.contains('\n')); // 单行无换行
    }

    #[test]
    fn test_two_lines() {
        // 32 字节 = 两整行 16 字节
        let input: Vec<u8> = (0..32).collect();
        let s = format_hex_dump(&input);
        let lines: Vec<&str> = s.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("00 01 02 03 04 05 06 07 "));
        assert!(lines[1].starts_with("10 11 12 13 14 15 16 17 "));
    }

    #[test]
    fn test_short_line_padding() {
        // 4 字节，第二组应为空格填充
        let s = format_hex_dump(b"ABCD");
        assert!(s.starts_with("41 42 43 44 "));
        // 第二组 8 字节全是空格填充
    }

    #[test]
    fn test_no_offset_prefix() {
        // 确认没有偏移量前缀
        let input: Vec<u8> = (0..48).collect();
        let s = format_hex_dump(&input);
        assert!(!s.contains("0000:"));
        assert!(!s.contains("0010:"));
        assert!(!s.contains("0020:"));
    }
}
