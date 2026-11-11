//! Native Win32 platform layer for LeanShell.
//!
//! Encapsulates HWND window management, AppBar registration, DWM backdrop composition,
//! keyboard hooks, and notification-area tray hosting.

pub mod appbar;
pub mod backdrop;
pub mod hook;
pub mod hotkey;
pub mod shell;
pub mod window_hook;

pub use appbar::AppBar;
pub use backdrop::apply_backdrop;
pub use hook::{WinKeyHook, WinKeyTracker};
pub use hotkey::HotkeyManager;
pub use shell::{
    is_exclusive_shell_registered, is_safe_mode_requested, register_exclusive_shell, unregister_exclusive_shell,
};
pub use window_hook::WindowHook;
