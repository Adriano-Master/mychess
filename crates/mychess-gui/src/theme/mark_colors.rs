//! Mark colors: Tk color names and `#rgb`/`#rrggbb` to [`Color`].
//!
//! Marks are stored in comments with the color as written (Scid uses `Tk`
//! names such as `skyBlue`; `ChessBase` letters become `green`, `firebrick`,
//! `yellow2`, `blue`). Spec rule 46. `Tk` is case-insensitive and ignores
//! spaces in color names.

use iced::Color;
use mychess_app::pattern_search::model::HighlightRole;

/// The color used when a mark names an unknown color (Scid's default).
pub const DEFAULT_MARK_COLOR_NAME: &str = "firebrick";

/// Named colors (lower case, no spaces) as `0xRRGGBB`: X11 values as Tk
/// defines them.
const NAMED: &[(&str, u32)] = &[
    // The 14 of the Scid comment editor palette (`comment.tcl:11`).
    ("cornsilk2", 0x00ee_e8cd),
    ("wheat2", 0x00ee_d8ae),
    ("burlywood3", 0x00cd_aa7d),
    ("rosybrown", 0x00bc_8f8f),
    ("mediumpurple1", 0x00ab_82ff),
    ("royalblue3", 0x003a_5fcd),
    ("skyblue", 0x0087_ceeb),
    ("cadetblue2", 0x008e_e5ee),
    ("palegreen2", 0x0090_ee90),
    ("yellow3", 0x00cd_cd00),
    ("darkorange1", 0x00ff_7f00),
    ("firebrick", 0x00b2_2222),
    // ChessBase colors.
    ("green", 0x0000_ff00),
    ("yellow2", 0x00ee_ee00),
    ("blue", 0x0000_00ff),
    ("turquoise", 0x0040_e0d0),
    // Board scheme colors (`board.tcl:12-25`).
    ("steelblue4", 0x0036_648b),
    ("rosybrown4", 0x008b_6969),
    ("tan4", 0x008b_5a2b),
    ("steelblue", 0x0046_82b4),
    ("lightsteelblue", 0x00b0_c4de),
    // Basic names.
    ("red", 0x00ff_0000),
    ("yellow", 0x00ff_ff00),
    ("white", 0x00ff_ffff),
    ("black", 0x0000_0000),
    ("grey", 0x00be_bebe),
    ("gray", 0x00be_bebe),
    ("orange", 0x00ff_a500),
    ("purple", 0x00a0_20f0),
    ("pink", 0x00ff_c0cb),
    ("brown", 0x00a5_2a2a),
    ("cyan", 0x0000_ffff),
    ("magenta", 0x00ff_00ff),
];

/// The color of the highlight of a role of the search for patterns (spec
/// M6-2, D3284), as the text a board mark carries. The dark variants are
/// lighter and more saturated, so that they read on the dark theme as the
/// others do on the light one. Mark colors are drawn over the squares of
/// the board, whose own colors are free of the window theme, so the choice
/// follows the theme preference only: "System" takes the light variants
/// (D3299 item 46).
pub const fn highlight_color(role: HighlightRole, dark: bool) -> &'static str {
    match (role, dark) {
        (HighlightRole::Actor, false) => "#2e9e4f",
        (HighlightRole::Actor, true) => "#4cc46d",
        (HighlightRole::Target, false) => "#d9381e",
        (HighlightRole::Target, true) => "#ff6a4d",
        (HighlightRole::Behind, false) => "#e08a00",
        (HighlightRole::Behind, true) => "#ffb03b",
        (HighlightRole::Info, false) => "#3b82f6",
        (HighlightRole::Info, true) => "#6ea8ff",
    }
}

fn rgb(value: u32) -> Color {
    let [_, r, g, b] = value.to_be_bytes();
    Color::from_rgb8(r, g, b)
}

fn parse_hex(digits: &str) -> Option<Color> {
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        3 => {
            let mut parts = digits
                .chars()
                .filter_map(|c| c.to_digit(16))
                .filter_map(|v| u8::try_from(v * 17).ok());
            Some(Color::from_rgb8(
                parts.next()?,
                parts.next()?,
                parts.next()?,
            ))
        }
        6 => u32::from_str_radix(digits, 16).ok().map(rgb),
        _ => None,
    }
}

