//! Extra contract tests of the search flow of the search for tactics and
//! patterns (spec `m6-2-taticas-e-padroes.md`: D3253, D3273, D3283-D3285,
//! D3296, D3383, the rules 29-32 of D3299 and the "Regras e casos de
//! borda"). They complement `m6_2c_search.rs`: the cost threshold, the
//! counters of the report (`UnknownPositions`, `Unreadable`), `NoDatabase`,
//! plies past 253, the reference database with its own filter, the cursor
//! rules of the occurrences, the lock of the online game and stale answers.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]
mod m6_2c_common;

use std::collections::BTreeSet;

use m6_2c_common::*;
use mychess_app::online::{OnlinePhase, PlayingState};
use mychess_app::pattern_search::model::{
    Condition, MateMode, PieceCond, Region, RoleName, Scope, Status,
};
use mychess_app::{
    Action, ConditionKind, GameMessage, Nav, ParamValue, PatternEdit, PatternFailure, PatternIssue,
    RegionSlot, RequestId, SearchOp,
};
use mychess_db::{Filter, FilterOp};

fn pat(message: Pat) -> Message {
    Message::PatternSearch(message)
}

fn request_of(effect: &Effect) -> RequestId {
    match effect {
        Effect::PatternSearch { request, .. } => *request,
        other => panic!("not a pattern search: {other:?}"),
    }
}

fn many_games(count: usize) -> String {
    "[Event \"x\"]\n[Result \"*\"]\n\n1. e4 *\n\n".repeat(count)
}

/// What `update` and its answers asked for, counted by kind.
#[derive(Default, Debug)]
struct Trace {
    searches: usize,
    hits: usize,
    saved: usize,
}

/// Like `Rig::send`, but counting the effects that were asked for.
fn trace(rig: &mut Rig, message: impl Into<Message>) -> Trace {
    let mut out = Trace::default();
    let mut queue = vec![message.into()];
    while let Some(message) = queue.pop() {
        for effect in rig.raw(message) {
            match &effect {
                Effect::PatternSearch { .. } => out.searches += 1,
                Effect::PatternHits { .. } => out.hits += 1,
                Effect::PatternSaved { .. } => out.saved += 1,
                _ => {}
            }
            if effect.runs_on_host() || matches!(effect, Effect::ScheduleAutoplayTick { .. }) {
                continue;
            }
            queue.push(rig.perform(effect));
        }
    }
    out
}

/// Any queen on the board, in the "Custom" tab.
fn pick_queen(rig: &mut Rig) {
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Piece,
    }));
    rig.pat(Pat::Edit(PatternEdit::Replace {
        path: vec![0],
        condition: Box::new(Condition::Piece(PieceCond {
            side: Who::Any,
            roles: vec![RoleName::Queen],
            region: Region::Anywhere,
            min: 1,
            max: None,
        })),
    }));
    assert!(rig.view().tree_issue.is_none());
}

/// A game that shuffles the kings `cycles` times (4 plies each) and then
/// plays `last`.
fn shuffle_game(fen: &str, cycle: [&str; 4], cycles: usize, last: &str, event: &str) -> String {
    let mut moves: Vec<&str> = Vec::new();
    for _ in 0..cycles {
        moves.extend(cycle);
    }
    moves.push(last);
    let mut text = String::new();
    for (i, m) in moves.iter().enumerate() {
        if i % 2 == 0 {
            text += &(i / 2 + 1).to_string();
            text += ". ";
        }
        text.push_str(m);
        text.push(' ');
    }
    format!("[Event \"{event}\"]\n[Result \"*\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n{text}*\n\n")
}

