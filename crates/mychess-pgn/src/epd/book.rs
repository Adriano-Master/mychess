//! The in-memory EPD book.

use std::collections::HashMap;

use mychess_core::{Game, Move, Position};

use super::record::{EpdOperation, EpdRecord, leading_int};

/// A stable handle to a record of an [`EpdBook`].
///
/// Removing other records does not invalidate it; it is only meaningful for
/// the book that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EpdIndex(usize);

impl EpdIndex {
    /// The position of the record in file order, counting removed records
    /// too.
    #[cfg(test)]
    pub(crate) fn get(self) -> usize {
        self.0
    }
}

/// What [`EpdBook::insert`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertResult {
    /// A new record was appended.
    Added,
    /// The position existed: its operations were replaced, keeping its place.
    Replaced,
}

/// An ordered collection of EPD records keyed by position.
///
/// Positions are compared as in [`Position`]'s `==` (board, side to move,
/// castling rights, en passant square; clocks ignored). Records stay in
/// insertion (file) order; a repeated position replaces the data of the
/// earlier record and keeps its place.
#[derive(Debug, Clone, Default)]
pub struct EpdBook {
    slots: Vec<Option<EpdRecord>>,
    index: HashMap<Position, usize>,
    modified: bool,
    read_only: bool,
}

impl PartialEq for EpdBook {
    /// Equal when both hold the same records in the same order (flags and
    /// removed slots are ignored).
    fn eq(&self, other: &EpdBook) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl Eq for EpdBook {}

impl<'a> IntoIterator for &'a EpdBook {
    type Item = &'a EpdRecord;
    type IntoIter = std::iter::Flatten<std::slice::Iter<'a, Option<EpdRecord>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.slots.iter().flatten()
    }
}

impl EpdBook {
    /// An empty book.
    pub fn new() -> EpdBook {
        EpdBook::default()
    }

    /// The number of distinct positions.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// True when the book holds no record.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// True after `insert`, `remove`, `strip_opcode`, `sort_operations` or
    /// `get_mut` changed the book; reading a file leaves it `false`.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Clears the modified flag (after saving).
    pub fn clear_modified(&mut self) {
        self.modified = false;
    }

    /// Whether the book was opened read-only. This is policy of whoever
    /// opens the file; the book does not enforce it.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Sets the read-only flag.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    /// Whether the position is in the book.
    pub fn contains(&self, position: &Position) -> bool {
        self.index.contains_key(position)
    }

    /// The handle of the record for this position.
    pub fn index_of(&self, position: &Position) -> Option<EpdIndex> {
        self.index.get(position).copied().map(EpdIndex)
    }

    /// The record for this position.
    pub fn get(&self, position: &Position) -> Option<&EpdRecord> {
        self.get_by_index(self.index_of(position)?)
    }

    /// The record for this position, for editing its operations. Marks the
    /// book as modified as soon as the record is found, whether or not the
    /// caller then changes it.
    pub fn get_mut(&mut self, position: &Position) -> Option<&mut EpdRecord> {
        let i = *self.index.get(position)?;
        self.modified = true;
        self.slots.get_mut(i)?.as_mut()
    }

    /// The record behind a handle; `None` if it was removed.
    pub fn get_by_index(&self, index: EpdIndex) -> Option<&EpdRecord> {
        self.slots.get(index.0)?.as_ref()
    }

