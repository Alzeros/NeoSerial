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
