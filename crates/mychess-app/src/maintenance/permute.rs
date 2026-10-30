//! Carrying the numbers of the games through the permutation of a sort (spec
//! M7-1, rule 41): the filter, the selection and the game on the editor go
//! to the new numbers. The permutation is `SortReport::order`: `order[i]` is
//! the new position (0-based) of the game that was at `i` (0-based).

use mychess_db::Filter;

/// The new number (from 1) of game `number` (from 1), if the permutation
/// knows it.
pub(crate) fn new_number(order: &[u32], number: u32) -> Option<u32> {
    let index = usize::try_from(number.checked_sub(1)?).ok()?;
    order.get(index).and_then(|n| n.checked_add(1))
}

/// The filter with every game at its new number, keeping the ply of each.
/// `None` when the filter and the permutation do not cover the same games.
pub(crate) fn permute_filter(filter: &Filter, order: &[u32]) -> Option<Filter> {
    if usize::try_from(filter.len()).ok() != Some(order.len()) {
        return None;
    }
    let mut next = Filter::new(filter.len());
    next.fill(0);
    for number in filter.iter_games() {
        let moved = new_number(order, number)?;
        next.set(moved, filter.get(number));
    }
    Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_goes_to_the_position_the_permutation_gives() {
        // Game 1 goes to position 3 (number 3), game 2 to 1, game 3 to 2.
        let order = [2, 0, 1];
        assert_eq!(new_number(&order, 1), Some(3));
        assert_eq!(new_number(&order, 2), Some(1));
        assert_eq!(new_number(&order, 3), Some(2));
        assert_eq!(new_number(&order, 0), None);
        assert_eq!(new_number(&order, 4), None);
    }

    #[test]
    fn the_filter_keeps_the_ply_of_each_game_at_its_new_number() {
        let order = [2, 0, 1];
        let mut filter = Filter::new(3);
        filter.fill(0);
        filter.set(1, 7);
        filter.set(3, 9);
        let moved = permute_filter(&filter, &order).expect("same size");
        assert_eq!(moved.get(3), 7, "game 1 is game 3 now");
        assert_eq!(moved.get(2), 9, "game 3 is game 2 now");
        assert!(!moved.contains(1));
        assert_eq!(moved.count(), 2);
    }

    #[test]
    fn a_filter_of_another_size_is_refused() {
        assert!(permute_filter(&Filter::new(4), &[0, 1, 2]).is_none());
    }
}
