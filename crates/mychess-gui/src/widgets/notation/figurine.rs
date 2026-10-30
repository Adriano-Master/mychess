//! Figurines: a SAN move cut into text and piece glyphs (spec M2-5, D243).
//!
//! The model, the copied PGN and the oracle keep English SAN; this is only
//! how a move is drawn. The pieces are the filled chess symbols of
//! `widgets::glyph`, drawn in the Noto Sans Symbols 2 font.

use mychess_core::Role;

use crate::widgets::glyph::{figurine_split, piece_glyph};

/// One piece of a move as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// A piece symbol (`♞`).
    Glyph(Role),
    /// Plain text (`f3+`, `e8=`).
    Text(String),
}

impl Part {
    /// The text to shape for this part.
    pub fn text(&self) -> String {
        match self {
            Part::Glyph(role) => piece_glyph(*role).to_string(),
            Part::Text(text) => text.clone(),
        }
    }
}

/// The parts of `san`. Without `figurines`, or for moves with no piece letter
/// (pawn moves, castling, the null move), it is the text alone. `Nf3+` is a
/// glyph and `f3+`; `e8=Q#` is `e8=`, a glyph and `#`.
pub fn san_parts(san: &str, figurines: bool) -> Vec<Part> {
    if !figurines {
        return vec![Part::Text(san.to_owned())];
    }
    let split = figurine_split(san);
    if split.piece.is_none() && split.promotion.is_none() {
        return vec![Part::Text(san.to_owned())];
    }
    let mut parts = Vec::with_capacity(4);
    if let Some(role) = split.piece {
        parts.push(Part::Glyph(role));
    }
    let mut text = split.body.to_owned();
    if let Some(role) = split.promotion {
        text.push('=');
        parts.push(Part::Text(text));
        parts.push(Part::Glyph(role));
        text = String::new();
    }
    text.push_str(split.suffix);
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> Part {
        Part::Text(text.to_owned())
    }

    #[test]
    fn piece_moves_start_with_a_glyph() {
        assert_eq!(san_parts("Nf3", true), [Part::Glyph(Role::Knight), t("f3")]);
        assert_eq!(
            san_parts("Qxd5+", true),
            [Part::Glyph(Role::Queen), t("xd5+")]
        );
        assert_eq!(
            san_parts("Rad1#", true),
            [Part::Glyph(Role::Rook), t("ad1#")]
        );
    }

    #[test]
    fn promotions_put_the_glyph_after_the_equals_sign() {
        assert_eq!(
            san_parts("e8=Q+", true),
            [t("e8="), Part::Glyph(Role::Queen), t("+")]
        );
        assert_eq!(
            san_parts("bxa8=N#", true),
            [t("bxa8="), Part::Glyph(Role::Knight), t("#")]
        );
        assert_eq!(san_parts("e8=R", true), [t("e8="), Part::Glyph(Role::Rook)]);
    }

    #[test]
    fn pawns_castling_and_the_null_move_stay_text() {
        for san in ["e4", "exd5+", "O-O", "O-O-O+", "--", ""] {
            assert_eq!(san_parts(san, true), [t(san)], "{san:?}");
        }
    }

    #[test]
    fn without_figurines_the_move_is_untouched() {
        for san in ["Nf3", "e8=Q+", "O-O"] {
            assert_eq!(san_parts(san, false), [t(san)]);
        }
    }

    #[test]
    fn the_parts_spell_the_move_back() {
        for san in ["Nf3", "Qxd5+", "e8=Q#", "bxa8=N", "Bb5", "Kg2"] {
            let back: String = san_parts(san, true)
                .iter()
                .map(|part| match part {
                    Part::Glyph(role) => match role {
                        Role::King => 'K',
                        Role::Queen => 'Q',
                        Role::Rook => 'R',
                        Role::Bishop => 'B',
                        Role::Knight => 'N',
                        Role::Pawn => 'P',
                    }
                    .to_string(),
                    Part::Text(text) => text.clone(),
                })
                .collect();
            assert_eq!(back, san);
        }
    }

    #[test]
    fn glyph_parts_shape_the_filled_symbol() {
        assert_eq!(Part::Glyph(Role::Knight).text(), "\u{265E}");
        assert_eq!(Part::Glyph(Role::Queen).text(), "\u{265B}");
        assert_eq!(t("f3").text(), "f3");
    }
}
