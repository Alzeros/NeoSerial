use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tokio::time::timeout;

use crate::buffer::rx_history::RxHistory;
use crate::commands::send::emit_tx_line;
use crate::connection::{spawn_connection, SerialParams, WriteCommand};
use crate::util::codec::{ascii_to_bytes, hex_to_bytes, LineEnding};

use super::registry::ConnInfo;
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

#[derive(Deserialize)]
pub struct DisconnectReq {
    pub port: String,
}

pub fn disconnect(shared: &McpShared, app_handle: &tauri::AppHandle, req: DisconnectReq) -> Result<(), String> {
    // 先从 map 取出 handle 并放锁,再等 exit(持 connections 锁等 1s 会阻塞所有其他操作,死锁风险)。
    // 在同一锁作用域内构造剩余连接快照(handle 移除后剩余 entries),避免二次取锁且快照与移除原子一致。
    let (handle, snapshot): (Option<crate::connection::ConnectionHandle>, Vec<ConnInfo>) = {
        let mut conns = shared.connections.lock().map_err(|e| e.to_string())?;
        let removed = conns.remove(&req.port);
        let snap = conns.values()
            .map(|h| ConnInfo { com: h.port.clone(), baud: h.baud })
            .collect();
        (removed, snap)
    };
    // window_label 从 handle 读(可能被 GUI 接管改过);无 handle 兜底 win-{port}
    let window_label = handle.as_ref()
        .and_then(|h| h.window_label.read().ok().map(|g| g.clone()))
        .unwrap_or_else(|| crate::connection::port_to_label(&req.port));
    if let Some(handle) = handle {
        let _ = handle.write_tx.send(WriteCommand::Close);
        handle.running.store(false, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut e) = handle.exit.0.lock() {
            if !*e {
                let _ = handle.exit.1.wait_timeout(e, std::time::Duration::from_secs(1));
            }
        }
    }
    let _ = app_handle.emit_to(
        &window_label,
        "connection-state",
        crate::connection::ConnectionState { connected: false, port: Some(req.port.clone()), baud_rate: None },
    );
    // 更新 registry:用当前剩余连接的完整快照整体替换(辅助发现机制,失败忽略)。
    if let Some(reg) = shared.registry.as_ref() {
        let _ = reg.update_connections(snapshot);
    }
    // 通知所有 GUI 窗口刷新"待接管"chip:agent 断了某端口,chip 应消失。
    let _ = shared.app_handle.emit("mcp-connections-changed", ());
    Ok(())
}

