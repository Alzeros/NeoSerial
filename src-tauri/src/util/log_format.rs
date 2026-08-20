use crate::buffer::log_line::{Dir, LogLine};

/// 显示与存盘共用的开关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayConfig {
    pub show_timestamp: bool,
    pub log_send: bool,
    /// 方向标签:'short'=Tx/Rx, 'full'=发送/接收。存盘跟随此设置(与日志区一致)。
    pub dir_label: DirLabel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirLabel {
    Short, // Tx / Rx
    Full,  // 发送 / 接收
}

impl DirLabel {
    fn tag(self, dir: Dir) -> &'static str {
        match (self, dir) {
            (DirLabel::Short, Dir::Tx) => "[Tx]",
            (DirLabel::Short, Dir::Rx) => "[Rx]",
            (DirLabel::Full, Dir::Tx) => "[发送]",
            (DirLabel::Full, Dir::Rx) => "[接收]",
        }
    }
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
    let tag = cfg.dir_label.tag(line.dir);
    let content = if is_hex { &line.hex } else { &line.ascii };
    if cfg.show_timestamp {
        format!("{} {} {}", line.ts, tag, content)
    } else {
        format!("{} {}", tag, content)
    }
}

/// 存盘用：把 raw 字节转成 ASCII，不可打印字节（CR/LF/控制字符）转成 `\\xHH`，
/// 但行尾 CR/LF 不转义（它们是行结构的一部分，存盘每行本就以换行结尾）。
/// 含 line_index 前缀(本次连接期间行号) + 方向标签(跟随 dir_label 设置,与日志区一致)。
pub fn format_line_for_file(line: &LogLine, cfg: &DisplayConfig, is_hex: bool) -> String {
    if !cfg.log_send && matches!(line.dir, Dir::Tx) {
        return String::new();
    }
    let tag = cfg.dir_label.tag(line.dir);
    let content = if is_hex {
        line.hex.clone()
    } else {
        raw_to_ascii_visible(&line.raw)
    };
    // line_index=0(旧数据)不显示前缀,避免显示 0
    let idx_prefix = if line.line_index > 0 {
        format!("#{} ", line.line_index)
    } else {
        String::new()
    };
    if cfg.show_timestamp {
        format!("{}{} {} {}", idx_prefix, line.ts, tag, content)
    } else {
        format!("{}{} {}", idx_prefix, tag, content)
    }
}

/// raw bytes → 可打印 ASCII + `\xHH` 占位。
/// CR(0x0D)/LF(0x0A) 不转义:行尾换行是行结构的一部分,存盘按行写文件本身有换行。
/// 其余控制字节(0x00-0x1F, 0x7F)转 \xHH 保留原始信息。
fn raw_to_ascii_visible(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for &b in raw {
        if (0x20..=0x7e).contains(&b) {
            // 可打印 ASCII 字节（0x20-0x7E）转 char 是安全的
            out.push(b as char);
        } else if b == 0x0D || b == 0x0A {
            // CR/LF 不转义(行尾,文件按行写)
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
        let cfg = DisplayConfig { show_timestamp: true, log_send: true, dir_label: DirLabel::Full };
        assert_eq!(format_line(&l, &cfg, false), "08:00:01.123 [接收] OK");
    }

    #[test]
    fn test_tx_with_timestamp() {
        let l = tx_line("08:00:01.000", b"AT");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true, dir_label: DirLabel::Full };
        assert_eq!(format_line(&l, &cfg, false), "08:00:01.000 [发送] AT");
    }

    #[test]
    fn test_no_timestamp() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true, dir_label: DirLabel::Full };
        assert_eq!(format_line(&l, &cfg, false), "[接收] OK");
    }

    #[test]
    fn test_log_send_false_drops_tx() {
        let l = tx_line("08:00:01.000", b"AT");
        let cfg = DisplayConfig { show_timestamp: true, log_send: false, dir_label: DirLabel::Full };
        assert_eq!(format_line(&l, &cfg, false), "");
    }

    #[test]
    fn test_log_send_false_keeps_rx() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: true, log_send: false, dir_label: DirLabel::Full };
        assert_eq!(format_line(&l, &cfg, false), "08:00:01.123 [接收] OK");
    }

    #[test]
    fn test_hex_mode() {
        let l = rx_line("08:00:01.123", b"AT");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true, dir_label: DirLabel::Full };
        let s = format_line(&l, &cfg, true);
        assert!(s.starts_with("[接收] "));
        assert!(s.contains("41 54"));
    }

    // ===== format_line_for_file =====

    #[test]
    fn test_file_format_crlf_shown_as_escape() {
        // CRLF 不转义(行尾),只看可见内容部分
        let l = rx_line("08:00:01.123", b"OK\r\n");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, false);
        assert!(s.starts_with("08:00:01.123 [接收] OK"));
        assert!(!s.contains("\\x0D"));
    }

    #[test]
    fn test_file_format_tx_crlf() {
        let l = tx_line("08:00:01.000", b"AT\r\n");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, false);
        assert!(s.starts_with("08:00:01.000 [发送] AT"));
    }

    #[test]
    fn test_file_format_no_timestamp() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, false);
        assert_eq!(s, "[接收] OK");
    }

    #[test]
    fn test_file_format_log_send_false_drops_tx() {
        let l = tx_line("08:00:01.000", b"AT");
        let cfg = DisplayConfig { show_timestamp: true, log_send: false, dir_label: DirLabel::Full };
        assert_eq!(format_line_for_file(&l, &cfg, false), "");
    }

    #[test]
    fn test_file_format_hex_mode() {
        let l = rx_line("08:00:01.123", b"AT");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, true);
        assert!(s.starts_with("[接收] "));
        assert!(s.contains("41 54"));
    }

    #[test]
    fn test_file_format_mixed_printable_and_control() {
        // 0x01, 0x09(TAB), 0x7F 转 \xHH;CR/LF 不转义(行尾)
        let l = rx_line("08:00:01.123", b"A\x01\x09\x0A\x0D\x7FB");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, false);
        // CR/LF 原样输出(0x0A/0x0D 不转义),其余控制字符转 \xHH
        assert!(s.contains("A\\x01\\x09"));
        assert!(s.contains("\\x7FB"));
        assert!(!s.contains("\\x0A"));
        assert!(!s.contains("\\x0D"));
    }
}
