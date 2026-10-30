//! M8-7 contract tests of the tactical game in the state (spec
//! `docs/specs/m8-7-jogo-tatico.md`, D2450-D2468): the strength of the
//! engine follows what it declares, the game is recorded as a tactical one,
//! the score and the Elo follow the result, and the start positions work.
//! The engine is the in-memory one of `m8_1_common`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m8_1_common;
mod m8_7_common;

use std::time::{Duration, Instant};

use m8_1_common::{Rig, play_search, set_options, spin};
use m8_7_common::*;
use mychess_app::tactical_game::{StrengthPlan, TacticalStats};
use mychess_app::{
    AppNotice, ColorChoice, Effect, Message, PlayEnd, PlayMessage, PlayNotice, PlayPhase,
    StartFrom, TimeMode, update,
};
use mychess_core::{GameResult, Position};
use mychess_engine::OptionValue;

fn extra(rig: &Rig, key: &str) -> Option<String> {
    rig.state
        .session
        .game()
        .tags()
        .get_extra(key)
        .map(str::to_owned)
}

fn event(rig: &Rig) -> String {
    rig.state.session.game().tags().event.clone()
}

fn index_of(effects: &[Effect], name: &str) -> Option<usize> {
    m8_1_common::commands(effects).iter().position(
        |c| matches!(c, mychess_engine::EngineCommand::SetOption { name: n, .. } if n == name),
    )
}

// ---- the plan of the strength (D2452-D2455) ----

#[test]
fn an_engine_with_uci_elo_is_limited_before_the_first_search() {
    let mut rig = Rig::with_options(full_options());
    let config = tactical(&rig, ColorChoice::Black, 1500);
    let effects = rig.start(config);
    let options = set_options(&effects);
    assert!(options.contains(&("UCI_LimitStrength".to_owned(), OptionValue::Bool(true))));
    assert!(options.contains(&("UCI_Elo".to_owned(), OptionValue::Int(1500))));
    let search = m8_1_common::commands(&effects)
        .iter()
        .position(|c| matches!(c, mychess_engine::EngineCommand::Search(_)))
        .expect("the engine plays first");
    assert!(index_of(&effects, "UCI_LimitStrength").unwrap() < search);
    assert!(index_of(&effects, "UCI_Elo").unwrap() < search);
    let view = rig.state.play_tactical().expect("a tactical game");
    assert_eq!(view.elo, 1500);
    assert_eq!(
        view.plan,
        Some(StrengthPlan::Elo {
            elo: 1500,
            clamped: false
        })
    );
}

#[test]
fn the_game_is_recorded_as_tactical_with_the_elo_of_the_engine() {
    let mut rig = Rig::with_options(full_options());
    let config = tactical(&rig, ColorChoice::Black, 1500);
    rig.start(config);
    let game = rig.state.session.game();
    let tags = game.tags();
    assert_eq!(tags.event, "Tactical game");
    // The engine has White: the player chose Black.
    assert_eq!(tags.white_rating.map(|r| r.value.get()), Some(1500));
    assert_eq!(tags.black_rating, None);
    assert_eq!(extra(&rig, "TacticalElo").as_deref(), Some("1500"));
    assert_eq!(
        extra(&rig, "TacticalLimit").as_deref(),
        Some("UCI_Elo 1500")
    );
    assert_eq!(extra(&rig, "UCI_Elo").as_deref(), Some("1500"));
    assert_eq!(extra(&rig, "Timemode").as_deref(), Some("Movetime 3"));
}

#[test]
fn an_elo_outside_the_declared_range_is_moved_in_and_reported() {
    let mut rig = Rig::with_options(full_options());
    let config = tactical(&rig, ColorChoice::Black, 900);
    let effects = rig.start(config);
    assert!(set_options(&effects).contains(&("UCI_Elo".to_owned(), OptionValue::Int(1320))));
    assert_eq!(rig.notice(), Some(PlayNotice::TacticalEloClamped));
    assert_eq!(extra(&rig, "TacticalElo").as_deref(), Some("900"));
    assert_eq!(extra(&rig, "UCI_Elo").as_deref(), Some("1320"));
    assert_eq!(
        rig.state
            .session
            .game()
            .tags()
            .white_rating
            .map(|r| r.value.get()),
        Some(1320),
        "the tag carries the Elo the engine really got"
    );
}

