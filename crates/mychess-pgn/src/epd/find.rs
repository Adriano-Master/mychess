//! Text search over the records of an [`EpdBook`] (spec M7-5a, D3813).

use std::ops::Range;

use super::book::{EpdBook, EpdIndex};
use super::record::EpdRecord;

impl EpdBook {
    /// Searches the records for `needle`, ignoring case, and returns the
    /// first record that contains it with the byte range of the match.
    ///
    /// The searched text of a record (also the text the range refers to) is
    /// the four-field FEN, then, for each operation, `\n` and
    /// `opcode operands` (the lines of [`EpdRecord::operations_text`]), see
    /// [`EpdRecord::search_text`]. A match never spans two characters and
    /// the range lies on character boundaries.
    ///
    /// The search starts at the record **after** `from` (or the first record,
    /// or the last when searching backwards, if `from` is `None`), goes
    /// forward or backward and wraps around, so `from` itself is the last
    /// one examined. An empty `needle`, an empty book or no match gives
    /// `None`. Case folding is per character (a character whose lowercase
    /// form is not a single character is compared as is).
    ///
    /// A `from` that was removed (or never existed) is invalid and the walk
    /// restarts at the first record (forward) or the last (backward): after a
    /// [`remove_at`](EpdBook::remove_at) the caller must pass the live
    /// neighbour of the removed record (for instance the record that followed
    /// it) to continue from the same place.
    ///
    /// Scid searches with a Tcl regular expression and loads each position
    /// on the way; this is a plain substring search that touches nothing.
    ///
    /// ```
    /// use mychess_core::Position;
    /// use mychess_pgn::epd::{EpdBook, EpdOperation};
    ///
    /// let mut book = EpdBook::new();
    /// book.insert(Position::new(), vec![EpdOperation::new("id", "\"WAC.001\"")]);
    /// let (index, range) = book.find_text(None, "wac", true).expect("found");
    /// let record = book.get_by_index(index).expect("record");
    /// assert_eq!(&record.search_text()[range], "WAC");
    /// ```
    pub fn find_text(
        &self,
        from: Option<EpdIndex>,
        needle: &str,
        forward: bool,
    ) -> Option<(EpdIndex, Range<usize>)> {
        let needle: Vec<char> = needle.chars().map(fold).collect();
        if needle.is_empty() {
            return None;
        }
        // Where the walk starts: `step` from here reaches the first candidate.
        let mut cur = match from {
            Some(i) if self.get_by_index(i).is_some() => i,
            _ if forward => self.last_index()?,
            _ => self.next(self.last_index()?)?,
        };
        for _ in 0..self.len() {
            cur = if forward {
                self.next(cur)?
            } else {
                self.prev(cur)?
            };
            let record = self.get_by_index(cur)?;
            if let Some(range) = find_folded(&record.search_text(), &needle) {
                return Some((cur, range));
            }
        }
        None
    }
}

impl EpdRecord {
    /// The text [`EpdBook::find_text`] searches: FEN, then
    /// `\n` + `opcode operands` per operation.
    pub fn search_text(&self) -> String {
        let fen = self.position().fen();
        let mut out = String::new();
        let mut fields = fen.split_ascii_whitespace().take(4);
        if let Some(first) = fields.next() {
            out.push_str(first);
        }
        for f in fields {
            out.push(' ');
            out.push_str(f);
        }
        if !self.operations().is_empty() {
            out.push('\n');
            out.push_str(self.operations_text().trim_end_matches('\n'));
        }
        out
    }
}

fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// First occurrence of the folded `needle` in `hay`, as a byte range.
fn find_folded(hay: &str, needle: &[char]) -> Option<Range<usize>> {
    let chars: Vec<(usize, char)> = hay.char_indices().collect();
    let last_start = chars.len().checked_sub(needle.len())?;
    for start in 0..=last_start {
        let window = chars.get(start..start + needle.len())?;
        if window.iter().zip(needle).all(|(&(_, c), &n)| fold(c) == n) {
            let begin = window.first()?.0;
            let end = chars
                .get(start + needle.len())
                .map_or(hay.len(), |&(i, _)| i);
            return Some(begin..end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epd::EpdOperation;
    use mychess_core::{Position, SanMode};

    fn after(sans: &str) -> Position {
        let mut p = Position::new();
        for s in sans.split_whitespace() {
            let m = p.parse_san(s, SanMode::Strict).expect("san").mv;
            p = p.after(m).expect("legal");
        }
        p
    }

    /// Three records: start (`id "Alpha"`), 1.e4 (`bm Nf6`), 1.d4 (`c0 "Zürich"`).
    fn book() -> (EpdBook, [EpdIndex; 3]) {
        let mut b = EpdBook::new();
        b.insert(Position::new(), vec![EpdOperation::new("id", "\"Alpha\"")]);
        b.insert(after("e4"), vec![EpdOperation::new("bm", "Nf6")]);
        b.insert(after("d4"), vec![EpdOperation::new("c0", "\"Zürich\"")]);
        let ix = [
            b.index_of(&Position::new()).expect("0"),
            b.index_of(&after("e4")).expect("1"),
            b.index_of(&after("d4")).expect("2"),
        ];
        (b, ix)
    }

    fn matched(b: &EpdBook, i: EpdIndex, r: Range<usize>) -> String {
        let rec = b.get_by_index(i).expect("record");
        rec.search_text()[r].to_owned()
    }

    #[test]
    fn finds_ignoring_case_and_returns_the_matched_slice() {
        let (b, ix) = book();
        let (i, r) = b.find_text(None, "ALPHA", true).expect("found");
        assert_eq!(i, ix[0]);
        assert_eq!(matched(&b, i, r), "Alpha");
        let (i, r) = b.find_text(None, "bm nf", true).expect("found");
        assert_eq!(i, ix[1]);
        assert_eq!(matched(&b, i, r), "bm Nf");
    }

    #[test]
    fn searches_the_fen_too() {
        let (b, ix) = book();
        let (i, _) = b.find_text(None, "4P3", true).expect("found");
        assert_eq!(i, ix[1]);
    }

    #[test]
    fn starts_after_from_and_wraps_around() {
        let (b, ix) = book();
        // "rnbqkbnr" is in every record: next one after `from`.
        assert_eq!(
            b.find_text(Some(ix[0]), "rnbqkbnr", true).map(|m| m.0),
            Some(ix[1])
        );
        assert_eq!(
            b.find_text(Some(ix[2]), "rnbqkbnr", true).map(|m| m.0),
            Some(ix[0])
        );
        // Only `from` matches: found after a full turn.
        assert_eq!(
            b.find_text(Some(ix[0]), "alpha", true).map(|m| m.0),
            Some(ix[0])
        );
    }

    #[test]
    fn backward_walks_in_reverse_with_wrap() {
        let (b, ix) = book();
        assert_eq!(
            b.find_text(Some(ix[2]), "rnbqkbnr", false).map(|m| m.0),
            Some(ix[1])
        );
        assert_eq!(
            b.find_text(Some(ix[0]), "rnbqkbnr", false).map(|m| m.0),
            Some(ix[2])
        );
        assert_eq!(
            b.find_text(None, "rnbqkbnr", false).map(|m| m.0),
            Some(ix[2])
        );
    }

    #[test]
    fn empty_needle_empty_book_and_no_hit_give_none() {
        let (b, ix) = book();
        assert!(b.find_text(None, "", true).is_none());
        assert!(b.find_text(Some(ix[0]), "zzzz", true).is_none());
        assert!(EpdBook::new().find_text(None, "a", true).is_none());
    }

    #[test]
    fn unicode_ranges_stay_on_char_boundaries() {
        let (b, _) = book();
        let (i, r) = b.find_text(None, "ZÜRICH", true).expect("found");
        let text = b.get_by_index(i).expect("record").search_text();
        assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
        assert_eq!(&text[r], "Zürich");
        let (i, r) = b.find_text(None, "ü", true).expect("found");
        let text = b.get_by_index(i).expect("record").search_text();
        assert_eq!(&text[r], "ü");
    }

    #[test]
    fn skips_holes_left_by_removed_records() {
        let (mut b, ix) = book();
        b.remove(&after("e4"));
        assert_eq!(
            b.find_text(Some(ix[0]), "rnbqkbnr", true).map(|m| m.0),
            Some(ix[2])
        );
    }
}
