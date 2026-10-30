//! Tests of the game report run: the fake engine of the annotation tests
//! answers the searches (spec M4-10b).

use mychess_engine::EngineCommand;

use super::annotate_tests::{
    answer, drive, game_of, last_search, material_engine, material_reply, position_of, send, setup,
};
use super::{AppState, update};
use crate::annotate::AnnotateNotice;
use crate::flow::AppNotice;
use crate::message::Message;
use crate::report::{MoveClass, ReportMessage};

/// 20 plies; 3.Qxe5 hangs the queen.
const HANGING_QUEEN: [&str; 20] = [
    "e2e4", "e7e5", "d1h5", "b8c6", "h5e5", "c6e5", "g1f3", "e5f3", "g2f3", "d7d6", "f1c4", "g8f6",
    "d2d3", "f8e7", "c1e3", "e8g8", "b1c3", "c7c6", "e1c1", "b7b5",
];

fn generate(state: &mut AppState) -> Vec<crate::effect::Effect> {
    send(state, ReportMessage::Generate)
}

#[test]
fn a_report_run_analyses_every_position_with_two_lines_and_leaves_the_game_alone() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let changes = state.session.changes();
    let depth = state.session.undo_depth();
    let effects = generate(&mut state);
    assert!(state.report_running());
    assert!(state.annotation_running());
    let log = drive(&mut state, run, effects, 100, &mut material_engine);
    assert!(!state.annotation_running(), "the run ended");
    assert!(
        log.commands
            .iter()
            .any(|c| matches!(c, EngineCommand::SetMultiPv(2))),
        "the report asks for the second line"
    );
    assert_eq!(log.requests.len(), 21, "one search per position");
    // Nothing was written to the game (D1447).
    assert_eq!(state.session.changes(), changes);
    assert_eq!(state.session.undo_depth(), depth);
    assert!(!state.is_dirty());

    let data = state.report_data().expect("the report");
    assert!(data.complete);
    assert_eq!(data.report.moves.len(), 20);
    assert_eq!(data.sans.len(), 20);
    assert_eq!(data.report.white.total() + data.report.black.total(), 20);
    // The hanging queen is the biggest fault of the game.
    let blunder = &data.report.moves[4];
    assert_eq!(blunder.ply, 5);
    assert!(
        matches!(blunder.class, MoveClass::Blunder | MoveClass::Mistake),
        "{:?}",
        blunder.class
    );
    assert_eq!(data.graph.plies, 20);
    assert!(
        data.graph
            .line
            .as_ref()
            .is_some_and(|l| l.markers.iter().any(|m| m.ply == 5))
    );
}

#[test]
fn cancelling_keeps_a_partial_report_and_says_so() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let effects = generate(&mut state);
    let log = drive(&mut state, run, effects, 3, &mut material_engine);
    assert!(state.report_running(), "three of twenty-one searches done");
    let effects = send(&mut state, ReportMessage::Cancel);
    // The search in flight answers the stop.
    let request = last_search(&log.last).expect("a search in flight");
    let reply = material_reply(&position_of(&request));
    drop(answer(&mut state, run, &log.last, 900, &reply));
    drop(effects);
    assert!(!state.annotation_running());
    let data = state.report_data().expect("partial report");
    assert!(!data.complete);
    assert_eq!(data.report.moves.len(), 2);
    assert!(matches!(
        state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportStopped {
            plies: 2,
            total: 20
        }))
    ));
}

#[test]
fn a_report_of_another_main_line_is_not_shown() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "g1f3"]));
    let effects = generate(&mut state);
    drop(drive(&mut state, run, effects, 10, &mut material_engine));
    assert!(state.report_data().is_some());
    state.session.replace_game(game_of(&["d2d4"]));
    assert!(state.report_data().is_none());
}

#[test]
fn a_game_changed_during_the_run_gives_no_report() {
    let (mut state, run) = setup(game_of(&["e2e4", "e7e5", "g1f3"]));
    let effects = generate(&mut state);
    let log = drive(&mut state, run, effects, 1, &mut material_engine);
    assert!(state.report_running());
    state.session.replace_game(game_of(&["d2d4"]));
    drop(update(&mut state, Message::FlushPreferences));
    drop(log);
    assert!(!state.annotation_running());
    assert!(state.report.data.is_none());
}

#[test]
fn a_game_without_moves_cannot_be_reported() {
    let (mut state, _run) = setup(mychess_core::Game::new());
    drop(generate(&mut state));
    assert!(!state.annotation_running());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::NothingToAnnotate))
    ));
}

