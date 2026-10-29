use std::future::Future;
use std::net::TcpListener;
use std::sync::Arc;

use rmcp::RoleServer;
use rmcp::ServerHandler;
use tauri::Emitter;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, JsonObject, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use axum::response::IntoResponse;
use serde_json::{json, Value};

use super::state::McpShared;
use super::tools;

/// 构造 JSON Schema 为 Arc<JsonObject>,供 Tool::new 的 input_schema 参数。
fn schema(json: serde_json::Value) -> Arc<JsonObject> {
    let map: JsonObject = serde_json::from_value(json).unwrap_or_default();
    Arc::new(map)
}

pub(crate) fn tool_list_response(tools: Vec<Tool>) -> ListToolsResult {
    // 2026-07-28 的客户端要求这两个字段；0 表示不缓存，保留原来的即时获取行为。
    ListToolsResult { tools, ..Default::default() }
        .with_ttl_ms(0)
        .with_cache_scope(rmcp::model::CacheScope::Private)
}

/// MCP HTTP 传输配置。**生产与协议测试夹具必须共用这一份**,
/// 否则会出现"测试全绿但线上配置没生效"。
/// 默认值(rmcp 3.4.0):
/// - allowed_hosts = ["localhost", "127.0.0.1", "::1"] —— 只接受回环 Host,挡 DNS rebinding;
/// - legacy_session_mode = true —— 旧客户端(2025-11-25 等)保留 initialize/session 路径;
/// - max_request_body_bytes = 4 MiB —— 流式读 body 时限长,超了返回 413;
/// - allowed_origins = [] —— **强制校验**:任何带 Origin 的请求一律拒绝(见下)。
///
/// **Origin 强制校验(S1)**:不设白名单,调用 `enforce_origin_validation()` 让
/// "带 Origin 但不在白名单"与"白名单为空"都变成拒绝。挡住的是网页通过
/// DNS rebinding 调本机串口 MCP 的路径:浏览器发请求必带 Origin,而原生
/// MCP 客户端(Claude Code / Codex / Cursor / VS Code 等)不发 Origin,照常通过。
/// 代价:浏览器内嵌的 MCP 客户端会被拒——本轮按保守策略接受,确有需求再单独设计白名单。
pub fn http_config() -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default().enforce_origin_validation()
}