#[test]
fn without_uci_elo_the_skill_level_is_used() {
    let mut rig = Rig::with_options(skill_only());
    let config = tactical(&rig, ColorChoice::Black, 1800);
    let effects = rig.start(config);
    let options = set_options(&effects);
    assert!(options.contains(&("Skill Level".to_owned(), OptionValue::Int(10))));
    assert!(!options.iter().any(|(n, _)| n.starts_with("UCI_")));
    assert_eq!(extra(&rig, "SkillLevel").as_deref(), Some("10"));
    assert_eq!(
        extra(&rig, "TacticalLimit").as_deref(),
        Some("Skill Level 10")
    );
    // Not an Elo the engine took: the tag keeps the Elo that was asked for.
    assert_eq!(
        rig.state
            .session
            .game()
            .tags()
            .white_rating
            .map(|r| r.value.get()),
        Some(1800)
    );
    assert_eq!(rig.notice(), None);
}

#[test]
fn with_no_option_of_strength_every_search_is_cut_at_a_depth_and_the_user_is_told() {
    let mut rig = Rig::with_options(vec![spin("Hash", 1, 1024)]);
    let config = tactical(&rig, ColorChoice::Black, 1500);
    let effects = rig.start(config);
    let options = set_options(&effects);
    assert!(
        !options
            .iter()
            .any(|(n, _)| n.starts_with("UCI_") || n == "Skill Level"),
        "{options:?}"
    );
    let search = play_search(&effects).expect("a search");
    assert_eq!(search.limits.depth, Some(5), "the cap of 1500");
    assert_eq!(
        search.limits.movetime,
        Some(Duration::from_secs(3)),
        "next to the limit of the mode"
    );
    assert_eq!(rig.notice(), Some(PlayNotice::TacticalDepthLimited));
    assert_eq!(extra(&rig, "TacticalLimit").as_deref(), Some("Depth 5"));
    assert_eq!(extra(&rig, "UCI_Elo"), None);
}

#[test]
fn the_cap_follows_the_elo_and_never_raises_a_lower_limit_of_the_mode() {
    let mut rig = Rig::with_options(Vec::new());
    let mut config = tactical(&rig, ColorChoice::Black, 2300);
    config.mode = TimeMode::Depth(3);
    let effects = rig.start(config);
    assert_eq!(
        play_search(&effects).expect("search").limits.depth,
        Some(3),
        "the mode asked for less than the cap of 2300 (12)"
    );
}

#[test]
fn an_ordinary_game_is_not_touched() {
    let mut rig = Rig::with_options(Vec::new());
    let config = rig.config(ColorChoice::Black, TimeMode::MoveTime(3));
    let effects = rig.start(config);
    assert_eq!(play_search(&effects).expect("search").limits.depth, None);
    assert_eq!(event(&rig), "UCI Game");
    assert_eq!(rig.state.play_tactical(), None);
    assert_eq!(extra(&rig, "TacticalElo"), None);
}

#[test]
fn the_engine_goes_back_to_its_options_when_the_game_is_left() {
    let mut rig = Rig::with_options(full_options());
    let config = tactical(&rig, ColorChoice::Black, 1500);
    rig.start(config);
    let effects = rig.send(PlayMessage::Quit(Instant::now()));
    let options = set_options(&effects);
    assert!(
        options.iter().any(|(n, _)| n == "UCI_LimitStrength"),
        "{options:?}"
    );
    assert!(options.iter().any(|(n, _)| n == "UCI_Elo"), "{options:?}");
    assert!(!rig.state.play_live());
}

// ---- the score and the Elo (D2456, D2463) ----

fn mate_in_one(rig: &Rig, step: Option<u32>) -> mychess_app::PlayConfig {
    let config = from_fen(tactical(rig, ColorChoice::White, 1500), MATE_IN_ONE);
    match step {
        Some(step) => with_adjust(config, step),
        None => config,
    }
}

fn win(rig: &mut Rig) {
    rig.player_move("d1", "d8", Instant::now());
    assert!(
        matches!(rig.phase(), PlayPhase::Ended(PlayEnd::Checkmate { .. })),
        "{:?}",
        rig.phase()
    );
}

#[test]
fn a_win_is_counted_and_the_elo_goes_up_when_the_adjustment_is_on() {
    let mut rig = Rig::with_options(full_options());
    let config = mate_in_one(&rig, Some(50));
    rig.start(config);
    win(&mut rig);
    let stats = rig.state.prefs.play.tactical.stats;
    assert_eq!((stats.games, stats.wins, stats.streak), (1, 1, 1));
    assert_eq!(stats.best_win_elo, 1500);
    assert_eq!(rig.state.prefs.play.tactical.elo, 1550);
    let view = rig.state.play_tactical().expect("still a tactical game");
    assert_eq!(view.elo, 1500, "the Elo this game was played at");
    assert_eq!(view.adjusted, Some((1500, 1550)));
    assert_eq!(rig.state.session.game().tags().result, GameResult::White);
}

