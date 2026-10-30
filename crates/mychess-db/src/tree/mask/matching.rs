// Derived from Scid vs PC 4.27, tcl/windows/tree.tcl (`::tree::mask::moveExists`
// 2125, `removeFromMask` 2099) and src/tkscid.cpp (`sc_pos matchMoves`
// 11638-11720). Copyright (c) 1999-2002 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License (see
// its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! Equivalence of move texts (decision D1805), without regular expressions.

use std::cell::OnceCell;
use std::collections::HashSet;

use mychess_core::{Move, Position, SanMode, SanSuffix};

use super::model::NULL_MOVE;

/// The text of a move without `+`, `#`, `!`, `?` and white space, with the
/// castling spellings (`0-0`, `OO`, `OK`, `0-0-0`, `OQ`) turned into `O-O`
/// and `O-O-O`.
pub fn normalize_move_text(text: &str) -> String {
    let s: String = text
        .chars()
        .filter(|c| !matches!(c, '+' | '#' | '!' | '?') && !c.is_whitespace())
        .collect();
    match s.as_str() {
        "O-O" | "0-0" | "OK" | "OO" => "O-O".to_owned(),
        "O-O-O" | "0-0-0" | "OQ" => "O-O-O".to_owned(),
        _ => s,
    }
}

/// [`normalize_move_text`] without `x` and `=` either: the form in which the
/// Scid's "add move" submenu stores moves.
pub fn bare_move_text(text: &str) -> String {
    strip_capture(&normalize_move_text(text))
}

fn strip_capture(normalized: &str) -> String {
    normalized
        .chars()
        .filter(|&c| c != 'x' && c != '=')
        .collect()
}

/// Compares move texts in the context of a position (D1805): equal after
/// [`normalize_move_text`], or equal after dropping `x` and `=` when that
/// text is the SAN of a legal move of the position (the entries of the Scid's
/// "add move" submenu have no `x` and no `=`). A text without `x` and `=`
/// names one move only, so the legality check is what keeps the rule honest.
#[derive(Debug)]
pub struct MoveMatcher<'p> {
    position: Option<&'p Position>,
    legal: OnceCell<HashSet<String>>,
}

impl<'p> MoveMatcher<'p> {
    /// A matcher for `position`. Without a position the second rule applies
    /// without checking legality.
    pub fn new(position: Option<&'p Position>) -> MoveMatcher<'p> {
        MoveMatcher {
            position,
            legal: OnceCell::new(),
        }
    }

    fn legal_bare(&self) -> &HashSet<String> {
        self.legal.get_or_init(|| {
            let mut set = HashSet::new();
            if let Some(pos) = self.position {
                for m in pos.legal_moves() {
                    if let Ok(san) = pos.san_with(m, SanSuffix::None) {
                        set.insert(strip_capture(&san));
                    }
                }
            }
            set
        })
    }

    /// Whether the texts name the same move.
    pub fn same(&self, a: &str, b: &str) -> bool {
        let (na, nb) = (normalize_move_text(a), normalize_move_text(b));
        if na == nb {
            return true;
        }
        if na == NULL_MOVE || nb == NULL_MOVE {
            return false;
        }
        let (sa, sb) = (strip_capture(&na), strip_capture(&nb));
        if sa != sb {
            return false;
        }
        self.position.is_none() || self.legal_bare().contains(&sa)
    }
}

/// The legal move of `position` that `text` names (the text of a mask move),
/// if any.
pub fn resolve_move(position: &Position, text: &str) -> Option<Move> {
    let normalized = normalize_move_text(text);
    if normalized.is_empty() || normalized == NULL_MOVE {
        return None;
    }
    if let Ok(parsed) = position.parse_san(&normalized, SanMode::Tolerant)
        && !parsed.mv.is_null()
        && position.is_legal(parsed.mv)
    {
        return Some(parsed.mv);
    }
    let bare = strip_capture(&normalized);
    position.legal_moves().into_iter().find(|&m| {
        position
            .san_with(m, SanSuffix::None)
            .is_ok_and(|san| strip_capture(&san) == bare)
    })
}

/// The SAN written for a move added to a mask (D1806): `+` for check and for
/// mate, never `#`.
pub fn mask_san(position: &Position, mv: Move) -> Option<String> {
    position.san_with(mv, SanSuffix::Check).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(moves: &[&str]) -> Position {
        let mut p = Position::new();
        for m in moves {
            let mv = p.parse_san(m, SanMode::Strict).unwrap().mv;
            p.play(mv).unwrap();
        }
        p
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize_move_text(" Qh5+! "), "Qh5");
        assert_eq!(normalize_move_text("Qxf7#"), "Qxf7");
        for s in ["O-O", "0-0", "OK", "OO", "O-O+"] {
            assert_eq!(normalize_move_text(s), "O-O");
        }
        for s in ["O-O-O", "0-0-0", "OQ"] {
            assert_eq!(normalize_move_text(s), "O-O-O");
        }
        assert_eq!(bare_move_text("exd8=Q+"), "ed8Q");
    }

    #[test]
    fn matches_with_and_without_position() {
        let p = after(&["e4", "e5", "Qh5", "Nc6", "Bc4", "Nf6"]);
        let m = MoveMatcher::new(Some(&p));
        assert!(m.same("Qh5", "Qh5+"));
        assert!(m.same("Qxf7", "Qxf7#"));
        assert!(m.same("Qf7", "Qxf7+"), "bare entry of the add-move submenu");
        assert!(!m.same("Qf7", "Qxf6"));
        assert!(!m.same("Qg6", "Qxf7"));
        assert!(!m.same("null", "Nf3"));
        let none = MoveMatcher::new(None);
        assert!(none.same("Qf7", "Qxf7"));
    }

    #[test]
    fn promotion_and_castling_spellings() {
        let p = Position::from_fen("1n2k3/P7/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let m = MoveMatcher::new(Some(&p));
        assert!(m.same("a8Q", "a8=Q"));
        assert!(m.same("axb8Q", "axb8=Q+"));
        assert!(!m.same("a8Q", "a8=R"));
        assert!(m.same("OK", "O-O"));
        assert!(m.same("OQ", "0-0-0"));
        assert!(!m.same("O-O", "O-O-O"));
    }

    #[test]
    fn resolves_moves() {
        let p = Position::from_fen("1n2k3/P7/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        for t in ["Kd2", "O-O", "OK", "0-0-0", "a8=Q", "a8Q", "axb8=N"] {
            assert!(resolve_move(&p, t).is_some(), "{t}");
        }
        for t in ["null", "", "Ke4", "zz"] {
            assert!(resolve_move(&p, t).is_none(), "{t}");
        }
        let start = Position::new();
        let mv = start.parse_uci("e2e4").unwrap();
        assert_eq!(mask_san(&start, mv).as_deref(), Some("e4"));
    }

    #[test]
    fn mate_is_written_with_plus() {
        let p = after(&["f3", "e5", "g4"]);
        let mv = p.parse_san("Qh4", SanMode::Strict).unwrap().mv;
        assert_eq!(mask_san(&p, mv).as_deref(), Some("Qh4+"));
        let mut p2 = p.clone();
        p2.play(mv).unwrap();
        assert!(p2.is_checkmate());
    }
}
