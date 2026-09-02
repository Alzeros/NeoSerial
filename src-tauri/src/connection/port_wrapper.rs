use std::io::Read;
use std::sync::{Arc, Mutex};

/// 读端口包装器
pub enum PortReader {
    /// serialport 句柄（非 Windows 或 fallback）
    Owned(Box<dyn serialport::SerialPort>),
    /// Windows 原生 overlapped 句柄
    #[cfg(target_os = "windows")]
    Win(crate::connection::win_port::WinPort),
    /// 共享端口句柄（降级模式）
    Shared(Arc<Mutex<Box<dyn serialport::SerialPort>>>),
}

impl Read for PortReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            PortReader::Owned(port) => port.read(buf),
            #[cfg(target_os = "windows")]
            PortReader::Win(port) => {
                // 等待上限故意大于 COMMTIMEOUTS 的 20ms 读超时:正常由驱动按 20ms 完成读
                // (0 字节),不走 WaitForSingleObject 超时 + CancelIo 的取消路径。
                match port.read_overlapped(buf, 100) {
                    // 与 serialport 语义对齐:超时无数据报 TimedOut。reader 只在 TimedOut 分支
                    // flush 不换行的残尾(shell 提示符、只以 CR 结尾的行);返回 Ok(0) 走的是
                    // 另一分支,残尾会压到下一个换行到来才显示,send_and_read 也等不到提示符式响应。
                    Ok(0) => Err(std::io::ErrorKind::TimedOut.into()),
                    other => other,
                }
            }
            PortReader::Shared(arc) => {
                let mut guard = arc.lock()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("端口锁错误: {}", e)))?;
                guard.read(buf)
            }
        }
    }
}

/// 写端口包装器
pub enum PortWriter {
    /// serialport 句柄（非 Windows 或 fallback）
    Owned(Box<dyn serialport::SerialPort>),
    /// Windows 原生 overlapped 句柄
    #[cfg(target_os = "windows")]
    Win(crate::connection::win_port::WinPort),
    /// 共享端口句柄（降级模式）
    Shared(Arc<Mutex<Box<dyn serialport::SerialPort>>>),
}

impl PortWriter {
    /// 写数据。Windows 上用 overlapped I/O（不阻塞），其他平台用同步 write。
    pub fn write_data(&self, data: &[u8]) -> std::io::Result<usize> {
        match self {
            PortWriter::Owned(_) => {
                // Fallback（非 Windows 或 serialport 路径）：不应走到这里
                Err(std::io::Error::new(std::io::ErrorKind::Other, "write_data 不支持 serialport 句柄"))
            }
            #[cfg(target_os = "windows")]
            PortWriter::Win(port) => port.write_overlapped(data, port.write_timeout_ms(data.len())),
            PortWriter::Shared(_) => {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "write_data 不支持共享句柄"))
            }
        }
    }
}
