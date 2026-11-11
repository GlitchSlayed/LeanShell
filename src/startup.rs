//! Logon startup application launcher for exclusive shell mode.
//!
//! When LeanShell runs as the exclusive shell (explorer not running at logon),
//! this module executes startup items from HKCU/HKLM Run and RunOnce registry
//! keys and the user/common Startup folders.

use std::path::PathBuf;
use windows::Win32::System::Registry::*;
use windows::core::w;

const RUN_SUBKEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const RUNONCE_SUBKEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\RunOnce");

/// Spawns a background worker thread to enumerate and execute startup applications.
pub fn launch_logon_startup_apps() {
    std::thread::Builder::new()
        .name("leanshell-startup-launcher".into())
        .spawn(|| {
            // 1. Enumerate HKCU Run
            launch_registry_run_keys(HKEY_CURRENT_USER, RUN_SUBKEY, false);
            // 2. Enumerate HKCU RunOnce (delete after launch)
            launch_registry_run_keys(HKEY_CURRENT_USER, RUNONCE_SUBKEY, true);
            // 3. Enumerate HKLM Run (read-only)
            launch_registry_run_keys(HKEY_LOCAL_MACHINE, RUN_SUBKEY, false);
            // 4. Enumerate user and common startup folders
            launch_startup_folders();
        })
        .ok();
}

fn launch_registry_run_keys(root: HKEY, subkey: windows::core::PCWSTR, delete_after: bool) {
    // SAFETY: Reading registry entries to execute user-configured startup apps
    unsafe {
        let mut key = HKEY::default();
        let access = if delete_after {
            KEY_READ | KEY_SET_VALUE
        } else {
            KEY_READ
        };
        if RegOpenKeyExW(root, subkey, Some(0), access, &mut key).is_err() {
            return;
        }

        let mut index = 0u32;
        let mut to_delete = Vec::new();
        loop {
            let mut value_name = [0u16; 260];
            let mut name_len = value_name.len() as u32;
            let mut value_type = 0u32;
            let mut data = [0u8; 1024];
            let mut data_len = data.len() as u32;

            if RegEnumValueW(
                key,
                index,
                Some(windows::core::PWSTR(value_name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut value_type),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            )
            .is_err()
            {
                break;
            }

            if value_type == REG_SZ.0 || value_type == REG_EXPAND_SZ.0 {
                let slice = std::slice::from_raw_parts(data.as_ptr() as *const u16, (data_len / 2) as usize);
                let cmd = String::from_utf16_lossy(slice).trim_matches('\0').trim().to_string();
                if !cmd.is_empty() {
                    let _ = crate::launch::open(&cmd);
                    if delete_after {
                        let name_slice = &value_name[..name_len as usize];
                        to_delete.push(name_slice.to_vec());
                    }
                }
            }

            index += 1;
        }

        if delete_after {
            for name in to_delete {
                let null_term: Vec<u16> = name.into_iter().chain(Some(0)).collect();
                let _ = RegDeleteValueW(key, windows::core::PCWSTR(null_term.as_ptr()));
            }
        }

        let _ = RegCloseKey(key);
    }
}

fn launch_startup_folders() {
    let mut folders = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        folders.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
    }
    if let Some(programdata) = std::env::var_os("ProgramData") {
        folders.push(PathBuf::from(programdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
    }

    for folder in folders {
        if let Ok(entries) = std::fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && let Some(ext) = path.extension().and_then(|e| e.to_str())
                {
                    let ext = ext.to_lowercase();
                    if matches!(ext.as_str(), "lnk" | "exe" | "bat" | "cmd") {
                        let _ = crate::launch::open(&path.to_string_lossy());
                    }
                }
            }
        }
    }
}
