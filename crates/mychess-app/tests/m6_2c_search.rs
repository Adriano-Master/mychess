//! Contract tests of the search flow of the search for tactics and patterns,
//! written from the spec `m6-2-taticas-e-padroes.md` (D3273, D3281-D3286,
//! D3283, D3284, D3285 and the rules 29 and 30): window and action, the
//! effect and its query, the question about the cost, progress, stop, stale
//! answers, other databases, the first game with its occurrences and their
//! navigation, "test on this game", the online lock and quit.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_2c_common;

use std::time::{Duration, Instant};

use m6_2c_common::*;
use mychess_app::online::{OnlineNotice, OnlinePhase, PlayingState};
use mychess_app::pattern_search::model::{Condition, MateMode, Status};
use mychess_app::{
    Action, ConditionKind, GameMessage, ListColumn, ListMessage, Nav, ParamValue, PatternEdit,
    PatternFailure, PatternIssue, RequestId,
};
use mychess_db::pattern::presets;

fn pat(message: Pat) -> Message {
    Message::PatternSearch(message)
}

fn request_of(effect: &Effect) -> RequestId {
    match effect {
        Effect::PatternSearch { request, .. } => *request,
        other => panic!("not a pattern search: {other:?}"),
    }
}

fn control_of(effect: &Effect) -> mychess_app::OpenControl {
    match effect {
        Effect::PatternSearch { control, .. } => control.clone(),
        other => panic!("not a pattern search: {other:?}"),
    }
}

/// The "side to move is in check" pattern in the Custom tab: it holds in
/// several positions of several games.
fn pick_check(rig: &mut Rig) {
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Status,
    }));
    assert_eq!(
        rig.view().draft.root,
        Condition::All {
            items: vec![Condition::Status {
                status: Status::Check
            }]
        }
    );
}

fn many_games(count: usize) -> String {
    "[Event \"x\"]\n[Result \"*\"]\n\n1. e4 *\n\n".repeat(count)
}

// ---------------------------------------------------------------- window

#[test]
fn the_view_is_none_until_the_window_opens_and_after_it_closes() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    assert!(rig.state.pattern_search_view().is_none());
    rig.send(Message::Action(Action::SearchPattern));
    assert!(rig.state.pattern_search_view().is_some());
    assert!(rig.state.pattern_search().is_open());
    rig.pat(Pat::Close);
    assert!(rig.state.pattern_search_view().is_none());
    // Closing twice is harmless.
    rig.pat(Pat::Close);
    assert!(rig.state.pattern_search_view().is_none());
}

#[test]
fn closing_keeps_what_was_edited_and_stops_the_search() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    pick_check(&mut rig);
    let effect = rig.start_effect();
    let control = control_of(&effect);
    rig.pat(Pat::Close);
    // Rule 30: closing the window cancels the search.
    assert!(control.is_cancelled());
    rig.pat(Pat::Open);
    assert_eq!(rig.view().tree.len(), 2);
}

// ------------------------------------------------------------- the start

#[test]
fn update_only_plans_the_search_and_perform_does_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effects = rig.raw(pat(Pat::Start));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(!effects[0].runs_on_host());
    // Nothing was searched inside `update`.
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, None);
    assert!(rig.state.pattern_search().is_running());
    assert!(rig.view().summary.is_none());
    let Effect::PatternSearch {
        pattern,
        filter,
        op,
        slot: asked,
        ..
    } = &effects[0]
    else {
        panic!("not a search");
    };
    // The query is the preset built with the parameters (D3279).
    let want = presets::build(
        "mate_in",
        &Params {
            by: Who::White,
            n: 1,
            ..Params::default()
        },
    )
    .unwrap();
    assert_eq!(**pattern, want);
    assert_eq!(*op, mychess_db::FilterOp::Reset);
    assert_eq!(*asked, slot);
    // The effect carries a copy.
    assert_eq!(**filter, before);
}

