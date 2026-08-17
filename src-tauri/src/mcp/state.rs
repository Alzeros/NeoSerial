use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::connection::ConnectionHandle;
use crate::mcp::registry::RegistryHandle;

/// MCP handler 与 Tauri command 共享的状态。
/// 关键:connections 必须与 AppState.connections 是**同一个 Arc** clone,
/// 否则手动断开和 agent 断开各持一份状态(见 spec "桥接"章节)。
/// 在 lib.rs setup 里构造:从 AppState 取出 connections 的 Arc clone 装入。
/// rx_history 已下沉为 per-connection(ConnectionHandle.rx_history),
/// 无全局字段(多连接隔离,不串数据)。
pub struct McpShared {
    pub app_handle: tauri::AppHandle,
    pub connections: Arc<Mutex<HashMap<String, ConnectionHandle>>>,
    /// registry 句柄,供 connect/disconnect 工具更新 connections 快照。None 时跳过更新。
    pub registry: Option<RegistryHandle>,
}
