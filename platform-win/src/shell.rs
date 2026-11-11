//! Winlogon exclusive shell registration, safe-mode fallback, and startup integration.
//!
//! Controls per-user `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell`
//! override. Enforces strict safety rules: NEVER writes to machine-wide `HKLM`.

use std::path::Path;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
use windows::core::{Result, w};

const WINLOGON_SUBKEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon");
const SHELL_VALUE: windows::core::PCWSTR = w!("Shell");

/// Returns true if a custom shell override is registered in HKCU Winlogon.
pub fn is_exclusive_shell_registered() -> bool {
    // SAFETY: Querying HKCU registry key
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            WINLOGON_SUBKEY,
            SHELL_VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
        .is_ok()
    }
}

/// Registers the given executable as the user's primary shell in HKCU Winlogon.
///
/// SAFETY: Only writes to HKEY_CURRENT_USER, never machine-wide HKLM.
pub fn register_exclusive_shell(exe_path: &Path) -> Result<()> {
    let command: Vec<u16> = format!("\"{}\"", exe_path.display())
        .encode_utf16()
        .chain(Some(0))
        .collect();

    // SAFETY: Writing per-user HKCU registry override
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            WINLOGON_SUBKEY,
            SHELL_VALUE,
            REG_SZ.0,
            Some(command.as_ptr().cast()),
            (command.len() * 2) as u32,
        )
        .ok()
    }
}

/// Removes the user's Winlogon Shell override in HKCU, restoring default Explorer.
pub fn unregister_exclusive_shell() -> Result<()> {
    // SAFETY: Deleting HKCU value
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            WINLOGON_SUBKEY,
            Some(0),
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            &mut key,
        )
        .is_ok()
        {
            let _ = RegDeleteValueW(key, SHELL_VALUE);
            let _ = RegCloseKey(key);
        }
        Ok(())
    }
}

/// Checks if emergency safe-mode modifier (Shift held) is active during logon/startup.
pub fn is_safe_mode_requested() -> bool {
    // SAFETY: Polling physical key state of VK_SHIFT
    unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 }
}
