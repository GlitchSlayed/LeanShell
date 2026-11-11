//! Low-level keyboard hook (`WH_KEYBOARD_LL`) to claim the bare Windows key.
//!
//! Intercepts bare Win key presses on a dedicated thread, passing through all
//! chords (Win+E, Win+R, Win+D, Win+L, etc.) untouched. When a bare Win tap
//! is detected, it swallows the key release, injects an innocuous key event to
//! neutralize Explorer's Start menu trigger, and notifies the launcher.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, VK_LWIN, VK_RWIN, keybd_event};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, MSG, PostMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::{Error, Result};

/// Extra info tag identifying synthetic events emitted by this hook.
pub const EXTRA_INFO_DUMMY: usize = 0x5749_5350; // "WISP"

/// Custom message sent to the launcher window when bare Win is tapped.
/// Using WM_APP + 3 (WM_APP_SHOW) directly toggles the launcher.
pub const WM_APP_SHOW: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 3;

pub use leanshell_core::hook::{HookDecision, WinKeyTracker};

static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static CHORD_DETECTED: AtomicBool = AtomicBool::new(false);
static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn ll_keyboard_proc(n_code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: Low-level keyboard hook callback called by Windows on our dedicated thread.
    // The pointer lparam points to a valid KBDLLHOOKSTRUCT allocated by the OS for this event.
    // Must return fast, never block, allocate, or lock.
    unsafe {
        if n_code < 0 {
            return CallNextHookEx(None, n_code, wparam, lparam);
        }
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if kbd.dwExtraInfo == EXTRA_INFO_DUMMY {
            return CallNextHookEx(None, n_code, wparam, lparam);
        }

        let vk = kbd.vkCode;
        let is_win = vk == VK_LWIN.0 as u32 || vk == VK_RWIN.0 as u32;
        let msg = wparam.0 as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

        if is_win && is_down {
            WIN_DOWN.store(true, Ordering::Relaxed);
            CHORD_DETECTED.store(false, Ordering::Relaxed);
            return CallNextHookEx(None, n_code, wparam, lparam);
        }

        if !is_win && is_down && WIN_DOWN.load(Ordering::Relaxed) {
            CHORD_DETECTED.store(true, Ordering::Relaxed);
            return CallNextHookEx(None, n_code, wparam, lparam);
        }

        if is_win && is_up {
            WIN_DOWN.store(false, Ordering::Relaxed);
            let chord = CHORD_DETECTED.swap(false, Ordering::Relaxed);
            if chord {
                return CallNextHookEx(None, n_code, wparam, lparam);
            }

            // Bare Windows key tap!
            // Inject an innocuous dummy key event so Windows Explorer's Start menu trigger sees an intervening key.
            keybd_event(0, 0, KEYEVENTF_KEYUP, EXTRA_INFO_DUMMY);

            let target = TARGET_HWND.load(Ordering::Relaxed);
            if target != 0 {
                let _ = PostMessageW(Some(HWND(target as _)), WM_APP_SHOW, WPARAM(0), LPARAM(0));
            }

            // Swallow the Windows key release.
            return LRESULT(1);
        }

        CallNextHookEx(None, n_code, wparam, lparam)
    }
}

pub struct WinKeyHook {
    thread: Option<std::thread::JoinHandle<()>>,
    thread_id: u32,
}

impl WinKeyHook {
    pub fn new(target_hwnd: HWND) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let target_raw = target_hwnd.0 as usize;
        let thread = std::thread::Builder::new()
            .name("leanshell-winkey-hook".into())
            .spawn(move || {
                run_hook_loop(target_raw, tx);
            })
            .map_err(|e| Error::new(windows::Win32::Foundation::E_FAIL, e.to_string()))?;

        let thread_id = rx
            .recv()
            .map_err(|_| Error::new(windows::Win32::Foundation::E_FAIL, "Hook thread setup failed"))?;

        Ok(Self {
            thread: Some(thread),
            thread_id,
        })
    }
}

impl Drop for WinKeyHook {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = thread.join();
        }
    }
}

fn run_hook_loop(target_raw: usize, ready: std::sync::mpsc::Sender<u32>) {
    TARGET_HWND.store(target_raw as isize, Ordering::Relaxed);
    unsafe {
        let thread_id = GetCurrentThreadId();
        let module = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0));
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), module, 0);
        match hook {
            Ok(h) => {
                HOOK_HANDLE.store(h.0 as isize, Ordering::Relaxed);
                let _ = ready.send(thread_id);
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = UnhookWindowsHookEx(h);
                HOOK_HANDLE.store(0, Ordering::Relaxed);
            }
            Err(e) => {
                eprintln!("Failed to install Win key hook: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_key_tracker_bare_win() {
        let mut tracker = WinKeyTracker::new();
        // Win down
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        // Win up (bare) -> ClaimBareWin!
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::ClaimBareWin);
    }

    #[test]
    fn test_win_key_tracker_chord_win_e() {
        let mut tracker = WinKeyTracker::new();
        // Win down
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        // 'E' down (chord)
        assert_eq!(tracker.handle_key(0x45, true), HookDecision::PassThrough);
        // 'E' up
        assert_eq!(tracker.handle_key(0x45, false), HookDecision::PassThrough);
        // Win up -> PassThrough (NOT bare Win!)
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_win_key_tracker_regular_typing() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x41, true), HookDecision::PassThrough); // 'A' down
        assert_eq!(tracker.handle_key(0x41, false), HookDecision::PassThrough); // 'A' up
        assert_eq!(tracker.handle_key(0x42, true), HookDecision::PassThrough); // 'B' down
        assert_eq!(tracker.handle_key(0x42, false), HookDecision::PassThrough); // 'B' up
    }
}
