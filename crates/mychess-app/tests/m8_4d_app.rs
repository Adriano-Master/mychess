//! M8-4d (spec D2130-D2159): the second opinion of an engine, playing the
//! exercise out against the engine, the question about the unsaved game at
//! the start, the filter of the Tactics flag and the count of the mates
//! beyond the limit. The engine itself is answered by hand, except in the
//! test that runs the fake engine of the project.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m3_4a_common;
mod m4_3_common;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mychess_app::engines::EngineDraft;
use mychess_app::tactics::{
    Cancel, FindQuery, OpinionJob, OpinionVerdict, SourceKind, TacticsConfig, TacticsEffect,
    TacticsMessage, TacticsMode, TacticsNotice, TacticsPhase, find_next, open_file,
};
use mychess_app::{
    AppState, DiscardChoice, Effect, GameMessage, Message, PlayOptions, RequestId, perform, update,
};
use mychess_core::Position;

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
/// White to move; the answer of the game is the push `e4`.
const PUSH: &str = "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1";

fn fen_game(event: &str, fen: &str, moves: &str) -> String {
    format!("[Event \"{event}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n{moves} *\n\n")
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write");
    path
}

struct Rig {
    state: AppState,
    dir: tempfile::TempDir,
    /// The questions to the engine, kept for the test to answer.
    asked: Vec<(RequestId, Box<OpinionJob>)>,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().expect("dir");
        let prefs = dir.path().join("preferences.toml");
        Rig {
            state: AppState::with_preferences_path(Some(prefs)),
            dir,
            asked: Vec::new(),
        }
    }

    fn with_engine(mut self) -> Rig {
        self.state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .expect("engine");
        self
    }

    async fn pump(&mut self, effects: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::Tactics(TacticsEffect::Opinion { request, job }) => {
                    self.asked.push((request, job));
                }
                Effect::Tactics(_) => {
                    let answer = perform(effect).await;
                    queue.extend(update(&mut self.state, answer));
                }
                _ => {}
            }
        }
    }

    async fn send(&mut self, message: Message) {
        let effects = update(&mut self.state, message);
        self.pump(effects).await;
    }

    async fn tactics(&mut self, message: TacticsMessage) {
        self.send(Message::Tactics(message)).await;
    }

    async fn start(&mut self, config: TacticsConfig) {
        self.tactics(TacticsMessage::Start {
            config: Box::new(config),
            seed: 1,
        })
        .await;
    }

    async fn play(&mut self, from: &str, to: &str) {
        let mv = self
            .state
            .session
            .position()
            .find_move(from.parse().unwrap(), to.parse().unwrap(), None)
            .expect("legal");
        self.send(Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }))
        .await;
    }

    fn phase(&self) -> TacticsPhase {
        self.state.tactics_phase()
    }
}

fn best_move_config(file: &Path) -> TacticsConfig {
    TacticsConfig {
        mode: TacticsMode::BestMove,
        file: Some(file.to_path_buf()),
        second_opinion: true,
        ..TacticsConfig::default()
    }
}

// ---- (4) the question about the game (D2145) ----

#[tokio::test]
async fn an_unsaved_game_is_asked_about_and_cancel_changes_nothing() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1, ""));
    rig.play("e2", "e4").await;
    assert!(rig.state.is_dirty());
    rig.tactics(TacticsMessage::Open).await;
    rig.start(TacticsConfig {
        file: Some(file),
        ..TacticsConfig::default()
    })
    .await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert!(rig.state.discard_prompt.is_some());
    assert!(
        rig.state.tactics_dialog_open(),
        "the dialog waits under the question"
    );
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel))
        .await;
    assert!(rig.state.discard_prompt.is_none());
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert!(rig.state.tactics_dialog_open());
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(rig.state.is_dirty());
}

