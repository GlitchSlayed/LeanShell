//! Keeps room for a dock that stays visible: a hidden app bar along the bottom
//! of the primary screen. Windows keeps maximized windows out of an app bar's
//! space, shares the screen edge with other app bars, follows display changes,
//! and gives the space back when the app bar is removed or Wisp exits.

use std::cell::Cell;
use std::sync::mpsc;
use std::thread::JoinHandle;

use windows::Win32::Foundation::{E_FAIL, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
    ABN_POSCHANGED, APPBARDATA, SHAllocShared, SHAppBarMessage, SHFreeShared, SHLockShared, SHUnlockShared,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, HRESULT, Result, w};

/// Windows sends app bar notifications with this message.
const WM_APPBAR: u32 = WM_APP + 40;
/// Asks the space's thread to check the space is still kept.
const WM_CHECK: u32 = WM_APP + 41;
/// Updates the reserved height as the dock's icon size changes.
const WM_SET_HEIGHT: u32 = WM_APP + 42;
/// Windows sends no message when it works the work area out again without
/// the space, so the space's thread also checks on a timer.
const CHECK_TIMER: usize = 1;
const CHECK_MS: u32 = 1000;

pub fn edge_to_win32(edge: leanshell_core::config::PanelEdge) -> u32 {
    match edge {
        leanshell_core::config::PanelEdge::Bottom => ABE_BOTTOM,
        leanshell_core::config::PanelEdge::Top => ABE_TOP,
        leanshell_core::config::PanelEdge::Left => ABE_LEFT,
        leanshell_core::config::PanelEdge::Right => ABE_RIGHT,
    }
}

thread_local! {
    /// The space's thickness in DIPs, for the window procedure.
    static THICKNESS: Cell<f32> = const { Cell::new(0.0) };
    /// The screen edge to dock against.
    static EDGE: Cell<u32> = const { Cell::new(ABE_BOTTOM) };
    /// The strip last claimed, in physical pixels.
    static STRIP: Cell<RECT> = const { Cell::new(RECT { left: 0, top: 0, right: 0, bottom: 0 }) };
    /// The primary screen's bounds and the strip's height in pixels when it
    /// was last claimed, to tell a real display change from a broadcast.
    static PLACED_FOR: Cell<(RECT, i32)> = const { Cell::new((RECT { left: 0, top: 0, right: 0, bottom: 0 }, 0)) };
    /// Explorer broadcasts this after it restarts, which drops every app bar.
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    /// Checks left before the app bar may be registered afresh again.
    static BACKOFF: Cell<u32> = const { Cell::new(0) };
}

/// Checks between fresh registrations while Windows will not keep the space.
const REREGISTER_EVERY: u32 = 10;

/// The space lives on its own thread: Explorer answers an app bar message
/// that changes the work area only after every window has seen the change,
/// which takes about half a second, too long for the UI thread to wait.
pub struct DockSpace {
    hwnd: HWND,
    edge: u32,
    thread: Option<JoinHandle<()>>,
}

impl DockSpace {
    /// Keeps `thickness` DIPs free at the designated edge of the primary screen.
    pub fn new(thickness: f32, edge: u32) -> Result<Self> {
        let (ready, started) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("dock space".into())
            .spawn(move || run(thickness, edge, ready))
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        match started.recv() {
            Ok(Ok(hwnd)) => Ok(Self {
                hwnd: HWND(hwnd as _),
                edge,
                thread: Some(thread),
            }),
            Ok(Err(code)) => {
                let _ = thread.join();
                Err(Error::from_hresult(code))
            }
            Err(_) => {
                let _ = thread.join();
                Err(Error::new(E_FAIL, "The dock space thread exited"))
            }
        }
    }

    pub fn edge(&self) -> u32 {
        self.edge
    }

