//! Contract tests of the shoot-out, "Finish game" (spec M4-8, D492, D846 and
//! rules 5-7, 9-11; Scid `toggleFinishGame`, `analysis.tcl` 3228-3265).
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unnecessary_wraps,
    clippy::default_trait_access,
    clippy::assert_is_empty
)]

mod m4_8_common;

use std::time::Duration;

use m4_8_common::*;
use mychess_app::{
    Action, EngineMode, GameMessage, Message, ModeKind, ModeMessage, ModeNotice, Nav, StopReason,
};
use mychess_core::{Game, Move, Position};
use mychess_engine::{EngineEvent, SearchId};

fn start(rig: &mut Rig) -> Vec<mychess_app::Effect> {
    let run = rig.run;
    rig.mode(ModeMessage::ToggleFinishGame(run))
}

fn ended(kind: ModeKind, reason: StopReason) -> Option<ModeNotice> {
    Some(ModeNotice::Ended { kind, reason })
}

/// An engine that plays the scripted UCI moves in order.
fn script<'a>(
    ucis: &'a [&'a str],
) -> impl FnMut(&mychess_engine::SearchRequest, &Position) -> Option<Move> + 'a {
    let mut next = 0;
    move |_, pos| {
        let uci = ucis.get(next)?;
        next += 1;
        pos.parse_uci(uci).ok()
    }
}

#[test]
fn it_plays_the_engine_moves_one_step_per_best_move_and_asks_the_autoplay_delay() {
    let mut rig = Rig::new(Game::new());
    let delay = rig.state.session.autoplay().delay;
    let fx = start(&mut rig);
    assert!(matches!(
        rig.state.engine_mode(rig.run),
        EngineMode::ShootOut { .. }
    ));
    let first = last_search(&fx).expect("first step");
    assert_eq!(first.limits.movetime, Some(delay));
    assert!(!first.limits.infinite);
    assert!(first.position.moves.is_empty());

    let (seen, last) = rig.drive(fx, 10, &mut first_legal);

    assert_eq!(seen.len(), 10);
    assert_eq!(rig.mainline().len(), 10);
    // One request per step, each on the position the game reached.
    for (i, request) in seen.iter().enumerate() {
        assert_eq!(request.position.moves.len(), i, "step {i}");
    }
    assert!(mode_searches(&last).len() <= 1);
    assert!(matches!(
        rig.state.engine_mode(rig.run),
        EngineMode::ShootOut { .. }
    ));
    assert_eq!(rig.mode_notice(), None);
}

#[test]
fn the_step_uses_the_current_autoplay_delay() {
    let mut rig = Rig::new(Game::new());
    rig.game_msg(GameMessage::SetAutoplayDelay(Duration::from_millis(250)));
    let fx = start(&mut rig);
    assert_eq!(
        last_search(&fx).expect("search").limits.movetime,
        Some(Duration::from_millis(250))
    );
}

#[test]
fn it_never_asks_a_second_search_before_the_best_move() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    let request = last_search(&fx).expect("search");
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(300),
        tag: request.tag,
    });
    let fx = rig.event(EngineEvent::SearchStarted(SearchId(300)));
    assert!(mode_searches(&fx).is_empty());
    assert!(rig.mainline().is_empty());
}

#[test]
fn from_the_middle_of_the_line_it_starts_a_variation_and_keeps_the_main_line() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "g1f3"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    rig.game_msg(GameMessage::Navigate(Nav::Back(1)));
    let main = rig.mainline();
    let nodes = rig.game().node_count();
    let fx = start(&mut rig);
    let first = last_search(&fx).expect("search");
    assert_eq!(first.position.moves.len(), 2, "from the cursor");

    let engine_move = mv(&position_of(&first), "b1c3");
    let fx = rig.answer(&fx, Some(engine_move)).expect("answered");

    assert_eq!(rig.mainline(), main, "the main line is not replaced");
    assert_eq!(rig.game().node_count(), nodes + 1);
    let next = last_search(&fx).expect("next step");
    let ucis: Vec<String> = {
        let mut p = next.position.start.clone();
        next.position
            .moves
            .iter()
            .map(|m| {
                let u = p.uci(*m);
                p.play(*m).expect("legal");
                u
            })
            .collect()
    };
    assert_eq!(ucis, ["e2e4", "e7e5", "b1c3"]);
}

#[test]
fn at_the_end_of_the_line_it_extends_the_main_line() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let fx = start(&mut rig);
    drop(rig.drive(fx, 3, &mut first_legal));
    let main = rig.mainline();
    assert_eq!(main.len(), 5);
    assert_eq!(&main[..2], ["e2e4", "e7e5"]);
}

#[test]
fn it_ends_when_the_engine_mates() {
    let mut rig = Rig::fen("rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 2");
    let before = rig.game().tags().result;
    let fx = start(&mut rig);
    let (seen, last) = rig.drive(fx, 5, &mut script(&["d8h4"]));
    assert_eq!(seen.len(), 1);
    assert!(
        mode_searches(&last).is_empty(),
        "no question in a terminal position"
    );
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::Checkmate)
    );
    assert_eq!(
        rig.game().tags().result,
        before,
        "the result is not written"
    );
    assert_eq!(rig.mainline(), ["d8h4"]);
}

