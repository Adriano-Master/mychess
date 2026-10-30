//! M5-2a contract tests: opening and closing the panel (D981), the table
//! (D968), the empty and unavailable states, the short display.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m5_2_common;

use m5_2_common::*;
use mychess_app::TreeRowView;
use tempfile::tempdir;

// ---- open / close ----

#[test]
fn the_panel_starts_closed_and_draws_nothing() {
    let rig = Rig::new();
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Closed);
    assert!(view.rows.is_empty() && view.total.is_none());
    assert!(!rig.state.tree().is_open());
    assert_eq!(rig.started(), 0);
}

#[test]
fn opening_starts_the_first_search_and_shows_loading_until_it_ends() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let effects = rig.raw(TreeMessage::Open);
    let mut jobs = jobs(effects);
    assert_eq!(jobs.len(), 1, "exactly one search");
    assert!(rig.state.tree().is_open());
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Loading);
    assert!(view.searching);
    // The answer arrives.
    let answer = rig.run_job(jobs.remove(0));
    assert!(rig.raw(answer).is_empty());
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Rows);
    assert!(!view.searching);
    assert_eq!(rig.started(), 1);
}

#[test]
fn toggle_opens_and_closes() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    rig.send(TreeMessage::Toggle);
    assert!(rig.state.tree().is_open());
    assert_eq!(rig.view().body, TreeBody::Rows);
    let effects = rig.raw(TreeMessage::Toggle);
    assert!(effects.is_empty());
    assert!(!rig.state.tree().is_open());
    assert_eq!(rig.view().body, TreeBody::Closed);
}

#[test]
fn opening_twice_does_not_search_again() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let started = rig.started();
    let effects = rig.raw(TreeMessage::Open);
    assert!(effects.is_empty());
    assert_eq!(rig.started(), started);
}

#[test]
fn reopening_forces_a_search_even_if_nothing_changed() {
    // D981: the shown result is dropped on close; reopening shows the
    // current position.
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    assert_eq!(rig.started(), 1);
    rig.send(TreeMessage::Close);
    assert_eq!(rig.view().body, TreeBody::Closed);
    let effects = rig.raw(TreeMessage::Open);
    assert_eq!(tree_searches(&effects), 1);
    assert_eq!(rig.view().body, TreeBody::Loading, "old rows are gone");
}

#[test]
fn open_turns_the_refresh_back_on() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.send(TreeMessage::ToggleAutoRefresh);
    assert!(!rig.view().autorefresh);
    rig.send(TreeMessage::Close);
    let effects = rig.raw(TreeMessage::Open);
    assert!(rig.view().autorefresh);
    assert_eq!(tree_searches(&effects), 1, "a search is forced");
}

#[test]
fn closing_cancels_the_search_in_flight_and_a_late_answer_is_not_shown() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut flight = jobs(rig.raw(TreeMessage::Open));
    rig.raw(TreeMessage::Close);
    assert_eq!(rig.view().body, TreeBody::Closed);
    let answer = rig.run_job(flight.remove(0));
    assert!(
        was_cancelled(&answer),
        "the token was cancelled: {answer:?}"
    );
    assert!(rig.raw(answer).is_empty());
    assert_eq!(rig.view().body, TreeBody::Closed);
    assert!(!rig.state.tree().searching());
}

#[test]
fn a_late_full_answer_after_close_is_still_not_shown() {
    // Even an answer that completed (not cancelled) must be dropped by the
    // generation, or reopening would flash a stale table.
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    let mut flight = jobs(rig.raw(TreeMessage::Open));
    let job = flight.remove(0);
    let answer = rig.run_job(job);
    rig.raw(TreeMessage::Close);
    rig.raw(answer);
    assert_eq!(rig.view().body, TreeBody::Closed);
    assert!(rig.view().rows.is_empty());
}

#[test]
fn close_when_already_closed_is_harmless() {
    let mut rig = Rig::new();
    assert!(rig.raw(TreeMessage::Close).is_empty());
    assert_eq!(rig.view().body, TreeBody::Closed);
}

// ---- the table ----

