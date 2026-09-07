//! MCP 工具调用记录:进程级环形缓冲,供前端"MCP 日志"侧栏 tab 实时展示 agent 的实际操作。
//! call_tool 是唯一分发口,每次调用进出都记一条(时间、工具、入参截断、成败、耗时)。
//! 仅内存,不落盘——是观察 agent 行为的实时窗口,不是审计日志。
use std::collections::VecDeque;

use serde::Serialize;

/// 一条 MCP 工具调用记录。emit 给前端 + get_mcp_call_log 命令返回 Vec<Self>。
#[derive(Clone, Serialize)]
pub struct McpCallRecord {
    /// RFC 3339 本地时间
    pub ts: String,
    /// 工具名(list_ports / connect / send …)
    pub tool: String,
    /// 入参 JSON(截断到 ARGS_MAX 字符,超长尾部加 …(N 字符))
    pub args: String,
    /// 工具返回的 ok 字段
    pub ok: bool,
    /// 失败原因(成功为空);早返回(arg 解析失败)守卫兜底为提示
    pub error: String,
    /// 调用耗时(毫秒)
    pub duration_ms: u64,
}

/// 入参/错误文本截断上限。send_file 的 path、send 的长文本都在这之内可读。
pub const ARGS_MAX: usize = 500;

/// 环形上限。够看最近一段操作,不占内存;满了挤掉最旧。
const CAP: usize = 200;

/// 有界环形缓冲(VecDeque + 容量)。push 满了丢最旧;snapshot 拷全量给前端。
#[derive(Default)]
pub struct McpCallLog {
    records: VecDeque<McpCallRecord>,
}

impl McpCallLog {
    pub fn new() -> Self {
        Self {
            records: VecDeque::with_capacity(CAP),
        }
    }

    pub fn push(&mut self, r: McpCallRecord) {
        if self.records.len() >= CAP {
            self.records.pop_front();
        }
        self.records.push_back(r);
    }

    /// 拷一份全量(最旧在前)给前端展示。
    pub fn snapshot(&self) -> Vec<McpCallRecord> {
        self.records.iter().cloned().collect()
    }
}

/// 截断文本到 max 字符:超长则保留前 max,尾部加 …(N 字符) 标注原长。供入参/错误展示。
pub fn truncate(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let mut s: String = chars[..max].iter().collect();
    s.push_str(&format!("…({} 字符)", chars.len()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_unchanged_long_truncated() {
        assert_eq!(truncate("abc", 10), "abc");
        let long = "x".repeat(600);
        let t = truncate(&long, 500);
        assert!(t.starts_with(&"x".repeat(500)));
        assert!(t.contains("600 字符"));
    }

    #[test]
    fn test_truncate_chars_not_bytes() {
        // 中文每字一个 char,按字符数截,不按字节
        let s = "指令".repeat(300);
        let t = truncate(&s, 10);
        assert_eq!(t.chars().take(10).collect::<String>().chars().count(), 10);
        assert!(t.contains("600 字符"));
    }

    #[test]
    fn test_ring_evicts_oldest_when_full() {
        let mut log = McpCallLog::new();
        let rec = || McpCallRecord {
            ts: "t".into(),
            tool: "send".into(),
            args: "{}".into(),
            ok: true,
            error: String::new(),
            duration_ms: 1,
        };
        for _ in 0..210 {
            log.push(rec());
        }
        let snap = log.snapshot();
        assert_eq!(snap.len(), 200, "环形上限 200,超出的挤掉");
        // snapshot 最旧在前
        assert_eq!(snap[0].duration_ms, 1);
    }
}

