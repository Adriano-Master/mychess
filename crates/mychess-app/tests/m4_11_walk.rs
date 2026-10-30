//! M4-11 contract tests: the animated walk of the guided review and the arrow
//! of the best move (spec `docs/specs/m4-11-revisao-animada.md`, sections 2 to
//! 5, D1660-D1668). Time never passes for real: the timers of the walk are
//! delivered by hand and `perform` runs on a paused tokio clock.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::assert_is_empty,
    clippy::type_complexity
)]

mod m4_6_common;

use std::time::Duration;

use m4_6_common::{HANGING_QUEEN, Reply, Rig, game_of, material_engine};
use mychess_app::report::{
    ReportMessage, ReportPrefs, ReviewStep, SideFilter, WALK_MAX_MS, walk_plan,
};
use mychess_app::{
    ArrowKind, Effect, GameMessage, Message, Nav, PlayOptions, PrefChange, PrefsMessage,
    load_preferences, perform, render_preferences, review_arrows,
};
use mychess_core::{Move, Position};
use mychess_engine::SearchRequest;
use proptest::prelude::*;

// ---- walk_plan: the pure plan (D1660-D1662, D1669) ----

proptest! {
    #[test]
    fn a_walk_never_lasts_more_than_a_second_and_covers_the_way(
        from in 0_u32..400,
        to in 0_u32..400,
        animation in 0_u32..=2000,
    ) {
        let plan = walk_plan(from, to, animation);
        let distance = from.abs_diff(to);
        prop_assert!(plan.steps * plan.interval_ms <= WALK_MAX_MS, "{plan:?}");
        prop_assert!(plan.interval_ms <= 250, "{plan:?}");
        if distance == 0 {
            prop_assert_eq!(plan.steps, 0);
            prop_assert_eq!(plan.jump_to, None);
        } else if animation == 0 {
            prop_assert_eq!(plan.steps, 0);
            prop_assert_eq!(plan.jump_to, Some(to));
        } else {
            prop_assert_eq!(plan.steps, distance.min(12));
            // What is left after the jump is exactly the steps to animate.
            let start = plan.jump_to.unwrap_or(from);
            prop_assert_eq!(start.abs_diff(to), plan.steps);
            if distance > 12 {
                prop_assert!(plan.jump_to.is_some());
                // The jump goes the same way as the walk.
                prop_assert_eq!(start < to, from < to);
            } else {
                prop_assert_eq!(plan.jump_to, None);
            }
        }
    }
}

#[test]
fn the_plan_follows_the_numbers_of_the_spec() {
    // (distance, steps, interval ms, jumps)
    for (n, steps, interval, jumps) in [
        (1_u32, 1_u32, 250_u32, false),
        (3, 3, 250, false),
        (4, 4, 250, false),
        (5, 5, 200, false),
        (12, 12, 83, false),
        (13, 12, 83, true),
        (40, 12, 83, true),
    ] {
        for (from, to) in [(0, n), (100, 100 - n)] {
            let plan = walk_plan(from, to, 200);
            assert_eq!(plan.steps, steps, "n = {n}");
            assert_eq!(plan.interval_ms, interval, "n = {n}");
            assert_eq!(plan.jump_to.is_some(), jumps, "n = {n}");
        }
    }
    let far = walk_plan(0, 40, 200);
    assert_eq!(far.jump_to, Some(28), "jumps to alvo - 12");
    let back = walk_plan(60, 20, 200);
    assert_eq!(back.jump_to, Some(32), "jumps to alvo + 12 going back");
}

#[test]
fn with_animations_off_the_plan_is_a_plain_jump() {
    let plan = walk_plan(3, 9, 0);
    assert_eq!((plan.jump_to, plan.steps), (Some(9), 0));
    let plan = walk_plan(9, 3, 0);
    assert_eq!((plan.jump_to, plan.steps), (Some(3), 0));
}

// ---- helpers ----

fn reported() -> Rig {
    let mut rig = Rig::new(game_of(&HANGING_QUEEN));
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut material_engine);
    assert!(rig.state.report_data().is_some(), "the report");
    rig.send(GameMessage::Navigate(Nav::End));
    rig
}