/// Game B: Black promotes at the 254th half move. Game A: White promotes
/// at the 253rd.
fn late_promotions() -> String {
    let b = shuffle_game(
        "k7/8/8/8/8/8/p7/7K w - - 0 1",
        ["Kg1", "Kb8", "Kh1", "Ka8"],
        63,
        "Kg1",
        "B",
    );
    // `shuffle_game` appends `last` as White's move; Black's promotion
    // needs one more move, so it is written out here.
    let b = b.replace("Kg1 *", "Kg1 a1=Q+ *");
    let a = shuffle_game(
        "7k/P7/8/8/8/8/8/K7 w - - 0 1",
        ["Kb1", "Kg8", "Ka1", "Kh8"],
        63,
        "a8=Q+",
        "A",
    );
    format!("{b}{a}")
}

// -------------------------------------------------------- opening, scope

#[test]
fn the_window_opens_by_the_action_and_by_open_and_open_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    assert!(rig.state.pattern_search_view().is_none());
    // The action opens it (and asks for the list of saved patterns once).
    let first = trace(&mut rig, Message::Action(Action::SearchPattern));
    assert_eq!(first.saved, 1);
    assert!(rig.state.pattern_search_view().is_some());
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::SelectPreset("pin".into()));
    rig.pat(Pat::Close);
    assert!(rig.state.pattern_search_view().is_none());
    // `Open` brings the same window back: the tab and the preset are kept
    // (the window "remembers"), and the list is not asked for again.
    let second = trace(&mut rig, pat(Pat::Open));
    assert_eq!(second.saved, 0);
    let view = rig.view();
    assert_eq!(view.tab, PatternTab::Advanced);
    assert_eq!(view.selected_preset.as_deref(), Some("pin"));
    // Opening twice changes nothing.
    rig.pat(Pat::Open);
    assert_eq!(rig.view(), view);
}

#[test]
fn every_message_is_harmless_with_the_window_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    for message in [
        Pat::Stop,
        Pat::Progress,
        Pat::AnswerCost(true),
        Pat::NextHit,
        Pat::PrevHit,
        Pat::GoToHit(3),
        Pat::ClearHits,
        Pat::Close,
        Pat::SelectTab(PatternTab::Advanced),
        Pat::Edit(PatternEdit::Remove(vec![4])),
    ] {
        let effects = rig.raw(pat(message.clone()));
        assert!(effects.is_empty(), "{message:?}: {effects:?}");
    }
    assert!(rig.state.pattern_search_view().is_none());
}

#[test]
fn a_reversed_range_of_moves_is_refused_and_pointed_at_the_scope() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    rig.pat(Pat::SetScope(Scope {
        min_move: 20,
        max_move: 5,
        ..Scope::default()
    }));
    let effects = rig.raw(pat(Pat::Start));
    assert!(effects.is_empty(), "{effects:?}");
    let notice = rig.view().issue.expect("notice");
    assert!(notice.is_form_error(), "{notice:?}");
    assert!(matches!(
        notice,
        PatternSearchNotice::InvalidPattern(PatternIssue::InvalidValue { .. })
    ));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    // Fixing the scope lets the search run.
    rig.pat(Pat::SetScope(Scope::default()));
    assert_eq!(rig.raw(pat(Pat::Start)).len(), 1);
}

#[test]
fn the_effect_carries_the_operation_of_the_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    for (op, want) in [
        (SearchOp::Reset, FilterOp::Reset),
        (SearchOp::And, FilterOp::And),
        (SearchOp::Or, FilterOp::Or),
    ] {
        rig.pat(Pat::SetOp(op));
        let Effect::PatternSearch { op: asked, .. } = rig.start_effect() else {
            panic!("search");
        };
        assert_eq!(asked, want);
        rig.pat(Pat::Stop);
    }
}

// ------------------------------------------------------------- the cost

