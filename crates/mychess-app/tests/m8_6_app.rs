//! M8-6b (spec D2300-D2334): the calculation exercise in the application:
//! starting, typing the lines, evaluating, the result, the history, the
//! user's game, the exclusion with the other modes and the online lock. The
//! engine is answered by hand; `m8_6_engine.rs` runs the fake engine.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::large_futures,
    clippy::assert_is_empty
)]

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use mychess_app::calvar::{
    Assessment, CalvarConfig, CalvarEffect, CalvarFailure, CalvarMessage, CalvarNotice,
    CalvarPhase, EngineFailure, Eval, LineProbes, LinesJob, PositionSource, Probe, RootJob,
    RootLine, store_path,
};
use mychess_app::engines::EngineDraft;
use mychess_app::online::{OnlinePhase, PlayingState};
use mychess_app::{
    Action, AppState, Command, DiscardChoice, Effect, GameMessage, Message, PlayOptions, RequestId,
    perform, update,
};
use mychess_core::Position;

const MATED: &str = "7k/6Q1/6K1/8/8/8/8/8 b - - 0 1";

struct Rig {
    state: AppState,
    dir: tempfile::TempDir,
    root: Vec<(RequestId, Box<RootJob>)>,
    lines: Vec<(RequestId, Box<LinesJob>)>,
    clock: Instant,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().expect("dir");
        let prefs = dir.path().join("preferences.toml");
        let mut state = AppState::with_preferences_path(Some(prefs));
        state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .expect("engine");
        Rig {
            state,
            dir,
            root: Vec::new(),
            lines: Vec::new(),
            clock: Instant::now(),
        }
    }

    fn without_engine() -> Rig {
        let mut rig = Rig::new();
        rig.state = AppState::with_preferences_path(Some(rig.dir.path().join("preferences.toml")));
        rig
    }

    async fn pump(&mut self, effects: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::Calvar(CalvarEffect::AnalyzeRoot { request, job }) => {
                    self.root.push((request, job));
                }
                Effect::Calvar(CalvarEffect::AnalyzeLines { request, job }) => {
                    self.lines.push((request, job));
                }
                Effect::Calvar(_) | Effect::Tactics(_) => {
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

    async fn calvar(&mut self, message: CalvarMessage) {
        self.send(Message::Calvar(message)).await;
    }

    async fn start(&mut self, config: CalvarConfig) {
        self.calvar(CalvarMessage::Start {
            config: Box::new(config),
            now: 1_700_000_000,
        })
        .await;
    }

    async fn text(&mut self, text: &str) {
        self.calvar(CalvarMessage::EnterText(text.to_owned())).await;
    }

    async fn end(&mut self, a: Assessment) {
        self.calvar(CalvarMessage::EndLine(a)).await;
    }

    async fn tick(&mut self, seconds: u64) {
        for _ in 0..seconds / 2 {
            self.clock += Duration::from_secs(2);
            let now = self.clock;
            self.calvar(CalvarMessage::Tick(now)).await;
        }
    }

    fn phase(&self) -> CalvarPhase {
        self.state.calvar_phase()
    }

    fn notice(&self) -> Option<CalvarNotice> {
        self.state.calvar_view().notice
    }

    /// The engine's answer to the first stage: the lines `(uci, cp)` of the
    /// position being calculated.
    async fn answer_root(&mut self, lines: &[(&str, i32)]) {
        let (request, job) = self.root.pop().expect("a root job");
        let p = job.position.clone();
        let result = Ok(lines
            .iter()
            .enumerate()
            .map(|(i, (uci, cp))| RootLine {
                multipv: u8::try_from(i + 1).unwrap(),
                depth: 14,
                eval: Eval::centipawns(*cp),
                pv: vec![p.parse_uci(uci).unwrap()],
            })
            .collect());
        self.calvar(CalvarMessage::RootAnalyzed { request, result })
            .await;
    }

    async fn answer_lines(&mut self, first_cp: i32, end_cp: i32) {
        let (request, job) = self.lines.pop().expect("a lines job");
        let probes = job
            .lines
            .iter()
            .map(|l| LineProbes {
                first: Some(Probe {
                    eval: Eval::centipawns(first_cp),
                    depth: 12,
                    pv: Vec::new(),
                }),
                end: (l.len() > 1).then(|| Probe {
                    eval: Eval::centipawns(end_cp),
                    depth: 12,
                    pv: Vec::new(),
                }),
            })
            .collect();
        self.calvar(CalvarMessage::LinesAnalyzed {
            request,
            result: Ok(probes),
        })
        .await;
    }

    async fn play(&mut self, uci: &str) {
        let mv = self.state.session.position().parse_uci(uci).unwrap();
        self.send(Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }))
        .await;
    }
}

