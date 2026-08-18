mod buffer;
mod commands;
mod config;
mod connection;
mod logging;
mod mcp;
mod state;
mod util;

use tauri::Manager;
use commands::connection::{connect, disconnect, list_ports, reset_stats, open_port_window, get_window_conn_state};
use commands::send::{send, send_file};
use commands::config::{get_settings, save_settings, save_commands};
use commands::logging::{start_logging, stop_logging, is_logging};
use commands::sequence::{sequence_run, sequence_stop, save_sequence_config, load_sequence_config, save_sequence_auto, load_sequence_auto};

use state::AppState;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 查询 MCP server 运行状态:是否在跑 + 实际监听端口。
/// 供前端设置页显示实际连接指令。
#[tauri::command]
fn get_mcp_status(state: tauri::State<'_, AppState>) -> std::result::Result<McpStatusResp, String> {
    let port = state.mcp_port.lock().map_err(|e| e.to_string())?.clone();
    Ok(McpStatusResp {
        running: port.is_some(),
        port,
    })
}

#[derive(serde::Serialize)]
struct McpStatusResp {
    running: bool,
    port: Option<u16>,
}

/// 选端口 + 登记 registry + 起 MCP HTTP server + 心跳 task。
/// 从 AppState 提取共享字段给 MCP(必须是同一 Arc clone)。
/// 返回 RegistryHandle(供 connect/disconnect 工具调 update_connections)。
fn start_mcp(handle: &tauri::AppHandle, state: &AppState) -> Option<mcp::registry::RegistryHandle> {
    // 从设置读固定端口(默认 34594),不递增——被占则失败提示用户改端口
    let port = state.settings.lock().ok().map(|s| s.mcp.port).unwrap_or(23333);
    let (port, listener) = match mcp::server::bind_port(port) {
        Ok(p) => p,
        Err(e) => {
            log::error!("MCP 端口 {} bind 失败: {}", port, e);
            return None;
        }
    };
    // 记录端口供前端查询(get_mcp_status)
    if let Ok(mut mp) = state.mcp_port.lock() {
        *mp = Some(port);
    }
    let registry = mcp::registry::RegistryHandle::register(port).ok();
    // 从 AppState 提取共享字段给 MCP(必须是同一 Arc clone),registry 同一 clone 供工具调 update_connections
    let mcp_shared = std::sync::Arc::new(mcp::state::McpShared {
        app_handle: handle.clone(),
        connections: state.connections.clone(),
        registry: registry.clone(),
    });
    // 起 HTTP server(tauri::async_runtime::spawn,勿用 tokio::spawn)
    let shared_clone = mcp_shared.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = mcp::server::run_server(shared_clone, listener).await {
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
    registry
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let mut state = AppState::new(handle.clone());
            // 清理 registry 中前次/崩溃残留的死 pid 条目(无论是否起 MCP)
            mcp::registry::cleanup_dead();
            // 读 MCP 配置:是否启动时自动起 MCP server
            let auto_start = state.settings.lock().ok().map(|s| s.mcp.auto_start).unwrap_or(true);
            let registry = if auto_start {
                start_mcp(&handle, &state)
            } else {
                None
            };
            state.registry = registry;
            app.manage(state);
            #[cfg(debug_assertions)]
            app.get_webview_window("main").unwrap().open_devtools();
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                // 硬约束窗口最小尺寸：decorations:false 无边框窗口下，Windows 的 resize grip
                // 能让窗口外框突破 set_min_size/配置的 minWidth（约少 16px），导致右栏右侧被截断。
                // 这里在拖动 resize 结束时检测，低于阈值则强制拉回（逻辑像素）。
                tauri::WindowEvent::Resized(_) => {
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
                // 窗口关闭生命周期:副窗口(win-{port})关→断该 port 连接;main 关→断所有+注销 registry+退 app。
                // 清理在 close 前完成(优于 Destroyed 事后通知):disconnect 同步等线程退出(≤1s)。
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let label = window.label().to_string();
                    let app_handle = window.app_handle();
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        if let Some(port) = crate::connection::label_to_port(&label) {
                            // 副窗口:断该 port 连接后放行关闭。未连接(空窗口)则直接关。
                            let _ = crate::commands::connection::disconnect_port(&state, &app_handle, &port);
                        } else {
                            // main 窗口:拦截关闭,遍历断所有连接 + 注销 registry,再 app.exit(0)。
                            // 退 app 会触发各窗口关闭,这里 prevent 避免重复,由 exit 统一收尾。
                            api.prevent_close();
                            let ports: Vec<String> = {
                                let conns = state.connections.lock().map(|c| {
                                    c.keys().cloned().collect::<Vec<_>>()
                                }).unwrap_or_default();
                                conns
                            };
                            for p in &ports {
                                let _ = crate::commands::connection::disconnect_port(&state, &app_handle, p);
                            }
                            if let Some(reg) = &state.registry {
                                let _ = reg.unregister();
                            }
                            app_handle.exit(0);
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_mcp_status,
            connect,
            disconnect,
            list_ports,
            reset_stats,
            open_port_window,
            get_window_conn_state,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 进程退出时注销 registry,避免残留条目让 agent 发现后连接失败
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Some(reg) = &state.registry {
                        let _ = reg.unregister();
                    }
                }
            }
        });
}
