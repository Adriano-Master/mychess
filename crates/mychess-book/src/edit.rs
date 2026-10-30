//! Editing a book in memory: weights, moves and lines (spec D1108, D1109).
//!
//! Scid: `sc_book` (`tkscid.cpp`) rewrites the file after every position;
//! here edits only change memory and one [`Book::save`] writes the file.

use std::collections::HashSet;

use mychess_core::{CastlingMode, Move, Position};

use crate::book::Book;
use crate::codec::{encode_move, legal_code_map, lookup_code};
use crate::error::BookError;
use crate::format::{Entry, MAX_WEIGHT, key_range};
use crate::zobrist::polyglot_key;

/// Most moves one position may carry through [`Book::set_position_moves`].
pub const MAX_MOVES_PER_POSITION: usize = 100;

/// Converts percentages to absolute weights in the Scid scale (`0xfff0`).
///
/// `weight = prob * 0xfff0 / prob_max`; a probability of `0` becomes weight
/// `1`; when the largest probability is `0` the coefficient is 1.
pub fn weights_from_percent(moves: &[(Move, u8)]) -> Vec<(Move, u16)> {
    let prob_max = moves.iter().map(|(_, p)| u32::from(*p)).max().unwrap_or(0);
    moves
        .iter()
        .map(|(m, p)| {
            // `prob_max == 0` implies every probability is 0 (weight 1); the
            // `max(1)` is the coefficient 1 of Scid and avoids a division by 0.
            let weight = if *p == 0 {
                1
            } else {
                u32::from(*p) * u32::from(MAX_WEIGHT) / prob_max.max(1)
            };
            (
                *m,
                u16::try_from(weight.clamp(1, u32::from(MAX_WEIGHT))).unwrap_or(1),
            )
        })
        .collect()
}

fn check_weight(weight: u16) -> Result<(), BookError> {
    match weight {
        0 => Err(BookError::ZeroWeight),
        w if w > MAX_WEIGHT => Err(BookError::WeightTooLarge {
            weight: u32::from(w),
        }),
        _ => Ok(()),
    }
}

fn check_standard(pos: &Position) -> Result<(), BookError> {
    if pos.castling_mode() == CastlingMode::Standard {
        Ok(())
    } else {
        Err(BookError::UnsupportedPosition)
    }
}

impl Book {
    /// Replaces **all** moves of `pos` with `moves` (absolute weights
    /// `1..=0xfff0`). A move left out is removed, a new one is inserted, the
    /// rewritten entries lose their `learn` data, other positions are
    /// untouched and the book stays sorted (D1108).
    ///
    /// The replacement also erases the key's entries that are illegal, have
    /// move `0` or weight `0`, because it replaces the whole key range; use
    /// [`Book::purge_invalid`] to remove only those. An illegal move reports
    /// its index in `moves` as [`BookError::IllegalMove::ply`](BookError)
    /// (D1123).
    pub fn set_position_moves(
        &mut self,
        pos: &Position,
        moves: &[(Move, u16)],
    ) -> Result<(), BookError> {
        self.ensure_writable()?;
        check_standard(pos)?;
        if moves.len() > MAX_MOVES_PER_POSITION {
            return Err(BookError::TooManyMoves { count: moves.len() });
        }
        let key = polyglot_key(pos);
        let legal = legal_code_map(pos);
        let mut fresh: Vec<Entry> = Vec::with_capacity(moves.len());
        for (index, (m, weight)) in moves.iter().enumerate() {
            check_weight(*weight)?;
            let code = encode_move(*m)
                .filter(|c| lookup_code(&legal, *c) == Some(*m))
                .ok_or(BookError::IllegalMove { ply: index })?;
            if fresh.iter().any(|e| e.mv == code) {
                return Err(BookError::DuplicateMove);
            }
            fresh.push(Entry {
                key,
                mv: code,
                weight: *weight,
                learn: 0,
            });
        }
        let range = key_range(&self.entries, key);
        self.entries.splice(range, fresh);
        self.dirty = true;
        Ok(())
    }

    /// Removes every entry of `pos` (all its moves, legal or not). Returns
    /// how many were removed.
    pub fn remove_position(&mut self, pos: &Position) -> Result<usize, BookError> {
        self.ensure_writable()?;
        check_standard(pos)?;
        let range = key_range(&self.entries, polyglot_key(pos));
        let removed = range.len();
        self.entries.drain(range);
        self.dirty |= removed > 0;
        Ok(removed)
    }