#[test]
fn the_question_about_the_cost_starts_just_above_two_million_positions() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    // 33 333 games x 60 positions = 1 999 980: runs without asking.
    let below = rig.open_text(dir.path(), "below.pgn", &many_games(33_333));
    assert_eq!(rig.state.bases.slot(below).unwrap().total(), 33_333);
    rig.pat(Pat::Open);
    rig.pick_mate(Who::Any, 2);
    let effects = rig.raw(pat(Pat::Start));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(rig.view().confirm.is_none());
    for effect in effects {
        if let Effect::PatternSearch { control, .. } = effect {
            control.cancel();
        }
    }
    // 33 334 games: 2 000 040, over the limit.
    let above = rig.open_text(dir.path(), "above.pgn", &many_games(33_334));
    assert_eq!(rig.state.bases.current(), above);
    let effects = rig.raw(pat(Pat::Start));
    assert!(effects.is_empty(), "{effects:?}");
    let cost = rig.view().confirm.expect("question");
    assert_eq!(cost.games, 33_334);
    assert_eq!(cost.estimated_positions, 33_334 * 60);
    // OR looks only at the games outside the filter: none here, so it is
    // cheap, and AND looks at the games inside it: all of them.
    rig.pat(Pat::AnswerCost(false));
    rig.pat(Pat::SetOp(SearchOp::Or));
    let effects = rig.raw(pat(Pat::Start));
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert!(rig.view().confirm.is_none());
    for effect in effects {
        if let Effect::PatternSearch { control, .. } = effect {
            control.cancel();
        }
    }
    rig.pat(Pat::SetOp(SearchOp::And));
    assert!(rig.raw(pat(Pat::Start)).is_empty());
    assert_eq!(rig.view().confirm.expect("question").games, 33_334);
    // The question dies with the window and with Stop.
    rig.pat(Pat::Stop);
    assert!(rig.view().confirm.is_none());
}

#[test]
fn closing_the_window_drops_the_question_and_a_late_yes_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.open_text(dir.path(), "big.pgn", &many_games(34_000));
    rig.pat(Pat::Open);
    rig.pick_mate(Who::Any, 2);
    assert!(rig.raw(pat(Pat::Start)).is_empty());
    assert!(rig.view().confirm.is_some());
    rig.pat(Pat::Close);
    rig.pat(Pat::Open);
    assert!(rig.view().confirm.is_none());
    assert!(rig.raw(pat(Pat::AnswerCost(true))).is_empty());
    assert!(!rig.state.pattern_search().is_running());
}

// ------------------------------------------------ the counters of the run

#[test]
fn an_exhausted_mate_budget_is_counted_told_and_never_a_match() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pat(Pat::SelectPreset("mate_in".into()));
    rig.pat(Pat::SetParam(ParamValue::By(Who::White)));
    rig.pat(Pat::SetParam(ParamValue::N(5)));
    rig.pat(Pat::SetParam(ParamValue::MateMode(MateMode::Available)));
    rig.pat(Pat::SetParam(ParamValue::ForcingOnly(false)));
    // The smallest budget the preset accepts: mate in 5 cannot be decided.
    rig.pat(Pat::SetParam(ParamValue::MaxNodes(1000)));
    assert!(rig.view().param_issue.is_none());
    rig.pat(Pat::Start);
    let summary = rig.view().summary.expect("summary");
    assert!(summary.unknown_positions > 0, "{summary:?}");
    assert_eq!(summary.unreadable, 0);
    assert_eq!(
        rig.view().issue,
        Some(PatternSearchNotice::UnknownPositions {
            count: summary.unknown_positions
        })
    );
    let notice = rig.view().issue.unwrap();
    assert_eq!(notice.message_key(), "pattern-unknown");
    assert!(!notice.is_form_error());
    // The status bar shows the same notice.
    assert_eq!(rig.state.notice, Some(AppNotice::PatternSearch(notice)));
    // The result is the same on a second run (D3296).
    let first = rig.filter_values(slot);
    rig.pat(Pat::Start);
    assert_eq!(rig.filter_values(slot), first);
    assert_eq!(
        rig.view().summary.unwrap().unknown_positions,
        summary.unknown_positions
    );
}

#[test]
fn a_run_without_unknown_positions_or_unreadable_games_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    let summary = rig.view().summary.expect("summary");
    assert_eq!((summary.unknown_positions, summary.unreadable), (0, 0));
    assert!(rig.view().issue.is_none());
}

