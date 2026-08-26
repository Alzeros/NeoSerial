use tauri::{Manager, State};

use crate::state::AppState;
use crate::logging::file_logger::FileLogger;
use crate::util::time_fmt::now_local_compact;

#[tauri::command]
pub async fn start_logging(
    app_handle: tauri::AppHandle,
    path: Option<String>,
) -> Result<String, String> {
    // 文件创建/目录创建是阻塞 IO,不放主线程。
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
        start_logging_impl(&state, path)
    })
    .await
    .map_err(|e| format!("启动日志任务执行异常: {}", e))?
}

/// start_logging 的同步实现(在阻塞线程池执行)。
fn start_logging_impl(state: &AppState, path: Option<String>) -> Result<String, String> {
    let (error_tx, _error_rx) = crossbeam_channel::unbounded();

    // 传 path → 新建/覆盖该文件（append=false）；记录为 last_log_path
    // 不传 path → 用上次路径续写（append=true）；无上次路径则生成默认路径新建
    let (actual_path, append) = match path {
        Some(p) => (p, false),
        None => {
            let last = state
                .last_log_path
                .lock()
                .ok()
                .and_then(|g| g.clone());
            match last {
                Some(p) => (p, true),
                None => {
                    let filename = format!("{}.log", now_local_compact());
                    let appdata = std::env::var("APPDATA")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::path::PathBuf::from("."));
                    let p = appdata
                        .join("neoserial")
                        .join("logs")
                        .join(filename)
                        .to_string_lossy()
                        .to_string();
                    (p, false)
                }
            }
        }
    };

    let logger = FileLogger::start(
        std::path::PathBuf::from(&actual_path),
        error_tx,
        append,
    ).map_err(|e| format!("启动日志失败: {}", e))?;

    // 记忆本次路径，供停止后续写用
    {
        let mut last = state.last_log_path.lock().map_err(|e| e.to_string())?;
        *last = Some(actual_path.clone());
    }

    // 把行文本 sender 暴露给 reader/writer 线程
    let sender = logger.line_sender();
    {
        let mut file_logger = state.file_logger.lock().map_err(|e| e.to_string())?;
        *file_logger = Some(logger);
    }
    {
        let mut sender_slot = state.line_sender.lock().map_err(|e| e.to_string())?;
        *sender_slot = Some(sender);
    }

    Ok(actual_path)
}

#[tauri::command]
pub async fn stop_logging(app_handle: tauri::AppHandle) -> Result<(), String> {
    // logger.stop()/文件句柄释放是阻塞操作,不放主线程。
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
        stop_logging_impl(&state)
    })
    .await
    .map_err(|e| format!("停止日志任务执行异常: {}", e))?
}

/// stop_logging 的同步实现(在阻塞线程池执行)。
fn stop_logging_impl(state: &AppState) -> Result<(), String> {
    // 先摘掉 sender，让 reader/writer 不再往已停止的 logger 送数据
    if let Ok(mut sender_slot) = state.line_sender.lock() {
        *sender_slot = None;
    }
    let mut file_logger = state.file_logger.lock().map_err(|e| e.to_string())?;
    if let Some(logger) = file_logger.take() {
        logger.stop();
    }
    // 注意：不清 last_log_path，再次 start_logging 不传 path 时续写同一文件
    Ok(())
}

#[tauri::command]
pub fn is_logging(state: State<'_, AppState>) -> bool {
    state.file_logger.lock().map(|g| g.is_some()).unwrap_or(false)
}
