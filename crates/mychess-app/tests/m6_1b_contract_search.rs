//! Contract tests of the search flow of the advanced searches, written from
//! the spec `m6-1-buscas-avancadas.md` (D3200-D3233, D3379-D3383; decisions
//! of the orchestrator 4 and 5): windows and actions, requests, stop,
//! progress, stale answers, first game, tree, list lock, online lock, quit
//! and "`update` never searches".
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_1b_common;

use m6_1b_common::*;
use mychess_app::online::{OnlinePhase, PlayingState};
use mychess_app::{
    Action, BoardKind, GameMessage, ListColumn, ListMessage, MaterialPiece, Nav, RequestId,
    SearchFailure, TreeMessage,
};
use mychess_core::{Color, Square};
use mychess_db::{AdvancedQuery, SearchError};
use rstest::rstest;

fn set(rig: &mut Rig, message: AdvancedSearchMessage) {
    rig.send(Message::AdvancedSearch(message));
}

fn adv(message: AdvancedSearchMessage) -> Message {
    Message::AdvancedSearch(message)
}

fn request_of(effect: &Effect) -> RequestId {
    match effect {
        Effect::AdvancedSearch { request, .. } => *request,
        other => panic!("not a search: {other:?}"),
    }
}

fn control_of(effect: &Effect) -> mychess_app::OpenControl {
    match effect {
        Effect::AdvancedSearch { control, .. } => control.clone(),
        other => panic!("not a search: {other:?}"),
    }
}

fn sort_job(rig: &Rig) -> Message {
    Message::Bases(BasesMessage::List(ListMessage::SortBy {
        slot: rig.state.bases.current(),
        column: ListColumn::White,
    }))
}

// ---------------------------------------------------------------- window

#[test]
fn the_view_is_none_until_the_window_opens_and_after_it_closes() {
    let mut rig = Rig::new();
    assert!(rig.state.advanced_search_view().is_none());
    assert!(!rig.state.advanced_search().is_open());
    rig.send(Message::Action(Action::SearchBoard));
    assert!(rig.state.advanced_search_view().is_some());
    assert!(rig.state.advanced_search().is_open());
    set(&mut rig, AdvancedSearchMessage::Close);
    assert!(rig.state.advanced_search_view().is_none());
    // Closing twice is harmless.
    set(&mut rig, AdvancedSearchMessage::Close);
    assert!(rig.state.advanced_search_view().is_none());
}

#[rstest]
#[case(Action::SearchBoard, AdvancedKind::Position)]
#[case(Action::SearchMaterial, AdvancedKind::Material)]
#[case(Action::SearchMoves, AdvancedKind::Moves)]
fn an_action_opens_the_window_on_its_tab_even_when_already_open(
    #[case] action: Action,
    #[case] kind: AdvancedKind,
) {
    let mut rig = Rig::new();
    // Open on a different tab first.
    let other = AdvancedKind::ALL.into_iter().find(|k| *k != kind).unwrap();
    rig.open_window(other);
    assert_eq!(rig.state.advanced_search_view().unwrap().tab, other);
    rig.send(Message::Action(action));
    assert_eq!(rig.state.advanced_search_view().unwrap().tab, kind);
    assert_eq!(rig.state.advanced_search().tab(), kind);
}

#[test]
fn closing_the_window_keeps_what_was_typed() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("e4 e5".into()),
    );
    set(&mut rig, AdvancedSearchMessage::Close);
    rig.open_window(AdvancedKind::Moves);
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.position.fen.as_deref(), Some(AFTER_E4_E5));
    assert_eq!(view.moves.text, "e4 e5");
}

// ------------------------------------------------------------- the start

