//! DWM backdrop, dark mode, and window composition management.

use leanshell_core::config::Backdrop;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Applies the requested DWM backdrop material (Mica, Acrylic, or Solid) and Dark Mode.
///
/// Acrylic on inactive windows requires composition blur attributes so DWM does not
/// drop transparency when focus shifts away.
pub fn apply_backdrop(hwnd: HWND, dark: bool, backdrop: Backdrop, persistent: bool) -> bool {
    let translucent = backdrop != Backdrop::Solid && transparency_available();
    unsafe {
        let set = |attr, value: i32| DwmSetWindowAttribute(hwnd, attr, (&value as *const i32).cast(), 4);
        let _ = set(DWMWA_USE_IMMERSIVE_DARK_MODE, i32::from(dark));
        let _ = set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND.0);
        let _ = set(DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE as i32);

        let accent = persistent && backdrop == Backdrop::Acrylic && translucent;
        let material = if !translucent || accent {
            DWMSBT_NONE
        } else {
            match backdrop {
                Backdrop::Acrylic => DWMSBT_TRANSIENTWINDOW,
                Backdrop::Mica => DWMSBT_MAINWINDOW,
                Backdrop::Solid => DWMSBT_NONE,
            }
        };
        let _ = set(DWMWA_SYSTEMBACKDROP_TYPE, material.0);

        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_STYLE,
            if persistent {
                style & !(WS_CAPTION.0 as isize)
            } else {
                style | WS_CAPTION.0 as isize
            },
        );
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    translucent
}

pub fn transparency_available() -> bool {
    unsafe { DwmIsCompositionEnabled().map(|b| b.as_bool()).unwrap_or(false) }
}