#[test]
fn a_mate_search_fills_the_filter_loads_the_first_game_and_computes_its_hits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    // Scholar's mate (ply 6 -> value 7) and Legal's mate (ply 12 -> 13).
    assert_eq!(rig.filter_values(slot), [(1, 7), (5, 13)]);
    let view = rig.view();
    let summary = view.summary.expect("summary");
    assert_eq!((summary.matched, summary.start_count), (2, 8));
    assert!(summary.positions_examined > 0);
    assert!(view.running.is_none());
    // The first game is in the editor at the ply of the match.
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(1));
    assert_eq!(rig.cursor_ply(), 6);
    // Its occurrences were computed by an effect, once the game arrived.
    let hits = view.hits.expect("hits");
    assert_eq!(hits.plies, [6]);
    assert_eq!(hits.current, 0);
    assert!(!hits.from_test);
    assert_eq!(hits.game, Some(1));
}

#[test]
fn the_tree_stops_following_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.send(Message::Tree(mychess_app::TreeMessage::SetAdjust(true)));
    assert!(rig.state.prefs.tree.auto_adjust);
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    assert!(!rig.state.prefs.tree.auto_adjust);
}

#[test]
fn the_operation_and_the_reference_database_decide_where_it_searches() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, a) = Rig::tactics(dir.path());
    let b = rig.open_text(dir.path(), "b.pgn", "[Event \"b\"]\n\n1. d4 *\n\n");
    assert_eq!(rig.state.bases.current(), b);
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::SetReference(Some(a)));
    rig.pat(Pat::Start);
    // Slot `a` was searched and shown; `b` was left alone.
    assert_eq!(rig.filter_values(a), [(1, 7), (5, 13)]);
    assert_eq!(rig.filter_values(b), [(1, 1)]);
    assert_eq!(rig.state.bases.current(), a);
    assert_eq!(rig.view().summary.unwrap().slot, a);
    // AND restricts what is in the filter; OR adds to it.
    rig.pat(Pat::SetOp(mychess_app::SearchOp::And));
    rig.pick_mate(Who::Black, 1);
    rig.pat(Pat::Start);
    assert!(rig.filter_values(a).is_empty());
    assert_eq!(
        rig.state.prefs.pattern_search.op,
        mychess_app::SearchOp::And
    );
    rig.pat(Pat::SetOp(mychess_app::SearchOp::Or));
    rig.pat(Pat::Start);
    assert_eq!(rig.filter_values(a), [(2, 4)]);
    // A slot that is not open is ignored.
    rig.pat(Pat::SetReference(Some(SlotId::CLIPBASE)));
    assert_eq!(rig.view().reference, Some(SlotId::CLIPBASE));
}

#[test]
fn the_scope_of_the_preset_and_the_user_reach_the_query() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    // "Mate in N" brings min_move 0 (D3279); the user then changes it.
    assert_eq!(rig.view().scope.min_move, 0);
    let mut scope = rig.view().scope;
    scope.hold = 2;
    scope.ignore_colors = true;
    rig.pat(Pat::SetScope(scope.clone()));
    let Effect::PatternSearch { pattern, .. } = rig.start_effect() else {
        panic!("search");
    };
    assert_eq!(pattern.scope, scope);
}

#[test]
fn ignoring_colors_finds_the_mirrored_mates() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::Black, 1);
    let mut scope = rig.view().scope;
    scope.ignore_colors = true;
    rig.pat(Pat::SetScope(scope));
    rig.pat(Pat::Start);
    // Black's mate in game 2 plus White's mates seen with colours swapped.
    assert_eq!(
        rig.filter_values(slot)
            .iter()
            .map(|g| g.0)
            .collect::<Vec<_>>(),
        [1, 2, 5]
    );
}

// ---------------------------------------------------------- invalid forms

#[test]
fn an_empty_group_is_refused_and_pointed_at() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effects = rig.raw(pat(Pat::Start));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(!rig.state.pattern_search().is_running());
    let notice = rig.view().issue.expect("notice");
    assert_eq!(
        notice,
        PatternSearchNotice::InvalidPattern(PatternIssue::EmptyGroup {
            path: "root.items".into()
        })
    );
    assert_eq!(notice.message_key(), "pattern-error-empty-group");
    assert!(notice.is_form_error());
    assert_eq!(rig.view().issue_path, Some(vec![]));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
}