#[tokio::test]
async fn dont_save_starts_and_the_game_comes_back_unsaved_at_the_end() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1, ""));
    rig.play("e2", "e4").await;
    rig.start(TacticsConfig {
        file: Some(file),
        ..TacticsConfig::default()
    })
    .await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.session.game().tags().event, "Tactics");
    // Nothing was lost: closing gives the move back, still unsaved.
    rig.tactics(TacticsMessage::Close).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(rig.state.is_dirty());
}

#[tokio::test]
async fn a_clean_game_starts_without_a_question() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1, ""));
    rig.start(TacticsConfig {
        file: Some(file),
        ..TacticsConfig::default()
    })
    .await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert!(rig.state.discard_prompt.is_none());
}

// ---- (1) the second opinion (D2130-D2139) ----

async fn push_problem(rig: &mut Rig, config: impl FnOnce(&Path) -> TacticsConfig) {
    let file = write(rig.dir.path(), "p.pgn", &fen_game("push", PUSH, "1. e4"));
    let config = config(&file);
    rig.start(config).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
}

#[tokio::test]
async fn without_the_option_a_different_move_is_wrong_at_once() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, |f| TacticsConfig {
        second_opinion: false,
        ..best_move_config(f)
    })
    .await;
    rig.play("e2", "e3").await;
    assert_eq!(rig.asked.len(), 0);
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.tactics_view().attempts_left, 2);
}

#[tokio::test]
async fn the_move_of_the_game_is_never_put_to_the_engine() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, best_move_config).await;
    rig.play("e2", "e4").await;
    assert_eq!(rig.asked.len(), 0);
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)));
}

#[tokio::test]
async fn a_different_move_waits_for_the_engine_and_equivalent_solves_it() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, best_move_config).await;
    rig.play("e2", "e3").await;
    assert_eq!(rig.phase(), TacticsPhase::Judging);
    let (request, job) = rig.asked.pop().expect("the question");
    assert_eq!(job.position, Position::from_fen(PUSH).unwrap());
    assert_ne!(job.user, job.best);
    assert!(!job.cancel.is_cancelled());
    // The board is locked while the engine thinks.
    rig.send(Message::Game(GameMessage::Navigate(
        mychess_app::Nav::GameEnd,
    )))
    .await;
    assert_eq!(rig.state.tactics_view().notice, Some(TacticsNotice::Locked));
    rig.tactics(TacticsMessage::Opinion {
        request,
        verdict: Box::new(OpinionVerdict::Equivalent),
    })
    .await;
    assert_eq!(
        rig.phase(),
        TacticsPhase::Solved(mychess_app::tactics::ProblemOutcome::Perfect)
    );
    assert_eq!(
        rig.state.tactics_view().attempts_left,
        3,
        "no attempt was spent"
    );
    assert_eq!(
        rig.state.session.game().mainline().count(),
        1,
        "the move of the user is in the game"
    );
}

#[tokio::test]
async fn a_worse_move_costs_an_attempt() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, best_move_config).await;
    rig.play("e2", "e3").await;
    let (request, _) = rig.asked.pop().unwrap();
    rig.tactics(TacticsMessage::Opinion {
        request,
        verdict: Box::new(OpinionVerdict::Worse),
    })
    .await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.state.tactics_view().attempts_left, 2);
    assert_eq!(
        rig.state.session.game().mainline().count(),
        0,
        "the move stayed out"
    );
}

#[tokio::test]
async fn an_engine_that_did_not_answer_counts_as_wrong_and_says_so() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, best_move_config).await;
    rig.play("e2", "e3").await;
    let (request, _) = rig.asked.pop().unwrap();
    rig.tactics(TacticsMessage::Opinion {
        request,
        verdict: Box::new(OpinionVerdict::Unavailable),
    })
    .await;
    assert_eq!(rig.state.tactics_view().attempts_left, 2);
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::OpinionUnavailable)
    );
}

#[tokio::test]
async fn a_stale_answer_is_ignored() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, best_move_config).await;
    rig.play("e2", "e3").await;
    rig.tactics(TacticsMessage::Opinion {
        request: RequestId(424_242),
        verdict: Box::new(OpinionVerdict::Equivalent),
    })
    .await;
    assert_eq!(rig.phase(), TacticsPhase::Judging, "still waiting");
}

