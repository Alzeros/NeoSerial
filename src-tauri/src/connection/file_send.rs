//! 文件发送:切块入队,进度与"发完"都以写线程**实际写进驱动**的字节为准。
//!
//! 旧实现把块塞进 64 格队列就算 sent,塞完即返回——"100%"时最多还有 64KB 没出 PC
//! (115200 下约 5.7s,9600 下一分多钟),前端以为发完了对端还在收。现在每块带一个
//! [`FileSendTracker`],写线程写完一块记一块,这里据此报进度并等最后一块写完才返回。
//! GUI command(commands/send.rs)与 MCP send_file 工具共用此实现。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{SendTimeoutError, Sender};

use super::WriteCommand;

/// 每块字节数。一块一次 write,块太大线速下单次 write 要跑几秒会撞写超时,
/// 太小则每块一条 Tx 日志行刷得太密。
pub const CHUNK_SIZE: usize = 1024;
/// 进度回调的最小间隔。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// 入队全部完成后等写线程排空队列的轮询间隔。
const DRAIN_POLL: Duration = Duration::from_millis(20);

/// 一次文件发送的写出跟踪,发送方与写线程各持一个 Arc。
/// - `written`:写线程每块实际写出的字节累加(短写/失败时小于块长)。
/// - `failed`/`error`:任一块写错即置位并记下第一个错误;之后写线程把同一文件
///   后续块直接丢弃(对端只会收到一个有洞的文件,还不如干净地停),发送方停止入队。
/// - `cancelled`:本次发送被取消。**与连接级 `running` 是两回事**——
///   取消只停这一次文件发送,不会把整个串口置为断开。
#[derive(Default)]
pub struct FileSendTracker {
    written: AtomicUsize,
    failed: AtomicBool,
    cancelled: AtomicBool,
    error: Mutex<Option<String>>,
}

impl FileSendTracker {
    pub fn written(&self) -> usize {
        self.written.load(Ordering::SeqCst)
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }

    /// 本次发送是否已被取消。
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// 标记取消。发送方观察到取消信号后调用:写线程据此把**还没写出的**排队块直接丢弃,
    /// 不再往串口里送。已经写出去字节不撤回(设备可能已经收到半个文件)。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// 第一个写错误的描述(failed 为 true 时有值)。
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }

    /// 写线程每处理完一块调一次。err 为 Some 表示这一块写错(written 是出错前写出的部分)。
    /// 先记错误再置 failed,读到 failed 的一方一定能读到错误文本。
    pub fn record(&self, written: usize, err: Option<&str>) {
        self.written.fetch_add(written, Ordering::SeqCst);
        if let Some(e) = err {
            if let Ok(mut slot) = self.error.lock() {
                if slot.is_none() {
                    *slot = Some(e.to_string());
                }
            }
            self.failed.store(true, Ordering::SeqCst);
        }
    }
}

/// 一次文件发送的结果。
#[derive(Debug, Clone, Copy)]
pub struct FileSendOutcome {
    /// 实际写进驱动的字节数(取消时是**下界**/取消瞬间快照:在途的最后一块可能
    /// 随后才 record,最终实际写出至多比该值多一个写入块)。
    pub written: usize,
    /// 文件总字节数
    pub total: usize,
    /// 是否被取消
    pub cancelled: bool,
}

/// 把 `path` 切成 [`CHUNK_SIZE`] 的块经 `write_tx` 入队,阻塞到写线程把最后一块写进驱动,
/// 返回 Ok(总字节数)。`on_progress(已写出, 总大小)` 至多每 100ms 调一次,成功时最后一次
/// 必为 `(total, total)`;失败时不再回调(前端按失败态处理)。
///
/// 失败返回中文原因,均带"已写出 x/y 字节":
/// - 某块写错(超时、设备拔掉):写线程置 failed,这里停止入队并带上写线程记下的错误;
/// - 连接断开(disconnect / reader 报错退出):`running` 变 false,或队列接收端已随写线程消失。
///
/// 空文件直接返回 Ok(0),不碰队列。
pub fn send_file_tracked(
    write_tx: &Sender<WriteCommand>,
    running: &AtomicBool,
    path: &str,
    on_progress: impl FnMut(usize, usize),
) -> Result<usize, String> {
    send_file_tracked_with_cancel(write_tx, running, path, None, on_progress).map(|o| o.written)
}

