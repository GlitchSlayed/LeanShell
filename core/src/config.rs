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

/// Shell mode: LeanShell is an exclusive Windows shell replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellMode {
    #[default]
    Exclusive,
}

impl<'de> serde::Deserialize<'de> for ShellMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_lowercase().as_str() {
            "exclusive" | "companion" => Ok(ShellMode::Exclusive),
            other => Err(serde::de::Error::unknown_variant(other, &["exclusive"])),
        }
    }
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

impl PanelEdge {
    pub fn is_vertical(self) -> bool {
        matches!(self, PanelEdge::Left | PanelEdge::Right)
    }
}

/// Horizontal alignment of search and app icons in panel mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PanelAlign {
    #[default]
    Center,
    Left,
}

/// Keys missing from the user's file take their value from `config-0.3.toml`,
/// then from the default config.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub hide_taskbar: bool,
    pub dock: bool,
    pub dock_autohide: bool,
    #[serde(default = "default_true")]
    pub dock_search_button: bool,
    #[serde(default = "default_dock_screen")]
    pub dock_screen: String,
    /// `#RRGGBB` or `#RRGGBBAA`, or empty for the theme's colors.
    #[serde(default)]
    pub dock_color: String,
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
    #[serde(default = "default_true")]
    pub file_search_everything: bool,
    pub window_list: WindowListMode,
    #[serde(default)]
    pub shell_mode: ShellMode,
    #[serde(default)]
    pub display_mode: DisplayMode,
    #[serde(default)]
    pub panel_edge: PanelEdge,
    #[serde(default)]
    pub panel_align: PanelAlign,
    pub panel_thickness: Option<u32>,
    #[serde(default = "default_true")]
    pub watchdog: bool,
    #[serde(default = "default_true")]
    pub claim_win_key: bool,
    #[serde(default = "default_true")]
    pub neuter_shell_hosts: bool,
    #[serde(default = "default_true")]
    pub auto_reap_explorer: bool,
    #[serde(default = "default_true")]
    pub claim_shell_chords: bool,
    #[serde(default = "default_true")]
    pub wm_enabled: bool,
    #[serde(default)]
    pub wm_tiling: bool,
    #[serde(default = "default_wm_gap")]
    pub wm_gap: u32,
    #[serde(default = "default_true")]
    pub wm_borders: bool,
    #[serde(default = "default_wm_border_color")]
    pub wm_border_color: String,
}

fn default_true() -> bool {
    true
}

fn default_dock_screen() -> String {
    "pointer".to_string()
}

fn default_wm_gap() -> u32 {
    8
}

fn default_wm_border_color() -> String {
    "#0891B2".to_string()
}