#[test]
fn going_to_a_ply_moves_the_cursor_and_the_filter_is_remembered() {
    let (mut state, _run) = setup(game_of(&HANGING_QUEEN));
    drop(send(&mut state, ReportMessage::GoTo(5)));
    let game = state.session.game();
    assert_eq!(game.mainline().nth(4), Some(state.session.cursor()));
    drop(send(
        &mut state,
        ReportMessage::Filter(Some(MoveClass::Blunder)),
    ));
    assert_eq!(state.report.filter, Some(MoveClass::Blunder));
    drop(send(&mut state, ReportMessage::Clear));
    assert!(state.report.data.is_none());
}

#[test]
fn writing_the_report_is_one_undo_point_and_nothing_happens_without_one() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    // Without a report there is nothing to write.
    let changes = state.session.changes();
    drop(send(&mut state, ReportMessage::WriteToGame));
    assert_eq!(state.session.changes(), changes);

    let effects = generate(&mut state);
    drop(drive(&mut state, run, effects, 100, &mut material_engine));
    let depth = state.session.undo_depth();
    drop(send(&mut state, ReportMessage::WriteToGame));
    assert_eq!(state.session.undo_depth(), depth + 1, "one undo point");
    assert!(matches!(
        state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportWritten {
            moves: 20,
            ..
        }))
    ));
    let game = state.session.game();
    let scored = game
        .mainline()
        .filter(|n| {
            game.comment(*n)
                .ok()
                .flatten()
                .is_some_and(|c| c.contains("[%eval"))
        })
        .count();
    assert_eq!(scored, 20);
    // The report still describes the (same moves of the) game.
    assert!(state.report_data().is_some());
    // And the game can be reported again from what it now says.
    drop(send(&mut state, ReportMessage::Clear));
    drop(send(&mut state, ReportMessage::FromGame));
    let data = state.report_data().expect("report from the comments");
    assert_eq!(data.report.moves.len(), 20);
}

#[test]
fn a_game_without_evaluations_cannot_be_reported_from_itself() {
    let (mut state, _run) = setup(game_of(&["e2e4", "e7e5"]));
    drop(send(&mut state, ReportMessage::FromGame));
    assert!(state.report.data.is_none());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportNoEvals))
    ));
}

#[test]
fn the_review_visits_the_faults_and_judges_the_attempts_on_the_board() {
    use crate::report::{ReviewStep, SideFilter};
    use crate::session::{GameMessage, PlayOptions};

    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    // The walk to the moment is the subject of the tests below.
    state.prefs.board.animation_ms = 0;
    let effects = generate(&mut state);
    drop(drive(&mut state, run, effects, 100, &mut material_engine));
    add_second_moment(&mut state);
    drop(send(
        &mut state,
        ReportMessage::StartReview {
            side: SideFilter::Both,
            highlights: false,
        },
    ));
    let review = state.report.review.clone().expect("a review");
    let ply = review.current().expect("a moment").ply;
    // The board is at the position before the faulty move.
    assert_eq!(
        state
            .session
            .game()
            .mainline()
            .nth(ply - 2)
            .unwrap_or(state.session.game().root()),
        state.session.cursor()
    );
    let best = state.report_data().expect("data").report.moves[ply - 1]
        .best
        .expect("the engine's move");
    let played = state.report_data().expect("data").report.moves[ply - 1].played;
    let changes = state.session.changes();

    // Not trying yet: the move goes to the game as usual (not consumed).
    drop(send(&mut state, ReportMessage::ReviewTry));
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(ReviewStep::Trying)
    );
    drop(send(
        &mut state,
        GameMessage::PlayMove {
            mv: played,
            options: PlayOptions::default(),
        },
    ));
    assert_eq!(
        state.session.changes(),
        changes,
        "the attempt is not added to the game"
    );
    assert_eq!(state.report.review.as_ref().map(|r| r.attempts), Some(1));
    drop(send(
        &mut state,
        GameMessage::PlayMove {
            mv: best,
            options: PlayOptions::default(),
        },
    ));
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(ReviewStep::Solved)
    );
    assert_eq!(state.session.changes(), changes);

    // Next moves on and the board follows; Exit ends the review.
    let more = state.report.review.as_ref().map_or(0, |r| r.moments.len());
    assert_eq!(more, 2, "the fixture has two moments");
    drop(send(&mut state, ReportMessage::ReviewNext));
    assert_eq!(state.report.review.as_ref().map(|r| r.index), Some(1));
    drop(send(&mut state, ReportMessage::ReviewExit));
    assert!(state.report.review.is_none());
}

