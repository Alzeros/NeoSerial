use crossbeam_channel::{Sender, unbounded};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// 存盘句柄。每条记录是一行文本（不含末尾换行符），存盘线程写入时补 \n。
/// channel send 纳秒级，绝不阻塞调用方（读线程）。
pub struct FileLogger {
    tx: Sender<String>,
    path: Mutex<Option<PathBuf>>,
}

impl FileLogger {
    /// 启动存盘线程。
    /// - append=false：覆盖新建文件（File::create）
    /// - append=true：在已有文件末尾续写（不存在则创建）
    pub fn start(path: PathBuf, error_tx: Sender<String>, append: bool) -> std::io::Result<Self> {
        let file = if append {
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)?
        } else {
            File::create(&path)?
        };
        let (tx, rx) = unbounded::<String>();

        std::thread::Builder::new()
            .name("neoserial-file-logger".into())
            .spawn(move || {
                let mut writer = BufWriter::new(file);
                let mut error_msg: Option<String> = None;
                for line in rx.iter() {
                    if let Err(e) = writer.write_all(line.as_bytes()) {
                        error_msg = Some(format!("存盘失败: {}", e));
                        break;
                    }
                    if let Err(e) = writer.write_all(b"\n") {
                        error_msg = Some(format!("存盘失败: {}", e));
                        break;
                    }
                    if let Err(e) = writer.flush() {
                        error_msg = Some(format!("存盘失败: {}", e));
                        break;
                    }
                }
                let _ = writer.flush();
                if let Some(msg) = error_msg {
                    let _ = error_tx.send(msg);
                }
            })?;

        Ok(FileLogger {
            tx,
            path: Mutex::new(Some(path)),
        })
    }

    /// 返回 tx 的 clone，供读线程直接 send 行文本。
    pub fn line_sender(&self) -> Sender<String> {
        self.tx.clone()
    }

    /// 记录一行文本（非阻塞）。传入的字符串不应含末尾换行符。
    pub fn log_line(&self, line: String) {
        let _ = self.tx.send(line);
    }

    /// 当前存盘文件路径。stop 之后返回 None。
    pub fn current_path(&self) -> Option<String> {
        self.path
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|p| p.to_string_lossy().to_string()))
    }

    pub fn stop(self) {
        drop(self.tx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_file_logger_writes_lines() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("neoserial_test_{}.log", std::process::id()));
        let (error_tx, error_rx) = crossbeam_channel::unbounded::<String>();
        let logger = FileLogger::start(path.clone(), error_tx, false).unwrap();
        let tx = logger.line_sender();
        let _ = tx.send("08:00:01.000 [发送] AT".to_string());
        let _ = tx.send("08:00:01.123 [接收] OK".to_string());
        logger.stop();

        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut f = File::open(&path).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        assert_eq!(s, "08:00:01.000 [发送] AT\n08:00:01.123 [接收] OK\n");
        assert!(error_rx.try_recv().is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_current_path() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("neoserial_test_path_{}.log", std::process::id()));
        let (error_tx, _) = crossbeam_channel::unbounded::<String>();
        let logger = FileLogger::start(path.clone(), error_tx, false).unwrap();
        assert_eq!(logger.current_path(), Some(path.to_string_lossy().to_string()));
        logger.stop();
        let _ = std::fs::remove_file(&path);
    }
}
