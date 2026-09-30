use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use crate::state::AppState;

const PROTOCOL: &str = "2026-07-28";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Serialize)]
pub struct McpCheckResult {
    pub port: u16,
    pub server_name: String,
    pub protocol_version: String,
    pub tool_count: usize,
}

/// 只检测本进程实际绑定的 MCP 地址，不接收任意 URL，也不执行串口工具。
#[tauri::command]
pub async fn check_mcp_connection(state: State<'_, AppState>) -> Result<McpCheckResult, String> {
    let port = state.mcp_port.lock().map_err(|e| e.to_string())?
        .ok_or("MCP 服务未运行，请启用后重启应用")?;
    check_endpoint(port).await
}

async fn check_endpoint(port: u16) -> Result<McpCheckResult, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build().map_err(|e| format!("无法启动检测: {e}"))?;
    let discover = rpc(&client, port, "server/discover", 1).await?;
    let supported = discover["supportedVersions"].as_array()
        .is_some_and(|v| v.iter().any(|version| version.as_str() == Some(PROTOCOL)));
    if !supported {
        return Err(format!("服务未声明支持 {PROTOCOL}，请检查是否运行了旧版 NeoSerial"));
    }
    let name = discover["_meta"]["io.modelcontextprotocol/serverInfo"]["name"]
        .as_str().unwrap_or("未知服务");
    if name != "NeoSerial" {
        return Err(format!("该端口返回的服务为 {name}，并非 NeoSerial"));
    }
    let tool_list = rpc(&client, port, "tools/list", 2).await?;
    let count = validate_tools(&tool_list)?;
    Ok(McpCheckResult { port, server_name: name.into(), protocol_version: PROTOCOL.into(), tool_count: count })
}

async fn rpc(client: &reqwest::Client, port: u16, method: &str, id: u32) -> Result<Value, String> {
    let url = format!("http://127.0.0.1:{port}/mcp");
    let mut response = client.post(url)
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Method", method)
        .header("Mcp-Protocol-Version", PROTOCOL)
        .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"_meta":{
            "io.modelcontextprotocol/protocolVersion":PROTOCOL,
            "io.modelcontextprotocol/clientCapabilities":{},
            "io.modelcontextprotocol/clientInfo":{"name":"NeoSerial connection check","version":env!("CARGO_PKG_VERSION")}
        }}}))
        .send().await.map_err(|e| format!("{method} 连接失败或超时: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("{method} 返回 HTTP {}", response.status().as_u16()));
    }
    let sse = response.headers().get("content-type").and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("{method} 读取失败: {e}"))? {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(format!("{method} 响应超过 1 MiB，检测已停止"));
        }
        bytes.extend_from_slice(&chunk);
        if sse {
            if let Some(value) = sse_message(&bytes)? { return rpc_result(value, id, method); }
        }
    }
    if sse { return Err(format!("{method} 未返回完整 MCP 消息")); }
    let value = serde_json::from_slice(&bytes).map_err(|e| format!("{method} 返回无效 JSON: {e}"))?;
    rpc_result(value, id, method)
}

fn sse_message(bytes: &[u8]) -> Result<Option<Value>, String> {
    // chunk 可能截在 UTF-8 字符中间；等到完整 SSE 帧再解析。
    let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    let mut frames = text.split("\n\n").collect::<Vec<_>>();
    frames.pop(); // 未完成的末尾片段
    for frame in frames {
        let data = frame.lines().filter_map(|line| line.strip_prefix("data:"))
            .map(|line| line.strip_prefix(' ').unwrap_or(line)).collect::<Vec<_>>().join("\n");
        if data.is_empty() { continue; }
        let value: Value = serde_json::from_str(&data).map_err(|e| format!("无效 MCP 事件: {e}"))?;
        if value.get("id").is_some() { return Ok(Some(value)); }
    }
    Ok(None)
}

fn rpc_result(value: Value, id: u32, method: &str) -> Result<Value, String> {
    if value["jsonrpc"] != "2.0" || value["id"] != id {
        return Err(format!("{method} 返回了不匹配的 MCP 响应"));
    }
    if let Some(error) = value.get("error") {
        return Err(format!("{method} 失败: {}", error["message"].as_str().unwrap_or("未知协议错误")));
    }
    value.get("result").filter(|v| v.is_object()).cloned()
        .ok_or_else(|| format!("{method} 缺少 MCP result"))
}

