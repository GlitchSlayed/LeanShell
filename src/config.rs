//! TOML config in `%APPDATA%\wisp\config.toml`.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::Deserialize;

use crate::util;

/// wisp-settings includes the same file, so both apps create the same config.
const DEFAULT_CONFIG: &str = include_str!("default-config.toml");

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

/// Keys missing from the user's file take their value from the default config,
/// as in wisp-settings, so both apps agree on what an old config means.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub hide_taskbar: bool,
    pub dock: bool,
    pub hotkey: String,
    pub max_results: usize,
    pub theme: Theme,
    pub backdrop: Backdrop,
    pub position: Position,
    pub show_recent: bool,
    pub search_url: String,
    pub file_search: bool,
    pub file_search_exclude: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self::parse("").expect("valid default config")
    }
}

impl Config {
    pub fn path() -> PathBuf {
        util::data_dir().join("config.toml")
    }

    pub fn modified() -> Option<SystemTime> {
        std::fs::metadata(Self::path()).and_then(|m| m.modified()).ok()
    }

    /// Creates the default config on first run. Returns defaults and an error
    /// if the file cannot be read or parsed.
    pub fn load() -> (Config, Option<String>) {
        let path = Self::path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::write(&path, DEFAULT_CONFIG);
                DEFAULT_CONFIG.to_string()
            }
            // Preserve unreadable files, including locked files and invalid UTF-8.
            Err(e) => return (Config::default(), Some(format!("config.toml: {e}"))),
        };
        match Self::parse(&text) {
            Ok(mut config) => {
                config.max_results = config.max_results.clamp(1, 20);
                (config, None)
            }
            Err(e) => (Config::default(), Some(format!("config.toml: {}", e.message()))),
        }
    }

    /// Reads a config, taking each missing key from the default config.
    fn parse(text: &str) -> Result<Config, toml::de::Error> {
        let mut table: toml::Table = toml::from_str(DEFAULT_CONFIG)?;
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
    }

    #[test]
    fn missing_keys_take_the_default_config() {
        // A config from before the dock has none of its keys.
        let config = Config::parse("hotkey = \"Ctrl+Space\"").unwrap();
        assert_eq!(config.hotkey, "Ctrl+Space");
        assert!(config.dock && config.hide_taskbar);
        assert_eq!(config.position, Position::Bottom);
        let default = Config::default();
        assert_eq!((default.hotkey.as_str(), default.max_results), ("Alt+Space", 8));
        assert!(Config::parse("dock = \"yes\"").is_err());
    }
}
