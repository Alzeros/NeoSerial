use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{State, Emitter};

use crate::state::AppState;
use crate::commands::connection::resolve_port;
use crate::commands::send::emit_tx_line;
use crate::connection::WriteCommand;
use crate::util::codec::{LineEnding, hex_to_bytes, ascii_to_bytes};

#[derive(Clone, Serialize)]
pub struct SequenceProgress {
    pub row: usize,
    pub total: usize,
}

#[derive(Clone, Serialize)]
pub struct SequenceDone {
    pub aborted: bool,
}

/// 正在运行的序列端口集合。per-port 隔离:同一 port 不可重复 run,
/// 不同 port 可并行。sequence_run 成功时 insert,线程退出(Drop guard)
/// 或 sequence_stop 时 remove。HashSet + Mutex 仅纳秒级 contains 检查,
/// 不跨 await,可接受。
static SEQUENCE_RUNNING: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// 序列运行期清理 guard:无论线程正常结束、aborted 退出、还是 panic,
/// Drop 都从 SEQUENCE_RUNNING 移除该 port,保证下次同 port run 不会被
/// 残留条目假报"已在运行"。sequence_stop 也会 remove(stop 在外层移除,
/// Drop 再 remove 是幂等 no-op)。
struct RunningGuard {
    port: String,
}
impl Drop for RunningGuard {
    fn drop(&mut self) {
        if let Ok(mut g) = SEQUENCE_RUNNING.lock() {
            g.remove(&self.port);
        }
    }
}

#[tauri::command]
pub fn sequence_run(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    commands: Vec<SequenceCommand>,
    run_count: u32,
    loop_interval: u32,
    port: Option<String>,
) -> Result<(), String> {
    let resolved = {
        let conns = state.connections.lock().map_err(|e| e.to_string())?;
        resolve_port(&conns, port)?
    };
    sequence_run_inner(app_handle.clone(), &state.connections, &resolved, commands, run_count, loop_interval)
}

/// 内部:跑脚本序列(已解析 port)。供 sequence_run command 与 MCP 工具复用。
/// app_handle 用 owned(被 thread::spawn move 进闭包,& 会逃逸借用)。
pub fn sequence_run_inner(
    app_handle: tauri::AppHandle,
    connections: &Arc<Mutex<HashMap<String, crate::connection::ConnectionHandle>>>,
    port: &str,
    commands: Vec<SequenceCommand>,
    run_count: u32,
    loop_interval: u32,
) -> Result<(), String> {
    // 1. 取连接句柄(write_tx + rx_history + window_label + line_index)
    let (write_tx, rx_history, window_label, line_index) = {
        let conns = connections.lock().map_err(|e| e.to_string())?;
        let handle = conns.get(port).ok_or("未连接串口")?;
        (handle.write_tx.clone(), handle.rx_history.clone(), handle.window_label.clone(), handle.line_index.clone())
    };
    let port = port.to_string();

    // 2. SEQUENCE_RUNNING: insert-then-check 契约。已含该 port → 拒绝;
    //    否则 insert,释放锁。线程内靠 contains 判断是否被 stop。
    {
        let mut running = SEQUENCE_RUNNING.lock().map_err(|e| e.to_string())?;
        if running.contains(&port) {
            return Err("该端口序列已在运行".to_string());
        }
        running.insert(port.clone());
    }

    thread::spawn(move || {
        // 3. 清理 guard:线程任意退出路径(正常/aborted/panic)都 remove(&port)。
        //    放在闭包首行,保证后续代码 panic 也能清理。
        let _guard = RunningGuard { port: port.clone() };

        let mut aborted = false;
        for _ in 0..run_count {
            // 被 stop:集合已不含该 port → 退出。锁内只做 contains 检查(纳秒级)。
            let still_running = SEQUENCE_RUNNING
                .lock()
                .map(|g| g.contains(&port))
                .unwrap_or(false);
            if !still_running {
                aborted = true;
                break;
            }
            for (i, cmd) in commands.iter().enumerate() {
                let still_running = SEQUENCE_RUNNING
                    .lock()
                    .map(|g| g.contains(&port))
                    .unwrap_or(false);
                if !still_running {
                    aborted = true;
                    break;
                }
                if !cmd.enabled {
                    continue;
                }

                let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
                let _ = app_handle.emit_to(&wl, "sequence-progress", SequenceProgress {
                    row: i + 1,
                    total: commands.len(),
                });

                // 解析并发送
                let mut data = if cmd.hex {
                    hex_to_bytes(&cmd.command).unwrap_or_default()
                } else {
                    ascii_to_bytes(&cmd.command)
                };
                let line_ending = if cmd.enter {
                    LineEnding::Crlf
                } else {
                    LineEnding::None
                };
                line_ending.append_bytes(&mut data);

                if !data.is_empty() {
                    // 复用 emit_tx_line:发起即 emit tx-line + push rx_history(Tx)
                    // + 文件日志,与 send 命令同一逻辑。log_send=false 时不回显/记录。
                    // (Task 2 deferred minor:原内联逻辑不 push Tx 到 rx_history,
                    //  导致 sequence 发送的命令在 get_history_since 不可见,已修。)
                    emit_tx_line(&app_handle, &rx_history, &window_label, &line_index, data.clone());
                    // 塞 channel,writer 异步写(SendSilent:不 emit,emit_tx_line 已 emit)
                    let _ = write_tx.send(WriteCommand::SendSilent(data));
                }

                // 行间延时:sequence 线程按 delay 间隔发起下一条,不受 writer 写阻塞影响
                if cmd.delay_ms > 0 {
                    thread::sleep(Duration::from_millis(cmd.delay_ms as u64));
                }
            }
            if aborted {
                break;
            }
            if loop_interval > 0 {
                thread::sleep(Duration::from_millis(loop_interval as u64));
            }
        }

        let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
        let _ = app_handle.emit_to(&wl, "sequence-done", SequenceDone { aborted });
        // _guard drop 在此:remove(&port)
    });

    Ok(())
}