fn one_line() -> CalvarConfig {
    CalvarConfig {
        lines_required: 1,
        root_ms: 200,
        line_ms: 200,
        ..CalvarConfig::default()
    }
}

/// An exercise on the start position with one line `e4 e5` ready to judge.
async fn ready() -> Rig {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    rig.text("e4 e5").await;
    rig.end(Assessment::Equal).await;
    rig
}

// ---- starting (D2301, D2307) ----

#[tokio::test]
async fn starting_asks_the_engine_about_the_position_of_the_editor() {
    let mut rig = Rig::new();
    rig.calvar(CalvarMessage::Open).await;
    assert!(rig.state.calvar_dialog_open());
    rig.start(CalvarConfig {
        multipv: 4,
        root_ms: 5_000,
        ..one_line()
    })
    .await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert!(!rig.state.calvar_dialog_open());
    let (_, job) = &rig.root[0];
    assert_eq!(job.position.fen(), Position::new().fen());
    assert_eq!(job.multipv, 4);
    assert_eq!(job.think, Duration::from_millis(5_000));
    assert_eq!(rig.state.session.game().tags().event, "Calculation");
}

#[tokio::test]
async fn the_fen_of_the_dialog_is_the_position_of_the_exercise() {
    let mut rig = Rig::new();
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";
    rig.start(CalvarConfig {
        source: PositionSource::Fen,
        fen: fen.to_owned(),
        ..one_line()
    })
    .await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert_eq!(rig.root[0].1.position.fen(), fen);
}

#[tokio::test]
async fn a_bad_fen_or_a_position_without_moves_is_refused() {
    let mut rig = Rig::new();
    rig.start(CalvarConfig {
        source: PositionSource::Fen,
        fen: "not a fen".to_owned(),
        ..one_line()
    })
    .await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::BadFen));
    rig.start(CalvarConfig {
        source: PositionSource::Fen,
        fen: MATED.to_owned(),
        ..one_line()
    })
    .await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::NoMoves));
    assert!(rig.root.is_empty(), "no engine was asked");
    assert_ne!(
        rig.state.session.game().tags().event,
        "Calculation",
        "editor untouched"
    );
}

#[tokio::test]
async fn without_a_uci_engine_the_exercise_does_not_start() {
    let mut rig = Rig::without_engine();
    rig.start(one_line()).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::NoEngine));
}

#[tokio::test]
async fn a_second_start_over_a_live_exercise_changes_nothing() {
    let mut rig = ready().await;
    rig.start(one_line()).await;
    assert_eq!(rig.state.calvar_view().lines.len(), 1);
    assert_eq!(rig.root.len(), 1);
}

// ---- entering the lines (D2302-D2305, D2334) ----

#[tokio::test]
async fn typed_moves_make_a_line_without_moving_the_pieces() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    rig.text("1. e4 e5 2. Nf3").await;
    let view = rig.state.calvar_view();
    assert_eq!(view.current_san, "1. e4 e5 2. Nf3");
    assert_eq!(view.current_plies, 3);
    assert_eq!(
        rig.state.session.game().mainline().count(),
        0,
        "the board stays at the start position"
    );
    assert_eq!(rig.state.session.position().fen(), Position::new().fen());
}

