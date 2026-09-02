pub mod line_assembler;
pub mod reader;
pub mod writer;
pub mod port_wrapper;
#[cfg(target_os = "windows")]
pub mod win_port;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64};

use crossbeam_channel::{bounded, Sender};
use serde::Serialize;
use tauri::Emitter;

use crate::buffer::rx_history::RxHistory;
use crate::config::settings::{DataBits, FlowControl, Parity, StopBits};

/// 窗口 label 前缀。多窗口场景下每个连接对应一个 webview 窗口,label 固定规则
/// `win-{port}`(如 `win-COM3`),port 与 label 双向推导,不维护映射表。
const WIN_LABEL_PREFIX: &str = "win-";

/// port → 窗口 label:`COM3` → `win-COM3`。
pub fn port_to_label(port: &str) -> String {
    format!("{}{}", WIN_LABEL_PREFIX, port)
}

/// 窗口 label → port:`win-COM3` → `Some(COM3)`;非 `win-` 前缀(main 窗口)返回 None。
pub fn label_to_port(label: &str) -> Option<String> {
    label.strip_prefix(WIN_LABEL_PREFIX).map(|s| s.to_string())
}

/// 发送到写线程的命令。
pub enum WriteCommand {
    Send(Vec<u8>),
    /// 只写串口不 emit tx-line（调用方已自行 emit）。用于脚本序列：
    /// sequence 线程按 delay 间隔发起并 emit，writer 异步写，避免写阻塞拖慢发起节奏。
    SendSilent(Vec<u8>),
    Close,
}

/// 串口参数。
#[derive(Clone, Debug)]
pub struct SerialParams {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow_control: FlowControl,
}

/// 连接句柄 — 持有写线程的 sender、运行标志、线程退出通知。
pub struct ConnectionHandle {
    pub running: Arc<AtomicBool>,
    pub tx_bytes: Arc<AtomicU64>,
    pub rx_bytes: Arc<AtomicU64>,
    pub write_tx: Sender<WriteCommand>,
    /// reader/writer 线程退出通知。monitoring 线程 join 完两者后置 true+notify_all。
    /// disconnect 持锁 wait_timeout 等它,保证旧连接释放完才返回(消重连竞态)。
    pub exit: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    /// 连接的串口名(如 "COM3"),供 get_status 返回。
    pub port: String,
    /// 连接的波特率,供 get_status 返回。
    pub baud: u32,
    /// 该连接的独立收发历史。reader push Rx,emit_tx_line/writer push Tx,
    /// MCP send_and_read/get_history_since 查询。per-connection 隔离,不串数据。
    pub rx_history: Arc<RxHistory>,
    /// 该连接归属的窗口 label。reader/writer emit_to 读它定向事件。
    /// 用 Arc<RwLock<String>>:GUI 接管 MCP 连接时改此值,reader/writer 自动读新 label,
    /// 事件定向到接管的 GUI 窗口(无需重启线程)。MCP connect 初始 mcp-{port},
    /// GUI 窗口 connect 复用时改成该窗口 label。
    pub window_label: Arc<std::sync::RwLock<String>>,
    /// 本次连接期间的单调行号计数器(tx+rx 共用)。开端口=0,每构造一行 +1。
    /// 连接关闭(新建连接)自然重置(新 Arc=0)。供 LogLine.line_index。
    pub line_index: Arc<AtomicU64>,
    /// 连接出身是否为 MCP:true=agent 经 MCP connect 建的(GUI 接管后关窗口
    /// 应回退到 mcp label,连接留着);false=GUI 窗口自己 connect 新建的
    /// (关窗口应断开)。供 CloseRequested 区分回退还是断开。
    pub mcp_origin: Arc<AtomicBool>,
}

/// 发送给前端的 Tx 更新事件。
#[derive(Clone, Serialize)]
pub struct TxUpdate {
    pub total: u64,
    pub port: String,
}

/// 发送给前端的 Rx 更新事件。
#[derive(Clone, Serialize)]
pub struct RxUpdate {
    pub total: u64,
    pub port: String,
}