/// The Tk "grey0".."grey100" family: `round(255 * n / 100)`.
fn parse_grey_level(name: &str) -> Option<Color> {
    let digits = name
        .strip_prefix("grey")
        .or_else(|| name.strip_prefix("gray"))?;
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    if n > 100 {
        return None;
    }
    let level = u8::try_from((n * 255 + 50) / 100).ok()?;
    Some(Color::from_rgb8(level, level, level))
}

/// Converts a color as written in a mark to a [`Color`]; `None` if unknown.
pub fn parse_mark_color(text: &str) -> Option<Color> {
    let text = text.trim();
    if let Some(digits) = text.strip_prefix('#') {
        return parse_hex(digits);
    }
    let name: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    NAMED
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| rgb(*value))
        .or_else(|| parse_grey_level(&name))
}

/// Like [`parse_mark_color`], falling back to the default mark color.
pub fn mark_color_or_default(text: &str) -> Color {
    parse_mark_color(text)
        .or_else(|| parse_mark_color(DEFAULT_MARK_COLOR_NAME))
        .unwrap_or(Color::BLACK)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("firebrick", (178, 34, 34))]
    #[case("FireBrick", (178, 34, 34))]
    #[case("skyBlue", (135, 206, 235))]
    #[case("sky Blue", (135, 206, 235))]
    #[case("#f00", (255, 0, 0))]
    #[case("#7389b6", (0x73, 0x89, 0xb6))]
    #[case("GREY50", (128, 128, 128))]
    #[case("gray10", (26, 26, 26))]
    #[case("grey0", (0, 0, 0))]
    #[case("grey100", (255, 255, 255))]
    #[case("yellow2", (238, 238, 0))]
    #[case("green", (0, 255, 0))]
    fn parses_known_colors(#[case] text: &str, #[case] expected: (u8, u8, u8)) {
        assert_eq!(
            parse_mark_color(text),
            Some(Color::from_rgb8(expected.0, expected.1, expected.2))
        );
    }

    #[rstest]
    #[case("")]
    #[case("nocolor")]
    #[case("#12")]
    #[case("#12345g")]
    #[case("#1234567")]
    #[case("grey101")]
    #[case("grey")]
    fn rejects_unknown_colors(#[case] text: &str) {
        // "grey" alone is the plain name, so it is the only one that parses.
        if text == "grey" {
            assert!(parse_mark_color(text).is_some());
        } else {
            assert_eq!(parse_mark_color(text), None);
        }
    }

    #[test]
    fn the_highlight_colors_parse_and_differ_by_role_and_theme() {
        let roles = [
            HighlightRole::Actor,
            HighlightRole::Target,
            HighlightRole::Behind,
            HighlightRole::Info,
        ];
        let mut seen = Vec::new();
        for dark in [false, true] {
            for role in roles {
                let text = highlight_color(role, dark);
                let color = parse_mark_color(text).expect("a color");
                assert!(!seen.contains(&color), "{text} repeats");
                seen.push(color);
            }
        }
    }

    #[test]
    fn every_palette_and_scheme_name_is_known() {
        for name in [
            "cornsilk2",
            "wheat2",
            "burlyWood3",
            "grey50",
            "gray10",
            "rosyBrown",
            "mediumPurple1",
            "royalBlue3",
            "skyBlue",
            "cadetBlue2",
            "paleGreen2",
            "yellow3",
            "darkOrange1",
            "firebrick",
            "green",
            "yellow2",
            "blue",
            "turquoise",
            "steelblue4",
            "grey20",
            "tan4",
            "rosybrown4",
            "lightsteelblue",
            "steelblue",
            "grey",
            "red",
            "orange",
            "purple",
            "pink",
            "brown",
            "cyan",
            "magenta",
            "white",
            "black",
            "yellow",
        ] {
            assert!(parse_mark_color(name).is_some(), "{name}");
        }
    }

    #[test]
    fn unknown_names_fall_back_to_firebrick() {
        assert_eq!(
            mark_color_or_default("nonsense"),
            Color::from_rgb8(178, 34, 34)
        );
    }
}
