//! 输入框发送历史:最近发送在前、去重、条数上限来自设置(command_index.history_limit),
//! 落在 %APPDATA%/neoserial/send-history.json。
//! 后端持有(AppState.send_history)并广播变化,多窗口同时 push 不会互相覆盖。
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 上限的默认值(设置项缺失时用)。改上限见 settings.command_index.history_limit。
pub const SEND_HISTORY_DEFAULT_MAX: usize = 200;

#[derive(Debug, Default)]
pub struct SendHistory {
    items: Vec<String>,
}

impl SendHistory {
    fn history_path() -> PathBuf {
        crate::config::config_dir().join("send-history.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::history_path())
    }

    /// 文件不存在 → 空;损坏 → 改名 .bad + 空。
    pub(crate) fn load_from(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match serde_json::from_str::<Vec<String>>(&text) {
            Ok(items) => SendHistory { items },
            Err(_) => {
                let _ = fs::rename(path, path.with_extension("json.bad"));
                Self::default()
            }
        }
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    /// 记一条:去首尾空格,空串不记;已有同文本挪到最前;超上限截掉最旧。
    /// `max` 是设置里的条数上限,兜底为 1——上限传 0 会把刚记下的这条也截掉。
    /// 返回列表是否有变化(已在最前的重复发送 = 无变化,调用方据此跳过落盘与广播)。
    pub fn push(&mut self, text: &str, max: usize) -> bool {
        let t = text.trim();
        if t.is_empty() {
            return false;
        }
        if self.items.first().map(|s| s == t).unwrap_or(false) {
            return false;
        }
        self.items.retain(|s| s != t);
        self.items.insert(0, t.to_string());
        self.items.truncate(max.max(1));
        true
    }

    /// 上限被调小后裁掉超出的(留最新的)。返回是否真的裁掉了东西,
    /// 调用方据此决定要不要落盘与广播。
    pub fn trim_to(&mut self, max: usize) -> bool {
        let cap = max.max(1);
        if self.items.len() <= cap {
            return false;
        }
        self.items.truncate(cap);
        true
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&Self::history_path())
    }

    /// 先写 .tmp 再改名,与 command-index.json 同法。写入方只有 send_history_push/clear 两个 command,
    /// 都在 AppState.send_history 的锁内完成,天然串行。
    pub(crate) fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.items)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neoserial_hist_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("send-history.json")
    }

    const MAX: usize = SEND_HISTORY_DEFAULT_MAX;

    #[test]
    fn test_push_trims_dedups_and_moves_to_front() {
        let mut h = SendHistory::default();
        assert!(h.push("AT", MAX));
        assert!(h.push("AT+CSQ", MAX));
        assert!(h.push("  AT  ", MAX), "去空格后与已有项相同,应挪到最前并算作变化");
        assert_eq!(h.items(), &["AT".to_string(), "AT+CSQ".to_string()]);
        assert!(!h.push("AT", MAX), "已在最前的重复发送 = 无变化");
        assert!(!h.push("   ", MAX), "空串不记");
        assert_eq!(h.items().len(), 2);
    }

    #[test]
    fn test_push_caps_at_max_keeping_newest() {
        let mut h = SendHistory::default();
        for i in 0..(MAX + 20) {
            h.push(&format!("AT+X={}", i), MAX);
        }
        assert_eq!(h.items().len(), MAX);
        assert_eq!(h.items()[0], format!("AT+X={}", MAX + 19), "最新在前");
        assert!(!h.items().contains(&"AT+X=0".to_string()), "最旧的被挤掉");

        // 上限来自设置,可以是任意值;传 0 兜底成 1,不能把刚记下的那条也截掉
        let mut small = SendHistory::default();
        for i in 0..5 {
            small.push(&format!("AT+Y={}", i), 2);
        }
        assert_eq!(small.items(), &["AT+Y=4".to_string(), "AT+Y=3".to_string()]);
        assert!(small.push("AT+Z", 0));
        assert_eq!(small.items(), &["AT+Z".to_string()]);
    }

    #[test]
    fn test_trim_to_only_reports_change_when_it_cuts() {
        let mut h = SendHistory::default();
        for i in 0..5 {
            h.push(&format!("AT+X={}", i), MAX);
        }
        assert!(!h.trim_to(5), "正好等于上限,没裁掉东西");
        assert!(!h.trim_to(9), "上限比现有条数大,不动");
        assert!(h.trim_to(2), "上限调小,裁掉 3 条");
        assert_eq!(h.items(), &["AT+X=4".to_string(), "AT+X=3".to_string()], "留最新的");
        assert!(h.trim_to(0), "上限 0 兜底成 1");
        assert_eq!(h.items().len(), 1);
    }

    #[test]
    fn test_save_load_roundtrip_overwrite_and_clear() {
        let path = temp_path("roundtrip");
        let mut h = SendHistory::default();
        h.push("AT+CGDCONT=1,\"IP\",\"cmnet\"", MAX);
        h.push("AT+CSQ", MAX);
        h.save_to(&path).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        let loaded = SendHistory::load_from(&path);
        assert_eq!(loaded.items(), &["AT+CSQ".to_string(), "AT+CGDCONT=1,\"IP\",\"cmnet\"".to_string()]);

        // 覆盖已有文件 + 残留 .tmp 的生产路径
        fs::write(path.with_extension("json.tmp"), b"stale").unwrap();
        let mut loaded = loaded;
        loaded.push("AT", MAX);
        loaded.save_to(&path).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        assert_eq!(SendHistory::load_from(&path).items()[0], "AT");

        loaded.clear();
        loaded.save_to(&path).unwrap();
        assert!(SendHistory::load_from(&path).items().is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_load_missing_and_corrupt() {
        let missing = temp_path("missing");
        assert!(SendHistory::load_from(&missing).items().is_empty());
        let _ = fs::remove_dir_all(missing.parent().unwrap());

        let corrupt = temp_path("corrupt");
        fs::write(&corrupt, b"[\"AT\",").unwrap();
        assert!(SendHistory::load_from(&corrupt).items().is_empty());
        assert!(!corrupt.exists());
        let bad = corrupt.with_extension("json.bad");
        assert!(bad.exists());
        assert_eq!(fs::read(&bad).unwrap(), b"[\"AT\",");
        let _ = fs::remove_dir_all(corrupt.parent().unwrap());
    }
}
