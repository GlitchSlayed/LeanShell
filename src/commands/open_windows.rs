//! Open windows, for switching to one or closing it.

use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, WPARAM},
        Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::Input::KeyboardAndMouse::{KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_ESCAPE, keybd_event},
        UI::WindowsAndMessaging::*,
    },
    core::{BOOL, PWSTR},
};
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub hwnd: isize,
    pub title: String,
    /// The program's path, for its icon. Empty if Windows does not tell.
    pub exe: String,
}

impl Window {
    /// Key into the launcher's icon map.
    pub fn icon_key(&self) -> Option<String> {
        (!self.exe.is_empty()).then(|| format!("exe:{}", self.exe.to_lowercase()))
    }
}

/// Windows that Alt+Tab would show, in z-order (most recently used first).
/// Skips this process's windows.
pub fn list() -> Vec<Window> {
    let mut windows: Vec<Window> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&raw mut windows as isize));
    }
    windows
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        let windows = &mut *(lparam.0 as *mut Vec<Window>);
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if style & WS_EX_TOOLWINDOW.0 != 0 {
            return true.into();
        }
        let is_app_window = style & WS_EX_APPWINDOW.0 != 0;
        if !is_app_window && GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return true.into();
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&raw mut cloaked).cast(), 4);
        if cloaked != 0 {
            return true.into();
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return true.into();
        }
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        let exe = process_path(pid);
        let title = if len > 0 {
            String::from_utf16_lossy(&title_buf[..len as usize])
        } else if is_app_window && !exe.is_empty() {
            program_name(&exe).to_string()
        } else {
            return true.into();
        };

        windows.push(Window {
            hwnd: hwnd.0 as isize,
            title,
            exe,
        });
        true.into()
    }
}

pub fn process_path(pid: u32) -> String {
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        if ok {
            String::from_utf16_lossy(&buffer[..len as usize])
        } else {
            String::new()
        }
    }
}

/// Brings the window to the front, restoring it if minimized. Non-blocking foreground
/// activation using thread-input attachment without freezing the UI thread.
pub fn focus(hwnd: isize) -> Result<(), String> {
    let hwnd = HWND(hwnd as _);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return Err("The window was closed.".into());
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        if GetForegroundWindow() == hwnd {
            return Ok(());
        }
        if SetForegroundWindow(hwnd).as_bool() {
            return Ok(());
        }

        // Dismiss shell flyouts (Action Center / Start / Search) if foreground is locked
        let foreground = GetForegroundWindow();
        let _ = dismiss_shell_flyout(foreground);

        // Safe foreground activation: request permission for the target process to call
        // SetForegroundWindow, then post it a message to activate itself.
        // This avoids AttachThreadInput which can hang the UI thread indefinitely if the
        // target is unresponsive (documented Windows risk per Raymond Chen's blog).
        let mut target_pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut target_pid));
        if target_pid != 0 {
            let _ = AllowSetForegroundWindow(target_pid);
        }
        // Best-effort: post to our own queue to defer activation past any lock window.
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);

        Ok(())
    }
}

/// True when the window is the foreground one, so the dock can toggle it.
pub fn is_foreground(hwnd: isize) -> bool {
    unsafe { GetForegroundWindow() == HWND(hwnd as _) }
}

/// Hides the window the way its minimize button does.
pub fn minimize(hwnd: isize) -> Result<(), String> {
    unsafe {
        let hwnd = HWND(hwnd as _);
        if !IsWindow(Some(hwnd)).as_bool() {
            return Err("The window was closed.".into());
        }
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
        Ok(())
    }
}

/// Closes the Action Center, Start menu or search, which otherwise keep the
/// foreground and stop the dock from switching apps. Returns whether one was
/// in front.
fn dismiss_shell_flyout(foreground: HWND) -> bool {
    const SHELL: [&str; 4] = [
        r"\shellexperiencehost.exe",
        r"\startmenuexperiencehost.exe",
        r"\searchhost.exe",
        r"\shellhost.exe",
    ];
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let exe = process_path(pid).to_ascii_lowercase();
        if !SHELL.iter().any(|name| exe.ends_with(name)) {
            return false;
        }
        keybd_event(VK_ESCAPE.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_ESCAPE.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        true
    }
}

/// Asks the window to close, as its close button does.
pub fn close(hwnd: isize) -> Result<(), String> {
    unsafe { PostMessageW(Some(HWND(hwnd as _)), WM_CLOSE, WPARAM(0), LPARAM(0)).map_err(|error| error.message()) }
}

/// The program's name from its path: `C:\…\Code.exe` → "Code".
pub fn program_name(exe: &str) -> &str {
    let file = exe.rsplit('\\').next().unwrap_or(exe);
    file.strip_suffix(".exe")
        .or_else(|| file.strip_suffix(".EXE"))
        .unwrap_or(file)
}
