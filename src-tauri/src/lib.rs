mod buffer;
mod commands;
mod config;
mod connection;
mod logging;
mod mcp;
mod state;
mod util;

use tauri::Manager;
use commands::connection::{connect, disconnect, list_ports, reset_stats};
use commands::send::{send, send_file};
use commands::config::{get_settings, save_settings, save_commands};
use commands::logging::{start_logging, stop_logging, is_logging};
use commands::sequence::{sequence_run, sequence_stop, save_sequence_config, load_sequence_config, save_sequence_auto, load_sequence_auto};

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
            let state = AppState::new(handle.clone());
            // 选端口 + 登记 registry(先于 McpShared,以便注入同一 registry clone)
            let port = match mcp::server::pick_port() {
                Ok(p) => p,
                Err(e) => {
                    log::error!("MCP 端口分配失败: {}", e);
                    // 端口分配失败不阻断主程序,只是 MCP 不可用
                    #[cfg(debug_assertions)]
                    app.get_webview_window("main").unwrap().open_devtools();
                    return Ok(());
                }
            };
            let registry = mcp::registry::RegistryHandle::register(port).ok();
            // 从 AppState 提取共享字段给 MCP(必须是同一 Arc clone)
            // registry 是同一 clone,供 connect/disconnect 工具调 update_com
            let mcp_shared = std::sync::Arc::new(mcp::state::McpShared {
                app_handle: handle.clone(),
                rx_history: state.rx_history.clone(),
                connection: state.connection.clone(),
                registry: registry.clone(),
            });
            app.manage(state);
            // 起 HTTP server(tauri::async_runtime::spawn,勿用 tokio::spawn)
            let shared_clone = mcp_shared.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = mcp::server::run_server(shared_clone, port).await {
                    log::error!("MCP server 退出: {}", e);
                }
            });
            // 心跳:每 5s 更新 registry(若登记成功)
            if let Some(reg) = registry.clone() {
                tauri::async_runtime::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                    loop {
                        interval.tick().await;
                        let _ = reg.heartbeat();
                    }
                });
            }
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
            reset_stats,
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
            save_sequence_auto,
            load_sequence_auto,

        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