#[test]
fn unreadable_games_are_counted_and_told_but_the_result_stays() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let effect = rig.start_effect();
    let Message::PatternSearch(Pat::Finished {
        request,
        result: Ok(mut done),
    }) = rig.perform(effect)
    else {
        panic!("finished");
    };
    done.report.unreadable_total = 3;
    rig.raw(pat(Pat::Finished {
        request,
        result: Ok(done),
    }));
    let summary = rig.view().summary.expect("summary");
    assert_eq!(summary.unreadable, 3);
    let notice = rig.view().issue.expect("notice");
    assert_eq!(notice, PatternSearchNotice::Unreadable { count: 3 });
    assert_eq!(notice.message_key(), "pattern-unreadable");
    assert!(!notice.is_form_error());
    assert_eq!(rig.filter_values(slot), [(1, 7), (5, 13)]);
}

#[test]
fn the_summary_counts_the_games_before_and_after() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    let s = rig.view().summary.unwrap();
    assert_eq!((s.slot, s.matched, s.start_count), (slot, 2, 8));
    // AND: the "before" is the filter's count (2), the "after" what stays.
    rig.pat(Pat::SetOp(SearchOp::And));
    rig.pick_mate(Who::Black, 1);
    rig.pat(Pat::Start);
    let s = rig.view().summary.unwrap();
    assert_eq!((s.matched, s.start_count), (0, 2));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filtered(), 0);
}

// --------------------------------------------------- reference, no base

#[test]
fn a_reference_database_that_was_closed_is_no_database() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, a) = Rig::tactics(dir.path());
    let b = rig.open_text(dir.path(), "b.pgn", "[Event \"b\"]\n\n1. d4 *\n\n");
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::SetReference(Some(a)));
    rig.send(BasesMessage::Close(a));
    let before = rig.state.bases.slot(b).unwrap().filter().clone();
    let effects = rig.raw(pat(Pat::Start));
    assert!(effects.is_empty(), "{effects:?}");
    assert!(!rig.state.pattern_search().is_running());
    let notice = rig.view().issue.expect("notice");
    assert_eq!(notice, PatternSearchNotice::NoDatabase);
    assert_eq!(notice.message_key(), "pattern-notice-no-database");
    assert!(!notice.is_form_error());
    assert_eq!(rig.state.bases.slot(b).unwrap().filter(), &before);
    // Back to "the current one" it runs.
    rig.pat(Pat::SetReference(None));
    assert_eq!(rig.raw(pat(Pat::Start)).len(), 1);
}

#[test]
fn the_reference_database_uses_its_own_filter_for_and() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, a) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    assert_eq!(rig.filter_values(a), [(1, 7), (5, 13)]);
    // Another database becomes the current one with a filter of its own.
    let b = rig.open_text(dir.path(), "b.pgn", TACTICS_PGN);
    assert_eq!(rig.state.bases.slot(b).unwrap().filtered(), 8);
    // AND on `a`, whatever the current slot is, starts from `a`'s filter:
    // "either colour" would find games 1, 2 and 5, but 2 is out of `a`.
    rig.pick_mate(Who::Any, 1);
    rig.pat(Pat::SetOp(SearchOp::And));
    rig.pat(Pat::SetReference(Some(a)));
    rig.pat(Pat::Start);
    assert_eq!(
        rig.filter_values(a).iter().map(|g| g.0).collect::<Vec<_>>(),
        [1, 5]
    );
    assert_eq!(rig.state.bases.slot(b).unwrap().filtered(), 8);
    let s = rig.view().summary.unwrap();
    assert_eq!((s.slot, s.start_count), (a, 2));
    // The result is shown where it was found: the reference is the current.
    assert_eq!(rig.state.bases.current(), a);
}

#[test]
fn stopping_with_nothing_running_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    assert!(rig.raw(pat(Pat::Stop)).is_empty());
    assert!(rig.view().issue.is_none());
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
}

