//! Board color themes: the ten presets of the Scid and the `MyChess` Green
//! of the modern interface (spec M2-5, D219).
//!
//! Scid: `colorSchemes1`/`colorSchemes2` in `tcl/board.tcl:12-25`; the field
//! order follows `setBoardColor` (`board.tcl:144-159`): light, dark,
//! highlight, best, background.

use iced::Color;

const GREY: Color = Color::from_rgb8(190, 190, 190);

/// Identifies a preset; this is what preferences store.
// The names mirror the Scid color scheme names.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum BoardThemeId {
    /// Light cream and green, the default of the modern interface (D219).
    #[default]
    MyChessGreen,
    /// `#f3f3f3` and `#7389b6` (the Scid default).
    ScidBlueWhite,
    /// Pale green and teal.
    ScidBlueish,
    /// M. Thomas green.
    ScidMThomas,
    /// Yellow and green.
    ScidGreenYellow,
    /// Brown wood.
    ScidBrown,
    /// Tan.
    ScidTan,
    /// Grey.
    ScidGrey,
    /// Rosy.
    ScidRosy,
    /// Steel blue.
    ScidSteelBlue,
    /// Green.
    ScidGreen,
}

impl BoardThemeId {
    /// Every preset, in menu order.
    pub const ALL: [BoardThemeId; 11] = [
        BoardThemeId::MyChessGreen,
        BoardThemeId::ScidBlueWhite,
        BoardThemeId::ScidBlueish,
        BoardThemeId::ScidMThomas,
        BoardThemeId::ScidGreenYellow,
        BoardThemeId::ScidBrown,
        BoardThemeId::ScidTan,
        BoardThemeId::ScidGrey,
        BoardThemeId::ScidRosy,
        BoardThemeId::ScidSteelBlue,
        BoardThemeId::ScidGreen,
    ];

    /// The `kebab-case` name used in the preferences file.
    pub fn kebab_name(self) -> &'static str {
        match self {
            BoardThemeId::MyChessGreen => "mychess-green",
            BoardThemeId::ScidBlueWhite => "scid-blue-white",
            BoardThemeId::ScidBlueish => "scid-blueish",
            BoardThemeId::ScidMThomas => "scid-mthomas",
            BoardThemeId::ScidGreenYellow => "scid-green-yellow",
            BoardThemeId::ScidBrown => "scid-brown",
            BoardThemeId::ScidTan => "scid-tan",
            BoardThemeId::ScidGrey => "scid-grey",
            BoardThemeId::ScidRosy => "scid-rosy",
            BoardThemeId::ScidSteelBlue => "scid-steel-blue",
            BoardThemeId::ScidGreen => "scid-green",
        }
    }

    /// Parses [`kebab_name`](Self::kebab_name).
    pub fn from_kebab_name(name: &str) -> Option<BoardThemeId> {
        BoardThemeId::ALL
            .into_iter()
            .find(|id| id.kebab_name() == name)
    }

    /// The Fluent key of the theme's display name.
    pub fn name_key(self) -> &'static str {
        match self {
            BoardThemeId::MyChessGreen => "board-theme-mychess-green",
            BoardThemeId::ScidBlueWhite => "board-theme-scid-blue-white",
            BoardThemeId::ScidBlueish => "board-theme-scid-blueish",
            BoardThemeId::ScidMThomas => "board-theme-scid-mthomas",
            BoardThemeId::ScidGreenYellow => "board-theme-scid-green-yellow",
            BoardThemeId::ScidBrown => "board-theme-scid-brown",
            BoardThemeId::ScidTan => "board-theme-scid-tan",
            BoardThemeId::ScidGrey => "board-theme-scid-grey",
            BoardThemeId::ScidRosy => "board-theme-scid-rosy",
            BoardThemeId::ScidSteelBlue => "board-theme-scid-steel-blue",
            BoardThemeId::ScidGreen => "board-theme-scid-green",
        }
    }
}

/// The colors of a board.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoardTheme {
    /// Which preset this is.
    pub id: BoardThemeId,
    /// Light squares.
    pub light: Color,
    /// Dark squares.
    pub dark: Color,
    /// Selected square and drop target.
    pub selected: Color,
    /// Legal destination dots and rings.
    pub hint: Color,
    /// The 1 px outline around the board.
    pub frame: Color,
    /// The last move fill (drawn translucent).
    pub last_move: Color,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

