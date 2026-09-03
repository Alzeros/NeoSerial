//! 系统托盘 + 后台运行模式。
//!
//! 后台模式(settings.ui.background_mode)下应用常驻:关窗口连接不断、关最后一个窗口
//! 应用留在托盘,只有托盘菜单/设置页的"退出"才真正退出;轻量模式下没有托盘,
//! 关最后一个窗口即退出。窗口一律平等,没有"主窗口"。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::commands::connection::open_port_window;
use crate::config::settings::default_background_mode;
use crate::connection::is_detached_label;
use crate::state::AppState;

const APP_NAME: &str = "NeoSerial";
const TRAY_ID: &str = "main-tray";

/// setup 阶段调用:监听连接集合变化刷新托盘(tooltip + 菜单),再按当前设置决定托盘显隐。
/// 所有连接增删路径(GUI/MCP 建连、断开、关窗口、拔线清理)都 emit 这个全局事件,
/// Rust 侧监听它,不必在每处调用点手工挂钩。托盘操作内部经 run_on_main_thread 派发,
/// 任意线程调用都安全。
pub fn setup(app: &tauri::App) {
    let handle = app.handle().clone();
    app.listen("mcp-connections-changed", move |_| refresh(&handle));
    sync_visibility(app.handle());
}

/// 按 background_mode 让托盘图标出现/消失。开关保存时调用,即时生效不用重启。
/// 托盘懒建:轻量模式启动时根本不建,避免图标闪一下。
pub fn sync_visibility(app: &AppHandle) {
    let background = background_mode(app);
    if background {
        if let Some(tray) = ensure_tray(app) {
            let _ = tray.set_visible(true);
            refresh(app);
        }
    } else if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
    }
}

pub fn background_mode(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.ui.background_mode))
        .unwrap_or_else(default_background_mode)
}

/// 串口窗口 label:启动时的 `main` 与 `+` 号开的 `win-N`。主题编辑器等辅助窗口不算。
pub fn is_serial_window_label(label: &str) -> bool {
    label == "main" || label.starts_with("win-")
}

fn ensure_tray(app: &AppHandle) -> Option<TrayIcon> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        return Some(tray);
    }
    let icon = app.default_window_icon()?.clone();
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .show_menu_on_left_click(false)
        .tooltip(format_tray_tooltip(&[]))
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if let Some(port) = port_from_menu_id(id) {
                activate_port(app, port);
                return;
            }
            match id {
                "tray-new" => open_blank_window(app),
                "tray-quit" => {
                    if let Some(state) = app.try_state::<AppState>() {
                        crate::cleanup_and_exit(&state, app);
                    }
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                focus_any_or_create(tray.app_handle());
            }
        })
        .build(app)
        .ok()
}

/// 重建 tooltip 与菜单。菜单整份重建而不是增删项:连接集合小、变化少,重建最简单可靠。
fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let conns = tray_conns(app);
    let ports: Vec<String> = conns.iter().map(|c| c.port.clone()).collect();
    let _ = tray.set_tooltip(Some(format_tray_tooltip(&ports)));
    if let Ok(menu) = build_menu(app, &menu_entries(&conns)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn build_menu(app: &AppHandle, entries: &[MenuEntry]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(app, "tray-new", "新建窗口", true, None::<&str>)?)?;
    if !entries.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        for e in entries {
            menu.append(&MenuItem::with_id(app, e.id.as_str(), e.text.as_str(), true, None::<&str>)?)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>)?)?;
    Ok(menu)
}

