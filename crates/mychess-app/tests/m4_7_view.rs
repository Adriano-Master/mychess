//! Contract tests of `choose_kind`, `score_graph_view` and `graph_title`
//! (M4-7). Counts follow Scid's `graphs.tcl:674-763`, where `llength` counts
//! the elements of the flat `x y` list, i.e. two per point:
//! Auto -> scores when `llength < 10` is false, i.e. >= 5 points;
//! lines need `llength white + llength black > 10`, i.e. > 5 points;
//! `Score Combo` uses the time y axis when `llength < 5`, i.e. < 3 points.
//! (The spec text "< 5" / "> 10 values" counts list elements as points; the
//! Scid code is the reference, decision D760.)

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

use m4_7_common::{emt, game_with, node_at, shuffle_game};
use mychess_app::graphs::{
    GraphKind, GraphPrefs, Invert, ScoreGraphView, choose_kind, current_x, graph_title,
    score_graph_view, time_step,
};
use mychess_core::{Elo, Game, GameResult, Position, Rating, RatingType, Tags};
use proptest::prelude::*;
use rstest::rstest;

const ALL: [GraphKind; 5] = [
    GraphKind::Auto,
    GraphKind::Score,
    GraphKind::Time,
    GraphKind::ScoreCombo,
    GraphKind::TimeCombo,
];

fn prefs(kind: GraphKind) -> GraphPrefs {
    GraphPrefs {
        kind,
        ..GraphPrefs::default()
    }
}

/// The view no longer depends on the cursor; the argument is kept so the
/// call sites read as before.
fn view(g: &Game, _cursor_ply: usize, kind: GraphKind) -> ScoreGraphView {
    score_graph_view(g, prefs(kind))
}

/// The current bar for a cursor at main-line ply `cursor_ply`.
fn cur(g: &Game, cursor_ply: usize, kind: GraphKind) -> Option<f32> {
    current_x(g, node_at(g, cursor_ply), prefs(kind))
}

/// `n` plies, ply `i` with an eval when `i < evals` and an emt `t` seconds
/// when `i < emts`.
fn mixed(n: usize, evals: usize, emts: usize, t: u32) -> Game {
    shuffle_game(n, |i| {
        let mut s = String::new();
        if i < evals {
            s.push_str(&format!("[%eval {}.5] ", (i % 5) as i32 - 2));
        }
        if i < emts {
            s.push_str(&emt(t));
        }
        Some(s).filter(|s| !s.is_empty())
    })
}

// --------------------------------------------------------- choose_kind

#[rstest]
#[case(4, 5, GraphKind::Time)]
#[case(3, 4, GraphKind::Time)]
#[case(0, 1, GraphKind::Time)]
#[case(4, 4, GraphKind::Score)]
#[case(4, 3, GraphKind::Score)]
#[case(5, 9, GraphKind::Score)]
#[case(5, 1000, GraphKind::Score)]
#[case(0, 0, GraphKind::Score)]
#[case(1, 1, GraphKind::Score)]
fn auto_follows_scid_counts(#[case] s: usize, #[case] t: usize, #[case] want: GraphKind) {
    assert_eq!(choose_kind(GraphKind::Auto, s, t), want);
}

#[test]
fn explicit_kinds_are_returned_unchanged() {
    for kind in ALL.into_iter().skip(1) {
        for (s, t) in [(0, 0), (0, 9), (9, 0), (4, 5), (100, 100)] {
            assert_eq!(choose_kind(kind, s, t), kind);
        }
    }
}

#[test]
fn auto_never_resolves_to_auto() {
    for s in 0..10 {
        for t in 0..10 {
            assert!(matches!(
                choose_kind(GraphKind::Auto, s, t),
                GraphKind::Score | GraphKind::Time
            ));
        }
    }
}

// ------------------------------------------------------------ the view