#[tokio::test]
async fn closing_and_the_watchdog_stop_the_question() {
    for by_watchdog in [false, true] {
        let mut rig = Rig::new().with_engine();
        push_problem(&mut rig, best_move_config).await;
        rig.play("e2", "e3").await;
        let (_, job) = rig.asked.pop().unwrap();
        if by_watchdog {
            let t0 = Instant::now();
            for i in 0..40u64 {
                rig.tactics(TacticsMessage::Tick(t0 + Duration::from_secs(i * 2)))
                    .await;
            }
            assert_eq!(rig.phase(), TacticsPhase::Solving, "back to the problem");
            assert_eq!(
                rig.state.tactics_view().notice,
                Some(TacticsNotice::TimedOut)
            );
        } else {
            rig.tactics(TacticsMessage::Close).await;
            assert_eq!(rig.phase(), TacticsPhase::Closed);
        }
        assert!(job.cancel.is_cancelled(), "watchdog = {by_watchdog}");
    }
}

#[tokio::test]
async fn without_an_engine_in_the_list_the_start_says_so_and_judging_is_the_game_alone() {
    let mut rig = Rig::new();
    push_problem(&mut rig, best_move_config).await;
    assert!(rig.state.tactics_view().opinion_missing);
    rig.play("e2", "e3").await;
    assert_eq!(rig.asked.len(), 0);
    assert!(rig.state.tactics_view().opinion_missing, "it stays on view");
    assert_eq!(rig.state.tactics_view().attempts_left, 2);
}

#[tokio::test]
async fn mate_in_n_never_asks_the_engine() {
    let mut rig = Rig::new().with_engine();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("m", MATE1, ""));
    rig.start(TacticsConfig {
        mode: TacticsMode::Mate,
        file: Some(file),
        second_opinion: true,
        ..TacticsConfig::default()
    })
    .await;
    rig.play("g1", "f1").await;
    assert_eq!(rig.asked.len(), 0);
}

#[tokio::test]
async fn the_fake_engine_answers_a_real_question() {
    let Some(binary) = m4_3_common::fake_binary() else {
        return;
    };
    let position = Position::from_fen(PUSH).unwrap();
    let job = OpinionJob {
        engine: mychess_app::tactics::OpinionEngine(mychess_engine::EngineConfig::new(binary)),
        user: position.parse_uci("e2e3").unwrap(),
        best: position.parse_uci("e2e4").unwrap(),
        position,
        cancel: Cancel::new(),
    };
    let verdict = tokio::time::timeout(
        Duration::from_secs(30),
        mychess_app::tactics::ask_opinion(&job),
    )
    .await
    .expect("it ends");
    assert_ne!(verdict, OpinionVerdict::Unavailable);
}

#[tokio::test]
async fn a_question_that_was_cancelled_is_unavailable_and_ends_the_process() {
    let Some(binary) = m4_3_common::fake_binary() else {
        return;
    };
    let position = Position::from_fen(PUSH).unwrap();
    let cancel = Cancel::new();
    cancel.cancel();
    let job = OpinionJob {
        engine: mychess_app::tactics::OpinionEngine(mychess_engine::EngineConfig::new(binary)),
        user: position.parse_uci("e2e3").unwrap(),
        best: position.parse_uci("e2e4").unwrap(),
        position,
        cancel,
    };
    assert_eq!(
        mychess_app::tactics::ask_opinion(&job).await,
        OpinionVerdict::Unavailable
    );
}

// ---- (2) playing the exercise out (D2140-D2144) ----

#[tokio::test]
async fn after_solving_the_game_goes_on_against_the_engine_from_the_position() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, |f| TacticsConfig {
        second_opinion: false,
        ..best_move_config(f)
    })
    .await;
    rig.play("e2", "e4").await;
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)));
    assert!(rig.state.tactics_view().can_continue);
    let after = rig.state.session.position().clone();
    rig.tactics(TacticsMessage::Continue).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert!(
        rig.state.play_live(),
        "a game against the engine was started"
    );
    assert_eq!(
        *rig.state.session.position(),
        after,
        "from where the exercise ended"
    );
}

