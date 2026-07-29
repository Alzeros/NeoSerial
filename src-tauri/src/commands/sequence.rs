use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{State, Emitter};

use crate::state::AppState;
use crate::connection::WriteCommand;
use crate::util::codec::{LineEnding, hex_to_bytes, ascii_to_bytes};

#[derive(Clone, Serialize)]
pub struct SequenceProgress {
    pub row: usize,
    pub total: usize,
}

#[derive(Clone, Serialize)]
pub struct SequenceDone {
    pub aborted: bool,
}

static SEQUENCE_RUNNING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn sequence_run(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    commands: Vec<SequenceCommand>,
    run_count: u32,
    loop_interval: u32,
) -> Result<(), String> {
    if SEQUENCE_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("序列已在运行中".to_string());
    }

    // 从连接句柄取出写通道 sender，移入异步线程
    let write_tx = {
        let conn = state.connection.lock().map_err(|e| e.to_string())?;
        let handle = conn.as_ref().ok_or("未连接串口")?;
        handle.write_tx.clone()
    };

    thread::spawn(move || {
        let mut aborted = false;
        for _ in 0..run_count {
            if !SEQUENCE_RUNNING.load(Ordering::SeqCst) {
                aborted = true;
                break;
            }
            for (i, cmd) in commands.iter().enumerate() {
                if !SEQUENCE_RUNNING.load(Ordering::SeqCst) {
                    aborted = true;
                    break;
                }
                if !cmd.enabled {
                    continue;
                }

                let _ = app_handle.emit("sequence-progress", SequenceProgress {
                    row: i + 1,
                    total: commands.len(),
                });

                // 解析并发送
                let mut data = if cmd.hex {
                    hex_to_bytes(&cmd.command).unwrap_or_default()
                } else {
                    ascii_to_bytes(&cmd.command)
                };
                let line_ending = if cmd.enter {
                    LineEnding::Crlf
                } else {
                    LineEnding::None
                };
                line_ending.append_bytes(&mut data);

                // 通过写线程通道实际发送
                if !data.is_empty() {
                    let _ = write_tx.send(WriteCommand::Send(data));
                }

                if cmd.delay_ms > 0 {
                    thread::sleep(Duration::from_millis(cmd.delay_ms as u64));
                }
            }
            if aborted {
                break;
            }
            if loop_interval > 0 {
                thread::sleep(Duration::from_millis(loop_interval as u64));
            }
        }

        SEQUENCE_RUNNING.store(false, Ordering::SeqCst);
        let _ = app_handle.emit("sequence-done", SequenceDone { aborted });
    });

    Ok(())
}

#[tauri::command]
pub fn sequence_stop() -> Result<(), String> {
    SEQUENCE_RUNNING.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn save_sequence_config(
    path: String,
    pages: Vec<serde_json::Value>,
) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&pages).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_sequence_config(path: String) -> Result<Vec<serde_json::Value>, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
pub struct SequenceCommand {
    pub enabled: bool,
    pub command: String,
    pub hex: bool,
    pub enter: bool,
    pub delay_ms: u32,
}
