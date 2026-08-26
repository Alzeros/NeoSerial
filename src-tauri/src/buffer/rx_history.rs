use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::buffer::log_line::{Dir, LogLine};
use crate::buffer::ring_buffer::RingBuffer;

/// 后端收发历史:存 (序号, Dir, LogLine) 的 ring buffer,供 MCP 阻塞读/历史查询。
///
/// 同时存 tx 与 rx——agent 既能读模组响应(rx),也能读用户/自己发过的命令(tx)。
/// send_and_read 内部过滤掉 tx 只返回 rx;get_history_since 返回全部(带 dir)。
///
/// 写入:reader 线程 push Rx,emit_tx_line/各发送路径 push Tx(sync 调用)。
/// 读取:MCP handler(async)调 since/latest_seq。
/// 用 std::sync::Mutex(临界区纳秒级,不跨 await 持锁);序号用 AtomicU64;
/// 等待用 tokio::sync::Notify(push 时 notify_all)。
pub struct RxHistory {
    inner: Mutex<RingBuffer<(u64, Dir, LogLine)>>,
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

    /// push 一行(带方向),返回分配的单调序号(从 1 开始)。push 后 notify_all 唤醒等待者。
    pub fn push(&self, dir: Dir, line: LogLine) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut buf) = self.inner.lock() {
            buf.push((seq, dir, line));
        }
        self.notify.notify_waiters();
        seq
    }

    /// 返回 seq > after_seq 的所有行(从旧到新,按插入序)+ 当前最大序号。
    pub fn since(&self, after_seq: u64) -> (Vec<(u64, Dir, LogLine)>, u64) {
        match self.inner.lock() {
            Ok(buf) => {
                // latest 必须取 buffer 内最后一行的 seq(而非原子计数器):
                // push 里 seq.fetch_add 与 buf.push 不是同一临界区,若返回计数器值,
                // send_and_read 的游标可能跳过"已分配序号但尚未插入"的行,造成丢数据。
                let latest = buf
                    .back()
                    .map(|(s, _, _)| *s)
                    .unwrap_or_else(|| self.seq.load(Ordering::SeqCst));
                // 锁内扫描,只克隆 seq > after_seq 的行。旧实现 snapshot() 先全量深拷贝
                // 最多 capacity 行(每行 raw/ascii/hex 多次堆分配)再 filter——是 MCP
                // 热路径(send_and_read 每次唤醒、get_history_since 每次 poll)的主要
                // 分配风暴。不二分定位:seq 分配与插入非同一临界区,理论上存在短暂
                // 乱序,二分会漏行;线性扫描保持与旧实现一致的插入序语义。
                let filtered: Vec<(u64, Dir, LogLine)> = buf
                    .iter()
                    .filter(|entry| entry.0 > after_seq)
                    .cloned()
                    .collect();
                (filtered, latest)
            }
            Err(_) => (Vec::new(), self.seq.load(Ordering::SeqCst)),
        }
    }

    /// 当前最大序号(无数据返回 0)。
    pub fn latest_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// buffer 内现存最旧一行的序号(空 buffer 返回 None)。
    ///
    /// 用途:让调用方识别"游标已被容量淘汰"。ring buffer 满后 push 会丢最旧行,
    /// 此后 since(旧游标) 只能返回残余部分且**无任何丢失提示**——agent 会把
    /// 有空洞的数据当成完整对话流。比对 oldest_seq 与游标即可判定空洞。
    pub fn oldest_seq(&self) -> Option<u64> {
        self.inner.lock().ok().and_then(|buf| buf.front().map(|(s, _, _)| *s))
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
        LogLine::new("08:00:00.000".into(), Dir::Rx, content.to_vec(), &[], 0)
    }
    fn tx(content: &[u8]) -> LogLine {
        LogLine::new("08:00:00.000".into(), Dir::Tx, content.to_vec(), &[], 0)
    }

    #[test]
    fn test_seq_monotonic_from_1() {
        let h = RxHistory::new(100);
        let s1 = h.push(Dir::Rx, rx(b"OK"));
        let s2 = h.push(Dir::Tx, tx(b"AT"));
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(h.latest_seq(), 2);
    }

    #[test]
    fn test_since_filters_after_seq() {
        let h = RxHistory::new(100);
        h.push(Dir::Rx, rx(b"line1")); // seq1
        let mid = h.push(Dir::Tx, tx(b"line2")); // seq2
        h.push(Dir::Rx, rx(b"line3")); // seq3
        let (rows, latest) = h.since(mid);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 3);
        assert_eq!(rows[0].1, Dir::Rx);
        assert_eq!(rows[0].2.ascii, "line3");
        assert_eq!(latest, 3);
    }

    #[test]
    fn test_since_empty_when_no_new() {
        let h = RxHistory::new(100);
        h.push(Dir::Rx, rx(b"only"));
        let (rows, latest) = h.since(h.latest_seq());
        assert!(rows.is_empty());
        assert_eq!(latest, 1);
    }

    #[test]
    fn test_capacity_overflow_drops_oldest() {
        let h = RxHistory::new(2);
        let s1 = h.push(Dir::Rx, rx(b"a")); // seq1
        h.push(Dir::Tx, tx(b"b"));          // seq2
        h.push(Dir::Rx, rx(b"c"));          // seq3,a 被挤出
        let (rows, _) = h.since(0);
        assert_eq!(rows.len(), 2);
        // 最旧的 a(seq1)已不在
        assert!(rows.iter().all(|(s, _, _)| *s != s1));
        // 但 since(0) 仍只返回 buffer 内现存的(seq2,seq3)
        assert_eq!(rows[0].2.ascii, "b");
        assert_eq!(rows[0].1, Dir::Tx);
        assert_eq!(rows[1].2.ascii, "c");
        assert_eq!(rows[1].1, Dir::Rx);
    }

    /// oldest_seq 反映 buffer 内实际最旧行:未溢出时=1,溢出后随之前移。
    /// get_history_since 靠它判断"游标之后的行是否已被挤出"。
    #[test]
    fn test_oldest_seq_tracks_eviction() {
        let h = RxHistory::new(2);
        assert_eq!(h.oldest_seq(), None); // 空 buffer
        h.push(Dir::Rx, rx(b"a")); // seq1
        assert_eq!(h.oldest_seq(), Some(1));
        h.push(Dir::Tx, tx(b"b")); // seq2,未满
        assert_eq!(h.oldest_seq(), Some(1));
        h.push(Dir::Rx, rx(b"c")); // seq3,seq1 被挤出
        assert_eq!(h.oldest_seq(), Some(2));
        h.push(Dir::Rx, rx(b"d")); // seq4,seq2 被挤出
        assert_eq!(h.oldest_seq(), Some(3));
        // 游标停在 seq1 的 agent:oldest(3) > seq+1(2) → 中间有洞(seq2 丢了)
        assert!(h.oldest_seq().unwrap() > 1 + 1);
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
        h.push(Dir::Rx, rx(b"x"));
        h.clear();
        assert_eq!(h.latest_seq(), 1); // seq 不回退,但 buffer 空
        let (rows, _) = h.since(0);
        assert!(rows.is_empty());
    }

    /// tx+rx 混合:since 返回全部行(含 tx),send_and_read 语义靠调用方过滤 tx。
    /// 这里验证 history 本身不过滤——完整保留 tx+rx,过滤责任在 send_and_read。
    #[test]
    fn test_mixed_tx_rx_kept_together() {
        let h = RxHistory::new(100);
        let s0 = h.latest_seq();
        h.push(Dir::Tx, tx(b"AT"));      // seq1: 发的命令
        h.push(Dir::Rx, rx(b""));        // seq2: 空行
        h.push(Dir::Rx, rx(b"OK"));      // seq3: 模组响应
        let (rows, latest) = h.since(s0);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].1, Dir::Tx);  // tx 也在
        assert_eq!(rows[1].1, Dir::Rx);
        assert_eq!(rows[2].1, Dir::Rx);
        assert_eq!(latest, 3);
        // 调用方过滤 tx 只取 rx:模拟 send_and_read 的行为
        let rx_only: Vec<_> = rows.into_iter().filter(|(_, d, _)| *d == Dir::Rx).collect();
        assert_eq!(rx_only.len(), 2);
        assert_eq!(rx_only[1].2.ascii, "OK");
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
            h2.push(Dir::Rx, rx(b"late"));
        });
        // 等待被唤醒(应在 ~50ms 后,push 调 notify_waiters)
        timeout(Duration::from_secs(1), notified).await.expect("notify 未唤醒");
        // 醒来重查
        let (rows2, _) = h.since(seq0);
        assert_eq!(rows2.len(), 1);
        assert_eq!(rows2[0].2.ascii, "late");
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
