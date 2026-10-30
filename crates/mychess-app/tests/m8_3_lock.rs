//! M8-3b contract tests of the lock of the assistance (spec D1330-D1332,
//! D1312, D1359, section 4.2): every family of help is dropped by `update`
//! during a game and freed at the end, the engines stop at the `gameStart`,
//! the marker `online-active.toml` is written, respected and removed, and an
//! online game excludes a game against an engine and a tournament and the
//! other way round.
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use m8_3_common::*;
use mychess_app::engines::{EngineDraft, EngineHub, EngineId, EngineMessage, RunId, StartMode};
use mychess_app::online::{
    MARKER_FILE, OnlineCommand, OnlineMessage, OnlineNotice, OnlinePhase, PlayingState,
    RejectReason, marker_path, read_marker,
};
use mychess_app::tournament::{Participant, TimeControl, TournamentConfig, TournamentMessage};
use mychess_app::{
    AnnotateMessage, AppNotice, AppState, ColorChoice, Effect, GameMessage, Message, Nav,
    PlayConfig, PlayMessage, PlayOptions, TimeMode, update,
};
use mychess_engine::testing::ScriptedEngine;
use mychess_engine::{Capabilities, EngineIdentity};

fn notice(rig: &Rig) -> Option<OnlineNotice> {
    match rig.notice() {
        Some(AppNotice::Online(n)) => Some(n.clone()),
        _ => None,
    }
}

fn marker(rig: &Rig) -> PathBuf {
    marker_path(rig.dir.path())
}

/// An engine of the list with a run that finished its handshake, without a
/// process (the state is fed directly, like the tests of the M8-1).
fn ready_engine(state: &mut AppState) -> (EngineId, RunId) {
    let engine = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add engine");
    let effects = update(
        state,
        Message::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
    );
    let run = effects
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("StartEngine");
    update(
        state,
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
    );
    (engine, run)
}

fn stops(rig: &Rig) -> Vec<RunId> {
    rig.seen
        .iter()
        .filter_map(|e| match e {
            Effect::StopEngine { run, .. } => Some(*run),
            _ => None,
        })
        .collect()
}

// ---- the sweep of the families of 4.2 ----

