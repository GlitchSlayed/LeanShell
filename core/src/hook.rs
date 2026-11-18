//! Low-level keyboard hook state tracking and chord detection logic.
//!
//! Uses a 256-bit physical-key bitmask so all physical keys (including left/right
//! modifiers, multiple simultaneous Win keys, pre-existing held keys, and mouse
//! clicks) participate in deciding whether a Win release is a bare tap.

const VK_TAB: u32 = 0x09;
const VK_SHIFT: u32 = 0x10;
const VK_MENU: u32 = 0x12;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;
const VK_VOLUME_MUTE: u32 = 0xAD;
const VK_VOLUME_DOWN: u32 = 0xAE;
const VK_VOLUME_UP: u32 = 0xAF;

/// System shell shortcuts and media volume keys handled natively by LeanShell
/// when `claim_shell_chords` is enabled (e.g., exclusive shell mode).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellChord {
    OpenFiles = 1,
    RunPrompt = 2,
    ShowDesktop = 3,
    OpenSettings = 4,
    QuickPowerMenu = 5,
    VolumeUp = 6,
    VolumeDown = 7,
    VolumeMute = 8,
    AltTabNext = 9,
    AltTabPrev = 10,
    AltReleased = 11,
    TaskView = 12,
    SnapLeft = 13,
    SnapRight = 14,
    SnapUp = 15,
    SnapDown = 16,
    ToggleTiling = 17,
}

impl ShellChord {
    pub const fn from_raw(raw: usize) -> Option<Self> {
        match raw {
            1 => Some(Self::OpenFiles),
            2 => Some(Self::RunPrompt),
            3 => Some(Self::ShowDesktop),
            4 => Some(Self::OpenSettings),
            5 => Some(Self::QuickPowerMenu),
            6 => Some(Self::VolumeUp),
            7 => Some(Self::VolumeDown),
            8 => Some(Self::VolumeMute),
            9 => Some(Self::AltTabNext),
            10 => Some(Self::AltTabPrev),
            11 => Some(Self::AltReleased),
            12 => Some(Self::TaskView),
            13 => Some(Self::SnapLeft),
            14 => Some(Self::SnapRight),
            15 => Some(Self::SnapUp),
            16 => Some(Self::SnapDown),
            17 => Some(Self::ToggleTiling),
            _ => None,
        }
    }

    /// Returns `true` when this chord was triggered with a Win key held and needs
    /// the unassigned `0xE8` mask pair so Windows does not pop Start on Win release.
    pub const fn requires_win_mask(self) -> bool {
        matches!(
            self,
            Self::OpenFiles
                | Self::RunPrompt
                | Self::ShowDesktop
                | Self::OpenSettings
                | Self::QuickPowerMenu
                | Self::TaskView
                | Self::SnapLeft
                | Self::SnapRight
                | Self::SnapUp
                | Self::SnapDown
                | Self::ToggleTiling
        )
    }

    /// Returns `true` when the hook callback must forward the physical key event
    /// rather than swallowing it (e.g., `Alt` key-up on `AltReleased`).
    pub const fn pass_through_key(self) -> bool {
        matches!(self, Self::AltReleased)
    }
}

fn sole_win_down(held: &[u64; 4]) -> bool {
    let slot = VK_LWIN as usize / 64;
    let lbit = 1u64 << (VK_LWIN % 64);
    let rbit = 1u64 << (VK_RWIN % 64);
    held.iter().enumerate().all(|(i, &bits)| {
        if i == slot {
            bits == lbit || bits == rbit
        } else {
            bits == 0
        }
    })
}

fn only_vk_or_none_held(held: &[u64; 4], vk: u32) -> bool {
    if *held == [0; 4] {
        return true;
    }
    let slot = vk as usize / 64;
    let bit = 1u64 << (vk % 64);
    held.iter()
        .enumerate()
        .all(|(i, &bits)| if i == slot { bits == bit } else { bits == 0 })
}

fn is_alt_vk(vk: u32) -> bool {
    matches!(vk, VK_MENU | VK_LMENU | VK_RMENU)
}

fn alt_held_in(held: &[u64; 4]) -> bool {
    (held[0] & (1u64 << VK_MENU)) != 0 || (held[2] & ((1u64 << (VK_LMENU % 64)) | (1u64 << (VK_RMENU % 64)))) != 0
}