/// 锁内取连接快照,锁外用(建菜单会派发到主线程,不能持锁)。
fn tray_conns(app: &AppHandle) -> Vec<TrayConn> {
    let Some(state) = app.try_state::<AppState>() else { return Vec::new() };
    state
        .connections
        .lock()
        .map(|c| {
            c.values()
                .map(|h| TrayConn {
                    port: h.port.clone(),
                    baud: h.baud,
                    window_label: h.window_label.read().map(|l| l.clone()).unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 点了某端口的菜单项:按**当前**连接表决定——无窗口的开新窗口挂上,有窗口的把窗口拉前台。
/// 菜单建好后连接可能已变,查不到就什么都不做(下一次事件会重建菜单)。
fn activate_port(app: &AppHandle, port: &str) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let found = state.connections.lock().ok().and_then(|c| {
        c.get(port).map(|h| (h.baud, h.window_label.read().map(|l| l.clone()).unwrap_or_default()))
    });
    let Some((baud, label)) = found else { return };
    if is_detached_label(&label) {
        let app = app.clone();
        let port = port.to_string();
        tauri::async_runtime::spawn(async move {
            let _ = open_port_window(app, Some(port), Some(baud)).await;
        });
    } else if let Some(window) = app.get_webview_window(&label) {
        bring_to_front(&window);
    } else {
        focus_any_or_create(app);
    }
}

fn open_blank_window(app: &AppHandle) {
    // 建窗口不能在主线程的事件回调里同步做(Windows 下死锁),和 open_port_window 必须是
    // async command 的原因一样,丢到 async runtime 执行。
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = open_port_window(app, None, None).await;
    });
}

/// 托盘左键 / 二次启动:有串口窗口就把一个拉到前台(`main` 优先,其余按 label 序),
/// 一个都没有就新建一个空白窗口。
pub fn focus_any_or_create(app: &AppHandle) {
    let mut labels: Vec<String> = app
        .webview_windows()
        .keys()
        .filter(|l| is_serial_window_label(l))
        .cloned()
        .collect();
    labels.sort();
    let pick = labels.iter().find(|l| l.as_str() == "main").or_else(|| labels.first());
    match pick.and_then(|l| app.get_webview_window(l)) {
        Some(window) => bring_to_front(&window),
        None => open_blank_window(app),
    }
}

/// unminimize:窗口若处于最小化,单 show/set_focus 会以最小化态"显示",看着像没反应。
fn bring_to_front(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// 后台模式下最后一个窗口关掉时:首次(且仅首次)发系统通知说明应用仍在后台、
/// 哪些端口仍被占、如何退出。标记落盘,后续整份回写由 merge_backend_owned 保住。
pub fn maybe_send_tray_hint(app: &AppHandle, state: &AppState) {
    let first_time = state
        .settings
        .lock()
        .map(|mut s| {
            let first = s.mark_tray_hint_shown();
            if first {
                let _ = s.save();
            }
            first
        })
        .unwrap_or(false);
    if first_time {
        let _ = app
            .notification()
            .builder()
            .title("NeoSerial 仍在后台运行")
            .body(format_tray_hint_body(&connected_ports(state)))
            .show();
    }
}

fn connected_ports(state: &AppState) -> Vec<String> {
    state
        .connections
        .lock()
        .map(|c| c.keys().cloned().collect())
        .unwrap_or_default()
}

/// 托盘悬停提示:无连接只显应用名,有连接列出端口(按 COM 序号自然排序),
/// 鼠标悬上去就知道谁还占着口。
pub fn format_tray_tooltip(ports: &[String]) -> String {
    if ports.is_empty() {
        return APP_NAME.to_string();
    }
    format!("{} · {}", APP_NAME, join_ports(ports, ", "))
}

/// 首次收进托盘时的系统通知正文。串口工具特有的坑是"窗口关了、COM 口还被占着,
/// 别的工具打不开",有连接时点名端口。
pub fn format_tray_hint_body(ports: &[String]) -> String {
    if ports.is_empty() {
        return "MCP 服务保持运行;右键托盘图标可退出应用。".to_string();
    }
    format!(
        "{} 仍保持连接(其他工具打不开该端口),MCP 服务保持运行;右键托盘图标可退出应用。",
        join_ports(ports, "、")
    )
}

/// 建菜单用的连接快照(锁内取,锁外建菜单)。
pub struct TrayConn {
    pub port: String,
    pub baud: u32,
    pub window_label: String,
}

/// 一条端口菜单项:id 带端口名,点击时按端口名重查连接表决定动作。
#[derive(Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: String,
    pub text: String,
}

const PORT_MENU_ID_PREFIX: &str = "tray-port:";

/// 连接 → 菜单项:无窗口的"打开"(开新窗口挂上),有窗口的"切到"(拉到前台);按 COM 号自然排序。
pub fn menu_entries(conns: &[TrayConn]) -> Vec<MenuEntry> {
    let mut sorted: Vec<&TrayConn> = conns.iter().collect();
    sorted.sort_by_key(|c| port_sort_key(&c.port));
    sorted
        .into_iter()
        .map(|c| {
            let verb = if is_detached_label(&c.window_label) { "打开" } else { "切到" };
            MenuEntry {
                id: format!("{}{}", PORT_MENU_ID_PREFIX, c.port),
                text: format!("{} {} · {}", verb, c.port, c.baud),
            }
        })
        .collect()
}

pub fn port_from_menu_id(id: &str) -> Option<&str> {
    id.strip_prefix(PORT_MENU_ID_PREFIX).filter(|p| !p.is_empty())
}

/// 关窗口时对该窗口所持连接的处理。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ConnPolicy {
    /// 后台模式:一律不断,label 回到无窗口态,留给托盘/红点重新打开。
    DetachAll,
    /// 轻量模式:窗口自己连的断开释放,agent 建的交还 agent(改回无窗口 label)。
    ReleaseGuiKeepAgent,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ClosePlan {
    pub connections: ConnPolicy,
    /// 关的是最后一个串口窗口且为轻量模式 → 退出应用。
    pub exit_app: bool,
    /// 后台模式下关掉最后一个窗口 → 该发"仍在后台运行"提示(是否真发还看 tray_hint_shown)。
    pub tray_hint: bool,
}

pub fn close_plan(background_mode: bool, is_last_window: bool) -> ClosePlan {
    if background_mode {
        ClosePlan {
            connections: ConnPolicy::DetachAll,
            exit_app: false,
            tray_hint: is_last_window,
        }
    } else {
        ClosePlan {
            connections: ConnPolicy::ReleaseGuiKeepAgent,
            exit_app: is_last_window,
            tray_hint: false,
        }
    }
}

/// 端口名按"字母前缀 + 末尾数字"排序:COM5 < COM7 < COM13,而非字典序的 COM13 < COM5。
fn join_ports(ports: &[String], sep: &str) -> String {
    let mut sorted: Vec<&String> = ports.iter().collect();
    sorted.sort_by_key(|p| port_sort_key(p));
    sorted.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(sep)
}

fn port_sort_key(port: &str) -> (String, u32) {
    let digits_start = port
        .rfind(|c: char| !c.is_ascii_digit())
        .map(|i| i + 1)
        .unwrap_or(0);
    let (prefix, num) = port.split_at(digits_start);
    (prefix.to_string(), num.parse().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ports(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tooltip_without_connections_is_app_name() {
        assert_eq!(format_tray_tooltip(&[]), "NeoSerial");
    }

    #[test]
    fn tooltip_lists_ports_in_natural_order() {
        assert_eq!(
            format_tray_tooltip(&ports(&["COM13", "COM5", "COM7"])),
            "NeoSerial · COM5, COM7, COM13"
        );
    }

    #[test]
    fn hint_body_without_connections_mentions_mcp_and_exit() {
        let body = format_tray_hint_body(&[]);
        assert!(body.contains("MCP 服务保持运行"));
        assert!(body.contains("右键托盘图标可退出"));
        assert!(!body.contains("COM"));
    }

    #[test]
    fn hint_body_names_connected_ports() {
        let body = format_tray_hint_body(&ports(&["COM13", "COM5"]));
        assert!(body.starts_with("COM5、COM13 仍保持连接"), "实际: {}", body);
        assert!(body.contains("右键托盘图标可退出"));
    }

    // ============ 托盘菜单条目 ============

    fn conn(port: &str, baud: u32, label: &str) -> TrayConn {
        TrayConn { port: port.into(), baud, window_label: label.into() }
    }

    #[test]
    fn menu_entries_empty_without_connections() {
        assert!(menu_entries(&[]).is_empty());
    }

    /// 无窗口的连接 → "打开",有窗口的 → "切到";id 带端口名;按 COM 号自然排序。
    #[test]
    fn menu_entries_distinguish_detached_and_attached_in_natural_order() {
        let entries = menu_entries(&[
            conn("COM13", 9600, "win-2"),
            conn("COM5", 115200, "mcp-COM5"),
            conn("COM7", 921600, "main"),
        ]);
        let texts: Vec<&str> = entries.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["打开 COM5 · 115200", "切到 COM7 · 921600", "切到 COM13 · 9600"]);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["tray-port:COM5", "tray-port:COM7", "tray-port:COM13"]);
    }

    #[test]
    fn port_menu_id_roundtrip() {
        assert_eq!(port_from_menu_id("tray-port:COM5"), Some("COM5"));
        assert_eq!(port_from_menu_id("tray-quit"), None);
        assert_eq!(port_from_menu_id("tray-port:"), None);
    }

    // ============ 关窗口策略 ============

    /// 后台模式:任何窗口关闭连接都留在后台;关最后一个窗口应用不退,且该提示一次。
    #[test]
    fn close_plan_background_mode_detaches_and_never_exits() {
        let not_last = close_plan(true, false);
        assert_eq!(not_last.connections, ConnPolicy::DetachAll);
        assert!(!not_last.exit_app);
        assert!(!not_last.tray_hint);
        let last = close_plan(true, true);
        assert_eq!(last.connections, ConnPolicy::DetachAll);
        assert!(!last.exit_app);
        assert!(last.tray_hint);
    }

    /// 轻量模式:窗口自己连的断开、agent 的交还;关最后一个窗口退出应用,没有托盘提示。
    #[test]
    fn close_plan_classic_mode_releases_and_exits_on_last_window() {
        let not_last = close_plan(false, false);
        assert_eq!(not_last.connections, ConnPolicy::ReleaseGuiKeepAgent);
        assert!(!not_last.exit_app);
        assert!(!not_last.tray_hint);
        let last = close_plan(false, true);
        assert!(last.exit_app);
        assert!(!last.tray_hint);
    }
}
