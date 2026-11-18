//! Pure window management engine: snap zones, keyboard/mouse snap state
//! machine, Master-Stack dynamic tiling, and Direct2D switcher layout/hit-testing.

/// Integer screen or DIP bounding rectangle (exclusive right/bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WmRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WmRect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn width(self) -> i32 {
        let w = self.right - self.left;
        if w > 0 { w } else { 0 }
    }

    pub const fn height(self) -> i32 {
        let h = self.bottom - self.top;
        if h > 0 { h } else { 0 }
    }

    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    /// Insets the rectangle on all sides without inverting edges.
    pub const fn inset(self, pad: i32) -> Self {
        let pad = if pad > 0 { pad } else { 0 };
        let left = self.left + pad;
        let top = self.top + pad;
        let right = self.right - pad;
        let bottom = self.bottom - pad;
        Self {
            left,
            top,
            right: if right > left { right } else { left },
            bottom: if bottom > top { bottom } else { top },
        }
    }
}

/// Screen edge, quarter, or maximized snap zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapZone {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Maximize,
}

impl SnapZone {
    /// Computes the pixel rectangle inside `work` with `gap` spacing.
    pub fn rect(self, work: WmRect, gap: i32) -> WmRect {
        let gap = gap.max(0);
        let inner = work.inset(gap);
        let half_w = (inner.width() - gap).max(0) / 2;
        let half_h = (inner.height() - gap).max(0) / 2;
        let left_r = inner.left + half_w;
        let right_l = left_r + gap;
        let top_b = inner.top + half_h;
        let bottom_t = top_b + gap;

        match self {
            SnapZone::LeftHalf => WmRect::new(inner.left, inner.top, left_r, inner.bottom),
            SnapZone::RightHalf => WmRect::new(right_l, inner.top, inner.right, inner.bottom),
            SnapZone::TopHalf => WmRect::new(inner.left, inner.top, inner.right, top_b),
            SnapZone::BottomHalf => WmRect::new(inner.left, bottom_t, inner.right, inner.bottom),
            SnapZone::TopLeft => WmRect::new(inner.left, inner.top, left_r, top_b),
            SnapZone::TopRight => WmRect::new(right_l, inner.top, inner.right, top_b),
            SnapZone::BottomLeft => WmRect::new(inner.left, bottom_t, left_r, inner.bottom),
            SnapZone::BottomRight => WmRect::new(right_l, bottom_t, inner.right, inner.bottom),
            SnapZone::Maximize => inner,
        }
    }
}

/// Arrow direction for `Win+Arrow` keyboard snapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Resulting window operation for a keyboard snap transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapAction {
    Snap(SnapZone),
    Restore,
    Minimize,
}

