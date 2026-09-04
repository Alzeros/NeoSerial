//! 手册指令索引本地缓存:知识库接口按手册拉下来的 AT 指令,落在
//! %APPDATA%/neoserial/command-index.json,前端启动时整份载入做本地前缀匹配。
//! 字段与接口(GET /api/v1/commands/documents、GET /api/v1/commands)原样对齐。
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 手册列表里的一本手册。只有 cmd_status == "done" 的手册有指令。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManualDocument {
    pub id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub status: String,
    /// "" 未提取 / running 提取中 / done 已提取 / failed 失败
    #[serde(default)]
    pub cmd_status: String,
    #[serde(default)]
    pub cmd_count: i64,
    #[serde(default)]
    pub category_id: i64,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandParameter {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: String,
}

/// 一条手册指令。接口还带 created_at,不保留。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManualCommand {
    pub id: i64,
    pub document_id: i64,
    pub command: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub syntax: String,
    #[serde(default)]
    pub parameters: Vec<CommandParameter>,
    #[serde(default)]
    pub example: String,
    #[serde(default)]
    pub page_no: Option<i64>,
    #[serde(default)]
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandIndexCache {
    /// 上次成功刷新的时间(RFC 3339 本地时区);None = 从未刷新
    #[serde(default)]
    pub fetched_at: Option<String>,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub documents: Vec<ManualDocument>,
    #[serde(default)]
    pub commands: Vec<ManualCommand>,
}

impl CommandIndexCache {
    fn cache_path() -> PathBuf {
        let appdata = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        appdata.join("neoserial").join("command-index.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::cache_path())
    }

    /// 文件不存在 → 空缓存;损坏 → 改名 .bad + 空缓存(下次刷新重建)。
    pub(crate) fn load_from(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(c) => c,
            Err(_) => {
                let _ = fs::rename(path, path.with_extension("json.bad"));
                Self::default()
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&Self::cache_path())
    }

    /// 先写 .tmp 再改名覆盖:中途崩溃不会留下半个 JSON。
    pub(crate) fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)
    }
}

