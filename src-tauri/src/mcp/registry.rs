use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// registry 文件路径:%APPDATA%/neoserial/mcp-registry.json
pub fn registry_path() -> PathBuf {
    let appdata = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    appdata.join("neoserial").join("mcp-registry.json")
}

/// 单个连接信息(COM 口 + 波特率)。registry 批量化(Task 5):
/// 每个实例持有多连接列表,替代旧 com/baud/connected 单值字段。
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ConnInfo {
    #[serde(default)]
    pub com: String,
    #[serde(default)]
    pub baud: u32,
}

/// 一个 neoserial 实例的 registry 条目。
/// connections 为完整快照(connect/disconnect 后用 update_connections 整体替换)。
/// connections 字段标 `#[serde(default)]`:旧 registry.json(单连接期写入,
/// 缺 connections 字段)反序列化时用空 Vec 兜底,不报错。
#[derive(Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub pid: u32,
    pub port: u16,
    #[serde(default)]
    pub connections: Vec<ConnInfo>,
    pub started_at: u64,
    pub heartbeat_at: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 读全部条目。文件不存在返回空 Vec。
fn read_all(path: &Path) -> Vec<RegistryEntry> {
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 原子写全部条目:写临时文件 + rename(避免多实例同时写的竞争撕裂数据)。
fn write_all(path: &Path, entries: &[RegistryEntry]) -> Result<(), String> {
    let s = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("建目录失败: {}", e))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, s).map_err(|e| format!("写临时文件失败: {}", e))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename 失败: {}", e))?;
    Ok(())
}

/// 持有本实例 pid 与 registry 路径,封装登记/更新/心跳/注销。
#[derive(Clone)]
pub struct RegistryHandle {
    pid: u32,
    port: u16,
    path: PathBuf,
}

impl RegistryHandle {
    /// 登记一条(com=null, connected=false)。返回 handle 供后续更新。
    pub fn register(port: u16) -> Result<Self, String> {
        let path = registry_path();
        let pid = std::process::id();
        let now = now_secs();
        let entry = RegistryEntry {
            pid,
            port,
            connections: vec![],
            started_at: now,
            heartbeat_at: now,
        };
        let mut all = read_all(&path);
        // 去重:同 pid 的旧条目先移除(防重复登记)
        all.retain(|e| e.pid != pid);
        all.push(entry);
        write_all(&path, &all)?;
        Ok(RegistryHandle { pid, port, path })
    }

    /// 更新本实例的 connections(完整快照整体替换)。
    /// connect 成功后/disconnect 后由 tools 构造当前所有连接的快照传入,
    /// read_all → 改本 pid 的 connections + heartbeat → write_all。
    /// 传空 Vec 即表示无连接(全部断开)。
    pub fn update_connections(&self, conns: Vec<ConnInfo>) -> Result<(), String> {
        let mut all = read_all(&self.path);
        for e in all.iter_mut() {
            if e.pid == self.pid {
                e.connections = conns.clone();
                e.heartbeat_at = now_secs();
            }
        }
        write_all(&self.path, &all)
    }

    /// 刷新心跳时间。
    pub fn heartbeat(&self) -> Result<(), String> {
        let mut all = read_all(&self.path);
        let now = now_secs();
        for e in all.iter_mut() {
            if e.pid == self.pid {
                e.heartbeat_at = now;
            }
        }
        write_all(&self.path, &all)
    }

    /// 注销本实例。
    pub fn unregister(&self) -> Result<(), String> {
        let mut all = read_all(&self.path);
        all.retain(|e| e.pid != self.pid);
        write_all(&self.path, &all)
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

/// 清理 registry 中本机已不存活的进程条目。
/// 启动时调一次,移除前次/崩溃残留(pid 不在存活的条目)。
/// 不清理自身 pid(尚未登记或在运行)。
pub fn cleanup_dead() {
    let path = registry_path();
    let mut all = read_all(&path);
    let before = all.len();
    all.retain(|e| is_pid_alive(e.pid));
    if all.len() != before {
        let _ = write_all(&path, &all);
    }
}

/// 判断 pid 对应进程是否存活(Windows: OpenProcess 成功即活)。
fn is_pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
            fn CloseHandle(h: *mut c_void) -> i32;
        }
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            CloseHandle(h);
            true
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        // 非 Windows 平台本机自用未支持,保守返回 true(不清理)。
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_path() -> PathBuf {
        // 用进程无关的临时文件名,避免并发测试互踩
        let dir = std::env::temp_dir();
        let nano = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        dir.join(format!("neoserial-mcp-test-{}.json", nano))
    }

