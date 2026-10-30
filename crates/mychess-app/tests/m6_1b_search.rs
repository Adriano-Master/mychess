//! Contract tests of the flow of the advanced searches (spec
//! `m6-1-buscas-avancadas.md`, slice M6-1b: D3200, D3202, D3204, D3213,
//! D3232, D3233; decisions of the orchestrator 4 and 5).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_1b_common;

use m6_1b_common::*;
use mychess_app::{BoardKind, CheckKind, MaterialPiece, SideKind};
use mychess_core::Color;
use mychess_db::{AdvancedQuery, BoardMatch, CheckTest, FilterOp};

fn set(rig: &mut Rig, message: AdvancedSearchMessage) {
    rig.send(Message::AdvancedSearch(message));
}

fn control_of(effect: &Effect) -> mychess_app::OpenControl {
    match effect {
        Effect::AdvancedSearch { control, .. } => control.clone(),
        other => panic!("not a search: {other:?}"),
    }
}

fn start(rig: &mut Rig, kind: AdvancedKind) {
    set(rig, AdvancedSearchMessage::SelectTab(kind));
    set(rig, AdvancedSearchMessage::Start);
}

#[test]
fn the_effect_carries_the_position_the_options_and_the_slot() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetBoardKind(BoardKind::Pawns),
    );
    set(&mut rig, AdvancedSearchMessage::SetBoardIgnoreColors(true));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Position, SearchOp::And),
    );
    let effect = rig.start_effect(AdvancedKind::Position);
    let Effect::AdvancedSearch {
        kind,
        slot: target,
        query,
        filter,
        ..
    } = effect
    else {
        panic!("not a search");
    };
    assert_eq!(kind, AdvancedKind::Position);
    assert_eq!(target, slot);
    assert_eq!(filter.len(), 6);
    let AdvancedQuery::Position(q) = *query else {
        panic!("not a position search");
    };
    assert_eq!(q.position.fen(), AFTER_E4_E5);
    assert_eq!(q.kind, BoardMatch::Pawns);
    assert!(q.ignore_colors && !q.in_variations);
    assert_eq!(q.op, FilterOp::And);
}

#[test]
fn the_position_of_the_search_is_the_one_on_the_board() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(BasesMessage::LoadGame {
        slot: rig.state.bases.current(),
        number: 4,
    });
    rig.send(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::GoToMainlinePly(2),
    ));
    let board = rig.state.session.position().fen();
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Position) else {
        panic!("not a search");
    };
    let AdvancedQuery::Position(q) = *query else {
        panic!("not a position search");
    };
    assert_eq!(q.position.fen(), board);
}

#[test]
fn a_position_search_filters_the_slot_and_loads_the_first_game_at_the_match() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    // The value is ply + 1 (D3206).
    assert_eq!(rig.filter_value(slot, 1), 3);
    assert!(!rig.state.advanced_search().is_running());
    let summary = rig
        .state
        .advanced_search_view()
        .and_then(|v| v.summary)
        .expect("summary");
    assert_eq!(
        (summary.matched, summary.start_count, summary.slot),
        (3, 6, slot)
    );
    assert_eq!(summary.kind, AdvancedKind::Position);
    // Decision 4: the first game of the filter, at the position that matched.
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(1));
    assert_eq!(rig.cursor_ply(), 2);
    assert_eq!(rig.state.session.position().fen(), AFTER_E4_E5);
}

#[test]
fn the_board_position_of_a_new_game_matches_every_game_at_ply_zero() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5, 6]);
    assert_eq!(rig.filter_value(slot, 3), 1);
    assert_eq!(rig.cursor_ply(), 0);
}

#[test]
fn a_material_search_records_the_ply_where_the_condition_began() {
    let (mut rig, slot, _dir) = sample();
    // Only game 4 loses a white pawn (3...cxd4, ply 6).
    set(
        &mut rig,
        AdvancedSearchMessage::SetRange {
            color: Color::White,
            piece: MaterialPiece::Pawn,
            min: 7,
            max: 7,
        },
    );
    start(&mut rig, AdvancedKind::Material);
    assert_eq!(rig.filter_numbers(slot), [4]);
    assert_eq!(rig.filter_value(slot, 4), 7);
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(4));
    assert_eq!(rig.cursor_ply(), 6);
}

