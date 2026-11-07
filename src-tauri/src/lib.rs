mod buffer;
mod commands;
mod dataproc;
mod config;
mod connection;
mod logging;
mod mcp;
mod state;
mod tray;
mod util;
mod window_sizing;
mod settings_open;
mod settings_diagnostics;

use tauri::{Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
use commands::connection::{connect, disconnect, list_ports, reset_stats, open_port_window, get_window_conn_state, get_window_history, get_mcp_only_connections, take_pending_takeover, open_theme_editor, get_modem_status};
use commands::send::{send, send_file};
use commands::config::{get_settings, save_settings, patch_settings, save_commands, export_theme_file, import_theme_file, data_dirs, open_data_dir};
use commands::logging::{start_logging, stop_logging, get_logging_status, export_log_snippet};
use commands::sequence::{sequence_run, sequence_stop, save_sequence_config, load_sequence_config, save_sequence_auto, load_sequence_auto};
use commands::command_index::{command_index_refresh, command_index_refresh_doc, command_index_load, command_index_test_connection, kb_credential_status, kb_set_api_key, send_history_push, send_history_load, send_history_clear};
use commands::mcp_log::{get_mcp_call_log, clear_mcp_call_log, get_mcp_activity};
use commands::data_processing::{build_frame, encode_gbk_texts};

use state::{AppState, SettingsOpenRequest};

const SETTINGS_WINDOW_LABEL: &str = "settings";
const SETTINGS_MIN_WIDTH: f64 = 680.0;
const SETTINGS_MIN_HEIGHT: f64 = 500.0;
static SETTINGS_OPEN_LOCK: std::sync::LazyLock<std::sync::Arc<tokio::sync::Mutex<()>>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Mutex::new(())));
static SETTINGS_OPEN_REPLIES: settings_open::SettingsOpenReplies = settings_open::SettingsOpenReplies::new();
static SETTINGS_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 打开设置独立窗口。固定 label 保证设置页只存在一个实例；窗口尚未加载时
/// 将跳转位置暂存，设置页挂载后主动取走，避免 IPC 事件竞态。
#[tauri::command]
async fn open_settings_window(
    app_handle: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    section: Option<String>,
    ext_module: Option<String>,
    anchor: Option<String>,
) -> Result<(), String> {
    let request_id = SETTINGS_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let request = SettingsOpenRequest {
        request_id: Some(request_id),
        section: section.unwrap_or_else(|| "about".to_string()),
        ext_module,
        anchor,
    };
    log::info!("[settings] request={} origin={} stage=queued", request_id, webview_window.label());
    settings_diagnostics::stage(webview_window.label(), Some(request_id), "queued", serde_json::json!({"section": request.section}));
    let result = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let guard = SETTINGS_OPEN_LOCK.clone().lock_owned().await;
        settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, Some(request_id), "lock_acquired", serde_json::json!({}));
        let reply = SETTINGS_OPEN_REPLIES.begin(request_id)?;
        let prepare_app = app_handle.clone();
        let prepare_source = webview_window.clone();
        let _guard = settings_open::run_preparation(guard, move || {
            prepare_settings_window(&prepare_app, &prepare_source, request)
        }).await?;
        settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, Some(request_id), "waiting_for_frontend", serde_json::json!({}));
        reply.await.map_err(|error| format!("设置窗口就绪确认中断: {}", error))?
    })
    .await
    .unwrap_or_else(|_| Err(format!("设置窗口未响应，打开超时，请重试（请求 {}）", request_id)));
    let _ = SETTINGS_OPEN_REPLIES.clear(request_id);
    match &result {
        Ok(settings_open::SettingsOpenOutcome::Ready) => log::info!("[settings] request={} stage=ready", request_id),
        Ok(settings_open::SettingsOpenOutcome::Cancelled) => log::info!("[settings] request={} stage=cancelled", request_id),
        Err(error) => log::warn!("[settings] request={} stage=failed error={}", request_id, error),
    }
    settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, Some(request_id), "finished", serde_json::json!({"result": format!("{:?}", result)}));
    if let Some(window) = app_handle.get_webview_window(SETTINGS_WINDOW_LABEL) {
        settings_diagnostics::snapshot(window, Some(request_id), "open_finished");
    }
    result.map(|_| ())
}

