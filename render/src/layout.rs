//! Device-independent layout geometry and metrics for launcher and panels.

pub const WIDTH: f32 = 680.0;
pub const INPUT_H: f32 = 58.0;
pub const ROW_H: f32 = 52.0;
pub const PAD: f32 = 8.0;
pub const LIST_PAD: f32 = 6.0;
pub const LIST_TOP: f32 = INPUT_H + 1.0 + LIST_PAD;

pub const ICON: f32 = 32.0;
pub const ICON_X: f32 = PAD + 12.0;
pub const TEXT_X: f32 = ICON_X + ICON + 14.0;
pub const HINT_W: f32 = 96.0;
pub const INPUT_TEXT_X: f32 = 52.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

pub fn height(rows: usize) -> f32 {
    if rows == 0 {
        INPUT_H
    } else {
        LIST_TOP + rows as f32 * ROW_H + LIST_PAD
    }
}

/// Where the query box starts in a window `window_h` DIPs tall. At the
/// bottom of the screen the box is the window's last row.
pub fn input_top(window_h: f32, bottom: bool) -> f32 {
    if bottom { window_h - INPUT_H } else { 0.0 }
}

/// The top of visible row `row`. At the bottom of the screen, results
/// stack upward so the best match sits next to the query box.
pub fn row_top(row: usize, window_h: f32, bottom: bool) -> f32 {
    if bottom {
        window_h - LIST_TOP - (row + 1) as f32 * ROW_H
    } else {
        LIST_TOP + row as f32 * ROW_H
    }
}

/// The visible row at window DIP `y`, if any of `rows` rows is there.
pub fn row_at(y: f32, rows: usize, window_h: f32, bottom: bool) -> Option<usize> {
    let offset = if bottom { window_h - LIST_TOP - y } else { y - LIST_TOP };
    if offset < 0.0 {
        return None;
    }
    let row = (offset / ROW_H) as usize;
    (row < rows).then_some(row)
}

/// Where a surface's widgets start. At the bottom of the screen the
/// surface fills the window above the query box.
pub fn surface_top(bottom: bool) -> f32 {
    if bottom { LIST_PAD } else { LIST_TOP }
}

pub fn surface_origin(bottom: bool) -> (f32, f32) {
    (PAD, surface_top(bottom))
}

pub fn surface_width() -> f32 {
    WIDTH - 2.0 * PAD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_rows_stack_upward_from_the_query_box() {
        let h = height(3);
        assert_eq!(input_top(h, true), h - INPUT_H);
        assert_eq!(row_top(0, h, true) + ROW_H, h - LIST_TOP);
        assert_eq!(row_top(2, h, true), LIST_PAD);
        assert_eq!(row_top(1, h, false), LIST_TOP + ROW_H);
    }

    #[test]
    fn row_hit_testing_matches_drawing() {
        for bottom in [false, true] {
            let h = height(4);
            for row in 0..4 {
                let mid = row_top(row, h, bottom) + ROW_H / 2.0;
                assert_eq!(row_at(mid, 4, h, bottom), Some(row));
            }
            assert_eq!(row_at(input_top(h, bottom) + 1.0, 4, h, bottom), None);
            assert_eq!(row_at(row_top(3, h, bottom) + 1.0, 2, h, bottom), None);
        }
    }
}
