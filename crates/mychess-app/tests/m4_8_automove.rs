//! Contract tests of the automove, "Training" (spec M4-8, D493, D844, D845,
//! D847 and rules 8-10; Scid `toggleAutomove`/`automove`/`automoveGo`,
//! `analysis.tcl` 4233-4327).
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
    Action, AutomoveState, DEFAULT_AUTOMOVE, EngineMode, GameMessage, Message, ModeKind,
    ModeMessage, ModeNotice, Nav, StopReason,
};
use mychess_core::{Game, Position};
use mychess_engine::{BestMove, EngineEvent, SearchId};

fn start(rig: &mut Rig) -> Vec<mychess_app::Effect> {
    let run = rig.run;
    rig.mode(ModeMessage::ToggleAutomove(run))
}

fn ended(reason: StopReason) -> Option<ModeNotice> {
    Some(ModeNotice::Ended {
        kind: ModeKind::Automove,
        reason,
    })
}

fn state_of(rig: &Rig) -> Option<AutomoveState> {
    match rig.state.engine_mode(rig.run) {
        EngineMode::Automove { state, .. } => Some(state),
        _ => None,
    }
}

#[test]
fn turning_it_on_makes_the_engine_play_the_current_side_with_the_default_time() {
    let mut rig = Rig::new(Game::new());
    assert_eq!(rig.state.automove_time(), DEFAULT_AUTOMOVE);
    let fx = start(&mut rig);
    let request = last_search(&fx).expect("the engine moves at once");
    assert_eq!(request.limits.movetime, Some(Duration::from_secs(3)));
    assert!(!request.limits.infinite);
    assert_eq!(
        rig.state.engine_mode(rig.run),
        EngineMode::Automove {
            generation: match rig.state.engine_mode(rig.run) {
                EngineMode::Automove { generation, .. } => generation,
                _ => 0,
            },
            thinking_ms: 3000,
            state: AutomoveState::EngineThinking,
        }
    );
}

#[test]
fn after_the_engine_moves_it_waits_for_the_user_without_another_search() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    let fx = rig
        .answer(&fx, Some(mv(&Position::new(), "e2e4")))
        .expect("answer");
    assert_eq!(rig.mainline(), ["e2e4"]);
    assert_eq!(state_of(&rig), Some(AutomoveState::UserToMove));
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.mode_notice(), None);
}

#[test]
fn each_move_of_the_user_starts_a_cycle_that_ends_with_the_engine_move() {
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    rig.answer(&fx, Some(mv(&Position::new(), "e2e4")));

    let fx = rig.play("e7e5");
    let request = last_search(&fx).expect("the engine answers the user");
    assert_eq!(state_of(&rig), Some(AutomoveState::EngineThinking));
    assert_eq!(request.limits.movetime, Some(Duration::from_secs(3)));
    assert_eq!(request.position.moves.len(), 2);

    rig.answer(&fx, Some(mv(&position_of(&request), "g1f3")));
    assert_eq!(rig.mainline(), ["e2e4", "e7e5", "g1f3"]);
    assert_eq!(state_of(&rig), Some(AutomoveState::UserToMove));
}

#[test]
fn set_time_is_clamped_and_used_by_the_next_move() {
    let mut rig = Rig::new(Game::new());
    let run = rig.run;
    let cases = [
        (Duration::from_millis(10), Duration::from_millis(100)),
        (Duration::ZERO, Duration::from_millis(100)),
        (Duration::from_secs(1000), Duration::from_secs(300)),
        (Duration::from_millis(2500), Duration::from_millis(2500)),
        (Duration::from_millis(100), Duration::from_millis(100)),
        (Duration::from_secs(300), Duration::from_secs(300)),
    ];
    for (given, kept) in cases {
        rig.mode(ModeMessage::SetAutomoveTime(run, given));
        assert_eq!(rig.state.automove_time(), kept, "{given:?}");
    }
    rig.mode(ModeMessage::SetAutomoveTime(
        run,
        Duration::from_millis(2500),
    ));
    let fx = start(&mut rig);
    assert_eq!(
        last_search(&fx).expect("search").limits.movetime,
        Some(Duration::from_millis(2500))
    );
}