/// 停止指定 port 的运行序列。port 解析与 sequence_run 一致:
/// - None + 0 连接:报错(未连接串口)
/// - None + 1 连接:停该 port 的序列
/// - None + 2+ 连接:报"多个连接,请指定端口"(不广播全停,避免误停其他连接的序列)
/// - Some(p) 存在:停 p 的序列
/// - Some(p) 不存在:报"端口 X 未连接"
///
/// 从集合 remove(&port) 后,sequence 线程下次循环 contains 检测到 false 即退出。
/// 若该 port 未在运行,remove 是 no-op,返回 Ok(幂等)。
#[tauri::command]
pub fn sequence_stop(
    state: State<'_, AppState>,
    port: Option<String>,
) -> Result<(), String> {
    let resolved = {
        let conns = state.connections.lock().map_err(|e| e.to_string())?;
        resolve_port(&conns, port)?
    };
    sequence_stop_inner(&state.connections, &resolved)
}

/// 内部:停指定 port 的运行序列(已解析 port)。供 command 与 MCP 工具复用。
pub fn sequence_stop_inner(
    connections: &Arc<Mutex<HashMap<String, crate::connection::ConnectionHandle>>>,
    port: &str,
) -> Result<(), String> {
    // 校验 port 已连接
    {
        let conns = connections.lock().map_err(|e| e.to_string())?;
        if !conns.contains_key(port) {
            return Err(format!("端口 {} 未连接", port));
        }
    }
    if let Ok(mut g) = SEQUENCE_RUNNING.lock() {
        g.remove(port);
    }
    Ok(())
}

/// 保存序列配置。data 为前端序列化后的 JSON（当前是 ScriptModule[]）。
/// 用 serde_json::Value 透传，不锁定具体结构，便于后续扩展模块类型。
#[tauri::command]
pub async fn save_sequence_config(
    path: String,
    data: Vec<serde_json::Value>,
) -> Result<(), String> {
    // 序列化 + 磁盘写入是阻塞操作,不放主线程。
    tauri::async_runtime::spawn_blocking(move || {
        let text = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("保存序列任务执行异常: {}", e))?
}

/// 反转义 QSettings ini 值。单遍左到右扫描,保证 `\\x1A`(转义反斜杠 + 字面 x1A)
/// 不被误当作 `\xHHHH` Unicode 转义。支持 \\ \" \n \r \t \; \, \= \xHHHH(1-4 位 hex)。
fn unescape_qsettings(raw: &str) -> String {
    // 剥外层引号:QSettings 在值含空格/逗号等特殊字符时加,成对出现才剥
    let s = if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1; // 跳过反斜杠
        if i >= chars.len() {
            out.push('\\');
            break;
        }
        match chars[i] {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            ';' | ',' | '=' => out.push(chars[i]),
            'x' => {
                // 贪婪吃最多 4 位十六进制
                let mut hex = String::new();
                while hex.len() < 4 && i + 1 < chars.len() && chars[i + 1].is_ascii_hexdigit() {
                    i += 1;
                    hex.push(chars[i]);
                }
                if !hex.is_empty() {
                    if let Ok(n) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(n) {
                            out.push(c);
                        }
                    }
                } else {
                    out.push('x'); // 孤立的 \x,原样保留 x
                }
            }
            other => {
                out.push('\\');
                out.push(other); // 未知转义原样保留
            }
        }
        i += 1;
    }
    out
}

