use tauri::{Manager, State};

use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<crate::config::settings::Settings, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
pub async fn save_settings(
    app_handle: tauri::AppHandle,
    settings: crate::config::settings::Settings,
) -> Result<(), String> {
    // settings.save() 写磁盘 JSON,不放主线程。
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
        save_settings_impl(&state, settings)
    })
    .await
    .map_err(|e| format!("保存设置任务执行异常: {}", e))?
}

/// save_settings 的同步实现(在阻塞线程池执行)。
///
/// 前端回传的是**整份**Settings,基于它启动时(或上次保存时)缓存的快照。后端自己改过的
/// 字段(目前是 resolve_close 写的 ui.close_prompted)前端拿不到,若照单全收,下一次任何
/// 前端保存(断开触发的 persistSettings、拨开关、设置页保存)都会把它冲回旧值——
/// 用户勾了"不再提醒"下次点 × 又弹。这类"只归后端写"的字段以内存态为准,见
/// [`Settings::merge_backend_owned`]。锁内落盘与 save_commands_impl 一致:先改内存再写盘
/// 会在写失败时内存/磁盘不一致,先写盘再改内存又要两次取锁给 resolve_close 留竞态窗口。
fn save_settings_impl(
    state: &AppState,
    mut settings: crate::config::settings::Settings,
) -> Result<(), String> {
    let mut s = state.settings.lock().map_err(|e| e.to_string())?;
    settings.merge_backend_owned(&s);
    settings.save().map_err(|e| e.to_string())?;
    *s = settings;
    Ok(())
}

#[tauri::command]
pub async fn save_commands(
    app_handle: tauri::AppHandle,
    groups: Vec<crate::config::command_group::CommandGroup>,
) -> Result<(), String> {
    // settings.save() 写磁盘 JSON,不放主线程。
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
        save_commands_impl(&state, groups)
    })
    .await
    .map_err(|e| format!("保存指令组任务执行异常: {}", e))?
}

/// save_commands 的同步实现(在阻塞线程池执行)。
fn save_commands_impl(
    state: &AppState,
    groups: Vec<crate::config::command_group::CommandGroup>,
) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.command_groups = groups;
    settings.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// 导出自定义主题到指定路径。data 为前端序列化的主题 JSON(结构由前端定义,透传)。
#[tauri::command]
pub async fn export_theme_file(path: String, data: serde_json::Value) -> Result<(), String> {
    // 序列化 + 磁盘写入是阻塞操作,不放主线程。
    tauri::async_runtime::spawn_blocking(move || {
        let text = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("写入主题文件失败: {}", e))
    })
    .await
    .map_err(|e| format!("导出主题任务执行异常: {}", e))?
}

/// 从指定路径读入主题 JSON。字段校验由前端完成。
#[tauri::command]
pub async fn import_theme_file(path: String) -> Result<serde_json::Value, String> {
    let text = tauri::async_runtime::spawn_blocking(move || {
        std::fs::read_to_string(&path).map_err(|e| format!("读取主题文件失败: {}", e))
    })
    .await
    .map_err(|e| format!("导入主题任务执行异常: {}", e))??;
    serde_json::from_str(&text).map_err(|e| format!("主题文件不是有效 JSON: {}", e))
}