// ------------------------------------------------------ plies past 253

#[test]
fn a_match_past_the_253rd_half_move_stores_one_but_lands_on_the_real_ply() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    // B is game 1 (the first of the filter), A is game 2.
    let slot = rig.open_text(dir.path(), "late.pgn", &late_promotions());
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 2);
    rig.pat(Pat::Open);
    pick_queen(&mut rig);
    rig.pat(Pat::Start);
    // D3253: ply 253 -> 254 (the last value that fits), ply 254 -> 1.
    assert_eq!(rig.filter_values(slot), [(1, 1), (2, 254)]);
    // The first game of the search lands on its first occurrence anyway.
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(1));
    assert_eq!(rig.cursor_ply(), 254);
    let hits = rig.view().hits.expect("hits");
    assert_eq!(hits.plies, [254]);
    assert_eq!(hits.current, 0);
    // The other game has its occurrence at 253: the filter value 254 says
    // the same, and the occurrences are exact.
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    assert_eq!(rig.view().hits.expect("hits").plies, [253]);
}

#[test]
fn the_first_game_already_in_the_editor_goes_to_its_occurrence() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    assert_eq!(rig.cursor_ply(), 0);
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    assert_eq!(rig.cursor_ply(), 6);
    assert_eq!(rig.view().hits.expect("hits").plies, [6]);
}

// ------------------------------------------------ cursor and occurrences

fn check_in_game_five() -> (Rig, SlotId, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
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
    rig.pat(Pat::Start);
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    assert_eq!(rig.view().hits.expect("hits").plies, [11, 13]);
    (rig, slot, dir)
}

#[test]
fn the_current_occurrence_follows_the_cursor_and_else_the_last_visited() {
    let (mut rig, _slot, _dir) = check_in_game_five();
    rig.pat(Pat::GoToHit(1));
    assert_eq!(rig.view().hits.unwrap().current, 1);
    // Between two occurrences the last visited stays.
    rig.send(Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(
        12,
    ))));
    assert_eq!(rig.view().hits.unwrap().current, 1);
    rig.send(Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(
        0,
    ))));
    assert_eq!(rig.view().hits.unwrap().current, 1);
    // On an occurrence the cursor decides, even if it got there by hand.
    rig.send(Message::Game(GameMessage::Navigate(Nav::GoToMainlinePly(
        11,
    ))));
    assert_eq!(rig.view().hits.unwrap().current, 0);
    // A step past the ends wraps (the labels "k of n" never leave 1..=n).
    rig.pat(Pat::PrevHit);
    assert_eq!(rig.view().hits.unwrap().current, 1);
    rig.pat(Pat::NextHit);
    assert_eq!(rig.view().hits.unwrap().current, 0);
}

#[test]
fn the_marks_are_only_drawn_at_the_cursor_on_the_main_line() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _slot) = Rig::tactics(dir.path());
    rig.pat(Pat::SelectPreset("fork".into()));
    rig.pat(Pat::SetParam(ParamValue::By(Who::White)));
    rig.pat(Pat::Start);
    assert_eq!(rig.cursor_ply(), 1);
    assert!(!rig.view().highlights.is_empty());
    // A variation from the same position is not the main line: no marks,
    // and the occurrence stays for the buttons.
    rig.send(Message::Game(GameMessage::Navigate(Nav::Back(1))));
    rig.send(Message::Game(GameMessage::StartVariation));
    rig.send(Message::Game(GameMessage::EnterMoveText("Kd2".into())));
    assert_eq!(rig.cursor_ply(), 1);
    assert!(
        rig.state
            .session
            .game()
            .mainline()
            .all(|n| n != rig.state.session.cursor())
    );
    assert!(rig.view().highlights.is_empty());
    assert_eq!(rig.view().hits.expect("hits").plies, [1]);
    // The marks are the app's: the game has no comment from them.
    assert!(
        rig.state
            .session
            .game()
            .comment(rig.state.session.cursor())
            .unwrap()
            .is_none()
    );
    // Back on the main line at the occurrence they are drawn again.
    rig.pat(Pat::GoToHit(0));
    assert_eq!(rig.cursor_ply(), 1);
    assert!(!rig.view().highlights.is_empty());
}

