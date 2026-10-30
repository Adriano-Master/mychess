//! M4-7 contract tests of the graph in the application state (rule 10,
//! D872, D875): cache, real PGNs, clicks.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::cast_precision_loss
)]

mod m4_7_wire_common;

use m4_7_wire_common::*;
use rstest::rstest;

#[test]
fn a_closed_panel_never_builds_even_after_navigation_and_prefs() {
    let mut state = state_with(LICHESS);
    send(&mut state, GraphMessage::SetKind(GraphKind::Time));
    nav(&mut state, Nav::End);
    assert!(state.graph().view().is_none());
    assert_eq!(state.graph().builds(), 0);
    assert_eq!(state.graph().current_x(), None);
}

#[test]
fn lichess_game_shows_scores_per_ply_with_title() {
    let mut state = state_with(LICHESS);
    open(&mut state);
    let v = state.graph().view().expect("view");
    assert_eq!(v.title, "alice (1500) - bob (1480) (1-0)");
    assert_eq!(v.plies, 6);
    // Mate in 3 for Black (#-3) is limited to -10 (D484).
    let want = [0.17, 0.25, 0.3, -0.4, 0.5, -10.0];
    assert_eq!(v.bars.len(), 6);
    for (i, (x, y)) in v.bars.iter().enumerate() {
        assert!(close_to(*x, 1.0 + i as f32 * 0.5), "x of bar {i}: {x}");
        assert!(close_to(*y, want[i]), "y of bar {i}: {y} want {}", want[i]);
    }
    assert!(!v.empty);
}

#[test]
fn lichess_clocks_give_move_times_in_the_time_kind() {
    let mut state = state_with(LICHESS);
    send(&mut state, GraphMessage::SetKind(GraphKind::Time));
    open(&mut state);
    let v = state.graph().view().expect("view");
    // D485: emt = previous clock - clock + 3; start clock ceil(300.1) = 301.
    let ys: Vec<f32> = v.bars.iter().map(|b| b.1).collect();
    assert_eq!(ys, vec![4.0, 4.0, 5.0, 8.0, 11.0, 5.0]);
}

#[test]
fn only_clocks_make_auto_choose_times() {
    let pgn = "[TimeControl \"60+0\"]\n\n1. e4 {[%clk 0:01:00]} e5 {[%clk 0:01:00]} \
               2. Nf3 {[%clk 0:00:50]} Nc6 {[%clk 0:00:55]} *\n";
    let mut state = state_with(pgn);
    open(&mut state);
    let v = state.graph().view().expect("view");
    assert_eq!(v.kind, GraphKind::Time);
    assert!(!v.empty);
    assert_eq!(v.bars.len(), 4);
}

#[test]
fn score_combo_with_only_times_draws_lines_and_stays_in_the_axis() {
    let pgn = "[TimeControl \"60+0\"]\n\n1. e4 {[%emt 0:00:05]} e5 {[%emt 0:00:07]} \
               2. Nf3 {[%emt 0:00:09]} Nc6 {[%emt 0:00:02]} 3. Bb5 {[%emt 0:00:03]} a6 {[%emt 0:00:04]} *\n";
    let mut state = state_with(pgn);
    send(&mut state, GraphMessage::SetKind(GraphKind::ScoreCombo));
    open(&mut state);
    let v = state.graph().view().expect("view");
    assert!(!v.white_line.is_empty() && !v.black_line.is_empty());
    for &(_, y) in v.white_line.iter().chain(&v.black_line) {
        assert!(
            y.is_finite() && y >= v.y_axis.min && y <= v.y_axis.max,
            "{y}"
        );
    }
}

#[test]
fn a_game_without_scores_or_times_is_empty() {
    let mut state = state_with("1. e4 e5 2. Nf3 *\n");
    open(&mut state);
    let v = state.graph().view().expect("view");
    assert!(v.empty);
    assert!(v.bars.is_empty());
    assert_eq!(v.plies, 3);
}

#[test]
fn navigation_never_rebuilds_but_the_bar_follows() {
    let mut state = state_with(LICHESS);
    open(&mut state);
    let (builds, stamp) = (state.graph().builds(), state.graph().stamp());
    nav(&mut state, Nav::GameEnd);
    assert_eq!(state.graph().current_x(), Some(3.5));
    nav(&mut state, Nav::GoToMainlinePly(2));
    assert_eq!(state.graph().current_x(), Some(1.5));
    nav(&mut state, Nav::Start);
    assert_eq!(state.graph().current_x(), None);
    send(&mut state, GraphMessage::SetOpen(true));
    assert_eq!(state.graph().builds(), builds);
    assert_eq!(state.graph().stamp(), stamp);
}