/// Checks if only Alt (and optionally Shift / Tab) are currently held.
/// Returns `Some(shift_held)` when valid for `Alt+Tab` / `Alt+Shift+Tab`.
fn alt_tab_modifiers(held: &[u64; 4]) -> Option<bool> {
    if held[1] != 0 || held[3] != 0 || !alt_held_in(held) {
        return None;
    }
    let allowed0 = (1u64 << VK_TAB) | (1u64 << VK_SHIFT) | (1u64 << VK_MENU);
    let allowed2 =
        (1u64 << (VK_LSHIFT % 64)) | (1u64 << (VK_RSHIFT % 64)) | (1u64 << (VK_LMENU % 64)) | (1u64 << (VK_RMENU % 64));
    if (held[0] & !allowed0) != 0 || (held[2] & !allowed2) != 0 {
        return None;
    }
    let shift = (held[0] & (1u64 << VK_SHIFT)) != 0
        || (held[2] & ((1u64 << (VK_LSHIFT % 64)) | (1u64 << (VK_RSHIFT % 64)))) != 0;
    Some(shift)
}

/// All physical keys participate, including modifiers held before Win-down.
/// Two overlapping Win keys cancel the tap until both have been released.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinKeyTracker {
    held: [u64; 4],
    candidate: Option<u32>,
    alt_tab_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookDecision {
    PassThrough,
    ClaimBareWin,
}

impl WinKeyTracker {
    pub const fn new() -> Self {
        Self {
            held: [0; 4],
            candidate: None,
            alt_tab_active: false,
        }
    }

