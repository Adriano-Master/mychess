//! M8-6d (spec D2367-D2374): the exercise from a game of the list, from the
//! problem of the tactics training, and the review of a past exercise.
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
    Assessment, CalvarConfig, CalvarEffect, CalvarMessage, CalvarNotice, CalvarOrigin, CalvarPhase,
    Eval, LineProbes, LinesJob, Probe, RootJob, RootLine,
};
use mychess_app::engines::EngineDraft;
use mychess_app::online::{OnlinePhase, PlayingState};
use mychess_app::tactics::{TacticsConfig, TacticsMessage, TacticsPhase};
use mychess_app::{
    AppState, DiscardChoice, Effect, GameMessage, Message, PlayOptions, RequestId, perform, update,
};
use mychess_app::{BaseError, BasesMessage, SlotId};

const TWO: &str = "[White \"A\"]\n[Black \"B\"]\n\n1. e4 e5 2. Nf3 Nc6 *\n\n\
                   [White \"C\"]\n[Black \"D\"]\n\n1. d4 d5 *\n";
const PROBLEM: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";

struct Rig {
    state: AppState,
    dir: tempfile::TempDir,
    root: Vec<(RequestId, Box<RootJob>)>,
    lines: Vec<(RequestId, Box<LinesJob>)>,
    /// `ReadGame` effects held back so a test can answer them by hand.
    reads: Vec<Effect>,
    hold_reads: bool,
    /// `LoadStore` effects held back, to answer them late.
    stores: Vec<Effect>,
    hold_store: bool,
    clock: Instant,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().expect("dir");
        let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
        state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .expect("engine");
        state.open_pgn_in_slots = true;
        Rig {
            state,
            dir,
            root: Vec::new(),
            lines: Vec::new(),
            reads: Vec::new(),
            hold_reads: false,
            stores: Vec::new(),
            hold_store: false,
            clock: Instant::now(),
        }
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
                Effect::Calvar(CalvarEffect::LoadStore { .. }) if self.hold_store => {
                    self.stores.push(effect);
                }
                Effect::Calvar(CalvarEffect::ReadGame { .. }) if self.hold_reads => {
                    self.reads.push(effect);
                }
                Effect::Calvar(_) | Effect::Tactics(_) | Effect::LoadBaseGame { .. } => {
                    let answer = perform(effect).await;
                    queue.extend(update(&mut self.state, answer));
                }
                other if other.runs_on_host() => {}
                other => {
                    let answer = perform(other).await;
                    queue.extend(update(&mut self.state, answer));
                }
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

    fn phase(&self) -> CalvarPhase {
        self.state.calvar_phase()
    }

    fn notice(&self) -> Option<CalvarNotice> {
        self.state.calvar_view().notice
    }

    async fn open_two(&mut self) -> SlotId {
        let path = self.dir.path().join("two.pgn");
        std::fs::write(&path, TWO).unwrap();
        self.send(Message::Bases(BasesMessage::Open(path))).await;
        self.state.bases.current()
    }

    /// A `calvar.toml` with times that are not the default ones.
    fn write_store(&self) {
        std::fs::write(
            self.dir.path().join("calvar.toml"),
            "version = 1\n[config]\nsource = \"current\"\nfen = \"\"\nlines_required = 1\n\
             multipv = 2\nroot_ms = 777\nline_ms = 555\ntime_limit_s = 300\nfollow_board = false\n",
        )
        .unwrap();
    }

    /// Answers the held reads of the file, one by one.
    async fn release_stores(&mut self) {
        for effect in std::mem::take(&mut self.stores) {
            let answer = perform(effect).await;
            self.send(answer).await;
        }
    }

    async fn start_game(&mut self, slot: SlotId, number: u32) {
        self.calvar(CalvarMessage::StartFromGame {
            slot,
            number,
            now: 1_700_000_000,
        })
        .await;
    }

    async fn tick(&mut self, seconds: u64) {
        for _ in 0..seconds / 2 {
            self.clock += Duration::from_secs(2);
            let now = self.clock;
            self.calvar(CalvarMessage::Tick(now)).await;
        }
    }