    /// Claims the space again if something gave it away, such as the taskbar
    /// hider reclaiming the bottom of the screen.
    pub fn check(&self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_CHECK, WPARAM(0), LPARAM(0));
        }
    }

    pub fn set_thickness(&self, thickness: f32) {
        unsafe {
            let _ = PostMessageW(
                Some(self.hwnd),
                WM_SET_HEIGHT,
                WPARAM(thickness.to_bits() as usize),
                LPARAM(0),
            );
        }
    }

    /// Gives the space back and waits until Windows has, as before exiting.
    pub fn close(mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    fn stop(&self) {
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

impl Drop for DockSpace {
    /// The thread gives the space back on its own.
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.stop();
        }
    }
}

fn run(thickness: f32, edge: u32, ready: mpsc::Sender<std::result::Result<usize, HRESULT>>) {
    THICKNESS.set(thickness);
    EDGE.set(edge);
    let hwnd = match create() {
        Ok(hwnd) => hwnd,
        Err(e) => {
            let _ = ready.send(Err(e.code()));
            return;
        }
    };
    // Before registering: Explorer may still be busy removing an earlier
    // space, and the UI thread must not wait for that.
    if ready.send(Ok(hwnd.0 as usize)).is_err() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return;
    }
    // A failure is retried by the check timer.
    if !register(hwnd) {
        eprintln!("could not keep space for the dock: {}", Error::from_thread());
    }
    unsafe {
        SetTimer(Some(hwnd), CHECK_TIMER, CHECK_MS, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn create() -> Result<HWND> {
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
        CreateWindowExW(
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
        )
    }
}

/// Cheap when nothing changed, so it can run on a timer.
fn check(hwnd: HWND) {
    if kept() {
        return;
    }
    // Asking for the same strip again is usually enough; a fresh app bar if
    // Windows forgot this one.
    place(hwnd);
    let backoff = BACKOFF.get();
    if !kept() && backoff == 0 {
        unregister(hwnd);
        register(hwnd);
        BACKOFF.set(REREGISTER_EVERY);
    } else {
        BACKOFF.set(backoff.saturating_sub(1));
    }
}

/// Whether the primary work area stops above the strip. Another app bar
/// below the strip is fine.
fn kept() -> bool {
    let strip = STRIP.get();
    primary().is_none_or(|(_, work, _)| match EDGE.get() {
        ABE_TOP => work.top >= strip.bottom,
        ABE_LEFT => work.left >= strip.right,
        ABE_RIGHT => work.right <= strip.left,
        _ => work.bottom <= strip.top,
    })
}

fn bar(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uEdge: EDGE.get(),
        ..Default::default()
    }
}

fn register(hwnd: HWND) -> bool {
    let mut data = bar(hwnd);
    data.uCallbackMessage = WM_APPBAR;
    if send(ABM_NEW, &mut data) == 0 {
        return false;
    }
    place(hwnd);
    true
}

fn unregister(hwnd: HWND) {
    let mut data = bar(hwnd);
    send(ABM_REMOVE, &mut data);
}

/// `APPBARDATA` as `SHAppBarMessage` sends it to the taskbar: handles and
/// `lParam` in fixed sizes, so 32- and 64-bit callers share one format.
#[repr(C)]
#[derive(Clone, Copy)]
struct WireData {
    cb_size: u32,
    hwnd: u32,
    callback: u32,
    edge: u32,
    rc: RECT,
    lparam: i64,
}

#[repr(C)]
struct WireMessage {
    data: WireData,
    message: u32,
    /// Holds a `WireData` for messages that return a rectangle.
    shared: u64,
    shared_process: u32,
}