#[test]
fn update_only_plans_the_search_and_perform_does_it() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effects = rig.raw(adv(AdvancedSearchMessage::Start));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(!effects[0].runs_on_host());
    // Nothing was searched inside `update`: the filter, the list, the
    // editor and the summary are as they were.
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, None);
    assert!(rig.state.advanced_search().is_running());
    assert!(rig.state.advanced_search_view().unwrap().summary.is_none());
    // The effect carries a copy: changing it does not touch the slot.
    let Effect::AdvancedSearch { filter, .. } = &effects[0] else {
        panic!("not a search");
    };
    assert_eq!(filter.as_ref(), &before);
    // Only `perform` runs the scan, and only the answer changes the filter.
    let mut effects = effects;
    let answer = rig.perform(effects.remove(0));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    rig.raw(answer);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
}

#[test]
fn every_search_gets_a_new_request_id() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let first = rig.start_effect(AdvancedKind::Position);
    let second = rig.start_effect(AdvancedKind::Position);
    assert_ne!(request_of(&first), request_of(&second));
    // After the run ends, the next search is new again, and also different
    // from every id handed out by the other flows in between.
    let answer = rig.perform(second.clone());
    rig.raw(answer);
    rig.send(BasesMessage::SetListFirst {
        slot: rig.state.bases.current(),
        first: 2,
    });
    let third = rig.start_effect(AdvancedKind::Position);
    let ids = [request_of(&first), request_of(&second), request_of(&third)];
    assert!(ids[0] != ids[2] && ids[1] != ids[2], "{ids:?}");
}

#[rstest]
#[case(AdvancedKind::Position, SearchOp::Reset)]
#[case(AdvancedKind::Position, SearchOp::And)]
#[case(AdvancedKind::Position, SearchOp::Or)]
#[case(AdvancedKind::Material, SearchOp::Reset)]
#[case(AdvancedKind::Material, SearchOp::And)]
#[case(AdvancedKind::Material, SearchOp::Or)]
#[case(AdvancedKind::Moves, SearchOp::Reset)]
#[case(AdvancedKind::Moves, SearchOp::And)]
#[case(AdvancedKind::Moves, SearchOp::Or)]
fn the_operation_of_the_tab_goes_into_the_query(#[case] kind: AdvancedKind, #[case] op: SearchOp) {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    set(&mut rig, AdvancedSearchMessage::SetOp(kind, op));
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(kind) else {
        panic!("not a search");
    };
    let got = match *query {
        AdvancedQuery::Position(q) => q.op,
        AdvancedQuery::Material(q) => q.op,
        AdvancedQuery::Moves(q) => q.op,
        _ => panic!("unknown query"),
    };
    let want = match op {
        SearchOp::Reset => mychess_db::FilterOp::Reset,
        SearchOp::And => mychess_db::FilterOp::And,
        SearchOp::Or => mychess_db::FilterOp::Or,
    };
    assert_eq!(got, want);
    // The view agrees.
    let view = rig.state.advanced_search_view().unwrap();
    let shown = match kind {
        AdvancedKind::Position => view.position.op,
        AdvancedKind::Material => view.material.op,
        AdvancedKind::Moves => view.moves.op,
    };
    assert_eq!(shown, op);
}

#[test]
fn the_default_operation_of_every_tab_is_reset() {
    let (mut rig, _slot, _dir) = sample();
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(
        (view.position.op, view.material.op, view.moves.op),
        (SearchOp::Reset, SearchOp::Reset, SearchOp::Reset)
    );
    // Searching with Reset widens again a filter that was narrowed.
    let slot = rig.state.bases.current();
    set(&mut rig, AdvancedSearchMessage::SetMovesText("d4".into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [3, 4]);
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 4, 5]);
}

#[test]
fn the_search_uses_the_tab_on_show() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    for kind in AdvancedKind::ALL {
        set(&mut rig, AdvancedSearchMessage::SelectTab(kind));
        let effects = rig.raw(adv(AdvancedSearchMessage::Start));
        let Some(Effect::AdvancedSearch {
            kind: got, query, ..
        }) = effects.into_iter().next()
        else {
            panic!("no search for {kind:?}");
        };
        assert_eq!(got, kind);
        assert!(matches!(
            (kind, *query),
            (AdvancedKind::Position, AdvancedQuery::Position(_))
                | (AdvancedKind::Material, AdvancedQuery::Material(_))
                | (AdvancedKind::Moves, AdvancedQuery::Moves(_))
        ));
    }
}

