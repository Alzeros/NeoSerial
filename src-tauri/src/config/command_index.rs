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
    /// 所属自定义分组名列表(管理员在知识库 Web 端按手册归的组)。无分组 → 空;
    /// 旧缓存里没这个键也是空,设置页据此退回平铺显示。
    /// 接口还带归组键 manual_name,我们直接用这里的分组名,不保留。
    #[serde(default)]
    pub group_names: Vec<String>,
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
        crate::config::config_dir().join("command-index.json")
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
    /// 临时文件名固定,同一时刻只能有一个写入者:唯一写入方是 commands::command_index 的刷新流程,由其 REFRESHING 标志串行化。
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
/// - `keep_old_ids`:不重拉、沿用 `old` 里这本指令的手册(拉失败的、增量刷新判定无变化的、
///   单本刷新时的其余各本);旧缓存里没有就是没有
/// - 既不在 `fetched` 也不在 `keep_old_ids` 的手册视为无指令(旧指令丢弃)。调用方要么拉它,
///   要么把它放进 `keep_old_ids`——漏了就等于清空这本。注意别为省配额跳过 disabled_doc_ids 里的:
///   那是前端显示层过滤,不是拉取过滤(跳过也要放进 keep_old_ids)。
/// - `fetched_at`:全量刷新传 Some(现在);单本刷新传旧值原样,免得"上次更新"变成谎话。
/// 不在 `documents` 里的旧指令一律丢弃(手册已删)。
pub fn merge_fetched(
    documents: Vec<ManualDocument>,
    mut fetched: HashMap<i64, Vec<ManualCommand>>,
    keep_old_ids: &[i64],
    old: &CommandIndexCache,
    base_url: &str,
    fetched_at: Option<String>,
) -> CommandIndexCache {
    let mut commands = Vec::new();
    for d in &documents {
        if let Some(items) = fetched.remove(&d.id) {
            commands.extend(items);
        } else if keep_old_ids.contains(&d.id) {
            commands.extend(old.commands.iter().filter(|c| c.document_id == d.id).cloned());
        }
    }
    CommandIndexCache {
        fetched_at,
        base_url: base_url.to_string(),
        documents,
        commands,
    }
}

/// 本地缓存里每本手册各有多少条指令(按 document_id 计数)。
/// 与手册列表报的 `cmd_count` 对比即知"这本同步过没有";前端也用它显示每行的实际条数。
pub fn cached_counts(cache: &CommandIndexCache) -> HashMap<i64, usize> {
    let mut m: HashMap<i64, usize> = HashMap::new();
    for c in &cache.commands {
        *m.entry(c.document_id).or_insert(0) += 1;
    }
    m
}