#[allow(clippy::too_many_lines)]
fn dropped_messages() -> Vec<(&'static str, Message)> {
    use mychess_app::engines::EngineId as E;
    use mychess_app::graphs::GraphMessage as G;
    use mychess_app::{
        AnalysisMessage as A, EvalBarMessage as V, ModeMessage as M, TreeMessage as T,
    };
    let run = RunId(1);
    let engine = E(1);
    let mut list: Vec<(&'static str, Message)> = Vec::new();
    // Engines.
    for (name, m) in [
        (
            "engine start",
            Message::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
        ),
        (
            "engine start idle",
            Message::Engine(EngineMessage::Start(engine, StartMode::Idle)),
        ),
        (
            "engine restart",
            Message::Engine(EngineMessage::Restart(run)),
        ),
        (
            "engine main analysis",
            Message::Engine(EngineMessage::ToggleMainAnalysis),
        ),
        ("analysis toggle", Message::Analysis(A::Toggle(run))),
        ("analysis add move", Message::Analysis(A::AddMove(run))),
        ("analysis add score", Message::Analysis(A::AddScore(run))),
        ("analysis add line", Message::Analysis(A::AddLine(run))),
        (
            "analysis add line n",
            Message::Analysis(A::AddLineNumber(run, 1)),
        ),
        ("analysis multipv", Message::Analysis(A::SetMultiPv(run, 3))),
        ("analysis lock", Message::Analysis(A::ToggleLock(run))),
        (
            "analysis exclude",
            Message::Analysis(A::SetExclusion(run, "e2e4".into())),
        ),
        ("analysis stats", Message::Analysis(A::RequestStats(run))),
        ("annotation open", Message::Annotate(AnnotateMessage::Open)),
        (
            "annotation start",
            Message::Annotate(AnnotateMessage::Start),
        ),
        ("threat", Message::Mode(M::ToggleThreat(run))),
        ("shoot-out", Message::Mode(M::ToggleFinishGame(run))),
        ("automove", Message::Mode(M::ToggleAutomove(run))),
        (
            "game against an engine",
            Message::Play(PlayMessage::Start(Box::new(PlayConfig::new(engine)))),
        ),
        (
            "tournament",
            Message::Tournament(TournamentMessage::Start {
                config: Box::default(),
                at: Instant::now(),
            }),
        ),
        (
            "F2 engine 1",
            Message::Action(mychess_app::Action::StartEngine(1)),
        ),
        (
            "F4 analysis",
            Message::Action(mychess_app::Action::ToggleEngineAnalysis),
        ),
        (
            "play engine",
            Message::Action(mychess_app::Action::PlayEngine),
        ),
    ] {
        list.push((name, m));
    }
    // Evaluation.
    for (name, m) in [
        ("eval bar", Message::EvalBar(V::ToggleBar)),
        ("traffic lights", Message::EvalBar(V::ToggleTrafficLights)),
        ("arrow numbers", Message::EvalBar(V::ToggleArrowNumbers)),
        ("pv arrows", Message::EvalBar(V::ToggleArrows(run))),
        ("arrow mode", Message::EvalBar(V::CycleArrowMode)),
        (
            "play arrow",
            Message::EvalBar(V::PlayArrow {
                run,
                line: 0,
                from: sq("e2"),
                to: sq("e4"),
            }),
        ),
        ("graph toggle", Message::Graph(G::Toggle)),
        ("graph open", Message::Graph(G::SetOpen(true))),
        ("graph click", Message::Graph(G::Click(0.5))),
        (
            "action eval bar",
            Message::Action(mychess_app::Action::ToggleEvalBar),
        ),
        (
            "action traffic lights",
            Message::Action(mychess_app::Action::ToggleTrafficLights),
        ),
        (
            "action arrow numbers",
            Message::Action(mychess_app::Action::ToggleArrowNumbers),
        ),
        (
            "action graph",
            Message::Action(mychess_app::Action::ToggleGraphPane),
        ),
    ] {
        list.push((name, m));
    }
    // Openings.
    for (name, m) in [
        ("tree toggle", Message::Tree(T::Toggle)),
        ("tree open", Message::Tree(T::Open)),
        ("tree short", Message::Tree(T::SetShort(true))),
        ("tree auto refresh", Message::Tree(T::ToggleAutoRefresh)),
        ("tree adjust", Message::Tree(T::SetAdjust(true))),
        ("tree row", Message::Tree(T::RowClicked(0))),
        (
            "action tree",
            Message::Action(mychess_app::Action::ToggleTreePane),
        ),
    ] {
        list.push((name, m));
    }
    // Editing the game.
    let g = |m: GameMessage| Message::Game(m);
    for (name, m) in [
        (
            "play squares",
            g(GameMessage::PlaySquares {
                from: sq("e2"),
                to: sq("e4"),
                promotion: None,
                options: PlayOptions::default(),
            }),
        ),
        ("null move", g(GameMessage::PlayNullMove)),
        ("variation", g(GameMessage::StartVariation)),
        ("typed move", g(GameMessage::EnterMoveText("e4".into()))),
        ("commit entry", g(GameMessage::MoveEntryCommit)),
        ("truncate end", g(GameMessage::TruncateEnd)),
        ("truncate from", g(GameMessage::TruncateFrom)),
        ("truncate start", g(GameMessage::TruncateStart)),
        ("strip comments", g(GameMessage::StripComments)),
        ("strip variations", g(GameMessage::StripVariations)),
        ("strip nags", g(GameMessage::StripNags)),
        ("clear nags", g(GameMessage::ClearNags)),
        ("undo", g(GameMessage::Undo)),
        ("redo", g(GameMessage::Redo)),
        (
            "comment",
            g(GameMessage::SetComment {
                node: mychess_core::Game::new().root(),
                text: "Qh5 wins".into(),
            }),
        ),
        (
            "action add variation",
            Message::Action(mychess_app::Action::AddVariation),
        ),
        (
            "action truncate",
            Message::Action(mychess_app::Action::TruncateGame),
        ),
        (
            "action trial mode",
            Message::Action(mychess_app::Action::ToggleTrialMode),
        ),
        ("action undo", Message::Action(mychess_app::Action::Undo)),
        ("action redo", Message::Action(mychess_app::Action::Redo)),
        // The ways to type a move (D1370).
        (
            "move entry char",
            Message::Game(GameMessage::MoveEntryChar('d')),
        ),
        (
            "move entry typed",
            Message::Game(GameMessage::MoveEntryTyped('d')),
        ),
        (
            "move entry backspace",
            Message::Game(GameMessage::MoveEntryBackspace),
        ),
        (
            "move entry clear",
            Message::Game(GameMessage::MoveEntryClear),
        ),
        (
            "chooser confirm",
            Message::Game(GameMessage::ChooserConfirm(Some(0))),
        ),
        (
            "answer promotion",
            Message::Game(GameMessage::AnswerPromotion(None)),
        ),
        (
            "action clear move entry",
            Message::Action(mychess_app::Action::ClearMoveEntry),
        ),
    ] {
        list.push((name, m));
    }
    list
}

#[tokio::test]
async fn every_family_of_help_is_dropped_by_update_during_the_game() {
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::white(), &state("e2e4 e7e5")))
        .await;
    rig.until("two plies", |s| s.session.game().mainline().count() == 2)
        .await;
    assert!(rig.lock());
    let revision = rig.state.session.revision();
    let game_before = rig.state.session.game().clone();
    for (name, message) in dropped_messages() {
        rig.state.notice = None;
        let effects = rig.update_only(message);
        assert!(effects.is_empty(), "{name}: effects {effects:?}");
        assert_eq!(
            rig.state.notice,
            Some(AppNotice::Online(OnlineNotice::AssistanceDisabled)),
            "{name}: dropped with the remark"
        );
        assert!(rig.state.engine_runs().is_empty(), "{name}");
        assert!(!rig.state.play_live(), "{name}");
        assert!(!rig.state.tournament_live(), "{name}");
        assert!(!rig.state.annotation_running(), "{name}");
    }
    assert_eq!(
        rig.state.session.revision(),
        revision,
        "the game did not change"
    );
    assert_eq!(
        rig.state.session.game().mainline().count(),
        game_before.mainline().count()
    );
    assert!(rig.state.online.game.as_ref().unwrap().plies() == 2);
}