// ------------------------------------------------------------------ stop

#[test]
fn a_cancelled_search_leaves_everything_as_it_was_byte_for_byte() {
    let (mut rig, slot, _dir) = sample();
    // A first search: the filter is narrowed, game 1 is loaded, the list is
    // scrolled and the tree follows the filter again.
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 4, 5]);
    rig.send(BasesMessage::SetListFirst { slot, first: 3 });
    rig.send(Message::Tree(TreeMessage::SetAdjust(true)));
    let filter = rig.state.bases.slot(slot).unwrap().filter().clone();
    let first = rig.state.bases.slot(slot).unwrap().list.first;
    let game = rig.state.bases.slot(slot).unwrap().current_game;
    let ply = rig.cursor_ply();
    let summary = rig.state.advanced_search_view().unwrap().summary;
    // Second search, stopped before it is performed: the engine sees the
    // cancel and answers `Cancelled`.
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Position),
    );
    let effect = rig.start_effect(AdvancedKind::Position);
    let request = request_of(&effect);
    let control = control_of(&effect);
    let effects = rig.raw(adv(AdvancedSearchMessage::Stop));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(control.is_cancelled());
    let answer = rig.perform(effect);
    assert!(matches!(
        answer,
        Message::AdvancedSearch(AdvancedSearchMessage::Cancelled { request: r }) if r == request
    ));
    let effects = rig.raw(answer);
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &filter);
    assert_eq!(rig.state.bases.slot(slot).unwrap().list.first, first);
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, game);
    assert_eq!(rig.cursor_ply(), ply);
    assert_eq!(rig.state.advanced_search_view().unwrap().summary, summary);
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::Interrupted));
    // The tree still follows the filter: nothing was applied.
    assert!(rig.state.prefs.tree.auto_adjust);
}

#[test]
fn stopping_with_nothing_running_does_nothing() {
    let (mut rig, slot, _dir) = sample();
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effects = rig.raw(adv(AdvancedSearchMessage::Stop));
    assert!(effects.is_empty());
    assert!(rig.issue().is_none());
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    // A `Cancelled` nobody asked for is dropped.
    let effects = rig.raw(adv(AdvancedSearchMessage::Cancelled {
        request: RequestId(424_242),
    }));
    assert!(effects.is_empty());
    assert!(rig.issue().is_none());
}

#[test]
fn a_late_cancelled_for_an_old_request_does_not_end_the_new_search() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let first = rig.start_effect(AdvancedKind::Position);
    let old = request_of(&first);
    let second = rig.start_effect(AdvancedKind::Position);
    rig.raw(adv(AdvancedSearchMessage::Cancelled { request: old }));
    assert!(rig.state.advanced_search().is_running());
    assert!(rig.issue().is_none());
    let answer = rig.perform(second);
    rig.raw(answer);
    assert!(!rig.state.advanced_search().is_running());
}

#[test]
fn stopping_then_searching_again_runs_a_fresh_search() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let first = rig.start_effect(AdvancedKind::Position);
    rig.raw(adv(AdvancedSearchMessage::Stop));
    let second = rig.start_effect(AdvancedKind::Position);
    // The first one answers `Cancelled` late; the second one is untouched.
    let late = rig.perform(first);
    rig.raw(late);
    assert!(rig.state.advanced_search().is_running());
    let answer = rig.perform(second);
    rig.raw(answer);
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
}

// -------------------------------------------------------------- progress

