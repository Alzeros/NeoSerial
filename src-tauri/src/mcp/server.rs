use std::future::Future;
use std::net::TcpListener;
use std::sync::Arc;

use rmcp::RoleServer;
use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, JsonObject, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use serde_json::Value;

use super::state::McpShared;
use super::tools;

/// 构造 JSON Schema 为 Arc<JsonObject>,供 Tool::new 的 input_schema 参数。
fn schema(json: serde_json::Value) -> Arc<JsonObject> {
    let map: JsonObject = serde_json::from_value(json).unwrap_or_default();
    Arc::new(map)
}

/// 从 start_port 起递增找空闲端口,上限 start_port+20。
/// 默认端口用没规律的值(34594)降低冲突概率;被占才递增兜底。
/// 成功返回 (端口号, 已绑定的 TcpListener);全被占返回错误。
/// 返回 listener 而非端口号,避免 drop 后 rebind 的 TOCTOU 竞态。
pub fn bind_port(start_port: u16) -> Result<(u16, std::net::TcpListener), String> {
    for port in start_port..start_port.saturating_add(20) {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => return Ok((port, listener)),
            Err(_) => continue,
        }
    }
    Err(format!("端口 {}-{} 均被占用", start_port, start_port + 19))
}

/// 工具名常量
const LIST_PORTS: &str = "list_ports";
const CONNECT: &str = "connect";
const DISCONNECT: &str = "disconnect";
const GET_STATUS: &str = "get_status";
const SEND_AND_READ: &str = "send_and_read";
const SEND: &str = "send";
const GET_HISTORY_SINCE: &str = "get_history_since";
const RESET_STATS: &str = "reset_stats";
const SEND_FILE: &str = "send_file";
const START_LOGGING: &str = "start_logging";
const STOP_LOGGING: &str = "stop_logging";
const SEQUENCE_RUN: &str = "sequence_run";
const SEQUENCE_STOP: &str = "sequence_stop";
const GET_SETTINGS: &str = "get_settings";
const SAVE_SETTINGS: &str = "save_settings";

pub struct NeoserialHandler {
    shared: Arc<McpShared>,
}

impl NeoserialHandler {
    pub fn new(shared: Arc<McpShared>) -> Self {
        NeoserialHandler { shared }
    }
}

