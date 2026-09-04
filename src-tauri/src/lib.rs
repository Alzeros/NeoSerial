mod buffer;
mod commands;
mod config;
mod connection;
mod logging;
mod mcp;
mod state;
mod tray;
mod util;

use tauri::{Emitter, Manager};
use commands::connection::{connect, disconnect, list_ports, reset_stats, open_port_window, get_window_conn_state, get_window_history, get_mcp_only_connections, take_pending_takeover, open_theme_editor};
use commands::send::{send, send_file};
use commands::config::{get_settings, save_settings, save_commands, export_theme_file, import_theme_file};
use commands::logging::{start_logging, stop_logging, is_logging};
use commands::sequence::{sequence_run, sequence_stop, save_sequence_config, load_sequence_config, save_sequence_auto, load_sequence_auto};
use commands::command_index::{command_index_refresh, command_index_load, send_history_push, send_history_load, send_history_clear};

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
    // 从设置读首选端口(默认 34594)。被占时 bind_port 向上递增最多 20 个找空闲
    // (单实例后只剩"别的程序占了端口"这种情况),实际端口写进 registry,
    // agent 经 registry(或从 34594 起逐端口探测)找到实际端口。
    // 固定 URL 配置(claude mcp add http://localhost:34594/mcp)只命中绑到首选端口的情况。
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
/// 被 CloseRequested(设置"点 × 直接退出"路径)、exit_app 命令(设置页"退出")、
/// 托盘"退出"菜单共用——这是应用真正退出的唯一出口。
pub(crate) fn cleanup_and_exit(state: &AppState, app_handle: &tauri::AppHandle) {
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

/// 应用内的"退出应用"入口(设置页按钮)。Windows 11 默认把托盘图标收进溢出区,
/// 只靠托盘右键退出太隐蔽,应用内必须有一个可见出口。
#[tauri::command]
fn exit_app(state: tauri::State<'_, AppState>, app_handle: tauri::AppHandle) {
    cleanup_and_exit(&state, &app_handle);
}

/// 轻量模式下关最后一个窗口撞上 agent 连接时,前端确认框的回调(取消不调这里)。
/// - exit:仍然退出,断开全部、停 MCP。
/// - background:开启后台运行(落盘、托盘出现、广播 settings-changed),再关一次窗口——
///   这回 CloseRequested 按后台规则放行,连接留给 agent,应用留在托盘。
///
/// async:window.close() 会触发 CloseRequested 回调,在主线程的同步 command 里直接调
/// 等于在 IPC 处理中重入窗口事件处理器;放到 async 线程经事件循环派发最稳。
#[tauri::command]
async fn resolve_last_close(
    action: String,
    webview_window: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
) -> std::result::Result<(), String> {
    let state = app_handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
    match action.as_str() {
        "exit" => cleanup_and_exit(&state, &app_handle),
        "background" => {
            {
                let mut s = state.settings.lock().map_err(|e| e.to_string())?;
                s.ui.background_mode = true;
                s.save().map_err(|e| e.to_string())?;
            }
            tray::sync_visibility(&app_handle);
            let _ = app_handle.emit("settings-changed", ());
            webview_window.close().map_err(|e| e.to_string())?;
        }
        other => return Err(format!("未知的关闭选择: {}", other)),
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例必须最先注册。应用在托盘里跑着(或窗口都关了)时用户再双击图标"打开软件",
        // 若起了第二个进程:MCP 落到 34595、第一个进程还占着 COM 口,第二个里连同一口报
        // "被占用"。二次启动只把已有实例的窗口拉出来(没有窗口就新建一个),然后自行退出。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::focus_any_or_create(app);
        }))
        .plugin(tauri_plugin_notification::init())
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
            // 指令联想:配了知识库地址和 Key 且开了自动刷新 → 后台刷新一次缓存(进程级一次,不按窗口)
            let cidx_cfg = state.settings.lock().ok().map(|s| s.command_index.clone());
            app.manage(state);
            if let Some(cfg) = cidx_cfg {
                commands::command_index::spawn_auto_refresh(&handle, &cfg);
            }
            tray::setup(app);
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
                // 窗口关闭生命周期:所有串口窗口(main / win-*)一个规则,由 tray::close_plan 决定——
                // 后台模式:连接一律留在后台,窗口放行销毁;关的是最后一个窗口则应用留在托盘
                //   (Tauri 随后发 ExitRequested(code=None),run 回调里拦下),并首次提示一次。
                // 轻量模式:该窗口自己连的断开、agent 的交还;关的是最后一个窗口则退出应用——
                //   但若 agent 建的连接还活着,退出会掐断它,先拦下让前端弹确认(见 close-guard),
                //   用户经 resolve_last_close 选退出 / 开后台运行 / 取消。
                // 关非最后一个窗口不会伤到别的窗口或 agent,不弹任何确认。
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let label = window.label().to_string();
                    if !tray::is_serial_window_label(&label) {
                        return;
                    }
                    let app_handle = window.app_handle();
                    let Some(state) = app_handle.try_state::<AppState>() else { return };
                    let others = app_handle
                        .webview_windows()
                        .keys()
                        .filter(|l| l.as_str() != label && tray::is_serial_window_label(l))
                        .count();
                    let agent_ports = crate::commands::connection::agent_connected_ports(&state);
                    let plan = tray::close_plan(
                        tray::background_mode(&app_handle),
                        others == 0,
                        !agent_ports.is_empty(),
                    );
                    if plan.confirm_exit {
                        api.prevent_close();
                        let _ = app_handle.emit_to(&label, "close-guard", tray::CloseGuard { ports: agent_ports });
                        return;
                    }
                    crate::commands::connection::release_window_connections(
                        &state, &app_handle, &label, plan.connections,
                    );
                    if plan.exit_app {
                        api.prevent_close();
                        cleanup_and_exit(&state, &app_handle);
                        return;
                    }
                    if plan.tray_hint {
                        tray::maybe_send_tray_hint(&app_handle, &state);
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
            get_window_history,
            get_mcp_only_connections,
            take_pending_takeover,
            exit_app,
            resolve_last_close,
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
            command_index_refresh,
            command_index_load,
            send_history_push,
            send_history_load,
            send_history_clear,

        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // 最后一个窗口关掉时 Tauri 请求退出(code=None);后台模式下拦下,应用留在托盘。
            // 显式 app.exit(code)(托盘"退出"/设置页/轻量模式关最后一个窗口)带 code,照常退出。
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                if code.is_none() && tray::background_mode(app) {
                    api.prevent_exit();
                }
            }
            // 进程退出时注销 registry,避免残留条目让 agent 发现后连接失败
            tauri::RunEvent::Exit => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Some(reg) = &state.registry {
                        let _ = reg.unregister();
                    }
                }
            }
            _ => {}
        });
}