#[tokio::test]
async fn an_illegal_move_keeps_the_ones_before_it_and_names_the_text() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    rig.text("e4 e5 Ke3").await;
    let view = rig.state.calvar_view();
    assert_eq!(view.current_plies, 2);
    assert_eq!(view.rejected.as_deref(), Some("Ke3"));
    assert_eq!(view.notice, Some(CalvarNotice::IllegalMove));
    rig.text("Nf3").await;
    assert_eq!(
        rig.state.calvar_view().rejected,
        None,
        "cleared by the next entry"
    );
}

#[tokio::test]
async fn undo_takes_back_one_move_and_an_empty_line_says_so() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    rig.calvar(CalvarMessage::Undo).await;
    assert_eq!(rig.notice(), Some(CalvarNotice::EmptyLine));
    rig.text("e4 e5").await;
    rig.calvar(CalvarMessage::Undo).await;
    assert_eq!(rig.state.calvar_view().current_san, "1. e4");
    rig.end(Assessment::Equal).await;
    assert_eq!(rig.state.calvar_view().lines.len(), 1);
    assert_eq!(rig.state.calvar_view().current_plies, 0);
    rig.end(Assessment::Equal).await;
    assert_eq!(rig.notice(), Some(CalvarNotice::EmptyLine));
}

#[tokio::test]
async fn a_finished_line_can_be_removed() {
    let mut rig = ready().await;
    rig.text("d4").await;
    rig.end(Assessment::WhiteSlightly).await;
    assert_eq!(rig.state.calvar_view().lines.len(), 2);
    rig.calvar(CalvarMessage::RemoveLine(0)).await;
    let view = rig.state.calvar_view();
    assert_eq!(view.lines.len(), 1);
    assert_eq!(view.lines[0].san, "1. d4");
    assert_eq!(view.lines[0].assessment, Assessment::WhiteSlightly);
    rig.calvar(CalvarMessage::RemoveLine(9)).await;
    assert_eq!(
        rig.state.calvar_view().lines.len(),
        1,
        "stale index is ignored"
    );
}

#[tokio::test]
async fn a_line_has_at_most_forty_moves_and_an_exercise_twelve_lines() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    let shuffle = "Nf3 Nf6 Ng1 Ng8 ".repeat(12);
    rig.text(&shuffle).await;
    let view = rig.state.calvar_view();
    assert_eq!(view.current_plies, 40);
    assert_eq!(view.notice, Some(CalvarNotice::LineTooLong));
    rig.calvar(CalvarMessage::Undo).await;
    rig.calvar(CalvarMessage::Undo).await;
    for _ in 0..12 {
        rig.end(Assessment::Equal).await;
        rig.text("e4").await;
    }
    assert_eq!(rig.state.calvar_view().lines.len(), 12);
    rig.end(Assessment::Equal).await;
    assert_eq!(rig.notice(), Some(CalvarNotice::TooManyLines));
}

#[tokio::test]
async fn keyboard_entry_at_the_board_goes_to_the_exercise_not_to_the_game() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    for c in "e4".chars() {
        rig.send(Message::Game(GameMessage::MoveEntryChar(c))).await;
    }
    assert_eq!(rig.state.calvar_view().entry, "e4");
    rig.send(Message::Game(GameMessage::MoveEntryBackspace))
        .await;
    assert_eq!(rig.state.calvar_view().entry, "e");
    rig.send(Message::Game(GameMessage::MoveEntryChar('4')))
        .await;
    rig.send(Message::Game(GameMessage::MoveEntryCommit)).await;
    let view = rig.state.calvar_view();
    assert_eq!(view.current_san, "1. e4");
    assert_eq!(view.entry, "");
    assert_eq!(rig.state.session.game().mainline().count(), 0);
}

