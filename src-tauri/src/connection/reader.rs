use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::RwLock;
use std::time::Instant;

use tauri::{Emitter, Manager};
use crate::buffer::log_line::{Dir, LogLine};
use crate::buffer::rx_history::RxHistory;
use crate::connection::line_assembler::LineAssembler;
use crate::connection::port_wrapper::PortReader;
use crate::state::AppState;
use crate::util::log_format::{DisplayConfig, format_line_for_file};
use crate::util::time_fmt::now_local_ts;

/// 把积攒的行 flush 出去：emit rx-lines 批量事件给前端 + 送文件日志（若正在记录）。
/// rx-lines 定向到该连接归属的窗口(emit_to 读 window_label RwLock),多窗口下不串流。
/// RwLock 让 GUI 接管 MCP 连接时改 window_label,reader 自动用新值。
///
/// 批量 emit(旧版逐行 rx-line):每行一次 IPC 序列化/跨线程派发,高吞吐下
/// 前端被事件洪泛。批量化后行内容/顺序/时间戳语义不变,前端一次循环入 store。
fn flush_lines(app_handle: &tauri::AppHandle, lines: &mut Vec<LogLine>, rx_history: &RxHistory, window_label: &Arc<RwLock<String>>) {
    if lines.is_empty() {
        return;
    }
    // 一次性从 state 取出文件日志 sender、显示配置（全局共享,不从 connection 取）
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
    // 读当前 window_label(GUI 接管时改 RwLock,下次 flush 自动用新值)
    let label = window_label.read().map(|s| s.clone()).unwrap_or_default();

    let batch: Vec<LogLine> = lines.drain(..).collect();
    let _ = app_handle.emit_to(&label, "rx-lines", &batch);
    if let (Some(c), Some(tx)) = (&cfg, &sender) {
        for line in &batch {
            let formatted = format_line_for_file(line, c, false);
            if !formatted.is_empty() {
                let _ = tx.send(formatted);
            }
        }
    }
    // 批量入历史:一次锁 + 一次 notify_waiters(逐行 push 每行唤醒一次 send_and_read,
    // 每次唤醒都触发一轮 since() 扫描;批量后每批只唤醒一次)。供 MCP 阻塞读/历史查询。
    rx_history.push_many(Dir::Rx, batch);
}

/// 发送 rx-update 事件(定向到该连接归属的窗口)。total 现读现发,
/// 节流补发时也是最新累计值,终值不丢。
fn emit_rx_update(app_handle: &tauri::AppHandle, window_label: &Arc<RwLock<String>>, port: &str, rx_bytes: &Arc<AtomicU64>) {
    let total = rx_bytes.load(std::sync::atomic::Ordering::SeqCst);
    if let Ok(wl) = window_label.read() {
        let _ = app_handle.emit_to(&*wl, "rx-update", crate::connection::RxUpdate { total, port: port.to_string() });
    }
}

/// 把 LineAssembler 中不换行的残尾（如 shell 的 "> " 提示符）补打为一行。
/// 残尾没有换行符，正常切行永远等不到，需在设备输出间歇（读超时）时主动取出。
/// 复用 flush_lines：统一处理前端 emit + 文件日志。
fn flush_pending_tail(assembler: &mut LineAssembler, app_handle: &tauri::AppHandle, pending_lines: &mut Vec<LogLine>, rx_history: &RxHistory, window_label: &Arc<RwLock<String>>, line_index: &Arc<AtomicU64>) {
    let Some(tail) = assembler.flush() else {
        return;
    };
    if tail.is_empty() {
        return;
    }
    let ts = now_local_ts();
    let idx = line_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let line = LogLine::new(ts, Dir::Rx, tail, &[], idx);
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
    window_label: Arc<RwLock<String>>,
    line_index: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // 4KB 读缓冲:高波特率下减少 read 系统调用次数(1KB 在 921600 波特率下
        // ~90 次/秒,3M 波特率更高)。行切分与批量 flush 语义与 chunk 大小无关。
        let mut buf = [0u8; 4096];
        let mut assembler = LineAssembler::new();
        let mut last_emit = Instant::now();
        let mut pending_lines: Vec<LogLine> = Vec::with_capacity(16);
        // rx-update 节流:≥100ms 一个(高波特率下每次 read 都发会冲垮 IPC),
        // 压下的记 pending,读超时(空闲)/线程退出前补发,字节终值不丢。
        let mut rx_update = crate::connection::EventThrottle::new(std::time::Duration::from_millis(100));

        while running.load(std::sync::atomic::Ordering::SeqCst) {
            match port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    rx_bytes.fetch_add(n as u64, std::sync::atomic::Ordering::SeqCst);
                    let lines = assembler.feed(&buf[..n]);
                    // 同一 read 批次内的所有行共用同一时间戳：保持一批输出整体一致，
                    // 也避免同一逻辑输出（如 help 列表）各行时间戳跳动。
                    let ts = now_local_ts();
                    for raw in lines {
                        let idx = line_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        let line = LogLine::new(ts.clone(), Dir::Rx, raw, &[], idx);
                        pending_lines.push(line);
                    }

                    // 批量 emit：攒够 16 行或超过 5ms
                    if pending_lines.len() >= 16 || last_emit.elapsed() > std::time::Duration::from_millis(5) {
                        flush_lines(&app_handle, &mut pending_lines, &rx_history, &window_label);
                        last_emit = Instant::now();
                    }

                    // rx-update 节流发送(定向到该连接归属的窗口)
                    if rx_update.on_activity() {
                        emit_rx_update(&app_handle, &window_label, &port_name, &rx_bytes);
                    }
                }
                Ok(_) => {
                    // 超时：有数据也可能在等下一批，先 flush 已有行
                    flush_lines(&app_handle, &mut pending_lines, &rx_history, &window_label);
                    if rx_update.take_pending() {
                        emit_rx_update(&app_handle, &window_label, &port_name, &rx_bytes);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    // 超时（读超时到）：此刻通常意味着设备输出告一段落，
                    // 把不换行的残尾（如 shell 的 "> " 提示符）也补打出来，
                    // 避免残留行滞留到下一批、带上下一个时间戳。
                    flush_lines(&app_handle, &mut pending_lines, &rx_history, &window_label);
                    flush_pending_tail(&mut assembler, &app_handle, &mut pending_lines, &rx_history, &window_label, &line_index);
                    if rx_update.take_pending() {
                        emit_rx_update(&app_handle, &window_label, &port_name, &rx_bytes);
                    }
                }
                Err(e) => {
                    // 共享模式下锁错误不致命，继续运行
                    if let PortReader::Shared(_) = port {
                        if e.kind() == std::io::ErrorKind::Other {
                            if let Ok(wl) = window_label.read() {
                                let _ = app_handle.emit_to(&*wl, "error", crate::connection::ErrorEvent {
                                    message: format!("读取端口锁错误: {}", e),
                                });
                            }
                            continue;
                        }
                    }
                    if let Ok(wl) = window_label.read() {
                        let _ = app_handle.emit_to(&*wl, "error", crate::connection::ErrorEvent {
                            message: format!("串口读取错误: {}", e),
                        });
                    }
                    running.store(false, std::sync::atomic::Ordering::SeqCst);
                    break;
                }
            }
        }
        // 线程退出前把残余行 flush
        flush_lines(&app_handle, &mut pending_lines, &rx_history, &window_label);
        // 补发 pending 的 rx-update 终值(拔线/断开时计数器定格在准确值)
        if rx_update.take_pending() {
            emit_rx_update(&app_handle, &window_label, &port_name, &rx_bytes);
        }
    })
}
