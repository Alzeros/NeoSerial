use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{State, Emitter, Manager};

use crate::state::AppState;
use crate::connection::WriteCommand;
use crate::util::codec::{LineEnding, hex_to_bytes, ascii_to_bytes};
use crate::buffer::log_line::{Dir, LogLine};
use crate::util::log_format::{DisplayConfig, format_line_for_file};
use crate::util::time_fmt::now_local_ts;

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

                if !data.is_empty() {
                    // 工具侧发起节奏 = delay：在此刻 emit tx-line（发起时刻打时间戳），
                    // 塞 channel 让 writer 异步写，sequence 线程按 delay 间隔继续下一行。
                    // 不等 write 完成，避免被串口驱动写阻塞拖慢发起节奏。
                    let line = LogLine::new(now_local_ts(), Dir::Tx, data.clone(), &[]);
                    // 取显示配置与文件日志 sender
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
                        let _ = app_handle.emit("tx-line", line.clone());
                        if let (Some(c), Some(tx)) = (cfg, sender) {
                            let formatted = format_line_for_file(&line, &c, false);
                            if !formatted.is_empty() {
                                let _ = tx.send(formatted);
                            }
                        }
                    }
                    // 塞 channel，writer 异步写（SendSilent：不 emit，sequence 已 emit）
                    let _ = write_tx.send(WriteCommand::SendSilent(data));
                }

                // 行间延时：sequence 线程按 delay 间隔发起下一条，不受 writer 写阻塞影响
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

/// 保存序列配置。data 为前端序列化后的 JSON（当前是 ScriptModule[]）。
/// 用 serde_json::Value 透传，不锁定具体结构，便于后续扩展模块类型。
#[tauri::command]
pub fn save_sequence_config(
    path: String,
    data: Vec<serde_json::Value>,
) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(())
}

/// 加载序列配置。返回原始 JSON 数组，结构判定/旧格式迁移由前端完成
/// （旧文件是裸 ScriptPage[]，前端检测无 id/pages 字段则包进默认模块）。
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