fn validate_tools(value: &Value) -> Result<usize, String> {
    if value["ttlMs"].as_u64().is_none() ||
        !matches!(value["cacheScope"].as_str(), Some("private" | "public")) {
        return Err("工具列表缺少有效缓存字段，部分客户端无法读取，请更新 NeoSerial".into());
    }
    let tools = value["tools"].as_array().ok_or("服务没有返回工具列表")?;
    if tools.is_empty() { return Err("服务返回了空工具列表".into()); }
    for tool in tools {
        if tool["name"].as_str().is_none_or(str::is_empty) || !tool["inputSchema"].is_object() {
            return Err("工具列表包含无效的名称或参数定义".into());
        }
    }
    Ok(tools.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_rpc_and_reports_protocol_errors() {
        assert_eq!(rpc_result(json!({"jsonrpc":"2.0","id":2,"result":{"tools":[]}}),2,"tools/list").unwrap(),json!({"tools":[]}));
        assert!(rpc_result(json!({"jsonrpc":"2.0","id":3,"result":{}}),2,"tools/list").is_err());
        assert!(rpc_result(json!({"jsonrpc":"2.0","id":2,"error":{"message":"unsupported"}}),2,"tools/list").unwrap_err().contains("unsupported"));
        assert!(rpc_result(json!({"jsonrpc":"2.0","id":2}),2,"tools/list").is_err());
    }

    #[test]
    fn parses_complete_sse_frames_and_skips_comments_notifications() {
        let prefix = ": ping\r\n\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notice\"}\r\n\r\n";
        assert!(sse_message(prefix.as_bytes()).unwrap().is_none());
        let frame = format!("{prefix}event: message\r\ndata: {{\"jsonrpc\":\"2.0\",\"id\":1,\n data ignored\ndata: \"result\":{{}}}}\r\n\r\n");
        assert_eq!(sse_message(frame.as_bytes()).unwrap().unwrap()["id"],1);
        assert!(sse_message(b"data: {\"id\":1}").unwrap().is_none());
        assert!(sse_message(b"data: invalid\n\n").is_err());
    }

    #[test]
    fn rejects_missing_cache_fields_and_invalid_tools() {
        let tools = json!([{"name":"list_ports","inputSchema":{"type":"object"}}]);
        assert!(validate_tools(&json!({"tools":tools})).is_err());
        assert_eq!(validate_tools(&json!({"tools":tools,"ttlMs":0,"cacheScope":"private"})).unwrap(),1);
        assert!(validate_tools(&json!({"tools":[],"ttlMs":0,"cacheScope":"private"})).is_err());
        assert!(validate_tools(&json!({"tools":[{}],"ttlMs":0,"cacheScope":"private"})).is_err());
    }

    fn http_reply(status: &'static str, body: String) -> (u16, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut reader = std::io::BufReader::new(&stream);
            let mut content_length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" { break; }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut request_body = vec![0; content_length];
            reader.read_exact(&mut request_body).unwrap();
            let request: Value = serde_json::from_slice(&request_body).unwrap();
            assert_eq!(request["method"], "server/discover");
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = stream.write_all(response.as_bytes());
        });
        (port, task)
    }

    #[tokio::test]
    async fn network_check_reports_http_failure_instead_of_success() {
        let (port, task) = http_reply("503 Service Unavailable", "{}".into());
        assert!(check_endpoint(port).await.err().unwrap().contains("HTTP 503"));
        task.join().unwrap();
    }

    #[tokio::test]
    async fn network_check_rejects_redirects_and_oversized_responses() {
        let (port, task) = http_reply("302 Found", "{}".into());
        assert!(check_endpoint(port).await.err().unwrap().contains("HTTP 302"));
        task.join().unwrap();
        let (port, task) = http_reply("200 OK", "x".repeat(MAX_RESPONSE_BYTES + 1));
        assert!(check_endpoint(port).await.err().unwrap().contains("1 MiB"));
        task.join().unwrap();
    }
}
