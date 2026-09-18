//! MCP 工具调用记录的前端命令。记录由 mcp/server.rs 的 call_tool 写入
//! (McpShared.call_log,与 AppState.call_log 同一 Arc),这里读快照供侧栏 tab 展示,以及清空。
use tauri::{AppHandle, Emitter, State};

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

/// 清空调用记录(侧栏右键"清除",类似控制台 clear)。
/// 记录是进程级的,清空后广播 mcp-call-log-cleared 让所有窗口的面板同步清掉;
/// 不然别的窗口(或本窗口切回 tab 重新拉快照时)会把已清的记录再显示出来。
#[tauri::command]
pub fn clear_mcp_call_log(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    state
        .call_log
        .lock()
        .map(|mut log| log.clear())
        .map_err(|e| e.to_string())?;
    let _ = app.emit("mcp-call-log-cleared", ());
    Ok(())
}