fn start(rig: &mut Rig) -> Vec<Effect> {
    rig.send(ReportMessage::StartReview {
        side: SideFilter::Both,
        highlights: false,
    })
}

fn ticks(effects: &[Effect]) -> Vec<(Duration, u64)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::ScheduleReviewTick { after, stamp } => Some((*after, *stamp)),
            _ => None,
        })
        .collect()
}

fn ply_of_cursor(rig: &Rig) -> usize {
    let cursor = rig.state.session.cursor();
    if cursor == rig.game().root() {
        return 0;
    }
    rig.game()
        .mainline()
        .position(|n| n == cursor)
        .map(|i| i + 1)
        .expect("the cursor is on the main line")
}

/// The ply before the move of the moment on show (where the walk ends).
fn target(rig: &Rig) -> usize {
    rig.state
        .report
        .review
        .as_ref()
        .and_then(mychess_app::report::Review::current)
        .expect("a moment")
        .ply
        - 1
}

fn step(rig: &Rig) -> ReviewStep {
    rig.state.report.review.as_ref().expect("a review").step
}

struct Trace {
    /// The cursor ply after the start and after each tick.
    plies: Vec<usize>,
    /// The waits asked for, in order.
    waits: Vec<Duration>,
    /// Whether the arrows of the board were empty at every point before the end.
    arrows_empty_before_end: bool,
}

/// Delivers the timers one by one until the walk ends.
fn run(rig: &mut Rig, mut effects: Vec<Effect>) -> Trace {
    let mut trace = Trace {
        plies: vec![ply_of_cursor(rig)],
        waits: ticks(&effects).iter().map(|t| t.0).collect(),
        arrows_empty_before_end: rig.state.board_arrows().is_empty() || !rig.state.review_walking(),
    };
    let mut count = 0;
    while let Some((_, stamp)) = ticks(&effects).first().copied() {
        count += 1;
        assert!(count < 40, "the walk does not end");
        assert!(rig.state.review_walking(), "a tick is due: still walking");
        assert!(
            rig.state.board_arrows().is_empty(),
            "no arrow while walking"
        );
        effects = rig.send(ReportMessage::WalkTick { stamp });
        trace.plies.push(ply_of_cursor(rig));
        trace.waits.extend(ticks(&effects).iter().map(|t| t.0));
        if rig.state.review_walking() {
            trace.arrows_empty_before_end &= rig.state.board_arrows().is_empty();
        }
    }
    trace
}

fn total_ms(trace: &Trace) -> u128 {
    trace.waits.iter().map(Duration::as_millis).sum()
}

// ---- the walk (section 2) ----

#[test]
fn a_short_way_forward_goes_one_ply_at_a_time_within_a_second() {
    let mut rig = reported();
    rig.send(GameMessage::Navigate(Nav::Start));
    let fx = start(&mut rig);
    let end = target(&rig);
    assert_eq!(end, 4);
    let trace = run(&mut rig, fx);
    assert_eq!(ply_of_cursor(&rig), end);
    assert!(!rig.state.review_walking());
    for pair in trace.plies.windows(2) {
        assert_eq!(
            pair[1],
            pair[0] + 1,
            "one move at a time: {:?}",
            trace.plies
        );
    }
    assert_eq!(*trace.plies.last().unwrap(), end);
    assert!(total_ms(&trace) <= 1000, "{:?}", trace.waits);
    assert!(trace.arrows_empty_before_end);
}

#[test]
fn a_short_way_backward_rewinds_one_ply_at_a_time_within_a_second() {
    let mut rig = reported();
    rig.send(GameMessage::Navigate(Nav::GoToMainlinePly(9)));
    let fx = start(&mut rig);
    let end = target(&rig);
    let trace = run(&mut rig, fx);
    assert_eq!(ply_of_cursor(&rig), end);
    for pair in trace.plies.windows(2) {
        assert_eq!(pair[1] + 1, pair[0], "one move back at a time");
    }
    assert!(total_ms(&trace) <= 1000, "{:?}", trace.waits);
}

