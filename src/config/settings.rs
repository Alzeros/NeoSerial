use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::util::codec::LineEnding;

const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DisplayMode {
    Ascii,
    Hex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum StopBits {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FlowControl {
    None,
    Software,
    Hardware,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowSettings {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerialDefaults {
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow_control: FlowControl,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiSettings {
    pub display_mode: DisplayMode,
    pub line_ending: LineEnding,
    pub auto_scroll: bool,
    pub ring_buffer_capacity: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuickCommand {
    pub label: String,
    pub content: String,
    pub mode: DisplayMode,
    pub line_ending: LineEnding,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub window: WindowSettings,
    pub serial_defaults: SerialDefaults,
    pub last_port: String,
    pub ui: UiSettings,
    pub quick_commands: Vec<QuickCommand>,
    pub error_keywords: Vec<String>,
}

impl Settings {
    pub fn default_settings() -> Self {
        Settings {
            version: CONFIG_VERSION,
            window: WindowSettings { width: 1100, height: 720, x: 100, y: 80 },
            serial_defaults: SerialDefaults {
                baud_rate: 115200,
                data_bits: DataBits::Eight,
                parity: Parity::None,
                stop_bits: StopBits::One,
                flow_control: FlowControl::None,
            },
            last_port: String::new(),
            ui: UiSettings {
                display_mode: DisplayMode::Ascii,
                line_ending: LineEnding::Crlf,
                auto_scroll: true,
                ring_buffer_capacity: 5000,
            },
            quick_commands: vec![
                QuickCommand {
                    label: "AT".into(),
                    content: "AT".into(),
                    mode: DisplayMode::Ascii,
                    line_ending: LineEnding::Crlf,
                },
                QuickCommand {
                    label: "CSQ".into(),
                    content: "AT+CSQ".into(),
                    mode: DisplayMode::Ascii,
                    line_ending: LineEnding::Crlf,
                },
                QuickCommand {
                    label: "Reset".into(),
                    content: "AT+CFUN=1,1".into(),
                    mode: DisplayMode::Ascii,
                    line_ending: LineEnding::Crlf,
                },
                QuickCommand {
                    label: "HEX5A".into(),
                    content: "5A A5 01 02".into(),
                    mode: DisplayMode::Hex,
                    line_ending: LineEnding::None,
                },
            ],
            error_keywords: vec![
                "ERROR".into(),
                "FAIL".into(),
                "+CME ERROR".into(),
                "+CMS ERROR".into(),
            ],
        }
    }

    fn config_path() -> PathBuf {
        let appdata = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        appdata.join("neoserial").join("settings.json")
    }

    /// 加载配置。文件不存在/损坏 → 用默认配置并静默降级；
    /// 损坏时把坏文件备份为 settings.json.bad。
    pub fn load() -> Self {
        Self::load_from(&Self::config_path())
    }

    /// 从指定路径加载配置。文件不存在 → 默认；损坏 → 备份为 `.bad` + 默认。
    /// 测试用（传入 temp 路径隔离，不依赖 APPDATA 环境变量）。
    pub(crate) fn load_from(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => s,
                Err(_) => {
                    let _ = fs::rename(path, path.with_extension("json.bad"));
                    Self::default_settings()
                }
            },
            Err(_) => Self::default_settings(),
        }
    }

    /// 保存配置。目录不存在会创建。
    pub fn save(&self) -> io::Result<()> {
        self.save_to(&Self::config_path())
    }

    /// 保存配置到指定路径。目录不存在会创建。
    /// 测试用（传入 temp 路径隔离，不依赖 APPDATA 环境变量）。
    pub(crate) fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_has_version() {
        let s = Settings::default_settings();
        assert_eq!(s.version, 1);
        assert_eq!(s.serial_defaults.baud_rate, 115200);
        assert_eq!(s.ui.ring_buffer_capacity, 5000);
    }

    #[test]
    fn test_default_quick_commands() {
        let s = Settings::default_settings();
        assert!(!s.quick_commands.is_empty());
        assert_eq!(s.quick_commands[0].label, "AT");
    }

    #[test]
    fn test_serde_roundtrip() {
        let s = Settings::default_settings();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, s.version);
        assert_eq!(back.serial_defaults.baud_rate, s.serial_defaults.baud_rate);
        assert_eq!(back.quick_commands.len(), s.quick_commands.len());
    }

    #[test]
    fn test_display_mode_serde() {
        let json = serde_json::to_string(&DisplayMode::Hex).unwrap();
        assert_eq!(json, "\"Hex\"");
    }

    #[test]
    fn test_line_ending_in_settings() {
        let s = Settings::default_settings();
        assert_eq!(s.ui.line_ending, LineEnding::Crlf);
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        // 用 temp 目录隔离，不碰真实 APPDATA
        let dir = std::env::temp_dir().join(format!(
            "neoserial_test_{}_missing",
            std::process::id()
        ));
        let path = dir.join("settings.json");
        // 不创建文件 → load_from 应返回 default
        let loaded = Settings::load_from(&path);
        let def = Settings::default_settings();
        assert_eq!(loaded.version, def.version);
        assert_eq!(loaded.serial_defaults.baud_rate, def.serial_defaults.baud_rate);
        assert_eq!(loaded.quick_commands.len(), def.quick_commands.len());
    }

    #[test]
    fn test_load_corrupt_file_backed_up_and_default() {
        let dir = std::env::temp_dir().join(format!(
            "neoserial_test_{}_corrupt",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        // 写一个损坏的 JSON
        std::fs::write(&path, b"{invalid json").unwrap();

        let loaded = Settings::load_from(&path);
        let def = Settings::default_settings();
        assert_eq!(loaded.version, def.version);
        assert_eq!(
            loaded.serial_defaults.baud_rate,
            def.serial_defaults.baud_rate
        );

        // 原文件应被重命名为 settings.json.bad
        assert!(!path.exists(), "原损坏文件应已被重命名");
        let bad_path = path.with_extension("json.bad");
        assert!(bad_path.exists(), "损坏文件应被备份为 .bad");
        // .bad 内容应与原损坏内容一致
        let bad_content = std::fs::read_to_string(&bad_path).unwrap();
        assert_eq!(bad_content, "{invalid json");

        // 清理
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_save_then_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "neoserial_test_{}_roundtrip",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        // 构造自定义 settings（非默认值，确保 roundtrip 真实有效）
        let mut original = Settings::default_settings();
        original.last_port = "COM7".into();
        original.serial_defaults.baud_rate = 9600;
        original.serial_defaults.data_bits = DataBits::Seven;
        original.serial_defaults.parity = Parity::Even;
        original.ui.display_mode = DisplayMode::Hex;
        original.ui.auto_scroll = false;
        original.ui.line_ending = LineEnding::Lf;
        original.error_keywords = vec!["MYERR".into()];

        original.save_to(&path).expect("save should succeed");

        let loaded = Settings::load_from(&path);
        assert_eq!(loaded.last_port, "COM7");
        assert_eq!(loaded.serial_defaults.baud_rate, 9600);
        assert_eq!(loaded.serial_defaults.data_bits, DataBits::Seven);
        assert_eq!(loaded.serial_defaults.parity, Parity::Even);
        assert_eq!(loaded.ui.display_mode, DisplayMode::Hex);
        assert!(!loaded.ui.auto_scroll);
        assert_eq!(loaded.ui.line_ending, LineEnding::Lf);
        assert_eq!(loaded.error_keywords, vec!["MYERR".to_string()]);

        // 清理
        let _ = std::fs::remove_dir_all(&dir);
    }
}
