use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::time::Instant;

use tauri::{Emitter, Manager};
use crate::buffer::log_line::{Dir, LogLine};
use crate::buffer::rx_history::RxHistory;
use crate::connection::line_assembler::LineAssembler;
use crate::connection::port_wrapper::PortReader;
use crate::state::AppState;
use crate::util::log_format::{DisplayConfig, format_line_for_file};
use crate::util::time_fmt::now_local_ts;

/// 把积攒的行 flush 出去：emit rx-line 给前端 + 送文件日志（若正在记录）。
/// rx-line 定向到该连接归属的窗口(emit_to window_label),多窗口下不串流。
fn flush_lines(app_handle: &tauri::AppHandle, lines: &mut Vec<LogLine>, rx_history: &RxHistory, window_label: &str) {
    if lines.is_empty() {
        return;
    }
    // 一次性从 state 取出文件日志 sender、显示配置（全局共享,不从 connection 取）
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
        let _ = app_handle.emit_to(window_label, "rx-line", line.clone());
        // 喂后端收发历史(rx),供 MCP 阻塞读/历史查询。push 触发 Notify 唤醒等待的 send_and_read。
        rx_history.push(crate::buffer::log_line::Dir::Rx, line.clone());
        if let (Some(c), Some(tx)) = (&cfg, &sender) {
            let formatted = format_line_for_file(&line, c, false);
            if !formatted.is_empty() {
                let _ = tx.send(formatted);
            }
        }
    }
}

/// 把 LineAssembler 中不换行的残尾（如 shell 的 "> " 提示符）补打为一行。
/// 残尾没有换行符，正常切行永远等不到，需在设备输出间歇（读超时）时主动取出。
/// 复用 flush_lines：统一处理前端 emit + 文件日志。
fn flush_pending_tail(assembler: &mut LineAssembler, app_handle: &tauri::AppHandle, pending_lines: &mut Vec<LogLine>, rx_history: &RxHistory, window_label: &str) {
    let Some(tail) = assembler.flush() else {
        return;
    };
    if tail.is_empty() {
        return;
    }
    let ts = now_local_ts();
    let line = LogLine::new(ts, Dir::Rx, tail, &[]);
    pending_lines.push(line);
    flush_lines(app_handle, pending_lines, rx_history, window_label);
}

/// 启动串口读取线程。
pub fn spawn_reader(
    mut port: PortReader,
    running: Arc<AtomicBool>,
    rx_bytes: Arc<AtomicU64>,
    app_handle: tauri::AppHandle,
    rx_history: Arc<RxHistory>,
    port_name: String,
    window_label: String,
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
                    // 同一 read 批次内的所有行共用同一时间戳：保持一批输出整体一致，
                    // 也避免同一逻辑输出（如 help 列表）各行时间戳跳动。
                    let ts = now_local_ts();
                    for raw in lines {
                        let line = LogLine::new(ts.clone(), Dir::Rx, raw, &[]);
                        pending_lines.push(line);
                    }

                    // 批量 emit：攒够 16 行或超过 5ms
                    if pending_lines.len() >= 16 || last_emit.elapsed() > std::time::Duration::from_millis(5) {
                        flush_lines(&app_handle, &mut pending_lines, &rx_history, &window_label);
                        last_emit = Instant::now();
                    }

                    // 定期发送 rx-update 事件(定向到该连接归属的窗口)
                    let total = rx_bytes.load(std::sync::atomic::Ordering::SeqCst);
                    let _ = app_handle.emit_to(&window_label, "rx-update", crate::connection::RxUpdate { total, port: port_name.clone() });
                }
                Ok(_) => {
                    // 超时：有数据也可能在等下一批，先 flush 已有行
                    flush_lines(&app_handle, &mut pending_lines, &rx_history, &window_label);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    // 超时（读超时到）：此刻通常意味着设备输出告一段落，
                    // 把不换行的残尾（如 shell 的 "> " 提示符）也补打出来，
                    // 避免残留行滞留到下一批、带上下一个时间戳。
                    flush_lines(&app_handle, &mut pending_lines, &rx_history, &window_label);
                    flush_pending_tail(&mut assembler, &app_handle, &mut pending_lines, &rx_history, &window_label);
                }
                Err(e) => {
                    // 共享模式下锁错误不致命，继续运行
                    if let PortReader::Shared(_) = port {
                        if e.kind() == std::io::ErrorKind::Other {
                            let _ = app_handle.emit_to(&window_label, "error", crate::connection::ErrorEvent {
                                message: format!("读取端口锁错误: {}", e),
                            });
                            continue;
                        }
                    }
                    let _ = app_handle.emit_to(&window_label, "error", crate::connection::ErrorEvent {
                        message: format!("串口读取错误: {}", e),
                    });
                    running.store(false, std::sync::atomic::Ordering::SeqCst);
                    break;
                }
            }
        }
        // 线程退出前把残余行 flush
        flush_lines(&app_handle, &mut pending_lines, &rx_history, &window_label);
    })
}