#[test]
fn a_moves_search_loads_the_position_after_the_first_move() {
    let (mut rig, slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("e4 ? Nf3".into()),
    );
    start(&mut rig, AdvancedKind::Moves);
    assert_eq!(rig.filter_numbers(slot), [1, 4, 5]);
    // Value = ply of the first move + 2; the board shows the position after
    // that move.
    assert_eq!(rig.filter_value(slot, 1), 2);
    assert_eq!(rig.cursor_ply(), 1);
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("e4 e5 Nf3".into()),
    );
    start(&mut rig, AdvancedKind::Moves);
    assert_eq!(rig.filter_numbers(slot), [1, 5]);
}

#[test]
fn the_operation_of_each_tab_is_its_own() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    start(&mut rig, AdvancedKind::Moves);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 4, 5]);
    // AND on the position tab narrows; the moves tab is still Reset.
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Position, SearchOp::And),
    );
    start(&mut rig, AdvancedKind::Position);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.position.op, SearchOp::And);
    assert_eq!(view.moves.op, SearchOp::Reset);
    assert_eq!(view.material.op, SearchOp::Reset);
    // OR adds what another search finds.
    set(&mut rig, AdvancedSearchMessage::SetMovesText("d4".into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Moves, SearchOp::Or),
    );
    start(&mut rig, AdvancedKind::Moves);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5]);
}

#[test]
fn the_progress_is_read_by_the_tick_and_the_answer_ends_the_run() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let Effect::AdvancedSearch { ref control, .. } = effect else {
        panic!("not a search");
    };
    let control = control.clone();
    assert!(rig.state.advanced_search().is_running());
    let view = rig.state.advanced_search_view().unwrap();
    let running = view.running.expect("running");
    assert_eq!(
        (running.kind, running.slot, running.progress),
        (AdvancedKind::Position, slot, (0, 0))
    );
    let answer = rig.perform(effect);
    // The engine reported the games it examined.
    let (done, total) = control.progress();
    assert!(total > 0 && done <= total, "{done}/{total}");
    rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Progress));
    assert_eq!(rig.state.advanced_search().progress(), Some((done, total)));
    rig.raw(answer);
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    assert!(rig.state.advanced_search_view().unwrap().running.is_none());
}

#[test]
fn stopping_keeps_the_filter_and_says_so() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Stop));
    let answer = rig.perform(effect);
    assert!(
        matches!(
            answer,
            Message::AdvancedSearch(AdvancedSearchMessage::Cancelled { .. })
        ),
        "{answer:?}"
    );
    rig.raw(answer);
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5, 6]);
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::Interrupted));
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, None);
    assert!(rig.state.advanced_search_view().unwrap().summary.is_none());
}

#[test]
fn an_old_answer_is_dropped() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let first = rig.start_effect(AdvancedKind::Position);
    // The answer of the first search, in hand before the second begins.
    let stale = rig.perform(first);
    let second = rig.start_effect(AdvancedKind::Position);
    rig.raw(stale);
    // Still running the second one, and nothing changed.
    assert!(rig.state.advanced_search().is_running());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5, 6]);
    let fresh = rig.perform(second);
    rig.raw(fresh);
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
}

#[test]
fn a_second_search_stops_the_first() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let first = rig.start_effect(AdvancedKind::Position);
    let Effect::AdvancedSearch { control, .. } = &first else {
        panic!("not a search");
    };
    let control = control.clone();
    let _second = rig.start_effect(AdvancedKind::Position);
    assert!(control.is_cancelled());
    // The answer of the stopped one changes nothing.
    let answer = rig.perform(first);
    rig.raw(answer);
    assert!(rig.state.advanced_search().is_running());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 3, 4, 5, 6]);
}

