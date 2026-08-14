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

/// 从 23333 起递增找空闲端口,上限 23353(20 次)。返回绑定的端口。
pub fn pick_port() -> Result<u16, String> {
    for port in 23333..=23353 {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                drop(listener);
                return Ok(port);
            }
            Err(_) => continue,
        }
    }
    Err("23333-23353 端口均被占用".to_string())
}

/// 工具名常量
const LIST_PORTS: &str = "list_ports";
const CONNECT: &str = "connect";
const DISCONNECT: &str = "disconnect";
const GET_STATUS: &str = "get_status";
const SEND_AND_READ: &str = "send_and_read";
const SEND: &str = "send";
const GET_HISTORY_SINCE: &str = "get_history_since";

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
                 单连接:已连接时先 disconnect。返回 { ok, mode } 或 { ok:false, error }。",
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
                "断开当前串口连接。无参数。返回 { ok: true } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                GET_STATUS,
                "查询当前连接状态与 rx 序号。无参数。返回 { ok, connected, rx_seq }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            Tool::new(
                SEND,
                "发送数据到串口(不读响应)。参数: text, ending(Cr|Lf|Crlf|None,默认 Crlf), \
                 is_hex(默认 false)。返回 { ok, sent } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "要发送的数据" },
                        "ending": { "type": "string", "enum": ["Cr","Lf","Crlf","None"], "default": "Crlf" },
                        "is_hex": { "type": "boolean", "default": false }
                    },
                    "required": ["text"]
                })),
            ),
            Tool::new(
                SEND_AND_READ,
                "发送数据并阻塞读取响应。参数: text, timeout_ms(静默间隙超时), \
                 ending(默认 Crlf), is_hex(默认 false)。\
                 超时语义:持续有数据时阻塞可远超 timeout,静默 gap 后返回。\
                 返回 { ok, sent, responses: [行], timed_out, seq0, seq1 } 或 { ok:false, error }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "timeout_ms": { "type": "integer", "description": "静默间隙超时(毫秒),最小50" },
                        "ending": { "type": "string", "enum": ["Cr","Lf","Crlf","None"], "default": "Crlf" },
                        "is_hex": { "type": "boolean", "default": false }
                    },
                    "required": ["text", "timeout_ms"]
                })),
            ),
            Tool::new(
                GET_HISTORY_SINCE,
                "获取指定序号之后的收发历史(tx+rx,带方向)。参数: seq(基准序号), max_lines(可选,截断最旧), is_hex(可选,默认 false 返回 ascii;true 返回 hex dump,纯 hex 数据应传 true 否则 ascii 显示空)。\
                 返回 { ok, lines: [{dir, text}], latest_seq }。dir 为 'rx' 或 'tx'。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "seq": { "type": "integer", "description": "基准序号,返回 seq > 此值 的行" },
                        "max_lines": { "type": "integer", "description": "最多返回行数,截断最旧" },
                        "is_hex": { "type": "boolean", "default": false, "description": "true 返回 hex dump(非可打印字节不丢);纯 hex 数据需传 true" }
                    },
                    "required": ["seq"]
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
                    match tools::disconnect(&shared, &shared.app_handle) {
                        Ok(_) => serde_json::json!({ "ok": true }),
                        Err(e) => serde_json::json!({ "ok": false, "error": e }),
                    }
                }
                GET_STATUS => serde_json::to_value(tools::get_status(&shared)).unwrap_or_default(),
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
/// 在 lib.rs setup 里用 tauri::async_runtime::spawn 调用。
pub async fn run_server(shared: Arc<McpShared>, port: u16) -> Result<(), String> {
    let shared_for_factory = shared.clone();
    let service = StreamableHttpService::new(
        move || Ok(NeoserialHandler::new(shared_for_factory.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("bind 127.0.0.1:{} 失败: {}", port, e))?;
    log::info!("MCP server listening on 127.0.0.1:{}/mcp", port);
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("axum serve 失败: {}", e))?;
    Ok(())
}
