//! Low-level keyboard & mouse hooks (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`) to claim the bare Windows key.
//!
//! Intercepts bare Win key presses on a dedicated thread, passing through all
//! chords (Win+E, Win+R, Win+D, Win+L, Win+click/wheel, etc.) untouched.
//! Elevated foreground apps can block the hook or SendInput through UIPI;
//! Start then opens as usual. Never swallow a physical key, including Win-down.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{E_FAIL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED,
    MSG, MSLLHOOKSTRUCT, PostMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WM_QUIT,
    WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN,
};
use windows::core::{Error, Result};

/// Custom message sent to the launcher window when bare Win is tapped.
/// Matches `crate::messages::WM_APP_TOGGLE` (`WM_APP + 9`).
pub const WM_APP_TOGGLE: u32 = WM_APP + 9;

/// Custom message sent to the launcher window when explicitly requested to show.
pub const WM_APP_SHOW: u32 = WM_APP + 3;

/// Custom message sent to the launcher window when a claimed shell chord or volume key fires.
/// Matches `crate::messages::WM_SHELL_CHORD` (`WM_APP + 61`).
pub const WM_SHELL_CHORD: u32 = WM_APP + 61;

pub use leanshell_core::hook::{HookDecision, ShellChord, WinKeyTracker};

thread_local! {
    static TRACKER: Cell<WinKeyTracker> = const {
        Cell::new(WinKeyTracker::new())
    };
}

// Note: WinKeyTracker::new() works in thread_local too; keep Cell<WinKeyTracker> clean.
static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static CLAIM_CHORDS: AtomicBool = AtomicBool::new(false);

fn send_win_mask_pair() -> bool {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0xE8),
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
    // SAFETY: Fixed stack array of two INPUT structs for unassigned VK 0xE8 chord masking.
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) == 2 }
}

unsafe extern "system" fn ll_keyboard_proc(n_code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: Low-level keyboard hook callback called by Windows on our dedicated thread.
    // The pointer lparam points to a valid KBDLLHOOKSTRUCT allocated by the OS for this event.
    // Must return fast, never block, allocate, or lock.
    unsafe {
        if n_code == HC_ACTION as i32 {
            let event = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let message = wparam.0 as u32;
            if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP) {
                let is_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
                let injected = event.flags.contains(LLKHF_INJECTED);
                let claim_chords = CLAIM_CHORDS.load(Ordering::Acquire);
                let (chord, tap) = TRACKER.with(|state| {
                    let mut tracker = state.get();
                    let chord = tracker.check_chord(event.vkCode, is_down, injected, claim_chords);
                    let tap = tracker.key(event.vkCode, is_down, injected);
                    state.set(tracker);
                    (chord, tap)
                });
                if let Some(chord) = chord {
                    let target = TARGET_HWND.load(Ordering::Acquire);
                    if chord.pass_through_key() {
                        if target != 0 {
                            let _ = PostMessageW(
                                Some(HWND(target as _)),
                                WM_SHELL_CHORD,
                                WPARAM(chord as usize),
                                LPARAM(0),
                            );
                        }
                    } else if !chord.requires_win_mask() || send_win_mask_pair() {
                        if target != 0 {
                            let _ = PostMessageW(
                                Some(HWND(target as _)),
                                WM_SHELL_CHORD,
                                WPARAM(chord as usize),
                                LPARAM(0),
                            );
                        }
                        return LRESULT(1);
                    }
                }
                if tap {
                    // Send a fixed unassigned VK (0xE8) pair before forwarding Win-up so Windows
                    // sees an intervening chord instead of a bare Start-menu trigger.
                    // Check SendInput == 2 so UIPI blocks let the default behavior run cleanly.
                    if send_win_mask_pair() {
                        let target = TARGET_HWND.load(Ordering::Acquire);
                        if target != 0 {
                            let _ = PostMessageW(Some(HWND(target as _)), WM_APP_TOGGLE, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, n_code, wparam, lparam)
    }
}

unsafe extern "system" fn ll_mouse_proc(n_code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: Low-level mouse hook callback on the hook thread; pointer lparam is valid MSLLHOOKSTRUCT.
    unsafe {
        if n_code == HC_ACTION as i32
            && matches!(
                wparam.0 as u32,
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_MOUSEWHEEL | WM_MOUSEHWHEEL
            )
        {
            let event = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            if event.flags & LLMHF_INJECTED == 0 {
                TRACKER.with(|state| {
                    let mut tracker = state.get();
                    tracker.click();
                    state.set(tracker);
                });
            }
        }
        // Preserve Win+click and Win+wheel. Pointer movement alone allows a tap.
        CallNextHookEx(None, n_code, wparam, lparam)
    }
}

pub struct WinKeyHook {
    thread: Option<std::thread::JoinHandle<()>>,
    thread_id: u32,
}

impl WinKeyHook {
    pub fn set_claim_chords(enabled: bool) {
        CLAIM_CHORDS.store(enabled, Ordering::Release);
    }

    pub fn new(target_hwnd: HWND) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        let target_raw = target_hwnd.0 as usize;
        let thread = std::thread::Builder::new()
            .name("leanshell-winkey-hook".into())
            .spawn(move || {
                run_hook_loop(target_raw, tx);
            })
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?;

        match rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                thread: Some(thread),
                thread_id,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(Error::new(E_FAIL, "Win-key hook thread stopped during startup"))
            }
        }
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

fn run_hook_loop(target_raw: usize, ready: std::sync::mpsc::Sender<Result<u32>>) {
    TARGET_HWND.store(target_raw as isize, Ordering::Release);
    unsafe {
        let installed = (|| -> Result<(HHOOK, HHOOK)> {
            let module = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0));
            let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), module, 0)?;
            match SetWindowsHookExW(WH_MOUSE_LL, Some(ll_mouse_proc), module, 0) {
                Ok(mouse) => Ok((keyboard, mouse)),
                Err(error) => {
                    let _ = UnhookWindowsHookEx(keyboard);
                    Err(error)
                }
            }
        })();

        let (keyboard, mouse) = match installed {
            Ok(hooks) => hooks,
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };

        // Seed held keys when enabling midway through a chord.
        let mut tracker = WinKeyTracker::new();
        for vk in 8..256 {
            // Low-level events identify left/right modifiers, not their generic aliases.
            if !matches!(vk, 0x10..=0x12) && GetAsyncKeyState(vk) < 0 {
                tracker.key(vk as u32, true, false);
            }
        }
        tracker.clear_candidate();
        TRACKER.with(|state| state.set(tracker));

        let _ = ready.send(Ok(GetCurrentThreadId()));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(keyboard);
        let _ = UnhookWindowsHookEx(mouse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_key_tracker_bare_win() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::ClaimBareWin);
    }

    #[test]
    fn test_win_key_tracker_chord_win_e() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x45, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x45, false), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_win_key_tracker_regular_typing() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x41, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x41, false), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x42, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x42, false), HookDecision::PassThrough);
    }
}
