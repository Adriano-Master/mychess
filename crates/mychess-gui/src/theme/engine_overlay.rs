//! Colors of the engine overlays of the board: the PV arrows, the
//! evaluation bar and the traffic lights (spec M4-5, D850).
//!
//! They are semantic (white and black of the bar, the highlight of the
//! engine) and do not follow the light or dark theme of the window.

use iced::Color;

/// The arrow of the best line (Scid `engineLineColor`, `start.tcl:453`,
/// a saturated blue).
pub const BEST_ARROW: Color = Color::from_rgb(0.12, 0.45, 0.85);
/// The arrows of the other lines (grey).
pub const OTHER_ARROW: Color = Color::from_rgb(0.5, 0.5, 0.5);
/// A sequence arrow of White (Scid `whiteLineColor`, `white`).
pub const SEQUENCE_WHITE: Color = Color::from_rgb(0.95, 0.95, 0.95);
/// A sequence arrow of Black (Scid `blackLineColor`, `grey50`).
pub const SEQUENCE_BLACK: Color = Color::from_rgb(0.5, 0.5, 0.5);
/// Alpha of the engine arrows (the pieces stay visible).
pub const ARROW_ALPHA: f32 = 0.8;

/// The thin edge drawn around every arrow, so that it reads on light and
/// on medium squares alike (D858).
pub const ARROW_OUTLINE: Color = Color::from_rgb(0.03, 0.03, 0.03);
/// The light ring just inside [`ARROW_OUTLINE`]: together they make the
/// arrow stand out on light and on dark squares alike.
pub const ARROW_LIGHT_EDGE: Color = Color::from_rgb(0.97, 0.97, 0.97);
/// Alpha of [`ARROW_OUTLINE`] and [`ARROW_LIGHT_EDGE`].
pub const OUTLINE_ALPHA: f32 = 0.9;

/// Width of the arrow edge for squares of `s` logical pixels: proportional
/// to the scale, never thinner than one pixel.
pub fn outline_width(s: f32) -> f32 {
    (0.012 * s).max(1.0)
}

/// WCAG relative luminance of an opaque color.
pub fn luminance(c: Color) -> f32 {
    let lin = |v: f32| {
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio (1 to 21) between two opaque colors.
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `top` with `alpha` over the opaque `under`.
pub fn over(top: Color, alpha: f32, under: Color) -> Color {
    let mix = |t: f32, u: f32| alpha * t + (1.0 - alpha) * u;
    Color::from_rgb(
        mix(top.r, under.r),
        mix(top.g, under.g),
        mix(top.b, under.b),
    )
}

/// The white part of the evaluation bar.
pub const BAR_WHITE: Color = Color::from_rgb(0.94, 0.94, 0.92);
/// The black part of the evaluation bar.
pub const BAR_BLACK: Color = Color::from_rgb(0.25, 0.24, 0.22);
/// The outline and the ticks of the bar.
pub const BAR_OUTLINE: Color = Color::from_rgb(0.45, 0.45, 0.45);
/// The label over the white part.
pub const BAR_TEXT_ON_WHITE: Color = Color::from_rgb(0.1, 0.1, 0.1);
/// The label over the black part.
pub const BAR_TEXT_ON_BLACK: Color = Color::from_rgb(0.95, 0.95, 0.95);
/// Opacity of a bar that shows the previous position's value (D452).
pub const STALE_ALPHA: f32 = 0.45;

/// A traffic light color from its 8-bit channels.
pub fn light(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::board_theme::{BoardTheme, BoardThemeId};

    fn fills() -> [Color; 4] {
        [BEST_ARROW, OTHER_ARROW, SEQUENCE_WHITE, SEQUENCE_BLACK]
    }

    #[test]
    fn the_arrow_edge_has_3_to_1_contrast_on_every_square_of_every_theme() {
        for id in BoardThemeId::ALL {
            let t = BoardTheme::of(id);
            for square in [t.light, t.dark] {
                // The edge is the light ring (outer) and the dark one:
                // whichever stands out from this square.
                let dark = over(ARROW_OUTLINE, OUTLINE_ALPHA, square);
                let light = over(ARROW_LIGHT_EDGE, OUTLINE_ALPHA, square);
                let ratio = contrast_ratio(dark, square).max(contrast_ratio(light, square));
                assert!(ratio >= 3.0, "{id:?}: edge {ratio} on {square:?}");
            }
        }
    }

    #[test]
    fn every_arrow_fill_has_3_to_1_contrast_against_its_edge() {
        for id in BoardThemeId::ALL {
            let t = BoardTheme::of(id);
            for square in [t.light, t.dark] {
                for fill in fills() {
                    let body = over(fill, ARROW_ALPHA, square);
                    let edge = over(ARROW_OUTLINE, OUTLINE_ALPHA, body);
                    let ratio = contrast_ratio(body, edge);
                    assert!(ratio >= 3.0, "{id:?}: {fill:?} {ratio} on {square:?}");
                }
            }
        }
    }

    #[test]
    fn the_edge_scales_with_the_squares_and_is_at_least_one_pixel() {
        assert!((outline_width(10.0) - 1.0).abs() < f32::EPSILON);
        assert!((outline_width(100.0) - 1.2).abs() < 1e-4);
        assert!(outline_width(200.0) > outline_width(100.0));
    }

    #[test]
    fn contrast_ratio_matches_the_wcag_extremes() {
        let r = contrast_ratio(Color::BLACK, Color::WHITE);
        assert!((r - 21.0).abs() < 0.01, "{r}");
        assert!((contrast_ratio(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
    }
}
