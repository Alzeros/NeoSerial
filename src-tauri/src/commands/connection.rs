use std::collections::HashMap;

use tauri::{State, Emitter};

use crate::connection::{ConnectionHandle, SerialParams, spawn_connection, WriteCommand};
use crate::state::AppState;

/// 解析目标端口:供 Tauri command / MCP 工具从 `connections` 取出唯一连接或校验指定端口。
///
/// - `None` + 0 连接:报错(未连接)
/// - `None` + 1 连接:返回该 port(保持单连接行为,前端不传 port 时取唯一)
/// - `None` + 2+ 连接:报"多个连接,请指定端口"
/// - `Some(p)` 存在:返回 p
/// - `Some(p)` 不存在:报"端口 X 未连接"
pub fn resolve_port(
    conns: &HashMap<String, ConnectionHandle>,
    port: Option<String>,
) -> Result<String, String> {
    match port {
        Some(p) => {
            if conns.contains_key(&p) {
                Ok(p)
            } else {
                Err(format!("端口 {} 未连接", p))
            }
        }
        None => {
            if conns.is_empty() {
                Err("未连接串口".to_string())
            } else if conns.len() == 1 {
                Ok(conns.keys().next().unwrap().clone())
            } else {
                Err("多个连接,请指定端口".to_string())
            }
        }
    }
}

#[tauri::command]
pub fn connect(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    port: String,
    baudRate: u32,
    dataBits: String,
    parity: String,
    stopBits: u8,
    flowControl: String,
) -> Result<(), String> {
    let data_bits = match dataBits.as_str() {
        "Five" => crate::config::settings::DataBits::Five,
        "Six" => crate::config::settings::DataBits::Six,
        "Seven" => crate::config::settings::DataBits::Seven,
        _ => crate::config::settings::DataBits::Eight,
    };
    let parity = match parity.as_str() {
        "Odd" => crate::config::settings::Parity::Odd,
        "Even" => crate::config::settings::Parity::Even,
        _ => crate::config::settings::Parity::None,
    };
    let stop_bits = match stopBits {
        2 => crate::config::settings::StopBits::Two,
        _ => crate::config::settings::StopBits::One,
    };
    let flow_control = match flowControl.as_str() {
        "Software" => crate::config::settings::FlowControl::Software,
        "Hardware" => crate::config::settings::FlowControl::Hardware,
        _ => crate::config::settings::FlowControl::None,
    };

    let params = SerialParams {
        port: port.clone(),
        baud_rate: baudRate,
        data_bits,
        parity,
        stop_bits,
        flow_control,
    };

    // 持锁贯穿 check→spawn→insert,防 TOCTOU 竞态:
    // 若 check 与 insert 之间放锁,另一线程可能在此间隙 connect 同一 port,
    // 双方都通过 contains_key 检查 → 双 spawn → 连接泄漏。全程持同一把锁串行化。
    let mut conns = state.connections.lock().map_err(|e| e.to_string())?;
    if conns.contains_key(&port) {
        return Err(format!("端口 {} 已连接", port));
    }
    let (handle, _mode) = match spawn_connection(params, app_handle) {
        Ok(h) => h,
        Err(e) => {
            // 端口被占用类:Windows "Access is denied" / "being used by another process"
            // 常因上次连接线程未完全释放。disconnect 已持锁等线程退出,此为驱动层极端延迟兜底。
            let lower = e.to_lowercase();
            let port_busy = lower.contains("access is denied")
                || lower.contains("being used by another process")
                || lower.contains("accessdenied")
                || lower.contains("permission");
            if port_busy {
                return Err("端口被占用,可能上次连接未完全释放,请稍后重试".to_string());
            }
            return Err(e);
        }
    };
    conns.insert(port.clone(), handle);
    // conns 锁在函数末尾随作用域释放
    Ok(())
}

