use serde::Serialize;
use crate::util::codec::bytes_to_ascii;
use crate::util::hex_fmt::format_hex_dump;

/// 数据方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Rx,
    Tx,
}

/// 日志流中的一行。预存 raw/ascii/hex 三份；文本编码转换在前端用 TextDecoder 做，
/// 这样 ASCII/UTF-8/GBK 切换无需后端参与（零 IPC 往返）。
/// line_index: 本次连接期间的单调行号(开端口=1,关闭重置),tx+rx 共用计数。
/// 旧历史(连接前已存的)line_index=0,get_history_since 返回时回填递增。
#[derive(Clone, Debug, Serialize)]
pub struct LogLine {
    pub ts: String,
    pub dir: Dir,
    pub raw: Vec<u8>,
    pub ascii: String,
    pub hex: String,
    pub is_error: bool,
    pub line_index: u64,
}

impl LogLine {
    /// 构造一行。ts 已格式化好；raw 为原始字节；error_keywords 用于判定是否标红；
    /// line_index 为本次连接期间的行号(调用方从 per-conn 计数器 fetch_add 拿)。
    pub fn new(ts: String, dir: Dir, raw: Vec<u8>, error_keywords: &[String], line_index: u64) -> Self {
        let ascii = bytes_to_ascii(&raw);
        let hex = format_hex_dump(&raw);
        let is_error = match dir {
            Dir::Rx => {
                // 用 UTF-8 lossy 解码做错误关键词匹配（中文关键词也能命中），
                // 只在此处局部计算，不存储。
                let lower = String::from_utf8_lossy(&raw).to_lowercase();
                error_keywords
                    .iter()
                    .any(|kw| lower.contains(&kw.to_lowercase()))
            }
            Dir::Tx => false,
        };
        LogLine {
            ts,
            dir,
            raw,
            ascii,
            hex,
            is_error,
            line_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kws() -> Vec<String> {
        vec!["ERROR".to_string(), "+CME ERROR".to_string()]
    }

    #[test]
    fn test_normal_line() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"OK".to_vec(), &kws(), 0);
        assert_eq!(line.ascii, "OK");
        assert!(!line.is_error);
    }

    #[test]
    fn test_error_keyword() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"+CME ERROR: 100".to_vec(), &kws(), 0);
        assert!(line.is_error);
    }

    #[test]
    fn test_error_case_insensitive() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"error: fail".to_vec(), &kws(), 0);
        assert!(line.is_error);
    }

    #[test]
    fn test_tx_never_error() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Tx, b"ERROR".to_vec(), &kws(), 0);
        assert!(!line.is_error);
    }

    #[test]
    fn test_hex_populated() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"AT".to_vec(), &kws(), 0);
        assert!(line.hex.contains("41 54"));
    }
}
