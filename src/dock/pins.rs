//! Apps pinned to the dock, kept in dock order in the channel's data folder.
//!
//! Pin parsing and serialization are provided by `leanshell_core::model::Pin`.

use std::path::Path;

pub use leanshell_core::model::Pin;

/// The pinned apps, in dock order. A missing or unreadable file pins nothing.
pub fn load(path: &Path) -> Vec<Pin> {
    Pin::load(path)
}

/// Saves the pins. The file is written aside first so a crash keeps the old one.
pub fn save<'a>(path: &Path, apps: impl IntoIterator<Item = &'a Pin>) -> std::io::Result<()> {
    Pin::save(path, apps)
}
