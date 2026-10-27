use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::connection::ConnectionHandle;
use crate::mcp::call_log::McpCallLog;
use crate::mcp::registry::RegistryHandle;

/// MCP handler 与 Tauri command 共享的状态。
/// 关键:connections 必须与 AppState.connections 是**同一个 Arc** clone,
/// 否则手动断开和 agent 断开各持一份状态(见 spec "桥接"章节)。
/// 在 lib.rs setup 里构造:从 AppState 取出 connections 的 Arc clone 装入。
/// rx_history 已下沉为 per-connection(ConnectionHandle.rx_history),
/// 无全局字段(多连接隔离,不串数据)。
pub struct McpShared {
    /// Tauri 应用句柄。生产路径恒为 Some——GUI 窗口、托盘、事件广播都靠它。
    /// None 只出现在协议测试夹具里(无 GUI 上下文):需要 GUI 的工具会明确报错,
    /// 不静默返回假数据。**不要**把它改成 per-session 的东西。
    pub app_handle: Option<tauri::AppHandle>,
    pub connections: Arc<Mutex<HashMap<String, ConnectionHandle>>>,
    /// 在途连接端口集合,同样必须与 AppState.connecting 是**同一个 Arc** clone,
    /// 否则 GUI connect 与 agent connect 各占一份,防不住同 port 并发双 spawn。
    pub connecting: crate::connection::ConnectingSet,
    /// registry 句柄,供 connect/disconnect 工具更新 connections 快照。None 时跳过更新。
    pub registry: Option<RegistryHandle>,
    /// MCP 工具调用记录(环形),与 AppState.call_log 同一 Arc:call_tool 写,前端命令读。
    pub call_log: Arc<Mutex<McpCallLog>>,
    /// 工具执行层并发闸门。旧 session 路径的 POST 会先把 SSE 响应流交回去、
    /// 工具在后台跑,HTTP 中间件的 permit 等不到工具结束;这个应用级信号量在
    /// call_tool 里兜底,新旧协议的工具执行统一受限。与中间件共用同一额度
    /// (MCP_MAX_CONCURRENT_REQUESTS),必须应用级共享,不能放 HTTP session。
    pub tool_gate: Arc<tokio::sync::Semaphore>,
    /// 恢复/诊断类工具的专用配额(disconnect / sequence_stop / get_status /
    /// list_ports / get_sequence_status,清单见 server.rs 的 PRIORITY_TOOLS)。
    /// 过载时恰恰需要这些操作来恢复,它们不能与发送类工具竞争主闸 permit,
    /// 否则会出现"服务过载到无法通过 MCP 恢复"的死结。
    pub tool_gate_priority: Arc<tokio::sync::Semaphore>,
}

impl McpShared {
    /// 生产构造:带 GUI 句柄。
    pub fn new(
        app_handle: tauri::AppHandle,
        connections: Arc<Mutex<HashMap<String, ConnectionHandle>>>,
        connecting: crate::connection::ConnectingSet,
        registry: Option<RegistryHandle>,
        call_log: Arc<Mutex<McpCallLog>>,
    ) -> Self {
        McpShared {
            app_handle: Some(app_handle),
            connections,
            connecting,
            registry,
            call_log,
            tool_gate: Arc::new(tokio::sync::Semaphore::new(
                crate::mcp::server::MCP_MAX_CONCURRENT_REQUESTS,
            )),
            tool_gate_priority: Arc::new(tokio::sync::Semaphore::new(
                crate::mcp::server::MCP_PRIORITY_TOOL_CONCURRENCY,
            )),
        }
    }

    /// 应用句柄。None = 无 GUI 上下文(协议测试夹具),此时依赖 GUI 的工具必须报错。
    pub fn app(&self) -> Option<&tauri::AppHandle> {
        self.app_handle.as_ref()
    }
}

/// 测试夹具共用的构造工具。
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// 造一个无 GUI 上下文的共享状态:连接表为空,调用日志独立。
    ///
    /// app_handle 为 None:依赖 GUI 的工具(connect/disconnect/save_settings 等)会明确报错,
    /// 纯逻辑工具(get_status/get_history_since/sequence_status…)照常工作。
    pub(crate) fn test_shared() -> Arc<McpShared> {
        Arc::new(McpShared {
            app_handle: None,
            connections: Arc::new(Mutex::new(HashMap::new())),
            connecting: crate::connection::ConnectingSet::default(),
            registry: None,
            call_log: Arc::new(Mutex::new(Default::default())),
            tool_gate: Arc::new(tokio::sync::Semaphore::new(
                crate::mcp::server::MCP_MAX_CONCURRENT_REQUESTS,
            )),
            tool_gate_priority: Arc::new(tokio::sync::Semaphore::new(
                crate::mcp::server::MCP_PRIORITY_TOOL_CONCURRENCY,
            )),
        })
    }

    /// 造一个"已连接"的假串口句柄:不碰硬件,只有一条写队列。
    ///
    /// 返回句柄与接收端——**接收端必须活着**,否则写队列断开,send 类工具会立刻报错返回。
    pub(crate) fn fake_connection(
        port: &str,
    ) -> (crate::connection::ConnectionHandle, crossbeam_channel::Receiver<crate::connection::WriteCommand>) {
        let (tx, rx) = crossbeam_channel::bounded::<crate::connection::WriteCommand>(256);
        let handle = crate::connection::ConnectionHandle {
            running: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            tx_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            rx_bytes: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            write_tx: tx,
            exit: Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
            port: port.to_string(),
            baud: 115200,
            rx_history: Arc::new(crate::buffer::rx_history::RxHistory::new(1000)),
            window_label: Arc::new(std::sync::RwLock::new(format!("mcp-{}", port))),
            line_index: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            mcp_origin: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            flow_control: crate::config::settings::FlowControl::None,
            status_port: None,
        };
        (handle, rx)
    }
}