#[tokio::test]
async fn navigating_the_moves_already_played_stays_allowed() {
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::white(), &state("e2e4 e7e5")))
        .await;
    rig.until("two plies", |s| s.session.game().mainline().count() == 2)
        .await;
    for nav in [Nav::Start, Nav::Forward(1), Nav::GameEnd, Nav::Back(1)] {
        rig.state.notice = None;
        rig.update_only(Message::Game(GameMessage::Navigate(nav)));
        assert_eq!(rig.state.notice, None, "{nav:?}");
    }
    rig.state.notice = None;
    rig.update_only(Message::Action(mychess_app::Action::FlipBoard));
    assert_eq!(rig.state.notice, None);
}

#[tokio::test]
async fn keys_typed_on_the_board_cannot_edit_the_game_through_the_nag_shortcuts() {
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::white(), &state("e2e4 e7e5")))
        .await;
    rig.until("two plies", |s| s.session.game().mainline().count() == 2)
        .await;
    let revision = rig.state.session.revision();
    let t = Instant::now();
    // `--` is the null move; `!` is a NAG; both would edit the game.
    for key in ['-', '-', '!'] {
        rig.update_only(Message::NagKey {
            key: mychess_app::NagKey::Char(key),
            at: t,
        });
    }
    rig.update_only(Message::NagTick(t + Duration::from_secs(5)));
    assert_eq!(rig.state.session.revision(), revision);
    assert_eq!(rig.mainline_count(), 2);
}

