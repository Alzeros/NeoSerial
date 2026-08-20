use crate::buffer::log_line::{Dir, LogLine};

/// 显示与存盘共用的开关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayConfig {
    pub show_timestamp: bool,
    pub log_send: bool,
}

/// 把一行 LogLine 格式化为 "[ts] [tag] content" 形式的字符串（不含末尾换行符）。
/// - show_timestamp=false 时省略时间戳前缀。
/// - log_send=false 且方向为 Tx 时返回空字符串（该行不记录）。
/// - is_hex=true 时 content 用 hex dump，否则用 ascii。
/// 注：UI 显示用 `line.ascii`/`line.hex`，存盘用 `format_line_for_file`，此函数仅保留给测试。
#[allow(dead_code)]
pub fn format_line(line: &LogLine, cfg: &DisplayConfig, is_hex: bool) -> String {
    if !cfg.log_send && matches!(line.dir, Dir::Tx) {
        return String::new();
    }
    let tag = if matches!(line.dir, Dir::Tx) { "[发送]" } else { "[接收]" };
    let content = if is_hex { &line.hex } else { &line.ascii };
    if cfg.show_timestamp {
        format!("{} {} {}", line.ts, tag, content)
    } else {
        format!("{} {}", tag, content)
    }
}

/// 存盘用：把 raw 字节转成 ASCII，不可打印字节（含 CR/LF/TAB 等）显式转成 `\\xHH` 形式，
/// 避免文件里出现 ".." 或奇怪的符号，同时保留全部原始信息便于事后分析。
pub fn format_line_for_file(line: &LogLine, cfg: &DisplayConfig, is_hex: bool) -> String {
    if !cfg.log_send && matches!(line.dir, Dir::Tx) {
        return String::new();
    }
    let tag = if matches!(line.dir, Dir::Tx) { "[发送]" } else { "[接收]" };
    let content = if is_hex {
        line.hex.clone()
    } else {
        raw_to_ascii_visible(&line.raw)
    };
    if cfg.show_timestamp {
        format!("{} {} {}", line.ts, tag, content)
    } else {
        format!("{} {}", tag, content)
    }
}

/// raw bytes → 可打印 ASCII + `\xHH` 占位。例如 `OK\r\n` → `OK\x0D\x0A`。
/// 用 String::push 逐字节拼接，仅当字节为可打印 ASCII 时原样输出，其余转为 `\xHH`，
/// 保证不会出现非法 UTF-8（`b as char` 对 0x80+ 会产生多字节字符，但这里已过滤掉）。
fn raw_to_ascii_visible(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for &b in raw {
        if (0x20..=0x7e).contains(&b) {
            // 可打印 ASCII 字节（0x20-0x7E）转 char 是安全的
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rx_line(ts: &str, content: &[u8]) -> LogLine {
        LogLine::new(ts.into(), Dir::Rx, content.to_vec(), &[], 0)
    }
    fn tx_line(ts: &str, content: &[u8]) -> LogLine {
        LogLine::new(ts.into(), Dir::Tx, content.to_vec(), &[], 0)
    }

    #[test]
    fn test_rx_with_timestamp() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true };
        assert_eq!(format_line(&l, &cfg, false), "08:00:01.123 [接收] OK");
    }

    #[test]
    fn test_tx_with_timestamp() {
        let l = tx_line("08:00:01.000", b"AT");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true };
        assert_eq!(format_line(&l, &cfg, false), "08:00:01.000 [发送] AT");
    }

    #[test]
    fn test_no_timestamp() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true };
        assert_eq!(format_line(&l, &cfg, false), "[接收] OK");
    }

    #[test]
    fn test_log_send_false_drops_tx() {
        let l = tx_line("08:00:01.000", b"AT");
        let cfg = DisplayConfig { show_timestamp: true, log_send: false };
        assert_eq!(format_line(&l, &cfg, false), "");
    }

    #[test]
    fn test_log_send_false_keeps_rx() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: true, log_send: false };
        assert_eq!(format_line(&l, &cfg, false), "08:00:01.123 [接收] OK");
    }

    #[test]
    fn test_hex_mode() {
        let l = rx_line("08:00:01.123", b"AT");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true };
        let s = format_line(&l, &cfg, true);
        assert!(s.starts_with("[接收] "));
        assert!(s.contains("41 54"));
    }

    // ===== format_line_for_file =====

    #[test]
    fn test_file_format_crlf_shown_as_escape() {
        let l = rx_line("08:00:01.123", b"OK\r\n");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true };
        let s = format_line_for_file(&l, &cfg, false);
        assert_eq!(s, "08:00:01.123 [接收] OK\\x0D\\x0A");
    }

    #[test]
    fn test_file_format_tx_crlf() {
        let l = tx_line("08:00:01.000", b"AT\r\n");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true };
        let s = format_line_for_file(&l, &cfg, false);
        assert_eq!(s, "08:00:01.000 [发送] AT\\x0D\\x0A");
    }

    #[test]
    fn test_file_format_no_timestamp() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true };
        let s = format_line_for_file(&l, &cfg, false);
        assert_eq!(s, "[接收] OK");
    }

    #[test]
    fn test_file_format_log_send_false_drops_tx() {
        let l = tx_line("08:00:01.000", b"AT");
        let cfg = DisplayConfig { show_timestamp: true, log_send: false };
        assert_eq!(format_line_for_file(&l, &cfg, false), "");
    }

    #[test]
    fn test_file_format_hex_mode() {
        let l = rx_line("08:00:01.123", b"AT");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true };
        let s = format_line_for_file(&l, &cfg, true);
        assert!(s.starts_with("[接收] "));
        assert!(s.contains("41 54"));
    }

    #[test]
    fn test_file_format_mixed_printable_and_control() {
        // 0x01, 0x09(TAB), 0x0A(LF), 0x0D(CR), 0x7F 都应转成 \xHH
        let l = rx_line("08:00:01.123", b"A\x01\x09\x0A\x0D\x7FB");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true };
        let s = format_line_for_file(&l, &cfg, false);
        assert_eq!(s, "[接收] A\\x01\\x09\\x0A\\x0D\\x7FB");
    }
}
