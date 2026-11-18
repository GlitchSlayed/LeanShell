//! Low-level keyboard hook state tracking and chord detection logic.
//!
//! Uses a 256-bit physical-key bitmask so all physical keys (including left/right
//! modifiers, multiple simultaneous Win keys, pre-existing held keys, and mouse
//! clicks) participate in deciding whether a Win release is a bare tap.

const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;

/// All physical keys participate, including modifiers held before Win-down.
/// Two overlapping Win keys cancel the tap until both have been released.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinKeyTracker {
    held: [u64; 4],
    candidate: Option<u32>,
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
}
