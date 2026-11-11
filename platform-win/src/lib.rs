//! Native Win32 platform layer for LeanShell.
//!
//! Encapsulates HWND window management, AppBar registration, DWM backdrop composition,
//! keyboard hooks, and notification-area tray hosting.

pub mod appbar;
pub mod backdrop;
pub mod hook;
pub mod hotkey;

pub use appbar::AppBar;
pub use backdrop::apply_backdrop;
pub use hook::{WinKeyHook, WinKeyTracker};
pub use hotkey::HotkeyManager;
