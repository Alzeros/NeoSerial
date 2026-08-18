use std::collections::HashMap;

use tauri::{State, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::connection::{ConnectionHandle, SerialParams, spawn_connection, WriteCommand, label_to_port, port_to_label};
use crate::state::AppState;

/// 解析目标端口:供 Tauri command / MCP 工具从 `connections` 取出唯一连接或校验指定端口。
///
/// - `None` + 0 连接:报错(未连接)
/// - `None` + 1 连接:返回该 port(保持单连接行为,前端不传 port 时取唯一)
/// - `None` + 2+ 连接:报"多个连接,请指定端口"
/// - `Some(p)` 存在:返回 p
/// - `Some(p)` 不存在:报"端口 X 未连接"
pub fn resolve_port(
    conns: &HashMap<String, ConnectionHandle>,
    port: Option<String>,
) -> Result<String, String> {
    match port {
        Some(p) => {
            if conns.contains_key(&p) {
                Ok(p)
            } else {
                Err(format!("端口 {} 未连接", p))
            }
        }
        None => {
            if conns.is_empty() {
                Err("未连接串口".to_string())
            } else if conns.len() == 1 {
                Ok(conns.keys().next().unwrap().clone())
            } else {
                Err("多个连接,请指定端口".to_string())
            }
        }
    }
}

#[tauri::command]
pub fn connect(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    port: String,
    baudRate: u32,
    dataBits: String,
    parity: String,
    stopBits: u8,
    flowControl: String,
) -> Result<(), String> {
    let data_bits = match dataBits.as_str() {
        "Five" => crate::config::settings::DataBits::Five,
        "Six" => crate::config::settings::DataBits::Six,
        "Seven" => crate::config::settings::DataBits::Seven,
        _ => crate::config::settings::DataBits::Eight,
    };
    let parity = match parity.as_str() {
        "Odd" => crate::config::settings::Parity::Odd,
        "Even" => crate::config::settings::Parity::Even,
        _ => crate::config::settings::Parity::None,
    };
    let stop_bits = match stopBits {
        2 => crate::config::settings::StopBits::Two,
        _ => crate::config::settings::StopBits::One,
    };
    let flow_control = match flowControl.as_str() {
        "Software" => crate::config::settings::FlowControl::Software,
        "Hardware" => crate::config::settings::FlowControl::Hardware,
        _ => crate::config::settings::FlowControl::None,
    };

    let params = SerialParams {
        port: port.clone(),
        baud_rate: baudRate,
        data_bits,
        parity,
        stop_bits,
        flow_control,
    };

    // 持锁贯穿 check→spawn→insert,防 TOCTOU 竞态:
    // 若 check 与 insert 之间放锁,另一线程可能在此间隙 connect 同一 port,
    // 双方都通过 contains_key 检查 → 双 spawn → 连接泄漏。全程持同一把锁串行化。
    let mut conns = state.connections.lock().map_err(|e| e.to_string())?;
    if conns.contains_key(&port) {
        return Err(format!("端口 {} 已连接", port));
    }
    // window_label = 调用方窗口 label(main 连→"main",副窗口连→该窗口 label)。
    // reader/writer emit_to 用它定向事件,确保事件回到发起连接的窗口。
    let window_label = webview_window.label().to_string();
    let (handle, _mode) = match spawn_connection(params, app_handle.clone(), window_label) {
        Ok(h) => h,
        Err(e) => {
            // 端口被占用类:Windows "Access is denied" / "being used by another process"
            // 常因上次连接线程未完全释放。disconnect 已持锁等线程退出,此为驱动层极端延迟兜底。
            let lower = e.to_lowercase();
            let port_busy = lower.contains("access is denied")
                || lower.contains("being used by another process")
                || lower.contains("accessdenied")
                || lower.contains("permission");
            if port_busy {
                return Err("端口被占用,可能上次连接未完全释放,请稍后重试".to_string());
            }
            return Err(e);
        }
    };
    conns.insert(port.clone(), handle);
    // 前端 connect 不建窗(用户在当前窗口操作,当前窗口就是界面)。
    // 建窗只在:前端"开新窗口"按钮(open_port_window) + MCP connect(ensure_port_window)。
    drop(conns);
    Ok(())
}

#[tauri::command]
pub fn disconnect(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    port: Option<String>,
) -> Result<(), String> {
    let resolved = {
        let conns = state.connections.lock().map_err(|e| e.to_string())?;
        resolve_port(&conns, port)?
    };
    disconnect_port(&state, &app_handle, &resolved)
}

/// 内部:断开指定 port 的连接(已解析 port)。供 disconnect command 与窗口关闭
/// (CloseRequested)复用。取 handle 放锁后再等线程退出(持 connections 锁等 1s
/// 会阻塞所有其他操作,死锁风险——exit 用 handle.exit 内部锁,与 connections 锁无关)。
pub fn disconnect_port(state: &AppState, app_handle: &tauri::AppHandle, port: &str) -> Result<(), String> {
    let handle = {
        let mut conns = state.connections.lock().map_err(|e| e.to_string())?;
        conns.remove(port)
    };
    // window_label 从 handle 拿(准确);handle 已不在(并发已断)则用 port_to_label 兜底。
    let window_label = handle.as_ref()
        .map(|h| h.window_label.clone())
        .unwrap_or_else(|| crate::connection::port_to_label(port));
    if let Some(handle) = handle {
        let _ = handle.write_tx.send(WriteCommand::Close);
        handle.running.store(false, std::sync::atomic::Ordering::SeqCst);
        // 持锁(此处已释放 conns,用 handle.exit 的内部锁)等线程退出。
        if let Ok(mut e) = handle.exit.0.lock() {
            if !*e {
                let _ = handle.exit.1.wait_timeout(e, std::time::Duration::from_secs(1));
            }
        }
    }
    // 定向通知该连接归属的窗口:主动断开完成。spawn_connection 的监控线程也会发一条
    // connected:false,但那是线程退净后发的;这里已同步等过线程退出,
    // 先发一条让窗口即时响应(幂等,前端取 connected:false 即可)。
    let _ = app_handle.emit_to(&window_label, "connection-state", crate::connection::ConnectionState {
        connected: false,
        port: Some(port.to_string()),
        baud_rate: None,
    });
    Ok(())
}

/// 内部:列出可用串口名。供 Tauri command 和 MCP 工具共用。
pub fn list_ports_inner() -> Vec<String> {
    serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub fn list_ports() -> Result<Vec<String>, String> {
    Ok(list_ports_inner())
}

/// 内部:确保 `win-{port}` 窗口存在,不存在则创建。fire-and-forget 语义:
/// connect 成功后调,窗口异步加载,不阻塞连接返回。已存在则跳过(幂等)。
///
/// Windows 下建窗需在主线程,WebviewWindowBuilder::build() 内部已处理调度;
/// 不在 connections 锁内调用(见 connect 注释)。
pub fn ensure_port_window(app_handle: &tauri::AppHandle, port: &str) {
    let label = port_to_label(port);
    // 已存在则跳过:可能 MCP 与前端先后 connect 同一 port(contains_key 拦了重复连接,
    // 但不同调用路径都走 ensure,幂等保护无妨)。
    if app_handle.get_webview_window(&label).is_some() {
        return;
    }
    let title = format!("NeoSerial - {}", port);
    if let Err(e) = WebviewWindowBuilder::new(app_handle, &label, WebviewUrl::App("index.html".into()))
        .title(&title)
        .build()
    {
        log::warn!("[窗口] 建 {} 窗口失败(连接已建立,不影响操作): {}", label, e);
    }
}

/// 前端调用:显式开一个新串口窗口(主窗口"打开新串口窗口"入口)。
/// 与 connect 解耦——只建窗,不连接。新窗口加载后若该 port 已连(MCP 先连了)
/// 则显示已连状态;未连则显示连接界面让用户配参连接。
#[tauri::command]
pub fn open_port_window(app_handle: tauri::AppHandle, port: String) -> Result<(), String> {
    let label = port_to_label(&port);
    if app_handle.get_webview_window(&label).is_some() {
        return Err(format!("端口 {} 的窗口已打开", port));
    }
    let title = format!("NeoSerial - {}", port);
    WebviewWindowBuilder::new(&app_handle, &label, WebviewUrl::App("index.html".into()))
        .title(&title)
        .build()
        .map_err(|e| format!("创建窗口失败: {}", e))?;
    Ok(())
}

/// 副窗口 onMount 调:按自己 label 反推 port,查 connections 返回本窗口连接状态。
/// main 窗口(label="main")调用时返回 connected:false(主窗口状态由它自己 connect 流程管理)。
#[derive(serde::Serialize)]
pub struct WindowConnState {
    pub ok: bool,
    /// 本窗口对应的 port(None 表示 main 窗口,无固定 port 绑定)
    pub port: Option<String>,
    pub connected: bool,
    pub baud: Option<u32>,
    pub tx_bytes: Option<u64>,
    pub rx_bytes: Option<u64>,
}

#[tauri::command]
pub fn get_window_conn_state(
    state: State<'_, AppState>,
    webview_window: tauri::WebviewWindow,
) -> Result<WindowConnState, String> {
    let label = webview_window.label().to_string();
    let port = label_to_port(&label);
    let Some(port) = port else {
        // main 窗口:无固定 port,主窗口自己管理连接,这里返回未连占位
        return Ok(WindowConnState { ok: true, port: None, connected: false, baud: None, tx_bytes: None, rx_bytes: None });
    };
    let conns = state.connections.lock().map_err(|e| e.to_string())?;
    match conns.get(&port) {
        Some(h) => Ok(WindowConnState {
            ok: true,
            port: Some(port),
            connected: true,
            baud: Some(h.baud),
            tx_bytes: Some(h.tx_bytes.load(std::sync::atomic::Ordering::SeqCst)),
            rx_bytes: Some(h.rx_bytes.load(std::sync::atomic::Ordering::SeqCst)),
        }),
        None => Ok(WindowConnState {
            ok: true,
            port: Some(port),
            connected: false,
            baud: None,
            tx_bytes: None,
            rx_bytes: None,
        }),
    }
}

/// 清零收发字节统计。把累计器置 0，并 emit tx-update/rx-update = 0 让前端同步。
/// 未连接时无 handle，仅 emit 0（前端已置 0，幂等）。
#[tauri::command]
pub fn reset_stats(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    port: Option<String>,
) -> Result<(), String> {
    let resolved = {
        let conns = state.connections.lock().map_err(|e| e.to_string())?;
        resolve_port(&conns, port)?
    };
    // resolve 与清零分两段持锁:resolve 放锁后,连接若在此间隙被 disconnect,
    // 下方 get 返回 None,跳过清零(幂等,仅 emit 0)。这是可接受的弱一致(reset_stats 非关键路径)。
    let conns = state.connections.lock().map_err(|e| e.to_string())?;
    let window_label = conns.get(&resolved)
        .map(|h| h.window_label.clone())
        .unwrap_or_else(|| crate::connection::port_to_label(&resolved));
    if let Some(handle) = conns.get(&resolved) {
        handle.tx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
        handle.rx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
    }
    let _ = app_handle.emit_to(&window_label, "tx-update", crate::connection::TxUpdate { total: 0, port: resolved.clone() });
    let _ = app_handle.emit_to(&window_label, "rx-update", crate::connection::RxUpdate { total: 0, port: resolved });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64};

    use crossbeam_channel::bounded;

    use crate::buffer::rx_history::RxHistory;

    /// 构造测试用 ConnectionHandle:bounded channel + 各 Arc 字段 + rx_history。
    fn mock_handle(port: &str) -> ConnectionHandle {
        let (write_tx, _write_rx) = bounded::<WriteCommand>(8);
        ConnectionHandle {
            running: Arc::new(AtomicBool::new(true)),
            tx_bytes: Arc::new(AtomicU64::new(0)),
            rx_bytes: Arc::new(AtomicU64::new(0)),
            write_tx,
            exit: Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
            port: port.to_string(),
            baud: 115200,
            rx_history: Arc::new(RxHistory::new(100)),
            window_label: format!("win-{}", port),
        }
    }

    #[test]
    fn test_resolve_port_none_single_connection() {
        let mut conns = HashMap::new();
        conns.insert("COM3".to_string(), mock_handle("COM3"));
        assert_eq!(resolve_port(&conns, None).unwrap(), "COM3");
    }

    #[test]
    fn test_resolve_port_none_empty() {
        let conns: HashMap<String, ConnectionHandle> = HashMap::new();
        assert!(resolve_port(&conns, None).is_err());
    }

    #[test]
    fn test_resolve_port_none_multiple() {
        let mut conns = HashMap::new();
        conns.insert("COM3".to_string(), mock_handle("COM3"));
        conns.insert("COM5".to_string(), mock_handle("COM5"));
        let err = resolve_port(&conns, None).unwrap_err();
        assert!(err.contains("多个"), "应提示多个连接,实际: {}", err);
    }

    #[test]
    fn test_resolve_port_some_exists() {
        let mut conns = HashMap::new();
        conns.insert("COM3".to_string(), mock_handle("COM3"));
        assert_eq!(resolve_port(&conns, Some("COM3".to_string())).unwrap(), "COM3");
    }

    #[test]
    fn test_resolve_port_some_not_exists() {
        let mut conns = HashMap::new();
        conns.insert("COM3".to_string(), mock_handle("COM3"));
        let err = resolve_port(&conns, Some("COM9".to_string())).unwrap_err();
        assert!(err.contains("COM9"), "应包含端口名,实际: {}", err);
    }
}