#[test]
fn without_the_adjustment_the_elo_stays_and_the_score_still_counts() {
    let mut rig = Rig::with_options(full_options());
    let config = mate_in_one(&rig, None);
    rig.start(config);
    win(&mut rig);
    assert_eq!(rig.state.prefs.play.tactical.stats.wins, 1);
    assert_eq!(rig.state.prefs.play.tactical.elo, 1500);
    assert_eq!(rig.state.play_tactical().unwrap().adjusted, None);
}

#[test]
fn resigning_is_a_loss_and_the_elo_goes_down() {
    let mut rig = Rig::with_options(full_options());
    let config = with_adjust(tactical(&rig, ColorChoice::White, 1500), 100);
    rig.start(config);
    rig.send(PlayMessage::Resign(Instant::now()));
    let stats = rig.state.prefs.play.tactical.stats;
    assert_eq!((stats.games, stats.losses, stats.streak), (1, 1, 0));
    assert_eq!(rig.state.prefs.play.tactical.elo, 1400);
}

#[test]
fn a_draw_counts_and_leaves_the_elo_alone() {
    let mut rig = Rig::with_options(full_options());
    let config = from_fen(
        with_adjust(tactical(&rig, ColorChoice::White, 1500), 50),
        STALEMATE_IN_ONE,
    );
    rig.start(config);
    rig.player_move("c1", "c7", Instant::now());
    assert_eq!(rig.view().result, GameResult::Draw);
    let stats = rig.state.prefs.play.tactical.stats;
    assert_eq!((stats.games, stats.draws), (1, 1));
    assert_eq!(rig.state.prefs.play.tactical.elo, 1500);
}

#[test]
fn leaving_a_game_does_not_count() {
    let mut rig = Rig::with_options(full_options());
    let config = with_adjust(tactical(&rig, ColorChoice::White, 1500), 50);
    rig.start(config);
    rig.send(PlayMessage::Quit(Instant::now()));
    assert_eq!(
        rig.state.prefs.play.tactical.stats,
        TacticalStats::default()
    );
    assert_eq!(rig.state.prefs.play.tactical.elo, 1500);
}

#[test]
fn the_engine_failing_does_not_count() {
    let mut rig = Rig::with_options(full_options());
    let config = with_adjust(tactical(&rig, ColorChoice::Black, 1500), 50);
    let effects = rig.start(config);
    // The engine answers with no move: the game stops without a result.
    rig.reply(&effects, None);
    assert!(matches!(
        rig.phase(),
        PlayPhase::Ended(PlayEnd::OpponentFailure)
    ));
    assert_eq!(
        rig.state.prefs.play.tactical.stats,
        TacticalStats::default()
    );
}

#[test]
fn restart_uses_the_elo_the_finished_game_adjusted() {
    let mut rig = Rig::with_options(full_options());
    let config = mate_in_one(&rig, Some(50));
    rig.start(config);
    win(&mut rig);
    rig.send(PlayMessage::Restart(Instant::now()));
    assert_eq!(rig.view().phase, PlayPhase::Player, "a new game");
    assert_eq!(extra(&rig, "TacticalElo").as_deref(), Some("1550"));
    assert_eq!(
        rig.state.play_tactical().map(|v| v.elo),
        Some(1550),
        "and the same position"
    );
    assert_eq!(
        rig.state.session.game().start_position().fen(),
        Position::from_fen(MATE_IN_ONE).unwrap().fen()
    );
}

// ---- the start positions (D2457-D2460) ----

#[test]
fn a_known_position_is_the_start_of_the_game() {
    let mut rig = Rig::with_options(full_options());
    let config = from_fen(tactical(&rig, ColorChoice::White, 1500), MATE_IN_ONE);
    rig.start(config);
    assert_eq!(
        rig.state.session.game().start_position().fen(),
        Position::from_fen(MATE_IN_ONE).unwrap().fen()
    );
    assert_eq!(rig.view().phase, PlayPhase::Player);
}

#[test]
fn a_position_with_nothing_to_play_is_refused() {
    let mut rig = Rig::with_options(full_options());
    let config = from_fen(
        tactical(&rig, ColorChoice::White, 1500),
        "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1",
    );
    rig.start(config);
    assert!(!rig.state.play_live());
    assert_eq!(rig.notice(), Some(PlayNotice::TacticalBadPosition));
}

