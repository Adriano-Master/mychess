//! M8-2a contract tests of the standings (spec D1282, D1316): 1/half/0,
//! tie-breaks, `*`, winners and the crosstable.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_2_common;

use mychess_app::tournament::{Finished, GameScore, cross_table, points_text, standings, winners};
use mychess_core::GameResult;
use proptest::prelude::*;

fn g(number: u32, white: usize, black: usize, result: GameResult) -> Finished {
    Finished {
        number,
        white,
        black,
        result,
    }
}

fn order(games: &[Finished], n: usize) -> Vec<usize> {
    standings(n, &[], games)
        .iter()
        .map(|r| r.participant)
        .collect()
}

#[test]
fn win_is_one_draw_is_half_and_loss_is_zero() {
    let rows = standings(
        3,
        &[],
        &[
            g(1, 0, 1, GameResult::White),
            g(2, 1, 2, GameResult::Draw),
            g(3, 2, 0, GameResult::White),
        ],
    );
    let by = |p| rows.iter().find(|r| r.participant == p).unwrap();
    assert_eq!(by(0).half_points, 2);
    assert_eq!(by(1).half_points, 1);
    assert_eq!(by(2).half_points, 3);
    assert_eq!(by(2).points_text(), "1.5");
    assert_eq!((by(2).played, by(2).wins, by(2).draws), (2, 1, 1));
}

#[test]
fn unknown_result_does_not_count_for_anything() {
    let rows = standings(2, &[], &[g(1, 0, 1, GameResult::Unknown)]);
    assert!(rows.iter().all(|r| r.played == 0 && r.half_points == 0));
    assert!(winners(&rows).is_empty());
    assert!(cross_table(2, &[g(1, 0, 1, GameResult::Unknown)])[0][1].is_empty());
}

#[test]
fn points_text_formats_halves() {
    assert_eq!(points_text(0), "0");
    assert_eq!(points_text(1), "0.5");
    assert_eq!(points_text(7), "3.5");
    assert_eq!(points_text(8), "4");
}

#[test]
fn percent_is_over_games_played_and_zero_without_games() {
    let rows = standings(2, &[], &[g(1, 0, 1, GameResult::Draw)]);
    assert!((rows[0].percent() - 50.0).abs() < 1e-3);
    let none = standings(2, &[], &[]);
    assert!(none[0].percent().abs() < 1e-3);
}

#[test]
fn direct_encounter_breaks_a_two_way_tie() {
    // 0 and 1 end with 1.5; 1 beat 0 (head to head), so 1 is first even
    // though 0 has the better index.
    let games = [
        g(1, 0, 1, GameResult::Black),
        g(2, 0, 2, GameResult::White),
        g(3, 2, 1, GameResult::Draw),
        g(4, 2, 0, GameResult::Draw),
    ];
    // 0: L, W, D = 1.5 ; 1: W, D = 1.5 ; 2: L, D, D = 1.0
    let rows = standings(3, &[], &games);
    assert_eq!(rows[0].half_points, rows[1].half_points);
    assert_eq!(order(&games, 3), [1, 0, 2]);
}

#[test]
fn direct_encounter_only_counts_among_the_tied() {
    // 0 beat 1, but 1 and 2 are tied with each other (not with 0) ...
    let games = [
        g(1, 0, 1, GameResult::White),
        g(2, 0, 2, GameResult::White),
        g(3, 1, 2, GameResult::Draw),
        g(4, 3, 1, GameResult::Draw),
        g(5, 3, 2, GameResult::Black),
    ];
    // 0=2, 1=1, 2=2.5? 2: L(0), D(1), W(3) = 1+... recompute via rows
    let rows = standings(4, &[], &games);
    let pts: Vec<u32> = (0..4)
        .map(|p| rows[rows.iter().position(|r| r.participant == p).unwrap()].half_points)
        .collect();
    assert_eq!(pts, [4, 2, 3, 1]);
    assert_eq!(order(&games, 4), [0, 2, 1, 3]);
}

#[test]
fn three_way_tie_uses_the_points_among_the_tied_and_then_sonneborn_berger() {
    // Cyclic: 0 beats 1, 1 beats 2, 2 beats 0; all against 3 win.
    let games = [
        g(1, 0, 1, GameResult::White),
        g(2, 1, 2, GameResult::White),
        g(3, 2, 0, GameResult::White),
        g(4, 0, 3, GameResult::White),
        g(5, 1, 3, GameResult::White),
        g(6, 2, 3, GameResult::White),
    ];
    // Everybody 2 points: head to head all equal, SB all equal, wins equal,
    // black wins: 0 won with white twice; 1 same; 2 same -> index order.
    assert_eq!(order(&games, 4), [0, 1, 2, 3]);
}

