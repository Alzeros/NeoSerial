use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// 读端口包装器 - 支持独立句柄或共享句柄
pub enum PortReader {
    /// 独立端口句柄（try_clone 成功）
    Owned(Box<dyn serialport::SerialPort>),
    /// 共享端口句柄（降级模式）
    Shared(Arc<Mutex<Box<dyn serialport::SerialPort>>>),
}

impl Read for PortReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            PortReader::Owned(port) => port.read(buf),
            PortReader::Shared(arc) => {
                let mut guard = arc.lock()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("端口锁错误: {}", e)))?;
                guard.read(buf)
            }
        }
    }
}

/// 写端口包装器 - 支持独立句柄或共享句柄
pub enum PortWriter {
    /// 独立端口句柄（try_clone 成功）
    Owned(Box<dyn serialport::SerialPort>),
    /// 共享端口句柄（降级模式）
    Shared(Arc<Mutex<Box<dyn serialport::SerialPort>>>),
}

impl Write for PortWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            PortWriter::Owned(port) => port.write(buf),
            PortWriter::Shared(arc) => {
                let mut guard = arc.lock()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("端口锁错误: {}", e)))?;
                guard.write(buf)
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            PortWriter::Owned(port) => port.flush(),
            PortWriter::Shared(arc) => {
                let mut guard = arc.lock()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("端口锁错误: {}", e)))?;
                guard.flush()
            }
        }
    }
}
