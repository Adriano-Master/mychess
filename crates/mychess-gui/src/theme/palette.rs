//! The two palettes of the interface, "MyChess Dark" (default) and
//! "MyChess Light" (spec M2-5, D217, section 4.1), as raw color tables.
//!
//! This is the only place where the interface colors are written down; the
//! widgets read them through [`Tokens`](super::tokens::Tokens).

use iced::theme::Palette;
use iced::{Color, Theme};

use super::accent::AccentId;

const fn rgb(hex: u32) -> Color {
    // Truncation is intended: each channel is one byte of `hex`.
    #[allow(clippy::cast_possible_truncation)]
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The tones a NAG symbol can take (D245).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NagTones {
    /// `!!`.
    pub brilliant: Color,
    /// `!`.
    pub good: Color,
    /// `!?`.
    pub interesting: Color,
    /// `?!`.
    pub dubious: Color,
    /// `?`.
    pub mistake: Color,
    /// `??`.
    pub blunder: Color,
    /// Evaluations (advantage, equality).
    pub eval: Color,
}

/// The raw colors of one palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Colors {
    pub window: Color,
    pub panel: Color,
    pub raised: Color,
    pub zebra: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub variation: Color,
    pub hover: Color,
    pub danger: Color,
    pub nag: NagTones,
}

/// "MyChess Dark": graphite, with panels slightly lighter than the window.
pub(super) const DARK: Colors = Colors {
    window: rgb(0x001b_1d20),
    panel: rgb(0x0025_282c),
    raised: rgb(0x002e_3238),
    zebra: rgb(0x0029_2d31),
    border: rgb(0x003b_4047),
    text: rgb(0x00e8_eaed),
    muted: rgb(0x00a7_aeb7),
    accent: rgb(0x007c_c04a),
    on_accent: rgb(0x0012_200a),
    variation: rgb(0x00c2_c8cf),
    hover: rgb(0x0034_373b),
    danger: rgb(0x00ff_7a70),
    nag: NagTones {
        brilliant: rgb(0x005f_d4d4),
        good: rgb(0x007f_d66b),
        interesting: rgb(0x00c7_9cff),
        dubious: rgb(0x00f0_d060),
        mistake: rgb(0x00ff_a75a),
        blunder: rgb(0x00ff_7a70),
        eval: rgb(0x009d_b4cf),
    },
};

/// "MyChess Light": the equivalent on white panels.
pub(super) const LIGHT: Colors = Colors {
    window: rgb(0x00e9_ecef),
    panel: rgb(0x00ff_ffff),
    raised: rgb(0x00f1_f3f5),
    zebra: rgb(0x00f6_f7f8),
    border: rgb(0x00d0_d5da),
    text: rgb(0x001b_1f23),
    muted: rgb(0x0054_5e69),
    accent: rgb(0x002f_6b17),
    on_accent: rgb(0x00ff_ffff),
    variation: rgb(0x003a_424a),
    hover: rgb(0x00f0_f0f0),
    danger: rgb(0x00b3_261e),
    nag: NagTones {
        brilliant: rgb(0x000a_6a70),
        good: rgb(0x0020_6b20),
        interesting: rgb(0x006a_3fb5),
        dubious: rgb(0x0075_5900),
        mistake: rgb(0x00a3_4a00),
        blunder: rgb(0x00b3_261e),
        eval: rgb(0x0041_566e),
    },
};

/// Name of the dark theme.
pub const DARK_NAME: &str = "MyChess Dark";
/// Name of the light theme.
pub const LIGHT_NAME: &str = "MyChess Light";

/// The iced theme of a palette and an accent (the accent is the `primary`
/// of the palette, D1603): the iced [`Palette`] feeds the widgets of
/// the toolkit (inputs, scrollbars...), the tokens feed ours.
pub(super) fn iced_theme(dark: bool, accent: AccentId) -> Theme {
    let (name, c) = if dark {
        (DARK_NAME, DARK)
    } else {
        (LIGHT_NAME, LIGHT)
    };
    Theme::custom(
        name,
        Palette {
            background: c.window,
            text: c.text,
            primary: accent.color(dark),
            success: c.nag.good,
            warning: c.nag.dubious,
            danger: c.danger,
        },
    )
}

/// Whether `theme` is a dark one (decided by the background luminance, so
/// the built-in iced themes also map to a palette).
pub(super) fn is_dark(theme: &Theme) -> bool {
    super::contrast::relative_luminance(theme.palette().background) < 0.35
}
