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

/// 加载序列配置。返回原始 JSON 数组，结构判定/旧格式迁移由前端完成
/// （旧文件是裸 ScriptPage[]，前端检测无 id/pages 字段则包进默认模块）。
#[tauri::command]
pub async fn load_sequence_config(path: String) -> Result<Vec<serde_json::Value>, String> {
    // 磁盘读取是阻塞操作,不放主线程。spawn_blocking 返回双层 Result
    // (JoinHandle 错误 + 内层 IO 错误),双 `?` 依次展开。
    let text = tauri::async_runtime::spawn_blocking(move || {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("加载序列任务执行异常: {}", e))??;
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
}