/// Computes the next `Win+Arrow` snap transition from the window's current zone.
pub const fn next_keyboard_snap(current: Option<SnapZone>, is_maximized: bool, dir: SnapDirection) -> SnapAction {
    if is_maximized && !matches!(current, Some(z) if !matches!(z, SnapZone::Maximize)) {
        return match dir {
            SnapDirection::Left => SnapAction::Snap(SnapZone::LeftHalf),
            SnapDirection::Right => SnapAction::Snap(SnapZone::RightHalf),
            SnapDirection::Up => SnapAction::Snap(SnapZone::Maximize),
            SnapDirection::Down => SnapAction::Restore,
        };
    }
    match (current, dir) {
        (None, SnapDirection::Left) => SnapAction::Snap(SnapZone::LeftHalf),
        (None, SnapDirection::Right) => SnapAction::Snap(SnapZone::RightHalf),
        (None, SnapDirection::Up) => SnapAction::Snap(SnapZone::Maximize),
        (None, SnapDirection::Down) => SnapAction::Minimize,

        (Some(SnapZone::LeftHalf), SnapDirection::Left) => SnapAction::Snap(SnapZone::LeftHalf),
        (Some(SnapZone::LeftHalf), SnapDirection::Right) => SnapAction::Restore,
        (Some(SnapZone::LeftHalf), SnapDirection::Up) => SnapAction::Snap(SnapZone::TopLeft),
        (Some(SnapZone::LeftHalf), SnapDirection::Down) => SnapAction::Snap(SnapZone::BottomLeft),

        (Some(SnapZone::RightHalf), SnapDirection::Left) => SnapAction::Restore,
        (Some(SnapZone::RightHalf), SnapDirection::Right) => SnapAction::Snap(SnapZone::RightHalf),
        (Some(SnapZone::RightHalf), SnapDirection::Up) => SnapAction::Snap(SnapZone::TopRight),
        (Some(SnapZone::RightHalf), SnapDirection::Down) => SnapAction::Snap(SnapZone::BottomRight),

        (Some(SnapZone::TopHalf), SnapDirection::Left) => SnapAction::Snap(SnapZone::TopLeft),
        (Some(SnapZone::TopHalf), SnapDirection::Right) => SnapAction::Snap(SnapZone::TopRight),
        (Some(SnapZone::TopHalf), SnapDirection::Up) => SnapAction::Snap(SnapZone::Maximize),
        (Some(SnapZone::TopHalf), SnapDirection::Down) => SnapAction::Restore,

        (Some(SnapZone::BottomHalf), SnapDirection::Left) => SnapAction::Snap(SnapZone::BottomLeft),
        (Some(SnapZone::BottomHalf), SnapDirection::Right) => SnapAction::Snap(SnapZone::BottomRight),
        (Some(SnapZone::BottomHalf), SnapDirection::Up) => SnapAction::Restore,
        (Some(SnapZone::BottomHalf), SnapDirection::Down) => SnapAction::Minimize,

        (Some(SnapZone::TopLeft), SnapDirection::Left) => SnapAction::Snap(SnapZone::TopLeft),
        (Some(SnapZone::TopLeft), SnapDirection::Right) => SnapAction::Snap(SnapZone::TopHalf),
        (Some(SnapZone::TopLeft), SnapDirection::Up) => SnapAction::Snap(SnapZone::Maximize),
        (Some(SnapZone::TopLeft), SnapDirection::Down) => SnapAction::Snap(SnapZone::LeftHalf),

        (Some(SnapZone::TopRight), SnapDirection::Left) => SnapAction::Snap(SnapZone::TopHalf),
        (Some(SnapZone::TopRight), SnapDirection::Right) => SnapAction::Snap(SnapZone::TopRight),
        (Some(SnapZone::TopRight), SnapDirection::Up) => SnapAction::Snap(SnapZone::Maximize),
        (Some(SnapZone::TopRight), SnapDirection::Down) => SnapAction::Snap(SnapZone::RightHalf),

        (Some(SnapZone::BottomLeft), SnapDirection::Left) => SnapAction::Snap(SnapZone::BottomLeft),
        (Some(SnapZone::BottomLeft), SnapDirection::Right) => SnapAction::Snap(SnapZone::BottomHalf),
        (Some(SnapZone::BottomLeft), SnapDirection::Up) => SnapAction::Snap(SnapZone::LeftHalf),
        (Some(SnapZone::BottomLeft), SnapDirection::Down) => SnapAction::Minimize,

        (Some(SnapZone::BottomRight), SnapDirection::Left) => SnapAction::Snap(SnapZone::BottomHalf),
        (Some(SnapZone::BottomRight), SnapDirection::Right) => SnapAction::Snap(SnapZone::BottomRight),
        (Some(SnapZone::BottomRight), SnapDirection::Up) => SnapAction::Snap(SnapZone::RightHalf),
        (Some(SnapZone::BottomRight), SnapDirection::Down) => SnapAction::Minimize,

        (Some(SnapZone::Maximize), SnapDirection::Left) => SnapAction::Snap(SnapZone::LeftHalf),
        (Some(SnapZone::Maximize), SnapDirection::Right) => SnapAction::Snap(SnapZone::RightHalf),
        (Some(SnapZone::Maximize), SnapDirection::Up) => SnapAction::Snap(SnapZone::Maximize),
        (Some(SnapZone::Maximize), SnapDirection::Down) => SnapAction::Restore,
    }
}

/// Detects an Aero-style mouse drag snap zone at screen coordinates `(x, y)` on `monitor`.
///
/// Bottom-center is intentionally left free so dragging near the dock never triggers
/// an accidental snap.
pub fn detect_mouse_snap_zone(
    x: i32,
    y: i32,
    monitor: WmRect,
    edge_threshold: i32,
    corner_span: i32,
) -> Option<SnapZone> {
    if !monitor.contains(x, y) {
        return None;
    }
    let edge = edge_threshold.max(1);
    let corner = corner_span.max(edge);

    let near_left = x < monitor.left + edge;
    let near_right = x >= monitor.right - edge;
    let near_top = y < monitor.top + edge;
    let near_bottom = y >= monitor.bottom - edge;

    let in_top_corner = y < monitor.top + corner;
    let in_bottom_corner = y >= monitor.bottom - corner;
    let in_left_corner = x < monitor.left + corner;
    let in_right_corner = x >= monitor.right - corner;

    if (near_left && in_top_corner) || (near_top && in_left_corner) {
        return Some(SnapZone::TopLeft);
    }
    if (near_right && in_top_corner) || (near_top && in_right_corner) {
        return Some(SnapZone::TopRight);
    }
    if (near_left && in_bottom_corner) || (near_bottom && in_left_corner) {
        return Some(SnapZone::BottomLeft);
    }
    if (near_right && in_bottom_corner) || (near_bottom && in_right_corner) {
        return Some(SnapZone::BottomRight);
    }
    if near_left {
        return Some(SnapZone::LeftHalf);
    }
    if near_right {
        return Some(SnapZone::RightHalf);
    }
    if near_top {
        return Some(SnapZone::Maximize);
    }
    None
}

