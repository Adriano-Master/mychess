//! M8-7 contract tests of the safety of the game of the user (spec
//! `docs/specs/m8-7-jogo-tatico.md`, D2459, D2466): a tactical game never
//! replaces a game with unsaved changes, also when the tactics training had
//! set that game aside, and the problem of the training can be the start.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m8_1_common;
mod m8_7_common;

use m8_1_common::identity;
use m8_7_common::*;
use mychess_app::engines::{EngineDraft, EngineId, EngineMessage};
use mychess_app::tactical_game::TacticalSetup;
use mychess_app::tactics::{TacticsConfig, TacticsMessage, TacticsPhase};
use mychess_app::{
    AppNotice, AppState, ColorChoice, DiscardChoice, Effect, GameMessage, Message, PlayConfig,
    PlayMessage, PlayNotice, PlayOptions, StartFrom, TimeMode, update,
};
use mychess_core::Position;

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";

/// An application with one engine in the list that is not running: the
/// game starts it, as it does for a user who has never analysed.
struct Lab {
    state: AppState,
    engine: EngineId,
    runner: Runner,
}

impl Lab {
    fn new() -> Lab {
        let mut state = AppState::default();
        let engine = state
            .engines
            .add(EngineDraft::new("Fake", "/fake"))
            .expect("add engine");
        Lab {
            state,
            engine,
            runner: Runner::new(),
        }
    }

    /// `update`, then the file work its effects ask for; the rest comes back.
    fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        let effects = update(&mut self.state, message.into());
        self.settle(effects)
    }

    fn settle(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
        self.runner.settle_state(&mut self.state, effects)
    }

    fn config(&self, color: ColorChoice) -> PlayConfig {
        let mut config = PlayConfig::new(self.engine);
        config.color = color;
        config.mode = TimeMode::MoveTime(3);
        config.tactical = Some(TacticalSetup {
            elo: 1500,
            adjust_step: None,
            plan: None,
        });
        config
    }

    /// Sends a start and, if it asked for the engine, answers that it is up.
    fn start(&mut self, config: PlayConfig) {
        let effects = self.send(PlayMessage::Start(Box::new(config)));
        let run = effects.iter().find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        });
        if let Some(run) = run {
            self.send(Message::Engine(EngineMessage::Started {
                run,
                identity: Box::new(identity(full_options())),
                at: 1_700_000_000,
            }));
        }
    }

    fn notice(&self) -> Option<PlayNotice> {
        match &self.state.notice {
            Some(AppNotice::Play(n)) => Some(*n),
            _ => None,
        }
    }

    fn user_move(&mut self, uci: &str) {
        let mv = self.state.session.position().parse_uci(uci).expect("uci");
        self.send(Message::Game(GameMessage::PlayMove {
            mv,
            options: PlayOptions::default(),
        }));
    }

    fn start_training(&mut self, file: std::path::PathBuf) {
        self.send(Message::Tactics(TacticsMessage::Start {
            config: Box::new(TacticsConfig {
                file: Some(file),
                ..TacticsConfig::default()
            }),
            seed: 1,
        }));
    }

    fn plies(&self) -> usize {
        self.state.session.game().mainline().count()
    }
}

fn problem_file(dir: &std::path::Path) -> std::path::PathBuf {
    write(
        dir,
        "p.pgn",
        &format!("[Event \"one\"]\n[SetUp \"1\"]\n[FEN \"{MATE1}\"]\n\n*\n\n"),
    )
}

#[test]
fn the_problem_of_the_training_is_a_start_position_only_while_it_is_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    assert_eq!(lab.state.tactical_problem_position(), None);
    lab.start_training(problem_file(dir.path()));
    assert_eq!(lab.state.tactics_phase(), TacticsPhase::Solving);
    assert_eq!(
        lab.state.tactical_problem_position(),
        Some(Position::from_fen(MATE1).unwrap())
    );
    lab.send(Message::Tactics(TacticsMessage::Close));
    assert_eq!(lab.state.tactical_problem_position(), None);
}

#[test]
fn a_tactical_game_from_the_problem_ends_the_training() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    lab.start_training(problem_file(dir.path()));
    let problem = lab.state.tactical_problem_position().expect("a problem");
    let mut config = lab.config(ColorChoice::White);
    config.start = StartFrom::Position(Box::new(problem.clone()));
    lab.start(config);
    assert_eq!(lab.state.tactics_phase(), TacticsPhase::Closed);
    assert!(lab.state.play_live(), "{:?}", lab.state.notice);
    assert_eq!(lab.state.session.game().start_position(), &problem);
    assert_eq!(lab.state.session.game().tags().event, "Tactical game");
}