#[test]
fn a_wrong_value_inside_a_card_points_at_the_card() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Piece,
    }));
    let mut piece = rig.view().tree[1].condition.clone();
    if let Condition::Piece(p) = &mut piece {
        p.min = 3;
        p.max = Some(1);
    }
    rig.pat(Pat::Edit(PatternEdit::Replace {
        path: vec![0],
        condition: Box::new(piece),
    }));
    assert_eq!(rig.view().issue_path, Some(vec![0]));
    assert!(rig.view().tree[1].has_issue);
    assert!(rig.raw(pat(Pat::Start)).is_empty());
}

// ---------------------------------------------------------- cost question

#[test]
fn an_expensive_search_asks_before_running_and_the_answer_decides() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    let slot = rig.open_text(dir.path(), "big.pgn", &many_games(34_000));
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 34_000);
    rig.pat(Pat::Open);
    // Mate "available" in every position of 34 000 games: 2 040 000
    // estimated positions, over the 2 000 000 limit (D3285).
    rig.pick_mate(Who::Any, 2);
    let effects = rig.raw(pat(Pat::Start));
    assert!(
        effects.is_empty(),
        "no search before the answer: {effects:?}"
    );
    assert!(!rig.state.pattern_search().is_running());
    let cost = rig.view().confirm.expect("question");
    assert_eq!(cost.games, 34_000);
    assert_eq!(cost.estimated_positions, 34_000 * 60);
    // "No": nothing runs, the question goes away.
    assert!(rig.raw(pat(Pat::AnswerCost(false))).is_empty());
    assert!(rig.view().confirm.is_none());
    // An answer with no question is ignored.
    assert!(rig.raw(pat(Pat::AnswerCost(true))).is_empty());
    assert!(!rig.state.pattern_search().is_running());
    // "Yes": the search is planned.
    rig.raw(pat(Pat::Start));
    let effects = rig.raw(pat(Pat::AnswerCost(true)));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(matches!(effects[0], Effect::PatternSearch { .. }));
    assert!(rig.view().confirm.is_none());
    assert!(rig.state.pattern_search().is_running());
    control_of(&effects[0]).cancel();
}

#[test]
fn a_cheap_search_never_asks() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.open_text(dir.path(), "big.pgn", &many_games(34_000));
    rig.pat(Pat::Open);
    // The same size, but the mate is only looked for where it was played.
    rig.pick_mate(Who::Any, 2);
    rig.pat(Pat::SetParam(ParamValue::MateMode(MateMode::Played)));
    let effects = rig.raw(pat(Pat::Start));
    assert_eq!(effects.len(), 1);
    assert!(rig.view().confirm.is_none());
    control_of(&effects[0]).cancel();
}

#[test]
fn stopping_a_costly_search_answers_quickly_and_keeps_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    let slot = rig.open_text(dir.path(), "big.pgn", &many_games(34_000));
    rig.pat(Pat::Open);
    rig.pick_mate(Who::Any, 2);
    rig.raw(pat(Pat::Start));
    let effects = rig.raw(pat(Pat::AnswerCost(true)));
    let effect = effects.into_iter().next().expect("effect");
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    rig.raw(pat(Pat::Stop));
    let started = Instant::now();
    let answer = rig.perform(effect);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "{:?}",
        started.elapsed()
    );
    assert!(matches!(
        answer,
        Message::PatternSearch(Pat::Cancelled { .. })
    ));
    rig.raw(answer);
    assert!(!rig.state.pattern_search().is_running());
    assert_eq!(rig.view().issue, Some(PatternSearchNotice::Interrupted));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    assert!(rig.view().summary.is_none());
}

// ------------------------------------------------------ progress and stop

#[test]
fn the_tick_copies_the_progress_into_the_view_and_ends_with_the_search() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let effect = rig.start_effect();
    let control = control_of(&effect);
    assert_eq!(rig.view().running.unwrap().progress, (0, 0));
    let answer = rig.perform(effect);
    let (done, total) = control.progress();
    assert!(total > 0 && done <= total);
    // The view only changes at the tick.
    assert_eq!(rig.view().running.unwrap().progress, (0, 0));
    assert!(rig.raw(pat(Pat::Progress)).is_empty());
    assert_eq!(rig.view().running.unwrap().progress, (done, total));
    rig.raw(answer);
    assert!(rig.view().running.is_none());
    // A late tick does nothing.
    assert!(rig.raw(pat(Pat::Progress)).is_empty());
    assert!(rig.state.pattern_search().progress().is_none());
}