/// 同 [`send_file_tracked`],但可以被取消。
///
/// `ct` 是取消令牌(客户端取消/断开时 rmcp 会 cancel 它)。取消后:
/// - 不再入队新的块;
/// - 已经排队但还没写出的块被写线程**丢弃**(写线程看 tracker 的 cancelled 标志);
/// - 正在执行的那一次 OS write 不撤回(设备可能已经收到半个文件),只能尽快收尾;
/// - 返回 `FileSendOutcome { written: 已写出的字节, total: 文件总长, cancelled: true }`。
///
/// **不会**自动从文件头重发:取消后要不要重发由调用方决定。
pub fn send_file_tracked_with_cancel(
    write_tx: &Sender<WriteCommand>,
    running: &AtomicBool,
    path: &str,
    ct: Option<&tokio_util::sync::CancellationToken>,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<FileSendOutcome, String> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let total = file.metadata().map_err(|e| e.to_string())?.len() as usize;
    if total == 0 {
        return Ok(FileSendOutcome { written: 0, total: 0, cancelled: false });
    }

    let tracker = Arc::new(FileSendTracker::default());
    let mut reader = std::io::BufReader::new(file);
    let mut buf = [0u8; CHUNK_SIZE];
    let mut last_progress = Instant::now();

    loop {
        // 读下一块之前先看取消:别再往队列里塞数据
        if cancelled(ct, &tracker) {
            tracker.cancel();
            return Ok(cancelled_outcome(&tracker, total));
        }
        let n = reader.read(&mut buf).map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        let mut cmd = WriteCommand::FileChunk(buf[..n].to_vec(), tracker.clone());
        // 队列满时 send 会阻塞,用带超时的 send 在阻塞期间照常报进度、及时发现写失败/断开
        // (低波特率下一块要写一秒以上,纯阻塞 send 会让进度条卡住)。
        loop {
            if tracker.failed() {
                return Err(failure_message(&tracker, total));
            }
            // 队列满、正在等 send_timeout 的这段时间里也可能被取消
            if cancelled(ct, &tracker) {
                tracker.cancel();
                return Ok(cancelled_outcome(&tracker, total));
            }
            match write_tx.send_timeout(cmd, PROGRESS_INTERVAL) {
                Ok(()) => break,
                Err(SendTimeoutError::Timeout(back)) => {
                    cmd = back;
                    if !running.load(Ordering::SeqCst) {
                        return Err(disconnected_message(&tracker, total));
                    }
                    if cancelled(ct, &tracker) {
                        tracker.cancel();
                        return Ok(cancelled_outcome(&tracker, total));
                    }
                    report_progress(&mut last_progress, &tracker, total, &mut on_progress);
                }
                Err(SendTimeoutError::Disconnected(_)) => {
                    return Err(disconnected_message(&tracker, total));
                }
            }
        }
        report_progress(&mut last_progress, &tracker, total, &mut on_progress);
    }

    // 全部入队 ≠ 全部写出:队列里最多还压着 64 块,等写线程真正写完。
    loop {
        let written = tracker.written();
        if written >= total {
            on_progress(total, total);
            return Ok(FileSendOutcome { written: total, total, cancelled: false });
        }
        if tracker.failed() {
            return Err(failure_message(&tracker, total));
        }
        if !running.load(Ordering::SeqCst) {
            return Err(disconnected_message(&tracker, total));
        }
        // 排空阶段也可能被取消:已排队的块由写线程丢弃,这里返回部分进度
        if cancelled(ct, &tracker) {
            tracker.cancel();
            return Ok(cancelled_outcome(&tracker, total));
        }
        report_progress(&mut last_progress, &tracker, total, &mut on_progress);
        std::thread::sleep(DRAIN_POLL);
    }
}

/// 取消信号:外部令牌或 tracker 自身的取消标志。
fn cancelled(ct: Option<&tokio_util::sync::CancellationToken>, tracker: &FileSendTracker) -> bool {
    ct.is_some_and(|c| c.is_cancelled()) || tracker.cancelled()
}