#[tokio::test]
async fn a_gesture_on_the_board_counts_when_it_is_legal_in_the_line() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    rig.play("e2e4").await;
    assert_eq!(rig.state.calvar_view().current_san, "1. e4");
    assert_eq!(
        rig.state.session.game().mainline().count(),
        0,
        "the board did not move"
    );
    // Black's reply cannot be made on a board that shows White to move.
    rig.text("e5").await;
    rig.play("g1f3").await;
    assert_eq!(
        rig.state.calvar_view().current_san,
        "1. e4 e5 2. Nf3",
        "a white move is legal again after two plies"
    );
    rig.play("e2e4").await;
    assert_eq!(rig.notice(), Some(CalvarNotice::UseKeyboard));
}

#[tokio::test]
async fn with_the_board_following_the_line_the_pieces_move() {
    let mut rig = Rig::new();
    rig.start(CalvarConfig {
        follow_board: true,
        ..one_line()
    })
    .await;
    rig.text("e4 e5").await;
    assert_eq!(rig.state.session.game().mainline().count(), 2);
    rig.calvar(CalvarMessage::Undo).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    // Black to move on the board: a black gesture is legal now.
    rig.play("c7c5").await;
    assert_eq!(rig.state.calvar_view().current_san, "1. e4 c5");
}

#[tokio::test]
async fn editing_the_game_is_refused_while_the_user_calculates() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    rig.send(Message::Game(GameMessage::TruncateEnd)).await;
    assert_eq!(rig.notice(), Some(CalvarNotice::Locked));
}

// ---- "Done" (D2305) ----

#[tokio::test]
async fn done_needs_the_lines_asked_for_and_none_in_progress() {
    let mut rig = Rig::new();
    rig.start(CalvarConfig {
        lines_required: 2,
        ..one_line()
    })
    .await;
    rig.calvar(CalvarMessage::Done).await;
    assert_eq!(rig.notice(), Some(CalvarNotice::NeedMoreLines));
    rig.text("e4").await;
    rig.calvar(CalvarMessage::Done).await;
    assert_eq!(rig.notice(), Some(CalvarNotice::LineOpen));
    rig.end(Assessment::Equal).await;
    rig.calvar(CalvarMessage::Done).await;
    assert_eq!(rig.notice(), Some(CalvarNotice::NeedMoreLines));
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    rig.text("d4").await;
    rig.end(Assessment::Equal).await;
    assert!(rig.state.calvar_view().can_done);
    rig.calvar(CalvarMessage::Done).await;
    assert_eq!(rig.phase(), CalvarPhase::Evaluating);
}

#[tokio::test]
async fn the_lines_asked_for_never_exceed_the_moves_of_the_position() {
    let mut rig = Rig::new();
    // Black has one legal move: Kh7.
    rig.start(CalvarConfig {
        source: PositionSource::Fen,
        fen: "7k/8/5K2/6Q1/8/8/8/8 b - - 0 1".to_owned(),
        lines_required: 5,
        ..one_line()
    })
    .await;
    assert_eq!(rig.state.calvar_view().required, 1);
}

// ---- the two stages (D2308, D2310) ----

#[tokio::test]
async fn the_second_stage_waits_for_the_first() {
    let mut rig = ready().await;
    rig.calvar(CalvarMessage::Done).await;
    assert_eq!(rig.phase(), CalvarPhase::Evaluating);
    assert!(rig.lines.is_empty(), "the root analysis is not in yet");
    rig.answer_root(&[("e2e4", 30), ("d2d4", 20)]).await;
    assert_eq!(rig.lines.len(), 1, "now the lines are sent");
    let job = &rig.lines[0].1;
    assert_eq!(job.lines.len(), 1);
    assert_eq!(job.lines[0].len(), 2);
    assert_eq!(job.per_search, Duration::from_millis(200));
}

#[tokio::test]
async fn with_the_first_stage_already_in_done_sends_the_lines_at_once() {
    let mut rig = ready().await;
    rig.answer_root(&[("e2e4", 30)]).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering, "keeps calculating");
    assert!(rig.state.calvar_view().root_ready);
    rig.calvar(CalvarMessage::Done).await;
    assert_eq!(rig.lines.len(), 1);
}

