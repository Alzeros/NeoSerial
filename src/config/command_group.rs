use crate::config::settings::{DisplayMode, LineEnding};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandItem {
    pub label: String,
    pub content: String,
    pub mode: DisplayMode,
    pub line_ending: LineEnding,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandGroup {
    pub name: String,
    pub items: Vec<CommandItem>,
}

impl CommandGroup {
    /// 默认命令组：含 AT / CSQ / Reset / HEX5A 四条示例。
    pub fn default_group() -> Self {
        CommandGroup {
            name: "默认".into(),
            items: vec![
                CommandItem {
                    label: "AT".into(), content: "AT".into(),
                    mode: DisplayMode::Ascii, line_ending: LineEnding::Crlf, enabled: true,
                },
                CommandItem {
                    label: "CSQ".into(), content: "AT+CSQ".into(),
                    mode: DisplayMode::Ascii, line_ending: LineEnding::Crlf, enabled: true,
                },
                CommandItem {
                    label: "Reset".into(), content: "AT+CFUN=1,1".into(),
                    mode: DisplayMode::Ascii, line_ending: LineEnding::Crlf, enabled: true,
                },
                CommandItem {
                    label: "HEX5A".into(), content: "5A A5 01 02".into(),
                    mode: DisplayMode::Hex, line_ending: LineEnding::None, enabled: true,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_group_has_commands() {
        let g = CommandGroup::default_group();
        assert_eq!(g.name, "默认");
        assert_eq!(g.items.len(), 4);
        assert_eq!(g.items[0].label, "AT");
    }

    #[test]
    fn test_serde_roundtrip() {
        let g = CommandGroup::default_group();
        let json = serde_json::to_string(&g).unwrap();
        let back: CommandGroup = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, g.name);
        assert_eq!(back.items.len(), g.items.len());
        assert_eq!(back.items[3].label, "HEX5A");
        assert!(back.items[3].enabled);
    }
}