    /// Removes only the entries of `pos` that are unusable: move `0`, weight
    /// `0` or a move that is not legal there (D1123). Valid entries keep
    /// their order and `learn` data. Returns how many were removed.
    pub fn purge_invalid(&mut self, pos: &Position) -> Result<usize, BookError> {
        self.ensure_writable()?;
        check_standard(pos)?;
        let legal = legal_code_map(pos);
        let range = key_range(&self.entries, polyglot_key(pos));
        let kept: Vec<Entry> = self
            .entries
            .get(range.clone())
            .unwrap_or_default()
            .iter()
            .filter(|e| e.weight > 0 && lookup_code(&legal, e.mv).is_some())
            .copied()
            .collect();
        let removed = range.len() - kept.len();
        self.entries.splice(range, kept);
        self.dirty |= removed > 0;
        Ok(removed)
    }

    /// Adds the moves of `line`, played from `from`, with `weight` each
    /// (D1109). A move already in the book keeps its weight (D1116). The line
    /// is validated first: on error nothing changes.
    pub fn add_line(
        &mut self,
        line: &[Move],
        from: &Position,
        weight: u16,
    ) -> Result<usize, BookError> {
        self.add_lines_inner(std::iter::once((line, weight)), from)
    }

    /// Adds several lines, all played from `from`, in one pass (D1122): the
    /// book is reordered once instead of once per move. Same semantics as
    /// calling [`Book::add_line`] for each line in order (an existing move
    /// keeps its weight; the first of two equal new moves wins), except that
    /// every line is validated before anything changes. An error reports the
    /// ply within the offending line. Returns the number of entries added.
    pub fn add_lines(
        &mut self,
        lines: &[(Vec<Move>, u16)],
        from: &Position,
    ) -> Result<usize, BookError> {
        self.add_lines_inner(lines.iter().map(|(l, w)| (l.as_slice(), *w)), from)
    }

    fn add_lines_inner<'a>(
        &mut self,
        lines: impl Iterator<Item = (&'a [Move], u16)>,
        from: &Position,
    ) -> Result<usize, BookError> {
        self.ensure_writable()?;
        let mut planned = Vec::new();
        for (line, weight) in lines {
            check_weight(weight)?;
            planned.push((walk_line(line, from)?, weight));
        }
        let mut pending: HashSet<(u64, u16)> = HashSet::new();
        let mut fresh: Vec<Entry> = Vec::new();
        for (steps, weight) in planned {
            for (key, code) in steps {
                let exists = self
                    .entries
                    .get(key_range(&self.entries, key))
                    .is_some_and(|r| r.iter().any(|e| e.mv == code));
                if !exists && pending.insert((key, code)) {
                    fresh.push(Entry {
                        key,
                        mv: code,
                        weight,
                        learn: 0,
                    });
                }
            }
        }
        let added = fresh.len();
        if added > 0 {
            // Stable sort: existing entries of a key stay before the new ones.
            self.entries.extend(fresh);
            self.entries.sort_by_key(|e| e.key);
            self.dirty = true;
        }
        Ok(added)
    }

    /// Removes the moves of `line`, played from `from`, from their positions
    /// (siblings stay). Returns how many entries were removed.
    pub fn remove_line(&mut self, line: &[Move], from: &Position) -> Result<usize, BookError> {
        self.ensure_writable()?;
        let steps = walk_line(line, from)?;
        let mut removed = 0;
        for (key, code) in steps {
            let range = key_range(&self.entries, key);
            let kept: Vec<Entry> = self
                .entries
                .get(range.clone())
                .unwrap_or_default()
                .iter()
                .filter(|e| e.mv != code)
                .copied()
                .collect();
            removed += range.len() - kept.len();
            self.entries.splice(range, kept);
        }
        self.dirty |= removed > 0;
        Ok(removed)
    }
}

