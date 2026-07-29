use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use crossbeam_channel::Receiver;
use tauri::Emitter;

use crate::connection::WriteCommand;
use crate::connection::port_wrapper::PortWriter;

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
