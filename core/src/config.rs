//! TOML config for LeanShell.

use std::path::Path;

use serde::Deserialize;

pub const DEFAULT_CONFIG: &str = include_str!("default-config.toml");
pub const MISSING_KEYS: &str = include_str!("config-0.3.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    Acrylic,
    Mica,
    Solid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    #[serde(alias = "top")]
    Center,
    /// The query box sits at the bottom of the screen, where the dock is.
    Bottom,
}

/// When hovering an app shows its window list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum WindowListMode {
    #[default]
    Multiple,
    Always,
    Off,
}

/// Shell mode: running alongside Windows ("companion") or exclusive shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ShellMode {
    #[default]
    Companion,
    Exclusive,
}

/// Layout and shell presentation modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DisplayMode {
    /// Floating, auto-hiding dock that slides up on edge touch.
    #[default]
    Dock,
    /// Persistent edge-docked panel/taskbar that reserves screen work area via AppBar.
    Panel,
    /// Center launcher only (no dock or panel).
    Center,
}

/// Screen edge on which the panel or dock is positioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PanelEdge {
    #[default]
    Bottom,
    Top,
    Left,
    Right,
}

/// Keys missing from the user's file take their value from `config-0.3.toml`,
/// then from the default config.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub hide_taskbar: bool,
    pub dock: bool,
    pub dock_autohide: bool,
    pub dock_icon_size: usize,
    pub hotkey: String,
    pub max_results: usize,
    pub theme: Theme,
    pub backdrop: Backdrop,
    pub position: Position,
    pub show_recent: bool,
    pub search_url: String,
    pub file_search: bool,
    pub file_search_exclude: Vec<String>,
    pub window_list: WindowListMode,
    #[serde(default)]
    pub shell_mode: ShellMode,
    #[serde(default)]
    pub display_mode: DisplayMode,
    #[serde(default)]
    pub panel_edge: PanelEdge,
    pub panel_thickness: Option<u32>,
    #[serde(default = "default_true")]
    pub watchdog: bool,
    #[serde(default = "default_true")]
    pub claim_win_key: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("valid default config")
    }
}

impl Config {
    /// Returns the last modification time of the config file.
    pub fn modified(path: &Path) -> Option<std::time::SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Creates the default config on first run if not found. Returns defaults and an error
    /// if the file cannot be read or parsed.
    pub fn load_from_path(path: &Path) -> (Config, Option<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::write(path, DEFAULT_CONFIG);
                DEFAULT_CONFIG.to_string()
            }
            // Preserve unreadable files, including locked files and invalid UTF-8.
            Err(e) => return (Config::default(), Some(format!("{}: {e}", path.display()))),
        };
        match Self::parse(&text) {
            Ok(mut config) => {
                config.max_results = config.max_results.clamp(1, 20);
                config.dock_icon_size = config.dock_icon_size.clamp(24, 40);
                (config, None)
            }
            Err(e) => (Config::default(), Some(format!("{}: {}", path.display(), e.message()))),
        }
    }

    /// Reads a config, taking each missing key from `config-0.3.toml` or the
    /// default config.
    pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
        let mut table: toml::Table = toml::from_str(DEFAULT_CONFIG)?;
        table.extend(toml::from_str::<toml::Table>(MISSING_KEYS)?);
        table.extend(toml::from_str::<toml::Table>(text)?);
        table.try_into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_pins_are_ignored() {
        let config = Config::parse("dock = true\ndock_apps = ['old.app']").unwrap();
        assert!(config.dock);
        assert_eq!(config.window_list, super::WindowListMode::Multiple);
    }

    #[test]
    fn window_list_modes_deserialize_and_older_config_defaults_to_multiple() {
        for (value, expected) in [
            ("multiple", super::WindowListMode::Multiple),
            ("always", super::WindowListMode::Always),
            ("off", super::WindowListMode::Off),
        ] {
            let config = Config::parse(&format!("window_list = '{value}'")).unwrap();
            assert_eq!(config.window_list, expected);
        }
    }

    #[test]
    fn missing_keys_keep_a_config_from_before_the_dock() {
        let config = Config::parse("hotkey = \"Ctrl+Space\"").unwrap();
        assert_eq!((config.hotkey.as_str(), config.max_results), ("Ctrl+Space", 8));
        assert!(!config.dock && !config.hide_taskbar);
        assert_eq!(config.position, Position::Center);

        let default = Config::default();
        assert!(default.dock && default.hide_taskbar);
        assert_eq!(default.dock_icon_size, 32);
        assert_eq!(default.position, Position::Bottom);
        assert_eq!(default.shell_mode, ShellMode::Companion);
        assert!(default.watchdog);
        assert!(Config::parse("dock = \"yes\"").is_err());
    }

    #[test]
    fn shell_mode_and_watchdog_deserialize() {
        let config = Config::parse("shell_mode = \"exclusive\"\nwatchdog = false").unwrap();
        assert_eq!(config.shell_mode, ShellMode::Exclusive);
        assert!(!config.watchdog);
    }

    #[test]
    fn display_mode_and_panel_edge_deserialize() {
        let config = Config::parse("display_mode = \"panel\"\npanel_edge = \"top\"\npanel_thickness = 48").unwrap();
        assert_eq!(config.display_mode, DisplayMode::Panel);
        assert_eq!(config.panel_edge, PanelEdge::Top);
        assert_eq!(config.panel_thickness, Some(48));

        let config2 = Config::parse("display_mode = \"center\"").unwrap();
        assert_eq!(config2.display_mode, DisplayMode::Center);
        assert_eq!(config2.panel_edge, PanelEdge::Bottom);
    }

    #[test]
    fn claim_win_key_deserializes() {
        let config = Config::parse("claim_win_key = false").unwrap();
        assert!(!config.claim_win_key);
        let default = Config::default();
        assert!(default.claim_win_key);
    }
}
