//! Colors of the score and time graph (spec M4-7, D874): the bars, the
//! running-total lines and the grid, per palette.

use iced::Color;

use mychess_app::graphs::MarkerKind;

use super::contrast::relative_luminance;
use super::report_colors::{class_color, marker_class};
use super::tokens::{Tokens, mix};

/// The colors the graph canvas draws with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphColors {
    /// Plot background.
    pub background: Color,
    /// Light grid lines.
    pub minor: Color,
    /// Highlighted grid lines.
    pub major: Color,
    /// The zero line over the bars.
    pub zero: Color,
    /// Axis labels.
    pub label: Color,
    /// The score or time bars.
    pub bar: Color,
    /// Running total of White's clock.
    pub white_line: Color,
    /// Running total of Black's clock.
    pub black_line: Color,
    /// The current ply (marker and hover).
    pub current: Color,
    /// The markers of the report, in the order of [`MARKER_KINDS`].
    markers: [Color; 6],
}

/// The kinds of marker, in the order of the colors of [`GraphColors`].
const MARKER_KINDS: [MarkerKind; 6] = [
    MarkerKind::Brilliant,
    MarkerKind::Great,
    MarkerKind::Inaccuracy,
    MarkerKind::Mistake,
    MarkerKind::Miss,
    MarkerKind::Blunder,
];

impl GraphColors {
    /// The colors that go with the palette of `t` (light or dark, decided by
    /// the luminance of the panel, so any theme maps to one of the two).
    pub fn of(t: &Tokens) -> GraphColors {
        let dark = relative_luminance(t.panel) < 0.35;
        GraphColors {
            background: t.panel,
            minor: mix(t.text, t.panel, 0.10),
            // `steelblue` of the Scid grid.
            major: Color::from_rgb8(0x46, 0x82, 0xb4).scale_alpha(0.55),
            zero: t.text,
            label: t.muted,
            // A blue-grey like the Scid's score color.
            bar: if dark {
                Color::from_rgb8(0x7f, 0x9c, 0xc0)
            } else {
                Color::from_rgb8(0x5b, 0x78, 0x9e)
            },
            white_line: if dark {
                Color::from_rgb8(0xd4, 0xd4, 0xd4)
            } else {
                Color::from_rgb8(0x8c, 0x8c, 0x8c)
            },
            black_line: if dark {
                Color::from_rgb8(0x7a, 0x7a, 0x7a)
            } else {
                Color::from_rgb8(0x1a, 0x1a, 0x1a)
            },
            current: t.accent,
            markers: MARKER_KINDS.map(|kind| class_color(t, marker_class(kind))),
        }
    }

    /// The color of a marker of the report (D1432).
    pub fn marker(&self, kind: MarkerKind) -> Color {
        MARKER_KINDS
            .iter()
            .position(|k| *k == kind)
            .and_then(|i| self.markers.get(i))
            .copied()
            .unwrap_or(self.current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_follows_the_luminance_of_the_panel() {
        let dark = GraphColors::of(&Tokens::dark());
        let light = GraphColors::of(&Tokens::light());
        assert_ne!(dark.bar, light.bar);
        // A built-in iced dark theme is dark too, though its tokens are not
        // `Tokens::dark()`.
        let other = GraphColors::of(&Tokens::of(&iced::Theme::Nord));
        assert_eq!(other.bar, dark.bar);
    }
}