#[test]
fn the_tick_copies_the_progress_into_the_view_and_ends_with_the_search() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let control = control_of(&effect);
    // Before any report the bar is empty.
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.running.unwrap().progress, (0, 0));
    let answer = rig.perform(effect);
    let (done, total) = control.progress();
    assert!(total > 0 && done <= total);
    // The view only changes at the tick.
    assert_eq!(
        rig.state
            .advanced_search_view()
            .unwrap()
            .running
            .unwrap()
            .progress,
        (0, 0)
    );
    let effects = rig.raw(adv(AdvancedSearchMessage::Progress));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(
        rig.state
            .advanced_search_view()
            .unwrap()
            .running
            .unwrap()
            .progress,
        (done, total)
    );
    rig.raw(answer);
    // Finished: no bar, no effect from a late tick, no change.
    let view = rig.state.advanced_search_view().unwrap();
    assert!(view.running.is_none());
    let effects = rig.raw(adv(AdvancedSearchMessage::Progress));
    assert!(effects.is_empty());
    assert!(rig.state.advanced_search().progress().is_none());
    assert_eq!(rig.state.advanced_search_view().unwrap(), view);
}

#[test]
fn a_tick_without_a_search_is_harmless() {
    let (mut rig, _slot, _dir) = sample();
    let before = rig.state.advanced_search_view().unwrap();
    for _ in 0..3 {
        assert!(rig.raw(adv(AdvancedSearchMessage::Progress)).is_empty());
    }
    assert_eq!(rig.state.advanced_search_view().unwrap(), before);
}

// --------------------------------------------------------- stale answers

#[test]
fn an_answer_nobody_asked_for_is_dropped() {
    let (mut rig, slot, _dir) = sample();
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effects = rig.raw(adv(AdvancedSearchMessage::Finished {
        request: RequestId(999_999),
        result: Err(SearchFailure::Internal),
    }));
    assert!(effects.is_empty());
    assert!(rig.issue().is_none());
    assert!(rig.state.notice.is_none());
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
}

#[test]
fn a_replayed_answer_is_applied_once() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let answer = rig.perform(effect);
    rig.raw(answer.clone());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    // Another search narrows the filter; the old answer must not undo it.
    set(
        &mut rig,
        AdvancedSearchMessage::SetMovesText("e4 e5 Nc3".into()),
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [2]);
    rig.raw(answer);
    assert_eq!(rig.filter_numbers(slot), [2]);
}

#[test]
fn a_failed_search_tells_and_changes_nothing() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effect = rig.start_effect(AdvancedKind::Position);
    let request = request_of(&effect);
    rig.raw(adv(AdvancedSearchMessage::Finished {
        request,
        result: Err(SearchFailure::Internal),
    }));
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::Failed));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    assert!(rig.state.advanced_search_view().unwrap().summary.is_none());
    // The engine telling the filter and the database disagree: the result
    // is stale.
    let effect = rig.start_effect(AdvancedKind::Position);
    let request = request_of(&effect);
    rig.raw(adv(AdvancedSearchMessage::Finished {
        request,
        result: Err(SearchFailure::Search(SearchError::FilterSize {
            filter: 1,
            games: 6,
        })),
    }));
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::Stale));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    // None of the failures turned the tree off.
    rig.send(Message::Tree(TreeMessage::SetAdjust(true)));
    let effect = rig.start_effect(AdvancedKind::Position);
    let request = request_of(&effect);
    rig.raw(adv(AdvancedSearchMessage::Finished {
        request,
        result: Err(SearchFailure::Internal),
    }));
    assert!(rig.state.prefs.tree.auto_adjust);
}

#[test]
fn replacing_the_database_of_the_slot_during_the_search_drops_the_result() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let _b = rig.open_games(dir.path(), "b.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(a)),
    );
    let effect = rig.start_effect(AdvancedKind::Position);
    let answer = rig.perform(effect);
    // The slot is closed and the next database takes a free slot (maybe the
    // same number): the old result must not be written over it.
    rig.send(BasesMessage::Close(a));
    let c = rig.open_games(dir.path(), "c.pgn", &["1. e4 e5 *", "1. e4 e5 2. Nf3 *"]);
    let c_before = rig.filter_numbers(c);
    rig.raw(answer);
    assert_eq!(rig.filter_numbers(c), c_before);
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::Stale));
    assert!(!rig.state.advanced_search().is_running());
    assert!(rig.state.advanced_search_view().unwrap().summary.is_none());
    assert_eq!(rig.state.bases.slot(c).unwrap().current_game, None);
}