#[test]
fn a_review_needs_a_report_with_something_to_review() {
    use crate::report::SideFilter;

    let (mut state, _run) = setup(game_of(&HANGING_QUEEN));
    drop(send(
        &mut state,
        ReportMessage::StartReview {
            side: SideFilter::White,
            highlights: false,
        },
    ));
    assert!(state.report.review.is_none());
}

// ---- the review under the interceptor (D1580) ----

/// A review in the trying step over the hanging-queen game; returns the
/// ply of the moment, the move played there and the engine's move.
fn trying_state() -> (AppState, usize, mychess_core::Move, mychess_core::Move) {
    use crate::report::SideFilter;

    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let effects = generate(&mut state);
    drop(drive(&mut state, run, effects, 100, &mut material_engine));
    drop(send(
        &mut state,
        ReportMessage::StartReview {
            side: SideFilter::Both,
            highlights: false,
        },
    ));
    drop(send(&mut state, ReportMessage::ReviewTry));
    let ply = state
        .report
        .review
        .as_ref()
        .and_then(crate::report::Review::current)
        .expect("a moment")
        .ply;
    let moves = &state.report_data().expect("data").report.moves;
    let (played, best) = (moves[ply - 1].played, moves[ply - 1].best.expect("best"));
    (state, ply, played, best)
}

/// Nothing reached the game and the attempt was counted.
fn assert_judged_not_played(state: &AppState, changes: u64, cursor: mychess_core::NodeId) {
    assert_eq!(state.session.changes(), changes, "the game is untouched");
    assert_eq!(state.session.cursor(), cursor);
    let review = state.report.review.as_ref().expect("review");
    assert!(
        review.attempts == 1 || review.step == crate::report::ReviewStep::Solved,
        "the attempt was judged: {review:?}"
    );
}

#[test]
fn typed_move_text_is_judged_and_not_played() {
    use crate::session::GameMessage;
    let (mut state, ply, played, best) = trying_state();
    let (changes, cursor) = (state.session.changes(), state.session.cursor());
    let before = state
        .session
        .game()
        .position_at(cursor)
        .expect("position")
        .clone();
    let wrong = before.san(played).expect("san");
    drop(send(&mut state, GameMessage::EnterMoveText(wrong)));
    assert_judged_not_played(&state, changes, cursor);
    let right = before.san(best).expect("san");
    drop(send(&mut state, GameMessage::EnterMoveText(right)));
    assert_eq!(state.session.changes(), changes);
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(crate::report::ReviewStep::Solved),
        "ply {ply}"
    );
}

#[test]
fn keyboard_entry_is_judged_and_not_played() {
    use crate::session::GameMessage;
    let (mut state, _ply, played, _best) = trying_state();
    let (changes, cursor) = (state.session.changes(), state.session.cursor());
    let text = state
        .session
        .game()
        .position_at(cursor)
        .expect("position")
        .san(played)
        .expect("san");
    for ch in text.chars().filter(|c| !matches!(c, 'x' | '+' | '#' | '=')) {
        drop(send(&mut state, GameMessage::MoveEntryTyped(ch)));
    }
    drop(send(&mut state, GameMessage::MoveEntryCommit));
    assert_judged_not_played(&state, changes, cursor);
}

#[test]
fn the_null_move_and_king_takes_king_are_wrong_attempts() {
    use crate::session::{GameMessage, PlayOptions};
    use mychess_core::Square;
    let (mut state, _ply, _played, _best) = trying_state();
    let (changes, cursor) = (state.session.changes(), state.session.cursor());
    drop(send(&mut state, GameMessage::PlayNullMove));
    assert_judged_not_played(&state, changes, cursor);
    drop(send(
        &mut state,
        GameMessage::PlaySquares {
            from: Square::E1,
            to: Square::E8,
            promotion: None,
            options: PlayOptions::default(),
        },
    ));
    assert_eq!(state.session.changes(), changes);
    assert_eq!(state.session.cursor(), cursor);
    assert_eq!(state.report.review.as_ref().map(|r| r.attempts), Some(2));
}

#[test]
fn variations_and_replace_answers_are_refused_while_trying() {
    use crate::session::{GameMessage, ReplaceChoice};
    let (mut state, ..) = trying_state();
    let (changes, cursor) = (state.session.changes(), state.session.cursor());
    for message in [
        GameMessage::StartVariation,
        GameMessage::AnswerReplace(ReplaceChoice::Replace),
    ] {
        state.notice = None;
        drop(send(&mut state, message));
        assert!(matches!(
            state.notice,
            Some(AppNotice::Annotate(AnnotateNotice::ReviewMoveIgnored))
        ));
    }
    assert_eq!(state.session.changes(), changes);
    assert_eq!(state.session.cursor(), cursor);
}

