//! The sorted view of a slot (spec M3-4, slice 4c; D506, D558).
//!
//! Sorting never touches the database file: the order of all the games is
//! computed once (by an effect, for large databases) and kept as a vector of
//! game numbers, and the games of the filter in that order are cached in
//! `visible`, which is what the list shows. Changing the direction or the
//! filter only rebuilds `visible`, one pass over the bytes of the filter.

use std::sync::Arc;

use mychess_db::Filter;

use super::columns::SortDir;

/// The games of a slot in the order of a column.
pub(crate) struct ViewOrder {
    dir: SortDir,
    /// Every game, smallest first; ties keep the database order.
    all: Arc<Vec<u32>>,
    /// The games of the filter, in the direction of the view.
    visible: Vec<u32>,
}

impl std::fmt::Debug for ViewOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewOrder")
            .field("dir", &self.dir)
            .field("games", &self.all.len())
            .field("visible", &self.visible.len())
            .finish()
    }
}

impl ViewOrder {
    /// The order `all` (smallest first) seen through `filter`.
    pub(crate) fn new(dir: SortDir, all: Arc<Vec<u32>>, filter: &Filter) -> ViewOrder {
        let mut order = ViewOrder {
            dir,
            all,
            visible: Vec::new(),
        };
        order.rebuild(filter);
        order
    }

    /// Recomputes `visible` after the filter changed.
    pub(crate) fn rebuild(&mut self, filter: &Filter) {
        let keep = |n: &&u32| filter.contains(**n);
        self.visible = match self.dir {
            SortDir::Ascending => self.all.iter().filter(keep).copied().collect(),
            SortDir::Descending => self.all.iter().rev().filter(keep).copied().collect(),
        };
    }

    /// Changes the direction and rebuilds.
    pub(crate) fn set_dir(&mut self, dir: SortDir, filter: &Filter) {
        if dir != self.dir {
            self.dir = dir;
            self.rebuild(filter);
        }
    }

    /// The game at `position` (from 1).
    pub(crate) fn nth(&self, position: u32) -> Option<u32> {
        let index = usize::try_from(position.checked_sub(1)?).ok()?;
        self.visible.get(index).copied()
    }

    /// The position (from 1) of `number` in the view.
    pub(crate) fn position_of(&self, number: u32) -> Option<u32> {
        let index = self.visible.iter().position(|n| *n == number)?;
        u32::try_from(index + 1).ok()
    }

    /// The games at positions `from..=to`, clamped to the view.
    pub(crate) fn range(&self, from: u32, to: u32) -> Vec<u32> {
        let start = usize::try_from(from.max(1) - 1).unwrap_or(usize::MAX);
        let end = usize::try_from(to).unwrap_or(usize::MAX);
        self.visible
            .get(start.min(self.visible.len())..end.min(self.visible.len()))
            .map(<[u32]>::to_vec)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(dir: SortDir, filter: &Filter) -> ViewOrder {
        // Games 1..=5 sorted as 3, 1, 5, 2, 4.
        ViewOrder::new(dir, Arc::new(vec![3, 1, 5, 2, 4]), filter)
    }

    #[test]
    fn the_view_follows_the_order_and_the_direction() {
        let filter = Filter::new(5);
        let mut view = order(SortDir::Ascending, &filter);
        assert_eq!(view.range(1, 5), [3, 1, 5, 2, 4]);
        assert_eq!(view.nth(3), Some(5));
        assert_eq!(view.position_of(2), Some(4));
        view.set_dir(SortDir::Descending, &filter);
        assert_eq!(view.range(1, 5), [4, 2, 5, 1, 3]);
        assert_eq!(view.range(2, 3), [2, 5]);
        assert_eq!(view.nth(0), None);
        assert_eq!(view.nth(6), None);
    }

    #[test]
    fn the_filter_hides_games_and_rebuilding_follows_it() {
        let mut filter = Filter::new(5);
        filter.set(1, 0);
        filter.set(4, 0);
        let mut view = order(SortDir::Ascending, &filter);
        assert_eq!(view.range(1, 9), [3, 5, 2]);
        assert_eq!(view.position_of(1), None);
        filter.fill(1);
        view.rebuild(&filter);
        assert_eq!(view.range(1, 9).len(), 5);
        assert_eq!(view.range(9, 12), Vec::<u32>::new());
    }

    /// Measures the work `update` does on a sorted view of a million games
    /// (D574). Run with `cargo test -p mychess-app --lib --release
    /// view_order -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not a check"]
    fn measure_the_cost_of_a_sorted_view_of_a_million_games() {
        use std::time::Instant;
        let games = 1_000_000u32;
        let all: Vec<u32> = (1..=games).rev().collect();
        let mut filter = Filter::new(games);
        let start = Instant::now();
        let mut view = ViewOrder::new(SortDir::Ascending, Arc::new(all), &filter);
        println!("build: {:?}", start.elapsed());
        filter.set(500_000, 0);
        let start = Instant::now();
        view.rebuild(&filter);
        println!("rebuild after a filter change: {:?}", start.elapsed());
        let start = Instant::now();
        let at = view.position_of(1);
        println!("position_of (worst case) {at:?}: {:?}", start.elapsed());
        let start = Instant::now();
        let range = view.range(1, games);
        println!(
            "range of all ({} games): {:?}",
            range.len(),
            start.elapsed()
        );
    }
}