/// Computes Master-Stack tiled rectangles inside `work` with `gap` spacing.
///
/// Window 0 takes the left master column (`master_ratio` clamped to `0.25..=0.75`);
/// remaining windows stack vertically in the right column.
pub fn compute_tile_rects(hwnds: &[isize], work: WmRect, gap: i32, master_ratio: f32) -> Vec<(isize, WmRect)> {
    let n = hwnds.len();
    if n == 0 {
        return Vec::new();
    }
    let gap = gap.max(0);
    let inner = work.inset(gap);
    if n == 1 {
        return vec![(hwnds[0], inner)];
    }

    let ratio = master_ratio.clamp(0.25, 0.75);
    let usable_w = (inner.width() - gap).max(0);
    let master_w = ((usable_w as f32) * ratio).round() as i32;
    let master_right = inner.left + master_w;
    let stack_left = master_right + gap;

    let mut out = Vec::with_capacity(n);
    out.push((hwnds[0], WmRect::new(inner.left, inner.top, master_right, inner.bottom)));

    let stack_count = (n - 1) as i32;
    let usable_h = (inner.height() - gap * (stack_count - 1)).max(0);
    let base_h = usable_h / stack_count;
    let extra = usable_h % stack_count;

    let mut y = inner.top;
    for (idx, &hwnd) in hwnds[1..].iter().enumerate() {
        let h = base_h + if (idx as i32) < extra { 1 } else { 0 };
        let bottom = if idx == n - 2 { inner.bottom } else { y + h };
        out.push((hwnd, WmRect::new(stack_left, y, inner.right, bottom)));
        y = bottom + gap;
    }
    out
}

/// Returns the slot index under screen coordinate `(x, y)` in a tiled layout.
pub fn hit_tile_slot(slots: &[(isize, WmRect)], x: i32, y: i32) -> Option<usize> {
    slots.iter().position(|(_, r)| r.contains(x, y))
}

/// Infers an updated master split ratio after a mouse resize sets the master right edge.
pub fn infer_master_ratio(work: WmRect, gap: i32, new_master_right: i32) -> f32 {
    let gap = gap.max(0);
    let inner = work.inset(gap);
    let usable_w = (inner.width() - gap).max(1);
    ((new_master_right - inner.left) as f32 / usable_w as f32).clamp(0.25, 0.75)
}

/// Floating-point DIP rectangle used by the Direct2D `Alt+Tab` / `Win+Tab` switcher.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DipRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl DipRect {
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// Interactive region hit in the Direct2D `Alt+Tab` / `Win+Tab` switcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitcherHit {
    Card(usize),
    CloseCard(usize),
    DesktopPill(usize),
    ToggleTiling,
    None,
}

/// Pure DIP geometry for the Direct2D `Alt+Tab` and `Win+Tab` Task View overlay.
#[derive(Debug, Clone)]
pub struct SwitcherLayout {
    pub width: f32,
    pub height: f32,
    pub cards: Vec<DipRect>,
    pub close_buttons: Vec<DipRect>,
    pub desktop_pills: Vec<DipRect>,
    pub tiling_button: Option<DipRect>,
}

impl SwitcherLayout {
    pub const CARD_W: f32 = 212.0;
    pub const CARD_H: f32 = 88.0;
    pub const GAP: f32 = 12.0;
    pub const PAD: f32 = 18.0;
    pub const HEADER_H: f32 = 34.0;
    pub const CLOSE_SIZE: f32 = 20.0;
    pub const PILL_H: f32 = 26.0;

