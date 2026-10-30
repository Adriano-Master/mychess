//! M8-1a contract tests of the glue between the controller and `AppState`
//! (spec D1223, D1227, D1231, D1232, D1237-D1242, D1245, D1250-D1252): a
//! fake engine answers through synthetic events fed to `update`.
//!
//! Engine events carry the real clock (the model reads it in one place), so
//! the tests that need time pass instants a safe margin away from "now".
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1_common;

use std::time::{Duration, Instant};

use m8_1_common::*;
use mychess_app::engines::{EngineDraft, EngineId, EngineMessage};
use mychess_app::{
    AppNotice, AppState, BookChoice, BookFailure, BookSelection, ColorChoice, Command, Effect,
    GameMessage, Message, Nav, OpeningLine, PlayConfig, PlayEnd, PlayMessage, PlayNotice,
    PlayOptions, PlayPhase, StartFrom, Strength, TimeMode, perform, update,
};
use mychess_core::{Color, Game, GameResult, Position, Role};
use mychess_engine::{
    BestMove, EngineCommand, EngineEvent, EngineWarning, OptionValue, Protocol, SearchId,
};
use rstest::rstest;

fn game_as(color: ColorChoice, mode: TimeMode) -> (Rig, Vec<Effect>) {
    let mut r = Rig::new();
    let cfg = r.config(color, mode);
    let fx = r.start(cfg);
    (r, fx)
}

fn white(mode: TimeMode) -> (Rig, Vec<Effect>) {
    game_as(ColorChoice::White, mode)
}

fn black(mode: TimeMode) -> (Rig, Vec<Effect>) {
    game_as(ColorChoice::Black, mode)
}

fn soon(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

fn e4() -> mychess_core::Move {
    Position::new().parse_uci("e2e4").unwrap()
}

// ---- starting (D1223) ----

#[test]
fn an_unknown_engine_does_not_start_a_game() {
    let mut state = AppState::default();
    update(
        &mut state,
        PlayMessage::Start(Box::new(PlayConfig::new(EngineId(99)))).into(),
    );
    assert!(!state.play_live());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Play(PlayNotice::NoEngine))
    ));
}

#[test]
fn an_engine_that_speaks_xboard_does_not_play_in_v1() {
    let mut state = AppState::default();
    let mut draft = EngineDraft::new("Old", "/old");
    draft.protocol = Protocol::XBoard;
    let id = state.engines.add(draft).unwrap();
    update(
        &mut state,
        PlayMessage::Start(Box::new(PlayConfig::new(id))).into(),
    );
    assert!(!state.play_live());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Play(PlayNotice::NotUci))
    ));
}

#[test]
fn a_game_with_the_player_as_white_waits_for_his_move_and_keeps_the_board_upright() {
    let (r, fx) = white(TimeMode::Depth(3));
    assert!(play_search(&fx).is_none());
    assert!(r.state.play_live());
    assert!(!r.state.board_flipped);
    let v = r.view();
    assert_eq!(v.phase, PlayPhase::Player);
    assert_eq!(v.player_color, Color::White);
    assert_eq!(v.result, GameResult::Unknown);
    assert!(!v.countdown);
    assert!(r.state.play_wants_ticks());
}

#[test]
fn a_game_with_the_player_as_black_turns_the_board_and_the_engine_moves_first() {
    let (r, fx) = black(TimeMode::Depth(3));
    let req = play_search(&fx).expect("the engine thinks");
    assert!(req.position.moves.is_empty());
    assert!(r.state.board_flipped);
    assert_eq!(r.view().phase, PlayPhase::Opponent);
    assert_eq!(r.view().player_color, Color::Black);
}

#[test]
fn a_random_colour_resolves_to_one_of_the_two_and_is_consistent_with_the_board() {
    let (r, fx) = game_as(ColorChoice::Random, TimeMode::Depth(1));
    let v = r.view();
    assert_eq!(r.state.board_flipped, v.player_color == Color::Black);
    assert_eq!(play_search(&fx).is_some(), v.player_color == Color::Black);
}

#[test]
fn a_second_game_cannot_start_over_the_first() {
    let (mut r, _) = white(TimeMode::Depth(1));
    let cfg = r.config(ColorChoice::Black, TimeMode::Depth(1));
    let fx = r.start(cfg);
    assert!(fx.is_empty());
    assert_eq!(r.notice(), Some(PlayNotice::AlreadyRunning));
    assert_eq!(
        r.view().player_color,
        Color::White,
        "the first game goes on"
    );
}

#[test]
fn trial_mode_holds_the_game_back() {
    let mut r = Rig::new();
    r.send(GameMessage::EnterTrial { null_move: false });
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(1));
    r.start(cfg);
    assert!(!r.state.play_live());
    assert_eq!(r.notice(), Some(PlayNotice::Busy));
}

// ---- go per mode (D1238, D1250) ----

#[rstest]
#[case(TimeMode::MoveTime(6), Some(Duration::from_millis(6000)), None, None)]
#[case(
    TimeMode::MoveTime(120),
    Some(Duration::from_millis(120_000)),
    None,
    None
)]
#[case(TimeMode::Depth(5), None, Some(5), None)]
#[case(TimeMode::Nodes(1000), None, None, Some(1000))]
fn each_mode_sends_its_own_limit(
    #[case] mode: TimeMode,
    #[case] movetime: Option<Duration>,
    #[case] depth: Option<u32>,
    #[case] nodes: Option<u64>,
) {
    let (_, fx) = black(mode);
    let req = play_search(&fx).expect("search");
    assert_eq!(req.limits.movetime, movetime);
    assert_eq!(req.limits.depth, depth);
    assert_eq!(req.limits.nodes, nodes);
    assert!(req.limits.clock.is_none());
}

