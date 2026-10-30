//! Regression tests of the review of the tactics training (spec M8-4,
//! D1947-D1969): a session that restarts, a refused start, the notice that
//! never went away, the store of a newer version, the ceiling of the search
//! and the exit of the program.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use mychess_app::engines::{EngineDraft, EngineMessage, StartMode};
use mychess_app::tactics::{
    CLASSIFY_PER_REQUEST, FindQuery, TacticsConfig, TacticsMessage, TacticsMode, TacticsNotice,
    TacticsPhase, extract_problems, find_next, open_file,
};
use mychess_app::{AppState, Command, Effect, GameMessage, Message, Nav, perform, update};
use mychess_core::Position;

const MATE1: &str = "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1";
const MATE2: &str = "kbK5/pp6/1P6/8/8/8/8/R7 w - - 0 1";

fn fen_game(event: &str, fen: &str) -> String {
    format!("[Event \"{event}\"]\n[SetUp \"1\"]\n[FEN \"{fen}\"]\n\n*\n\n")
}

fn plain_game(event: &str) -> String {
    format!("[Event \"{event}\"]\n\n1. e4 e5 *\n\n")
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write");
    path
}

fn config(path: &Path) -> TacticsConfig {
    TacticsConfig {
        file: Some(path.to_path_buf()),
        ..TacticsConfig::default()
    }
}

struct Rig {
    state: AppState,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().expect("dir");
        let prefs = dir.path().join("preferences.toml");
        Rig {
            state: AppState::with_preferences_path(Some(prefs)),
            dir,
        }
    }

    /// Runs the effects of the training; `hold` effects of the kind `Find`
    /// are kept back when `hold_find` is set.
    async fn pump(&mut self, effects: Vec<Effect>, hold_find: bool) -> Vec<Effect> {
        let mut held = Vec::new();
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            if !matches!(effect, Effect::Tactics(_)) {
                continue;
            }
            if hold_find
                && matches!(&effect, Effect::Tactics(t) if format!("{t:?}").starts_with("Find"))
            {
                held.push(effect);
                continue;
            }
            let answer = perform(effect).await;
            queue.extend(update(&mut self.state, answer));
        }
        held
    }

    async fn send(&mut self, message: Message) {
        let effects = update(&mut self.state, message);
        self.pump(effects, false).await;
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

    fn phase(&self) -> TacticsPhase {
        self.state.tactics_phase()
    }

    fn event(&self) -> String {
        self.state.session.game().tags().event.clone()
    }
}

#[tokio::test]
async fn closing_while_searching_and_restarting_on_a_larger_source_is_not_exhausted() {
    let mut rig = Rig::new();
    let small = write(
        rig.dir.path(),
        "small.pgn",
        &(0..3)
            .map(|i| plain_game(&format!("p{i}")))
            .collect::<String>(),
    );
    let mut big_text: String = (0..9).map(|i| plain_game(&format!("q{i}"))).collect();
    big_text.push_str(&fen_game("the one", MATE1));
    let big = write(rig.dir.path(), "big.pgn", &big_text);

    // The first search is asked for and never answered.
    let effects = update(
        &mut rig.state,
        Message::Tactics(TacticsMessage::Start {
            config: Box::new(config(&small)),
            seed: 1,
        }),
    );
    let held = rig.pump(effects, true).await;
    assert_eq!(rig.phase(), TacticsPhase::Searching, "{held:?}");
    assert_eq!(held.len(), 1, "the search waits for its answer");
    rig.tactics(TacticsMessage::Close).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);

    rig.start(config(&big)).await;
    assert_eq!(
        rig.phase(),
        TacticsPhase::Solving,
        "a stale count of games left must not end the walk before the tenth game"
    );
    assert_eq!(rig.event(), "Tactics");
}

#[tokio::test]
async fn a_start_while_a_session_is_on_changes_nothing() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    rig.start(config(&file)).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    let problem = rig.state.session.position().clone();

    // No file: refused without touching the session.
    rig.start(TacticsConfig::default()).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(*rig.state.session.position(), problem);
    // Another file: also ignored, the parked game must not be lost.
    let other = write(rig.dir.path(), "q.pgn", &fen_game("two", MATE2));
    rig.start(config(&other)).await;
    assert_eq!(*rig.state.session.position(), problem);
    // The dialog does not open over a live session.
    rig.tactics(TacticsMessage::Open).await;
    assert!(!rig.state.tactics_dialog_open());

    rig.tactics(TacticsMessage::Close).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert_eq!(*rig.state.session.position(), Position::new());
    assert_ne!(rig.event(), "Tactics");
}

