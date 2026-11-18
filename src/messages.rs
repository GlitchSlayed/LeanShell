//! Every WM_APP message LeanShell uses, in one place so no two share a number.
//! 3, 4 and 6 are also sent by other processes (a second instance, the settings
//! app), so they must keep their values.

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

/// The tray icon's callback.
#[allow(dead_code)]
pub const WM_TRAY: u32 = WM_APP + 1;
/// The app index has news.
pub const WM_APP_INDEX: u32 = WM_APP + 2;
/// Show the launcher; also sent by another instance started twice.
pub const WM_APP_SHOW: u32 = WM_APP + 3;
/// Quit; also sent by the settings app or `--restore-shell`.
pub const WM_APP_QUIT: u32 = WM_APP + 4;
/// File search results arrived.
pub const WM_APP_FILES: u32 = WM_APP + 5;
/// Reload the config; also sent by the settings app.
pub const WM_APP_RELOAD: u32 = WM_APP + 6;
/// Command state changed.
pub const WM_APP_COMMANDS: u32 = WM_APP + 7;
/// The next animation frame.
pub const WM_APP_FRAME: u32 = WM_APP + 8;
/// Toggle the launcher (posted by the low-level Win-key hook).
pub const WM_APP_TOGGLE: u32 = WM_APP + 9;
pub const WM_SHOW_LAUNCHER: u32 = WM_APP + 20;
pub const WM_REFRESH: u32 = WM_APP + 21;
pub const WM_TRAY_CHANGED: u32 = WM_APP + 22;
pub const WM_DOCK_SHOW_TASKBAR: u32 = WM_APP + 23;
pub const WM_CLOSE_FLYOUT: u32 = WM_APP + 24;
#[allow(dead_code)]
pub const WM_SHOW_NOW: u32 = WM_APP + 25;
#[allow(dead_code)]
pub const WM_RADIOS: u32 = WM_APP + 26;
#[allow(dead_code)]
pub const WM_DOCK_DRAIN: u32 = WM_APP + 27;
#[allow(dead_code)]
pub const WM_DOCK_WAKE: u32 = WM_APP + 28;
pub const WM_ARTWORK: u32 = WM_APP + 29;
/// The volume, mute or default output device changed.
pub const WM_DOCK_STATUS: u32 = WM_APP + 30;
/// Foreground window changed (WinEventHook notification to dock).
pub const WM_FOREGROUND_CHANGED: u32 = WM_APP + 31;
#[allow(dead_code)]
pub const WM_TASKBAR_SHOW: u32 = WM_APP + 40;
#[allow(dead_code)]
pub const WM_RECOVERY_DONE: u32 = WM_APP + 41;
/// The dock space's app bar callback.
pub const WM_APPBAR: u32 = WM_APP + 50;
pub const WM_CHECK: u32 = WM_APP + 51;
pub const WM_SET_HEIGHT: u32 = WM_APP + 52;
#[allow(dead_code)]
pub const WM_OUTRANK: u32 = WM_APP + 60;
/// A claimed Win+chord or hardware volume key event from `WH_KEYBOARD_LL`.
pub const WM_SHELL_CHORD: u32 = WM_APP + 61;
/// Window lifecycle notification to schedule an idle `explorer.exe` reap check.
pub const WM_EXPLORER_REAP_CHECK: u32 = WM_APP + 62;
/// External window move/size lifecycle event (`EVENT_SYSTEM_MOVESIZESTART`/`END`).
pub const WM_WM_MOVESIZE: u32 = WM_APP + 63;
/// Window management action request (`WPARAM`: 1 = Task View, 2 = Toggle Tiling, 3 = Retile).
pub const WM_WM_ACTION: u32 = WM_APP + 64;
/// Background auto-update check/stage result notification.
pub const WM_APP_UPDATE_READY: u32 = WM_APP + 65;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_numbers_are_unique() {
        let all = [
            WM_TRAY,
            WM_APP_INDEX,
            WM_APP_SHOW,
            WM_APP_QUIT,
            WM_APP_FILES,
            WM_APP_RELOAD,
            WM_APP_COMMANDS,
            WM_APP_FRAME,
            WM_APP_TOGGLE,
            WM_SHOW_LAUNCHER,
            WM_REFRESH,
            WM_TRAY_CHANGED,
            WM_DOCK_SHOW_TASKBAR,
            WM_CLOSE_FLYOUT,
            WM_SHOW_NOW,
            WM_RADIOS,
            WM_DOCK_DRAIN,
            WM_DOCK_WAKE,
            WM_ARTWORK,
            WM_DOCK_STATUS,
            WM_FOREGROUND_CHANGED,
            WM_TASKBAR_SHOW,
            WM_RECOVERY_DONE,
            WM_APPBAR,
            WM_CHECK,
            WM_SET_HEIGHT,
            WM_OUTRANK,
            WM_SHELL_CHORD,
            WM_EXPLORER_REAP_CHECK,
            WM_WM_MOVESIZE,
            WM_WM_ACTION,
            WM_APP_UPDATE_READY,
        ];
        let mut sorted = all.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
    }
}