#[test]
fn timebonus_sends_the_clock_of_both_sides() {
    let (_, fx) = black(timebonus(10, 5, 3, 2));
    let req = play_search(&fx).expect("search");
    let clock = req.limits.clock.expect("clock");
    assert_eq!(clock.white_time, 600 * SEC);
    assert_eq!(clock.black_time, 180 * SEC);
    assert_eq!(clock.white_increment, 5 * SEC);
    assert_eq!(clock.black_increment, 2 * SEC);
    assert!(req.limits.movetime.is_none());
    assert!(req.limits.depth.is_none());
}

#[test]
fn a_new_game_goes_to_the_engine_before_the_first_search_and_the_options_before_both() {
    let mut r = Rig::with_options(vec![
        check("UCI_LimitStrength"),
        spin("UCI_Elo", 1320, 3190),
        spin("Skill Level", 0, 20),
    ]);
    let mut cfg = r.config(ColorChoice::Black, TimeMode::Depth(2));
    cfg.strength = Strength {
        elo: Some(1000),
        skill: Some(50),
    };
    let fx = r.start(cfg);
    let cmds = commands(&fx);
    let pos = |pred: &dyn Fn(&EngineCommand) -> bool| cmds.iter().position(pred);
    let new_game = pos(&|c| *c == EngineCommand::NewGame).expect("NewGame");
    let search = pos(&|c| matches!(c, EngineCommand::Search(_))).expect("Search");
    let limit =
        pos(&|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "UCI_LimitStrength"))
            .expect("UCI_LimitStrength");
    assert!(new_game < search);
    assert!(limit < search, "options go before the search");
}

// ---- limited strength (D1237, D1245) ----

#[test]
fn strength_is_sent_clamped_to_the_range_the_engine_declares_and_tagged_as_asked() {
    let mut r = Rig::with_options(vec![
        check("UCI_LimitStrength"),
        spin("UCI_Elo", 1320, 3190),
        spin("Skill Level", 0, 20),
    ]);
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    cfg.strength = Strength {
        elo: Some(1000),
        skill: Some(50),
    };
    let fx = r.start(cfg);
    let options = set_options(&fx);
    assert!(options.contains(&("UCI_LimitStrength".to_owned(), OptionValue::Bool(true))));
    assert!(options.contains(&("UCI_Elo".to_owned(), OptionValue::Int(1320))));
    assert!(options.contains(&("Skill Level".to_owned(), OptionValue::Int(20))));
    let tags = r.state.session.game().tags();
    assert_eq!(tags.get_extra("UCI_Elo"), Some("1000"));
    assert_eq!(tags.get_extra("SkillLevel"), Some("50"));
}

#[test]
fn a_value_inside_the_range_goes_through_unchanged() {
    let mut r = Rig::with_options(vec![
        check("UCI_LimitStrength"),
        spin("UCI_Elo", 1320, 3190),
    ]);
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    cfg.strength.elo = Some(2000);
    let fx = r.start(cfg);
    assert!(set_options(&fx).contains(&("UCI_Elo".to_owned(), OptionValue::Int(2000))));
}

#[test]
fn options_the_engine_does_not_declare_are_not_sent() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    cfg.strength = Strength {
        elo: Some(1500),
        skill: Some(5),
    };
    let fx = r.start(cfg);
    assert!(set_options(&fx).is_empty(), "{:?}", set_options(&fx));
    assert!(r.state.play_live());
}

#[test]
fn without_a_strength_nothing_is_sent_and_no_tag_is_written() {
    let mut r = Rig::with_options(vec![
        check("UCI_LimitStrength"),
        spin("UCI_Elo", 1320, 3190),
        spin("Skill Level", 0, 20),
    ]);
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    let fx = r.start(cfg);
    assert!(set_options(&fx).is_empty());
    let tags = r.state.session.game().tags();
    assert_eq!(tags.get_extra("UCI_Elo"), None);
    assert_eq!(tags.get_extra("SkillLevel"), None);
}

// ---- tags in the editor (D1231, D1232) ----

#[test]
fn the_tags_of_the_game_follow_the_spec() {
    let mut r = Rig::new();
    r.state.prefs.play.player_name = "Ana".to_owned();
    let cfg = r.config(ColorChoice::White, blitz());
    r.start(cfg);
    let tags = r.state.session.game().tags();
    assert_eq!(tags.event, "UCI Game");
    assert_eq!(tags.white, "Ana");
    assert_eq!(tags.black, "Fake 1");
    assert_eq!(tags.result, GameResult::Unknown);
    assert!(
        tags.date.year().is_some_and(|y| y >= 2026),
        "{:?}",
        tags.date
    );
    assert_eq!(tags.get_extra("Timemode"), Some("Time+Bonus 5+3/5+3"));
    assert_eq!(tags.get_extra("TimeControl"), Some("300+3"));
}

