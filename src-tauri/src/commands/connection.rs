use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

use tauri::{State, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::connection::{detached_label, is_detached_label, ConnectingGuard, ConnectionHandle, SerialParams, spawn_connection, WriteCommand};
use crate::state::AppState;

/// 副窗口自增序号:每开一个新窗口 +1,label = win-{n}。不绑 port——
/// 窗口与连接解耦,用户在任何窗口都能选任意 port 连。
static WINDOW_SEQ: AtomicU32 = AtomicU32::new(1);

/// 主题编辑器窗口最小尺寸(逻辑像素)。后端 builder / set_min_size 两处共用同一常量,
/// 前端 ThemeEditor 不再设 CSS min-width/min-height,保证全局只有一个下限。
const THEME_EDITOR_MIN_W: f64 = 380.0;
const THEME_EDITOR_MIN_H: f64 = 360.0;

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
pub async fn connect(
    app_handle: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    port: String,
    baudRate: u32,
    dataBits: String,
    parity: String,
    stopBits: u8,
    flowControl: String,
) -> Result<(), String> {
    // 同步 command 在 Tauri 主线程执行,而 spawn_connection 内的 serialport::open +
    // DTR/RTS 拉高是阻塞调用(实测可卡数百 ms 到十几秒),期间所有窗口冻结、
    // 事件停止派发。改 async + spawn_blocking:阻塞打开在线程池执行,
    // 对前端 invoke 的返回值/时序语义不变(本来就是 Promise)。
    let handle = app_handle.clone();
    let caller_label = webview_window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
        connect_impl(&state, &handle, caller_label, port, baudRate, dataBits, parity, stopBits, flowControl)
    })
    .await
    .map_err(|e| format!("连接任务执行异常: {}", e))?
}

/// connect 的同步实现(在阻塞线程池执行)。逻辑与旧同步 command 逐行一致
/// (参数名用 snake_case,camelCase 只保留在 command 层供前端 IPC)。
#[allow(clippy::too_many_arguments)]
fn connect_impl(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    caller_label: String,
    port: String,
    baud_rate: u32,
    data_bits: String,
    parity: String,
    stop_bits: u8,
    flow_control: String,
) -> Result<(), String> {
    let data_bits = match data_bits.as_str() {
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
    let stop_bits = match stop_bits {
        2 => crate::config::settings::StopBits::Two,
        _ => crate::config::settings::StopBits::One,
    };
    let flow_control = match flow_control.as_str() {
        "Software" => crate::config::settings::FlowControl::Software,
        "Hardware" => crate::config::settings::FlowControl::Hardware,
        _ => crate::config::settings::FlowControl::None,
    };

    let params = SerialParams {
        port: port.clone(),
        baud_rate,
        data_bits,
        parity,
        stop_bits,
        flow_control,
    };

    // window_label = 调用方窗口 label(main 连→"main",副窗口连→该窗口 label),
    // 由 async command 层取自 webview_window 传入。

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
    // 重取锁 insert + 取快照。占位期间没有别的 connect 能走到 spawn,这里必然是空位。
    let snapshot: Vec<crate::mcp::registry::ConnInfo> = {
        let mut conns = state.connections.lock().map_err(|e| e.to_string())?;
        conns.insert(port.clone(), handle);
        conns.values()
            .map(|h| crate::mcp::registry::ConnInfo { com: h.port.clone(), baud: h.baud })
            .collect()
    };
    // 先 insert 再放占位:两者之间到来的 connect 会命中 connections 走正常复用/接管路径。
    drop(guard);
    // 与 MCP connect 同步:registry 记下 GUI 连的口,agent 按 COM 找实例才找得到;
    // 全局广播连接集合变化(托盘 tooltip 靠它刷新;chip 列表多刷一次无害)。
    if let Some(reg) = state.registry.as_ref() {
        let _ = reg.update_connections(snapshot);
    }
    let _ = app_handle.emit("mcp-connections-changed", ());
    Ok(())
}

/// 已有连接当前是否没有窗口显示(label 为 mcp-{port}):agent 建的、或窗口关掉后留在后台的。
/// GUI connect 命中时挂上(改成本窗口 label),不重开串口。
fn existing_label_is_mcp(h: &ConnectionHandle) -> bool {
    h.window_label.read().ok().map(|s| is_detached_label(&s)).unwrap_or(false)
}

/// 判断已有连接是否为僵尸(reader/writer 已退出但 handle 仍残留在 map)。
/// 被动断开(拔线)时 reader 报错退出 → monitoring 线程未及时 remove 前,
/// 此 handle running=false 但仍在 map 中。重连 connect 命中它时按僵尸处理:
/// remove 后重建,不报"已连接"卡死重连。
fn is_zombie(h: &ConnectionHandle) -> bool {
    !h.running.load(std::sync::atomic::Ordering::SeqCst)
}

#[tauri::command]
pub async fn disconnect(
    app_handle: tauri::AppHandle,
    port: Option<String>,
) -> Result<(), String> {
    // disconnect_port 会同步等 reader/writer 线程退出(最多 1s),
    // 放主线程会冻结 UI;丢阻塞线程池执行。
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
        let resolved = {
            let conns = state.connections.lock().map_err(|e| e.to_string())?;
            resolve_port(&conns, port)?
        };
        disconnect_port(&state, &handle, &resolved)
    })
    .await
    .map_err(|e| format!("断开连接任务执行异常: {}", e))?
}