#[test]
fn a_long_way_jumps_and_animates_only_the_last_twelve_plies() {
    // The game is too short for 40, but 20 -> 4 is already more than 12.
    let mut rig = reported();
    let fx = start(&mut rig);
    let end = target(&rig);
    assert_eq!(end, 4);
    let trace = run(&mut rig, fx);
    let first = trace.plies[0];
    assert_eq!(first, end + 12, "the jump leaves the cursor 12 plies away");
    let moves = trace.plies.len() - 1;
    assert!(moves <= 12, "at most 12 animated plies: {:?}", trace.plies);
    assert_eq!(ply_of_cursor(&rig), end);
    assert!(total_ms(&trace) <= 1000, "{:?}", trace.waits);
}

#[test]
fn without_animation_the_walk_is_a_jump_with_no_timer_in_any_direction() {
    let mut rig = reported();
    rig.state.prefs.board.animation_ms = 0;
    let fx = start(&mut rig);
    assert!(ticks(&fx).is_empty(), "{fx:?}");
    assert!(!rig.state.review_walking());
    assert_eq!(ply_of_cursor(&rig), target(&rig));
    for message in [
        ReportMessage::ReviewNext,
        ReportMessage::ReviewPrevious,
        ReportMessage::GoTo(2),
        ReportMessage::GoTo(18),
    ] {
        let fx = rig.send(message);
        assert!(ticks(&fx).is_empty(), "{message:?}: {fx:?}");
        assert!(!rig.state.review_walking(), "{message:?}");
    }
}

#[test]
fn a_tick_of_another_walk_is_ignored() {
    let mut rig = reported();
    let fx = start(&mut rig);
    let (_, stamp) = ticks(&fx)[0];
    let before = rig.state.session.cursor();
    for wrong in [stamp.wrapping_add(1), stamp.wrapping_sub(1), 0, u64::MAX] {
        if wrong == stamp {
            continue;
        }
        let fx = rig.send(ReportMessage::WalkTick { stamp: wrong });
        assert!(ticks(&fx).is_empty(), "a stale tick schedules nothing");
        assert_eq!(rig.state.session.cursor(), before);
        assert!(rig.state.review_walking());
    }
    // The right one still works afterwards.
    rig.send(ReportMessage::WalkTick { stamp });
    assert_ne!(rig.state.session.cursor(), before);
}

#[test]
fn a_tick_with_no_walk_in_progress_changes_nothing() {
    let mut rig = reported();
    let before = rig.state.session.cursor();
    let fx = rig.send(ReportMessage::WalkTick { stamp: 1 });
    assert!(fx.is_empty(), "{fx:?}");
    assert_eq!(rig.state.session.cursor(), before);
}

#[test]
fn a_new_destination_makes_the_old_timer_stale() {
    let mut rig = reported();
    let fx = start(&mut rig);
    let (_, old) = ticks(&fx)[0];
    // The way to the next moment replaces the walk.
    let fx = rig.send(ReportMessage::ReviewNext);
    let end = target(&rig);
    let cursor = rig.state.session.cursor();
    rig.send(ReportMessage::WalkTick { stamp: old });
    // Either the old stamp is dead (cursor kept) or the walk was already at the
    // target; in both cases the cursor never leaves the way to the target.
    let ply = ply_of_cursor(&rig);
    assert_eq!(
        rig.state.session.cursor(),
        cursor,
        "a stale timer moved the cursor to {ply}, target {end}"
    );
    run(&mut rig, fx);
    assert_eq!(ply_of_cursor(&rig), end);
}

#[test]
fn a_user_action_in_the_middle_ends_the_walk_on_the_target_and_then_runs() {
    let mut rig = reported();
    start(&mut rig);
    let end = target(&rig);
    assert_ne!(ply_of_cursor(&rig), end, "in the middle of the walk");
    // The user steps to the previous ply: the walk finishes first (on the
    // target) and the step is taken from there.
    rig.send(GameMessage::Navigate(Nav::Back(1)));
    assert!(!rig.state.review_walking());
    assert_eq!(ply_of_cursor(&rig), end - 1);
}