#[test]
fn a_player_without_a_name_is_a_question_mark_and_colours_swap_for_black() {
    let (r, _) = black(TimeMode::Depth(4));
    let tags = r.state.session.game().tags();
    assert_eq!(tags.white, "Fake 1");
    assert_eq!(tags.black, "?");
    assert_eq!(tags.get_extra("Timemode"), Some("Depth 4"));
}

#[rstest]
#[case(TimeMode::Nodes(700), "Nodes 700")]
#[case(TimeMode::MoveTime(9), "Movetime 9")]
#[case(timebonus(10, 0, 3, 5), "Time+Bonus 10+0/3+5")]
fn the_timemode_tag_has_the_text_of_the_scid(#[case] mode: TimeMode, #[case] text: &str) {
    let (r, _) = white(mode);
    let tags = r.state.session.game().tags();
    assert_eq!(tags.get_extra("Timemode"), Some(text));
}

#[test]
fn out_of_range_values_are_normalized_before_the_game_starts() {
    let (r, fx) = black(TimeMode::Depth(500));
    assert_eq!(play_search(&fx).unwrap().limits.depth, Some(20));
    assert_eq!(
        r.state.session.game().tags().get_extra("Timemode"),
        Some("Depth 20")
    );
}

// ---- the round trip and the end ----

#[test]
fn the_player_and_the_engine_alternate_and_the_game_is_mirrored_in_the_editor() {
    let (mut r, _) = white(TimeMode::Depth(2));
    let fx = r.player_move("e2", "e4", soon(0));
    let req = play_search(&fx).expect("the engine thinks");
    assert_eq!(req.position.moves, [e4()]);
    assert_eq!(r.view().phase, PlayPhase::Opponent);
    assert_eq!(r.mainline(), ["e2e4"]);
    let fx = r.reply(&fx, Some("e7e5"));
    assert!(play_search(&fx).is_none());
    assert_eq!(r.view().phase, PlayPhase::Player);
    assert_eq!(r.mainline(), ["e2e4", "e7e5"]);
}

#[test]
fn an_illegal_move_of_the_player_is_refused_with_a_notice() {
    let (mut r, _) = white(TimeMode::Depth(2));
    let fx = r.player_move("e2", "e5", soon(0));
    assert!(fx.is_empty());
    assert_eq!(r.notice(), Some(PlayNotice::IllegalMove));
    assert!(r.mainline().is_empty());
    assert_eq!(r.view().phase, PlayPhase::Player);
}

#[test]
fn a_move_out_of_turn_is_refused() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    // A legal move of Black's, on the engine's turn.
    r.player_move("e7", "e5", soon(0));
    assert_eq!(r.notice(), Some(PlayNotice::NotYourTurn));
    assert_eq!(r.mainline(), ["e2e4"]);
}

fn fools_mate(r: &mut Rig) {
    let t = soon(0);
    let fx = r.player_move("f2", "f3", t);
    r.reply(&fx, Some("e7e5"));
    let fx = r.player_move("g2", "g4", t);
    r.reply(&fx, Some("d8h4"));
}

#[test]
fn mate_ends_the_game_marks_the_editor_altered_and_hands_the_editor_back() {
    let (mut r, _) = white(TimeMode::Depth(2));
    fools_mate(&mut r);
    assert!(!r.state.play_live());
    assert!(!r.state.play_wants_ticks());
    assert_eq!(r.state.session.game().tags().result, GameResult::Black);
    assert!(r.state.is_dirty(), "the game is waiting to be saved");
    assert_eq!(
        r.notice(),
        Some(PlayNotice::Ended(PlayEnd::Checkmate {
            winner: Color::Black
        }))
    );
    // The editor is the user's again: an edit works.
    r.send(GameMessage::Navigate(Nav::Back(1)));
    let before = r.mainline().len();
    r.send(GameMessage::PlayMove {
        mv: r.state.session.position().parse_uci("d8h4").unwrap(),
        options: PlayOptions::default(),
    });
    assert!(r.mainline().len() >= before);
    assert!(r.notice() != Some(PlayNotice::Locked));
}

#[test]
fn the_engine_is_given_back_after_the_end_with_a_stop_if_it_was_thinking() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    let fx = r.send(PlayMessage::Resign(soon(0)));
    assert!(commands(&fx).contains(&EngineCommand::Stop));
    assert!(!r.state.play_live());
}

#[test]
fn resign_records_the_loss_of_the_player_and_leaves_the_game_to_be_saved() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    r.send(PlayMessage::Resign(soon(0)));
    assert_eq!(r.state.session.game().tags().result, GameResult::Black);
    assert!(r.state.is_dirty());
    assert_eq!(
        r.notice(),
        Some(PlayNotice::Ended(PlayEnd::Resignation {
            loser: Color::White
        }))
    );
    assert_eq!(r.mainline(), ["e2e4"]);
}

#[test]
fn quit_records_no_result_does_not_alter_the_game_and_stops_the_engine() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    let fx = r.send(PlayMessage::Quit(soon(0)));
    assert!(commands(&fx).contains(&EngineCommand::Stop));
    assert!(!r.state.play_live());
    assert_eq!(r.state.session.game().tags().result, GameResult::Unknown);
    assert!(!r.state.is_dirty(), "quit does not ask to be saved");
    assert!(r.state.play_view(Instant::now()).is_none());
}

