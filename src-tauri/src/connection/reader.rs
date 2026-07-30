use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Instant;

use tauri::{Emitter, Manager};
use crate::buffer::log_line::{Dir, LogLine};
use crate::connection::line_assembler::LineAssembler;
use crate::connection::port_wrapper::PortReader;
use crate::state::AppState;
use crate::util::log_format::{DisplayConfig, format_line};
use crate::util::time_fmt::now_local_ts;

/// 把积攒的行 flush 出去：emit rx-line 给前端 + 送文件日志（若正在记录）。
fn flush_lines(app_handle: &tauri::AppHandle, lines: &mut Vec<LogLine>) {
    if lines.is_empty() {
        return;
    }
    // 一次性从 state 取出文件日志 sender 和显示配置
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

    for line in lines.drain(..) {
        let _ = app_handle.emit("rx-line", line.clone());
        if let (Some(c), Some(tx)) = (&cfg, &sender) {
            let formatted = format_line(&line, c, false);
            if !formatted.is_empty() {
                let _ = tx.send(formatted);
            }
        }
    }
}

/// 启动串口读取线程。
pub fn spawn_reader(
    mut port: PortReader,
    running: Arc<AtomicBool>,
    rx_bytes: Arc<AtomicU64>,
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
                        flush_lines(&app_handle, &mut pending_lines);
                        last_emit = Instant::now();
                    }

                    // 定期发送 rx-update 事件
                    let total = rx_bytes.load(std::sync::atomic::Ordering::SeqCst);
                    let _ = app_handle.emit("rx-update", crate::connection::RxUpdate { total });
                }
                Ok(_) => {
                    // 超时，继续循环检查 running
                    flush_lines(&app_handle, &mut pending_lines);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    flush_lines(&app_handle, &mut pending_lines);
                }
                Err(e) => {
                    // 共享模式下锁错误不致命，继续运行
                    if let PortReader::Shared(_) = port {
                        if e.kind() == std::io::ErrorKind::Other {
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
        // 线程退出前把残余行 flush
        flush_lines(&app_handle, &mut pending_lines);
    })
}
