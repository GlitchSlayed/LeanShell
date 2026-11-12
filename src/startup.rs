//! Logon startup application launcher for exclusive shell mode.
//!
//! When LeanShell runs as the exclusive shell (explorer not running at logon),
//! this module executes startup items from HKCU/HKLM Run and RunOnce registry
//! keys and the user/common Startup folders.
//!
//! # Session-scoped dispatch guard (U09)
//!
//! The original implementation used a named kernel event (`Local\LeanShell_StartupDispatched`)
//! to prevent re-launching startup apps if LeanShell restarted within the same logon session.
//! That event is released when the process exits, so a crash-restart cycle re-dispatches every
//! startup item.
//!
//! The correct guard is a persistent registry key stamped with the Windows *authentication ID*
//! (a LUID from `GetTokenInformation(TokenStatistics)`). The LUID identifies the logon session
//! at the kernel level and is stable across all LeanShell process instances in that session.
//! On first dispatch we write the key; on subsequent launches we find it and skip.
//! The key is under `HKCU\Software\LeanShell\StartupDispatched` and is specific to the LUID,
//! so it does not interfere with other users' sessions or other Windows installations.

use std::path::PathBuf;
use windows::Win32::Foundation::LUID;
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_STATISTICS, TokenStatistics};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{PCWSTR, PWSTR, w};

const RUN_SUBKEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const RUNONCE_SUBKEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\RunOnce");
const DISPATCH_SUBKEY: &str = r"Software\LeanShell\StartupDispatched";

/// Returns the authentication LUID for the current logon session.
///
/// The LUID from `TokenStatistics.AuthenticationId` uniquely identifies the logon session
/// at the kernel level. It is constant for all processes in the same user logon, including
/// after a shell restart or crash-recovery cycle.
fn logon_session_luid() -> Option<LUID> {
    // SAFETY: Querying the current process token for read-only statistics.
    unsafe {
        let mut token = windows::Win32::Foundation::HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut stats = TOKEN_STATISTICS::default();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenStatistics,
            Some(&mut stats as *mut _ as *mut _),
            size_of::<TOKEN_STATISTICS>() as u32,
            &mut returned,
        );
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok.ok()?;
        Some(stats.AuthenticationId)
    }
}

/// Returns `true` if startup apps have already been dispatched in this logon session.
/// Writes a durable registry marker so the guard survives process restarts within the session.
fn check_and_mark_dispatched() -> bool {
    let Some(luid) = logon_session_luid() else {
        // If we cannot determine the session LUID (e.g. token query fails) fall through
        // to dispatch rather than silently suppressing startup items.
        return false;
    };

    // Key name encodes both LUID halves so each logon session has its own marker.
    let marker_value = format!("{:08X}_{:08X}", luid.HighPart, luid.LowPart);
    let subkey_wide: Vec<u16> = DISPATCH_SUBKEY.encode_utf16().chain(Some(0)).collect();
    let marker_wide: Vec<u16> = marker_value.encode_utf16().chain(Some(0)).collect();

    // SAFETY: Reading/writing our own HKCU subkey for session dispatch tracking.
    unsafe {
        // Try to read the marker — if it exists we have already dispatched.
        let mut buffer = [0u16; 4];
        let mut size = (buffer.len() * 2) as u32;
        let already = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(marker_wide.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .is_ok();

        if already {
            return true;
        }

        // Not dispatched yet — write the marker before we start dispatch so that
        // a crash mid-dispatch still marks the session (apps will not relaunch).
        let one: u32 = 1;
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_wide.as_ptr()),
            PCWSTR(marker_wide.as_ptr()),
            REG_DWORD.0,
            Some(one.to_ne_bytes().as_ptr().cast()),
            4,
        );

        false
    }
}