fn prepare_settings_window(
    app_handle: &tauri::AppHandle,
    webview_window: &tauri::WebviewWindow,
    request: SettingsOpenRequest,
) -> Result<(), String> {
    let state = app_handle
        .try_state::<AppState>()
        .ok_or("无法访问应用状态")?;
    *state.pending_settings.lock().map_err(|e| e.to_string())? = Some(request.clone());
    settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request.request_id, "preparing", serde_json::json!({}));
    if let Some(window) = app_handle.get_webview_window(SETTINGS_WINDOW_LABEL) {
        settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request.request_id, "reuse_found", serde_json::json!({}));
        if !window.is_visible().map_err(|e| e.to_string())? {
            if let (Ok(origin), Ok(parent_size), Ok(size)) = (
                webview_window.outer_position(),
                webview_window.outer_size(),
                window.outer_size(),
            ) {
                let position = PhysicalPosition::new(
                    origin.x + ((parent_size.width as i64 - size.width as i64) / 2) as i32,
                    origin.y + ((parent_size.height as i64 - size.height as i64) / 2) as i32,
                );
                window.set_position(position).map_err(|e| e.to_string())?;
            }
        }
        log::info!("[settings] request={:?} stage=restore", request.request_id);
        settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request.request_id, "restore_started", serde_json::json!({}));
        window.unminimize().map_err(|error| format!("恢复设置窗口失败: {}", error))?;
        window.show().map_err(|error| format!("显示设置窗口失败: {}", error))?;
        window.set_focus().map_err(|error| format!("聚焦设置窗口失败: {}", error))?;
        settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request.request_id, "native_restored", serde_json::json!({}));
        settings_diagnostics::snapshot(window.clone(), request.request_id, "reuse_restored");
        let request_id = request.request_id;
        window.emit("settings-open-request", request).map_err(|e| e.to_string())?;
        settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request_id, "request_emitted", serde_json::json!({}));
        return Ok(());
    }

    // 设置窗口首次打开时放在调用窗口正中间。这里使用物理像素计算，
    // 使用物理像素适配缩放；在隐藏状态下完成定位，避免默认位置的窗口先闪现。
    let centered_position = match (
        webview_window.outer_position(),
        webview_window.outer_size(),
        webview_window.scale_factor(),
    ) {
        (Ok(origin), Ok(size), Ok(scale)) => Some(PhysicalPosition::new(
            origin.x + ((size.width as f64 - SETTINGS_MIN_WIDTH * scale) / 2.0).round() as i32,
            origin.y + ((size.height as f64 - SETTINGS_MIN_HEIGHT * scale) / 2.0).round() as i32,
        )),
        _ => None,
    };

    log::info!("[settings] request={:?} stage=create", request.request_id);
    settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request.request_id, "create_started", serde_json::json!({}));
    let window = WebviewWindowBuilder::new(
        app_handle,
        SETTINGS_WINDOW_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("设置")
    .inner_size(SETTINGS_MIN_WIDTH, SETTINGS_MIN_HEIGHT)
    .min_inner_size(SETTINGS_MIN_WIDTH, SETTINGS_MIN_HEIGHT)
    .decorations(false)
    .resizable(true)
    .visible(false)
    .build()
    .map_err(|e| format!("创建设置窗口失败: {}", e))?;
    settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request.request_id, "created", serde_json::json!({}));
    if let Some(position) = centered_position {
        let _ = window.set_position(position);
    }
    settings_diagnostics::snapshot(window, request.request_id, "created_hidden");

    Ok(())
}