#[test]
fn an_illegal_attempt_is_consumed_with_the_sessions_remark() {
    use crate::session::{GameMessage, PlayOptions};
    use mychess_core::Square;
    let (mut state, ..) = trying_state();
    let changes = state.session.changes();
    drop(send(
        &mut state,
        GameMessage::PlaySquares {
            from: Square::A1,
            to: Square::A5,
            promotion: None,
            options: PlayOptions::default(),
        },
    ));
    assert_eq!(state.session.changes(), changes);
    assert_eq!(state.report.review.as_ref().map(|r| r.attempts), Some(0));
    assert!(state.session.notice().is_some());
}

/// 1.a4 b5 2.axb5 Nf6 3.b6 Ne4 4.bxc7 Ng5 5.cxb8: the knight is the better
/// promotion (fake evaluations), the game has the queen.
fn promotion_state() -> AppState {
    use crate::annotate::PositionEval;
    use crate::report::{
        ReportData, ReportPreset, ReportThresholds, build_report, inputs_from_game,
    };
    let game = game_of(&[
        "a2a4", "b7b5", "a4b5", "g8f6", "b5b6", "f6e4", "b6c7", "e4g5", "c7b8q",
    ]);
    let node = game.mainline().nth(7).expect("before the promotion");
    let best = game
        .position_at(node)
        .expect("position")
        .parse_uci("c7b8n")
        .expect("uci");
    let eval = |score: f32, pv: Vec<mychess_core::Move>| PositionEval {
        score,
        mate: None,
        pv,
        depth: 10,
        second: None,
    };
    let mut evals: Vec<PositionEval> = (0..9).map(|_| eval(0.0, Vec::new())).collect();
    evals[8] = eval(0.0, vec![best]);
    evals.push(eval(-6.0, Vec::new()));
    let (mut state, _run) = setup(game);
    let inputs = inputs_from_game(state.session.game(), &evals, None);
    let thresholds = ReportThresholds::default();
    let report = build_report(&inputs.moves, &thresholds);
    state.report.data = Some(ReportData::new(
        state.session.game(),
        report,
        inputs.sans,
        evals,
        ReportPreset::default(),
        thresholds,
    ));
    state
}

#[test]
fn a_promotion_without_a_piece_asks_and_a_subpromotion_can_be_the_answer() {
    use crate::report::{ReviewStep, SideFilter};
    use crate::session::{GameMessage, PlayOptions, Prompt};
    use mychess_core::{Role, Square};

    let mut state = promotion_state();
    drop(send(
        &mut state,
        ReportMessage::StartReview {
            side: SideFilter::Both,
            highlights: false,
        },
    ));
    drop(send(&mut state, ReportMessage::ReviewTry));
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(ReviewStep::Trying)
    );
    let (changes, cursor) = (state.session.changes(), state.session.cursor());
    drop(send(
        &mut state,
        GameMessage::PlaySquares {
            from: Square::C7,
            to: Square::B8,
            promotion: None,
            options: PlayOptions::default(),
        },
    ));
    assert!(
        matches!(state.session.prompt(), Some(Prompt::ChoosePromotion { .. })),
        "the piece is asked, not guessed"
    );
    assert_eq!(state.report.review.as_ref().map(|r| r.attempts), Some(0));
    drop(send(
        &mut state,
        GameMessage::AnswerPromotion(Some(Role::Queen)),
    ));
    assert_eq!(state.report.review.as_ref().map(|r| r.attempts), Some(1));
    // Again, now with the knight: the engine's choice.
    drop(send(
        &mut state,
        GameMessage::PlaySquares {
            from: Square::C7,
            to: Square::B8,
            promotion: None,
            options: PlayOptions::default(),
        },
    ));
    drop(send(
        &mut state,
        GameMessage::AnswerPromotion(Some(Role::Knight)),
    ));
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(ReviewStep::Solved)
    );
    assert_eq!(state.session.changes(), changes);
    assert_eq!(state.session.cursor(), cursor);
}

#[test]
fn changing_the_game_ends_the_review() {
    let (mut state, ..) = trying_state();
    assert!(state.report.review.is_some());
    state.session.replace_game(game_of(&["d2d4"]));
    drop(send(&mut state, ReportMessage::Filter(None)));
    assert!(state.report.review.is_none());
}

// ---- writing the report (D1582, D1585) ----