#[test]
fn a_position_is_drawn_from_a_file_and_the_game_starts_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let runner = Runner::new();
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    let effects = rig.start(config);
    assert!(rig.state.play_live(), "the drawing holds the board");
    assert!(rig.state.play_picking());
    assert!(rig.state.play_wants_ticks(), "the watchdog needs ticks");
    assert!(effects.iter().any(|e| matches!(e, Effect::PlayPick { .. })));
    let rest = runner.settle(&mut rig, effects);
    assert!(!rig.state.play_picking());
    // Either side may be to move in the position drawn.
    assert!(
        matches!(rig.view().phase, PlayPhase::Player | PlayPhase::Opponent),
        "{rest:?}"
    );
    let start = rig.state.session.game().start_position().clone();
    assert_ne!(start, Position::new());
    assert!(!start.legal_moves().is_empty(), "a position to play from");
    assert_eq!(event(&rig), "Tactical game");
}

#[test]
fn restart_repeats_the_position_that_was_drawn() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let runner = Runner::new();
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    let effects = rig.start(config);
    runner.settle(&mut rig, effects);
    let first = rig.state.session.game().start_position().fen();
    let effects = rig.send(PlayMessage::Restart(Instant::now()));
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::PlayPick { .. })),
        "no new drawing"
    );
    assert_eq!(rig.state.session.game().start_position().fen(), first);
}

#[test]
fn an_answer_for_another_request_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    let effects = rig.start(config);
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::PlayPick { request, .. } => Some(*request),
            _ => None,
        })
        .expect("a drawing");
    rig.send(PlayMessage::Picked {
        request: request + 7,
        result: Ok(mychess_app::tactical_game::Picked {
            position: Position::from_fen(MATE_IN_ONE).unwrap(),
            label: "old".to_owned(),
        }),
    });
    assert!(rig.state.play_picking(), "still waiting for its own answer");
    assert!(rig.state.play_phase().is_none());
}

#[test]
fn quitting_cancels_the_drawing_and_its_late_answer_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let runner = Runner::new();
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    let effects = rig.start(config);
    let job = effects
        .iter()
        .find_map(|e| match e {
            Effect::PlayPick { job, .. } => Some(job.clone()),
            _ => None,
        })
        .expect("a drawing");
    rig.send(PlayMessage::Quit(Instant::now()));
    assert!(job.cancel.is_cancelled());
    assert!(!rig.state.play_live());
    // The effect runs anyway and answers: nothing starts.
    runner.settle(&mut rig, effects);
    assert!(!rig.state.play_live());
    assert!(rig.state.play_phase().is_none());
}

#[test]
fn a_second_start_while_a_position_is_being_drawn_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    rig.start(config.clone());
    let effects = rig.start(config);
    assert!(!effects.iter().any(|e| matches!(e, Effect::PlayPick { .. })));
    assert_eq!(rig.notice(), Some(PlayNotice::AlreadyRunning));
}

#[test]
fn the_watchdog_gives_up_a_drawing_that_never_answers() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    let effects = rig.start(config);
    let job = effects
        .iter()
        .find_map(|e| match e {
            Effect::PlayPick { job, .. } => Some(job.clone()),
            _ => None,
        })
        .expect("a drawing");
    rig.send(PlayMessage::Tick(Instant::now() + Duration::from_secs(5)));
    assert!(rig.state.play_picking(), "not yet");
    rig.send(PlayMessage::Tick(Instant::now() + Duration::from_secs(40)));
    assert!(!rig.state.play_picking());
    assert!(job.cancel.is_cancelled());
    assert_eq!(rig.notice(), Some(PlayNotice::TacticalPickFailed));
}

#[test]
fn a_file_that_cannot_be_read_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let runner = Runner::new();
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(
        dir.path().join("missing.pgn"),
    ));
    let effects = rig.start(config);
    runner.settle(&mut rig, effects);
    assert!(!rig.state.play_live());
    assert_eq!(rig.notice(), Some(PlayNotice::TacticalPickFailed));
}

#[test]
fn a_file_of_short_games_has_no_position_to_offer() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(
        dir.path(),
        "short.pgn",
        "[Event \"s\"]\n[Result \"*\"]\n\n1. e4 e5 *\n",
    );
    let runner = Runner::new();
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    let effects = rig.start(config);
    runner.settle(&mut rig, effects);
    assert!(!rig.state.play_live());
    assert_eq!(rig.notice(), Some(PlayNotice::TacticalPickFailed));
}

