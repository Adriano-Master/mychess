//! Colors of the tree masks (spec M5-4, D1808, D1827, D1829): the swatch of
//! a move, the highlight of the moves that are in the mask and the strip of
//! the comment of the position. Every color derives from the [`Tokens`] of
//! the theme, so contrast holds in the light and in the dark palette.

use iced::widget::container;
use iced::{Border, Color, Theme};
use mychess_app::MaskColor;

use super::contrast::contrast_ratio;
use super::tokens::{Tokens, mix};

/// Share of the accent in the strip of the comment of the position.
const STRIP_TINT: f32 = 0.18;
/// The contrast the highlighted move keeps against the panel (WCAG AA).
const MIN_CONTRAST: f32 = 4.5;

/// The colors the mask marks are drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskColors {
    /// The text of a move that is in the mask (the `SteelBlue` of the Scid).
    pub highlight: Color,
    green: Color,
    yellow: Color,
    blue: Color,
    red: Color,
    /// The outline of a swatch.
    pub outline: Color,
}

impl MaskColors {
    /// The colors that go with the palette of `t`.
    pub fn of(t: &Tokens) -> MaskColors {
        // The accent itself, or the closest mix toward the text color that
        // still reads on the panel.
        let highlight = (0..=10_u8)
            .map(|step| mix(t.accent, t.text, 1.0 - f32::from(step) / 10.0))
            .find(|candidate| contrast_ratio(*candidate, t.panel) >= MIN_CONTRAST)
            .unwrap_or(t.text);
        MaskColors {
            highlight,
            green: Color::from_rgb8(0x2e, 0xa0, 0x43),
            yellow: Color::from_rgb8(0xe0, 0xb0, 0x2a),
            blue: Color::from_rgb8(0x2f, 0x7f, 0xd0),
            red: Color::from_rgb8(0xd0, 0x3a, 0x4a),
            outline: t.border,
        }
    }

    /// The color of the swatch of `color`; `None` when nothing is drawn (the
    /// default, and the Tk names that are not `#rrggbb`).
    pub fn swatch(&self, color: &MaskColor) -> Option<Color> {
        match color {
            MaskColor::Green => Some(self.green),
            MaskColor::Yellow => Some(self.yellow),
            MaskColor::Blue => Some(self.blue),
            MaskColor::Red => Some(self.red),
            MaskColor::Other(text) => parse_hex(text),
            MaskColor::White => None,
        }
    }
}

/// `#rgb`, `#rrggbb`, `#rrrgggbbb` and `#rrrrggggbbbb` as Tk reads them: the
/// most significant digits of each channel count.
pub fn parse_hex(text: &str) -> Option<Color> {
    let digits = text.trim().strip_prefix('#')?;
    if !digits.is_ascii() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let per = match digits.len() {
        3 => 1,
        6 => 2,
        9 => 3,
        12 => 4,
        _ => return None,
    };
    let channel = |index: usize| -> Option<u8> {
        let part = digits.get(index * per..(index + 1) * per)?;
        // Scale a value of `per` digits to eight bits, as Tk does.
        let value = u32::from_str_radix(part, 16).ok()?;
        let max = (1u32 << (4 * per)) - 1;
        u8::try_from(value * 255 / max).ok()
    };
    Some(Color::from_rgb8(channel(0)?, channel(1)?, channel(2)?))
}

/// The strip above the table with the comment of the position (D1829).
pub fn strip(theme: &Theme) -> container::Style {
    let t = Tokens::of(theme);
    container::Style {
        text_color: Some(t.text),
        background: Some(mix(t.accent, t.panel, STRIP_TINT).into()),
        border: Border {
            radius: t.radius.chip.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The square of a swatch: the color with a thin outline.
pub fn swatch_box(theme: &Theme, color: Color) -> container::Style {
    let colors = MaskColors::of(&Tokens::of(theme));
    container::Style {
        background: Some(color.into()),
        border: Border {
            color: colors.outline,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highlight_reads_on_the_panel_in_both_palettes() {
        for tokens in [Tokens::dark(), Tokens::light()] {
            let colors = MaskColors::of(&tokens);
            assert!(
                contrast_ratio(colors.highlight, tokens.panel) >= MIN_CONTRAST,
                "{:?}",
                colors.highlight
            );
        }
    }

    #[test]
    fn hex_colors_scale_every_width_to_eight_bits() {
        assert_eq!(parse_hex("#fff"), Some(Color::from_rgb8(255, 255, 255)));
        assert_eq!(parse_hex("#102030"), Some(Color::from_rgb8(16, 32, 48)));
        assert_eq!(
            parse_hex("#ffffffffffff"),
            Some(Color::from_rgb8(255, 255, 255))
        );
        assert_eq!(parse_hex("#12"), None);
        assert_eq!(parse_hex("red"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        assert_eq!(parse_hex("#é1é1é1"), None);
    }

    #[test]
    fn the_standard_colors_have_a_swatch_and_the_default_has_none() {
        let colors = MaskColors::of(&Tokens::light());
        assert!(colors.swatch(&MaskColor::White).is_none());
        for color in [
            MaskColor::Green,
            MaskColor::Yellow,
            MaskColor::Blue,
            MaskColor::Red,
        ] {
            assert!(colors.swatch(&color).is_some());
        }
        assert!(
            colors
                .swatch(&MaskColor::Other("skyblue".to_owned()))
                .is_none()
        );
        assert!(
            colors
                .swatch(&MaskColor::Other("#336699".to_owned()))
                .is_some()
        );
    }
}
