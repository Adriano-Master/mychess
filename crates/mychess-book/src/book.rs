//! [`Book`]: an in-memory Polyglot book with binary-search queries.
//!
//! Scid: `src/polyglot/book.cpp` (`book_open`, `book_moves`, `book_disp`),
//! `src/tkscid.cpp` `sc_book`.

use std::path::{Path, PathBuf};

use mychess_core::{CastlingMode, Move, Position};

use crate::codec::{legal_code_map, lookup_code};
use crate::error::BookError;
use crate::file::{SaveFault, read_book_file, write_atomic};
use crate::format::{Entry, RECORD_SIZE, key_range};
use crate::zobrist::polyglot_key;

/// A move of the book in a given position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookMove {
    /// The legal move. The app renders the SAN (and sorts alphabetically, if
    /// wanted) with the SAN of `mychess-core` over this move; the crate does
    /// not format moves.
    pub mv: Move,
    /// The raw Polyglot encoding found in the file.
    pub code: u16,
    /// The raw weight.
    pub weight: u16,
    /// `round(weight / total * 100)` where `total` sums **every** entry of the
    /// position (Scid), rounded half to even like C's `printf("%.0f")`.
    pub percent: u8,
}

/// The outcome of [`Book::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ValidationReport {
    /// Number of records.
    pub entries: usize,
    /// Bytes after the last whole record in the file (ignored).
    pub trailing_bytes: u64,
    /// Whether the file was already sorted by key.
    pub was_sorted: bool,
    /// Records whose move is `0`.
    pub zero_moves: usize,
    /// Records whose weight is `0`.
    pub zero_weights: usize,
}

impl ValidationReport {
    /// `true` when nothing was found.
    pub fn is_clean(&self) -> bool {
        self.trailing_bytes == 0
            && self.was_sorted
            && self.zero_moves == 0
            && self.zero_weights == 0
    }
}

/// A Polyglot opening book held entirely in memory.
#[derive(Debug, Clone)]
pub struct Book {
    pub(crate) entries: Vec<Entry>,
    path: Option<PathBuf>,
    read_only: bool,
    pub(crate) dirty: bool,
    was_sorted: bool,
    trailing_bytes: u64,
}

impl Book {
    /// An empty book with no file.
    pub fn new() -> Book {
        Book::from_entries(Vec::new())
    }

    /// A book over `entries`; sorted (stably) by key when needed, which
    /// [`Book::was_sorted`] then reports as `false`.
    pub fn from_entries(mut entries: Vec<Entry>) -> Book {
        let was_sorted = entries.windows(2).all(|w| match w {
            [a, b] => a.key <= b.key,
            _ => true,
        });
        if !was_sorted {
            entries.sort_by_key(|e| e.key);
        }
        Book {
            entries,
            path: None,
            read_only: false,
            dirty: false,
            was_sorted,
            trailing_bytes: 0,
        }
    }

    /// Parses the contents of a `.bin` file. Fewer than 16 bytes is an error;
    /// a trailing partial record is ignored.
    pub fn from_bytes(bytes: &[u8]) -> Result<Book, BookError> {
        if bytes.len() < RECORD_SIZE {
            return Err(BookError::TooShort {
                len: bytes.len() as u64,
            });
        }
        let (records, rest) = bytes.as_chunks::<RECORD_SIZE>();
        let entries: Vec<Entry> = records.iter().filter_map(|r| Entry::parse(r)).collect();
        let mut book = Book::from_entries(entries);
        book.trailing_bytes = rest.len() as u64;
        Ok(book)
    }

    /// Opens a book file, reading it whole. The book is read-only when the
    /// file has no write permission.
    ///
    /// The permission test is approximate: it looks at the permission bits
    /// (`!permissions().readonly()`), not at what the current user may do, so
    /// [`Book::save`] can still fail with [`BookError::Io`].
    pub fn open(path: impl AsRef<Path>) -> Result<Book, BookError> {
        let path = path.as_ref();
        let (bytes, writable) = read_book_file(path)?;
        let mut book = Book::from_bytes(&bytes)?;
        book.path = Some(path.to_path_buf());
        book.read_only = !writable;
        Ok(book)
    }

