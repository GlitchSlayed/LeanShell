//! Native Windows Shell Context Menu integration (Track T3 & Track 3, ported from Open-Shell).
//!
//! Queries `IShellItem` and binds to `IContextMenu` to display true OS shell context menus
//! with Jump Lists, Open With, Run as Administrator, and shell extensions via `TrackPopupMenuEx`.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::UI::Shell::{
    BHID_SFUIObject, CMF_NORMAL, CMINVOKECOMMANDINFO, IContextMenu, IShellItem, SHCreateItemFromParsingName,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx,
};
use windows::core::{PCSTR, PCWSTR, Result};

/// Displays the native Windows Shell context menu for a file or application path at `(x, y)`.
pub fn show_shell_context_menu(hwnd: HWND, path: &Path, x: i32, y: i32) -> Result<bool> {
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

    // SAFETY: COM calls to create IShellItem and bind to IContextMenu handler
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide_path.as_ptr()), None)?;
        let context_menu: IContextMenu = item.BindToHandler(None::<&IBindCtx>, &BHID_SFUIObject)?;

        let menu = CreatePopupMenu()?;
        let res = context_menu.QueryContextMenu(menu, 0, 1, 0x7FFF, CMF_NORMAL);
        if res.is_err() {
            let _ = DestroyMenu(menu);
            return Ok(false);
        }

        let cmd = TrackPopupMenuEx(menu, TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0, x, y, hwnd, None);

        let invoked = if cmd.0 > 0 {
            let cmd_offset = (cmd.0 - 1) as usize;
            let info = CMINVOKECOMMANDINFO {
                cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd,
                lpVerb: PCSTR(cmd_offset as *const u8),
                nShow: 1, // SW_SHOWNORMAL
                ..Default::default()
            };
            context_menu.InvokeCommand(&info).is_ok()
        } else {
            false
        };

        let _ = DestroyMenu(menu);
        Ok(invoked)
    }
}
