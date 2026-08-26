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
fn save_settings_impl(
    state: &AppState,
    settings: crate::config::settings::Settings,
) -> Result<(), String> {
    settings.save().map_err(|e| e.to_string())?;
    let mut s = state.settings.lock().map_err(|e| e.to_string())?;
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