#[tokio::test]
async fn a_refused_start_leaves_the_editor_and_the_dialog_alone() {
    let mut rig = Rig::new();
    rig.tactics(TacticsMessage::Open).await;
    assert!(rig.state.tactics_dialog_open());
    // No file chosen.
    rig.start(TacticsConfig::default()).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert!(rig.state.tactics_dialog_open());
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::NoSource)
    );
    assert_eq!(*rig.state.session.position(), Position::new());
    assert_ne!(rig.event(), "Tactics");
    // The current base (the Clipbase) has no game.
    let cfg = TacticsConfig {
        source: mychess_app::tactics::SourceKind::CurrentBase,
        ..TacticsConfig::default()
    };
    rig.start(cfg).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert!(rig.state.tactics_dialog_open());
    assert_eq!(*rig.state.session.position(), Position::new());
    assert_ne!(rig.event(), "Tactics");
    // An empty file.
    let empty = write(rig.dir.path(), "empty.pgn", "");
    rig.start(config(&empty)).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert!(rig.state.tactics_dialog_open());
    assert_eq!(*rig.state.session.position(), Position::new());
}

#[tokio::test]
async fn the_notice_goes_away_with_the_next_thing_the_user_does() {
    let mut rig = Rig::new();
    let file = write(
        rig.dir.path(),
        "p.pgn",
        &format!("{}{}", fen_game("one", MATE1), fen_game("two", MATE2)),
    );
    rig.start(config(&file)).await;
    // Navigating is locked: the remark appears...
    rig.send(Message::Game(GameMessage::Navigate(Nav::GameEnd)))
        .await;
    assert_eq!(rig.state.tactics_view().notice, Some(TacticsNotice::Locked));
    // ... and a move of the user clears it.
    let wrong = rig
        .state
        .session
        .position()
        .find_move("g1".parse().unwrap(), "f1".parse().unwrap(), None)
        .expect("legal");
    rig.send(Message::Game(GameMessage::PlayMove {
        mv: wrong,
        options: mychess_app::PlayOptions::default(),
    }))
    .await;
    assert_eq!(rig.state.tactics_view().notice, None);
    // Locked again, then "next" clears it too.
    rig.send(Message::Game(GameMessage::Navigate(Nav::GameEnd)))
        .await;
    assert_eq!(rig.state.tactics_view().notice, Some(TacticsNotice::Locked));
    rig.tactics(TacticsMessage::Next).await;
    assert_eq!(rig.state.tactics_view().notice, None);
}

#[tokio::test]
async fn a_failed_session_keeps_the_reason_it_ended() {
    let mut rig = Rig::new();
    let missing = rig.dir.path().join("missing.pgn");
    rig.start(config(&missing)).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::SourceUnreadable)
    );
    // Opening the dialog again starts clean.
    rig.tactics(TacticsMessage::Open).await;
    assert_eq!(rig.state.tactics_view().notice, None);
}

#[tokio::test]
async fn the_file_of_a_newer_version_is_never_overwritten() {
    let mut rig = Rig::new();
    let store = rig.dir.path().join("tactics.toml");
    let original = "version = 99\nfuture = \"x\"\n[stats.mate]\nshown = 5\n";
    std::fs::write(&store, original).expect("write");
    rig.tactics(TacticsMessage::Open).await;
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    rig.start(config(&file)).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    let mv = rig
        .state
        .session
        .position()
        .find_move("a1".parse().unwrap(), "a8".parse().unwrap(), None)
        .expect("legal");
    rig.send(Message::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }))
    .await;
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)));
    rig.tactics(TacticsMessage::Close).await;
    assert_eq!(std::fs::read_to_string(&store).expect("read"), original);
    assert!(!rig.dir.path().join("tactics.toml.corrupt").exists());
}

#[tokio::test]
async fn quitting_ends_the_session_and_brings_the_game_back() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    rig.start(config(&file)).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    let effects = update(&mut rig.state, Message::Quit);
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert_eq!(*rig.state.session.position(), Position::new());
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Tactics(t) if format!("{t:?}").starts_with("SaveStore"))),
        "the score is written on the way out: {effects:?}"
    );
}