#[test]
fn a_move_that_is_not_there_is_retried_every_second_at_most_ten_times() {
    let mut rig = Rig::new(Game::new());
    let run = rig.run;
    let mut fx = start(&mut rig);
    let mut requests = 1;
    let mut generations = Vec::new();
    for _ in 0..20 {
        fx = rig.answer(&fx, None).expect("answer");
        let timers = ticks(&fx);
        if timers.is_empty() {
            break;
        }
        assert_eq!(timers.len(), 1);
        let (r, generation, delay) = timers[0];
        assert_eq!((r, delay), (run, Duration::from_secs(1)));
        generations.push(generation);
        fx = rig.mode(ModeMessage::ModeTick { run, generation });
        assert_eq!(mode_searches(&fx).len(), 1, "the tick asks again");
        requests += 1;
    }
    assert_eq!(requests, 11, "the first ask plus ten retries");
    assert_eq!(rig.state.engine_mode(run), EngineMode::Idle);
    assert_eq!(rig.mode_notice(), ended(StopReason::NoMove));
    let mut unique = generations.clone();
    unique.dedup();
    assert_eq!(
        unique.len(),
        generations.len(),
        "each retry is a new generation"
    );
}

#[test]
fn illegal_best_moves_end_with_an_engine_error_after_the_retries() {
    let mut rig = Rig::new(Game::new());
    let run = rig.run;
    let mut fx = start(&mut rig);
    for _ in 0..30 {
        fx = rig.answer_with(&fx, None, true).expect("answer");
        let Some(&(_, generation, _)) = ticks(&fx).first() else {
            break;
        };
        fx = rig.mode(ModeMessage::ModeTick { run, generation });
    }
    assert_eq!(rig.state.engine_mode(run), EngineMode::Idle);
    assert_eq!(rig.mode_notice(), ended(StopReason::EngineError));
}

#[test]
fn a_retry_that_works_resets_the_counter_and_plays() {
    let mut rig = Rig::new(Game::new());
    let run = rig.run;
    let mut fx = start(&mut rig);
    for _ in 0..5 {
        fx = rig.answer(&fx, None).expect("answer");
        let (_, generation, _) = ticks(&fx)[0];
        fx = rig.mode(ModeMessage::ModeTick { run, generation });
    }
    rig.answer(&fx, Some(mv(&Position::new(), "d2d4")));
    assert_eq!(rig.mainline(), ["d2d4"]);
    assert_eq!(state_of(&rig), Some(AutomoveState::UserToMove));
}

#[test]
fn a_tick_of_an_old_generation_or_without_a_pending_retry_is_ignored() {
    let mut rig = Rig::new(Game::new());
    let run = rig.run;
    let fx = start(&mut rig);
    let fx = rig.answer(&fx, None).expect("answer");
    let (_, generation, _) = ticks(&fx)[0];
    for stale in [generation.wrapping_sub(1), generation + 1, 0, u64::MAX] {
        let fx = rig.mode(ModeMessage::ModeTick {
            run,
            generation: stale,
        });
        assert!(mode_searches(&fx).is_empty(), "generation {stale}");
    }
    // The right one still works, and only once.
    let fx = rig.mode(ModeMessage::ModeTick { run, generation });
    assert_eq!(mode_searches(&fx).len(), 1);
    let fx = rig.mode(ModeMessage::ModeTick { run, generation });
    assert!(
        mode_searches(&fx).is_empty(),
        "a second tick finds a search in flight"
    );
}

#[test]
fn a_tick_with_no_mode_does_nothing() {
    let mut rig = Rig::new(Game::new());
    let run = rig.run;
    let fx = rig.mode(ModeMessage::ModeTick { run, generation: 1 });
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.notice, None);
}