#[test]
fn leaving_the_review_in_the_middle_of_a_walk_never_leaves_the_board_in_between() {
    let mut rig = reported();
    let fx = start(&mut rig);
    let (_, stamp) = ticks(&fx)[0];
    let end = target(&rig);
    assert_ne!(ply_of_cursor(&rig), end);
    rig.send(ReportMessage::ReviewExit);
    assert!(!rig.state.review_walking());
    assert!(rig.state.report.review.is_none());
    let at = ply_of_cursor(&rig);
    assert!(
        at == end,
        "the walk ends on the target before leaving: {at} vs {end}"
    );
    // The timer that was left alive changes nothing.
    let fx = rig.send(ReportMessage::WalkTick { stamp });
    assert!(ticks(&fx).is_empty());
    assert_eq!(ply_of_cursor(&rig), end);
}

#[test]
fn clearing_the_report_drops_the_walk_and_its_timer_is_harmless() {
    let mut rig = reported();
    let fx = start(&mut rig);
    let (_, stamp) = ticks(&fx)[0];
    rig.send(ReportMessage::Clear);
    assert!(!rig.state.review_walking());
    assert!(rig.state.report.review.is_none());
    let cursor = rig.state.session.cursor();
    rig.send(ReportMessage::WalkTick { stamp });
    assert_eq!(rig.state.session.cursor(), cursor, "no live tick moves it");
}

#[test]
fn changing_the_game_drops_the_walk_and_its_timer_is_harmless() {
    let mut rig = reported();
    let fx = start(&mut rig);
    let (_, stamp) = ticks(&fx)[0];
    rig.state.session.replace_game(game_of(&["d2d4", "d7d5"]));
    // Any message lets the state notice (the watchdog runs at the end of update).
    rig.send(ReportMessage::Filter(None));
    assert!(!rig.state.review_walking());
    assert!(rig.state.report.review.is_none());
    let cursor = rig.state.session.cursor();
    let fx = rig.send(ReportMessage::WalkTick { stamp });
    assert!(ticks(&fx).is_empty(), "{fx:?}");
    assert_eq!(rig.state.session.cursor(), cursor);
}

#[test]
fn quitting_during_a_walk_asks_for_nothing_that_delays_the_exit() {
    let mut rig = reported();
    start(&mut rig);
    // The only pending effect is a timer that the host may drop: nothing
    // else is waiting on the walk.
    assert!(rig.state.review_walking());
    let fx = rig.send(ReportMessage::ReviewExit);
    assert!(
        fx.iter()
            .all(|e| !matches!(e, Effect::ScheduleReviewTick { .. })),
        "no new timer after leaving: {fx:?}"
    );
}

// ---- perform of the timer ----

#[tokio::test(start_paused = true)]
async fn perform_sleeps_the_requested_time_and_answers_the_walk_tick() {
    let started = tokio::time::Instant::now();
    let message = perform(Effect::ScheduleReviewTick {
        after: Duration::from_millis(250),
        stamp: 77,
    })
    .await;
    assert!(
        matches!(
            message,
            Message::Report(ReportMessage::WalkTick { stamp: 77 })
        ),
        "{message:?}"
    );
    assert_eq!(started.elapsed(), Duration::from_millis(250));
}

#[tokio::test(start_paused = true)]
async fn perform_does_not_answer_before_the_wait_is_over() {
    let work = perform(Effect::ScheduleReviewTick {
        after: Duration::from_millis(100),
        stamp: 1,
    });
    let early = tokio::time::timeout(Duration::from_millis(99), work).await;
    assert!(early.is_err(), "the tick came early");
}

// ---- the arrow of the best move (section 3) ----

fn arrow_squares(rig: &Rig) -> Vec<(String, String, ArrowKind)> {
    rig.state
        .board_arrows()
        .iter()
        .map(|a| (a.from.to_string(), a.to.to_string(), a.kind))
        .collect()
}

fn best_squares(rig: &Rig, ply: usize) -> (String, String) {
    let best = rig.state.report_data().unwrap().report.moves[ply - 1]
        .best
        .expect("best");
    (
        best.from().unwrap().to_string(),
        best.to().unwrap().to_string(),
    )
}

fn at_moment_without_walk() -> Rig {
    let mut rig = reported();
    rig.state.prefs.board.animation_ms = 0;
    start(&mut rig);
    rig
}

