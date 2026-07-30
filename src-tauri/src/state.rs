use std::sync::Mutex;

use crossbeam_channel::Sender;
use tauri::AppHandle;

use crate::connection::ConnectionHandle;
use crate::logging::file_logger::FileLogger;

/// 应用全局状态。
pub struct AppState {
    pub connection: Mutex<Option<ConnectionHandle>>,
    pub file_logger: Mutex<Option<FileLogger>>,
    /// 文件日志的行文本通道。start_logging 时写入，stop_logging 时置 None。
    /// reader/writer 线程通过 app_handle.state() 拿到它，把每行格式化后送存盘线程。
    pub line_sender: Mutex<Option<Sender<String>>>,
    pub settings: Mutex<crate::config::settings::Settings>,
    #[allow(dead_code)]
    pub handle: AppHandle,
}

impl AppState {
    pub fn new(handle: AppHandle) -> Self {
        AppState {
            connection: Mutex::new(None),
            file_logger: Mutex::new(None),
            line_sender: Mutex::new(None),
            settings: Mutex::new(crate::config::settings::Settings::load()),
            handle,
        }
    }
}
