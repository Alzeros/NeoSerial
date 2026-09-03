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
        let mut tx_update = crate::connection::EventThrottle::new(std::time::Duration::from_millis(100));
        while running.load(std::sync::atomic::Ordering::SeqCst) {
            match write_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(cmd) => {
                    let (data, emit_line, tracker) = match cmd {
                        WriteCommand::FileChunk(data, tracker) => (data, true, Some(tracker)),
                        WriteCommand::SendSilent(data) => (data, false, None),
                        WriteCommand::Close => break,
                    };
                    // 同一文件前面已有块写失败:后面的块写出去对端也只是收到一个有洞的文件,
                    // 直接丢弃,也免得队列里积压的几十块逐块弹"发送失败"。
                    if tracker.as_ref().is_some_and(|t| t.failed()) {
                        continue;
                    }
                    // 一条命令一次完整写入,不合并积压命令。合并的问题:send_file 以内存速度
                    // 塞满 64 格 channel,合并后单次 write 几十 KB,线速下要跑几秒,必撞超时;
                    // FileChunk/SendSilent 混在一起却只按第一条决定是否回显(手动命令重复回显、
                    // 文件块不进日志)。单次 write 的短写由 write_all 补写。
                    let data_len = data.len();
                    let (written, write_err) = write_all(&mut port, &data, &running);
                    if let Some(t) = &tracker {
                        t.record(written, write_err.as_ref().map(|e| e.to_string()).as_deref());
                    }
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

/// 写超时且一个字节都没出去,累计多久才算设备真卡住。
/// USB 转串口驱动的发送缓冲可达数 MB,排空期间接收新写入的粒度可能是秒级
/// (COM44 实测:连发几个 1.5MB 文件后,1KB 块出现过 >0.7s 一字节不收、随后又恢复的停顿),
/// 单次超时(线速 ×2 + 500ms)就放弃会让大文件半途而废。零进展持续超过此值才报错。
const WRITE_STALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(3);

/// 把 data 全部写进端口。单次 write 可能短写——overlapped 超时取消前只出去了一部分,
/// 或 serialport 的 write 只接受了一部分——循环补写余下,直到写完或出错。
/// 超时且零进展的写会重试(见 [`WRITE_STALL_LIMIT`]),连接已在断开(running=false)则不再重试。
/// 返回 (已写出字节数, 首个错误)。
fn write_all(port: &mut PortWriter, data: &[u8], running: &AtomicBool) -> (usize, Option<std::io::Error>) {
    write_all_with(|d| port.write_data(d), data, running, WRITE_STALL_LIMIT)
}

/// [`write_all`] 的可测试内核:`write` 抽象掉端口。
fn write_all_with(
    mut write: impl FnMut(&[u8]) -> std::io::Result<usize>,
    data: &[u8],
    running: &AtomicBool,
    stall_limit: std::time::Duration,
) -> (usize, Option<std::io::Error>) {
    let mut written = 0usize;
    // 第一次零进展超时的起点(取该次 write 发起时刻,这次等掉的时间也算进停顿)
    let mut stall_start: Option<std::time::Instant> = None;
    while written < data.len() {
        let attempt = std::time::Instant::now();
        match write(&data[written..]) {
            Ok(0) => {
                return (written, Some(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "端口未接受任何字节",
                )));
            }
            Ok(n) => {
                written += n;
                stall_start = None;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                let since = *stall_start.get_or_insert(attempt);
                if since.elapsed() >= stall_limit || !running.load(std::sync::atomic::Ordering::SeqCst) {
                    return (written, Some(e));
                }
            }
            Err(e) => return (written, Some(e)),
        }
    }
    (written, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};
    use std::time::Duration;

    fn timed_out() -> Error {
        Error::new(ErrorKind::TimedOut, "write 超时")
    }

    #[test]
    fn test_transient_timeouts_are_retried_until_the_write_goes_through() {
        let running = AtomicBool::new(true);
        let mut calls = 0;
        let (written, err) = write_all_with(
            |d| {
                calls += 1;
                if calls <= 2 { Err(timed_out()) } else { Ok(d.len()) }
            },
            &[0u8; 1024],
            &running,
            Duration::from_secs(3),
        );
        assert_eq!((written, err.is_none()), (1024, true));
        assert_eq!(calls, 3, "两次超时后第三次写成功");
    }

    #[test]
    fn test_stall_longer_than_limit_is_an_error() {
        let running = AtomicBool::new(true);
        let mut calls = 0;
        let (written, err) = write_all_with(
            |_| {
                calls += 1;
                std::thread::sleep(Duration::from_millis(30));
                Err(timed_out())
            },
            &[0u8; 1024],
            &running,
            Duration::from_millis(100),
        );
        assert_eq!(written, 0);
        assert_eq!(err.map(|e| e.kind()), Some(ErrorKind::TimedOut));
        assert!((3..=6).contains(&calls), "约 100ms/30ms ≈ 4 次尝试后放弃,实际 {} 次", calls);
    }

    #[test]
    fn test_progress_resets_the_stall_clock() {
        // 每次超时前先短写 1 字节:有进展就不算卡住,哪怕总耗时远超 limit
        let running = AtomicBool::new(true);
        let mut calls = 0;
        let (written, err) = write_all_with(
            |_| {
                calls += 1;
                std::thread::sleep(Duration::from_millis(15));
                if calls % 2 == 1 { Ok(1) } else { Err(timed_out()) }
            },
            &[0u8; 8],
            &running,
            Duration::from_millis(40),
        );
        assert_eq!((written, err.is_none()), (8, true));
    }

    #[test]
    fn test_timeout_while_disconnecting_does_not_retry() {
        let running = AtomicBool::new(false);
        let mut calls = 0;
        let (written, err) = write_all_with(
            |_| { calls += 1; Err(timed_out()) },
            &[0u8; 1024],
            &running,
            Duration::from_secs(3),
        );
        assert_eq!((written, calls), (0, 1));
        assert_eq!(err.map(|e| e.kind()), Some(ErrorKind::TimedOut));
    }

    #[test]
    fn test_short_writes_are_completed_and_other_errors_abort() {
        let running = AtomicBool::new(true);
        let mut calls = 0;
        let (written, err) = write_all_with(
            |d| { calls += 1; Ok(d.len().min(300)) },
            &[0u8; 1000],
            &running,
            Duration::from_secs(3),
        );
        assert_eq!((written, err.is_none(), calls), (1000, true, 4));

        let (written, err) = write_all_with(
            |_| Err(Error::new(ErrorKind::BrokenPipe, "设备已拔出")),
            &[0u8; 10],
            &running,
            Duration::from_secs(3),
        );
        assert_eq!(written, 0);
        assert_eq!(err.map(|e| e.kind()), Some(ErrorKind::BrokenPipe));
    }
}
