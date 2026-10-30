// Derived from Scid vs PC 4.27, src/crosstab.cpp (`RatingChange`,
// `FideCategory`, `OpponentElo`, `Performance`). Copyright (c) 2001 Shane
// Hudson; Scid vs PC contributors. Scid is distributed under the GNU General
// Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Rating arithmetic of the tournament table (rules T6, T8, T9 and T24 of the
//! spec). All integer, with the truncating division of C.

use crate::tree::{PERF_ELODIFF, performance};

/// Largest rating difference counted between a player and an opponent.
const MARGIN: u32 = 350;

/// The performance rating for an average opponent Elo and a percentage score
/// (`Crosstable::Performance`; never below zero, see
/// [`crate::tree::performance`]).
///
/// ```
/// use mychess_db::crosstable::performance_rating;
///
/// assert_eq!(performance_rating(2500, 75), 2693);
/// assert_eq!(performance_rating(2400, 70), 2549);
/// ```
pub fn performance_rating(opponent_average: u32, percentage: u32) -> u32 {
    performance(opponent_average, percentage)
}

/// The Elo of an opponent as counted in the average: limited to 350 points
/// from the player's own Elo (`OpponentElo`). A player without Elo (0) counts
/// the opponent's.
///
/// ```
/// use mychess_db::crosstable::opponent_elo;
///
/// assert_eq!(opponent_elo(2400, 2900), 2750);
/// assert_eq!(opponent_elo(2400, 1900), 2050);
/// assert_eq!(opponent_elo(0, 1900), 1900);
/// assert_eq!(opponent_elo(2400, 2500), 2500);
/// ```
pub fn opponent_elo(player: u32, opponent: u32) -> u32 {
    if player == 0 {
        opponent
    } else if player > opponent && player - opponent > MARGIN {
        player - MARGIN
    } else if opponent > player && opponent - player > MARGIN {
        player + MARGIN
    } else {
        opponent
    }
}

/// The rating change of a player with Elo `player` who scored `percentage` per
/// cent in `games` games against an average opponent of `opponent_average`
/// (`RatingChange`). The division truncates toward zero.
///
/// ```
/// use mychess_db::crosstable::rating_change;
///
/// assert_eq!(rating_change(2500, 2400, 75, 9), 10);
/// assert_eq!(rating_change(2400, 2500, 50, 10), 14);
/// assert_eq!(rating_change(2400, 2400, 25, 4), -10);
/// ```
pub fn rating_change(player: u32, opponent_average: u32, percentage: u32, games: u32) -> i32 {
    let diff = player.abs_diff(opponent_average);
    let index = PERF_ELODIFF
        .iter()
        .take(50)
        .position(|&limit| diff <= limit)
        .unwrap_or(50);
    // `index` is at most 50.
    let index = i64::try_from(index).unwrap_or(50);
    let expected = if player > opponent_average {
        50 + index
    } else {
        50 - index
    };
    let percentage = i64::from(percentage);
    let cutoff = if percentage > expected { 5 } else { -5 };
    let value = ((percentage - expected) * i64::from(games) + cutoff) / 10;
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

/// The FIDE category of a tournament with the given average Elo: 0 up to
/// 2250, then 1 for 2251-2275, 2 for 2276-2300, and so on (`FideCategory`).
///
/// ```
/// use mychess_db::crosstable::fide_category;
///
/// assert_eq!(fide_category(2250), 0);
/// assert_eq!(fide_category(2251), 1);
/// assert_eq!(fide_category(2275), 1);
/// assert_eq!(fide_category(2276), 2);
/// ```
pub fn fide_category(average_elo: u32) -> u32 {
    if average_elo <= 2250 {
        0
    } else {
        1 + (average_elo - 2251) / 25
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn performance_examples_of_the_spec() {
        assert_eq!(performance_rating(2500, 75), 2693);
        assert_eq!(performance_rating(2400, 70), 2549);
    }

    #[test]
    fn rating_change_examples_of_the_spec() {
        assert_eq!(rating_change(2500, 2400, 75, 9), 10);
        assert_eq!(rating_change(2400, 2500, 50, 10), 14);
        assert_eq!(rating_change(2400, 2400, 25, 4), -10);
    }

    #[test]
    fn rating_change_with_a_huge_difference_uses_index_50() {
        // diff 2000 is above every table value: i = 50, expected 100 or 0.
        assert_eq!(rating_change(3000, 1000, 90, 10), -10);
        assert_eq!(rating_change(1000, 3000, 10, 10), 10);
    }

    #[test]
    fn opponent_elo_margins() {
        assert_eq!(opponent_elo(2400, 2900), 2750);
        assert_eq!(opponent_elo(2400, 1900), 2050);
        assert_eq!(opponent_elo(2400, 2750), 2750);
        assert_eq!(opponent_elo(2400, 2751), 2751 - 1);
        assert_eq!(opponent_elo(2400, 2050), 2050);
        assert_eq!(opponent_elo(0, 2000), 2000);
    }

    #[test]
    fn fide_category_steps() {
        assert_eq!(fide_category(0), 0);
        assert_eq!(fide_category(2250), 0);
        assert_eq!(fide_category(2251), 1);
        assert_eq!(fide_category(2275), 1);
        assert_eq!(fide_category(2276), 2);
        assert_eq!(fide_category(2500), 10);
    }
}
