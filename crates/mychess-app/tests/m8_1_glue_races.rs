//! M8-1a contract tests of the races and hostile events around the game
//! against an engine (spec D1239-D1241, D1250): late and duplicate answers,
//! refused commands, the engine stopped from outside, the analysis that must
//! not drive the engine of the game, the position on the board as start.
#![allow(
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_1_common;

use std::time::{Duration, Instant};

use m8_1_common::*;
use mychess_app::engines::{EngineMessage, RunId};
use mychess_app::{
    ColorChoice, Effect, GameMessage, Nav, PlayEnd, PlayMessage, PlayNotice, PlayPhase, StartFrom,
    TimeMode,
};
use mychess_core::{Color, Game, GameResult, Position};
use mychess_engine::{BestMove, EngineCommand, EngineEvent, EngineWarning, SearchId};

fn soon(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

fn started(color: ColorChoice, mode: TimeMode) -> (Rig, Vec<Effect>) {
    let mut r = Rig::new();
    let cfg = r.config(color, mode);
    let fx = r.start(cfg);
    (r, fx)
}

fn is_infinite_search(c: &EngineCommand) -> bool {
    matches!(c, EngineCommand::Search(r)
        if r.limits.depth.is_none()
            && r.limits.nodes.is_none()
            && r.limits.movetime.is_none()
            && r.limits.clock.is_none())
}

fn e4() -> mychess_core::Move {
    Position::new().parse_uci("e2e4").unwrap()
}

// ---- late and duplicate answers ----

#[test]
fn an_answer_that_arrives_twice_plays_only_once() {
    let (mut r, fx) = started(ColorChoice::Black, TimeMode::Depth(1));
    let req = play_search(&fx).unwrap();
    let s = SearchId(3001);
    r.event(EngineEvent::SearchTagged {
        search: s,
        tag: req.tag,
    });
    r.event(EngineEvent::SearchStarted(s));
    let best = BestMove {
        search: s,
        mv: Some(e4()),
        ponder: None,
    };
    r.event(EngineEvent::BestMove(best));
    assert_eq!(r.mainline(), ["e2e4"]);
    r.event(EngineEvent::BestMove(best));
    assert_eq!(r.mainline(), ["e2e4"]);
    assert_eq!(r.view().phase, PlayPhase::Player);
}

#[test]
fn an_answer_for_a_search_the_game_never_asked_is_ignored() {
    let (mut r, _) = started(ColorChoice::Black, TimeMode::Depth(1));
    r.event(EngineEvent::BestMove(BestMove {
        search: SearchId(424_242),
        mv: Some(e4()),
        ponder: None,
    }));
    assert!(r.mainline().is_empty());
    assert_eq!(r.view().phase, PlayPhase::Opponent);
}

#[test]
fn events_of_another_run_do_not_touch_the_game() {
    let (mut r, fx) = started(ColorChoice::Black, TimeMode::Depth(1));
    let req = play_search(&fx).unwrap();
    let s = SearchId(3002);
    r.send(EngineMessage::Event {
        run: RunId(987_654),
        event: Box::new(EngineEvent::SearchTagged {
            search: s,
            tag: req.tag,
        }),
    });
    r.send(EngineMessage::Event {
        run: RunId(987_654),
        event: Box::new(EngineEvent::BestMove(BestMove {
            search: s,
            mv: Some(e4()),
            ponder: None,
        })),
    });
    assert!(r.mainline().is_empty());
    assert_eq!(r.view().phase, PlayPhase::Opponent);
}

#[test]
fn resigning_before_the_engine_labelled_its_search_drops_the_whole_late_answer() {
    let (mut r, fx) = started(ColorChoice::Black, TimeMode::Depth(1));
    let req = play_search(&fx).unwrap();
    let fx = r.send(PlayMessage::Resign(soon(0)));
    assert!(commands(&fx).contains(&EngineCommand::Stop));
    assert_eq!(r.state.session.game().tags().result, GameResult::White);
    // The labels and the answer of the dead search arrive afterwards.
    let s = SearchId(3003);
    r.event(EngineEvent::SearchTagged {
        search: s,
        tag: req.tag,
    });
    r.event(EngineEvent::SearchStarted(s));
    r.event(EngineEvent::BestMove(BestMove {
        search: s,
        mv: Some(e4()),
        ponder: None,
    }));
    assert!(r.mainline().is_empty(), "nothing is played after the end");
    assert_eq!(r.state.session.game().tags().result, GameResult::White);
}

#[test]
fn quitting_while_the_engine_thinks_drops_its_late_answer() {
    let (mut r, fx) = started(ColorChoice::Black, TimeMode::Depth(1));
    let req = play_search(&fx).unwrap();
    let s = SearchId(3004);
    r.event(EngineEvent::SearchTagged {
        search: s,
        tag: req.tag,
    });
    r.event(EngineEvent::SearchStarted(s));
    r.send(PlayMessage::Quit(soon(0)));
    r.event(EngineEvent::BestMove(BestMove {
        search: s,
        mv: Some(e4()),
        ponder: None,
    }));
    assert!(r.mainline().is_empty());
    assert!(!r.state.play_live());
}

#[test]
fn a_late_answer_does_not_leak_into_the_next_game() {
    let (mut r, fx) = started(ColorChoice::Black, TimeMode::Depth(1));
    let old = play_search(&fx).unwrap();
    let s = SearchId(3005);
    r.event(EngineEvent::SearchTagged {
        search: s,
        tag: old.tag,
    });
    r.event(EngineEvent::SearchStarted(s));
    r.send(PlayMessage::Quit(soon(0)));
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(1));
    r.start(cfg);
    r.event(EngineEvent::BestMove(BestMove {
        search: s,
        mv: Some(e4()),
        ponder: None,
    }));
    assert!(r.mainline().is_empty(), "the new game is the player's turn");
    assert_eq!(r.view().phase, PlayPhase::Player);
}