#[test]
fn empty_game_view_is_empty_with_default_axes() {
    let g = Game::new();
    for kind in ALL {
        let v = view(&g, 0, kind);
        assert!(v.empty && v.bars.is_empty(), "{kind:?}");
        assert!(v.white_line.is_empty() && v.black_line.is_empty());
        assert_eq!(cur(&g, 0, kind), None);
        assert_eq!(v.plies, 0);
        assert!(v.x_axis.min < v.x_axis.max && v.y_axis.min < v.y_axis.max);
    }
}

#[test]
fn game_without_data_is_empty_for_every_kind() {
    let g = game_with(&["hello", "", "no data"]);
    for kind in ALL {
        let v = view(&g, 3, kind);
        assert!(v.empty, "{kind:?}");
        assert_eq!(v.plies, 3);
    }
}

#[test]
fn only_evals_auto_shows_score_bars_without_lines() {
    let g = game_with(&["[%eval 0.5]", "[%eval -2.5]", "", "[%eval 30]"]);
    let v = view(&g, 4, GraphKind::Auto);
    assert_eq!(v.kind, GraphKind::Score);
    assert_eq!(v.bars, vec![(1.0, 0.5), (1.5, -2.5), (2.5, 10.0)]);
    assert!(v.white_line.is_empty() && v.black_line.is_empty());
    assert!(!v.empty);
}

#[test]
fn only_times_auto_shows_time_bars() {
    let g = game_with(&[&emt(3), &emt(4), &emt(5)]);
    let v = view(&g, 0, GraphKind::Auto);
    assert_eq!(v.kind, GraphKind::Time);
    assert_eq!(v.bars, vec![(1.0, 3.0), (1.5, 4.0), (2.0, 5.0)]);
}

#[test]
fn auto_switches_to_time_with_four_scores_and_five_times() {
    let g = mixed(12, 4, 5, 8);
    assert_eq!(view(&g, 0, GraphKind::Auto).kind, GraphKind::Time);
}

#[test]
fn auto_keeps_scores_with_five_scores_and_many_times() {
    let g = mixed(12, 5, 12, 8);
    assert_eq!(view(&g, 0, GraphKind::Auto).kind, GraphKind::Score);
}

#[test]
fn auto_ties_go_to_scores() {
    let g = mixed(6, 4, 4, 8);
    assert_eq!(view(&g, 0, GraphKind::Auto).kind, GraphKind::Score);
}

/// `graphs.tcl:718`: lines need more than 5 points in total (> 10 list
/// elements), on 5 points they do not appear.
#[rstest]
#[case(5, false)]
#[case(6, true)]
#[case(12, true)]
fn lines_need_more_than_five_time_points(#[case] n: usize, #[case] lines: bool) {
    let g = mixed(n, 0, n, 10);
    for kind in [GraphKind::Auto, GraphKind::TimeCombo] {
        let v = view(&g, 0, kind);
        assert_eq!(!v.white_line.is_empty(), lines, "{kind:?} with {n} times");
    }
}

#[test]
fn auto_draws_lines_but_plain_score_and_time_do_not() {
    let g = mixed(12, 0, 12, 10);
    assert!(!view(&g, 0, GraphKind::Auto).white_line.is_empty());
    assert!(view(&g, 0, GraphKind::Time).white_line.is_empty());
    let g = mixed(12, 12, 12, 10);
    assert!(view(&g, 0, GraphKind::Score).white_line.is_empty());
    assert!(!view(&g, 0, GraphKind::ScoreCombo).white_line.is_empty());
}

#[test]
fn time_kind_ignores_scores_and_score_kind_ignores_times() {
    let g = mixed(8, 8, 8, 4);
    let t = view(&g, 0, GraphKind::Time);
    assert_eq!(t.kind, GraphKind::Time);
    assert!(t.bars.iter().all(|b| b.1 == 4.0));
    let s = view(&g, 0, GraphKind::Score);
    assert_eq!(s.kind, GraphKind::Score);
    assert!(s.bars.iter().all(|b| b.1.abs() <= 2.5));
}

