//! MCP 传输层协议黑盒测试。
//!
//! 这些测试**不依赖物理串口**:起一个绑定 127.0.0.1:0 的临时 MCP endpoint,
//! 用原始 HTTP/1.1 字节流发请求,逐字检查状态码、响应头与响应体。
//!
//! 关键约束:
//! - 不用任何 HTTP 客户端库。reqwest 之类会替你补 Host、改写头,
//!   没法验证"恶意 Host 被拒""超长 body 被拒"这类边界。原始 socket 才测得准。
//! - 同时支持 `application/json` 与 `text/event-stream` 两种响应。
//!   MCP 2026-07-28 的服务端可以任选其一,客户端必须都认。
//! - 夹具复用生产代码的 `http_config()` / `build_tools()` / `mcp_router()`,
//!   避免"测试通过但线上配置没生效"。

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::server::{
    build_tools, mcp_router, server_info, tool_list_response,
};
use super::state::test_support::{fake_connection, test_shared};
use super::state::McpShared;

/// 连接表里的端口名快照。
///
/// 单独一个同步函数:锁只在这个函数的作用域里,不跨 await——
/// std Mutex 的 guard 跨 await 会占死 tokio worker,clippy 也会报
/// `await_holding_lock`(它不认显式 `drop()` 结束 guard 生命周期)。
fn connection_ports(shared: &McpShared) -> Vec<String> {
    let conns = shared.connections.lock().unwrap();
    conns.keys().cloned().collect()
}

/// 该端口的连接是否仍然存活(同上:锁不跨 await)。
fn connection_running(shared: &McpShared, port: &str) -> bool {
    let conns = shared.connections.lock().unwrap();
    conns.get(port)
        .map(|h| h.running.load(std::sync::atomic::Ordering::SeqCst))
        .unwrap_or(false)
}

/// MCP 协议版本(新版,无状态路径)。
const PROTOCOL_2026: &str = "2026-07-28";
/// MCP 协议版本(旧版,session 路径)。
const PROTOCOL_2025: &str = "2025-11-25";

// ============ 原始 HTTP 客户端 ============

/// 一次原始 HTTP 响应的解析结果。
#[derive(Debug, Clone)]
struct RawResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RawResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn content_type(&self) -> &str {
        self.header("content-type").unwrap_or("")
    }

    fn is_sse(&self) -> bool {
        self.content_type().starts_with("text/event-stream")
    }

    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// 解析 JSON-RPC 响应体。JSON 与 SSE 两种编码都认。
    /// 返回 None 表示没有可解析的 JSON-RPC 响应(例如 403/413 的纯文本错误体)。
    fn jsonrpc(&self) -> Option<Value> {
        if self.is_sse() {
            for frame in self.body_text().split("\n\n") {
                let data = frame
                    .lines()
                    .filter_map(|l| l.strip_prefix("data:"))
                    .map(|l| l.strip_prefix(' ').unwrap_or(l))
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(&data) {
                    if v.get("id").is_some() {
                        return Some(v);
                    }
                }
            }
            None
        } else {
            serde_json::from_slice(&self.body).ok()
        }
    }

    /// JSON-RPC 的 result 字段。
    fn result(&self) -> Option<Value> {
        self.jsonrpc()?.get("result").cloned()
    }
}

/// 一次原始 HTTP 请求的描述。
struct RawRequest {
    method: &'static str,
    path: &'static str,
    /// 额外的请求头。会原样写进报文,不做任何规范化。
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// 覆盖 Host 头。为 None 时用 `127.0.0.1:<port>`。
    host: Option<String>,
}

impl RawRequest {
    fn new(method: &'static str, path: &'static str) -> Self {
        RawRequest { method, path, headers: Vec::new(), body: Vec::new(), host: None }
    }

    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    fn host(mut self, host: &str) -> Self {
        self.host = Some(host.to_string());
        self
    }

    fn body(mut self, body: Value) -> Self {
        self.body = serde_json::to_vec(&body).unwrap();
        self
    }

    fn raw_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }
}

/// 发一个原始 HTTP 请求并读完响应。
///
/// 读法上有两个分支:
/// - 带 `Content-Length`:按长度读满即止;
/// - 不带(SSE / chunked):读到"出现一个完整的、带 id 的 SSE 帧"或连接关闭为止。
///   SSE 响应服务端不一定会立刻关连接,所以不能简单地 read_to_end。
async fn send_raw(port: u16, req: &RawRequest) -> RawResponse {
    send_raw_inner(port, req, false).await
}

/// 同上,但用 `Transfer-Encoding: chunked` 发 body(不带 Content-Length)。
async fn send_raw_chunked(port: u16, req: &RawRequest) -> RawResponse {
    send_raw_inner(port, req, true).await
}

async fn send_raw_inner(port: u16, req: &RawRequest, chunked_request: bool) -> RawResponse {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.expect("连接测试 endpoint");
    let host = req.host.clone().unwrap_or_else(|| format!("127.0.0.1:{}", port));
    let mut head = format!("{} {} HTTP/1.1\r\n", req.method, req.path);
    head.push_str(&format!("Host: {}\r\n", host));
    for (k, v) in &req.headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    if chunked_request {
        head.push_str("Transfer-Encoding: chunked\r\n");
    } else {
        head.push_str(&format!("Content-Length: {}\r\n", req.body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).await.expect("写请求头");
    if chunked_request {
        // 分块写:每块 64 KiB,末块长度为 0
        for piece in req.body.chunks(64 * 1024) {
            let header = format!("{:x}\r\n", piece.len());
            stream.write_all(header.as_bytes()).await.expect("写 chunk 头");
            stream.write_all(piece).await.expect("写 chunk 体");
            stream.write_all(b"\r\n").await.expect("写 chunk 尾");
        }
        stream.write_all(b"0\r\n\r\n").await.expect("写末块");
    } else {
        stream.write_all(&req.body).await.expect("写请求体");
    }
    stream.flush().await.expect("flush");

    // 读够头部(\r\n\r\n)
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    let header_end = loop {
        if let Some(pos) = find_sub(&raw, b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut buf).await.expect("读响应头");
        if n == 0 {
            break raw.len();
        }
        raw.extend_from_slice(&buf[..n]);
    };
    let header_text = String::from_utf8_lossy(&raw[..header_end]).to_string();
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next().unwrap_or("");
    let status: u16 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut body = raw[std::cmp::min(header_end + 4, raw.len())..].to_vec();

    let content_length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok());
    let chunked = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked"));

    if let Some(len) = content_length {
        while body.len() < len {
            let n = stream.read(&mut buf).await.expect("读响应体");
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
        }
        body.truncate(len);
    } else {
        // 无长度:可能是 SSE。读到出现完整的 JSON-RPC 响应帧为止,否则读到连接关闭。
        let sse = headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.starts_with("text/event-stream"));
        loop {
            if sse && sse_frame_complete(&body) {
                break;
            }
            if chunked && chunked_body_complete(&body) {
                break;
            }
            let n = stream.read(&mut buf).await.expect("读响应体");
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
        }
    }
    RawResponse { status, headers, body }
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// SSE 帧是否已经完整:出现了以空行结尾、且 data 里带 JSON-RPC id 的帧。
fn sse_frame_complete(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body);
    for frame in text.split("\n\n") {
        let data = frame
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|l| l.strip_prefix(' ').unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&data) {
            if v.get("id").is_some() {
                return true;
            }
        }
    }
    false
}

/// chunked body 是否已经收到结束块(0\r\n\r\n)。
fn chunked_body_complete(body: &[u8]) -> bool {
    find_sub(body, b"\r\n0\r\n\r\n").is_some() || body.starts_with(b"0\r\n\r\n")
}

// ============ 测试 endpoint 夹具 ============

/// 起一个只绑定回环地址的临时 MCP endpoint,返回实际端口。
///
/// 走生产的 `mcp_router()`:路由、http_config()、handler 全是线上那份,
/// 只有监听端口是临时的。
pub(crate) async fn spawn_test_endpoint() -> u16 {
    spawn_test_endpoint_with(test_shared()).await
}

/// 同上,但用指定的共享状态(便于测试里预置连接)。
pub(crate) async fn spawn_test_endpoint_with(shared: Arc<McpShared>) -> u16 {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("绑定临时端口");
    let port = listener.local_addr().unwrap().port();
    let app = mcp_router(shared);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    // 等 server 真正起来(accept 队列已就绪)
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    port
}


// ============ 请求构造辅助 ============

/// 新版(2026-07-28)请求体:带协议版本与客户端能力的 _meta。
fn modern_body(id: u32, method: &str, params: Value) -> Value {
    let mut body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": PROTOCOL_2026,
                "io.modelcontextprotocol/clientCapabilities": {},
                "io.modelcontextprotocol/clientInfo": {"name": "protocol-tests", "version": "0"}
            }
        }
    });
    if let Some(extra) = params.as_object() {
        body["params"].as_object_mut().expect("params 是对象").extend(extra.clone());
    }
    body
}

/// 新版请求的标准头。
/// `tool_name` 非空时额外带 `Mcp-Name`:rmcp 3.4.0 要求 tools/call 的
/// Mcp-Name 头与正文里的工具名一致,不一致直接 400。
fn modern_headers(method: &str, tool_name: Option<&str>) -> Vec<(String, String)> {
    let mut headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Accept".to_string(), "application/json, text/event-stream".to_string()),
        ("Mcp-Method".to_string(), method.to_string()),
        ("MCP-Protocol-Version".to_string(), PROTOCOL_2026.to_string()),
    ];
    if let Some(name) = tool_name {
        headers.push(("Mcp-Name".to_string(), name.to_string()));
    }
    headers
}

// ============ 工具清单契约 ============

/// 19 个工具的顺序快照。顺序变了就是破坏性变更,必须显式改这里。
const EXPECTED_TOOL_ORDER: [&str; 19] = [
    "list_ports",
    "connect",
    "disconnect",
    "get_status",
    "send",
    "send_and_read",
    "get_history_since",
    "reset_stats",
    "send_file",
    "start_logging",
    "stop_logging",
    "sequence_run",
    "sequence_stop",
    "get_sequence_status",
    "get_settings",
    "save_settings",
    "kb_search",
    "kb_get",
    "kb_manuals",
];