#[test]
fn an_unsaved_game_set_aside_by_the_training_is_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    lab.user_move("e2e4");
    assert!(lab.state.is_dirty());
    lab.start_training(problem_file(dir.path()));
    assert!(lab.state.discard_prompt.is_some(), "the usual question");
    lab.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert_eq!(lab.state.tactics_phase(), TacticsPhase::Solving);
    // The editor holds the problem, which looks saved: the window must still
    // chain the question for the game that is set aside.
    assert!(lab.state.play_start_needs_discard());
    let config = lab.config(ColorChoice::White);
    lab.start(config);
    assert!(!lab.state.play_live(), "the game was not replaced");
    assert_eq!(lab.notice(), Some(PlayNotice::UnsavedGame));
    assert_eq!(lab.state.tactics_phase(), TacticsPhase::Closed);
    assert_eq!(lab.plies(), 1, "the move of the user");
    assert!(lab.state.is_dirty(), "still marked as not saved");
    assert_ne!(lab.state.session.game().tags().event, "Tactical game");
}

#[test]
fn after_the_question_is_answered_the_game_can_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    lab.user_move("e2e4");
    lab.start_training(problem_file(dir.path()));
    lab.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    // What the window does: a new game first (it asks), then the start.
    lab.send(Message::Command(mychess_app::Command::NewGame));
    assert!(lab.state.discard_prompt.is_some());
    lab.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(!lab.state.play_start_needs_discard());
    let config = lab.config(ColorChoice::White);
    lab.start(config);
    assert!(lab.state.play_live(), "{:?}", lab.state.notice);
}

#[test]
fn a_clean_game_does_not_ask_anything_with_the_training_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    lab.start_training(problem_file(dir.path()));
    assert!(!lab.state.play_start_needs_discard());
    let config = lab.config(ColorChoice::White);
    lab.start(config);
    assert!(lab.state.play_live(), "{:?}", lab.state.notice);
    assert_eq!(lab.state.tactics_phase(), TacticsPhase::Closed);
}

#[test]
fn a_refused_start_does_not_end_the_training() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    lab.start_training(problem_file(dir.path()));
    let mut config = lab.config(ColorChoice::White);
    config.engine = EngineId(9999);
    lab.start(config);
    assert_eq!(lab.notice(), Some(PlayNotice::NoEngine));
    assert_eq!(lab.state.tactics_phase(), TacticsPhase::Solving);
}

#[test]
fn the_normal_game_keeps_the_same_protection() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    lab.user_move("e2e4");
    lab.start_training(problem_file(dir.path()));
    lab.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    let mut config = lab.config(ColorChoice::White);
    config.tactical = None;
    lab.start(config);
    assert!(!lab.state.play_live());
    assert_eq!(lab.plies(), 1);
}

/// What the dialog sends for "current position" when the game needs the
/// question: the set-up of the board, whose OK replaces the game.
fn current_position_chain(lab: &mut Lab) {
    lab.send(Message::Command(mychess_app::Command::SetupBoard));
    lab.send(Message::Setup(mychess_app::SetupMessage::Ok));
}

#[test]
fn current_position_with_the_training_on_starts_from_what_the_user_saw() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    lab.user_move("e2e4");
    lab.start_training(problem_file(dir.path()));
    lab.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    let seen = lab.state.session.position().fen();
    assert_eq!(
        seen,
        Position::from_fen(MATE1).unwrap().fen(),
        "the problem"
    );
    assert!(lab.state.play_start_needs_discard());
    current_position_chain(&mut lab);
    // The question for the game that was set aside comes before anything is
    // replaced.
    assert!(lab.state.discard_prompt.is_some());
    assert!(!lab.state.play_live());
    lab.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    assert!(!lab.state.play_start_needs_discard());
    let mut config = lab.config(ColorChoice::White);
    config.start = StartFrom::CurrentPosition;
    lab.start(config);
    assert!(lab.state.play_live(), "{:?}", lab.state.notice);
    assert_eq!(
        lab.state.session.game().start_position().fen(),
        seen,
        "the position the user saw"
    );
}

#[test]
fn cancelling_the_question_of_current_position_loses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut lab = Lab::new();
    lab.user_move("e2e4");
    lab.start_training(problem_file(dir.path()));
    lab.send(Message::AnswerDiscard(DiscardChoice::DontSave));
    current_position_chain(&mut lab);
    lab.send(Message::AnswerDiscard(DiscardChoice::Cancel));
    assert!(!lab.state.play_live());
    assert_eq!(lab.plies(), 1, "the move of the user");
    assert!(lab.state.is_dirty());
    // A start that arrives anyway is refused (the set-up dialog is still
    // open, or the game is unsaved).
    let mut config = lab.config(ColorChoice::White);
    config.start = StartFrom::CurrentPosition;
    lab.start(config);
    assert!(!lab.state.play_live());
    assert!(matches!(
        lab.notice(),
        Some(PlayNotice::Busy | PlayNotice::UnsavedGame)
    ));
    assert_eq!(lab.plies(), 1);
}