#[tauri::command]
fn complete_settings_open(
    app_handle: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    request_id: u64,
    error: Option<String>,
    cancelled: Option<bool>,
) -> Result<(), String> {
    if webview_window.label() != SETTINGS_WINDOW_LABEL {
        return Err("只能由设置窗口确认打开结果".to_string());
    }
    settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, Some(request_id), "ack_received", serde_json::json!({"cancelled": cancelled, "error": error}));
    let outcome = if cancelled.unwrap_or(false) {
        Ok(settings_open::SettingsOpenOutcome::Cancelled)
    } else {
        error.map_or(Ok(settings_open::SettingsOpenOutcome::Ready), Err)
    };
    let matched = SETTINGS_OPEN_REPLIES.complete(request_id, outcome)?;
    settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, Some(request_id), "ack_matched", serde_json::json!({"matched": matched}));
    if matched {
        if let Some(state) = app_handle.try_state::<AppState>() {
            let mut pending = state.pending_settings.lock().map_err(|error| error.to_string())?;
            if pending.as_ref().and_then(|request| request.request_id) == Some(request_id) {
                *pending = None;
            }
        }
    }
    if !matched {
        log::info!("[settings] request={} stage=stale-acknowledgement", request_id);
    }
    Ok(())
}

#[tauri::command]
fn show_settings_window(webview_window: tauri::WebviewWindow) -> Result<(), String> {
    if webview_window.label() != SETTINGS_WINDOW_LABEL {
        return Err("只能由设置窗口请求显示".to_string());
    }
    let request_id = webview_window.app_handle().try_state::<AppState>().and_then(|state| {
        state.pending_settings.lock().ok().and_then(|pending| pending.as_ref().and_then(|request| request.request_id))
    });
    settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request_id, "show_started", serde_json::json!({}));
    webview_window.unminimize().map_err(|e| e.to_string())?;
    webview_window.show().map_err(|e| e.to_string())?;
    webview_window.set_focus().map_err(|e| e.to_string())?;
    settings_diagnostics::stage(SETTINGS_WINDOW_LABEL, request_id, "show_returned", serde_json::json!({}));
    settings_diagnostics::snapshot(webview_window, request_id, "show_returned");
    Ok(())
}