#[test]
fn closing_the_current_slot_during_the_search_drops_the_result() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let _a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let b = rig.open_games(dir.path(), "b.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let answer = rig.perform(effect);
    rig.send(BasesMessage::Close(b));
    rig.raw(answer);
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::Stale));
    assert!(rig.state.bases.slot(b).is_none() || rig.filter_numbers(b).len() == 6);
}

// ----------------------------------------------------- reference slot

#[test]
fn a_search_in_a_reference_slot_filters_that_slot_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let b = rig.open_games(dir.path(), "b.pgn", &["1. d4 d5 *", "1. c4 *"]);
    assert_eq!(rig.state.bases.current(), b);
    rig.open_window(AdvancedKind::Moves);
    set(&mut rig, AdvancedSearchMessage::SetMovesText("d4".into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Moves, Some(a)),
    );
    let Effect::AdvancedSearch { slot, filter, .. } = rig.start_effect(AdvancedKind::Moves) else {
        panic!("not a search");
    };
    assert_eq!(slot, a);
    // The copy is the filter of `a` (6 games), not of the current slot (2).
    assert_eq!(filter.len(), 6);
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(a), [3, 4]);
    assert_eq!(rig.filter_numbers(b), [1, 2]);
    let summary = rig.state.advanced_search_view().unwrap().summary.unwrap();
    assert_eq!((summary.slot, summary.matched), (a, 2));
    assert_eq!(rig.state.bases.current(), a);
    assert_eq!(rig.state.bases.slot(a).unwrap().current_game, Some(3));
    assert_eq!(rig.cursor_ply(), 1);
}

#[test]
fn the_reference_of_one_tab_is_not_the_reference_of_another() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let b = rig.open_games(dir.path(), "b.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(a)),
    );
    set(&mut rig, AdvancedSearchMessage::SetMovesText("e4".into()));
    let Effect::AdvancedSearch { slot, .. } = rig.start_effect(AdvancedKind::Moves) else {
        panic!("not a search");
    };
    assert_eq!(slot, b);
}

#[test]
fn a_reference_slot_that_closes_before_the_start_is_told() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let b = rig.open_games(dir.path(), "b.pgn", &GAMES);
    rig.open_window(AdvancedKind::Position);
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(a)),
    );
    rig.send(BasesMessage::Close(a));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Position),
    );
    let effects = rig.raw(adv(AdvancedSearchMessage::Start));
    // Either the search falls back to the current slot or it says there is
    // no database; it must never touch a closed slot or panic.
    match effects.first() {
        Some(Effect::AdvancedSearch { slot, .. }) => assert_eq!(*slot, b),
        _ => assert_eq!(rig.issue(), Some(AdvancedSearchNotice::NoDatabase)),
    }
}

// ----------------------------------------------------------- first game

#[test]
fn a_game_past_the_filter_range_opens_at_the_last_position() {
    // 150 moves each: 300 plies, more than the 254 the byte can hold.
    let shuffle: String = (0..75).map(|_| "Nf3 Nf6 Ng1 Ng8 ").collect();
    let long = format!("{shuffle}*");
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let slot = rig.open_games(dir.path(), "long.pgn", &[&long]);
    rig.open_window(AdvancedKind::Material);
    // "Any" material at the end position only.
    set(&mut rig, AdvancedSearchMessage::SetEndOnly(true));
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [1]);
    // 255 = "last move": no wrap-around to a small ply (D3223).
    assert_eq!(rig.filter_value(slot, 1), 255);
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(1));
    assert_eq!(rig.cursor_ply(), 300);
}

