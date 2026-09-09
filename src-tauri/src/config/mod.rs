pub mod settings;
pub mod command_group;
pub mod command_index;
pub mod migrate;
pub mod secret;
pub mod send_history;

use std::path::PathBuf;
use std::sync::OnceLock;

/// 便携模式标记文件:放在 exe 同目录即生效(内容无所谓,空文件即可)。
const PORTABLE_MARKER: &str = "neoserial.portable";

/// 数据目录来源。UI 显示用,也决定 WebView2 的数据目录跟不跟着走。
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DirMode {
    /// 系统标准位置:配置在 Roaming、缓存在 Local
    Standard,
    /// exe 同目录有 neoserial.portable 标记:全部落在 exe 旁边
    Portable,
    /// NEOSERIAL_DATA_DIR 指定
    Custom,
}

struct Dirs {
    /// 配置与用户数据:settings.json / sequence.json / send-history.json / 凭据
    config: PathBuf,
    /// 缓存与日志:command-index.json / 默认日志目录。可重建或体积大的东西,不该跟着漫游配置同步
    local: PathBuf,
    mode: DirMode,
}

static DIRS: OnceLock<Dirs> = OnceLock::new();

fn dirs() -> &'static Dirs {
    DIRS.get_or_init(resolve_dirs)
}

/// 解析目录并记一条日志。启动时调一次;之后各处直接用 config_dir/local_dir。
/// 允许被 webview2_dir() 抢先触发解析(main.rs 设环境变量时),这里照样把结果记下来。
pub fn init_dirs() {
    let d = dirs();
    log::info!(
        "数据目录[{:?}]: 配置={} 缓存={}",
        d.mode,
        d.config.display(),
        d.local.display()
    );
}

/// 配置与用户数据目录。默认 %APPDATA%\neoserial。
pub(crate) fn config_dir() -> PathBuf {
    dirs().config.clone()
}

/// 缓存与日志目录。默认 %LOCALAPPDATA%\neoserial(取不到则退回配置目录)。
pub(crate) fn local_dir() -> PathBuf {
    dirs().local.clone()
}

pub fn dir_mode() -> DirMode {
    dirs().mode
}

/// 非标准模式下 WebView2 的数据目录:跟着数据走,便携版不往系统盘写这几十 MB。
/// 标准模式返回 None(用系统默认位置)。
pub fn webview2_dir() -> Option<PathBuf> {
    let d = dirs();
    match d.mode {
        DirMode::Standard => None,
        _ => Some(d.config.join("webview2")),
    }
}

/// 优先级:NEOSERIAL_DATA_DIR > exe 同目录的便携标记 > 系统标准位置。
fn resolve_dirs() -> Dirs {
    if let Some(root) = env_path("NEOSERIAL_DATA_DIR") {
        return single_root(root, DirMode::Custom);
    }
    if let Some(dir) = exe_dir() {
        if dir.join(PORTABLE_MARKER).exists() {
            return single_root(dir.join("data"), DirMode::Portable);
        }
    }
    // 取不到环境变量时退到当前目录,与历史行为一致(不 panic、不静默换盘)
    let config = env_path("APPDATA")
        .unwrap_or_else(|| PathBuf::from("."))
        .join("neoserial");
    let local = env_path("LOCALAPPDATA")
        .map(|p| p.join("neoserial"))
        .unwrap_or_else(|| config.clone());
    Dirs { config, local, mode: DirMode::Standard }
}

/// 便携/自定义模式:一个根目录装下全部,缓存进 cache 子目录(便于整体删除)。
fn single_root(root: PathBuf, mode: DirMode) -> Dirs {
    Dirs { local: root.join("cache"), config: root, mode }
}

/// 读环境变量取路径。空串按"没设"处理——CI/批处理里 `set VAR=` 很常见。
fn env_path(key: &str) -> Option<PathBuf> {
    let v = std::env::var(key).ok()?;
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    Some(PathBuf::from(v))
}

fn exe_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三种模式的目录布局。DIRS 是进程级 OnceLock、环境变量也是进程级,
    /// 所以测纯函数 single_root/env_path,不碰全局状态(否则与其他测试并行时互相污染)。
    #[test]
    fn test_single_root_puts_cache_under_root() {
        let d = single_root(PathBuf::from("D:\\ns"), DirMode::Custom);
        assert_eq!(d.config, PathBuf::from("D:\\ns"));
        assert_eq!(d.local, PathBuf::from("D:\\ns").join("cache"));
        assert_eq!(d.mode, DirMode::Custom);
    }

    #[test]
    fn test_env_path_treats_blank_as_unset() {
        std::env::set_var("NEOSERIAL_TEST_BLANK", "   ");
        assert!(env_path("NEOSERIAL_TEST_BLANK").is_none());
        std::env::set_var("NEOSERIAL_TEST_BLANK", " D:\\x ");
        assert_eq!(env_path("NEOSERIAL_TEST_BLANK"), Some(PathBuf::from("D:\\x")));
        std::env::remove_var("NEOSERIAL_TEST_BLANK");
        assert!(env_path("NEOSERIAL_TEST_BLANK").is_none());
    }
}
