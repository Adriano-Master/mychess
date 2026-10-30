//! Contract tests of `score_series` and `time_series` (M4-7), derived from
//! the spec and from Scid (`tkscid.cpp` `addScoreToList`, `sc_game_scores`,
//! `sc_game_values`; `graphs.tcl:505-610`).

// Test code: exact float comparisons are intended (values are built from small
// integers) and the casts are bounded by the tiny sizes used here.
#![allow(
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::field_reassign_with_default,
    clippy::float_cmp,
    clippy::format_push_string,
    clippy::single_match_else,
    clippy::type_complexity,
    clippy::unnecessary_wraps,
    clippy::useless_vec
)]

mod m4_7_common;

use m4_7_common::{clk, clock_game, emt, game_with, node_at, shuffle_game};
use mychess_app::graphs::{Invert, SCORE_LIMIT, ScorePoint, score_series, time_series};
use mychess_core::Game;
use proptest::prelude::*;
use rstest::rstest;

const NONE: Invert = Invert {
    white: false,
    black: false,
};

fn ys(game: &Game, inv: Invert) -> Vec<f32> {
    score_series(game, inv).iter().map(|p| p.y).collect()
}

fn seconds(game: &Game) -> Vec<(f32, u32)> {
    time_series(game)
        .per_move
        .iter()
        .map(|p| (p.x, p.seconds))
        .collect()
}

// ---------------------------------------------------------------- scores

#[test]
fn empty_game_has_no_series() {
    let g = Game::new();
    assert!(score_series(&g, NONE).is_empty());
    let t = time_series(&g);
    assert!(t.per_move.is_empty() && t.white_total.is_empty() && t.black_total.is_empty());
}

#[test]
fn game_without_comments_has_no_series() {
    let g = shuffle_game(8, |_| None);
    assert!(score_series(&g, NONE).is_empty());
    assert!(time_series(&g).per_move.is_empty());
}

#[rstest]
#[case("[%eval 0.35]", Some(0.35))]
#[case("[%eval -1.2]", Some(-1.2))]
#[case("[%eval +0.5]", Some(0.5))]
#[case("[%eval 0]", Some(0.0))]
#[case("[%eval 0.00]", Some(0.0))]
#[case("[%eval #3]", Some(10.0))]
#[case("[%eval #-2]", Some(-10.0))]
#[case("[%eval 25]", Some(10.0))]
#[case("[%eval -25.5]", Some(-10.0))]
#[case("[%eval 10]", Some(10.0))]
#[case("[%eval 10.01]", Some(10.0))]
#[case("+0.35: good move", Some(0.35))]
#[case("-1.50: bad move", Some(-1.5))]
#[case("Nf3 1-0", None)]
#[case("0-1", None)]
#[case("+-", None)]
#[case("+", None)]
#[case("-", None)]
#[case("just text", None)]
#[case("", None)]
#[case("[%eval ]", None)]
#[case("[%eval abc]", None)]
#[case("[%eval -]", None)]
#[case("[%clk 0:05:00]", None)]
fn score_of_first_comment(#[case] comment: &str, #[case] want: Option<f32>) {
    let g = game_with(&[comment]);
    let got = ys(&g, NONE);
    match want {
        None => assert!(got.is_empty(), "{comment:?} gave {got:?}"),
        Some(w) => {
            assert_eq!(got.len(), 1, "{comment:?}");
            assert!((got[0] - w).abs() < 1e-5, "{comment:?}: {} != {w}", got[0]);
        }
    }
}

#[test]
fn eval_command_has_priority_over_old_form() {
    let g = game_with(&["+3.0 [%eval 0.5]"]);
    assert_eq!(ys(&g, NONE), vec![0.5]);
}

#[test]
fn x_is_half_a_move_per_ply_with_gaps_for_missing_scores() {
    let g = game_with(&[
        "[%eval 0.1]",
        "",
        "[%eval 0.3]",
        "[%eval 0.4]",
        "",
        "[%eval 0.6]",
    ]);
    let xs: Vec<f32> = score_series(&g, NONE).iter().map(|p| p.x).collect();
    assert_eq!(xs, vec![1.0, 2.0, 2.5, 3.5]);
}

#[test]
fn first_points_are_one_and_one_and_a_half() {
    let g = game_with(&["[%eval 1]", "[%eval 2]", "[%eval 3]"]);
    assert_eq!(
        score_series(&g, NONE),
        vec![
            ScorePoint { x: 1.0, y: 1.0 },
            ScorePoint { x: 1.5, y: 2.0 },
            ScorePoint { x: 2.0, y: 3.0 },
        ]
    );
}

