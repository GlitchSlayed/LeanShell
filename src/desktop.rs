//! Minimal Win32 desktop background window for exclusive shell mode.
//!
//! Spans the virtual desktop at `HWND_BOTTOM`, paints the user's active
//! desktop wallpaper image (or falls back to `PaintDesktop`), and hosts a
//! right-click desktop menu.

use std::cell::RefCell;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, PCWSTR, Result, s, w};

const DESKTOP_CLASS: PCWSTR = w!("LeanShellDesktop");

const IDM_SETTINGS: usize = 1001;
const IDM_EXPLORER: usize = 1002;
const IDM_TERMINAL: usize = 1003;
const IDM_DISPLAY: usize = 1004;
const IDM_RESTORE: usize = 1005;
const IDM_TASK_VIEW: usize = 1006;
const IDM_TOGGLE_TILING: usize = 1007;

struct CachedWallpaper {
    path: String,
    cx: i32,
    cy: i32,
    bitmap: HBITMAP,
}

thread_local! {
    static CACHED_WALLPAPER: RefCell<Option<CachedWallpaper>> = const { RefCell::new(None) };
}

fn reload_wallpaper(cx: i32, cy: i32, force: bool) {
    let cx = cx.max(1);
    let cy = cy.max(1);
    let Some(path) = get_wallpaper_path() else {
        clear_wallpaper();
        return;
    };
    CACHED_WALLPAPER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if !force
            && let Some(cached) = slot.as_ref()
            && cached.path == path
            && cached.cx == cx
            && cached.cy == cy
        {
            return;
        }
        if let Some(old) = slot.take() {
            // SAFETY: Deleting old wallpaper bitmap
            unsafe {
                let _ = DeleteObject(old.bitmap.into());
            }
        }
        if let Some(bitmap) = load_wallpaper_bitmap(&path, cx, cy) {
            *slot = Some(CachedWallpaper { path, cx, cy, bitmap });
        }
    });
}

fn clear_wallpaper() {
    CACHED_WALLPAPER.with(|cell| {
        if let Some(old) = cell.borrow_mut().take() {
            // SAFETY: Deleting wallpaper bitmap
            unsafe {
                let _ = DeleteObject(old.bitmap.into());
            }
        }
    });
}

fn load_wallpaper_bitmap(path: &str, cx: i32, cy: i32) -> Option<HBITMAP> {
    // SAFETY: Creating IShellItemImageFactory to decode image to HBITMAP with balanced COM init.
    unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let bitmap = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(&HSTRING::from(path), None)
            .ok()
            .and_then(|factory| {
                factory
                    .GetImage(
                        SIZE {
                            cx: cx.max(1),
                            cy: cy.max(1),
                        },
                        SIIGBF_SCALEUP | SIIGBF_RESIZETOFIT,
                    )
                    .ok()
            });
        if com.is_ok() {
            CoUninitialize();
        }
        bitmap
    }
}