/// server 能力与身份信息。生产与协议测试夹具共用。
pub fn server_info() -> ServerConfig {
    // server name 设为 NeoSerial(应用名),否则 rmcp 默认用 from_build_env 显示 "rmcp"。
    let info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
    info.with_server_info(rmcp::model::Implementation::new("NeoSerial", env!("CARGO_PKG_VERSION")))
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
const GET_SEQUENCE_STATUS: &str = "get_sequence_status";
const GET_SETTINGS: &str = "get_settings";
const SAVE_SETTINGS: &str = "save_settings";
const KB_SEARCH: &str = "kb_search";
const KB_GET: &str = "kb_get";
const KB_MANUALS: &str = "kb_manuals";

pub struct NeoserialHandler {
    shared: Arc<McpShared>,
}

impl NeoserialHandler {
    pub fn new(shared: Arc<McpShared>) -> Self {
        NeoserialHandler { shared }
    }
}

/// 19 个工具的输出 schema(2026-07-28 客户端据此校验结果结构)。
///
/// 覆盖成功与业务失败两种形态:`ok` 必填;成功字段与错误字段各自可选
/// (成功响应没有 error,失败响应没有成功字段)。
/// 手写而不是从 Rust 类型派生:派生要动配置模型,收益不抵风险;
/// 这份由 protocol_tests 用**真实响应**逐个校验,和实际响应漂移会被测出来。
pub(crate) fn output_schema(tool: &str) -> serde_json::Value {
    match tool {
        LIST_PORTS => json!({
            "type": "object",
            "description": "列出当前可用串口",
            "properties": {
                "ok": { "type": "boolean" },
                "ports": { "type": "array", "items": { "type": "string" }, "description": "可用串口名" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        CONNECT => json!({
            "type": "object",
            "description": "连接串口",
            "properties": {
                "ok": { "type": "boolean" },
                "mode": { "type": ["string", "null"], "description": "连接模式;复用时为 reused" },
                "reused": { "type": "boolean", "description": "是否复用了已有连接" },
                "baud": { "type": "integer", "description": "连接实际波特率" },
                "owner": { "type": ["string", "null"], "description": "gui 或 agent" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        DISCONNECT => json!({
            "type": "object",
            "description": "断开串口连接",
            "properties": {
                "ok": { "type": "boolean" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        GET_STATUS => json!({
            "type": "object",
            "description": "查询连接状态",
            "properties": {
                "ok": { "type": "boolean" },
                "connected": { "type": "boolean" },
                "port": { "type": ["string", "null"], "description": "串口名" },
                "baud": { "type": ["integer", "null"], "description": "波特率" },
                "tx_bytes": { "type": ["integer", "null"], "description": "已发送字节数" },
                "rx_bytes": { "type": ["integer", "null"], "description": "已接收字节数" },
                "seq": { "type": "integer", "description": "收发历史当前最大序号" },
                "cts": { "type": ["boolean", "null"], "description": "CTS 电平(仅硬件流控时有值)" },
                "dsr": { "type": ["boolean", "null"], "description": "DSR 电平(仅硬件流控时有值)" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        SEND => json!({
            "type": "object",
            "description": "发送数据",
            "properties": {
                "ok": { "type": "boolean" },
                "sent": { "type": "boolean", "description": "是否已交给写队列" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        SEND_AND_READ => json!({
            "type": "object",
            "description": "发送并读取响应",
            "properties": {
                "ok": { "type": "boolean" },
                "sent": { "type": "boolean", "description": "命令是否已发出" },
                "responses": { "type": "array", "items": { "type": "string" }, "description": "收到的行" },
                "timed_out": { "type": "boolean", "description": "是否因静默间隙超时结束" },
                "seq0": { "type": "integer", "description": "发送前的游标" },
                "seq1": { "type": "integer", "description": "本次实际收到的最后序号" },
                "cancelled": { "type": "boolean", "description": "本次调用被取消" },
                "retryable": { "type": "boolean", "description": "取消后能否安全重发(命令已发出时为 false)" },
                "error": { "type": "string", "description": "失败/取消说明" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        GET_HISTORY_SINCE => json!({
            "type": "object",
            "description": "拉取收发历史",
            "properties": {
                "ok": { "type": "boolean" },
                "lines": {
                    "type": "array",
                    "description": "历史行",
                    "items": {
                        "type": "object",
                        "properties": {
                            "dir": { "type": "string", "description": "rx 或 tx" },
                            "text": { "type": "string" },
                            "ts": { "type": "string", "description": "到达时间 HH:MM:SS.mmm" },
                            "ts_ms": { "type": "integer", "description": "到达时间 Unix 毫秒" },
                            "index": { "type": "integer", "description": "本次连接的行号" },
                            "seq": { "type": "integer", "description": "全局序号,可当游标" }
                        },
                        "required": ["dir", "text", "ts", "ts_ms", "index", "seq"],
                        "additionalProperties": false
                    }
                },
                "latest_seq": { "type": "integer", "description": "下次查询应传的游标" },
                "truncated": { "type": "boolean", "description": "还有更多新行未返回" },
                "dropped": { "type": "boolean", "description": "游标之后有行已被挤出" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        RESET_STATS => json!({
            "type": "object",
            "description": "清零收发字节统计",
            "properties": {
                "ok": { "type": "boolean" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        SEND_FILE => json!({
            "type": "object",
            "description": "发送文件",
            "properties": {
                "ok": { "type": "boolean" },
                "sent": { "type": "integer", "description": "实际写进串口的字节数(取消时为取消瞬间的下界快照,可能比最终实际写出少一个块)" },
                "total": { "type": ["integer", "null"], "description": "文件总字节数" },
                "cancelled": { "type": "boolean", "description": "本次发送被取消" },
                "retryable": { "type": "boolean", "description": "取消后能否安全重发(一律 false)" },
                "error": { "type": "string", "description": "失败/取消说明" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        START_LOGGING => json!({
            "type": "object",
            "description": "开始存盘",
            "properties": {
                "ok": { "type": "boolean" },
                "path": { "type": "string", "description": "日志文件绝对路径" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        STOP_LOGGING => json!({
            "type": "object",
            "description": "停止存盘",
            "properties": {
                "ok": { "type": "boolean" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        SEQUENCE_RUN => json!({
            "type": "object",
            "description": "跑脚本序列",
            "properties": {
                "ok": { "type": "boolean" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        SEQUENCE_STOP => json!({
            "type": "object",
            "description": "停止运行中的序列",
            "properties": {
                "ok": { "type": "boolean" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        GET_SEQUENCE_STATUS => json!({
            "type": "object",
            "description": "查询序列运行状态",
            "properties": {
                "ok": { "type": "boolean" },
                "running": { "type": "boolean", "description": "序列是否还在跑" },
                "completed": { "type": "boolean", "description": "是否全部执行完毕" },
                "current_index": { "type": "integer", "description": "当前/中止的命令索引(0-based)" },
                "total": { "type": "integer", "description": "命令总数" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        GET_SETTINGS => json!({
            "type": "object",
            "description": "读取当前配置",
            "properties": {
                "ok": { "type": "boolean" },
                "settings": {
                    "type": ["object", "null"],
                    "description": "完整 Settings 快照(不含知识库 API Key——那存在凭据文件里)"
                },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        SAVE_SETTINGS => json!({
            "type": "object",
            "description": "保存配置",
            "properties": {
                "ok": { "type": "boolean" },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        KB_SEARCH => json!({
            "type": "object",
            "description": "搜手册指令",
            "properties": {
                "ok": { "type": "boolean" },
                "total": { "type": "integer", "description": "命中总数(未截断)" },
                "items": {
                    "type": "array",
                    "description": "命中项",
                    "items": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string" },
                            "name": { "type": "string" },
                            "manual": { "type": "string", "description": "来源手册标题" },
                            "page_no": { "type": ["integer", "null"], "description": "页码" },
                            "also_in_count": { "type": "integer", "description": "同名指令还在几本别的手册里" }
                        },
                        "required": ["command", "name", "manual", "page_no", "also_in_count"],
                        "additionalProperties": false
                    }
                },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        KB_GET => json!({
            "type": "object",
            "description": "取一条指令的完整手册记录",
            "properties": {
                "ok": { "type": "boolean" },
                "command": { "type": "string" },
                "name": { "type": "string" },
                "syntax": { "type": "string" },
                "parameters": { "$ref": "#/$defs/parameters" },
                "example": { "type": "string" },
                "summary": { "type": "string" },
                "manual": { "type": "string", "description": "来源手册标题" },
                "page_no": { "type": ["integer", "null"], "description": "页码" },
                "also_in": {
                    "type": "array",
                    "description": "同名指令在其他手册里的记录",
                    "items": {
                        "type": "object",
                        "properties": {
                            "manual": { "type": "string" },
                            "page_no": { "type": ["integer", "null"] },
                            "name": { "type": "string" },
                            "syntax": { "type": "string" },
                            "parameters": { "$ref": "#/$defs/parameters" },
                            "example": { "type": "string" }
                        },
                        "required": ["manual", "page_no", "name", "syntax", "parameters", "example"],
                        "additionalProperties": false
                    }
                },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false,
            "$defs": {
                "parameters": {
                    "type": "array",
                    "description": "参数表",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "required": { "type": "boolean", "description": "是否必选" },
                            "description": { "type": "string" }
                        },
                        "required": ["name", "required", "description"],
                        "additionalProperties": false
                    }
                }
            }
        }),
        KB_MANUALS => json!({
            "type": "object",
            "description": "列本地缓存的手册",
            "properties": {
                "ok": { "type": "boolean" },
                "fetched_at": { "type": ["string", "null"], "description": "上次全量刷新时间" },
                "manuals": {
                    "type": "array",
                    "description": "手册列表",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer" },
                            "title": { "type": "string" },
                            "cached_commands": { "type": "integer", "description": "本地缓存条数" },
                            "server_commands": { "type": "integer", "description": "服务器报的条数" },
                            "cmd_status": { "type": "string", "description": "指令提取状态" },
                            "groups": { "type": "array", "items": { "type": "string" }, "description": "分组名" }
                        },
                        "required": ["id", "title", "cached_commands", "server_commands", "cmd_status", "groups"],
                        "additionalProperties": false
                    }
                },
                "error": { "type": "string", "description": "失败原因" }
            },
            "required": ["ok"],
            "additionalProperties": false
        }),
        other => panic!("没有为工具 {} 定义输出 schema", other),
    }
}

/// 19 个工具的保守注解(客户端可据此决定确认 UI 与并行策略)。
/// **只是 hint**:不改变工具权限、不新增确认弹窗、不影响实际执行。
///
/// 基线(与 protocol_tests 的 EXPECTED_ANNOTATIONS 逐项对齐,改这里必须同步改测试):
/// - 只读(不碰环境):list_ports / get_status / get_history_since / get_sequence_status /
///   get_settings / kb_search / kb_get / kb_manuals;
/// - 幂等(同参数重入无额外副作用):只读组 + connect(复用)/ disconnect / reset_stats /
///   sequence_stop / stop_logging / save_settings(完整快照覆盖);
/// - 破坏性(发任意设备指令、覆盖文件、写设置):send / send_and_read / send_file /
///   sequence_run / start_logging(传 path 时覆盖)/ save_settings;
/// - openWorld:与外部串口设备交互的为 true;纯本地操作(内存/文件/设置/缓存)为 false。
fn annotations(tool: &str) -> ToolAnnotations {
    match tool {
        LIST_PORTS => ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        CONNECT => ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(true)
            .open_world(true),
        DISCONNECT => ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(true)
            .open_world(true),
        GET_STATUS => ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(true),
        SEND => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(true),
        SEND_AND_READ => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(true),
        GET_HISTORY_SINCE => ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        RESET_STATS => ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        SEND_FILE => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(true),
        START_LOGGING => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(false),
        STOP_LOGGING => ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        SEQUENCE_RUN => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(true),
        SEQUENCE_STOP => ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .idempotent(true)
            .open_world(true),
        GET_SEQUENCE_STATUS => ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        GET_SETTINGS => ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        SAVE_SETTINGS => ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(true)
            .open_world(false),
        KB_SEARCH => ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        KB_GET => ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        KB_MANUALS => ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
        other => panic!("没有为工具 {} 定义注解", other),
    }
}

/// 给工具挂上输出 schema 与注解。
fn with_output(tool: Tool, name: &str) -> Tool {
    tool.with_raw_output_schema(schema(output_schema(name)))
        .with_annotations(annotations(name))
}

/** 19 个工具的固定顺序清单。
 * 生产 `list_tools` 与协议测试夹具共用同一份:顺序、名称、描述、inputSchema 的任何改动
 * 都会同时反映到 tools/list 响应与快照测试,不会出现"测试里的清单"与"线上的清单"两套。
 */
pub fn build_tools() -> Vec<Tool> {
    vec![
        with_output(
            Tool::new(
                LIST_PORTS,
                "列出当前可用的串口。无参数。返回 { ok, ports: [端口号] }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            LIST_PORTS,
        ),
        with_output(
            Tool::new(
                CONNECT,
                "连接串口。参数: port(如 COM3), baud_rate, data_bits(Five|Six|Seven|Eight), \
                 parity(None|Odd|Even), stop_bits(1|2), flow_control(None|Software|Hardware);\
                 枚举值不认识直接报错,不会静默改用默认值。\
                 同一 port 已被 GUI 窗口或前次 connect 连着时复用该连接、不重开串口:返回 reused:true、\
                 实际 baud 与 owner(gui|agent);请求的 baud_rate 与已有连接不同则报错(不会静默沿用旧波特率),\
                 要改波特率须先 disconnect。不同 port 各自连接。\
                 返回 { ok, mode, reused, baud, owner } 或 { ok:false, error }。",
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
            CONNECT,
        ),
        with_output(
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
            DISCONNECT,
        ),
        with_output(
            Tool::new(
                GET_STATUS,
                "查询指定端口的连接状态与 rx 序号。参数: port。返回 { ok, connected, port, baud, tx_bytes, rx_bytes, seq, cts, dsr }。\
                 cts/dsr 为本机 CTS/DSR 引脚电平,只在该连接流控为 Hardware 时有值(接了 RTS/CTS 线才有意义),否则 null。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" }
                    },
                    "required": ["port"],
                    "additionalProperties": false
                })),
            ),
            GET_STATUS,
        ),
        with_output(
            Tool::new(
                SEND,
                "发送数据到串口(不读响应)。参数: port, text, ending(Cr|Lf|Crlf|None,默认 Crlf,大小写不敏感,\
                 其他写法报错), is_hex(默认 false)。返回 { ok, sent } 或 { ok:false, error }。",
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
            SEND,
        ),
        with_output(
            Tool::new(
                SEND_AND_READ,
                "发送数据并阻塞读取响应。参数: port, text, timeout_ms(静默间隙超时), \
                 ending(默认 Crlf), is_hex(默认 false)。\
                 超时语义:持续有数据时阻塞可远超 timeout,静默 gap 后返回。\
                 返回 { ok, sent, responses: [行], timed_out, seq0, seq1 } 或 { ok:false, error }。\
                 **取消语义**:被取消(发取消通知;新版 2026-07-28 客户端断开连接也会取消)时返回 \
                 cancelled:true、retryable:false,并保留已收到的 responses 与 seq0/seq1 游标。\
                 命令可能已被设备接收,**不要重发**——用 get_history_since(port, seq1) 续读已到达的数据。\
                 旧版 2025-11-25 session 客户端直接断开**不会**中断已提交的调用,取消须发 notifications/cancelled。",
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
            SEND_AND_READ,
        ),
        with_output(
            Tool::new(
                GET_HISTORY_SINCE,
                "获取指定端口的收发历史(tx+rx,带方向)。参数: port, seq(基准序号), max_lines(可选,截断最旧), is_hex(可选,默认 false 返回 ascii;true 返回 hex dump,纯 hex 数据应传 true 否则 ascii 显示空)。\
                 返回 { ok, lines: [{dir, text, ts, ts_ms, index, seq}], latest_seq, truncated, dropped }。dir 为 'rx' 或 'tx'。\
                 ts 为到达时间 HH:MM:SS.mmm(与界面日志一致),ts_ms 为 Unix 毫秒(做时间轴/算命令→响应延迟用 rx 行与 tx 行的 ts_ms 相减;连接前的旧数据为 0)。\
                 latest_seq 是本次实际返回的最后一行序号,始终把它当下次查询的 seq。\
                 truncated=true 表示还有更多新行被 max_lines 挡住,应立即用 latest_seq 再拉一次直到 false。\
                 dropped=true 表示游标之后有行已被缓冲容量挤出、取不回来,该段历史不完整。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" },
                        "seq": { "type": "integer", "description": "基准序号,返回 seq > 此值 的行" },
                        "max_lines": { "type": "integer", "description": "最多返回行数,截断最旧。被截断时 latest_seq 只推进到已返回的末行,truncated=true,继续拉即可,不会丢行" },
                        "is_hex": { "type": "boolean", "default": false, "description": "true 返回 hex dump(非可打印字节不丢);纯 hex 数据需传 true" }
                    },
                    "required": ["port", "seq"]
                })),
            ),
            GET_HISTORY_SINCE,
        ),
        with_output(
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
            RESET_STATS,
        ),
        with_output(
            Tool::new(
                SEND_FILE,
                "发送文件(二进制安全,用于固件/二进制烧录)。参数: port, path(文件绝对路径)。分块 1024 字节写入,阻塞到最后一字节写进串口驱动才返回(大文件按波特率可能要等几分钟),sent 等于文件大小。中途断开/写错返回 { ok:false, error }(带已写出 x/y 字节)。**取消语义**:被取消时停止后续写出,返回 { ok:false, sent:已写字节数的下界(取消瞬间快照,在途最后一块可能随后完成,至多再写出一个 1024 字节块), total:文件总字节, cancelled:true, retryable:false }。设备可能已收到半个文件,**不要自动重发**,需人工确认设备状态后再决定是否重传。",
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
            SEND_FILE,
        ),
        with_output(
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
            START_LOGGING,
        ),
        with_output(
            Tool::new(
                STOP_LOGGING,
                "停止存盘。无参数。返回 { ok }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            STOP_LOGGING,
        ),
        with_output(
            Tool::new(
                SEQUENCE_RUN,
                "跑脚本序列:多条命令批量下发,支持循环和行间延时(用于自动化测试/批量配置)。参数: port, commands(命令数组,每项含 command/hex/enter/delay_ms), run_count(循环次数),loop_interval(可选,轮间隔 ms)。返回 { ok } 或 { ok:false, error }。序列在后台跑:发完用 get_sequence_status(port) 查进度(running/completed/current_index)。若序列被中止(断开/stop/取消)或输出流断了,**不要重复启动**——先查状态确认再决定。",
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
            SEQUENCE_RUN,
        ),
        with_output(
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
            SEQUENCE_STOP,
        ),
        with_output(
            Tool::new(
                GET_SEQUENCE_STATUS,
                "查询指定 port 的序列运行状态(含进度)。agent 发完 sequence_run 后可轮询:\
                 running=true 序列仍在跑;running=false+completed=true 正常跑完;\
                 running=false+completed=false 被中止(断开/stop),current_index 指示停在哪条(0-based)。\
                 参数: port。返回 { ok, running, completed, current_index, total }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "port": { "type": "string", "description": "串口名,如 COM3" }
                    },
                    "required": ["port"],
                    "additionalProperties": false
                })),
            ),
            GET_SEQUENCE_STATUS,
        ),
        with_output(
            Tool::new(
                GET_SETTINGS,
                "读取当前配置(波特率预设/显示模式/编码/MCP 设置等)。无参数。返回 { ok, settings }。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            GET_SETTINGS,
        ),
        with_output(
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
            SAVE_SETTINGS,
        ),
        with_output(
            Tool::new(
                KB_SEARCH,
                "搜知识库手册里的 AT 指令(发指令前查语法,别凭记忆猜)。多关键词空格分隔,                 匹配指令名/中文名/摘要,大小写无关,不带 AT+ 也能命中(如 \"csq\"、\"mqtt 配置\")。                 参数: query, limit(可选,默认 10,最大 50)。返回 { ok, total, items:[{command,name,manual,page_no,also_in_count}] };                 total > items 长度说明还有更多,收窄 query。要完整语法/参数/示例用 kb_get;                 also_in_count>0 说明同名指令还在别的手册里(manual 只是排最前那本,各本语法可能不同),kb_get 的 also_in 里有各本的版本。                 **只读本地缓存,不请求知识库、不耗配额**;缓存空时返回 ok:false 并说明该让用户做什么。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "关键词,空格分隔多个(需全部命中)" },
                        "limit": { "type": "integer", "default": 10, "description": "返回条数上限,1-50" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                })),
            ),
            KB_SEARCH,
        ),
        with_output(
            Tool::new(
                KB_GET,
                "取一条 AT 指令的完整手册记录:语法、参数表(名称/是否必选/说明)、示例、摘要、来源手册与页码;                 同名指令在别的手册里的记录进 also_in,各带自己的语法/参数表/示例(顶层字段只属于排最前那本手册,别拿它去套 also_in 的语法)。                 参数: command(完整指令名,大小写无关,如 AT+MIPLCREATE)。                 找不到返回 ok:false,改用 kb_search 按关键词找。只读本地缓存,不耗配额。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "完整指令名,如 AT+CSQ" }
                    },
                    "required": ["command"],
                    "additionalProperties": false
                })),
            ),
            KB_GET,
        ),
        with_output(
            Tool::new(
                KB_MANUALS,
                "列本地缓存里有哪些手册(知道能查到什么范围)。无参数。返回 { ok, fetched_at, manuals:[{id,title,                 cached_commands,server_commands,cmd_status,groups}] };cached_commands 与 server_commands 不等                 说明那本没拉全,可让用户在设置页刷新。只读本地缓存,不耗配额。",
                schema(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                })),
            ),
            KB_MANUALS,
        ),
    ]
}

impl ServerHandler for NeoserialHandler {
    fn get_info(&self) -> ServerConfig {
        server_info()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(tool_list_response(build_tools()))
    }

    fn call_tool(
        &self,
        params: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, rmcp::ErrorData>> + Send + '_ {
        let name = params.name.clone();
        let args: Value = params
            .arguments
            .clone()
            .map(Value::Object)
            .unwrap_or(Value::Null);
        let shared = self.shared.clone();
        // 请求取消令牌:客户端断开/取消通知时 rmcp 会 cancel 它。
        // 需要协作式取消的工具(长读、长文件发送)拿到它后停止等待或停止写出;
        // 取消只停本地工作,绝不自动重放串口命令(见 S5 重试分类)。
        let ct = ctx.ct.clone();
        async move {
            // 调用记录:Drop 守卫兜底所有退出路径(含 arg 解析失败的早返回 / panic),
            // 正常流在底部 set 真实 ok+error。守卫 drop 时写环形 + emit 给前端。
            let mut recorder = CallRecorder::new(shared.clone(), name.to_string(), &args);
            let call = parse_tool_call(name.as_ref(), args, &mut recorder)?;
            // 工具执行层并发闸门:旧 session 路径的 POST 会先把 SSE 响应流交回去、
            // 工具在后台跑,HTTP 中间件的 permit 等不到工具结束,盖不住这一段;
            // 新版无状态路径已被中间件全程覆盖,这里再过一道闸只是多记一个额度。
            // 普通工具的新旧协议合计最多 32 个,恢复工具另有 4 个专用额度;
            // 恢复/诊断类工具走专用小配额,过载时仍可通过 MCP 断开、停序列、查状态。
            // 拿不到立刻拒绝,不排队;被拒也留一条调用记录。
            let gate = if PRIORITY_TOOLS.contains(&name.as_ref()) {
                &shared.tool_gate_priority
            } else {
                &shared.tool_gate
            };
            let _tool_permit = match gate.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    let error = format!(
                        "MCP 服务忙:并发工具调用已达上限({}),本次调用未执行,请稍后重试",
                        MCP_MAX_CONCURRENT_REQUESTS
                    );
                    recorder.set_result(false, error.clone());
                    let payload = serde_json::json!({ "ok": false, "error": error });
                    return Ok(content_text(payload).into());
                }
            };
            let resp: Value = match call {
                ToolCall::ListPorts => {
                    // available_ports 在 Windows 走 SetupAPI 枚举(可卡几十~几百 ms),
                    // 丢到阻塞线程池执行,不占 tokio worker。
                    let shared_for_tool = shared.clone();
                    let result = tokio::task::spawn_blocking(move || tools::list_ports(&shared_for_tool)).await;
                    match result {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("枚举串口任务异常终止: {}", e) }),
                    }
                }
                ToolCall::Disconnect(req) => {
                    // tools::disconnect 会同步等 reader/writer 线程退出(最多 1s),
                    // 直接在 async 块里调会占死一个 tokio worker 线程,期间整个
                    // MCP server 的其它 tool 调用一起卡住。丢到阻塞线程池执行。
                    let shared_for_tool = shared.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        tools::disconnect(&shared_for_tool, req)
                    })
                    .await;
                    match result {
                        Ok(Ok(_)) => serde_json::json!({ "ok": true }),
                        Ok(Err(e)) => serde_json::json!({ "ok": false, "error": e }),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("断开任务异常终止: {}", e) }),
                    }
                }
                ToolCall::GetStatus(req) => {
                    serde_json::to_value(tools::get_status(&shared, req)).unwrap_or_default()
                }
                ToolCall::Send(req) => {
                    match tools::send(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                ToolCall::GetHistorySince(req) => {
                    serde_json::to_value(tools::get_history_since(&shared, req)).unwrap_or_default()
                }
                ToolCall::ResetStats(req) => {
                    serde_json::to_value(tools::reset_stats(&shared, req)).unwrap_or_default()
                }
                ToolCall::SendFile(req) => {
                    // 文件读取循环是阻塞 IO,丢到阻塞线程池执行。
                    // 取消令牌 clone 进闭包:阻塞线程里也能看到取消信号。
                    let shared_for_tool = shared.clone();
                    let ct_for_file = ct.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        tools::send_file(&shared_for_tool, req, Some(&ct_for_file))
                    })
                    .await;
                    match result {
                        Ok(Ok(r)) => serde_json::to_value(r).unwrap_or_default(),
                        Ok(Err(e)) => serde_json::json!({ "ok": false, "error": e.error }),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("发送文件任务异常终止: {}", e) }),
                    }
                }
                ToolCall::StartLogging(req) => {
                    // 文件创建/目录创建是阻塞 IO,丢到阻塞线程池执行。
                    let shared_for_tool = shared.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        tools::start_logging(&shared_for_tool, req)
                    })
                    .await;
                    match result {
                        Ok(Ok(r)) => serde_json::to_value(r).unwrap_or_default(),
                        Ok(Err(e)) => serde_json::json!({ "ok": false, "error": e.error }),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("启动日志任务异常终止: {}", e) }),
                    }
                }
                ToolCall::StopLogging => {
                    // 文件句柄释放等阻塞操作,丢到阻塞线程池执行。
                    let shared_for_tool = shared.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        tools::stop_logging(&shared_for_tool)
                    })
                    .await;
                    match result {
                        Ok(Ok(r)) => serde_json::to_value(r).unwrap_or_default(),
                        Ok(Err(e)) => serde_json::json!({ "ok": false, "error": e.error }),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("停止日志任务异常终止: {}", e) }),
                    }
                }
                ToolCall::SequenceRun(req) => {
                    match tools::sequence_run(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                ToolCall::SequenceStop(req) => {
                    match tools::sequence_stop(&shared, req) {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                ToolCall::SequenceStatus(req) => {
                    serde_json::to_value(tools::sequence_status(req)).unwrap_or_default()
                }
                ToolCall::KbSearch(req) => {
                    // 读缓存文件(百 KB 级 JSON 解析),丢到阻塞线程池,不占 tokio worker
                    let shared_for_tool = shared.clone();
                    match tokio::task::spawn_blocking(move || tools::kb_search(&shared_for_tool, req)).await {
                        Ok(Ok(r)) => serde_json::to_value(r).unwrap_or_default(),
                        Ok(Err(e)) => serde_json::to_value(e).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("查询任务异常终止: {}", e) }),
                    }
                }
                ToolCall::KbGet(req) => {
                    let shared_for_tool = shared.clone();
                    match tokio::task::spawn_blocking(move || tools::kb_get(&shared_for_tool, req)).await {
                        Ok(Ok(r)) => serde_json::to_value(r).unwrap_or_default(),
                        Ok(Err(e)) => serde_json::to_value(e).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("查询任务异常终止: {}", e) }),
                    }
                }
                ToolCall::KbManuals => {
                    let shared_for_tool = shared.clone();
                    match tokio::task::spawn_blocking(move || tools::kb_manuals(&shared_for_tool)).await {
                        Ok(Ok(r)) => serde_json::to_value(r).unwrap_or_default(),
                        Ok(Err(e)) => serde_json::to_value(e).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("查询任务异常终止: {}", e) }),
                    }
                }
                ToolCall::GetSettings => {
                    serde_json::to_value(tools::get_settings(&shared)).unwrap_or_default()
                }
                ToolCall::SaveSettings(req) => {
                    // settings.save() 写磁盘 JSON,丢到阻塞线程池执行。
                    let shared_for_tool = shared.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        tools::save_settings(&shared_for_tool, *req)
                    })
                    .await;
                    match result {
                        Ok(Ok(r)) => serde_json::to_value(r).unwrap_or_default(),
                        Ok(Err(e)) => serde_json::json!({ "ok": false, "error": e.error }),
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("保存设置任务异常终止: {}", e) }),
                    }
                }
                ToolCall::SendAndRead(req) => {
                    match tools::send_and_read(&shared, req, Some(&ct)).await {
                        Ok(r) => serde_json::to_value(r).unwrap_or_default(),
                        Err(e) => serde_json::json!({ "ok": false, "error": e.error }),
                    }
                }
                ToolCall::Connect(req) => {
                    // tools::connect 内含阻塞的 serialport open(实测可卡数秒),
                    // 直接在这个 async 块里调会占死一个 tokio worker 线程,期间整个
                    // MCP server 的其它 tool 调用一起卡住。丢到阻塞线程池执行。
                    let shared_for_connect = shared.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        tools::connect(&shared_for_connect, req)
                    })
                    .await;
                    match result {
                        Ok(Ok(r)) => serde_json::to_value(r).unwrap_or_default(),
                        Ok(Err(e)) => serde_json::json!({ "ok": false, "error": e.error }),
                        // 阻塞任务 panic:占位由 ConnectingGuard 的 Drop 清掉,不会留幽灵。
                        Err(e) => serde_json::json!({ "ok": false, "error": format!("连接任务异常终止: {}", e) }),
                    }
                }
            };
            // 正常流(到此处)记真实 ok/error;早返回的 arg 解析失败由守卫兜底记录。
            recorder.set_result(
                resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
                resp.get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            );
            Ok(content_text(resp).into())
        }
    }
}

/// call_tool 的调用记录守卫:无论从哪条路径退出(正常返回 / arg 解析早返回 / panic),
/// drop 时都把这条调用写进环形并 emit 给前端。早返回路径未 set_result → 兜底记 ok=false。
struct CallRecorder {
    shared: Arc<McpShared>,
    activity_id: Option<u64>,
    tool: String,
    args: String,
    start: std::time::Instant,
    /// None = 未走到底部(早返回/异常);Some = 正常返回的真实结果
    result: Option<(bool, String)>,
}

impl CallRecorder {
    fn new(shared: Arc<McpShared>, tool: String, args: &Value) -> Self {
        use crate::mcp::call_log::{truncate, ARGS_MAX};
        let activity = shared.call_log.lock().ok().map(|mut log| {
            let id = log.activity.start(
                &tool,
                super::activity::call_port(&tool, args),
                chrono::Utc::now().timestamp_millis(),
            );
            (id, log.activity.snapshot())
        });
        if let (Some(app), Some((_, snapshot))) = (&shared.app_handle, &activity) {
            let _ = app.emit("mcp-activity", super::activity::McpActivityState {
                running: true,
                activity: snapshot.clone(),
            });
        }
        Self {
            activity_id: activity.map(|(id, _)| id),
            shared,
            tool,
            args: truncate(&args.to_string(), ARGS_MAX),
            start: std::time::Instant::now(),
            result: None,
        }
    }

    fn set_result(&mut self, ok: bool, error: String) {
        self.result = Some((ok, error));
    }
}

impl Drop for CallRecorder {
    fn drop(&mut self) {
        use crate::mcp::call_log::McpCallRecord;
        let (ok, error) = self
            .result
            .take()
            .unwrap_or_else(|| (false, "早返回(参数解析失败?)".to_string()));
        let rec = McpCallRecord {
            ts: chrono::Local::now().to_rfc3339(),
            tool: self.tool.clone(),
            args: self.args.clone(),
            ok,
            error,
            duration_ms: self.start.elapsed().as_millis() as u64,
        };
        let activity = if let Ok(mut log) = self.shared.call_log.lock() {
            log.push(rec.clone());
            if let Some(id) = self.activity_id {
                log.activity.finish(id, ok, chrono::Utc::now().timestamp_millis());
            }
            Some(log.activity.snapshot())
        } else {
            None
        };
        if let Some(app) = &self.shared.app_handle {
            let _ = app.emit("mcp-call", rec);
            if let Some(snapshot) = activity {
                let _ = app.emit("mcp-activity", super::activity::McpActivityState {
                    running: true,
                    activity: snapshot,
                });
            }
        }
    }
}

enum ToolCall {
    ListPorts,
    Connect(tools::ConnectReq),
    Disconnect(tools::DisconnectReq),
    GetStatus(tools::StatusReq),
    Send(tools::SendReq),
    SendAndRead(tools::SendAndReadReq),
    GetHistorySince(tools::GetHistorySinceReq),
    ResetStats(tools::ResetStatsReq),
    SendFile(tools::SendFileReq),
    StartLogging(tools::StartLoggingReq),
    StopLogging,
    SequenceRun(tools::SequenceRunReq),
    SequenceStop(tools::SequenceStopReq),
    SequenceStatus(tools::SequenceStatusReq),
    GetSettings,
    SaveSettings(Box<tools::SaveSettingsReq>),
    KbSearch(tools::KbSearchReq),
    KbGet(tools::KbGetReq),
    KbManuals,
}

fn parse_tool_call(
    name: &str,
    args: Value,
    recorder: &mut CallRecorder,
) -> Result<ToolCall, rmcp::ErrorData> {
    match name {
        LIST_PORTS => Ok(ToolCall::ListPorts),
        CONNECT => parse_tool_args(args, recorder).map(ToolCall::Connect),
        DISCONNECT => parse_tool_args(args, recorder).map(ToolCall::Disconnect),
        GET_STATUS => parse_tool_args(args, recorder).map(ToolCall::GetStatus),
        SEND => parse_tool_args(args, recorder).map(ToolCall::Send),
        SEND_AND_READ => parse_tool_args(args, recorder).map(ToolCall::SendAndRead),
        GET_HISTORY_SINCE => parse_tool_args(args, recorder).map(ToolCall::GetHistorySince),
        RESET_STATS => parse_tool_args(args, recorder).map(ToolCall::ResetStats),
        SEND_FILE => parse_tool_args(args, recorder).map(ToolCall::SendFile),
        START_LOGGING => parse_tool_args(args, recorder).map(ToolCall::StartLogging),
        STOP_LOGGING => Ok(ToolCall::StopLogging),
        SEQUENCE_RUN => parse_tool_args(args, recorder).map(ToolCall::SequenceRun),
        SEQUENCE_STOP => parse_tool_args(args, recorder).map(ToolCall::SequenceStop),
        GET_SEQUENCE_STATUS => parse_tool_args(args, recorder).map(ToolCall::SequenceStatus),
        GET_SETTINGS => Ok(ToolCall::GetSettings),
        SAVE_SETTINGS => parse_tool_args(args, recorder)
            .map(|req| ToolCall::SaveSettings(Box::new(req))),
        KB_SEARCH => parse_tool_args(args, recorder).map(ToolCall::KbSearch),
        KB_GET => parse_tool_args(args, recorder).map(ToolCall::KbGet),
        KB_MANUALS => Ok(ToolCall::KbManuals),
        _ => {
            let error = format!("未知工具: {}", name);
            recorder.set_result(false, error.clone());
            Err(rmcp::ErrorData::invalid_params(error, None))
        }
    }
}

fn parse_tool_args<T: serde::de::DeserializeOwned>(
    args: Value,
    recorder: &mut CallRecorder,
) -> Result<T, rmcp::ErrorData> {
    serde_json::from_value(args).map_err(|error| {
        let message = error.to_string();
        recorder.set_result(false, message.clone());
        rmcp::ErrorData::invalid_params(message, None)
    })
}

/// 把 JSON 执行结果包成 CallToolResult,业务失败标记 isError=true。
///
/// **统一构造器**:正常与业务失败结果都走这里,
/// 保证双表示永远一致:
/// - `content[0].text` = JSON 的字符串形式——**旧客户端只读文本也完全可用**;
/// - `structuredContent` = 同一份 JSON 的结构化形式——新客户端直接读字段。
///
fn content_text(json: Value) -> CallToolResult {
    let mut result = CallToolResult::success(vec![ContentBlock::text(json.to_string())]);
    result.is_error = Some(json.get("ok").and_then(Value::as_bool) == Some(false));
    result.structured_content = Some(json);
    result
}

/// `/mcp` 入口的最大并发请求数。
///
/// 32 是折中值:一个 agent 的正常批量调用远少于 32 个并发,而超过这个量之后
/// 继续排队只会把串口服务与 Tauri 进程一起拖死,不如立刻回 429 让客户端重试。
/// 常量写死、不做成设置项——它保护的是服务端资源,不是用户偏好。
pub const MCP_MAX_CONCURRENT_REQUESTS: usize = 32;

/// 恢复/诊断类工具的**专用**并发配额(与主闸相互独立)。
///
/// 过载(主闸占满)恰恰是需要 disconnect / sequence_stop 来恢复、get_status /
/// list_ports / get_sequence_status 来确认状态的时刻——这些轻量工具不该和
/// 发送类工具抢主闸 permit,否则会出现"服务过载到无法通过 MCP 恢复"的死结。
/// 4 个并发远超任何真实 agent 的诊断/恢复需要,同时保持有界。
pub const MCP_PRIORITY_TOOL_CONCURRENCY: usize = 4;

/// 走专用配额的工具:只读诊断 + 紧急恢复,全部轻量、无长阻塞。
const PRIORITY_TOOLS: &[&str] = &[
    LIST_PORTS,
    GET_STATUS,
    GET_SEQUENCE_STATUS,
    DISCONNECT,
    SEQUENCE_STOP,
];

/// 过载拒绝累计次数。只用于日志,不参与限流判断(没有客户端身份,按人头限流无从谈起)。
static MCP_OVERLOAD_REJECTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn mcp_overloaded_response() -> axum::response::Response {
    let rejected = MCP_OVERLOAD_REJECTED
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    log::warn!(
        "MCP 过载拒绝(累计 {} 次):并发请求数已达上限 {}",
        rejected,
        MCP_MAX_CONCURRENT_REQUESTS
    );
    (
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        [(axum::http::header::RETRY_AFTER, "1")],
        format!(
            "MCP 服务忙:并发请求数已达上限(普通 {},恢复 {}),请稍后重试",
            MCP_MAX_CONCURRENT_REQUESTS,
            MCP_PRIORITY_TOOL_CONCURRENCY
        ),
    )
        .into_response()
}

async fn mcp_concurrency_gate(
    limiter: Arc<tokio::sync::Semaphore>,
    priority_limiter: Arc<tokio::sync::Semaphore>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let permit = match limiter.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            if req.method() != axum::http::Method::POST
                || req.headers().get("mcp-method").is_some_and(|method| method != "tools/call")
                || req.headers().get("mcp-name").is_some_and(|name| {
                    !PRIORITY_TOOLS.iter().any(|tool| name == *tool)
                })
            {
                return mcp_overloaded_response();
            }
            let Ok(permit) = priority_limiter.try_acquire_owned() else {
                return mcp_overloaded_response();
            };
            let (parts, body) = req.into_parts();
            let body = match tokio::time::timeout(
                std::time::Duration::from_secs(1),
                axum::body::to_bytes(body, http_config().max_request_body_bytes),
            ).await {
                Ok(Ok(body)) => body,
                Ok(Err(error)) => return (
                    axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                    format!("MCP 请求体读取失败或超过大小限制: {error}"),
                ).into_response(),
                Err(_) => return (
                    axum::http::StatusCode::REQUEST_TIMEOUT,
                    "MCP 恢复请求体读取超时",
                ).into_response(),
            };
            let priority = serde_json::from_slice::<Value>(&body).ok().is_some_and(|message| {
                message["method"] == "tools/call"
                    && message["params"]["name"].as_str()
                        .is_some_and(|name| PRIORITY_TOOLS.contains(&name))
            });
            if !priority {
                return mcp_overloaded_response();
            }
            req = axum::extract::Request::from_parts(parts, axum::body::Body::from(body));
            permit
        }
    };
    let _permit = permit;
    next.run(req).await
}

/// 组装 `/mcp` 路由。**生产与协议测试夹具共用同一份路由、同一份 http_config()**,
/// 避免"测试通过但线上配置没生效"。
/// factory 每次请求 clone 同一个 Arc<McpShared>:业务状态是应用级的,
/// HTTP session 只承载协议状态,绝不管理串口连接生命周期。
pub fn mcp_router(shared: Arc<McpShared>) -> axum::Router {
    let shared_for_factory = shared.clone();
    let service = StreamableHttpService::new(
        move || Ok(NeoserialHandler::new(shared_for_factory.clone())),
        Arc::new(LocalSessionManager::default()),
        http_config(),
    );
    // 这个 router 只有 /mcp 一个入口,所以整层限流等价于"只保护 /mcp"。
    let limiter = Arc::new(tokio::sync::Semaphore::new(MCP_MAX_CONCURRENT_REQUESTS));
    let priority_limiter = Arc::new(tokio::sync::Semaphore::new(MCP_PRIORITY_TOOL_CONCURRENCY));
    axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn(move |req, next| {
            let limiter = limiter.clone();
            let priority_limiter = priority_limiter.clone();
            async move { mcp_concurrency_gate(limiter, priority_limiter, req, next).await }
        }))
}

/// 起 MCP streamable HTTP server,bind 127.0.0.1:port/mcp。
/// 接收已绑定的 std TcpListener(由 bind_port 返回),转 tokio listener 避免竞态。
/// 在 lib.rs setup 里用 tauri::async_runtime::spawn 调用。
pub async fn run_server(shared: Arc<McpShared>, listener: std::net::TcpListener) -> Result<(), String> {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let app = mcp_router(shared);
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

#[cfg(test)]
#[path = "activity_recorder_tests.rs"]
mod activity_recorder_tests;