/// 连接状态变化事件。
#[derive(Clone, Serialize)]
/// 连接状态变化事件。
pub struct ConnectionState {
    pub connected: bool,
    pub port: Option<String>,
    /// 波特率(连接成功时带;断开为 None)。供前端回填端口/波特率下拉框。
    pub baud_rate: Option<u32>,
}

/// 错误事件。
#[derive(Clone, Serialize)]
pub struct ErrorEvent {
    pub message: String,
}

/// 连接模式。
#[derive(Clone, Serialize)]
pub struct ConnectionMode {
    /// "independent" 表示独立句柄模式，"shared" 表示共享端口降级模式
    pub mode: String,
}

/// 事件节流器:限制事件最小发射间隔,窗口内被压下的更新记 pending,
/// 由调用方在空闲/线程退出路径 [`EventThrottle::take_pending`] 补发。
///
/// 用于 rx-update/tx-update 这类"终值重要、中间值可合并"的计数器事件:
/// 高波特率下每次 read/write 都 emit 会以每秒成百上千个事件冲垮 IPC 与前端,
/// 节流后只有计数器刷新粒度变粗(≤ min_interval + 空闲检测周期),终值永不丢。
pub struct EventThrottle {
    min_interval: std::time::Duration,
    last_emit: Option<std::time::Instant>,
    pending: bool,
}

impl EventThrottle {
    pub fn new(min_interval: std::time::Duration) -> Self {
        EventThrottle { min_interval, last_emit: None, pending: false }
    }

    /// 有新数据时调用。返回 true = 到点,应立即 emit;false = 已记 pending。
    pub fn on_activity(&mut self) -> bool {
        let due = self.last_emit.map_or(true, |t| t.elapsed() >= self.min_interval);
        if due {
            self.last_emit = Some(std::time::Instant::now());
            self.pending = false;
            true
        } else {
            self.pending = true;
            false
        }
    }

    /// 空闲/退出时调用。返回 true = 有被压下的更新需补发(标记已清)。
    pub fn take_pending(&mut self) -> bool {
        if self.pending {
            self.pending = false;
            // 补发也占用一个节流窗口,避免空闲补发后紧跟的密集活动立即再发
            self.last_emit = Some(std::time::Instant::now());
            true
        } else {
            false
        }
    }
}

/// 在途连接的端口集合。connect 在放掉 `connections` 锁之前把 port 记进来占位,
/// 打开完成(或失败)后移除。见 [`ConnectingGuard`]。
pub type ConnectingSet = Arc<Mutex<HashSet<String>>>;

/// 在途连接占位守卫:创建时把 port 记入 `connecting` 集合,Drop 时自动移除。
///
/// 为什么需要它:`spawn_connection` 里的 `serialport::open()` 与 DTR/RTS 拉高是**阻塞**
/// 驱动调用(Windows 上实测可卡数百 ms 到十几秒)。若为了防 TOCTOU 而全程持 `connections`
/// 全局锁,打开一个坏端口期间,其它所有已连端口的 send/disconnect/get_status/reset_stats
/// 都会被这把锁挡住——正好违背"并发收发不阻塞"。
///
/// 所以拆成:持锁期间只做检查 + 占位(原子),阻塞的 open 放到锁外,完成后重取锁 insert。
/// 占位守住了 TOCTOU:并发的第二个 connect 即使看到 `connections` 里没有该 port,
/// 也会在 `acquire` 处被挡回,不会二次 spawn 造成连接泄漏。
///
/// 用 RAII 而非手动 remove:`spawn_connection` 失败有多条 early return 分支(端口占用
/// 文案、通用错误…),漏掉任一条都会留下永久幽灵占位,该端口从此再也连不上。
pub struct ConnectingGuard {
    set: ConnectingSet,
    port: String,
}

impl ConnectingGuard {
    /// 尝试为 port 占位。已有在途连接返回 None(调用方应报"正在连接中")。
    ///
    /// **必须在持 `connections` 锁期间调用**——"port 不在 connections"的检查与占位
    /// 只有在同一临界区内才是原子的。锁序固定为 connections → connecting,勿反向。
    pub fn acquire(set: &ConnectingSet, port: &str) -> Option<ConnectingGuard> {
        let mut in_flight = set.lock().ok()?;
        if !in_flight.insert(port.to_string()) {
            return None;
        }
        Some(ConnectingGuard { set: set.clone(), port: port.to_string() })
    }
}