#[test]
fn an_engine_without_a_move_ends_the_game_without_a_result() {
    let (mut r, fx) = black(TimeMode::Depth(2));
    r.reply(&fx, None);
    assert!(!r.state.play_live());
    assert_eq!(r.state.session.game().tags().result, GameResult::Unknown);
    assert_eq!(
        r.notice(),
        Some(PlayNotice::Ended(PlayEnd::OpponentFailure))
    );
}

#[test]
fn a_move_the_engine_session_called_illegal_arrives_as_no_move_and_ends_the_game() {
    let (mut r, fx) = black(TimeMode::Depth(2));
    let req = play_search(&fx).unwrap();
    let s = SearchId(5000);
    r.event(EngineEvent::SearchTagged {
        search: s,
        tag: req.tag,
    });
    r.event(EngineEvent::SearchStarted(s));
    r.event(EngineEvent::Warning(EngineWarning::IllegalBestMove(
        "e2e5".to_owned(),
    )));
    r.event(EngineEvent::BestMove(BestMove {
        search: s,
        mv: None,
        ponder: None,
    }));
    assert!(!r.state.play_live());
    assert!(r.mainline().is_empty());
}

#[test]
fn an_illegal_move_for_the_position_handed_over_as_a_real_move_also_ends_the_game() {
    // The engine session already filters, but the glue must not trust it.
    let (mut r, fx) = black(TimeMode::Depth(2));
    let req = play_search(&fx).unwrap();
    let black_move =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1")
            .unwrap()
            .parse_uci("e7e5")
            .unwrap();
    r.answer(&req, Some(black_move));
    assert!(!r.state.play_live());
    assert!(r.mainline().is_empty());
    assert_eq!(r.state.session.game().tags().result, GameResult::Unknown);
}

// ---- the flag and the ticks (D1226, D1251) ----

#[test]
fn a_tick_after_the_flag_ends_the_game_and_before_it_does_not() {
    let (mut r, _) = white(timebonus(1, 0, 1, 0));
    assert!(r.view().countdown);
    r.send(PlayMessage::Tick(soon(50)));
    assert!(r.state.play_live());
    r.send(PlayMessage::Tick(soon(61)));
    assert!(!r.state.play_live());
    assert_eq!(r.state.session.game().tags().result, GameResult::Black);
    assert_eq!(
        r.notice(),
        Some(PlayNotice::Ended(PlayEnd::Timeout {
            loser: Color::White
        }))
    );
}

#[test]
fn the_flag_of_the_engine_wins_the_game_for_the_player() {
    let (mut r, _) = white(timebonus(1, 0, 1, 0));
    r.player_move("e2", "e4", soon(1));
    r.send(PlayMessage::Tick(soon(62)));
    assert!(!r.state.play_live());
    assert_eq!(r.state.session.game().tags().result, GameResult::White);
}

#[test]
fn the_clock_of_the_view_counts_down_in_timebonus_and_up_otherwise() {
    let (r, _) = white(timebonus(2, 0, 2, 0));
    let v = r.view_at(soon(10)).unwrap();
    assert!(v.countdown);
    assert!(v.white_time < 120 * SEC && v.white_time >= 109 * SEC);
    assert_eq!(v.black_time, 120 * SEC);
    let (r, _) = white(TimeMode::MoveTime(5));
    let v = r.view_at(soon(10)).unwrap();
    assert!(!v.countdown);
    assert!(v.white_time >= 9 * SEC && v.white_time <= 11 * SEC);
    assert_eq!(v.black_time, Duration::ZERO);
}

#[test]
fn ticks_are_wanted_only_while_a_clock_runs() {
    let (mut r, _) = white(TimeMode::Depth(1));
    assert!(r.state.play_wants_ticks());
    r.player_move("e2", "e4", soon(0));
    assert!(r.state.play_wants_ticks(), "the engine's clock runs");
    r.send(PlayMessage::Pause(soon(0)));
    assert!(!r.state.play_wants_ticks());
    r.send(PlayMessage::Resume(soon(0)));
    assert!(r.state.play_wants_ticks());
    r.send(PlayMessage::Resign(soon(0)));
    assert!(!r.state.play_wants_ticks());
}

// ---- the engine goes silent or dies (D1239, D1251) ----

#[rstest]
#[case::movetime(TimeMode::MoveTime(6), 30, 40)]
#[case::depth(TimeMode::Depth(3), 100, 130)]
#[case::nodes(TimeMode::Nodes(500), 100, 130)]
fn a_silent_engine_is_given_up_after_its_time_plus_a_grace(
    #[case] mode: TimeMode,
    #[case] still_waiting: u64,
    #[case] given_up: u64,
) {
    let (mut r, _) = black(mode);
    r.send(PlayMessage::Tick(soon(still_waiting)));
    assert_eq!(r.view().phase, PlayPhase::Opponent);
    r.send(PlayMessage::Tick(soon(given_up)));
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: true });
    assert_eq!(r.notice(), Some(PlayNotice::EngineLost));
    assert!(r.view().engine_lost);
    assert!(!r.view().can_resume);
    assert!(r.state.play_live(), "the game is paused, not over");
    assert!(!r.state.is_dirty());
}

#[test]
fn a_silent_engine_is_told_to_stop() {
    let (mut r, _) = black(TimeMode::Depth(3));
    let fx = r.send(PlayMessage::Tick(soon(130)));
    assert!(commands(&fx).contains(&EngineCommand::Stop));
}