// ---- refused commands, stopped engine ----

#[test]
fn a_refused_go_pauses_the_game_with_the_engine_lost() {
    let (mut r, _) = started(ColorChoice::Black, TimeMode::Depth(1));
    r.event(EngineEvent::Warning(EngineWarning::CommandRefused {
        command: "go",
        reason: "queue full".to_owned(),
    }));
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: true });
    assert_eq!(r.notice(), Some(PlayNotice::EngineLost));
}

#[test]
fn the_user_stopping_the_engine_from_the_engines_window_pauses_the_game() {
    let (mut r, _) = started(ColorChoice::Black, TimeMode::Depth(1));
    let run = r.run;
    r.send(EngineMessage::Stop(run));
    // The session confirms the stop asynchronously.
    r.send(EngineMessage::Stopped(run));
    assert!(r.state.play_live(), "paused, not over");
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: true });
    assert_eq!(r.state.session.game().tags().result, GameResult::Unknown);
}

#[test]
fn the_engine_dying_while_the_player_thinks_is_noticed_at_once() {
    let (mut r, _) = started(ColorChoice::White, TimeMode::Depth(1));
    r.event(exited());
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: true });
    assert!(r.view().engine_lost);
    // The player can still look at the game and quit.
    r.send(GameMessage::Navigate(Nav::Start));
    r.send(PlayMessage::Quit(soon(0)));
    assert!(!r.state.play_live());
}

#[test]
fn the_engine_dying_before_the_game_begins_aborts_it_with_a_notice() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(1));
    cfg.book = Some(mychess_app::BookChoice {
        path: std::path::PathBuf::from("none.bin"),
        selection: mychess_app::BookSelection::Best,
    });
    r.start(cfg);
    assert!(r.state.play_live(), "waiting for the book");
    r.event(exited());
    assert!(!r.state.play_live());
    assert_eq!(r.notice(), Some(PlayNotice::NoEngine));
}

#[test]
fn a_dead_engine_while_a_question_waits_pauses_without_losing_the_game() {
    let mut r = Rig::new();
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(1));
    cfg.start = StartFrom::Opening(mychess_app::OpeningLine::parse("A: e4 e5").unwrap());
    r.start(cfg);
    r.player_move("d2", "d4", soon(0));
    assert!(matches!(r.view().phase, PlayPhase::Asking(_)));
    r.event(exited());
    assert_eq!(r.view().phase, PlayPhase::Paused { lost: true });
    assert!(r.mainline().is_empty(), "the pending move was not played");
    r.send(PlayMessage::Resign(soon(0)));
    assert!(!r.state.play_live());
}

// ---- the end through the editor ----

#[test]
fn a_repetition_reaches_the_editor_with_its_result_and_comment() {
    let (mut r, _) = started(ColorChoice::White, TimeMode::Depth(1));
    let cycle = [("g1", "f3", "g8f6"), ("f3", "g1", "f6g8")];
    let t = soon(0);
    for _ in 0..2 {
        for (from, to, reply) in cycle {
            let fx = r.player_move(from, to, t);
            if play_search(&fx).is_some() {
                r.reply(&fx, Some(reply));
            }
        }
    }
    assert!(!r.state.play_live());
    let game = r.state.session.game();
    assert_eq!(game.tags().result, GameResult::Draw);
    let last = game.mainline().last().unwrap();
    assert_eq!(game.comment(last).unwrap(), Some("3 fold repetition"));
    assert_eq!(
        r.notice(),
        Some(PlayNotice::Ended(PlayEnd::ThreefoldRepetition))
    );
    assert!(r.state.is_dirty());
}

// ---- the analysis does not drive the engine of the game (D1241) ----