impl ServerHandler for NeoserialHandler {
    fn get_info(&self) -> ServerInfo {
        // server name 设为 NeoSerial(应用名),否则 rmcp 默认用 from_build_env 显示 "rmcp"。
        let info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.with_server_info(rmcp::model::Implementation::new("NeoSerial", env!("CARGO_PKG_VERSION")))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_ {
        let tools = vec![
            Tool::new(
                LIST_PORTS,
                "列出当前可用的串口。无参数。返回 { ok, ports: [端口号] }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                CONNECT,
                "连接串口。参数: port(如 COM3), baud_rate, data_bits(Five|Six|Seven|Eight), \
                 parity(None|Odd|Even), stop_bits(1|2), flow_control(None|Software|Hardware)。\
                 多连接:同一 port 已连接报错,不同 port 各自连接。返回 { ok, mode } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" },
                        "baud_rate": { "type": "integer", "description": "波特率,如 115200" },
                        "data_bits": { "type": "string", "enum": ["Five","Six","Seven","Eight"], "default": "Eight" },
                        "parity": { "type": "string", "enum": ["None","Odd","Even"], "default": "None" },
                        "stop_bits": { "type": "integer", "enum": [1,2], "default": 1 },
                        "flow_control": { "type": "string", "enum": ["None","Software","Hardware"], "default": "None" }
                    },
                    "required": ["port", "baud_rate"]
                })),
            ),
            Tool::new(
                DISCONNECT,
                "断开指定串口连接。参数: port(要断开的串口名,如 COM3)。返回 { ok: true } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "要断开的串口名,如 COM3" }
                    },
                    "required": ["port"],
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                GET_STATUS,
                "查询指定端口的连接状态与 rx 序号。参数: port。返回 { ok, connected, port, baud, tx_bytes, rx_bytes, seq }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" }
                    },
                    "required": ["port"],
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                SEND,
                "发送数据到串口(不读响应)。参数: port, text, ending(Cr|Lf|Crlf|None,默认 Crlf), \
                 is_hex(默认 false)。返回 { ok, sent } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" },
                        "text": { "type": "string", "description": "要发送的数据" },
                        "ending": { "type": "string", "enum": ["Cr","Lf","Crlf","None"], "default": "Crlf" },
                        "is_hex": { "type": "boolean", "default": false }
                    },
                    "required": ["port", "text"]
                })),
            ),
            Tool::new(
                SEND_AND_READ,
                "发送数据并阻塞读取响应。参数: port, text, timeout_ms(静默间隙超时), \
                 ending(默认 Crlf), is_hex(默认 false)。\
                 超时语义:持续有数据时阻塞可远超 timeout,静默 gap 后返回。\
                 返回 { ok, sent, responses: [行], timed_out, seq0, seq1 } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" },
                        "text": { "type": "string" },
                        "timeout_ms": { "type": "integer", "description": "静默间隙超时(毫秒),最小50" },
                        "ending": { "type": "string", "enum": ["Cr","Lf","Crlf","None"], "default": "Crlf" },
                        "is_hex": { "type": "boolean", "default": false }
                    },
                    "required": ["port", "text", "timeout_ms"]
                })),
            ),
            Tool::new(
                GET_HISTORY_SINCE,
                "获取指定端口的收发历史(tx+rx,带方向)。参数: port, seq(基准序号), max_lines(可选,截断最旧), is_hex(可选,默认 false 返回 ascii;true 返回 hex dump,纯 hex 数据应传 true 否则 ascii 显示空)。\
                 返回 { ok, lines: [{dir, text}], latest_seq }。dir 为 'rx' 或 'tx'。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" },
                        "seq": { "type": "integer", "description": "基准序号,返回 seq > 此值 的行" },
                        "max_lines": { "type": "integer", "description": "最多返回行数,截断最旧" },
                        "is_hex": { "type": "boolean", "default": false, "description": "true 返回 hex dump(非可打印字节不丢);纯 hex 数据需传 true" }
                    },
                    "required": ["port", "seq"]
                })),
            ),
            Tool::new(
                RESET_STATS,
                "清零指定端口的收发字节统计。参数: port。返回 { ok } 或 { ok:false }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" }
                    },
                    "required": ["port"],
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                SEND_FILE,
                "发送文件(二进制安全,用于固件/二进制烧录)。参数: port, path(文件绝对路径)。分块 1024 字节写入。返回 { ok, sent } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" },
                        "path": { "type": "string", "description": "文件绝对路径" }
                    },
                    "required": ["port", "path"],
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                START_LOGGING,
                "开始存盘(全局,所有连接的收发都进同一文件)。参数: path(可选,传则新建/覆盖该文件;不传用上次路径续写,无上次路径则生成默认路径)。返回 { ok, path }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "日志文件绝对路径,可选" }
                    },
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                STOP_LOGGING,
                "停止存盘。无参数。返回 { ok }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                SEQUENCE_RUN,
                "跑脚本序列:多条命令批量下发,支持循环和行间延时(用于自动化测试/批量配置)。参数: port, commands(命令数组,每项含 command/hex/enter/delay_ms), run_count(循环次数), loop_interval(可选,轮间隔 ms)。返回 { ok } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" },
                        "commands": {
                            "type": "array",
                            "description": "命令数组",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "command": { "type": "string", "description": "命令内容" },
                                    "hex": { "type": "boolean", "default": false, "description": "是否 hex 数据" },
                                    "enter": { "type": "boolean", "default": true, "description": "是否追加 CRLF 行尾" },
                                    "delay_ms": { "type": "integer", "default": 0, "description": "行间延时 ms" }
                                },
                                "required": ["command"]
                            }
                        },
                        "run_count": { "type": "integer", "description": "循环次数" },
                        "loop_interval": { "type": "integer", "default": 0, "description": "轮间隔 ms" }
                    },
                    "required": ["port", "commands", "run_count"]
                })),
            ),
            Tool::new(
                SEQUENCE_STOP,
                "停止指定 port 的运行序列。参数: port。返回 { ok } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" }
                    },
                    "required": ["port"],
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                GET_SETTINGS,
                "读取当前配置(波特率预设/显示模式/编码/MCP 设置等)。无参数。返回 { ok, settings }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                SAVE_SETTINGS,
                "保存配置(持久化到 settings.json)。参数: settings(完整 Settings 对象,从 get_settings 拿原值改后回传)。返回 { ok } 或 { ok:false, error }。注意:部分项(如 MCP 端口)改后需重启生效。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "settings": { "type": "object", "description": "完整 Settings 对象" }
                    },
                    "required": ["settings"],
                    "additionalProperties": false
                })),
            ),
        ];
        async move { Ok(ListToolsResult { tools, ..Default::default() }) }
    }

    fn call_tool(
        &self,
        params: CallToolRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, rmcp::ErrorData>> + Send + '_ {
        let name = params.name.clone();
        let args: Value = params
            .arguments
            .clone()
            .map(Value::Object)
            .unwrap_or(Value::Null);
        let shared = self.shared.clone();
        async move {
            let resp: Value = match name.as_ref() {
                LIST_PORTS => serde_json::to_value(tools::list_ports(&shared)).unwrap_or_default(),
                DISCONNECT => {
                    let req: tools::DisconnectReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::disconnect(&shared, &shared.app_handle, req) {
                        Ok(_) => serde_json::json!({ "ok": true }),
                        Err(e) => serde_json::json!({ "ok": false, "error": e }),
                    }
                }
                GET_STATUS => {
                    let req: tools::StatusReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    serde_json::to_value(tools::get_status(&shared, req)).unwrap_or_default()
                }
                SEND => {
                    let req: tools::SendReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::send(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                GET_HISTORY_SINCE => {
                    let req: tools::GetHistorySinceReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    serde_json::to_value(tools::get_history_since(&shared, req)).unwrap_or_default()
                }
                RESET_STATS => {
                    let req: tools::ResetStatsReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    serde_json::to_value(tools::reset_stats(&shared, req)).unwrap_or_default()
                }
                SEND_FILE => {
                    let req: tools::SendFileReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::send_file(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                START_LOGGING => {
                    let req: tools::StartLoggingReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::start_logging(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                STOP_LOGGING => {
                    match tools::stop_logging(&shared) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                SEQUENCE_RUN => {
                    let req: tools::SequenceRunReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::sequence_run(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                SEQUENCE_STOP => {
                    let req: tools::SequenceStopReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::sequence_stop(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                GET_SETTINGS => {
                    serde_json::to_value(tools::get_settings(&shared)).unwrap_or_default()
                }
                SAVE_SETTINGS => {
                    let req: tools::SaveSettingsReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::save_settings(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                SEND_AND_READ => {
                    let req: tools::SendAndReadReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::send_and_read(&shared, req).await {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                CONNECT => {
                    let req: tools::ConnectReq = match serde_json::from_value(args) {
                        Ok(r) => r,
                        Err(e) => {
                            return Ok(content_text(serde_json::json!({ "ok": false, "error": e.to_string() })).into());
                        }
                    };
                    match tools::connect(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                _ => serde_json::json!({ "ok": false, "error": format!("未知工具: {}", name) }),
            };
            Ok(content_text(resp).into())
        }
    }
}

/// 把 JSON 结果包成 CallToolResult(is_error=false) 的 text content。
fn content_text(json: Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(json.to_string())])
}

/// 起 MCP streamable HTTP server,bind 127.0.0.1:port/mcp。
/// 接收已绑定的 std TcpListener(由 bind_port 返回),转 tokio listener 避免竞态。
/// 在 lib.rs setup 里用 tauri::async_runtime::spawn 调用。
pub async fn run_server(shared: Arc<McpShared>, listener: std::net::TcpListener) -> Result<(), String> {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let shared_for_factory = shared.clone();
    let service = StreamableHttpService::new(
        move || Ok(NeoserialHandler::new(shared_for_factory.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = axum::Router::new().nest_service("/mcp", service);
    // from_std 接管已绑定的 listener,不会重新 bind
    listener.set_nonblocking(true).map_err(|e| format!("set_nonblocking 失败: {}", e))?;
    let tokio_listener = tokio::net::TcpListener::from_std(listener)
        .map_err(|e| format!("from_std 失败: {}", e))?;
    log::info!("MCP server listening on 127.0.0.1:{}/mcp", port);
    axum::serve(tokio_listener, app)
        .await
        .map_err(|e| format!("axum serve 失败: {}", e))?;
    Ok(())
}
