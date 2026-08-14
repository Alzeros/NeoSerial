use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::time::timeout;

use crate::commands::send::emit_tx_line;
use crate::connection::{spawn_connection, SerialParams, WriteCommand};
use crate::util::codec::{ascii_to_bytes, hex_to_bytes, LineEnding};

use super::state::McpShared;

// ============ 通用响应 ============

/// 所有工具的统一错误返回:{ ok: false, error } 。成功 { ok: true, ... }。
/// 不走 MCP 协议级 error,让 agent 从结果文本读到可操作中文错误。
#[derive(Clone, Serialize)]
pub struct ErrorResp {
    pub ok: bool,
    pub error: String,
}

impl ErrorResp {
    pub fn new(msg: impl Into<String>) -> Self {
        ErrorResp { ok: false, error: msg.into() }
    }
}

// ============ list_ports ============

#[derive(Serialize)]
pub struct ListPortsResp {
    pub ok: bool,
    pub ports: Vec<String>,
}

pub fn list_ports(_shared: &McpShared) -> ListPortsResp {
    let ports = crate::commands::connection::list_ports_inner();
    ListPortsResp { ok: true, ports }
}

// ============ connect ============

#[derive(Deserialize)]
pub struct ConnectReq {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: String,   // "Five".."Eight"
    pub parity: String,     // "None"|"Odd"|"Even"
    pub stop_bits: u8,       // 1|2
    pub flow_control: String, // "None"|"Software"|"Hardware"
}

#[derive(Serialize)]
pub struct ConnectResp {
    pub ok: bool,
    pub mode: Option<String>,
}

// ============ disconnect ============

pub fn disconnect(shared: &McpShared, app_handle: &tauri::AppHandle) -> Result<(), String> {
    let handle = {
        let mut conn = shared.connection.lock().map_err(|e| e.to_string())?;
        conn.take()
    };
    if let Some(handle) = handle {
        let _ = handle.write_tx.send(WriteCommand::Close);
        handle.running.store(false, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut e) = handle.exit.0.lock() {
            if !*e {
                let _ = handle.exit.1.wait_timeout(e, std::time::Duration::from_secs(1));
            }
        }
    }
    let _ = app_handle.emit(
        "connection-state",
        crate::connection::ConnectionState { connected: false, port: None, baud_rate: None },
    );
    // 更新 registry:com/connected 清空(辅助发现机制,失败忽略)
    if let Some(reg) = shared.registry.as_ref() {
        let _ = reg.update_com(None, None, false);
    }
    Ok(())
}

/// 连接串口(复用 spawn_connection + 端口占用文案)。
/// spawn_connection 内部会 emit connection-state/connection-mode,前端原样接收。
pub fn connect(shared: &McpShared, req: ConnectReq) -> Result<ConnectResp, ErrorResp> {
    let params = SerialParams {
        port: req.port.clone(),
        baud_rate: req.baud_rate,
        data_bits: parse_data_bits(&req.data_bits),
        parity: parse_parity(&req.parity),
        stop_bits: if req.stop_bits == 2 {
            crate::config::settings::StopBits::Two
        } else {
            crate::config::settings::StopBits::One
        },
        flow_control: parse_flow_control(&req.flow_control),
    };
    // 已连接则报错(单连接)
    {
        let conn = shared.connection.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
        if conn.is_some() {
            return Err(ErrorResp::new("已连接,请先 disconnect"));
        }
    }
    match spawn_connection(params, shared.app_handle.clone()) {
        Ok((handle, mode)) => {
            {
                let mut conn = shared.connection.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
                *conn = Some(handle);
            }
            // 更新 registry 的 com/connected(锁已释放,避免持锁做文件 IO)
            // update_com 失败忽略:registry 只是辅助发现机制,不该阻塞 connect
            if let Some(reg) = shared.registry.as_ref() {
                let _ = reg.update_com(Some(req.port.clone()), Some(req.baud_rate), true);
            }
            Ok(ConnectResp { ok: true, mode: Some(mode) })
        }
        Err(e) => {
            let lower = e.to_lowercase();
            let port_busy = lower.contains("access is denied")
                || lower.contains("being used by another process")
                || lower.contains("accessdenied")
                || lower.contains("permission");
            if port_busy {
                Err(ErrorResp::new("端口被占用,可能上次连接未完全释放,请稍后重试"))
            } else {
                Err(ErrorResp::new(e))
            }
        }
    }
}

fn parse_data_bits(s: &str) -> crate::config::settings::DataBits {
    use crate::config::settings::DataBits;
    match s {
        "Five" => DataBits::Five,
        "Six" => DataBits::Six,
        "Seven" => DataBits::Seven,
        _ => DataBits::Eight,
    }
}
fn parse_parity(s: &str) -> crate::config::settings::Parity {
    use crate::config::settings::Parity;
    match s {
        "Odd" => Parity::Odd,
        "Even" => Parity::Even,
        _ => Parity::None,
    }
}
fn parse_flow_control(s: &str) -> crate::config::settings::FlowControl {
    use crate::config::settings::FlowControl;
    match s {
        "Software" => FlowControl::Software,
        "Hardware" => FlowControl::Hardware,
        _ => FlowControl::None,
    }
}