    /// Checks whether a low-level key event matches a shell chord (`Win+E/R/D/I/X`,
    /// `Win+Tab`, `Win+Arrow`, `Win+T`, `Alt+Tab` / `Alt+Shift+Tab` / `Alt` release)
    /// or hardware volume key (`VK_VOLUME_UP/DOWN/MUTE`) before `key()` updates `held`.
    ///
    /// Cancels any pending bare-Win tap candidate when a `Win+*` chord matches.
    pub fn check_chord(&mut self, vk: u32, down: bool, injected: bool, claim_chords: bool) -> Option<ShellChord> {
        if injected || vk >= 256 {
            return None;
        }
        if !down {
            if self.alt_tab_active && is_alt_vk(vk) {
                let mut after = self.held;
                after[vk as usize / 64] &= !(1u64 << (vk % 64));
                if !alt_held_in(&after) {
                    self.alt_tab_active = false;
                    return Some(ShellChord::AltReleased);
                }
            }
            return None;
        }
        if !claim_chords {
            return None;
        }
        if vk == VK_TAB
            && let Some(shift) = alt_tab_modifiers(&self.held)
        {
            self.alt_tab_active = true;
            return Some(if shift {
                ShellChord::AltTabPrev
            } else {
                ShellChord::AltTabNext
            });
        }
        match vk {
            VK_VOLUME_UP if only_vk_or_none_held(&self.held, vk) => Some(ShellChord::VolumeUp),
            VK_VOLUME_DOWN if only_vk_or_none_held(&self.held, vk) => Some(ShellChord::VolumeDown),
            VK_VOLUME_MUTE if self.held == [0; 4] => Some(ShellChord::VolumeMute),
            0x45 if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::OpenFiles)
            }
            0x52 if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::RunPrompt)
            }
            0x44 if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::ShowDesktop)
            }
            0x49 if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::OpenSettings)
            }
            0x58 if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::QuickPowerMenu)
            }
            VK_TAB if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::TaskView)
            }
            VK_LEFT if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::SnapLeft)
            }
            VK_RIGHT if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::SnapRight)
            }
            VK_UP if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::SnapUp)
            }
            VK_DOWN if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::SnapDown)
            }
            0x54 if sole_win_down(&self.held) => {
                self.candidate = None;
                Some(ShellChord::ToggleTiling)
            }
            _ => None,
        }
    }

    /// Processes a low-level keyboard event. Returns `true` when releasing a
    /// bare Win tap that had no intervening key press, chord, or mouse click.
    pub fn key(&mut self, vk: u32, down: bool, injected: bool) -> bool {
        if injected || vk >= 256 {
            return false;
        }
        let slot = vk as usize / 64;
        let bit = 1u64 << (vk % 64);
        let was_down = self.held[slot] & bit != 0;
        let win = vk == VK_LWIN || vk == VK_RWIN;
        if down {
            if !was_down {
                self.candidate = if win && self.held == [0; 4] { Some(vk) } else { None };
                self.held[slot] |= bit;
            }
            false
        } else {
            self.held[slot] &= !bit;
            let tap = was_down && self.candidate == Some(vk);
            if win {
                self.candidate = None;
            }
            tap
        }
    }

    /// Cancels any pending Win tap candidate when a physical mouse click or wheel occurs.
    pub fn click(&mut self) {
        self.candidate = None;
    }

    /// Clears the pending tap candidate (used after seeding initially held keys).
    pub fn clear_candidate(&mut self) {
        self.candidate = None;
    }

    /// Non-injected key helper returning `HookDecision`.
    pub fn handle_key(&mut self, vk: u32, is_down: bool) -> HookDecision {
        if self.key(vk, is_down, false) {
            HookDecision::ClaimBareWin
        } else {
            HookDecision::PassThrough
        }
    }

    pub fn is_win_down(&self) -> bool {
        let slot = (VK_LWIN as usize) / 64;
        let lbit = 1u64 << (VK_LWIN % 64);
        let rbit = 1u64 << (VK_RWIN % 64);
        (self.held[slot] & (lbit | rbit)) != 0
    }

    pub fn is_chord_detected(&self) -> bool {
        self.is_win_down() && self.candidate.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: u32 = VK_LWIN;
    const R: u32 = VK_RWIN;
    const E: u32 = b'E' as u32;

    fn sequence(events: &[(u32, bool, bool)], expected: &[usize]) {
        let mut tracker = WinKeyTracker::new();
        let taps: Vec<_> = events
            .iter()
            .enumerate()
            .filter_map(|(i, &(vk, down, injected))| tracker.key(vk, down, injected).then_some(i))
            .collect();
        assert_eq!(taps, expected);
    }

    #[test]
    fn lone_and_repeated_taps_on_either_side() {
        sequence(
            &[
                (L, true, false),
                (L, false, false),
                (R, true, false),
                (R, false, false),
                (L, true, false),
                (L, false, false),
            ],
            &[1, 3, 5],
        );
    }

    #[test]
    fn chords_cancel_even_after_other_key_released() {
        sequence(
            &[(L, true, false), (E, true, false), (E, false, false), (L, false, false)],
            &[],
        );
        sequence(
            &[(L, true, false), (E, true, false), (L, false, false), (E, false, false)],
            &[],
        );
        sequence(
            &[(E, true, false), (L, true, false), (E, false, false), (L, false, false)],
            &[],
        );
    }

    #[test]
    fn overlapping_win_keys_cancel_both_taps() {
        for (a, b) in [(L, R), (R, L)] {
            sequence(
                &[
                    (a, true, false),
                    (b, true, false),
                    (a, false, false),
                    (b, false, false),
                    (a, true, false),
                    (a, false, false),
                ],
                &[5],
            );
        }
    }

    #[test]
    fn win_auto_repeat_toggles_once() {
        sequence(
            &[
                (L, true, false),
                (L, true, false),
                (L, true, false),
                (L, false, false),
                (L, false, false),
            ],
            &[3],
        );
    }

    #[test]
    fn injected_events_do_not_start_cancel_or_complete_taps() {
        sequence(
            &[(L, true, true), (E, true, true), (E, false, true), (L, false, true)],
            &[],
        );
        sequence(
            &[
                (L, true, false),
                (E, true, true),
                (E, false, true),
                (L, false, true),
                (L, false, false),
            ],
            &[4],
        );
        // The dock injects Ctrl+Win+Arrow, all of which must be ignored.
        sequence(
            &[
                (0xA2, true, true),
                (L, true, true),
                (0x27, true, true),
                (0x27, false, true),
                (L, false, true),
                (0xA2, false, true),
            ],
            &[],
        );
    }

    #[test]
    fn mouse_click_cancels_until_next_tap() {
        let mut tracker = WinKeyTracker::new();
        assert!(!tracker.key(L, true, false));
        tracker.click();
        assert!(!tracker.key(L, false, false));
        assert!(!tracker.key(L, true, false));
        assert!(tracker.key(L, false, false));
    }

    #[test]
    fn test_win_key_tracker_bare_win() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(VK_LWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, false), HookDecision::ClaimBareWin);
    }

    #[test]
    fn test_win_key_tracker_chord_win_e() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(VK_LWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x45, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x45, false), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_win_key_tracker_pre_existing_modifiers() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x11, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, false), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x11, false), HookDecision::PassThrough);

        assert_eq!(tracker.handle_key(0x10, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_win_key_tracker_regular_typing() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x41, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x41, false), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x42, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x42, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_dual_ctrl_keys_right_ctrl_still_held() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0xA2, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0xA3, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0xA2, false), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_win_key_repeat_does_not_reset_chord() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(VK_LWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x45, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x45, false), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_dual_win_keys_left_release_while_right_held() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(VK_LWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_RWIN, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_LWIN, false), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(VK_RWIN, false), HookDecision::PassThrough);
    }

    #[test]
    fn claims_all_win_shell_chords_once_and_cancels_bare_win_tap() {
        for (vk, expected) in [
            (b'E' as u32, ShellChord::OpenFiles),
            (b'R' as u32, ShellChord::RunPrompt),
            (b'D' as u32, ShellChord::ShowDesktop),
            (b'I' as u32, ShellChord::OpenSettings),
            (b'X' as u32, ShellChord::QuickPowerMenu),
        ] {
            for win in [L, R] {
                let mut tracker = WinKeyTracker::new();
                assert_eq!(tracker.check_chord(win, true, false, true), None);
                assert!(!tracker.key(win, true, false));

                assert_eq!(tracker.check_chord(vk, true, false, true), Some(expected));
                assert!(!tracker.key(vk, true, false));

                // Auto-repeat while held must not refire the chord.
                assert_eq!(tracker.check_chord(vk, true, false, true), None);
                assert!(!tracker.key(vk, true, false));

                assert_eq!(tracker.check_chord(vk, false, false, true), None);
                assert!(!tracker.key(vk, false, false));

                // Releasing Win after chord must NOT fire bare Win tap.
                assert_eq!(tracker.check_chord(win, false, false, true), None);
                assert!(!tracker.key(win, false, false));
                assert!(expected.requires_win_mask());
                assert_eq!(ShellChord::from_raw(expected as usize), Some(expected));
            }
        }
    }

    #[test]
    fn ignores_modified_or_disabled_shell_chords() {
        let mut tracker = WinKeyTracker::new();
        // Ctrl + Win + E must pass through (not bare Win+E).
        assert!(!tracker.key(0xA2, true, false)); // VK_LCONTROL
        assert!(!tracker.key(L, true, false));
        assert_eq!(tracker.check_chord(b'E' as u32, true, false, true), None);
        assert!(!tracker.key(b'E' as u32, true, false));
        assert!(!tracker.key(b'E' as u32, false, false));
        assert!(!tracker.key(L, false, false));
        assert!(!tracker.key(0xA2, false, false));

        // Win + Shift + D must pass through.
        assert!(!tracker.key(L, true, false));
        assert!(!tracker.key(0xA0, true, false)); // VK_LSHIFT
        assert_eq!(tracker.check_chord(b'D' as u32, true, false, true), None);
        assert!(!tracker.key(0xA0, false, false));
        assert!(!tracker.key(L, false, false));

        // Disabled claim_chords returns None even for Win+E.
        assert!(!tracker.key(L, true, false));
        assert_eq!(tracker.check_chord(b'E' as u32, true, false, false), None);
    }

    #[test]
    fn handles_volume_keys_and_repeat_behavior() {
        let mut tracker = WinKeyTracker::new();
        // Volume up/down repeat when held alone.
        assert_eq!(
            tracker.check_chord(VK_VOLUME_UP, true, false, true),
            Some(ShellChord::VolumeUp)
        );
        assert!(!tracker.key(VK_VOLUME_UP, true, false));
        assert_eq!(
            tracker.check_chord(VK_VOLUME_UP, true, false, true),
            Some(ShellChord::VolumeUp)
        );
        assert!(!tracker.key(VK_VOLUME_UP, false, false));

        assert_eq!(
            tracker.check_chord(VK_VOLUME_DOWN, true, false, true),
            Some(ShellChord::VolumeDown)
        );
        assert!(!tracker.key(VK_VOLUME_DOWN, true, false));
        assert!(!tracker.key(VK_VOLUME_DOWN, false, false));

        // Volume mute fires once and ignores auto-repeat.
        assert_eq!(
            tracker.check_chord(VK_VOLUME_MUTE, true, false, true),
            Some(ShellChord::VolumeMute)
        );
        assert!(!tracker.key(VK_VOLUME_MUTE, true, false));
        assert_eq!(tracker.check_chord(VK_VOLUME_MUTE, true, false, true), None);
        assert!(!tracker.key(VK_VOLUME_MUTE, false, false));

        // Modified volume key (e.g. Ctrl+VolUp) is not claimed.
        assert!(!tracker.key(0xA2, true, false));
        assert_eq!(tracker.check_chord(VK_VOLUME_UP, true, false, true), None);
        assert!(!tracker.key(0xA2, false, false));

        // Injected volume keys are ignored.
        assert_eq!(tracker.check_chord(VK_VOLUME_UP, true, true, true), None);
        assert!(!ShellChord::VolumeUp.requires_win_mask());
    }

    #[test]
    fn handles_alt_tab_and_win_wm_chords() {
        let mut tracker = WinKeyTracker::new();
        // Alt+Tab forward, then Alt+Shift+Tab backward, then Alt release.
        assert_eq!(tracker.check_chord(VK_LMENU, true, false, true), None);
        assert!(!tracker.key(VK_LMENU, true, false));
        assert_eq!(
            tracker.check_chord(VK_TAB, true, false, true),
            Some(ShellChord::AltTabNext)
        );
        assert!(!tracker.key(VK_TAB, true, false));
        assert!(!tracker.key(VK_TAB, false, false));

        assert!(!tracker.key(VK_LSHIFT, true, false));
        assert_eq!(
            tracker.check_chord(VK_TAB, true, false, true),
            Some(ShellChord::AltTabPrev)
        );
        assert!(!tracker.key(VK_TAB, true, false));
        assert!(!tracker.key(VK_TAB, false, false));
        assert!(!tracker.key(VK_LSHIFT, false, false));

        assert_eq!(
            tracker.check_chord(VK_LMENU, false, false, true),
            Some(ShellChord::AltReleased)
        );
        assert!(!tracker.key(VK_LMENU, false, false));
        assert!(ShellChord::AltReleased.pass_through_key());

        // Ctrl+Alt+Tab must NOT be claimed as Alt+Tab.
        assert!(!tracker.key(0xA2, true, false));
        assert!(!tracker.key(VK_LMENU, true, false));
        assert_eq!(tracker.check_chord(VK_TAB, true, false, true), None);
        assert!(!tracker.key(VK_LMENU, false, false));
        assert!(!tracker.key(0xA2, false, false));

        // Win+Tab, Win+Arrows, Win+T
        for (vk, expected) in [
            (VK_TAB, ShellChord::TaskView),
            (VK_LEFT, ShellChord::SnapLeft),
            (VK_RIGHT, ShellChord::SnapRight),
            (VK_UP, ShellChord::SnapUp),
            (VK_DOWN, ShellChord::SnapDown),
            (b'T' as u32, ShellChord::ToggleTiling),
        ] {
            let mut t = WinKeyTracker::new();
            assert!(!t.key(L, true, false));
            assert_eq!(t.check_chord(vk, true, false, true), Some(expected));
            assert!(!t.key(vk, true, false));
            assert!(!t.key(vk, false, false));
            assert!(!t.key(L, false, false));
            assert!(expected.requires_win_mask());
        }

        // Ctrl+Win+Left must pass through for Virtual Desktop switching.
        let mut t = WinKeyTracker::new();
        assert!(!t.key(0xA2, true, false));
        assert!(!t.key(L, true, false));
        assert_eq!(t.check_chord(VK_LEFT, true, false, true), None);
    }
}