    async fn start_tactics(&mut self) {
        let file = self.dir.path().join("p.pgn");
        std::fs::write(
            &file,
            format!("[Event \"p\"]\n[SetUp \"1\"]\n[FEN \"{PROBLEM}\"]\n\n*\n\n"),
        )
        .unwrap();
        self.send(Message::Tactics(TacticsMessage::Start {
            config: Box::new(TacticsConfig {
                file: Some(file),
                ..TacticsConfig::default()
            }),
            seed: 1,
        }))
        .await;
    }

    /// A whole exercise on the start position, ending in a result.
    async fn exercise(&mut self) {
        let config = CalvarConfig {
            lines_required: 1,
            root_ms: 200,
            line_ms: 200,
            ..CalvarConfig::default()
        };
        self.calvar(CalvarMessage::Start {
            config: Box::new(config),
            now: 1_700_000_000,
        })
        .await;
        assert_eq!(self.phase(), CalvarPhase::Entering);
        self.calvar(CalvarMessage::EnterText("e4 e5".to_owned()))
            .await;
        self.calvar(CalvarMessage::EndLine(Assessment::WhiteSlightly))
            .await;
        let (request, job) = self.root.pop().expect("root job");
        let p = job.position.clone();
        let result = Ok(vec![
            RootLine {
                multipv: 1,
                depth: 14,
                eval: Eval::centipawns(30),
                pv: vec![p.parse_uci("e2e4").unwrap()],
            },
            RootLine {
                multipv: 2,
                depth: 14,
                eval: Eval::centipawns(20),
                pv: vec![p.parse_uci("d2d4").unwrap()],
            },
        ]);
        self.calvar(CalvarMessage::RootAnalyzed { request, result })
            .await;
        self.calvar(CalvarMessage::Done).await;
        let (request, job) = self.lines.pop().expect("lines job");
        let probes = job
            .lines
            .iter()
            .map(|_| LineProbes {
                first: Some(Probe {
                    eval: Eval::centipawns(28),
                    depth: 12,
                    pv: Vec::new(),
                }),
                end: Some(Probe {
                    eval: Eval::centipawns(35),
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
        assert!(matches!(self.phase(), CalvarPhase::Reported(_)));
    }
}

// ---- from a game of the list (D2368, D2371) ----

#[tokio::test]
async fn a_game_of_the_list_becomes_the_problem_and_the_user_game_comes_back() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    // The user's game in the editor, with a change.
    let mv = rig.state.session.position().parse_uci("c2c4").unwrap();
    rig.send(Message::Game(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    }))
    .await;
    rig.start_game(slot, 1).await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    let view = rig.state.calvar_view();
    assert_eq!(view.origin, Some(CalvarOrigin::Game(1)));
    // The game has four plies: no ply of its own, so its end.
    let expected = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";
    assert_eq!(rig.root[0].1.position.fen(), expected);
    assert!(!rig.state.calvar_can_add_to_game());
    assert_eq!(
        view.config.source,
        mychess_app::calvar::PositionSource::Current
    );
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(rig.state.session.is_dirty(), "it comes back unsaved");
}

#[tokio::test]
async fn the_dialog_configuration_is_not_changed_by_a_game_start() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.start_game(slot, 2).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    let saved = rig.state.calvar_view().config;
    assert_eq!(saved.fen, "");
    assert_eq!(saved.source, mychess_app::calvar::PositionSource::Current);
}

#[tokio::test]
async fn an_old_or_foreign_answer_is_dropped() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.hold_reads = true;
    rig.start_game(slot, 1).await;
    assert_eq!(rig.reads.len(), 1);
    assert!(rig.state.calvar_view().reading_game);
    rig.calvar(CalvarMessage::GameRead {
        request: RequestId(999_999),
        result: Err(BaseError::GameUnreadable { number: 1 }),
    })
    .await;
    assert!(rig.state.calvar_view().reading_game, "the wait stays");
    assert_eq!(rig.notice(), None);
    // A newer request replaces the older one.
    rig.start_game(slot, 2).await;
    let first = rig.reads.remove(0);
    let answer = perform(first).await;
    rig.send(answer).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed, "the old read is dropped");
    let second = rig.reads.remove(0);
    let answer = perform(second).await;
    rig.send(answer).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
}