#[test]
fn tool_list_has_cache_metadata_for_strict_clients() {
    let tool = rmcp::model::Tool::new(
        "sample",
        "sample tool",
        Arc::new(serde_json::Map::new()),
    );
    let value = serde_json::to_value(tool_list_response(vec![tool])).unwrap();
    assert_eq!(value["ttlMs"], 0);
    assert_eq!(value["cacheScope"], "private");
    assert_eq!(value["tools"][0]["name"], "sample");
    // 旧客户端仍可以读取原有 tools;缓存字段是附加信息。
    assert_eq!(value["tools"].as_array().unwrap().len(), 1);
}

#[test]
fn build_tools_keeps_the_19_tools_in_fixed_order() {
    let tools = build_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(names, EXPECTED_TOOL_ORDER.to_vec());
    // 每个工具都必须有名字、描述和 inputSchema
    for t in &tools {
        assert!(!t.name.is_empty(), "工具名不能为空");
        assert!(t.description.as_ref().is_some_and(|d| !d.is_empty()), "{} 缺描述", t.name);
        assert!(t.input_schema.get("type").is_some(), "{} 缺 inputSchema.type", t.name);
    }
}

#[test]
fn tool_list_response_carries_cache_metadata_and_same_order() {
    let value = serde_json::to_value(tool_list_response(build_tools())).unwrap();
    assert_eq!(value["ttlMs"], 0, "TTL 必须保持 0(不缓存)");
    assert_eq!(value["cacheScope"], "private");
    let names: Vec<&str> = value["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, EXPECTED_TOOL_ORDER.to_vec());
}

#[test]
fn server_info_identifies_neoserial() {
    let value = serde_json::to_value(server_info()).unwrap();
    assert_eq!(value["serverInfo"]["name"], "NeoSerial");
}

/// 受影响的三个工具必须把"取消后怎么办"写进描述里,否则 agent 只能猜。
/// 同时盯住"别自相矛盾":不能一处说重发、另一处说不重发。
#[test]
fn affected_tool_descriptions_contain_recovery_guidance() {
    let tools = build_tools();
    let desc = |name: &str| -> String {
        tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("没有工具 {}", name))
            .description
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default()
    };

    let sar = desc("send_and_read");
    assert!(sar.contains("get_history_since"), "send_and_read 要指路续读: {}", sar);
    assert!(sar.contains("不要重发"), "send_and_read 要写明不要重发: {}", sar);
    assert!(sar.contains("retryable"), "send_and_read 要说明 retryable: {}", sar);

    let sf = desc("send_file");
    assert!(sf.contains("不要自动重发"), "send_file 要写明不要自动重发: {}", sf);
    assert!(sf.contains("人工确认"), "send_file 要写明需人工确认设备状态: {}", sf);
    assert!(sf.contains("retryable"), "send_file 要说明 retryable: {}", sf);

    let sr = desc("sequence_run");
    assert!(sr.contains("get_sequence_status"), "sequence_run 要指路查状态: {}", sr);
    assert!(sr.contains("不要重复启动"), "sequence_run 要写明不要重复启动: {}", sr);

    // 不该出现"取消后自动重发"这种自相矛盾的说法
    for name in ["send_and_read", "send_file", "sequence_run"] {
        let d = desc(name);
        assert!(
            !d.contains("自动重发") || d.contains("不要自动重发") || d.contains("不要重发"),
            "{} 的描述出现了矛盾的自动重发说法: {}",
            name,
            d
        );
    }
}

/// 打印当前 tools/list 快照(名称、顺序、描述、inputSchema、ttlMs、cacheScope)。
/// 改了工具定义后用它重新生成基线:`cargo test -- --ignored print_tool_snapshot --nocapture`。
#[test]
#[ignore]
fn print_tool_snapshot() {
    let value = serde_json::to_value(tool_list_response(build_tools())).unwrap();
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}

// ============ 黑盒:新版无状态路径 ============

#[tokio::test]
async fn modern_discover_then_tools_list_without_session() {
    let port = spawn_test_endpoint().await;

    // server/discover:声明支持的协议版本
    let discover = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("server/discover", None))
            .body(modern_body(1, "server/discover", json!({}))),
    )
    .await;
    assert_eq!(discover.status, 200, "discover 应成功: {}", discover.body_text());
    let versions = discover.result().unwrap()["supportedVersions"].as_array().cloned().unwrap();
    assert!(
        versions.iter().any(|v| v.as_str() == Some(PROTOCOL_2026)),
        "supportedVersions 应含 {}: {:?}",
        PROTOCOL_2026,
        versions
    );

    // 直接 tools/list:不 initialize、不带 session
    let list = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(modern_body(2, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(list.status, 200, "tools/list 应成功: {}", list.body_text());
    assert!(
        list.header("mcp-session-id").is_none(),
        "新版无状态路径不应签发 Mcp-Session-Id,实际: {:?}",
        list.header("mcp-session-id")
    );
    let result = list.result().expect("tools/list 应有 result");
    assert_eq!(result["ttlMs"], 0);
    assert_eq!(result["cacheScope"], "private");
    let names: Vec<&str> = result["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, EXPECTED_TOOL_ORDER.to_vec());
}

#[tokio::test]
async fn modern_tool_call_returns_text_content() {
    let port = spawn_test_endpoint().await;
    // get_status 是只读工具:端口没连,返回 connected=false,不碰硬件。
    let call = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/call", Some("get_status")))
            .body(modern_body(
                3,
                "tools/call",
                json!({"name": "get_status", "arguments": {"port": "COM-not-open"}}),
            )),
    )
    .await;
    assert_eq!(call.status, 200, "tools/call 应成功: {}", call.body_text());
    let result = call.result().expect("tools/call 应有 result");
    assert_eq!(result["isError"], false, "成功查询不应标记为工具错误");
    assert!(result["content"].is_array(), "必须有 content 文本表示");
    // S0 基线:文本表示就是完整 JSON,旧客户端只读文本即可
    let text = result["content"][0]["text"].as_str().unwrap();
    let from_text: Value = serde_json::from_str(text).expect("content[0].text 必须是 JSON");
    assert_eq!(from_text["ok"], true);
    assert_eq!(from_text["connected"], false);
}

// ============ 黑盒:Origin / Host / 请求体边界 ============

/// 浏览器发来的请求必带 Origin;原生 MCP 客户端不带。
/// 开启强制校验后,带 Origin 的一律 403,不带 Origin 的照常 200。
#[tokio::test]
async fn requests_with_origin_are_rejected() {
    let port = spawn_test_endpoint().await;
    for origin in [
        "https://evil.example",
        "http://localhost:8080",
        "null",
        "ht!tp://malformed",
        "http://",
    ] {
        let resp = send_raw(
            port,
            &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/list", None))
                .header("Origin", origin),
        )
        .await;
        assert_eq!(resp.status, 403, "Origin: {} 应被拒,实际 {}", origin, resp.status);
    }
}

/// 不带 Origin 的原生客户端不受影响(两种协议版本都要能跑)。
#[tokio::test]
async fn requests_without_origin_still_work() {
    let port = spawn_test_endpoint().await;
    let modern = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(modern_body(1, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(modern.status, 200, "无 Origin 的新版请求应成功: {}", modern.body_text());

    let legacy = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .body(json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": PROTOCOL_2025,
                    "capabilities": {},
                    "clientInfo": {"name": "legacy-tests", "version": "0"}
                }
            })),
    )
    .await;
    assert_eq!(legacy.status, 200, "无 Origin 的旧版 initialize 应成功: {}", legacy.body_text());
    assert!(legacy.header("mcp-session-id").is_some(), "旧协议仍应签发 session");
}

/// Host 必须是回环地址:挡 DNS rebinding(把 evil.example 解析到 127.0.0.1)。
#[tokio::test]
async fn non_loopback_host_is_rejected() {
    let port = spawn_test_endpoint().await;
    for host in ["evil.example", "evil.example:1234", "192.168.1.10"] {
        let resp = send_raw(
            port,
            &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/list", None))
                .host(host)
                .body(modern_body(1, "tools/list", json!({}))),
        )
        .await;
        assert_eq!(resp.status, 403, "Host: {} 应被拒,实际 {}", host, resp.status);
    }
    // 合法的回环 Host(带实际端口)照常通过
    let ok = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .host(&format!("127.0.0.1:{}", port))
            .body(modern_body(2, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(ok.status, 200, "回环 Host 应成功: {}", ok.body_text());
}

/// 超过 SDK 请求体上限(4 MiB)返回 413。SDK 是流式读 body 时限长,
/// 与 Content-Length / chunked / HTTP 版本都无关。
#[tokio::test]
async fn oversized_body_is_rejected_with_413() {
    let port = spawn_test_endpoint().await;
    // 4 MiB + 1 KiB:刚好越过 SDK 默认上限
    let big = vec![b'x'; 4 * 1024 * 1024 + 1024];
    let resp = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .raw_body(big),
    )
    .await;
    assert_eq!(resp.status, 413, "超大 body 应返回 413,实际 {}", resp.status);
    assert!(
        resp.body_text().contains("Payload Too Large"),
        "413 的响应体应说明原因: {}",
        resp.body_text()
    );
}

/// chunked 传输同样受上限约束(SDK 流式读 body,不看 Content-Length)。
#[tokio::test]
async fn oversized_chunked_body_is_rejected_with_413() {
    let port = spawn_test_endpoint().await;
    let big = vec![b'x'; 4 * 1024 * 1024 + 1024];
    let resp = send_raw_chunked(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .raw_body(big),
    )
    .await;
    assert_eq!(resp.status, 413, "超大 chunked body 应返回 413,实际 {}", resp.status);
}

// ============ 黑盒:并发过载保护 ============

/// 发起一次"慢"调用:send_and_read 在假连接上等满静默间隙超时才返回。
/// 用来占住 /mcp 的并发额度。
async fn slow_call(port: u16, id: u32) -> RawResponse {
    send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/call", Some("send_and_read")))
            .body(modern_body(
                id,
                "tools/call",
                json!({
                    "name": "send_and_read",
                    "arguments": {"port": "COM-slow", "text": "AT", "timeout_ms": 3000}
                }),
            )),
    )
    .await
}