#[test]
fn stopping_leaves_the_filter_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effect = rig.start_effect();
    let control = control_of(&effect);
    rig.raw(pat(Pat::Stop));
    assert!(control.is_cancelled());
    let answer = rig.perform(effect);
    assert!(matches!(
        answer,
        Message::PatternSearch(Pat::Cancelled { .. })
    ));
    rig.raw(answer);
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    assert_eq!(rig.view().issue, Some(PatternSearchNotice::Interrupted));
    assert_eq!(
        rig.view().issue.unwrap().message_key(),
        "pattern-interrupted"
    );
    assert!(rig.state.bases.slot(slot).unwrap().current_game.is_none());
}

// ---------------------------------------------------------- stale answers

#[test]
fn starting_again_stops_the_old_search_and_drops_its_answer() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let old = rig.start_effect();
    let old_control = control_of(&old);
    let new = rig.start_effect();
    assert!(old_control.is_cancelled());
    assert_ne!(request_of(&old), request_of(&new));
    let late = rig.perform(old);
    rig.raw(late);
    // The old answer changed nothing and the new search is still waited for.
    assert!(rig.state.pattern_search().is_running());
    assert!(rig.view().issue.is_none());
    let answer = rig.perform(new);
    rig.raw(answer);
    assert_eq!(rig.filter_values(slot).len(), 2);
}

#[test]
fn a_replayed_answer_is_applied_once() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let effect = rig.start_effect();
    let answer = rig.perform(effect);
    rig.raw(answer.clone());
    assert_eq!(rig.filter_values(slot).len(), 2);
    // Another search narrows the filter; the old answer must not undo it.
    rig.pick_mate(Who::Black, 1);
    rig.pat(Pat::SetOp(mychess_app::SearchOp::And));
    rig.pat(Pat::Start);
    assert!(rig.filter_values(slot).is_empty());
    rig.raw(answer);
    assert!(rig.filter_values(slot).is_empty());
}

#[test]
fn closing_the_database_during_the_search_drops_the_result() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, a) = Rig::tactics(dir.path());
    let b = rig.open_text(dir.path(), "b.pgn", "[Event \"b\"]\n\n1. d4 *\n\n");
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::SetReference(Some(a)));
    let effect = rig.start_effect();
    rig.send(BasesMessage::Close(a));
    let answer = rig.perform(effect);
    rig.send(answer);
    assert_eq!(rig.view().issue, Some(PatternSearchNotice::Stale));
    assert!(rig.view().summary.is_none());
    assert_eq!(rig.filter_values(b), [(1, 1)]);
}

#[test]
fn a_failed_search_tells_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let cases = [
        (PatternFailure::Internal, PatternSearchNotice::Failed),
        (PatternFailure::FilterSize, PatternSearchNotice::Stale),
        (
            PatternFailure::Pattern(PatternIssue::TooDeep { max: 6 }),
            PatternSearchNotice::InvalidPattern(PatternIssue::TooDeep { max: 6 }),
        ),
    ];
    for (failure, notice) in cases {
        let request = request_of(&rig.start_effect());
        rig.raw(pat(Pat::Finished {
            request,
            result: Err(failure),
        }));
        assert!(!rig.state.pattern_search().is_running());
        assert_eq!(rig.view().issue, Some(notice));
        assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
        assert!(rig.view().summary.is_none());
    }
    assert_eq!(
        PatternSearchNotice::InvalidPattern(PatternIssue::TooDeep { max: 6 }).message_key(),
        "pattern-error-too-deep"
    );
}

// ----------------------------------------------- hits and their navigation