/// 把一次刷新的结果拼成新缓存。
/// - `documents`:本次拉到的手册列表,顺序保留(前端"同名指令主记录取排前面那本"以此为准)
/// - `fetched`:成功拉到的手册指令,按 document_id
/// - `failed_ids`:拉失败的手册,沿用 `old` 里这本的指令(旧缓存没有就没有)
/// 不在 `documents` 里的旧指令一律丢弃(手册已删)。
pub fn merge_fetched(
    documents: Vec<ManualDocument>,
    mut fetched: HashMap<i64, Vec<ManualCommand>>,
    failed_ids: &[i64],
    old: &CommandIndexCache,
    base_url: &str,
    fetched_at: String,
) -> CommandIndexCache {
    let mut commands = Vec::new();
    for d in &documents {
        if let Some(items) = fetched.remove(&d.id) {
            commands.extend(items);
        } else if failed_ids.contains(&d.id) {
            commands.extend(old.commands.iter().filter(|c| c.document_id == d.id).cloned());
        }
    }
    CommandIndexCache {
        fetched_at: Some(fetched_at),
        base_url: base_url.to_string(),
        documents,
        commands,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: i64, title: &str, cmd_status: &str) -> ManualDocument {
        ManualDocument {
            id,
            title: title.into(),
            filename: String::new(),
            status: "done".into(),
            cmd_status: cmd_status.into(),
            cmd_count: 0,
            category_id: 0,
            updated_at: String::new(),
        }
    }

    fn cmd(id: i64, document_id: i64, command: &str) -> ManualCommand {
        ManualCommand {
            id,
            document_id,
            command: command.into(),
            name: String::new(),
            syntax: String::new(),
            parameters: vec![],
            example: String::new(),
            page_no: None,
            summary: String::new(),
        }
    }

    /// 每个测试独立目录,互不干扰;返回缓存文件路径
    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("neoserial_cidx_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("command-index.json")
    }

    /// 接口原样 JSON(含不保留的 created_at、page_no 为 null)能解析;缺字段取默认。
    #[test]
    fn test_manual_command_parses_api_json() {
        let json = r#"{"id":2,"document_id":27,"command":"AT+MHTTPCREATE","name":"创建HTTP实例",
            "syntax":"AT+MHTTPCREATE=<host>",
            "parameters":[{"name":"host","required":true,"description":"服务器域名"}],
            "example":"AT+MHTTPCREATE=\"http://a\"\n+MHTTPCREATE: 0","page_no":null,"summary":"创建实例",
            "created_at":"2026-09-03T09:22:58"}"#;
        let c: ManualCommand = serde_json::from_str(json).unwrap();
        assert_eq!(c.command, "AT+MHTTPCREATE");
        assert_eq!(c.parameters.len(), 1);
        assert!(c.parameters[0].required);
        assert_eq!(c.page_no, None);

        let minimal: ManualCommand =
            serde_json::from_str(r#"{"id":1,"document_id":1,"command":"AT"}"#).unwrap();
        assert_eq!(minimal.name, "");
        assert!(minimal.parameters.is_empty());
        assert_eq!(minimal.page_no, None);
    }

    #[test]
    fn test_merge_keeps_document_order_and_reuses_old_for_failed() {
        let old = CommandIndexCache {
            fetched_at: None,
            base_url: String::new(),
            documents: vec![doc(3, "MQTT", "done")],
            commands: vec![cmd(1, 3, "AT+MQTTCFG"), cmd(2, 3, "AT+MQTTCONN"), cmd(9, 99, "AT+GONE")],
        };
        let docs = vec![doc(4, "LwM2M", "done"), doc(3, "MQTT", "done"), doc(2, "SSL", "running")];
        let mut fetched = HashMap::new();
        fetched.insert(4, vec![cmd(17, 4, "AT+MIPLCREATE")]);

        let merged = merge_fetched(docs, fetched, &[3], &old, "http://h:8200", "2026-09-03T10:00:00+08:00".into());

        let names: Vec<&str> = merged.commands.iter().map(|c| c.command.as_str()).collect();
        assert_eq!(
            names,
            vec!["AT+MIPLCREATE", "AT+MQTTCFG", "AT+MQTTCONN"],
            "按手册顺序排;失败手册沿用旧缓存;旧缓存里已不存在的手册(99)丢弃;running 的手册没有指令"
        );
        assert_eq!(merged.documents.len(), 3);
        assert_eq!(merged.fetched_at.as_deref(), Some("2026-09-03T10:00:00+08:00"));
        assert_eq!(merged.base_url, "http://h:8200");
    }

    #[test]
    fn test_merge_failed_doc_without_old_yields_nothing() {
        let merged = merge_fetched(
            vec![doc(4, "LwM2M", "done")],
            HashMap::new(),
            &[4],
            &CommandIndexCache::default(),
            "",
            "t".into(),
        );
        assert!(merged.commands.is_empty());
        assert_eq!(merged.documents.len(), 1);
    }

    #[test]
    fn test_save_then_load_roundtrip_and_no_tmp_left() {
        let path = temp_path("roundtrip");
        let cache = CommandIndexCache {
            fetched_at: Some("t".into()),
            base_url: "http://h".into(),
            documents: vec![doc(1, "A", "done")],
            commands: vec![cmd(1, 1, "AT+CSQ")],
        };
        cache.save_to(&path).unwrap();
        assert!(!path.with_extension("json.tmp").exists(), "临时文件应已改名为正式文件");
        let loaded = CommandIndexCache::load_from(&path);
        assert_eq!(loaded, cache);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_load_missing_returns_empty() {
        let path = temp_path("missing");
        assert_eq!(CommandIndexCache::load_from(&path), CommandIndexCache::default());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_load_corrupt_renames_bad_and_returns_empty() {
        let path = temp_path("corrupt");
        fs::write(&path, b"{not json").unwrap();
        assert_eq!(CommandIndexCache::load_from(&path), CommandIndexCache::default());
        assert!(!path.exists(), "损坏文件应被改名");
        assert!(path.with_extension("json.bad").exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
