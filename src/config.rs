//! Configuration loader and path management.

use std::path::PathBuf;
use std::time::SystemTime;

pub use leanshell_core::config::*;

use crate::util;

pub fn path() -> PathBuf {
    util::data_dir().join("config.toml")
}

pub fn modified() -> Option<SystemTime> {
    Config::modified(&path())
}

pub fn load() -> (Config, Option<String>) {
    Config::load_from_path(&path())
}

pub fn read() -> Result<(Config, Option<String>), String> {
    Config::read_from_path(&path())
}

/// The dock's color, or `None` for the theme's colors.
pub fn dock_tint(config: &Config) -> Option<crate::theme::Tint> {
    crate::theme::Tint::parse(&config.dock_color)
}