#[test]
fn the_engine_dying_mid_search_pauses_with_a_notice_and_the_ui_keeps_working() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    r.event(exited());
    let v = r.view();
    assert_eq!(v.phase, PlayPhase::Paused { lost: true });
    assert!(v.engine_lost);
    assert_eq!(r.notice(), Some(PlayNotice::EngineLost));
    assert_eq!(r.mainline(), ["e2e4"]);
    // Resume does not work without a new engine, and nothing panics.
    r.send(PlayMessage::Resume(soon(0)));
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: true });
    r.send(PlayMessage::Tick(soon(999)));
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: true });
    // The game can still be abandoned.
    r.send(PlayMessage::Quit(soon(0)));
    assert!(!r.state.play_live());
}

#[test]
fn a_dead_engine_is_replaced_by_another_one_that_takes_the_position_over() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    r.event(exited());
    let other = r
        .state
        .engines
        .add(EngineDraft::new("Other", "/other"))
        .unwrap();
    let fx = r.send(PlayMessage::SwitchEngine(other, soon(0)));
    let run2 = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("a new engine starts");
    let mut ident = identity(Vec::new());
    ident.name = "Other 2".to_owned();
    let fx = update(
        &mut r.state,
        Message::Engine(EngineMessage::Started {
            run: run2,
            identity: Box::new(ident),
            at: 1_700_000_001,
        }),
    );
    let req = play_search(&fx).expect("the new engine is asked for the move");
    assert_eq!(req.position.moves, [e4()]);
    assert_eq!(r.view().phase, PlayPhase::Opponent);
    assert_eq!(r.view().opponent.name, "Other 2");
    assert_eq!(r.state.session.game().tags().black, "Other 2");
}

#[test]
fn switching_engines_when_nothing_was_lost_does_nothing() {
    let (mut r, _) = white(TimeMode::Depth(2));
    let other = r
        .state
        .engines
        .add(EngineDraft::new("Other", "/other"))
        .unwrap();
    let fx = r.send(PlayMessage::SwitchEngine(other, soon(0)));
    assert!(fx.is_empty());
    assert_eq!(r.view().phase, PlayPhase::Player);
}

// ---- pause, resume, Clear Hash (D1240) ----

#[test]
fn navigating_pauses_the_game_and_resume_asks_the_engine_again() {
    let (mut r, _) = white(TimeMode::Depth(2));
    let fx = r.player_move("e2", "e4", soon(0));
    let first = play_search(&fx).unwrap();
    let fx = r.send(GameMessage::Navigate(Nav::Start));
    assert!(commands(&fx).contains(&EngineCommand::Stop));
    let v = r.view();
    assert_eq!(v.phase, PlayPhase::Paused { lost: false });
    assert!(v.can_resume);
    let fx = r.send(PlayMessage::Resume(soon(0)));
    let again = play_search(&fx).expect("asked again");
    assert_eq!(again.position.moves, [e4()]);
    assert_ne!(again.tag, first.tag, "a new search, a new label");
    assert_eq!(r.view().phase, PlayPhase::Opponent);
}

#[test]
fn the_answer_of_a_search_stopped_by_the_pause_is_ignored() {
    let (mut r, _) = white(TimeMode::Depth(2));
    let fx = r.player_move("e2", "e4", soon(0));
    let req = play_search(&fx).unwrap();
    let s = SearchId(7000);
    r.event(EngineEvent::SearchTagged {
        search: s,
        tag: req.tag,
    });
    r.event(EngineEvent::SearchStarted(s));
    r.send(PlayMessage::Pause(soon(0)));
    let e5 = Position::new()
        .after(e4())
        .unwrap()
        .parse_uci("e7e5")
        .unwrap();
    r.event(EngineEvent::BestMove(BestMove {
        search: s,
        mv: Some(e5),
        ponder: None,
    }));
    assert_eq!(r.mainline(), ["e2e4"], "the late answer was dropped");
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: false });
    // And after Resume the old answer still finds no home.
    r.send(PlayMessage::Resume(soon(0)));
    r.event(EngineEvent::BestMove(BestMove {
        search: s,
        mv: Some(e5),
        ponder: None,
    }));
    assert_eq!(r.mainline(), ["e2e4"]);
    assert_eq!(r.view().phase, PlayPhase::Opponent);
}

#[test]
fn resume_clears_the_hash_before_the_engine_thinks_again_if_the_engine_can() {
    let mut r = Rig::with_options(vec![button("Clear Hash")]);
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    r.start(cfg);
    r.player_move("e2", "e4", soon(0));
    r.send(PlayMessage::Pause(soon(0)));
    let fx = r.send(PlayMessage::Resume(soon(0)));
    let cmds = commands(&fx);
    let clear = cmds
        .iter()
        .position(|c| matches!(c, EngineCommand::SetOption { name, .. } if name == "Clear Hash"))
        .expect("Clear Hash");
    let search = cmds
        .iter()
        .position(|c| matches!(c, EngineCommand::Search(_)))
        .expect("Search");
    assert!(clear < search);
}

#[test]
fn an_engine_without_the_clear_hash_button_is_not_sent_the_option() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    r.send(PlayMessage::Pause(soon(0)));
    let fx = r.send(PlayMessage::Resume(soon(0)));
    assert!(set_options(&fx).is_empty());
}