#[test]
fn time_combo_scales_both_lines_to_the_tallest_bar() {
    // Totals are 60 each; tallest bar is 10 (Scid: max/maxLines).
    let g = mixed(12, 0, 12, 10);
    let v = view(&g, 0, GraphKind::TimeCombo);
    assert_eq!(v.kind, GraphKind::TimeCombo);
    assert_eq!(v.white_line.len(), 6);
    assert_eq!(v.black_line.len(), 6);
    assert_eq!(v.white_line.last().map(|p| p.1), Some(10.0));
    assert_eq!(v.black_line.last().map(|p| p.1), Some(10.0));
}

#[test]
fn lines_scale_by_the_larger_side_total() {
    // White spends 10 per move, Black 5: Black's line ends at half.
    let g = shuffle_game(12, |i| Some(emt(if i % 2 == 0 { 10 } else { 5 })));
    let v = view(&g, 0, GraphKind::TimeCombo);
    assert_eq!(v.white_line.last().map(|p| p.1), Some(10.0));
    assert_eq!(v.black_line.last().map(|p| p.1), Some(5.0));
}

#[test]
fn score_combo_scales_lines_to_the_tallest_score_bar() {
    let g = shuffle_game(12, |i| {
        Some(format!("[%eval {}] {}", [1, 2, 3, 4][i % 4], emt(10)))
    });
    let v = view(&g, 0, GraphKind::ScoreCombo);
    assert_eq!(v.kind, GraphKind::ScoreCombo);
    assert_eq!(v.white_line.last().map(|p| p.1), Some(4.0));
}

/// `graphs.tcl:736-743`: `Score Combo` with `llength < 5` (< 3 points)
/// scales the lines to `maxEmt` and uses the time y axis.
#[test]
fn score_combo_with_two_scores_uses_time_axis_and_emt_scale() {
    let g = shuffle_game(12, |i| {
        Some(if i < 2 {
            format!("[%eval 2] {}", emt(30))
        } else {
            emt(30)
        })
    });
    let v = view(&g, 0, GraphKind::ScoreCombo);
    assert_eq!(v.bars.len(), 2);
    assert_eq!(v.y_axis.min, 0.0);
    assert_eq!(v.y_axis.label_step, time_step(30));
    assert_eq!(v.white_line.last().map(|p| p.1), Some(30.0));
}

#[test]
fn score_combo_with_three_scores_uses_score_axis() {
    let g = shuffle_game(12, |i| {
        Some(if i < 3 {
            format!("[%eval -2] {}", emt(30))
        } else {
            emt(30)
        })
    });
    let v = view(&g, 0, GraphKind::ScoreCombo);
    assert_eq!(v.bars.len(), 3);
    assert!(v.y_axis.min < 0.0, "score axis includes negatives");
    assert_eq!(v.y_axis.label_step, 1.0);
    // Scaled to the tallest bar, `max` starts at 1 in Scid.
    assert_eq!(v.white_line.last().map(|p| p.1), Some(1.0));
}

/// Scid's y axis autoscales to the data (the tick table only sets the
/// spacing), so a negative score bar is never cut off. In the MyChess view the
/// time axis starts at 0 and the bar of `#-1` (-10) falls outside it.
#[test]
fn score_combo_negative_bars_stay_inside_the_time_axis() {
    let g = shuffle_game(12, |i| {
        Some(if i == 2 {
            format!("[%eval -3] {}", emt(2))
        } else {
            emt(2)
        })
    });
    let v = view(&g, 0, GraphKind::ScoreCombo);
    assert_eq!(v.bars, vec![(2.0, -3.0)]);
    assert!(
        v.y_axis.min <= -3.0,
        "bar at -3 is outside the y axis {:?}",
        v.y_axis
    );
}