/// Spawns a background worker thread to enumerate and execute startup applications.
///
/// Guarded by a durable per-logon-session registry marker. The marker is keyed by the
/// authentication LUID so it survives process restarts (unlike a named kernel event which
/// disappears when the process exits).
pub fn launch_logon_startup_apps() {
    // Check and stamp the dispatch marker. Returns true if already dispatched this session.
    if check_and_mark_dispatched() {
        return;
    }

    std::thread::Builder::new()
        .name("leanshell-startup-launcher".into())
        .spawn(|| {
            // Initialize COM for the startup thread so shell shortcuts/extensions work
            // SAFETY: Initializing COM apartment on dedicated worker thread.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }

            // 1. Enumerate HKCU Run
            launch_registry_run_keys(HKEY_CURRENT_USER, RUN_SUBKEY, false);
            // 2. Enumerate HKCU RunOnce (honor ! deferral and delete after execution)
            launch_registry_run_keys(HKEY_CURRENT_USER, RUNONCE_SUBKEY, true);
            // 3. Enumerate HKLM Run (read-only)
            launch_registry_run_keys(HKEY_LOCAL_MACHINE, RUN_SUBKEY, false);
            // 4. Enumerate user and common startup folders
            launch_startup_folders();

            // SAFETY: Uninitializing COM on worker thread exit.
            unsafe {
                CoUninitialize();
            }
        })
        .ok();
}

fn expand_environment(raw: &str) -> String {
    let wide: Vec<u16> = raw.encode_utf16().chain(Some(0)).collect();
    let mut buffer = [0u16; 1024];
    // SAFETY: Calling ExpandEnvironmentStringsW with stack buffer
    let len = unsafe { ExpandEnvironmentStringsW(PCWSTR(wide.as_ptr()), Some(&mut buffer)) };
    if len > 0 && (len as usize) <= buffer.len() {
        String::from_utf16_lossy(&buffer[..(len as usize).saturating_sub(1)])
    } else {
        raw.to_string()
    }
}

fn launch_registry_run_keys(root: HKEY, subkey: PCWSTR, is_runonce: bool) {
    // SAFETY: Reading registry entries to execute user-configured startup apps
    unsafe {
        let mut key = HKEY::default();
        let access = if is_runonce { KEY_READ | KEY_SET_VALUE } else { KEY_READ };
        if RegOpenKeyExW(root, subkey, Some(0), access, &mut key).is_err() {
            return;
        }

        let mut index = 0u32;
        let mut to_delete = Vec::new();
        loop {
            let mut value_name = [0u16; 260];
            let mut name_len = value_name.len() as u32;
            let mut value_type = 0u32;
            let mut data = [0u16; 1024]; // Properly aligned 16-bit buffer
            let mut data_len = (data.len() * 2) as u32;

            if RegEnumValueW(
                key,
                index,
                Some(PWSTR(value_name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut value_type),
                Some(data.as_mut_ptr().cast()),
                Some(&mut data_len),
            )
            .is_err()
            {
                break;
            }

            if value_type == REG_SZ.0 || value_type == REG_EXPAND_SZ.0 {
                let units = (data_len as usize / 2).min(data.len());
                let raw_cmd = String::from_utf16_lossy(&data[..units])
                    .trim_matches('\0')
                    .trim()
                    .to_string();

                let cmd = if value_type == REG_EXPAND_SZ.0 {
                    expand_environment(&raw_cmd)
                } else {
                    raw_cmd
                };

                let name_str = String::from_utf16_lossy(&value_name[..name_len as usize]);
                let (prefix, _) = leanshell_core::startup_cmd::parse_runonce_prefix(&name_str);

                if !cmd.is_empty() {
                    let launch_result = crate::launch::command_line(&cmd);
                    if is_runonce {
                        // Defer deletion if ! was present and launch failed
                        let should_delete = if prefix.defer_deletion {
                            launch_result.is_ok()
                        } else {
                            true
                        };
                        if should_delete {
                            to_delete.push(value_name[..name_len as usize].to_vec());
                        }
                    }
                }
            }

            index += 1;
        }

        if is_runonce {
            for name in to_delete {
                let null_term: Vec<u16> = name.into_iter().chain(Some(0)).collect();
                let _ = RegDeleteValueW(key, PCWSTR(null_term.as_ptr()));
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
                    // .lnk and .url are shell shortcuts — ShellExecute handles them
                    // correctly without any command-line parsing. .exe/.bat/.cmd are
                    // plain executables that can be passed to execute().
                    let path_str = path.to_string_lossy();
                    if matches!(ext.as_str(), "lnk" | "url") {
                        let _ = crate::launch::open(&path_str);
                    } else if matches!(ext.as_str(), "exe" | "bat" | "cmd") {
                        let _ = crate::launch::command_line(&path_str);
                    }
                }
            }
        }
    }
}