    /// Like [`Book::open`], but editing is refused whatever the permissions.
    pub fn open_readonly(path: impl AsRef<Path>) -> Result<Book, BookError> {
        let mut book = Book::open(path)?;
        book.read_only = true;
        Ok(book)
    }

    /// Number of records.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the book has no records.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All records, sorted by key.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The file this book is bound to, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Whether the book was modified since it was opened or last saved
    /// (D1124). Sorting an unsorted file in memory does not count.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether editing is refused.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// `false` when the source was not sorted by key and was sorted in memory.
    pub fn was_sorted(&self) -> bool {
        self.was_sorted
    }

    /// The records stored for `pos` (the key's whole range, raw).
    pub fn entries_for(&self, pos: &Position) -> &[Entry] {
        self.entries
            .get(key_range(&self.entries, polyglot_key(pos)))
            .unwrap_or_default()
    }

    /// The legal book moves of `pos`, in file order (spec D1105). Positions
    /// that are not standard chess have no book moves.
    pub fn moves(&self, pos: &Position) -> Vec<BookMove> {
        if pos.castling_mode() != CastlingMode::Standard {
            return Vec::new();
        }
        let raw = self.entries_for(pos);
        let total: u64 = raw.iter().map(|e| u64::from(e.weight)).sum();
        // Legal moves are generated once per call, not once per entry (D1122).
        let legal = legal_code_map(pos);
        raw.iter()
            .filter(|e| e.weight > 0)
            .filter_map(|e| {
                let mv = lookup_code(&legal, e.mv)?;
                Some(BookMove {
                    mv,
                    code: e.mv,
                    weight: e.weight,
                    percent: percent_of(u64::from(e.weight), total),
                })
            })
            .collect()
    }

    /// The book moves sorted by decreasing weight (stable, so ties keep the
    /// file order).
    pub fn moves_sorted_by_weight(&self, pos: &Position) -> Vec<BookMove> {
        let mut moves = self.moves(pos);
        moves.sort_by_key(|m| std::cmp::Reverse(m.weight));
        moves
    }

    /// Legal moves of `pos` whose resulting position has at least one entry
    /// in the book, whether or not the move itself is in the book (D1106).
    pub fn leading_to_book(&self, pos: &Position) -> Vec<Move> {
        if pos.castling_mode() != CastlingMode::Standard {
            return Vec::new();
        }
        pos.legal_moves()
            .into_iter()
            .filter(|m| {
                pos.after(*m)
                    .is_ok_and(|next| !self.entries_for(&next).is_empty())
            })
            .collect()
    }

    /// Picks a book move with probability proportional to its raw weight
    /// (D1107). `rng(bound)` must return a value in `0..bound` (`bound` is
    /// the sum of the weights, at least 1); larger values are reduced modulo
    /// `bound`. `None` when the position has no book move.
    pub fn pick(&self, pos: &Position, mut rng: impl FnMut(u64) -> u64) -> Option<BookMove> {
        let moves = self.moves(pos);
        let total: u64 = moves.iter().map(|m| u64::from(m.weight)).sum();
        if total == 0 {
            return None;
        }
        let mut target = rng(total) % total;
        for m in moves {
            let w = u64::from(m.weight);
            if target < w {
                return Some(m);
            }
            target -= w;
        }
        None
    }

    /// The heaviest book move; ties go to the first in file order.
    pub fn best(&self, pos: &Position) -> Option<BookMove> {
        self.moves(pos)
            .into_iter()
            .reduce(|best, m| if m.weight > best.weight { m } else { best })
    }

    /// Structural checks (D1113): order, trailing bytes, move `0`, weight `0`.
    pub fn validate(&self) -> ValidationReport {
        ValidationReport {
            entries: self.entries.len(),
            trailing_bytes: self.trailing_bytes,
            was_sorted: self.was_sorted,
            zero_moves: self.entries.iter().filter(|e| e.mv == 0).count(),
            zero_weights: self.entries.iter().filter(|e| e.weight == 0).count(),
        }
    }