#[test]
fn all_zero_times_give_flat_lines_without_nan() {
    let g = mixed(12, 0, 12, 0);
    for kind in [GraphKind::Auto, GraphKind::TimeCombo, GraphKind::ScoreCombo] {
        let v = view(&g, 0, kind);
        for p in v.white_line.iter().chain(&v.black_line) {
            assert!(p.1.is_finite() && p.1 == 0.0, "{kind:?}: {p:?}");
        }
        assert!(v.y_axis.max > v.y_axis.min);
    }
}

// Axes follow what is drawn (D762).

#[test]
fn score_axis_is_used_when_auto_shows_a_single_score() {
    let g = game_with(&["[%eval 0.5]"]);
    let v = view(&g, 0, GraphKind::Auto);
    assert_eq!(v.kind, GraphKind::Score);
    assert_eq!((v.y_axis.min, v.y_axis.max), (-1.0, 1.0));
    assert_eq!(v.y_axis.label_step, 1.0);
    assert_eq!(v.y_axis.major_step, Some(5.0));
}

#[test]
fn score_axis_grows_with_data_and_stops_at_ten() {
    let g = game_with(&["[%eval -3.2]", "[%eval 7.1]"]);
    let v = view(&g, 0, GraphKind::Score);
    assert_eq!((v.y_axis.min, v.y_axis.max), (-4.0, 8.0));
    let g = game_with(&["[%eval -99]", "[%eval 99]"]);
    let v = view(&g, 0, GraphKind::Score);
    assert_eq!((v.y_axis.min, v.y_axis.max), (-10.0, 10.0));
    let g = game_with(&["[%eval 0]"]);
    let v = view(&g, 0, GraphKind::Score);
    assert_eq!((v.y_axis.min, v.y_axis.max), (-1.0, 1.0));
}

#[test]
fn auto_time_axis_follows_the_times() {
    let g = game_with(&[&emt(45), &emt(3)]);
    let v = view(&g, 0, GraphKind::Auto);
    assert_eq!(v.kind, GraphKind::Time);
    assert_eq!(v.y_axis.min, 0.0);
    assert_eq!(v.y_axis.label_step, 5.0);
    assert!(v.y_axis.max >= 45.0);
}

#[rstest]
#[case(20, 2.0)]
#[case(21, 5.0)]
#[case(50, 5.0)]
#[case(51, 10.0)]
#[case(100, 10.0)]
#[case(101, 20.0)]
#[case(500, 20.0)]
#[case(501, 50.0)]
#[case(1000, 50.0)]
#[case(1001, 100.0)]
#[case(2000, 100.0)]
#[case(2001, 200.0)]
fn time_axis_step_thresholds(#[case] max_emt: u32, #[case] step: f32) {
    let g = game_with(&[&emt(max_emt), &emt(1)]);
    let v = view(&g, 0, GraphKind::Time);
    assert_eq!(v.y_axis.label_step, step);
    assert_eq!(time_step(max_emt), step);
    assert!(v.y_axis.max >= max_emt as f32);
    assert!(
        v.y_axis.max < max_emt as f32 + step,
        "no more than one step of headroom"
    );
    assert_eq!((v.y_axis.max / step).fract(), 0.0);
}

#[rstest]
#[case(12, 10.0)] // max x 6.5 -> 10
#[case(8, 5.0)] // max x 4.5 -> 5
#[case(9, 10.0)] // max x 5.0 -> 10 (bar at 5 is 0.35 wide each side)
#[case(1, 5.0)]
fn x_axis_covers_the_last_bar_in_steps_of_five(#[case] n: usize, #[case] max: f32) {
    let comments = vec!["[%eval 1]"; n];
    let g = game_with(&comments);
    let v = view(&g, 0, GraphKind::Score);
    assert_eq!(v.x_axis.min, 0.0);
    assert_eq!(v.x_axis.max, max);
    assert_eq!(v.x_axis.minor_step, 1.0);
    assert_eq!(v.x_axis.major_step, Some(5.0));
}

