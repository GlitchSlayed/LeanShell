//! The curves Wisp's animations follow. Each takes progress from 0 to 1 and
//! stays within 0 and 1: nothing overshoots.

/// Fast at first, then settling: the launcher's morph.
pub fn out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Gentle at both ends: the dock's slide and the lift of a hovered icon.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_at_zero_end_at_one_and_never_overshoot() {
        for curve in [out_cubic, smoothstep] {
            assert_eq!(curve(0.0), 0.0);
            assert_eq!(curve(1.0), 1.0);
            assert_eq!(curve(-0.5), 0.0);
            assert_eq!(curve(1.5), 1.0);
            let mut last = 0.0;
            for i in 0..=100 {
                let value = curve(i as f32 / 100.0);
                assert!((0.0..=1.0).contains(&value));
                assert!(value >= last);
                last = value;
            }
        }
    }

    #[test]
    fn curves_keep_their_shapes() {
        // The morph covers most of its way early; the slide is even about its middle.
        assert_eq!(out_cubic(0.5), 0.875);
        assert_eq!(smoothstep(0.5), 0.5);
        assert_eq!(smoothstep(0.25), 0.15625);
    }
}
