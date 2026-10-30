//! Colors of the tricolour bar of the opening tree (spec M5-2, D969): the
//! White part, the Black part and the grey between them, per palette.

use iced::Color;

use super::contrast::relative_luminance;
use super::tokens::{Tokens, mix};

/// The colors the bar of a row is drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeBarColors {
    /// The part for White's wins.
    pub white: Color,
    /// The part for Black's wins.
    pub black: Color,
    /// The draws, between the two.
    pub grey: Color,
    /// A thin outline so the white part stays visible on a light panel.
    pub outline: Color,
}

impl TreeBarColors {
    /// The colors that go with the palette of `t` (light or dark, decided by
    /// the luminance of the panel, like the graph).
    pub fn of(t: &Tokens) -> TreeBarColors {
        let dark = relative_luminance(t.panel) < 0.35;
        TreeBarColors {
            white: if dark {
                Color::from_rgb8(0xee, 0xee, 0xee)
            } else {
                Color::from_rgb8(0xff, 0xff, 0xff)
            },
            black: if dark {
                Color::from_rgb8(0x0a, 0x0a, 0x0a)
            } else {
                Color::from_rgb8(0x1a, 0x1a, 0x1a)
            },
            grey: mix(t.muted, t.panel, 0.55),
            outline: t.border,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_is_lighter_than_black_in_both_palettes() {
        for tokens in [Tokens::dark(), Tokens::light()] {
            let c = TreeBarColors::of(&tokens);
            assert!(relative_luminance(c.white) > relative_luminance(c.grey));
            assert!(relative_luminance(c.grey) > relative_luminance(c.black));
        }
    }
}
