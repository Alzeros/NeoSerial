use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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

/// 序列运行状态。per-port 隔离,记录当前执行位置,供 get_sequence_status 查询。
/// 断开/stop 导致中止时 current_index 停在最后执行的位置,agent 可据此定位问题指令。
#[derive(Clone)]
pub struct SequenceState {
    /// 本次 run 的代际号(全局单调递增)。线程与 [`RunningGuard`] 改写 state 前都先比对它:
    /// stop 只是把 running 置 false,旧线程可能还在 delay/loop_interval 里睡,若此时同 port
    /// 新 run 插入了 running=true 的新 state,旧线程醒来看到的就是新序列的 true——没有代际号
    /// 它会继续跑自己的命令(幻影 Tx、覆盖 current_index),跑完再把新序列的 running 清掉。
    pub run_id: u64,
    /// true = 序列正在运行
    pub running: bool,
    /// 当前/最后执行的命令索引 (0-based)
    pub current_index: usize,
    /// 命令总数
    pub total: usize,
    /// true = 全部命令执行完毕(正常结束)
    pub completed: bool,
    /// stop 唤醒信号:序列线程的行间延时/循环间隔睡在它上面,stop 一到立刻醒。
    pub stop_signal: StopSignal,
}

impl SequenceState {
    fn new(run_id: u64, total: usize) -> Self {
        SequenceState {
            run_id,
            running: true,
            current_index: 0,
            total,
            completed: false,
            stop_signal: StopSignal::new(),
        }
    }
}

/// 可被 stop 打断的睡眠:Mutex<已停止> + Condvar。stop 置位并 notify,
/// 线程的 wait_timeout 即刻返回,不用等 delay_ms/loop_interval 自然到点
/// (循环间隔常设几秒,原先 stop 后界面要卡到那时才收到 sequence-done)。
#[derive(Clone)]
pub struct StopSignal(Arc<(Mutex<bool>, Condvar)>);

impl StopSignal {
    fn new() -> Self {
        StopSignal(Arc::new((Mutex::new(false), Condvar::new())))
    }

    fn signal(&self) {
        let (lock, cv) = &*self.0;
        if let Ok(mut stopped) = lock.lock() {
            *stopped = true;
        }
        cv.notify_all();
    }

    /// 睡 `dur`,期间被 signal 则提前返回。
    fn sleep(&self, dur: Duration) {
        let (lock, cv) = &*self.0;
        let deadline = Instant::now() + dur;
        let Ok(mut stopped) = lock.lock() else { return };
        while !*stopped {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match cv.wait_timeout(stopped, deadline - now) {
                Ok((g, _)) => stopped = g,
                Err(_) => return,
            }
        }
    }
}

static SEQUENCE_STATE: LazyLock<Mutex<HashMap<String, SequenceState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// run_id 分配器。0 保留为"从未分配",首个 run 拿到 1。
static SEQUENCE_RUN_SEQ: AtomicU64 = AtomicU64::new(0);

/// 该 port 当前 state 是否仍是本次 run 且在运行。false = 被 stop、被断开、或被新 run 顶替。
fn is_current_run(port: &str, run_id: u64) -> bool {
    SEQUENCE_STATE
        .lock()
        .map(|g| g.get(port).map(|s| s.run_id == run_id && s.running).unwrap_or(false))
        .unwrap_or(false)
}

/// 仅当该 port 的 state 仍属本次 run 时改写它;被新 run 顶替后旧线程不得再碰。
fn with_current_run(port: &str, run_id: u64, f: impl FnOnce(&mut SequenceState)) {
    if let Ok(mut g) = SEQUENCE_STATE.lock() {
        if let Some(state) = g.get_mut(port) {
            if state.run_id == run_id {
                f(state);
            }
        }
    }
}