/// 并发打满上限后,第 N+1 个请求要**快速**收到 429,而不是挂在那儿排队。
#[tokio::test]
async fn requests_over_the_concurrency_limit_get_429() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared).await;

    // 占满 32 个额度
    let mut slow = Vec::new();
    for i in 0..crate::mcp::server::MCP_MAX_CONCURRENT_REQUESTS as u32 {
        slow.push(tokio::spawn(slow_call(port, 100 + i)));
    }
    // 等慢请求真正占住额度(轮询到出现 429 为止,避免竞态导致 flaky)
    let mut over = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let resp = send_raw(
            port,
            &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/list", None))
                .body(modern_body(1, "tools/list", json!({}))),
        )
        .await;
        if resp.status == 429 {
            over = Some(resp);
            break;
        }
    }
    let over = over.expect("并发打满后应有请求收到 429");
    assert_eq!(over.header("retry-after"), Some("1"), "429 必须带 Retry-After");
    assert!(
        over.body_text().contains("并发请求数已达上限"),
        "429 的响应体应说明原因: {}",
        over.body_text()
    );

    // 慢请求结束后额度释放,新请求恢复正常
    for h in slow {
        let _ = h.await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let ok = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(modern_body(2, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(ok.status, 200, "额度释放后应恢复正常: {}", ok.body_text());
}

/// 一个长调用只占一个额度,轻量请求照常通行(不能因为有慢请求就整体被拒)。
#[tokio::test]
async fn a_slow_call_does_not_block_lightweight_requests() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared).await;

    let slow = tokio::spawn(slow_call(port, 200));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    for id in 201..205u32 {
        let resp = send_raw(
            port,
            &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/call", Some("get_status")))
                .body(modern_body(
                    id,
                    "tools/call",
                    json!({"name": "get_status", "arguments": {"port": "COM-slow"}}),
                )),
        )
        .await;
        assert_eq!(resp.status, 200, "轻量请求不该被慢请求挤掉: {}", resp.body_text());
    }
    let _ = slow.await;
}

// ============ 黑盒:协议元数据一致性(SEP-2243) ============

/// Header 与 Body 的方法名不一致 → 400 + HEADER_MISMATCH,不是 500。
#[tokio::test]
async fn method_header_mismatch_is_rejected() {
    let port = spawn_test_endpoint().await;
    // 注意:这里不能叠加 modern_headers(它自己带一个 Mcp-Method),
    // 两个同名头时服务端取第一个,测的就不是"不一致"了。
    let resp = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2026)
            .header("Mcp-Method", "tools/call")
            .body(modern_body(1, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(resp.status, 400, "方法名不一致应 400,实际 {}", resp.status);
    let err = resp.jsonrpc().expect("应返回 JSON-RPC 错误");
    assert_eq!(err["error"]["code"], -32020, "应是 HEADER_MISMATCH: {}", err);
    assert!(
        err["error"]["message"].as_str().unwrap().contains("does not match body method"),
        "错误信息应说明哪里不一致: {}",
        err["error"]["message"]
    );
}

/// tools/call 的 Mcp-Name 头与正文工具名不一致 → 400。
#[tokio::test]
async fn tool_name_header_mismatch_is_rejected() {
    let port = spawn_test_endpoint().await;
    let resp = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/call", Some("get_status")))
            .body(modern_body(
                1,
                "tools/call",
                json!({"name": "list_ports", "arguments": {}}),
            )),
    )
    .await;
    assert_eq!(resp.status, 400, "工具名不一致应 400,实际 {}", resp.status);
    let err = resp.jsonrpc().expect("应返回 JSON-RPC 错误");
    assert!(
        err["error"]["message"].as_str().unwrap().contains("does not match body value"),
        "错误信息应说明哪里不一致: {}",
        err["error"]["message"]
    );
}

/// 缺 MCP-Protocol-Version 头(但 _meta 里带了版本)→ 400,不是静默放行。
#[tokio::test]
async fn missing_protocol_version_header_is_rejected() {
    let port = spawn_test_endpoint().await;
    let resp = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("Mcp-Method", "tools/list")
            .body(modern_body(1, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(resp.status, 400, "缺协议头应 400,实际 {}", resp.status);
    let err = resp.jsonrpc().expect("应返回 JSON-RPC 错误");
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("requires MCP-Protocol-Version header"),
        "错误信息应说明缺协议头: {}",
        err["error"]["message"]
    );
}

/// 头与 _meta 的协议版本不一致 → 400。
#[tokio::test]
async fn protocol_version_header_meta_mismatch_is_rejected() {
    let port = spawn_test_endpoint().await;
    let mut body = modern_body(1, "tools/list", json!({}));
    body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(PROTOCOL_2025);
    let resp = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(body),
    )
    .await;
    assert_eq!(resp.status, 400, "版本不一致应 400,实际 {}", resp.status);
    let err = resp.jsonrpc().expect("应返回 JSON-RPC 错误");
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not match request _meta protocolVersion"),
        "错误信息应说明版本不一致: {}",
        err["error"]["message"]
    );
}

/// 2026 请求完全不带 _meta(缺协议版本/客户端能力)→ 400 invalid params,不是 500。
#[tokio::test]
async fn missing_request_meta_is_rejected_with_invalid_params() {
    let port = spawn_test_endpoint().await;
    let resp = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}})),
    )
    .await;
    assert_eq!(resp.status, 400, "缺 _meta 应 400,实际 {}", resp.status);
    let err = resp.jsonrpc().expect("应返回 JSON-RPC 错误");
    assert_eq!(err["error"]["code"], -32602, "应是 INVALID_PARAMS: {}", err);
}

/// 未知协议版本 → 400,不是 500。
#[tokio::test]
async fn unknown_protocol_version_is_rejected() {
    let port = spawn_test_endpoint().await;
    let resp = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("Mcp-Method", "tools/list")
            .header("MCP-Protocol-Version", "1999-01-01")
            .body(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}})),
    )
    .await;
    assert_eq!(resp.status, 400, "未知协议版本应 400,实际 {}", resp.status);
    assert!(
        resp.body_text().contains("Unsupported MCP-Protocol-Version"),
        "错误信息应说明版本不支持: {}",
        resp.body_text()
    );
}

/// 伪造的 Mcp-Session-Id 不会被用来恢复或隔离业务状态。
///
/// - 新版请求走无状态路径:这个头被直接忽略(200),既不恢复到任何会话,
///   也不把请求绑到那个会话上;
/// - 旧版请求走 session 路径:session 不存在 → 404,不会凭空造出一个会话。
///
/// 两种情况都不许动串口连接表和调用日志。
#[tokio::test]
async fn forged_session_id_does_not_touch_business_state() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-forged");
    shared.connections.lock().unwrap().insert("COM-forged".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;

    let before_log = shared.call_log.lock().unwrap().snapshot().len();

    // 新版:伪造 session 被忽略,请求照常走无状态路径
    let modern = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .header("Mcp-Session-Id", "forged-session-id-not-real")
            .body(modern_body(1, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(modern.status, 200, "新版请求应忽略伪造 session: {}", modern.body_text());
    assert!(
        modern.header("mcp-session-id").is_none(),
        "伪造 session 不该被回签: {:?}",
        modern.header("mcp-session-id")
    );

    // 旧版:伪造 session 直接 404
    let legacy = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .header("Mcp-Session-Id", "forged-session-id-not-real")
            .body(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})),
    )
    .await;
    assert_eq!(legacy.status, 404, "旧版伪造 session 应 404,实际 {}", legacy.status);

    // 业务状态没被动过
    let ports = connection_ports(&shared);
    assert_eq!(ports.len(), 1, "伪造 session 不该动连接表");
    assert!(ports.contains(&"COM-forged".to_string()));
    assert_eq!(
        shared.call_log.lock().unwrap().snapshot().len(),
        before_log,
        "伪造 session 的请求没进 handler,不该产生调用记录"
    );
    // 之后的正常请求照常工作
    let ok = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/call", Some("get_status")))
            .body(modern_body(
                3,
                "tools/call",
                json!({"name": "get_status", "arguments": {"port": "COM-forged"}}),
            )),
    )
    .await;
    assert_eq!(ok.status, 200, "伪造 session 不应影响后续请求: {}", ok.body_text());
}

/// 旧 session 存在时,新版请求仍然走无状态路径(不签发新 session,不被旧 session 影响)。
#[tokio::test]
async fn modern_requests_stay_stateless_while_a_legacy_session_exists() {
    let port = spawn_test_endpoint().await;
    let init = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .body(json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": PROTOCOL_2025,
                    "capabilities": {},
                    "clientInfo": {"name": "legacy-tests", "version": "0"}
                }
            })),
    )
    .await;
    assert_eq!(init.status, 200);
    assert!(init.header("mcp-session-id").is_some(), "旧协议应签发 session");

    let modern = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(modern_body(2, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(modern.status, 200, "旧 session 存在不该影响新版请求: {}", modern.body_text());
    assert!(
        modern.header("mcp-session-id").is_none(),
        "新版请求不应被旧 session 拉成有状态: {:?}",
        modern.header("mcp-session-id")
    );
}

// ============ 黑盒:应用级状态隔离 ============

/// 走旧协议握手,返回签发的 Mcp-Session-Id。
async fn legacy_initialize(port: u16) -> String {
    let init = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .body(json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": PROTOCOL_2025,
                    "capabilities": {},
                    "clientInfo": {"name": "legacy-tests", "version": "0"}
                }
            })),
    )
    .await;
    assert_eq!(init.status, 200, "initialize 应成功: {}", init.body_text());
    init.header("mcp-session-id")
        .expect("旧协议 initialize 必须签发 Mcp-Session-Id")
        .to_string()
}

/// 旧协议下带 session 的 tools/call。
async fn legacy_call(port: u16, session: &str, id: u32, tool: &str, args: Value) -> RawResponse {
    send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .header("Mcp-Session-Id", session)
            .body(json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": tool, "arguments": args}
            })),
    )
    .await
}

/// 业务状态是**应用级**的:不同 HTTP session(甚至没有 session)看到的是同一份连接表。
/// HTTP session 只承载协议状态,绝不能管串口连接的生死。
#[tokio::test]
async fn business_state_is_shared_across_sessions() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-shared");
    shared.connections.lock().unwrap().insert("COM-shared".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;

    let session_a = legacy_initialize(port).await;
    let session_b = legacy_initialize(port).await;
    assert_ne!(session_a, session_b, "两次 initialize 应拿到不同 session");

    // 两个不同的旧 session 都看得到同一个应用连接
    for (session, id) in [(&session_a, 10u32), (&session_b, 11)] {
        let resp = legacy_call(port, session, id, "get_status", json!({"port": "COM-shared"})).await;
        assert_eq!(resp.status, 200, "session {} 的调用应成功: {}", session, resp.body_text());
        let text = resp.result().expect("应有 result")["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["connected"], true, "session {} 应看到同一条连接", session);
    }

    // 新版无状态请求(完全不同的 HTTP 上下文)看到的是同一份应用状态
    let modern = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/call", Some("get_status")))
            .body(modern_body(
                12,
                "tools/call",
                json!({"name": "get_status", "arguments": {"port": "COM-shared"}}),
            )),
    )
    .await;
    assert_eq!(modern.status, 200, "新版调用应成功: {}", modern.body_text());
    let text = modern.result().unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["connected"], true, "新版请求应看到同一条连接");
}