#[test]
fn it_does_not_ask_the_engine_in_a_position_that_is_already_over() {
    let mut rig = Rig::fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3");
    let fx = start(&mut rig);
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::Checkmate)
    );
}

#[test]
fn it_ends_in_stalemate() {
    let mut rig = Rig::fen("7k/8/6K1/8/8/8/8/5Q2 w - - 0 1");
    let fx = start(&mut rig);
    rig.drive(fx, 5, &mut script(&["f1f7"]));
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::Stalemate)
    );
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
}

#[test]
fn it_ends_in_insufficient_material() {
    let mut rig = Rig::fen("4k3/8/8/8/8/8/3p4/4K3 w - - 0 1");
    let fx = start(&mut rig);
    rig.drive(fx, 5, &mut script(&["e1d2"]));
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::InsufficientMaterial)
    );
}

#[test]
fn it_ends_by_the_fifty_move_rule() {
    let mut rig = Rig::fen("4k3/8/8/8/8/8/4P3/4K3 w - - 99 80");
    let fx = start(&mut rig);
    let (seen, _) = rig.drive(fx, 5, &mut script(&["e1d1"]));
    assert_eq!(seen.len(), 1);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::FiftyMoves)
    );
}

#[test]
fn it_ends_by_threefold_repetition_using_the_game_history() {
    let mut rig = Rig::new(game_of(&["g1f3", "g8f6", "f3g1", "f6g8"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let fx = start(&mut rig);
    let (seen, _) = rig.drive(fx, 10, &mut script(&["g1f3", "g8f6", "f3g1", "f6g8"]));
    assert_eq!(seen.len(), 4, "the 3rd occurrence ends it, not before");
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::Repetition)
    );
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
}

#[test]
fn a_none_best_move_without_warning_ends_with_no_move() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    let fx = rig.answer(&fx, None).expect("answer");
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::NoMove)
    );
    assert!(rig.mainline().is_empty());
}

#[test]
fn an_illegal_best_move_is_asked_again_once_and_then_it_is_an_engine_error() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    let fx = rig.answer_with(&fx, None, true).expect("first answer");
    assert_eq!(mode_searches(&fx).len(), 1, "asks once more");
    assert!(matches!(
        rig.state.engine_mode(rig.run),
        EngineMode::ShootOut { .. }
    ));
    let fx = rig.answer_with(&fx, None, true).expect("second answer");
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::EngineError)
    );
}

#[test]
fn a_move_the_game_does_not_accept_counts_as_illegal() {
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let bad = mv(&Position::new(), "e2e4"); // no pawn on e2 any more
    let fx = start(&mut rig);
    let fx = rig.answer(&fx, Some(bad)).expect("first");
    assert_eq!(mode_searches(&fx).len(), 1, "one more try");
    let fx = rig.answer(&fx, Some(bad)).expect("second");
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::EngineError)
    );
    assert_eq!(rig.mainline(), ["e2e4"], "nothing was played");
}

#[test]
fn the_retry_allowance_comes_back_after_a_good_move() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    let fx = rig.answer_with(&fx, None, true).expect("bad");
    let legal = mv(&Position::new(), "e2e4");
    let fx = rig.answer(&fx, Some(legal)).expect("good");
    let fx = rig.answer_with(&fx, None, true).expect("bad again");
    assert_eq!(mode_searches(&fx).len(), 1, "a new retry is allowed");
    assert!(matches!(
        rig.state.engine_mode(rig.run),
        EngineMode::ShootOut { .. }
    ));
}

#[test]
fn the_engine_moves_are_one_undo_step() {
    let mut rig = Rig::new(Game::new());
    let nodes = rig.game().node_count();
    let fx = start(&mut rig);
    rig.drive(fx, 6, &mut first_legal);
    rig.mode(ModeMessage::StopAll);
    assert_eq!(rig.mainline().len(), 6);
    rig.game_msg(GameMessage::Undo);
    assert_eq!(rig.game().node_count(), nodes, "one undo reverts the game");
}

#[test]
fn esc_stops_it_silently_and_a_late_best_move_is_not_played() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    let request = last_search(&fx).expect("search");
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(310),
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(310)));

    let fx = rig.send(Message::Action(Action::ClearMoveEntry));

    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(rig.state.notice, None, "stopping is not reported");
    assert!(count_stops(&fx) >= 1, "the search in flight is stopped");
    let late = mv(&Position::new(), "e2e4");
    let fx = rig.event(EngineEvent::BestMove(mychess_engine::BestMove {
        search: SearchId(310),
        mv: Some(late),
        ponder: None,
    }));
    assert!(rig.mainline().is_empty());
    assert!(mode_searches(&fx).is_empty());
}

#[test]
fn stop_all_and_the_button_stop_it_without_a_notice() {
    for use_button in [false, true] {
        let mut rig = Rig::new(Game::new());
        start(&mut rig);
        if use_button {
            start(&mut rig);
        } else {
            rig.mode(ModeMessage::StopAll);
        }
        assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
        assert_eq!(rig.state.notice, None, "button: {use_button}");
    }
}