#[tokio::test]
async fn the_result_has_the_score_the_history_and_the_file() {
    let mut rig = ready().await;
    rig.answer_root(&[("e2e4", 30), ("d2d4", 20)]).await;
    rig.calvar(CalvarMessage::Done).await;
    rig.answer_lines(30, 20).await;
    let view = rig.state.calvar_view();
    let CalvarPhase::Reported(score) = view.phase else {
        panic!("{:?}", view.phase);
    };
    let report = view.report.expect("report");
    assert_eq!(report.score, score);
    assert_eq!((report.found, report.good_total), (1, 2));
    assert_eq!(view.stats.done, 1);
    assert!(view.arrow.is_some(), "the missed d4 is drawn");
    // The file was written beside the preferences.
    let path = store_path(&rig.dir.path().join("preferences.toml"));
    let text = std::fs::read_to_string(path).expect("calvar.toml");
    assert!(text.contains("[[history]]"), "{text}");
    assert!(text.contains(&format!("score = {score}")));
    assert!(text.contains("when = 1700000000"));
}

#[tokio::test]
async fn the_history_survives_a_new_session_of_the_program() {
    let mut rig = ready().await;
    rig.answer_root(&[("e2e4", 30)]).await;
    rig.calvar(CalvarMessage::Done).await;
    rig.answer_lines(30, 20).await;
    rig.calvar(CalvarMessage::Close).await;
    let prefs = rig.dir.path().join("preferences.toml");
    let mut second = AppState::with_preferences_path(Some(prefs));
    let effects = update(&mut second, Message::Calvar(CalvarMessage::Open));
    for effect in effects {
        if matches!(effect, Effect::Calvar(_)) {
            let answer = perform(effect).await;
            update(&mut second, answer);
        }
    }
    assert_eq!(second.calvar_view().stats.done, 1);
}

#[tokio::test]
async fn a_late_or_foreign_answer_is_ignored() {
    let mut rig = ready().await;
    rig.calvar(CalvarMessage::RootAnalyzed {
        request: RequestId(999),
        result: Ok(Vec::new()),
    })
    .await;
    assert_eq!(
        rig.phase(),
        CalvarPhase::Entering,
        "a stranger changes nothing"
    );
    rig.answer_root(&[("e2e4", 30)]).await;
    // A second answer to the same request is a stranger too.
    rig.calvar(CalvarMessage::RootAnalyzed {
        request: RequestId(1),
        result: Err(EngineFailure::Failed),
    })
    .await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    rig.calvar(CalvarMessage::Done).await;
    rig.calvar(CalvarMessage::LinesAnalyzed {
        request: RequestId(999),
        result: Ok(Vec::new()),
    })
    .await;
    assert_eq!(rig.phase(), CalvarPhase::Evaluating);
}

#[tokio::test]
async fn an_engine_that_fails_ends_the_exercise_without_losing_the_game() {
    let mut rig = Rig::new();
    rig.play("e2e4").await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    rig.start(one_line()).await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    let (request, _) = rig.root.pop().unwrap();
    rig.calvar(CalvarMessage::RootAnalyzed {
        request,
        result: Err(EngineFailure::NotStarted),
    })
    .await;
    assert_eq!(
        rig.phase(),
        CalvarPhase::Failed(CalvarFailure::EngineUnavailable)
    );
    assert_eq!(rig.notice(), Some(CalvarNotice::EngineFailed));
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(
        rig.state.session.game().mainline().count(),
        1,
        "the user's game is back"
    );
}

#[tokio::test]
async fn an_empty_answer_from_the_engine_is_a_failure_too() {
    let mut rig = ready().await;
    let (request, _) = rig.root.pop().unwrap();
    rig.calvar(CalvarMessage::RootAnalyzed {
        request,
        result: Ok(Vec::new()),
    })
    .await;
    assert_eq!(
        rig.phase(),
        CalvarPhase::Failed(CalvarFailure::EngineUnavailable)
    );
}

