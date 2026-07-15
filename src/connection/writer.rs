use serialport::SerialPort;
use std::time::Duration;

use crate::connection::handle::{ConnCommand, ConnEvent};
use crossbeam_channel::{Receiver, Sender};

/// 在读线程内同步处理一次发送命令。
/// 返回是否应该继续（false 表示已 Close）。
/// SetLogger 命令在这里处理：更新读线程持有的 log_tx。
pub fn handle_send(
    cmd: &ConnCommand,
    port: &mut dyn SerialPort,
    event_tx: &Sender<ConnEvent>,
) -> bool {
    match cmd {
        ConnCommand::Send(bytes) => {
            let result = port
                .write_all(bytes)
                .and_then(|_| port.flush());
            if let Err(e) = result {
                let _ = event_tx.send(ConnEvent::SendFailed(format!("发送失败: {}", e)));
            }
            true
        }
        ConnCommand::Close => false,
        // SetLogger 的处理在 reader_loop 中完成（需修改 log_tx），此处不处理。
        ConnCommand::SetLogger(_) => true,
    }
}

/// 阻塞等待命令（带超时以便读线程能穿插处理读）。
pub fn try_recv_command(cmd_rx: &Receiver<ConnCommand>) -> Option<ConnCommand> {
    cmd_rx.recv_timeout(Duration::from_millis(1)).ok()
}
