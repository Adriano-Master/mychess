//! Regression tests of the review of the M8-4d (spec D2158-D2189): the game
//! that "Don't save" parks is never lost, "Continue" refuses before it lets
//! the session go, the count of mates beyond the limit counts a problem
//! once, the time of the engine is not the time of the user, and a question
//! to the engine does not outlive the session.
#![allow(
    clippy::large_futures, // the online rig owns the whole `AppState` across awaits
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

mod m3_4a_common;
mod m4_3_common;
mod m8_3_common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use m3_4a_common as slots;
use mychess_app::engines::EngineDraft;
use mychess_app::online::{OnlineMessage, OnlinePhase, PlayingState};
use mychess_app::tactics::{
    Cancel, OpinionEngine, OpinionJob, OpinionVerdict, TacticsConfig, TacticsEffect,
    TacticsMessage, TacticsMode, TacticsNotice, TacticsPhase, ask_opinion,
};
use mychess_app::{
    BasesMessage, Command, DiscardChoice, Effect, GameMessage, Message, PlayOptions,
};
use mychess_core::Position;
use mychess_engine::Protocol;

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
const PUSH: &str = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";

fn fen_game(event: &str, fen: &str, moves: &str) -> String {
    format!("[Event \"{event}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n{moves} *\n\n")
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write");
    path
}

fn config(file: &Path) -> TacticsConfig {
    TacticsConfig {
        file: Some(file.to_path_buf()),
        ..TacticsConfig::default()
    }
}

fn two_problems(dir: &Path) -> PathBuf {
    write(
        dir,
        "p.pgn",
        &(fen_game("one", MATE1, "") + &fen_game("two", PUSH, "1. e4")),
    )
}

fn start(rig: &mut slots::Rig, config: TacticsConfig) {
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config),
        seed: 1,
    }));
}

fn plies(rig: &slots::Rig) -> usize {
    rig.state.session.game().mainline().count()
}

/// A user game with a move that is not saved, a session started over it
/// with "Don't save": the state the review found the hole in.
fn parked_dirty(rig: &mut slots::Rig, dir: &Path) {
    rig.play("e2e4");
    assert!(rig.state.is_dirty());
    start(rig, config(&two_problems(dir)));
    assert!(rig.state.discard_prompt.is_some());
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.session.game().tags().event, "Tactics");
}

fn the_game_of_the_user_is_back_and_unsaved(rig: &slots::Rig) {
    assert_eq!(plies(rig), 1, "the move of the user");
    assert!(rig.state.is_dirty(), "still marked as not saved");
    assert_ne!(rig.state.session.game().tags().event, "Tactics");
}

// ---- 1. the game parked with "Don't save" (D2158, D2159) ----

#[test]
fn new_game_during_a_session_asks_about_the_game_of_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    parked_dirty(&mut rig, dir.path());
    rig.send(Message::Command(Command::NewGame));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert!(rig.state.discard_prompt.is_some(), "the normal question");
    the_game_of_the_user_is_back_and_unsaved(&rig);
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    the_game_of_the_user_is_back_and_unsaved(&rig);
}

#[test]
fn loading_a_game_from_the_list_asks_about_the_game_of_the_user() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    rig.open(&slots::write(dir.path(), "a.pgn", slots::TWO));
    parked_dirty(&mut rig, dir.path());
    rig.send(BasesMessage::LoadGame {
        slot: slots::sel(1),
        number: 2,
    });
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert!(rig.state.discard_prompt.is_some());
    the_game_of_the_user_is_back_and_unsaved(&rig);
}

#[test]
fn switching_slots_keeps_the_game_of_the_user_unsaved() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    rig.open(&slots::write(dir.path(), "a.pgn", slots::TWO));
    rig.open(&slots::write(dir.path(), "b.pgn", slots::TWO));
    rig.select(1);
    parked_dirty(&mut rig, dir.path());
    rig.select(2);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    rig.select(1);
    the_game_of_the_user_is_back_and_unsaved(&rig);
}

#[test]
fn the_watchdog_gives_the_unsaved_game_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    parked_dirty(&mut rig, dir.path());
    // The search of the next problem is asked for and never answered.
    drop(rig.raw(Message::Tactics(TacticsMessage::Next)));
    let t0 = Instant::now();
    for i in 0..40u64 {
        drop(rig.raw(Message::Tactics(TacticsMessage::Tick(
            t0 + Duration::from_secs(i * 2),
        ))));
    }
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    the_game_of_the_user_is_back_and_unsaved(&rig);
}

#[test]
fn closing_gives_the_unsaved_game_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    parked_dirty(&mut rig, dir.path());
    rig.send(Message::Tactics(TacticsMessage::Close));
    the_game_of_the_user_is_back_and_unsaved(&rig);
}