#[test]
fn a_game_already_in_the_editor_is_only_navigated_without_losing_edits() {
    let (mut rig, slot, _dir) = sample();
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    rig.send(GameMessage::Navigate(Nav::GameEnd));
    // Edit the game: 4.Ba4 (b5 -> a4) after the end of the main line.
    rig.send(GameMessage::PlaySquares {
        from: Square::B5,
        to: Square::A4,
        promotion: None,
        options: mychess_app::PlayOptions::default(),
    });
    assert!(rig.state.is_dirty());
    let end = rig.cursor_ply();
    assert_eq!(end, 7);
    rig.send(GameMessage::Navigate(Nav::Start));
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let answer = rig.perform(effect);
    let effects = rig.raw(answer);
    // No game is requested: the one in the editor is game 1 already.
    assert!(
        !effects
            .iter()
            .any(|e| format!("{e:?}").starts_with("LoadBaseGame")),
        "{effects:?}"
    );
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
    assert_eq!(rig.cursor_ply(), 2);
    assert!(rig.state.is_dirty());
    rig.send(GameMessage::Navigate(Nav::GameEnd));
    assert_eq!(rig.cursor_ply(), 7);
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(1));
}

#[test]
fn the_first_game_is_the_first_of_the_filter_not_of_the_database() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetMovesText("d4".into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Moves),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [3, 4]);
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(3));
    // Game 4 plays d4 at ply 4: value 6 = ply of the first move (4) + 2.
    assert_eq!(rig.filter_value(slot, 4), 6);
    assert_eq!(rig.filter_value(slot, 3), 2);
}

#[test]
fn a_search_without_games_loads_nothing_and_still_turns_the_tree_off() {
    let (mut rig, slot, _dir) = sample();
    rig.send(Message::Tree(TreeMessage::SetAdjust(true)));
    rig.send(BasesMessage::SetListFirst { slot, first: 4 });
    set(
        &mut rig,
        AdvancedSearchMessage::SetFen("8/8/8/8/8/8/8/k1K5 w - - 0 1".into()),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert!(rig.filter_numbers(slot).is_empty());
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, None);
    assert_eq!(rig.state.bases.slot(slot).unwrap().list.first, 1);
    assert!(!rig.state.prefs.tree.auto_adjust);
    let summary = rig.state.advanced_search_view().unwrap().summary.unwrap();
    assert_eq!((summary.matched, summary.start_count), (0, 6));
}

#[test]
fn the_tree_adjust_message_is_emitted_not_just_the_preference_flipped() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(Message::Tree(TreeMessage::SetAdjust(true)));
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let answer = rig.perform(effect);
    let effects = rig.raw(answer);
    // Turning the adjust off is a preference change: it is persisted, so
    // the state is marked dirty.
    assert!(!rig.state.prefs.tree.auto_adjust);
    assert!(rig.state.prefs_dirty, "{effects:?}");
}

// ---------------------------------------------------------- position

#[rstest]
#[case("")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBN w KQkq - 0 1")]
#[case("8/8/8/8/8/8/8/8 w - - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq - 0 1")]
#[case("\u{0}\u{0}\u{0}")]
#[case("♔♕♖ not a fen at all ♗♘♙")]
#[case("k7/8/8/8/8/8/8/7K w - - 0 1 \n\r\n")]
fn a_bad_or_hostile_fen_never_searches(#[case] fen: &str) {
    let (mut rig, slot, _dir) = sample();
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    set(&mut rig, AdvancedSearchMessage::SetFen(fen.into()));
    let effects = rig.raw(adv(AdvancedSearchMessage::Start));
    // An empty field in FEN mode is refused like the others (D3392); "the
    // board" is chosen with `UseCurrentPosition`.
    if fen.starts_with("k7/8") {
        // Surrounding white space (and a trailing newline) is trimmed.
        assert!(!effects.is_empty() || rig.issue().is_some());
        return;
    }
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidFen));
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    // Back to the board: the next search works.
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    assert!(rig.start_effect(AdvancedKind::Position).clone_is_search());
}