#[test]
fn resume_on_the_players_turn_also_sends_clear_hash() {
    let mut r = Rig::with_options(vec![button("Clear Hash")]);
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    r.start(cfg);
    r.send(PlayMessage::Pause(soon(0)));
    let fx = r.send(PlayMessage::Resume(soon(0)));
    assert!(set_options(&fx).iter().any(|(n, _)| n == "Clear Hash"));
}

// ---- the editor during the game (D1252) ----

#[test]
fn edits_are_refused_while_the_game_lives() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    let mv = Position::new().parse_uci("d2d4").unwrap();
    let fx = r.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert!(fx.is_empty());
    assert_eq!(r.notice(), Some(PlayNotice::Locked));
    assert_eq!(r.mainline(), ["e2e4"]);
    assert_eq!(
        r.view().phase,
        PlayPhase::Opponent,
        "a refused edit does not pause"
    );
}

#[test]
fn a_new_game_is_refused_while_the_game_lives() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    r.send(Message::Command(Command::NewGame));
    assert!(r.state.play_live());
    assert_eq!(r.mainline(), ["e2e4"]);
}

#[test]
fn navigation_goes_through_and_pauses_while_edits_are_refused() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.player_move("e2", "e4", soon(0));
    r.send(GameMessage::Navigate(Nav::Start));
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: false });
    assert_eq!(r.state.session.cursor(), r.state.session.game().root());
    // Still locked for edits.
    let mv = Position::new().parse_uci("d2d4").unwrap();
    r.send(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    });
    assert_eq!(r.notice(), Some(PlayNotice::Locked));
    assert_eq!(r.mainline(), ["e2e4"]);
}

// ---- the unsaved-game guard (D1252) ----

fn dirty_editor() -> Rig {
    let mut r = Rig::new();
    r.send(GameMessage::PlayMove {
        mv: e4(),
        options: PlayOptions::default(),
    });
    assert!(r.state.is_dirty());
    r
}

#[test]
fn a_game_does_not_start_over_an_edited_game_and_nothing_is_lost() {
    let mut r = dirty_editor();
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    let fx = r.start(cfg);
    assert!(fx.is_empty());
    assert!(!r.state.play_live());
    assert_eq!(r.notice(), Some(PlayNotice::UnsavedGame));
    assert_eq!(r.mainline(), ["e2e4"]);
    assert!(r.state.is_dirty());
}

#[test]
fn a_saved_game_in_the_editor_is_replaced_without_asking() {
    let mut r = dirty_editor();
    r.state.session.mark_saved();
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    r.start(cfg);
    assert!(r.state.play_live());
    assert!(r.mainline().is_empty());
}

#[test]
fn a_pristine_editor_is_replaced_without_asking() {
    let (r, _) = white(TimeMode::Depth(2));
    assert!(r.state.play_live());
}

// ---- restart (D1227) ----

#[test]
fn restart_begins_again_with_the_same_configuration_and_a_new_search() {
    let (mut r, _) = black(timebonus(3, 2, 3, 2));
    let fx = r.send(PlayMessage::Restart(soon(0)));
    let req = play_search(&fx).expect("the engine opens the new game");
    assert!(req.position.moves.is_empty());
    assert_eq!(req.limits.clock.unwrap().white_time, 180 * SEC);
    assert!(r.state.play_live());
    assert!(r.state.board_flipped);
    assert_eq!(
        r.state.session.game().tags().get_extra("Timemode"),
        Some("Time+Bonus 3+2/3+2")
    );
    assert!(r.mainline().is_empty());
}

#[test]
fn restart_after_a_played_game_throws_the_old_one_away_without_asking() {
    let (mut r, _) = white(TimeMode::Depth(2));
    fools_mate(&mut r);
    assert!(r.state.is_dirty());
    r.send(PlayMessage::Restart(soon(0)));
    assert!(r.state.play_live());
    assert!(r.mainline().is_empty());
    assert_eq!(r.state.session.game().tags().result, GameResult::Unknown);
    assert_eq!(r.view().phase, PlayPhase::Player);
}

#[test]
fn restart_in_the_middle_of_a_search_stops_it_and_drops_its_late_answer() {
    let (mut r, fx) = black(TimeMode::Depth(2));
    let old = play_search(&fx).unwrap();
    let s = SearchId(9000);
    r.event(EngineEvent::SearchTagged {
        search: s,
        tag: old.tag,
    });
    r.event(EngineEvent::SearchStarted(s));
    let fx = r.send(PlayMessage::Restart(soon(0)));
    assert!(commands(&fx).contains(&EngineCommand::Stop));
    let new = play_search(&fx).expect("the new game's search");
    assert_ne!(new.tag, old.tag);
    // The answer of the old search must not play in the new game.
    r.event(EngineEvent::BestMove(BestMove {
        search: s,
        mv: Some(e4()),
        ponder: None,
    }));
    assert!(r.mainline().is_empty());
    assert_eq!(r.view().phase, PlayPhase::Opponent);
    // The answer of the new one does.
    let d4 = Position::new().parse_uci("d2d4").unwrap();
    r.answer(&new, Some(d4));
    assert_eq!(r.mainline(), ["d2d4"]);
}

#[test]
fn restart_without_a_game_does_nothing() {
    let mut r = Rig::new();
    let fx = r.send(PlayMessage::Restart(soon(0)));
    assert!(fx.is_empty());
    assert!(!r.state.play_live());
}

// ---- the start position (D1236) ----

