// Derived from Scid vs PC 4.27, src/tree.cpp (`TreeCache::Add`,
// `SetCacheSize`, `CacheResize`, `Clear`) and src/tree.h. Copyright (c) 2000
// Shane Hudson; Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The slots of one cache layer and their replacement policy (decision
//! D1046).

use mychess_core::Color;

use super::format::StcEntry;

/// Which entry a full cache gives up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Policy {
    /// The entry with the lowest total (`TREECACHE_Smallest`): the policy of
    /// the main cache on disk. A tree with a lower total than the lowest one
    /// is not even added.
    Smallest,
    /// A ring (`TREECACHE_Oldest`): the policy of the memory-only backup
    /// cache.
    Oldest,
}

/// The entries of one layer.
#[derive(Debug, Clone)]
pub(super) struct Slots {
    policy: Policy,
    capacity: usize,
    entries: Vec<StcEntry>,
    /// Ring position of `Oldest`; the entry last found by `add_with`.
    most_recent: usize,
    lowest_total: u32,
    lowest_index: usize,
}

impl Slots {
    pub(super) fn new(policy: Policy, capacity: usize) -> Slots {
        Slots {
            policy,
            capacity,
            entries: Vec::new(),
            most_recent: 0,
            lowest_total: 0,
            lowest_index: 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }

    pub(super) fn entries(&self) -> &[StcEntry] {
        &self.entries
    }

    /// `(LowestTotal, LowestTotalIndex)`; meaningless when empty.
    pub(super) fn lowest(&self) -> (u32, usize) {
        (self.lowest_total, self.lowest_index)
    }

    /// Forgets the entries (`Clear`); the capacity stays.
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.most_recent = 0;
        self.lowest_total = 0;
        self.lowest_index = 0;
    }

    /// `CacheResize`: a smaller size empties the layer, a bigger one keeps
    /// the entries.
    pub(super) fn resize(&mut self, capacity: usize) {
        if capacity < self.capacity {
            self.clear();
        }
        self.capacity = capacity;
    }

    /// Replaces the contents with the entries of a file (`ReadFile`). The
    /// caller has checked `entries.len() <= capacity`. `lowest` from the file
    /// is kept when it points to an entry, otherwise it is recomputed.
    pub(super) fn load(&mut self, capacity: usize, entries: Vec<StcEntry>, lowest: (u32, usize)) {
        self.capacity = capacity;
        self.entries = entries;
        self.most_recent = 0;
        // The values of the file are only trusted when they are the minimum.
        let valid = self
            .entries
            .get(lowest.1)
            .is_some_and(|e| e.total_count == lowest.0)
            && self.entries.iter().all(|e| e.total_count >= lowest.0);
        if valid {
            self.lowest_total = lowest.0;
            self.lowest_index = lowest.1;
        } else {
            self.recompute_lowest();
        }
    }