#[test]
fn a_game_of_another_database_gets_no_occurrences_of_this_search() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, a) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    assert!(rig.view().hits.is_some());
    let b = rig.open_text(dir.path(), "b.pgn", TACTICS_PGN);
    assert_ne!(a, b);
    let t = trace(
        &mut rig,
        Message::Bases(BasesMessage::LoadGame { slot: b, number: 1 }),
    );
    assert_eq!(t.hits, 0, "{t:?}");
    assert!(rig.view().hits.is_none());
    assert!(rig.view().highlights.is_empty());
    // Coming back to the searched database brings them back.
    let t = trace(
        &mut rig,
        Message::Bases(BasesMessage::LoadGame { slot: a, number: 1 }),
    );
    assert_eq!(t.hits, 1, "{t:?}");
    assert_eq!(rig.view().hits.expect("hits").plies, [6]);
}

#[test]
fn a_new_search_replaces_the_pattern_the_games_show() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    assert_eq!(rig.view().hits.unwrap().plies, [6]);
    rig.pick_mate(Who::Black, 1);
    rig.pat(Pat::SetOp(SearchOp::Reset));
    rig.pat(Pat::Start);
    // The first game is now game 2, at its own ply.
    assert_eq!(rig.state.bases.slot(slot).unwrap().current_game, Some(2));
    assert_eq!(rig.cursor_ply(), 3);
    assert_eq!(rig.view().hits.unwrap().plies, [3]);
}

#[test]
fn a_search_without_results_leaves_the_game_and_has_no_occurrences() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    // Game 6 is out of the filter; it stays in the editor.
    rig.send(BasesMessage::LoadGame { slot, number: 6 });
    let fen = rig.state.session.position().fen();
    rig.pat(Pat::SetOp(SearchOp::And));
    rig.pick_mate(Who::Black, 1);
    rig.pat(Pat::Start);
    assert!(rig.filter_values(slot).is_empty());
    assert_eq!(rig.state.session.position().fen(), fen);
    assert!(rig.view().hits.is_none_or(|h| h.plies.is_empty()));
    assert!(rig.view().highlights.is_empty());
}

// ------------------------------------------------------- test this game

#[test]
fn testing_the_game_asks_only_for_the_occurrences_and_does_not_move() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    rig.pick_mate(Who::White, 1);
    let cursor = rig.state.session.cursor();
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let t = trace(&mut rig, pat(Pat::TestGame));
    assert_eq!((t.searches, t.hits, t.saved), (0, 1, 0), "{t:?}");
    assert_eq!(rig.state.session.cursor(), cursor);
    let hits = rig.view().hits.expect("hits");
    assert_eq!(hits.plies, [12]);
    assert!(hits.from_test);
    assert_eq!(hits.game, Some(5));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    // A result for one game does not follow the others.
    rig.send(BasesMessage::LoadGame { slot, number: 1 });
    assert!(rig.view().hits.is_none());
}

#[test]
fn testing_a_game_that_is_not_from_a_database_works() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new(dir.path());
    rig.pat(Pat::Open);
    rig.pat(Pat::SelectTab(PatternTab::Advanced));
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::Edit(PatternEdit::Add {
        parent: vec![],
        kind: ConditionKind::Status,
    }));
    for text in ["f3", "e5", "g4", "Qh4#"] {
        rig.send(Message::Game(GameMessage::EnterMoveText(text.into())));
    }
    let t = trace(&mut rig, pat(Pat::TestGame));
    assert_eq!(t.hits, 1);
    let hits = rig.view().hits.expect("hits");
    assert!(hits.from_test);
    assert_eq!(hits.game, None);
    assert_eq!(hits.plies, [4]);
}