/// 窗口关闭时处理它持有的连接,策略由 tray::close_plan 按模式给出:
/// - DetachAll(后台模式):一律不断,label 改回 `mcp-{port}` 进入后台,托盘/红点可重新打开。
/// - ReleaseGuiKeepAgent(轻量模式):agent 建的(mcp_origin)交还 agent(同上改 label);
///   窗口自己 connect 新建的断开释放——没人"等用"的连接不该占着口。
pub fn release_window_connections(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    window_label: &str,
    policy: crate::tray::ConnPolicy,
) {
    // 锁内一次扫完:本窗口持有的连接按策略分成"转后台"与"断开"两组,转后台的顺手改 label。
    let (detached, to_disconnect): (Vec<String>, Vec<String>) = {
        let Ok(conns) = state.connections.lock() else { return };
        let mut detached = Vec::new();
        let mut to_disconnect = Vec::new();
        for h in conns.values() {
            let is_ours = h.window_label.read().ok().map(|wl| wl.as_str() == window_label).unwrap_or(false);
            if !is_ours {
                continue;
            }
            let keep = match policy {
                crate::tray::ConnPolicy::DetachAll => true,
                crate::tray::ConnPolicy::ReleaseGuiKeepAgent => {
                    h.mcp_origin.load(std::sync::atomic::Ordering::SeqCst)
                }
            };
            if keep {
                if let Ok(mut wl) = h.window_label.write() {
                    *wl = detached_label(&h.port);
                }
                detached.push(h.port.clone());
            } else {
                to_disconnect.push(h.port.clone());
            }
        }
        (detached, to_disconnect)
    };
    if !detached.is_empty() {
        // 无窗口连接集合变了:所有窗口的 + 号红点、托盘菜单/tooltip 据此刷新
        let _ = app_handle.emit("mcp-connections-changed", ());
    }
    for p in &to_disconnect {
        let _ = disconnect_port(state, app_handle, p);
    }
}