/// 连接串口(复用 spawn_connection + 端口占用文案)。
/// **复用语义**:若该 port 已被(GUI 或前次 MCP)连接且仍存活,直接复用返回 ok——
/// 不报冲突、不重建。agent 与人共享同一连接:agent 发命令的事件显示在 GUI 窗口
/// (用该连接的 window_label),GUI 能看到;agent 读同一份 rx_history。
/// 这符合 CLAUDE.md "NeoSerial 持有串口,agent 不直接占端口"——MCP 操作已有连接。
///
/// **僵尸检测**:若 port 已存在但 running=false(reader/writer 已退出,如拔线),
/// 说明是 monitoring 线程还没来得及清理的僵尸 handle,remove 后重建,避免 agent
/// 操作死连接。
pub fn connect(shared: &McpShared, req: ConnectReq) -> Result<ConnectResp, ErrorResp> {
    let mut conns = shared.connections.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
    if let Some(existing) = conns.get(&req.port) {
        // 僵尸:running=false → remove 重建(不直接返回 ok,否则操作的是死连接)
        if !existing.running.load(std::sync::atomic::Ordering::SeqCst) {
            conns.remove(&req.port);
        } else {
            // 复用:port 已连且仍存活 → 直接返回 ok(不重建)。GUI 连的 agent 能操作,反之亦然。
            return Ok(ConnectResp {
                ok: true,
                mode: Some("independent".to_string()),
            });
        }
    }
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
    // MCP 先连(GUI 没连)的连接:window_label = mcp-{port}。
    // 若用户后来在 GUI 窗口连同 port,GUI connect 应复用并把 window_label 改成该窗口 label
    // (见 Tauri connect 的复用逻辑)。这里只处理 MCP 首次建连。
    let window_label = format!("mcp-{}", req.port);
    let (handle, mode) = match spawn_connection(params, shared.app_handle.clone(), window_label, shared.connections.clone(), shared.registry.clone(), true) {
        Ok(h) => h,
        Err(e) => {
            let lower = e.to_lowercase();
            let port_busy = lower.contains("access is denied")
                || lower.contains("being used by another process")
                || lower.contains("accessdenied")
                || lower.contains("permission");
            if port_busy {
                return Err(ErrorResp::new("端口被占用,可能上次连接未完全释放,请稍后重试"));
            } else {
                return Err(ErrorResp::new(e));
            }
        }
    };
    conns.insert(req.port.clone(), handle);
    // 锁仍持有:构造当前所有连接的完整快照(含刚 insert 的这条),用于整体更新 registry。
    let snapshot: Vec<ConnInfo> = conns.values()
        .map(|h| ConnInfo { com: h.port.clone(), baud: h.baud })
        .collect();
    drop(conns);
    if let Some(reg) = shared.registry.as_ref() {
        let _ = reg.update_connections(snapshot);
    }
    // 通知所有 GUI 窗口刷新"待接管"chip 列表:agent 新连了端口,可能需要 GUI 接管。
    // 用全局 emit:chip 是全局连接视图,所有窗口都要看到变化。
    let _ = shared.app_handle.emit("mcp-connections-changed", ());
    Ok(ConnectResp { ok: true, mode: Some(mode) })
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
    /// 收发历史当前最大序号(tx+rx 共用同一计数器,per-conn)。
    pub seq: u64,
}

#[derive(Deserialize)]
pub struct StatusReq {
    pub port: String,
}

pub fn get_status(shared: &McpShared, req: StatusReq) -> StatusResp {
    let conns = shared.connections.lock().ok();
    let (connected, port, baud, tx_bytes, rx_bytes, seq) = match conns.as_ref() {
        Some(g) => match g.get(&req.port) {
            Some(h) => (
                true,
                Some(h.port.clone()),
                Some(h.baud),
                Some(h.tx_bytes.load(std::sync::atomic::Ordering::SeqCst)),
                Some(h.rx_bytes.load(std::sync::atomic::Ordering::SeqCst)),
                h.rx_history.latest_seq(),
            ),
            None => (false, None, None, None, None, 0),
        },
        None => (false, None, None, None, None, 0),
    };
    StatusResp {
        ok: true,
        connected,
        port,
        baud,
        tx_bytes,
        rx_bytes,
        seq,
    }
}

// ============ send / send_and_read ============

