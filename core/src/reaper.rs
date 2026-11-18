//! Pure decision model for on-demand `explorer.exe` auto-reaping in exclusive shell mode.

/// Classification of a visible top-level window owned by `explorer.exe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerWindowKind {
    /// A file-browsing window (`CabinetWClass` or `ExploreWClass`).
    Folder,
    /// An active copy/move/delete/properties dialog (`OperationStatusWindow` or `#32770`).
    FileDialog,
    /// Any other Explorer window (e.g. hidden shell helper or desktop proxy).
    Other,
}

/// Classifies an `explorer.exe` window by its Win32 window class name.
pub fn classify_explorer_window(class_name: &str) -> ExplorerWindowKind {
    match class_name {
        "CabinetWClass" | "ExploreWClass" => ExplorerWindowKind::Folder,
        "OperationStatusWindow" | "#32770" => ExplorerWindowKind::FileDialog,
        _ => ExplorerWindowKind::Other,
    }
}

/// Decision produced after inspecting live `explorer.exe` window counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReapDecision {
    /// Keep `explorer.exe` alive (either not exclusive, reaping disabled, or active folder/dialog open).
    KeepAlive,
    /// No active folders or file dialogs remain; schedule or perform a reap.
    ScheduleReap,
    /// Explorer is not currently running (nothing to reap).
    Idle,
}

/// Stateful helper tracking Explorer reaper scheduling.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExplorerReapState {
    scheduled: bool,
}

impl ExplorerReapState {
    pub const fn new() -> Self {
        Self { scheduled: false }
    }

    pub fn is_scheduled(&self) -> bool {
        self.scheduled
    }

    /// Evaluates whether `explorer.exe` should be reaped given current session and window state.
    pub fn evaluate(
        &mut self,
        is_session_exclusive: bool,
        auto_reap_enabled: bool,
        explorer_running: bool,
        open_folder_windows: usize,
        active_file_dialogs: usize,
    ) -> ReapDecision {
        let decision = evaluate_reap(
            is_session_exclusive,
            auto_reap_enabled,
            explorer_running,
            open_folder_windows,
            active_file_dialogs,
        );
        self.scheduled = decision == ReapDecision::ScheduleReap;
        decision
    }

    pub fn clear(&mut self) {
        self.scheduled = false;
    }
}

/// Pure reap decision function.
pub fn evaluate_reap(
    is_session_exclusive: bool,
    auto_reap_enabled: bool,
    explorer_running: bool,
    open_folder_windows: usize,
    active_file_dialogs: usize,
) -> ReapDecision {
    if !is_session_exclusive || !auto_reap_enabled {
        return ReapDecision::KeepAlive;
    }
    if !explorer_running {
        return ReapDecision::Idle;
    }
    if open_folder_windows > 0 || active_file_dialogs > 0 {
        return ReapDecision::KeepAlive;
    }
    ReapDecision::ScheduleReap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_explorer_window_classes() {
        assert_eq!(classify_explorer_window("CabinetWClass"), ExplorerWindowKind::Folder);
        assert_eq!(classify_explorer_window("ExploreWClass"), ExplorerWindowKind::Folder);
        assert_eq!(
            classify_explorer_window("OperationStatusWindow"),
            ExplorerWindowKind::FileDialog
        );
        assert_eq!(classify_explorer_window("#32770"), ExplorerWindowKind::FileDialog);
        assert_eq!(classify_explorer_window("Shell_TrayWnd"), ExplorerWindowKind::Other);
    }

    #[test]
    fn never_reaps_in_companion_mode_or_when_disabled() {
        assert_eq!(evaluate_reap(false, true, true, 0, 0), ReapDecision::KeepAlive);
        assert_eq!(evaluate_reap(true, false, true, 0, 0), ReapDecision::KeepAlive);
    }

    #[test]
    fn keeps_alive_while_folders_or_file_dialogs_are_open() {
        assert_eq!(evaluate_reap(true, true, true, 1, 0), ReapDecision::KeepAlive);
        assert_eq!(evaluate_reap(true, true, true, 0, 1), ReapDecision::KeepAlive);
        assert_eq!(evaluate_reap(true, true, true, 2, 1), ReapDecision::KeepAlive);
    }

    #[test]
    fn schedules_reap_when_last_folder_and_dialog_close() {
        let mut state = ExplorerReapState::new();
        assert_eq!(state.evaluate(true, true, true, 1, 0), ReapDecision::KeepAlive);
        assert!(!state.is_scheduled());

        assert_eq!(state.evaluate(true, true, true, 0, 0), ReapDecision::ScheduleReap);
        assert!(state.is_scheduled());

        // If a dialog opens before the debounce timer fires, cancel reaping.
        assert_eq!(state.evaluate(true, true, true, 0, 1), ReapDecision::KeepAlive);
        assert!(!state.is_scheduled());

        // When explorer is not running, report Idle.
        assert_eq!(state.evaluate(true, true, false, 0, 0), ReapDecision::Idle);
        assert!(!state.is_scheduled());
    }
}