#[test]
fn the_occurrences_of_each_game_follow_the_game_in_the_editor() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    pick_check(&mut rig);
    rig.pat(Pat::Start);
    assert_eq!(
        rig.filter_values(slot)
            .iter()
            .map(|g| g.0)
            .collect::<Vec<_>>(),
        [1, 2, 3, 5, 7]
    );
    // Game 1: Qxf7# is the first check, after seven half moves.
    assert_eq!(rig.view().hits.unwrap().plies, [7]);
    // Another game of the filter arrives: its own occurrences are computed.
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    let hits = rig.view().hits.expect("hits of game 5");
    assert_eq!(hits.plies, [11, 13]);
    assert_eq!(hits.game, Some(5));
    // A game of the base that is not in the filter has none: no occurrence.
    rig.send(BasesMessage::LoadGame { slot, number: 6 });
    assert!(rig.view().hits.expect("hits").plies.is_empty());
    // The hits are an effect, not work of `update`.
    let effects = rig.raw(Message::Bases(BasesMessage::LoadGame { slot, number: 5 }));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadBaseGame { .. }))
    );
}

#[test]
fn next_and_previous_visit_the_occurrences_and_wrap() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    pick_check(&mut rig);
    rig.pat(Pat::Start);
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    assert_eq!(rig.cursor_ply(), 0);
    rig.pat(Pat::NextHit);
    assert_eq!(rig.cursor_ply(), 11);
    assert_eq!(rig.view().hits.unwrap().current, 0);
    rig.pat(Pat::NextHit);
    assert_eq!(rig.cursor_ply(), 13);
    assert_eq!(rig.view().hits.unwrap().current, 1);
    // After the last: the first.
    rig.pat(Pat::NextHit);
    assert_eq!(rig.cursor_ply(), 11);
    // Before the first: the last.
    rig.pat(Pat::PrevHit);
    assert_eq!(rig.cursor_ply(), 13);
    rig.pat(Pat::PrevHit);
    assert_eq!(rig.cursor_ply(), 11);
    rig.pat(Pat::GoToHit(1));
    assert_eq!(rig.cursor_ply(), 13);
    // An index that does not exist changes nothing.
    rig.pat(Pat::GoToHit(9));
    assert_eq!(rig.cursor_ply(), 13);
    // From a ply between two occurrences, "next" is the one after it.
    rig.send(Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(
        12,
    ))));
    rig.pat(Pat::NextHit);
    assert_eq!(rig.cursor_ply(), 13);
    rig.send(Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(
        12,
    ))));
    rig.pat(Pat::PrevHit);
    assert_eq!(rig.cursor_ply(), 11);
}

#[test]
fn the_marks_only_show_on_the_position_of_the_occurrence() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pat(Pat::SelectPreset("fork".into()));
    rig.pat(Pat::SetParam(ParamValue::By(Who::White)));
    rig.pat(Pat::Start);
    // The knight fork of game 3 is on the board after one half move.
    assert_eq!(rig.cursor_ply(), 1);
    let view = rig.view();
    assert_eq!(view.hits.as_ref().unwrap().plies, [1]);
    assert!(!view.highlights.is_empty());
    // Away from the position the marks go (they are not part of the game).
    rig.send(Message::Game(GameMessage::Navigate(Nav::Back(1))));
    assert!(rig.view().highlights.is_empty());
    assert!(rig.view().hits.is_some());
    rig.pat(Pat::NextHit);
    assert_eq!(rig.cursor_ply(), 1);
    assert!(!rig.view().highlights.is_empty());
    assert!(
        rig.state
            .session
            .game()
            .comment(rig.state.session.cursor())
            .unwrap()
            .is_none()
    );
}

#[test]
fn clearing_the_hits_stops_following_the_games() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    pick_check(&mut rig);
    rig.pat(Pat::Start);
    assert!(rig.view().hits.is_some());
    rig.pat(Pat::ClearHits);
    assert!(rig.view().hits.is_none());
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    assert!(rig.view().hits.is_none());
    assert!(rig.view().highlights.is_empty());
}

#[test]
fn a_new_game_without_a_base_has_no_hits() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    pick_check(&mut rig);
    rig.pat(Pat::Start);
    assert!(rig.view().hits.is_some());
    rig.send(Message::Command(mychess_app::Command::NewGame));
    assert!(rig.view().hits.is_none());
}