#[tauri::command]
pub fn disconnect(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    port: Option<String>,
) -> Result<(), String> {
    // 先从 map 取出 handle 并放锁,再等 exit:
    // 持 connections 锁等线程退出(最多 1s)会阻塞所有其他 connect/disconnect/send 操作,
    // 死锁风险(尤其 connect 也持此锁)。exit 的 Condvar 用的是 handle.exit 内部锁,与 connections 锁无关。
    let handle = {
        let mut conns = state.connections.lock().map_err(|e| e.to_string())?;
        let resolved = resolve_port(&conns, port)?;
        conns.remove(&resolved)
    };
    if let Some(handle) = handle {
        let _ = handle.write_tx.send(WriteCommand::Close);
        handle.running.store(false, std::sync::atomic::Ordering::SeqCst);
        // 持锁(此处已释放 conns,用 handle.exit 的内部锁)等线程退出。
        if let Ok(mut e) = handle.exit.0.lock() {
            if !*e {
                let _ = handle.exit.1.wait_timeout(e, std::time::Duration::from_secs(1));
            }
        }
    }
    let _ = app_handle.emit("connection-state", crate::connection::ConnectionState {
        connected: false,
        port: None,
        baud_rate: None,
    });
    Ok(())
}

/// 内部:列出可用串口名。供 Tauri command 和 MCP 工具共用。
pub fn list_ports_inner() -> Vec<String> {
    serialport::available_ports()
        .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub fn list_ports() -> Result<Vec<String>, String> {
    Ok(list_ports_inner())
}

/// 清零收发字节统计。把累计器置 0，并 emit tx-update/rx-update = 0 让前端同步。
/// 未连接时无 handle，仅 emit 0（前端已置 0，幂等）。
#[tauri::command]
pub fn reset_stats(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    port: Option<String>,
) -> Result<(), String> {
    let resolved = {
        let conns = state.connections.lock().map_err(|e| e.to_string())?;
        resolve_port(&conns, port)?
    };
    // resolve 与清零分两段持锁:resolve 放锁后,连接若在此间隙被 disconnect,
    // 下方 get 返回 None,跳过清零(幂等,仅 emit 0)。这是可接受的弱一致(reset_stats 非关键路径)。
    let conns = state.connections.lock().map_err(|e| e.to_string())?;
    if let Some(handle) = conns.get(&resolved) {
        handle.tx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
        handle.rx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
    }
    let _ = app_handle.emit("tx-update", crate::connection::TxUpdate { total: 0, port: resolved.clone() });
    let _ = app_handle.emit("rx-update", crate::connection::RxUpdate { total: 0, port: resolved });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64};

    use crossbeam_channel::bounded;

    use crate::buffer::rx_history::RxHistory;

    /// 构造测试用 ConnectionHandle:bounded channel + 各 Arc 字段 + rx_history。
    fn mock_handle(port: &str) -> ConnectionHandle {
        let (write_tx, _write_rx) = bounded::<WriteCommand>(8);
        ConnectionHandle {
            running: Arc::new(AtomicBool::new(true)),
            tx_bytes: Arc::new(AtomicU64::new(0)),
            rx_bytes: Arc::new(AtomicU64::new(0)),
            write_tx,
            exit: Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
            port: port.to_string(),
            baud: 115200,
            rx_history: Arc::new(RxHistory::new(100)),
        }
    }

    #[test]
    fn test_resolve_port_none_single_connection() {
        let mut conns = HashMap::new();
        conns.insert("COM3".to_string(), mock_handle("COM3"));
        assert_eq!(resolve_port(&conns, None).unwrap(), "COM3");
    }

    #[test]
    fn test_resolve_port_none_empty() {
        let conns: HashMap<String, ConnectionHandle> = HashMap::new();
        assert!(resolve_port(&conns, None).is_err());
    }

    #[test]
    fn test_resolve_port_none_multiple() {
        let mut conns = HashMap::new();
        conns.insert("COM3".to_string(), mock_handle("COM3"));
        conns.insert("COM5".to_string(), mock_handle("COM5"));
        let err = resolve_port(&conns, None).unwrap_err();
        assert!(err.contains("多个"), "应提示多个连接,实际: {}", err);
    }

    #[test]
    fn test_resolve_port_some_exists() {
        let mut conns = HashMap::new();
        conns.insert("COM3".to_string(), mock_handle("COM3"));
        assert_eq!(resolve_port(&conns, Some("COM3".to_string())).unwrap(), "COM3");
    }

    #[test]
    fn test_resolve_port_some_not_exists() {
        let mut conns = HashMap::new();
        conns.insert("COM3".to_string(), mock_handle("COM3"));
        let err = resolve_port(&conns, Some("COM9".to_string())).unwrap_err();
        assert!(err.contains("COM9"), "应包含端口名,实际: {}", err);
    }
}
