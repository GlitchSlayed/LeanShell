//! Native Win32 window management primitives: DWM active border styling,
//! invisible-frame-compensated placement, monitor work-area queries, tileable
//! window enumeration, and `EVENT_SYSTEM_MOVESIZESTART`/`END` hooks.

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use leanshell_core::wm::{SnapZone, WmRect};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CLOAKED, DWMWA_COLOR_DEFAULT, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
    MonitorFromWindow,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, Error, Result};

/// Parses `#RRGGBB` or `#RRGGBBAA` hex text into a Win32 `COLORREF` (`0x00BBGGRR`).
pub fn parse_colorref(hex: &str) -> Option<u32> {
    let hex = hex.trim().strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()? as u32;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()? as u32;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()? as u32;
    Some((b << 16) | (g << 8) | r)
}

/// Applies or clears the Windows 11 DWM border color (`DWMWA_BORDER_COLOR`) on `hwnd`.
pub fn set_window_border_color(hwnd: HWND, colorref: Option<u32>) {
    // SAFETY: Checking HWND validity and calling DwmSetWindowAttribute with a stack u32.
    unsafe {
        if hwnd.is_invalid() || !IsWindow(Some(hwnd)).as_bool() {
            return;
        }
        let value = colorref.unwrap_or(DWMWA_COLOR_DEFAULT);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            (&raw const value).cast(),
            size_of::<u32>() as u32,
        );
    }
}

/// Resets `DWMWA_BORDER_COLOR` back to `DWMWA_COLOR_DEFAULT` on all visible top-level windows.
pub fn reset_all_window_borders() {
    unsafe extern "system" fn reset_one(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        // SAFETY: Called synchronously by EnumWindows on each top-level window.
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != std::process::id() {
                    let value = DWMWA_COLOR_DEFAULT;
                    let _ = DwmSetWindowAttribute(
                        hwnd,
                        DWMWA_BORDER_COLOR,
                        (&raw const value).cast(),
                        size_of::<u32>() as u32,
                    );
                }
            }
        }
        true.into()
    }
    // SAFETY: Enumerating top-level windows synchronously to reset DWM border overrides.
    unsafe {
        let _ = EnumWindows(Some(reset_one), LPARAM(0));
    }
}

fn rect_to_wm(r: RECT) -> WmRect {
    WmRect::new(r.left, r.top, r.right, r.bottom)
}

/// Returns the visible DWM extended frame bounds of `hwnd` (or `GetWindowRect` fallback).
pub fn get_visible_window_rect(hwnd: HWND) -> Option<WmRect> {
    // SAFETY: Querying DWM extended frame bounds or standard window rect for a live HWND.
    unsafe {
        if hwnd.is_invalid() || !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let mut frame = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            size_of::<RECT>() as u32,
        )
        .is_ok()
        {
            return Some(rect_to_wm(frame));
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        Some(rect_to_wm(rect))
    }
}

/// Moves and resizes `hwnd` to match `target` visually, compensating for Windows 11's
/// invisible DWM shadow/resize border margins.
pub fn set_window_rect_exact(hwnd: HWND, target: WmRect) {
    // SAFETY: Restoring/querying/repositioning an external window handle safely.
    unsafe {
        if hwnd.is_invalid() || !IsWindow(Some(hwnd)).as_bool() {
            return;
        }
        if IsZoomed(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let mut win_rect = RECT::default();
        if GetWindowRect(hwnd, &mut win_rect).is_err() {
            return;
        }
        let mut frame_rect = win_rect;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame_rect).cast(),
            size_of::<RECT>() as u32,
        );
        let pad_left = frame_rect.left - win_rect.left;
        let pad_top = frame_rect.top - win_rect.top;
        let pad_right = win_rect.right - frame_rect.right;
        let pad_bottom = win_rect.bottom - frame_rect.bottom;

        let x = target.left - pad_left;
        let y = target.top - pad_top;
        let w = (target.width() + pad_left + pad_right).max(1);
        let h = (target.height() + pad_top + pad_bottom).max(1);

        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }
}

/// Queries `(hmonitor_raw, rcMonitor, rcWork, dpi)` for the monitor hosting `monitor`.
fn query_monitor(monitor: HMONITOR) -> (isize, WmRect, WmRect, u32) {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: Querying monitor bounds, work area, and effective DPI for a valid HMONITOR.
    unsafe {
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        (
            monitor.0 as isize,
            rect_to_wm(info.rcMonitor),
            rect_to_wm(info.rcWork),
            dpi_x.max(96),
        )
    }
}

/// Returns `(hmonitor, rcMonitor, rcWork, dpi)` for the monitor containing `hwnd`.
pub fn monitor_work_rect_for_window(hwnd: HWND) -> (isize, WmRect, WmRect, u32) {
    // SAFETY: Resolving HMONITOR for a window handle.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    query_monitor(monitor)
}

/// Returns `(hmonitor, rcMonitor, rcWork, dpi)` for the monitor under the cursor
/// (or primary monitor fallback).
pub fn cursor_monitor_work_rect() -> (isize, WmRect, WmRect, u32) {
    // SAFETY: Querying cursor position and matching monitor.
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            query_monitor(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST))
        } else {
            query_monitor(MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY))
        }
    }
}

