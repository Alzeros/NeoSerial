use crossbeam_channel::{Receiver, Sender};
use serialport::SerialPort;
use std::io::Read;
use std::time::Duration;

use crate::buffer::log_line::{Dir, LogLine};
use crate::connection::handle::{ConnCommand, ConnEvent};
use crate::connection::line_assembler::LineAssembler;
use crate::connection::writer::{handle_send, try_recv_command};
use crate::util::log_format::{format_line, DisplayConfig};
use crate::util::time_fmt::now_local_ts;

/// 读线程主循环。
/// 存盘（格式化文本日志）：log_tx 存在时，切出完整行后按 DisplayConfig 生成
/// "[ts] [tag] content" 文本行 send 给 FileLogger（独立于 UI Timer，暂停照写）。
/// 显示过滤：log_send=false 时 TX 行不进 event（也不进存盘），但字节统计照算。
pub fn reader_loop(
    mut port: Box<dyn SerialPort>,
    cmd_rx: Receiver<ConnCommand>,
    event_tx: Sender<ConnEvent>,
    error_keywords: Vec<String>,
    mut log_tx: Option<Sender<String>>,
    mut display: DisplayConfig,
) {
    let _ = port.set_timeout(Duration::from_millis(10));
    let mut assembler = LineAssembler::new();
    let mut buf = [0u8; 1024];
    let mut tx_bytes: u64 = 0;
    let mut rx_bytes: u64 = 0;
    let mut stats_since_report: u64 = 0;

    loop {
        while let Some(cmd) = try_recv_command(&cmd_rx) {
            match &cmd {
                ConnCommand::SetLogger(new_tx) => {
                    log_tx = new_tx.clone();
                    continue;
                }
                ConnCommand::SetDisplay(d) => {
                    display = *d;
                    continue;
                }
                _ => {}
            }
            if !handle_send(&cmd, port.as_mut(), &event_tx) {
                return; // Close
            }
            if let ConnCommand::Send(bytes) = &cmd {
                tx_bytes += bytes.len() as u64;
                stats_since_report += bytes.len() as u64;
                // TX 行：log_send=true 才进日志流和存盘
                if display.log_send {
                    let line = LogLine::new(now_local_ts(), Dir::Tx, bytes.clone(), &error_keywords);
                    if let Some(tx) = &log_tx {
                        let s = format_line(&line, &display, false);
                        if !s.is_empty() {
                            let _ = tx.send(s);
                        }
                    }
                    let _ = event_tx.send(ConnEvent::Lines(vec![line]));
                }
            }
        }

        match port.read(&mut buf) {
            Ok(0) => {
                let _ = event_tx.send(ConnEvent::Disconnected("端口 EOF".into()));
                return;
            }
            Ok(n) => {
                rx_bytes += n as u64;
                stats_since_report += n as u64;
                let new_lines: Vec<LogLine> = assembler
                    .feed(&buf[..n])
                    .into_iter()
                    .map(|raw| LogLine::new(now_local_ts(), Dir::Rx, raw, &error_keywords))
                    .collect();
                // 存盘：每行格式化后送 FileLogger
                if let Some(tx) = &log_tx {
                    for l in &new_lines {
                        let s = format_line(l, &display, false);
                        if !s.is_empty() {
                            let _ = tx.send(s);
                        }
                    }
                }
                if !new_lines.is_empty() {
                    let _ = event_tx.send(ConnEvent::Lines(new_lines));
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                let _ = event_tx.send(ConnEvent::Disconnected(format!("连接已断开: {}", e)));
                return;
            }
        }

        // 每 256 字节上报一次统计
        if stats_since_report >= 256 {
            stats_since_report = 0;
            let _ = event_tx.send(ConnEvent::Stats { tx_bytes, rx_bytes });
        }
    }
}