#[tokio::test]
async fn a_game_that_cannot_be_read_opens_the_dialog_with_the_notice() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.hold_reads = true;
    rig.start_game(slot, 1).await;
    let Some(Effect::Calvar(CalvarEffect::ReadGame { request, .. })) = rig.reads.pop() else {
        panic!("a read");
    };
    rig.calvar(CalvarMessage::GameRead {
        request,
        result: Err(BaseError::GameUnreadable { number: 1 }),
    })
    .await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert!(rig.state.calvar_dialog_open());
    assert_eq!(rig.notice(), Some(CalvarNotice::GameUnreadable));
    // A number outside the base is refused before any read.
    rig.calvar(CalvarMessage::CloseDialog).await;
    rig.start_game(slot, 77).await;
    assert!(rig.reads.is_empty());
    assert_eq!(rig.notice(), Some(CalvarNotice::GameUnreadable));
}

#[tokio::test]
async fn a_read_that_never_answers_is_given_up_by_the_ticks() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.hold_reads = true;
    rig.start_game(slot, 1).await;
    assert!(rig.state.calvar_wants_ticks());
    rig.tick(30).await;
    assert!(!rig.state.calvar_wants_ticks());
    assert_eq!(rig.notice(), Some(CalvarNotice::TimedOut));
    assert!(rig.state.calvar_dialog_open());
    let late = rig.reads.remove(0);
    let answer = perform(late).await;
    rig.send(answer).await;
    assert_eq!(
        rig.phase(),
        CalvarPhase::Closed,
        "the late answer is ignored"
    );
}

#[tokio::test]
async fn closing_the_dialog_gives_up_the_read() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.hold_reads = true;
    rig.start_game(slot, 1).await;
    rig.calvar(CalvarMessage::CloseDialog).await;
    assert!(!rig.state.calvar_view().reading_game);
}

#[tokio::test]
async fn a_game_start_is_refused_online_in_trial_and_under_a_dialog() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    let effects = update(
        &mut rig.state,
        Message::Calvar(CalvarMessage::StartFromGame {
            slot,
            number: 1,
            now: 1,
        }),
    );
    assert!(effects.is_empty(), "online lock: {effects:?}");
    assert!(!rig.state.calvar_dialog_open());

    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.send(Message::Game(GameMessage::EnterTrial { null_move: false }))
        .await;
    rig.start_game(slot, 1).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::Busy));
    assert!(rig.state.session.in_trial(), "the trial is kept");
}

#[tokio::test]
async fn a_game_start_without_engine_says_so() {
    let mut rig = Rig::new();
    rig.state = AppState::with_preferences_path(Some(rig.dir.path().join("preferences.toml")));
    rig.state.open_pgn_in_slots = true;
    let slot = rig.open_two().await;
    rig.start_game(slot, 1).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::NoEngine));
    assert!(rig.state.calvar_dialog_open());
}

// ---- from the problem of the tactics training (D2372) ----

#[tokio::test]
async fn the_tactics_problem_becomes_the_exercise_and_the_user_game_comes_back() {
    let mut rig = Rig::new();
    rig.start_tactics().await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert!(rig.state.calvar_can_start_from_tactics());
    rig.calvar(CalvarMessage::StartFromTactics { now: 5 }).await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert_eq!(rig.state.calvar_view().origin, Some(CalvarOrigin::Tactic));
    assert_eq!(rig.root[0].1.position.fen(), PROBLEM);
    assert!(!rig.state.calvar_can_start_from_tactics());
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(rig.state.session.game().tags().event, "?");
}

#[tokio::test]
async fn the_hand_over_does_not_ask_about_the_game_again() {
    let mut rig = Rig::new();
    let mv = rig.state.session.position().parse_uci("e2e4").unwrap();
    rig.send(Message::Game(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    }))
    .await;
    rig.start_tactics().await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    rig.calvar(CalvarMessage::StartFromTactics { now: 5 }).await;
    assert_eq!(rig.phase(), CalvarPhase::Entering, "no second question");
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(rig.state.session.is_dirty());
}

#[tokio::test]
async fn without_a_problem_the_tactics_start_says_so() {
    let mut rig = Rig::new();
    assert!(!rig.state.calvar_can_start_from_tactics());
    rig.calvar(CalvarMessage::StartFromTactics { now: 5 }).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::NoProblem));
    assert!(rig.state.calvar_dialog_open());
}

#[tokio::test]
async fn the_tactics_start_is_help_of_the_engines_and_leaves_the_training_online() {
    let mut rig = Rig::new();
    rig.start_tactics().await;
    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    rig.calvar(CalvarMessage::StartFromTactics { now: 5 }).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert!(!rig.state.calvar_can_start_from_tactics());
}

