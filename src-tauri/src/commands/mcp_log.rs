//! MCP 工具调用记录的前端读取命令。记录由 mcp/server.rs 的 call_tool 写入
//! (McpShared.call_log,与 AppState.call_log 同一 Arc),这里只读快照供侧栏 tab 展示。
use tauri::State;

use crate::mcp::call_log::McpCallRecord;
use crate::state::AppState;

/// 返回环形缓冲全量(最旧在前,上限 200)。前端开"MCP 日志"tab 时拉一次,
/// 之后靠 mcp-call 事件实时 append。
#[tauri::command]
pub fn get_mcp_call_log(state: State<'_, AppState>) -> Vec<McpCallRecord> {
    state
        .call_log
        .lock()
        .map(|log| log.snapshot())
        .unwrap_or_default()
}
