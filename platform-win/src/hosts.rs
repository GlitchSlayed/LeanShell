//! Reversible Image File Execution Options (IFEO) neutering of bloated
//! Windows Shell Experience hosts (`StartMenuExperienceHost.exe`, `SearchHost.exe`,
//! `Widgets.exe`, `ShellExperienceHost.exe`, etc.).
//!
//! All mutations under `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options`
//! are transactionally backed up under `HKCU\Software\LeanShell\BackupIFEO` and
//! cleanly reversed by `restore_shell_hosts()`, `--restore-shell`, and `scripts/restore-shell.ps1`.

use leanshell_core::hosts::{
    DEFAULT_NEUTERED_HOSTS, IFEO_NOOP_DEBUGGER, RestoreIfeoAction, is_safe_host_target, plan_ifeo_restore,
};
use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
use windows::core::{HSTRING, PCWSTR, Result, w};

const IFEO_BASE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";
const BACKUP_IFEO_BASE: &str = r"Software\LeanShell\BackupIFEO";
const DEBUGGER_VALUE: PCWSTR = w!("Debugger");
const ORIGINAL_DEBUGGER_VALUE: PCWSTR = w!("OriginalDebugger");
const MANAGED_VALUE: PCWSTR = w!("ManagedByLeanShell");

pub(crate) struct OwnedHandle(pub(crate) HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: Handle was opened by CreateToolhelp32Snapshot or OpenProcess.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn read_reg_sz(root: HKEY, subkey: &str, value_name: PCWSTR) -> Option<String> {
    let subkey_wide = HSTRING::from(subkey);
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: Querying a REG_SZ value into a fixed stack UTF-16 buffer.
    let res = unsafe {
        RegGetValueW(
            root,
            &subkey_wide,
            value_name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if res.is_ok() {
        let units = (size as usize / 2).min(buffer.len());
        let trimmed = String::from_utf16_lossy(&buffer[..units])
            .trim_end_matches('\0')
            .to_string();
        Some(trimmed)
    } else {
        None
    }
}

fn write_reg_sz(root: HKEY, subkey: &str, value_name: PCWSTR, data: &str) -> Result<()> {
    let subkey_wide = HSTRING::from(subkey);
    let wide: Vec<u16> = data.encode_utf16().chain(Some(0)).collect();
    // SAFETY: Writing null-terminated UTF-16 REG_SZ data to an open registry root.
    unsafe {
        RegSetKeyValueW(
            root,
            &subkey_wide,
            value_name,
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            (wide.len() * 2) as u32,
        )
        .ok()
    }
}

fn delete_reg_value(root: HKEY, subkey: &str, value_name: PCWSTR) -> Result<()> {
    let subkey_wide = HSTRING::from(subkey);
    let mut key = HKEY::default();
    // SAFETY: Opening key with KEY_SET_VALUE to delete a single named value.
    let open_res = unsafe { RegOpenKeyExW(root, &subkey_wide, Some(0), KEY_SET_VALUE, &mut key) };
    if open_res == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    open_res.ok()?;
    // SAFETY: Deleting value from opened key handle and always closing it.
    let del_res = unsafe {
        let res = RegDeleteValueW(key, value_name);
        let _ = RegCloseKey(key);
        res
    };
    if del_res == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        del_res.ok()
    }
}

/// Returns PIDs of running processes matching any executable name in `targets`.
pub(crate) fn find_pids_by_name(targets: &[&str]) -> Vec<u32> {
    let mut pids = Vec::new();
    if targets.is_empty() {
        return pids;
    }
    // SAFETY: Taking a process snapshot via ToolHelp32.
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return pids;
    };
    let snapshot = OwnedHandle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: First entry read into initialized PROCESSENTRY32W.
    if unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_err() {
        return pids;
    }
    let self_pid = std::process::id();
    loop {
        if entry.th32ProcessID != 0 && entry.th32ProcessID != self_pid {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if targets.iter().any(|t| name.eq_ignore_ascii_case(t)) {
                pids.push(entry.th32ProcessID);
            }
        }
        // SAFETY: Advancing process snapshot iteration.
        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }
    pids
}

/// Terminates the specified PIDs with `PROCESS_TERMINATE`, returning how many succeeded.
pub(crate) fn terminate_pids(pids: &[u32]) -> usize {
    let mut killed = 0usize;
    for &pid in pids {
        // SAFETY: Opening target process strictly with PROCESS_TERMINATE and closing handle on drop.
        if let Ok(proc_handle) = unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) } {
            let proc_handle = OwnedHandle(proc_handle);
            // SAFETY: Terminating matched process handle with non-zero exit code (1) so
            // Winlogon AutoRestartShell does not respawn an intentionally nuked explorer.exe.
            if unsafe { TerminateProcess(proc_handle.0, 1) }.is_ok() {
                killed += 1;
            }
        }
    }
    killed
}