#[test]
fn three_way_tie_is_ordered_by_the_points_made_among_the_tied() {
    // 0, 1 and 2 end with 2 points each; among themselves 0 scored 2, 1
    // scored 1 and 2 scored 0, the rest came from games against 3 and 4.
    let games = [
        g(1, 0, 1, GameResult::White),
        g(2, 0, 2, GameResult::White),
        g(3, 1, 2, GameResult::White),
        g(4, 1, 3, GameResult::White),
        g(5, 2, 3, GameResult::White),
        g(6, 2, 4, GameResult::White),
    ];
    let rows = standings(5, &[], &games);
    let pts = |p: usize| {
        rows.iter()
            .find(|r| r.participant == p)
            .unwrap()
            .half_points
    };
    assert_eq!((pts(0), pts(1), pts(2)), (4, 4, 4));
    assert_eq!(order(&games, 5)[..3], [0, 1, 2]);
    // Reverse registration order of the same results flips the table
    // indices but not the logic: rename 0<->2.
    let swapped: Vec<Finished> = games
        .iter()
        .map(|f| {
            let m = |p: usize| match p {
                0 => 2,
                2 => 0,
                o => o,
            };
            g(f.number, m(f.white), m(f.black), f.result)
        })
        .collect();
    assert_eq!(order(&swapped, 5)[..3], [2, 1, 0]);
}

#[test]
fn sonneborn_berger_breaks_a_tie_with_no_direct_game_decisive() {
    // 0 and 1 draw each other; both tied on points; 0 beat the strong 2,
    // 1 beat the weak 3.
    let games = [
        g(1, 0, 1, GameResult::Draw),
        g(2, 0, 2, GameResult::White),
        g(3, 1, 3, GameResult::White),
        g(4, 2, 3, GameResult::White),
        g(5, 2, 1, GameResult::White),
        g(6, 0, 3, GameResult::Black),
    ];
    let rows = standings(4, &[], &games);
    let pts = |p: usize| {
        rows.iter()
            .find(|r| r.participant == p)
            .unwrap()
            .half_points
    };
    assert_eq!(pts(0), 3);
    assert_eq!(pts(1), 3);
    assert_eq!(pts(2), 4);
    let sb = |p: usize| {
        rows.iter()
            .find(|r| r.participant == p)
            .unwrap()
            .sonneborn_berger
    };
    // 0: draw with 1 (3 half pts *1) + win vs 2 (4*2) = 11 ; 1: 3*1 + win vs 3 (2*2)=7
    assert_eq!(sb(0), 11);
    assert_eq!(sb(1), 7);
    assert_eq!(order(&games, 4)[..3], [2, 0, 1]);
}

#[test]
fn wins_then_black_wins_then_registration_order_break_the_rest() {
    // 0: one win (white) = 1.0 ; 1: two draws = 1.0 ; all against others
    // that do not differ. Wins decide.
    let games = [
        g(1, 0, 2, GameResult::White),
        g(2, 0, 3, GameResult::Black),
        g(3, 1, 2, GameResult::Draw),
        g(4, 1, 3, GameResult::Draw),
    ];
    let rows = standings(4, &[], &games);
    let pts = |p: usize| {
        rows.iter()
            .find(|r| r.participant == p)
            .unwrap()
            .half_points
    };
    assert_eq!((pts(0), pts(1)), (2, 2));
    // Black wins: win with Black beats win with White at the end when the
    // rest is equal.
    let games = [g(1, 0, 2, GameResult::White), g(2, 3, 1, GameResult::Black)];
    let o = order(&games, 4);
    assert_eq!(o[..2], [1, 0], "the win with Black comes first: {o:?}");
    // No game at all: registration order, stable, no draw of lots.
    assert_eq!(order(&[], 5), [0, 1, 2, 3, 4]);
}

#[test]
fn winners_lists_everybody_tied_on_points_even_when_a_tie_break_orders_them() {
    let games = [
        g(1, 0, 1, GameResult::Black),
        g(2, 0, 2, GameResult::White),
        g(3, 1, 2, GameResult::Draw),
    ];
    // 0: 1, 1: 1.5, 2: 0.5 -> sole winner
    assert_eq!(winners(&standings(3, &[], &games)), [1]);
    let tie = [g(1, 0, 1, GameResult::White), g(2, 1, 0, GameResult::White)];
    let rows = standings(2, &[], &tie);
    assert_eq!(winners(&rows).len(), 2);
}