    #[test]
    fn test_register_and_read() {
        let path = tmp_path();
        // 临时替换 registry_path:用独立函数测试 read_all/write_all
        let entry = RegistryEntry {
            pid: 99991, port: 23333,
            connections: vec![],
            started_at: 1, heartbeat_at: 1,
        };
        write_all(&path, &[entry.clone()]).unwrap();
        let all = read_all(&path);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].pid, 99991);
        assert_eq!(all[0].port, 23333);
        assert!(all[0].connections.is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_read_missing_file_empty() {
        let path = tmp_path();
        let _ = fs::remove_file(&path);
        let all = read_all(&path);
        assert!(all.is_empty());
    }

    #[test]
    fn test_write_replaces_not_appends() {
        let path = tmp_path();
        write_all(&path, &[RegistryEntry {
            pid: 1, port: 23333,
            connections: vec![],
            started_at: 1, heartbeat_at: 1,
        }]).unwrap();
        write_all(&path, &[RegistryEntry {
            pid: 2, port: 23334,
            connections: vec![ConnInfo { com: "COM5".into(), baud: 9600 }],
            started_at: 2, heartbeat_at: 2,
        }]).unwrap();
        let all = read_all(&path);
        assert_eq!(all.len(), 1); // 第二次写覆盖了第一次
        assert_eq!(all[0].pid, 2);
        assert_eq!(all[0].connections.len(), 1);
        assert_eq!(all[0].connections[0].com, "COM5");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_registry_path_under_neoserial() {
        let p = registry_path();
        assert!(p.to_string_lossy().contains("neoserial"));
        assert!(p.to_string_lossy().ends_with("mcp-registry.json"));
    }

    #[test]
    fn test_is_pid_alive_current_process_true() {
        // 当前测试进程本身应存活
        assert!(is_pid_alive(std::process::id()));
    }

    #[test]
    fn test_is_pid_alive_dead_pid_false() {
        // 极不可能存活的 pid(Windows pid 上限远小于此)
        assert!(!is_pid_alive(999_999_999));
    }

    // ============ Task 5: connections 列表批量化 ============

    /// write_all 写含 connections 列表的 entry,read_all 读回验证序列化/反序列化正确。
    #[test]
    fn test_write_read_connections_list() {
        let path = tmp_path();
        let entry = RegistryEntry {
            pid: 100, port: 20000,
            connections: vec![
                ConnInfo { com: "COM5".into(), baud: 9600 },
                ConnInfo { com: "COM7".into(), baud: 115200 },
            ],
            started_at: 1, heartbeat_at: 1,
        };
        write_all(&path, &[entry]).unwrap();
        let all = read_all(&path);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].connections.len(), 2);
        assert_eq!(all[0].connections[0].com, "COM5");
        assert_eq!(all[0].connections[0].baud, 9600);
        assert_eq!(all[0].connections[1].com, "COM7");
        assert_eq!(all[0].connections[1].baud, 115200);
        let _ = fs::remove_file(&path);
    }

    /// update_connections 用完整快照整体替换本 pid 的 connections,并刷新 heartbeat。
    #[test]
    fn test_update_connections_replaces_snapshot() {
        let path = tmp_path();
        // 先写一条空 connections 的 entry
        let entry = RegistryEntry {
            pid: 200, port: 30000,
            connections: vec![],
            started_at: 1, heartbeat_at: 1,
        };
        write_all(&path, &[entry]).unwrap();
        // 构造 handle 指向同一临时文件(测试模块可访问私有字段)
        let handle = RegistryHandle { pid: 200, port: 30000, path: path.clone() };
        // 调 update_connections 写入两个连接快照
        handle.update_connections(vec![
            ConnInfo { com: "COM3".into(), baud: 115200 },
            ConnInfo { com: "COM9".into(), baud: 9600 },
        ]).unwrap();
        // 读回验证:connections 被整体替换为快照
        let all = read_all(&path);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].connections.len(), 2);
        assert_eq!(all[0].connections[0].com, "COM3");
        assert_eq!(all[0].connections[0].baud, 115200);
        assert_eq!(all[0].connections[1].com, "COM9");
        assert_eq!(all[0].connections[1].baud, 9600);
        // heartbeat 应被刷新(>初始值 1)
        assert!(all[0].heartbeat_at > 1);
        let _ = fs::remove_file(&path);
    }

    /// update_connections 用空快照清空连接列表(disconnect 最后一个连接后的场景)。
    #[test]
    fn test_update_connections_empty_clears() {
        let path = tmp_path();
        let entry = RegistryEntry {
            pid: 300, port: 40000,
            connections: vec![ConnInfo { com: "COM3".into(), baud: 115200 }],
            started_at: 1, heartbeat_at: 1,
        };
        write_all(&path, &[entry]).unwrap();
        let handle = RegistryHandle { pid: 300, port: 40000, path: path.clone() };
        handle.update_connections(vec![]).unwrap();
        let all = read_all(&path);
        assert_eq!(all[0].connections.len(), 0);
        let _ = fs::remove_file(&path);
    }
}
