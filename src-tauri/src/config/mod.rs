pub mod settings;
pub mod command_group;
pub mod command_index;
pub mod send_history;

use std::path::PathBuf;

/// 应用配置目录 %APPDATA%/neoserial(settings.json / command-index.json / send-history.json 都在这里)。
/// 取不到 APPDATA 时退到当前目录,与历史行为一致。
pub(crate) fn config_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("neoserial")
}