#[test]
fn the_verdict_shows_the_arrow_of_the_best_move() {
    let rig = at_moment_without_walk();
    assert_eq!(step(&rig), ReviewStep::Looking);
    let (from, to) = best_squares(&rig, target(&rig) + 1);
    assert_eq!(arrow_squares(&rig), [(from, to, ArrowKind::ReviewBest)]);
    assert_eq!(review_arrows(&rig.state).len(), 1);
}

#[test]
fn try_again_hides_the_arrow_and_see_the_best_move_brings_it_back() {
    let mut rig = at_moment_without_walk();
    rig.send(ReportMessage::ReviewTry);
    assert_eq!(step(&rig), ReviewStep::Trying);
    assert!(
        rig.state.board_arrows().is_empty(),
        "it would be the answer"
    );
    rig.send(ReportMessage::ReviewShowBest);
    assert_eq!(step(&rig), ReviewStep::Revealed);
    assert_eq!(rig.state.board_arrows().len(), 1);
    assert_eq!(rig.state.board_arrows()[0].kind, ArrowKind::ReviewBest);
}

#[test]
fn the_arrow_is_shown_again_after_the_user_finds_the_best_move() {
    let mut rig = at_moment_without_walk();
    let ply = target(&rig) + 1;
    let best = rig.state.report_data().unwrap().report.moves[ply - 1]
        .best
        .unwrap();
    rig.send(ReportMessage::ReviewTry);
    rig.send(GameMessage::PlayMove {
        mv: best,
        options: PlayOptions::default(),
    });
    assert_eq!(step(&rig), ReviewStep::Solved);
    assert_eq!(
        rig.state.board_arrows().len(),
        1,
        "the arrow is back once the attempt is right"
    );
}

#[test]
fn a_report_without_best_moves_draws_no_arrow() {
    let mut state = mychess_app::AppState::default();
    let mut game = game_of(&HANGING_QUEEN[..6]);
    let nodes: Vec<_> = game.mainline().collect();
    for (node, text) in nodes
        .iter()
        .zip(["+0.3", "+0.3", "+0.2", "+0.3", "-6.0", "-6.0"])
    {
        game.set_comment(*node, &format!("[%eval {text}]")).unwrap();
    }
    state.session.replace_game(game);
    state.prefs.board.animation_ms = 0;
    mychess_app::update(&mut state, Message::Report(ReportMessage::FromGame));
    mychess_app::update(
        &mut state,
        Message::Report(ReportMessage::StartReview {
            side: SideFilter::Both,
            highlights: false,
        }),
    );
    assert!(state.report.review.is_some(), "a review");
    assert!(state.board_arrows().is_empty());
    mychess_app::update(&mut state, Message::Report(ReportMessage::ReviewShowBest));
    assert!(state.board_arrows().is_empty(), "nothing to show either");
}

#[test]
fn the_preference_turns_off_only_the_arrow_of_the_verdict() {
    let mut rig = at_moment_without_walk();
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::Report(
        ReportPrefs {
            review_arrow: false,
        },
    ))));
    assert!(!rig.state.prefs.report.review_arrow);
    assert!(
        rig.state.board_arrows().is_empty(),
        "verdict, preference off"
    );
    rig.send(ReportMessage::ReviewShowBest);
    assert_eq!(
        rig.state.board_arrows().len(),
        1,
        "see the best move shows it"
    );
    // Turning it back on brings it to the verdict of the next moment.
    rig.send(Message::Prefs(PrefsMessage::Set(PrefChange::Report(
        ReportPrefs { review_arrow: true },
    ))));
    rig.send(ReportMessage::ReviewNext);
    if step(&rig) == ReviewStep::Looking {
        assert_eq!(rig.state.board_arrows().len(), 1);
    }
}