/// Plays `line` from `from`, returning `(key, code)` for every move.
fn walk_line(line: &[Move], from: &Position) -> Result<Vec<(u64, u16)>, BookError> {
    check_standard(from)?;
    let mut pos = from.clone();
    let mut steps = Vec::with_capacity(line.len());
    for (ply, m) in line.iter().enumerate() {
        let legal = legal_code_map(&pos);
        let code = encode_move(*m)
            .filter(|c| lookup_code(&legal, *c) == Some(*m))
            .ok_or(BookError::IllegalMove { ply })?;
        steps.push((polyglot_key(&pos), code));
        pos.play(*m).map_err(|_| BookError::IllegalMove { ply })?;
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(pos: &Position, uci: &str) -> Move {
        pos.parse_uci(uci).expect("legal")
    }

    #[test]
    fn set_position_moves_replaces_and_keeps_others() {
        let pos = Position::new();
        let after = pos.after(mv(&pos, "e2e4")).expect("play");
        let mut book = Book::new();
        book.set_position_moves(&after, &[(mv(&after, "e7e5"), 7)])
            .expect("set");
        book.set_position_moves(&pos, &[(mv(&pos, "e2e4"), 10), (mv(&pos, "d2d4"), 5)])
            .expect("set");
        book.set_position_moves(&pos, &[(mv(&pos, "d2d4"), 9)])
            .expect("replace");
        assert_eq!(book.len(), 2);
        assert_eq!(book.moves(&pos).len(), 1);
        assert_eq!(book.moves(&after)[0].weight, 7);
        assert!(book.validate().was_sorted);
    }

    #[test]
    fn invalid_updates_are_rejected() {
        let pos = Position::new();
        let e4 = mv(&pos, "e2e4");
        let mut book = Book::new();
        assert!(matches!(
            book.set_position_moves(&pos, &[(e4, 0)]),
            Err(BookError::ZeroWeight)
        ));
        assert!(matches!(
            book.set_position_moves(&pos, &[(e4, 1), (e4, 2)]),
            Err(BookError::DuplicateMove)
        ));
        let many = vec![(e4, 1); 101];
        assert!(matches!(
            book.set_position_moves(&pos, &many),
            Err(BookError::TooManyMoves { count: 101 })
        ));
        let illegal = Move::Null;
        assert!(book.set_position_moves(&pos, &[(illegal, 1)]).is_err());
        assert!(book.is_empty());
    }

    #[test]
    fn percent_scale_matches_scid() {
        let pos = Position::new();
        let (a, b, c) = (mv(&pos, "e2e4"), mv(&pos, "d2d4"), mv(&pos, "c2c4"));
        let w = weights_from_percent(&[(a, 50), (b, 25), (c, 0)]);
        assert_eq!(
            w.iter().map(|x| x.1).collect::<Vec<_>>(),
            vec![0xfff0, 0x7ff8, 1]
        );
        let w = weights_from_percent(&[(a, 0)]);
        assert_eq!(w[0].1, 1);
    }

    #[test]
    fn add_and_remove_line() {
        let pos = Position::new();
        let e4 = mv(&pos, "e2e4");
        let after = pos.after(e4).expect("play");
        let e5 = mv(&after, "e7e5");
        let mut book = Book::new();
        assert_eq!(book.add_line(&[e4, e5], &pos, 1).expect("add"), 2);
        assert_eq!(book.add_line(&[e4, e5], &pos, 5).expect("again"), 0);
        assert_eq!(book.moves(&pos)[0].weight, 1);
        assert_eq!(book.len(), 2);
        // Illegal continuation: nothing is added.
        assert!(matches!(
            book.add_line(&[e4, e4], &pos, 1),
            Err(BookError::IllegalMove { ply: 1 })
        ));
        assert_eq!(book.len(), 2);
        assert_eq!(book.remove_line(&[e4, e5], &pos).expect("remove"), 2);
        assert!(book.is_empty());
    }

    #[test]
    fn batch_remove_and_purge() {
        let pos = Position::new();
        let (e4, d4) = (mv(&pos, "e2e4"), mv(&pos, "d2d4"));
        let after = pos.after(e4).expect("play");
        let e5 = mv(&after, "e7e5");
        let mut book = Book::new();
        let lines = vec![(vec![e4, e5], 3), (vec![e4], 9), (vec![d4], 2)];
        assert_eq!(book.add_lines(&lines, &pos).expect("add"), 3);
        assert!(book.is_dirty());
        assert!(book.validate().was_sorted);
        assert_eq!(book.moves(&pos)[0].weight, 3);
        // Atomic validation: the bad second line leaves the book untouched.
        let bad = vec![(vec![d4], 1), (vec![e4, e4], 1)];
        assert!(matches!(
            book.add_lines(&bad, &after),
            Err(BookError::IllegalMove { ply: 0 })
        ));
        assert_eq!(book.len(), 3);
        // Index in the list for set_position_moves.
        assert!(matches!(
            book.set_position_moves(&pos, &[(e4, 1), (Move::Null, 1)]),
            Err(BookError::IllegalMove { ply: 1 })
        ));
        // Purge keeps valid entries only.
        let key = polyglot_key(&pos);
        book.entries.push(Entry {
            key,
            mv: 0x0abc,
            weight: 1,
            learn: 0,
        });
        book.entries.sort_by_key(|e| e.key);
        assert_eq!(book.purge_invalid(&pos).expect("purge"), 1);
        assert_eq!(book.moves(&pos).len(), 2);
        assert_eq!(book.remove_position(&pos).expect("remove"), 2);
        assert_eq!(book.len(), 1);
    }
}
