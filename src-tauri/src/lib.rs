mod buffer;
mod commands;
mod config;
mod connection;
mod logging;
mod state;
mod util;

use tauri::Manager;
use commands::connection::{connect, disconnect, list_ports};
use commands::send::{send, send_file};
use commands::config::{get_settings, save_settings, save_commands};
use commands::logging::{start_logging, stop_logging, is_logging};
use commands::sequence::{sequence_run, sequence_stop, save_sequence_config, load_sequence_config};

use state::AppState;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(AppState::new(handle));
            #[cfg(debug_assertions)]
            app.get_webview_window("main").unwrap().open_devtools();
            Ok(())
        })
        .on_window_event(|window, event| {
            // 硬约束窗口最小尺寸：decorations:false 无边框窗口下，Windows 的 resize grip
            // 能让窗口外框突破 set_min_size/配置的 minWidth（约少 16px），导致右栏右侧被截断。
            // 这里在拖动 resize 结束时检测，低于阈值则强制拉回（逻辑像素）。
            if let tauri::WindowEvent::Resized(_) = event {
                // 目标内容区 1200，但无边框窗口下 Windows resize grip 会扣约 16px，
                // 故阈值取 1216：被扣后落地 1200，右栏不再被截断。
                let min_w = 1216.0;
                let min_h = 500.0;
                if let Ok(size) = window.outer_size() {
                    let scale = window.scale_factor().unwrap_or(1.0);
                    let w = size.width as f64 / scale;
                    let h = size.height as f64 / scale;
                    if w < min_w || h < min_h {
                        let _ = window.set_size(tauri::LogicalSize::new(w.max(min_w), h.max(min_h)));
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            connect,
            disconnect,
            list_ports,
            send,
            send_file,
            get_settings,
            save_settings,
            save_commands,
            start_logging,
            stop_logging,
            is_logging,
            sequence_run,
            sequence_stop,
            save_sequence_config,
            load_sequence_config,

        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
