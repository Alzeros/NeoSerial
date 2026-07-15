use crossbeam_channel::{Receiver, Sender};
use serialport::SerialPort;
use std::io::Read;
use std::time::Duration;

use crate::buffer::log_line::{Dir, LogLine};
use crate::connection::handle::{ConnCommand, ConnEvent};
use crate::connection::line_assembler::LineAssembler;
use crate::connection::writer::{handle_send, try_recv_command};
use crate::util::time_fmt::now_local_ts;

/// 读线程主循环。
/// 阻塞读串口 + 1ms timeout 穿插处理命令；按行切分后通过 event_tx 发送 ConnEvent::Lines。
/// 端口断开（非 TimedOut 错误）时发送 ConnEvent::Disconnected 并退出。
///
/// 存盘（设计 §3.2/§4.1）：log_tx 存在时，读线程把原始字节 chunk 直接 send 给存盘线程，
/// **在 LineAssembler 切行之前**，保留换行符与半行。TX 字节也经此路径存盘。
/// 这样存盘路径与 UI 显示路径彻底解耦：暂停屏幕时存盘照写。
pub fn reader_loop(
    mut port: Box<dyn SerialPort>,
    cmd_rx: Receiver<ConnCommand>,
    event_tx: Sender<ConnEvent>,
    error_keywords: Vec<String>,
    mut log_tx: Option<Sender<Vec<u8>>>,
) {
    // 设置 10ms 读超时，使 loop 能定期返回处理命令
    let _ = port.set_timeout(Duration::from_millis(10));

    let mut assembler = LineAssembler::new();
    let mut buf = [0u8; 1024];

    loop {
        // 先处理待发命令（非阻塞）
        while let Some(cmd) = try_recv_command(&cmd_rx) {
            // SetLogger 在此处理：更新读线程持有的 log_tx（支持连接期间动态启停存盘）
            if let ConnCommand::SetLogger(new_tx) = &cmd {
                log_tx = new_tx.clone();
                continue;
            }
            if !handle_send(&cmd, port.as_mut(), &event_tx) {
                return; // Close
            }
            // 若是 Send，回显进日志流；同时 TX 字节经读线程存盘（不重复）
            if let ConnCommand::Send(bytes) = &cmd {
                if let Some(tx) = &log_tx {
                    let _ = tx.send(bytes.clone());
                }
                let line = LogLine::new(
                    now_local_ts(),
                    Dir::Tx,
                    bytes.clone(),
                    &error_keywords,
                );
                let _ = event_tx.send(ConnEvent::Lines(vec![line]));
            }
        }

        // 读串口
        match port.read(&mut buf) {
            Ok(0) => {
                // EOF，视作断开
                let _ = event_tx.send(ConnEvent::Disconnected("端口 EOF".into()));
                return;
            }
            Ok(n) => {
                // 存盘原始字节（含换行符、半行），在切行之前 —— 设计 §4.1
                if let Some(tx) = &log_tx {
                    let _ = tx.send(buf[..n].to_vec());
                }
                let new_lines: Vec<LogLine> = assembler
                    .feed(&buf[..n])
                    .into_iter()
                    .map(|raw| {
                        LogLine::new(now_local_ts(), Dir::Rx, raw, &error_keywords)
                    })
                    .collect();
                if !new_lines.is_empty() {
                    let _ = event_tx.send(ConnEvent::Lines(new_lines));
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                // 正常，无数据，继续
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // 正常，继续
            }
            Err(e) => {
                let _ = event_tx.send(ConnEvent::Disconnected(format!("连接已断开: {}", e)));
                return;
            }
        }
    }
}