// ---- reviewing the history (D2369, D2370) ----

#[tokio::test]
async fn a_finished_exercise_can_be_reviewed_without_an_engine() {
    let mut rig = Rig::new();
    rig.exercise().await;
    let original = rig.state.calvar_view().report.expect("report");
    let score = original.score;
    rig.calvar(CalvarMessage::Close).await;
    let row = rig.state.calvar_view().history[0].clone();
    assert!(row.reviewable);
    assert_eq!((row.index, row.score), (0, score));
    let root_jobs = rig.root.len();
    let mv = rig.state.session.position().parse_uci("d2d4").unwrap();
    rig.send(Message::Game(GameMessage::PlayMove {
        mv,
        options: PlayOptions::default(),
    }))
    .await;
    rig.calvar(CalvarMessage::Review(0)).await;
    rig.send(Message::AnswerDiscard(DiscardChoice::DontSave))
        .await;
    assert_eq!(rig.phase(), CalvarPhase::Reported(score));
    assert!(rig.state.calvar_is_review());
    let view = rig.state.calvar_view();
    assert_eq!(view.origin, Some(CalvarOrigin::Review(1_700_000_000)));
    assert_eq!(view.report, Some(original));
    assert_eq!(rig.root.len(), root_jobs, "no engine was asked");
    assert!(rig.lines.is_empty());
    assert!(!rig.state.calvar_can_add_to_game());
    assert!(!rig.state.calvar_wants_ticks());
    assert_eq!(view.stats.done, 1, "nothing was recorded");
    rig.calvar(CalvarMessage::Close).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert_eq!(rig.state.calvar_view().stats.done, 1);
}

#[tokio::test]
async fn a_review_survives_a_restart_of_the_program() {
    let mut rig = Rig::new();
    rig.exercise().await;
    let original = rig.state.calvar_view().report.expect("report");
    rig.calvar(CalvarMessage::Close).await;
    assert!(rig.dir.path().join("calvar.toml").exists());

    let mut again = Rig::new();
    again.state = AppState::with_preferences_path(Some(rig.dir.path().join("preferences.toml")));
    // The file is read first: the review waits for it.
    again.calvar(CalvarMessage::Review(0)).await;
    assert!(matches!(again.phase(), CalvarPhase::Reported(_)));
    assert_eq!(again.state.calvar_view().report, Some(original));
}

#[tokio::test]
async fn a_review_that_cannot_be_opened_says_so() {
    let mut rig = Rig::new();
    rig.calvar(CalvarMessage::Review(3)).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::NoReview));
    assert!(rig.state.calvar_dialog_open());

    // An entry written before the lines were kept.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("calvar.toml"),
        "[[history]]\nwhen = 5\nscore = 40\nfen = \"8/8/8/8/8/8/8/K6k w - - 0 1\"\n",
    )
    .unwrap();
    let mut old = Rig::new();
    old.state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    old.calvar(CalvarMessage::Open).await;
    let row = old.state.calvar_view().history[0].clone();
    assert!(!row.reviewable);
    old.calvar(CalvarMessage::Review(row.index)).await;
    assert_eq!(old.phase(), CalvarPhase::Closed);
    assert_eq!(old.notice(), Some(CalvarNotice::NoReview));
}

#[tokio::test]
async fn a_review_is_refused_online_in_trial_and_while_an_exercise_is_on() {
    let mut rig = Rig::new();
    rig.exercise().await;
    rig.calvar(CalvarMessage::Close).await;

    rig.state.online.phase = OnlinePhase::Playing(PlayingState::Live);
    let effects = update(&mut rig.state, Message::Calvar(CalvarMessage::Review(0)));
    assert!(effects.is_empty());
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    rig.state.online.phase = OnlinePhase::Idle;

    rig.send(Message::Game(GameMessage::EnterTrial { null_move: false }))
        .await;
    rig.calvar(CalvarMessage::Review(0)).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.notice(), Some(CalvarNotice::Busy));
    rig.send(Message::Game(GameMessage::ExitTrial)).await;

    rig.calvar(CalvarMessage::Review(0)).await;
    assert!(matches!(rig.phase(), CalvarPhase::Reported(_)));
    // A second review over the first one is ignored.
    rig.calvar(CalvarMessage::Review(0)).await;
    assert!(matches!(rig.phase(), CalvarPhase::Reported(_)));
}

