//! 系统托盘:主窗口 × 只是收进托盘,应用(串口连接 + MCP 服务)常驻,
//! 只有托盘菜单/设置页的"退出"才真正退出。

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;

const APP_NAME: &str = "NeoSerial";
const TRAY_ID: &str = "main-tray";

/// 建托盘:左键点图标显示主窗口,右键菜单"显示窗口"/"退出";
/// 监听连接集合变化刷新 tooltip(列出仍被占用的端口)。
pub fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "tray-show", "显示窗口", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(format_tray_tooltip(&[]))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-show" => show_main_window(app),
            "tray-quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    crate::cleanup_and_exit(&state, app);
                }
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    // 所有连接增删路径(GUI/MCP 建连、断开、接管回退、拔线清理)都 emit 这个全局事件,
    // Rust 侧监听它刷新 tooltip,不必在每处调用点手工挂钩。
    // set_tooltip 内部经 run_on_main_thread 派发,任意线程调用都安全。
    let handle = app.handle().clone();
    app.listen("mcp-connections-changed", move |_| refresh_tooltip(&handle));
    Ok(())
}

/// 显示并聚焦主窗口(托盘点击/菜单、二次启动都走这里)。
/// unminimize:窗口收托盘前若处于最小化,单 show 会以最小化态复现,看着像没反应。
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 主窗口 × 的默认路径:隐藏到托盘。首次(且仅首次)发系统通知说明应用仍在后台、
/// 哪些端口仍被占、如何退出。标记落盘,后续整份回写由 merge_backend_owned 保住。
pub fn hide_main_to_tray(app: &AppHandle, state: &AppState) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
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

fn refresh_tooltip(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let ports = connected_ports(&state);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(format_tray_tooltip(&ports)));
    }
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
}