// ============ get_status ============

#[derive(Serialize)]
pub struct StatusResp {
    pub ok: bool,
    pub connected: bool,
    pub port: Option<String>,
    pub baud: Option<u32>,
    pub tx_bytes: Option<u64>,
    pub rx_bytes: Option<u64>,
    /// 收发历史当前最大序号(tx+rx 共用同一计数器)。
    pub seq: u64,
}

pub fn get_status(shared: &McpShared) -> StatusResp {
    let conn = shared.connection.lock().ok();
    let (connected, port, baud, tx_bytes, rx_bytes) = match conn {
        Some(g) => match g.as_ref() {
            Some(h) => (
                true,
                Some(h.port.clone()),
                Some(h.baud),
                Some(h.tx_bytes.load(std::sync::atomic::Ordering::SeqCst)),
                Some(h.rx_bytes.load(std::sync::atomic::Ordering::SeqCst)),
            ),
            None => (false, None, None, None, None),
        },
        None => (false, None, None, None, None),
    };
    StatusResp {
        ok: true,
        connected,
        port,
        baud,
        tx_bytes,
        rx_bytes,
        seq: shared.rx_history.latest_seq(),
    }
}

// ============ send / send_and_read ============

#[derive(Deserialize)]
pub struct SendReq {
    pub text: String,
    pub ending: Option<String>,  // 默认 "Crlf"
    pub is_hex: Option<bool>,    // 默认 false
}

#[derive(Serialize)]
pub struct SendResp {
    pub ok: bool,
    pub sent: bool,
}

#[derive(Deserialize)]
pub struct SendAndReadReq {
    pub text: String,
    pub timeout_ms: u64,
    pub ending: Option<String>,
    pub is_hex: Option<bool>,
}

#[derive(Serialize)]
pub struct SendAndReadResp {
    pub ok: bool,
    pub sent: bool,
    pub responses: Vec<String>,
    pub timed_out: bool,
    pub seq0: u64,
    pub seq1: u64,
}

/// 解析发送数据:text + ending + is_hex → Vec<u8>。
/// 复用 commands/send.rs 的解析逻辑(ascii_to_bytes / hex_to_bytes / LineEnding)。
fn parse_send_data(text: &str, ending: &Option<String>, is_hex: &Option<bool>) -> Result<Vec<u8>, String> {
    let is_hex = is_hex.unwrap_or(false);
    let mut data = if is_hex {
        hex_to_bytes(text)?
    } else {
        ascii_to_bytes(text)
    };
    let le = match ending.as_deref().unwrap_or("Crlf") {
        "Cr" => LineEnding::Cr,
        "Lf" => LineEnding::Lf,
        "Crlf" => LineEnding::Crlf,
        _ => LineEnding::None,
    };
    le.append_bytes(&mut data);
    Ok(data)
}

/// 发送(复用 write_tx + emit_tx_line),不读。返回发送字节数语义的 sent=true。
fn do_send(shared: &McpShared, data: Vec<u8>) -> Result<(), String> {
    let conn = shared.connection.lock().map_err(|e| e.to_string())?;
    let handle = conn.as_ref().ok_or("未连接串口")?;
    emit_tx_line(&shared.app_handle, data.clone());
    handle.write_tx.send(WriteCommand::SendSilent(data)).map_err(|e| format!("发送队列写入失败: {}", e))?;
    Ok(())
}

pub fn send(shared: &McpShared, req: SendReq) -> Result<SendResp, ErrorResp> {
    let data = match parse_send_data(&req.text, &req.ending, &req.is_hex) {
        Ok(d) => d,
        Err(e) => return Err(ErrorResp::new(e)),
    };
    match do_send(shared, data) {
        Ok(_) => Ok(SendResp { ok: true, sent: true }),
        Err(e) => Err(ErrorResp::new(e)),
    }
}

// ============ get_rx_since ============

#[derive(Deserialize)]
pub struct GetHistorySinceReq {
    pub seq: u64,
    pub max_lines: Option<usize>,
    /// true 时每行 text 返回 hex dump(非可打印字节不丢);默认 false 返回 ascii。
    /// 纯 hex 数据(如 AA 0D 0A)在 ascii 模式下会显示空,agent 应切 is_hex=true 重查。
    pub is_hex: Option<bool>,
}

/// 历史行:方向(rx/tx)+ 文本(按 is_hex 选 ascii/hex)。
#[derive(Serialize)]
pub struct HistoryLine {
    pub dir: String,
    pub text: String,
}

#[derive(Serialize)]
pub struct GetHistorySinceResp {
    pub ok: bool,
    pub lines: Vec<HistoryLine>,
    pub latest_seq: u64,
}

