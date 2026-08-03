use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use crossbeam_channel::Receiver;
use tauri::{Emitter, Manager};

use crate::buffer::log_line::{Dir, LogLine};
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
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
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
                    // 只 write_all，不 flush。flush() 在 USB 转串口下会阻塞等待 USB 批量传输完成，
                    // 单次耗时数十至上百 ms。去掉后由 OS/驱动缓冲区按波特率自动发送。
                    match port.write_all(&data) {
                        Ok(_) => {
                            let total = tx_bytes.fetch_add(data.len() as u64, std::sync::atomic::Ordering::SeqCst) + data.len() as u64;
                            let _ = app_handle.emit("tx-update", crate::connection::TxUpdate { total });

                            if emit_line {
                                // 从 state 读取显示配置和文件日志 sender
                                let (cfg, sender) = match app_handle.try_state::<AppState>() {
                                    Some(state) => {
                                        let cfg = state.settings.lock().ok().map(|s| DisplayConfig {
                                            show_timestamp: s.ui.show_timestamp,
                                            log_send: s.ui.log_send,
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
                                    let line = LogLine::new(now_local_ts(), Dir::Tx, data.clone(), &[]);
                                    let _ = app_handle.emit("tx-line", line.clone());
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
                Err(_) => {
                    // timeout, continue loop to check running
                }
            }
        }
    })
}
