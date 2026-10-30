//! The accent colors of the interface (spec M9-1, D1600 to D1607): nine
//! predefined colors, each with a dark-theme and a light-theme variant.
//!
//! The accent travels inside the `iced::Theme` (as the `primary` of its
//! palette, D1603); [`Tokens::of`](super::tokens::Tokens::of) reads it back.

use iced::Color;

use super::contrast::contrast_ratio;

const fn rgb(hex: u32) -> Color {
    // Truncation is intended: each channel is one byte of `hex`.
    #[allow(clippy::cast_possible_truncation)]
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// One of the predefined accent colors (mirrors `mychess_app::AccentPref`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AccentId {
    /// The MyChess green (default).
    #[default]
    Mychess,
    /// Blue.
    Blue,
    /// Purple.
    Purple,
    /// Pink.
    Pink,
    /// Red.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Gray.
    Gray,
}

impl AccentId {
    /// Every accent, in menu order.
    pub const ALL: [AccentId; 9] = [
        AccentId::Mychess,
        AccentId::Blue,
        AccentId::Purple,
        AccentId::Pink,
        AccentId::Red,
        AccentId::Orange,
        AccentId::Yellow,
        AccentId::Green,
        AccentId::Gray,
    ];

    /// Fluent key of the name.
    pub fn name_key(self) -> &'static str {
        match self {
            AccentId::Mychess => "accent-mychess",
            AccentId::Blue => "accent-blue",
            AccentId::Purple => "accent-purple",
            AccentId::Pink => "accent-pink",
            AccentId::Red => "accent-red",
            AccentId::Orange => "accent-orange",
            AccentId::Yellow => "accent-yellow",
            AccentId::Green => "accent-green",
            AccentId::Gray => "accent-gray",
        }
    }

    /// The accent color for a dark or light theme.
    pub fn color(self, dark: bool) -> Color {
        let (d, l) = match self {
            AccentId::Mychess => (0x007c_c04a, 0x002f_6b17),
            AccentId::Blue => (0x004a_9eef, 0x001c_6fd1),
            AccentId::Purple => (0x00b6_83de, 0x007a_3fb0),
            AccentId::Pink => (0x00e8_7fb0, 0x00c2_3d7e),
            AccentId::Red => (0x00f0_727a, 0x00c9_303a),
            AccentId::Orange => (0x00f5_9a3d, 0x00b4_5a0b),
            AccentId::Yellow => (0x00e8_c53a, 0x0087_6c00),
            AccentId::Green => (0x005c_c47f, 0x0026_7f45),
            AccentId::Gray => (0x00a0_a7b0, 0x005d_6670),
        };
        rgb(if dark { d } else { l })
    }

    /// Text and icons over [`color`](Self::color). The default keeps the
    /// hand-tuned values of the original palette (D1600); the others are
    /// computed (D1607).
    pub fn on_color(self, dark: bool) -> Color {
        match (self, dark) {
            (AccentId::Mychess, true) => rgb(0x0012_200a),
            (AccentId::Mychess, false) => rgb(0x00ff_ffff),
            _ => {
                // The one with the higher WCAG ratio (D1607); the relative
                // luminance heuristic of `contrast_on` can pick the worse one.
                let background = self.color(dark);
                if contrast_ratio(Color::BLACK, background)
                    >= contrast_ratio(Color::WHITE, background)
                {
                    Color::BLACK
                } else {
                    Color::WHITE
                }
            }
        }
    }

    /// The accent whose color in the theme `dark` is `color`, if any.
    pub fn from_color(color: Color, dark: bool) -> Option<AccentId> {
        AccentId::ALL.into_iter().find(|a| a.color(dark) == color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{I18n, Language};

    #[test]
    fn every_accent_has_a_name_in_both_languages() {
        for language in [Language::EnUs, Language::PtBr] {
            let i18n = I18n::new(language);
            for accent in AccentId::ALL {
                let key = accent.name_key();
                let name = i18n.tr(key);
                assert!(!name.is_empty() && name != key, "{key} in {language:?}");
            }
            for key in ["menu-view-accent", "accent-selected"] {
                assert_ne!(i18n.tr(key), key, "{key} in {language:?}");
            }
        }
    }

    #[test]
    fn colors_round_trip_through_from_color() {
        for dark in [true, false] {
            for accent in AccentId::ALL {
                assert_eq!(AccentId::from_color(accent.color(dark), dark), Some(accent));
            }
        }
        assert_eq!(AccentId::from_color(Color::BLACK, true), None);
    }

    #[test]
    fn computed_on_colors_are_the_best_of_black_and_white() {
        for dark in [true, false] {
            for accent in AccentId::ALL.into_iter().skip(1) {
                let bg = accent.color(dark);
                let best = if contrast_ratio(Color::BLACK, bg) >= contrast_ratio(Color::WHITE, bg) {
                    Color::BLACK
                } else {
                    Color::WHITE
                };
                assert_eq!(accent.on_color(dark), best, "{accent:?} dark={dark}");
            }
        }
    }

    #[test]
    fn computed_on_colors_are_black_or_white() {
        for dark in [true, false] {
            for accent in AccentId::ALL.into_iter().skip(1) {
                let on = accent.on_color(dark);
                assert!(on == Color::BLACK || on == Color::WHITE);
            }
        }
    }
}
