//! Core data models for LeanShell: apps, pins, desktops, and tray icons.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

/// Indexed desktop or Store application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppEntry {
    pub name: String,
    /// Windows shell parsing name in `shell:AppsFolder`: a path or an AppUserModelID.
    pub id: String,
    /// File path for desktop apps, if resolvable.
    pub path: Option<String>,
}

/// Premultiplied BGRA image pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconPixels {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl IconPixels {
    pub fn new(width: u32, height: u32, bgra: Vec<u8>) -> Self {
        Self { width, height, bgra }
    }
}

/// Virtual desktops model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desktops {
    /// Active desktop index (from 0).
    pub current: usize,
    /// Each desktop's name (e.g. "Desktop 1").
    pub names: Vec<String>,
}

/// Pinned application in dock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// A matched file or folder hit from search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHit {
    pub name: String,
    pub path: String,
    pub is_folder: bool,
    /// UTF-16 `(start, len)` ranges in `name` that match the query.
    pub highlights: Vec<(u32, u32)>,
    pub icon_key: String,
}

#[derive(Default, Serialize, Deserialize)]
struct PinFile {
    #[serde(default)]
    pin: Vec<Pin>,
}

impl Pin {
    pub fn parse(text: &str) -> Vec<Pin> {
        let Ok(file) = toml::from_str::<PinFile>(text) else {
            return Vec::new();
        };
        let mut pins: Vec<Pin> = Vec::with_capacity(file.pin.len());
        for pin in file.pin {
            if !pin.id.trim().is_empty() && !pins.iter().any(|old| old.id.eq_ignore_ascii_case(&pin.id)) {
                pins.push(pin);
            }
        }
        pins
    }

    pub fn render<'a>(apps: impl IntoIterator<Item = &'a Pin>) -> Option<String> {
        let file = PinFile {
            pin: apps
                .into_iter()
                .map(|app| Pin {
                    id: app.id.clone(),
                    name: app.name.clone(),
                    path: app.path.clone(),
                })
                .collect(),
        };
        toml::to_string(&file).ok()
    }

    pub fn load(path: &Path) -> Vec<Pin> {
        std::fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save<'a>(path: &Path, apps: impl IntoIterator<Item = &'a Pin>) -> std::io::Result<()> {
        let text = Self::render(apps).ok_or_else(|| std::io::Error::other("Cannot save dock pins."))?;
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("Cannot find the pins folder."))?;
        std::fs::create_dir_all(parent)?;
        let tmp = path.with_extension("toml.tmp");
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    }
}

/// Unique identifier for a system notification area icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrayKey {
    Guid([u8; 16]),
    Window { hwnd: u32, uid: u32 },
}

/// Generic tray icon model independent of drawing toolkit.
#[derive(Debug, Clone)]
pub struct TrayIcon<I> {
    pub hwnd: u32,
    pub uid: u32,
    pub guid: Option<[u8; 16]>,
    pub callback: u32,
    pub version: u32,
    pub state: u32,
    pub tip: String,
    pub image: Option<I>,
    pub revision: u64,
    pub pid: u32,
    pub exes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pin_serialization() {
        let pins = vec![
            Pin {
                id: "test1".into(),
                name: "App 1".into(),
                path: Some("C:\\app1.exe".into()),
            },
            Pin {
                id: "test2".into(),
                name: "App 2".into(),
                path: None,
            },
        ];
        let rendered = Pin::render(&pins).unwrap();
        let loaded = Pin::parse(&rendered);
        assert_eq!(pins, loaded);
    }
}