#[tokio::test]
async fn the_opening_name_is_hidden_during_the_game() {
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::white(), &state("e2e4 e7e5")))
        .await;
    assert!(rig.state.opening().is_none());
}

#[tokio::test]
async fn nothing_is_dropped_while_only_searching_and_everything_is_freed_at_the_end() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert_eq!(rig.state.online.phase, OnlinePhase::Seeking);
    for (name, message) in dropped_messages() {
        rig.state.notice = None;
        rig.update_only(message);
        assert_ne!(
            rig.state.notice,
            Some(AppNotice::Online(OnlineNotice::AssistanceDisabled)),
            "{name}: a seek does not lock (D1331)"
        );
    }
    // The game starts, ends, and the same messages are accepted again.
    let mut rig = Rig::new(1).await;
    rig.start_with(&full(&Full::default(), &state("e2e4 e7e5")))
        .await;
    rig.games[0].line(&finished("e2e4 e7e5", "draw", None));
    rig.until("unlocked", |s| !s.assist_locked()).await;
    for (name, message) in dropped_messages() {
        rig.state.notice = None;
        rig.update_only(message);
        assert_ne!(
            rig.state.notice,
            Some(AppNotice::Online(OnlineNotice::AssistanceDisabled)),
            "{name}: freed at the end"
        );
    }
}

// ---- engines stop at the game start (D1331) ----

fn scripted_hub(state: &mut AppState) {
    let fake_engine = ScriptedEngine::builder().build();
    state.engine_hub = EngineHub::with_launcher(Arc::new(move |config| {
        let fake = fake_engine.clone();
        Box::pin(async move { fake.start(config).await })
    }));
}

#[tokio::test]
async fn every_running_engine_is_stopped_at_the_game_start_and_not_before() {
    let mut rig = Rig::new(1).await;
    scripted_hub(&mut rig.state);
    let a = rig
        .state
        .engines
        .add(EngineDraft::new("A", "/fake/a"))
        .expect("engine");
    let b = rig
        .state
        .engines
        .add(EngineDraft::new("B", "/fake/b"))
        .expect("engine");
    rig.send(Message::Engine(EngineMessage::Start(a, StartMode::Analyze)))
        .await;
    rig.send(Message::Engine(EngineMessage::Start(b, StartMode::Idle)))
        .await;
    assert_eq!(rig.state.engine_runs().len(), 2);
    // Seeking alone stops nothing: if nobody accepts, nothing is lost.
    rig.connect().await;
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert_eq!(stops(&rig).len(), 0);
    assert_eq!(rig.state.engine_runs().len(), 2);
    assert_eq!(notice(&rig), Some(OnlineNotice::AssistanceWillBeDisabled));
    // The game starts: both are stopped.
    rig.game_start();
    rig.until("started", |s| s.online.phase.is_live_for_lock())
        .await;
    assert_eq!(stops(&rig).len(), 2, "one stop per run");
    rig.until("engines gone", |s| s.engine_runs().is_empty())
        .await;
    // A new run is refused during the game.
    let effects = rig.update_only(Message::Engine(EngineMessage::Start(a, StartMode::Analyze)));
    assert!(effects.is_empty());
    assert!(rig.state.engine_runs().is_empty());
    // And allowed again when it is over.
    rig.games[0].line(&full(&Full::default(), &state("e2e4")));
    rig.until("described", |s| s.online.game.is_some()).await;
    rig.games[0].line(&finished("e2e4", "draw", None));
    rig.until("unlocked", |s| !s.assist_locked()).await;
    rig.send(Message::Engine(EngineMessage::Start(a, StartMode::Analyze)))
        .await;
    assert_eq!(rig.state.engine_runs().len(), 1);
}

trait LiveForLock {
    fn is_live_for_lock(&self) -> bool;
}