/// Scid: `moveCounter % 2 ? inv_b : inv_w` (`tkscid.cpp:9299`): "invert
/// black" negates the score after White's move (odd ply).
#[rstest]
#[case(false, false, 1.0, 2.0)]
#[case(true, false, 1.0, -2.0)]
#[case(false, true, -1.0, 2.0)]
#[case(true, true, -1.0, -2.0)]
fn inversion_follows_scid_convention(
    #[case] white: bool,
    #[case] black: bool,
    #[case] ply1: f32,
    #[case] ply2: f32,
) {
    let g = game_with(&["[%eval 1]", "[%eval 2]"]);
    assert_eq!(ys(&g, Invert { white, black }), vec![ply1, ply2]);
}

#[rstest]
#[case("[%eval 25]", true, -10.0)]
#[case("[%eval -25]", true, 10.0)]
#[case("[%eval #3]", true, -10.0)]
#[case("[%eval #-3]", true, 10.0)]
#[case("[%eval 12]", false, 10.0)]
fn inversion_happens_before_the_clamp(
    #[case] comment: &str,
    #[case] invert: bool,
    #[case] want: f32,
) {
    let g = game_with(&[comment]);
    let inv = Invert {
        white: false,
        black: invert,
    };
    assert_eq!(ys(&g, inv), vec![want]);
}

#[test]
fn variations_are_ignored_by_both_series() {
    let mut g = game_with(&["[%eval 1] [%emt 0:01]", "[%eval 2] [%emt 0:02]"]);
    let pos = mychess_core::Position::new();
    let var = g
        .add_move(g.root(), pos.parse_uci("d2d4").expect("uci"))
        .expect("variation");
    g.set_comment(var, "[%eval 9] [%emt 0:09]")
        .expect("comment");
    assert_eq!(ys(&g, NONE), vec![1.0, 2.0]);
    assert_eq!(seconds(&g), vec![(1.0, 1), (1.5, 2)]);
}

#[test]
fn comment_before_the_move_is_not_read() {
    let mut g = game_with(&["", ""]);
    let n2 = node_at(&g, 2);
    g.set_comment_before(n2, "[%eval 3] [%emt 0:03]")
        .expect("comment");
    assert!(score_series(&g, NONE).is_empty());
    assert!(time_series(&g).per_move.is_empty());
}

#[test]
fn start_position_from_fen_does_not_shift_x() {
    let pos = mychess_core::Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").expect("fen");
    let mut g = Game::from_position(pos.clone());
    let mv = pos.parse_uci("e2e4").expect("uci");
    let n = g.add_move(g.root(), mv).expect("move");
    g.set_comment(n, "[%eval 1]").expect("comment");
    assert_eq!(score_series(&g, NONE), vec![ScorePoint { x: 1.0, y: 1.0 }]);
}

#[test]
fn hostile_comments_never_panic_and_stay_in_range() {
    let long_digits = format!("[%eval {}]", "9".repeat(5000));
    let long_clock = format!("[%emt {}:{}]", "9".repeat(40), "9".repeat(40));
    let g = game_with(&[
        &long_digits,
        &long_clock,
        "[%eval 1e999]",
        "[%eval nan]",
        "[%eval inf]",
        "[%eval ",
        "[%emt ",
        "[%clk ::]",
        "\u{1F600} +\u{0661} \u{202E}-3 [%eval \u{FF11}]",
        "[%eval -0.0000000001]",
        "[%emt 0:-5]",
        "[%emt 0:60:60]",
    ]);
    for p in score_series(&g, NONE) {
        assert!(p.y.is_finite() && p.y.abs() <= SCORE_LIMIT, "{p:?}");
    }
    drop(time_series(&g));
}

// ----------------------------------------------------------------- times

#[rstest]
#[case("[%emt 0:05]", Some(5))]
#[case("[%emt 1:05]", Some(65))]
#[case("[%emt 0:00:05]", Some(5))]
#[case("[%emt 1:02:03]", Some(3723))]
#[case("[%emt 5]", None)]
#[case("[%emt a:b]", None)]
#[case("[%emt ]", None)]
#[case("[%emt 1:2:3:4]", None)]
#[case("[%eval 1]", None)]
fn emt_formats(#[case] comment: &str, #[case] want: Option<u32>) {
    let g = game_with(&[comment]);
    let got: Vec<u32> = seconds(&g).iter().map(|p| p.1).collect();
    assert_eq!(got, want.into_iter().collect::<Vec<_>>(), "{comment:?}");
}

