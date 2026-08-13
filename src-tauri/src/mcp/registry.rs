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

/// 一个 cmserial 实例的 registry 条目。
#[derive(Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub pid: u32,
    pub port: u16,
    pub com: Option<String>,
    pub baud: Option<u32>,
    pub connected: bool,
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
            com: None,
            baud: None,
            connected: false,
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

    /// 更新本实例的 com/baud/connected。
    pub fn update_com(&self, com: Option<String>, baud: Option<u32>, connected: bool) -> Result<(), String> {
        let mut all = read_all(&self.path);
        for e in all.iter_mut() {
            if e.pid == self.pid {
                e.com = com.clone();
                e.baud = baud;
                e.connected = connected;
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
        dir.join(format!("cmserial-mcp-test-{}.json", nano))
    }

    #[test]
    fn test_register_and_read() {
        let path = tmp_path();
        // 临时替换 registry_path:用独立函数测试 read_all/write_all
        let entry = RegistryEntry {
            pid: 99991, port: 23333, com: None, baud: None,
            connected: false, started_at: 1, heartbeat_at: 1,
        };
        write_all(&path, &[entry.clone()]).unwrap();
        let all = read_all(&path);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].pid, 99991);
        assert_eq!(all[0].port, 23333);
        assert!(!all[0].connected);
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
            pid: 1, port: 23333, com: None, baud: None,
            connected: false, started_at: 1, heartbeat_at: 1,
        }]).unwrap();
        write_all(&path, &[RegistryEntry {
            pid: 2, port: 23334, com: Some("COM5".into()), baud: Some(9600),
            connected: true, started_at: 2, heartbeat_at: 2,
        }]).unwrap();
        let all = read_all(&path);
        assert_eq!(all.len(), 1); // 第二次写覆盖了第一次
        assert_eq!(all[0].pid, 2);
        assert_eq!(all[0].com.as_deref(), Some("COM5"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_registry_path_under_neoserial() {
        let p = registry_path();
        assert!(p.to_string_lossy().contains("neoserial"));
        assert!(p.to_string_lossy().ends_with("mcp-registry.json"));
    }
}