impl LiveForLock for OnlinePhase {
    fn is_live_for_lock(&self) -> bool {
        matches!(self, OnlinePhase::Playing(PlayingState::Live))
    }
}

// ---- the marker (D1332, D1359) ----

#[tokio::test]
async fn the_marker_is_written_by_an_effect_not_by_update_and_removed_at_the_end() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.online(OnlineMessage::Seek(Rig::seek_request())).await;
    assert!(rig.fake.wait_requests("POST", "/api/board/seek", 1).await);
    assert!(!marker(&rig).exists());
    rig.game_start();
    // Take the message by hand to look at `update` alone.
    let effects = loop {
        let message = tokio::time::timeout(Duration::from_secs(5), rig.events.next())
            .await
            .expect("an event")
            .expect("hub alive");
        let effects = rig.update_only(message);
        if effects.iter().any(|e| {
            matches!(
                e,
                Effect::Online {
                    command: OnlineCommand::WriteMarker { .. },
                    ..
                }
            )
        }) {
            break effects;
        }
    };
    assert!(!marker(&rig).exists(), "update does no disk I/O");
    rig.run(effects).await;
    let info = read_marker(rig.dir.path()).expect("marker");
    assert_eq!(info.game_id, ID);
    assert!(!info.date.is_empty());
    assert!(marker(&rig).file_name().is_some_and(|n| n == MARKER_FILE));
    rig.games[0].line(&full(&Full::default(), &state("e2e4")));
    rig.until("described", |s| s.online.game.is_some()).await;
    rig.games[0].line(&finished("e2e4", "resign", Some("white")));
    rig.until("unlocked", |s| !s.assist_locked()).await;
    let gone = marker(&rig);
    rig.until("marker removed", |_| !gone.exists()).await;
}

#[tokio::test]
async fn the_marker_goes_away_when_the_game_is_aborted_too() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    assert!(marker(&rig).exists());
    rig.games[0].line(&finished("", "aborted", None));
    let gone = marker(&rig);
    rig.until("marker removed", |_| !gone.exists()).await;
    assert!(!rig.lock());
}

#[tokio::test]
async fn the_marker_stays_when_the_program_quits_in_the_middle_of_a_game() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.send(Message::Quit).await;
    assert!(
        marker(&rig).exists(),
        "closing the app does not free the lock"
    );
    assert_eq!(read_marker(rig.dir.path()).expect("marker").game_id, ID);
}

#[tokio::test]
async fn a_marker_found_at_start_locks_before_anything_connects_and_the_resumed_game_keeps_it() {
    let mut rig = Rig::new(1).await;
    std::fs::write(
        marker(&rig),
        format!("game = \"{ID}\"\ndate = \"2026.10.24\"\n"),
    )
    .expect("marker");
    assert!(!rig.lock(), "nothing read yet");
    rig.online(OnlineMessage::LoadMarker).await;
    assert!(rig.lock(), "locked from the marker");
    for (name, message) in dropped_messages() {
        let effects = rig.update_only(message);
        assert!(effects.is_empty(), "{name}");
    }
    rig.connect().await;
    // The game of the marker is still going on: the event stream says so.
    rig.game_start();
    rig.until("resumed", |s| s.online.phase.is_live_for_lock())
        .await;
    assert_eq!(notice(&rig), Some(OnlineNotice::Resumed));
    let generation = rig
        .seen
        .iter()
        .rev()
        .find_map(|e| match e {
            Effect::Online {
                command: OnlineCommand::Grace { generation, .. },
                ..
            } => Some(*generation),
            _ => None,
        })
        .expect("a grace timer was asked for at the connection");
    rig.online(OnlineMessage::MarkerGrace(generation)).await;
    assert!(rig.lock(), "the grace period never frees a live game");
    assert!(marker(&rig).exists());
    // The resumed game is described by the first line of its stream.
    rig.games[0].line(&full(&Full::default(), &state("e2e4 e7e5")));
    rig.until("described", |s| s.online.game.is_some()).await;
    assert_eq!(rig.state.online.game.as_ref().unwrap().plies(), 2);
}