#[test]
fn a_late_hits_answer_for_another_game_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    pick_check(&mut rig);
    rig.pat(Pat::Start);
    // Ask for game 5, but do not run the effect yet.
    let effects = rig.raw(Message::Bases(BasesMessage::LoadGame { slot, number: 5 }));
    let load = effects
        .into_iter()
        .find(|e| matches!(e, Effect::LoadBaseGame { .. }))
        .expect("load");
    let loaded = rig.perform(load);
    let effects = rig.raw(loaded);
    let hits: Vec<_> = effects
        .into_iter()
        .filter(|e| matches!(e, Effect::PatternHits { .. }))
        .collect();
    assert_eq!(hits.len(), 1);
    // Meanwhile another game is loaded.
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    let answer = rig.perform(hits.into_iter().next().unwrap());
    rig.raw(answer);
    assert_eq!(rig.view().hits.expect("hits").game, Some(2));
    assert_eq!(rig.view().hits.unwrap().plies, [4]);
}

// ------------------------------------------------------ test on this game

#[test]
fn testing_the_game_does_not_touch_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    pick_check(&mut rig);
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effects = rig.raw(pat(Pat::TestGame));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(matches!(effects[0], Effect::PatternHits { .. }));
    assert!(!rig.state.pattern_search().is_running());
    let answer = rig.perform(effects.into_iter().next().unwrap());
    rig.raw(answer);
    let hits = rig.view().hits.expect("test result");
    assert_eq!(hits.plies, [11, 13]);
    assert!(hits.from_test);
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    assert!(rig.view().summary.is_none());
    // The test result is for this game only.
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    assert!(rig.view().hits.is_none());
}

#[test]
fn testing_with_a_wrong_pattern_tells_and_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    assert!(rig.raw(pat(Pat::TestGame)).is_empty());
    assert!(matches!(
        rig.view().issue,
        Some(PatternSearchNotice::InvalidPattern(_))
    ));
}

// -------------------------------------------------- list lock, quit, online

#[test]
fn a_running_search_refuses_other_list_jobs_and_a_list_job_refuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let effect = rig.start_effect();
    let sort = Message::Bases(BasesMessage::List(ListMessage::SortBy {
        slot,
        column: ListColumn::White,
    }));
    let effects = rig.raw(sort.clone());
    assert!(effects.is_empty(), "{effects:?}");
    rig.raw(pat(Pat::Stop));
    let answer = rig.perform(effect);
    rig.raw(answer);
    // Now the list job runs and the search is refused.
    let sorting = rig.raw(sort);
    assert!(!sorting.is_empty());
    let effects = rig.raw(pat(Pat::Start));
    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn quitting_stops_the_search_and_its_answer_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let effect = rig.start_effect();
    let control = control_of(&effect);
    rig.raw(Message::Quit);
    assert!(control.is_cancelled());
    let answer = rig.perform(effect);
    assert!(matches!(
        answer,
        Message::PatternSearch(Pat::Cancelled { .. })
    ));
    rig.raw(answer);
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter().count(), 8);
}

#[test]
fn during_a_live_online_game_searches_run_but_the_game_in_the_editor_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    let a = rig.open_text(dir.path(), "a.pgn", TACTICS_PGN);
    let b = rig.open_text(dir.path(), "b.pgn", "[Event \"b\"]\n\n1. d4 *\n\n");
    rig.pat(Pat::Open);
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    assert!(rig.state.assist_locked());
    let fen_before = rig.state.session.position().fen();
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::SetReference(Some(a)));
    // Searching a local database is not help (as in M6-1, D3239)...
    let effects = rig.send(pat(Pat::Start));
    assert!(effects.is_empty());
    assert_eq!(rig.filter_values(a), [(1, 7), (5, 13)]);
    // ... but the editor is not replaced and no occurrence of the game
    // being played is computed.
    assert_eq!(rig.state.bases.slot(a).unwrap().current_game, None);
    assert_eq!(rig.state.session.position().fen(), fen_before);
    assert_eq!(rig.state.bases.current(), b);
    assert!(rig.view().hits.is_none());
    // "Test on this game" runs the solver on the live position: help.
    let effects = rig.raw(pat(Pat::TestGame));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled))
    );
    // The family of the lock, for the sweep: engines.
    assert_eq!(
        mychess_app::online::family_of(&pat(Pat::TestGame)),
        Some(mychess_app::online::Family::Engines)
    );
    assert_eq!(mychess_app::online::family_of(&pat(Pat::Start)), None);
    assert_eq!(
        mychess_app::online::family_of(&Message::Action(Action::SearchPattern)),
        None
    );
}