#[test]
fn editing_a_comment_rebuilds_with_the_new_value() {
    let mut state = state_with(LICHESS);
    open(&mut state);
    let builds = state.graph().builds();
    let node = state
        .session
        .game()
        .children(state.session.game().root())
        .unwrap()[0];
    send(
        &mut state,
        Message::Game(GameMessage::SetComment {
            node,
            text: "[%eval 2.5]".to_owned(),
        }),
    );
    assert_eq!(state.graph().builds(), builds + 1);
    let v = state.graph().view().unwrap();
    assert!(close_to(v.bars[0].1, 2.5), "{:?}", v.bars[0]);
}

#[test]
fn replacing_the_game_rebuilds_and_clears_the_old_series() {
    let mut state = state_with(LICHESS);
    open(&mut state);
    let builds = state.graph().builds();
    state.session.replace_game(game_of("1. d4 *\n"));
    nav(&mut state, Nav::Start);
    assert_eq!(state.graph().builds(), builds + 1);
    let v = state.graph().view().unwrap();
    assert!(v.empty && v.bars.is_empty());
    assert_eq!(v.plies, 1);
    assert!(
        !v.title.contains("alice") && !v.title.contains("1-0"),
        "{}",
        v.title
    );
}

#[test]
fn closing_discards_and_reopening_rebuilds() {
    let mut state = state_with(LICHESS);
    open(&mut state);
    let builds = state.graph().builds();
    send(&mut state, GraphMessage::SetOpen(false));
    assert!(state.graph().view().is_none());
    assert_eq!(state.graph().current_x(), None);
    send(&mut state, GraphMessage::Toggle);
    assert!(state.graph().is_open());
    assert_eq!(state.graph().builds(), builds + 1);
}

#[test]
fn each_preference_change_rebuilds_once_and_a_repeat_does_not() {
    let mut state = state_with(LICHESS);
    open(&mut state);
    let mut builds = state.graph().builds();
    for msg in [
        GraphMessage::SetKind(GraphKind::Score),
        GraphMessage::ToggleInvertWhite,
        GraphMessage::ToggleInvertBlack,
    ] {
        send(&mut state, msg);
        builds += 1;
        assert_eq!(state.graph().builds(), builds, "{msg:?}");
    }
    // D937: the bar of the current move is not part of the series, so the
    // highlight does not rebuild the view (nor change its stamp).
    let stamp = state.graph().stamp();
    send(&mut state, GraphMessage::ToggleHighlight);
    assert_eq!(state.graph().builds(), builds, "ToggleHighlight");
    assert_eq!(state.graph().stamp(), stamp);
    send(&mut state, GraphMessage::SetKind(GraphKind::Score));
    assert_eq!(state.graph().builds(), builds);
}

#[test]
fn inversion_follows_the_scid_convention_through_the_state() {
    let mut state = state_with(LICHESS);
    send(&mut state, GraphMessage::SetKind(GraphKind::Score));
    open(&mut state);
    let base: Vec<f32> = state
        .graph()
        .view()
        .unwrap()
        .bars
        .iter()
        .map(|b| b.1)
        .collect();
    // "Invert White" inverts what follows Black's moves: bars 2, 4, 6.
    send(&mut state, GraphMessage::ToggleInvertWhite);
    let w: Vec<f32> = state
        .graph()
        .view()
        .unwrap()
        .bars
        .iter()
        .map(|b| b.1)
        .collect();
    for i in 0..6 {
        let want = if i % 2 == 1 { -base[i] } else { base[i] };
        assert!(
            close_to(w[i], want),
            "white-inverted bar {i}: {} want {want}",
            w[i]
        );
    }
    send(&mut state, GraphMessage::ToggleInvertWhite);
    send(&mut state, GraphMessage::ToggleInvertBlack);
    let b: Vec<f32> = state
        .graph()
        .view()
        .unwrap()
        .bars
        .iter()
        .map(|b| b.1)
        .collect();
    for i in 0..6 {
        let want = if i % 2 == 0 { -base[i] } else { base[i] };
        assert!(
            close_to(b[i], want),
            "black-inverted bar {i}: {} want {want}",
            b[i]
        );
    }
}

#[rstest]
#[case(0.0, 0)]
#[case(0.5, 0)]
#[case(0.74, 0)]
#[case(1.0, 1)]
#[case(1.5, 2)]
#[case(2.0, 3)]
#[case(3.5, 6)]
#[case(4.9, 6)]
#[case(5.0, 6)]
fn a_click_goes_to_the_mainline_ply(#[case] x: f32, #[case] want: u32) {
    let mut state = state_with(LICHESS);
    open(&mut state);
    nav(&mut state, Nav::GoToMainlinePly(4));
    send(&mut state, GraphMessage::Click(x));
    assert_eq!(ply(&state), want, "click at {x}");
}

