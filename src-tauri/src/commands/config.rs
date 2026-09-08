use tauri::{Emitter, Manager, State};

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
        save_settings_impl(&state, settings)?;
        // 发送历史上限调小了就当场裁掉多余的(自带落盘与广播)
        crate::commands::command_index::enforce_history_limit(&handle, &state);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("保存设置任务执行异常: {}", e))??;
    // 后台运行开关即时生效:托盘图标随之出现/消失
    crate::tray::sync_visibility(&app_handle);
    // 通知所有窗口刷新各自的 settings 快照:窗口整份回写时以它为底,不刷新的话
    // 别的窗口/agent 改过的字段(如 background_mode)会被这里的旧快照冲回去。
    let _ = app_handle.emit("settings-changed", ());
    Ok(())
}

/// save_settings 的同步实现(在阻塞线程池执行)。
///
/// 回传的是**整份**Settings,基于调用方(MCP agent)手里的快照。后端自己改过的
/// 字段(目前是首次收托盘时写的 ui.tray_hint_shown)调用方拿不到,若照单全收会被冲回旧值——
/// 下次收托盘又弹"仍在后台运行"的通知。这类"只归后端写"的字段以内存态为准,见
/// [`Settings::merge_backend_owned`]。锁内落盘与 save_commands_impl 一致:先改内存再写盘
/// 会在写失败时内存/磁盘不一致,先写盘再改内存又要两次取锁给后端写入留竞态窗口。
/// GUI 窗口不再走这条整份覆盖的路,改用 patch_settings 只写自己改的字段。
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

/// 局部更新设置:patch 是 Settings 的任意子集(嵌套对象按键深合并,数组/标量整体替换),
/// 以后端内存态为底,落盘并广播 settings-changed,返回合并后的完整 Settings 供调用方刷新快照。
/// 各窗口/主题编辑器只回写自己改动的字段,持旧快照也不会把别处刚保存的字段冲回去。
/// 值无效(类型不对、端口越界)→ Err,内存与磁盘都不动。
#[tauri::command]
pub async fn patch_settings(
    app_handle: tauri::AppHandle,
    patch: serde_json::Value,
) -> Result<crate::config::settings::Settings, String> {
    let handle = app_handle.clone();
    let next = tauri::async_runtime::spawn_blocking(move || {
        let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
        let next = patch_settings_impl(&state, &patch)?;
        // 发送历史上限调小了就当场裁掉多余的(自带落盘与广播)
        crate::commands::command_index::enforce_history_limit(&handle, &state);
        Ok::<_, String>(next)
    })
    .await
    .map_err(|e| format!("保存设置任务执行异常: {}", e))??;
    // 后台运行开关即时生效:托盘图标随之出现/消失
    crate::tray::sync_visibility(&app_handle);
    let _ = app_handle.emit("settings-changed", ());
    Ok(next)
}

fn patch_settings_impl(
    state: &AppState,
    patch: &serde_json::Value,
) -> Result<crate::config::settings::Settings, String> {
    let mut s = state.settings.lock().map_err(|e| e.to_string())?;
    let next = s.apply_patch(patch)?;
    next.save().map_err(|e| e.to_string())?;
    *s = next.clone();
    Ok(next)
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
