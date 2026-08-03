use tauri::{State, Emitter};

use crate::connection::{SerialParams, spawn_connection, WriteCommand};
use crate::state::AppState;

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
        port,
        baud_rate: baudRate,
        data_bits,
        parity,
        stop_bits,
        flow_control,
    };

    let handle = spawn_connection(params, app_handle)?;

    let mut conn = state.connection.lock().map_err(|e| e.to_string())?;
    *conn = Some(handle);

    Ok(())
}

#[tauri::command]
pub fn disconnect(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut conn = state.connection.lock().map_err(|e| e.to_string())?;
    if let Some(handle) = conn.take() {
        let _ = handle.write_tx.send(WriteCommand::Close);
        handle.running.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = app_handle.emit("connection-state", crate::connection::ConnectionState {
            connected: false,
            port: None,
        });
    }
    Ok(())
}

#[tauri::command]
pub fn list_ports() -> Result<Vec<String>, String> {
    let ports = serialport::available_ports().map_err(|e| e.to_string())?;
    Ok(ports.into_iter().map(|p| p.port_name).collect())
}

/// 清零收发字节统计。把累计器置 0，并 emit tx-update/rx-update = 0 让前端同步。
/// 未连接时无 handle，仅 emit 0（前端已置 0，幂等）。
#[tauri::command]
pub fn reset_stats(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let conn = state.connection.lock().map_err(|e| e.to_string())?;
    if let Some(handle) = conn.as_ref() {
        handle.tx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
        handle.rx_bytes.store(0, std::sync::atomic::Ordering::SeqCst);
    }
    let _ = app_handle.emit("tx-update", crate::connection::TxUpdate { total: 0 });
    let _ = app_handle.emit("rx-update", crate::connection::RxUpdate { total: 0 });
    Ok(())
}