impl Drop for ConnectingGuard {
    fn drop(&mut self) {
        if let Ok(mut in_flight) = self.set.lock() {
            in_flight.remove(&self.port);
        }
    }
}

/// 建立串口连接。成功返回 (ConnectionHandle, mode_str)，失败返回错误信息。
/// mode_str: "independent"(独立句柄) 或 "shared"(共享端口降级)。
///
/// 策略：
/// 1. 首先尝试 port.try_clone()，为读/写线程各持有一个独立句柄（无锁，性能最优）
/// 2. 如果克隆失败（某些 USB 转串口芯片不支持），降级为 Arc<Mutex<>> 共享单个端口
///
/// `connections` 与 `registry`:监控线程在 reader/writer 退出后(含拔线等被动断开),
/// 从 map remove 本连接的僵尸 handle 并更新 registry——否则重连会撞到残留条目报"已连接"。
/// 主动 disconnect 也走同一 remove 路径(disconnect 先 remove,此处幂等)。
pub fn spawn_connection(
    params: SerialParams,
    app_handle: tauri::AppHandle,
    window_label: String,
    connections: Arc<Mutex<HashMap<String, ConnectionHandle>>>,
    registry: Option<crate::mcp::registry::RegistryHandle>,
    mcp_origin: bool,
) -> Result<(ConnectionHandle, String), String> {
    #[cfg(not(target_os = "windows"))]
    let data_bits = match params.data_bits {
        DataBits::Five => serialport::DataBits::Five,
        DataBits::Six => serialport::DataBits::Six,
        DataBits::Seven => serialport::DataBits::Seven,
        DataBits::Eight => serialport::DataBits::Eight,
    };
    #[cfg(not(target_os = "windows"))]
    let parity = match params.parity {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    };
    #[cfg(not(target_os = "windows"))]
    let stop_bits = match params.stop_bits {
        StopBits::One => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
    };
    #[cfg(not(target_os = "windows"))]
    let flow_control = match params.flow_control {
        FlowControl::None => serialport::FlowControl::None,
        FlowControl::Software => serialport::FlowControl::Software,
        FlowControl::Hardware => serialport::FlowControl::Hardware,
    };

    // Windows: 用原生 overlapped I/O 打开端口（serialport 的同步 WriteFile 会阻塞数秒）。
    // 其他平台: 用 serialport crate。
    #[cfg(target_os = "windows")]
    let (reader_port, writer_port, mode_str) = {
        let win_port = win_port::WinPort::open(
            &params.port,
            params.baud_rate,
            match params.data_bits { DataBits::Five => 5, DataBits::Six => 6, DataBits::Seven => 7, DataBits::Eight => 8 },
            match params.parity { Parity::None => "none", Parity::Odd => "odd", Parity::Even => "even" },
            match params.stop_bits { StopBits::One => 1, StopBits::Two => 2 },
            match params.flow_control { FlowControl::None => "none", FlowControl::Software => "software", FlowControl::Hardware => "hardware" },
        ).map_err(|e| format!("打开串口失败: {}", e))?;

        let writer = win_port.try_clone()
            .map_err(|e| format!("克隆端口失败: {}", e))?;

        log::info!("[Connection] 使用 Windows 原生 overlapped I/O");
        (win_port, writer, "overlapped".to_string())
    };

    #[cfg(not(target_os = "windows"))]
    let (reader_port, writer_port, mode_str) = {
        let port = serialport::new(&params.port, params.baud_rate)
            .data_bits(data_bits)
            .parity(parity)
            .stop_bits(stop_bits)
            .flow_control(flow_control)
            .timeout(std::time::Duration::from_millis(20))
            .open()
            .map_err(|e| format!("打开串口失败: {}", e))?;

        let _ = port.write_data_terminal_ready(true);
        let _ = port.write_request_to_send(true);

        match port.try_clone() {
            Ok(cloned) => {
                log::info!("[Connection] 使用独立句柄模式");
                (port_wrapper::PortReader::Owned(cloned),
                 port_wrapper::PortWriter::Owned(port),
                 "independent".to_string())
            }
            Err(e) => {
                log::warn!("[Connection] try_clone 失败 ({}), 降级为共享端口模式", e);
                let shared = Arc::new(Mutex::new(port));
                (port_wrapper::PortReader::Shared(shared.clone()),
                 port_wrapper::PortWriter::Shared(shared),
                 "shared".to_string())
            }
        }
    };

    let running = Arc::new(AtomicBool::new(true));
    let tx_bytes = Arc::new(AtomicU64::new(0));
    let rx_bytes = Arc::new(AtomicU64::new(0));
    let exit = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let rx_history = Arc::new(RxHistory::new(10_000));
    // window_label 用 Arc<RwLock>:reader/writer 持 clone,emit 时读;GUI 接管时改值,线程自动用新 label
    let window_label = Arc::new(std::sync::RwLock::new(window_label));
    // per-conn 行号计数器(开端口=0,每行 +1,新建连接自然重置)
    let line_index = Arc::new(AtomicU64::new(0));
    // 连接出身:MCP 建的=true(关窗口回退不断),GUI 建的=false(关窗口断开)
    let mcp_origin = Arc::new(AtomicBool::new(mcp_origin));

    // 创建写通道
    let (write_tx, write_rx) = bounded::<WriteCommand>(64);

    let handle = ConnectionHandle {
        running: running.clone(),
        tx_bytes: tx_bytes.clone(),
        rx_bytes: rx_bytes.clone(),
        write_tx,
        exit: exit.clone(),
        port: params.port.clone(),
        baud: params.baud_rate,
        rx_history: rx_history.clone(),
        window_label: window_label.clone(),
        line_index: line_index.clone(),
        mcp_origin: mcp_origin.clone(),
    };

    // 启动 reader 和 writer 线程
    #[cfg(target_os = "windows")]
    let (reader_handle, writer_handle) = {
        let r_handle = reader::spawn_reader(
            port_wrapper::PortReader::Win(reader_port),
            running.clone(),
            rx_bytes.clone(),
            app_handle.clone(),
            rx_history.clone(),
            params.port.clone(),
            window_label.clone(),
            line_index.clone(),
        );
        let w_handle = writer::spawn_writer(
            port_wrapper::PortWriter::Win(writer_port),
            write_rx,
            running.clone(),
            tx_bytes.clone(),
            app_handle.clone(),
            rx_history.clone(),
            params.port.clone(),
            window_label.clone(),
            line_index.clone(),
        );
        (r_handle, w_handle)
    };

    #[cfg(not(target_os = "windows"))]
    let (reader_handle, writer_handle) = {
        let r_handle = reader::spawn_reader(
            reader_port,
            running.clone(),
            rx_bytes.clone(),
            app_handle.clone(),
            rx_history.clone(),
            params.port.clone(),
            window_label.clone(),
            line_index.clone(),
        );
        let w_handle = writer::spawn_writer(
            writer_port,
            write_rx,
            running.clone(),
            tx_bytes.clone(),
            app_handle.clone(),
            rx_history.clone(),
            params.port.clone(),
            window_label.clone(),
            line_index.clone(),
        );
        (r_handle, w_handle)
    };

    // 发送连接模式事件
    {
        let wl = window_label.read().map_err(|e| e.to_string())?;
        let _ = app_handle.emit_to(&*wl, "connection-mode", ConnectionMode { mode: mode_str.clone() });
    }

    // 监控线程：等待读/写线程都退出后，清理状态并通知前端
    let port_name = params.port.clone();
    let window_label_clone = window_label.clone();
    let app_handle_clone = app_handle.clone();
    let running_clone = running.clone();
    let exit_clone = exit.clone();
    let connections_clone = connections.clone();
    let registry_clone = registry.clone();
    std::thread::spawn(move || {
        // 阻塞等待两个线程真正退出
        let _ = reader_handle.join();
        let _ = writer_handle.join();
        running_clone.store(false, std::sync::atomic::Ordering::SeqCst);
        // 通知 disconnect:线程已退净,可安全返回(供下一次 connect)
        if let Ok(mut e) = exit_clone.0.lock() {
            *e = true;
            exit_clone.1.notify_all();
        }
        // 从 connections map 移除本连接的僵尸 handle 并更新 registry。
        // 被动断开(拔线)时 reader 报错退出 → 此处 remove,避免重连撞残留条目报"已连接"。
        // 主动 disconnect 已先 remove 此 key,这里 remove 幂等返回 None,不冲突。
        // 仅当 map 里仍是本 handle(同地址)才 remove——防止把 disconnect 后用同 port 新建的
        // 连接误删(极小竞态窗口:disconnect 与重连几乎同时)。地址比较兜底。
        let snapshot: Vec<crate::mcp::registry::ConnInfo> = {
            if let Ok(mut conns) = connections_clone.lock() {
                // 用 Arc 地址比较:map 里该 port 的 handle 若仍是本次 spawn 的同一个,
                // 则是僵尸,remove;若已是新连接(重连覆盖了),则不动。
                let ours = conns.get(&port_name).map(|h| Arc::as_ptr(&h.window_label) as usize);
                let is_ours = ours.is_some_and(|addr| addr == (Arc::as_ptr(&window_label_clone) as usize));
                if is_ours {
                    conns.remove(&port_name);
                }
                conns.values()
                    .map(|h| crate::mcp::registry::ConnInfo { com: h.port.clone(), baud: h.baud })
                    .collect()
            } else {
                Vec::new()
            }
        };
        if let Some(reg) = registry_clone.as_ref() {
            let _ = reg.update_connections(snapshot);
        }
        // 连接被清理(拔线等被动断开),mcp-only 集合变化,通知所有 GUI 窗口刷新 chip。
        let _ = app_handle_clone.emit("mcp-connections-changed", ());
        // 定向通知该连接归属的窗口:连接已断开(线程退出,可能是被动断开如拔线)
        if let Ok(wl) = window_label_clone.read() {
            let _ = app_handle_clone.emit_to(&*wl, "connection-state", ConnectionState {
                connected: false,
                port: Some(port_name),
                baud_rate: None,
            });
        }
    });

    // 发送连接成功事件(定向到该连接归属的窗口)
    {
        let wl = window_label.read().map_err(|e| e.to_string())?;
        let _ = app_handle.emit_to(&*wl, "connection-state", ConnectionState {
            connected: true,
            port: Some(params.port.clone()),
            baud_rate: Some(params.baud_rate),
        });
    }

    Ok((handle, mode_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_to_label() {
        assert_eq!(port_to_label("COM3"), "win-COM3");
        assert_eq!(port_to_label("COM12"), "win-COM12");
    }

    #[test]
    fn test_label_to_port() {
        assert_eq!(label_to_port("win-COM3"), Some("COM3".to_string()));
        assert_eq!(label_to_port("win-COM12"), Some("COM12".to_string()));
        // main 窗口 label 无 win- 前缀,返回 None
        assert_eq!(label_to_port("main"), None);
        assert_eq!(label_to_port(""), None);
    }

    #[test]
    fn test_roundtrip() {
        for port in ["COM1", "COM3", "COM12", "COM255"] {
            assert_eq!(label_to_port(&port_to_label(port)), Some(port.to_string()));
        }
    }

    #[test]
    fn test_event_throttle_first_emit_immediate() {
        let mut t = EventThrottle::new(std::time::Duration::from_secs(3600));
        assert!(t.on_activity(), "首次应立即放行");
        assert!(!t.on_activity(), "窗口内应压制并记 pending");
        assert!(t.take_pending(), "有 pending 需补发");
        assert!(!t.take_pending(), "取过即清,不重复补发");
        assert!(!t.on_activity(), "补发占用了窗口,继续压制");
    }

    #[test]
    fn test_event_throttle_window_expires() {
        let mut t = EventThrottle::new(std::time::Duration::from_millis(1));
        assert!(t.on_activity());
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(t.on_activity(), "窗口过期后应放行");
    }

    #[test]
    fn test_event_throttle_no_pending_no_flush() {
        let mut t = EventThrottle::new(std::time::Duration::from_secs(3600));
        assert!(t.on_activity());
        // 已立即 emit 且无压制,空闲补发应为 no-op
        assert!(!t.take_pending());
    }
}
