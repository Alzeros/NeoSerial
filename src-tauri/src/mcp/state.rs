use std::sync::{Arc, Mutex};

use crate::buffer::rx_history::RxHistory;
use crate::connection::ConnectionHandle;

/// MCP handler 与 Tauri command 共享的状态。
/// 关键:connection 必须与 AppState.connection 是**同一个 Arc** clone,
/// 否则手动断开和 agent 断开各持一份状态(见 spec "桥接"章节)。
/// 在 lib.rs setup 里构造:从 AppState 取出 connection/rx_history 的 Arc clone 装入。
pub struct McpShared {
    pub app_handle: tauri::AppHandle,
    pub rx_history: Arc<RxHistory>,
    pub connection: Arc<Mutex<Option<ConnectionHandle>>>,
}
