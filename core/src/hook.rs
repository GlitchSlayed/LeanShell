//! Low-level keyboard hook state tracking and chord detection logic.

/// Per-key down-state bitmask for modifier families.
/// Each bit corresponds to one of the two physical keys in the family:
/// bit 0 = generic/left VK, bit 1 = right VK.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct KeyPair(u8);

impl KeyPair {
    fn press(&mut self, slot: u8) {
        self.0 |= 1 << slot;
    }
    fn release(&mut self, slot: u8) {
        self.0 &= !(1 << slot);
    }
    fn any_down(self) -> bool {
        self.0 != 0
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinKeyTracker {
    /// bit 0 = VK_LWIN (0x5B), bit 1 = VK_RWIN (0x5C)
    win: KeyPair,
    chord_detected: bool,
    /// bit 0 = VK_CONTROL/VK_LCONTROL, bit 1 = VK_RCONTROL
    ctrl: KeyPair,
    /// bit 0 = VK_SHIFT/VK_LSHIFT, bit 1 = VK_RSHIFT
    shift: KeyPair,
    /// bit 0 = VK_MENU/VK_LMENU, bit 1 = VK_RMENU
    alt: KeyPair,
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
        // Classify the key
        let (is_lwin, is_rwin) = (vk == 0x5B, vk == 0x5C);
        let is_win = is_lwin || is_rwin;

        // Update modifier families using per-physical-key slots so that
        // "left Ctrl down, right Ctrl down, left Ctrl up" leaves right Ctrl still active.
        match vk {
            0x11 => {
                // VK_CONTROL (generic)
                if is_down {
                    self.ctrl.press(0);
                } else {
                    self.ctrl.release(0);
                }
            }
            0xA2 => {
                // VK_LCONTROL
                if is_down {
                    self.ctrl.press(0);
                } else {
                    self.ctrl.release(0);
                }
            }
            0xA3 => {
                // VK_RCONTROL
                if is_down {
                    self.ctrl.press(1);
                } else {
                    self.ctrl.release(1);
                }
            }
            0x10 => {
                // VK_SHIFT (generic)
                if is_down {
                    self.shift.press(0);
                } else {
                    self.shift.release(0);
                }
            }
            0xA0 => {
                // VK_LSHIFT
                if is_down {
                    self.shift.press(0);
                } else {
                    self.shift.release(0);
                }
            }
            0xA1 => {
                // VK_RSHIFT
                if is_down {
                    self.shift.press(1);
                } else {
                    self.shift.release(1);
                }
            }
            0x12 => {
                // VK_MENU (generic)
                if is_down {
                    self.alt.press(0);
                } else {
                    self.alt.release(0);
                }
            }
            0xA4 => {
                // VK_LMENU
                if is_down {
                    self.alt.press(0);
                } else {
                    self.alt.release(0);
                }
            }
            0xA5 => {
                // VK_RMENU
                if is_down {
                    self.alt.press(1);
                } else {
                    self.alt.release(1);
                }
            }
            _ => {}
        }

        if is_win && is_down {
            // Win key-repeat: do NOT reset chord_detected here — a repeated Win-down after
            // a chord (e.g. Win+E kept held) must still be treated as a chord. Only update
            // the win bitmask and set chord on first press.
            let was_down = self.win.any_down();
            if is_lwin {
                self.win.press(0);
            } else {
                self.win.press(1);
            }
            if !was_down {
                // First Win press: snapshot modifier state
                self.chord_detected = self.ctrl.any_down() || self.shift.any_down() || self.alt.any_down();
            } else {
                // A second Win key pressed while another Win key is already held is a chord.
                // This handles LWin+RWin, which should not claim bare Win on release.
                self.chord_detected = true;
            }
            HookDecision::PassThrough
        } else if !is_win && is_down && self.win.any_down() {
            // Another key while Win is held — chord
            self.chord_detected = true;
            HookDecision::PassThrough
        } else if is_win && !is_down {
            if is_lwin {
                self.win.release(0);
            } else {
                self.win.release(1);
            }
            // Only claim bare-Win tap when NO Win key remains held
            if !self.win.any_down() {
                let was_chord = self.chord_detected;
                self.chord_detected = false;
                if was_chord {
                    HookDecision::PassThrough
                } else {
                    HookDecision::ClaimBareWin
                }
            } else {
                HookDecision::PassThrough
            }
        } else {
            HookDecision::PassThrough
        }
    }

    pub fn is_win_down(&self) -> bool {
        self.win.any_down()
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

    /// Left Ctrl down, right Ctrl down, left Ctrl up → right Ctrl still active → Win tap NOT claimed.
    #[test]
    fn test_dual_ctrl_keys_right_ctrl_still_held() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0xA2, true), HookDecision::PassThrough); // LCtrl down
        assert_eq!(tracker.handle_key(0xA3, true), HookDecision::PassThrough); // RCtrl down
        assert_eq!(tracker.handle_key(0xA2, false), HookDecision::PassThrough); // LCtrl up
        // Win tap — right Ctrl is still down, so this is a chord
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough);
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::PassThrough);
    }

    /// Win key repeat (key-down fires multiple times while held) must not reset chord_detected.
    #[test]
    fn test_win_key_repeat_does_not_reset_chord() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough); // Win down
        assert_eq!(tracker.handle_key(0x45, true), HookDecision::PassThrough); // 'E' down — chord!
        // Win key-repeat while still holding 'E'
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough); // Win repeat
        assert_eq!(tracker.handle_key(0x45, false), HookDecision::PassThrough); // 'E' up
        // Win release must still pass through (chord was detected)
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::PassThrough);
    }

    /// Left Win + Right Win tap — releasing left Win while right is still held should not claim.
    #[test]
    fn test_dual_win_keys_left_release_while_right_held() {
        let mut tracker = WinKeyTracker::new();
        assert_eq!(tracker.handle_key(0x5B, true), HookDecision::PassThrough); // LWin down
        assert_eq!(tracker.handle_key(0x5C, true), HookDecision::PassThrough); // RWin down
        // Release LWin — RWin still held, should NOT claim bare Win
        assert_eq!(tracker.handle_key(0x5B, false), HookDecision::PassThrough);
        // Release RWin — no other Win held, but chord_detected should have been set when
        // RWin went down while LWin was already down (treated as a chord)
        assert_eq!(tracker.handle_key(0x5C, false), HookDecision::PassThrough);
    }
}
