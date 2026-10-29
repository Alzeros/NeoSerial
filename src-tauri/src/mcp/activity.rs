use std::collections::{BTreeMap, VecDeque};

use serde::Serialize;
use serde_json::Value;

pub const RECENT_PORT_LIMIT: usize = 64;

#[derive(Clone, Debug, Serialize)]
pub struct McpActivityCall {
    pub id: u64,
    pub tool: String,
    pub port: Option<String>,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub ok: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct McpActivitySnapshot {
    pub revision: u64,
    pub active: Vec<McpActivityCall>,
    pub recent: Vec<McpActivityCall>,
}

#[derive(Clone, Serialize)]
pub struct McpActivityState {
    pub running: bool,
    #[serde(flatten)]
    pub activity: McpActivitySnapshot,
}

#[derive(Default)]
pub struct McpActivity {
    next_id: u64,
    revision: u64,
    active: BTreeMap<u64, McpActivityCall>,
    recent: VecDeque<McpActivityCall>,
}

impl McpActivity {
    pub fn start(&mut self, tool: &str, port: Option<String>, started_at_ms: i64) -> u64 {
        self.next_id += 1;
        self.revision += 1;
        self.active.insert(
            self.next_id,
            McpActivityCall {
                id: self.next_id,
                tool: tool.chars().take(128).collect(),
                port,
                started_at_ms,
                finished_at_ms: None,
                ok: None,
            },
        );
        self.next_id
    }

    pub fn finish(&mut self, id: u64, ok: bool, finished_at_ms: i64) {
        if let Some(mut call) = self.active.remove(&id) {
            call.finished_at_ms = Some(finished_at_ms);
            call.ok = Some(ok);
            self.recent.retain(|previous| previous.port != call.port);
            self.recent.push_front(call);
            self.recent.truncate(RECENT_PORT_LIMIT);
            self.revision += 1;
        }
    }

    pub fn clear_recent(&mut self) {
        self.recent.clear();
        self.revision += 1;
    }

    pub fn snapshot(&self) -> McpActivitySnapshot {
        McpActivitySnapshot {
            revision: self.revision,
            active: self.active.values().cloned().collect(),
            recent: self.recent.iter().cloned().collect(),
        }
    }
}

pub fn call_port(tool: &str, args: &Value) -> Option<String> {
    match tool {
        "connect"
        | "disconnect"
        | "get_status"
        | "send"
        | "send_and_read"
        | "get_history_since"
        | "reset_stats"
        | "send_file"
        | "sequence_run"
        | "sequence_stop"
        | "get_sequence_status" => args
            .get("port")
            .and_then(Value::as_str)
            .filter(|port| !port.is_empty() && port.len() <= 4096)
            .map(str::to_owned),
        _ => None,
    }
}
