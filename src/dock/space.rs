//! Keeps room for a dock that stays visible: a hidden app bar along the bottom
//! of the primary screen. Windows keeps maximized windows out of an app bar's
//! space, shares the screen edge with other app bars, follows display changes,
//! and gives the space back when the app bar is removed or Wisp exits.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABN_POSCHANGED, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, Result, w};

/// Windows sends app bar notifications with this message.
const WM_APPBAR: u32 = WM_APP + 40;

thread_local! {
    /// The space's height in DIPs, for the window procedure.
    static HEIGHT: Cell<f32> = const { Cell::new(0.0) };
    /// The strip last claimed, in physical pixels.
    static STRIP: Cell<RECT> = const { Cell::new(RECT { left: 0, top: 0, right: 0, bottom: 0 }) };
    /// Explorer broadcasts this after it restarts, which drops every app bar.
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// Create and drop on the thread that pumps Wisp's messages.
pub struct DockSpace {
    hwnd: HWND,
}

impl DockSpace {
    /// Keeps `height` DIPs free at the bottom of the primary screen.
    pub fn new(height: f32) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = w!("WispDockSpace");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(space_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            // Fails harmlessly when an earlier space registered it.
            RegisterClassW(&wc);
            TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
            // Never shown: an app bar's window needs no pixels to keep its space.
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            HEIGHT.set(height);
            if !register(hwnd) {
                let _ = DestroyWindow(hwnd);
                return Err(Error::from_thread());
            }
            Ok(Self { hwnd })
        }
    }

    /// Claims the space again if something gave it away, such as the taskbar
    /// hider reclaiming the bottom of the screen. Windows then works out the
    /// work area from every app bar again.
    pub fn check(&self) {
        let Some((_, work, _)) = primary() else { return };
        // Only a work area reaching into the strip; another app bar below it is fine.
        if work.bottom > STRIP.get().top {
            unregister(self.hwnd);
            register(self.hwnd);
        }
    }
}

impl Drop for DockSpace {
    fn drop(&mut self) {
        unregister(self.hwnd);
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn bar(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uEdge: ABE_BOTTOM,
        ..Default::default()
    }
}

fn register(hwnd: HWND) -> bool {
    let mut data = bar(hwnd);
    data.uCallbackMessage = WM_APPBAR;
    if unsafe { SHAppBarMessage(ABM_NEW, &mut data) } == 0 {
        return false;
    }
    place(hwnd);
    true
}

fn unregister(hwnd: HWND) {
    let mut data = bar(hwnd);
    unsafe { SHAppBarMessage(ABM_REMOVE, &mut data) };
}

/// The primary screen's bounds and work area, and its height for the space in
/// physical pixels.
fn primary() -> Option<(RECT, RECT, i32)> {
    unsafe {
        let monitor: HMONITOR = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let (mut x, mut y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        let height = (HEIGHT.get() * y.max(96) as f32 / 96.0).round() as i32;
        Some((info.rcMonitor, info.rcWork, height))
    }
}

/// Claims a strip at the bottom of the primary screen, above any app bar
/// already there, at the screen's current scale.
fn place(hwnd: HWND) {
    let Some((monitor, _, height)) = primary() else { return };
    let mut data = bar(hwnd);
    data.rc = monitor;
    data.rc.top = data.rc.bottom - height;
    unsafe {
        // Windows moves the strip up past other app bars on the same edge.
        SHAppBarMessage(ABM_QUERYPOS, &mut data);
        data.rc.top = data.rc.bottom - height;
        SHAppBarMessage(ABM_SETPOS, &mut data);
        // The window itself never moves or shows, so it needs no
        // ABM_WINDOWPOSCHANGED.
        STRIP.set(data.rc);
    }
}

/// Places the strip again only if the screen or its scale changed, so a
/// broadcast caused by the strip itself does not repeat forever.
fn place_if_changed(hwnd: HWND) {
    let Some((monitor, _, height)) = primary() else { return };
    let strip = STRIP.get();
    if strip.bottom - strip.top != height || strip.left != monitor.left || strip.right != monitor.right {
        place(hwnd);
    }
}

unsafe extern "system" fn space_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Another app bar or the taskbar changed: ask for the strip again.
        WM_APPBAR if wparam.0 as u32 == ABN_POSCHANGED => {
            place(hwnd);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => {
            place_if_changed(hwnd);
            LRESULT(0)
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.get() => {
            register(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
