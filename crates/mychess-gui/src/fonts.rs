//! Embedded fonts (no text depends on a system font).

use iced::Font;
use iced::font::{Family, Weight};

/// The interface font, regular weight.
pub(crate) const NOTO_SANS: Font = Font::with_name("Noto Sans");

/// The interface font, bold weight.
pub(crate) const NOTO_SANS_BOLD: Font = Font {
    family: Family::Name("Noto Sans"),
    weight: Weight::Bold,
    ..Font::DEFAULT
};

/// The raw font files to register with `Application::font`.
pub(crate) fn font_data() -> [&'static [u8]; 4] {
    [
        crate::assets::NOTO_SANS_REGULAR,
        crate::assets::NOTO_SANS_BOLD,
        crate::assets::NOTO_SANS_MATH,
        crate::assets::NOTO_SANS_SYMBOLS2,
    ]
}