/// `SHAppBarMessage`, sent straight to Explorer's taskbar with the shared
/// block in Explorer's process. `SHAppBarMessage` finds the dock's tray
/// proxy instead and puts the block in Wisp's process; Explorer then answers
/// `ABM_SETPOS` without working the work area out again, so maximized
/// windows would cover the dock until something else made it.
fn send(message: u32, data: &mut APPBARDATA) -> usize {
    let Some((tray, explorer)) = crate::tray_host::explorer_tray() else {
        return unsafe { SHAppBarMessage(message, data) };
    };
    let wire = WireData {
        cb_size: size_of::<WireData>() as u32,
        hwnd: data.hWnd.0 as usize as u32,
        callback: data.uCallbackMessage,
        edge: data.uEdge,
        rc: data.rc,
        lparam: data.lParam.0 as i64,
    };
    let returns_rect = matches!(message, ABM_QUERYPOS | ABM_SETPOS | ABM_GETTASKBARPOS);
    let shared = if returns_rect {
        unsafe { SHAllocShared(Some(&wire as *const _ as _), size_of::<WireData>() as u32, explorer) }
    } else {
        HANDLE::default()
    };
    let packet = WireMessage {
        data: wire,
        message,
        shared: shared.0 as usize as u64,
        shared_process: explorer,
    };
    let copy = COPYDATASTRUCT {
        dwData: 0,
        cbData: size_of::<WireMessage>() as u32,
        lpData: &packet as *const _ as *mut _,
    };
    let mut result = 0usize;
    unsafe {
        let _ = SendMessageTimeoutW(
            tray,
            WM_COPYDATA,
            WPARAM(data.hWnd.0 as usize),
            LPARAM(&copy as *const _ as isize),
            SMTO_ABORTIFHUNG,
            5000,
            Some(&mut result),
        );
        if !shared.is_invalid() && !shared.0.is_null() {
            let view = SHLockShared(shared, explorer) as *const WireData;
            if !view.is_null() {
                data.rc = (*view).rc;
                let _ = SHUnlockShared(view as _);
            }
            let _ = SHFreeShared(shared, explorer);
        }
    }
    result
}

/// The primary screen's bounds and work area, and its thickness for the space in
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
        let dpi = match EDGE.get() {
            ABE_LEFT | ABE_RIGHT => x.max(96),
            _ => y.max(96),
        };
        let thickness = (THICKNESS.get() * dpi as f32 / 96.0).round() as i32;
        Some((info.rcMonitor, info.rcWork, thickness))
    }
}

/// Claims a strip at the designated edge of the primary screen, above any app bar
/// already there, at the screen's current scale.
fn place(hwnd: HWND) {
    let Some((monitor, _, thickness)) = primary() else {
        return;
    };
    let mut data = bar(hwnd);
    data.rc = monitor;
    match EDGE.get() {
        ABE_TOP => data.rc.bottom = data.rc.top + thickness,
        ABE_LEFT => data.rc.right = data.rc.left + thickness,
        ABE_RIGHT => data.rc.left = data.rc.right - thickness,
        _ => data.rc.top = data.rc.bottom - thickness,
    }
    // Windows moves the strip past other app bars on the same edge.
    send(ABM_QUERYPOS, &mut data);
    match EDGE.get() {
        ABE_TOP => data.rc.bottom = data.rc.top + thickness,
        ABE_LEFT => data.rc.right = data.rc.left + thickness,
        ABE_RIGHT => data.rc.left = data.rc.right - thickness,
        _ => data.rc.top = data.rc.bottom - thickness,
    }
    send(ABM_SETPOS, &mut data);
    // The window itself never moves or shows, so it needs no
    // ABM_WINDOWPOSCHANGED.
    STRIP.set(data.rc);
    PLACED_FOR.set((monitor, thickness));
}

/// Places the strip again only if the screen or strip thickness changed, so a
/// broadcast caused by the strip itself does not repeat forever.
fn place_if_changed(hwnd: HWND) {
    let Some((monitor, _, thickness)) = primary() else {
        return;
    };
    if PLACED_FOR.get() != (monitor, thickness) {
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
        WM_CHECK => {
            check(hwnd);
            LRESULT(0)
        }
        WM_SET_HEIGHT => {
            THICKNESS.set(f32::from_bits(wparam.0 as u32));
            place_if_changed(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CHECK_TIMER => {
            check(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            unregister(hwnd);
            unsafe {
                let _ = DestroyWindow(hwnd);
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => {
            // Explorer broadcasts these and waits; placing asks Explorer.
            unsafe {
                let _ = ReplyMessage(LRESULT(0));
            }
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