#[tokio::test]
async fn a_stale_marker_is_freed_after_the_grace_period_and_only_that_generation_counts() {
    let mut rig = Rig::new(1).await;
    std::fs::write(
        marker(&rig),
        format!("game = \"{ID}\"\ndate = \"2026.10.24\"\n"),
    )
    .expect("marker");
    rig.online(OnlineMessage::LoadMarker).await;
    rig.connect().await;
    let generation = rig
        .seen
        .iter()
        .rev()
        .find_map(|e| match e {
            Effect::Online {
                command: OnlineCommand::Grace { generation, delay },
                ..
            } => Some((*generation, *delay)),
            _ => None,
        })
        .map(|(g, delay)| {
            assert!(delay >= Duration::from_secs(1), "a real grace: {delay:?}");
            g
        })
        .expect("grace timer");
    rig.online(OnlineMessage::MarkerGrace(generation.wrapping_add(7)))
        .await;
    assert!(rig.lock(), "a timer of another generation is ignored");
    rig.online(OnlineMessage::MarkerGrace(generation)).await;
    assert!(!rig.lock());
    assert_eq!(notice(&rig), Some(OnlineNotice::MarkerCleared));
    let gone = marker(&rig);
    rig.until("file removed", |_| !gone.exists()).await;
}

#[tokio::test]
async fn a_game_finish_event_frees_the_marker_of_that_game() {
    let mut rig = Rig::new(1).await;
    std::fs::write(
        marker(&rig),
        format!("game = \"{ID}\"\ndate = \"2026.10.24\"\n"),
    )
    .expect("marker");
    rig.online(OnlineMessage::LoadMarker).await;
    rig.connect().await;
    rig.event_stream.line(&format!(
        r#"{{"type":"gameFinish","game":{{"gameId":"{ID}","color":"black"}}}}"#
    ));
    rig.until("freed", |s| !s.assist_locked()).await;
    let gone = marker(&rig);
    rig.until("file removed", |_| !gone.exists()).await;
}

#[tokio::test]
async fn a_malformed_marker_does_not_lock() {
    for text in [
        "",
        "game = 3",
        "game = \"../etc\"\ndate = \"x\"",
        "\u{0}\u{1}",
    ] {
        let mut rig = Rig::new(1).await;
        std::fs::write(marker(&rig), text).expect("marker");
        rig.online(OnlineMessage::LoadMarker).await;
        assert!(!rig.lock(), "{text:?}");
    }
}

#[tokio::test]
async fn releasing_the_lock_by_hand_is_refused_while_the_link_is_up() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    assert!(marker(&rig).exists());
    rig.online(OnlineMessage::RemoveLock { confirmed: false })
        .await;
    assert!(rig.lock(), "without the confirmation nothing happens");
    assert!(marker(&rig).exists());
    // D1332/D1373: with the network up and the game alive, the confirmed
    // request is refused too (the lock is released by hand only offline).
    rig.online(OnlineMessage::RemoveLock { confirmed: true })
        .await;
    assert!(rig.lock(), "a live game on a live link cannot be unlocked");
    assert!(marker(&rig).exists());
    assert_ne!(notice(&rig), Some(OnlineNotice::LockReleased));
}

#[tokio::test]
async fn releasing_a_lock_that_is_not_on_changes_nothing() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    let before = rig.state.notice.clone();
    rig.online(OnlineMessage::RemoveLock { confirmed: true })
        .await;
    assert!(!rig.lock());
    assert_eq!(rig.state.notice, before);
}

// ---- mutual exclusion (D1312) ----

fn play_config(engine: EngineId) -> PlayConfig {
    let mut config = PlayConfig::new(engine);
    config.color = ColorChoice::White;
    config.mode = TimeMode::Depth(1);
    config
}