#[test]
fn the_start_position_lists_one_row_per_first_move_and_the_total() {
    let dir = tempdir().unwrap();
    let rig = Rig::std_with_tree(dir.path());
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Rows);
    assert_eq!(rig.sorted_sans(), ["Nf3", "d4", "e4"]);
    let total = view.total.expect("total");
    assert_eq!(total.games, 5);
    assert_eq!(total.results, [2, 1, 1, 1], "White, Black, draw, no result");
    // (2*2 + 1 + 1) * 500 / 5 = 600 per mille.
    assert_eq!(total.score, (60, 0));
    // draws: (1 * 1000 / 5 + 5) / 10 = 20.
    assert_eq!(total.draw_percent, 20);
    assert_eq!(view.turn, mychess_core::Color::White);
}

#[test]
fn rows_carry_counts_shares_scores_and_results() {
    let dir = tempdir().unwrap();
    let rig = Rig::std_with_tree(dir.path());
    let e4 = rig.row("e4");
    assert_eq!(e4.games, 3);
    assert_eq!(e4.results, [1, 0, 1, 1]);
    assert_eq!(e4.share, (60, 0), "100 * 3 / 5 and (1000 * 3 / 5) % 10");
    // (1 * 2 + 1 + 1) * 500 / 3 = 666 per mille.
    assert_eq!(e4.score, (66, 6));
    // (1 * 1000 / 3 + 5) / 10 = 33.
    assert_eq!(e4.draw_percent, 33);
    let d4 = rig.row("d4");
    assert_eq!((d4.games, d4.score, d4.draw_percent), (1, (0, 0), 0));
    assert_eq!(d4.share, (20, 0));
    let nf3 = rig.row("Nf3");
    assert_eq!((nf3.games, nf3.score), (1, (100, 0)));
}

#[test]
fn the_bar_of_each_row_is_the_one_of_its_numbers() {
    let dir = tempdir().unwrap();
    let rig = Rig::std_with_tree(dir.path());
    for row in rig.view().rows {
        let permille = row.score.0 * 10 + row.score.1;
        assert_eq!(
            row.bar,
            bar_segments(permille, row.draw_percent),
            "{}",
            row.san
        );
    }
    // All wins for White: the white part fills the bar.
    let nf3 = rig.row("Nf3");
    assert_eq!(nf3.bar.white_end, Some(61.0));
    assert_eq!(nf3.bar.black_start, None);
}

#[test]
fn the_default_order_is_by_frequency() {
    let dir = tempdir().unwrap();
    let rig = Rig::std_with_tree(dir.path());
    assert_eq!(rig.sans()[0], "e4", "3 games against 1 and 1");
    assert_eq!(rig.view().sort, TreeSort::Frequency);
}

#[test]
fn rows_are_indexed_in_display_order() {
    let dir = tempdir().unwrap();
    let rig = Rig::std_with_tree(dir.path());
    let indexes: Vec<usize> = rig.view().rows.iter().map(|r| r.index).collect();
    assert_eq!(indexes, [0, 1, 2]);
}

#[test]
fn a_position_reached_by_games_that_end_there_has_a_non_clickable_end_row() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    assert_eq!(rig.sorted_sans(), ["[end]", "c5", "e5"]);
    let end = rig.row("[end]");
    assert!(end.is_end && !end.clickable && !end.hidden);
    assert_eq!(end.games, 1);
    // No result counts as half a point.
    assert_eq!(end.score, (50, 0));
    let total = rig.view().total.expect("total");
    assert_eq!(total.games, 3, "the end row is part of the total");
    for san in ["c5", "e5"] {
        let row = rig.row(san);
        assert!(row.clickable && !row.is_end, "{san}");
    }
}

#[test]
fn no_row_is_hidden_outside_training() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    assert!(rig.view().rows.iter().all(|r| !r.hidden));
    rig.play("e2e4");
    assert!(rig.view().rows.iter().all(|r| !r.hidden));
}

#[test]
fn the_turn_follows_the_side_to_move() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("e2e4");
    assert_eq!(rig.view().turn, mychess_core::Color::Black);
}

#[test]
fn a_position_nobody_reached_shows_the_empty_tree_without_total() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("a2a3");
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Empty);
    assert!(view.rows.is_empty());
    assert!(view.total.is_none(), "the total exists only with moves");
    assert!(!view.searching);
}

#[test]
fn the_tree_recovers_from_empty_when_the_cursor_goes_back() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    rig.play("a2a3");
    assert_eq!(rig.view().body, TreeBody::Empty);
    rig.nav(Nav::Start);
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert_eq!(rig.view().rows.len(), 3);
}