impl BoardTheme {
    /// The theme of a preset.
    pub fn of(id: BoardThemeId) -> BoardTheme {
        let (light, dark, selected, hint, frame) = match id {
            BoardThemeId::MyChessGreen => (
                rgb(0xee, 0xf3, 0xdc),
                rgb(0x55, 0x82, 0x3a),
                rgb(0xf2, 0xf2, 0x7a),
                rgb(0x2f, 0x4a, 0x1f),
                rgb(0x2b, 0x4a, 0x1d),
            ),
            BoardThemeId::ScidBlueWhite => (
                rgb(0xf3, 0xf3, 0xf3),
                rgb(0x73, 0x89, 0xb6),
                rgb(0xf3, 0xf4, 0x84),
                rgb(0xb8, 0xcb, 0xf8),
                rgb(54, 100, 139), // steelblue4
            ),
            BoardThemeId::ScidBlueish => (
                rgb(0xd0, 0xe0, 0xd0),
                rgb(0x80, 0xa0, 0xa0),
                rgb(0xb0, 0xd0, 0xe0),
                rgb(0xf0, 0xf0, 0xa0),
                rgb(190, 190, 190), // grey
            ),
            BoardThemeId::ScidMThomas => (
                rgb(0xd3, 0xd9, 0xa8),
                rgb(0x51, 0xa0, 0x68),
                rgb(0xe0, 0xd8, 0x73),
                rgb(0x86, 0xa0, 0x00),
                rgb(51, 51, 51), // grey20
            ),
            BoardThemeId::ScidGreenYellow => (
                rgb(0xe0, 0xd0, 0x70),
                rgb(0x70, 0xa0, 0x70),
                rgb(0xb0, 0xd0, 0xe0),
                rgb(0xbe, 0xbe, 0xbe),
                rgb(0x29, 0x76, 0x4e),
            ),
            BoardThemeId::ScidBrown => (
                rgb(0xd0, 0xc0, 0xa0),
                rgb(0xa0, 0x80, 0x50),
                rgb(0xb0, 0xd0, 0xe0),
                rgb(0xbe, 0xbe, 0xbe),
                rgb(139, 90, 43), // tan4
            ),
            BoardThemeId::ScidTan => (
                rgb(0xfb, 0xdb, 0xc4),
                rgb(0xcc, 0x9c, 0x83),
                rgb(0xb0, 0xd0, 0xe0),
                rgb(0xbe, 0xbe, 0xbe),
                rgb(139, 105, 105), // rosybrown4
            ),
            BoardThemeId::ScidGrey => (
                rgb(0x9d, 0x9d, 0x9b),
                rgb(0x7a, 0x79, 0x77),
                rgb(0x6c, 0x88, 0x98),
                rgb(0xbe, 0xbe, 0xbe),
                rgb(54, 100, 139), // steelblue4
            ),
            BoardThemeId::ScidRosy => (
                rgb(0xf8, 0xdb, 0xcc),
                rgb(188, 143, 143), // rosybrown
                rgb(0xb0, 0xd0, 0xe0),
                rgb(0xbe, 0xbe, 0xbe),
                rgb(139, 105, 105), // rosybrown4
            ),
            BoardThemeId::ScidSteelBlue => (
                rgb(176, 196, 222), // lightsteelblue
                rgb(70, 130, 180),  // steelblue
                rgb(0x51, 0xa0, 0x68),
                rgb(0xe0, 0xd8, 0x73),
                rgb(0x00, 0x29, 0x58),
            ),
            BoardThemeId::ScidGreen => (
                rgb(0x46, 0xa6, 0x31),
                rgb(0x00, 0x64, 0x00),
                rgb(0x00, 0x80, 0x64),
                rgb(139, 105, 105), // rosybrown4
                rgb(0x8b, 0xb8, 0x69),
            ),
        };
        BoardTheme {
            id,
            light,
            dark,
            selected,
            hint,
            frame,
            last_move: GREY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::mark_colors::parse_mark_color;

    #[test]
    fn default_preset_is_the_mychess_green() {
        let t = BoardTheme::of(BoardThemeId::default());
        assert_eq!(BoardThemeId::default().kebab_name(), "mychess-green");
        assert_eq!(t.light, Color::from_rgb8(0xee, 0xf3, 0xdc));
        assert_eq!(t.dark, Color::from_rgb8(0x55, 0x82, 0x3a));
        assert_eq!(t.selected, Color::from_rgb8(0xf2, 0xf2, 0x7a));
        assert_eq!(t.hint, Color::from_rgb8(0x2f, 0x4a, 0x1f));
        assert_eq!(t.frame, Color::from_rgb8(0x2b, 0x4a, 0x1d));
    }

    #[test]
    fn scid_default_preset_keeps_its_colors() {
        let t = BoardTheme::of(BoardThemeId::ScidBlueWhite);
        assert_eq!(t.light, Color::from_rgb8(0xf3, 0xf3, 0xf3));
        assert_eq!(t.dark, Color::from_rgb8(0x73, 0x89, 0xb6));
        assert_eq!(t.selected, Color::from_rgb8(0xf3, 0xf4, 0x84));
    }

    #[test]
    fn named_scheme_colors_agree_with_the_tk_table() {
        let check = |id, name: &str, pick: fn(&BoardTheme) -> Color| {
            assert_eq!(
                Some(pick(&BoardTheme::of(id))),
                parse_mark_color(name),
                "{id:?} {name}"
            );
        };
        check(BoardThemeId::ScidBlueWhite, "steelblue4", |t| t.frame);
        check(BoardThemeId::ScidBlueish, "grey", |t| t.frame);
        check(BoardThemeId::ScidMThomas, "grey20", |t| t.frame);
        check(BoardThemeId::ScidBrown, "tan4", |t| t.frame);
        check(BoardThemeId::ScidTan, "rosybrown4", |t| t.frame);
        check(BoardThemeId::ScidRosy, "rosybrown", |t| t.dark);
        check(BoardThemeId::ScidSteelBlue, "lightsteelblue", |t| t.light);
        check(BoardThemeId::ScidSteelBlue, "steelblue", |t| t.dark);
        check(BoardThemeId::ScidGreen, "rosybrown4", |t| t.hint);
        check(BoardThemeId::ScidGrey, "steelblue4", |t| t.frame);
    }

    #[test]
    fn kebab_names_round_trip_and_are_unique() {
        assert_eq!(BoardThemeId::ALL.len(), 11);
        let names: std::collections::HashSet<_> =
            BoardThemeId::ALL.iter().map(|id| id.kebab_name()).collect();
        assert_eq!(names.len(), 11);
        for id in BoardThemeId::ALL {
            assert_eq!(BoardThemeId::from_kebab_name(id.kebab_name()), Some(id));
        }
        assert_eq!(BoardThemeId::from_kebab_name("nope"), None);
    }
}