#[test]
fn closing_the_slot_during_the_search_drops_the_result() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let b = rig.open_games(dir.path(), "b.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(a)),
    );
    let effect = rig.start_effect(AdvancedKind::Position);
    let answer = rig.perform(effect);
    rig.send(BasesMessage::Close(a));
    rig.raw(answer);
    assert!(rig.state.bases.slot(a).is_none());
    assert_eq!(rig.state.bases.current(), b);
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::Stale));
}

#[test]
fn another_database_gets_the_filter_and_becomes_current_when_it_matches() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let b = rig.open_games(dir.path(), "b.pgn", &["1. d4 d5 *", "1. c4 *"]);
    assert_eq!(rig.state.bases.current(), b);
    rig.open_window(AdvancedKind::Position);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(a)),
    );
    let Effect::AdvancedSearch { slot, filter, .. } = rig.start_effect(AdvancedKind::Position)
    else {
        panic!("not a search");
    };
    // The filter of the other database, not the current one.
    assert_eq!((slot, filter.len()), (a, 6));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(a), [1, 2, 5]);
    assert_eq!(rig.filter_numbers(b), [1, 2]);
    // Games found: the other database is current, with its first game.
    assert_eq!(rig.state.bases.current(), a);
    assert_eq!(rig.state.bases.slot(a).unwrap().current_game, Some(1));
    assert_eq!(rig.cursor_ply(), 2);
}

#[test]
fn another_database_without_a_match_does_not_switch() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &["1. d4 d5 *"]);
    let b = rig.open_games(dir.path(), "b.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(a)),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert!(rig.filter_numbers(a).is_empty());
    assert_eq!(rig.filter_numbers(b).len(), 6);
    assert_eq!(rig.state.bases.current(), b);
}

#[test]
fn the_filter_of_the_target_is_the_one_an_and_search_narrows() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let _b = rig.open_games(dir.path(), "b.pgn", &GAMES);
    rig.open_window(AdvancedKind::Moves);
    for kind in AdvancedKind::ALL {
        set(&mut rig, AdvancedSearchMessage::SetReference(kind, Some(a)));
    }
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    start(&mut rig, AdvancedKind::Moves);
    assert_eq!(rig.filter_numbers(a), [1, 2, 4, 5]);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetOp(AdvancedKind::Position, SearchOp::And),
    );
    let Effect::AdvancedSearch { filter, .. } = rig.start_effect(AdvancedKind::Position) else {
        panic!("not a search");
    };
    // The copy handed to the search is the filter of slot `a` as it is.
    assert_eq!(filter.iter_games().collect::<Vec<_>>(), [1, 2, 4, 5]);
}

#[test]
fn the_tree_stops_adjusting_the_filter_when_a_search_applies_one() {
    let (mut rig, slot, _dir) = sample();
    rig.send(Message::Tree(mychess_app::TreeMessage::SetAdjust(true)));
    assert!(rig.state.prefs.tree.auto_adjust);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert!(!rig.state.prefs.tree.auto_adjust);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
}

#[test]
fn the_list_goes_back_to_the_start() {
    let (mut rig, slot, _dir) = sample();
    rig.send(BasesMessage::SetListFirst { slot, first: 4 });
    assert_eq!(rig.state.bases.slot(slot).unwrap().list.first, 4);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.state.bases.slot(slot).unwrap().list.first, 1);
}

#[test]
fn another_list_job_is_refused_while_a_search_runs_and_vice_versa() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    // A list job (sorting) is refused with the busy notice.
    let effects = rig.raw(Message::Bases(BasesMessage::List(
        mychess_app::ListMessage::SortBy {
            slot: rig.state.bases.current(),
            column: mychess_app::ListColumn::White,
        },
    )));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::ListBusy))
    );
    let answer = rig.perform(effect);
    rig.raw(answer);
    assert!(!rig.state.advanced_search().is_running());
}

#[test]
fn quitting_stops_the_search() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let Effect::AdvancedSearch { control, .. } = &effect else {
        panic!("not a search");
    };
    let control = control.clone();
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
}