#[test]
fn the_game_can_start_from_the_position_on_the_board() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";
    let mut r = Rig::new();
    r.state
        .session
        .replace_game(Game::from_position(Position::from_fen(fen).unwrap()));
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    cfg.start = StartFrom::CurrentPosition;
    r.start(cfg);
    assert_eq!(r.state.session.game().start_position().fen(), fen);
    let fx = r.player_move("f1", "b5", soon(0));
    let req = play_search(&fx).unwrap();
    assert_eq!(req.position.start, Position::from_fen(fen).unwrap());
}

#[test]
fn a_new_game_start_ignores_the_position_on_the_board() {
    let mut r = Rig::new();
    r.state.session.replace_game(Game::from_position(
        Position::from_fen("4k3/8/8/8/8/8/R7/4K3 w - - 0 1").unwrap(),
    ));
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    r.start(cfg);
    assert_eq!(r.state.session.game().start_position(), &Position::new());
}

#[test]
fn a_specific_opening_is_played_for_free_and_tagged() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::Black, TimeMode::Depth(2));
    cfg.start = StartFrom::Opening(OpeningLine::parse("Sicilian: e4 c5 Nf3").unwrap());
    let fx = r.start(cfg);
    assert!(
        play_search(&fx).is_none(),
        "the line moves are not searched"
    );
    assert_eq!(r.mainline(), ["e2e4"]);
    assert_eq!(
        r.state.session.game().tags().get_extra("Opening"),
        Some("Sicilian")
    );
}

#[test]
fn a_bad_opening_aborts_the_game_and_gives_the_engine_back() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::Black, TimeMode::Depth(2));
    cfg.start = StartFrom::Opening(OpeningLine::parse("Bad: e4 e4").unwrap());
    r.start(cfg);
    assert!(!r.state.play_live());
    assert_eq!(r.notice(), Some(PlayNotice::BadOpening));
}

#[test]
fn promotion_needs_the_piece_and_the_chosen_piece_is_played() {
    let fen = "8/P6k/8/8/8/8/6K1/8 w - - 0 1";
    let mut r = Rig::new();
    r.state
        .session
        .replace_game(Game::from_position(Position::from_fen(fen).unwrap()));
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(2));
    cfg.start = StartFrom::CurrentPosition;
    r.start(cfg);
    assert!(r.state.play_needs_promotion(sq("a7"), sq("a8")));
    assert!(!r.state.play_needs_promotion(sq("g2"), sq("g3")));
    r.send(PlayMessage::PlayerMove {
        from: sq("a7"),
        to: sq("a8"),
        promotion: Some(Role::Knight),
        at: soon(0),
    });
    assert_eq!(r.mainline(), ["a7a8n"]);
}

// ---- messages without a game never panic ----

#[test]
fn every_message_without_a_game_is_harmless() {
    let mut r = Rig::new();
    let t = soon(0);
    let messages = vec![
        PlayMessage::PlayerMove {
            from: sq("e2"),
            to: sq("e4"),
            promotion: None,
            at: t,
        },
        PlayMessage::Answer { yes: true, at: t },
        PlayMessage::Answer { yes: false, at: t },
        PlayMessage::Resign(t),
        PlayMessage::Quit(t),
        PlayMessage::Restart(t),
        PlayMessage::Pause(t),
        PlayMessage::Resume(t),
        PlayMessage::SwitchEngine(EngineId(1), t),
        PlayMessage::Tick(t),
        PlayMessage::BookLoaded {
            request: 1,
            result: Err(BookFailure::NotFound),
        },
    ];
    for m in messages {
        let fx = r.send(m.clone());
        assert!(fx.is_empty(), "{m:?}");
        assert!(!r.state.play_live(), "{m:?}");
    }
    assert!(!r.state.play_wants_ticks());
    assert!(r.state.play_view(t).is_none());
    assert!(!r.state.play_needs_promotion(sq("e7"), sq("e8")));
}

#[test]
fn messages_after_quit_are_harmless() {
    let (mut r, _) = white(TimeMode::Depth(2));
    r.send(PlayMessage::Quit(soon(0)));
    for m in [
        PlayMessage::Resign(soon(0)),
        PlayMessage::Pause(soon(0)),
        PlayMessage::Resume(soon(0)),
        PlayMessage::Tick(soon(999)),
        PlayMessage::Answer {
            yes: true,
            at: soon(0),
        },
    ] {
        r.send(m);
    }
    assert!(!r.state.play_live());
    assert_eq!(r.state.session.game().tags().result, GameResult::Unknown);
}

// ---- the book through effects (D1234, D1245) ----

fn tmp_book(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("Mini.bin");
    let start = Position::new();
    let after = start.after(e4()).unwrap();
    let e5 = after.parse_uci("e7e5").unwrap();
    let mut book = mychess_book::Book::new();
    book.set_position_moves(&start, &[(e4(), 10)]).unwrap();
    book.set_position_moves(&after, &[(e5, 10)]).unwrap();
    book.save_as(&path).unwrap();
    path
}

fn book_cfg(r: &Rig, path: std::path::PathBuf) -> PlayConfig {
    let mut cfg = r.config(ColorChoice::Black, TimeMode::Depth(2));
    cfg.book = Some(BookChoice {
        path,
        selection: BookSelection::Best,
    });
    cfg
}

