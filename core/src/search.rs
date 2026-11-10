//! Search grammar, matching, ranking, and query parsing.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher, Utf32Str};

use crate::calc;
use crate::command::SystemCommand;
use crate::config::Config;
use crate::glyphs;
use crate::history::History;
use crate::model::{AppEntry, FileHit};

const MAX_APP_RESULTS: usize = 40;
/// Shortest query that also searches files when it has no `?` prefix.
const MIN_MIXED_FILE_QUERY: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    App,
    File,
    Folder,
    Calc,
    Url,
    Shell,
    Web,
    Command,
}

impl Kind {
    pub fn verb(self) -> &'static str {
        match self {
            Kind::App | Kind::File | Kind::Folder | Kind::Url => "Open",
            Kind::Calc => "Copy",
            Kind::Shell => "Run",
            Kind::Web => "Search",
            Kind::Command => "Run",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Launch {
        id: String,
        path: Option<String>,
        admin: bool,
    },
    Copy(String),
    Open(String),
    Shell {
        command: String,
        admin: bool,
    },
    Reveal(String),
    Command(SystemCommand),
    Surface(usize),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drill {
    WifiNetworks,
    BluetoothDevices,
    AudioOutputs,
}

impl Drill {
    pub fn title(self) -> &'static str {
        match self {
            Drill::WifiNetworks => "Networks",
            Drill::BluetoothDevices => "Devices",
            Drill::AudioOutputs => "Output devices",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Drill::WifiNetworks => glyphs::WIFI,
            Drill::BluetoothDevices => glyphs::BLUETOOTH,
            Drill::AudioOutputs => glyphs::SPEAKER,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResultItem {
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub highlights: Vec<(u32, u32)>,
    pub action: Action,
    pub icon: Option<String>,
    pub glyph: Option<&'static str>,
    pub verb: Option<&'static str>,
    pub drill: Option<Drill>,
}

impl ResultItem {
    pub fn new(kind: Kind, title: impl Into<String>, subtitle: impl Into<String>, action: Action) -> Self {
        ResultItem {
            kind,
            title: title.into(),
            subtitle: subtitle.into(),
            highlights: Vec::new(),
            action,
            icon: None,
            glyph: None,
            verb: None,
            drill: None,
        }
    }

    pub fn glyph(mut self, glyph: &'static str) -> Self {
        self.glyph = Some(glyph);
        self
    }

    pub fn verb(mut self, verb: &'static str) -> Self {
        self.verb = Some(verb);
        self
    }

    pub fn enter_label(&self) -> &'static str {
        self.verb.unwrap_or(self.kind.verb())
    }

    pub fn actions(&self) -> Vec<ResultItem> {
        let mut actions = Vec::new();
        let item = |title: &str, action: Action| ResultItem::new(self.kind, title, self.title.clone(), action);
        match &self.action {
            Action::Launch { id, path, .. } => {
                actions.push(item("Open", self.action.clone()).glyph("\u{E8A7}").verb("Open"));
                let admin = Action::Launch {
                    id: id.clone(),
                    path: path.clone(),
                    admin: true,
                };
                actions.push(item("Run as administrator", admin).glyph("\u{E7EF}").verb("Run"));
            }
            Action::Shell { command, .. } if !command.is_empty() => {
                actions.push(item("Run", self.action.clone()).glyph("\u{E756}").verb("Run"));
                let admin = Action::Shell {
                    command: command.clone(),
                    admin: true,
                };
                actions.push(item("Run as administrator", admin).glyph("\u{E7EF}").verb("Run"));
            }
            Action::Command(SystemCommand::FocusWindow(hwnd)) => {
                actions.push(item("Switch to", self.action.clone()).glyph("\u{E8A7}").verb("Switch"));
                let close = Action::Command(SystemCommand::CloseWindow(*hwnd));
                actions.push(item("Close window", close).glyph(glyphs::CLOSE).verb("Close"));
            }
            Action::Open(_) if self.path().is_some() => {
                actions.push(item("Open", self.action.clone()).glyph("\u{E8A7}").verb("Open"));
            }
            _ => {}
        }
        if let Some(path) = self.path() {
            let reveal = Action::Reveal(path.into());
            actions.push(item("Open file location", reveal).glyph("\u{E838}").verb("Open"));
            actions.push(
                item("Copy path", Action::Copy(path.into()))
                    .glyph("\u{E8C8}")
                    .verb("Copy"),
            );
        }
        actions
    }

    pub fn path(&self) -> Option<&str> {
        match (&self.kind, &self.action) {
            (Kind::App, Action::Launch { path, .. }) => path.as_deref(),
            (Kind::File | Kind::Folder, Action::Open(path)) => Some(path),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FileScope<'a> {
    None,
    Mixed(&'a str),
    Only(&'a str),
}

impl<'a> FileScope<'a> {
    pub fn of(query: &'a str, results: &[ResultItem]) -> FileScope<'a> {
        let query = query.trim();
        if let Some(rest) = query.strip_prefix('?') {
            let rest = rest.trim();
            return if rest.is_empty() {
                FileScope::None
            } else {
                FileScope::Only(rest)
            };
        }
        let special = results
            .iter()
            .any(|r| matches!(r.kind, Kind::Calc | Kind::Url | Kind::Shell));
        if special || query.chars().count() < MIN_MIXED_FILE_QUERY {
            FileScope::None
        } else {
            FileScope::Mixed(query)
        }
    }
}

pub struct NameMatcher {
    matcher: Matcher,
    buf: Vec<char>,
    indices: Vec<u32>,
}

impl Default for NameMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl NameMatcher {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(MatcherConfig::DEFAULT),
            buf: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub fn score(&mut self, pattern: &Pattern, text: &str) -> Option<(u32, Vec<(u32, u32)>)> {
        self.indices.clear();
        let score = pattern.indices(haystack(text, &mut self.buf), &mut self.matcher, &mut self.indices)?;
        self.indices.sort_unstable();
        self.indices.dedup();
        is_clean_match(text, &self.indices).then(|| (score, utf16_ranges(text, &self.indices)))
    }
}

pub struct Searcher {
    names: NameMatcher,
}

impl Default for Searcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Searcher {
    pub fn new() -> Self {
        Self {
            names: NameMatcher::new(),
        }
    }

    pub fn search(
        &mut self,
        query: &str,
        apps: &[AppEntry],
        history: &History,
        config: &Config,
        now: u64,
    ) -> Vec<ResultItem> {
        let query = query.trim();
        let mut results = Vec::new();

        if query.is_empty() {
            if config.show_recent {
                let mut recent: Vec<_> = apps
                    .iter()
                    .map(|app| (history.boost(&app.id, now), app))
                    .filter(|(boost, _)| *boost > 0)
                    .collect();
                recent.sort_by_key(|&(boost, _)| std::cmp::Reverse(boost));
                results.extend(
                    recent
                        .into_iter()
                        .take(config.max_results)
                        .map(|(_, app)| app_result(app, Vec::new())),
                );
            }
            return results;
        }

        if query.starts_with('?') {
            return results;
        }

        if let Some(command) = query.strip_prefix('>') {
            let command = command.trim();
            results.push(ResultItem::new(
                Kind::Shell,
                if command.is_empty() {
                    "Type a command to run"
                } else {
                    command
                },
                "Command Prompt · Tab to run as administrator",
                Action::Shell {
                    command: command.into(),
                    admin: false,
                },
            ));
            return results;
        }

        if let Some(value) = calc::evaluate(query) {
            let text = calc::format(value);
            results.push(ResultItem::new(
                Kind::Calc,
                text.clone(),
                format!("{query} · Enter to copy"),
                Action::Copy(text),
            ));
        }

        if let Some(url) = as_url(query) {
            results.push(ResultItem::new(
                Kind::Url,
                url.clone(),
                "Open in browser",
                Action::Open(url),
            ));
        }

        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let lower = query.to_lowercase();
        let mut scored: Vec<_> = apps
            .iter()
            .filter_map(|app| {
                let (score, highlights) = self.names.score(&pattern, &app.name)?;
                let prefix = if app.name.to_lowercase().starts_with(&lower) {
                    40
                } else {
                    0
                };
                Some((score + prefix + history.boost(&app.id, now), app, highlights))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.len().cmp(&b.1.name.len())));
        for (_, app, highlights) in scored.into_iter().take(MAX_APP_RESULTS) {
            results.push(app_result(app, highlights));
        }

        results.push(ResultItem::new(
            Kind::Web,
            format!("Search the web for \u{201C}{query}\u{201D}"),
            host(&config.search_url).unwrap_or("Web search"),
            Action::Open(config.search_url.replace("{query}", &url_encode(query))),
        ));
        results
    }
}

pub fn app_result(app: &AppEntry, highlights: Vec<(u32, u32)>) -> ResultItem {
    let mut item = ResultItem::new(
        Kind::App,
        app.name.clone(),
        app.path.clone().unwrap_or_else(|| "App".into()),
        Action::Launch {
            id: app.id.clone(),
            path: app.path.clone(),
            admin: false,
        },
    );
    item.highlights = highlights;
    item.icon = Some(app.id.clone());
    item
}

pub fn file_result(hit: FileHit) -> ResultItem {
    let kind = if hit.is_folder { Kind::Folder } else { Kind::File };
    let mut item = ResultItem::new(kind, hit.name, hit.path.clone(), Action::Open(hit.path));
    item.highlights = hit.highlights;
    item.icon = Some(hit.icon_key);
    item
}

fn haystack<'a>(text: &'a str, buf: &'a mut Vec<char>) -> Utf32Str<'a> {
    if text.is_ascii() {
        Utf32Str::Ascii(text.as_bytes())
    } else {
        buf.clear();
        buf.extend(text.chars());
        Utf32Str::Unicode(buf)
    }
}

pub fn is_clean_match(text: &str, char_indices: &[u32]) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let word_start = |i: usize| {
        i == 0 || !chars[i - 1].is_alphanumeric() || (chars[i - 1].is_lowercase() && chars[i].is_uppercase())
    };
    let mid_word_runs = char_indices
        .iter()
        .enumerate()
        .filter(|&(n, &i)| n == 0 || char_indices[n - 1] + 1 != i)
        .filter(|&(_, &i)| !word_start(i as usize))
        .count();
    mid_word_runs <= 1
}

pub fn utf16_ranges(text: &str, char_indices: &[u32]) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    let mut wanted = char_indices.iter().peekable();
    let mut offset = 0u32;
    for (i, c) in text.chars().enumerate() {
        let len = c.len_utf16() as u32;
        if wanted.next_if(|&&w| w as usize == i).is_some() {
            match ranges.last_mut() {
                Some((start, l)) if *start + *l == offset => *l += len,
                _ => ranges.push((offset, len)),
            }
        }
        offset += len;
    }
    ranges
}

pub fn as_url(query: &str) -> Option<String> {
    if query.contains(char::is_whitespace) {
        return None;
    }
    let lower = query.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(query.into())
    } else if lower.starts_with("www.") && query.len() > 4 {
        Some(format!("https://{query}"))
    } else {
        None
    }
}

pub fn host(url: &str) -> Option<&str> {
    let rest = url.split("://").nth(1)?;
    rest.split(['/', '?']).next()
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppEntry {
        AppEntry {
            name: name.into(),
            id: name.into(),
            path: None,
        }
    }

    #[test]
    fn ranks_and_highlights_apps() {
        let apps = [
            app("Visual Studio Code"),
            app("Notepad"),
            app("Visual Studio Installer"),
        ];
        let mut searcher = Searcher::new();
        let results = searcher.search("vsc", &apps, &History::default(), &Config::default(), 0);
        assert_eq!(results[0].title, "Visual Studio Code");
        assert_eq!(results[0].highlights, vec![(0, 1), (7, 1), (14, 1)]);
        assert_eq!(results.last().unwrap().kind, Kind::Web);
    }

    #[test]
    fn test_urls_and_calculations() {
        assert_eq!(as_url("https://github.com"), Some("https://github.com".into()));
        assert_eq!(as_url("www.rust-lang.org"), Some("https://www.rust-lang.org".into()));
        assert_eq!(as_url("not a url"), None);
    }
}
