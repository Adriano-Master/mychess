//! WCAG 2.x contrast (spec M2-5, D221 and section 6).

use iced::Color;

fn linear(channel: f32) -> f32 {
    if channel <= 0.039_28 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// Relative luminance of an sRGB color (alpha is ignored).
pub fn relative_luminance(color: Color) -> f32 {
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// The contrast ratio of two colors, from 1 (equal) to 21 (black on white).
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (light, dark) = if la >= lb { (la, lb) } else { (lb, la) };
    (light + 0.05) / (dark + 0.05)
}

/// Minimum ratio for text (WCAG AA).
pub const MIN_TEXT: f32 = 4.5;
/// Minimum ratio for graphic elements such as icons (WCAG 1.4.11).
pub const MIN_GRAPHIC: f32 = 3.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ratios() {
        assert!((contrast_ratio(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(Color::WHITE, Color::BLACK) - 21.0).abs() < 0.01);
        let grey = Color::from_rgb8(0x77, 0x77, 0x77);
        assert!((contrast_ratio(grey, grey) - 1.0).abs() < 1e-6);
        // #767676 on white is the classic 4.54:1 boundary.
        let edge = contrast_ratio(Color::from_rgb8(0x76, 0x76, 0x76), Color::WHITE);
        assert!((edge - 4.54).abs() < 0.02, "{edge}");
    }
}
