//! Minimal Win32 desktop background window for exclusive shell mode.
//!
//! Spans the virtual desktop at `HWND_BOTTOM`, paints the user's active
//! desktop wallpaper using `PaintDesktop`, and hosts a right-click desktop menu.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, Result, w};

const DESKTOP_CLASS: PCWSTR = w!("LeanShellDesktop");

const IDM_SETTINGS: usize = 1001;
const IDM_EXPLORER: usize = 1002;
const IDM_TERMINAL: usize = 1003;
const IDM_DISPLAY: usize = 1004;
const IDM_RESTORE: usize = 1005;

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

        Ok(Self { hwnd })
    }

    #[allow(dead_code)]
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for DesktopWindow {
    fn drop(&mut self) {
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
                    let _ = PaintDesktop(hdc);
                    let _ = EndPaint(hwnd, &ps);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => {
                let hdc = HDC(wparam.0 as *mut _);
                if !hdc.0.is_null() {
                    let _ = PaintDesktop(hdc);
                }
                LRESULT(1)
            }
            WM_DISPLAYCHANGE => {
                let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let cx = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let cy = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), x, y, cx, cy, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                let _ = InvalidateRect(Some(hwnd), None, true);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                let _ = InvalidateRect(Some(hwnd), None, true);
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let x = (lparam.0 & 0xffff) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                let mut pt = POINT { x, y };
                let _ = ClientToScreen(hwnd, &mut pt);

                if let Ok(menu) = CreatePopupMenu() {
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
                        IDM_SETTINGS => {
                            let _ = crate::launch::settings(false);
                        }
                        IDM_EXPLORER => {
                            let _ = crate::launch::open("explorer.exe");
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
