pub mod line_assembler;
pub mod reader;
pub mod writer;
pub mod port_wrapper;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use crossbeam_channel::{bounded, Sender};
use serde::Serialize;
use tauri::Emitter;

use crate::config::settings::{DataBits, FlowControl, Parity, StopBits};

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
}

/// 发送给前端的 Tx 更新事件。
#[derive(Clone, Serialize)]
pub struct TxUpdate {
    pub total: u64,
}

/// 发送给前端的 Rx 更新事件。
#[derive(Clone, Serialize)]
pub struct RxUpdate {
    pub total: u64,
}

/// 连接状态变化事件。
#[derive(Clone, Serialize)]
pub struct ConnectionState {
    pub connected: bool,
    pub port: Option<String>,
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

/// 建立串口连接。成功返回 ConnectionHandle，失败返回错误信息。
///
/// 策略：
/// 1. 首先尝试 port.try_clone()，为读/写线程各持有一个独立句柄（无锁，性能最优）
/// 2. 如果克隆失败（某些 USB 转串口芯片不支持），降级为 Arc<Mutex<>> 共享单个端口
pub fn spawn_connection(
    params: SerialParams,
    app_handle: tauri::AppHandle,
) -> Result<ConnectionHandle, String> {
    let data_bits = match params.data_bits {
        DataBits::Five => serialport::DataBits::Five,
        DataBits::Six => serialport::DataBits::Six,
        DataBits::Seven => serialport::DataBits::Seven,
        DataBits::Eight => serialport::DataBits::Eight,
    };
    let parity = match params.parity {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    };
    let stop_bits = match params.stop_bits {
        StopBits::One => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
    };
    let flow_control = match params.flow_control {
        FlowControl::None => serialport::FlowControl::None,
        FlowControl::Software => serialport::FlowControl::Software,
        FlowControl::Hardware => serialport::FlowControl::Hardware,
    };

    let mut port = serialport::new(&params.port, params.baud_rate)
        .data_bits(data_bits)
        .parity(parity)
        .stop_bits(stop_bits)
        .flow_control(flow_control)
        .timeout(std::time::Duration::from_millis(50))
        .open()
        .map_err(|e| format!("打开串口失败: {}", e))?;

    // 通信模组的 USB 虚拟串口常以 DTR/RTS 作为"DTE 已连接"信号。
    // 若未拉高，部分模组会认为主机未就绪而拒绝处理收到的数据，导致
    // 串口写入被驱动阻塞数秒（实测 write_all 卡 4-14s）。连接建立后显式拉高。
    let _ = port.write_data_terminal_ready(true);
    let _ = port.write_request_to_send(true);

    let running = Arc::new(AtomicBool::new(true));
    let tx_bytes = Arc::new(AtomicU64::new(0));
    let rx_bytes = Arc::new(AtomicU64::new(0));
    let exit = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));

    // 创建写通道
    let (write_tx, write_rx) = bounded::<WriteCommand>(64);

    let handle = ConnectionHandle {
        running: running.clone(),
        tx_bytes: tx_bytes.clone(),
        rx_bytes: rx_bytes.clone(),
        write_tx,
        exit: exit.clone(),
    };

    // 尝试克隆端口，失败则降级为共享模式
    let clone_result = port.try_clone();
    
    let (reader_handle, writer_handle, mode_str) = match clone_result {
        Ok(reader_port) => {
            // 模式1：独立句柄（无锁，性能最优）
            log::info!("[Connection] 使用独立句柄模式（try_clone 成功）");
            
            let r_handle = reader::spawn_reader(
                port_wrapper::PortReader::Owned(reader_port),
                running.clone(),
                rx_bytes.clone(),
                app_handle.clone(),
            );

            let w_handle = writer::spawn_writer(
                port_wrapper::PortWriter::Owned(port),
                write_rx,
                running.clone(),
                tx_bytes.clone(),
                app_handle.clone(),
            );
            
            (r_handle, w_handle, "independent".to_string())
        }
        Err(e) => {
            // 模式2：共享端口（Arc<Mutex<>> 降级模式）
            log::warn!("[Connection] try_clone 失败 ({}), 降级为共享端口模式", e);
            
            let shared = Arc::new(std::sync::Mutex::new(port));
            
            let r_handle = reader::spawn_reader(
                port_wrapper::PortReader::Shared(shared.clone()),
                running.clone(),
                rx_bytes.clone(),
                app_handle.clone(),
            );

            let w_handle = writer::spawn_writer(
                port_wrapper::PortWriter::Shared(shared),
                write_rx,
                running.clone(),
                tx_bytes.clone(),
                app_handle.clone(),
            );
            
            (r_handle, w_handle, "shared".to_string())
        }
    };

    // 发送连接模式事件
    let _ = app_handle.emit("connection-mode", ConnectionMode { mode: mode_str });

    // 监控线程：等待读/写线程都退出后，清理状态并通知前端
    let port_name = params.port.clone();
    let app_handle_clone = app_handle.clone();
    let running_clone = running.clone();
    let exit_clone = exit.clone();
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
        let _ = app_handle_clone.emit("connection-state", ConnectionState {
            connected: false,
            port: Some(port_name),
        });
    });

    // 发送连接成功事件
    let _ = app_handle.emit("connection-state", ConnectionState {
        connected: true,
        port: Some(params.port),
    });

    Ok(handle)
}