#[test]
fn emt_series_has_totals_per_side() {
    let g = game_with(&[
        "[%emt 0:05]",
        "[%emt 0:07]",
        "[%emt 0:03]",
        "",
        "[%emt 0:10]",
        "[%emt 0:01]",
    ]);
    let t = time_series(&g);
    let per: Vec<(f32, u32)> = t.per_move.iter().map(|p| (p.x, p.seconds)).collect();
    assert_eq!(per, vec![(1.0, 5), (1.5, 7), (2.0, 3), (3.0, 10), (3.5, 1)]);
    assert_eq!(t.white_total, vec![(1.0, 5.0), (2.0, 8.0), (3.0, 18.0)]);
    assert_eq!(t.black_total, vec![(1.5, 7.0), (3.5, 8.0)]);
}

/// 5+3 game: clocks after each ply; start = `max(w1, b1) + 1`.
#[test]
fn clock_game_five_plus_three_gives_emt_per_move() {
    // initial = max(298, 299) + 1 = 300; increment 3.
    // w1 = 300-298+3 = 5; b1 = 300-299+3 = 4; w2 = 298-295+3 = 6;
    // b2 = 299-305+3 = -3 -> 0 (negative becomes zero).
    let g = clock_game(&[298, 299, 295, 305], Some("300+3"));
    assert_eq!(seconds(&g), vec![(1.0, 5), (1.5, 4), (2.0, 6), (2.5, 0)]);
    let t = time_series(&g);
    assert_eq!(t.white_total, vec![(1.0, 5.0), (2.0, 11.0)]);
    assert_eq!(t.black_total, vec![(1.5, 4.0), (2.5, 4.0)]);
}

#[rstest]
#[case(300, 295, 1, 6)] // white has more: start = 301
#[case(295, 300, 6, 1)] // black has more: start = 301
#[case(300, 300, 1, 1)]
fn initial_clock_is_the_larger_first_clock_plus_one(
    #[case] w1: u32,
    #[case] b1: u32,
    #[case] ew: u32,
    #[case] eb: u32,
) {
    let g = clock_game(&[w1, b1], None);
    let got: Vec<u32> = seconds(&g).iter().map(|p| p.1).collect();
    assert_eq!(got, vec![ew, eb]);
}

#[rstest]
#[case(None, 0)]
#[case(Some("300+3"), 3)]
#[case(Some("300"), 0)]
#[case(Some("40/7200"), 0)] // Scid reads 7200 as increment; spec D485 omits it
#[case(Some("40/7200:3600"), 0)]
#[case(Some("-"), 0)]
#[case(Some("?"), 0)]
#[case(Some(""), 0)]
#[case(Some("300+"), 0)]
#[case(Some("+3"), 0)]
#[case(Some("180+2"), 2)]
fn increment_comes_only_from_base_plus_increment(#[case] tc: Option<&str>, #[case] incr: u32) {
    // initial = 101; w1 = 101-100+incr.
    let g = clock_game(&[100, 100, 90, 90], tc);
    let got: Vec<u32> = seconds(&g).iter().map(|p| p.1).collect();
    assert_eq!(
        got,
        vec![1 + incr, 1 + incr, 10 + incr, 10 + incr],
        "{tc:?}"
    );
}

#[test]
fn missing_clock_of_black_reply_ends_the_series_immediately() {
    // Scid breaks before emitting the white move of the pair
    // (`graphs.tcl:549-554`): x "2.0" is not "*.5".
    let mut clocks = vec![Some(298), None, Some(295), Some(294)];
    let g = shuffle_game(4, |i| clocks[i].take().map(clk));
    assert!(seconds(&g).is_empty());
}

#[test]
fn missing_clock_of_a_white_move_ends_the_series_there() {
    // x "2.5" where "*.0" is expected: only the first pair survives.
    let mut clocks = vec![Some(298), Some(299), None, Some(295), Some(290), Some(288)];
    let g = shuffle_game(6, |i| clocks[i].take().map(clk));
    let xs: Vec<f32> = seconds(&g).iter().map(|p| p.0).collect();
    assert_eq!(xs, vec![1.0, 1.5]);
}

#[test]
fn last_single_white_clock_is_used() {
    let g = clock_game(&[298, 299, 295], Some("300+3"));
    assert_eq!(seconds(&g), vec![(1.0, 5), (1.5, 4), (2.0, 6)]);
}

#[test]
fn clocks_not_starting_at_ply_one_are_not_used() {
    let mut cl = vec![None, Some(299), Some(295), Some(290)];
    let g = shuffle_game(4, |i| cl[i].take().map(clk));
    assert!(seconds(&g).is_empty());
}

#[test]
fn single_clock_gives_no_times() {
    let g = clock_game(&[298], None);
    assert!(seconds(&g).is_empty());
}

#[test]
fn as_many_emt_as_clk_uses_emt() {
    let g = shuffle_game(4, |i| {
        Some(format!(
            "{} {}",
            emt(3 + i as u32),
            clk(300 - 10 * i as u32)
        ))
    });
    let got: Vec<u32> = seconds(&g).iter().map(|p| p.1).collect();
    assert_eq!(got, vec![3, 4, 5, 6]);
}