/// 全量刷新时挑出"可以沿用旧缓存、不必重拉"的手册 id(增量刷新)。
/// 判据(全部满足才跳过):
/// - 缓存来自同一个知识库地址——换了服务器,旧数据一律作废
/// - 该手册 `cmd_status == "done"`(其他状态本就没有指令可拉)
/// - 手册列表报的 `updated_at` 非空且与缓存里这本的完全一致
/// - 缓存里这本的指令条数与列表报的 `cmd_count` 相等
///
/// 最后一条是关键:上次拉这本失败时,缓存里的**手册条目**是新的(documents 整份换),
/// **指令**却是空的。只看 updated_at 会认为"没变、跳过",这本就永远空着。
/// 条数不符一律重拉,方向上偏保守(顶多退化成旧的每次全拉)。
pub fn reusable_doc_ids(
    documents: &[ManualDocument],
    old: &CommandIndexCache,
    base_url: &str,
) -> Vec<i64> {
    if old.base_url != base_url {
        return Vec::new();
    }
    let counts = cached_counts(old);
    let old_docs: HashMap<i64, &ManualDocument> = old.documents.iter().map(|d| (d.id, d)).collect();
    documents
        .iter()
        .filter(|d| {
            if d.cmd_status != "done" || d.updated_at.is_empty() {
                return false;
            }
            let Some(prev) = old_docs.get(&d.id) else { return false };
            prev.updated_at == d.updated_at
                && counts.get(&d.id).copied().unwrap_or(0) as i64 == d.cmd_count
        })
        .map(|d| d.id)
        .collect()
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
            group_names: vec![],
        }
    }

    /// 带 updated_at 与 cmd_count 的手册(增量判定用)
    fn doc_at(id: i64, updated_at: &str, cmd_count: i64) -> ManualDocument {
        ManualDocument {
            cmd_count,
            updated_at: updated_at.into(),
            ..doc(id, "T", "done")
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

    /// 手册列表接口(v2)的分组字段能读到;不认识的键(manual_name)忽略;
    /// 旧缓存/旧接口没有 group_names 时取空表(设置页据此退回平铺)。
    #[test]
    fn test_manual_document_parses_group_names() {
        let json = r#"{"id":27,"title":"HTTP-HTTPS用户手册","filename":"a.pdf","status":"done",
            "cmd_status":"done","cmd_count":9,"category_id":3,"updated_at":"2026-09-03T09:22:58",
            "manual_name":"HTTP-HTTPS用户手册","group_names":["网络类手册","常用"]}"#;
        let d: ManualDocument = serde_json::from_str(json).unwrap();
        assert_eq!(d.group_names, vec!["网络类手册", "常用"]);

        let old: ManualDocument = serde_json::from_str(r#"{"id":1,"title":"A"}"#).unwrap();
        assert!(old.group_names.is_empty(), "缺 group_names 取空表,不是解析失败");
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

        let merged = merge_fetched(docs, fetched, &[3], &old, "http://h:8200", Some("2026-09-03T10:00:00+08:00".into()));

        let names: Vec<&str> = merged.commands.iter().map(|c| c.command.as_str()).collect();
        assert_eq!(
            names,
            vec!["AT+MIPLCREATE", "AT+MQTTCFG", "AT+MQTTCONN"],
            "按手册顺序排;沿用旧缓存的手册(3)保留旧指令;旧缓存里已不存在的手册(99)丢弃;running 的手册没有指令"
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
            Some("t".into()),
        );
        assert!(merged.commands.is_empty());
        assert_eq!(merged.documents.len(), 1);
    }

    /// 单本刷新:只有被点的那本重拉,其余各本进 keep_old 原样保留;fetched_at 传旧值不动。
    #[test]
    fn test_merge_single_doc_keeps_others() {
        let old = CommandIndexCache {
            fetched_at: Some("2026-09-01T00:00:00+08:00".into()),
            base_url: "http://h:8200".into(),
            documents: vec![doc(1, "A", "done"), doc(2, "B", "done")],
            commands: vec![cmd(1, 1, "AT+A1"), cmd(2, 2, "AT+B1"), cmd(3, 2, "AT+B2")],
        };
        let mut fetched = HashMap::new();
        fetched.insert(2, vec![cmd(9, 2, "AT+B1NEW")]);
        let merged = merge_fetched(
            vec![doc(1, "A", "done"), doc(2, "B", "done")],
            fetched,
            &[1], // 除被刷的 2 以外都沿用
            &old,
            "http://h:8200",
            old.fetched_at.clone(),
        );
        let names: Vec<&str> = merged.commands.iter().map(|c| c.command.as_str()).collect();
        assert_eq!(names, vec!["AT+A1", "AT+B1NEW"], "1 沿用旧指令,2 换成新拉的");
        assert_eq!(merged.fetched_at.as_deref(), Some("2026-09-01T00:00:00+08:00"), "单本刷新不动全量刷新时间");
    }

    /// 增量刷新:updated_at 与条数都对得上才跳过重拉。
    #[test]
    fn test_reusable_doc_ids() {
        let old = CommandIndexCache {
            fetched_at: Some("t".into()),
            base_url: "http://h:8200".into(),
            documents: vec![
                doc_at(1, "2026-09-07T08:00:00", 2), // 未变,2 条都在 → 可跳过
                doc_at(2, "2026-09-07T08:00:00", 1), // updated_at 会变 → 要重拉
                doc_at(3, "2026-09-07T08:00:00", 5), // 上次拉失败,缓存里 0 条 → 要重拉
                doc_at(4, "2026-09-07T08:00:00", 0), // 服务器就是 0 条 → 可跳过
            ],
            commands: vec![cmd(1, 1, "AT+A1"), cmd(2, 1, "AT+A2"), cmd(3, 2, "AT+B1")],
        };
        let fresh = vec![
            doc_at(1, "2026-09-07T08:00:00", 2),
            doc_at(2, "2026-09-08T01:40:00", 1),
            doc_at(3, "2026-09-07T08:00:00", 5),
            doc_at(4, "2026-09-07T08:00:00", 0),
            doc_at(5, "2026-09-07T09:00:00", 3), // 新手册,缓存里没有 → 要拉
        ];
        assert_eq!(reusable_doc_ids(&fresh, &old, "http://h:8200"), vec![1, 4]);
        // 换了知识库地址:旧数据一律作废,全部重拉
        assert!(reusable_doc_ids(&fresh, &old, "http://other:8200").is_empty());
        // updated_at 为空(接口没给)无法判断,不跳过
        let mut no_ts = old.clone();
        no_ts.documents[0].updated_at = String::new();
        assert!(!reusable_doc_ids(&[doc_at(1, "", 2)], &no_ts, "http://h:8200").contains(&1));
        // 非 done 状态没有指令可拉,也不算"可沿用"(由调用方放进 keep_old)
        let running = vec![ManualDocument { cmd_status: "running".into(), ..doc_at(1, "2026-09-07T08:00:00", 2) }];
        assert!(reusable_doc_ids(&running, &old, "http://h:8200").is_empty());
    }

    #[test]
    fn test_cached_counts() {
        let cache = CommandIndexCache {
            commands: vec![cmd(1, 7, "AT+A"), cmd(2, 7, "AT+B"), cmd(3, 9, "AT+C")],
            ..Default::default()
        };
        let m = cached_counts(&cache);
        assert_eq!(m.get(&7), Some(&2));
        assert_eq!(m.get(&9), Some(&1));
        assert_eq!(m.get(&11), None);
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

        // 覆盖写:目录里留着一份陈旧的 .tmp(比如上次写到一半就被杀掉的进程遗留下的),
        // 生产路径里第二次刷新应正常覆盖为新内容,不受旧 .tmp 干扰,写完也不留 .tmp。
        fs::write(path.with_extension("json.tmp"), b"stale").unwrap();
        let second = CommandIndexCache {
            fetched_at: Some("t2".into()),
            base_url: "http://h".into(),
            documents: vec![doc(1, "A", "done")],
            commands: vec![cmd(2, 1, "AT+CGDCONT")],
        };
        second.save_to(&path).unwrap();
        assert!(!path.with_extension("json.tmp").exists(), "残留的旧 .tmp 应被新一次 save_to 覆盖掉");
        assert_eq!(CommandIndexCache::load_from(&path), second);

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
        let bad_path = path.with_extension("json.bad");
        assert!(bad_path.exists());
        let bad_content = fs::read_to_string(&bad_path).unwrap();
        assert_eq!(bad_content, "{not json");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