    /// Raw move codes stored for `pos` that are not legal there.
    pub fn illegal_moves_in(&self, pos: &Position) -> Vec<u16> {
        let legal = legal_code_map(pos);
        self.entries_for(pos)
            .iter()
            .filter(|e| e.mv != 0 && lookup_code(&legal, e.mv).is_none())
            .map(|e| e.mv)
            .collect()
    }

    /// The file contents: sorted records, no trailing bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.entries.len().saturating_mul(RECORD_SIZE));
        for e in &self.entries {
            e.write_to(&mut out);
        }
        out
    }

    /// Writes the book to its own file atomically (D1110). Permissions and
    /// symlinks of the existing file are preserved (D1121). May fail with
    /// [`BookError::Io`] even when the book is not flagged read-only.
    pub fn save(&mut self) -> Result<(), BookError> {
        self.save_inner(None, None)
    }

    /// Writes the book to `path` atomically and binds the book to it (it
    /// stops being read-only).
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), BookError> {
        self.save_inner(Some(path.as_ref()), None)
    }

    /// Saves with a simulated failure. For tests only.
    #[doc(hidden)]
    pub fn save_with_fault(&mut self, fault: SaveFault) -> Result<(), BookError> {
        self.save_inner(None, Some(fault))
    }

    fn save_inner(&mut self, to: Option<&Path>, fault: Option<SaveFault>) -> Result<(), BookError> {
        let target = match to {
            Some(p) => p.to_path_buf(),
            None if self.read_only => return Err(BookError::ReadOnly),
            None => self.path.clone().ok_or(BookError::NoPath)?,
        };
        write_atomic(&target, &self.to_bytes(), fault)?;
        self.path = Some(target);
        self.read_only = false;
        self.was_sorted = true;
        self.trailing_bytes = 0;
        self.dirty = false;
        Ok(())
    }

    /// Fails with [`BookError::ReadOnly`] when editing is not allowed.
    pub(crate) fn ensure_writable(&self) -> Result<(), BookError> {
        if self.read_only {
            Err(BookError::ReadOnly)
        } else {
            Ok(())
        }
    }
}

impl Default for Book {
    fn default() -> Book {
        Book::new()
    }
}

