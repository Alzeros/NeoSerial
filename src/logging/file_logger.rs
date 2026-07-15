use crossbeam_channel::{Sender, unbounded};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

/// 存盘句柄。log() 是纳秒级的 channel send，绝不阻塞调用方（读线程）。
/// 实际磁盘 I/O 隔离在存盘线程内。
pub struct FileLogger {
    tx: Sender<Vec<u8>>,
}

impl FileLogger {
    /// 打开文件并启动存盘线程。
    /// error_tx 用于存盘线程出错时通知 UI（设计 §7.2：存盘失败→通知 UI→关闭存盘开关+红字提示）。
    pub fn start(path: PathBuf, error_tx: Sender<String>) -> std::io::Result<Self> {
        let file = File::create(&path)?;
        let (tx, rx) = unbounded::<Vec<u8>>();

        std::thread::Builder::new()
            .name("neoserial-file-logger".into())
            .spawn(move || {
                let mut writer = BufWriter::new(file);
                let mut error_msg: Option<String> = None;
                for chunk in rx.iter() {
                    if let Err(e) = writer.write_all(&chunk) {
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

        Ok(FileLogger { tx })
    }

    /// 返回 tx 的 clone，供读线程直接 send 原始字节。
    /// 读线程持有此 clone，存盘路径与 UI 显示路径彻底解耦。
    pub fn log_sender(&self) -> Sender<Vec<u8>> {
        self.tx.clone()
    }

    /// 记录一段原始字节（非阻塞）。
    pub fn log(&self, chunk: Vec<u8>) {
        let _ = self.tx.send(chunk);
    }

    /// 停止存盘：drop 发送端，存盘线程写完队列后退出。
    pub fn stop(self) {
        drop(self.tx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_file_logger_writes_all() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("neoserial_test_{}.bin", std::process::id()));
        let (error_tx, error_rx) = crossbeam_channel::unbounded::<String>();
        let logger = FileLogger::start(path.clone(), error_tx).unwrap();
        logger.log(b"hello ".to_vec());
        logger.log(b"world".to_vec());
        logger.stop();

        // 等待文件写完
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut f = File::open(&path).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello world");
        // 不应有错误
        assert!(error_rx.try_recv().is_err());
        let _ = std::fs::remove_file(&path);
    }
}