/// Returns `(hmonitor, rcMonitor, rcWork, dpi)` for the active foreground window
/// or cursor monitor if no normal foreground window is active.
pub fn active_monitor_work_rect() -> (isize, WmRect, WmRect, u32) {
    // SAFETY: Querying current foreground window monitor.
    let fg = unsafe { GetForegroundWindow() };
    if !fg.is_invalid() {
        monitor_work_rect_for_window(fg)
    } else {
        cursor_monitor_work_rect()
    }
}

/// Applies `zone` to `hwnd` inside its monitor work area using `gap_dip`.
pub fn apply_snap_zone(hwnd: HWND, zone: SnapZone, gap_dip: u32) {
    let (_, _, work, dpi) = monitor_work_rect_for_window(hwnd);
    if zone == SnapZone::Maximize && gap_dip == 0 {
        // SAFETY: Maximizing target window asynchronously.
        unsafe {
            let _ = ShowWindowAsync(hwnd, SW_MAXIMIZE);
        }
        return;
    }
    let gap_px = ((gap_dip as f32) * (dpi as f32 / 96.0)).round() as i32;
    set_window_rect_exact(hwnd, zone.rect(work, gap_px));
}

/// Returns `true` if `hwnd` is a normal, resizable user application window
/// eligible for window management / snapping / tiling.
pub fn is_manageable_window(hwnd: HWND) -> bool {
    // SAFETY: Inspecting style, visibility, cloaked state, and class of an enumerated HWND.
    unsafe {
        if hwnd.is_invalid()
            || !IsWindow(Some(hwnd)).as_bool()
            || !IsWindowVisible(hwnd).as_bool()
            || IsIconic(hwnd).as_bool()
            || hwnd == GetDesktopWindow()
            || hwnd == GetShellWindow()
        {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return false;
        }
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if style & WS_THICKFRAME.0 == 0 || ex_style & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), 4);
        if cloaked != 0 {
            return false;
        }
        if GetWindowTextLengthW(hwnd) <= 0 {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return false;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let class_name = String::from_utf16_lossy(&class[..len]);
        !matches!(
            class_name.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "LeanShellDesktop"
        )
    }
}

/// Lists all visible, resizable, non-minimized user windows currently on `hmonitor`,
/// ordered by z-order (front to back).
pub fn list_tileable_windows_on_monitor(hmonitor: isize) -> Vec<isize> {
    struct Collect {
        hmonitor: isize,
        hwnds: Vec<isize>,
    }
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam points to a valid mutable Collect struct during EnumWindows.
        let state = unsafe { &mut *(lparam.0 as *mut Collect) };
        if is_manageable_window(hwnd) {
            // SAFETY: Querying monitor for valid manageable HWND.
            let mon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
            if mon.0 as isize == state.hmonitor {
                state.hwnds.push(hwnd.0 as isize);
            }
        }
        true.into()
    }
    let mut state = Collect {
        hmonitor,
        hwnds: Vec::new(),
    };
    // SAFETY: Synchronously enumerating top-level windows with valid stack Collect pointer.
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&raw mut state as isize));
    }
    state.hwnds
}

static MOVESIZE_TARGET: AtomicIsize = AtomicIsize::new(0);
static MOVESIZE_MSG: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn movesize_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    // SAFETY: Out-of-context WinEvent callback on UI thread; never allocates or locks.
    if id_object != 0 || id_child != 0 || hwnd.is_invalid() {
        return;
    }
    let target = MOVESIZE_TARGET.load(Ordering::Relaxed);
    let msg = MOVESIZE_MSG.load(Ordering::Relaxed);
    if target != 0 && msg != 0 {
        // SAFETY: Posting WM_WM_MOVESIZE to our registered launcher HWND.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(target as _)),
                msg,
                WPARAM(event as usize),
                LPARAM(hwnd.0 as isize),
            );
        }
    }
}

/// RAII handle monitoring `EVENT_SYSTEM_MOVESIZESTART` and `EVENT_SYSTEM_MOVESIZEEND`
/// for mouse drag-to-snap and tile slot drag/swap/resize.
pub struct MoveSizeHook {
    hook: HWINEVENTHOOK,
}

impl MoveSizeHook {
    pub fn install(target_hwnd: HWND, movesize_msg: u32) -> Result<Self> {
        MOVESIZE_TARGET.store(target_hwnd.0 as isize, Ordering::Release);
        MOVESIZE_MSG.store(movesize_msg, Ordering::Release);
        // SAFETY: Installing out-of-context WinEventHook for move/size start & end.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_MOVESIZESTART,
                EVENT_SYSTEM_MOVESIZEEND,
                None,
                Some(movesize_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_invalid() {
            MOVESIZE_TARGET.store(0, Ordering::Release);
            MOVESIZE_MSG.store(0, Ordering::Release);
            return Err(Error::from_thread());
        }
        Ok(Self { hook })
    }
}

impl Drop for MoveSizeHook {
    fn drop(&mut self) {
        if !self.hook.is_invalid() {
            // SAFETY: Unregistering hook installed by SetWinEventHook.
            unsafe {
                let _ = UnhookWinEvent(self.hook);
            }
        }
        MOVESIZE_TARGET.store(0, Ordering::Release);
        MOVESIZE_MSG.store(0, Ordering::Release);
    }
}