    pub(super) fn find(&self, to_move: Color, board: &[u8; 64]) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.to_move == to_move && e.board == *board)
    }

    /// The lowest total over **all** the entries, the first one on a tie.
    /// The Scid starts from the total of the entry just inserted, so it
    /// differs only on a tie between the new entry and an older one with the
    /// same total (decision D1046).
    fn recompute_lowest(&mut self) {
        let lowest = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(i, e)| (e.total_count, *i));
        if let Some((i, e)) = lowest {
            self.lowest_total = e.total_count;
            self.lowest_index = i;
        } else {
            self.lowest_total = 0;
            self.lowest_index = 0;
        }
    }

    /// `TreeCache::Add`. The entry is only built (compressing its filter) if
    /// it is going to be kept. Returns whether the tree is in the layer.
    pub(super) fn add_with(
        &mut self,
        to_move: Color,
        board: &[u8; 64],
        total: u32,
        build: impl FnOnce() -> StcEntry,
    ) -> bool {
        if self.capacity == 0 {
            return false;
        }
        if let Some(i) = self.find(to_move, board) {
            // Already there: only the "most recent" mark moves.
            self.most_recent = i;
            return true;
        }
        let full = self.entries.len() >= self.capacity;
        if self.policy == Policy::Smallest && full && total < self.lowest_total {
            return false;
        }
        let entry = build();
        if full {
            let slot = match self.policy {
                Policy::Oldest => {
                    self.most_recent = (self.most_recent + 1) % self.capacity;
                    self.most_recent
                }
                Policy::Smallest => self.lowest_index,
            };
            let Some(target) = self.entries.get_mut(slot) else {
                return false;
            };
            *target = entry;
            if self.policy == Policy::Smallest {
                self.recompute_lowest();
            }
        } else {
            let index = self.entries.len();
            self.entries.push(entry);
            if index == 0 || total < self.lowest_total {
                self.lowest_total = total;
                self.lowest_index = index;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::Filter;
    use crate::tree::stc::format::CompressedFilter;

    fn key(n: u8) -> [u8; 64] {
        let mut board = [7u8; 64];
        board[0] = n;
        board
    }

    fn entry(n: u8, total: u32) -> StcEntry {
        StcEntry {
            to_move: Color::White,
            board: key(n),
            total_count: total,
            nodes: Vec::new(),
            filter: CompressedFilter::compress(&Filter::new(1)),
        }
    }

    fn add(slots: &mut Slots, n: u8, total: u32) -> bool {
        slots.add_with(Color::White, &key(n), total, || entry(n, total))
    }

    fn totals(slots: &Slots) -> Vec<u32> {
        slots.entries().iter().map(|e| e.total_count).collect()
    }

    #[test]
    fn smallest_rejects_lower_replaces_equal_and_evicts_the_lowest() {
        let mut s = Slots::new(Policy::Smallest, 3);
        assert!(add(&mut s, 1, 50));
        assert!(add(&mut s, 2, 10));
        assert!(add(&mut s, 3, 30));
        assert_eq!(s.lowest(), (10, 1));
        // Full: lower than the lowest is rejected, equal replaces it.
        assert!(!add(&mut s, 4, 9));
        assert!(add(&mut s, 5, 10));
        assert_eq!(totals(&s), [50, 10, 30]);
        assert_eq!(s.find(Color::White, &key(5)), Some(1));
        // The new lowest is recomputed over all entries, the new one included.
        assert_eq!(s.lowest(), (10, 1));
        assert!(add(&mut s, 6, 40));
        assert_eq!(totals(&s), [50, 40, 30]);
        assert_eq!(s.lowest(), (30, 2));
    }

    #[test]
    fn existing_position_only_updates_the_mark() {
        let mut s = Slots::new(Policy::Oldest, 3);
        assert!(add(&mut s, 1, 1));
        assert!(add(&mut s, 2, 1));
        assert!(add(&mut s, 2, 99));
        assert_eq!(totals(&s), [1, 1]);
    }

    #[test]
    fn oldest_is_a_ring_starting_after_index_zero() {
        let mut s = Slots::new(Policy::Oldest, 3);
        for n in 1..=3 {
            assert!(add(&mut s, n, 1));
        }
        // Scid: MostRecentIndex starts at 0, so slot 1 goes first.
        assert!(add(&mut s, 4, 1));
        assert_eq!(s.find(Color::White, &key(4)), Some(1));
        assert!(add(&mut s, 5, 1));
        assert!(add(&mut s, 6, 1));
        assert!(add(&mut s, 7, 1));
        assert_eq!(s.find(Color::White, &key(5)), Some(2));
        assert_eq!(s.find(Color::White, &key(6)), Some(0));
        assert_eq!(s.find(Color::White, &key(7)), Some(1));
    }

    #[test]
    fn load_distrusts_a_lowest_that_is_not_the_minimum() {
        let mut s = Slots::new(Policy::Smallest, 3);
        let entries = vec![entry(1, 50), entry(2, 10), entry(3, 30)];
        s.load(3, entries.clone(), (30, 2));
        assert_eq!(s.lowest(), (10, 1));
        s.load(3, entries.clone(), (99, 1));
        assert_eq!(s.lowest(), (10, 1));
        s.load(3, entries.clone(), (10, 1));
        assert_eq!(s.lowest(), (10, 1));
        s.load(3, entries, (10, 7));
        assert_eq!(s.lowest(), (10, 1));
    }

    #[test]
    fn resize_down_clears_and_up_keeps() {
        let mut s = Slots::new(Policy::Smallest, 4);
        assert!(add(&mut s, 1, 5));
        s.resize(8);
        assert_eq!((s.len(), s.capacity()), (1, 8));
        s.resize(2);
        assert_eq!((s.len(), s.capacity()), (0, 2));
    }
}