#[test]
fn inversion_preferences_reach_the_bars() {
    let g = game_with(&["[%eval 1]", "[%eval 1]"]);
    let v = score_graph_view(
        &g,
        GraphPrefs {
            kind: GraphKind::Score,
            invert: Invert {
                white: false,
                black: true,
            },
            highlight_current: true,
            ..GraphPrefs::default()
        },
    );
    assert_eq!(v.bars, vec![(1.0, -1.0), (1.5, 1.0)]);
}

// ----------------------------------------------------------- current bar

#[test]
fn current_bar_follows_the_cursor_on_the_main_line() {
    let g = game_with(&["[%eval 1]"; 6]);
    assert_eq!(cur(&g, 0, GraphKind::Score), None, "root");
    assert_eq!(cur(&g, 1, GraphKind::Score), Some(1.0));
    assert_eq!(cur(&g, 2, GraphKind::Score), Some(1.5));
    assert_eq!(cur(&g, 6, GraphKind::Score), Some(3.5));
}

#[test]
fn current_bar_is_reported_even_for_a_move_without_a_score() {
    let g = game_with(&["[%eval 1]", "", "[%eval 1]"]);
    assert_eq!(cur(&g, 2, GraphKind::Score), Some(1.5));
}

#[test]
fn current_bar_can_be_switched_off() {
    let g = game_with(&["[%eval 1]"; 3]);
    let off = GraphPrefs {
        highlight_current: false,
        ..GraphPrefs::default()
    };
    assert_eq!(current_x(&g, node_at(&g, 2), off), None);
}

#[test]
fn cursor_in_a_variation_has_no_current_bar() {
    let mut g = game_with(&["[%eval 1]"; 3]);
    let pos = Position::new();
    let var = g
        .add_move(g.root(), pos.parse_uci("d2d4").expect("uci"))
        .expect("variation");
    for kind in ALL {
        assert_eq!(current_x(&g, var, prefs(kind)), None);
    }
    // The main line is unchanged by the variation.
    assert_eq!(view(&g, 0, GraphKind::Score).plies, 3);
}

#[test]
fn cursor_from_another_game_is_harmless() {
    let g = game_with(&["[%eval 1]"; 2]);
    let other = game_with(&["", "", "", ""]);
    assert_eq!(
        current_x(&g, node_at(&other, 3), prefs(GraphKind::Score)),
        None
    );
}

#[test]
fn score_combo_with_only_times_is_not_empty() {
    let g = mixed(12, 0, 12, 30);
    let v = view(&g, 0, GraphKind::ScoreCombo);
    assert!(v.bars.is_empty() && !v.white_line.is_empty());
    assert!(!v.empty);
}

#[test]
fn huge_move_time_keeps_few_ticks() {
    let g = game_with(&["[%emt 99999:59:59]", &emt(1)]);
    let v = view(&g, 0, GraphKind::Time);
    assert!(v.y_axis.label_ticks().count() <= 50);
    assert!(v.y_axis.minor_ticks().count() <= 200);
}

// -------------------------------------------------------------- title

fn rating(v: u16) -> Option<Rating> {
    Some(Rating::new(Elo::new(v).expect("elo"), RatingType::Elo))
}

#[test]
fn title_omits_missing_ratings_zero_ratings_and_unknown_result() {
    let mut t = Tags::default();
    t.white = "Anna".into();
    t.black = "Bob".into();
    assert_eq!(graph_title(&t), "Anna - Bob");
    t.white_rating = rating(0);
    t.black_rating = rating(0);
    assert_eq!(graph_title(&t), "Anna - Bob");
}

#[rstest]
#[case(GameResult::White, " (1-0)")]
#[case(GameResult::Black, " (0-1)")]
#[case(GameResult::Draw, " (1/2-1/2)")]
#[case(GameResult::Unknown, "")]
fn title_results(#[case] result: GameResult, #[case] suffix: &str) {
    let mut t = Tags::default();
    t.white = "Anna".into();
    t.black = "Bob".into();
    t.result = result;
    assert_eq!(graph_title(&t), format!("Anna - Bob{suffix}"));
}

