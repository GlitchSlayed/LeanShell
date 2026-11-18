//! Pure policy, validation, and backup-restore planning for IFEO shell host neutering.

/// Windows Shell Experience host processes that waste background memory/CPU
/// when LeanShell owns the shell.
pub const DEFAULT_NEUTERED_HOSTS: &[&str] = &[
    "StartMenuExperienceHost.exe",
    "SearchHost.exe",
    "Widgets.exe",
    "WidgetService.exe",
    "ShellExperienceHost.exe",
    "CrossDeviceService.exe",
];

/// Inbox no-op executable used as the IFEO `Debugger` redirect target so
/// Windows immediately terminates spawned host attempts without UI or prompts.
pub const IFEO_NOOP_DEBUGGER: &str = "systray.exe";

/// Processes that must NEVER be neutered because doing so would break
/// on-demand file browsing, terminals, recovery scripts, DWM, logon, or LeanShell itself.
const PROTECTED_EXECUTABLES: &[&str] = &[
    "explorer.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "conhost.exe",
    "openconsole.exe",
    "windowsterminal.exe",
    "svchost.exe",
    "winlogon.exe",
    "csrss.exe",
    "dwm.exe",
    "taskmgr.exe",
    "lsass.exe",
    "services.exe",
    "smss.exe",
    "wininit.exe",
    "systray.exe",
    "reg.exe",
    "leanshell.exe",
    "leanshell-nightly.exe",
    "leanshell-settings.exe",
    "wisp.exe",
];

/// Validates that `exe` is a plain `.exe` filename and is not a protected
/// system or shell recovery executable.
pub fn is_safe_host_target(exe: &str) -> bool {
    let trimmed = exe.trim();
    if trimmed.is_empty() || trimmed != exe {
        return false;
    }
    if !trimmed.is_ascii()
        || trimmed.contains(['/', '\\', ':', '"', '\'', ' ', '\t', '\r', '\n', '\0'])
        || trimmed.contains("..")
    {
        return false;
    }
    let Some(stem) = trimmed.strip_suffix(".exe").or_else(|| {
        // Case-insensitive .exe check while keeping the stem non-empty.
        (trimmed.len() > 4 && trimmed[trimmed.len() - 4..].eq_ignore_ascii_case(".exe"))
            .then(|| &trimmed[..trimmed.len() - 4])
    }) else {
        return false;
    };
    if stem.is_empty() || stem.contains('.') {
        return false;
    }
    !PROTECTED_EXECUTABLES
        .iter()
        .any(|protected| trimmed.eq_ignore_ascii_case(protected))
}

/// Action to take when restoring an IFEO registry entry for a host executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreIfeoAction {
    /// Restore a pre-existing third-party or user `Debugger` value.
    RestoreOriginal(String),
    /// Remove LeanShell's `Debugger` redirect value.
    DeleteDebugger,
    /// Leave an unrelated `Debugger` value alone.
    LeaveUntouched,
}

/// Computes the exact IFEO rollback action from backup metadata and the current `Debugger` value.
pub fn plan_ifeo_restore(
    managed_by_leanshell: bool,
    original_debugger: Option<&str>,
    current_debugger: Option<&str>,
) -> RestoreIfeoAction {
    if let Some(orig) = original_debugger.map(str::trim).filter(|s| !s.is_empty()) {
        return RestoreIfeoAction::RestoreOriginal(orig.to_string());
    }
    if managed_by_leanshell || current_debugger.is_some_and(|cur| cur.trim().eq_ignore_ascii_case(IFEO_NOOP_DEBUGGER)) {
        return RestoreIfeoAction::DeleteDebugger;
    }
    RestoreIfeoAction::LeaveUntouched
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_neutered_hosts_are_all_safe_targets() {
        assert!(!DEFAULT_NEUTERED_HOSTS.is_empty());
        for exe in DEFAULT_NEUTERED_HOSTS {
            assert!(is_safe_host_target(exe), "{exe} should be accepted");
        }
    }

    #[test]
    fn rejects_protected_and_malformed_executables() {
        for bad in [
            "",
            "explorer.exe",
            "EXPLORER.EXE",
            "cmd.exe",
            "powershell.exe",
            "dwm.exe",
            "winlogon.exe",
            "taskmgr.exe",
            "leanshell.exe",
            "systray.exe",
            "StartMenuExperienceHost",
            ".exe",
            "C:\\Windows\\StartMenuExperienceHost.exe",
            "../SearchHost.exe",
            "Search Host.exe",
            "Search.Host.exe",
            "SearchHost.dll",
            " SearchHost.exe",
        ] {
            assert!(!is_safe_host_target(bad), "{bad:?} must be rejected");
        }
        assert!(is_safe_host_target("CustomHost.EXE"));
    }

    #[test]
    fn plans_ifeo_restore_without_clobbering_unrelated_debuggers() {
        assert_eq!(
            plan_ifeo_restore(true, Some(r"C:\Tools\dbg.exe"), Some(IFEO_NOOP_DEBUGGER)),
            RestoreIfeoAction::RestoreOriginal(r"C:\Tools\dbg.exe".to_string())
        );
        assert_eq!(
            plan_ifeo_restore(true, None, Some(IFEO_NOOP_DEBUGGER)),
            RestoreIfeoAction::DeleteDebugger
        );
        assert_eq!(
            plan_ifeo_restore(false, None, Some("SYSTRAY.EXE")),
            RestoreIfeoAction::DeleteDebugger
        );
        assert_eq!(
            plan_ifeo_restore(false, None, Some(r"C:\Custom\other.exe")),
            RestoreIfeoAction::LeaveUntouched
        );
        assert_eq!(plan_ifeo_restore(false, None, None), RestoreIfeoAction::LeaveUntouched);
    }
}