#[test]
fn navigating_cancels_the_pending_cycle_and_the_next_user_move_restarts_it() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let fx = start(&mut rig);
    let request = last_search(&fx).expect("thinking");
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(400),
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(400)));

    let fx = rig.game_msg(GameMessage::Navigate(Nav::Back(1)));

    assert!(count_stops(&fx) >= 1, "the search is stopped");
    assert_eq!(state_of(&rig), Some(AutomoveState::UserToMove), "still on");
    assert_eq!(rig.mode_notice(), None);
    // The late move of the cancelled search is not played.
    let late = rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(400),
        mv: Some(mv(&position_of(&request), "g1f3")),
        ponder: None,
    }));
    assert_eq!(rig.mainline(), ["e2e4", "e7e5"]);
    assert!(mode_searches(&late).is_empty());
    // The user plays again: a new cycle.
    let fx = rig.play("e7e5");
    let again = last_search(&fx).expect("restarted by the user's move");
    assert_eq!(again.position.moves.len(), 2);
    assert_eq!(state_of(&rig), Some(AutomoveState::EngineThinking));
}

#[test]
fn editing_a_comment_while_it_thinks_cancels_the_cycle() {
    // Conservative decision D847 (implementer's note).
    let mut rig = Rig::new(game_of(&["e2e4"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    let fx = start(&mut rig);
    let request = last_search(&fx).expect("thinking");
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(410),
        tag: request.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(410)));
    let node = rig.state.session.cursor();
    let fx = rig.game_msg(GameMessage::SetComment {
        node,
        text: "hello".to_owned(),
    });
    assert!(count_stops(&fx) >= 1);
    assert_eq!(state_of(&rig), Some(AutomoveState::UserToMove));
    rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(410),
        mv: Some(mv(&position_of(&request), "e7e5")),
        ponder: None,
    }));
    assert_eq!(rig.mainline(), ["e2e4"]);
}

#[test]
fn the_move_of_a_replaced_search_is_not_taken_for_the_new_one() {
    // The user moves twice before the engine answers: the first search is
    // replaced and its (late) best move belongs to an older generation.
    let mut rig = Rig::new(Game::new());
    let fx = start(&mut rig);
    rig.answer(&fx, Some(mv(&Position::new(), "e2e4")));
    let first = rig.play("e7e5");
    let s1 = last_search(&first).expect("s1");
    let second = rig.play("g1f3");
    let s2 = last_search(&second).expect("s2");
    assert_ne!(s1.tag, s2.tag);
    // Events as the engine session would deliver them.
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(701),
        tag: s1.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(701)));
    rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(701),
        mv: Some(mv(&position_of(&s1), "g1f3")),
        ponder: None,
    }));
    rig.event(EngineEvent::SearchTagged {
        search: SearchId(702),
        tag: s2.tag,
    });
    rig.event(EngineEvent::SearchStarted(SearchId(702)));
    let fx = rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(702),
        mv: Some(mv(&position_of(&s2), "b8c6")),
        ponder: None,
    }));
    assert_eq!(
        rig.mainline(),
        ["e2e4", "e7e5", "g1f3", "b8c6"],
        "the answer of the current search is played at once; effects {fx:?}"
    );
}

#[test]
fn it_ends_when_the_engine_mates() {
    let mut rig = Rig::fen("rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 2");
    let fx = start(&mut rig);
    let fx = rig
        .answer(&fx, Some(mv(rig.position(), "d8h4")))
        .expect("answer");
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(rig.mode_notice(), ended(StopReason::Checkmate));
}

#[test]
fn it_ends_when_the_move_of_the_user_mates_without_asking_the_engine() {
    let mut rig = Rig::fen("rnbqkbnr/pppp1ppp/8/4p3/8/5P2/PPPPP1PP/RNBQKBNR w KQkq - 0 2");
    let fx = start(&mut rig);
    rig.answer(&fx, Some(mv(rig.position(), "g2g4")));
    let fx = rig.play("d8h4");
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(rig.mode_notice(), ended(StopReason::Checkmate));
}