#[test]
fn writing_keeps_the_users_nag_and_a_second_write_changes_nothing() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let effects = generate(&mut state);
    drop(drive(&mut state, run, effects, 100, &mut material_engine));
    let blunder = state.session.game().mainline().nth(4).expect("3.Qxe5");
    let nag = mychess_core::Nag::new(1).expect("nag");
    state
        .session
        .edit(|game, _| game.add_nag(blunder, nag).map(drop))
        .expect("edit");
    drop(send(&mut state, ReportMessage::WriteToGame));
    assert_eq!(
        state.session.game().nags(blunder).expect("nags"),
        [nag],
        "the user's mark stands alone"
    );
    let depth = state.session.undo_depth();
    drop(send(&mut state, ReportMessage::WriteToGame));
    assert_eq!(state.session.undo_depth(), depth, "no empty undo point");
    assert!(matches!(
        state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportWritten {
            moves: 0,
            variations: 0
        }))
    ));
}

// ---- cancel does not shrink a report (D1584) ----

#[test]
fn a_cancelled_rerun_keeps_the_complete_report() {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let effects = generate(&mut state);
    drop(drive(&mut state, run, effects, 100, &mut material_engine));
    assert_eq!(state.report_data().map(|d| d.report.moves.len()), Some(20));
    let effects = generate(&mut state);
    let log = drive(&mut state, run, effects, 3, &mut material_engine);
    drop(send(&mut state, ReportMessage::Cancel));
    let request = last_search(&log.last).expect("a search in flight");
    let reply = material_reply(&position_of(&request));
    drop(answer(&mut state, run, &log.last, 900, &reply));
    assert!(!state.annotation_running());
    assert_eq!(state.report_data().map(|d| d.report.moves.len()), Some(20));
    assert!(matches!(
        state.notice,
        Some(AppNotice::Annotate(AnnotateNotice::ReportKept {
            kept: 20,
            ..
        }))
    ));
}

#[test]
fn the_preset_cannot_change_during_a_run() {
    use crate::report::ReportPreset;
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let effects = generate(&mut state);
    let log = drive(&mut state, run, effects, 2, &mut material_engine);
    let started = state.report.preset;
    let other = ReportPreset::ALL
        .iter()
        .copied()
        .find(|p| *p != started)
        .expect("another preset");
    drop(send(&mut state, ReportMessage::SetPreset(other)));
    assert_eq!(state.report.preset, started);
    drop(log);
}

// ---- the animated walk to the moment (M4-11, D1660-D1668) ----

/// The material engine finds one fault only; the report gets a second one
/// (ply 8, a mistake with a known best move) so that the review has two
/// moments to move between.
fn add_second_moment(state: &mut AppState) {
    if let Some(data) = state.report.data.as_mut() {
        let m = &mut data.report.moves[7];
        m.class = MoveClass::Mistake;
        m.best.get_or_insert(m.played);
    }
}

/// A report of the hanging-queen game, the cursor at the end of the game.
fn reported() -> AppState {
    let (mut state, run) = setup(game_of(&HANGING_QUEEN));
    let effects = generate(&mut state);
    drop(drive(&mut state, run, effects, 100, &mut material_engine));
    add_second_moment(&mut state);
    drop(send(
        &mut state,
        crate::session::GameMessage::Navigate(crate::session::Nav::End),
    ));
    state
}

fn start(state: &mut AppState) -> Vec<crate::effect::Effect> {
    send(
        state,
        ReportMessage::StartReview {
            side: crate::report::SideFilter::Both,
            highlights: false,
        },
    )
}

fn ticks(effects: &[crate::effect::Effect]) -> Vec<(std::time::Duration, u64)> {
    effects
        .iter()
        .filter_map(|e| match e {
            crate::effect::Effect::ScheduleReviewTick { after, stamp } => Some((*after, *stamp)),
            _ => None,
        })
        .collect()
}

fn mainline_ply(state: &AppState) -> Option<usize> {
    let cursor = state.session.cursor();
    if cursor == state.session.game().root() {
        return Some(0);
    }
    state
        .session
        .game()
        .mainline()
        .position(|n| n == cursor)
        .map(|i| i + 1)
}

fn target_ply(state: &AppState) -> usize {
    state
        .report
        .review
        .as_ref()
        .and_then(crate::report::Review::current)
        .map_or(0, |m| m.ply - 1)
}

/// Runs the timers until the walk ends; returns the number of ticks.
fn run_walk(state: &mut AppState, mut effects: Vec<crate::effect::Effect>) -> usize {
    let mut count = 0;
    while let Some((_, stamp)) = ticks(&effects).first().copied() {
        count += 1;
        assert!(count < 50, "the walk does not end");
        effects = send(state, ReportMessage::WalkTick { stamp });
    }
    count
}

