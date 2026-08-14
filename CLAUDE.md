# CLAUDE.md — NeoSerial MCP 约定

## NeoSerial 是本机的串口中介
NeoSerial 内嵌 MCP server(streamable HTTP),Claude Code 经由它操作串口。
NeoSerial 单独持有串口,agent 不直接占端口。(代码仓库目录名仍为 cmserial,应用名是 neoserial/NeoSerial)

## 启动 NeoSerial
需要串口时,启动 NeoSerial(若已运行则跳过):
- 开发版:`cd S:/Mydevelop/cmserial && npm run tauri dev`
- 生产版(若已安装):启动 NeoSerial.exe

NeoSerial 启动后用配置端口(默认 23334)起 MCP server,并登记到 registry。

## agent 发现实例(按 COM 口)
操作某 COM 口前:
1. 读 registry:`%APPDATA%/neoserial/mcp-registry.json`,找 `com==目标COM && connected && heartbeat 新鲜(30s 内)` 的实例 → 连其 `port`。
2. 没有连目标 COM 的实例 → 找 `connected==false` 的空闲实例 → `connect(COM,...)`。
3. 没有空闲实例 → 启动一个新 NeoSerial,等其登记进 registry(轮询文件 1-2s),再连。
4. registry 缺失/全过期 → 从 23334 起逐端口 `get_status` 探测(兜底)。

## 配置 Claude Code 连接某实例
实例端口从 registry 拿到后(如 23334),配:
```
claude mcp add --transport http neoserial http://localhost:23334/mcp
```
或 .mcp.json:`{ "mcpServers": { "neoserial": { "type": "http", "url": "http://localhost:23334/mcp" } } }`

## 重连约定
disconnect 后立即 connect 同一端口,极少数情况下驱动未完全释放会报"端口被占用,可能上次连接未完全释放,请稍后重试"——稍等 0.5-1s 重试一次即可(disconnect 已持锁等线程退出,正常不会触发)。

## 工具
- list_ports / connect / disconnect / get_status
- send_and_read(text, timeout_ms, ending?, is_hex?) — 发后阻塞读,timeout_ms 是静默间隙超时(持续有数据会继续等);只返回模组响应(rx),自己发的 tx 不在 responses
- send(text, ending?, is_hex?) — 只发
- get_history_since(seq, max_lines?, is_hex?) — 拉取序号 > seq 的收发历史(tx+rx,带 dir);纯 hex 数据须传 is_hex=true 才可见(ascii 模式下显示空)

所有工具返回 { ok: bool, ... },ok=false 时 error 字段有中文原因。