#[tokio::test]
async fn an_engine_started_during_a_problem_is_stopped_at_once() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    let engine = rig
        .state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("engine");
    rig.start(config(&file)).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    let effects = update(
        &mut rig.state,
        Message::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::StopEngine { .. })),
        "{effects:?}"
    );
}

#[test]
fn a_game_full_of_markers_cannot_cost_more_than_the_ceiling_in_one_request() {
    let dir = tempfile::tempdir().expect("dir");
    // Sixty markers in one game: more than the ceiling of one request.
    let mut text = String::from("[Event \"many\"]\n\n");
    text.push_str(&"Nf3 {****D3} Nf6 {****D3} Ng1 {****D3} Ng8 {****D3} ".repeat(30));
    text.push_str("*\n");
    let path = write(dir.path(), "many.pgn", &text);
    let opened = open_file(&path).expect("opens");
    let game = opened.handle.game(0).expect("the game").0;
    assert!(
        extract_problems(&game, 1).len() > CLASSIFY_PER_REQUEST,
        "the fixture must have more problems than the ceiling"
    );
    let config = TacticsConfig {
        mode: TacticsMode::Mate,
        mate_min: 1,
        mate_max: 1,
        ..TacticsConfig::default()
    };
    let query = FindQuery {
        config: Box::new(config),
        start: 0,
        step: 1,
        k: 0,
        max_games: 5,
        solved: std::sync::Arc::new(Vec::new()),
        stamp: None,
        cancel: mychess_app::tactics::Cancel::new(),
    };
    let outcome = find_next(&opened.handle, &query);
    assert!(outcome.found.is_none());
    assert_eq!(outcome.classified, CLASSIFY_PER_REQUEST);
}

#[tokio::test]
async fn a_new_game_command_during_a_session_leaves_a_clean_new_game_and_no_remark() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    rig.start(config(&file)).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    rig.send(Message::Command(Command::NewGame)).await;
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert_ne!(rig.event(), "Tactics");
    assert_eq!(*rig.state.session.position(), Position::new());
    assert!(!rig.state.is_dirty(), "a new game is not an altered one");
    assert_ne!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::SourceChanged),
        "the user asked for this: no reason to give"
    );
    assert!(!rig.state.tactics_dialog_open());
}

#[tokio::test]
async fn a_game_edited_while_the_problem_was_being_looked_for_comes_back_altered() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    // The search is asked for and held back.
    let effects = update(
        &mut rig.state,
        Message::Tactics(TacticsMessage::Start {
            config: Box::new(config(&file)),
            seed: 1,
        }),
    );
    let held = rig.pump(effects, true).await;
    assert_eq!(rig.phase(), TacticsPhase::Searching);
    assert_eq!(held.len(), 1);
    // The editor is still the user's: it takes a move, which is not saved.
    let e4 = rig
        .state
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .expect("legal");
    rig.send(Message::Game(GameMessage::PlayMove {
        mv: e4,
        options: mychess_app::PlayOptions::default(),
    }))
    .await;
    assert!(rig.state.is_dirty());
    // The search answers: the problem takes the editor.
    rig.pump(held, false).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    assert_eq!(rig.event(), "Tactics");
    assert!(!rig.state.is_dirty(), "the problem is not an altered game");
    // Closing brings the move back, still flagged as unsaved.
    rig.tactics(TacticsMessage::Close).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(
        rig.state.is_dirty(),
        "the move of the user is still unsaved"
    );
}

#[tokio::test]
async fn a_game_edited_while_the_source_was_being_read_comes_back_altered() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    let effects = update(
        &mut rig.state,
        Message::Tactics(TacticsMessage::Start {
            config: Box::new(config(&file)),
            seed: 1,
        }),
    );
    // Nothing runs: the file is still being read.
    assert_eq!(rig.phase(), TacticsPhase::Opening);
    let e4 = rig
        .state
        .session
        .position()
        .find_move("e2".parse().unwrap(), "e4".parse().unwrap(), None)
        .expect("legal");
    rig.send(Message::Game(GameMessage::PlayMove {
        mv: e4,
        options: mychess_app::PlayOptions::default(),
    }))
    .await;
    rig.pump(effects, false).await;
    assert_eq!(rig.phase(), TacticsPhase::Solving);
    rig.tactics(TacticsMessage::Close).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(rig.state.is_dirty());
}

