//! Winlogon exclusive shell registration, safe-mode fallback, and session ownership detection.
//!
//! Controls per-user `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell`
//! override with transactional backups. Enforces strict safety rules: NEVER writes to machine-wide `HKLM`.

use std::path::Path;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};
use windows::core::{Error, PCWSTR, Result, w};

const WINLOGON_SUBKEY: PCWSTR = w!(r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon");
const LEANSHELL_BACKUP_SUBKEY: PCWSTR = w!(r"Software\LeanShell\BackupShell");
const SHELL_VALUE: PCWSTR = w!("Shell");
const ORIGINAL_SHELL_VALUE: PCWSTR = w!("OriginalShell");

/// Returns the currently registered shell in HKCU Winlogon, if present.
pub fn get_registered_shell() -> Option<String> {
    // SAFETY: Querying HKCU registry key
    unsafe {
        let mut buffer = [0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        let res = RegGetValueW(
            HKEY_CURRENT_USER,
            WINLOGON_SUBKEY,
            SHELL_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        );
        if res.is_ok() {
            let units = (size as usize / 2).saturating_sub(1);
            Some(String::from_utf16_lossy(&buffer[..units]))
        } else {
            None
        }
    }
}

/// Returns the backed-up prior shell value from HKCU\Software\LeanShell\BackupShell, if present.
pub fn get_backup_shell() -> Option<String> {
    // SAFETY: Querying HKCU backup key
    unsafe {
        let mut buffer = [0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        let res = RegGetValueW(
            HKEY_CURRENT_USER,
            LEANSHELL_BACKUP_SUBKEY,
            ORIGINAL_SHELL_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        );
        if res.is_ok() {
            let units = (size as usize / 2).saturating_sub(1);
            Some(String::from_utf16_lossy(&buffer[..units]))
        } else {
            None
        }
    }
}

/// Returns true if a custom shell override is registered in HKCU Winlogon.
pub fn is_exclusive_shell_registered() -> bool {
    get_registered_shell().is_some()
}

/// Detects whether LeanShell is ACTUALLY owning the current desktop session as the shell,
/// rather than running as a companion alongside Windows Explorer.
///
/// Verifies `GetShellWindow()` ownership: if the shell window belongs to another process
/// (such as explorer.exe), LeanShell is in companion mode regardless of configured preferences.
pub fn is_session_exclusive() -> bool {
    // SAFETY: Querying Windows desktop shell window
    unsafe {
        let shell_hwnd = GetShellWindow();
        if shell_hwnd.is_invalid() {
            // No desktop shell window exists -> we are in an exclusive session!
            return true;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(shell_hwnd, Some(&mut pid));
        // If the shell window belongs to our process, we are the shell!
        pid == 0 || pid == std::process::id()
    }
}

/// Registers the given executable as the user's primary shell in HKCU Winlogon.
/// Backs up any prior configured value to `HKCU\Software\LeanShell\BackupShell`
/// before overwriting, and validates that the target executable exists on disk.
///
/// SAFETY: Only writes to HKEY_CURRENT_USER, never machine-wide HKLM.
pub fn register_exclusive_shell(exe_path: &Path) -> Result<()> {
    if !exe_path.exists() {
        return Err(Error::new(
            ERROR_FILE_NOT_FOUND.to_hresult(),
            "Target executable path does not exist on disk",
        ));
    }

    // 1. Transactional Backup: preserve existing shell value ONLY if no backup exists yet.
    //    If we unconditionally overwrite the backup on every registration, a second call to
    //    register_exclusive_shell() while LeanShell is already registered would store
    //    "LeanShell" as the OriginalShell, destroying the real prior shell reference.
    if get_backup_shell().is_none()
        && let Some(existing) = get_registered_shell()
    {
        let wide_existing: Vec<u16> = existing.encode_utf16().chain(Some(0)).collect();
        // SAFETY: Storing backup key in HKCU
        unsafe {
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                LEANSHELL_BACKUP_SUBKEY,
                ORIGINAL_SHELL_VALUE,
                REG_SZ.0,
                Some(wide_existing.as_ptr().cast()),
                (wide_existing.len() * 2) as u32,
            );
        }
    }

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

/// Removes the user's Winlogon Shell override in HKCU, restoring previous backup or Explorer.
pub fn unregister_exclusive_shell() -> Result<()> {
    // Check if we have a backed up original shell
    let backup: Option<Vec<u16>> = unsafe {
        let mut buffer = [0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            LEANSHELL_BACKUP_SUBKEY,
            ORIGINAL_SHELL_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .is_ok()
        {
            let units = (size as usize / 2).saturating_sub(1);
            Some(buffer[..units].iter().copied().chain(Some(0)).collect())
        } else {
            None
        }
    };

    // SAFETY: Deleting or restoring HKCU value
    unsafe {
        if let Some(orig) = backup {
            // Restore previous custom shell
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                WINLOGON_SUBKEY,
                SHELL_VALUE,
                REG_SZ.0,
                Some(orig.as_ptr().cast()),
                (orig.len() * 2) as u32,
            );
            // Clear backup key
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, LEANSHELL_BACKUP_SUBKEY, ORIGINAL_SHELL_VALUE);
        } else {
            // Fall back to Explorer by deleting per-user Shell override
            let mut key = HKEY::default();
            if RegOpenKeyExW(HKEY_CURRENT_USER, WINLOGON_SUBKEY, Some(0), KEY_SET_VALUE, &mut key).is_ok() {
                let _ = RegDeleteValueW(key, SHELL_VALUE);
                let _ = RegCloseKey(key);
            }
        }

        Ok(())
    }
}

/// Checks if emergency safe-mode modifier (Shift held) is active during logon/startup.
pub fn is_safe_mode_requested() -> bool {
    // SAFETY: Polling physical key state of VK_SHIFT
    unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 }
}
