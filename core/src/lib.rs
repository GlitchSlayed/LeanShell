//! Pure core crate for LeanShell.
//!
//! Contains data models, configuration, search grammar, launch history, and calculator logic.
//! Strictly decoupled from Win32 platform types and UI drawing libraries.

pub mod calc;
pub mod command;
pub mod config;
pub mod glyphs;
pub mod history;
pub mod hook;
pub mod model;
pub mod search;
pub mod startup_cmd;

pub use calc::{evaluate as eval_calc, format as format_calc};
pub use command::{MediaControl, PowerAction, SystemCommand};
pub use config::{Backdrop, Config, DisplayMode, PanelEdge, Position, ShellMode, Theme, WindowListMode};
pub use history::History;
pub use hook::{HookDecision, WinKeyTracker};
pub use model::{AppEntry, Desktops, FileHit, IconPixels, Pin, TrayIcon, TrayKey};
pub use search::{Action, Drill, FileScope, Kind, ResultItem, Searcher};