fn get_wallpaper_path() -> Option<String> {
    // 1. Try SPI_GETDESKWALLPAPER
    let mut buf = [0u16; 260];
    // SAFETY: Querying desktop wallpaper path from system
    let res = unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buf.len() as u32,
            Some(buf.as_mut_ptr() as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if res.is_ok() {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        if !s.is_empty() && std::path::Path::new(&s).exists() {
            return Some(s);
        }
    }

    // 2. Try %APPDATA%\Microsoft\Windows\Themes\TranscodedWallpaper
    if let Ok(appdata) = std::env::var("APPDATA") {
        let transcoded = format!(r"{appdata}\Microsoft\Windows\Themes\TranscodedWallpaper");
        if std::path::Path::new(&transcoded).exists() {
            return Some(transcoded);
        }
    }

    None
}

pub struct DesktopWindow {
    hwnd: HWND,
}

impl DesktopWindow {
    pub fn new() -> Result<Self> {
        let instance = unsafe { GetModuleHandleW(None)? };

        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(desktop_wnd_proc),
            hInstance: instance.into(),
            lpszClassName: DESKTOP_CLASS,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
            ..Default::default()
        };

        // SAFETY: Registering desktop window class
        unsafe {
            RegisterClassExW(&class);
        }

        let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let cx = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
        let cy = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };

        // SAFETY: Creating fullscreen bottom-level desktop window
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                DESKTOP_CLASS,
                w!("Desktop"),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                cx,
                cy,
                None,
                None,
                Some(instance.into()),
                None,
            )?
        };

        // Send to bottom
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), x, y, cx, cy, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }

        reload_wallpaper(cx, cy, true);

        let desktop = Self { hwnd };
        desktop.claim_shell_window();
        Ok(desktop)
    }

    /// Registers this desktop window via `SetShellWindow` when `GetShellWindow()` is
    /// vacant so on-demand `explorer.exe` opens folder windows without creating `Shell_TrayWnd`.
    pub fn claim_shell_window(&self) -> bool {
        // SAFETY: Checks GetShellWindow() and dynamically resolves SetShellWindow(HWND)
        // from user32.dll when no shell window is currently registered.
        unsafe {
            let current = GetShellWindow();
            if !current.is_invalid() {
                let mut pid = 0u32;
                GetWindowThreadProcessId(current, Some(&mut pid));
                return pid == std::process::id();
            }
            let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else {
                return false;
            };
            let Some(proc) = GetProcAddress(user32, s!("SetShellWindow")) else {
                return false;
            };
            let set_shell_window: unsafe extern "system" fn(HWND) -> BOOL = std::mem::transmute(proc);
            set_shell_window(self.hwnd).as_bool()
        }
    }

    #[allow(dead_code)]
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for DesktopWindow {
    fn drop(&mut self) {
        clear_wallpaper();
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

unsafe extern "system" fn desktop_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: Desktop window procedure handling paint, context menu, and screen resizing
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                if !hdc.0.is_null() {
                    let mut painted = false;
                    CACHED_WALLPAPER.with(|cell| {
                        if let Some(cached) = cell.borrow().as_ref() {
                            let bitmap = cached.bitmap;
                            let mem_dc = CreateCompatibleDC(Some(hdc));
                            if !mem_dc.0.is_null() {
                                let old = SelectObject(mem_dc, bitmap.into());
                                let mut bm = BITMAP::default();
                                GetObjectW(
                                    bitmap.into(),
                                    size_of::<BITMAP>() as i32,
                                    Some((&mut bm as *mut BITMAP).cast()),
                                );
                                SetStretchBltMode(hdc, HALFTONE);
                                let _ = SetBrushOrgEx(hdc, 0, 0, None);
                                let mut client = RECT::default();
                                let _ = GetClientRect(hwnd, &mut client);
                                let _ = StretchBlt(
                                    hdc,
                                    0,
                                    0,
                                    client.right - client.left,
                                    client.bottom - client.top,
                                    Some(mem_dc),
                                    0,
                                    0,
                                    bm.bmWidth,
                                    bm.bmHeight,
                                    SRCCOPY,
                                );
                                SelectObject(mem_dc, old);
                                let _ = DeleteDC(mem_dc);
                                painted = true;
                            }
                        }
                    });
                    if !painted {
                        let _ = PaintDesktop(hdc);
                    }
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_DISPLAYCHANGE => {
                let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let cx = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let cy = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), x, y, cx, cy, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                reload_wallpaper(cx, cy, false);
                let _ = InvalidateRect(Some(hwnd), None, true);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                if wparam.0 == SPI_SETDESKWALLPAPER.0 as usize || wparam.0 == 0 {
                    let cx = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                    let cy = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                    reload_wallpaper(cx, cy, wparam.0 == SPI_SETDESKWALLPAPER.0 as usize);
                    let _ = InvalidateRect(Some(hwnd), None, true);
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let x = (lparam.0 & 0xffff) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                let mut pt = POINT { x, y };
                let _ = ClientToScreen(hwnd, &mut pt);

                if let Ok(menu) = CreatePopupMenu() {
                    let _ = AppendMenuW(menu, MF_STRING, IDM_TASK_VIEW, w!("Task View (Win+Tab)"));
                    let _ = AppendMenuW(menu, MF_STRING, IDM_TOGGLE_TILING, w!("Toggle Window Tiling (Win+T)"));
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                    let _ = AppendMenuW(menu, MF_STRING, IDM_SETTINGS, w!("LeanShell Settings"));
                    let _ = AppendMenuW(menu, MF_STRING, IDM_EXPLORER, w!("File Explorer"));
                    let _ = AppendMenuW(menu, MF_STRING, IDM_TERMINAL, w!("Terminal"));
                    let _ = AppendMenuW(menu, MF_STRING, IDM_DISPLAY, w!("Display Settings"));
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                    let _ = AppendMenuW(menu, MF_STRING, IDM_RESTORE, w!("Restore Windows Taskbar & Explorer"));

                    let _ = SetForegroundWindow(hwnd);
                    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
                    let _ = DestroyMenu(menu);

                    match cmd.0 as usize {
                        IDM_TASK_VIEW | IDM_TOGGLE_TILING => {
                            let action = if cmd.0 as usize == IDM_TASK_VIEW { 1 } else { 2 };
                            if let Ok(launcher) = FindWindowW(crate::channel::LAUNCHER_CLASS, None)
                                && !launcher.is_invalid()
                            {
                                let _ = PostMessageW(
                                    Some(launcher),
                                    crate::messages::WM_WM_ACTION,
                                    WPARAM(action),
                                    LPARAM(0),
                                );
                            }
                        }
                        IDM_SETTINGS => {
                            let _ = crate::launch::settings(false);
                        }
                        IDM_EXPLORER => {
                            let _ = crate::launch::file_explorer();
                        }
                        IDM_TERMINAL => {
                            if crate::launch::open("wt.exe").is_err() {
                                let _ = crate::launch::open("cmd.exe");
                            }
                        }
                        IDM_DISPLAY => {
                            let _ = crate::launch::open("ms-settings:display");
                        }
                        IDM_RESTORE => {
                            let _ = crate::taskbar::restore_shell_cli();
                            std::process::exit(0);
                        }
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_wallpaper_bitmap() {
        let bmp = get_wallpaper_path().and_then(|path| load_wallpaper_bitmap(&path, 800, 600));
        println!("load_wallpaper_bitmap: {:?}", bmp.is_some());
        if let Some(bmp) = bmp {
            unsafe {
                let _ = DeleteObject(bmp.into());
            }
        }
    }
}
