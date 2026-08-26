use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::RwLock;

use crossbeam_channel::Receiver;
use tauri::{Emitter, Manager};

use crate::buffer::log_line::{Dir, LogLine};
use crate::buffer::rx_history::RxHistory;
use crate::connection::WriteCommand;
use crate::connection::port_wrapper::PortWriter;
use crate::state::AppState;
use crate::util::log_format::{DisplayConfig, format_line_for_file};
use crate::util::time_fmt::now_local_ts;

/// 启动串口写入线程。从 channel 接收命令并写入端口。
pub fn spawn_writer(
    mut port: PortWriter,
    write_rx: Receiver<WriteCommand>,
    running: Arc<AtomicBool>,
    tx_bytes: Arc<AtomicU64>,
    app_handle: tauri::AppHandle,
    rx_history: Arc<RxHistory>,
    port_name: String,
    window_label: Arc<RwLock<String>>,
    line_index: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // tx-update 节流:手动发送频率远低于 10Hz,行为不变(立即发);
        // 文件发送/序列爆发时 ≥100ms 一个,空闲(100ms 无写)与线程退出前补发,终值不丢。
        let mut tx_update = crate::connection::EventThrottle::new(std::time::Duration::from_millis(100));
        while running.load(std::sync::atomic::Ordering::SeqCst) {
            match write_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(cmd) => {
                    // Send：写 + emit tx-line（手动/文件发送）
                    // SendSilent：只写，不 emit（脚本序列已自行 emit，避免重复 + 不被写阻塞拖慢发起节奏）
                    let (data, emit_line) = match cmd {
                        WriteCommand::Send(data) => (data, true),
                        WriteCommand::SendSilent(data) => (data, false),
                        WriteCommand::Close => break,
                    };
                    // write_all：把数据交给驱动缓冲区即返回，不主动 flush。
                    // flush()（Windows FlushFileBuffers）会等待所有数据物理发送完毕；
                    // 对端设备未及时接收或 USB 转串口驱动等待确认时，flush 会阻塞数百毫秒
                    // 甚至数十秒（实测 14s），导致快速连续发送被拖成几秒一条。
                    // 数据进入驱动缓冲区后硬件会立即开始发送，小数据包无需 flush 也能及时到达。
                    match port.write_all(&data) {
                        Ok(_) => {
                            let _ = tx_bytes.fetch_add(data.len() as u64, std::sync::atomic::Ordering::SeqCst);
                            // tx-update 节流发送(定向到该连接归属的窗口)
                            if tx_update.on_activity() {
                                let total = tx_bytes.load(std::sync::atomic::Ordering::SeqCst);
                                let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                                let _ = app_handle.emit_to(&wl, "tx-update", crate::connection::TxUpdate { total, port: port_name.clone() });
                            }

                            if emit_line {
                                // 从 state 读取显示配置和文件日志 sender
                                let (cfg, sender) = match app_handle.try_state::<AppState>() {
                                    Some(state) => {
                                        let cfg = state.settings.lock().ok().map(|s| DisplayConfig {
                                            show_timestamp: s.ui.show_timestamp,
                                            log_send: s.ui.log_send,
                                            dir_label: if s.ui.log_dir_label == "full" {
                                                crate::util::log_format::DirLabel::Full
                                            } else {
                                                crate::util::log_format::DirLabel::Short
                                            },
                                        });
                                        let sender = state
                                            .line_sender
                                            .lock()
                                            .ok()
                                            .and_then(|ls| ls.as_ref().cloned());
                                        (cfg, sender)
                                    }
                                    None => (None, None),
                                };
                                let log_send = cfg.map(|c| c.log_send).unwrap_or(true);
                                if log_send {
                                    let idx = line_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                                    let line = LogLine::new(now_local_ts(), Dir::Tx, data.clone(), &[], idx);
                                    let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                                    let _ = app_handle.emit_to(&wl, "tx-line", line.clone());
                                    // 喂 per-connection 收发历史(tx)。Send 分支(send_file)不经 emit_tx_line,
                                    // 这里补 push,让 get_history_since 也能看到文件发送的 Tx。
                                    // SendSilent 不走这(其 Tx 由 emit_tx_line 已 push)。
                                    rx_history.push(Dir::Tx, line.clone());
                                    if let (Some(c), Some(tx)) = (cfg, sender) {
                                        let formatted = format_line_for_file(&line, &c, false);
                                        if !formatted.is_empty() {
                                            let _ = tx.send(formatted);
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // 共享模式下锁错误不致命，继续运行
                            if let PortWriter::Shared(_) = port {
                                if e.kind() == std::io::ErrorKind::Other {
                                    if let Ok(wl) = window_label.read() {
                                        let _ = app_handle.emit_to(&*wl, "error", crate::connection::ErrorEvent {
                                            message: format!("写入端口锁错误: {}", e),
                                        });
                                    }
                                    continue;
                                }
                            }
                            if let Ok(wl) = window_label.read() {
                                let _ = app_handle.emit_to(&*wl, "error", crate::connection::ErrorEvent {
                                    message: format!("发送失败: {}", e),
                                });
                            }
                        }
                    }
                }
                Err(_) => {
                    // timeout:补发被节流压下的 tx-update(空闲 100ms,终值不丢)
                    if tx_update.take_pending() {
                        let total = tx_bytes.load(std::sync::atomic::Ordering::SeqCst);
                        if let Ok(wl) = window_label.read() {
                            let _ = app_handle.emit_to(&*wl, "tx-update", crate::connection::TxUpdate { total, port: port_name.clone() });
                        }
                    }
                }
            }
        }
        // 退出前补发 pending 的 tx-update 终值(断开时计数器定格在准确值)
        if tx_update.take_pending() {
            let total = tx_bytes.load(std::sync::atomic::Ordering::SeqCst);
            if let Ok(wl) = window_label.read() {
                let _ = app_handle.emit_to(&*wl, "tx-update", crate::connection::TxUpdate { total, port: port_name.clone() });
            }
        }
    })
}