// ---- review fixes (D2375) ----

#[tokio::test]
async fn a_game_start_before_the_file_is_read_uses_the_saved_times() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.write_store();
    rig.hold_store = true;
    rig.start_game(slot, 1).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed, "waits for the file");
    assert_eq!(rig.stores.len(), 1, "the file is asked for at once");
    rig.release_stores().await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    rig.calvar(CalvarMessage::EnterText("e4 e5".to_owned()))
        .await;
    assert_eq!(rig.root[0].1.think, Duration::from_millis(777));
}

#[tokio::test]
async fn a_tactics_start_before_the_file_is_read_uses_the_saved_times() {
    let mut rig = Rig::new();
    rig.start_tactics().await;
    rig.write_store();
    rig.hold_store = true;
    rig.calvar(CalvarMessage::StartFromTactics { now: 5 }).await;
    assert_eq!(rig.stores.len(), 1);
    rig.release_stores().await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert_eq!(rig.root[0].1.think, Duration::from_millis(777));
}

#[tokio::test]
async fn a_tactics_start_without_a_uci_engine_keeps_the_training() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    rig.start_tactics().await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving);
    assert!(!rig.state.calvar_can_start_from_tactics());
    rig.calvar(CalvarMessage::StartFromTactics { now: 5 }).await;
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving, "kept");
    assert_eq!(rig.notice(), Some(CalvarNotice::NoEngine));
}

#[tokio::test]
async fn a_start_that_waits_for_a_file_that_never_comes_gives_up() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.hold_store = true;
    rig.start_game(slot, 1).await;
    assert!(rig.state.calvar_wants_ticks());
    rig.tick(30).await;
    assert!(!rig.state.calvar_wants_ticks());
    assert_eq!(rig.notice(), Some(CalvarNotice::TimedOut));
    assert!(rig.state.calvar_dialog_open());
}

#[tokio::test]
async fn the_dialog_start_cancels_a_late_game_read() {
    let mut rig = Rig::new();
    let slot = rig.open_two().await;
    rig.hold_reads = true;
    rig.start_game(slot, 1).await;
    rig.calvar(CalvarMessage::Start {
        config: Box::new(CalvarConfig::default()),
        now: 1,
    })
    .await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert_eq!(rig.state.calvar_view().origin, Some(CalvarOrigin::Editor));
    let late = rig.reads.remove(0);
    let answer = perform(late).await;
    rig.send(answer).await;
    assert_eq!(rig.state.calvar_view().origin, Some(CalvarOrigin::Editor));
}

// ---- the training ends only when the start is accepted (D2376) ----

#[tokio::test]
async fn a_tactics_start_waiting_for_the_file_keeps_the_training_until_it_begins() {
    let mut rig = Rig::new();
    rig.start_tactics().await;
    rig.hold_store = true;
    rig.calvar(CalvarMessage::StartFromTactics { now: 5 }).await;
    assert_eq!(rig.phase(), CalvarPhase::Closed, "waits for the file");
    assert_eq!(
        rig.state.tactics_phase(),
        TacticsPhase::Solving,
        "the training is not ended yet"
    );
    rig.release_stores().await;
    assert_eq!(rig.phase(), CalvarPhase::Entering);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Closed);
}

#[tokio::test]
async fn a_tactics_start_that_times_out_keeps_the_training() {
    let mut rig = Rig::new();
    rig.start_tactics().await;
    rig.hold_store = true;
    rig.calvar(CalvarMessage::StartFromTactics { now: 5 }).await;
    rig.tick(30).await;
    assert_eq!(rig.notice(), Some(CalvarNotice::TimedOut));
    assert_eq!(rig.phase(), CalvarPhase::Closed);
    assert_eq!(rig.state.tactics_phase(), TacticsPhase::Solving, "kept");
}

#[tokio::test]
async fn a_trial_is_refused_while_the_result_is_on_the_board() {
    let mut rig = Rig::new();
    rig.exercise().await;
    assert!(matches!(rig.phase(), CalvarPhase::Reported(_)));
    rig.send(Message::Game(GameMessage::EnterTrial { null_move: false }))
        .await;
    assert!(!rig.state.session.in_trial(), "the backup would be lost");
}
