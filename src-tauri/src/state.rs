use std::sync::Mutex;

use tauri::AppHandle;

use crate::connection::ConnectionHandle;
use crate::logging::file_logger::FileLogger;

/// 应用全局状态。
pub struct AppState {
    pub connection: Mutex<Option<ConnectionHandle>>,
    pub file_logger: Mutex<Option<FileLogger>>,
    pub settings: Mutex<crate::config::settings::Settings>,
    #[allow(dead_code)]
    pub handle: AppHandle,
}

impl AppState {
    pub fn new(handle: AppHandle) -> Self {
        AppState {
            connection: Mutex::new(None),
            file_logger: Mutex::new(None),
            settings: Mutex::new(crate::config::settings::Settings::load()),
            handle,
        }
    }
}
