use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

use tauri::{State, Emitter, WebviewUrl, WebviewWindowBuilder};

use crate::connection::{ConnectionHandle, SerialParams, spawn_connection, WriteCommand};
use crate::state::AppState;

/// 副窗口自增序号:每开一个新窗口 +1,label = win-{n}。不绑 port——
/// 窗口与连接解耦,用户在任何窗口都能选任意 port 连。
static WINDOW_SEQ: AtomicU32 = AtomicU32::new(1);

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
    // window_label = 调用方窗口 label(main 连→"main",副窗口连→该窗口 label)。
    let caller_label = webview_window.label().to_string();
    if let Some(existing) = conns.get(&port) {
        // port 已连:看是谁连的。MCP 连的(window_label=mcp-xxx)→GUI 接管,改 window_label 为当前窗口,
        // 事件从此显示在该 GUI 窗口。别的 GUI 窗口连的→报已连接(防两个窗口抢同一 port)。
        let existing_label = existing.window_label.read().ok().map(|s| s.clone()).unwrap_or_default();
        if existing_label.starts_with("mcp-") {
            // 接管 MCP 连接:改 window_label,reader/writer 下次 emit 自动用新值
            if let Ok(mut wl) = existing.window_label.write() {
                *wl = caller_label.clone();
            }
            // 接管成功:发连接成功事件到当前窗口,让 UI 显示已连
            drop(conns);
            let _ = app_handle.emit_to(&caller_label, "connection-state", crate::connection::ConnectionState {
                connected: true,
                port: Some(port.clone()),
                baud_rate: Some(params.baud_rate),
            });
            return Ok(());
        }
        return Err(format!("端口 {} 已连接", port));
    }
    let (handle, _mode) = match spawn_connection(params, app_handle.clone(), caller_label) {
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
    let Some(handle) = handle else {
        // 连接已不在(并发已断/重复 disconnect):无需清理,也无窗口可通知。
        return Ok(());
    };
    let window_label = handle.window_label.clone();
    let _ = handle.write_tx.send(WriteCommand::Close);
    handle.running.store(false, std::sync::atomic::Ordering::SeqCst);
    // 持锁(此处已释放 conns,用 handle.exit 的内部锁)等线程退出。
    if let Ok(mut e) = handle.exit.0.lock() {
        if !*e {
            let _ = handle.exit.1.wait_timeout(e, std::time::Duration::from_secs(1));
        }
    }
    // 定向通知该连接归属的窗口:主动断开完成。spawn_connection 的监控线程也会发一条
    // connected:false,但那是线程退净后发的;这里已同步等过线程退出,
    // 先发一条让窗口即时响应(幂等,前端取 connected:false 即可)。
    let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
    let _ = app_handle.emit_to(&wl, "connection-state", crate::connection::ConnectionState {
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

/// 查询是否有"需要二次确认关闭"的活跃会话:其他可见窗口 + 活跃连接。
/// 供 main 窗口关闭按钮判断是否弹确认(关 main 会断所有连接+退 app+停 MCP)。
#[derive(serde::Serialize)]
pub struct ActiveSessions {
    /// 其他可见窗口数(非 main 的 win-* 窗口)
    pub other_windows: usize,
    /// 活跃串口连接数
    pub connections: usize,
}

#[tauri::command]
pub fn has_active_sessions(state: State<'_, AppState>, app_handle: tauri::AppHandle) -> Result<ActiveSessions, String> {
    let connections = state.connections.lock().map_err(|e| e.to_string())?.len();
    // 其他可见窗口:非 main 的 webview 窗口
    use tauri::Manager;
    let other_windows = app_handle.webview_windows()
        .into_iter()
        .filter(|(label, _)| label != "main")
        .count();
    Ok(ActiveSessions { other_windows, connections })
}

/// 前端调用:开一个新串口窗口(完整界面的复制品,空白未连接)。
/// label = win-{自增序号},不绑 port——用户进去自己在左上角选端口/波特率再点连接。
///
/// **必须 async fn**:Windows 下创建窗口的同步 command 会死锁(spec 警告)。
/// async 让建窗在 Tauri 主线程异步执行,command 立即返回不阻塞。
///
/// 窗口配置与 main 一致(tauri.conf.json):尺寸 1216x750、minWidth 1216、
/// decorations:false(用自定义 TitleBar,否则会有两排窗口按钮)。
#[tauri::command]
pub async fn open_port_window(app_handle: tauri::AppHandle) -> Result<(), String> {
    let n = WINDOW_SEQ.fetch_add(1, AtomicOrdering::SeqCst);
    let label = format!("win-{}", n);
    WebviewWindowBuilder::new(&app_handle, &label, WebviewUrl::App("index.html".into()))
        .title("NeoSerial")
        .inner_size(1216.0, 750.0)
        .min_inner_size(1216.0, 500.0)
        .decorations(false)
        .resizable(true)
        .build()
        .map_err(|e| format!("创建窗口失败: {}", e))?;
    Ok(())
}

/// 窗口 onMount 调:按本窗口 label 查"该窗口是否已连了某个 port"。
/// 遍历 connections 找 window_label == 本窗口 label 的连接(MCP 可能先连了,
/// 或窗口重连场景)。未找到返回未连占位。
#[derive(serde::Serialize)]
pub struct WindowConnState {
    pub ok: bool,
    /// 本窗口已连的 port(未连为 None)
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
    let conns = state.connections.lock().map_err(|e| e.to_string())?;
    // 遍历找 window_label == 本窗口 label 的连接(读 RwLock 比较)
    let found = conns.values().find(|h| {
        h.window_label.read().map(|wl| wl.as_str() == label).unwrap_or(false)
    });
    match found {
        Some(h) => Ok(WindowConnState {
            ok: true,
            port: Some(h.port.clone()),
            connected: true,
            baud: Some(h.baud),
            tx_bytes: Some(h.tx_bytes.load(std::sync::atomic::Ordering::SeqCst)),
            rx_bytes: Some(h.rx_bytes.load(std::sync::atomic::Ordering::SeqCst)),
        }),
        None => Ok(WindowConnState {
            ok: true,
            port: None,
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
    // 下方 get 返回 None,跳过清零+emit(幂等,前端已 0)。弱一致可接受(reset_stats 非关键路径)。
    let conns = state.connections.lock().map_err(|e| e.to_string())?;
    if let Some(handle) = conns.get(&resolved) {
        let window_label = handle.window_label.clone();
        handle.tx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
        handle.rx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
        let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
        let _ = app_handle.emit_to(&wl, "tx-update", crate::connection::TxUpdate { total: 0, port: resolved.clone() });
        let _ = app_handle.emit_to(&wl, "rx-update", crate::connection::RxUpdate { total: 0, port: resolved });
    }
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
            window_label: Arc::new(std::sync::RwLock::new(format!("win-{}", port))),
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