/// 从 ini 段的 key-value 列表里取 bool 值。
fn ini_bool(kvs: &[(String, String)], key: &str, dflt: bool) -> bool {
    for (k, v) in kvs {
        if k == key {
            return match v.trim().to_lowercase().as_str() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => dflt,
            };
        }
    }
    dflt
}

/// 从 ini 段的 key-value 列表里取原始字符串值(未反转义)。
fn ini_str<'a>(kvs: &'a [(String, String)], key: &str) -> &'a str {
    for (k, v) in kvs {
        if k == key {
            return v;
        }
    }
    ""
}

/// 解析旧串口工具导出的 QSettings ini 配置,转换为 ScriptModule 结构。
/// 只迁移命令文本/hex/回车/勾选/页签名;delay 与 note 不迁移(置 0/空)。
/// 分页:命令段(纯数字段名 [1] [2] ...)按数字序平均切分到 [TabName] 指定的页签。
fn parse_cmiot_ini(text: &str) -> Result<Vec<serde_json::Value>, String> {
    // BOM 剥除
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    // 解析 ini:段名 → key-value 列表(值保持原始转义文本)
    let mut sections: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut cur_name = String::new();

    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with(';') || t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') {
            cur_name = t[1..t.len() - 1].to_string();
            sections.entry(cur_name.clone()).or_default();
            continue;
        }
        if let Some(eq) = t.find('=') {
            let key = t[..eq].trim().to_string();
            let val = t[eq + 1..].to_string();
            if let Some(kvs) = sections.get_mut(&cur_name) {
                kvs.push((key, val));
            }
        }
    }

    // 页签名:从 [TabName] 取 0=xxx 1=xxx ...,按数字序
    let mut page_names: Vec<String> = Vec::new();
    if let Some(tab) = sections.get("TabName") {
        let mut indexed: Vec<(u32, &str)> = tab
            .iter()
            .filter_map(|(k, v)| k.parse::<u32>().ok().map(|n| (n, v.as_str())))
            .collect();
        indexed.sort_by_key(|(n, _)| *n);
        page_names = indexed.iter().map(|(_, v)| unescape_qsettings(v)).collect();
    }
    if page_names.is_empty() {
        page_names.push("Page0".to_string());
    }

    // 命令段:纯数字段名,按数字序
    let mut cmd_keys: Vec<u32> = sections.keys().filter_map(|n| n.parse::<u32>().ok()).collect();
    cmd_keys.sort();
    if cmd_keys.is_empty() {
        return Err("未找到命令段([1]、[2]...),这可能不是旧串口工具导出的配置".to_string());
    }

    // 每页行数 = 命令段数 ÷ 页签数(旧工具每页行数固定)
    if cmd_keys.len() % page_names.len() != 0 {
        return Err(format!(
            "命令段 {} 条除不尽页签数 {},无法推断每页行数",
            cmd_keys.len(),
            page_names.len()
        ));
    }
    let rows_per_page = cmd_keys.len() / page_names.len();

    // 构建 pages
    let mut pages: Vec<serde_json::Value> = Vec::new();
    for (p, page_name) in page_names.iter().enumerate() {
        let mut commands: Vec<serde_json::Value> = Vec::new();
        for r in 0..rows_per_page {
            let idx = p * rows_per_page + r;
            if idx >= cmd_keys.len() {
                break; // 末页不足一页
            }
            let sec_name = cmd_keys[idx].to_string();
            if let Some(kvs) = sections.get(&sec_name) {
                commands.push(serde_json::json!({
                    "enabled": ini_bool(kvs, "select", false),
                    "command": unescape_qsettings(ini_str(kvs, "cmd")),
                    "hex": ini_bool(kvs, "hex", false),
                    "enter": ini_bool(kvs, "enter", true),
                    "delay_ms": 0,
                    "note": "",
                }));
            }
        }
        // 裁掉尾部空行(命令为空);面板要求每页至少 1 行
        while commands.len() > 1 {
            let is_empty = commands
                .last()
                .and_then(|c| c["command"].as_str())
                .map(|s| s.is_empty())
                .unwrap_or(true);
            if is_empty {
                commands.pop();
            } else {
                break;
            }
        }
        if commands.is_empty() {
            commands.push(serde_json::json!({
                "enabled": true, "command": "", "hex": false,
                "enter": true, "delay_ms": 0, "note": "",
            }));
        }
        pages.push(serde_json::json!({ "name": page_name, "commands": commands }));
    }

    Ok(vec![serde_json::json!({
        "id": "quick_commands",
        "name": "快捷指令",
        "type": "quick_commands",
        "pages": pages,
    })])
}

