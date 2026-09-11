//! 存量数据的一次性迁移。启动时在 init_dirs() 之后、Settings::load() 之前跑一遍。
//!
//! 每步都是"只在检测到旧状态时动手",重复执行无害;失败一律降级不阻断启动
//! (最差情况是这一步下次启动再试,或者缓存重新拉一遍)。

use std::fs;
use std::path::Path;

/// 需要从配置目录搬到缓存目录的文件(可重建的缓存,不该跟着漫游配置同步)
const CACHE_FILES: [&str; 2] = ["command-index.json", "command-index.json.bad"];

pub fn run_once() {
    let config = super::config_dir();
    let local = super::local_dir();
    if config != local {
        move_cache_files(&config, &local);
    }
    let settings_path = config.join("settings.json");
    let cred_path = config.join(super::secret::CREDENTIAL_FILE);
    move_plaintext_api_key(&settings_path, &cred_path);
    sweep_webview_backups();
}

/// WebView2 改名留下的 profile 备份前缀。完整名形如 EBWebView.bak.1788165529(unix 秒)。
const WEBVIEW_BACKUP_PREFIX: &str = "EBWebView.bak.";
/// 多久以前的备份才删。留个窗口:刚产生的那份里有 WebView2 的崩溃转储,可能正要看。
const WEBVIEW_BACKUP_KEEP_SECS: u64 = 7 * 24 * 60 * 60;

/// 清理 WebView2 遗留的 profile 备份。
///
/// WebView2 运行时在 profile 版本回退/损坏时,会把整个 EBWebView 改名成
/// EBWebView.bak.<unix 秒> 再另起一个新的——但它从不回收这些备份。实测一份就 332 MB
/// (旧 HTTP/GPU 缓存 + 崩溃转储 + DevTools 扩展),而活的 profile 只有 36 MB。
///
/// 删掉是安全的:里面没有本应用的任何用户数据(设置/快捷指令/发送历史/凭据全在后端文件,
/// 前端一处 localStorage/IndexedDB 都没用),丢的只是 WebView2 自己可重建的缓存。
///
/// 便携/自定义模式下我们会显式指定 WEBVIEW2_USER_DATA_FOLDER,但用户从标准模式切过去
/// 之前的残留还躺在 %LOCALAPPDATA% 里,所以两处都扫。
fn sweep_webview_backups() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut seen: Vec<std::path::PathBuf> = Vec::new();
    for dir in [super::webview_default_parent(), super::webview2_dir()]
        .into_iter()
        .flatten()
    {
        if seen.contains(&dir) {
            continue;
        }
        seen.push(dir.clone());
        let (count, freed) = cleanup_webview_backups(&dir, now);
        if count > 0 {
            log::info!(
                "清理 WebView2 残留 profile {} 个, 释放 {:.1} MB ({})",
                count,
                freed as f64 / 1_048_576.0,
                dir.display()
            );
        }
    }
}

/// 扫 parent 下的 EBWebView.bak.<时间戳>,够旧就整个删掉。
/// 返回 (删掉几个, 释放多少字节)。now_secs 由调用方给,便于测试。
fn cleanup_webview_backups(parent: &Path, now_secs: u64) -> (usize, u64) {
    let Ok(entries) = fs::read_dir(parent) else {
        return (0, 0);
    };
    let mut count = 0usize;
    let mut freed = 0u64;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(suffix) = name.strip_prefix(WEBVIEW_BACKUP_PREFIX) else {
            continue;
        };
        // 后缀不是纯时间戳就不动:宁可留着,也别顺手删了不认识的目录
        let Ok(ts) = suffix.parse::<u64>() else { continue };
        if now_secs.saturating_sub(ts) < WEBVIEW_BACKUP_KEEP_SECS {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // 先量再删:remove_dir_all 之后就没法报"释放了多少"了。多走一遍目录树的代价
        // 只在真有残留时付一次(删完就没有了)。
        let size = dir_size(&path);
        match fs::remove_dir_all(&path) {
            Ok(()) => {
                count += 1;
                freed += size;
            }
            // 正被别的进程占着(比如另一个实例刚好在用)就留到下次
            Err(e) => log::warn!("清理 WebView2 残留 {} 失败: {}", name, e),
        }
    }
    (count, freed)
}

/// 目录树总字节数。只用于日志里报个数,取不到就算 0,不影响删除。
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(t) if t.is_dir() => total += dir_size(&entry.path()),
            Ok(t) if t.is_file() => total += entry.metadata().map(|m| m.len()).unwrap_or(0),
            _ => {}
        }
    }
    total
}