#[tokio::test]
async fn the_watchdog_drops_a_request_that_never_answers() {
    let mut rig = ready().await;
    rig.calvar(CalvarMessage::Done).await;
    rig.tick(40).await;
    assert_eq!(
        rig.phase(),
        CalvarPhase::Evaluating,
        "still within the budget"
    );
    rig.tick(80).await;
    assert_eq!(rig.phase(), CalvarPhase::Failed(CalvarFailure::TimedOut));
    assert_eq!(rig.notice(), Some(CalvarNotice::TimedOut));
    // The answer that comes too late is dropped.
    rig.answer_root(&[("e2e4", 30)]).await;
    assert_eq!(rig.phase(), CalvarPhase::Failed(CalvarFailure::TimedOut));
}

#[tokio::test]
async fn closing_cancels_the_jobs_and_makes_their_answers_stale() {
    let mut rig = ready().await;
    let cancel = rig.root[0].1.cancel.clone();
    assert!(!cancel.is_cancelled());
    rig.calvar(CalvarMessage::Close).await;
    assert!(cancel.is_cancelled());
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    rig.answer_root(&[("e2e4", 30)]).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
}

// ---- the time limit (D2306) ----

#[tokio::test]
async fn the_time_limit_evaluates_the_finished_lines_and_drops_the_open_one() {
    let mut rig = Rig::new();
    rig.start(CalvarConfig {
        time_limit_s: 30,
        lines_required: 3,
        ..one_line()
    })
    .await;
    rig.text("e4 e5").await;
    rig.end(Assessment::Equal).await;
    rig.text("d4").await;
    rig.answer_root(&[("e2e4", 30)]).await;
    rig.tick(20).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    rig.tick(20).await;
    assert_eq!(rig.phase(), CalvarPhase::Evaluating);
    assert_eq!(rig.lines.len(), 1);
    assert_eq!(rig.lines[0].1.lines.len(), 1, "only the finished line");
}

#[tokio::test]
async fn the_time_limit_with_no_line_is_a_failure_with_no_analysis() {
    let mut rig = Rig::new();
    rig.start(CalvarConfig {
        time_limit_s: 30,
        ..one_line()
    })
    .await;
    rig.tick(40).await;
    assert_eq!(rig.phase(), CalvarPhase::Failed(CalvarFailure::TimeUp));
    assert_eq!(rig.notice(), Some(CalvarNotice::TimeUp));
    assert_eq!(
        rig.state.calvar_view().stats.done,
        0,
        "no result, no history"
    );
}

#[tokio::test]
async fn without_a_limit_the_clock_only_counts() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    rig.answer_root(&[("e2e4", 30)]).await;
    rig.tick(600).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert!(rig.state.calvar_view().elapsed_s >= 590);
}

// ---- the user's game (D2320, D2321) ----

#[tokio::test]
async fn closing_puts_the_game_of_the_user_back_where_it_was() {
    let mut rig = Rig::new();
    rig.play("e2e4").await;
    rig.play("e7e5").await;
    // Cursor back one move: the exercise starts from there.
    rig.send(Message::Game(GameMessage::Navigate(
        mychess_app::Nav::Back(1),
    )))
    .await;
    let cursor = rig.state.session.cursor();
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    rig.start(one_line()).await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert_eq!(rig.root[0].1.position.fen(), {
        let mut p = Position::new();
        p = p.after(p.parse_uci("e2e4").unwrap()).unwrap();
        p.fen()
    });
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(rig.state.session.game().mainline().count(), 2);
    assert_eq!(rig.state.session.cursor(), cursor);
    assert!(rig.state.is_dirty(), "it was unsaved, it is unsaved");
}

#[tokio::test]
async fn an_unsaved_game_is_asked_about_and_cancel_changes_nothing() {
    let mut rig = Rig::new();
    rig.play("e2e4").await;
    assert!(rig.state.is_dirty());
    rig.calvar(CalvarMessage::Open).await;
    rig.start(one_line()).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert!(rig.state.discard_prompt.is_some());
    assert!(
        rig.state.calvar_dialog_open(),
        "the dialog waits under the question"
    );
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel))
        .await;
    assert!(rig.state.discard_prompt.is_none());
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(rig.root.is_empty());
}

