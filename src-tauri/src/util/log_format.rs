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

#[cfg(test)]
mod tests {
    use super::*;

    fn rx_line(ts: &str, content: &[u8]) -> LogLine {
        LogLine::new(ts.into(), Dir::Rx, content.to_vec(), &[])
    }
    fn tx_line(ts: &str, content: &[u8]) -> LogLine {
        LogLine::new(ts.into(), Dir::Tx, content.to_vec(), &[])
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
}