/// 内部:断开指定 port 的连接(已解析 port)。供 disconnect command 与窗口关闭
/// (CloseRequested)复用。取 handle 放锁后再等线程退出(持 connections 锁等 1s
/// 会阻塞所有其他操作,死锁风险——exit 用 handle.exit 内部锁,与 connections 锁无关)。
pub fn disconnect_port(state: &AppState, app_handle: &tauri::AppHandle, port: &str) -> Result<(), String> {
    // 先停该 port 的运行序列,避免 disconnect 后序列线程空转(写 dead channel)
    // 且 SEQUENCE_RUNNING 残留导致 reconnect 后新序列被拒"已在运行"。
    let seq_stopped = crate::commands::sequence::sequence_stop_inner(&state.connections, port)
        .unwrap_or(false);

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
    let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
    // 如果序列因断开被停,立即通知窗口(不等线程退出,给用户即时反馈)。
    // 序列线程退出时也会 emit sequence-done,但可能延迟(delay sleep)且
    // MCP 起的序列 window_label=mcp-{port},GUI 窗口收不到。
    if seq_stopped {
        let _ = app_handle.emit_to(&wl, "sequence-done",
            crate::commands::sequence::SequenceDone { aborted: true });
    }
    // 定向通知该连接归属的窗口:主动断开完成。spawn_connection 的监控线程也会发一条
    // connected:false,但那是线程退净后发的;这里已同步等过线程退出,
    // 先发一条让窗口即时响应(幂等,前端取 connected:false 即可)。
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
pub async fn list_ports() -> Result<Vec<String>, String> {
    // available_ports 在 Windows 走 SetupAPI/注册表枚举(可卡几十~几百 ms),
    // 不放主线程。
    let ports = tauri::async_runtime::spawn_blocking(list_ports_inner)
        .await
        .map_err(|e| format!("枚举串口任务执行异常: {}", e))?;
    Ok(ports)
}

/// 查询"没有窗口显示"的连接:agent 经 MCP 建的、或后台模式下窗口关掉留在后台的
/// (window_label 为 mcp-{port})。供每个窗口 + 号旁的红点列表与托盘菜单:点击 = 开窗口挂上。
/// 窗口挂上后 window_label 改成该窗口 label,此处不再返回。命令名沿用旧称,前端已引用。
#[derive(serde::Serialize)]
pub struct McpOnlyConn {
    pub port: String,
    pub baud: u32,
}
#[tauri::command]
pub fn get_mcp_only_connections(state: State<'_, AppState>) -> Result<Vec<McpOnlyConn>, String> {
    let conns = state.connections.lock().map_err(|e| e.to_string())?;
    let mut list: Vec<McpOnlyConn> = conns.iter()
        .filter(|(_, h)| existing_label_is_mcp(h))
        .map(|(_, h)| McpOnlyConn { port: h.port.clone(), baud: h.baud })
        .collect();
    // 按 port 排序,保证 chip 顺序稳定(COM 号顺序)
    list.sort_by(|a, b| a.port.cmp(&b.port));
    Ok(list)
}

/// 本窗口挂上一个已有连接(agent 建的 / 后台留下的)后拉历史回填日志区:
/// 该连接 rx_history 里的全部行(tx+rx,按序)。新建的连接历史为空,调用无害。
/// 前端按 line_index 与已到达的实时行去重,见 App.svelte。
#[tauri::command]
pub fn get_window_history(state: State<'_, AppState>, port: String) -> Result<Vec<crate::buffer::log_line::LogLine>, String> {
    let history = {
        let conns = state.connections.lock().map_err(|e| e.to_string())?;
        conns.get(&port).map(|h| h.rx_history.clone())
    };
    let Some(history) = history else { return Ok(Vec::new()) };
    let (lines, _) = history.since(0);
    Ok(lines.into_iter().map(|(_, _, line)| line).collect())
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
/// 窗口配置与 main 一致(tauri.conf.json):尺寸 1216x800、minWidth 1216、
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
        .inner_size(1216.0, 800.0)
        .min_inner_size(1216.0, 600.0)
        .decorations(false)
        .resizable(true)
        .build()
        .map_err(|e| format!("创建窗口失败: {}", e))?;
    Ok(())
}

/// 打开自定义主题编辑器窗口(单例:已存在则聚焦,不重复开)。
/// 窗口 label 固定 "theme-editor",前端按 label 渲染 ThemeEditor 组件。
#[tauri::command]
pub async fn open_theme_editor(app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    // 已存在则聚焦置顶(单例语义)
    if let Some(w) = app_handle.get_webview_window("theme-editor") {
        w.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(&app_handle, "theme-editor", WebviewUrl::App("index.html".into()))
        .title("主题编辑器")
        .inner_size(385.0, 680.0)
        // 窗口最小尺寸:380x360。调色面板不再含预览（预览在主窗口），
        // 窗口只需容纳标题栏 + 工具条 + 色板 + 圆角 + 底栏。
        .min_inner_size(THEME_EDITOR_MIN_W, THEME_EDITOR_MIN_H)
        .decorations(false)
        .resizable(true)
        .build()
        .map_err(|e| format!("创建主题编辑器窗口失败: {}", e))?;
    // 无边框窗口在 Windows 上 builder 的 min_inner_size 不总生效,build 后显式
    // 再设一次。必须用 LogicalSize 与 builder 保持同一个数 —— 之前用
    // PhysicalSize(640,420),在缩放 >100% 的屏幕上会被换算成 640/scale 逻辑像素,
    // 于是 OS 下限和 CSS 下限变成两个不同的值,拖动到临界点时钳制值在两数之间
    // 反复跳变,就是看到的"整个框一直在闪"。
    window.set_min_size(Some(tauri::LogicalSize::new(THEME_EDITOR_MIN_W, THEME_EDITOR_MIN_H)))
        .map_err(|e| format!("设置最小尺寸失败: {}", e))?;
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