#[tokio::test]
async fn a_clean_game_starts_without_a_question() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert!(rig.state.discard_prompt.is_none());
}

#[tokio::test]
async fn a_new_game_ends_the_exercise_and_the_game_of_the_user_asks_to_be_saved() {
    let mut rig = Rig::new();
    rig.play("e2e4").await;
    rig.start(one_line()).await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    rig.send(Message::Command(Command::NewGame)).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed, "the exercise ended first");
    // The user's game came back altered, so the normal question protects it.
    assert!(rig.state.discard_prompt.is_some());
    rig.send(Message::AnswerDiscard(DiscardChoice::Cancel))
        .await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(rig.state.is_dirty());
}

#[tokio::test]
async fn quitting_ends_the_exercise_and_writes_the_history() {
    let mut rig = ready().await;
    rig.answer_root(&[("e2e4", 30)]).await;
    rig.calvar(CalvarMessage::Done).await;
    rig.answer_lines(30, 20).await;
    rig.send(Message::Quit).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    let path = store_path(&rig.dir.path().join("preferences.toml"));
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("[[history]]")
    );
}

#[tokio::test]
async fn the_exercise_ends_when_the_editor_is_taken_by_another_game() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    let game = mychess_core::Game::new();
    rig.state.session.replace_game(game);
    rig.send(Message::Game(GameMessage::StopAutoplay)).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
}

// ---- exclusion and the online lock (D2309, D2322) ----

#[tokio::test]
async fn it_does_not_start_during_an_online_game_and_a_live_one_ends_when_it_begins() {
    let mut rig = Rig::new();
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    assert!(rig.state.assist_locked());
    rig.calvar(CalvarMessage::Open).await;
    assert!(!rig.state.calvar_dialog_open(), "opening is help too");
    rig.start(one_line()).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert!(rig.root.is_empty());

    let mut rig = Rig::new();
    rig.start(one_line()).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    rig.send(Message::Game(GameMessage::StopAutoplay)).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed, "the guard ends it");
}

#[tokio::test]
async fn the_action_of_the_shortcut_is_help_of_the_engines() {
    let mut rig = Rig::new();
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    rig.send(Message::Action(Action::CalvarTrainer)).await;
    assert!(!rig.state.calvar_dialog_open());
}

#[tokio::test]
async fn closing_the_exercise_is_allowed_during_an_online_game() {
    let mut rig = Rig::new();
    rig.start(one_line()).await;
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
}

#[tokio::test]
async fn it_does_not_start_while_the_tactics_training_is_on_and_vice_versa() {
    use mychess_app::tactics::{TacticsConfig, TacticsMessage, TacticsPhase};
    let mut rig = Rig::new();
    let file = rig.dir.path().join("p.pgn");
    std::fs::write(
        &file,
        "[Event \"p\"]\n[SetUp \"1\"]\n[FEN \"6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1\"]\n\n*\n\n",
    )
    .unwrap();
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(TacticsConfig {
            file: Some(file.clone()),
            ..TacticsConfig::default()
        }),
        seed: 1,
    }))
    .await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    rig.start(one_line()).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::Busy));
    rig.send(Message::Tactics(TacticsMessage::Close)).await;

    rig.start(one_line()).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    rig.send(Message::Tactics(TacticsMessage::Start {
        config: Box::new(TacticsConfig {
            file: Some(file),
            ..TacticsConfig::default()
        }),
        seed: 1,
    }))
    .await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(
        rig.phase(),
        CalvarPhase::Entering,
        "the exercise was not disturbed"
    );
}

// ---- adding the lines to the game (D2324) ----