/// 知识库缓存从旧位置(配置目录)搬到缓存目录。目标已存在就不动旧的,
/// 搬不动(跨盘、占用)也只记一句——缓存丢了下次刷新自愈,不值得为它阻断启动。
fn move_cache_files(config: &Path, local: &Path) {
    for name in CACHE_FILES {
        let from = config.join(name);
        let to = local.join(name);
        if !from.exists() || to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // 跨盘 rename 会失败(便携目录在 D 盘、旧缓存在 C 盘),退回复制+删除
        let moved = fs::rename(&from, &to).is_ok()
            || (fs::copy(&from, &to).is_ok() && fs::remove_file(&from).is_ok());
        if moved {
            log::info!("已迁移 {} 到 {}", name, local.display());
        } else {
            log::warn!("迁移 {} 失败,保留在 {}(下次刷新会在新位置重建)", name, config.display());
        }
    }
}

/// 旧版把知识库 API Key 明文存在 settings.json 的 command_index.api_key 里。
/// 搬进 DPAPI 加密的凭据文件,再重写 settings.json 把明文从磁盘上抹掉
/// (不能等下一次正常保存——那之前明文会一直躺在盘上)。
///
/// 值恰好等于编译期注入的内置 Key 时不必存(它本来就是注入值被物化出来的),
/// 但仍要重写文件把这个键去掉。存不进凭据文件则整步放弃:宁可留着明文,
/// 也不能把用户唯一的 Key 弄丢。
fn move_plaintext_api_key(settings_path: &Path, cred_path: &Path) {
    let text = match fs::read_to_string(settings_path) {
        Ok(t) => t,
        Err(_) => return, // 全新安装,没有存量
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return, // 坏文件交给 Settings::load_from 处理(改名 .bad + 默认值)
    };
    let legacy = match v.pointer("/command_index/api_key").and_then(|k| k.as_str()) {
        Some(k) => k.trim().to_string(),
        None => return, // 已经是新格式
    };

    if !legacy.is_empty() && legacy != super::secret::builtin_api_key() {
        if let Err(e) = super::secret::set_user_key_at(cred_path, &legacy) {
            log::warn!("迁移知识库 Key 到凭据文件失败,settings.json 暂不重写: {}", e);
            return;
        }
        log::info!("知识库 Key 已迁移到凭据文件(DPAPI 加密)");
    }

    // 重写前留一份备份:这是 schema 变更,出问题时用户还能自己捞回来
    let backup = settings_path.with_extension("json.bak.pre-secret");
    if !backup.exists() {
        let _ = fs::copy(settings_path, &backup);
    }
    // load_from + save_to:新 schema 里没有 api_key 字段,序列化时自然消失
    let s = super::settings::Settings::load_from(settings_path);
    if let Err(e) = s.save_to(settings_path) {
        log::warn!("重写 settings.json(移除明文 Key)失败: {}", e);
    } else {
        log::info!("settings.json 已重写:不再包含明文 API Key");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::{read_user_key_at, UserKey};

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("neoserial_mig_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_move_cache_files_moves_and_skips_existing() {
        let dir = tmp_dir("cache");
        let (config, local) = (dir.join("roaming"), dir.join("local"));
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("command-index.json"), b"old").unwrap();
        move_cache_files(&config, &local);
        assert_eq!(fs::read(local.join("command-index.json")).unwrap(), b"old");
        assert!(!config.join("command-index.json").exists(), "旧位置应已清掉");

        // 新位置已有文件:不覆盖,也不删旧的
        fs::write(config.join("command-index.json"), b"stale").unwrap();
        move_cache_files(&config, &local);
        assert_eq!(fs::read(local.join("command-index.json")).unwrap(), b"old");
        assert!(config.join("command-index.json").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    /// 够旧的 .bak profile 删掉,新的留着,活的 EBWebView 和认不出的名字一概不碰。
    #[test]
    fn test_cleanup_webview_backups_only_removes_old_backups() {
        let dir = tmp_dir("ebwv");
        let now = 1_800_000_000u64;
        let old = now - WEBVIEW_BACKUP_KEEP_SECS - 1;
        let fresh = now - 60;

        for name in [
            format!("{}{}", WEBVIEW_BACKUP_PREFIX, old),
            format!("{}{}", WEBVIEW_BACKUP_PREFIX, fresh),
            // 后缀不是时间戳:不认识的东西不删
            format!("{}manual", WEBVIEW_BACKUP_PREFIX),
            "EBWebView".to_string(),
        ] {
            fs::create_dir_all(dir.join(&name).join("Default")).unwrap();
            fs::write(dir.join(&name).join("Default").join("f"), b"0123456789").unwrap();
        }

        let (count, freed) = cleanup_webview_backups(&dir, now);
        assert_eq!(count, 1, "只该删那一个够旧的");
        assert_eq!(freed, 10, "释放字节数按目录树实际大小报");
        assert!(!dir.join(format!("{}{}", WEBVIEW_BACKUP_PREFIX, old)).exists());
        assert!(dir.join(format!("{}{}", WEBVIEW_BACKUP_PREFIX, fresh)).exists(), "新备份留着");
        assert!(dir.join(format!("{}manual", WEBVIEW_BACKUP_PREFIX)).exists(), "非时间戳后缀不碰");
        assert!(dir.join("EBWebView").exists(), "活的 profile 绝不能动");

        // 重复跑无害(已经没得删了)
        assert_eq!(cleanup_webview_backups(&dir, now), (0, 0));
        let _ = fs::remove_dir_all(&dir);
    }

    /// 目录不存在(没装过 WebView2 / 便携模式还没起过窗口)不该报错
    #[test]
    fn test_cleanup_webview_backups_missing_dir_is_noop() {
        let dir = tmp_dir("ebwv_none").join("not-there");
        assert_eq!(cleanup_webview_backups(&dir, 1_800_000_000), (0, 0));
    }

    /// 用户手填过的明文 Key:进凭据文件、从 settings.json 消失、留备份,其他设置项不受影响。
    #[test]
    #[cfg(windows)]
    fn test_plaintext_key_moves_into_credential_file() {
        let dir = tmp_dir("key");
        let settings = dir.join("settings.json");
        let cred = dir.join("kb-credential.dat");

        let mut v = serde_json::to_value(crate::config::settings::Settings::default_settings()).unwrap();
        v["command_index"]["api_key"] = serde_json::json!("kb_user_typed");
        v["command_index"]["base_url"] = serde_json::json!("http://h:8200");
        v["command_index"]["suggest_min_chars"] = serde_json::json!(4);
        fs::write(&settings, serde_json::to_string_pretty(&v).unwrap()).unwrap();

        move_plaintext_api_key(&settings, &cred);

        match read_user_key_at(&cred) {
            UserKey::Ok(k) => assert_eq!(k, "kb_user_typed"),
            _ => panic!("Key 应已进凭据文件"),
        }
        let rewritten = fs::read_to_string(&settings).unwrap();
        assert!(!rewritten.contains("api_key"), "明文键应从文件里消失");
        assert!(!rewritten.contains("kb_user_typed"));
        assert!(settings.with_extension("json.bak.pre-secret").exists(), "应留备份");
        let s = crate::config::settings::Settings::load_from(&settings);
        assert_eq!(s.command_index.base_url, "http://h:8200", "其他设置项原样保留");
        assert_eq!(s.command_index.suggest_min_chars, 4);

        let _ = fs::remove_dir_all(&dir);
    }

    /// 已经是新格式(没有 api_key 键)→ 什么都不做,不留无谓的备份、不改文件
    #[test]
    fn test_no_legacy_key_leaves_file_untouched() {
        let dir = tmp_dir("clean");
        let settings = dir.join("settings.json");
        let cred = dir.join("kb-credential.dat");
        let text = serde_json::to_string_pretty(&crate::config::settings::Settings::default_settings()).unwrap();
        fs::write(&settings, &text).unwrap();

        move_plaintext_api_key(&settings, &cred);
        assert_eq!(fs::read_to_string(&settings).unwrap(), text);
        assert!(!settings.with_extension("json.bak.pre-secret").exists());
        assert!(!cred.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    /// 值就是内置注入值时不必存凭据,但键要去掉(否则内网地址/Key 一直留在用户文件里)
    #[test]
    fn test_legacy_key_equal_to_builtin_is_dropped_without_credential() {
        let builtin = crate::config::secret::builtin_api_key();
        if builtin.is_empty() {
            return; // 本次构建没注入内置 Key,该分支无意义
        }
        let dir = tmp_dir("builtin");
        let settings = dir.join("settings.json");
        let cred = dir.join("kb-credential.dat");
        let mut v = serde_json::to_value(crate::config::settings::Settings::default_settings()).unwrap();
        v["command_index"]["api_key"] = serde_json::json!(builtin);
        fs::write(&settings, serde_json::to_string_pretty(&v).unwrap()).unwrap();

        move_plaintext_api_key(&settings, &cred);
        assert!(!cred.exists(), "内置值不必写进凭据文件");
        assert!(!fs::read_to_string(&settings).unwrap().contains("api_key"));

        let _ = fs::remove_dir_all(&dir);
    }
}
