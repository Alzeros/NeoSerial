use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use crossbeam_channel::Receiver;
use tauri::{Emitter, Manager};

use crate::buffer::log_line::{Dir, LogLine};
use crate::connection::WriteCommand;
use crate::connection::port_wrapper::PortWriter;
use crate::state::AppState;
use crate::util::log_format::{DisplayConfig, format_line};
use crate::util::time_fmt::now_local_ts;

/// 启动串口写入线程。从 channel 接收命令并写入端口。
pub fn spawn_writer(
    mut port: PortWriter,
    write_rx: Receiver<WriteCommand>,
    running: Arc<AtomicBool>,
    tx_bytes: Arc<AtomicU64>,
    app_handle: tauri::AppHandle,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while running.load(std::sync::atomic::Ordering::SeqCst) {
            match write_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(WriteCommand::Send(data)) => {
                    match port.write_all(&data).and_then(|_| port.flush()) {
                        Ok(_) => {
                            let total = tx_bytes.fetch_add(data.len() as u64, std::sync::atomic::Ordering::SeqCst) + data.len() as u64;
                            let _ = app_handle.emit("tx-update", crate::connection::TxUpdate { total });

                            // 统一在此 emit tx-line，让手动发送/脚本序列/文件发送三条路径都在日志区可见
                            let line = LogLine::new(now_local_ts(), Dir::Tx, data, &[]);
                            let _ = app_handle.emit("tx-line", line.clone());

                            // 送文件日志（受 ui.log_send 控制）
                            if let Some(state) = app_handle.try_state::<AppState>() {
                                let cfg = state.settings.lock().ok().map(|s| DisplayConfig {
                                    show_timestamp: s.ui.show_timestamp,
                                    log_send: s.ui.log_send,
                                });
                                let sender = state
                                    .line_sender
                                    .lock()
                                    .ok()
                                    .and_then(|ls| ls.as_ref().cloned());
                                if let (Some(c), Some(tx)) = (cfg, sender) {
                                    let formatted = format_line(&line, &c, false);
                                    if !formatted.is_empty() {
                                        let _ = tx.send(formatted);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // 共享模式下锁错误不致命，继续运行
                            if let PortWriter::Shared(_) = port {
                                if e.kind() == std::io::ErrorKind::Other {
                                    let _ = app_handle.emit("error", crate::connection::ErrorEvent {
                                        message: format!("写入端口锁错误: {}", e),
                                    });
                                    continue;
                                }
                            }
                            let _ = app_handle.emit("error", crate::connection::ErrorEvent {
                                message: format!("发送失败: {}", e),
                            });
                        }
                    }
                }
                Ok(WriteCommand::Close) => {
                    break;
                }
                Err(_) => {
                    // timeout, continue loop to check running
                }
            }
        }
    })
}