#[test]
fn the_book_is_loaded_by_an_effect_and_the_game_waits_for_it() {
    let mut r = Rig::new();
    let cfg = book_cfg(&r, std::path::PathBuf::from("none.bin"));
    let fx = r.start(cfg);
    assert!(load_book_request(&fx).is_some());
    assert!(r.state.play_live());
    assert!(r.state.play_view(Instant::now()).is_none(), "not begun yet");
    assert!(play_search(&fx).is_none());
}

#[test]
fn an_answer_for_another_request_is_ignored() {
    let mut r = Rig::new();
    let cfg = book_cfg(&r, std::path::PathBuf::from("none.bin"));
    let fx = r.start(cfg);
    let request = load_book_request(&fx).unwrap();
    r.send(PlayMessage::BookLoaded {
        request: request + 1000,
        result: Ok(mini_book(BookSelection::Best)),
    });
    assert!(r.state.play_view(Instant::now()).is_none());
}

#[test]
fn a_loaded_book_plays_the_first_move_for_the_engine_without_a_search() {
    let mut r = Rig::new();
    let cfg = book_cfg(&r, std::path::PathBuf::from("none.bin"));
    let fx = r.start(cfg);
    let request = load_book_request(&fx).unwrap();
    let fx = r.send(PlayMessage::BookLoaded {
        request,
        result: Ok(mini_book(BookSelection::Best)),
    });
    assert!(play_search(&fx).is_none());
    assert_eq!(r.mainline(), ["e2e4"]);
    assert_eq!(r.view().phase, PlayPhase::Player);
}

#[test]
fn an_unreadable_book_warns_and_the_game_goes_on_without_it() {
    let mut r = Rig::new();
    let cfg = book_cfg(&r, std::path::PathBuf::from("none.bin"));
    let fx = r.start(cfg);
    let request = load_book_request(&fx).unwrap();
    let fx = r.send(PlayMessage::BookLoaded {
        request,
        result: Err(BookFailure::Unreadable),
    });
    assert!(play_search(&fx).is_some(), "the engine thinks for itself");
    assert_eq!(r.notice(), Some(PlayNotice::BookUnreadable));
    assert!(r.state.play_live());
}

#[test]
fn the_question_of_the_book_reaches_the_view_and_no_undoes_the_move() {
    let mut r = Rig::new();
    let cfg = book_cfg(&r, std::path::PathBuf::from("none.bin"));
    let fx = r.start(cfg);
    let request = load_book_request(&fx).unwrap();
    r.send(PlayMessage::BookLoaded {
        request,
        result: Ok(mini_book(BookSelection::Best)),
    });
    // The book has e5 after e4; c5 leaves it.
    r.player_move("c7", "c5", soon(0));
    let v = r.view();
    assert!(matches!(v.phase, PlayPhase::Asking(_)));
    assert!(v.question.is_some());
    assert_eq!(r.mainline(), ["e2e4"]);
    assert!(
        !r.state.play_wants_ticks(),
        "the clock waits for the answer"
    );
    r.send(PlayMessage::Answer {
        yes: false,
        at: soon(0),
    });
    assert_eq!(r.view().phase, PlayPhase::Player);
    assert_eq!(r.notice(), Some(PlayNotice::MoveTakenBack));
    assert_eq!(r.mainline(), ["e2e4"]);
}

#[test]
fn the_question_answered_yes_plays_the_move_and_the_engine_thinks() {
    let mut r = Rig::new();
    let cfg = book_cfg(&r, std::path::PathBuf::from("none.bin"));
    let fx = r.start(cfg);
    let request = load_book_request(&fx).unwrap();
    r.send(PlayMessage::BookLoaded {
        request,
        result: Ok(mini_book(BookSelection::Best)),
    });
    r.player_move("c7", "c5", soon(0));
    let fx = r.send(PlayMessage::Answer {
        yes: true,
        at: soon(0),
    });
    assert!(play_search(&fx).is_some());
    assert_eq!(r.mainline(), ["e2e4", "c7c5"]);
    let comment = r
        .state
        .session
        .game()
        .comment(r.state.session.game().mainline().last().unwrap())
        .unwrap()
        .map(str::to_owned);
    assert!(
        comment.is_some_and(|c| c.starts_with("Move out of book")),
        "the editor shows the comment"
    );
}

#[tokio::test]
async fn the_effect_reads_a_real_book_file_and_a_missing_one_fails_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let path = tmp_book(dir.path());
    let mut r = Rig::new();
    let cfg = book_cfg(&r, path);
    let fx = r.start(cfg);
    let effect = fx
        .into_iter()
        .find(|e| matches!(e, Effect::LoadPlayBook { .. }))
        .expect("LoadPlayBook");
    let message = perform(effect).await;
    let fx = update(&mut r.state, message);
    assert!(play_search(&fx).is_none(), "the book plays e4");
    assert_eq!(r.mainline(), ["e2e4"]);

    // A missing file.
    let mut r = Rig::new();
    let cfg = book_cfg(&r, dir.path().join("missing.bin"));
    let fx = r.start(cfg);
    let effect = fx
        .into_iter()
        .find(|e| matches!(e, Effect::LoadPlayBook { .. }))
        .expect("LoadPlayBook");
    let message = perform(effect).await;
    let fx = update(&mut r.state, message);
    assert!(play_search(&fx).is_some());
    assert_eq!(r.notice(), Some(PlayNotice::BookUnreadable));
}