#[tokio::test]
async fn an_online_game_keeps_the_unsaved_game_of_the_user_for_afterwards() {
    use m8_3_common::{Rig, finished};
    let mut rig = Rig::new(1).await;
    let e4 = rig
        .state
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .unwrap();
    rig.send(Message::Game(GameMessage::PlayMove {
        mv: e4,
        options: PlayOptions::default(),
    }))
    .await;
    let file = two_problems(rig.dir.path());
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(config(&file)),
        seed: 1,
    }))
    .await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);

    rig.start_game().await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    rig.online(OnlineMessage::DismissResult).await;
    rig.online(OnlineMessage::RestorePrevious).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(
        rig.state.is_dirty(),
        "the move of the user is still unsaved"
    );
    assert_ne!(rig.state.session.game().tags().event, "Tactics");
}

// ---- 2. "Continue" refuses before it lets the session go (D2162) ----

fn solved_push(rig: &mut slots::Rig, dir: &Path) {
    let file = write(dir, "push.pgn", &fen_game("push", PUSH, "1. e4"));
    start(
        rig,
        TacticsConfig {
            mode: TacticsMode::BestMove,
            ..config(&file)
        },
    );
    rig.play("e2e4");
    assert!(matches!(rig.state.tactics_phase(), TacticsPhase::Solved(_)));
}

fn add_engine(rig: &mut slots::Rig, name: &str, protocol: Protocol) {
    let mut draft = EngineDraft::new(name, format!("/{name}"));
    draft.protocol = protocol;
    rig.state.engines.add(draft).expect("engine");
}

#[test]
fn an_xboard_engine_cannot_play_the_game_and_the_session_stays() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    add_engine(&mut rig, "Old", Protocol::XBoard);
    solved_push(&mut rig, dir.path());
    rig.send(Message::Tactics(TacticsMessage::Continue));
    assert!(matches!(rig.state.tactics_phase(), TacticsPhase::Solved(_)));
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::NoEngineToPlay),
        "its own text, not the one of the second opinion"
    );
    assert!(!rig.state.play_live());
}

#[test]
fn the_uci_engine_is_chosen_over_an_xboard_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    add_engine(&mut rig, "Old", Protocol::XBoard);
    add_engine(&mut rig, "New", Protocol::Uci);
    solved_push(&mut rig, dir.path());
    let effects = rig.raw(Message::Tactics(TacticsMessage::Continue));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert!(rig.state.play_live());
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::StartEngine { .. })),
        "the game starts the engine that speaks UCI"
    );
}

#[test]
fn a_variation_being_tried_keeps_the_session_and_says_it_is_busy() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    add_engine(&mut rig, "New", Protocol::Uci);
    solved_push(&mut rig, dir.path());
    rig.send(Message::Game(GameMessage::EnterTrial { null_move: false }));
    assert!(rig.state.session.in_trial());
    rig.send(Message::Tactics(TacticsMessage::Continue));
    assert!(matches!(rig.state.tactics_phase(), TacticsPhase::Solved(_)));
    assert_eq!(rig.state.tactics_view().notice, Some(TacticsNotice::Busy));
    assert!(!rig.state.play_live());
}

// ---- 4. the count of the mates beyond the limit, once each (D2164) ----

#[test]
fn a_problem_that_comes_back_on_the_next_lap_is_counted_once() {
    // A mate in four (found by the solver) and a mate in one.
    let fen = {
        use mychess_core::mate::{MateLimits, MateResult, find_mate};
        let files = "abcdefgh";
        let mut found = None;
        'search: for bk in 0..64usize {
            for q in 0..64usize {
                if bk == q || bk == 0 || q == 0 {
                    continue;
                }
                let mut board = [[' '; 8]; 8];
                board[0][0] = 'K';
                board[q / 8][q % 8] = 'Q';
                board[bk / 8][bk % 8] = 'k';
                let rows: Vec<String> = (0..8)
                    .rev()
                    .map(|r| {
                        let mut text = String::new();
                        let mut empty = 0;
                        for c in board[r] {
                            if c == ' ' {
                                empty += 1;
                            } else {
                                if empty > 0 {
                                    text.push_str(&empty.to_string());
                                    empty = 0;
                                }
                                text.push(c);
                            }
                        }
                        if empty > 0 {
                            text.push_str(&empty.to_string());
                        }
                        text
                    })
                    .collect();
                let fen = format!("{} w - - 0 1", rows.join("/"));
                let Ok(position) = Position::from_fen(&fen) else {
                    continue;
                };
                let limits = MateLimits {
                    max_moves: 5,
                    max_nodes: 100_000,
                };
                if matches!(
                    find_mate(&position, limits),
                    MateResult::Mate { moves: 4, .. }
                ) {
                    found = Some(fen);
                    break 'search;
                }
            }
        }
        let _ = files;
        found.expect("a mate in four")
    };
    let dir = tempfile::tempdir().unwrap();
    let file = write(
        dir.path(),
        "p.pgn",
        &(fen_game("long", &fen, "") + &fen_game("short", MATE1, "")),
    );
    let mut rig = slots::Rig::new();
    start(
        &mut rig,
        TacticsConfig {
            mate_max: 3,
            include_solved: true,
            ..config(&file)
        },
    );
    assert_eq!(rig.state.tactics_view().beyond_limit, 1);
    // Round and round: the same problem is met again and again.
    for _ in 0..4 {
        rig.send(Message::Tactics(TacticsMessage::Next));
        assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
        assert_eq!(rig.state.tactics_view().beyond_limit, 1);
    }
}

