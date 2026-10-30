//! Colors of the charts of the filter and of the rating graph (spec M6-4d,
//! D3430): the grid, the axes and the lines per palette. The line colors are
//! those of the Scid (`steelblue`, `rosybrown` and the nine of
//! `RATING_PALETTE`); only the neutral parts follow the theme.

use iced::Color;

use super::contrast::relative_luminance;
use super::tokens::{Tokens, mix};

/// The colors a chart canvas draws with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartColors {
    /// Plot background.
    pub background: Color,
    /// Light grid lines.
    pub minor: Color,
    /// Highlighted (blue) grid lines.
    pub major: Color,
    /// The axes (left and bottom edges of the plot).
    pub axis: Color,
    /// Axis labels and the hover text.
    pub label: Color,
    /// The line of the graph of the filter (`steelblue`).
    pub line: Color,
    /// The "mean" line (`rosybrown`).
    pub mean: Color,
    /// The marker under the pointer.
    pub hover: Color,
}

impl ChartColors {
    /// The colors that go with the palette of `t` (light or dark, decided by
    /// the luminance of the panel, so any theme maps to one of the two).
    pub fn of(t: &Tokens) -> ChartColors {
        let dark = relative_luminance(t.panel) < 0.35;
        ChartColors {
            background: t.panel,
            minor: mix(t.text, t.panel, 0.10),
            major: Color::from_rgb8(0x46, 0x82, 0xb4).scale_alpha(if dark { 0.7 } else { 0.5 }),
            axis: mix(t.text, t.panel, 0.45),
            label: t.muted,
            line: if dark {
                Color::from_rgb8(0x6c, 0xa6, 0xd6)
            } else {
                Color::from_rgb8(0x46, 0x82, 0xb4)
            },
            mean: if dark {
                Color::from_rgb8(0xcf, 0xa5, 0xa5)
            } else {
                Color::from_rgb8(0xbc, 0x8f, 0x8f)
            },
            hover: t.accent,
        }
    }
}

/// The color of a series of the rating graph. The palette is the Scid's; on
/// a dark panel it is lightened a little so every line keeps its contrast.
pub fn series_color(t: &Tokens, rgb: [u8; 3]) -> Color {
    let base = Color::from_rgb8(rgb[0], rgb[1], rgb[2]);
    if relative_luminance(t.panel) < 0.35 {
        mix(Color::WHITE, base, 0.2)
    } else {
        base
    }
}

/// `#rrggbb` as a color; `None` for anything else.
pub fn parse_hex(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    Some(Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?))
}

/// The color of a square of the heat map of the piece tracker: the RGB of
/// the scheme of the Scid, with `alpha` (below 1 while the map is stale).
pub fn heat_fill(rgb: [u8; 3], alpha: f32) -> Color {
    Color::from_rgb8(rgb[0], rgb[1], rgb[2]).scale_alpha(alpha)
}

/// The edge of a square of the heat map.
pub fn heat_edge(t: &Tokens) -> Color {
    t.text.scale_alpha(0.25)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_follows_the_luminance_of_the_panel() {
        let dark = ChartColors::of(&Tokens::dark());
        let light = ChartColors::of(&Tokens::light());
        assert_ne!(dark.line, light.line);
        assert_ne!(dark.minor, light.minor);
        let other = ChartColors::of(&Tokens::of(&iced::Theme::Nord));
        assert_eq!(other.line, dark.line);
    }

    #[test]
    fn a_series_keeps_its_hue_and_is_lighter_on_dark() {
        let rgb = [0x46, 0x82, 0xb4];
        let light = series_color(&Tokens::light(), rgb);
        let dark = series_color(&Tokens::dark(), rgb);
        assert!((light.b - 180.0 / 255.0).abs() < 0.01);
        assert!(dark.r > light.r && dark.b >= light.b);
    }
}