#[rstest]
#[case(f32::NAN)]
#[case(f32::INFINITY)]
#[case(f32::NEG_INFINITY)]
#[case(-0.1)]
#[case(-1000.0)]
#[case(5.5)]
#[case(1.0e9)]
fn a_click_outside_or_not_finite_is_ignored(#[case] x: f32) {
    let mut state = state_with(LICHESS);
    open(&mut state);
    nav(&mut state, Nav::GoToMainlinePly(4));
    send(&mut state, GraphMessage::Click(x));
    assert_eq!(ply(&state), 4, "click at {x}");
}

#[test]
fn a_click_with_the_panel_closed_does_nothing() {
    let mut state = state_with(LICHESS);
    send(&mut state, GraphMessage::Click(2.0));
    assert_eq!(ply(&state), 0);
}

#[test]
fn a_click_on_an_empty_game_does_not_panic() {
    let mut state = AppState::default();
    open(&mut state);
    for x in [0.0, 0.5, 1.0, 2.5, f32::NAN] {
        send(&mut state, GraphMessage::Click(x));
    }
    assert_eq!(ply(&state), 0);
    assert!(state.graph().view().unwrap().empty);
}

const WITH_VARIATION: &str = "1. e4 {[%eval 0.2]} e5 {[%eval 0.1]} (1... c5 {[%eval 0.4]} 2. Nf3 {[%eval 0.3]}) \
    2. Nf3 {[%eval 0.3]} Nc6 {[%eval 0.2]} *\n";

#[test]
fn variations_do_not_enter_the_series_and_have_no_current_bar() {
    let mut state = state_with(WITH_VARIATION);
    open(&mut state);
    assert_eq!(state.graph().view().unwrap().bars.len(), 4);
    nav(&mut state, Nav::GoToMainlinePly(1));
    nav(&mut state, Nav::EnterVariation(1));
    assert_eq!(ply(&state), 2);
    assert_eq!(state.graph().current_x(), None);
    nav(&mut state, Nav::Forward(1));
    assert_eq!(state.graph().current_x(), None);
}

#[test]
fn a_click_from_inside_a_variation_returns_to_the_main_line() {
    let mut state = state_with(WITH_VARIATION);
    open(&mut state);
    nav(&mut state, Nav::GoToMainlinePly(1));
    nav(&mut state, Nav::EnterVariation(1));
    send(&mut state, GraphMessage::Click(2.0));
    assert_eq!(ply(&state), 3);
    assert_eq!(state.graph().current_x(), Some(2.0));
}

#[test]
fn hostile_eval_comments_do_not_panic_and_give_no_point() {
    let pgn = "1. e4 {[%eval ]} e5 {[%eval abc]} 2. Nf3 {[%eval 1e999]} Nc6 {[%eval NaN]} \
               3. Bb5 {[%eval inf]} a6 {[%eval 99999999999999999999]} *\n";
    let mut state = state_with(pgn);
    open(&mut state);
    let v = state.graph().view().unwrap();
    for &(x, y) in &v.bars {
        assert!(x.is_finite() && y.is_finite() && y.abs() <= 10.0, "{x} {y}");
    }
}

#[test]
fn extreme_clock_values_keep_the_axes_bounded() {
    let pgn = "1. e4 {[%emt 99999:59:59]} e5 {[%emt 0:00:01]} 2. Nf3 {[%emt 0:00:02]} \
               Nc6 {[%emt 0:00:02]} 3. Bb5 {[%emt 0:00:02]} a6 {[%emt 0:00:02]} *\n";
    let mut state = state_with(pgn);
    send(&mut state, GraphMessage::SetKind(GraphKind::TimeCombo));
    open(&mut state);
    let v = state.graph().view().unwrap();
    let labels = v.y_axis.label_ticks().count();
    let minor = v.y_axis.minor_ticks().count();
    assert!(
        labels <= 200 && minor <= mychess_app::graphs::MAX_TICKS,
        "{labels} {minor}"
    );
    assert!(v.y_axis.max >= 359_999.0 - 1.0);
}

#[test]
fn six_thousand_plies_build_and_click_without_a_real_clock() {
    let mut state = AppState::default();
    state
        .session
        .replace_game(shuffle_game(6000, "[%eval 0.20] [%clk 1:00:00]"));
    open(&mut state);
    let v = state.graph().view().unwrap();
    assert_eq!(v.plies, 6000);
    assert_eq!(v.bars.len(), 6000);
    assert!(v.x_axis.label_ticks().count() <= 200);
    send(&mut state, GraphMessage::Click(1500.5));
    assert_eq!(ply(&state), 3000);
}

#[test]
fn the_graph_message_converts_into_message() {
    let m: Message = GraphMessage::Toggle.into();
    assert!(matches!(m, Message::Graph(GraphMessage::Toggle)));
}
