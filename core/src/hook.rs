//! Low-level keyboard hook state tracking and chord detection logic.

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinKeyTracker {
    win_down: bool,
    chord_detected: bool,
    ctrl_down: bool,
    shift_down: bool,
    alt_down: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookDecision {
    PassThrough,
    ClaimBareWin,
}

impl WinKeyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_key(&mut self, vk: u32, is_down: bool) -> HookDecision {
        let is_win = vk == 0x5B || vk == 0x5C; // VK_LWIN, VK_RWIN
        let is_ctrl = vk == 0x11 || vk == 0xA2 || vk == 0xA3;
        let is_shift = vk == 0x10 || vk == 0xA0 || vk == 0xA1;
        let is_alt = vk == 0x12 || vk == 0xA4 || vk == 0xA5;

        if is_ctrl {
            self.ctrl_down = is_down;
        }
        if is_shift {
            self.shift_down = is_down;
        }
        if is_alt {
            self.alt_down = is_down;
        }

        if is_win && is_down {
            self.win_down = true;
            // Pre-existing modifiers (e.g. Ctrl/Shift/Alt held before Win) mark this as a chord
            self.chord_detected = self.ctrl_down || self.shift_down || self.alt_down;
            HookDecision::PassThrough
        } else if !is_win && is_down && self.win_down {
            self.chord_detected = true;
            HookDecision::PassThrough
        } else if is_win && !is_down {
            self.win_down = false;
            if self.chord_detected {
                self.chord_detected = false;
                HookDecision::PassThrough
            } else {
                HookDecision::ClaimBareWin
            }
        } else {
            HookDecision::PassThrough
        }
    }

    pub fn is_win_down(&self) -> bool {
        self.win_down
    }

    pub fn is_chord_detected(&self) -> bool {
        self.chord_detected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_win_key_tracker_bare_win() {
        let mut tracker = WinKeyTracker::new();
        // Win down
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        // Win up (bare) -> ClaimBareWin!
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::ClaimBareWin);
    }

    #[test]
    fn test_win_key_tracker_chord_win_e() {
        let mut tracker = WinKeyTracker::new();
        // Win down
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        // 'E' down (chord)
        assert_eq!(tracker.handle_key(0x45, true), HookDecision::PassThrough);
        // 'E' up
        assert_eq!(tracker.handle_key(0x45, false), HookDecision::PassThrough);
        // Win up -> PassThrough (NOT bare Win!)
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_win_key_tracker_pre_existing_modifiers() {
        let mut tracker = WinKeyTracker::new();
        // Ctrl down first
        assert_eq!(tracker.handle_key(0x11, true), HookDecision::PassThrough);
        // Win down
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        // Win up -> must NOT claim bare Win because Ctrl was held!
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::PassThrough);
        // Ctrl up
        assert_eq!(tracker.handle_key(0x11, false), HookDecision::PassThrough);

        // Shift down first
        assert_eq!(tracker.handle_key(0x10, true), HookDecision::PassThrough);
        // Win down
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        // Win up -> PassThrough
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::PassThrough);
    }

    #[test]
    fn test_win_key_tracker_regular_typing() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x41, true), HookDecision::PassThrough); // 'A' down
        assert_eq!(tracker.handle_key(0x41, false), HookDecision::PassThrough); // 'A' up
        assert_eq!(tracker.handle_key(0x42, true), HookDecision::PassThrough); // 'B' down
        assert_eq!(tracker.handle_key(0x42, false), HookDecision::PassThrough); // 'B' up
    }
}