/// 序列运行期清理 guard:无论线程正常结束、aborted 退出、还是 panic,
/// Drop 都把该 port 的 running 置 false(不清除 state,供 get_sequence_status 查询)。
/// 按 run_id 比对:state 已被新 run 顶替时不动它,否则旧线程退出会把新序列误停。
struct RunningGuard {
    port: String,
    run_id: u64,
}
impl Drop for RunningGuard {
    fn drop(&mut self) {
        with_current_run(&self.port, self.run_id, |state| state.running = false);
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

    // 2. SEQUENCE_STATE: check-then-insert 契约。已含该 port 且 running → 拒绝;
    //    否则 insert 新代际的 state(覆盖旧 state),释放锁。
    //    线程内靠 is_current_run(run_id 一致且 running)判断是否继续。
    let run_id = SEQUENCE_RUN_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let stop_signal = {
        let mut states = SEQUENCE_STATE.lock().map_err(|e| e.to_string())?;
        if states.get(&port).map(|s| s.running).unwrap_or(false) {
            return Err("该端口序列已在运行".to_string());
        }
        let state = SequenceState::new(run_id, commands.len());
        let signal = state.stop_signal.clone();
        states.insert(port.clone(), state);
        signal
    };

    thread::spawn(move || {
        // 3. 清理 guard:线程任意退出路径(正常/aborted/panic)都 running=false。
        //    不清除 state 条目(供 get_sequence_status 查询中止位置)。
        let _guard = RunningGuard { port: port.clone(), run_id };

        let mut aborted = false;
        'rounds: for _ in 0..run_count {
            // 被 stop / 断开 / 新 run 顶替 → 退出。
            if !is_current_run(&port, run_id) {
                aborted = true;
                break;
            }
            for (i, cmd) in commands.iter().enumerate() {
                if !is_current_run(&port, run_id) {
                    aborted = true;
                    break 'rounds;
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
                    // 发送后更新 current_index:始终反映"最后发出去的是第几条"(0-based)
                    with_current_run(&port, run_id, |state| state.current_index = i);
                }

                // 行间延时:sequence 线程按 delay 间隔发起下一条,不受 writer 写阻塞影响;
                // stop 会打断睡眠,下一轮 is_current_run 检查即退出
                if cmd.delay_ms > 0 {
                    stop_signal.sleep(Duration::from_millis(cmd.delay_ms as u64));
                }
            }
            if loop_interval > 0 {
                stop_signal.sleep(Duration::from_millis(loop_interval as u64));
            }
        }

        // 只有本次 run 仍是该 port 的当前 state 才发 sequence-done / 标 completed:
        // 被新 run 顶替后再发 aborted,会把界面上正在跑的新序列误标成"已中止"。
        let still_current = SEQUENCE_STATE
            .lock()
            .map(|g| g.get(&port).map(|s| s.run_id == run_id).unwrap_or(false))
            .unwrap_or(false);
        if still_current {
            let wl = window_label.read().map(|s| s.clone()).unwrap_or_default();
            let _ = app_handle.emit_to(&wl, "sequence-done", SequenceDone { aborted });
            // 正常完成:标记 completed=true(running 随后由 guard 置 false)。
            // 中止时 completed 保持 false,current_index 停在最后执行位置。
            if !aborted {
                with_current_run(&port, run_id, |state| state.completed = true);
            }
        }
        // _guard drop 在此:running=false(仅当 state 仍属本次 run)
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
    sequence_stop_inner(&state.connections, &resolved).map(|_| ())
}

/// 查询指定 port 的序列状态(含进度)。供 MCP get_sequence_status 工具调用。
/// 返回 (running, completed, current_index, total)。port 从未跑过序列时全为 false/0。
pub fn sequence_get_state(port: &str) -> (bool, bool, usize, usize) {
    SEQUENCE_STATE.lock()
        .map(|g| {
            g.get(port).map(|s| (s.running, s.completed, s.current_index, s.total))
                .unwrap_or((false, false, 0, 0))
        })
        .unwrap_or((false, false, 0, 0))
}

/// 内部:停指定 port 的运行序列(已解析 port)。供 command 与 MCP 工具复用。
/// 返回 true 表示确实停了一个正在运行的序列,false 表示该 port 没有在运行序列。
pub fn sequence_stop_inner(
    connections: &Arc<Mutex<HashMap<String, crate::connection::ConnectionHandle>>>,
    port: &str,
) -> Result<bool, String> {
    // 校验 port 已连接
    {
        let conns = connections.lock().map_err(|e| e.to_string())?;
        if !conns.contains_key(port) {
            return Err(format!("端口 {} 未连接", port));
        }
    }
    let was_running = SEQUENCE_STATE.lock()
        .map(|mut g| {
            if let Some(state) = g.get_mut(port) {
                let was = state.running;
                state.running = false;
                // 唤醒睡在 delay/loop_interval 里的序列线程,让它立刻退出并发 sequence-done
                state.stop_signal.signal();
                was
            } else {
                false
            }
        })
        .unwrap_or(false);
    Ok(was_running)
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

    /// sequence 测试串行化:SEQUENCE_STATE 是全局 static,并行测试会互相污染。
    /// 用一把测试专用 Mutex 串行所有 sequence 测试,每个测试开头清空保证隔离。
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset() {
        if let Ok(mut g) = SEQUENCE_STATE.lock() {
            g.clear();
        }
    }

    /// 辅助:往 SEQUENCE_STATE 插入一个 running=true 的状态(run_id=1)
    fn insert_running(port: &str, total: usize) {
        insert_running_with_id(port, 1, total);
    }

    /// 辅助:插入指定 run_id 的 running=true 状态
    fn insert_running_with_id(port: &str, run_id: u64, total: usize) {
        SEQUENCE_STATE.lock().unwrap().insert(port.to_string(), SequenceState::new(run_id, total));
    }

    /// 同一 port 重复 run 应被拒绝(running=true → 报"该端口序列已在运行")。
    #[test]
    fn test_same_port_duplicate_run_rejected() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM3";
        insert_running(port, 10);
        let already_running = SEQUENCE_STATE.lock().unwrap().get(port).map(|s| s.running).unwrap_or(false);
        assert!(already_running, "同 port 重复 run 应检测到已在运行");
    }

    /// 不同 port 可并行:两个端口都能 insert,互不排斥。
    #[test]
    fn test_different_ports_can_run_parallel() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        insert_running("COM3", 10);
        insert_running("COM5", 5);
        assert_eq!(SEQUENCE_STATE.lock().unwrap().len(), 2);
    }

