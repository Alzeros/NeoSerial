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

/// 日志流中的一行。预存 raw/ascii 两份；文本编码转换在前端用 TextDecoder 做，
/// 这样 ASCII/UTF-8/GBK 切换无需后端参与（零 IPC 往返）。
/// hex dump 不预存：前端 hex 视图用 raw 自行渲染，文件日志恒用 ascii，
/// 唯一消费者是 MCP get_history_since(is_hex=true)——按需经 [`LogLine::hex`]
/// 现算（预存一份 ~3x raw 大小的字符串对绝大多数行是纯浪费）。
/// line_index: 本次连接期间的单调行号(开端口=1,关闭重置),tx+rx 共用计数。
/// 旧历史(连接前已存的)line_index=0,get_history_since 返回时回填递增。
#[derive(Clone, Debug, Serialize)]
pub struct LogLine {
    pub ts: String,
    /// 与 ts 同一次取钟的 Unix 毫秒。MCP get_history_since 透传给 agent 做时间轴;
    /// 0 表示旧数据/测试构造(用 `new`),生产路径一律用 `now`。
    pub ts_ms: u64,
    pub dir: Dir,
    pub raw: Vec<u8>,
    pub ascii: String,
    pub is_error: bool,
    pub line_index: u64,
}

impl LogLine {
    /// 以当前时间构造一行(生产路径唯一入口):ts 与 ts_ms 来自同一次取钟,严格一致。
    pub fn now(dir: Dir, raw: Vec<u8>, error_keywords: &[String], line_index: u64) -> Self {
        let (ts, ts_ms) = crate::util::time_fmt::now_local_ts_pair();
        let mut line = Self::new(ts, dir, raw, error_keywords, line_index);
        line.ts_ms = ts_ms;
        line
    }

    /// 构造一行。ts 已格式化好；raw 为原始字节；error_keywords 用于判定是否标红；
    /// line_index 为本次连接期间的行号(调用方从 per-conn 计数器 fetch_add 拿)。
    /// ts_ms 置 0:只给测试与旧数据用,生产路径用 `now`。
    pub fn new(ts: String, dir: Dir, raw: Vec<u8>, error_keywords: &[String], line_index: u64) -> Self {
        let ascii = bytes_to_ascii(&raw);
        let is_error = match dir {
            // 只有 Rx 行参与判定:reader 传设置里的 error_keywords,Tx 路径传 &[](自己发的
            // 命令永不标红)。关键词为空时早退,省掉 from_utf8_lossy + to_lowercase 两次分配。
            Dir::Rx if !error_keywords.is_empty() => {
                // 用 UTF-8 lossy 解码做错误关键词匹配（中文关键词也能命中），
                // 只在此处局部计算，不存储。
                let lower = String::from_utf8_lossy(&raw).to_lowercase();
                error_keywords
                    .iter()
                    .any(|kw| lower.contains(&kw.to_lowercase()))
            }
            _ => false,
        };
        LogLine {
            ts,
            ts_ms: 0,
            dir,
            raw,
            ascii,
            is_error,
            line_index,
        }
    }

    /// 按需计算 16 字节/行的 hex dump（见结构体注释：唯一消费者是 MCP is_hex 路径）。
    pub fn hex(&self) -> String {
        format_hex_dump(&self.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kws() -> Vec<String> {
        vec!["ERROR".to_string(), "+CME ERROR".to_string()]
    }

    /// now():ts_ms 是当前 Unix 毫秒,ts 是同一时刻的 HH:MM:SS.mmm,毫秒位一致;new():ts_ms=0。
    #[test]
    fn test_now_stamps_consistent_ts_and_ts_ms() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let line = LogLine::now(Dir::Rx, b"OK".to_vec(), &kws(), 7);
        assert!(line.ts_ms >= before && line.ts_ms <= before + 1000, "ts_ms 应是当前 Unix 毫秒: {}", line.ts_ms);
        assert_eq!(line.ts.len(), "HH:MM:SS.mmm".len(), "ts 格式: {}", line.ts);
        let millis_in_ts: u64 = line.ts[9..].parse().unwrap();
        assert_eq!(millis_in_ts, line.ts_ms % 1000, "ts 的毫秒位应与 ts_ms 同一次取钟");
        assert_eq!(line.line_index, 7);

        let fixed = LogLine::new("08:00:00.000".into(), Dir::Rx, b"OK".to_vec(), &kws(), 0);
        assert_eq!(fixed.ts_ms, 0, "手工 ts 的构造不猜 ts_ms");
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
        assert!(line.hex().contains("41 54"));
    }
}