/// 取消后的结果:已写出的字节数(不会超过文件总长)。
fn cancelled_outcome(tracker: &FileSendTracker, total: usize) -> FileSendOutcome {
    FileSendOutcome {
        written: tracker.written().min(total),
        total,
        cancelled: true,
    }
}

fn report_progress(
    last: &mut Instant,
    tracker: &FileSendTracker,
    total: usize,
    on_progress: &mut impl FnMut(usize, usize),
) {
    if last.elapsed() >= PROGRESS_INTERVAL {
        *last = Instant::now();
        on_progress(tracker.written().min(total), total);
    }
}

fn failure_message(tracker: &FileSendTracker, total: usize) -> String {
    format!(
        "发送失败: {} (已写出 {}/{} 字节)",
        tracker.error().unwrap_or_else(|| "写入出错".to_string()),
        tracker.written().min(total),
        total
    )
}

fn disconnected_message(tracker: &FileSendTracker, total: usize) -> String {
    format!("连接已断开 (已写出 {}/{} 字节)", tracker.written().min(total), total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;
    use std::sync::atomic::AtomicBool;

    fn temp_file(name: &str, len: usize) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("neoserial_fs_{}_{}", std::process::id(), name));
        std::fs::write(&p, vec![0xA5u8; len]).unwrap();
        p
    }

    /// 模拟写线程:按块 record;`fail_at` 指定第几块(1-based)写错;`stop_at` 指定收满几块后
    /// 退出(丢掉接收端,模拟断开)。返回 (收到的块数, 实际写出的块数)。
    ///
    /// 与真实写线程一致:失败或已取消的块直接丢弃,不写也不计入 written。
    fn fake_writer(
        rx: crossbeam_channel::Receiver<WriteCommand>,
        running: Arc<AtomicBool>,
        fail_at: Option<usize>,
        stop_at: Option<usize>,
        per_chunk: Duration,
    ) -> std::thread::JoinHandle<(usize, usize)> {
        std::thread::spawn(move || {
            let mut n = 0usize;
            let mut written = 0usize;
            while let Ok(cmd) = rx.recv() {
                let WriteCommand::FileChunk(data, tracker) = cmd else { continue };
                n += 1;
                if tracker.failed() {
                    continue; // 与真实写线程一致:失败后的块直接丢弃
                }
                if tracker.cancelled() {
                    continue; // 与真实写线程一致:取消后排队中的块直接丢弃
                }
                std::thread::sleep(per_chunk);
                if fail_at == Some(n) {
                    tracker.record(data.len() / 2, Some("写超时"));
                } else {
                    tracker.record(data.len(), None);
                    written += 1;
                }
                if stop_at == Some(n) {
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
            (n, written)
        })
    }

    #[test]
    fn test_tracker_accumulates_and_keeps_first_error() {
        let t = FileSendTracker::default();
        assert_eq!(t.written(), 0);
        assert!(!t.failed());
        assert_eq!(t.error(), None);
        t.record(1024, None);
        t.record(512, Some("写超时"));
        t.record(0, Some("后来的错误"));
        assert_eq!(t.written(), 1536);
        assert!(t.failed());
        assert_eq!(t.error().as_deref(), Some("写超时"));
    }

    #[test]
    fn test_empty_file_returns_zero_without_touching_queue() {
        let p = temp_file("empty", 0);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = AtomicBool::new(true);
        let r = send_file_tracked(&tx, &running, p.to_str().unwrap(), |_, _| panic!("空文件不应报进度"));
        assert_eq!(r, Ok(0));
        assert!(rx.is_empty());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_returns_only_after_every_byte_is_written() {
        // 200 块,队列 64 格:入队早就完了,必须等写线程写完最后一块才能返回
        let total = 200 * CHUNK_SIZE + 17;
        let p = temp_file("ok", total);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = Arc::new(AtomicBool::new(true));
        let writer = fake_writer(rx, running.clone(), None, None, Duration::from_millis(1));
        let mut progress: Vec<(usize, usize)> = Vec::new();
        let r = send_file_tracked(&tx, &running, p.to_str().unwrap(), |w, t| progress.push((w, t)));
        assert_eq!(r, Ok(total));
        drop(tx);
        assert_eq!(writer.join().unwrap().0, 201, "应收到全部 201 块");
        assert_eq!(progress.last(), Some(&(total, total)), "成功时最后一次进度必为 (total, total)");
        assert!(progress.windows(2).all(|w| w[0].0 <= w[1].0), "进度单调不减: {:?}", progress);
        assert!(progress.iter().all(|(w, t)| *t == total && *w <= total));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_write_error_stops_enqueue_and_reports_written_bytes() {
        let total = 300 * CHUNK_SIZE;
        let p = temp_file("fail", total);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = Arc::new(AtomicBool::new(true));
        let writer = fake_writer(rx, running.clone(), Some(3), None, Duration::from_millis(1));
        let r = send_file_tracked(&tx, &running, p.to_str().unwrap(), |_, _| {});
        let err = r.expect_err("第 3 块写错必须返回 Err");
        assert!(err.contains("写超时"), "应带写线程记下的错误: {}", err);
        assert!(err.contains(&format!("已写出 {}/{} 字节", 2 * CHUNK_SIZE + CHUNK_SIZE / 2, total)), "{}", err);
        drop(tx);
        let (received, _written) = writer.join().unwrap();
        assert!(received < 300, "写错后应停止入队,实际入队 {} 块", received);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_disconnect_while_draining_reports_interrupted() {
        // 写线程收 5 块后退出(running=false、接收端丢弃):入队阶段或排空阶段都要能报"连接已断开"
        let total = 100 * CHUNK_SIZE;
        let p = temp_file("disc", total);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = Arc::new(AtomicBool::new(true));
        let writer = fake_writer(rx, running.clone(), None, Some(5), Duration::from_millis(1));
        let r = send_file_tracked(&tx, &running, p.to_str().unwrap(), |_, _| {});
        let err = r.expect_err("断开必须返回 Err");
        assert!(err.contains("连接已断开"), "{}", err);
        assert!(err.contains(&format!("已写出 {}/{} 字节", 5 * CHUNK_SIZE, total)), "{}", err);
        writer.join().unwrap();
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn test_progress_reflects_written_not_enqueued() {
        // 写线程每块 20ms:入队瞬间塞满 64 格,但 100ms 时的进度只能是已写出的 ~5 块,不能是 64 块
        let total = 80 * CHUNK_SIZE;
        let p = temp_file("slow", total);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = Arc::new(AtomicBool::new(true));
        let writer = fake_writer(rx, running.clone(), None, None, Duration::from_millis(20));
        let mut first: Option<usize> = None;
        let r = send_file_tracked(&tx, &running, p.to_str().unwrap(), |w, _| {
            if first.is_none() && w > 0 {
                first = Some(w);
            }
        });
        assert_eq!(r, Ok(total));
        drop(tx);
        writer.join().unwrap();
        let first = first.expect("应有进度回调");
        assert!(first < 32 * CHUNK_SIZE, "首个进度 {} 字节:按入队算会是 64KB 起跳,按写出算只有几 KB", first);
        let _ = std::fs::remove_file(p);
    }

    // ============ 取消语义(S7) ============

    /// 入队阶段取消:后面的块不再入队,已写的字节数不多算。
    #[test]
    fn test_cancel_during_enqueue_stops_sending_more_chunks() {
        let total = 200 * CHUNK_SIZE;
        let p = temp_file("cancel_enqueue", total);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = Arc::new(AtomicBool::new(true));
        // 故意不启动写线程:队列很快满,发送方卡在 send_timeout 上等待
        let ct = tokio_util::sync::CancellationToken::new();
        let ct2 = ct.clone();
        let path = p.to_str().unwrap().to_string();
        let running_for_sender = running.clone();
        let sender = std::thread::spawn(move || {
            send_file_tracked_with_cancel(&tx, &running_for_sender, &path, Some(&ct2), |_, _| {})
        });
        // 等它把队列塞满
        std::thread::sleep(Duration::from_millis(200));
        ct.cancel();
        let outcome = sender.join().unwrap().expect("取消应返回 Ok(部分结果)");
        assert!(outcome.cancelled, "应报告被取消");
        assert_eq!(outcome.total, total, "总长度要如实报告");
        assert!(outcome.written <= total, "已写字节数不能超过文件总长");
        // 队列里最多 64 格:后续块没再入队
        let queued = rx.len();
        assert!(queued <= 64, "入队的块数应受队列容量限制,实际 {}", queued);
        assert!(running.load(Ordering::SeqCst), "取消文件不能把串口置为断开");
        let _ = std::fs::remove_file(p);
    }

    /// 排空阶段取消:已排队的块被写线程丢弃,返回部分进度。
    #[test]
    fn test_cancel_during_drain_discards_queued_chunks() {
        // 10 块:队列装得下,发送方进入排空循环等写线程
        let total = 10 * CHUNK_SIZE;
        let p = temp_file("cancel_drain", total);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = Arc::new(AtomicBool::new(true));
        // 写线程每块 100ms:排空阶段有足够时间取消
        let writer = fake_writer(rx, running.clone(), None, None, Duration::from_millis(100));
        let ct = tokio_util::sync::CancellationToken::new();
        let ct2 = ct.clone();
        let path = p.to_str().unwrap().to_string();
        let running_for_sender = running.clone();
        let sender = std::thread::spawn(move || {
            send_file_tracked_with_cancel(&tx, &running_for_sender, &path, Some(&ct2), |_, _| {})
        });
        std::thread::sleep(Duration::from_millis(250));
        ct.cancel();
        let outcome = sender.join().unwrap().expect("取消应返回 Ok(部分结果)");
        assert!(outcome.cancelled, "应报告被取消");
        assert_eq!(outcome.total, total);
        assert!(outcome.written < total, "取消时不该报完整,已写 {}", outcome.written);
        assert_eq!(outcome.written % CHUNK_SIZE, 0, "已写字节数应按块计,实际 {}", outcome.written);
        assert!(running.load(Ordering::SeqCst), "取消文件不能把串口置为断开");
        // 写线程继续跑完队列,但被取消的块直接丢弃:实际写出的块数少于总数
        let (_received, written) = writer.join().unwrap();
        assert!(written < 10, "取消后不该再写出剩余块,实际写出 {} 块", written);
        let _ = std::fs::remove_file(p);
    }

    /// 取消后不会自动重发:只发一次,没有第二轮入队。
    #[test]
    fn test_cancel_does_not_resend_from_the_beginning() {
        let total = 20 * CHUNK_SIZE;
        let p = temp_file("cancel_noresend", total);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = Arc::new(AtomicBool::new(true));
        let writer = fake_writer(rx, running.clone(), None, None, Duration::from_millis(20));
        let ct = tokio_util::sync::CancellationToken::new();
        let ct2 = ct.clone();
        let path = p.to_str().unwrap().to_string();
        let running_for_sender = running.clone();
        let sender = std::thread::spawn(move || {
            send_file_tracked_with_cancel(&tx, &running_for_sender, &path, Some(&ct2), |_, _| {})
        });
        std::thread::sleep(Duration::from_millis(150));
        ct.cancel();
        let outcome = sender.join().unwrap().expect("取消应返回 Ok(部分结果)");
        assert!(outcome.cancelled);
        let (_received, written) = writer.join().unwrap();
        // 总共只发了 20 块:没有"取消后从头再来"导致写出超过 20 块
        assert!(written <= 20, "写出块数 {} 超过文件总块数,说明重发了", written);
        let _ = std::fs::remove_file(p);
    }

    /// 旧路径(无取消令牌)行为完全不变:整份文件发完。
    #[test]
    fn test_legacy_path_without_token_still_sends_everything() {
        let total = 20 * CHUNK_SIZE;
        let p = temp_file("legacy_full", total);
        let (tx, rx) = bounded::<WriteCommand>(64);
        let running = Arc::new(AtomicBool::new(true));
        let writer = fake_writer(rx, running.clone(), None, None, Duration::from_millis(1));
        let r = send_file_tracked(&tx, &running, p.to_str().unwrap(), |_, _| {});
        assert_eq!(r, Ok(total));
        drop(tx);
        let (_received, written) = writer.join().unwrap();
        assert_eq!(written, 20, "20 块都要写出去");
        let _ = std::fs::remove_file(p);
    }
}