#[tokio::test]
async fn playing_against_an_engine_from_the_current_position_gives_the_game_back_first() {
    use mychess_app::engines::EngineMessage;
    use mychess_app::{PlayConfig, PlayMessage, StartFrom};
    use mychess_engine::{Capabilities, EngineIdentity};

    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    let engine = rig
        .state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("engine");
    rig.start(config(&file)).await;
    // Solve it: the training is in `Solved`, the problem still on the board.
    let mv = rig
        .state
        .session
        .position()
        .find_move("a1".parse().unwrap(), "a8".parse().unwrap(), None)
        .expect("legal");
    rig.send(Message::Game(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }))
    .await;
    assert!(matches!(rig.phase(), TacticsPhase::Solved(_)));

    let mut play = PlayConfig::new(engine);
    play.start = StartFrom::CurrentPosition;
    let effects = update(
        &mut rig.state,
        Message::Play(PlayMessage::Start(Box::new(play))),
    );
    assert_eq!(rig.phase(), TacticsPhase::Closed);
    assert_eq!(*rig.state.session.position(), Position::new());
    let run = effects
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("the engine starts");
    drop(update(
        &mut rig.state,
        Message::Engine(EngineMessage::Started {
            run,
            identity: Box::new(EngineIdentity {
                name: "Fake".to_owned(),
                author: None,
                options: Vec::new(),
                capabilities: Capabilities::default(),
            }),
            at: 1_700_000_000,
        }),
    ));
    assert!(rig.state.play_live(), "the game started");
    assert_ne!(rig.event(), "Tactics");
    assert_ne!(
        *rig.state.session.position(),
        Position::from_fen(MATE1).expect("fen"),
        "the game must not start from the position of the problem"
    );
}

#[tokio::test]
async fn the_reason_the_score_is_not_saved_stays_in_the_view_and_a_reset_says_so() {
    let mut rig = Rig::new();
    let store = rig.dir.path().join("tactics.toml");
    std::fs::write(&store, "version = 99\n").expect("write");
    rig.tactics(TacticsMessage::Open).await;
    assert_eq!(
        rig.state.tactics_view().store_issue,
        Some(TacticsNotice::StoreNewer)
    );
    // Opening the dialog again or starting a session does not wipe it.
    rig.tactics(TacticsMessage::CloseDialog).await;
    rig.tactics(TacticsMessage::Open).await;
    assert_eq!(
        rig.state.tactics_view().store_issue,
        Some(TacticsNotice::StoreNewer)
    );
    // A reset cannot be written: the user is told.
    let cfg = TacticsConfig::default();
    rig.tactics(TacticsMessage::ResetStats(Box::new(cfg))).await;
    assert_eq!(
        rig.state.tactics_view().notice,
        Some(TacticsNotice::StoreNotSaved)
    );
    assert_eq!(
        std::fs::read_to_string(&store).expect("read"),
        "version = 99\n"
    );
}

#[tokio::test]
async fn the_exit_waits_for_the_score_that_is_being_written() {
    let mut rig = Rig::new();
    let file = write(rig.dir.path(), "p.pgn", &fen_game("one", MATE1));
    rig.start(config(&file)).await;
    // Solve it, but keep the write of the score in the air.
    let mv = rig
        .state
        .session
        .position()
        .find_move("a1".parse().unwrap(), "a8".parse().unwrap(), None)
        .expect("legal");
    let effects = update(
        &mut rig.state,
        Message::Game(GameMessage::PlayMove {
            mv,
            options: mychess_app::PlayOptions::default(),
        }),
    );
    let save = effects
        .into_iter()
        .find(|e| matches!(e, Effect::Tactics(t) if format!("{t:?}").starts_with("SaveStore")))
        .expect("the score is written");
    // The program is asked to quit: it must not exit yet.
    let effects = update(&mut rig.state, Message::Quit);
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Exit | Effect::SavePreferences { .. })),
        "{effects:?}"
    );
    // The writes end (the one in the air and the one that the exit queued):
    // only then does the exit go on.
    let mut queue = VecDeque::from([save]);
    let mut exited = false;
    let mut rounds = 0;
    while let Some(effect) = queue.pop_front() {
        rounds += 1;
        assert!(rounds < 6, "the writes do not settle");
        assert!(!exited, "the exit came before the last write");
        let answer = perform(effect).await;
        for effect in update(&mut rig.state, answer) {
            match effect {
                // With a preferences path the exit follows the save of them.
                Effect::Exit | Effect::SavePreferences { .. } => exited = true,
                Effect::Tactics(_) => queue.push_back(effect),
                _ => {}
            }
        }
    }
    assert!(exited, "the exit goes on after the last write");
}