#[test]
fn a_moves_search_with_a_check_marker_raises_the_test_and_tells() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText("Qh7+".into()));
    let effect = rig.start_effect(AdvancedKind::Moves);
    let Effect::AdvancedSearch { query, .. } = effect else {
        panic!("not a search");
    };
    let AdvancedQuery::Moves(q) = *query else {
        panic!("not a moves search");
    };
    assert_eq!(q.check_test, CheckTest::Check);
    assert_eq!(
        rig.issue(),
        Some(AdvancedSearchNotice::CheckTestRaised(CheckKind::Check))
    );
    let _ = SideKind::Both;
}

// ------------------------------------------- review fixes (D3389-D3393)

fn dirty_game(rig: &mut Rig, number: u32) {
    let slot = rig.state.bases.current();
    rig.send(BasesMessage::LoadGame { slot, number });
    rig.send(mychess_app::GameMessage::Navigate(
        mychess_app::Nav::GoToMainlinePly(1),
    ));
    rig.send(mychess_app::GameMessage::AddNag(
        mychess_core::Nag::new(1).unwrap(),
    ));
    assert!(rig.state.session.is_dirty());
}

#[test]
fn a_form_error_on_start_leaves_the_old_search_running() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let control = control_of(&effect);
    set(&mut rig, AdvancedSearchMessage::SetFen("garbage".into()));
    let effects = rig.raw(Message::AdvancedSearch(AdvancedSearchMessage::Start));
    assert!(effects.is_empty());
    assert!(!control.is_cancelled());
    assert!(rig.state.advanced_search().is_running());
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidFen));
    let answer = rig.perform(effect);
    rig.raw(answer);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
}

#[test]
fn a_valid_restart_still_replaces_the_search_in_flight() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let first = rig.start_effect(AdvancedKind::Position);
    let control = control_of(&first);
    let _second = rig.start_effect(AdvancedKind::Position);
    assert!(control.is_cancelled());
    assert!(rig.state.advanced_search().is_running());
}

#[test]
fn closing_the_window_stops_the_search_and_says_so() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let control = control_of(&effect);
    set(&mut rig, AdvancedSearchMessage::Close);
    assert!(control.is_cancelled());
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::AdvancedSearch(AdvancedSearchNotice::Interrupted))
    );
    let answer = rig.perform(effect);
    rig.raw(answer);
    assert_eq!(rig.filter_numbers(slot).len(), 6);
}

#[test]
fn a_dirty_editor_asks_before_the_first_game_of_the_filter_replaces_it() {
    let (mut rig, slot, _dir) = sample();
    dirty_game(&mut rig, 3);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    start(&mut rig, AdvancedKind::Position);
    // The filter is applied, the editor keeps the altered game and asks.
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    assert!(rig.state.discard_prompt.is_some());
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(3));
    assert!(rig.state.session.is_dirty());
}

#[test]
fn a_search_in_another_slot_parks_the_altered_game_instead_of_losing_it() {
    let (mut rig, a, dir) = sample();
    let b = rig.open_games(dir.path(), "t.pgn", &["1. c4 c5 *", "1. g3 g6 *"]);
    assert_eq!(rig.state.bases.current(), b);
    dirty_game(&mut rig, 2);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(a)),
    );
    start(&mut rig, AdvancedKind::Position);
    assert_eq!(rig.filter_numbers(a), [1, 2, 5]);
    assert_eq!(rig.state.bases.current(), a);
    assert!(rig.state.discard_prompt.is_none());
    assert!(rig.state.bases.slot(b).unwrap().parked_dirty());
    // Going back brings the altered game with it.
    rig.send(BasesMessage::Select(b));
    assert!(rig.state.session.is_dirty());
}

#[test]
fn a_game_that_cannot_replace_the_editor_is_told_when_a_question_is_open() {
    let (mut rig, slot, _dir) = sample();
    dirty_game(&mut rig, 3);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    start(&mut rig, AdvancedKind::Position);
    assert!(rig.state.discard_prompt.is_some());
    rig.state.notice = None;
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("e4 c5".into()),
    );
    start(&mut rig, AdvancedKind::Moves);
    assert_eq!(rig.filter_numbers(slot), [4]);
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::GameNotLoaded))
    );
}