#[test]
fn a_game_with_no_moves_is_an_end_row_only_at_the_start_position() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let pgn = format!("{STD}[Result \"*\"]\n\n*\n\n");
    rig.open_pgn(dir.path(), "z.pgn", &pgn);
    rig.open_tree();
    // Spec rule 3: a zero-ply game enters only at the standard start (an
    // `[end]` node at ply 0).
    assert!(rig.sans().contains(&"[end]".to_owned()), "{:?}", rig.sans());
    assert_eq!(rig.view().total.expect("total").games, 6);
}

// ---- unavailable ----

#[test]
fn an_empty_base_is_unavailable_and_searches_nothing() {
    let mut rig = Rig::new();
    let effects = rig.raw(TreeMessage::Open);
    assert!(effects.is_empty());
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    assert_eq!(rig.started(), 0);
    assert!(!rig.view().searching);
}

#[test]
fn opening_a_base_after_an_unavailable_panel_fills_it() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_tree();
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    rig.open_pgn(dir.path(), "std.pgn", STD);
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert_eq!(rig.view().rows.len(), 3);
}

#[test]
fn closing_the_current_base_makes_the_panel_unavailable_and_stops_searching() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let slot = rig.state.bases.current();
    rig.send(BasesMessage::Close(slot));
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    assert!(!rig.state.tree().searching());
    assert!(rig.view().rows.is_empty());
}

// ---- short display ----

#[test]
fn the_short_display_is_the_default() {
    assert!(Rig::new().view().short);
}

#[test]
fn the_short_display_changes_columns_only_and_never_searches() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::std_with_tree(dir.path());
    let before = rig.view().rows;
    let started = rig.started();
    let effects = rig.raw(TreeMessage::SetShort(false));
    assert!(effects.is_empty());
    assert_eq!(rig.started(), started);
    assert!(!rig.view().short);
    assert_eq!(rig.view().rows, before, "same data, other columns");
    let effects = rig.raw(TreeMessage::SetShort(true));
    assert!(effects.is_empty());
    assert!(rig.view().short);
    assert!(!rig.state.tree().searching());
}

#[test]
fn the_short_flag_is_kept_in_the_preferences() {
    let mut rig = Rig::new();
    rig.raw(TreeMessage::SetShort(false));
    assert!(!rig.state.prefs.tree.short);
}

// ---- rating, performance, year (display data of the long columns) ----

fn rated_base() -> String {
    // e4: ten games with ratings (White 2000, Black 1900), all won by White;
    // d4: nine games rated (below the limit of ten).
    let mut text = String::new();
    // Years: three of 2000, six of 2001 and one without a date.
    for i in 0..10 {
        let date = match i {
            0..=2 => "2000.01.01",
            3..=8 => "2001.01.01",
            _ => "????.??.??",
        };
        text.push_str(&game_pgn("1. e4 e5", "1-0", 2000, 1900, date, None));
    }
    for _ in 0..9 {
        text.push_str(&game_pgn("1. d4 d5", "1-0", 2200, 2100, "2010.01.01", None));
    }
    text
}

#[test]
fn averages_appear_only_from_ten_rated_games() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "rated.si4", &rated_base());
    rig.open_tree();
    let e4 = rig.row("e4");
    assert_eq!(e4.games, 10);
    assert_eq!(e4.avg_elo, Some(2000), "the Elo of the side that moves");
    // Opponent average 1900; all wins: pct = (1000 + 5) / 10 = 100;
    // Performance(1900, 100) = 1900 + 800.
    assert_eq!(e4.perf, Some(2700));
    let d4 = rig.row("d4");
    assert_eq!(d4.games, 9);
    assert_eq!(d4.avg_elo, None);
    assert_eq!(d4.perf, None);
}

#[test]
fn the_average_year_is_rounded_and_ignores_unknown_years() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "rated.si4", &rated_base());
    rig.open_tree();
    // (3 * 2000 + 6 * 2001 + 4) / 9 = 2001 (the exact mean is 2000.67).
    assert_eq!(rig.row("e4").avg_year, Some(2001));
    assert_eq!(rig.row("d4").avg_year, Some(2010));
}

#[test]
fn the_total_row_applies_the_same_limits() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "rated.si4", &rated_base());
    rig.open_tree();
    let total = rig.view().total.expect("total");
    assert_eq!(total.games, 19);
    // 19 rated games of White: (10 * 2000 + 9 * 2200) / 19 = 2094.
    assert_eq!(total.avg_elo, Some(2094));
}

fn _unused(_: &TreeRowView) {}
