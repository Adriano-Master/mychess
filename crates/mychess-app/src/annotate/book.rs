//! The hook for opening books (spec M4-6, D472).
//!
//! The annotation can skip the moves that are in a book and comment the
//! first one that is not (`bookAnnotation`, `analysis.tcl` 1243-1304). The
//! book itself arrives with milestone M5; until then the option is disabled
//! and nothing implements [`BookProbe`] outside the tests of the hook.

use mychess_core::{Move, Position};

/// A source of opening book moves.
pub trait BookProbe {
    /// The moves the book has for `position`.
    fn moves(&self, position: &Position) -> Vec<Move>;

    /// The name of the book, for the comments.
    fn name(&self) -> &str;
}

/// How a move relates to a book (D472).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookVerdict {
    /// The move is in the book and the next position still has book
    /// moves.
    InBook,
    /// The move is in the book and the next position has none: the
    /// "last book move".
    LastBookMove,
    /// The move is not in the book; the book's moves are listed (SAN).
    OutOfBook(Vec<String>),
}

/// Classifies `played`, made from `before`, against `book`.
pub fn classify(book: &dyn BookProbe, before: &Position, played: Move) -> BookVerdict {
    let candidates = book.moves(before);
    if !candidates.contains(&played) {
        let san = candidates
            .iter()
            .filter_map(|m| before.san(*m).ok())
            .collect();
        return BookVerdict::OutOfBook(san);
    }
    let mut after = before.clone();
    if after.play(played).is_err() || book.moves(&after).is_empty() {
        BookVerdict::LastBookMove
    } else {
        BookVerdict::InBook
    }
}

/// The comment of the first move out of the book (`MoveOutOfBook`). It is
/// written in English like every comment of the annotation (they are data of
/// the game, not interface).
///
/// Without book moves of the position before there is no list, no colon
/// (Scid `analysis.tcl` 1283-1287).
pub fn out_of_book_comment(book: &str, moves: &[String]) -> String {
    if moves.is_empty() {
        format!("Move out of book ({book})")
    } else {
        format!("Move out of book ({book}: {})", moves.join(" "))
    }
}

/// The comment of the last move of the book (`LastBookMove`).
pub fn last_book_move_comment(book: &str) -> String {
    format!("Last book move ({book})")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Vec<(String, Vec<&'static str>)>);

    impl BookProbe for Fixed {
        fn moves(&self, position: &Position) -> Vec<Move> {
            let fen = position.fen();
            self.0
                .iter()
                .find(|(f, _)| *f == fen)
                .map(|(_, ms)| {
                    ms.iter()
                        .filter_map(|u| position.parse_uci(u).ok())
                        .collect()
                })
                .unwrap_or_default()
        }

        #[allow(clippy::unnecessary_literal_bound)] // the trait says `&str`
        fn name(&self) -> &str {
            "Fixed"
        }
    }

    #[test]
    fn classifies_in_last_and_out_of_book() {
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        let d4 = start.parse_uci("d2d4").expect("uci");
        let mut after = start.clone();
        after.play(e4).expect("play");
        let book = Fixed(vec![
            (start.fen(), vec!["e2e4", "c2c4"]),
            (after.fen(), vec!["e7e5"]),
        ]);
        assert_eq!(classify(&book, &start, e4), BookVerdict::InBook);
        assert_eq!(
            classify(&book, &start, d4),
            BookVerdict::OutOfBook(vec!["e4".to_owned(), "c4".to_owned()])
        );
        let e5 = after.parse_uci("e7e5").expect("uci");
        assert_eq!(classify(&book, &after, e5), BookVerdict::LastBookMove);
    }

    #[test]
    fn the_comments_are_written_in_english() {
        assert_eq!(
            out_of_book_comment("Fixed", &["e4".to_owned(), "c4".to_owned()]),
            "Move out of book (Fixed: e4 c4)"
        );
        assert_eq!(last_book_move_comment("Fixed"), "Last book move (Fixed)");
    }
}