#[test]
fn the_current_database_without_games_says_so() {
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::CurrentBase);
    let effects = rig.start(config);
    assert!(!effects.iter().any(|e| matches!(e, Effect::PlayPick { .. })));
    assert_eq!(rig.notice(), Some(PlayNotice::TacticalPickEmpty));
    assert!(!rig.state.play_live());
}

#[test]
fn a_position_is_drawn_from_the_filter_of_the_current_database() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let runner = Runner::new();
    let mut rig = Rig::with_options(full_options());
    rig.state.open_pgn_in_slots = true;
    let effects = update(
        &mut rig.state,
        Message::Bases(mychess_app::BasesMessage::Open(file)),
    );
    runner.settle(&mut rig, effects);
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::CurrentBase);
    let effects = rig.start(config);
    assert!(effects.iter().any(|e| matches!(e, Effect::PlayPick { .. })));
    runner.settle(&mut rig, effects);
    assert!(matches!(
        rig.view().phase,
        PlayPhase::Player | PlayPhase::Opponent
    ));
    assert_ne!(
        rig.state.session.game().start_position().clone(),
        Position::new()
    );
}

// ---- never lose the game of the user, never an online help (D2466, D2467) ----

#[test]
fn an_online_game_forbids_the_tactical_game() {
    let mut rig = Rig::with_options(full_options());
    rig.state.online.marker = Some(mychess_app::online::MarkerInfo {
        game_id: "abc".to_owned(),
        date: "2026-10-26".to_owned(),
    });
    assert!(rig.state.assist_locked());
    let config = tactical(&rig, ColorChoice::White, 1500);
    rig.start(config);
    assert!(!rig.state.play_live());
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Online(
            mychess_app::online::OnlineNotice::AssistanceDisabled
        ))
    ));
}

#[test]
fn the_tactical_start_and_its_shortcut_are_help_of_the_engines_family() {
    use mychess_app::online::{Family, family_of};
    let rig = Rig::with_options(full_options());
    let config = tactical(&rig, ColorChoice::White, 1500);
    assert_eq!(
        family_of(&Message::Play(PlayMessage::Start(Box::new(config)))),
        Some(Family::Engines)
    );
    assert_eq!(
        family_of(&Message::Action(mychess_app::Action::TacticalGame)),
        Some(Family::Engines)
    );
}

#[test]
fn the_start_defends_itself_when_the_lock_comes_from_a_late_answer() {
    // A drawing answered after an online game started must not start the
    // game (D2467).
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let runner = Runner::new();
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    let effects = rig.start(config);
    rig.state.online.marker = Some(mychess_app::online::MarkerInfo {
        game_id: "abc".to_owned(),
        date: "2026-10-26".to_owned(),
    });
    runner.settle(&mut rig, effects);
    assert!(!rig.state.play_live());
    assert!(rig.state.play_phase().is_none());
    assert!(
        matches!(
            rig.state.notice,
            Some(AppNotice::Online(
                mychess_app::online::OnlineNotice::AssistanceDisabled
            ))
        ),
        "the user is told why nothing started"
    );
}

#[test]
fn quitting_the_program_gives_up_the_drawing() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "g.pgn", RUY_LOPEZ);
    let mut rig = Rig::with_options(full_options());
    let mut config = tactical(&rig, ColorChoice::White, 1500);
    config.start = StartFrom::Pick(mychess_app::tactical_game::PickSource::File(file));
    let effects = rig.start(config);
    let job = effects
        .iter()
        .find_map(|e| match e {
            Effect::PlayPick { job, .. } => Some(job.clone()),
            _ => None,
        })
        .expect("a drawing");
    rig.send(Message::Quit);
    assert!(job.cancel.is_cancelled());
    assert!(!rig.state.play_picking());
}

#[test]
fn the_tactical_game_is_the_same_mutual_exclusion_as_any_game() {
    let mut rig = Rig::with_options(full_options());
    let config = tactical(&rig, ColorChoice::White, 1500);
    rig.start(config.clone());
    assert!(rig.state.play_live());
    rig.start(config);
    assert_eq!(rig.notice(), Some(PlayNotice::AlreadyRunning));
}

#[test]
fn the_color_of_the_player_decides_who_has_the_elo_tag() {
    let mut rig = Rig::with_options(full_options());
    let config = tactical(&rig, ColorChoice::White, 1500);
    rig.start(config);
    let tags = rig.state.session.game().tags();
    assert_eq!(tags.white_rating, None);
    assert_eq!(tags.black_rating.map(|r| r.value.get()), Some(1500));
}