// ---- 8. the time of the engine is not the time of the user (D2166) ----

#[test]
fn the_time_of_the_engine_counts_neither_for_the_score_nor_for_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = slots::Rig::new();
    add_engine(&mut rig, "New", Protocol::Uci);
    let file = write(dir.path(), "push.pgn", &fen_game("push", PUSH, "1. e4"));
    start(
        &mut rig,
        TacticsConfig {
            mode: TacticsMode::BestMove,
            second_opinion: true,
            time_limit_s: 5,
            ..config(&file)
        },
    );
    // A different move: the question goes out and nobody answers it yet.
    let e3 = rig
        .state
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e3".parse().unwrap(), None)
        .unwrap();
    let effects = rig.raw(Message::Game(GameMessage::PlayMove {
        mv: e3,
        options: PlayOptions::default(),
    }));
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::Tactics(TacticsEffect::Opinion { request, .. }) => Some(*request),
            _ => None,
        })
        .expect("the question");
    let t0 = Instant::now();
    for i in 0..20u64 {
        drop(rig.raw(Message::Tactics(TacticsMessage::Tick(
            t0 + Duration::from_secs(i),
        ))));
    }
    assert_eq!(
        rig.state.tactics_phase(),
        TacticsPhase::Judging,
        "20 s of engine"
    );
    assert_eq!(rig.state.tactics_view().elapsed_s, 0);
    drop(rig.raw(Message::Tactics(TacticsMessage::Opinion {
        request,
        verdict: Box::new(OpinionVerdict::Worse),
    })));
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(
        rig.state.tactics_view().elapsed_s,
        0,
        "the limit is still whole"
    );
    // The time of the user runs again.
    for i in 20..24u64 {
        drop(rig.raw(Message::Tactics(TacticsMessage::Tick(
            t0 + Duration::from_secs(i),
        ))));
    }
    assert!(rig.state.tactics_view().elapsed_s >= 2);
}

// ---- 7, 9. the question to the engine does not outlive the session ----

#[tokio::test]
async fn a_question_in_the_air_is_cancelled_when_an_online_game_starts() {
    use m8_3_common::Rig;
    let mut rig = Rig::new(1).await;
    rig.state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("engine");
    let file = write(rig.dir.path(), "push.pgn", &fen_game("push", PUSH, "1. e4"));
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(TacticsConfig {
            mode: TacticsMode::BestMove,
            second_opinion: true,
            ..config(&file)
        }),
        seed: 1,
    }))
    .await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    let e3 = rig
        .state
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e3".parse().unwrap(), None)
        .unwrap();
    let effects = rig.update_only(Message::Game(GameMessage::PlayMove {
        mv: e3,
        options: PlayOptions::default(),
    }));
    let cancel = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::Tactics(TacticsEffect::Opinion { job, .. }) => Some(job.cancel),
            _ => None,
        })
        .expect("the question");
    assert!(!cancel.is_cancelled());
    rig.start_game().await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert!(cancel.is_cancelled(), "the engine is told to stop");
}

#[tokio::test]
async fn a_cancel_while_the_engine_starts_ends_the_question_at_once() {
    let Some(binary) = m4_3_common::fake_binary() else {
        return;
    };
    let position = Position::from_fen(PUSH).unwrap();
    let cancel = Cancel::new();
    let job = OpinionJob {
        engine: OpinionEngine(
            mychess_engine::EngineConfig::new(binary).with_args(["--uciok-delay-ms", "10000"]),
        ),
        user: position.parse_uci("e2e3").unwrap(),
        best: position.parse_uci("e2e4").unwrap(),
        position,
        cancel: cancel.clone(),
    };
    let started = Instant::now();
    let asking = tokio::spawn(async move { ask_opinion(&job).await });
    tokio::time::sleep(Duration::from_millis(400)).await;
    cancel.cancel();
    let verdict = tokio::time::timeout(Duration::from_secs(5), asking)
        .await
        .expect("it did not wait for the engine")
        .expect("no panic");
    assert_eq!(verdict, OpinionVerdict::Unavailable);
    assert!(started.elapsed() < Duration::from_secs(5));
}