#[derive(Deserialize)]
pub struct SendReq {
    pub port: String,
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
    pub port: String,
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

/// 发送(复用 write_tx + emit_tx_line),不读。返回该 port 的 rx_history Arc,
/// 供 send_and_read 后续 latest_seq/since/notify 使用——**不持有 connections 锁跨 await**。
/// 锁只在取 handle + push tx + 发写命令的同步段持有,返回前释放;send_and_read
/// 后续用返回的 Arc 直接调 rx_history 的方法,无需再锁 connections(避免长 await 持锁)。
fn do_send(shared: &McpShared, port: &str, data: Vec<u8>) -> Result<Arc<RxHistory>, String> {
    let conn = shared.connections.lock().map_err(|e| e.to_string())?;
    let handle = conn.get(port).ok_or_else(|| format!("端口 {} 未连接", port))?;
    emit_tx_line(&shared.app_handle, &handle.rx_history, &handle.window_label, &handle.line_index, data.clone());
    handle.write_tx.send(WriteCommand::SendSilent(data)).map_err(|e| format!("发送队列写入失败: {}", e))?;
    Ok(handle.rx_history.clone())
}

pub fn send(shared: &McpShared, req: SendReq) -> Result<SendResp, ErrorResp> {
    let data = match parse_send_data(&req.text, &req.ending, &req.is_hex) {
        Ok(d) => d,
        Err(e) => return Err(ErrorResp::new(e)),
    };
    match do_send(shared, &req.port, data) {
        Ok(_) => Ok(SendResp { ok: true, sent: true }),
        Err(e) => Err(ErrorResp::new(e)),
    }
}

// ============ get_rx_since ============

#[derive(Deserialize)]
pub struct GetHistorySinceReq {
    pub port: String,
    pub seq: u64,
    pub max_lines: Option<usize>,
    /// true 时每行 text 返回 hex dump(非可打印字节不丢);默认 false 返回 ascii。
    /// 纯 hex 数据(如 AA 0D 0A)在 ascii 模式下会显示空,agent 应切 is_hex=true 重查。
    pub is_hex: Option<bool>,
}

/// 历史行:方向(rx/tx)+ 文本(按 is_hex 选 ascii/hex)+ 行号(index)。
#[derive(Serialize)]
pub struct HistoryLine {
    pub dir: String,
    pub text: String,
    /// 本次连接期间的行号(tx+rx 共用,开端口=1)。
    /// 旧历史(连接前已存的,line_index=0)按返回顺序回填递增,保证连续可读。
    pub index: u64,
}

#[derive(Serialize)]
pub struct GetHistorySinceResp {
    pub ok: bool,
    pub lines: Vec<HistoryLine>,
    pub latest_seq: u64,
}

/// 拉取序号 > seq 的全部历史行(tx+rx,带方向)。供 agent 主动查对话流/异步 URC。
/// 用指定 port 的 per-conn rx_history:多连接不串数据。
pub fn get_history_since(shared: &McpShared, req: GetHistorySinceReq) -> GetHistorySinceResp {
    let is_hex = req.is_hex.unwrap_or(false);
    // 取该 port 的 per-conn rx_history(锁只用于取 Arc,不跨 await)
    let rx_history = match shared.connections.lock() {
        Ok(conns) => match conns.get(&req.port) {
            Some(h) => h.rx_history.clone(),
            None => {
                return GetHistorySinceResp {
                    ok: false,
                    lines: Vec::new(),
                    latest_seq: 0,
                };
            }
        },
        Err(_) => {
            return GetHistorySinceResp {
                ok: false,
                lines: Vec::new(),
                latest_seq: 0,
            };
        }
    };
    let (rows, latest) = rx_history.since(req.seq);
    // 旧历史(连接前,line_index=0)按返回顺序回填递增,保证连续;
    // 新数据(连接后)用真实 line_index。
    let mut fill_idx: u64 = 0;
    let mut lines: Vec<HistoryLine> = rows
        .into_iter()
        .map(|(_, dir, l)| {
            let idx = if l.line_index == 0 {
                fill_idx += 1;
                fill_idx
            } else {
                l.line_index
            };
            HistoryLine {
                dir: match dir {
                    crate::buffer::log_line::Dir::Rx => "rx".to_string(),
                    crate::buffer::log_line::Dir::Tx => "tx".to_string(),
                },
                text: if is_hex { l.hex } else { l.ascii },
                index: idx,
            }
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
///
/// **断连超时语义(per-conn history)**: send_and_read 阻塞在
/// `rx_history.notify().timeout(...)` 期间,若该 port 被 disconnect,reader 线程
/// 退出不再 push,notify 不再触发 → 最终静默超时返回空 responses(无死锁)。
/// 与旧全局 history 行为差异:旧实现读全局 history,disconnect 后全局仍可能被
/// 其他连接的 reader push 唤醒(数据串连);新实现 per-conn history 隔离,断连后
/// 该 history 静默,纯超时返回空——符合多连接隔离语义。
pub async fn send_and_read(shared: &McpShared, req: SendAndReadReq) -> Result<SendAndReadResp, ErrorResp> {
    let data = match parse_send_data(&req.text, &req.ending, &req.is_hex) {
        Ok(d) => d,
        Err(e) => return Err(ErrorResp::new(e)),
    };
    // do_send 内取该 port 的 per-conn rx_history 并返回 Arc,锁不跨 await。
    let rx_history = match do_send(shared, &req.port, data) {
        Ok(h) => h,
        Err(e) => return Err(ErrorResp::new(e)),
    };
    // 先记基准 seq0(发之前),这样发送后到达的响应都 > seq0
    let seq0 = rx_history.latest_seq();
    let gap = Duration::from_millis(req.timeout_ms.max(50));

    let mut responses: Vec<String> = Vec::new();
    let mut cur_seq = seq0;
    let mut timed_out = false;

    loop {
        // 先查:有没有 seq > cur_seq 的新行(tx+rx 都会触发,但只收集 rx)
        let (rows, latest) = rx_history.since(cur_seq);
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
        let notified = rx_history.notify().notified();
        match timeout(gap, notified).await {
            Ok(_) => {
                // 被唤醒(有新行 push,可能是 tx 或 rx),回循环顶重查
                continue;
            }
            Err(_) => {
                // 静默超时:再查一次(防止唤醒与查询的竞态漏数据)
                let (rows, latest) = rx_history.since(cur_seq);
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

// ============ reset_stats ============

#[derive(Deserialize)]
pub struct ResetStatsReq {
    pub port: String,
}

#[derive(Serialize)]
pub struct ResetStatsResp {
    pub ok: bool,
}

/// 清零指定 port 的收发字节统计。
pub fn reset_stats(shared: &McpShared, req: ResetStatsReq) -> ResetStatsResp {
    let conns = match shared.connections.lock() {
        Ok(c) => c,
        Err(_) => return ResetStatsResp { ok: false },
    };
    if let Some(h) = conns.get(&req.port) {
        h.tx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
        h.rx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
        ResetStatsResp { ok: true }
    } else {
        ResetStatsResp { ok: false }
    }
}

// ============ send_file ============

#[derive(Deserialize)]
pub struct SendFileReq {
    pub port: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct SendFileResp {
    pub ok: bool,
    pub sent: usize,
}

/// 发送文件(二进制安全)。分块 1024 字节写入,返回已发送字节数。
pub fn send_file(shared: &McpShared, req: SendFileReq) -> Result<SendFileResp, ErrorResp> {
    let write_tx = {
        let conns = shared.connections.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
        conns.get(&req.port)
            .ok_or_else(|| ErrorResp::new(format!("端口 {} 未连接", req.port)))?
            .write_tx.clone()
    };
    let file = std::fs::File::open(&req.path).map_err(|e| ErrorResp::new(format!("打开文件失败: {}", e)))?;
    let total = file.metadata().map_err(|e| ErrorResp::new(e.to_string()))?.len() as usize;
    if total == 0 {
        return Ok(SendFileResp { ok: true, sent: 0 });
    }
    use std::io::Read;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = [0u8; 1024];
    let mut sent = 0usize;
    loop {
        let n = reader.read(&mut buf).map_err(|e| ErrorResp::new(format!("读取文件失败: {}", e)))?;
        if n == 0 { break; }
        let chunk = buf[..n].to_vec();
        write_tx.send(WriteCommand::Send(chunk))
            .map_err(|e| ErrorResp::new(format!("发送队列写入失败: {}", e)))?;
        sent += n;
    }
    Ok(SendFileResp { ok: true, sent })
}

// ============ logging (全局,不接 port) ============

#[derive(Deserialize)]
pub struct StartLoggingReq {
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct StartLoggingResp {
    pub ok: bool,
    pub path: String,
}

/// 开始存盘(全局,所有连接的收发都进同一文件)。传 path 新建/覆盖,不传用上次路径续写。
pub fn start_logging(shared: &McpShared, req: StartLoggingReq) -> Result<StartLoggingResp, ErrorResp> {
    use crate::logging::file_logger::FileLogger;
    use crate::util::time_fmt::now_local_compact;
    let state = shared.app_handle.try_state::<crate::state::AppState>()
        .ok_or_else(|| ErrorResp::new("无法访问应用状态"))?;
    let (actual_path, append) = match req.path {
        Some(p) => (p, false),
        None => {
            let last = state.last_log_path.lock().ok().and_then(|g| g.clone());
            match last {
                Some(p) => (p, true),
                None => {
                    let filename = format!("{}.log", now_local_compact());
                    let appdata = std::env::var("APPDATA")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::path::PathBuf::from("."));
                    let p = appdata.join("neoserial").join("logs").join(filename)
                        .to_string_lossy().to_string();
                    (p, false)
                }
            }
        }
    };
    let (error_tx, _error_rx) = crossbeam_channel::unbounded();
    let logger = FileLogger::start(
        std::path::PathBuf::from(&actual_path), error_tx, append,
    ).map_err(|e| ErrorResp::new(format!("启动日志失败: {}", e)))?;
    {
        let mut last = state.last_log_path.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
        *last = Some(actual_path.clone());
    }
    let sender = logger.line_sender();
    {
        let mut fl = state.file_logger.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
        *fl = Some(logger);
    }
    {
        let mut ss = state.line_sender.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
        *ss = Some(sender);
    }
    Ok(StartLoggingResp { ok: true, path: actual_path })
}

#[derive(Serialize)]
pub struct StopLoggingResp {
    pub ok: bool,
}

/// 停止存盘。
pub fn stop_logging(shared: &McpShared) -> Result<StopLoggingResp, ErrorResp> {
    let state = shared.app_handle.try_state::<crate::state::AppState>()
        .ok_or_else(|| ErrorResp::new("无法访问应用状态"))?;
    if let Ok(mut ss) = state.line_sender.lock() {
        *ss = None;
    }
    let mut fl = state.file_logger.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
    if let Some(logger) = fl.take() {
        logger.stop();
    }
    Ok(StopLoggingResp { ok: true })
}

// ============ sequence_run / sequence_stop ============

/// MCP 简化版序列命令:command 必填,其余可选(enabled 默认 true,enter 默认 true)。
#[derive(Deserialize)]
pub struct McpSequenceCommand {
    pub command: String,
    #[serde(default)]
    pub hex: bool,
    #[serde(default = "default_true")]
    pub enter: bool,
    #[serde(default)]
    pub delay_ms: u32,
}
fn default_true() -> bool { true }

#[derive(Deserialize)]
pub struct SequenceRunReq {
    pub port: String,
    pub commands: Vec<McpSequenceCommand>,
    pub run_count: u32,
    #[serde(default)]
    pub loop_interval: u32,
}

#[derive(Serialize)]
pub struct SequenceRunResp {
    pub ok: bool,
}

/// 跑脚本序列:多条命令批量下发,支持循环(run_count)和行间延时。与 GUI sequence_run 同逻辑。
pub fn sequence_run(shared: &McpShared, req: SequenceRunReq) -> Result<SequenceRunResp, ErrorResp> {
    use crate::commands::sequence::{sequence_run_inner, SequenceCommand};
    // 转 GUI 的 SequenceCommand
    let cmds: Vec<SequenceCommand> = req.commands.into_iter().map(|c| SequenceCommand {
        enabled: true,
        command: c.command,
        hex: c.hex,
        enter: c.enter,
        delay_ms: c.delay_ms,
    }).collect();
    sequence_run_inner(shared.app_handle.clone(), &shared.connections, &req.port, cmds, req.run_count, req.loop_interval)
        .map(|_| SequenceRunResp { ok: true })
        .map_err(ErrorResp::new)
}

#[derive(Deserialize)]
pub struct SequenceStopReq {
    pub port: String,
}

#[derive(Serialize)]
pub struct SequenceStopResp {
    pub ok: bool,
}

/// 停止指定 port 的运行序列。
pub fn sequence_stop(shared: &McpShared, req: SequenceStopReq) -> Result<SequenceStopResp, ErrorResp> {
    use crate::commands::sequence::sequence_stop_inner;
    sequence_stop_inner(&shared.connections, &req.port)
        .map(|_| SequenceStopResp { ok: true })
        .map_err(ErrorResp::new)
}

// ============ get_settings / save_settings ============

#[derive(Serialize)]
pub struct GetSettingsResp {
    pub ok: bool,
    pub settings: Option<crate::config::settings::Settings>,
}

/// 读当前配置(波特率预设/显示模式/编码/MCP 设置等)。
pub fn get_settings(shared: &McpShared) -> GetSettingsResp {
    let state = match shared.app_handle.try_state::<crate::state::AppState>() {
        Some(s) => s,
        None => return GetSettingsResp { ok: false, settings: None },
    };
    let s = state.settings.lock().ok().map(|g| g.clone());
    GetSettingsResp { ok: s.is_some(), settings: s }
}

#[derive(Deserialize)]
pub struct SaveSettingsReq {
    pub settings: crate::config::settings::Settings,
}

#[derive(Serialize)]
pub struct SaveSettingsResp {
    pub ok: bool,
}

/// 保存配置(持久化到 settings.json)。改后部分项(如 MCP 端口)需重启生效。
pub fn save_settings(shared: &McpShared, req: SaveSettingsReq) -> Result<SaveSettingsResp, ErrorResp> {
    let state = shared.app_handle.try_state::<crate::state::AppState>()
        .ok_or_else(|| ErrorResp::new("无法访问应用状态"))?;
    {
        let mut s = state.settings.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
        *s = req.settings;
    }
    // 落盘(用默认路径 %APPDATA%/neoserial/settings.json)
    let s = state.settings.lock().map_err(|e| ErrorResp::new(e.to_string()))?;
    s.save().map_err(|e| ErrorResp::new(format!("保存失败: {}", e)))?;
    Ok(SaveSettingsResp { ok: true })
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

    // ============ per-connection history 隔离契约 ============

    /// 两个独立 RxHistory,push 到 A 不影响 B —— 验证多连接不串数据。
    /// 这是 Task 4 的核心契约:每个 ConnectionHandle 持自己的 rx_history,
    /// send_and_read/get_history_since 按指定 port 取对应 history,不共用全局。
    #[test]
    fn test_per_conn_history_isolation() {
        use std::sync::Arc;
        use crate::buffer::rx_history::RxHistory;
        use crate::buffer::log_line::{Dir, LogLine};

        fn rx(content: &[u8]) -> LogLine {
            LogLine::new("08:00:00.000".into(), Dir::Rx, content.to_vec(), &[], 0)
        }

        let a = Arc::new(RxHistory::new(100));
        let b = Arc::new(RxHistory::new(100));
        // A push 两行
        a.push(Dir::Rx, rx(b"OK-A1"));
        a.push(Dir::Rx, rx(b"OK-A2"));
        // B push 一行
        b.push(Dir::Rx, rx(b"OK-B1"));

        // A 只有自己的两行,不含 B
        let (rows_a, latest_a) = a.since(0);
        assert_eq!(rows_a.len(), 2);
        assert_eq!(rows_a[0].2.ascii, "OK-A1");
        assert_eq!(rows_a[1].2.ascii, "OK-A2");
        assert_eq!(latest_a, 2);
        // B 只有一行,不含 A
        let (rows_b, latest_b) = b.since(0);
        assert_eq!(rows_b.len(), 1);
        assert_eq!(rows_b[0].2.ascii, "OK-B1");
        assert_eq!(latest_b, 1);
        // seq 各自单调,不共享计数器
        assert_ne!(a.latest_seq(), b.latest_seq());
    }
}
