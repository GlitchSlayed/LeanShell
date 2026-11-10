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