#[test]
fn no_infinite_search_is_sent_to_the_engine_while_the_game_lives() {
    let mut r = Rig::new();
    // The engine was analysing before the game.
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(1));
    let mut all = r.start(cfg);
    all.extend(r.player_move("e2", "e4", soon(0)));
    all.extend(r.send(GameMessage::Navigate(Nav::Start)));
    all.extend(r.send(GameMessage::Navigate(Nav::GameEnd)));
    all.extend(r.send(PlayMessage::Resume(soon(0))));
    let cmds = commands(&all);
    assert!(
        !cmds.iter().any(is_infinite_search),
        "the analysis must not drive the engine of the game: {cmds:?}"
    );
}

#[test]
fn the_engine_can_analyse_again_after_the_game() {
    let mut r = Rig::new();
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(1));
    r.start(cfg);
    r.send(PlayMessage::Quit(soon(0)));
    // Moves in the editor are free again, and nothing panics.
    r.send(GameMessage::PlayMove {
        mv: e4(),
        options: mychess_app::PlayOptions::default(),
    });
    assert_eq!(r.mainline(), ["e2e4"]);
}

// ---- the position on the board (D1236) ----

#[test]
fn the_cursor_position_of_the_editor_is_the_start_not_the_end_of_the_game() {
    let mut r = Rig::new();
    let mut game = Game::new();
    let m1 = game.add_move(game.root(), e4()).unwrap();
    let after = game.position_at(m1).unwrap();
    let e5 = after.parse_uci("e7e5").unwrap();
    game.add_move(m1, e5).unwrap();
    r.state.session.replace_game(game);
    r.send(GameMessage::Navigate(Nav::GameEnd));
    r.send(GameMessage::Navigate(Nav::Back(1)));
    let on_board = r.state.session.position().clone();
    assert_eq!(on_board.turn(), Color::Black);
    let mut cfg = r.config(ColorChoice::Black, TimeMode::Depth(1));
    cfg.start = StartFrom::CurrentPosition;
    let fx = r.start(cfg);
    assert!(r.state.play_live(), "{:?}", r.notice());
    assert_eq!(r.state.session.game().start_position(), &on_board);
    assert!(r.mainline().is_empty());
    assert!(play_search(&fx).is_none(), "Black (the player) moves first");
    assert_eq!(r.view().phase, PlayPhase::Player);
}

#[test]
fn a_game_cannot_start_from_a_position_that_is_already_over_without_a_clean_end() {
    // Fool's mate position on the board: the game starts and ends at once
    // with the result, never hangs waiting for a move.
    let mut r = Rig::new();
    r.state.session.replace_game(Game::from_position(
        Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
            .unwrap(),
    ));
    let mut cfg = r.config(ColorChoice::White, TimeMode::Depth(1));
    cfg.start = StartFrom::CurrentPosition;
    r.start(cfg);
    assert!(!r.state.play_live());
    assert_eq!(r.state.session.game().tags().result, GameResult::Black);
}

// ---- the analysis of other engines waits (D1241) ----

fn second_engine_analysing(r: &mut Rig) -> (mychess_app::engines::EngineId, RunId) {
    use mychess_app::engines::{EngineDraft, StartMode};
    let id = r
        .state
        .engines
        .add(EngineDraft::new("Other", "/other"))
        .unwrap();
    let fx = r.send(EngineMessage::Start(id, StartMode::Analyze));
    let run = fx
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("StartEngine");
    r.send(EngineMessage::Started {
        run,
        identity: Box::new(identity(Vec::new())),
        at: 1_700_000_002,
    });
    assert!(r.state.engine_runs.get(run).unwrap().analyzing);
    (id, run)
}

#[test]
fn the_analysis_of_another_engine_is_paused_during_the_game_and_back_after_it() {
    let mut r = Rig::new();
    // The engine of the game is idle, another one analyses.
    let (_, other) = second_engine_analysing(&mut r);
    let cfg = r.config(ColorChoice::White, TimeMode::Depth(1));
    let fx = r.start(cfg);
    assert!(
        !r.state.engine_runs.get(other).unwrap().analyzing,
        "the other analysis is paused"
    );
    assert!(
        fx.iter().any(|e| matches!(e,
            Effect::EngineCommand { run, command, .. }
                if *run == other && **command == EngineCommand::Stop)),
        "and stopped on the wire"
    );
    r.player_move("e2", "e4", soon(0));
    r.send(GameMessage::Navigate(Nav::Start));
    assert!(!r.state.engine_runs.get(other).unwrap().analyzing);
    let fx = r.send(PlayMessage::Quit(soon(0)));
    assert!(
        r.state.engine_runs.get(other).unwrap().analyzing,
        "the analysis is back after the game"
    );
    assert!(
        fx.iter().any(|e| matches!(e,
            Effect::EngineCommand { run, .. } if *run == other)),
        "the engine was told to analyse again"
    );
}