#[test]
fn it_ends_in_stalemate_after_the_engine_move() {
    let mut rig = Rig::fen("7k/8/6K1/8/8/8/8/5Q2 w - - 0 1");
    let fx = start(&mut rig);
    rig.answer(&fx, Some(mv(rig.position(), "f1f7")));
    assert_eq!(rig.mode_notice(), ended(StopReason::Stalemate));
}

#[test]
fn starting_in_a_finished_game_reports_the_reason_and_asks_nothing() {
    let mut rig = Rig::fen("8/8/4k3/8/8/3K4/8/8 w - - 0 1");
    let fx = start(&mut rig);
    assert!(mode_searches(&fx).is_empty());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(rig.mode_notice(), ended(StopReason::InsufficientMaterial));
}

#[test]
fn the_button_and_esc_stop_it_silently_and_stop_the_search() {
    for esc in [false, true] {
        let mut rig = Rig::new(Game::new());
        let fx = start(&mut rig);
        let request = last_search(&fx).expect("search");
        rig.event(EngineEvent::SearchTagged {
            search: SearchId(420),
            tag: request.tag,
        });
        rig.event(EngineEvent::SearchStarted(SearchId(420)));
        let fx = if esc {
            rig.send(Message::Action(Action::ClearMoveEntry))
        } else {
            start(&mut rig)
        };
        assert_eq!(
            rig.state.engine_mode(rig.run),
            EngineMode::Idle,
            "esc {esc}"
        );
        assert_eq!(rig.state.notice, None, "esc {esc}");
        assert!(count_stops(&fx) >= 1, "esc {esc}");
        rig.event(EngineEvent::BestMove(BestMove {
            search: SearchId(420),
            mv: Some(mv(&Position::new(), "e2e4")),
            ponder: None,
        }));
        assert!(rig.mainline().is_empty(), "esc {esc}");
    }
}

#[test]
fn a_dead_engine_turns_it_off_with_an_engine_error() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    rig.event(exited());
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(rig.mode_notice(), ended(StopReason::EngineError));
    let fx = start(&mut rig);
    assert!(mode_searches(&fx).is_empty(), "cannot start a dead engine");
}

#[test]
fn loading_another_game_turns_it_off() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    rig.state.session.replace_game(game_of(&["d2d4"]));
    rig.mode(ModeMessage::SetAutomoveTime(
        rig.run,
        Duration::from_secs(1),
    ));
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
    assert_eq!(rig.mode_notice(), ended(StopReason::GameChanged));
}

#[test]
fn entering_trial_mode_turns_it_off() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    rig.game_msg(GameMessage::EnterTrial { null_move: false });
    assert_eq!(rig.state.engine_mode(rig.run), EngineMode::Idle);
}

#[test]
fn the_best_move_of_an_unknown_search_is_ignored() {
    let mut rig = Rig::new(Game::new());
    start(&mut rig);
    rig.event(EngineEvent::BestMove(BestMove {
        search: SearchId(31337),
        mv: Some(mv(&Position::new(), "e2e4")),
        ponder: None,
    }));
    assert!(rig.mainline().is_empty());
    assert_eq!(state_of(&rig), Some(AutomoveState::EngineThinking));
}

#[test]
fn it_plays_in_the_middle_of_the_line_as_a_variation_without_asking() {
    let mut rig = Rig::new(game_of(&["e2e4", "e7e5", "g1f3"]));
    rig.game_msg(GameMessage::Navigate(Nav::End));
    rig.game_msg(GameMessage::Navigate(Nav::Back(2)));
    let main = rig.mainline();
    let fx = start(&mut rig);
    let request = last_search(&fx).expect("search");
    rig.answer(&fx, Some(mv(&position_of(&request), "c7c5")));
    assert_eq!(rig.mainline(), main, "main line intact");
    assert_eq!(state_of(&rig), Some(AutomoveState::UserToMove));
    assert!(
        rig.state.session.prompt().is_none(),
        "no question is left open"
    );
}