#[test]
fn testing_in_the_presets_tab_uses_the_preset() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    rig.pick_mate(Who::Black, 1);
    rig.send(pat(Pat::TestGame));
    assert_eq!(rig.view().hits.expect("hits").plies, [3]);
}

// --------------------------------------------------- the online lock

#[test]
fn the_online_lock_keeps_occurrences_away_and_unlocking_brings_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    assert!(rig.state.assist_locked());
    // The search itself is allowed, but no game is loaded and no
    // occurrence is computed (D3383).
    let t = trace(&mut rig, pat(Pat::Start));
    assert_eq!(t.hits, 0, "{t:?}");
    assert_eq!(rig.filter_values(slot), [(1, 7), (5, 13)]);
    assert!(rig.view().hits.is_none());
    assert!(rig.view().highlights.is_empty());
    // The buttons do nothing without occurrences.
    let ply = rig.cursor_ply();
    rig.pat(Pat::NextHit);
    rig.pat(Pat::GoToHit(0));
    assert_eq!(rig.cursor_ply(), ply);
    // Unlocked, the next game of the filter brings the occurrences back.
    rig.state.online.phase = OnlinePhase::Idle;
    assert!(!rig.state.assist_locked());
    let t = trace(
        &mut rig,
        Message::Bases(BasesMessage::LoadGame { slot, number: 5 }),
    );
    assert_eq!(t.hits, 1, "{t:?}");
    assert_eq!(rig.view().hits.expect("hits").plies, [12]);
}

#[test]
fn the_test_on_this_game_is_refused_while_the_assistance_is_locked() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.send(BasesMessage::LoadGame { slot, number: 5 });
    rig.pick_mate(Who::White, 1);
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    let t = trace(&mut rig, pat(Pat::TestGame));
    assert_eq!(t.hits, 0);
    assert!(rig.view().hits.is_none());
    // After the game the lock is released and the test works.
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Finished);
    assert!(!rig.state.assist_locked());
    let t = trace(&mut rig, pat(Pat::TestGame));
    assert_eq!(t.hits, 1);
    assert_eq!(rig.view().hits.unwrap().plies, [12]);
}

// ---------------------------------------------------------- stale answers

#[test]
fn an_answer_with_a_filter_of_the_wrong_size_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let before = rig.state.bases.slot(slot).unwrap().filter().clone();
    let effect = rig.start_effect();
    let Message::PatternSearch(Pat::Finished {
        request,
        result: Ok(mut done),
    }) = rig.perform(effect)
    else {
        panic!("finished");
    };
    done.filter = Filter::new(3);
    rig.raw(pat(Pat::Finished {
        request,
        result: Ok(done),
    }));
    assert_eq!(rig.view().issue, Some(PatternSearchNotice::Stale));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filter(), &before);
    assert!(rig.view().summary.is_none());
    assert!(!rig.state.pattern_search().is_running());
}

#[test]
fn an_answer_for_a_database_that_was_replaced_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, a) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let effect = rig.start_effect();
    rig.send(BasesMessage::Close(a));
    let c = rig.open_text(dir.path(), "c.pgn", TACTICS_PGN);
    let untouched = rig.state.bases.slot(c).unwrap().filter().clone();
    let answer = rig.perform(effect);
    rig.send(answer);
    assert_eq!(rig.view().issue, Some(PatternSearchNotice::Stale));
    assert_eq!(rig.state.bases.slot(c).unwrap().filter(), &untouched);
    assert!(rig.view().hits.is_none());
}

#[test]
fn an_answer_with_an_unknown_request_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    let effect = rig.start_effect();
    let real = request_of(&effect);
    for request in [RequestId(real.0 + 1000), RequestId(0)] {
        rig.raw(pat(Pat::Finished {
            request,
            result: Err(PatternFailure::Internal),
        }));
        rig.raw(pat(Pat::Cancelled { request }));
        rig.raw(pat(Pat::HitsDone {
            request,
            result: Ok(vec![]),
        }));
        assert!(rig.state.pattern_search().is_running());
        assert!(rig.view().issue.is_none());
    }
    let answer = rig.perform(effect);
    rig.raw(answer);
    assert_eq!(rig.filter_values(slot).len(), 2);
}