#[test]
fn withdrawn_participant_keeps_points_and_is_marked() {
    let rows = standings(3, &[false, true, false], &[g(1, 0, 1, GameResult::Black)]);
    let r1 = rows.iter().find(|r| r.participant == 1).unwrap();
    assert!(r1.withdrawn);
    assert_eq!(r1.half_points, 2);
}

#[test]
fn games_with_out_of_range_participants_do_not_panic() {
    let rows = standings(2, &[], &[g(1, 0, 7, GameResult::White)]);
    assert_eq!(rows.len(), 2);
    drop(cross_table(2, &[g(1, 9, 7, GameResult::White)]));
}

#[test]
fn cross_table_has_one_cell_entry_per_game_from_each_side() {
    let games = [
        g(1, 0, 1, GameResult::White),
        g(2, 1, 0, GameResult::Draw),
        g(3, 0, 1, GameResult::Black),
    ];
    let cells = cross_table(2, &games);
    assert_eq!(
        cells[0][1],
        [
            (1, GameScore::Win),
            (2, GameScore::Draw),
            (3, GameScore::Loss)
        ]
    );
    assert_eq!(
        cells[1][0],
        [
            (1, GameScore::Loss),
            (2, GameScore::Draw),
            (3, GameScore::Win)
        ]
    );
    assert!(cells[0][0].is_empty());
}

#[test]
fn game_score_symbols_and_half_points() {
    assert_eq!(GameScore::Win.symbol(), "1");
    assert_eq!(GameScore::Draw.symbol(), "\u{bd}");
    assert_eq!(GameScore::Loss.symbol(), "0");
    assert_eq!(
        [GameScore::Win, GameScore::Draw, GameScore::Loss].map(GameScore::half_points),
        [2, 1, 0]
    );
}

fn result_strategy() -> impl Strategy<Value = GameResult> {
    prop_oneof![
        Just(GameResult::White),
        Just(GameResult::Black),
        Just(GameResult::Draw),
        Just(GameResult::Unknown)
    ]
}

proptest! {
    #[test]
    fn total_points_equal_games_counted_and_table_is_a_permutation(
        n in 2usize..8,
        raw in proptest::collection::vec((0usize..8, 0usize..8, result_strategy()), 0..40)
    ) {
        let games: Vec<Finished> = raw
            .iter()
            .enumerate()
            .filter(|(_, (w, b, _))| w != b && *w < n && *b < n)
            .map(|(i, (w, b, r))| g(i as u32 + 1, *w, *b, *r))
            .collect();
        let rows = standings(n, &[], &games);
        let counted = games.iter().filter(|g| g.result != GameResult::Unknown).count() as u32;
        prop_assert_eq!(rows.iter().map(|r| r.half_points).sum::<u32>(), counted * 2);
        let mut parts: Vec<usize> = rows.iter().map(|r| r.participant).collect();
        parts.sort_unstable();
        prop_assert_eq!(parts, (0..n).collect::<Vec<_>>());
        for w in rows.windows(2) { prop_assert!(w[0].half_points >= w[1].half_points); }
        for (i, r) in rows.iter().enumerate() { prop_assert_eq!(r.rank, i + 1); }
        // Shuffling the input never changes the order of the table.
        let mut rev = games.clone();
        rev.reverse();
        let a: Vec<usize> = rows.iter().map(|r| r.participant).collect();
        let b: Vec<usize> = standings(n, &[], &rev).iter().map(|r| r.participant).collect();
        prop_assert_eq!(a, b);
    }
}

#[test]
fn direct_encounter_outranks_sonneborn_berger_wins_black_wins_and_index() {
    // 0 and 2 both end with 1 point. 0 is better on Sonneborn-Berger (3 vs 1),
    // equal on wins (1) and on wins with Black (1) and has the lower index;
    // but 2 beat 0 directly, and that comes first (D1282).
    let games = [
        g(1, 0, 1, GameResult::Black),
        g(2, 1, 0, GameResult::Black),
        g(3, 3, 1, GameResult::White),
        g(4, 3, 1, GameResult::Draw),
        g(5, 3, 1, GameResult::Draw),
        g(6, 1, 0, GameResult::White),
        g(7, 0, 2, GameResult::Black),
    ];
    let rows = standings(4, &[], &games);
    let by = |p: usize| rows.iter().find(|r| r.participant == p).unwrap();
    assert_eq!((by(0).half_points, by(2).half_points), (2, 2));
    assert!(by(0).sonneborn_berger > by(2).sonneborn_berger);
    assert_eq!(
        (by(0).wins, by(0).black_wins),
        (by(2).wins, by(2).black_wins)
    );
    assert_eq!(order(&games, 4), [1, 3, 2, 0]);
}
