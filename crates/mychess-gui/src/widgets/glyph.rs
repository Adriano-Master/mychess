//! Chess piece glyphs and figurines (spec M2-5, D234, D243, D244): the
//! filled chess symbols `U+265A..=U+265F` of Noto Sans Symbols 2, which the
//! binary already embeds, and the split of a SAN move into the part that
//! becomes a figurine and the rest.

use iced::Font;
use mychess_core::Role;

/// The font that draws the glyphs (Noto Sans Symbols 2, SIL OFL 1.1).
pub const GLYPH_FONT: Font = Font::with_name("Noto Sans Symbols2");

/// The text-presentation selector, so the pawn is not drawn as an emoji.
pub const TEXT_PRESENTATION: char = '\u{FE0E}';

/// The filled glyph of a piece.
pub fn piece_glyph(role: Role) -> char {
    match role {
        Role::King => '\u{265A}',
        Role::Queen => '\u{265B}',
        Role::Rook => '\u{265C}',
        Role::Bishop => '\u{265D}',
        Role::Knight => '\u{265E}',
        Role::Pawn => '\u{265F}',
    }
}

/// The glyph as a string; the pawn carries [`TEXT_PRESENTATION`].
pub fn piece_glyph_text(role: Role) -> String {
    let mut out = String::from(piece_glyph(role));
    if role == Role::Pawn {
        out.push(TEXT_PRESENTATION);
    }
    out
}

/// The six characters the glyph font must cover.
pub const GLYPHS: [char; 6] = [
    '\u{265A}', '\u{265B}', '\u{265C}', '\u{265D}', '\u{265E}', '\u{265F}',
];

/// A SAN move split for drawing with figurines. The text stays English SAN
/// everywhere else (model, copied PGN); this is only presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FigurineSan<'a> {
    /// The moving piece when the move starts with a piece letter (`Nf3`);
    /// `None` for pawn moves, castling and the null move.
    pub piece: Option<Role>,
    /// What follows the piece letter, up to the promotion or the check
    /// suffix: `f3`, `xd5`, `e8`, `O-O`, `--`.
    pub body: &'a str,
    /// The promotion piece of `e8=Q`.
    pub promotion: Option<Role>,
    /// `+`, `#` or empty.
    pub suffix: &'a str,
}

fn role_of(letter: char) -> Option<Role> {
    match letter {
        'K' => Some(Role::King),
        'Q' => Some(Role::Queen),
        'R' => Some(Role::Rook),
        'B' => Some(Role::Bishop),
        'N' => Some(Role::Knight),
        _ => None,
    }
}

/// Splits `san` into piece, body, promotion and check suffix.
pub fn figurine_split(san: &str) -> FigurineSan<'_> {
    let without_suffix = san.trim_end_matches(['+', '#']);
    let suffix = &san[without_suffix.len()..];
    let (core, promotion) = match without_suffix.rsplit_once('=') {
        Some((head, tail)) => {
            let mut chars = tail.chars();
            match (chars.next().and_then(role_of), chars.next()) {
                (Some(role), None) => (head, Some(role)),
                _ => (without_suffix, None),
            }
        }
        None => (without_suffix, None),
    };
    let mut chars = core.chars();
    match chars.next().and_then(role_of) {
        Some(role) if !chars.as_str().is_empty() => FigurineSan {
            piece: Some(role),
            body: chars.as_str(),
            promotion,
            suffix,
        },
        _ => FigurineSan {
            piece: None,
            body: core,
            promotion,
            suffix,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(san: &str) -> (Option<Role>, &str, Option<Role>, &str) {
        let f = figurine_split(san);
        (f.piece, f.body, f.promotion, f.suffix)
    }

    #[test]
    fn splits_piece_moves() {
        assert_eq!(split("Nf3"), (Some(Role::Knight), "f3", None, ""));
        assert_eq!(split("Qxd5+"), (Some(Role::Queen), "xd5", None, "+"));
        assert_eq!(split("Bb5"), (Some(Role::Bishop), "b5", None, ""));
        assert_eq!(split("Rad1#"), (Some(Role::Rook), "ad1", None, "#"));
        assert_eq!(split("Kg2"), (Some(Role::King), "g2", None, ""));
    }

    #[test]
    fn pawn_moves_castling_and_null_have_no_piece() {
        assert_eq!(split("b4"), (None, "b4", None, ""));
        assert_eq!(split("exd5"), (None, "exd5", None, ""));
        assert_eq!(split("O-O"), (None, "O-O", None, ""));
        assert_eq!(split("O-O-O+"), (None, "O-O-O", None, "+"));
        assert_eq!(split("--"), (None, "--", None, ""));
        // A file letter in lower case is never a bishop.
        assert_eq!(split("bxa8"), (None, "bxa8", None, ""));
    }

    #[test]
    fn promotions_split_the_piece_off() {
        assert_eq!(split("e8=Q"), (None, "e8", Some(Role::Queen), ""));
        assert_eq!(split("bxa8=N#"), (None, "bxa8", Some(Role::Knight), "#"));
        assert_eq!(split("e8=Q+"), (None, "e8", Some(Role::Queen), "+"));
    }

    #[test]
    fn odd_input_does_not_panic() {
        assert_eq!(split(""), (None, "", None, ""));
        assert_eq!(split("N"), (None, "N", None, ""));
        assert_eq!(split("+"), (None, "", None, "+"));
        assert_eq!(split("e8="), (None, "e8=", None, ""));
        assert_eq!(split("e8=X"), (None, "e8=X", None, ""));
        assert_eq!(split("é4"), (None, "é4", None, ""));
    }

    #[test]
    fn glyphs_are_the_filled_chess_symbols() {
        let chars: Vec<char> = Role::ALL.iter().map(|&r| piece_glyph(r)).collect();
        assert_eq!(chars.len(), 6);
        for c in chars {
            assert!(GLYPHS.contains(&c));
        }
        assert_eq!(piece_glyph_text(Role::Pawn).chars().count(), 2);
        assert!(piece_glyph_text(Role::Pawn).ends_with(TEXT_PRESENTATION));
        assert_eq!(piece_glyph_text(Role::Queen).chars().count(), 1);
    }
}
