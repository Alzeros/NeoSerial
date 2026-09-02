mod buffer;
mod commands;
mod config;
mod connection;
mod logging;
mod mcp;
mod state;
mod util;

use tauri::{Emitter, Manager};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use commands::connection::{connect, disconnect, list_ports, reset_stats, open_port_window, get_window_conn_state, has_active_sessions, get_mcp_only_connections, take_pending_takeover, open_theme_editor};
use commands::send::{send, send_file};
use commands::config::{get_settings, save_settings, save_commands, export_theme_file, import_theme_file};
use commands::logging::{start_logging, stop_logging, is_logging};
use commands::sequence::{sequence_run, sequence_stop, save_sequence_config, load_sequence_config, save_sequence_auto, load_sequence_auto};

use state::AppState;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 用系统默认浏览器打开 URL(关于页 GitHub 链接)。
/// 不用 shell plugin scope:直接 spawn 系统 opener(Windows:cmd /c start)。
#[tauri::command]
fn open_url(url: String) -> std::result::Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", "start", "", &url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    };
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };
    cmd.spawn().map_err(|e| format!("打开链接失败: {}", e))?;
    Ok(())
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
    // 从设置读首选端口(默认 34594)。被占时 bind_port 向上递增最多 20 个找空闲——
    // 这是多实例设计的一部分:第二个实例必然撞首选端口,递增绑定后把实际端口写进
    // registry,agent 经 registry(或从 34594 起逐端口探测)找到实际端口。
    // 固定 URL 配置(claude mcp add http://localhost:34594/mcp)只命中绑到首选端口的实例。
    let port = state.settings.lock().ok().map(|s| s.mcp.port).unwrap_or(34594);
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
        connecting: state.connecting.clone(),
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

/// 断开所有连接 + 注销 registry + 退出应用。
/// 被 CloseRequested（设置关闭=直接退出路径）、resolve_close（用户选退出）、
/// 托盘"退出"菜单共用。
fn cleanup_and_exit(state: &AppState, app_handle: &tauri::AppHandle) {
    let ports: Vec<String> = {
        let conns = state.connections.lock().map(|c| {
            c.keys().cloned().collect::<Vec<_>>()
        }).unwrap_or_default();
        conns
    };
    for p in &ports {
        let _ = crate::commands::connection::disconnect_port(state, app_handle, p);
    }
    if let Some(reg) = &state.registry {
        let _ = reg.unregister();
    }
    app_handle.exit(0);
}

/// 前端关闭确认弹窗的回调：用户选了"最小化到托盘"或"退出应用"。
/// dont_remind=true 时把选择写入设置（close_prompted=true），下次不再弹窗。
#[tauri::command]
fn resolve_close(
    minimize: bool,
    dont_remind: bool,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> std::result::Result<(), String> {
    if dont_remind {
        if let Ok(mut s) = state.settings.lock() {
            s.ui.close_prompted = true;
            s.ui.minimize_to_tray = minimize;
            let _ = s.save();
        }
    }
    if minimize {
        if let Some(window) = app_handle.get_webview_window("main") {
            let _ = window.hide();
        }
    } else {
        cleanup_and_exit(&state, &app_handle);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            // 系统托盘：左键点击显示主窗口，右键菜单"显示窗口"/"退出"
            let show_item = MenuItem::with_id(app, "tray-show", "显示窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .tooltip("NeoSerial")
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "tray-show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "tray-quit" => {
                            if let Some(state) = app.try_state::<AppState>() {
                                cleanup_and_exit(&state, app);
                            }
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    // 左键点击：显示主窗口（只显示，不切换隐藏）
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
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
                    // 只对串口窗口(main / win-*)生效。主题编辑器窗口有自己的最小尺寸
                    // (720x480,见 open_theme_editor),不能在这里被统一拉到 1216x500:
                    //   1) 拖动中 Resized -> set_size -> Resized 反复触发,窗口尺寸抖动闪烁;
                    //   2) set_size 同时写宽和高,缩宽度会把高度顶到 500,缩高度会把宽度顶到 1216。
                    if window.label() != "theme-editor" {
                        // 目标内容区 1200，但无边框窗口下 Windows resize grip 会扣约 16px，
                        // 故阈值取 1216：被扣后落地 1200，右栏不再被截断。
                        let min_w = 1216.0;
                        let min_h = 600.0;
                        if let Ok(size) = window.outer_size() {
                            let scale = window.scale_factor().unwrap_or(1.0);
                            let w = size.width as f64 / scale;
                            let h = size.height as f64 / scale;
                            if w < min_w || h < min_h {
                                let _ = window.set_size(tauri::LogicalSize::new(w.max(min_w), h.max(min_h)));
                            }
                        }
                    }
                }
                // 窗口关闭生命周期:
                // main 关 → 按 minimize_to_tray 设置决定隐藏到托盘还是退出;
                // 副窗口(win-*)关 → 回退本窗口接管的连接(改回 mcp label,不断开)后放行。
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let label = window.label().to_string();
                    let app_handle = window.app_handle();
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        if label == "main" {
                            api.prevent_close();
                            let (minimize_to_tray, close_prompted) = state.settings.lock()
                                .map(|s| (s.ui.minimize_to_tray, s.ui.close_prompted))
                                .unwrap_or((true, false));
                            if !minimize_to_tray {
                                // 设置关闭=直接退出
                                cleanup_and_exit(&state, &app_handle);
                            } else if close_prompted {
                                // 已弹过提示=直接隐藏到托盘
                                if let Some(window) = app_handle.get_webview_window("main") {
                                    let _ = window.hide();
                                }
                            } else {
                                // 首次：emit 给前端弹确认对话框
                                let _ = app_handle.emit_to("main", "close-requested", ());
                            }
                        } else {
                            // 副窗口(win-{自增}):回退本窗口接管的连接(改回 mcp label,
                            // 连接不断),让 + 号红点恢复。未接管任何连接时为空操作。
                            let _ = crate::commands::connection::release_takeover(&state, &app_handle, &label);
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            open_url,
            get_mcp_status,
            connect,
            disconnect,
            list_ports,
            reset_stats,
            open_port_window,
            open_theme_editor,
            get_window_conn_state,
            has_active_sessions,
            get_mcp_only_connections,
            take_pending_takeover,
            resolve_close,
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
            export_theme_file,
            import_theme_file,

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
