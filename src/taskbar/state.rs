//! State comparisons independent of Win32.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub const AUTOHIDE: u32 = 1;

pub fn hidden_state(previous: u32) -> u32 {
    previous | AUTOHIDE
}

/// Reclaim only a strip attributable to this taskbar; retain other appbars.
pub fn reclaimed_work(monitor: Rect, work: Rect, bar: Rect) -> Rect {
    let mut target = work;
    if bar.left <= monitor.left && bar.right >= monitor.right {
        if bar.top <= work.bottom && bar.bottom >= monitor.bottom && bar.top > monitor.top {
            target.bottom = monitor.bottom;
        } else if bar.bottom >= work.top && bar.top <= monitor.top && bar.bottom < monitor.bottom {
            target.top = monitor.top;
        }
    } else if bar.top <= monitor.top && bar.bottom >= monitor.bottom {
        if bar.left <= work.right && bar.right >= monitor.right && bar.left > monitor.left {
            target.right = monitor.right;
        } else if bar.right >= work.left && bar.left <= monitor.left && bar.right < monitor.right {
            target.left = monitor.left;
        }
    }
    target
}

/// The primary screen's work area with `reserve` pixels kept free at the
/// bottom for the dock. `applied` is the reserve set last time, which comes
/// off first so a smaller reserve, or none, gives the space back.
pub fn reserved_work(monitor: Rect, work: Rect, applied: i32, reserve: i32) -> Rect {
    let mut target = work;
    if applied > 0 && target.bottom == monitor.bottom - applied {
        target.bottom = monitor.bottom;
    }
    if reserve > 0 {
        target.bottom = target.bottom.min(monitor.bottom - reserve);
    }
    target
}

pub fn visibility_diff(previous: bool, current: bool) -> Option<bool> {
    (previous != current).then_some(previous)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_and_returns_space_for_the_dock() {
        let monitor = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let full = monitor;
        let reserved = Rect {
            bottom: 1006,
            ..monitor
        };
        assert_eq!(reserved_work(monitor, full, 0, 74), reserved);
        // Applying the same reserve again changes nothing.
        assert_eq!(reserved_work(monitor, reserved, 74, 74), reserved);
        // A new size replaces the old one, and none gives the space back.
        assert_eq!(
            reserved_work(monitor, reserved, 74, 92),
            Rect { bottom: 988, ..monitor }
        );
        assert_eq!(reserved_work(monitor, reserved, 74, 0), full);
        // A work area someone else made smaller is left alone.
        let other = Rect { bottom: 900, ..monitor };
        assert_eq!(reserved_work(monitor, other, 74, 0), other);
        assert_eq!(reserved_work(monitor, other, 0, 74), other);
    }

    #[test]
    fn preserves_other_state_bits() {
        for state in 0..8 {
            assert_eq!(hidden_state(state), state | 1);
        }
    }

    #[test]
    fn visibility_is_idempotent() {
        assert_eq!(visibility_diff(true, false), Some(true));
        assert_eq!(visibility_diff(false, true), Some(false));
        assert_eq!(visibility_diff(false, false), None);
        assert_eq!(visibility_diff(true, true), None);
    }

    #[test]
    fn reclaims_only_taskbar_edge() {
        let monitor = Rect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        let work = Rect {
            top: 30,
            bottom: 1040,
            ..monitor
        };
        let bar = Rect { top: 1040, ..monitor };
        assert_eq!(reclaimed_work(monitor, work, bar), Rect { bottom: 1080, ..work });
        let unrelated = Rect {
            left: -1000,
            right: -900,
            top: 500,
            bottom: 600,
        };
        assert_eq!(reclaimed_work(monitor, work, unrelated), work);
        let left = Rect {
            right: -1880,
            ..monitor
        };
        let work = Rect { left: -1880, ..monitor };
        assert_eq!(reclaimed_work(monitor, work, left), monitor);
        let top = Rect { bottom: 40, ..monitor };
        assert_eq!(reclaimed_work(monitor, Rect { top: 40, ..monitor }, top), monitor);
        let right = Rect { left: -40, ..monitor };
        assert_eq!(reclaimed_work(monitor, Rect { right: -40, ..monitor }, right), monitor);
    }
}