#[tokio::test]
async fn a_lost_problem_can_be_played_out_too() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, |f| TacticsConfig {
        second_opinion: false,
        ..best_move_config(f)
    })
    .await;
    rig.tactics(TacticsMessage::ShowSolution).await;
    assert!(matches!(rig.phase(), TacticsPhase::Failed(_)));
    assert!(rig.state.tactics_view().can_continue);
    rig.tactics(TacticsMessage::Continue).await;
    assert!(rig.state.play_live());
}

#[tokio::test]
async fn a_delivered_mate_leaves_nothing_to_play() {
    let mut rig = Rig::new().with_engine();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("m", MATE1, ""));
    rig.start(TacticsConfig {
        file: Some(file),
        ..TacticsConfig::default()
    })
    .await;
    rig.play("a1", "a8").await;
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)));
    assert!(!rig.state.tactics_view().can_continue);
    rig.tactics(TacticsMessage::Continue).await;
    assert!(
        matches!(rig.phase(), TacticsPhase::Solved(_)),
        "nothing happened"
    );
    assert!(!rig.state.play_live());
}

#[tokio::test]
async fn without_an_engine_continue_says_so_and_keeps_the_session() {
    let mut rig = Rig::new();
    push_problem(&mut rig, |f| TacticsConfig {
        second_opinion: false,
        ..best_move_config(f)
    })
    .await;
    rig.play("e2", "e4").await;
    rig.tactics(TacticsMessage::Continue).await;
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)));
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::NoEngineToPlay)
    );
    assert!(!rig.state.play_live());
}

#[tokio::test]
async fn continue_is_for_a_problem_that_is_over() {
    let mut rig = Rig::new().with_engine();
    push_problem(&mut rig, |f| TacticsConfig {
        second_opinion: false,
        ..best_move_config(f)
    })
    .await;
    assert!(!rig.state.tactics_view().can_continue);
    rig.tactics(TacticsMessage::Continue).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert!(!rig.state.play_live());
}

#[tokio::test]
async fn continue_never_lets_go_of_a_game_set_aside_with_unsaved_changes() {
    let mut rig = Rig::new().with_engine();
    rig.play("e2", "e4").await;
    let file = write(rig.dir.path(), "p.pgn", &fen_game("push", PUSH, "1. e4"));
    rig.start(TacticsConfig {
        second_opinion: false,
        ..best_move_config(&file)
    })
    .await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    rig.play("e2", "e4").await;
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)));
    rig.tactics(TacticsMessage::Continue).await;
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)), "refused");
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::ParkedUnsaved)
    );
    assert!(!rig.state.play_live());
    // Closing gives the game back, unsaved.
    rig.tactics(TacticsMessage::Close).await;
    assert!(rig.state.is_dirty());
    assert_eq!(rig.state.session.game().mainline().count(), 1);
}

// ---- (5) the Tactics flag (D2150-D2152) ----

fn flagged_base(dir: &Path) -> PathBuf {
    use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};
    let pgn = fen_game("plain", MATE1, "") + &fen_game("flagged", PUSH, "1. e4");
    let si4 = m3_4a_common::make_si4(dir, "flags", &pgn);
    let mut writer = Si4Writer::open(&si4).expect("opens for writing");
    writer
        .set_flag(2, GameFlags::TACTICS, FlagOp::Set)
        .expect("flag");
    drop(writer);
    si4
}

fn query(config: TacticsConfig) -> FindQuery {
    FindQuery {
        config: Box::new(config),
        start: 0,
        step: 1,
        k: 0,
        max_games: 10,
        solved: std::sync::Arc::new(Vec::new()),
        stamp: None,
        cancel: Cancel::new(),
    }
}