    /// The records in file order.
    pub fn iter(&self) -> impl Iterator<Item = &EpdRecord> + '_ {
        self.slots.iter().flatten()
    }

    /// The records in file order, with their handles.
    pub fn iter_indexed(&self) -> impl Iterator<Item = (EpdIndex, &EpdRecord)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|r| (EpdIndex(i), r)))
    }

    /// Adds a record or, if the position exists, replaces its operations
    /// (the last entry wins) keeping the original place.
    pub fn insert(&mut self, position: Position, operations: Vec<EpdOperation>) -> InsertResult {
        self.modified = true;
        if self.insert_inner(position, operations).1 {
            InsertResult::Added
        } else {
            InsertResult::Replaced
        }
    }

    /// Inserts without touching the modified flag; returns the slot and
    /// whether it is new.
    pub(crate) fn insert_inner(
        &mut self,
        position: Position,
        operations: Vec<EpdOperation>,
    ) -> (usize, bool) {
        if let Some(&i) = self.index.get(&position) {
            if let Some(slot) = self.slots.get_mut(i) {
                *slot = Some(EpdRecord::with_operations(position, operations));
            }
            (i, false)
        } else {
            let i = self.slots.len();
            self.index.insert(position.clone(), i);
            self.slots
                .push(Some(EpdRecord::with_operations(position, operations)));
            (i, true)
        }
    }

    /// Adds the position with no operations if it is not yet in the book.
    /// Returns whether it was added.
    pub fn append_position(&mut self, position: &Position) -> bool {
        if self.contains(position) {
            return false;
        }
        self.insert(position.clone(), Vec::new());
        true
    }

    /// Removes the record of this position. The other handles stay valid.
    /// The slot stays as a hole (and counts in [`EpdBook::compact`]'s
    /// result); call [`EpdBook::compact`] to reclaim the holes.
    pub fn remove(&mut self, position: &Position) -> Option<EpdRecord> {
        let i = self.index.remove(position)?;
        self.modified = true;
        self.slots.get_mut(i)?.take()
    }

    /// Removes the record at `index`; like [`EpdBook::remove`] but by handle.
    /// `None` for a handle that is out of range or already removed.
    pub fn remove_at(&mut self, index: EpdIndex) -> Option<EpdRecord> {
        let record = self.slots.get_mut(index.0)?.take()?;
        self.index.remove(record.position());
        self.modified = true;
        Some(record)
    }

    /// Reclaims the holes left by [`remove`](EpdBook::remove), keeping the
    /// file order. Handles of records that moved become stale; the result
    /// lists `(old, new)` for each record whose handle changed. Does not mark
    /// the book as modified (the contents are the same).
    pub fn compact(&mut self) -> Vec<(EpdIndex, EpdIndex)> {
        let mut moved = Vec::new();
        let old = std::mem::take(&mut self.slots);
        for (i, slot) in old.into_iter().enumerate() {
            let Some(record) = slot else { continue };
            let new = self.slots.len();
            if new != i {
                moved.push((EpdIndex(i), EpdIndex(new)));
            }
            if let Some(entry) = self.index.get_mut(record.position()) {
                *entry = new;
            }
            self.slots.push(Some(record));
        }
        moved
    }

    /// Removes **every** operation with this opcode from all records;
    /// returns how many records changed.
    pub fn strip_opcode(&mut self, opcode: &str) -> usize {
        let changed = self
            .slots
            .iter_mut()
            .flatten()
            .map(|r| r.remove_opcode(opcode))
            .filter(|&n| n > 0)
            .count();
        if changed > 0 {
            self.modified = true;
        }
        changed
    }

    /// Sorts the operations of every record by the bytes of the text
    /// `opcode operands` (stable). Returns how many records changed order.
    pub fn sort_operations(&mut self) -> usize {
        let mut changed = 0;
        for record in self.slots.iter_mut().flatten() {
            let ops = record.operations_mut();
            if !ops.is_sorted_by(|a, b| a.sort_cmp(b).is_le()) {
                ops.sort_by(EpdOperation::sort_cmp);
                changed += 1;
            }
        }
        if changed > 0 {
            self.modified = true;
        }
        changed
    }

    /// The handle of the last record, where the Scid's cursor starts so the
    /// first [`next`](EpdBook::next) reaches the first record.
    pub fn last_index(&self) -> Option<EpdIndex> {
        self.slots.iter().rposition(Option::is_some).map(EpdIndex)
    }

    /// The next live record after `from`, wrapping around. `None` only for an
    /// empty book.
    pub fn next(&self, from: EpdIndex) -> Option<EpdIndex> {
        let n = self.slots.len();
        (1..=n)
            .map(|step| (from.0.checked_add(step)).map_or(0, |v| v % n))
            .find(|&i| self.slots.get(i).is_some_and(Option::is_some))
            .map(EpdIndex)
    }

    /// The previous live record before `from`, wrapping around. `None` only
    /// for an empty book.
    pub fn prev(&self, from: EpdIndex) -> Option<EpdIndex> {
        let n = self.slots.len();
        if n == 0 {
            return None;
        }
        let start = from.0 % n;
        (1..=n)
            .map(|step| (start + n - (step % n)) % n)
            .find(|&i| self.slots.get(i).is_some_and(Option::is_some))
            .map(EpdIndex)
    }

    /// The greatest ply of the main line of `game` whose position is in the
    /// book (ply 0 is the start position); `None` when none is.
    pub fn deepest_match(&self, game: &Game) -> Option<u32> {
        let mut best = self.contains(game.start_position()).then_some(0);
        for (i, id) in game.mainline().enumerate() {
            let Some(ply) = u32::try_from(i).ok().and_then(|p| p.checked_add(1)) else {
                break;
            };
            if game.position_at(id).is_ok_and(|p| self.contains(p)) {
                best = Some(ply);
            }
        }
        best
    }

    /// The legal moves in `position` whose resulting position is in the book.
    pub fn moves_in_book(&self, position: &Position) -> Vec<Move> {
        position
            .legal_moves()
            .into_iter()
            .filter(|&m| position.after(m).is_ok_and(|p| self.contains(&p)))
            .collect()
    }

    /// A one-line summary of the record, as the Scid's information panel
    /// shows: the `ce` evaluation in pawns seen from White (`+0.25`), or else
    /// the operands of the first of `eco`, `nic`, `pv`, `pm`, `bm`, `id`
    /// present with non-empty operands. A `ce` that is not a number counts as
    /// 0 (`+0.00`), as in the Scid.
    pub fn summary(&self, position: &Position) -> Option<String> {
        let record = self.get(position)?;
        if let Some(op) = record
            .operations_named("ce")
            .find(|o| !o.operands.is_empty())
        {
            // Like the Scid's `strGetInteger`: no digits reads as 0.
            let cp = leading_int(&op.operands).unwrap_or(0);
            let cp = if position.turn() == mychess_core::Color::Black {
                cp.saturating_neg()
            } else {
                cp
            };
            let sign = if cp < 0 { '-' } else { '+' };
            let abs = cp.unsigned_abs();
            return Some(format!("{sign}{}.{:02}", abs / 100, abs % 100));
        }
        ["eco", "nic", "pv", "pm", "bm", "id"]
            .iter()
            .find_map(|code| {
                record
                    .operations_named(code)
                    .find(|o| !o.operands.is_empty())
            })
            .map(|op| op.operands.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mychess_core::SanMode;

    fn after(moves: &str) -> Position {
        let mut p = Position::new();
        for san in moves.split_whitespace() {
            let m = p.parse_san(san, SanMode::Strict).expect("legal").mv;
            p = p.after(m).expect("legal");
        }
        p
    }

    fn op(o: &str, a: &str) -> EpdOperation {
        EpdOperation::new(o, a)
    }

    #[test]
    fn last_entry_wins_keeping_the_first_place() {
        let mut b = EpdBook::new();
        let (p1, p2) = (after("e4"), after("d4"));
        assert_eq!(
            b.insert(p1.clone(), vec![op("id", "1")]),
            InsertResult::Added
        );
        assert_eq!(b.insert(p2.clone(), vec![]), InsertResult::Added);
        assert_eq!(
            b.insert(p1.clone(), vec![op("id", "3")]),
            InsertResult::Replaced
        );
        assert_eq!(b.len(), 2);
        let ids: Vec<_> = b.iter().map(|r| r.operations().len()).collect();
        assert_eq!(ids, [1, 0]);
        assert_eq!(b.get(&p1).and_then(EpdRecord::id), Some("3"));
        assert_eq!(b.index_of(&p1).map(EpdIndex::get), Some(0));
    }

    #[test]
    fn clocks_do_not_matter_for_identity() {
        let mut b = EpdBook::new();
        let a = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 5 9")
            .expect("fen");
        b.insert(a, vec![]);
        assert!(b.contains(&Position::new()));
    }

    #[test]
    fn remove_keeps_other_indices_and_navigation_wraps() {
        let mut b = EpdBook::new();
        let ps = [after("e4"), after("d4"), after("c4"), after("Nf3")];
        for p in &ps {
            b.insert(p.clone(), vec![]);
        }
        let i3 = b.index_of(&ps[3]).expect("in book");
        assert!(b.remove(&ps[1]).is_some());
        assert!(b.remove(&ps[1]).is_none());
        assert_eq!(b.len(), 3);
        assert_eq!(b.index_of(&ps[3]), Some(i3));
        assert_eq!(b.iter().count(), 3);
        // Cursor starts at the last record; the first next reaches the first.
        let last = b.last_index().expect("non-empty");
        assert_eq!(last, i3);
        let first = b.next(last).expect("wraps");
        assert_eq!(first.get(), 0);
        let second = b.next(first).expect("skips the hole");
        assert_eq!(second.get(), 2);
        assert_eq!(b.prev(second), Some(first));
        assert_eq!(b.prev(first), Some(last));
        // Navigating from a removed handle works too.
        assert_eq!(b.next(EpdIndex(1)), Some(second));
        assert_eq!(b.prev(EpdIndex(1)), Some(first));
        assert_eq!(EpdBook::new().next(EpdIndex(0)), None);
        assert_eq!(EpdBook::new().prev(EpdIndex(0)), None);
        assert_eq!(EpdBook::new().last_index(), None);
    }

    #[test]
    fn strip_opcode_removes_all_occurrences_and_counts_records() {
        let mut b = EpdBook::new();
        b.insert(
            after("e4"),
            vec![op("noop", ""), op("id", "x"), op("noop", "")],
        );
        b.insert(after("d4"), vec![op("id", "y")]);
        b.insert(after("c4"), vec![op("noop", "")]);
        b.clear_modified();
        assert_eq!(b.strip_opcode("noop"), 2);
        assert!(b.is_modified());
        assert_eq!(b.iter().map(|r| r.operations().len()).sum::<usize>(), 2);
        b.clear_modified();
        assert_eq!(b.strip_opcode("noop"), 0);
        assert!(!b.is_modified());
    }

    #[test]
    fn sort_operations_by_bytes() {
        let mut b = EpdBook::new();
        b.insert(
            after("e4"),
            vec![
                op("id", "z"),
                op("bm", "e4"),
                op("c0", "\"x\""),
                op("bm", "d4"),
            ],
        );
        b.insert(after("d4"), vec![op("a", ""), op("b", "")]);
        assert_eq!(b.sort_operations(), 1);
        let ops: Vec<_> = b
            .get(&after("e4"))
            .expect("record")
            .operations()
            .iter()
            .map(|o| format!("{} {}", o.opcode, o.operands))
            .collect();
        assert_eq!(ops, ["bm d4", "bm e4", "c0 \"x\"", "id z"]);
        assert_eq!(b.sort_operations(), 0);
    }

    #[test]
    fn append_position_only_when_new() {
        let mut b = EpdBook::new();
        assert!(b.append_position(&Position::new()));
        assert!(!b.append_position(&Position::new()));
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn deepest_match_and_moves_in_book() {
        let mut b = EpdBook::new();
        b.insert(after("e4"), vec![]);
        b.insert(after("e4 e5"), vec![]);
        b.insert(after("d4"), vec![]);
        let mut game = Game::new();
        let mut node = game.root();
        let mut p = Position::new();
        for san in ["e4", "e5", "Nf3"] {
            let m = p.parse_san(san, SanMode::Strict).expect("legal").mv;
            node = game.add_move(node, m).expect("added");
            p = p.after(m).expect("legal");
        }
        assert_eq!(b.deepest_match(&game), Some(2));
        assert_eq!(EpdBook::new().deepest_match(&game), None);
        let mut with_start = EpdBook::new();
        with_start.append_position(&Position::new());
        assert_eq!(with_start.deepest_match(&game), Some(0));
        assert_eq!(b.moves_in_book(&Position::new()).len(), 2);
        assert_eq!(b.moves_in_book(&after("e4")).len(), 1);
        assert_eq!(b.moves_in_book(&after("c4")).len(), 0);
    }

    #[test]
    fn summary_rules() {
        let mut book = EpdBook::new();
        let white = Position::new();
        let black = after("e4");
        book.insert(white.clone(), vec![op("ce", "25"), op("id", "\"x\"")]);
        book.insert(black.clone(), vec![op("ce", "25")]);
        assert_eq!(book.summary(&white).as_deref(), Some("+0.25"));
        assert_eq!(book.summary(&black).as_deref(), Some("-0.25"));
        let d4 = after("d4");
        book.insert(
            d4.clone(),
            vec![op("id", "\"i\""), op("bm", "d5"), op("eco", "A40")],
        );
        assert_eq!(book.summary(&d4).as_deref(), Some("A40"));
        book.get_mut(&d4).expect("record").remove_opcode("eco");
        assert_eq!(book.summary(&d4).as_deref(), Some("d5"));
        let c4 = after("c4");
        book.insert(c4.clone(), vec![op("noop", "")]);
        assert_eq!(book.summary(&c4), None);
        assert_eq!(book.summary(&after("a3")), None);
        let b3 = after("b3");
        book.insert(b3.clone(), vec![op("ce", "0")]);
        assert_eq!(book.summary(&b3).as_deref(), Some("+0.00"));
        // Non-numeric `ce` is 0 as in the Scid; empty operands are skipped.
        let a3 = after("a3");
        book.insert(a3.clone(), vec![op("ce", "abc")]);
        assert_eq!(book.summary(&a3).as_deref(), Some("+0.00"));
        let h3 = after("h3");
        book.insert(h3.clone(), vec![op("eco", ""), op("id", "\"z\"")]);
        assert_eq!(book.summary(&h3).as_deref(), Some("\"z\""));
    }

    #[test]
    fn remove_then_insert_same_position_goes_to_the_end() {
        let mut b = EpdBook::new();
        let (p1, p2) = (after("e4"), after("d4"));
        b.insert(p1.clone(), vec![op("id", "1")]);
        b.insert(p2.clone(), vec![]);
        b.remove(&p1);
        assert_eq!(
            b.insert(p1.clone(), vec![op("id", "2")]),
            InsertResult::Added
        );
        assert_eq!(b.len(), 2);
        assert_eq!(b.index_of(&p1).map(EpdIndex::get), Some(2));
        let order: Vec<_> = (&b).into_iter().map(|r| r.position().clone()).collect();
        assert_eq!(order, [p2, p1]);
    }

    #[test]
    fn compact_reclaims_holes_and_reports_moves() {
        let mut b = EpdBook::new();
        let ps = [after("e4"), after("d4"), after("c4"), after("Nf3")];
        for p in &ps {
            b.insert(p.clone(), vec![]);
        }
        b.remove(&ps[0]);
        b.remove(&ps[2]);
        b.clear_modified();
        let moved = b.compact();
        assert_eq!(
            moved,
            [(EpdIndex(1), EpdIndex(0)), (EpdIndex(3), EpdIndex(1))]
        );
        assert!(!b.is_modified());
        assert_eq!(b.len(), 2);
        assert_eq!(b.index_of(&ps[1]), Some(EpdIndex(0)));
        assert_eq!(b.index_of(&ps[3]), Some(EpdIndex(1)));
        assert_eq!(b.last_index(), Some(EpdIndex(1)));
        assert_eq!(b.compact().len(), 0);
    }
}
