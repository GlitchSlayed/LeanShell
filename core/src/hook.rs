//! Low-level keyboard hook state tracking and chord detection logic.

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinKeyTracker {
    win_down: bool,
    chord_detected: bool,
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
        if is_win && is_down {
            self.win_down = true;
            self.chord_detected = false;
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
    fn test_win_key_tracker_regular_typing() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x41, true), HookDecision::PassThrough); // 'A' down
        assert_eq!(tracker.handle_key(0x41, false), HookDecision::PassThrough); // 'A' up
        assert_eq!(tracker.handle_key(0x42, true), HookDecision::PassThrough); // 'B' down
        assert_eq!(tracker.handle_key(0x42, false), HookDecision::PassThrough); // 'B' up
    }
}
