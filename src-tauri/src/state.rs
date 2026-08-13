use std::sync::{Arc, Mutex};

use crossbeam_channel::Sender;
use tauri::AppHandle;

use crate::buffer::rx_history::RxHistory;
use crate::connection::ConnectionHandle;
use crate::logging::file_logger::FileLogger;

/// 应用全局状态。
pub struct AppState {
    /// 连接句柄。Arc<Mutex<>> 是为与 MCP handler 共享**同一**连接状态
    /// (手动断开和 agent 断开操作同一个 Mutex,失去"同一条数据流"意义则败)。
    pub connection: Arc<Mutex<Option<ConnectionHandle>>>,
    pub file_logger: Mutex<Option<FileLogger>>,
    /// 文件日志的行文本通道。start_logging 时写入,stop_logging 时置 None。
    /// reader/writer 线程通过 app_handle.state() 拿到它,把每行格式化后送存盘线程。
    pub line_sender: Mutex<Option<Sender<String>>>,
    /// 上次日志保存路径。stop_logging 后保留,再次 start_logging 不传 path 时续写此文件。
    pub last_log_path: Mutex<Option<String>>,
    pub settings: Mutex<crate::config::settings::Settings>,
    /// 后端 rx 历史,供 MCP 阻塞读查询。reader 线程喂入,MCP handler 读取。
    pub rx_history: Arc<RxHistory>,
    #[allow(dead_code)]
    pub handle: AppHandle,
}

impl AppState {
    pub fn new(handle: AppHandle) -> Self {
        AppState {
            connection: Arc::new(Mutex::new(None)),
            file_logger: Mutex::new(None),
            line_sender: Mutex::new(None),
            last_log_path: Mutex::new(None),
            settings: Mutex::new(crate::config::settings::Settings::load()),
            rx_history: Arc::new(RxHistory::new(10_000)),
            handle,
        }
    }
}
