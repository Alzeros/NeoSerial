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

/// 存盘用：把 raw 字节转成 ASCII，不可打印字节转成 `\\xHH`。
/// 行尾 CR/LF 先去掉——发送时 raw 自带行尾（如 `AT\r\n`），存盘线程每行还会补 `\n`，
/// 保留行尾会在文件里多出空行。行内 CR/LF 保持原样。
/// 含 line_index 前缀(本次连接期间行号) + 方向标签(跟随 dir_label 设置,与日志区一致)，
/// 字段顺序与日志区一致：`#行号 [方向] 时间 | 内容`。空内容不留下孤立的 ` | `。
pub fn format_line_for_file(line: &LogLine, cfg: &DisplayConfig, is_hex: bool) -> String {
    if !cfg.log_send && matches!(line.dir, Dir::Tx) {
        return String::new();
    }
    let tag = cfg.dir_label.tag(line.dir);
    let content = if is_hex {
        line.hex()
    } else {
        raw_to_ascii_visible(trim_trailing_newlines(&line.raw))
    };
    // line_index=0(旧数据)不显示前缀,避免显示 0
    let idx_prefix = if line.line_index > 0 {
        format!("#{} ", line.line_index)
    } else {
        String::new()
    };
    let meta = if cfg.show_timestamp {
        format!("{}{} {}", idx_prefix, tag, line.ts)
    } else {
        format!("{}{}", idx_prefix, tag)
    };
    // 对应界面元数据列与正文之间的竖线；空内容(空行)不留下孤立的 " | "。
    if content.is_empty() {
        meta
    } else {
        format!("{} | {}", meta, content)
    }
}

/// 去掉 raw 末尾的 CR/LF（发送时追加的行尾）。行内 CR/LF 不受影响。
fn trim_trailing_newlines(raw: &[u8]) -> &[u8] {
    let mut end = raw.len();
    while end > 0 && matches!(raw[end - 1], b'\r' | b'\n') {
        end -= 1;
    }
    &raw[..end]
}

/// raw bytes → 可打印 ASCII + `\xHH` 占位。
/// 行尾 CR/LF 由 [`trim_trailing_newlines`] 先去掉；行内 CR/LF 原样保留。
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
    fn test_file_format_trailing_crlf_trimmed() {
        // 行尾 CR/LF 去掉(存盘线程每行统一补 \n),不应转义也不应留下空行
        let l = rx_line("08:00:01.123", b"OK\r\n");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, false);
        assert_eq!(s, "[接收] 08:00:01.123 | OK");
        assert!(!s.contains("\\x0D"));
    }

    #[test]
    fn test_file_format_tx_crlf_has_no_extra_blank_line() {
        // 发送 raw 自带行尾(AT\r\n);存盘线程再补 \n,格式化时必须先去掉行尾,
        // 否则文件里每条发送记录后面都会多一个空行。
        let l = tx_line("08:00:01.000", b"AT\r\n");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true, dir_label: DirLabel::Full };
        assert_eq!(format_line_for_file(&l, &cfg, false), "[发送] 08:00:01.000 | AT");
    }

    #[test]
    fn test_file_format_index_then_direction_then_timestamp() {
        // 字段顺序与日志区一致:行号 → 方向 → 时间 → 内容
        let l = LogLine::new("08:00:01.000".into(), Dir::Tx, b"AT\r\n".to_vec(), &[], 5);
        let cfg = DisplayConfig { show_timestamp: true, log_send: true, dir_label: DirLabel::Full };
        assert_eq!(format_line_for_file(&l, &cfg, false), "#5 [发送] 08:00:01.000 | AT");
    }

    #[test]
    fn test_file_format_no_timestamp() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, false);
        assert_eq!(s, "[接收] | OK");
    }

    #[test]
    fn test_file_format_empty_content_has_no_separator() {
        // 空行(只有行尾)不应留下孤立的 " | "
        let l = rx_line("08:00:01.123", b"\r\n");
        let cfg = DisplayConfig { show_timestamp: true, log_send: true, dir_label: DirLabel::Full };
        assert_eq!(format_line_for_file(&l, &cfg, false), "[接收] 08:00:01.123");
    }

    #[test]
    fn test_file_format_log_send_false_drops_tx() {
        let l = tx_line("08:00:01.000", b"AT");
        let cfg = DisplayConfig { show_timestamp: true, log_send: false, dir_label: DirLabel::Full };
        assert_eq!(format_line_for_file(&l, &cfg, false), "");
    }

    #[test]
    fn test_file_format_log_send_false_keeps_rx() {
        let l = rx_line("08:00:01.123", b"OK");
        let cfg = DisplayConfig { show_timestamp: true, log_send: false, dir_label: DirLabel::Full };
        assert_eq!(format_line_for_file(&l, &cfg, false), "[接收] 08:00:01.123 | OK");
    }

    #[test]
    fn test_file_format_hex_mode() {
        let l = rx_line("08:00:01.123", b"AT");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, true);
        assert!(s.starts_with("[接收] | "));
        assert!(s.contains("41 54"));
    }

    #[test]
    fn test_file_format_mixed_printable_and_control() {
        // 0x01, 0x09(TAB), 0x7F 转 \xHH;行内 CR/LF 原样保留
        let l = rx_line("08:00:01.123", b"A\x01\x09\x0A\x0D\x7FB");
        let cfg = DisplayConfig { show_timestamp: false, log_send: true, dir_label: DirLabel::Full };
        let s = format_line_for_file(&l, &cfg, false);
        // 行内 CR/LF 原样输出(0x0A/0x0D 不转义),其余控制字符转 \xHH
        assert!(s.contains("A\\x01\\x09"));
        assert!(s.contains("\\x7FB"));
        assert!(!s.contains("\\x0A"));
        assert!(!s.contains("\\x0D"));
    }
}