#[tokio::test]
async fn a_game_against_an_engine_forbids_seeks_challenges_and_accepting() {
    let mut rig = Rig::new(1).await;
    let (engine, _run) = ready_engine(&mut rig.state);
    rig.update_only(Message::Play(PlayMessage::Start(Box::new(play_config(
        engine,
    )))));
    assert!(rig.state.play_live(), "the game against the engine is on");
    rig.connect().await;
    rig.event_stream.line(&challenge_event(
        "chal0001", "Alice", None, "standard", 600, 0,
    ));
    rig.until("listed", |s| !s.online.incoming.is_empty()).await;
    for message in [
        OnlineMessage::Seek(Rig::seek_request()),
        OnlineMessage::Challenge(mychess_app::online::ChallengeRequest {
            variant: mychess_app::online::OnlineVariant::Standard,
            username: "Alice".into(),
            limit_secs: 600,
            increment: 0,
            rated: false,
            color: mychess_app::online::OnlineColor::Random,
        }),
        OnlineMessage::AcceptChallenge("chal0001".into()),
    ] {
        let effects = rig.update_only(Message::Online(message.clone()));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::Online { .. })),
            "{message:?}"
        );
        assert_eq!(
            notice(&rig),
            Some(OnlineNotice::Rejected(RejectReason::Busy)),
            "{message:?}"
        );
        assert_eq!(rig.state.online.phase, OnlinePhase::Idle, "{message:?}");
    }
    assert!(rig.state.play_live(), "the game against the engine goes on");
    assert_eq!(rig.posts("/api/board/seek"), 0);
}

#[tokio::test]
async fn an_online_game_forbids_a_game_against_an_engine_and_a_tournament() {
    let mut rig = Rig::new(1).await;
    let (engine, _run) = ready_engine(&mut rig.state);
    rig.start_game().await;
    rig.update_only(Message::Play(PlayMessage::Start(Box::new(play_config(
        engine,
    )))));
    assert!(!rig.state.play_live());
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled))
    );
    let config = TournamentConfig {
        participants: vec![Participant::new(1, "A"), Participant::new(2, "B")],
        cycles: 2,
        time: TimeControl::Depth(1),
        ..TournamentConfig::default()
    };
    let effects = rig.update_only(Message::Tournament(TournamentMessage::Start {
        config: Box::new(config),
        at: Instant::now(),
    }));
    assert!(effects.is_empty());
    assert!(!rig.state.tournament_live());
}

#[tokio::test]
async fn a_game_that_starts_while_a_game_against_an_engine_is_on_ends_it_and_stops_the_engine() {
    let mut rig = Rig::new(1).await;
    let (engine, run) = ready_engine(&mut rig.state);
    rig.update_only(Message::Play(PlayMessage::Start(Box::new(play_config(
        engine,
    )))));
    assert!(rig.state.play_live());
    rig.connect().await;
    // A game of the account starts elsewhere (a resumed game).
    rig.game_start();
    rig.until("online game", |s| s.online.phase.is_live_for_lock())
        .await;
    assert!(
        !rig.state.play_live(),
        "the game against the engine is over"
    );
    assert!(stops(&rig).contains(&run), "and its engine is stopped");
    assert!(rig.lock());
}

fn tournament_config() -> TournamentConfig {
    TournamentConfig {
        participants: vec![Participant::new(1, "A"), Participant::new(2, "B")],
        cycles: 2,
        time: TimeControl::Depth(1),
        ..TournamentConfig::default()
    }
}

fn start_tournament(rig: &mut Rig) {
    let a = rig
        .state
        .engines
        .add(EngineDraft::new("A", "/fake/a"))
        .expect("a");
    let b = rig
        .state
        .engines
        .add(EngineDraft::new("B", "/fake/b"))
        .expect("b");
    let config = TournamentConfig {
        participants: vec![Participant::new(a.0, "A"), Participant::new(b.0, "B")],
        ..tournament_config()
    };
    rig.update_only(Message::Tournament(TournamentMessage::Start {
        config: Box::new(config),
        at: Instant::now(),
    }));
}

