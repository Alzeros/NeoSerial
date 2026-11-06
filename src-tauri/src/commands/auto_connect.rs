use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

use crate::config::settings::{DataBits, FlowControl, Parity, StopBits};
use crate::connection::{ConnectingGuard, ConnectionHandle, ConnectionMode, ConnectionState, SerialParams, WriteCommand};
use crate::state::AppState;

static BINDING_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize, Deserialize)]
pub struct AutoConnectParams {
    pub port: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: u8,
    pub flow_control: FlowControl,
}

#[derive(Clone, Serialize)]
pub struct AutoConnectBinding {
    pub id: u64,
    pub params: AutoConnectParams,
}

struct PendingConnection(Option<ConnectionHandle>);

impl Drop for PendingConnection {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.running.store(false, Ordering::SeqCst);
            let _ = handle.write_tx.try_send(WriteCommand::Close);
        }
    }
}

pub fn cancel_window(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    label: &str,
) -> Result<Option<Arc<AutoConnectBinding>>, String> {
    let mut bindings = state.auto_connect_bindings.lock().map_err(|error| error.to_string())?;
    let previous = bindings.remove(label);
    let _ = app_handle.emit_to(label, "auto-connect-changed", Option::<AutoConnectBinding>::None);
    Ok(previous)
}

#[tauri::command]
pub fn get_auto_connect(
    state: State<'_, AppState>,
    webview_window: tauri::WebviewWindow,
) -> Result<Option<AutoConnectBinding>, String> {
    let bindings = state.auto_connect_bindings.lock().map_err(|error| error.to_string())?;
    Ok(bindings.get(webview_window.label()).map(|binding| binding.as_ref().clone()))
}

#[tauri::command]
pub fn set_auto_connect(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    params: Option<AutoConnectParams>,
) -> Result<Option<AutoConnectBinding>, String> {
    let label = webview_window.label();
    let Some(mut params) = params else {
        cancel_window(&state, &app_handle, label)?;
        return Ok(None);
    };
    params.port = params.port.trim().to_string();
    if params.port.is_empty() || params.baud_rate == 0 || !matches!(params.stop_bits, 1 | 2) {
        return Err("请先选择有效的端口和串口参数".to_string());
    }
    let mut bindings = state.auto_connect_bindings.lock().map_err(|error| error.to_string())?;
    if bindings.iter().any(|(owner, binding)| owner != label && binding.params.port == params.port) {
        return Err(format!("端口 {} 已在其他窗口开启自动连接", params.port));
    }
    let binding = AutoConnectBinding { id: BINDING_ID.fetch_add(1, Ordering::Relaxed), params };
    bindings.insert(label.to_string(), Arc::new(binding.clone()));
    let _ = app_handle.emit_to(label, "auto-connect-changed", Some(binding.clone()));
    Ok(Some(binding))
}

#[tauri::command]
pub async fn auto_connect_once(
    app_handle: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    binding_id: u64,
) -> Result<bool, String> {
    let label = webview_window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || attempt_connection(&app_handle, &label, binding_id))
        .await
        .map_err(|error| format!("自动连接任务执行异常: {}", error))?
}

fn attempt_connection(app_handle: &tauri::AppHandle, label: &str, binding_id: u64) -> Result<bool, String> {
    let state = app_handle.try_state::<AppState>().ok_or("无法访问应用状态")?;
    let binding = {
        let bindings = state.auto_connect_bindings.lock().map_err(|error| error.to_string())?;
        let Some(binding) = bindings.get(label).filter(|binding| binding.id == binding_id) else {
            return Ok(false);
        };
        binding.clone()
    };
    let params = &binding.params;
    let _connecting = {
        let connections = state.connections.lock().map_err(|error| error.to_string())?;
        if let Some(existing) = connections.get(&params.port) {
            let owned = existing.running.load(Ordering::SeqCst)
                && existing.window_label.read().map(|owner| owner.as_str() == label).unwrap_or(false);
            if owned {
                let _ = app_handle.emit_to(label, "connection-state", ConnectionState {
                    connected: true, port: Some(params.port.clone()), baud_rate: Some(existing.baud),
                });
            }
            return Ok(owned);
        }
        if connections.values().any(|handle| handle.window_label.read().map(|owner| owner.as_str() == label).unwrap_or(false)) {
            return Ok(false);
        }
        let Some(guard) = ConnectingGuard::acquire(&state.connecting, &params.port) else {
            return Ok(false);
        };
        guard
    };
    let serial_params = SerialParams {
        port: params.port.clone(),
        baud_rate: params.baud_rate,
        data_bits: params.data_bits,
        parity: params.parity,
        stop_bits: if params.stop_bits == 2 { StopBits::Two } else { StopBits::One },
        flow_control: params.flow_control,
    };
    let (handle, mode) = crate::connection::spawn_connection(
        serial_params,
        app_handle.clone(),
        format!("auto-connect-pending-{}", binding_id),
        state.connections.clone(),
        state.registry.clone(),
        false,
    )?;
    let mut pending = PendingConnection(Some(handle));
    let bindings = state.auto_connect_bindings.lock().map_err(|error| error.to_string())?;
    if !bindings.get(label).is_some_and(|current| current.id == binding_id)
        || app_handle.get_webview_window(label).is_none()
        || !pending.0.as_ref().is_some_and(|handle| handle.running.load(Ordering::SeqCst))
    {
        return Ok(false);
    }
    let snapshot = {
        let mut connections = state.connections.lock().map_err(|error| error.to_string())?;
        if connections.contains_key(&params.port)
            || connections.values().any(|handle| handle.window_label.read().map(|owner| owner.as_str() == label).unwrap_or(false))
        {
            return Ok(false);
        }
        let handle = pending.0.as_ref().ok_or("自动连接已取消")?;
        *handle.window_label.write().map_err(|error| error.to_string())? = label.to_string();
        connections.insert(params.port.clone(), pending.0.take().ok_or("自动连接已取消")?);
        connections.values().map(|handle| crate::mcp::registry::ConnInfo {
            com: handle.port.clone(), baud: handle.baud,
        }).collect()
    };
    let _ = app_handle.emit_to(label, "connection-mode", ConnectionMode { mode });
    let _ = app_handle.emit_to(label, "connection-state", ConnectionState {
        connected: true, port: Some(params.port.clone()), baud_rate: Some(params.baud_rate),
    });
    drop(bindings);
    if let Some(registry) = &state.registry {
        let _ = registry.update_connections(snapshot);
    }
    let _ = app_handle.emit("mcp-connections-changed", ());
    Ok(true)
}