// ----------------------------------- review fixes (D3299 items 41 to 44)

#[test]
fn an_invalid_start_leaves_the_search_in_flight_running_and_applied() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let old = rig.start_effect();
    let control = control_of(&old);
    // The form turns invalid while the search runs: an empty group.
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    assert!(rig.raw(pat(Pat::Start)).is_empty());
    assert!(rig.state.pattern_search().is_running());
    assert!(!control.is_cancelled());
    assert!(rig.view().issue.is_some());
    // The answer of the old search is applied.
    let answer = rig.perform(old);
    rig.raw(answer);
    assert!(!rig.state.pattern_search().is_running());
    assert_eq!(rig.filter_values(slot).len(), 2);
}

#[test]
fn answering_no_to_the_cost_keeps_the_search_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.open_text(dir.path(), "big.pgn", &many_games(34_000));
    rig.pat(Pat::Open);
    pick_check(&mut rig);
    let old = rig.start_effect();
    let control = control_of(&old);
    // An expensive search is asked for while the cheap one runs.
    rig.pat(Pat::SelectTab(PatternTab::Presets));
    rig.pick_mate(Who::Any, 2);
    assert!(rig.raw(pat(Pat::Start)).is_empty());
    assert!(rig.view().confirm.is_some());
    assert!(rig.state.pattern_search().is_running());
    assert!(!control.is_cancelled());
    // "No": the old one still runs.
    assert!(rig.raw(pat(Pat::AnswerCost(false))).is_empty());
    assert!(rig.view().confirm.is_none());
    assert!(rig.state.pattern_search().is_running());
    assert!(!control.is_cancelled());
    // "Yes" replaces it.
    rig.raw(pat(Pat::Start));
    let effects = rig.raw(pat(Pat::AnswerCost(true)));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(control.is_cancelled());
    control_of(&effects[0]).cancel();
}

#[test]
fn a_late_answer_after_closing_changes_nothing_and_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effect = rig.start_effect();
    // The search ended with a good result just before the window closed.
    let request = request_of(&effect);
    let answer = rig.perform(effect);
    rig.pat(Pat::Close);
    assert!(!rig.state.pattern_search().is_running());
    rig.raw(answer);
    rig.raw(pat(Pat::Cancelled { request }));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    assert!(rig.state.notice.is_none());
    rig.pat(Pat::Open);
    let view = rig.view();
    assert!(view.issue.is_none());
    assert!(view.summary.is_none());
    assert!(view.hits.is_none());
    assert!(rig.state.bases.slot(slot).unwrap().current_game.is_none());
}

#[test]
fn starting_a_new_pattern_forgets_the_old_remark() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::Start);
    assert!(rig.view().issue.is_some());
    rig.pat(Pat::Edit(PatternEdit::Clear));
    assert!(rig.view().issue.is_none());
}

#[test]
fn an_unknown_preset_without_a_name_uses_the_message_that_names_nothing() {
    assert_eq!(
        PatternSearchNotice::UnknownPreset(String::new()).message_key(),
        "pattern-notice-unknown-preset-blank"
    );
    assert_eq!(
        PatternSearchNotice::UnknownPreset("zzz".into()).message_key(),
        "pattern-notice-unknown-preset"
    );
}

#[test]
fn the_light_readers_agree_with_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    assert!(rig.state.pattern_search_open());
    assert!(rig.state.pattern_search_confirm().is_none());
    assert!(rig.state.pattern_search_highlights().is_empty());
    pick_check(&mut rig);
    rig.pat(Pat::Start);
    let view = rig.view();
    assert_eq!(rig.state.pattern_search_hits(), view.hits);
    assert_eq!(rig.state.pattern_search_highlights(), &view.highlights[..]);
    rig.pat(Pat::Close);
    assert!(!rig.state.pattern_search_open());
    assert!(rig.state.pattern_search_hits().is_none());
    assert!(rig.state.pattern_search_highlights().is_empty());
}