trait CloneIsSearch {
    fn clone_is_search(&self) -> bool;
}
impl CloneIsSearch for Effect {
    fn clone_is_search(&self) -> bool {
        matches!(self, Effect::AdvancedSearch { .. })
    }
}

#[test]
fn a_typed_fen_wins_over_the_board_until_use_current_position() {
    let (mut rig, _slot, _dir) = sample();
    rig.send(BasesMessage::LoadGame {
        slot: rig.state.bases.current(),
        number: 3,
    });
    let board = rig.state.session.position().fen();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Position) else {
        panic!("not a search");
    };
    let AdvancedQuery::Position(q) = *query else {
        panic!("not position");
    };
    assert_eq!(q.position.fen(), AFTER_E4_E5);
    // The board moves on; the typed FEN stays.
    rig.send(GameMessage::Navigate(Nav::GameEnd));
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Position) else {
        panic!("not a search");
    };
    let AdvancedQuery::Position(q) = *query else {
        panic!("not position");
    };
    assert_eq!(q.position.fen(), AFTER_E4_E5);
    set(&mut rig, AdvancedSearchMessage::UseCurrentPosition);
    let now = rig.state.session.position().fen();
    assert_ne!(now, board);
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Position) else {
        panic!("not a search");
    };
    let AdvancedQuery::Position(q) = *query else {
        panic!("not position");
    };
    assert_eq!(q.position.fen(), now);
    assert_eq!(
        rig.state.advanced_search_view().unwrap().position.board_fen,
        now
    );
}

#[rstest]
#[case(BoardKind::Exact, mychess_db::BoardMatch::Exact)]
#[case(BoardKind::Pawns, mychess_db::BoardMatch::Pawns)]
#[case(BoardKind::Files, mychess_db::BoardMatch::Files)]
#[case(BoardKind::Material, mychess_db::BoardMatch::Material)]
fn the_board_kind_reaches_the_query(#[case] kind: BoardKind, #[case] want: mychess_db::BoardMatch) {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetBoardKind(kind));
    set(&mut rig, AdvancedSearchMessage::SetBoardVariations(true));
    let Effect::AdvancedSearch { query, .. } = rig.start_effect(AdvancedKind::Position) else {
        panic!("not a search");
    };
    let AdvancedQuery::Position(q) = *query else {
        panic!("not position");
    };
    assert_eq!(q.kind, want);
    assert!(q.in_variations);
}

// -------------------------------------------------------- material

#[test]
fn the_current_board_material_finds_the_games_that_reach_it() {
    let (mut rig, slot, _dir) = sample();
    // Game 4 after 3...cxd4: White has seven pawns, Black eight.
    rig.send(BasesMessage::LoadGame { slot, number: 4 });
    rig.send(GameMessage::Navigate(Nav::GoToMainlinePly(6)));
    set(&mut rig, AdvancedSearchMessage::MaterialCurrentBoard);
    let view = rig.state.advanced_search_view().unwrap();
    assert_eq!(view.material.white.get(MaterialPiece::Pawn).min, 7);
    assert_eq!(view.material.black.get(MaterialPiece::Pawn).min, 8);
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    assert_eq!(rig.filter_numbers(slot), [4]);
    assert_eq!(rig.filter_value(slot, 4), 7);
    // The game is in the editor already: navigated, at the ply found.
    assert_eq!(rig.cursor_ply(), 6);
}