#[test]
fn stopping_gives_the_board_analysis_back() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    let fx = rig.mode(ModeMessage::StopAll);
    let after_more = rig.mode(ModeMessage::SetAutomoveTime(
        rig.run,
        Duration::from_secs(2),
    ));
    let all: Vec<_> = searches(&fx)
        .into_iter()
        .chain(searches(&after_more))
        .collect();
    assert!(
        all.iter().any(|s| s.limits.movetime.is_none()),
        "the board analysis returns after the shoot-out: {all:?}"
    );
}

#[test]
fn a_move_of_the_user_ends_it_as_game_changed() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    rig.play("e2e4");
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::GameChanged)
    );
}

#[test]
fn navigating_ends_it_as_game_changed() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let fx = start(&mut rig);
    rig.drive(fx, 2, &mut first_legal);
    rig.game_msg(GameMessage::Navigate(Nav::Back(1)));
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::GameChanged)
    );
}

#[test]
fn loading_another_game_ends_it_as_game_changed() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    rig.state.session.replace_game(game_of(&["d2d4"]));
    rig.mode(ModeMessage::SetAutomoveTime(
        rig.run,
        Duration::from_secs(1),
    ));
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::GameChanged)
    );
}

#[test]
fn entering_trial_mode_ends_it() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    rig.game_msg(GameMessage::EnterTrial { null_move: false });
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
}

#[test]
fn a_dead_engine_ends_it_with_an_engine_error() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    rig.answer(&fx, Some(mv(&Position::new(), "e2e4")));
    rig.event(exited());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(
        rig.mode_notice(),
        ended(ModeKind::ShootOut, StopReason::EngineError)
    );
    assert_eq!(rig.mainline(), ["e2e4"]);
}

#[test]
fn closing_the_engine_ends_it() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    let run = rig.run;
    rig.send(Message::Engine(
        mychess_app::engines::EngineMessage::Stopped(run),
    ));
    assert_eq!(rig.state.engine_mode(run), EngineMode::Idle);
    assert!(rig.mode_notice().is_some());
}

#[test]
fn it_cannot_start_with_a_dead_engine() {
    let mut rig = Rig::new(Game::new());
    rig.event(exited());
    let fx = start(&mut rig);
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
}

#[test]
fn a_best_move_for_an_unknown_search_is_ignored() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    let legal = mv(&Position::new(), "e2e4");
    let fx = rig.event(EngineEvent::BestMove(mychess_engine::BestMove {
        search: SearchId(9999),
        mv: Some(legal),
        ponder: None,
    }));
    assert!(rig.mainline().is_empty());
    assert!(mode_searches(&fx).is_empty());
    assert!(matches!(
        rig.state.engine_mode(rig.run),
        EngineMode::ShootOut { .. }
    ));
}

#[test]
fn a_duplicate_best_move_is_ignored() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    let legal = mv(&Position::new(), "e2e4");
    rig.answer(&fx, Some(legal));
    let sent = rig.event(EngineEvent::BestMove(mychess_engine::BestMove {
        search: SearchId(101),
        mv: Some(legal),
        ponder: None,
    }));
    assert_eq!(rig.mainline(), ["e2e4"]);
    assert!(mode_searches(&sent).is_empty());
}

#[test]
fn a_tick_of_an_old_generation_does_nothing() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    let run = rig.run;
    let fx = rig.mode(ModeMessage::ModeTick { run, generation: 0 });
    assert!(mode_searches(&fx).is_empty());
    let fx = rig.mode(ModeMessage::ModeTick {
        run,
        generation: u64::MAX,
    });
    assert!(mode_searches(&fx).is_empty());
}

#[test]
fn it_survives_a_game_with_hundreds_of_moves() {
    // A pseudo-random game of up to 300 plies, deterministic.
    let mut game = Game::new();
    let mut node = game.root();
    let mut pos = Position::new();
    let mut seed = 0x2545_F491_4F6C_DD1D_u64;
    for _ in 0..300 {
        let moves = pos.legal_moves();
        if moves.is_empty() {
            break;
        }
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let m = moves[usize::try_from(seed >> 33).expect("fits") % moves.len()];
        pos.play(m).expect("legal");
        node = game.add_move(node, m).expect("legal");
        if mychess_app::terminal_reason(&pos, &[]).is_some() {
            break;
        }
    }
    let mut rig = Rig::new(game);
    let plies = rig.mainline().len();
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let fx = start(&mut rig);
    if let Some(request) = last_search(&fx) {
        assert_eq!(request.position.moves.len(), plies);
    }
}

#[test]
fn only_search_effects_and_timers_come_out_of_the_steps() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    let (_, last) = rig.drive(fx, 4, &mut first_legal);
    for e in &last {
        let name = format!("{e:?}");
        assert!(
            name.starts_with("EngineCommand") || name.starts_with("Schedule"),
            "unexpected effect {name}"
        );
    }
}