    /// Computes card, close-button, and Task View header rectangles in DIPs.
    pub fn compute(window_count: usize, desktop_count: usize, task_view: bool, max_cols: usize) -> Self {
        let cols = max_cols.clamp(1, 5);
        let grid_cols = window_count.max(1).min(cols);
        let rows = if window_count == 0 {
            1
        } else {
            window_count.div_ceil(cols)
        };
        let min_width = if task_view {
            460.0
        } else {
            Self::CARD_W + 2.0 * Self::PAD
        };
        let grid_w =
            grid_cols as f32 * Self::CARD_W + (grid_cols.saturating_sub(1)) as f32 * Self::GAP + 2.0 * Self::PAD;
        let width = grid_w.max(min_width);
        let header_offset = if task_view { Self::HEADER_H + 10.0 } else { 0.0 };
        let height =
            2.0 * Self::PAD + header_offset + rows as f32 * Self::CARD_H + (rows.saturating_sub(1)) as f32 * Self::GAP;

        let mut desktop_pills = Vec::new();
        let mut tiling_button = None;
        if task_view {
            let mut x = Self::PAD;
            let pill_w = 88.0;
            let pill_gap = 8.0;
            let top = Self::PAD;
            for _ in 0..desktop_count.max(1) {
                if x + pill_w > width - Self::PAD - 124.0 {
                    break;
                }
                desktop_pills.push(DipRect::new(x, top, x + pill_w, top + Self::PILL_H));
                x += pill_w + pill_gap;
            }
            let btn_w = 112.0;
            tiling_button = Some(DipRect::new(
                width - Self::PAD - btn_w,
                top,
                width - Self::PAD,
                top + Self::PILL_H,
            ));
        }

        let start_x =
            (width - (grid_cols as f32 * Self::CARD_W + (grid_cols.saturating_sub(1)) as f32 * Self::GAP)) / 2.0;
        let start_y = Self::PAD + header_offset;

        let mut cards = Vec::with_capacity(window_count);
        let mut close_buttons = Vec::with_capacity(window_count);
        for i in 0..window_count {
            let r = i / cols;
            let c = i % cols;
            let left = start_x + c as f32 * (Self::CARD_W + Self::GAP);
            let top = start_y + r as f32 * (Self::CARD_H + Self::GAP);
            let card = DipRect::new(left, top, left + Self::CARD_W, top + Self::CARD_H);
            let close = DipRect::new(
                card.right - 6.0 - Self::CLOSE_SIZE,
                card.top + 6.0,
                card.right - 6.0,
                card.top + 6.0 + Self::CLOSE_SIZE,
            );
            cards.push(card);
            close_buttons.push(close);
        }

        Self {
            width,
            height,
            cards,
            close_buttons,
            desktop_pills,
            tiling_button,
        }
    }

