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

/// 日志流中的一行。预存 raw/ascii/hex 三份，切换显示模式时无需重新转换。
#[derive(Clone, Debug, Serialize)]
pub struct LogLine {
    pub ts: String,
    pub dir: Dir,
    pub raw: Vec<u8>,
    pub ascii: String,
    pub hex: String,
    pub is_error: bool,
}

impl LogLine {
    /// 构造一行。ts 已格式化好；raw 为原始字节；error_keywords 用于判定是否标红。
    pub fn new(ts: String, dir: Dir, raw: Vec<u8>, error_keywords: &[String]) -> Self {
        let ascii = bytes_to_ascii(&raw);
        let hex = format_hex_dump(&raw);
        let is_error = match dir {
            Dir::Rx => {
                let lower = ascii.to_lowercase();
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
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"OK".to_vec(), &kws());
        assert_eq!(line.ascii, "OK");
        assert!(!line.is_error);
    }

    #[test]
    fn test_error_keyword() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"+CME ERROR: 100".to_vec(), &kws());
        assert!(line.is_error);
    }

    #[test]
    fn test_error_case_insensitive() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"error: fail".to_vec(), &kws());
        assert!(line.is_error);
    }

    #[test]
    fn test_tx_never_error() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Tx, b"ERROR".to_vec(), &kws());
        assert!(!line.is_error);
    }

    #[test]
    fn test_hex_populated() {
        let line = LogLine::new("08:00:00.000".into(), Dir::Rx, b"AT".to_vec(), &kws());
        assert!(line.hex.contains("41 54"));
    }
}