#[tokio::test]
async fn a_tournament_forbids_seeks_and_challenges() {
    let mut rig = Rig::new(1).await;
    start_tournament(&mut rig);
    assert!(
        rig.state.tournament_live(),
        "the tournament is being prepared"
    );
    rig.connect().await;
    for message in [
        OnlineMessage::Seek(Rig::seek_request()),
        OnlineMessage::Challenge(mychess_app::online::ChallengeRequest {
            variant: mychess_app::online::OnlineVariant::Standard,
            username: "Alice".into(),
            limit_secs: 600,
            increment: 0,
            rated: false,
            color: mychess_app::online::OnlineColor::Random,
        }),
    ] {
        let effects = rig.update_only(Message::Online(message.clone()));
        assert!(!effects.iter().any(|e| matches!(e, Effect::Online { .. })));
        assert_eq!(
            notice(&rig),
            Some(OnlineNotice::Rejected(RejectReason::Busy)),
            "{message:?}"
        );
    }
    assert_eq!(rig.posts("/api/board/seek"), 0);
    assert!(rig.state.tournament_live());
}

#[tokio::test]
async fn a_game_that_starts_while_a_tournament_loads_does_not_let_it_start_engines_later() {
    let mut rig = Rig::new(1).await;
    let a = rig
        .state
        .engines
        .add(EngineDraft::new("A", "/fake/a"))
        .expect("a");
    let b = rig
        .state
        .engines
        .add(EngineDraft::new("B", "/fake/b"))
        .expect("b");
    let config = TournamentConfig {
        participants: vec![Participant::new(a.0, "A"), Participant::new(b.0, "B")],
        ..tournament_config()
    };
    let load = rig.update_only(Message::Tournament(TournamentMessage::Start {
        config: Box::new(config),
        at: Instant::now(),
    }));
    assert!(rig.state.tournament_live(), "being prepared");
    rig.connect().await;
    rig.game_start();
    rig.until("online game", |s| s.online.phase.is_live_for_lock())
        .await;
    // The slow load of the tournament finishes only now (D1331: nothing
    // assistive may start while the game is on).
    let tmp = tempfile::tempdir().expect("dir");
    let mut started = Vec::new();
    for effect in load {
        let effect = match effect {
            Effect::TournamentLoad {
                request,
                config,
                slug,
                dir: None,
                tablebase_dirs,
            } => Effect::TournamentLoad {
                request,
                config,
                slug,
                dir: Some(tmp.path().to_path_buf()),
                tablebase_dirs,
            },
            other => other,
        };
        if matches!(effect, Effect::TournamentLoad { .. }) {
            let answer = mychess_app::perform(effect).await;
            started.extend(update(&mut rig.state, answer));
        }
    }
    let engine_work = started.iter().any(|e| {
        matches!(
            e,
            Effect::TournamentPrepare { .. }
                | Effect::TournamentSearch { .. }
                | Effect::StartEngine { .. }
                | Effect::EngineCommand { .. }
        )
    });
    assert!(
        !engine_work,
        "no engine may start during the online game: {started:?}"
    );
    assert!(rig.lock());
    assert!(
        !rig.state.tournament_live(),
        "the tournament must not run beside the online game"
    );
}

#[tokio::test]
async fn an_online_game_in_progress_forbids_a_tournament_resume_too() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    let effects = rig.update_only(Message::Tournament(TournamentMessage::Resume {
        path: rig.dir.path().join("none.toml"),
        replacements: Vec::new(),
        at: Instant::now(),
    }));
    assert!(effects.is_empty());
    assert!(!rig.state.tournament_live());
}

#[tokio::test]
async fn an_automatic_annotation_is_not_possible_during_the_game() {
    let mut rig = Rig::new(1).await;
    rig.start_game().await;
    rig.update_only(Message::Annotate(AnnotateMessage::Start));
    assert!(!rig.state.annotation_running());
}