#[test]
fn starting_a_review_far_away_jumps_then_walks_the_last_twelve_plies() {
    let mut state = reported();
    let effects = start(&mut state);
    let target = target_ply(&state);
    let walk = state.report.walk.expect("a walk");
    assert_eq!(walk.target_ply as usize, target);
    // The jump left the cursor 12 plies from the moment; a tick is due.
    assert_eq!(
        mainline_ply(&state),
        Some(20_usize.saturating_sub(0).min(target + 12))
    );
    let due = ticks(&effects);
    assert_eq!(due.len(), 1);
    assert!(due[0].0.as_millis() * 12 <= 1000);
    let steps = run_walk(&mut state, effects);
    assert!(steps >= 1);
    assert_eq!(mainline_ply(&state), Some(target));
    assert!(state.report.walk.is_none());
}

#[test]
fn a_short_way_steps_at_once_and_ticks_for_the_rest() {
    let mut state = reported();
    drop(send(
        &mut state,
        crate::session::GameMessage::Navigate(crate::session::Nav::GoToMainlinePly(6)),
    ));
    let effects = start(&mut state);
    let target = target_ply(&state);
    assert_eq!(target, 4);
    // 6 -> 4: one step now, one on the tick.
    assert_eq!(mainline_ply(&state), Some(5));
    assert_eq!(ticks(&effects).len(), 1);
    assert_eq!(ticks(&effects)[0].0.as_millis(), 250);
    run_walk(&mut state, effects);
    assert_eq!(mainline_ply(&state), Some(4));
}

#[test]
fn without_animation_the_review_jumps_and_schedules_nothing() {
    let mut state = reported();
    state.prefs.board.animation_ms = 0;
    let effects = start(&mut state);
    assert!(ticks(&effects).is_empty());
    assert!(state.report.walk.is_none());
    assert_eq!(mainline_ply(&state), Some(target_ply(&state)));
}

#[test]
fn a_stale_tick_changes_nothing() {
    let mut state = reported();
    let effects = start(&mut state);
    let (_, stamp) = ticks(&effects)[0];
    let before = state.session.cursor();
    drop(send(
        &mut state,
        ReportMessage::WalkTick {
            stamp: stamp.wrapping_add(7),
        },
    ));
    assert_eq!(state.session.cursor(), before);
    assert!(state.report.walk.is_some());
}

#[test]
fn an_action_of_the_user_finishes_the_walk_on_the_spot() {
    let mut state = reported();
    let effects = start(&mut state);
    let (_, stamp) = ticks(&effects)[0];
    let target = target_ply(&state);
    assert_ne!(mainline_ply(&state), Some(target));
    drop(send(&mut state, ReportMessage::ReviewExit));
    assert!(state.report.walk.is_none());
    assert_eq!(mainline_ply(&state), Some(target));
    // The timer that was left alive finds nothing to do.
    let cursor = state.session.cursor();
    drop(send(&mut state, ReportMessage::WalkTick { stamp }));
    assert_eq!(state.session.cursor(), cursor);
}

#[test]
fn next_during_a_walk_arrives_first_and_then_goes_on() {
    let mut state = reported();
    drop(start(&mut state));
    let more = state.report.review.as_ref().map_or(0, |r| r.moments.len());
    assert_eq!(more, 2, "the fixture has two moments");
    let effects = send(&mut state, ReportMessage::ReviewNext);
    let target = target_ply(&state);
    assert_eq!(target, 7);
    run_walk(&mut state, effects);
    assert_eq!(mainline_ply(&state), Some(target));
}

#[test]
fn leaving_the_review_clearing_or_changing_the_game_drops_the_walk() {
    let mut state = reported();
    drop(start(&mut state));
    assert!(state.report.walk.is_some());
    state.session.replace_game(game_of(&["d2d4"]));
    drop(send(&mut state, ReportMessage::Filter(None)));
    assert!(state.report.walk.is_none() && state.report.review.is_none());

    let mut state = reported();
    drop(start(&mut state));
    drop(send(&mut state, ReportMessage::Clear));
    assert!(state.report.walk.is_none());
}

