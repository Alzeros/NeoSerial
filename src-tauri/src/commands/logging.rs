use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::state::AppState;
use crate::logging::file_logger::FileLogger;
use crate::util::time_fmt::now_local_compact;

/// 文件日志的全局状态。logger 是进程级单份(所有连接进同一文件),窗口却各有一份 UI 状态,
/// 所以每次 start/stop/写盘失败都广播 `logging-changed`,各窗口据此刷新;窗口挂载时用
/// get_logging_status 拉一次(后台模式重开窗口、agent 经 MCP 开的记录都能对上)。
/// path:active 时是正在写的文件;停止后是上次路径(前端据此显示"继续记录")。
#[derive(Clone, Serialize)]
pub struct LoggingStatus {
    pub active: bool,
    pub path: Option<String>,
    /// 写盘失败的原因(仅失败广播带),前端展示后 logger 已被摘掉
    pub error: Option<String>,
}

pub(crate) fn logging_status(state: &AppState) -> LoggingStatus {
    let active_path = state
        .file_logger
        .lock()
        .ok()
        .and_then(|fl| fl.as_ref().map(|l| l.current_path()));
    let last = state.last_log_path.lock().ok().and_then(|g| g.clone());
    LoggingStatus {
        active: active_path.is_some(),
        path: active_path.or(last),
        error: None,
    }
}

fn emit_logging_changed(app_handle: &tauri::AppHandle, status: LoggingStatus) {
    let _ = app_handle.emit("logging-changed", status);
}

#[tauri::command]
pub async fn start_logging(
    app_handle: tauri::AppHandle,
    path: Option<String>,
) -> Result<String, String> {
    // 文件创建/目录创建是阻塞 IO,不放主线程。
    tauri::async_runtime::spawn_blocking(move || start_logging_impl(&app_handle, path))
        .await
        .map_err(|e| format!("启动日志任务执行异常: {}", e))?
}

/// start_logging 的同步实现(在阻塞线程池执行)。GUI 命令与 MCP 工具共用。
/// 传 path → 新建/覆盖该文件(append=false),记录为 last_log_path;
/// 不传 path → 用上次路径续写(append=true);无上次路径则生成默认路径新建。
/// 已在记录时先停掉旧的(等它写完),再开新的——同一时刻只有一个 logger。
pub(crate) fn start_logging_impl(app_handle: &tauri::AppHandle, path: Option<String>) -> Result<String, String> {
    let state = app_handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
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
                    // 默认日志目录放缓存目录(%LOCALAPPDATA%):长时间抓包可能到 GB 级,
                    // 不该跟着漫游配置同步。用户在界面上选了路径就写他选的地方,与此无关。
                    let p = crate::config::local_dir()
                        .join("logs")
                        .join(filename)
                        .to_string_lossy()
                        .to_string();
                    (p, false)
                }
            }
        }
    };

    // 旧 logger 先停(等缓冲写完),否则它的线程还在往旧文件写,界面却已经显示新路径
    if let Ok(mut fl) = state.file_logger.lock() {
        if let Some(old) = fl.take() {
            if let Ok(mut s) = state.line_sender.lock() {
                *s = None;
            }
            old.stop();
        }
    }

    let app_for_fail = app_handle.clone();
    let fail_path = actual_path.clone();
    let logger = FileLogger::start(
        std::path::PathBuf::from(&actual_path),
        append,
        move |id, msg| on_logger_failed(&app_for_fail, id, fail_path, msg),
    ).map_err(|e| format!("启动日志失败: {}", e))?;

    // 记忆本次路径,供停止后续写用
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

    emit_logging_changed(app_handle, LoggingStatus {
        active: true,
        path: Some(actual_path.clone()),
        error: None,
    });
    Ok(actual_path)
}

/// 存盘线程写失败(磁盘满、U 盘/网络盘掉了)时在该线程上被调:线程已退出,不再收行。
/// 摘掉 AppState 里的 logger 与 sender(不 join——就是自己这条线程),广播失败让所有窗口
/// 把"记录中"翻回去并显示原因。只在 AppState 里挂的还是这个失败的 logger(按 id)时才摘:
/// 用户可能已经 stop 并开了新的(甚至同一路径续写),旧线程收尾 flush 才失败,不能把新 logger 误清。
fn on_logger_failed(app_handle: &tauri::AppHandle, failed_id: u64, path: String, msg: String) {
    let Some(state) = app_handle.try_state::<AppState>() else { return };
    let still_active = state
        .file_logger
        .lock()
        .ok()
        .map(|fl| fl.as_ref().is_some_and(|l| l.id() == failed_id))
        .unwrap_or(false);
    if still_active {
        if let Ok(mut s) = state.line_sender.lock() {
            *s = None;
        }
        if let Ok(mut fl) = state.file_logger.lock() {
            // drop 而非 stop():stop 会等本线程自己退出
            let _ = fl.take();
        }
    }
    emit_logging_changed(app_handle, LoggingStatus {
        active: false,
        path: Some(path),
        error: Some(msg),
    });
}

#[tauri::command]
pub async fn stop_logging(app_handle: tauri::AppHandle) -> Result<(), String> {
    // logger.stop() 会等存盘线程写完,不放主线程。
    tauri::async_runtime::spawn_blocking(move || stop_logging_impl(&app_handle))
        .await
        .map_err(|e| format!("停止日志任务执行异常: {}", e))?
}

/// stop_logging 的同步实现。GUI 命令、MCP 工具、退出流程(cleanup_and_exit)共用。
/// 等存盘线程把缓冲写完再返回,所以退出前调它不会丢最后几行。未在记录时是 no-op。
pub(crate) fn stop_logging_impl(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
    // 先摘掉 sender,让 reader/writer 不再往已停止的 logger 送数据
    if let Ok(mut sender_slot) = state.line_sender.lock() {
        *sender_slot = None;
    }
    let logger = {
        let mut file_logger = state.file_logger.lock().map_err(|e| e.to_string())?;
        file_logger.take()
    };
    let Some(logger) = logger else { return Ok(()) };
    logger.stop();
    // 注意:不清 last_log_path,再次 start_logging 不传 path 时续写同一文件
    emit_logging_changed(app_handle, logging_status(&state));
    Ok(())
}

#[tauri::command]
pub fn get_logging_status(state: State<'_, AppState>) -> LoggingStatus {
    logging_status(&state)
}
