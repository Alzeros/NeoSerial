use crossbeam_channel::{RecvTimeoutError, Receiver, Sender, unbounded};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// stop() 等存盘线程把缓冲写完的上限。正常几毫秒;文件所在盘卡死时不让退出流程无限等。
const STOP_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

static LOGGER_ID_SEQ: AtomicU64 = AtomicU64::new(0);

/// 存盘句柄。每条记录是一行文本（不含末尾换行符），存盘线程写入时补 \n。
/// channel send 纳秒级，绝不阻塞调用方（读线程）。
pub struct FileLogger {
    /// 每个 logger 一个 id:失败回调用它确认 AppState 里挂的还是自己,不误清后来新开的 logger
    id: u64,
    tx: Option<Sender<String>>,
    path: PathBuf,
    /// 存盘线程退出(最后一次 flush 之后)时 drop 对端 → recv 立刻返回 Disconnected。
    /// 用它代替 JoinHandle:std 的 join 不能限时。
    done_rx: Receiver<()>,
}

impl FileLogger {
    /// 启动存盘线程。目录不存在会创建。
    /// - append=false：覆盖新建文件（File::create）
    /// - append=true：在已有文件末尾续写（不存在则创建）
    /// - on_fail(logger_id, 原因):写盘失败时在存盘线程上调一次,之后线程退出、不再收行。
    ///   调用方应据此摘掉 AppState 里的 logger/sender 并通知界面,否则按钮一直显示"记录中"
    ///   而实际一行没写;用 logger_id 比对 AppState 里挂的是否还是这个 logger(用户可能已经
    ///   停了它并开了新的)。回调里不要调 stop()(会等自己这条线程)——直接 drop 即可。
    pub fn start(
        path: PathBuf,
        append: bool,
        on_fail: impl FnOnce(u64, String) + Send + 'static,
    ) -> std::io::Result<Self> {
        let id = LOGGER_ID_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = if append {
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)?
        } else {
            File::create(&path)?
        };
        let (tx, rx) = unbounded::<String>();
        let (done_tx, done_rx) = crossbeam_channel::bounded::<()>(0);

        std::thread::Builder::new()
            .name("neoserial-file-logger".into())
            .spawn(move || {
                // 线程退出时 drop,stop() 在对端等它
                let _done = done_tx;
                let mut writer = BufWriter::new(file);
                let mut error_msg: Option<String> = None;
                // 空闲即落盘:突发批量写(BufWriter 8KB 满自动落),50ms 无新行时统一 flush。
                // 旧版每行 flush = 每行一次 write syscall,BufWriter 形同虚设;
                // 代价:应用崩溃时最多丢最后 50ms 的行(旧版 flush 也只到内核页缓存未 fsync,
                // OS 崩溃两种方案同样丢)。
                loop {
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(line) => {
                            if let Err(e) = writer.write_all(line.as_bytes()) {
                                error_msg = Some(format!("存盘失败: {}", e));
                                break;
                            }
                            if let Err(e) = writer.write_all(b"\n") {
                                error_msg = Some(format!("存盘失败: {}", e));
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            // 空闲:把攒的行推给内核
                            if let Err(e) = writer.flush() {
                                error_msg = Some(format!("存盘失败: {}", e));
                                break;
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                if error_msg.is_none() {
                    if let Err(e) = writer.flush() {
                        error_msg = Some(format!("存盘失败: {}", e));
                    }
                }
                if let Some(msg) = error_msg {
                    on_fail(id, msg);
                }
            })?;

        Ok(FileLogger {
            id,
            tx: Some(tx),
            path,
            done_rx,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// 返回 tx 的 clone，供读线程直接 send 行文本。
    pub fn line_sender(&self) -> Sender<String> {
        self.tx.clone().expect("tx 只在 stop() 里被取走,而 stop 消费 self")
    }

    /// 当前存盘文件路径。
    pub fn current_path(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    /// 停止并等存盘线程把缓冲写完(上限 STOP_FLUSH_TIMEOUT)。
    /// 退出应用/停止记录时若不等,BufWriter 里最后 ≤8KB、最近 50ms 的行会丢——
    /// 恰好是"断开前最后发生了什么"。
    pub fn stop(mut self) {
        drop(self.tx.take());
        let _ = self.done_rx.recv_timeout(STOP_FLUSH_TIMEOUT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::{Arc, Mutex};

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("neoserial_test_{}_{}.log", std::process::id(), name))
    }

    #[test]
    fn test_file_logger_writes_lines() {
        let path = tmp("writes");
        let failed = Arc::new(Mutex::new(None::<String>));
        let f2 = failed.clone();
        let logger = FileLogger::start(path.clone(), false, move |_, m| { *f2.lock().unwrap() = Some(m); }).unwrap();
        let tx = logger.line_sender();
        let _ = tx.send("08:00:01.000 [发送] AT".to_string());
        let _ = tx.send("08:00:01.123 [接收] OK".to_string());
        // stop 返回时内容必须已经在文件里(等过存盘线程),不需要 sleep
        logger.stop();

        let mut f = File::open(&path).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        assert_eq!(s, "08:00:01.000 [发送] AT\n08:00:01.123 [接收] OK\n");
        assert!(failed.lock().unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_current_path_and_id_unique() {
        let path = tmp("path");
        let logger = FileLogger::start(path.clone(), false, |_, _| {}).unwrap();
        assert_eq!(logger.current_path(), path.to_string_lossy().to_string());
        let other = FileLogger::start(tmp("path2"), false, |_, _| {}).unwrap();
        assert_ne!(logger.id(), other.id());
        logger.stop();
        other.stop();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(tmp("path2"));
    }

    /// 默认日志目录(%APPDATA%/neoserial/logs)从未被别处创建:start 必须自己建父目录,
    /// 否则 MCP start_logging 不传 path 在新机器上必失败。
    #[test]
    fn test_start_creates_missing_parent_dir() {
        let dir = std::env::temp_dir().join(format!("neoserial_test_{}_mkdir", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("deeper").join("a.log");
        let logger = FileLogger::start(path.clone(), false, |_, _| {}).unwrap();
        logger.stop();
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 打不开文件在 start 就报错,不能起一个必然失败的线程再靠 on_fail 兜底。
    /// (运行中的写失败在 Windows 上没有稳定的模拟手段,不在单测覆盖。)
    #[test]
    fn test_start_on_directory_path_errors() {
        let dir = std::env::temp_dir();
        let err = FileLogger::start(dir, false, |_, _| {});
        assert!(err.is_err(), "把目录当文件打开应返回 Err,而不是起一个必然失败的线程");
    }
}