#[test]
fn the_review_arrow_follows_the_step_the_preference_and_the_walk() {
    let mut state = reported();
    state.prefs.board.animation_ms = 0;
    drop(start(&mut state));
    let ply = target_ply(&state) + 1;
    let best = state.report_data().expect("data").report.moves[ply - 1]
        .best
        .expect("best");
    let arrows = crate::evalbar::review_arrows(&state);
    assert_eq!(arrows.len(), 1);
    assert_eq!(arrows[0].kind, crate::evalbar::ArrowKind::ReviewBest);
    assert_eq!(
        (Some(arrows[0].from), Some(arrows[0].to)),
        (best.from(), best.to())
    );
    // The state keeps it for the board.
    assert_eq!(state.board_arrows(), arrows.as_slice());

    // Try again: hidden (it would give the answer away).
    drop(send(&mut state, ReportMessage::ReviewTry));
    assert!(crate::evalbar::review_arrows(&state).is_empty());
    assert!(state.board_arrows().is_empty());
    // See the best move: shown.
    drop(send(&mut state, ReportMessage::ReviewShowBest));
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(crate::report::ReviewStep::Revealed)
    );
    assert_eq!(crate::evalbar::review_arrows(&state).len(), 1);

    // The preference turns off the arrow of the verdict only.
    state.prefs.report.review_arrow = false;
    drop(send(&mut state, ReportMessage::ReviewNext));
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(crate::report::ReviewStep::Looking)
    );
    assert!(crate::evalbar::review_arrows(&state).is_empty());
    drop(send(&mut state, ReportMessage::ReviewShowBest));
    assert_eq!(crate::evalbar::review_arrows(&state).len(), 1);
}

#[test]
fn the_arrow_is_hidden_while_walking_and_off_the_moment() {
    let mut state = reported();
    drop(start(&mut state));
    assert!(state.report.walk.is_some());
    assert!(crate::evalbar::review_arrows(&state).is_empty());
    drop(send(&mut state, ReportMessage::ReviewExit));
    assert!(
        crate::evalbar::review_arrows(&state).is_empty(),
        "no review, no arrow"
    );
}

#[test]
fn castling_best_moves_are_drawn_on_the_squares_of_the_king() {
    use mychess_core::Position;
    let position = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").expect("fen");
    let castle = position.parse_uci("e1g1").expect("castling");
    assert_eq!(
        (
            castle.from().map(|s| s.to_string()),
            castle.to().map(|s| s.to_string())
        ),
        (Some("e1".into()), Some("g1".into()))
    );
}

#[test]
fn the_report_preference_round_trips_and_defaults_to_on() {
    use crate::prefs::{Preferences, load_preferences, render_preferences};
    let mut prefs = Preferences::default();
    assert!(prefs.report.review_arrow);
    prefs.report.review_arrow = false;
    let loaded = load_preferences(&render_preferences(&prefs));
    assert!(!loaded.prefs.report.review_arrow);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert!(
        load_preferences("[report]\nreview_arrow = 3\n")
            .prefs
            .report
            .review_arrow
    );
}

// ---- review fixes (D1679-D1682) ----

use crate::session::{GameMessage as Gm, PlayOptions as Po};

/// A review started with the walk off, at the first moment.
fn review_here() -> AppState {
    let mut state = reported();
    state.prefs.board.animation_ms = 0;
    drop(start(&mut state));
    state
}

fn moment_best(state: &AppState) -> (mychess_core::Move, mychess_core::Move) {
    let ply = target_ply(state) + 1;
    let m = &state.report_data().expect("data").report.moves[ply - 1];
    (m.played, m.best.expect("best"))
}

fn play(state: &mut AppState, mv: mychess_core::Move) {
    drop(send(
        state,
        Gm::PlayMove {
            mv,
            options: Po::default(),
        },
    ));
}

#[test]
fn the_arrow_returns_when_the_attempt_is_right_and_stays_hidden_when_wrong() {
    let mut state = review_here();
    drop(send(&mut state, ReportMessage::ReviewTry));
    let (played, best) = moment_best(&state);
    assert!(state.board_arrows().is_empty());
    // A wrong attempt keeps the answer hidden.
    play(&mut state, played);
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(crate::report::ReviewStep::Trying)
    );
    assert!(state.board_arrows().is_empty(), "no spoiler");
    // The right one (the update returned early) shows it at once.
    play(&mut state, best);
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(crate::report::ReviewStep::Solved)
    );
    assert_eq!(state.board_arrows().len(), 1);
}

#[test]
fn moving_on_from_solved_or_revealed_goes_back_to_the_verdict_rules() {
    let mut state = review_here();
    drop(send(&mut state, ReportMessage::ReviewTry));
    let (_, best) = moment_best(&state);
    play(&mut state, best);
    drop(send(&mut state, ReportMessage::ReviewNext));
    assert_eq!(
        state.report.review.as_ref().map(|r| r.step),
        Some(crate::report::ReviewStep::Looking)
    );
    assert_eq!(state.board_arrows().len(), 1, "the verdict shows it");
    drop(send(&mut state, ReportMessage::ReviewShowBest));
    drop(send(&mut state, ReportMessage::ReviewPrevious));
    assert_eq!(state.board_arrows().len(), 1);
    state.prefs.report.review_arrow = false;
    drop(send(&mut state, ReportMessage::ReviewNext));
    assert!(state.board_arrows().is_empty(), "the preference is off");
    drop(send(&mut state, ReportMessage::ReviewShowBest));
    assert_eq!(state.board_arrows().len(), 1);
}

