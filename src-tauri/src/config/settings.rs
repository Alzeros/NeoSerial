use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::command_group::CommandGroup;

pub use crate::util::codec::LineEnding;

const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DisplayMode {
    Ascii,
    Hex,
}

/// 文本模式的编码方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum TextEncoding {
    Ascii,
    Utf8,
    Gbk,
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
    pub show_timestamp: bool,
    /// 日志区最左侧行号(本次连接期间 index)开关,旧配置无此字段时默认 false
    #[serde(default)]
    pub show_line_index: bool,
    pub log_send: bool,
    /// 日志区字号（px），默认 14
    #[serde(default = "default_log_font_size")]
    pub log_font_size: u32,
    /// 日志区行高（无单位倍数），默认 1.6
    #[serde(default = "default_log_line_height")]
    pub log_line_height: f64,
    /// 方向标签样式："short"=Tx/Rx，"full"=发送/接收
    #[serde(default = "default_log_dir_label")]
    pub log_dir_label: String,
    /// 日志区英文字体族：'default' 或 CSS font-family 值，默认 'default'
    #[serde(default = "default_log_font_latin")]
    pub log_font_latin: String,
    /// 日志区中文字体族：'default'=跟随英文，或 CSS font-family 值
    #[serde(default = "default_log_font_cjk")]
    pub log_font_cjk: String,
    /// 文本模式的编码方式：Ascii/Utf8/Gbk，默认 Ascii
    #[serde(default = "default_text_encoding")]
    pub text_encoding: TextEncoding,
    /// 后台运行(托盘常驻)。开:有托盘图标,关窗口连接不断、进入后台,关最后一个窗口
    /// 应用仍在;关(默认,经典串口工具体验):无托盘,关窗口断开该窗口自己连的
    /// (agent 的交还 agent),关最后一个窗口即退出。改后即时生效。
    #[serde(default = "default_background_mode")]
    pub background_mode: bool,
    /// 首次收进后台的系统通知是否已发过(只提示一次)。后端写,见 merge_backend_owned。
    #[serde(default)]
    pub tray_hint_shown: bool,
}

fn default_text_encoding() -> TextEncoding {
    TextEncoding::Ascii
}

/// 后台运行的默认值,单独收在这里便于一处改。默认关:新用户拿到的是和其他串口工具
/// 一致的体验(关窗即退、不占口),需要给 agent 当常驻服务的人在设置里打开。
pub fn default_background_mode() -> bool {
    false
}

fn default_log_font_size() -> u32 {
    14
}

fn default_log_line_height() -> f64 {
    1.6
}

fn default_log_dir_label() -> String {
    "short".into()
}

fn default_log_font_latin() -> String {
    "default".into()
}

