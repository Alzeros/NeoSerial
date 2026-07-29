use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Instant;

use tauri::Emitter;
use crate::buffer::log_line::{Dir, LogLine};
use crate::connection::line_assembler::LineAssembler;
use crate::connection::port_wrapper::PortReader;
use crate::util::time_fmt::now_local_ts;

/// 启动串口读取线程。
pub fn spawn_reader(
    mut port: PortReader,
    running: Arc<AtomicBool>,
    rx_bytes: Arc<AtomicU64>,
    ring_tx: std::sync::mpsc::Sender<LogLine>,
    app_handle: tauri::AppHandle,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let mut assembler = LineAssembler::new();
        let mut last_emit = Instant::now();
        let mut pending_lines: Vec<LogLine> = Vec::with_capacity(16);

        while running.load(std::sync::atomic::Ordering::SeqCst) {
            match port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    rx_bytes.fetch_add(n as u64, std::sync::atomic::Ordering::SeqCst);
                    let lines = assembler.feed(&buf[..n]);
                    for raw in lines {
                        let ts = now_local_ts();
                        let line = LogLine::new(ts, Dir::Rx, raw, &[]);
                        pending_lines.push(line);
                    }

                    // 批量 emit：攒够 16 行或超过 5ms
                    if pending_lines.len() >= 16 || last_emit.elapsed() > std::time::Duration::from_millis(5) {
                        for line in pending_lines.drain(..) {
                            let _ = ring_tx.send(line.clone());
                            let _ = app_handle.emit("rx-line", line);
                        }
                        last_emit = Instant::now();
                    }

                    // 定期发送 rx-update 事件
                    let total = rx_bytes.load(std::sync::atomic::Ordering::SeqCst);
                    let _ = app_handle.emit("rx-update", crate::connection::RxUpdate { total });
                }
                Ok(_) => {
                    // 超时，继续循环检查 running
                    if !pending_lines.is_empty() {
                        for line in pending_lines.drain(..) {
                            let _ = ring_tx.send(line.clone());
                            let _ = app_handle.emit("rx-line", line);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    // 超时，继续循环
                    if !pending_lines.is_empty() {
                        for line in pending_lines.drain(..) {
                            let _ = ring_tx.send(line.clone());
                            let _ = app_handle.emit("rx-line", line);
                        }
                    }
                }
                Err(e) => {
                    // 共享模式下锁错误不致命，继续运行
                    if let PortReader::Shared(_) = port {
                        if e.kind() == std::io::ErrorKind::Other {
                            // 锁错误，记录但继续
                            let _ = app_handle.emit("error", crate::connection::ErrorEvent {
                                message: format!("读取端口锁错误: {}", e),
                            });
                            continue;
                        }
                    }
                    let _ = app_handle.emit("error", crate::connection::ErrorEvent {
                        message: format!("串口读取错误: {}", e),
                    });
                    running.store(false, std::sync::atomic::Ordering::SeqCst);
                    break;
                }
            }
        }
    })
}