/// A game 1. e4 e5 with the cursor on e4; the exercise calculates Black's
/// reply and is ready to show its result.
async fn reported_inside_a_game() -> Rig {
    let mut rig = Rig::new();
    rig.play("e2e4").await;
    rig.play("e7e5").await;
    rig.send(Message::Game(GameMessage::Navigate(
        mychess_app::Nav::Back(1),
    )))
    .await;
    rig.start(CalvarConfig {
        lines_required: 2,
        ..one_line()
    })
    .await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    rig.text("e5 Nf3").await;
    rig.end(Assessment::WhiteSlightly).await;
    rig.text("c5").await;
    rig.end(Assessment::Equal).await;
    rig.answer_root(&[("e7e5", 25), ("c7c5", 20), ("e7e6", 15)])
        .await;
    rig.calvar(CalvarMessage::Done).await;
    rig.answer_lines(25, 30).await;
    assert!(matches!(rig.phase(), CalvarPhase::Reported(_)));
    rig
}

#[tokio::test]
async fn the_lines_are_added_to_the_game_as_variants_only_when_asked() {
    let mut rig = reported_inside_a_game().await;
    assert!(rig.state.calvar_can_add_to_game());
    assert_eq!(
        rig.state.session.game().node_count(),
        1,
        "the editor holds the dedicated game until the lines are added"
    );
    rig.calvar(CalvarMessage::AddToGame).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    // The game of the user is back, with the cursor where the exercise began.
    let e4 = rig.state.session.cursor();
    assert_eq!(rig.state.session.game().mainline().next(), Some(e4));
    assert_eq!(
        rig.state.session.game().mainline().count(),
        2,
        "the main line of the user is not extended"
    );
    assert!(rig.state.is_dirty());
    let game = rig.state.session.game();
    let children = game.children(e4).expect("children");
    assert_eq!(children.len(), 3, "e5, and the variants c5 and e6 (missed)");
    // The variant has the symbol of the user and no word of any language.
    let c5 = children[1];
    let comment = game.comment_before(c5).expect("comment").unwrap_or("");
    assert!(
        comment.starts_with("d12:") || comment.starts_with('='),
        "{comment}"
    );
}

#[tokio::test]
async fn at_the_end_of_the_game_the_lines_cannot_be_variants_and_nothing_is_lost() {
    let mut rig = Rig::new();
    rig.play("e2e4").await;
    rig.start(CalvarConfig {
        lines_required: 1,
        ..one_line()
    })
    .await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    rig.text("e5").await;
    rig.end(Assessment::Equal).await;
    rig.answer_root(&[("e7e5", 25)]).await;
    rig.calvar(CalvarMessage::Done).await;
    rig.answer_lines(25, 25).await;
    assert!(!rig.state.calvar_can_add_to_game());
    rig.calvar(CalvarMessage::AddToGame).await;
    assert!(
        matches!(rig.phase(), CalvarPhase::Reported(_)),
        "still on the screen"
    );
    assert_eq!(rig.notice(), Some(CalvarNotice::CannotAdd));
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
}

#[tokio::test]
async fn a_fen_exercise_cannot_add_to_a_game() {
    let mut rig = Rig::new();
    rig.start(CalvarConfig {
        source: PositionSource::Fen,
        fen: "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3".to_owned(),
        ..one_line()
    })
    .await;
    rig.text("Bb5").await;
    rig.end(Assessment::WhiteSlightly).await;
    rig.answer_root(&[("f1b5", 30)]).await;
    rig.calvar(CalvarMessage::Done).await;
    rig.answer_lines(30, 30).await;
    assert!(!rig.state.calvar_can_add_to_game());
    rig.calvar(CalvarMessage::AddToGame).await;
    assert!(matches!(rig.phase(), CalvarPhase::Reported(_)), "ignored");
}

#[tokio::test]
async fn reset_clears_the_history_and_writes_the_file() {
    let mut rig = ready().await;
    rig.answer_root(&[("e2e4", 30)]).await;
    rig.calvar(CalvarMessage::Done).await;
    rig.answer_lines(30, 20).await;
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(rig.state.calvar_view().stats.done, 1);
    rig.calvar(CalvarMessage::ResetStats).await;
    assert_eq!(rig.state.calvar_view().stats.done, 0);
    let path = store_path(&rig.dir.path().join("preferences.toml"));
    assert!(
        !std::fs::read_to_string(path)
            .unwrap()
            .contains("[[history]]")
    );
}