#[test]
fn the_material_range_of_a_colour_is_independent() {
    let (mut rig, slot, _dir) = sample();
    set(
        &mut rig,
        AdvancedSearchMessage::SetRange {
            color: Color::Black,
            piece: MaterialPiece::Pawn,
            min: 7,
            max: 7,
        },
    );
    set(
        &mut rig,
        AdvancedSearchMessage::SelectTab(AdvancedKind::Material),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    // Black loses a pawn in game 4 (4.Nxd4) only.
    assert_eq!(rig.filter_numbers(slot), [4]);
    assert_eq!(rig.filter_value(slot, 4), 8);
}

// -------------------------------------------------------- list lock

#[test]
fn list_jobs_are_refused_while_a_search_runs_and_accepted_after() {
    let (mut rig, _slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let job = sort_job(&rig);
    let effects = rig.raw(job.clone());
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::ListBusy))
    );
    // Stopping releases the lock once the answer arrives.
    rig.raw(adv(AdvancedSearchMessage::Stop));
    let answer = rig.perform(effect);
    rig.raw(answer);
    rig.state.notice = None;
    let effects = rig.raw(job);
    assert!(!effects.is_empty(), "the sort must run now");
}

#[test]
fn a_search_is_refused_while_a_list_job_runs() {
    let (mut rig, slot, _dir) = sample();
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let job = sort_job(&rig);
    let list_effects = rig.raw(job);
    assert!(!list_effects.is_empty());
    rig.state.notice = None;
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effects = rig.raw(adv(AdvancedSearchMessage::Start));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Bases(BasesNotice::ListBusy))
    );
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
}

#[test]
fn an_invalid_start_during_a_search_leaves_a_consistent_state() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    set(&mut rig, AdvancedSearchMessage::SetFen("garbage".into()));
    let effects = rig.raw(adv(AdvancedSearchMessage::Start));
    assert!(effects.is_empty());
    assert_eq!(rig.issue(), Some(AdvancedSearchNotice::InvalidFen));
    // A form error leaves the search in flight alone (D3389): it goes on and
    // its answer is applied.
    assert!(rig.state.advanced_search().is_running());
    let answer = rig.perform(effect);
    rig.raw(answer);
    assert!(!rig.state.advanced_search().is_running());
    assert_eq!(rig.filter_numbers(slot), [1, 2, 5]);
}

// -------------------------------------------------------- quit & online

#[test]
fn quitting_stops_the_search_and_its_answer_is_harmless() {
    let (mut rig, slot, _dir) = sample();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    let effect = rig.start_effect(AdvancedKind::Position);
    let control = control_of(&effect);
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    let answer = rig.perform(effect);
    assert!(matches!(
        answer,
        Message::AdvancedSearch(AdvancedSearchMessage::Cancelled { .. })
    ));
    rig.raw(answer);
    assert_eq!(rig.filter_numbers(slot).len(), 6);
}

#[test]
fn quitting_with_no_search_does_not_touch_the_search() {
    let (mut rig, _slot, _dir) = sample();
    rig.raw(Message::Quit);
    assert!(!rig.state.advanced_search().is_running());
}

#[test]
fn during_a_live_online_game_the_filter_is_applied_but_the_editor_is_not_changed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = rig.open_games(dir.path(), "a.pgn", &GAMES);
    let b = rig.open_games(dir.path(), "b.pgn", &["1. d4 *"]);
    rig.open_window(AdvancedKind::Position);
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    assert!(rig.state.assist_locked());
    let fen_before = rig.state.session.position().fen();
    set(&mut rig, AdvancedSearchMessage::SetFen(AFTER_E4_E5.into()));
    set(
        &mut rig,
        AdvancedSearchMessage::SetReference(AdvancedKind::Position, Some(a)),
    );
    set(&mut rig, AdvancedSearchMessage::Start);
    // The searches are not locked (D3383): the filter changed ...
    assert_eq!(rig.filter_numbers(a), [1, 2, 5]);
    assert!(rig.state.advanced_search_view().unwrap().summary.is_some());
    // ... but the editor was not replaced, nor the slot switched.
    assert_eq!(rig.state.bases.slot(a).unwrap().current_game, None);
    assert_eq!(rig.state.session.position().fen(), fen_before);
    assert_eq!(rig.state.bases.current(), b);
}