fn default_log_font_cjk() -> String {
    "default".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuickCommand {
    pub label: String,
    pub content: String,
    pub mode: DisplayMode,
    pub line_ending: LineEnding,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PresetSettings {
    /// 用户自定义的预设波特率，会追加到连接栏波特率下拉中。
    #[serde(default = "default_baud_rates")]
    pub baud_rates: Vec<u32>,
    /// 主题预设 key：preset-1..preset-4 或 custom，默认 preset-1
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 自定义主题色板：变量名 → 颜色值/数值，结构由前端定义并透传。
    /// 空 map = 用户从未配置过自定义主题（前端以预设 1 为底稿初始化）。
    #[serde(default)]
    pub custom_theme: std::collections::BTreeMap<String, String>,
}

fn default_baud_rates() -> Vec<u32> {
    vec![9600, 115200, 921600]
}

fn default_theme() -> String {
    "preset-1".into()
}

impl Default for PresetSettings {
    fn default() -> Self {
        PresetSettings {
            baud_rates: default_baud_rates(),
            theme: default_theme(),
            custom_theme: Default::default(),
        }
    }
}

/// MCP 服务设置。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpSettings {
    /// 打开软件时是否自动启动 MCP server（默认 true）。
    /// 改后需重启软件生效（setup 时读一次决定是否起 server）。
    #[serde(default = "default_mcp_auto_start")]
    pub auto_start: bool,
    /// MCP server **首选**监听端口（默认 34594）。
    /// 被占时(别的程序占了该端口)自动向上递增(最多 +20)找空闲绑定。
    /// 实际绑定端口见设置页(get_mcp_status)与 registry。
    /// claude mcp add 的固定 URL 只命中绑到首选端口的实例;改端口后需重新配置。
    #[serde(default = "default_mcp_port")]
    pub port: u16,
}

fn default_mcp_auto_start() -> bool {
    true
}

fn default_mcp_port() -> u16 {
    34594
}

impl Default for McpSettings {
    fn default() -> Self {
        McpSettings {
            auto_start: default_mcp_auto_start(),
            port: default_mcp_port(),
        }
    }
}

/// 输入框指令联想设置:知识库手册索引接入 + 联想开关。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandIndexSettings {
    /// 知识库服务器地址,如 http://10.12.16.11:8200;空 = 未配置,联想只用发送历史
    #[serde(default)]
    pub base_url: String,
    /// X-API-Key。明文存 settings.json,与本机其他配置同等对待
    #[serde(default)]
    pub api_key: String,
    /// 手册 id 排除名单:不在此列的手册都参与候选,新出现的手册默认参与
    #[serde(default)]
    pub disabled_doc_ids: Vec<i64>,
    /// 启动时后台刷新一次缓存(仅在地址与 key 都非空时)
    #[serde(default = "default_true")]
    pub auto_refresh: bool,
    /// 联想总开关
    #[serde(default = "default_true")]
    pub suggest_enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CommandIndexSettings {
    fn default() -> Self {
        CommandIndexSettings {
            base_url: String::new(),
            api_key: String::new(),
            disabled_doc_ids: Vec::new(),
            auto_refresh: default_true(),
            suggest_enabled: default_true(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub window: WindowSettings,
    pub serial_defaults: SerialDefaults,
    pub last_port: String,
    pub ui: UiSettings,
    pub command_groups: Vec<CommandGroup>,
    pub error_keywords: Vec<String>,
    /// 预设项（预设波特率等）。旧配置缺该字段时取默认。
    #[serde(default)]
    pub presets: PresetSettings,
    /// MCP 服务设置。旧配置缺该字段时取默认（auto_start=true）。
    #[serde(default)]
    pub mcp: McpSettings,
    /// 指令联想设置。旧配置缺该字段时取默认(联想开、只用历史)。
    #[serde(default)]
    pub command_index: CommandIndexSettings,
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
                show_timestamp: true,
                show_line_index: false,
                log_send: true,
                log_font_size: default_log_font_size(),
                log_line_height: default_log_line_height(),
                log_dir_label: default_log_dir_label(),
                log_font_latin: default_log_font_latin(),
                log_font_cjk: default_log_font_cjk(),
                text_encoding: default_text_encoding(),
                background_mode: default_background_mode(),
                tray_hint_shown: false,
            },
            command_groups: vec![CommandGroup::default_group()],
            error_keywords: vec![
                "ERROR".into(),
                "FAIL".into(),
                "+CME ERROR".into(),
                "+CMS ERROR".into(),
            ],
            presets: PresetSettings::default(),
            mcp: McpSettings::default(),
            command_index: CommandIndexSettings::default(),
        }
    }

    fn config_path() -> PathBuf {
        crate::config::config_dir().join("settings.json")
    }

    /// 加载配置。文件不存在/损坏 → 用默认配置并静默降级；
    /// 损坏时把坏文件备份为 settings.json.bad。
    pub fn load() -> Self {
        Self::load_from(&Self::config_path())
    }

    /// 从指定路径加载配置。文件不存在 → 默认；损坏 → 备份为 `.bad` + 默认。
    /// 测试用（传入 temp 路径隔离，不依赖 APPDATA 环境变量）。
    pub(crate) fn load_from(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Self::default_settings(),
        };
        // 先尝试新格式
        if let Ok(s) = serde_json::from_str::<Settings>(&text) {
            return s;
        }
        // 再尝试旧格式迁移（quick_commands → command_groups）
        if let Ok(legacy) = serde_json::from_str::<LegacySettings>(&text) {
            return legacy.migrate();
        }
        // 都失败：备份 + 默认
        let _ = fs::rename(path, path.with_extension("json.bad"));
        Self::default_settings()
    }

    /// 用内存态 `current` 覆盖本份配置里"只归后端写"的字段。
    ///
    /// 前端/MCP 的 save_settings 回传的是整份 Settings,基于调用方手里(可能已过期)的快照;
    /// 后端自行改过、调用方拿不到的字段若照单全收就会被冲回旧值。目前只有
    /// `ui.tray_hint_shown`(主窗口首次收进托盘时由 mark_tray_hint_shown 写),
    /// 后续再有这类字段加到这里。
    pub fn merge_backend_owned(&mut self, current: &Settings) {
        self.ui.tray_hint_shown = current.ui.tray_hint_shown;
    }

    /// 首次收进托盘的提示只发一次:未发过则置标记并返回 true(调用方负责落盘与发通知)。
    pub fn mark_tray_hint_shown(&mut self) -> bool {
        if self.ui.tray_hint_shown {
            return false;
        }
        self.ui.tray_hint_shown = true;
        true
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

/// 旧版配置（quick_commands）。用于向后兼容迁移。
#[derive(Deserialize)]
struct LegacySettings {
    version: u32,
    window: WindowSettings,
    serial_defaults: SerialDefaults,
    last_port: String,
    ui: LegacyUi,
    quick_commands: Vec<QuickCommand>,
    error_keywords: Vec<String>,
}

#[derive(Deserialize)]
struct LegacyUi {
    display_mode: DisplayMode,
    line_ending: LineEnding,
    auto_scroll: bool,
    ring_buffer_capacity: usize,
}

impl LegacySettings {
    fn migrate(self) -> Settings {
        let def = Settings::default_settings();
        Settings {
            version: self.version,
            window: self.window,
            serial_defaults: self.serial_defaults,
            last_port: self.last_port,
            ui: UiSettings {
                display_mode: self.ui.display_mode,
                line_ending: self.ui.line_ending,
                auto_scroll: self.ui.auto_scroll,
                ring_buffer_capacity: self.ui.ring_buffer_capacity,
                show_timestamp: def.ui.show_timestamp,
                show_line_index: false,
                log_send: def.ui.log_send,
                log_font_size: def.ui.log_font_size,
                log_line_height: def.ui.log_line_height,
                log_dir_label: def.ui.log_dir_label,
                log_font_latin: default_log_font_latin(),
                log_font_cjk: default_log_font_cjk(),
                text_encoding: default_text_encoding(),
                background_mode: default_background_mode(),
                tray_hint_shown: false,
            },
            command_groups: vec![CommandGroup {
                name: "默认".into(),
                items: self.quick_commands.into_iter().map(|q| crate::config::command_group::CommandItem {
                    label: q.label,
                    content: q.content,
                    mode: q.mode,
                    line_ending: q.line_ending,
                    enabled: true,
                }).collect(),
            }],
            error_keywords: self.error_keywords,
            presets: PresetSettings::default(),
            mcp: def.mcp.clone(),
            command_index: CommandIndexSettings::default(),
        }
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
        assert!(!s.command_groups.is_empty());
        assert_eq!(s.command_groups[0].items[0].label, "AT");
    }

    #[test]
    fn test_serde_roundtrip() {
        let s = Settings::default_settings();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, s.version);
        assert_eq!(back.serial_defaults.baud_rate, s.serial_defaults.baud_rate);
        assert_eq!(back.command_groups.len(), s.command_groups.len());
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
        assert_eq!(loaded.command_groups.len(), def.command_groups.len());
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

    #[test]
    fn test_default_has_display_switches() {
        let s = Settings::default_settings();
        assert!(s.ui.show_timestamp);
        assert!(s.ui.log_send);
    }

    /// 后台运行默认值收在 default_background_mode() 一处;首次收后台提示尚未显示。
    #[test]
    fn test_default_background_mode_follows_constant() {
        let s = Settings::default_settings();
        assert_eq!(s.ui.background_mode, default_background_mode());
        assert!(!s.ui.tray_hint_shown);
    }

    /// 旧配置没有 background_mode 键 → 取默认常量,而不是 bool 的 false。
    #[test]
    fn test_missing_background_mode_takes_default() {
        let mut v = serde_json::to_value(Settings::default_settings()).unwrap();
        v["ui"].as_object_mut().unwrap().remove("background_mode");
        let s: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(s.ui.background_mode, default_background_mode());
    }

    /// mark_tray_hint_shown:首次返回 true(该提示),之后一直 false,且标记落在字段上。
    #[test]
    fn test_mark_tray_hint_shown_only_first_time() {
        let mut s = Settings::default_settings();
        assert!(s.mark_tray_hint_shown(), "首次应提示");
        assert!(s.ui.tray_hint_shown);
        assert!(!s.mark_tray_hint_shown(), "第二次不再提示");
    }

    /// 前端/MCP 整份回写带的是旧快照(tray_hint_shown=false),不能把后端已置 true 的标记冲掉。
    #[test]
    fn test_merge_backend_owned_keeps_tray_hint_shown() {
        let mut current = Settings::default_settings();
        current.ui.tray_hint_shown = true;
        let mut incoming = Settings::default_settings();
        incoming.ui.tray_hint_shown = false;
        incoming.ui.background_mode = !default_background_mode();
        incoming.merge_backend_owned(&current);
        assert!(incoming.ui.tray_hint_shown, "后端持有字段以内存态为准");
        assert_ne!(incoming.ui.background_mode, default_background_mode(), "用户可改字段照回写值");
    }

    /// 旧开发版写过 ui.minimize_to_tray / close_prompted / close_exits_app,
    /// 读到这些多余键应忽略而非解析失败。
    #[test]
    fn test_load_ignores_removed_tray_fields() {
        let mut v = serde_json::to_value(Settings::default_settings()).unwrap();
        let ui = v["ui"].as_object_mut().unwrap();
        ui.insert("minimize_to_tray".into(), serde_json::Value::Bool(false));
        ui.insert("close_prompted".into(), serde_json::Value::Bool(true));
        ui.insert("close_exits_app".into(), serde_json::Value::Bool(true));
        let s: Settings = serde_json::from_value(v).expect("多余的旧键应被忽略");
        assert_eq!(s.ui.background_mode, default_background_mode());
        assert!(!s.ui.tray_hint_shown);
    }

    #[test]
    fn test_default_has_command_groups() {
        let s = Settings::default_settings();
        assert_eq!(s.command_groups.len(), 1);
        assert_eq!(s.command_groups[0].name, "默认");
        assert!(!s.command_groups[0].items.is_empty());
    }

    #[test]
    fn test_migrate_legacy_quick_commands() {
        let dir = std::env::temp_dir().join(format!(
            "neoserial_test_{}_legacy", std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        // 旧格式：含 quick_commands，无 command_groups
        let legacy = r#"{
            "version": 1,
            "window": {"width": 1100, "height": 720, "x": 100, "y": 80},
            "serial_defaults": {"baud_rate": 115200, "data_bits": "Eight", "parity": "None", "stop_bits": "One", "flow_control": "None"},
            "last_port": "",
            "ui": {"display_mode": "Ascii", "line_ending": "Crlf", "auto_scroll": true, "ring_buffer_capacity": 5000},
            "quick_commands": [{"label": "AT", "content": "AT", "mode": "Ascii", "line_ending": "Crlf"}],
            "error_keywords": ["ERROR"]
        }"#;
        std::fs::write(&path, legacy).unwrap();

        let loaded = Settings::load_from(&path);
        // 旧 quick_commands 应迁移为 command_groups
        assert_eq!(loaded.command_groups.len(), 1);
        assert_eq!(loaded.command_groups[0].items.len(), 1);
        assert_eq!(loaded.command_groups[0].items[0].label, "AT");
        // 开关字段缺失应取默认
        assert!(loaded.ui.show_timestamp);
        assert!(loaded.ui.log_send);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 旧配置没有 command_index 段 → 整段取默认(auto_refresh/suggest_enabled 为 true,不是 bool 默认的 false)。
    #[test]
    fn test_missing_command_index_takes_default() {
        let mut v = serde_json::to_value(Settings::default_settings()).unwrap();
        v.as_object_mut().unwrap().remove("command_index");
        let s: Settings = serde_json::from_value(v).unwrap();
        assert!(s.command_index.auto_refresh);
        assert!(s.command_index.suggest_enabled);
        assert!(s.command_index.base_url.is_empty());
        assert!(s.command_index.api_key.is_empty());
        assert!(s.command_index.disabled_doc_ids.is_empty());
    }

    /// 段里只写了部分键 → 缺的键各取默认。
    #[test]
    fn test_command_index_partial_keys_take_default() {
        let mut v = serde_json::to_value(Settings::default_settings()).unwrap();
        v["command_index"] = serde_json::json!({ "base_url": "http://h:8200/", "disabled_doc_ids": [3] });
        let s: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(s.command_index.base_url, "http://h:8200/");
        assert_eq!(s.command_index.disabled_doc_ids, vec![3]);
        assert!(s.command_index.auto_refresh, "缺 auto_refresh 键应为 true");
        assert!(s.command_index.suggest_enabled);
    }
}
