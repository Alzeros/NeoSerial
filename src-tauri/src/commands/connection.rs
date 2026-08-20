use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

use tauri::{State, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::connection::{ConnectingGuard, ConnectionHandle, SerialParams, spawn_connection, WriteCommand};
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

    // window_label = 调用方窗口 label(main 连→"main",副窗口连→该窗口 label)。
    let caller_label = webview_window.label().to_string();

    // check→占位 持锁(原子,防 TOCTOU),真正的 spawn 放到锁外。
    // 不能持锁贯穿到 spawn:spawn_connection 内的 serialport open + DTR/RTS 拉高是阻塞
    // 调用(实测可卡数秒),持全局 connections 锁做它会把其它已连端口的
    // send/disconnect/get_status/reset_stats 一起冻住。改用 ConnectingGuard 占位守 TOCTOU。
    let guard = {
        let mut conns = state.connections.lock().map_err(|e| e.to_string())?;
        if let Some(existing) = conns.get(&port) {
            // 僵尸检测:running=false 说明 reader/writer 已退出(拔线等被动断开),
            // 但 monitoring 线程还没来得及 remove(或线程清理与用户重连有竞态)。
            // 此时该 handle 已失效,remove 后走正常 spawn 重建,避免报"已连接"卡死重连。
            if is_zombie(existing) {
                conns.remove(&port);
            } else if existing_label_is_mcp(existing) {
                // port 已连:看是谁连的。MCP 连的(window_label=mcp-xxx)→GUI 接管,改 window_label 为当前窗口,
                // 事件从此显示在该 GUI 窗口。别的 GUI 窗口连的→报已连接(防两个窗口抢同一 port)。
                // 接管 MCP 连接:改 window_label,reader/writer 下次 emit 自动用新值
                if let Ok(mut wl) = existing.window_label.write() {
                    *wl = caller_label.clone();
                }
                // 回传该连接**实际**的波特率(MCP 建连时用的),不是本次请求的
                // params.baud_rate——接管不重开串口,请求参数并未生效;若回传请求值,
                // UI 会显示 9600 而硬件实际跑 115200,用户按错的波特率排查问题。
                let actual_baud = existing.baud;
                // 接管成功:发连接成功事件到当前窗口,让 UI 显示已连
                drop(conns);
                let _ = app_handle.emit_to(&caller_label, "connection-state", crate::connection::ConnectionState {
                    connected: true,
                    port: Some(port.clone()),
                    baud_rate: Some(actual_baud),
                });
                // 接管让一个 mcp-only 连接消失(window_label 从 mcp- 改成 GUI label),
                // 通知所有窗口刷新 chip。
                let _ = app_handle.emit("mcp-connections-changed", ());
                return Ok(());
            } else {
                return Err(format!("端口 {} 已连接", port));
            }
        }
        // 占位:并发的第二个 connect 会在此被挡回,不会二次 spawn。
        // 该 port 已在途(可能是 agent 正在连)→ 报错让用户重试,而不是阻塞等它开完。
        match ConnectingGuard::acquire(&state.connecting, &port) {
            Some(g) => g,
            None => return Err(format!("端口 {} 正在连接中,请稍后重试", port)),
        }
    }; // conns 锁在此释放,只有 guard 还占着位

    // 锁外执行阻塞的打开操作。任何 return 路径上 guard 都会析构并清掉占位。
    let (handle, _mode) = match spawn_connection(params, app_handle.clone(), caller_label, state.connections.clone(), state.registry.clone(), false) {
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
    // 重取锁 insert。占位期间没有别的 connect 能走到 spawn,这里必然是空位。
    {
        let mut conns = state.connections.lock().map_err(|e| e.to_string())?;
        conns.insert(port.clone(), handle);
    }
    // 先 insert 再放占位:两者之间到来的 connect 会命中 connections 走正常复用/接管路径。
    drop(guard);
    Ok(())
}

/// 判断已有连接的 window_label 是否为 MCP 前缀(mcp-xxx)。
/// MCP 首次建连时 window_label = mcp-{port};GUI connect 命中时接管改成本窗口 label。
fn existing_label_is_mcp(h: &ConnectionHandle) -> bool {
    h.window_label.read().ok().map(|s| s.starts_with("mcp-")).unwrap_or(false)
}

/// 判断已有连接是否为僵尸(reader/writer 已退出但 handle 仍残留在 map)。
/// 被动断开(拔线)时 reader 报错退出 → monitoring 线程未及时 remove 前,
/// 此 handle running=false 但仍在 map 中。重连 connect 命中它时按僵尸处理:
/// remove 后重建,不报"已连接"卡死重连。
fn is_zombie(h: &ConnectionHandle) -> bool {
    !h.running.load(std::sync::atomic::Ordering::SeqCst)
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

/// 接管窗口关闭时:把它持有的连接按出身处理。
/// - mcp_origin=true(agent 经 MCP 建的,GUI 接管的):改回 `mcp-{port}` 回退接管,
///   连接留着不断,agent 继续用,+ 号红点恢复。
/// - mcp_origin=false(GUI 自己 connect 新建的):断开连接(原语义,关窗口即断)。
///
/// 为什么区分:接管 mcp 连接是 GUI"借走"显示,关窗口应还回去不断开;
/// GUI 自建的连接没人"等用",关窗口应断开避免泄漏。
pub fn release_takeover(state: &AppState, app_handle: &tauri::AppHandle, window_label: &str) {
    // 先扫出本窗口持有的连接:区分 mcp_origin 决定回退还是断开。
    let (mcp_ports, gui_ports): (Vec<String>, Vec<String>) = {
        let conns = state.connections.lock().ok();
        let Some(conns) = conns else { return };
        let mut mcp = Vec::new();
        let mut gui = Vec::new();
        for h in conns.values() {
            let is_ours = h.window_label.read().ok().map(|wl| wl.as_str() == window_label).unwrap_or(false);
            if is_ours {
                if h.mcp_origin.load(std::sync::atomic::Ordering::SeqCst) {
                    mcp.push(h.port.clone());
                } else {
                    gui.push(h.port.clone());
                }
            }
        }
        (mcp, gui)
    };
    // mcp 出身的:改回 mcp label 回退(连接留着)
    if !mcp_ports.is_empty() {
        let conns = state.connections.lock().ok();
        if let Some(conns) = conns {
            for h in conns.values() {
                let is_ours = h.window_label.read().ok().map(|wl| wl.as_str() == window_label).unwrap_or(false);
                if is_ours && h.mcp_origin.load(std::sync::atomic::Ordering::SeqCst) {
                    let mcp_label = format!("mcp-{}", h.port);
                    if let Ok(mut wl) = h.window_label.write() {
                        *wl = mcp_label;
                    }
                }
            }
        }
        // 红点状态变化:回退的连接重新进 mcp-only 列表
        let _ = app_handle.emit("mcp-connections-changed", ());
        // 通知原窗口:本窗口不再接管,UI 显示断开(连接还在但归 mcp 了)
        for p in &mcp_ports {
            let _ = app_handle.emit_to(window_label, "connection-state", crate::connection::ConnectionState {
                connected: false,
                port: Some(p.clone()),
                baud_rate: None,
            });
        }
    }
    // GUI 出身的:断开(关窗口即断,原语义)
    for p in &gui_ports {
        let _ = disconnect_port(state, app_handle, p);
    }
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
    // 连接被移除,mcp-only 集合可能变化,通知所有窗口刷新 chip。
    let _ = app_handle.emit("mcp-connections-changed", ());
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

/// 查询"agent 连了但还没 GUI 窗口接管"的端口:即 window_label 仍是 mcp- 前缀的连接。
/// 供 main 窗口 + 号旁渲染快捷 chip:点击 = 开窗口并接管该连接。
/// GUI 接管后 window_label 改成本窗口 label,此处不再返回(从 chip 列表消失)。
#[derive(serde::Serialize)]
pub struct McpOnlyConn {
    pub port: String,
    pub baud: u32,
}
#[tauri::command]
pub fn get_mcp_only_connections(state: State<'_, AppState>) -> Result<Vec<McpOnlyConn>, String> {
    let conns = state.connections.lock().map_err(|e| e.to_string())?;
    let mut list: Vec<McpOnlyConn> = conns.iter()
        .filter(|(_, h)| {
            // window_label 仍是 mcp- 前缀 = agent 连的、GUI 还没接管
            h.window_label.read().ok().map(|s| s.starts_with("mcp-")).unwrap_or(false)
        })
        .map(|(_, h)| McpOnlyConn { port: h.port.clone(), baud: h.baud })
        .collect();
    // 按 port 排序,保证 chip 顺序稳定(COM 号顺序)
    list.sort_by(|a, b| a.port.cmp(&b.port));
    Ok(list)
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

/// 前端调用:开一个新串口窗口(完整界面的复制品)。
/// - 不传 port:空白未连接窗口,用户进去自己选端口连接。
/// - 传 port:把 {label→port,baud} 记进 pending,新窗口 onMount 调
///   take_pending_takeover 取走后自动 connect 接管该 MCP 连接。
///   用 pending 而非 emit 事件:窗口 JS 加载有先后,事件可能早于监听注册而丢失;
///   pending 由新窗口 ready 后主动查询,无时序依赖。
///
/// label = win-{自增序号},不绑 port——用户在任何窗口都能选任意 port 连。
///
/// **必须 async fn**:Windows 下创建窗口的同步 command 会死锁(spec 警告)。
/// async 让建窗在 Tauri 主线程异步执行,command 立即返回不阻塞。
///
/// 窗口配置与 main 一致(tauri.conf.json):尺寸 1216x750、minWidth 1216、
/// decorations:false(用自定义 TitleBar,否则会有两排窗口按钮)。
#[tauri::command]
pub async fn open_port_window(app_handle: tauri::AppHandle, port: Option<String>, baud: Option<u32>) -> Result<(), String> {
    let n = WINDOW_SEQ.fetch_add(1, AtomicOrdering::SeqCst);
    let label = format!("win-{}", n);
    // 若带 port:先记进 pending(开窗前记,保证新窗口 onMount 查询时已存在)
    if let Some(p) = &port {
        if let Ok(mut pending) = app_handle.try_state::<AppState>()
            .ok_or("无法访问应用状态")?
            .pending_takeover.lock() {
            pending.insert(label.clone(), (p.clone(), baud.unwrap_or(115200)));
        }
    }
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

/// 新窗口 onMount 查"本窗口是否被要求自动接管某端口"。
/// open_port_window(port) 开窗时把 {label→port,baud} 记进 pending,
/// 新窗口 ready 后调此查询(避免事件早于监听注册丢失)。
/// 取走即删(一次性)。
#[tauri::command]
pub fn take_pending_takeover(state: State<'_, AppState>, webview_window: tauri::WebviewWindow) -> Result<Option<McpOnlyConn>, String> {
    let label = webview_window.label().to_string();
    let mut pending = state.pending_takeover.lock().map_err(|e| e.to_string())?;
    Ok(pending.remove(&label).map(|(port, baud)| McpOnlyConn { port, baud }))
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
            line_index: Arc::new(AtomicU64::new(0)),
            mcp_origin: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 构造僵尸 handle:running=false,模拟 reader/writer 已退出(拔线等被动断开)
    /// 但 handle 仍残留在 connections map 的状态。
    fn mock_zombie_handle(port: &str) -> ConnectionHandle {
        let mut h = mock_handle(port);
        h.running.store(false, std::sync::atomic::Ordering::SeqCst);
        h
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

    // ===== 僵尸连接检测契约 =====
    // 拔线等被动断开时 reader 报错退出 → monitoring 线程 remove 前,
    // handle 仍残留在 map 且 running=false。重连 connect 命中时按僵尸处理,
    // 不报"已连接"卡死重连。以下测试锁定判定逻辑:running=false 即僵尸。

    #[test]
    fn test_alive_handle_not_zombie() {
        // 正常存活连接:running=true → 非僵尸
        let h = mock_handle("COM3");
        assert!(!is_zombie(&h), "存活连接不应判为僵尸");
    }

    #[test]
    fn test_dead_handle_is_zombie() {
        // reader/writer 已退出:running=false → 僵尸
        let h = mock_zombie_handle("COM3");
        assert!(is_zombie(&h), "running=false 的残留 handle 应判为僵尸");
    }

    #[test]
    fn test_zombie_label_not_mcp() {
        // 僵尸 handle 的 window_label 判定不受 running 影响:
        // win-{port} label 非 mcp 前缀,existing_label_is_mcp 返回 false。
        // (确认僵尸分支靠 running 判定,不与 mcp 接管分支混淆)
        let h = mock_zombie_handle("COM3");
        assert!(!existing_label_is_mcp(&h));
    }
}