#[test]
fn only_the_games_with_the_tactics_flag_are_taken_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let opened = open_file(&flagged_base(dir.path())).unwrap();
    let best = TacticsConfig {
        mode: TacticsMode::BestMove,
        ..TacticsConfig::default()
    };
    let all = find_next(&opened.handle, &query(best.clone()));
    assert_eq!(all.found.unwrap().problem.game_number, 1);
    let flagged = find_next(
        &opened.handle,
        &query(TacticsConfig {
            flagged_only: true,
            ..best
        }),
    );
    assert_eq!(
        flagged.found.expect("the flagged game").problem.game_number,
        2
    );
    assert_eq!(
        flagged.examined, 2,
        "the unflagged game was looked at and left"
    );
}

#[test]
fn a_database_without_flagged_games_gives_nothing_and_a_pgn_ignores_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = m3_4a_common::make_si4(dir.path(), "plain", &fen_game("plain", MATE1, ""));
    let opened = open_file(&si4).unwrap();
    let config = TacticsConfig {
        mode: TacticsMode::BestMove,
        flagged_only: true,
        ..TacticsConfig::default()
    };
    assert!(
        find_next(&opened.handle, &query(config.clone()))
            .found
            .is_none()
    );
    let pgn = write(dir.path(), "p.pgn", &fen_game("plain", MATE1, ""));
    let opened = open_file(&pgn).unwrap();
    assert!(
        find_next(&opened.handle, &query(config)).found.is_some(),
        "no flags in a PGN file: the filter does not apply"
    );
}

// ---- (6) the mates beyond the limit (D2155) ----

/// A position of king and queen against king where White mates in exactly
/// four moves (the solver says so), found by looking.
fn a_mate_in_four() -> String {
    use mychess_core::mate::{MateLimits, MateResult, find_mate};
    let files = "abcdefgh";
    for black_king in 0..64usize {
        for queen in 0..64usize {
            let sq = |i: usize| format!("{}{}", &files[i % 8..=i % 8], i / 8 + 1);
            let (bk, q) = (sq(black_king), sq(queen));
            if bk == q || bk == "a1" || q == "a1" {
                continue;
            }
            // White king on a1, as a FEN built from the board.
            let mut board = [[' '; 8]; 8];
            board[0][0] = 'K';
            board[queen / 8][queen % 8] = 'Q';
            board[black_king / 8][black_king % 8] = 'k';
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
                return fen;
            }
        }
    }
    panic!("no mate in four of king and queen was found");
}

#[test]
fn problems_with_a_longer_mate_than_the_limit_are_counted() {
    let fen = a_mate_in_four();
    let dir = tempfile::tempdir().unwrap();
    let pgn = write(dir.path(), "p.pgn", &fen_game("long", &fen, ""));
    let opened = open_file(&pgn).unwrap();
    let wide = find_next(
        &opened.handle,
        &query(TacticsConfig {
            mate_max: 5,
            ..TacticsConfig::default()
        }),
    );
    assert_eq!(
        wide.found.expect("found with the wide limit").mate_moves,
        Some(4)
    );
    assert_eq!(wide.beyond_ids.len(), 0);
    let narrow = find_next(
        &opened.handle,
        &query(TacticsConfig {
            mate_max: 3,
            ..TacticsConfig::default()
        }),
    );
    assert!(
        narrow.found.is_none(),
        "a mate in four is out of a limit of three"
    );
    assert_eq!(narrow.beyond_ids.len(), 1, "but the window can say so");
}

#[tokio::test]
async fn the_count_reaches_the_view() {
    let mut rig = Rig::new();
    let fen = a_mate_in_four();
    let file = write(
        rig.dir.path(),
        "p.pgn",
        &(fen_game("long", &fen, "") + &fen_game("short", MATE1, "")),
    );
    rig.start(TacticsConfig {
        file: Some(file),
        source: SourceKind::File,
        mate_max: 3,
        ..TacticsConfig::default()
    })
    .await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    // The first game is a mate in four and was passed by, and counted.
    assert_eq!(rig.state.tactics_view().mate_in, Some(1));
    assert_eq!(rig.state.tactics_view().beyond_limit, 1);
}