/// 删掉一个旧 session 不影响串口连接、调用日志,也不影响别的 session。
#[tokio::test]
async fn deleting_a_session_keeps_connections_and_call_log() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-keep");
    shared.connections.lock().unwrap().insert("COM-keep".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;

    let session_a = legacy_initialize(port).await;
    let session_b = legacy_initialize(port).await;
    // 先产生一条调用记录
    let _ = legacy_call(port, &session_a, 20, "get_status", json!({"port": "COM-keep"})).await;
    let log_before = shared.call_log.lock().unwrap().snapshot().len();
    assert!(log_before > 0, "应有调用记录");

    let del = send_raw(
        port,
        &RawRequest::new("DELETE", "/mcp")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .header("Mcp-Session-Id", &session_a),
    )
    .await;
    assert!(del.status < 300, "DELETE session 应被接受: {}", del.status);

    // 串口连接没被清掉
    assert!(
        connection_ports(&shared).contains(&"COM-keep".to_string()),
        "删 session 不该断串口连接"
    );
    assert!(connection_running(&shared, "COM-keep"), "删 session 不该把连接置为断开");
    // 调用日志没被清掉
    assert_eq!(shared.call_log.lock().unwrap().snapshot().len(), log_before);

    // 别的 session 照常工作
    let resp = legacy_call(port, &session_b, 21, "get_status", json!({"port": "COM-keep"})).await;
    assert_eq!(resp.status, 200, "另一个 session 应照常工作: {}", resp.body_text());
    // 新版无状态请求也照常工作
    let modern = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/call", Some("get_status")))
            .body(modern_body(
                22,
                "tools/call",
                json!({"name": "get_status", "arguments": {"port": "COM-keep"}}),
            )),
    )
    .await;
    assert_eq!(modern.status, 200, "新版请求应照常工作: {}", modern.body_text());
}

// ============ 极简 JSON Schema 校验器(测试用) ============
//
// 只支持本项目 outputSchema 用到的子集:
// type(含类型数组)/properties/required/additionalProperties/items/enum/
// anyOf/oneOf/minimum/maximum/$ref(#/$defs/xxx)。
//
// 不引第三方校验库:为一组手写 schema 拉一整套依赖不划算,而这组 schema 的
// 子集是封闭的、下面还有校验器自身的自测。schema 漂移由"用真实响应校验"兜住。

