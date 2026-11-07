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

/// Space Wisp keeps at the bottom of the primary screen for the dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    /// The screen it is on.
    pub monitor: Rect,
    /// The work area's bottom that Wisp set.
    pub bottom: i32,
    /// The bottom before Wisp changed it, which may leave room for another
    /// appbar.
    pub base: i32,
}

/// The primary screen's work area with `reserve` pixels kept free at the
/// bottom for the dock, and the reservation to remember once it is applied.
/// `previous` comes off first, back to the bottom it replaced, so a smaller
/// reserve, or none, gives the space back without taking another appbar's.
pub fn reserved_work(
    monitor: Rect,
    work: Rect,
    previous: Option<Reservation>,
    reserve: i32,
) -> (Rect, Option<Reservation>) {
    let base = match previous {
        Some(previous) if previous.monitor == monitor && previous.bottom == work.bottom => previous.base,
        _ => work.bottom,
    };
    if reserve <= 0 {
        return (Rect { bottom: base, ..work }, None);
    }
    let bottom = base.min(monitor.bottom - reserve);
    (Rect { bottom, ..work }, Some(Reservation { monitor, bottom, base }))
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
        let at = |bottom| Rect { bottom, ..monitor };
        let (work, kept) = reserved_work(monitor, monitor, None, 74);
        assert_eq!(work, at(1006));
        // Applying the same reserve again changes nothing.
        assert_eq!(reserved_work(monitor, work, kept, 74), (work, kept));
        // A new size replaces the old one, and none gives the space back.
        assert_eq!(reserved_work(monitor, work, kept, 92).0, at(988));
        assert_eq!(reserved_work(monitor, work, kept, 0), (monitor, None));
        // Another appbar's 60 px stays when Wisp gives its space back.
        let (work, kept) = reserved_work(monitor, at(1020), None, 74);
        assert_eq!(work, at(1006));
        assert_eq!(reserved_work(monitor, work, kept, 0).0, at(1020));
        // A work area someone else changed since is left alone.
        assert_eq!(reserved_work(monitor, at(900), kept, 0).0, at(900));
        assert_eq!(reserved_work(monitor, at(900), None, 74).0, at(900));
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