#[test]
fn title_with_ratings_and_unicode_names() {
    let mut t = Tags::default();
    t.white = "Zoë Müller".into();
    t.black = "李小龙 (☯)".into();
    t.white_rating = rating(2500);
    t.black_rating = rating(2400);
    t.result = GameResult::Black;
    assert_eq!(
        graph_title(&t),
        "Zoë Müller (2500) - 李小龙 (☯) (2400) (0-1)"
    );
}

#[test]
fn view_title_comes_from_the_game_tags() {
    let mut g = game_with(&["[%eval 1]"]);
    g.tags_mut().white = "Ana".into();
    g.tags_mut().black = "Beto".into();
    g.tags_mut().result = GameResult::Draw;
    assert_eq!(view(&g, 0, GraphKind::Auto).title, "Ana - Beto (1/2-1/2)");
}

// -------------------------------------------------------- large and props

#[test]
fn view_of_a_game_with_thousands_of_plies() {
    let n = 20_000;
    let g = shuffle_game(n, |i| {
        Some(format!("[%eval {}.5] {}", i % 9, emt((i % 40) as u32)))
    });
    for kind in ALL {
        let v = view(&g, n, kind);
        assert_eq!(v.plies, n);
        assert_eq!(v.bars.len(), n);
        let c = cur(&g, n, kind);
        assert_eq!(c, Some(n as f32 * 0.5 + 0.5));
        assert!(v.x_axis.max >= c.unwrap_or(0.0));
        assert!(v.x_axis.label_ticks().count() <= 21);
        assert!(v.x_axis.minor_ticks().count() <= 201);
    }
}

fn comment_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        (-300i32..300).prop_map(|v| format!(
            "[%eval {}{}.{}]",
            if v < 0 { "-" } else { "" },
            (v / 10).abs(),
            v.abs() % 10
        )),
        (-3i32..3).prop_map(|v| format!("[%eval #{v}]")),
        (0u32..4000).prop_map(emt),
        ((-300i32..300), (0u32..4000)).prop_map(|(v, t)| format!(
            "[%eval {}.0] {}",
            v / 10,
            emt(t)
        )),
        "\\PC{0,20}",
    ]
}

proptest! {
    #[test]
    fn view_invariants(
        comments in proptest::collection::vec(comment_strategy(), 0..40),
        kind in 0usize..5,
        cursor in 0usize..45,
        hl in any::<bool>(),
        iw in any::<bool>(),
        ib in any::<bool>(),
    ) {
        let refs: Vec<&str> = comments.iter().map(String::as_str).collect();
        let g = game_with(&refs);
        let n = comments.len();
        let cursor = cursor.min(n);
        let p = GraphPrefs { kind: ALL[kind], invert: Invert { white: iw, black: ib }, highlight_current: hl, ..GraphPrefs::default() };
        let v = score_graph_view(&g, p);
        prop_assert_ne!(v.kind, GraphKind::Auto);
        prop_assert_eq!(v.plies, n);
        prop_assert_eq!(v.empty, v.bars.is_empty() && v.white_line.is_empty() && v.black_line.is_empty());
        prop_assert!(v.x_axis.min < v.x_axis.max && v.y_axis.min < v.y_axis.max);
        let drawn = v.bars.iter().chain(&v.white_line).chain(&v.black_line);
        for &(x, y) in drawn {
            prop_assert!(x.is_finite() && y.is_finite());
            prop_assert!(x >= v.x_axis.min && x <= v.x_axis.max, "x {x} outside {:?}", v.x_axis);
            prop_assert!(y >= v.y_axis.min && y <= v.y_axis.max, "y {y} outside {:?} ({:?})", v.y_axis, v.kind);
        }
        if !hl || cursor == 0 {
            prop_assert_eq!(current_x(&g, node_at(&g, cursor), p), None);
        } else {
            prop_assert_eq!(current_x(&g, node_at(&g, cursor), p), Some(cursor as f32 * 0.5 + 0.5));
        }
    }
}