/// `round(weight / total * 100)` with ties to even.
///
/// Scid computes `(double(score) / double(sum)) * 100.0` and prints it with
/// `%.0f`, which rounds the already-floating value half to even. Exact integer
/// arithmetic differs from that in a few cases (e.g. 23/40 is 57 in Scid), so
/// the same floating-point steps are reproduced for byte-level compatibility
/// of the displayed percentages (D1120).
fn percent_of(weight: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    // Weights are at most 0xfff0 and a position holds a bounded number of
    // entries, so both values are far below 2^53 and convert exactly.
    #[allow(clippy::cast_precision_loss)]
    let ratio = weight as f64 / total as f64;
    let rounded = (ratio * 100.0).round_ties_even();
    // Saturating float-to-int cast; the value is within 0..=100 anyway.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = rounded.clamp(0.0, 100.0) as u8;
    percent
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: u64, mv: u16, weight: u16) -> Entry {
        Entry {
            key,
            mv,
            weight,
            learn: 0,
        }
    }

    fn code(pos: &Position, uci: &str) -> u16 {
        let m = pos.parse_uci(uci).expect("legal");
        crate::codec::encode_move(m).expect("encodable")
    }

    fn start_book() -> (Position, Book) {
        let pos = Position::new();
        let key = polyglot_key(&pos);
        let book = Book::from_entries(vec![
            entry(key, code(&pos, "e2e4"), 30),
            entry(key, code(&pos, "d2d4"), 10),
            entry(key, 0, 5),
            entry(key, 0x0abc, 5), // not legal
        ]);
        (pos, book)
    }

    #[test]
    fn moves_keep_file_order_and_use_the_total_of_all_entries() {
        let (pos, book) = start_book();
        let moves = book.moves(&pos);
        assert_eq!(moves.len(), 2);
        assert_eq!(moves[0].weight, 30);
        // total = 30 + 10 + 5 + 5 = 50
        assert_eq!((moves[0].percent, moves[1].percent), (60, 20));
        assert_eq!(book.moves_sorted_by_weight(&pos)[0].weight, 30);
        assert_eq!(book.illegal_moves_in(&pos), vec![0x0abc]);
    }

    #[test]
    fn percent_rounds_half_to_even() {
        assert_eq!(percent_of(1, 8), 12); // 12.5 -> 12
        assert_eq!(percent_of(3, 8), 38); // 37.5 -> 38
        assert_eq!(percent_of(2, 3), 67);
        assert_eq!(percent_of(0, 0), 0);
        // Cases where exact integer rounding disagrees with Scid's `%.0f`.
        assert_eq!(percent_of(23, 40), 57);
        assert_eq!(percent_of(109, 200), 55);
    }

    #[test]
    fn pick_is_proportional_and_best_takes_the_heaviest() {
        let (pos, book) = start_book();
        assert_eq!(book.pick(&pos, |_| 0).map(|m| m.weight), Some(30));
        assert_eq!(book.pick(&pos, |_| 29).map(|m| m.weight), Some(30));
        assert_eq!(book.pick(&pos, |_| 30).map(|m| m.weight), Some(10));
        assert_eq!(book.pick(&pos, |_| 39).map(|m| m.weight), Some(10));
        assert_eq!(book.best(&pos).map(|m| m.weight), Some(30));
        assert!(Book::new().pick(&pos, |_| 0).is_none());
    }

    #[test]
    fn leading_to_book_lists_legal_moves_into_known_positions() {
        let pos = Position::new();
        let after = pos
            .after(pos.parse_uci("e2e4").expect("legal"))
            .expect("play");
        let book = Book::from_entries(vec![entry(polyglot_key(&after), 1, 1)]);
        let leading = book.leading_to_book(&pos);
        assert_eq!(leading, vec![pos.parse_uci("e2e4").expect("legal")]);
    }

    #[test]
    fn synthetic_book_round_trips_through_a_file() {
        let (pos, mut book) = start_book();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.bin");
        book.save_as(&path).expect("save");
        let reopened = Book::open(&path).expect("open");
        assert_eq!(reopened.entries(), book.entries());
        assert_eq!(reopened.moves(&pos), book.moves(&pos));
        assert!(reopened.was_sorted());
        assert_eq!(reopened.validate().zero_moves, 1);
    }

    #[test]
    fn open_rejects_short_files_and_ignores_trailing_bytes() {
        assert!(matches!(
            Book::from_bytes(&[0; 15]),
            Err(BookError::TooShort { len: 15 })
        ));
        assert!(Book::from_bytes(&[]).is_err());
        let (_, book) = start_book();
        let mut bytes = book.to_bytes();
        bytes.extend_from_slice(&[1, 2, 3]);
        let reopened = Book::from_bytes(&bytes).expect("parse");
        assert_eq!(reopened.len(), book.len());
        assert_eq!(reopened.validate().trailing_bytes, 3);
    }

    #[test]
    fn unsorted_input_is_sorted_stably_in_memory() {
        let book = Book::from_entries(vec![entry(9, 1, 1), entry(2, 1, 1), entry(2, 2, 2)]);
        assert!(!book.was_sorted());
        let keys: Vec<(u64, u16)> = book.entries().iter().map(|e| (e.key, e.mv)).collect();
        assert_eq!(keys, vec![(2, 1), (2, 2), (9, 1)]);
    }

    #[test]
    fn read_only_books_refuse_to_save() {
        let (_, mut book) = start_book();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.bin");
        book.save_as(&path).expect("save");
        let mut ro = Book::open_readonly(&path).expect("open");
        assert!(matches!(ro.save(), Err(BookError::ReadOnly)));
        assert!(matches!(Book::new().save(), Err(BookError::NoPath)));
    }

    #[test]
    fn dirty_flag_follows_edits_and_saves() {
        let (pos, mut book) = start_book();
        assert!(!book.is_dirty());
        let dir = tempfile::tempdir().expect("tempdir");
        book.save_as(dir.path().join("t.bin")).expect("save");
        assert!(!book.is_dirty());
        let e4 = pos.parse_uci("e2e4").expect("legal");
        book.set_position_moves(&pos, &[(e4, 3)]).expect("set");
        assert!(book.is_dirty());
        book.save().expect("save");
        assert!(!book.is_dirty());
    }
}