#[test]
fn closing_the_window_cancels_and_a_late_hits_answer_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    // Ask for the game 5 occurrences but hold the answer back.
    let effects = rig.raw(Message::Bases(BasesMessage::LoadGame { slot, number: 5 }));
    let load = effects
        .into_iter()
        .find(|e| matches!(e, Effect::LoadBaseGame { .. }))
        .expect("load");
    let loaded = rig.perform(load);
    let hits = rig
        .raw(loaded)
        .into_iter()
        .find(|e| matches!(e, Effect::PatternHits { .. }))
        .expect("hits effect");
    rig.pat(Pat::Close);
    let answer = rig.perform(hits);
    rig.raw(answer);
    rig.pat(Pat::Open);
    assert!(rig.view().hits.is_none());
    assert!(rig.view().highlights.is_empty());
}

#[test]
fn a_failed_computation_of_occurrences_tells_and_shows_none() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::White, 1);
    rig.pat(Pat::Start);
    let effects = rig.raw(Message::Bases(BasesMessage::LoadGame { slot, number: 5 }));
    let load = effects
        .into_iter()
        .find(|e| matches!(e, Effect::LoadBaseGame { .. }))
        .expect("load");
    let loaded = rig.perform(load);
    let request = rig
        .raw(loaded)
        .into_iter()
        .find_map(|e| match e {
            Effect::PatternHits { request, .. } => Some(request),
            _ => None,
        })
        .expect("hits effect");
    rig.raw(pat(Pat::HitsDone {
        request,
        result: Err(PatternFailure::Internal),
    }));
    let notice = rig.view().issue.expect("notice");
    assert_eq!(notice, PatternSearchNotice::HitsFailed);
    assert_eq!(notice.message_key(), "pattern-notice-hits-failed");
    assert!(!notice.is_form_error());
    assert!(rig.view().hits.is_none());
}

#[test]
fn editing_the_draft_during_a_search_does_not_change_the_search() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    pick_queen(&mut rig);
    rig.pat(Pat::SetOp(SearchOp::Reset));
    let effect = rig.start_effect();
    // The user keeps editing: the search runs on the copy it took.
    rig.pat(Pat::Edit(PatternEdit::Clear));
    rig.pat(Pat::Edit(PatternEdit::ToggleSquare {
        path: vec![],
        slot: RegionSlot::Main,
        square: mychess_core::Square::A1,
    }));
    let answer = rig.perform(effect);
    rig.raw(answer);
    // Queens appear in the promotion games 7 and 8 (a8=N is no queen) and
    // in the scholar's mate position (the queen is there from the start,
    // so every game matches at its first position of the scope).
    assert!(!rig.filter_values(slot).is_empty());
}

#[test]
fn the_tree_of_the_set_of_games_is_independent_of_the_threads() {
    // D3296: the same pattern and base give the same filter and counters.
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, slot) = Rig::tactics(dir.path());
    rig.pick_mate(Who::Any, 2);
    rig.pat(Pat::SetParam(ParamValue::MateMode(MateMode::Available)));
    rig.pat(Pat::Start);
    let first = (
        rig.filter_values(slot),
        rig.view().summary.unwrap().positions_examined,
        rig.view().summary.unwrap().unknown_positions,
    );
    rig.pat(Pat::SetOp(SearchOp::Reset));
    rig.pat(Pat::Start);
    let second = (
        rig.filter_values(slot),
        rig.view().summary.unwrap().positions_examined,
        rig.view().summary.unwrap().unknown_positions,
    );
    assert_eq!(first, second);
    let games: BTreeSet<u32> = first.0.iter().map(|g| g.0).collect();
    assert!(games.contains(&5), "{games:?}");
}