/// 设置窗口读取跳转请求，确认完成后才清除，允许 IPC 失败后重试。
#[tauri::command]
fn take_pending_settings(
    state: tauri::State<'_, AppState>,
) -> Result<Option<SettingsOpenRequest>, String> {
    state
        .pending_settings
        .lock()
        .map_err(|e| e.to_string())
        .map(|request| request.clone())
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
    // 客户端的固定 URL 只命中配置的端口，接入页始终展示实际端口。
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
    let mcp_shared = std::sync::Arc::new(mcp::state::McpShared::new(
        handle.clone(),
        state.connections.clone(),
        state.connecting.clone(),
        registry.clone(),
        state.call_log.clone(),
    ));
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

/// 断开所有连接 + 写完文件日志 + 注销 registry + 退出应用。
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
    // 断开之后再停日志:reader 退出前最后一次 flush 的行还在存盘通道里,stop 会等它们写进文件。
    // Tauri 的 exit 走 process::exit,托管状态不析构,不在这里停的话 BufWriter 里的尾巴就丢了。
    let _ = crate::commands::logging::stop_logging_impl(app_handle);
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
/// 供 main.rs 在创建窗口前查 WebView2 数据目录(便携/自定义目录模式下跟着数据走)。
/// config 模块是私有的,这里开一个口子。
pub fn webview2_data_dir() -> Option<std::path::PathBuf> {
    config::webview2_dir()
}

/// 同上:dev 构建给 WebView2 单独指一个目录,免得与已安装的 release 抢同一 profile。
pub fn webview_default_parent() -> Option<std::path::PathBuf> {
    config::webview_default_parent()
}

pub fn run() {
    // 目录解析必须最先:之后所有读写(设置、快捷指令、缓存、凭据)都基于它。
    // 紧接着跑一次性迁移(缓存挪到 Local、明文 Key 挪进凭据文件),再让 AppState 去 load 设置。
    config::init_dirs();
    config::migrate::run_once();
    tauri::Builder::default()
        .append_invoke_initialization_script(include_str!("logging/bridge-bootstrap.js"))
        // 单实例必须最先注册。应用在托盘里跑着(或窗口都关了)时用户再双击图标"打开软件",
        // 若起了第二个进程:MCP 落到 34595、第一个进程还占着 COM 口,第二个里连同一口报
        // "被占用"。二次启动只把已有实例的窗口拉出来(没有窗口就新建一个),然后自行退出。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::focus_any_or_create(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_page_load(|webview, payload| {
            logging::webview::install(webview);
            let url = payload.url();
            let own_frontend = url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost")
                || webview.app_handle().config().build.dev_url.as_ref().is_some_and(|dev| url.origin() == dev.origin());
            logging::runtime::page_load(webview.label(), &format!("{:?}", payload.event()), own_frontend);
            logging::frontend::native_event(webview.label().to_string(), "webview.page_load", serde_json::json!({
                "stage": format!("{:?}", payload.event()),
            }));
        })
        .setup(|app| {
            let handle = app.handle().clone();
            logging::runtime::start(&handle);
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
            if let Some(window) = app.get_webview_window("main") {
                window_sizing::install(&window)?;
            }
            #[cfg(debug_assertions)]
            app.get_webview_window("main").unwrap().open_devtools();
            Ok(())
        })
        .on_window_event(|window, event| {
            logging::runtime::window_event(window.label(), event);
            match event {
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
                tauri::WindowEvent::Destroyed => {
                    logging::webview::closed(window.label());
                    logging::runtime::closed(window.label());
                    logging::frontend::native_event(window.label().to_string(), "webview.window_destroyed", serde_json::json!({}));
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        let _ = commands::auto_connect::cancel_window(&state, window.app_handle(), window.label());
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            open_url,
            get_mcp_status,
            mcp::diagnostics::check_mcp_connection,
            connect,
            disconnect,
            commands::auto_connect::get_auto_connect,
            commands::auto_connect::set_auto_connect,
            commands::auto_connect::auto_connect_once,
            list_ports,
            reset_stats,
            open_port_window,
            open_theme_editor,
            get_window_conn_state,
            get_window_history,
            get_mcp_only_connections,
            get_modem_status,
            take_pending_takeover,
            open_settings_window,
            complete_settings_open,
            show_settings_window,
            take_pending_settings,
            exit_app,
            resolve_last_close,
            send,
            send_file,
            get_settings,
            save_settings,
            patch_settings,
            data_dirs,
            open_data_dir,
            save_commands,
            start_logging,
            stop_logging,
            get_logging_status,
            export_log_snippet,
            logging::frontend::append_frontend_diagnostics,
            logging::frontend::export_frontend_diagnostics,
            logging::replay::append_frontend_replay,
            logging::runtime::frontend_diagnostics_heartbeat,
            sequence_run,
            sequence_stop,
            save_sequence_config,
            load_sequence_config,
            save_sequence_auto,
            load_sequence_auto,
            export_theme_file,
            import_theme_file,
            command_index_refresh,
            command_index_refresh_doc,
            command_index_load,
            command_index_test_connection,
            kb_credential_status,
            kb_set_api_key,
            send_history_push,
            send_history_load,
            send_history_clear,
            build_frame,
            encode_gbk_texts,
            get_mcp_call_log,
            get_mcp_activity,
            clear_mcp_call_log,

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
            // 进程退出时注销 registry,避免残留条目让 agent 发现后连接失败;
            // 没经 cleanup_and_exit 的退出路径(更新器 relaunch 等)也把文件日志写完。
            tauri::RunEvent::Exit => {
                let _ = crate::commands::logging::stop_logging_impl(app);
                if let Some(state) = app.try_state::<AppState>() {
                    if let Some(reg) = &state.registry {
                        let _ = reg.unregister();
                    }
                }
            }
            _ => {}
        });
}