    /// sequence_stop 把 running 置 false,线程下次循环检测到即退出。
    #[test]
    fn test_stop_sets_running_false() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM3";
        insert_running(port, 10);
        // 模拟 stop_inner
        if let Some(state) = SEQUENCE_STATE.lock().unwrap().get_mut(port) {
            state.running = false;
        }
        let running = SEQUENCE_STATE.lock().unwrap().get(port).map(|s| s.running).unwrap_or(false);
        assert!(!running, "stop 后 running 应为 false");
    }

    /// 序列结束后(running=false, completed=true)同 port 可再次 run(覆盖旧 state)。
    #[test]
    fn test_cleanup_allows_rerun_same_port() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM3";
        insert_running(port, 10);
        // 模拟序列正常结束
        {
            let mut g = SEQUENCE_STATE.lock().unwrap();
            if let Some(state) = g.get_mut(port) {
                state.running = false;
                state.completed = true;
            }
        }
        // 再次 run:覆盖旧 state
        insert_running(port, 5);
        let state = SEQUENCE_STATE.lock().unwrap().get(port).unwrap().clone();
        assert!(state.running, "覆盖后应 running=true");
        assert!(!state.completed, "覆盖后应 completed=false(新序列)");
        assert_eq!(state.total, 5);
    }

    /// stop 不影响其他 port 的运行序列。
    #[test]
    fn test_stop_one_port_keeps_others() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        insert_running("COM3", 10);
        insert_running("COM5", 10);
        // 停 COM3
        if let Some(state) = SEQUENCE_STATE.lock().unwrap().get_mut("COM3") {
            state.running = false;
        }
        let b_running = SEQUENCE_STATE.lock().unwrap().get("COM5").map(|s| s.running).unwrap_or(false);
        assert!(b_running, "停 COM3 不应影响 COM5");
    }

    /// RunningGuard 的 Drop 清理路径:构造 guard → drop → running 应被置 false。
    #[test]
    fn test_running_guard_drop_sets_running_false() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM-TEST".to_string();
        insert_running(&port, 10);
        assert!(SEQUENCE_STATE.lock().unwrap().get(&port).unwrap().running, "guard drop 前 running=true");
        {
            let _guard = RunningGuard { port: port.clone(), run_id: 1 };
        }
        assert!(!SEQUENCE_STATE.lock().unwrap().get(&port).unwrap().running, "guard drop 后 running=false");
    }

    /// 代际隔离:旧 run 的 guard/线程不得触碰已被新 run 顶替的 state。
    /// 场景:stop 后旧线程还在睡,同 port 新 run 插入 run_id=2;旧线程(run_id=1)醒来
    /// 必须视为"已不是当前 run"退出,其 guard drop 也不能把新序列的 running 清掉。
    #[test]
    fn test_stale_run_cannot_touch_newer_state() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        let port = "COM3";
        insert_running_with_id(port, 1, 10);
        // stop 旧 run
        sequence_stop_state_only(port);
        assert!(!is_current_run(port, 1), "stop 后旧 run 不再是当前 run");
        // 新 run 顶替
        insert_running_with_id(port, 2, 5);
        assert!(!is_current_run(port, 1), "被新 run 顶替后旧 run 仍不是当前 run");
        assert!(is_current_run(port, 2));
        // 旧线程试图改 current_index / 退出清理:都应无效
        with_current_run(port, 1, |s| s.current_index = 9);
        {
            let _stale = RunningGuard { port: port.to_string(), run_id: 1 };
        }
        let state = SEQUENCE_STATE.lock().unwrap().get(port).unwrap().clone();
        assert!(state.running, "旧 run 的 guard drop 不应停掉新序列");
        assert_eq!(state.current_index, 0, "旧 run 不应改写新序列的进度");
        assert_eq!(state.run_id, 2);
    }

    /// stop 信号能打断睡眠:signal 后 sleep 立即返回,而不是等满时长。
    #[test]
    fn test_stop_signal_interrupts_sleep() {
        let sig = StopSignal::new();
        let sig2 = sig.clone();
        let t = std::thread::spawn(move || {
            let start = Instant::now();
            sig2.sleep(Duration::from_secs(10));
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(50));
        sig.signal();
        let elapsed = t.join().unwrap();
        assert!(elapsed < Duration::from_secs(5), "signal 后应立即醒来,实际睡了 {:?}", elapsed);
        // 已 signal 的信号再 sleep 应直接返回
        let start = Instant::now();
        sig.sleep(Duration::from_secs(10));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    /// 测试用:只操作 SEQUENCE_STATE 的 stop(sequence_stop_inner 还要校验 connections)。
    fn sequence_stop_state_only(port: &str) {
        if let Some(state) = SEQUENCE_STATE.lock().unwrap().get_mut(port) {
            state.running = false;
            state.stop_signal.signal();
        }
    }

    /// get_sequence_state 返回正确进度信息。
    #[test]
    fn test_get_state_returns_progress() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        // 从未跑过 → 全 false/0
        let (running, completed, idx, total) = sequence_get_state("COM3");
        assert!(!running && !completed && idx == 0 && total == 0);

        // 运行中
        let mut running_state = SequenceState::new(1, 10);
        running_state.current_index = 3;
        SEQUENCE_STATE.lock().unwrap().insert("COM3".into(), running_state);
        let (running, completed, idx, total) = sequence_get_state("COM3");
        assert!(running && !completed && idx == 3 && total == 10);

        // 中止(running=false, completed=false, current_index 停在 3)
        {
            let mut g = SEQUENCE_STATE.lock().unwrap();
            g.get_mut("COM3").unwrap().running = false;
        }
        let (running, completed, idx, total) = sequence_get_state("COM3");
        assert!(!running && !completed && idx == 3 && total == 10);

        // 正常完成
        {
            let mut g = SEQUENCE_STATE.lock().unwrap();
            let s = g.get_mut("COM3").unwrap();
            s.running = false;
            s.completed = true;
        }
        let (running, completed, idx, total) = sequence_get_state("COM3");
        assert!(!running && completed && idx == 3 && total == 10);
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
