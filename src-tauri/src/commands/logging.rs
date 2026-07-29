use tauri::State;

use crate::state::AppState;
use crate::logging::file_logger::FileLogger;
use crate::util::time_fmt::now_local_compact;

#[tauri::command]
pub fn start_logging(
    state: State<'_, AppState>,
    path: Option<String>,
) -> Result<String, String> {
    let (error_tx, _error_rx) = crossbeam_channel::unbounded();
    let actual_path = path.unwrap_or_else(|| {
        let filename = format!("{}.log", now_local_compact());
        let appdata = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        appdata.join("neoserial").join("logs").join(filename).to_string_lossy().to_string()
    });

    let logger = FileLogger::start(
        std::path::PathBuf::from(&actual_path),
        error_tx,
    ).map_err(|e| format!("启动日志失败: {}", e))?;

    let mut file_logger = state.file_logger.lock().map_err(|e| e.to_string())?;
    *file_logger = Some(logger);

    Ok(actual_path)
}

#[tauri::command]
pub fn stop_logging(state: State<'_, AppState>) -> Result<(), String> {
    let mut file_logger = state.file_logger.lock().map_err(|e| e.to_string())?;
    if let Some(logger) = file_logger.take() {
        logger.stop();
    }
    Ok(())
}

#[tauri::command]
pub fn is_logging(state: State<'_, AppState>) -> bool {
    state.file_logger.lock().map(|g| g.is_some()).unwrap_or(false)
}
