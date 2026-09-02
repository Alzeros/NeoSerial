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
                    // 合并积压命令
                    let mut all_data = data.clone();
                    let mut pending_close = false;
                    loop {
                        match write_rx.try_recv() {
                            Ok(WriteCommand::Send(d)) => all_data.extend_from_slice(&d),
                            Ok(WriteCommand::SendSilent(d)) => all_data.extend_from_slice(&d),
                            Ok(WriteCommand::Close) => { pending_close = true; break; }
                            Err(_) => break,
                        }
                    }

                    let data_len = all_data.len();
                    let data_for_emit = all_data.clone();

                    // write_data: Windows 上用 overlapped I/O（不阻塞），其他平台同步 write
                    match port.write_data(&all_data) {
                        Ok(n) => {
                            if n < data_len {
                                log::warn!("[Writer] 部分写入: {}/{} 字节", n, data_len);
                            }
                            let _ = tx_bytes.fetch_add(n as u64, std::sync::atomic::Ordering::SeqCst);
                            if tx_update.on_activity() {
                                let total = tx_bytes.load(std::sync::atomic::Ordering::SeqCst);
                                let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                                let _ = app_handle.emit_to(&wl, "tx-update", crate::connection::TxUpdate { total, port: port_name.clone() });
                            }
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
                                    let line = LogLine::new(now_local_ts(), Dir::Tx, data_for_emit.clone(), &[], idx);
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
                        Err(e) => {
                            let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                            let _ = app_handle.emit_to(&wl, "error", crate::connection::ErrorEvent {
                                message: format!("发送失败: {}", e),
                            });
                        }
                    }
                    if pending_close { break; }
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
