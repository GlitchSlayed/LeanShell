//! Keeps room for a dock that stays visible: a hidden app bar along the bottom
//! of the primary screen. Windows keeps maximized windows out of an app bar's
//! space, shares the screen edge with other app bars, follows display changes,
//! and gives the space back when the app bar is removed or Wisp exits.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABN_POSCHANGED, APPBARDATA, SHAppBarMessage,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, Result, w};

/// Windows sends app bar notifications with this message.
const WM_APPBAR: u32 = WM_APP + 40;

thread_local! {
    /// The space's height in physical pixels, for the window procedure.
    static HEIGHT: Cell<i32> = const { Cell::new(0) };
}

/// Create and drop on the thread that pumps Wisp's messages.
pub struct DockSpace {
    hwnd: HWND,
}

impl DockSpace {
    pub fn new(height: i32) -> Result<Self> {
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
            let mut data = bar(hwnd);
            data.uCallbackMessage = WM_APPBAR;
            if SHAppBarMessage(ABM_NEW, &mut data) == 0 {
                let _ = DestroyWindow(hwnd);
                return Err(Error::from_thread());
            }
            HEIGHT.set(height);
            place(hwnd);
            Ok(Self { hwnd })
        }
    }

    /// Changes the height, such as after the primary screen's scale changed.
    pub fn set_height(&mut self, height: i32) {
        if HEIGHT.get() != height {
            HEIGHT.set(height);
            place(self.hwnd);
        }
    }
}

impl Drop for DockSpace {
    fn drop(&mut self) {
        unsafe {
            let mut data = bar(self.hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut data);
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

/// Claims a strip of the current height at the bottom of the primary screen,
/// above any app bar already there.
fn place(hwnd: HWND) {
    unsafe {
        let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let height = HEIGHT.get();
        let mut data = bar(hwnd);
        data.rc = info.rcMonitor;
        data.rc.top = data.rc.bottom - height;
        // Windows moves the strip up past other app bars on the same edge.
        SHAppBarMessage(ABM_QUERYPOS, &mut data);
        data.rc.top = data.rc.bottom - height;
        SHAppBarMessage(ABM_SETPOS, &mut data);
        let r = data.rc;
        let _ = MoveWindow(hwnd, r.left, r.top, r.right - r.left, r.bottom - r.top, false);
    }
}

unsafe extern "system" fn space_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Another app bar, the taskbar or the display changed.
        WM_DISPLAYCHANGE => {
            place(hwnd);
            LRESULT(0)
        }
        WM_APPBAR if wparam.0 as u32 == ABN_POSCHANGED => {
            place(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
