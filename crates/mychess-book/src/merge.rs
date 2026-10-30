//! Merging two books (spec D1111).
//!
//! Scid: `src/polyglot/book_merge.cpp` (`polyglot merge-book`).

use crate::book::Book;
use crate::format::{Entry, MAX_WEIGHT, key_range};

/// How entries with the same position key are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeMode {
    /// Scid/Polyglot semantics: for a key present in both books the first
    /// book wins entirely and the second book's entries are dropped.
    #[default]
    KeepFirst,
    /// Same move in both books: the weights are added, saturating at `0xfff0`;
    /// distinct moves are united. Positions found in both books end up sorted
    /// by decreasing weight (stable); other positions keep their source order.
    SumWeights,
}

/// What [`merge`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeReport {
    /// Records in the first book.
    pub first_entries: usize,
    /// Records in the second book.
    pub second_entries: usize,
    /// Records in the result.
    pub written: usize,
    /// Records of the second book that were dropped (`KeepFirst`).
    pub skipped: usize,
    /// Positions present in both books.
    pub common_keys: usize,
}

/// Merges `a` and `b` into a new, sorted book bound to no file.
pub fn merge(a: &Book, b: &Book, mode: MergeMode) -> (Book, MergeReport) {
    let mut report = MergeReport {
        first_entries: a.entries.len(),
        second_entries: b.entries.len(),
        ..MergeReport::default()
    };
    let mut out: Vec<Entry> = Vec::with_capacity(a.entries.len() + b.entries.len());
    let mut ai = 0;
    let mut bi = 0;
    while ai < a.entries.len() || bi < b.entries.len() {
        let ka = a.entries.get(ai).map(|e| e.key);
        let kb = b.entries.get(bi).map(|e| e.key);
        let key = match (ka, kb) {
            (Some(x), Some(y)) => x.min(y),
            (Some(x), None) | (None, Some(x)) => x,
            (None, None) => break,
        };
        let ra = if ka == Some(key) {
            key_range(&a.entries, key)
        } else {
            0..0
        };
        let rb = if kb == Some(key) {
            key_range(&b.entries, key)
        } else {
            0..0
        };
        let ea = a.entries.get(ra.clone()).unwrap_or_default();
        let eb = b.entries.get(rb.clone()).unwrap_or_default();
        ai = ra.end.max(ai);
        bi = rb.end.max(bi);
        match (ea.is_empty(), eb.is_empty(), mode) {
            (false, false, MergeMode::KeepFirst) => {
                report.common_keys += 1;
                report.skipped += eb.len();
                out.extend_from_slice(ea);
            }
            (false, false, MergeMode::SumWeights) => {
                report.common_keys += 1;
                let start = out.len();
                out.extend_from_slice(ea);
                for e in eb {
                    let same = out
                        .get_mut(start..)
                        .and_then(|s| s.iter_mut().find(|x| x.mv == e.mv));
                    if let Some(x) = same {
                        let sum = u32::from(x.weight) + u32::from(e.weight);
                        x.weight =
                            u16::try_from(sum.min(u32::from(MAX_WEIGHT))).unwrap_or(MAX_WEIGHT);
                    } else {
                        out.push(*e);
                    }
                }
                if let Some(s) = out.get_mut(start..) {
                    s.sort_by_key(|e| std::cmp::Reverse(e.weight));
                }
            }
            (false, true, _) => out.extend_from_slice(ea),
            (true, false, _) => out.extend_from_slice(eb),
            (true, true, _) => break,
        }
    }
    report.written = out.len();
    (Book::from_entries(out), report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(key: u64, mv: u16, weight: u16) -> Entry {
        Entry {
            key,
            mv,
            weight,
            learn: 0,
        }
    }

    fn books() -> (Book, Book) {
        (
            Book::from_entries(vec![e(1, 1, 10), e(2, 1, 5), e(2, 2, 6)]),
            Book::from_entries(vec![e(2, 1, 4), e(2, 3, 9), e(3, 1, 1)]),
        )
    }

    #[test]
    fn keep_first_drops_the_second_books_common_keys() {
        let (a, b) = books();
        let (m, r) = merge(&a, &b, MergeMode::KeepFirst);
        let got: Vec<_> = m
            .entries()
            .iter()
            .map(|x| (x.key, x.mv, x.weight))
            .collect();
        assert_eq!(got, vec![(1, 1, 10), (2, 1, 5), (2, 2, 6), (3, 1, 1)]);
        assert_eq!((r.skipped, r.written, r.common_keys), (2, 4, 1));
        assert!(m.was_sorted());
    }

    #[test]
    fn sum_weights_adds_and_saturates() {
        let (a, b) = books();
        let (m, r) = merge(&a, &b, MergeMode::SumWeights);
        let got: Vec<_> = m
            .entries()
            .iter()
            .map(|x| (x.key, x.mv, x.weight))
            .collect();
        assert_eq!(
            got,
            vec![(1, 1, 10), (2, 1, 9), (2, 3, 9), (2, 2, 6), (3, 1, 1)]
        );
        assert_eq!((r.skipped, r.written), (0, 5));
        let big = Book::from_entries(vec![e(7, 1, 0xfff0)]);
        let (m, _) = merge(&big, &big, MergeMode::SumWeights);
        assert_eq!(m.entries()[0].weight, MAX_WEIGHT);
    }
}
