use std::sync::Arc;

use tauri::{State, Emitter, Manager};

use crate::state::AppState;
use crate::buffer::rx_history::RxHistory;
use crate::commands::connection::resolve_port;
use crate::connection::WriteCommand;
use crate::buffer::log_line::{Dir, LogLine};
use crate::util::codec::{LineEnding, hex_to_bytes, ascii_to_bytes};
use crate::util::log_format::{DisplayConfig, format_line_for_file};
use crate::util::time_fmt::now_local_ts;

#[tauri::command]
pub fn send(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    text: String,
    ending: String,
    is_hex: bool,
    port: Option<String>,
) -> Result<usize, String> {
    let conns = state.connections.lock().map_err(|e| e.to_string())?;
    let resolved = resolve_port(&conns, port)?;
    let handle = conns.get(&resolved).ok_or("未连接串口")?;

    // 解析数据
    let mut data = if is_hex {
        hex_to_bytes(&text)?
    } else {
        ascii_to_bytes(&text)
    };

    // 追加行尾
    let line_ending = match ending.as_str() {
        "Cr" => LineEnding::Cr,
        "Lf" => LineEnding::Lf,
        "Crlf" => LineEnding::Crlf,
        _ => LineEnding::None,
    };
    line_ending.append_bytes(&mut data);

    let len = data.len();

    // 乐观回显：入队前先 emit tx-line + 写文件日志。
    // 串口写入受驱动延迟影响（USB 延迟定时器，实测每条 20-60ms 甚至更久），
    // 若等写入完成再回显，界面会显得卡顿。这里发起即回显，写线程静默写。
    emit_tx_line(&app_handle, &handle.rx_history, data.clone());

    // 通过 channel 发送给写线程（SendSilent：写线程只写不 emit，避免重复回显）
    handle.write_tx.send(WriteCommand::SendSilent(data))
        .map_err(|e| format!("发送队列写入失败: {}", e))?;

    Ok(len)
}

/// 发送方向的即时回显 + 文件日志。与脚本序列（sequence.rs）共用同一逻辑。
/// - log_send=false 时不回显也不记录（"记录发送"开关）
/// - 与 write 线程解耦：发起即打时间戳，不受串口写阻塞影响
pub fn emit_tx_line(app_handle: &tauri::AppHandle, rx_history: &Arc<RxHistory>, data: Vec<u8>) {
    let (cfg, sender) = match app_handle.try_state::<AppState>() {
        Some(state) => {
            let cfg = state.settings.lock().ok().map(|s| DisplayConfig {
                show_timestamp: s.ui.show_timestamp,
                log_send: s.ui.log_send,
            });
            let sender = state
                .line_sender
                .lock()
                .ok()
                .and_then(|ls| ls.as_ref().cloned());
            (cfg, sender)
        }
        None => (None, None),
    };
    let log_send = cfg.map(|c| c.log_send).unwrap_or(true);
    if log_send {
        let line = LogLine::new(now_local_ts(), Dir::Tx, data, &[]);
        let _ = app_handle.emit("tx-line", line.clone());
        // 喂后端收发历史(tx),供 agent 经 get_history_since 读用户/自己发过的命令。
        // 与 emit tx-line 同步:log_send=false 时不进 history(与 GUI 一致,尊重"不记录发送")。
        rx_history.push(Dir::Tx, line.clone());
        if let (Some(c), Some(tx)) = (cfg, sender) {
            let formatted = format_line_for_file(&line, &c, false);
            if !formatted.is_empty() {
                let _ = tx.send(formatted);
            }
        }
    }
}

#[tauri::command]
pub fn send_file(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    path: String,
    port: Option<String>,
) -> Result<usize, String> {
    let write_tx = {
        let conns = state.connections.lock().map_err(|e| e.to_string())?;
        let resolved = resolve_port(&conns, port)?;
        conns.get(&resolved).ok_or("未连接串口")?.write_tx.clone()
    };
    // 锁已在块内释放,send_file 的文件读 + 循环发送不持 connections 锁

    // 获取文件大小
    let file = std::fs::File::open(&path).map_err(|e| format!("打开文件失败: {}", e))?;
    let total_size = file.metadata().map_err(|e| e.to_string())?.len() as usize;
    if total_size == 0 {
        return Ok(0);
    }

    // 分块读取 + 发送，每块 1024 字节
    use std::io::Read;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = [0u8; 1024];
    let mut sent = 0usize;

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        let chunk = buf[..n].to_vec();
        write_tx.send(WriteCommand::Send(chunk))
            .map_err(|e| format!("发送队列写入失败: {}", e))?;
        sent += n;

        // 发送进度事件
        let _ = app_handle.emit("file-send-progress", FileSendProgress {
            sent,
            total: total_size,
        });
    }

    Ok(sent)
}

#[derive(Clone, serde::Serialize)]
pub struct FileSendProgress {
    pub sent: usize,
    pub total: usize,
}
