use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<crate::config::settings::Settings, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.clone())
}

#[tauri::command]
pub fn save_settings(
    state: State<'_, AppState>,
    settings: crate::config::settings::Settings,
) -> Result<(), String> {
    settings.save().map_err(|e| e.to_string())?;
    let mut s = state.settings.lock().map_err(|e| e.to_string())?;
    *s = settings;
    Ok(())
}

#[tauri::command]
pub fn save_commands(
    state: State<'_, AppState>,
    groups: Vec<crate::config::command_group::CommandGroup>,
) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.command_groups = groups;
    settings.save().map_err(|e| e.to_string())?;
    Ok(())
}