    /// Hit-tests a DIP coordinate `(x, y)` against close buttons, cards, desktop pills, and tiling button.
    pub fn hit_test(&self, x: f32, y: f32) -> SwitcherHit {
        if let Some(btn) = self.tiling_button
            && btn.contains(x, y)
        {
            return SwitcherHit::ToggleTiling;
        }
        if let Some(idx) = self.desktop_pills.iter().position(|r| r.contains(x, y)) {
            return SwitcherHit::DesktopPill(idx);
        }
        if let Some(idx) = self.close_buttons.iter().position(|r| r.contains(x, y)) {
            return SwitcherHit::CloseCard(idx);
        }
        if let Some(idx) = self.cards.iter().position(|r| r.contains(x, y)) {
            return SwitcherHit::Card(idx);
        }
        SwitcherHit::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_zone_rects_respect_gaps_and_quarters() {
        let work = WmRect::new(0, 0, 1920, 1080);
        assert_eq!(SnapZone::LeftHalf.rect(work, 0), WmRect::new(0, 0, 960, 1080));
        assert_eq!(SnapZone::RightHalf.rect(work, 0), WmRect::new(960, 0, 1920, 1080));
        assert_eq!(SnapZone::TopLeft.rect(work, 0), WmRect::new(0, 0, 960, 540));
        assert_eq!(SnapZone::BottomRight.rect(work, 0), WmRect::new(960, 540, 1920, 1080));

        let left = SnapZone::LeftHalf.rect(work, 8);
        let right = SnapZone::RightHalf.rect(work, 8);
        assert_eq!(left.left, 8);
        assert_eq!(right.left - left.right, 8);
        assert_eq!(right.right, 1912);
        assert_eq!(left.top, 8);
        assert_eq!(left.bottom, 1072);
    }

    #[test]
    fn keyboard_snap_transitions_handle_quadrants_and_restore() {
        assert_eq!(
            next_keyboard_snap(None, false, SnapDirection::Left),
            SnapAction::Snap(SnapZone::LeftHalf)
        );
        assert_eq!(
            next_keyboard_snap(Some(SnapZone::LeftHalf), false, SnapDirection::Up),
            SnapAction::Snap(SnapZone::TopLeft)
        );
        assert_eq!(
            next_keyboard_snap(Some(SnapZone::LeftHalf), false, SnapDirection::Right),
            SnapAction::Restore
        );
        assert_eq!(next_keyboard_snap(None, true, SnapDirection::Down), SnapAction::Restore);
        assert_eq!(
            next_keyboard_snap(None, false, SnapDirection::Down),
            SnapAction::Minimize
        );
        assert_eq!(
            next_keyboard_snap(Some(SnapZone::TopRight), false, SnapDirection::Left),
            SnapAction::Snap(SnapZone::TopHalf)
        );
    }

    #[test]
    fn mouse_snap_detects_edges_and_corners_without_capturing_bottom_center() {
        let mon = WmRect::new(0, 0, 1920, 1080);
        assert_eq!(detect_mouse_snap_zone(5, 500, mon, 16, 96), Some(SnapZone::LeftHalf));
        assert_eq!(
            detect_mouse_snap_zone(1915, 500, mon, 16, 96),
            Some(SnapZone::RightHalf)
        );
        assert_eq!(detect_mouse_snap_zone(960, 5, mon, 16, 96), Some(SnapZone::Maximize));
        assert_eq!(detect_mouse_snap_zone(5, 5, mon, 16, 96), Some(SnapZone::TopLeft));
        assert_eq!(
            detect_mouse_snap_zone(1910, 1070, mon, 16, 96),
            Some(SnapZone::BottomRight)
        );
        // Bottom center stays free for the auto-hide dock.
        assert_eq!(detect_mouse_snap_zone(960, 1075, mon, 16, 96), None);
        assert_eq!(detect_mouse_snap_zone(500, 500, mon, 16, 96), None);
    }

    #[test]
    fn master_stack_tiling_and_mouse_hit_testing() {
        let work = WmRect::new(0, 0, 1000, 800);
        assert!(compute_tile_rects(&[], work, 8, 0.5).is_empty());

        let single = compute_tile_rects(&[101], work, 8, 0.5);
        assert_eq!(single, vec![(101, WmRect::new(8, 8, 992, 792))]);

        let three = compute_tile_rects(&[1, 2, 3], work, 10, 0.6);
        assert_eq!(three.len(), 3);
        let (_, m) = three[0];
        let (_, s1) = three[1];
        let (_, s2) = three[2];
        assert_eq!(m.left, 10);
        assert_eq!(s1.left - m.right, 10);
        assert_eq!(s2.top - s1.bottom, 10);
        assert_eq!(s2.bottom, 790);

        assert_eq!(hit_tile_slot(&three, m.left + 20, m.top + 20), Some(0));
        assert_eq!(hit_tile_slot(&three, s1.left + 20, s1.top + 20), Some(1));
        assert_eq!(hit_tile_slot(&three, s2.left + 20, s2.top + 20), Some(2));

        let ratio = infer_master_ratio(work, 10, m.right);
        assert!((ratio - 0.6).abs() < 0.01);
        assert_eq!(infer_master_ratio(work, 10, 50), 0.25);
        assert_eq!(infer_master_ratio(work, 10, 990), 0.75);
    }

    #[test]
    fn switcher_layout_supports_mouse_close_card_and_task_view_controls() {
        let layout = SwitcherLayout::compute(4, 3, true, 4);
        assert_eq!(layout.cards.len(), 4);
        assert_eq!(layout.desktop_pills.len(), 3);
        assert!(layout.tiling_button.is_some());

        let btn = layout.tiling_button.unwrap();
        assert_eq!(
            layout.hit_test(btn.left + 4.0, btn.top + 4.0),
            SwitcherHit::ToggleTiling
        );
        let pill = layout.desktop_pills[1];
        assert_eq!(
            layout.hit_test(pill.left + 4.0, pill.top + 4.0),
            SwitcherHit::DesktopPill(1)
        );
        let close = layout.close_buttons[2];
        assert_eq!(
            layout.hit_test(close.left + 2.0, close.top + 2.0),
            SwitcherHit::CloseCard(2)
        );
        let card = layout.cards[2];
        assert_eq!(layout.hit_test(card.left + 8.0, card.top + 20.0), SwitcherHit::Card(2));
        assert_eq!(layout.hit_test(1.0, 1.0), SwitcherHit::None);
    }
}
