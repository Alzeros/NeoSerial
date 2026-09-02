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
    port: PortWriter,
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
        let mut tx_update = crate::connection::EventThrottle::new(std::time::Duration::from_millis(100));
        while running.load(std::sync::atomic::Ordering::SeqCst) {
            match write_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(cmd) => {
                    let (data, emit_line) = match cmd {
                        WriteCommand::Send(data) => (data, true),
                        WriteCommand::SendSilent(data) => (data, false),
                        WriteCommand::Close => break,
                    };
                    // 一条命令一次完整写入,不合并积压命令。合并的问题:send_file 以内存速度
                    // 塞满 64 格 channel,合并后单次 write 几十 KB,线速下要跑几秒,必撞超时;
                    // Send/SendSilent 混在一起却只按第一条决定是否回显(手动命令重复回显、
                    // 文件块不进日志)。单次 write 的短写由 write_all 补写。
                    let data_len = data.len();
                    let (written, write_err) = write_all(&port, &data);
                    if written > 0 {
                        let _ = tx_bytes.fetch_add(written as u64, std::sync::atomic::Ordering::SeqCst);
                        if tx_update.on_activity() {
                            let total = tx_bytes.load(std::sync::atomic::Ordering::SeqCst);
                            let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                            let _ = app_handle.emit_to(&wl, "tx-update", crate::connection::TxUpdate { total, port: port_name.clone() });
                        }
                    }
                    match write_err {
                        None => {
                            if emit_line {
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
                                    let line = LogLine::new(now_local_ts(), Dir::Tx, data, &[], idx);
                                    let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                                    let _ = app_handle.emit_to(&wl, "tx-line", line.clone());
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
                        Some(e) => {
                            // 部分写出后失败也报出去:界面/日志上这条命令(SendSilent 已乐观回显)
                            // 看起来是发了,实际对端只收到前 written 字节
                            let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                            let _ = app_handle.emit_to(&wl, "error", crate::connection::ErrorEvent {
                                message: format!("发送失败: {} (已写出 {}/{} 字节)", e, written, data_len),
                            });
                        }
                    }
                }
                Err(_) => {
                    if tx_update.take_pending() {
                        let total = tx_bytes.load(std::sync::atomic::Ordering::SeqCst);
                        let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                        let _ = app_handle.emit_to(&wl, "tx-update", crate::connection::TxUpdate { total, port: port_name.clone() });
                    }
                }
            }
        }
        if tx_update.take_pending() {
            let total = tx_bytes.load(std::sync::atomic::Ordering::SeqCst);
            let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
            let _ = app_handle.emit_to(&wl, "tx-update", crate::connection::TxUpdate { total, port: port_name.clone() });
        }
    })
}

/// 把 data 全部写进端口。单次 write 可能短写——overlapped 超时取消前只出去了一部分,
/// 或 serialport 的 write 只接受了一部分——循环补写余下,直到写完或出错。
/// 返回 (已写出字节数, 首个错误)。一次进展为 0 视为失败,避免设备卡死时空转。
fn write_all(port: &PortWriter, data: &[u8]) -> (usize, Option<std::io::Error>) {
    let mut written = 0usize;
    while written < data.len() {
        match port.write_data(&data[written..]) {
            Ok(0) => {
                return (written, Some(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "端口未接受任何字节",
                )));
            }
            Ok(n) => written += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return (written, Some(e)),
        }
    }
    (written, None)
}
