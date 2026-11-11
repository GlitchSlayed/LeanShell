//! Window lifecycle and task tracking hooks using `SetWinEventHook`.
//!
//! Event-driven window monitoring inspired by `ManagedShell.WindowsTasks`.
//! Replaces polling with direct OS notifications for:
//! - Foreground window switch (`EVENT_SYSTEM_FOREGROUND`)
//! - Window creation / destruction (`EVENT_OBJECT_CREATE`, `EVENT_OBJECT_DESTROY`)
//! - Window visibility changes (`EVENT_OBJECT_SHOW`, `EVENT_OBJECT_HIDE`)
//! - Window title changes (`EVENT_OBJECT_NAMECHANGE`)
//! - Minimize / restore (`EVENT_SYSTEM_MINIMIZESTART`, `EVENT_SYSTEM_MINIMIZEEND`)

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_OBJECT_CREATE, EVENT_OBJECT_NAMECHANGE, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, PostMessageW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};
use windows::core::{Error, Result};

static TARGET_HWND: AtomicIsize = AtomicIsize::new(0);
static REFRESH_MSG: AtomicU32 = AtomicU32::new(0);
static FOREGROUND_MSG: AtomicU32 = AtomicU32::new(0);

/// Callback invoked by Windows whenever an observed window lifecycle event occurs.
unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    // We only care about top-level window objects (OBJID_WINDOW is 0, child is 0).
    // SAFETY: Called by Windows on the message loop thread. Must never block or allocate.
    if id_object != 0 || id_child != 0 || hwnd.is_invalid() {
        return;
    }

    let target = TARGET_HWND.load(Ordering::Relaxed);
    if target == 0 {
        return;
    }

    if event == EVENT_SYSTEM_FOREGROUND {
        let fg_msg = FOREGROUND_MSG.load(Ordering::Relaxed);
        if fg_msg != 0 {
            // SAFETY: PostMessageW is called with our stored HWND and message ID.
            unsafe {
                let _ = PostMessageW(Some(HWND(target as _)), fg_msg, WPARAM(0), LPARAM(hwnd.0 as isize));
            }
        }
    } else {
        let refresh_msg = REFRESH_MSG.load(Ordering::Relaxed);
        if refresh_msg != 0 {
            // SAFETY: PostMessageW is called with our stored HWND and message ID.
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(target as _)),
                    refresh_msg,
                    WPARAM(event as usize),
                    LPARAM(hwnd.0 as isize),
                );
            }
        }
    }
}

/// RAII handle managing active `SetWinEventHook` registrations.
///
/// Unregisters all installed hooks automatically on drop.
pub struct WindowHook {
    hooks: Vec<HWINEVENTHOOK>,
}

impl WindowHook {
    /// Installs out-of-context WinEvent hooks on the current thread's message queue.
    ///
    /// When events occur, `win_event_proc` posts `refresh_msg` or `foreground_msg`
    /// to `target_hwnd` for debounced processing.
    pub fn install(target_hwnd: HWND, refresh_msg: u32, foreground_msg: u32) -> Result<Self> {
        TARGET_HWND.store(target_hwnd.0 as isize, Ordering::Relaxed);
        REFRESH_MSG.store(refresh_msg, Ordering::Relaxed);
        FOREGROUND_MSG.store(foreground_msg, Ordering::Relaxed);

        let mut hooks = Vec::with_capacity(3);

        // 1. Foreground window changes (instant active pill update)
        // SAFETY: Registering an out-of-context hook on the calling thread's message loop.
        let fg_hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if !fg_hook.is_invalid() {
            hooks.push(fg_hook);
        }

        // 2. Minimize and restore events
        // SAFETY: Registering out-of-context hook for window minimize / restore state transitions.
        let min_hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_SYSTEM_MINIMIZEEND,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if !min_hook.is_invalid() {
            hooks.push(min_hook);
        }

        // 3. Window creation, destruction, show, hide, and title changes
        // SAFETY: Registering out-of-context hook for top-level window lifecycle events.
        let obj_hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_CREATE,
                EVENT_OBJECT_NAMECHANGE,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if !obj_hook.is_invalid() {
            hooks.push(obj_hook);
        }

        if hooks.is_empty() {
            TARGET_HWND.store(0, Ordering::Relaxed);
            REFRESH_MSG.store(0, Ordering::Relaxed);
            FOREGROUND_MSG.store(0, Ordering::Relaxed);
            return Err(Error::from_thread());
        }

        Ok(Self { hooks })
    }
}

impl Drop for WindowHook {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            if !hook.is_invalid() {
                // SAFETY: Unregistering hook handle previously returned by SetWinEventHook.
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            }
        }
        TARGET_HWND.store(0, Ordering::Relaxed);
        REFRESH_MSG.store(0, Ordering::Relaxed);
        FOREGROUND_MSG.store(0, Ordering::Relaxed);
    }
}