/// 按 schema 校验一个实例。失败时返回人类可读的原因。
/// 供本文件与 tools.rs 的单元测试共用(成功形态也要过 schema)。
pub(crate) fn validate_schema(schema: &Value, instance: &Value, root: &Value, path: &str) -> Result<(), String> {
    let Some(obj) = schema.as_object() else {
        return Ok(());
    };
    if let Some(reference) = obj.get("$ref").and_then(|v| v.as_str()) {
        let target = resolve_ref(reference, root)
            .ok_or_else(|| format!("{}: 无法解析 $ref {}", path, reference))?;
        return validate_schema(&target, instance, root, path);
    }
    if let Some(expected) = obj.get("type") {
        if !type_matches(expected, instance) {
            return Err(format!("{}: 类型应为 {},实际 {}", path, expected, type_name(instance)));
        }
    }
    if let Some(allowed) = obj.get("enum").and_then(|v| v.as_array()) {
        if !allowed.contains(instance) {
            return Err(format!("{}: 值 {:?} 不在 enum 允许范围内", path, instance));
        }
    }
    if let Some(branches) = obj.get("anyOf").and_then(|v| v.as_array()) {
        let errs: Vec<String> = branches
            .iter()
            .filter_map(|b| validate_schema(b, instance, root, path).err())
            .collect();
        if errs.len() == branches.len() {
            return Err(format!("{}: 不满足 anyOf 的任何一个分支: {:?}", path, errs));
        }
    }
    if let Some(branches) = obj.get("oneOf").and_then(|v| v.as_array()) {
        let ok = branches
            .iter()
            .filter(|b| validate_schema(b, instance, root, path).is_ok())
            .count();
        if ok != 1 {
            return Err(format!("{}: 应恰好满足一个 oneOf 分支,实际满足 {} 个", path, ok));
        }
    }
    if let Some(min) = obj.get("minimum").and_then(|v| v.as_f64()) {
        if let Some(n) = instance.as_f64() {
            if n < min {
                return Err(format!("{}: {} 小于最小值 {}", path, n, min));
            }
        }
    }
    if let Some(max) = obj.get("maximum").and_then(|v| v.as_f64()) {
        if let Some(n) = instance.as_f64() {
            if n > max {
                return Err(format!("{}: {} 大于最大值 {}", path, n, max));
            }
        }
    }
    match instance {
        Value::Object(map) => {
            if let Some(required) = obj.get("required").and_then(|v| v.as_array()) {
                for key in required.iter().filter_map(|v| v.as_str()) {
                    if !map.contains_key(key) {
                        return Err(format!("{}: 缺少必填字段 {}", path, key));
                    }
                }
            }
            let properties = obj.get("properties").and_then(|v| v.as_object());
            if let Some(props) = properties {
                for (key, value) in map {
                    if let Some(sub) = props.get(key) {
                        validate_schema(sub, value, root, &format!("{}.{}", path, key))?;
                    }
                }
            }
            if obj.get("additionalProperties").and_then(|v| v.as_bool()) == Some(false) {
                for key in map.keys() {
                    let known = properties.map(|p| p.contains_key(key)).unwrap_or(false);
                    if !known {
                        return Err(format!("{}: 出现了 schema 未声明的字段 {}", path, key));
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(item_schema) = obj.get("items") {
                for (i, item) in items.iter().enumerate() {
                    validate_schema(item_schema, item, root, &format!("{}[{}]", path, i))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn resolve_ref(reference: &str, root: &Value) -> Option<Value> {
    let rest = reference.strip_prefix("#/")?;
    let mut current = root;
    for part in rest.split('/') {
        current = current.get(part)?;
    }
    Some(current.clone())
}

fn type_matches(expected: &Value, instance: &Value) -> bool {
    let names: Vec<&str> = match expected {
        Value::String(s) => vec![s.as_str()],
        Value::Array(list) => list.iter().filter_map(|v| v.as_str()).collect(),
        _ => return true,
    };
    names.iter().any(|n| match *n {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        "integer" => instance.as_i64().is_some() || instance.as_u64().is_some(),
        "number" => instance.is_number(),
        _ => true,
    })
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 校验器自测:该拒的必须拒,否则"用真实响应校验"就形同虚设。
#[test]
fn mini_schema_validator_actually_rejects_bad_instances() {
    let schema = json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "count": { "type": "integer" },
            "name": { "type": ["string", "null"] },
            "items": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["ok"],
        "additionalProperties": false
    });
    // 合法的
    assert!(validate_schema(&schema, &json!({"ok": true, "count": 1, "name": null, "items": ["a"]}), &schema, "$").is_ok());
    // 缺必填
    assert!(validate_schema(&schema, &json!({"count": 1}), &schema, "$").is_err());
    // 类型不对
    assert!(validate_schema(&schema, &json!({"ok": "yes"}), &schema, "$").is_err());
    assert!(validate_schema(&schema, &json!({"ok": true, "count": "3"}), &schema, "$").is_err());
    // 多出来的字段
    assert!(validate_schema(&schema, &json!({"ok": true, "zzz": 1}), &schema, "$").is_err());
    // 数组元素类型不对
    assert!(validate_schema(&schema, &json!({"ok": true, "items": [1]}), &schema, "$").is_err());
    // $ref 能解析
    let with_ref = json!({
        "type": "object",
        "properties": {"p": { "$ref": "#/$defs/params" }},
        "$defs": {"params": {"type": "array", "items": {"type": "string"}}}
    });
    assert!(validate_schema(&with_ref, &json!({"p": ["a"]}), &with_ref, "$").is_ok());
    assert!(validate_schema(&with_ref, &json!({"p": [1]}), &with_ref, "$").is_err());
}

// ============ 黑盒:双表示(content + structuredContent) ============

/// 每个工具的最小可用入参(只求"能被调用",不求业务成功)。
fn call_args_for(tool: &str) -> Value {
    match tool {
        "list_ports" | "stop_logging" | "get_settings" | "kb_manuals" => json!({}),
        "connect" => json!({"port": "COM-no-such-port", "baud_rate": 115200}),
        "disconnect" | "get_status" | "reset_stats" | "sequence_stop" | "get_sequence_status" => {
            json!({"port": "COM-no-such-port"})
        }
        "send" => json!({"port": "COM-no-such-port", "text": "AT"}),
        "send_and_read" => json!({"port": "COM-no-such-port", "text": "AT", "timeout_ms": 100}),
        "get_history_since" => json!({"port": "COM-no-such-port", "seq": 0}),
        "send_file" => json!({"port": "COM-no-such-port", "path": "/tmp/neoserial-no-such-file"}),
        "start_logging" => json!({}),
        "sequence_run" => json!({"port": "COM-no-such-port", "commands": [{"command": "AT"}], "run_count": 1}),
        "save_settings" => json!({"settings": crate::config::settings::Settings::default_settings()}),
        "kb_search" => json!({"query": "csq"}),
        "kb_get" => json!({"command": "AT+CSQ"}),
        other => panic!("没给 {} 准备入参", other),
    }
}

/// 19 个工具全部同时给文本与结构化两种表示,且两者内容一致。
/// 业务失败也算——失败路径同样走统一构造器。
#[tokio::test]
async fn every_tool_returns_matching_text_and_structured_content() {
    let port = spawn_test_endpoint().await;
    for (i, tool) in EXPECTED_TOOL_ORDER.iter().enumerate() {
        let id = 1000 + i as u32;
        let resp = send_raw(
            port,
            &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/call", Some(tool)))
                .body(modern_body(
                    id,
                    "tools/call",
                    json!({"name": tool, "arguments": call_args_for(tool)}),
                )),
        )
        .await;
        assert_eq!(resp.status, 200, "{} 调用应成功(协议层): {}", tool, resp.body_text());
        let result = resp.result().unwrap_or_else(|| panic!("{} 应有 result", tool));
        assert!(result["content"].is_array(), "{} 缺 content", tool);
        assert!(
            result["structuredContent"].is_object(),
            "{} 缺 structuredContent: {}",
            tool,
            result
        );
        let text = result["content"][0]["text"].as_str().unwrap();
        let from_text: Value = serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("{} 的 content[0].text 不是 JSON: {} ({})", tool, text, e));
        assert_eq!(
            from_text, result["structuredContent"],
            "{} 的两种表示不一致",
            tool
        );
        assert_eq!(result["isError"], from_text["ok"] == false, "{} 的错误标志不匹配", tool);
        // 旧客户端只读文本:JSON 里有 ok 字段
        assert!(from_text.get("ok").is_some(), "{} 的结果应有 ok 字段", tool);
    }
}

#[tokio::test]
async fn argument_errors_and_unknown_tools_return_protocol_errors() {
    let shared = test_shared();
    let port = spawn_test_endpoint_with(shared.clone()).await;
    let session = legacy_initialize(port).await;
    let mut cases: Vec<_> = build_tools().into_iter()
        .filter(|tool| tool.input_schema.get("required")
            .and_then(Value::as_array).is_some_and(|fields| !fields.is_empty()))
        .map(|tool| (tool.name.to_string(), json!({})))
        .collect();
    cases.push(("start_logging".to_string(), json!({"path": 123})));
    cases.push(("send".to_string(), json!({"port": "COM-not-open", "text": 123})));
    cases.push(("no_such_tool".to_string(), json!({})));

    for legacy in [false, true] {
        for (index, (tool, args)) in cases.iter().enumerate() {
            let id = 100 + index as u32;
            let response = if legacy {
                legacy_call(port, &session, id, tool, args.clone()).await
            } else {
                send_raw(port, &RawRequest::new("POST", "/mcp")
                    .with_headers(modern_headers("tools/call", Some(tool)))
                    .body(modern_body(id, "tools/call", json!({"name": tool, "arguments": args}))))
                    .await
            };
            let error = response.jsonrpc().expect("expected JSON-RPC response");
            assert_eq!(error["id"], id);
            assert_eq!(error["error"]["code"], -32602, "legacy={legacy}, tool={tool}: {error}");
            assert!(error.get("result").is_none(), "{error}");
            assert!(!error["error"]["message"].as_str().unwrap().is_empty());
        }
    }
    let records = shared.call_log.lock().unwrap().snapshot();
    assert_eq!(records.len(), cases.len() * 2);
    assert!(records.iter().all(|record| !record.ok && !record.error.contains("早返回")));
}

/// 19 个工具都要有**有效**的 outputSchema:不能宽到只写 {},也不能严到拒绝合法响应。
/// 用 tools/list 里真实下发的 schema 校验**真实调用结果**——
/// 只做文本快照的话,schema 和实际响应漂移不会被发现。
#[tokio::test]
async fn every_tool_result_validates_against_its_output_schema() {
    let port = spawn_test_endpoint().await;

    // 先取 tools/list 里的 outputSchema
    let list = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(modern_body(1, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(list.status, 200, "tools/list 应成功: {}", list.body_text());
    let tools = list.result().expect("应有 result")["tools"].as_array().cloned().unwrap();
    assert_eq!(tools.len(), 19, "工具数应仍是 19");

    let mut schemas: HashMap<String, Value> = HashMap::new();
    for tool in &tools {
        let name = tool["name"].as_str().unwrap().to_string();
        let schema = tool.get("outputSchema").cloned().unwrap_or(Value::Null);
        assert!(schema.is_object(), "{} 缺 outputSchema", name);
        assert_eq!(schema["type"], "object", "{} 的 outputSchema 应声明 object", name);
        assert!(
            schema["properties"].get("ok").is_some(),
            "{} 的 outputSchema 应描述 ok 字段",
            name
        );
        assert!(
            schema["required"].as_array().is_some_and(|r| r.iter().any(|v| v == "ok")),
            "{} 的 outputSchema 应要求 ok 必填",
            name
        );
        assert_ne!(schema, json!({}), "{} 的 outputSchema 不能是空对象", name);
        schemas.insert(name, schema);
    }

    // 再逐个调工具,用真实响应校验自己的 schema
    for (i, tool) in EXPECTED_TOOL_ORDER.iter().enumerate() {
        let id = 2000 + i as u32;
        let resp = send_raw(
            port,
            &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/call", Some(tool)))
                .body(modern_body(
                    id,
                    "tools/call",
                    json!({"name": tool, "arguments": call_args_for(tool)}),
                )),
        )
        .await;
        assert_eq!(resp.status, 200, "{} 调用应成功: {}", tool, resp.body_text());
        let result = resp.result().expect("应有 result");
        let instance = &result["structuredContent"];
        let schema = &schemas[*tool];
        validate_schema(schema, instance, schema, "$")
            .unwrap_or_else(|e| panic!("{} 的真实结果不符合自己的 outputSchema: {}", tool, e));
    }
}

#[tokio::test]
async fn business_error_results_validate_against_the_tool_schema() {
    let port = spawn_test_endpoint().await;
    let list = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(modern_body(1, "tools/list", json!({}))),
    )
    .await;
    let tools = list.result().unwrap()["tools"].as_array().cloned().unwrap();
    let schema = tools
        .iter()
        .find(|t| t["name"] == "send")
        .unwrap()["outputSchema"]
        .clone();

    let resp = send_raw(port, &RawRequest::new("POST", "/mcp")
        .with_headers(modern_headers("tools/call", Some("send")))
        .body(modern_body(2, "tools/call", json!({
            "name": "send", "arguments": {"port": "COM-not-open", "text": "AT"}
        })))).await;
    let result = resp.result().expect("应有 result");
    assert_eq!(result["isError"], true);
    validate_schema(&schema, &result["structuredContent"], &schema, "$")
        .unwrap_or_else(|error| panic!("business error violates outputSchema: {error}"));
}

// ============ 工具注解(2026-07-28 annotations) ============

/// 19 个工具的期望注解表:(名称, readOnly, destructive, idempotent, openWorld)。
/// 与 server.rs 的 `annotations()` 一一对应——改注解必须显式改这里,表本身就是审查对象。
const EXPECTED_ANNOTATIONS: [(&str, bool, bool, bool, bool); 19] = [
    // 名称与顺序同 EXPECTED_TOOL_ORDER
    ("list_ports", true, false, true, false),
    ("connect", false, false, true, true),
    ("disconnect", false, false, true, true),
    ("get_status", true, false, true, true),
    ("send", false, true, false, true),
    ("send_and_read", false, true, false, true),
    ("get_history_since", true, false, true, false),
    ("reset_stats", false, false, true, false),
    ("send_file", false, true, false, true),
    ("start_logging", false, true, false, false),
    ("stop_logging", false, false, true, false),
    ("sequence_run", false, true, false, true),
    ("sequence_stop", false, false, true, true),
    ("get_sequence_status", true, false, true, false),
    ("get_settings", true, false, true, false),
    ("save_settings", false, true, true, false),
    ("kb_search", true, false, true, false),
    ("kb_get", true, false, true, false),
    ("kb_manuals", true, false, true, false),
];

/// 19 个工具都有注解,序列化名是 MCP 规范的 readOnlyHint/destructiveHint/
/// idempotentHint/openWorldHint,四个 hint 与注解表逐项一致;
/// 加注解不挤掉 tools/list 的其他字段(顺序/描述/schema/缓存字段)。
#[test]
fn tool_annotations_match_the_reviewed_table_with_spec_names() {
    let value = serde_json::to_value(tool_list_response(build_tools())).unwrap();
    assert_eq!(value["ttlMs"], 0, "缓存字段不受注解影响");
    assert_eq!(value["cacheScope"], "private");
    let tools = value["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 19, "工具数应仍是 19");

    for (tool, expected) in tools.iter().zip(EXPECTED_ANNOTATIONS.iter()) {
        let name = tool["name"].as_str().unwrap();
        assert_eq!(name, expected.0, "注解表与工具顺序错位");
        let ann = &tool["annotations"];
        assert!(ann.is_object(), "{} 缺 annotations: {}", name, tool);
        for key in ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"] {
            assert!(ann[key].is_boolean(), "{} 的 {} 应为布尔: {}", name, key, ann);
        }
        assert_eq!(ann["readOnlyHint"], expected.1, "{} readOnlyHint 不符", name);
        assert_eq!(ann["destructiveHint"], expected.2, "{} destructiveHint 不符", name);
        assert_eq!(ann["idempotentHint"], expected.3, "{} idempotentHint 不符", name);
        assert_eq!(ann["openWorldHint"], expected.4, "{} openWorldHint 不符", name);
        // 注解是附加信息,原有字段必须原样保留
        assert!(tool["description"].is_string(), "{} 缺描述", name);
        assert!(tool["inputSchema"].is_object(), "{} 缺 inputSchema", name);
        assert!(tool["outputSchema"].is_object(), "{} 缺 outputSchema", name);
    }
}

/// 注解不变量(独立于注解表逐项值,表达审查基线):
/// 只读工具不许标 destructive 或非幂等;发送类工具不许标只读或幂等。
#[test]
fn annotation_invariants_hold_across_the_tool_set() {
    let send_class = ["send", "send_and_read", "send_file", "sequence_run", "start_logging"];
    for (name, read_only, destructive, idempotent, _open_world) in EXPECTED_ANNOTATIONS {
        if read_only {
            assert!(!destructive, "{} 是只读工具,不能标 destructive", name);
            assert!(idempotent, "{} 是只读工具,必须幂等", name);
        }
        if send_class.contains(&name) {
            assert!(!read_only, "{} 是发送类工具,不能标只读", name);
            assert!(!idempotent, "{} 是发送类工具,不能标幂等", name);
            assert!(destructive, "{} 是发送类工具,应保守标 destructive", name);
        }
    }
}

// ============ 黑盒:取消一个在途的工具调用 ============

/// 客户端发 notifications/cancelled → rmcp cancel 该请求的令牌 →
/// send_and_read 停止等待(不再等满 timeout_ms)。
///
/// 可观察到的黑盒事实:
/// - 调用**很快**结束(远早于 timeout_ms=10000)——工具真的停了,没有后台幽灵任务;
/// - **没有** JSON-RPC 响应——MCP 规定被取消的请求不再回响应,rmcp 因此丢弃了它;
/// - 调用日志留下一条记录,ok=false,错误说明指路 `get_history_since` 续读而不是重发。
#[tokio::test]
async fn cancelling_an_in_flight_call_stops_it_and_is_logged_as_cancelled() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-live");
    shared.connections.lock().unwrap().insert("COM-live".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;

    let session = legacy_initialize(port).await;

    // 发起一个会等很久的调用(timeout_ms=10000)
    let session_for_call = session.clone();
    let call = tokio::spawn(async move {
        legacy_call(
            port,
            &session_for_call,
            42,
            "send_and_read",
            json!({"port": "COM-live", "text": "AT", "timeout_ms": 10000}),
        )
        .await
    });
    // 等它真的进到工具里
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // 取消它
    let cancel = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .header("Mcp-Session-Id", &session)
            .body(json!({
                "jsonrpc": "2.0", "method": "notifications/cancelled",
                "params": {"requestId": 42, "reason": "user aborted"}
            })),
    )
    .await;
    assert!(cancel.status < 300, "取消通知应被接受: {}", cancel.status);

    // 调用应很快结束(不该等满 timeout_ms=10000)
    let started = std::time::Instant::now();
    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), call)
        .await
        .expect("取消后调用应很快结束,不该等满 timeout_ms")
        .expect("任务不该 panic");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "取消后应立即收尾,实际耗时 {:?}",
        started.elapsed()
    );
    // 被取消的请求没有 JSON-RPC 响应(MCP 规定)
    assert!(
        resp.jsonrpc().is_none(),
        "被取消的请求不该有 JSON-RPC 响应: {}",
        resp.body_text()
    );

    // 调用记录:一条,标记为失败(不是成功),并指路续读
    let log = shared.call_log.lock().unwrap().snapshot();
    let records: Vec<_> = log.iter().filter(|r| r.tool == "send_and_read").collect();
    assert_eq!(records.len(), 1, "取消只应留一条调用记录,实际 {}", records.len());
    assert!(!records[0].ok, "取消的调用不能记成成功");
    assert!(
        records[0].error.contains("get_history_since"),
        "调用记录的错误说明要指路续读: {}",
        records[0].error
    );
}

// ============ 黑盒:旧版 session 路径 ============

#[tokio::test]
async fn legacy_initialize_issues_session_and_tools_list_works() {
    let port = spawn_test_endpoint().await;
    let init = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .body(json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": PROTOCOL_2025,
                    "capabilities": {},
                    "clientInfo": {"name": "legacy-tests", "version": "0"}
                }
            })),
    )
    .await;
    assert_eq!(init.status, 200, "initialize 应成功: {}", init.body_text());
    let session = init
        .header("mcp-session-id")
        .expect("旧协议 initialize 必须签发 Mcp-Session-Id")
        .to_string();
    assert!(!session.is_empty());

    // notifications/initialized
    let notif = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .header("Mcp-Session-Id", &session)
            .body(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
    )
    .await;
    assert!(notif.status < 300, "notifications/initialized 应被接受: {}", notif.status);

    // 带 session 的 tools/list
    let list = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .header("Mcp-Session-Id", &session)
            .body(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})),
    )
    .await;
    assert_eq!(list.status, 200, "带 session 的 tools/list 应成功: {}", list.body_text());
    let result = list.result().expect("tools/list 应有 result");
    assert_eq!(result["tools"].as_array().unwrap().len(), 19);

    // 删掉 session 后,新版无状态请求仍然正常
    let del = send_raw(
        port,
        &RawRequest::new("DELETE", "/mcp")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .header("Mcp-Session-Id", &session),
    )
    .await;
    assert!(del.status < 300, "DELETE session 应被接受: {}", del.status);

    let after = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(modern_body(4, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(after.status, 200, "删掉旧 session 不应影响新版请求: {}", after.body_text());
}

// ============ 黑盒:过载保护对旧协议路径同样生效 ============

/// 旧 session 路径的工具执行同样受并发约束。
///
/// 机制:旧版 POST 会先把 SSE 响应流交回去、工具在后台跑(实测:32 个慢调用
/// 在途时 tools/list 仍返回 200),HTTP 中间件的 permit 盖不住工具执行段——
/// 所以工具执行层有独立的 `McpShared.tool_gate`,与中间件共用 32 的额度。
/// 占满后,第 33 个**普通**工具调用立刻拿到"服务忙"结果而不是无限排队;
/// 恢复/诊断类工具(get_status 等)走专用配额,主闸占满时仍可执行;
/// 新版请求的 HTTP 层不受影响;额度释放后恢复。
#[tokio::test]
async fn legacy_tool_calls_are_bounded_by_the_shared_tool_gate() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared).await;
    let session = legacy_initialize(port).await;

    // 占满 32 个工具执行额度:旧协议的长 send_and_read
    let mut calls = Vec::new();
    for i in 0..crate::mcp::server::MCP_MAX_CONCURRENT_REQUESTS as u32 {
        let session_for_call = session.clone();
        calls.push(tokio::spawn(async move {
            legacy_call(
                port,
                &session_for_call,
                100 + i,
                "send_and_read",
                json!({"port": "COM-slow", "text": "AT", "timeout_ms": 3000}),
            )
            .await
        }));
    }
    // 轮询等工具闸占满:第 33 个普通工具调用应立刻收到"服务忙"结果。
    // 探测工具用 get_history_since(普通配额);get_status 属于专用配额,不能用来探测。
    let mut busy = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let resp = legacy_call(
            port,
            &session,
            999,
            "get_history_since",
            json!({"port": "COM-slow", "seq": 0}),
        )
        .await;
        let Some(result) = resp.result() else { continue };
        if result["isError"] != true {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap_or("")) {
            if v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("服务忙")) {
                assert!(
                    v["error"].as_str().unwrap().contains("未执行"),
                    "拒绝说明应写明调用未执行: {}",
                    v
                );
                busy = Some(v);
                break;
            }
        }
    }
    let busy = busy.expect("工具闸占满后,第 33 个普通工具调用应立刻收到服务忙结果");
    assert!(
        busy["error"].as_str().unwrap().contains("32"),
        "错误说明应写明上限: {}",
        busy
    );

    // 专用配额:主闸占满时,恢复/诊断类工具(get_status)仍照常执行
    let resp = legacy_call(port, &session, 996, "get_status", json!({"port": "COM-slow"})).await;
    assert_eq!(resp.status, 200, "诊断工具不应被主闸拒绝: {}", resp.body_text());
    let result = resp.result().expect("应有 result");
    let v: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["ok"], true, "主闸占满时 get_status 应照常执行: {}", v);

    // 工具闸占满不影响新版请求的 HTTP 层(tools/list 不占工具额度)
    let modern = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/list", None))
            .body(modern_body(997, "tools/list", json!({}))),
    )
    .await;
    assert_eq!(modern.status, 200, "工具闸占满不该影响新版 HTTP 层: {}", modern.body_text());

    // 慢调用结束后额度释放,旧协议调用恢复执行
    for c in calls {
        let _ = c.await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let resp = legacy_call(port, &session, 998, "get_status", json!({"port": "COM-slow"})).await;
    assert_eq!(resp.status, 200, "额度释放后调用应成功: {}", resp.body_text());
    let v: Value =
        serde_json::from_str(resp.result().expect("应有 result")["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["ok"], true, "额度释放后应恢复正常: {}", v);
}

// ============ 黑盒:send_file 通过 HTTP 取消 ============

/// 客户端取消在途的 send_file:工具停止后续写出,调用日志如实记为取消
/// (MCP 规定被取消的请求不回响应,所以可观察证据在调用日志里)。
/// 队列无消费者时 send_file 卡在入队等待,取消正好落在这个等待点上。
#[tokio::test]
async fn cancelling_send_file_over_http_stops_it_and_is_logged() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-file-http");
    shared.connections.lock().unwrap().insert("COM-file-http".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;
    let session = legacy_initialize(port).await;

    // 大文件:512 块,写队列只有 256 格且无消费者,入队必然堵住 → 取消落在等待中
    let path = std::env::temp_dir().join(format!("neoserial_http_cancel_{}", std::process::id()));
    std::fs::write(&path, vec![0xA5u8; 512 * 1024]).unwrap();
    let before_log = shared.call_log.lock().unwrap().snapshot().len();

    let session_for_call = session.clone();
    let path_str = path.to_str().unwrap().to_string();
    let call = tokio::spawn(async move {
        legacy_call(
            port,
            &session_for_call,
            42,
            "send_file",
            json!({"port": "COM-file-http", "path": path_str}),
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let cancel = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL_2025)
            .header("Mcp-Session-Id", &session)
            .body(json!({
                "jsonrpc": "2.0", "method": "notifications/cancelled",
                "params": {"requestId": 42, "reason": "user aborted"}
            })),
    )
    .await;
    assert!(cancel.status < 300, "取消通知应被接受: {}", cancel.status);

    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), call)
        .await
        .expect("取消后调用应很快结束")
        .expect("任务不该 panic");
    assert!(resp.jsonrpc().is_none(), "被取消的请求不该有 JSON-RPC 响应");

    // 工具在 spawn_blocking 里收尾:rmcp 响应取消时它可能还没返回,轮询日志最多 3s
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let record = loop {
        let log = shared.call_log.lock().unwrap().snapshot();
        let found = log
            .iter()
            .skip(before_log)
            .find(|r| r.tool == "send_file")
            .cloned();
        if let Some(r) = found {
            break r;
        }
        assert!(std::time::Instant::now() < deadline, "取消后 3s 内应出现调用记录");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_eq!(
        shared
            .call_log
            .lock()
            .unwrap()
            .snapshot()
            .iter()
            .filter(|r| r.tool == "send_file")
            .count(),
        1,
        "取消只应留一条调用记录"
    );
    assert!(!record.ok, "取消的发送不能记成成功");
    assert!(
        record.error.contains("不要自动重发"),
        "调用记录要写明不能自动重发: {}",
        record.error
    );
    let _ = std::fs::remove_file(&path);
}

/// 现代客户端不发取消通知、直接断开 SSE 流:rmcp 检测到连接断开 → cancel
/// 请求令牌 → send_and_read 停止等待。可观察证据:调用日志在断开后很快留下
/// 一条 ok=false 的取消记录,错误说明指路 get_history_since 续读。
#[tokio::test]
async fn dropping_the_modern_stream_cancels_the_in_flight_call() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-drop");
    shared.connections.lock().unwrap().insert("COM-drop".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;
    let before_log = shared.call_log.lock().unwrap().snapshot().len();

    // 发起慢调用,读到响应头(连接已建立)后直接断开 socket,不发任何取消通知
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.expect("连接测试 endpoint");
    let body = serde_json::to_vec(&modern_body(7, "tools/call", json!({
        "name": "send_and_read",
        "arguments": {"port": "COM-drop", "text": "AT", "timeout_ms": 8000}
    })))
    .unwrap();
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nMcp-Method: tools/call\r\n\
         MCP-Protocol-Version: {}\r\nMcp-Name: send_and_read\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        port,
        PROTOCOL_2026,
        body.len()
    );
    stream.write_all(head.as_bytes()).await.expect("写请求头");
    stream.write_all(&body).await.expect("写请求体");
    stream.flush().await.expect("flush");
    let mut buf = [0u8; 256];
    let _ = tokio::time::timeout(std::time::Duration::from_millis(400), stream.read(&mut buf)).await;
    let activity_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let activity_id = loop {
        let snapshot = shared.call_log.lock().unwrap().activity.snapshot();
        if let Some(call) = snapshot.active.iter().find(|call| call.port.as_deref() == Some("COM-drop")) {
            assert_eq!(call.tool, "send_and_read");
            break call.id;
        }
        assert!(std::time::Instant::now() < activity_deadline, "请求执行时应发布活动状态");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    let dropped_at = std::time::Instant::now();
    drop(stream);

    // 断开后短时间内应出现取消记录(工具没有继续等满 timeout_ms=8000)
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let log = shared.call_log.lock().unwrap().snapshot();
        let found = log
            .iter()
            .skip(before_log)
            .find(|r| r.tool == "send_and_read")
            .cloned();
        if let Some(r) = found {
            assert!(
                dropped_at.elapsed() < std::time::Duration::from_secs(5),
                "记录出现得太晚,工具像是等满了超时"
            );
            assert!(!r.ok, "断开连接的调用应记为取消/失败: {}", r.error);
            assert!(
                r.error.contains("get_history_since"),
                "错误说明要指路续读: {}",
                r.error
            );
            break;
        }
        assert!(std::time::Instant::now() < deadline, "断开后 5s 内应出现取消记录(工具没有停)");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let activity = shared.call_log.lock().unwrap().activity.snapshot();
    assert!(!activity.active.iter().any(|call| call.id == activity_id));
    assert_eq!(activity.recent[0].port.as_deref(), Some("COM-drop"));
    assert_eq!(activity.recent[0].ok, Some(false));
}

// ============ outputSchema:成功形态校验 ============
//
// 黑盒调用用的是不存在的端口/无 GUI 上下文,大多触发**错误形态**;
// 成功形态(尤其带完整可选字段的)在这里用不碰硬件的单元级响应补上,
// 防止 schema 从来没被成功响应验证过。
// connect/disconnect/日志/设置类成功形态需要 GUI 上下文,黑盒与单元层都不可达,
// 靠人工对照 struct 字段审查(见交付记录)。

#[tokio::test]
async fn success_shapes_validate_against_output_schema() {
    use super::server::output_schema;
    use super::tools;
    use crate::buffer::log_line::{Dir, LogLine};

    let validate = |tool: &str, instance: &Value| {
        let schema = output_schema(tool);
        validate_schema(&schema, instance, &schema, "$")
            .unwrap_or_else(|e| panic!("{} 的成功形态不符合 outputSchema: {} ({})", tool, e, instance));
    };

    let shared = test_shared();

    // list_ports:真实枚举(空列表也是合法成功形态)
    let r = serde_json::to_value(tools::list_ports(&shared)).unwrap();
    validate("list_ports", &r);

    // get_status:connected=true 的成功形态(假连接,不碰硬件)
    let (handle, rx) = fake_connection("COM-ok");
    let history = handle.rx_history.clone();
    shared.connections.lock().unwrap().insert("COM-ok".to_string(), handle);
    let r = serde_json::to_value(tools::get_status(
        &shared,
        tools::StatusReq { port: "COM-ok".into() },
    ))
    .unwrap();
    assert_eq!(r["connected"], true, "前置:应看到已连接的假连接");
    validate("get_status", &r);

    // get_history_since:有历史行的成功形态
    history.push(Dir::Rx, LogLine::new("08:00:00.000".into(), Dir::Rx, b"OK-1".to_vec(), &[], 1));
    history.push(Dir::Tx, LogLine::new("08:00:00.001".into(), Dir::Tx, b"AT".to_vec(), &[], 2));
    let r = serde_json::to_value(tools::get_history_since(
        &shared,
        tools::GetHistorySinceReq { port: "COM-ok".into(), seq: 0, max_lines: None, is_hex: None },
    ))
    .unwrap();
    assert_eq!(r["lines"].as_array().unwrap().len(), 2, "前置:应有两行历史");
    validate("get_history_since", &r);

    // send:成功形态
    let r = serde_json::to_value(tools::send(
        &shared,
        tools::SendReq { port: "COM-ok".into(), text: "AT".into(), ending: None, is_hex: None },
    )
    .unwrap())
    .unwrap();
    assert_eq!(r["ok"], true);
    validate("send", &r);

    // send_and_read:静默超时的成功形态
    let r = serde_json::to_value(
        tools::send_and_read(
            &shared,
            tools::SendAndReadReq {
                port: "COM-ok".into(),
                text: "AT".into(),
                timeout_ms: 80,
                ending: None,
                is_hex: None,
            },
            None,
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(r["timed_out"], true);
    validate("send_and_read", &r);

    // reset_stats:成功形态
    let r = serde_json::to_value(tools::reset_stats(
        &shared,
        tools::ResetStatsReq { port: "COM-ok".into() },
    ))
    .unwrap();
    assert_eq!(r["ok"], true);
    validate("reset_stats", &r);

    // get_sequence_status:空闲形态(全局状态,未运行过的端口)
    let r = serde_json::to_value(tools::sequence_status(tools::SequenceStatusReq {
        port: "COM-idle".into(),
    }))
    .unwrap();
    validate("get_sequence_status", &r);

    // send_file:成功形态(补一个把块记完的假写线程)
    let p = std::env::temp_dir().join(format!("neoserial_schema_ok_{}", std::process::id()));
    std::fs::write(&p, vec![0xA5u8; 2048]).unwrap();
    let writer = std::thread::spawn(move || {
        let mut chunks = 0;
        while let Ok(cmd) = rx.recv() {
            if let crate::connection::WriteCommand::FileChunk(data, tracker) = cmd {
                tracker.record(data.len(), None);
                chunks += 1;
                if chunks >= 2 {
                    break;
                }
            }
        }
    });
    let r = serde_json::to_value(
        tools::send_file(
            &shared,
            tools::SendFileReq { port: "COM-ok".into(), path: p.to_str().unwrap().into() },
            None,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(r["sent"], 2048);
    validate("send_file", &r);
    writer.join().ok();
    let _ = std::fs::remove_file(&p);

    // send_file:取消形态(预先取消,零写出)
    let p = std::env::temp_dir().join(format!("neoserial_schema_cancel_{}", std::process::id()));
    std::fs::write(&p, vec![0xA5u8; 4096]).unwrap();
    let ct = tokio_util::sync::CancellationToken::new();
    ct.cancel();
    let r = serde_json::to_value(
        tools::send_file(
            &shared,
            tools::SendFileReq { port: "COM-ok".into(), path: p.to_str().unwrap().into() },
            Some(&ct),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(r["cancelled"], true);
    assert_eq!(r["ok"], false);
    validate("send_file", &r);
    let _ = std::fs::remove_file(&p);

    // GUI 依赖类工具的成功形态:直接构造响应 struct(serde 派生)对照 schema。
    // 运行时路径需要 AppHandle,黑盒与运行时都不可达;这里锁住的是
    // "struct 字段与 schema 不漂移"——字段改名/改类型会在这里被测出。
    let connect = tools::ConnectResp {
        ok: true,
        mode: Some("opened".into()),
        reused: false,
        baud: 115200,
        owner: None,
    };
    validate("connect", &serde_json::to_value(connect).unwrap());
    let connect_reused = tools::ConnectResp {
        ok: true,
        mode: Some("reused".into()),
        reused: true,
        baud: 9600,
        owner: Some("gui".into()),
    };
    validate("connect", &serde_json::to_value(connect_reused).unwrap());
    // disconnect 的成功结果由 server.rs 内联构造,恒为 {ok:true} 或 {ok:false,error}
    validate("disconnect", &json!({ "ok": true }));
    validate(
        "start_logging",
        &serde_json::to_value(tools::StartLoggingResp {
            ok: true,
            path: "C:\\log\\neoserial.txt".into(),
        })
        .unwrap(),
    );
    validate(
        "stop_logging",
        &serde_json::to_value(tools::StopLoggingResp { ok: true }).unwrap(),
    );
    validate(
        "save_settings",
        &serde_json::to_value(tools::SaveSettingsResp { ok: true }).unwrap(),
    );
    validate(
        "get_settings",
        &serde_json::to_value(tools::GetSettingsResp { ok: true, settings: None }).unwrap(),
    );
}

/// 跨协议过载语义:旧协议调用占满工具主闸后,**现代客户端**发起的普通工具
/// 调用同样立刻收到 isError=true 的"服务忙"结果(HTTP 200,不是 429——
/// 工具闸在 HTTP 层之内,只有 HTTP 入口满才 429),专用配额工具照常可用。
#[tokio::test]
async fn modern_calls_see_is_error_when_the_tool_gate_is_full() {
    let shared = test_shared();
    let (handle, _rx) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared).await;
    let session = legacy_initialize(port).await;

    // 占满 32 个主闸额度(旧协议长调用)
    let mut calls = Vec::new();
    for i in 0..crate::mcp::server::MCP_MAX_CONCURRENT_REQUESTS as u32 {
        let session_for_call = session.clone();
        calls.push(tokio::spawn(async move {
            legacy_call(
                port,
                &session_for_call,
                100 + i,
                "send_and_read",
                json!({"port": "COM-slow", "text": "AT", "timeout_ms": 3000}),
            )
            .await
        }));
    }

    // 现代客户端的普通工具调用:HTTP 200 + isError=true + 服务忙
    let mut busy = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let resp = send_raw(
            port,
            &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/call", Some("get_history_since")))
                .body(modern_body(
                    300,
                    "tools/call",
                    json!({"name": "get_history_since", "arguments": {"port": "COM-slow", "seq": 0}}),
                )),
        )
        .await;
        let Some(result) = resp.result() else { continue };
        if result["isError"] != true {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap_or("")) {
            if v["ok"] == false && v["error"].as_str().is_some_and(|e| e.contains("服务忙")) {
                busy = Some(v);
                break;
            }
        }
    }
    busy.expect("主闸占满后,现代客户端也应立刻收到 isError=true 的服务忙结果");

    // 专用配额对新协议同样生效:现代客户端的 get_status 照常执行
    let resp = send_raw(
        port,
        &RawRequest::new("POST", "/mcp")
            .with_headers(modern_headers("tools/call", Some("get_status")))
            .body(modern_body(
                301,
                "tools/call",
                json!({"name": "get_status", "arguments": {"port": "COM-slow"}}),
            )),
    )
    .await;
    assert_eq!(resp.status, 200, "专用配额工具应照常执行: {}", resp.body_text());
    let result = resp.result().expect("应有 result");
    let v: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["ok"], true, "主闸占满时 get_status 应照常执行: {}", v);

    for c in calls {
        let _ = c.await;
    }
}

async fn saturate_http_calls(
    port: u16,
    shared: &McpShared,
) -> Vec<tokio::task::JoinHandle<RawResponse>> {
    let calls = (0..super::server::MCP_MAX_CONCURRENT_REQUESTS)
        .map(|index| tokio::spawn(async move {
            send_raw(port, &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/call", Some("send_and_read")))
                .body(modern_body(1000 + index as u32, "tools/call", json!({
                    "name": "send_and_read",
                    "arguments": {"port": "COM-slow", "text": "AT", "timeout_ms": 60000}
                })))).await
        }))
        .collect();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while shared.tool_gate.available_permits() != 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("slow calls should occupy all normal HTTP slots");
    calls
}

async fn abort_http_calls(calls: Vec<tokio::task::JoinHandle<RawResponse>>) {
    for call in &calls {
        call.abort();
    }
    for call in calls {
        let _ = call.await;
    }
}

#[tokio::test]
async fn http_saturation_keeps_modern_and_legacy_recovery_tools_available() {
    let shared = test_shared();
    let (handle, _receiver) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;
    let session = legacy_initialize(port).await;
    let calls = saturate_http_calls(port, &shared).await;

    let ordinary = send_raw(port, &RawRequest::new("POST", "/mcp")
        .with_headers(modern_headers("tools/call", Some("get_history_since")))
        .body(modern_body(2000, "tools/call", json!({
            "name": "get_history_since", "arguments": {"port": "COM-slow", "seq": 0}
        })))).await;
    assert_eq!(ordinary.status, 429);
    assert_eq!(ordinary.header("retry-after"), Some("1"));

    for legacy in [false, true] {
        for (index, tool) in ["list_ports", "get_status", "get_sequence_status", "disconnect", "sequence_stop"]
            .into_iter().enumerate()
        {
            let args = if tool == "get_status" { json!({"port": "COM-slow"}) } else { call_args_for(tool) };
            let response = if legacy {
                legacy_call(port, &session, 2100 + index as u32, tool, args).await
            } else {
                send_raw(port, &RawRequest::new("POST", "/mcp")
                    .with_headers(modern_headers("tools/call", Some(tool)))
                    .body(modern_body(2100 + index as u32, "tools/call", json!({"name": tool, "arguments": args})))).await
            };
            assert_eq!(response.status, 200, "legacy={legacy}, tool={tool}: {}", response.body_text());
            let result = response.result().expect("recovery tools should reach execution");
            assert!(!result["structuredContent"]["error"].as_str().unwrap_or("").contains("服务忙"));
            if tool == "get_status" {
                assert_eq!(result["isError"], false);
                assert_eq!(result["structuredContent"]["connected"], true);
            }
        }
    }
    abort_http_calls(calls).await;
}

#[tokio::test]
async fn http_recovery_reserve_does_not_trust_headers_or_bypass_security() {
    let shared = test_shared();
    let (handle, _receiver) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;
    let calls = saturate_http_calls(port, &shared).await;

    let forged = send_raw(port, &RawRequest::new("POST", "/mcp")
        .with_headers(modern_headers("tools/call", Some("get_status")))
        .body(modern_body(3000, "tools/call", json!({
            "name": "send_and_read", "arguments": {"port": "COM-slow", "text": "AT", "timeout_ms": 60000}
        })))).await;
    assert_eq!(forged.status, 429);
    assert!(shared.call_log.lock().unwrap().snapshot().is_empty());

    let mismatch = send_raw(port, &RawRequest::new("POST", "/mcp")
        .with_headers(modern_headers("tools/call", Some("disconnect")))
        .body(modern_body(3001, "tools/call", json!({"name": "get_status", "arguments": {"port": "COM-slow"}})))).await;
    assert_eq!(mismatch.status, 400);
    assert_eq!(mismatch.jsonrpc().unwrap()["error"]["code"], -32020);

    let origin = send_raw(port, &RawRequest::new("POST", "/mcp")
        .with_headers(modern_headers("tools/call", Some("get_status")))
        .header("Origin", "https://evil.example")
        .body(modern_body(3002, "tools/call", json!({"name": "get_status", "arguments": {"port": "COM-slow"}})))).await;
    assert_eq!(origin.status, 403);

    let host = send_raw(port, &RawRequest::new("POST", "/mcp")
        .with_headers(modern_headers("tools/call", Some("get_status")))
        .host("evil.example")
        .body(modern_body(3003, "tools/call", json!({"name": "get_status", "arguments": {"port": "COM-slow"}})))).await;
    assert_eq!(host.status, 403);
    assert!(shared.call_log.lock().unwrap().snapshot().is_empty());
    abort_http_calls(calls).await;
}

#[tokio::test]
async fn http_recovery_reserve_is_bounded_and_releases_abandoned_probes() {
    let shared = test_shared();
    let (handle, _receiver) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;
    let calls = saturate_http_calls(port, &shared).await;
    let mut stalled = Vec::new();
    for _ in 0..super::server::MCP_PRIORITY_TOOL_CONCURRENCY {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream.write_all(format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMcp-Method: tools/call\r\nMcp-Name: get_status\r\nMCP-Protocol-Version: {PROTOCOL_2026}\r\nContent-Length: 1024\r\n\r\n"
        ).as_bytes()).await.unwrap();
        stalled.push(stream);
    }
    let probe = RawRequest::new("POST", "/mcp")
        .with_headers(modern_headers("tools/call", Some("get_status")))
        .body(modern_body(4000, "tools/call", json!({"name": "get_status", "arguments": {"port": "COM-slow"}})));
    tokio::time::timeout(std::time::Duration::from_millis(750), async {
        loop {
            let response = send_raw(port, &probe).await;
            if response.status == 429 {
                assert_eq!(response.header("retry-after"), Some("1"));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("the recovery reserve must have a finite capacity");
    drop(stalled);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if send_raw(port, &probe).await.status == 200 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("abandoned probes should release their recovery slots");
    abort_http_calls(calls).await;
}

#[tokio::test]
async fn http_recovery_reserve_limits_request_body_size() {
    let shared = test_shared();
    let (handle, _receiver) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;
    let calls = saturate_http_calls(port, &shared).await;
    let response = send_raw_chunked(port, &RawRequest::new("POST", "/mcp")
        .with_headers(modern_headers("tools/call", Some("get_status")))
        .body(modern_body(5000, "tools/call", json!({
            "name": "get_status", "arguments": {"port": "COM-slow"}, "padding": "x".repeat(4 * 1024 * 1024)
        })))).await;
    assert_eq!(response.status, 413);
    assert!(shared.call_log.lock().unwrap().snapshot().is_empty());
    abort_http_calls(calls).await;
}

#[tokio::test]
async fn http_recovery_reserve_times_out_stalled_request_bodies() {
    let shared = test_shared();
    let (handle, _receiver) = fake_connection("COM-slow");
    shared.connections.lock().unwrap().insert("COM-slow".to_string(), handle);
    let port = spawn_test_endpoint_with(shared.clone()).await;
    let calls = saturate_http_calls(port, &shared).await;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMcp-Method: tools/call\r\nMcp-Name: get_status\r\nMCP-Protocol-Version: {PROTOCOL_2026}\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n"
    ).as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(3), stream.read_to_end(&mut response))
        .await.expect("stalled recovery probes must time out").unwrap();
    assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 408"));
    abort_http_calls(calls).await;
}

#[tokio::test]
async fn modern_and_legacy_business_failures_are_tool_errors() {
    let port = spawn_test_endpoint().await;
    let session = legacy_initialize(port).await;
    for legacy in [false, true] {
        let args = json!({"port": "COM-not-open", "text": "AT"});
        let response = if legacy {
            legacy_call(port, &session, 6000, "send", args).await
        } else {
            send_raw(port, &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/call", Some("send")))
                .body(modern_body(6000, "tools/call", json!({"name": "send", "arguments": args})))).await
        };
        assert_eq!(response.status, 200);
        let result = response.result().expect("execution failure should remain a tool result");
        assert_eq!(result["isError"], true, "legacy={legacy}: {result}");
        assert_eq!(result["structuredContent"]["ok"], false);
        let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["structuredContent"]);
    }
}

#[tokio::test]
async fn protocol_errors_take_precedence_over_tool_quota_rejection() {
    let shared = test_shared();
    let _permits = shared.tool_gate.clone().acquire_many_owned(
        super::server::MCP_MAX_CONCURRENT_REQUESTS as u32,
    ).await.unwrap();
    let port = spawn_test_endpoint_with(shared).await;
    let session = legacy_initialize(port).await;
    for legacy in [false, true] {
        for (index, (tool, args)) in [
            ("send", json!({})),
            ("send", json!({"port": "COM-not-open", "text": 123})),
            ("no_such_tool", json!({})),
        ].into_iter().enumerate() {
            let id = 7000 + index as u32;
            let response = if legacy {
                legacy_call(port, &session, id, tool, args).await
            } else {
                send_raw(port, &RawRequest::new("POST", "/mcp")
                    .with_headers(modern_headers("tools/call", Some(tool)))
                    .body(modern_body(id, "tools/call", json!({"name": tool, "arguments": args})))).await
            };
            let error = response.jsonrpc().expect("expected JSON-RPC response");
            assert_eq!(error["error"]["code"], -32602, "legacy={legacy}, tool={tool}: {error}");
            assert!(error.get("result").is_none());
        }
        let args = json!({"port": "COM-not-open", "text": "AT"});
        let response = if legacy {
            legacy_call(port, &session, 7100, "send", args).await
        } else {
            send_raw(port, &RawRequest::new("POST", "/mcp")
                .with_headers(modern_headers("tools/call", Some("send")))
                .body(modern_body(7100, "tools/call", json!({"name": "send", "arguments": args})))).await
        };
        let result = response.result().expect("valid calls should still obey the tool quota");
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"].as_str().unwrap().contains("未执行"));
    }
}
