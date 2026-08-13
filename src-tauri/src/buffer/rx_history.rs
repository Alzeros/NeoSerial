use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::buffer::log_line::LogLine;
use crate::buffer::ring_buffer::RingBuffer;

/// 后端 rx 历史:存 (序号, LogLine) 的 ring buffer,供 MCP 阻塞读按 seq 查询。
///
/// 写入:reader 线程(sync)调 push;读取:MCP handler(async)调 since/latest_seq。
/// 用 std::sync::Mutex(临界区纳秒级,不跨 await 持锁);序号用 AtomicU64;
/// 等待用 tokio::sync::Notify(push 时 notify_all)。
pub struct RxHistory {
    inner: Mutex<RingBuffer<(u64, LogLine)>>,
    seq: AtomicU64,
    notify: tokio::sync::Notify,
}

impl RxHistory {
    pub fn new(capacity: usize) -> Self {
        RxHistory {
            inner: Mutex::new(RingBuffer::new(capacity)),
            seq: AtomicU64::new(0),
            notify: tokio::sync::Notify::new(),
        }
    }

    /// push 一行,返回分配的单调序号(从 1 开始)。push 后 notify_all 唤醒等待者。
    pub fn push(&self, line: LogLine) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut buf) = self.inner.lock() {
            buf.push((seq, line));
        }
        self.notify.notify_waiters();
        seq
    }

    /// 返回 seq > after_seq 的所有行(从旧到新)+ 当前最大序号。
    pub fn since(&self, after_seq: u64) -> (Vec<(u64, LogLine)>, u64) {
        let latest = self.seq.load(Ordering::SeqCst);
        let rows = match self.inner.lock() {
            Ok(buf) => buf
                .snapshot()
                .into_iter()
                .filter(|(s, _)| *s > after_seq)
                .collect(),
            Err(_) => Vec::new(),
        };
        (rows, latest)
    }

    /// 当前最大序号(无数据返回 0)。
    pub fn latest_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// 供 MCP 侧异步等待:notify.notified().await。
    pub fn notify(&self) -> &tokio::sync::Notify {
        &self.notify
    }

    /// 清空历史(测试用)。
    pub fn clear(&self) {
        if let Ok(mut buf) = self.inner.lock() {
            buf.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::log_line::{Dir, LogLine};

    fn rx(content: &[u8]) -> LogLine {
        LogLine::new("08:00:00.000".into(), Dir::Rx, content.to_vec(), &[])
    }

    #[test]
    fn test_seq_monotonic_from_1() {
        let h = RxHistory::new(100);
        let s1 = h.push(rx(b"OK"));
        let s2 = h.push(rx(b"AT"));
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(h.latest_seq(), 2);
    }

    #[test]
    fn test_since_filters_after_seq() {
        let h = RxHistory::new(100);
        h.push(rx(b"line1")); // seq1
        let mid = h.push(rx(b"line2")); // seq2
        h.push(rx(b"line3")); // seq3
        let (rows, latest) = h.since(mid);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 3);
        assert_eq!(rows[0].1.ascii, "line3");
        assert_eq!(latest, 3);
    }

    #[test]
    fn test_since_empty_when_no_new() {
        let h = RxHistory::new(100);
        h.push(rx(b"only"));
        let (rows, latest) = h.since(h.latest_seq());
        assert!(rows.is_empty());
        assert_eq!(latest, 1);
    }

    #[test]
    fn test_capacity_overflow_drops_oldest() {
        let h = RxHistory::new(2);
        let s1 = h.push(rx(b"a")); // seq1
        h.push(rx(b"b"));          // seq2
        h.push(rx(b"c"));          // seq3,a 被挤出
        let (rows, _) = h.since(0);
        assert_eq!(rows.len(), 2);
        // 最旧的 a(seq1)已不在
        assert!(rows.iter().all(|(s, _)| *s != s1));
        // 但 since(0) 仍只返回 buffer 内现存的(seq2,seq3)
        assert_eq!(rows[0].1.ascii, "b");
        assert_eq!(rows[1].1.ascii, "c");
    }

    #[test]
    fn test_latest_seq_zero_when_empty() {
        let h = RxHistory::new(100);
        assert_eq!(h.latest_seq(), 0);
        let (rows, latest) = h.since(0);
        assert!(rows.is_empty());
        assert_eq!(latest, 0);
    }

    #[test]
    fn test_clear() {
        let h = RxHistory::new(100);
        h.push(rx(b"x"));
        h.clear();
        assert_eq!(h.latest_seq(), 1); // seq 不回退,但 buffer 空
        let (rows, _) = h.since(0);
        assert!(rows.is_empty());
    }

    use std::time::Duration;
    use tokio::time::timeout;

    // 验证"先查后等、醒来重查"模式:等待时被 push 唤醒,重查能拿到新行。
    #[tokio::test]
    async fn test_notify_wakes_waiter() {
        let h = std::sync::Arc::new(RxHistory::new(100));
        // 先查:此刻无数据,seq0=0
        let (rows, seq0) = h.since(0);
        assert!(rows.is_empty());
        // 异步等 notify,带 1s 超时(避免永远挂起)
        let notified = h.notify().notified();
        // 另一线程 50ms 后 push
        let h2 = h.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            h2.push(rx(b"late"));
        });
        // 等待被唤醒(应在 ~50ms 后,push 调 notify_waiters)
        timeout(Duration::from_secs(1), notified).await.expect("notify 未唤醒");
        // 醒来重查
        let (rows2, _) = h.since(seq0);
        assert_eq!(rows2.len(), 1);
        assert_eq!(rows2[0].1.ascii, "late");
    }

    // 验证超时路径:无人 push 时 timeout 正常到期,不漏唤醒也不死等。
    #[tokio::test]
    async fn test_timeout_when_no_push() {
        let h = RxHistory::new(100);
        let seq0 = h.latest_seq();
        let notified = h.notify().notified();
        let r = timeout(Duration::from_millis(50), notified).await;
        assert!(r.is_err(), "无 push 时应超时");
        let (rows, _) = h.since(seq0);
        assert!(rows.is_empty());
    }
}