/// 加载序列配置。支持 JSON(本工具导出)与 INI(旧串口工具导出)两种格式:
/// - .json:原样返回 serde_json::Value 数组,结构判定/旧格式迁移由前端完成
///   (旧文件是裸 ScriptPage[],前端检测无 id/pages 字段则包进默认模块)
/// - .ini:解析旧串口工具(QSettings ini)格式,转换为 ScriptModule 结构返回。
///   只迁移命令文本/hex/回车/勾选/页签名;delay 与 note 不迁移(置 0/空)。
#[tauri::command]
pub async fn load_sequence_config(path: String) -> Result<Vec<serde_json::Value>, String> {
    let is_ini = path.to_lowercase().ends_with(".ini");
    // 磁盘读取是阻塞操作,不放主线程。spawn_blocking 返回双层 Result
    // (JoinHandle 错误 + 内层 IO 错误),双 `?` 依次展开。
    let text = tauri::async_runtime::spawn_blocking(move || {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("加载序列任务执行异常: {}", e))??;
    if is_ini {
        return parse_cmiot_ini(&text);
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// 自动保存序列配置到默认路径（%APPDATA%/neoserial/sequence.json）。
/// 前端数据变化时自动调用，无需用户手动选择路径。
/// 保存后广播 "sequence-changed"(带 source label),其他窗口收到后 reload,
/// 实现多窗口快捷指令同步(避免 A 窗口改了 B 窗口不知道,后续保存覆盖 A 的改动)。
#[tauri::command]
pub async fn save_sequence_auto(
    app_handle: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    data: Vec<serde_json::Value>,
) -> Result<(), String> {
    // 序列化 + 磁盘写入是阻塞操作,不放主线程。
    let source = webview_window.label().to_string();
    let handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let appdata = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let dir = appdata.join("neoserial");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("sequence.json");
        let text = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        // 广播给所有窗口(含自己,前端按 source label 跳过自己)
        let _ = handle.emit("sequence-changed", serde_json::json!({
            "source": source
        }));
        Ok(())
    })
    .await
    .map_err(|e| format!("自动保存序列任务执行异常: {}", e))?
}

/// 自动加载序列配置（从 %APPDATA%/neoserial/sequence.json）。
/// 文件不存在时返回空数组，前端用默认模块。
#[tauri::command]
pub async fn load_sequence_auto() -> Result<Vec<serde_json::Value>, String> {
    // 磁盘读取是阻塞操作,不放主线程。文件不存在时返回空数组(前端用默认模块)。
    // spawn_blocking 返回双层 Result(JoinHandle 错误 + 内层 IO 错误),双 `?` 展开。
    let text = tauri::async_runtime::spawn_blocking(|| {
        let appdata = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let path = appdata.join("neoserial").join("sequence.json");
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&path).map(Some).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("加载序列任务执行异常: {}", e))??;
    match text {
        Some(t) => serde_json::from_str(&t).map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

#[derive(serde::Deserialize)]
pub struct SequenceCommand {
    pub enabled: bool,
    pub command: String,
    pub hex: bool,
    pub enter: bool,
    pub delay_ms: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// sequence 测试串行化:SEQUENCE_RUNNING 是全局 static,并行测试会互相污染集合。
    /// 用一把测试专用 Mutex 串行所有 sequence 测试,每个测试开头清空集合保证隔离。
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset() {
        if let Ok(mut g) = SEQUENCE_RUNNING.lock() {
            g.clear();
        }
    }

    /// 同一 port 重复 run 应被拒绝(已在集合 → 报"该端口序列已在运行")。
    /// 验证 sequence_run 的 insert-then-check 契约:首次 insert 成功,二次检测到 contains。
    #[test]
    fn test_same_port_duplicate_run_rejected() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM3".to_string();
        // 首次标记运行中(模拟 sequence_run 成功路径)
        let inserted = SEQUENCE_RUNNING.lock().unwrap().insert(port.clone());
        assert!(inserted, "首次 insert 应成功");

        // 二次 run:检测到该 port 已在集合 → 应报错,不再 insert
        let already_running = SEQUENCE_RUNNING.lock().unwrap().contains(&port);
        assert!(already_running, "同 port 重复 run 应检测到已在运行");
    }

    /// 不同 port 可并行:两个端口都能 insert 进集合,互不排斥。
    /// 验证 per-port 语义(对比原 AtomicBool 全局互斥)。
    #[test]
    fn test_different_ports_can_run_parallel() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let p1 = "COM3".to_string();
        let p2 = "COM5".to_string();
        let ok1 = SEQUENCE_RUNNING.lock().unwrap().insert(p1);
        let ok2 = SEQUENCE_RUNNING.lock().unwrap().insert(p2);
        assert!(ok1 && ok2, "不同 port 应可同时进入运行集合(并行序列)");
        assert_eq!(SEQUENCE_RUNNING.lock().unwrap().len(), 2);
    }

    /// sequence_stop 经 resolve_port 解析后从集合 remove,线程下次循环检测到
    /// contains==false 即退出。验证 stop 的移除语义。
    #[test]
    fn test_stop_removes_port_from_set() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM3".to_string();
        SEQUENCE_RUNNING.lock().unwrap().insert(port.clone());
        let removed = SEQUENCE_RUNNING.lock().unwrap().remove(&port);
        assert!(removed, "stop 应从集合移除该 port");
        assert!(
            !SEQUENCE_RUNNING.lock().unwrap().contains(&port),
            "stop 后该 port 不应在集合"
        );
    }

    /// 序列结束(线程退出)必须 remove(&port) 清理,否则下次同 port run 假报已在运行。
    /// 验证清理后同 port 可再次 insert(模拟线程 Drop guard 清理路径)。
    #[test]
    fn test_cleanup_allows_rerun_same_port() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM3".to_string();
        SEQUENCE_RUNNING.lock().unwrap().insert(port.clone());
        // 模拟序列结束清理(Drop guard / 正常退出路径)
        SEQUENCE_RUNNING.lock().unwrap().remove(&port);
        // 再次 run:应能 insert 成功
        let ok = SEQUENCE_RUNNING.lock().unwrap().insert(port);
        assert!(ok, "清理后同 port 应可再次 run");
    }

    /// stop 不影响其他 port 的运行序列:移除 A 不应误删 B。
    /// 验证 per-port 隔离(避免广播式全停)。
    #[test]
    fn test_stop_one_port_keeps_others() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let a = "COM3".to_string();
        let b = "COM5".to_string();
        SEQUENCE_RUNNING.lock().unwrap().insert(a.clone());
        SEQUENCE_RUNNING.lock().unwrap().insert(b.clone());
        // 停 A
        SEQUENCE_RUNNING.lock().unwrap().remove(&a);
        // B 仍在运行
        assert!(
            SEQUENCE_RUNNING.lock().unwrap().contains(&b),
            "停 A 不应影响 B 的运行序列"
        );
    }

    /// RunningGuard 的 Drop 清理路径本身。原 5 个测试直接操作 HashSet,
    /// 未覆盖 guard 的 Drop trait——这是 sequence_run 线程实际用的清理机制
    /// (panic/正常退出/aborted 都靠它 remove)。构造 guard → insert port →
    /// drop guard → 断言 port 已从集合移除。
    #[test]
    fn test_running_guard_drop_removes_port() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM-TEST".to_string();
        SEQUENCE_RUNNING.lock().unwrap().insert(port.clone());
        assert!(
            SEQUENCE_RUNNING.lock().unwrap().contains(&port),
            "guard drop 前该 port 应在运行集合"
        );
        // 构造 guard 并立即 drop:模拟线程退出(_guard 离开作用域)
        {
            let _guard = RunningGuard { port: port.clone() };
        }
        assert!(
            !SEQUENCE_RUNNING.lock().unwrap().contains(&port),
            "guard drop 后应从运行集合移除该 port"
        );
    }

    // ============ ini 导入测试 ============

    /// QSettings 值反转义:基本转义、引号剥离、Unicode 转义、`\\x1A` 陷阱。
    #[test]
    fn test_unescape_qsettings() {
        // 无转义
        assert_eq!(unescape_qsettings("AT+CFUN?"), "AT+CFUN?");
        // 引号 + 转义引号
        assert_eq!(
            unescape_qsettings(r#""AT+MPING=\"www.baidu.com\"""#),
            r#"AT+MPING="www.baidu.com""#
        );
        // `\\x1A`:转义反斜杠 + 字面 x1A,不是 Unicode 转义
        assert_eq!(unescape_qsettings(r"D5\\x1A"), r"D5\x1A");
        // `\x8bc1`:Unicode 转义 → 证(U+8BC1)
        assert_eq!(unescape_qsettings(r"\x8bc1"), "证");
        // `\x41`:不足 4 位也认(A = U+0041)
        assert_eq!(unescape_qsettings(r"\x41"), "A");
        // 尾部孤立反斜杠
        assert_eq!(unescape_qsettings(r"AT\"), r"AT\");
    }

    /// 完整 ini 解析:页签切分、字段映射、delay/note 不迁移。
    #[test]
    fn test_parse_cmiot_ini_basic() {
        let ini = r#"
[TabName]
0=Page0
1=Page1

[globalSetting]
runTimes=1
delay=500

[1]
select=true
cmd=AT+CFUN?
hex=false
enter=true
delay=1000

[2]
select=false
cmd="AT+MPING=\"www.baidu.com\""
hex=false
enter=true
delay=

[3]
select=false
cmd=AT+CGSN
hex=false
enter=false
delay=500

[4]
select=true
cmd=ATI
hex=false
enter=true
delay=2000
"#;
        let result = parse_cmiot_ini(ini).unwrap();
        assert_eq!(result.len(), 1);
        let module = &result[0];
        assert_eq!(module["id"], "quick_commands");
        assert_eq!(module["name"], "快捷指令");
        assert_eq!(module["type"], "quick_commands");
        let pages = module["pages"].as_array().unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0]["name"], "Page0");
        assert_eq!(pages[1]["name"], "Page1");
        // Page0: [1] [2]
        let p0 = pages[0]["commands"].as_array().unwrap();
        assert_eq!(p0.len(), 2);
        assert_eq!(p0[0]["command"], "AT+CFUN?");
        assert_eq!(p0[0]["enabled"], true);
        assert_eq!(p0[0]["hex"], false);
        assert_eq!(p0[0]["enter"], true);
        assert_eq!(p0[0]["delay_ms"], 0);
        assert_eq!(p0[0]["note"], "");
        assert_eq!(p0[1]["command"], r#"AT+MPING="www.baidu.com""#);
        assert_eq!(p0[1]["enabled"], false);
        // Page1: [3] [4]
        let p1 = pages[1]["commands"].as_array().unwrap();
        assert_eq!(p1.len(), 2);
        assert_eq!(p1[0]["command"], "AT+CGSN");
        assert_eq!(p1[0]["enter"], false);
        assert_eq!(p1[1]["command"], "ATI");
        assert_eq!(p1[1]["enabled"], true);
    }

    /// 尾部空行裁剪:末页空命令不保留,每页至少 1 行。
    #[test]
    fn test_parse_cmiot_ini_trim_empty() {
        let ini = r#"
[TabName]
0=Page0

[1]
select=false
cmd=AT+CFUN?
hex=false
enter=true
delay=

[2]
select=false
cmd=
hex=false
enter=true
delay=
"#;
        let result = parse_cmiot_ini(ini).unwrap();
        let pages = result[0]["pages"].as_array().unwrap();
        let cmds = pages[0]["commands"].as_array().unwrap();
        assert_eq!(cmds.len(), 1, "尾部空行应裁掉");
        assert_eq!(cmds[0]["command"], "AT+CFUN?");
    }

    /// 除不尽时报错:3 条命令 2 个页签 → 无法推断每页行数。
    #[test]
    fn test_parse_cmiot_ini_indivisible() {
        let ini = r#"
[TabName]
0=Page0
1=Page1

[1]
select=false
cmd=AT+CFUN?
hex=false
enter=true
delay=

[2]
select=false
cmd=ATI
hex=false
enter=true
delay=

[3]
select=false
cmd=AT+CGSN
hex=false
enter=true
delay=
"#;
        let err = parse_cmiot_ini(ini).unwrap_err();
        assert!(err.contains("除不尽"), "应报除不尽错误,实际: {}", err);
    }

    /// 无命令段时报错。
    #[test]
    fn test_parse_cmiot_ini_no_commands() {
        let ini = r#"
[TabName]
0=Page0

[globalSetting]
runTimes=1
delay=500
"#;
        let err = parse_cmiot_ini(ini).unwrap_err();
        assert!(err.contains("未找到命令段"), "应报未找到命令段,实际: {}", err);
    }
}
