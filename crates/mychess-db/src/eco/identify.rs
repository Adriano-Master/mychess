// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_eco_game`). Copyright
// (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is distributed
// under the GNU General Public License (see its COPYING); this file is part
// of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! [`identify_opening`]: the deepest book position of a game (D1189).

use mychess_core::{ClassifyOptions, Eco, EcoBook, Game};

/// The opening found in a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningMatch {
    /// The extended ECO code of the position.
    pub eco: Eco,
    /// Half-moves played to reach the position (0 = the start).
    pub ply: u32,
    /// The name of the opening.
    pub name: String,
}

/// The deepest position of the main line of `game` that is in the book
/// (`Game > Identify Opening`). The whole line is examined, without the
/// limits of the batch classification; transpositions match by position.
/// `None` when nothing matches. The game is not changed.
pub fn identify_opening(book: &EcoBook, game: &Game) -> Option<OpeningMatch> {
    let found = book.classify(game, ClassifyOptions::game())?;
    Some(OpeningMatch {
        eco: found.eco(true),
        ply: found.ply,
        name: found.entry().name().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::testutil::game_from_sans as game;
    use super::*;

    const BOOK: &str =
        "A00a \"Start\" *\nC20 \"King's Pawn\" 1.e4 e5 *\nC40 \"Knights\" 1.e4 e5 2.Nf3 *\n";

    #[test]
    fn finds_the_deepest_book_position() {
        let book = EcoBook::parse(BOOK).unwrap();
        let found = identify_opening(&book, &game(&["e4", "e5", "Nf3", "Nc6"])).unwrap();
        assert_eq!(found.eco.to_string(), "C40");
        assert_eq!(found.ply, 3);
        assert_eq!(found.name, "Knights");
        let start = identify_opening(&book, &game(&["d4"])).unwrap();
        assert_eq!((start.eco.to_string(), start.ply), ("A00a".to_owned(), 0));
    }

    #[test]
    fn nothing_matches_without_the_start_entry() {
        let book = EcoBook::parse("C20 \"KP\" 1.e4 e5 *\n").unwrap();
        assert_eq!(identify_opening(&book, &game(&["d4"])), None);
    }
}