/// 拉取序号 > seq 的全部历史行(tx+rx,带方向)。供 agent 主动查对话流/异步 URC。
pub fn get_history_since(shared: &McpShared, req: GetHistorySinceReq) -> GetHistorySinceResp {
    let is_hex = req.is_hex.unwrap_or(false);
    let (rows, latest) = shared.rx_history.since(req.seq);
    let mut lines: Vec<HistoryLine> = rows
        .into_iter()
        .map(|(_, dir, l)| HistoryLine {
            dir: match dir {
                crate::buffer::log_line::Dir::Rx => "rx".to_string(),
                crate::buffer::log_line::Dir::Tx => "tx".to_string(),
            },
            text: if is_hex { l.hex } else { l.ascii },
        })
        .collect();
    if let Some(max) = req.max_lines {
        // 最旧优先:截断保留前 max 条(先进先出,不跳过早期数据)
        if lines.len() > max {
            lines.truncate(max);
        }
    }
    GetHistorySinceResp { ok: true, lines, latest_seq: latest }
}

// ============ send_and_read (async) ============

/// 发后阻塞读:发数据 → 记 seq0 → async 等 timeout 内的新 rx 行。
/// 超时语义:静默间隙超时——每次收到新行刷新截止时间,持续有数据时阻塞可远超 timeout_ms。
#[allow(dead_code)] // 在 Task 10 的 server 注册前暂未被调用
pub async fn send_and_read(shared: &McpShared, req: SendAndReadReq) -> Result<SendAndReadResp, ErrorResp> {
    let data = match parse_send_data(&req.text, &req.ending, &req.is_hex) {
        Ok(d) => d,
        Err(e) => return Err(ErrorResp::new(e)),
    };
    // 先记基准 seq0(发之前),这样发送后到达的响应都 > seq0
    let seq0 = shared.rx_history.latest_seq();
    if let Err(e) = do_send(shared, data) {
        return Err(ErrorResp::new(e));
    }
    let gap = Duration::from_millis(req.timeout_ms.max(50));

    let mut responses: Vec<String> = Vec::new();
    let mut cur_seq = seq0;
    let mut timed_out = false;

    loop {
        // 先查:有没有 seq > cur_seq 的新行(tx+rx 都会触发,但只收集 rx)
        let (rows, latest) = shared.rx_history.since(cur_seq);
        let new_rx: Vec<_> = rows.iter()
            .filter(|(_, dir, _)| *dir == crate::buffer::log_line::Dir::Rx)
            .collect();
        if !new_rx.is_empty() {
            for (_, _, l) in &new_rx {
                let text = if req.is_hex.unwrap_or(false) { &l.hex } else { &l.ascii };
                responses.push(text.clone());
            }
            cur_seq = latest; // 推进到最新(含已跳过的 tx)
            continue; // 拿到新 rx,刷新静默窗口:回循环顶重新等 gap
        }
        // 无新 rx 行:等 Notify,带 gap 超时
        let notified = shared.rx_history.notify().notified();
        match timeout(gap, notified).await {
            Ok(_) => {
                // 被唤醒(有新行 push,可能是 tx 或 rx),回循环顶重查
                continue;
            }
            Err(_) => {
                // 静默超时:再查一次(防止唤醒与查询的竞态漏数据)
                let (rows, latest) = shared.rx_history.since(cur_seq);
                let new_rx: Vec<_> = rows.iter()
                    .filter(|(_, dir, _)| *dir == crate::buffer::log_line::Dir::Rx)
                    .collect();
                if !new_rx.is_empty() {
                    for (_, _, l) in &new_rx {
                        let text = if req.is_hex.unwrap_or(false) { &l.hex } else { &l.ascii };
                        responses.push(text.clone());
                    }
                    cur_seq = latest;
                    continue;
                }
                timed_out = true;
                break;
            }
        }
    }

    Ok(SendAndReadResp {
        ok: true,
        sent: true,
        responses,
        timed_out,
        seq0,
        seq1: cur_seq,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_send_ascii_default_crlf() {
        let d = parse_send_data("AT", &None, &None).unwrap();
        assert_eq!(d, b"AT\r\n");
    }

    #[test]
    fn test_parse_send_hex() {
        let d = parse_send_data("41 54", &Some("None".into()), &Some(true)).unwrap();
        assert_eq!(d, b"AT");
    }

    #[test]
    fn test_parse_send_lf_only() {
        let d = parse_send_data("OK", &Some("Lf".into()), &None).unwrap();
        assert_eq!(d, b"OK\n");
    }

    #[test]
    fn test_parse_send_bad_hex_err() {
        assert!(parse_send_data("ZZ", &None, &Some(true)).is_err());
    }

    #[test]
    fn test_parse_send_empty_text_still_adds_ending() {
        let d = parse_send_data("", &Some("Crlf".into()), &None).unwrap();
        assert_eq!(d, b"\r\n");
    }
}