/// Terminates all currently running instances of processes matching any name in `targets`.
pub fn terminate_processes_by_name(targets: &[&str]) -> usize {
    terminate_pids(&find_pids_by_name(targets))
}

/// Applies reversible IFEO `Debugger = "systray.exe"` neutering to `targets` and
/// kills any already-running instances.
///
/// Returns the number of host targets configured. Fails with access-denied HRESULT
/// when called without administrator privileges.
pub fn neuter_shell_hosts(targets: &[&str]) -> Result<usize> {
    let mut configured = 0usize;
    let mut valid_targets = Vec::with_capacity(targets.len());

    for &exe in targets {
        if !is_safe_host_target(exe) {
            continue;
        }
        valid_targets.push(exe);

        let ifeo_subkey = format!(r"{IFEO_BASE}\{exe}");
        let backup_subkey = format!(r"{BACKUP_IFEO_BASE}\{exe}");

        if let Some(existing) = read_reg_sz(HKEY_LOCAL_MACHINE, &ifeo_subkey, DEBUGGER_VALUE)
            && !existing.trim().eq_ignore_ascii_case(IFEO_NOOP_DEBUGGER)
            && read_reg_sz(HKEY_CURRENT_USER, &backup_subkey, ORIGINAL_DEBUGGER_VALUE).is_none()
        {
            write_reg_sz(HKEY_CURRENT_USER, &backup_subkey, ORIGINAL_DEBUGGER_VALUE, &existing)?;
        }

        write_reg_sz(HKEY_CURRENT_USER, &backup_subkey, MANAGED_VALUE, "1")?;
        write_reg_sz(HKEY_LOCAL_MACHINE, &ifeo_subkey, DEBUGGER_VALUE, IFEO_NOOP_DEBUGGER)?;
        configured += 1;
    }

    let _ = terminate_processes_by_name(&valid_targets);
    Ok(configured)
}

/// Restores all LeanShell-managed IFEO host overrides back to their prior state
/// and removes backup metadata from `HKCU\Software\LeanShell\BackupIFEO`.
pub fn restore_shell_hosts() -> Result<usize> {
    let mut restored = 0usize;
    let mut first_error = None;

    for &exe in DEFAULT_NEUTERED_HOSTS {
        let ifeo_subkey = format!(r"{IFEO_BASE}\{exe}");
        let backup_subkey = format!(r"{BACKUP_IFEO_BASE}\{exe}");

        let managed = read_reg_sz(HKEY_CURRENT_USER, &backup_subkey, MANAGED_VALUE).is_some();
        let original = read_reg_sz(HKEY_CURRENT_USER, &backup_subkey, ORIGINAL_DEBUGGER_VALUE);
        let current = read_reg_sz(HKEY_LOCAL_MACHINE, &ifeo_subkey, DEBUGGER_VALUE);

        let action = plan_ifeo_restore(managed, original.as_deref(), current.as_deref());
        let step_result = match action {
            RestoreIfeoAction::RestoreOriginal(orig) => {
                restored += 1;
                write_reg_sz(HKEY_LOCAL_MACHINE, &ifeo_subkey, DEBUGGER_VALUE, &orig)
            }
            RestoreIfeoAction::DeleteDebugger => {
                restored += 1;
                delete_reg_value(HKEY_LOCAL_MACHINE, &ifeo_subkey, DEBUGGER_VALUE)
            }
            RestoreIfeoAction::LeaveUntouched => Ok(()),
        };

        if let Err(err) = step_result {
            if first_error.is_none() {
                first_error = Some(err);
            }
            continue;
        }

        let _ = delete_reg_value(HKEY_CURRENT_USER, &backup_subkey, ORIGINAL_DEBUGGER_VALUE);
        let _ = delete_reg_value(HKEY_CURRENT_USER, &backup_subkey, MANAGED_VALUE);
    }

    match first_error {
        Some(err) => Err(err),
        None => Ok(restored),
    }
}

/// Returns `true` if `exe` currently has IFEO `Debugger` set to `systray.exe`.
pub fn is_host_neutered(exe: &str) -> bool {
    if !is_safe_host_target(exe) {
        return false;
    }
    let ifeo_subkey = format!(r"{IFEO_BASE}\{exe}");
    read_reg_sz(HKEY_LOCAL_MACHINE, &ifeo_subkey, DEBUGGER_VALUE)
        .is_some_and(|val| val.trim().eq_ignore_ascii_case(IFEO_NOOP_DEBUGGER))
}
