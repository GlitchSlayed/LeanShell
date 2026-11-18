//! On-demand `explorer.exe` auto-reaper for exclusive shell mode.
//!
//! When LeanShell owns `GetShellWindow()`, `explorer.exe` may be launched on-demand
//! for file browsing (`Win+E`, folder views). Once all folder windows (`CabinetWClass`,
//! `ExploreWClass`) and active file dialogs (`OperationStatusWindow`, `#32770`) close,
//! `reap_idle_explorer()` terminates the remaining idle `explorer.exe` host process.

use leanshell_core::reaper::{ExplorerWindowKind, ReapDecision, classify_explorer_window, evaluate_reap};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible};
use windows::core::BOOL;

fn explorer_pids() -> Vec<u32> {
    crate::hosts::find_pids_by_name(&["explorer.exe"])
}

struct WindowCounts<'a> {
    pids: &'a [u32],
    folders: usize,
    dialogs: usize,
}

unsafe extern "system" fn count_explorer_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam points to a stack-allocated WindowCounts for the duration of EnumWindows.
    let counts = unsafe { &mut *(lparam.0 as *mut WindowCounts<'_>) };
    // SAFETY: Querying window visibility and owning PID on a valid enumerated HWND.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if !counts.pids.contains(&pid) {
            return true.into();
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut buf);
        if len <= 0 {
            return true.into();
        }
        let class = String::from_utf16_lossy(&buf[..len as usize]);
        match classify_explorer_window(&class) {
            ExplorerWindowKind::Folder => counts.folders += 1,
            ExplorerWindowKind::FileDialog => counts.dialogs += 1,
            ExplorerWindowKind::Other => {}
        }
    }
    true.into()
}

/// Returns `true` if at least one `explorer.exe` process is currently running.
pub fn is_explorer_running() -> bool {
    !explorer_pids().is_empty()
}

fn count_explorer_windows_for_pids(pids: &[u32]) -> (usize, usize) {
    let mut counts = WindowCounts {
        pids,
        folders: 0,
        dialogs: 0,
    };
    // SAFETY: Enumerating top-level windows synchronously with valid mutable WindowCounts pointer.
    unsafe {
        let _ = EnumWindows(Some(count_explorer_windows), LPARAM(&raw mut counts as isize));
    }
    (counts.folders, counts.dialogs)
}

/// Inspects running `explorer.exe` processes and visible folder/dialog windows.
///
/// Returns `(explorer_running, open_folder_windows, active_file_dialogs)`.
pub fn inspect_explorer_windows() -> (bool, usize, usize) {
    let pids = explorer_pids();
    if pids.is_empty() {
        return (false, 0, 0);
    }
    let (folders, dialogs) = count_explorer_windows_for_pids(&pids);
    (true, folders, dialogs)
}

/// Reaps idle `explorer.exe` instances if and only if LeanShell exclusively owns
/// the desktop shell, `auto_reap_enabled` is true, and no folder windows or
/// file-operation dialogs are currently visible.
pub fn reap_idle_explorer(auto_reap_enabled: bool) -> bool {
    if !auto_reap_enabled || !crate::shell::is_session_exclusive() {
        return false;
    }
    let pids = explorer_pids();
    if pids.is_empty() {
        return false;
    }
    let (folders, dialogs) = count_explorer_windows_for_pids(&pids);
    if evaluate_reap(true, auto_reap_enabled, true, folders, dialogs) != ReapDecision::ScheduleReap {
        return false;
    }
    crate::hosts::terminate_pids(&pids) > 0
}
