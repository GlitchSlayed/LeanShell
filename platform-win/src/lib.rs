//! Native Win32 platform layer for LeanShell.
//!
//! Encapsulates HWND window management, AppBar registration, DWM backdrop composition,
//! keyboard hooks, and notification-area tray hosting.

pub mod appbar;
pub mod backdrop;
pub mod hook;
pub mod hosts;
pub mod hotkey;
pub mod reaper;
pub mod shell;
pub mod shell_menu;
pub mod window_hook;
pub mod wm;

pub use appbar::AppBar;
pub use backdrop::apply_backdrop;
pub use hook::{ShellChord, WM_SHELL_CHORD, WinKeyHook, WinKeyTracker};
pub use hosts::{is_host_neutered, neuter_shell_hosts, restore_shell_hosts, terminate_processes_by_name};
pub use hotkey::HotkeyManager;
pub use reaper::{inspect_explorer_windows, is_explorer_running, reap_idle_explorer};
pub use shell::{
    get_backup_shell, get_registered_shell, is_exclusive_shell_registered, is_safe_mode_requested,
    is_session_exclusive, register_exclusive_shell, unregister_exclusive_shell,
};
pub use shell_menu::show_shell_context_menu;
pub use window_hook::WindowHook;
pub use wm::{
    MoveSizeHook, active_monitor_work_rect, apply_snap_zone, cursor_monitor_work_rect, get_visible_window_rect,
    is_manageable_window, list_tileable_windows_on_monitor, monitor_work_rect_for_window, parse_colorref,
    reset_all_window_borders, set_window_border_color, set_window_rect_exact,
};
