//! The Polyglot book as seen by the app (spec M8-1, D1234; M4-6, D472).
//!
//! [`PlayBook`] wraps a [`mychess_book::Book`] read from disk (by
//! [`load_play_book`], which blocks and belongs to `perform`) and implements
//! [`BookProbe`], the hook of the automatic annotation, so one book serves
//! both the game against the engine and the annotation.

use std::path::Path;
use std::sync::Arc;

use mychess_book::{Book, BookError};
use mychess_core::{Move, Position};

use super::config::BookSelection;
use super::message::BookFailure;
use crate::annotate::BookProbe;

/// A book ready to play from: shared, cheap to clone.
#[derive(Debug, Clone)]
pub struct PlayBook {
    book: Arc<Book>,
    name: String,
    selection: BookSelection,
}

impl PartialEq for PlayBook {
    fn eq(&self, other: &PlayBook) -> bool {
        Arc::ptr_eq(&self.book, &other.book)
            && self.name == other.name
            && self.selection == other.selection
    }
}

impl PlayBook {
    /// Wraps `book`. The name is the file stem of the path, or `book`.
    pub fn new(book: Book, selection: BookSelection) -> PlayBook {
        let name = book
            .path()
            .and_then(Path::file_stem)
            .and_then(|s| s.to_str())
            .unwrap_or("book")
            .to_owned();
        PlayBook {
            book: Arc::new(book),
            name,
            selection,
        }
    }

    /// The same book with another way of picking the move.
    #[must_use]
    pub fn with_selection(mut self, selection: BookSelection) -> PlayBook {
        self.selection = selection;
        self
    }

    /// The name shown in the comments (`Last book move (name)`).
    pub fn book_name(&self) -> &str {
        &self.name
    }

    /// Whether the book has a move for `position`.
    pub fn has_moves(&self, position: &Position) -> bool {
        !self.book.moves(position).is_empty()
    }

    /// The move the engine plays: the heaviest one, or one drawn with
    /// probability proportional to the weight using `roll`.
    pub fn pick(&self, position: &Position, roll: u64) -> Option<Move> {
        match self.selection {
            BookSelection::Best => self.book.best(position).map(|m| m.mv),
            BookSelection::Weighted => self.book.pick(position, |_| roll).map(|m| m.mv),
        }
    }
}

impl BookProbe for PlayBook {
    fn moves(&self, position: &Position) -> Vec<Move> {
        self.book
            .moves(position)
            .into_iter()
            .map(|m| m.mv)
            .collect()
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Reads the book at `path` (blocking: call from `perform`).
///
/// # Errors
///
/// Whatever [`Book::open_readonly`] reports.
pub fn load_play_book(path: &Path, selection: BookSelection) -> Result<PlayBook, BookError> {
    Book::open_readonly(path).map(|book| PlayBook::new(book, selection))
}

/// Like [`load_play_book`], with the failure reduced to what the message
/// tells the front-end.
pub(crate) fn read_book(path: &Path, selection: BookSelection) -> Result<PlayBook, BookFailure> {
    load_play_book(path, selection).map_err(|error| match error {
        BookError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
            BookFailure::NotFound
        }
        _ => BookFailure::Unreadable,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A book with `e4` (weight 70) and `d4` (30) at the start, `e5` after
    /// `e4`.
    pub(crate) fn sample_book(selection: BookSelection) -> PlayBook {
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        let d4 = start.parse_uci("d2d4").expect("uci");
        let after = start.after(e4).expect("after");
        let e5 = after.parse_uci("e7e5").expect("uci");
        let mut book = Book::new();
        book.set_position_moves(&start, &[(e4, 70), (d4, 30)])
            .expect("set");
        book.set_position_moves(&after, &[(e5, 100)]).expect("set");
        PlayBook::new(book, selection)
    }

    #[test]
    fn best_picks_the_heaviest_move() {
        let book = sample_book(BookSelection::Best);
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        for roll in [0, 50, 99] {
            assert_eq!(book.pick(&start, roll), Some(e4));
        }
    }

    #[test]
    fn weighted_follows_the_roll() {
        let book = sample_book(BookSelection::Weighted);
        let start = Position::new();
        let moves = book.moves(&start);
        assert_eq!(moves.len(), 2);
        let first = book.pick(&start, 0).expect("move");
        let last = book.pick(&start, 99).expect("move");
        assert_ne!(first, last, "the two ends of the roll differ");
    }

    #[test]
    fn a_position_out_of_the_book_has_no_move() {
        let book = sample_book(BookSelection::Best);
        let p = Position::from_fen("3k4/8/8/8/8/8/4R3/4K3 w - - 0 1").expect("fen");
        assert!(!book.has_moves(&p));
        assert_eq!(book.pick(&p, 3), None);
    }

    #[test]
    fn the_book_is_an_annotation_probe() {
        let book = sample_book(BookSelection::Best);
        let probe: &dyn BookProbe = &book;
        let start = Position::new();
        assert_eq!(probe.moves(&start).len(), 2);
        assert_eq!(probe.name(), "book");
    }

    #[test]
    fn the_name_comes_from_the_file_and_a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("Mini.bin");
        let mut book = Book::new();
        let start = Position::new();
        let e4 = start.parse_uci("e2e4").expect("uci");
        book.set_position_moves(&start, &[(e4, 10)]).expect("set");
        book.save_as(&path).expect("save");
        let loaded = load_play_book(&path, BookSelection::Best).expect("load");
        assert_eq!(loaded.book_name(), "Mini");
        assert!(load_play_book(&dir.path().join("none.bin"), BookSelection::Best).is_err());
        assert_eq!(
            read_book(&dir.path().join("none.bin"), BookSelection::Best).err(),
            Some(BookFailure::NotFound)
        );
        let bad = dir.path().join("bad.bin");
        std::fs::write(&bad, b"xyz").expect("write");
        assert_eq!(
            read_book(&bad, BookSelection::Best).err(),
            Some(BookFailure::Unreadable)
        );
    }
}
