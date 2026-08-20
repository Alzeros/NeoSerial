use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crossbeam_channel::Sender;
use tauri::AppHandle;

use crate::connection::ConnectionHandle;
use crate::logging::file_logger::FileLogger;
use crate::mcp::registry::RegistryHandle;

/// 应用全局状态。
pub struct AppState {
    /// 多连接句柄表:port → ConnectionHandle。Arc<Mutex<>> 是为与 MCP handler 共享**同一**连接状态
    /// (手动断开和 agent 断开操作同一个 Mutex,失去"同一条数据流"意义则败)。
    /// Tauri command 用 resolve_port 从中取出指定端口或唯一连接。
    pub connections: Arc<Mutex<HashMap<String, ConnectionHandle>>>,
    /// 在途连接的端口集合(打开串口是阻塞操作,不能持 connections 锁做)。
    /// 与 McpShared.connecting 必须是**同一个 Arc** clone,否则 GUI 与 agent 各占一份,
    /// 两边同时 connect 同一 port 时防不住 TOCTOU。见 ConnectingGuard。
    pub connecting: crate::connection::ConnectingSet,
    pub file_logger: Mutex<Option<FileLogger>>,
    /// 文件日志的行文本通道。start_logging 时写入,stop_logging 时置 None。
    /// reader/writer 线程通过 app_handle.state() 拿到它,把每行格式化后送存盘线程。
    pub line_sender: Mutex<Option<Sender<String>>>,
    /// 上次日志保存路径。stop_logging 后保留,再次 start_logging 不传 path 时续写此文件。
    pub last_log_path: Mutex<Option<String>>,
    pub settings: Mutex<crate::config::settings::Settings>,
    /// MCP server 当前监听端口(运行中 Some(port),未启动 None)。供前端显示实际连接指令。
    pub mcp_port: Mutex<Option<u16>>,
    /// MCP registry 句柄,进程退出时调 unregister 注销。
    pub registry: Option<RegistryHandle>,
    /// 开窗时若带 port,把 {窗口label→(port,baud)} 记进此 pending,
    /// 新窗口 onMount 调 take_pending_takeover 取走后自动 connect 接管。
    /// 用 pending 而非 emit 事件:窗口 JS 加载有先后,事件可能早于监听注册丢失。
    pub pending_takeover: Mutex<std::collections::HashMap<String, (String, u32)>>,
    #[allow(dead_code)]
    pub handle: AppHandle,
}

impl AppState {
    pub fn new(handle: AppHandle) -> Self {
        AppState {
            connections: Arc::new(Mutex::new(HashMap::new())),
            connecting: Arc::new(Mutex::new(std::collections::HashSet::new())),
            file_logger: Mutex::new(None),
            line_sender: Mutex::new(None),
            last_log_path: Mutex::new(None),
            settings: Mutex::new(crate::config::settings::Settings::load()),
            mcp_port: Mutex::new(None),
            registry: None,
            pending_takeover: Mutex::new(std::collections::HashMap::new()),
            handle,
        }
    }
}
