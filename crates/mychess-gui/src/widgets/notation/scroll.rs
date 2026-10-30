//! The vertical scroll of the notation panel: offset, bar and reveal
//! (spec M2-4, D170 and rule 21). Pure arithmetic, no toolkit types.

/// Width of the scroll bar in logical pixels.
pub const BAR_WIDTH: f32 = 8.0;

/// Smallest height of the scroll bar thumb.
const MIN_THUMB: f32 = 16.0;

/// The scroll position of a column of content in a viewport.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Scroll {
    offset: f32,
    content: f32,
    viewport: f32,
}

impl Scroll {
    /// The distance from the top of the content to the top of the viewport.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// The largest offset: the content does not scroll past its end.
    pub fn max_offset(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    /// Sets the heights of the content and the viewport and keeps the
    /// offset in range.
    pub fn set_extent(&mut self, content: f32, viewport: f32) {
        self.content = if content.is_finite() {
            content.max(0.0)
        } else {
            0.0
        };
        self.viewport = if viewport.is_finite() {
            viewport.max(0.0)
        } else {
            0.0
        };
        self.offset = self.offset.clamp(0.0, self.max_offset());
    }

    /// Scrolls to `offset` (clamped).
    pub fn set_offset(&mut self, offset: f32) {
        let offset = if offset.is_finite() { offset } else { 0.0 };
        self.offset = offset.clamp(0.0, self.max_offset());
    }

    /// Scrolls by `delta` (positive moves the content up). Returns whether
    /// the offset changed.
    pub fn scroll_by(&mut self, delta: f32) -> bool {
        let before = self.offset;
        self.set_offset(self.offset + delta);
        (self.offset - before).abs() > f32::EPSILON
    }

    /// Makes the span `top..bottom` of the content visible with `margin`
    /// around it. When it does not fit, its top is aligned (rule 21).
    pub fn reveal(&mut self, top: f32, bottom: f32, margin: f32) {
        if !(top.is_finite() && bottom.is_finite()) {
            return;
        }
        let first = top - margin;
        let last = bottom + margin;
        let target = if last - first > self.viewport || first < self.offset {
            first
        } else if last > self.offset + self.viewport {
            last - self.viewport
        } else {
            return;
        };
        self.set_offset(target);
    }

    /// The thumb of the bar as `(top, height)` inside a track of
    /// `track` pixels, `None` when everything fits.
    pub fn thumb(&self, track: f32) -> Option<(f32, f32)> {
        let max = self.max_offset();
        if max <= 0.0 || track <= 0.0 || self.content <= 0.0 {
            return None;
        }
        let height = (track * self.viewport / self.content).clamp(MIN_THUMB.min(track), track);
        let top = (track - height) * (self.offset / max);
        Some((top, height))
    }

    /// The offset that puts the thumb with its top at `thumb_top` (the
    /// inverse of [`thumb`](Self::thumb)).
    pub fn offset_for_thumb(&self, track: f32, thumb_top: f32) -> f32 {
        let Some((_, height)) = self.thumb(track) else {
            return 0.0;
        };
        let room = track - height;
        if room <= 0.0 {
            return 0.0;
        }
        (thumb_top / room).clamp(0.0, 1.0) * self.max_offset()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]
    use super::*;

    fn scroll(content: f32, viewport: f32) -> Scroll {
        let mut s = Scroll::default();
        s.set_extent(content, viewport);
        s
    }

    #[test]
    fn the_offset_is_kept_inside_the_content() {
        let mut s = scroll(1000.0, 200.0);
        assert!(s.scroll_by(5000.0));
        assert_eq!(s.offset(), 800.0);
        assert!(s.scroll_by(-5000.0));
        assert_eq!(s.offset(), 0.0);
        assert!(!s.scroll_by(-1.0), "already at the top");
        s.set_offset(500.0);
        s.set_extent(300.0, 200.0);
        assert_eq!(s.offset(), 100.0, "shrinking the content pulls it back");
        s.set_extent(100.0, 200.0);
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn garbage_numbers_do_not_poison_the_state() {
        let mut s = scroll(f32::NAN, f32::INFINITY);
        assert_eq!(s.max_offset(), 0.0);
        s.set_offset(f32::NAN);
        assert_eq!(s.offset(), 0.0);
        s.reveal(f32::NAN, 1.0, 1.0);
        assert_eq!(s.offset(), 0.0);
    }

    #[test]
    fn reveal_scrolls_the_least_that_shows_the_item_with_a_margin() {
        let mut s = scroll(1000.0, 200.0);
        // Already visible with margin: nothing moves.
        s.reveal(50.0, 70.0, 20.0);
        assert_eq!(s.offset(), 0.0);
        // Below the viewport: the bottom (plus margin) meets the edge.
        s.reveal(300.0, 320.0, 20.0);
        assert_eq!(s.offset(), 140.0);
        // Above it: the top (minus margin) meets the edge.
        s.reveal(100.0, 120.0, 20.0);
        assert_eq!(s.offset(), 80.0);
        // Touching the margin zone counts as outside.
        s.reveal(90.0, 110.0, 20.0);
        assert_eq!(s.offset(), 70.0);
    }

    #[test]
    fn an_item_taller_than_the_viewport_aligns_its_top() {
        let mut s = scroll(1000.0, 100.0);
        s.reveal(400.0, 600.0, 10.0);
        assert_eq!(s.offset(), 390.0);
    }

    #[test]
    fn thumb_and_offset_are_inverse() {
        let mut s = scroll(1000.0, 250.0);
        assert_eq!(s.thumb(250.0), Some((0.0, 62.5)));
        s.set_offset(750.0);
        let (top, height) = s.thumb(250.0).expect("thumb");
        assert!((top + height - 250.0).abs() < 1e-3, "thumb at the bottom");
        let back = s.offset_for_thumb(250.0, top);
        assert!((back - 750.0).abs() < 1e-2);
        assert_eq!(s.offset_for_thumb(250.0, -50.0), 0.0);
        assert_eq!(s.offset_for_thumb(250.0, 9999.0), 750.0);
    }

    #[test]
    fn there_is_no_thumb_when_everything_fits() {
        let s = scroll(100.0, 200.0);
        assert_eq!(s.thumb(200.0), None);
        assert_eq!(s.offset_for_thumb(200.0, 10.0), 0.0);
    }

    #[test]
    fn the_thumb_has_a_minimum_height() {
        let s = scroll(1_000_000.0, 100.0);
        let (_, height) = s.thumb(100.0).expect("thumb");
        assert_eq!(height, MIN_THUMB);
    }
}