#[test]
fn more_emt_than_clk_uses_emt() {
    let g = shuffle_game(4, |i| {
        let mut s = emt(7);
        if i < 2 {
            s.push_str(&clk(300 - i as u32));
        }
        Some(s)
    });
    let got: Vec<u32> = seconds(&g).iter().map(|p| p.1).collect();
    assert_eq!(got, vec![7; 4]);
}

#[test]
fn more_clk_than_emt_prefers_clocks() {
    let g = shuffle_game(4, |i| {
        let mut s = clk([298, 299, 295, 290][i]);
        if i == 0 {
            s.push_str(&emt(40));
        }
        Some(s)
    });
    let got: Vec<u32> = seconds(&g).iter().map(|p| p.1).collect();
    // initial 300; black's previous clock is 299 after ply 2, so 299 - 290.
    assert_eq!(got, vec![2, 1, 3, 9]);
}

#[test]
fn only_scores_give_no_times_and_only_times_give_no_scores() {
    let g = game_with(&["[%eval 1]"; 4]);
    assert!(time_series(&g).per_move.is_empty());
    let g = game_with(&[&emt(3), &emt(3)]);
    assert!(score_series(&g, NONE).is_empty());
    let g = clock_game(&[10, 9, 8], None);
    assert!(score_series(&g, NONE).is_empty());
}

#[test]
fn totals_do_not_overflow_with_huge_times() {
    // 99:59:59-ish values accumulate in u64/f32 without panicking.
    let g = shuffle_game(200, |_| Some("[%emt 99999:59:59]".to_owned()));
    let t = time_series(&g);
    assert_eq!(t.per_move.len(), 200);
    assert!(t.white_total.iter().all(|p| p.1.is_finite()));
}

#[test]
fn thousands_of_plies_are_processed() {
    let n = 20_000;
    let g = shuffle_game(n, |i| {
        Some(format!("[%eval {}.5] {}", i % 7, emt((i % 30) as u32)))
    });
    assert_eq!(score_series(&g, NONE).len(), n);
    assert_eq!(time_series(&g).per_move.len(), n);
}

// ------------------------------------------------------------- proptests

fn comment_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        (-300i32..300).prop_map(|v| format!(
            "[%eval {}{}.{}]",
            if v < 0 { "-" } else { "" },
            (v / 10).abs(),
            v.abs() % 10
        )),
        (-5i32..5).prop_map(|v| format!("[%eval #{v}]")),
        (0u32..4000).prop_map(emt),
        (0u32..4000).prop_map(clk),
        "\\PC{0,30}",
        Just("1-0".to_owned()),
    ]
}

proptest! {
    #[test]
    fn score_series_invariants(
        comments in proptest::collection::vec(comment_strategy(), 0..30),
        white in any::<bool>(),
        black in any::<bool>(),
    ) {
        let refs: Vec<&str> = comments.iter().map(String::as_str).collect();
        let g = game_with(&refs);
        let s = score_series(&g, Invert { white, black });
        prop_assert!(s.len() <= comments.len());
        let mut last = 0.0f32;
        for p in &s {
            prop_assert!(p.y.is_finite() && p.y.abs() <= SCORE_LIMIT);
            prop_assert!(p.x > last);
            prop_assert!((p.x * 2.0).fract() == 0.0 && p.x >= 1.0);
            last = p.x;
        }
        // Inverting both sides negates every value (the clamp is symmetric).
        let a = ys(&g, NONE);
        let b = ys(&g, Invert { white: true, black: true });
        prop_assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            prop_assert_eq!(*x, -*y);
        }
    }

    #[test]
    fn time_series_invariants(comments in proptest::collection::vec(comment_strategy(), 0..30)) {
        let refs: Vec<&str> = comments.iter().map(String::as_str).collect();
        let g = game_with(&refs);
        let t = time_series(&g);
        prop_assert!(t.per_move.len() <= comments.len());
        prop_assert_eq!(t.per_move.len(), t.white_total.len() + t.black_total.len());
        let mut last = 0.0f32;
        for p in &t.per_move {
            prop_assert!(p.x > last);
            last = p.x;
        }
        for totals in [&t.white_total, &t.black_total] {
            prop_assert!(totals.windows(2).all(|w| w[0].1 <= w[1].1 && w[0].0 < w[1].0));
        }
        let white_sum: u64 = t.per_move.iter().filter(|p| p.x.fract() == 0.0).map(|p| u64::from(p.seconds)).sum();
        prop_assert_eq!(t.white_total.last().map_or(0.0, |p| p.1), white_sum as f32);
    }
}
