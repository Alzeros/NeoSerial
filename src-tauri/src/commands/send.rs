use tauri::{State, Emitter};

use crate::state::AppState;
use crate::connection::WriteCommand;
use crate::buffer::log_line::{Dir, LogLine};
use crate::util::codec::{LineEnding, hex_to_bytes, ascii_to_bytes};
use crate::util::time_fmt::now_local_ts;

#[tauri::command]
pub fn send(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    text: String,
    ending: String,
    is_hex: bool,
) -> Result<usize, String> {
    let conn = state.connection.lock().map_err(|e| e.to_string())?;
    let handle = conn.as_ref().ok_or("未连接串口")?;

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

    // 通过 channel 发送给写线程
    handle.write_tx.send(WriteCommand::Send(data.clone()))
        .map_err(|e| format!("发送队列写入失败: {}", e))?;

    // 发送成功后 emit tx-line 事件，让前端显示发送行
    let ts = now_local_ts();
    let tx_line = LogLine::new(ts, Dir::Tx, data, &[]);
    let _ = app_handle.emit("tx-line", tx_line);

    Ok(len)
}

#[tauri::command]
pub fn send_file(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    path: String,
) -> Result<usize, String> {
    let conn = state.connection.lock().map_err(|e| e.to_string())?;
    let handle = conn.as_ref().ok_or("未连接串口")?;
    let write_tx = handle.write_tx.clone();
    drop(conn); // 释放锁，避免阻塞其他操作

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