#[test]
fn the_review_arrow_preference_defaults_on_and_round_trips_through_toml() {
    let prefs = mychess_app::Preferences::default();
    assert!(prefs.report.review_arrow);
    for value in [true, false] {
        let mut prefs = mychess_app::Preferences::default();
        prefs.report.review_arrow = value;
        let text = render_preferences(&prefs);
        assert!(text.contains("[report]"), "{text}");
        let loaded = load_preferences(&text);
        assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
        assert_eq!(loaded.prefs.report.review_arrow, value);
    }
    // A file without the table keeps the default.
    assert!(load_preferences("").prefs.report.review_arrow);
}

/// A game where White declines to castle at ply 11 and the engine says
/// that `e1g1` was best.
fn castling_engine(_: &SearchRequest, pos: &Position) -> Reply {
    let fen = pos.fen();
    let rank3 = fen
        .split(' ')
        .next()
        .unwrap()
        .split('/')
        .nth(5)
        .unwrap()
        .to_owned();
    let black_to_move = fen.split(' ').nth(1) == Some("b");
    if black_to_move && rank3.ends_with('P') {
        // After 11.h3: White lost the thread.
        let reply = pos.legal_moves()[0];
        return Reply::cp(-600, vec![reply]);
    }
    let castle: Option<Move> = pos.parse_uci("e1g1").ok().filter(|_| !black_to_move);
    match castle {
        Some(mv) => Reply::cp(0, vec![mv]),
        None => Reply::cp(0, vec![pos.legal_moves()[0]]),
    }
}

#[test]
fn a_castling_best_move_is_drawn_on_the_squares_of_the_king() {
    let ucis = [
        "e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6", "d2d3", "f8c5", "a2a3", "d7d6", "h2h3",
    ];
    let mut rig = Rig::new(game_of(&ucis));
    let fx = rig.send(ReportMessage::Generate);
    rig.drive(fx, 5000, &mut castling_engine);
    assert!(rig.state.report_data().is_some());
    rig.state.prefs.board.animation_ms = 0;
    rig.send(ReportMessage::StartReview {
        side: SideFilter::White,
        highlights: false,
    });
    assert_eq!(target(&rig) + 1, 11, "the moment is 11.h3");
    let best = rig.state.report_data().unwrap().report.moves[10]
        .best
        .unwrap();
    assert_eq!(best.from().map(|s| s.to_string()), Some("e1".to_owned()));
    assert_eq!(
        arrow_squares(&rig),
        [("e1".to_owned(), "g1".to_owned(), ArrowKind::ReviewBest)]
    );
}

#[test]
fn the_arrow_is_not_there_while_the_walk_is_on_and_appears_when_it_ends() {
    let mut rig = reported();
    let fx = start(&mut rig);
    assert!(rig.state.review_walking());
    assert!(rig.state.board_arrows().is_empty());
    let trace = run(&mut rig, fx);
    assert!(trace.arrows_empty_before_end);
    assert_eq!(rig.state.board_arrows().len(), 1, "it appears at the end");
}

#[test]
fn the_review_arrow_is_only_on_the_position_of_the_moment() {
    let mut rig = at_moment_without_walk();
    assert_eq!(rig.state.board_arrows().len(), 1);
    rig.send(GameMessage::Navigate(Nav::Forward(1)));
    assert!(
        rig.state.board_arrows().is_empty(),
        "off the position of the moment the arrow would point at the wrong board"
    );
}

#[test]
fn leaving_the_review_removes_its_arrow() {
    let mut rig = at_moment_without_walk();
    rig.send(ReportMessage::ReviewExit);
    assert!(rig.state.board_arrows().is_empty());
}

#[test]
fn live_engine_arrows_are_suppressed_while_the_review_is_open() {
    let mut rig = reported();
    rig.state.prefs.board.animation_ms = 0;
    rig.state.prefs.engine_view.arrows.enabled = true;
    start(&mut rig);
    rig.send(ReportMessage::ReviewTry);
    assert!(
        rig.state.engine_arrows().is_empty(),
        "no live arrow of the engine under the review"
    );
    assert!(
        rig.state.board_arrows().is_empty(),
        "Try again: nothing on the board (the engine would give the answer away)"
    );
    rig.send(ReportMessage::ReviewShowBest);
    assert!(
        rig.state
            .board_arrows()
            .iter()
            .all(|a| a.kind == ArrowKind::ReviewBest),
        "only arrows of the review"
    );
}