/// Returns true if `value` is empty (meaning use theme colors) or a valid `#RRGGBB` / `#RRGGBBAA` hex color.
pub fn is_valid_color(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return true;
    }
    matches!(value.strip_prefix('#'), Some(hex) if matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
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
    /// if the file cannot be read or parsed, or a warning if an unusable color fell back.
    pub fn load_from_path(path: &Path) -> (Config, Option<String>) {
        Self::read_from_path(path).unwrap_or_else(|error| (Config::default(), Some(error)))
    }

    /// Reads the config, creating the default one on first run. `Err` means
    /// the file cannot be read or parsed. A warning comes with a config that
    /// is usable, where one value fell back to its default.
    pub fn read_from_path(path: &Path) -> Result<(Config, Option<String>), String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::write(path, DEFAULT_CONFIG);
                DEFAULT_CONFIG.to_string()
            }
            // Preserve unreadable files, including locked files and invalid UTF-8.
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut config = Self::parse(&text).map_err(|e| format!("{}: {}", path.display(), e.message()))?;
        config.max_results = config.max_results.clamp(1, 20);
        config.dock_icon_size = config.dock_icon_size.clamp(24, 40);
        config.wm_gap = config.wm_gap.min(32);
        let warning = config.drop_unusable_color();
        Ok((config, warning))
    }

    /// Clears a `dock_color` or `wm_border_color` that is not a valid hex color.
    pub fn drop_unusable_color(&mut self) -> Option<String> {
        let mut warnings = Vec::new();
        if !self.dock_color.trim().is_empty() && !is_valid_color(&self.dock_color) {
            warnings.push(format!(
                "config.toml: dock_color \"{}\" must look like #1E3A8A or #1E3A8AC0. Using the theme's colors.",
                self.dock_color
            ));
            self.dock_color.clear();
        }
        if !self.wm_border_color.trim().is_empty() && !is_valid_color(&self.wm_border_color) {
            warnings.push(format!(
                "config.toml: wm_border_color \"{}\" must look like #0891B2. Using the default border color.",
                self.wm_border_color
            ));
            self.wm_border_color = default_wm_border_color();
        }
        (!warnings.is_empty()).then(|| warnings.join(" "))
    }

    /// Reads a config, taking each missing key from `config-0.3.toml` or the
    /// default config.
    pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
        let mut table: toml::Table = toml::from_str(DEFAULT_CONFIG)?;
        table.extend(toml::from_str::<toml::Table>(MISSING_KEYS)?);
        let user: toml::Table = toml::from_str(text)?;
        let has_claim_win_key = user.contains_key("claim_win_key");
        let win_key_value = user.get("win_key").cloned();
        table.extend(user);
        if !has_claim_win_key && let Some(v) = win_key_value {
            table.insert("claim_win_key".into(), v);
        }
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
        assert_eq!(default.shell_mode, ShellMode::Exclusive);
        assert!(default.watchdog);
        assert!(Config::parse("dock = \"yes\"").is_err());
    }

    #[test]
    fn shell_mode_and_watchdog_deserialize() {
        let config = Config::parse("shell_mode = \"exclusive\"\nwatchdog = false").unwrap();
        assert_eq!(config.shell_mode, ShellMode::Exclusive);
        assert!(!config.watchdog);

        // Legacy companion config value automatically maps to exclusive
        let legacy = Config::parse("shell_mode = \"companion\"").unwrap();
        assert_eq!(legacy.shell_mode, ShellMode::Exclusive);
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

    #[test]
    fn legacy_renderer_keys_are_ignored() {
        let config = Config::parse("backend = \"slint\"\nslint_renderer = \"vello\"").unwrap();
        assert!(config.wm_enabled);
    }

    #[test]
    fn panel_align_deserializes() {
        let config = Config::parse("panel_align = \"left\"").unwrap();
        assert_eq!(config.panel_align, PanelAlign::Left);

        let default = Config::default();
        assert_eq!(default.panel_align, PanelAlign::Center);
    }

    #[test]
    fn win_key_alias_and_new_dock_keys_deserialize() {
        let default = Config::default();
        assert!(default.dock_search_button);
        assert_eq!(default.dock_screen, "pointer");
        assert!(default.dock_color.is_empty());

        let config = Config::parse(
            "win_key = false\ndock_search_button = false\ndock_screen = 'main'\ndock_color = '#1E3A8AC0'",
        )
        .unwrap();
        assert!(!config.claim_win_key);
        assert!(!config.dock_search_button);
        assert_eq!(config.dock_screen, "main");
        assert_eq!(config.dock_color, "#1E3A8AC0");

        let mut bad = Config::parse("dock_color = 'not-a-color'\ndock_search_button = false").unwrap();
        assert!(bad.drop_unusable_color().is_some());
        assert!(bad.dock_color.is_empty());
        assert!(!bad.dock_search_button);
    }

    #[test]
    fn phase7_takeover_keys_default_true_and_deserialize() {
        let default = Config::default();
        assert!(default.neuter_shell_hosts);
        assert!(default.auto_reap_explorer);
        assert!(default.claim_shell_chords);

        let config =
            Config::parse("neuter_shell_hosts = false\nauto_reap_explorer = false\nclaim_shell_chords = false")
                .unwrap();
        assert!(!config.neuter_shell_hosts);
        assert!(!config.auto_reap_explorer);
        assert!(!config.claim_shell_chords);
    }

    #[test]
    fn phase8_wm_keys_defaults_and_deserialize() {
        let default = Config::default();
        assert!(default.wm_enabled);
        assert!(!default.wm_tiling);
        assert_eq!(default.wm_gap, 8);
        assert!(default.wm_borders);
        assert_eq!(default.wm_border_color, "#0891B2");

        let mut custom = Config::parse(
            "wm_enabled = false\nwm_tiling = true\nwm_gap = 12\nwm_borders = false\nwm_border_color = 'bad-hex'",
        )
        .unwrap();
        assert!(!custom.wm_enabled);
        assert!(custom.wm_tiling);
        assert_eq!(custom.wm_gap, 12);
        assert!(!custom.wm_borders);
        assert!(custom.drop_unusable_color().is_some());
        assert_eq!(custom.wm_border_color, "#0891B2");
    }
}