#[test]
fn a_dialog_opened_during_the_walk_does_not_leave_the_cursor_halfway() {
    let mut state = reported();
    let effects = start(&mut state);
    let (_, stamp) = ticks(&effects)[0];
    let target = target_ply(&state);
    assert_ne!(mainline_ply(&state), Some(target));
    state.bookmark_editor = Some(crate::bookmarks::BookmarkEditor {
        working: crate::bookmarks::Bookmarks::default(),
        selected: None,
        title: String::new(),
    });
    assert!(state.modal_open());
    drop(send(&mut state, ReportMessage::WalkTick { stamp }));
    assert!(state.report.walk.is_none());
    assert_eq!(mainline_ply(&state), Some(target));
}

#[test]
fn the_walk_goes_on_while_a_game_against_an_engine_is_not_involved() {
    // The steps do not go through the locks of the user's actions.
    let mut state = reported();
    let effects = start(&mut state);
    let (_, stamp) = ticks(&effects)[0];
    let before = mainline_ply(&state);
    drop(send(&mut state, ReportMessage::WalkTick { stamp }));
    assert_ne!(mainline_ply(&state), before);
}

#[test]
fn quit_during_the_walk_ends_on_the_moment() {
    let mut state = reported();
    drop(start(&mut state));
    let target = target_ply(&state);
    drop(update(&mut state, Message::Quit));
    assert!(state.report.walk.is_none());
    assert_eq!(mainline_ply(&state), Some(target));
}

#[test]
fn timers_and_streams_do_not_interrupt_the_walk() {
    let mut state = reported();
    drop(start(&mut state));
    let walk = state.report.walk;
    assert!(walk.is_some());
    let cursor = state.session.cursor();
    let messages = [
        Message::Game(Gm::AutoplayTick { generation: 0 }),
        Message::WindowGeometryChanged(crate::prefs::WindowGeometry {
            width: 800,
            height: 600,
            x: None,
            y: None,
            maximized: false,
        }),
        Message::NagTick(std::time::Instant::now()),
        Message::FlushPreferences,
    ];
    for message in messages {
        drop(update(&mut state, message));
        assert_eq!(state.report.walk, walk);
        assert_eq!(state.session.cursor(), cursor);
    }
}

#[test]
fn walking_back_from_far_jumps_then_animates_twelve_plies() {
    let mut state = reported();
    assert_eq!(mainline_ply(&state), Some(20));
    let effects = start(&mut state);
    // 20 -> 4 is 16 plies: the jump leaves 12 to walk.
    assert_eq!(mainline_ply(&state), Some(16));
    let walk = state.report.walk.expect("walk");
    assert_eq!(walk.direction, crate::report::WalkDirection::Backward);
    assert_eq!(run_walk(&mut state, effects), 12);
    assert_eq!(mainline_ply(&state), Some(4));
}

#[test]
fn a_castling_best_move_is_drawn_on_the_squares_of_the_king() {
    let mut state = review_here();
    let ply = target_ply(&state) + 1;
    let castle = mychess_core::Move::Castle {
        king_from: "e1".parse().expect("sq"),
        rook_from: "h1".parse().expect("sq"),
    };
    if let Some(data) = state.report.data.as_mut() {
        data.report.moves[ply - 1].best = Some(castle);
    }
    let arrows = crate::evalbar::review_arrows(&state);
    assert_eq!(arrows.len(), 1);
    assert_eq!(
        (arrows[0].from.to_string(), arrows[0].to.to_string()),
        ("e1".to_owned(), "g1".to_owned())
    );
}

#[test]
fn a_moment_without_a_best_move_draws_no_arrow() {
    let mut state = review_here();
    let ply = target_ply(&state) + 1;
    assert_eq!(state.board_arrows().len(), 1);
    if let Some(data) = state.report.data.as_mut() {
        data.report.moves[ply - 1].best = None;
    }
    assert!(crate::evalbar::review_arrows(&state).is_empty());
}

#[test]
fn starting_a_review_stops_the_autoplay() {
    let mut state = reported();
    drop(send(&mut state, Gm::StartAutoplay));
    drop(start(&mut state));
    assert!(!state.session.autoplay().running);
}
