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
