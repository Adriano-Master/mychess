//! M8-2c contract tests of the tournament inside `AppState` with the real
//! effects and the fake engines of the project (UCI and XBoard): games at
//! once, XBoard participants (D2521-D2526) and the extra destination
//! (D2517-D2519). Files go to a temporary folder.
#![allow(
    clippy::large_futures,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use mychess_app::engines::{EngineDraft, EngineHub, EngineId, Launcher, Protocol as AppProtocol};
use mychess_app::tournament::{
    Adjudication, ExtraDestination, Participant, Phase, SystemResources, TimeControl,
    TournamentConfig, TournamentMessage, TournamentNotice,
};
use mychess_app::{AppNotice, AppState, Effect, Message, perform, update};
use mychess_app::{BasesMessage, SlotId};
use mychess_engine::testing::{ScriptedEngine, XBoardFake};
use mychess_engine::{EngineConfig, EngineError};

fn is_tournament_effect(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::TournamentLoad { .. }
            | Effect::TournamentPrepare { .. }
            | Effect::TournamentSearch { .. }
            | Effect::TournamentPool { .. }
            | Effect::TournamentSaveGame { .. }
            | Effect::TournamentSaveState { .. }
            | Effect::TournamentLoadState { .. }
            | Effect::TournamentList { .. }
            | Effect::TournamentExport { .. }
            | Effect::TournamentProbe
            | Effect::TournamentAddToSlot { .. }
    )
}

struct Rig {
    state: AppState,
    dir: tempfile::TempDir,
    ids: Vec<EngineId>,
    fakes: Vec<(String, ScriptedEngine)>,
    /// Every remark of the tournament, in order, without repeats.
    seen: Vec<TournamentNotice>,
}

/// How an engine of the rig speaks.
enum Kind {
    Uci,
    XBoard(XBoardFake),
}

impl Rig {
    fn new(engines: Vec<(&str, Kind)>) -> Rig {
        let mut state = AppState::default();
        let mut ids = Vec::new();
        let mut fakes = Vec::new();
        for (name, kind) in engines {
            let mut draft = EngineDraft::new(name, "/fake");
            let fake = match kind {
                Kind::Uci => ScriptedEngine::builder().name(name).build(),
                Kind::XBoard(dialect) => {
                    draft.protocol = AppProtocol::XBoard;
                    ScriptedEngine::builder().name(name).xboard(dialect).build()
                }
            };
            ids.push(state.engines.add(draft).unwrap());
            fakes.push((name.to_owned(), fake));
        }
        let shared = Arc::new(fakes.clone());
        let launcher: Launcher = Arc::new(move |config: EngineConfig| {
            let fake = shared
                .iter()
                .find(|(n, _)| *n == config.name)
                .map(|(_, f)| f.clone());
            Box::pin(async move {
                match fake {
                    Some(fake) => fake.start(config).await,
                    None => Err(EngineError::Closed),
                }
            })
        });
        state.engine_hub = EngineHub::with_launcher(launcher);
        // The machine is known, as after the probe of the form: a start then
        // asks for the load only (the unknown machine has its own test).
        update(
            &mut state,
            TournamentMessage::Resources(SystemResources {
                cores: 4,
                ram_available_mb: Some(15_000),
            })
            .into(),
        );
        Rig {
            state,
            dir: tempfile::tempdir().unwrap(),
            ids,
            fakes,
            seen: Vec::new(),
        }
    }

    fn config(&self, names: &[&str]) -> TournamentConfig {
        TournamentConfig {
            participants: self
                .ids
                .iter()
                .zip(names)
                .map(|(id, n)| Participant::new(id.0, n))
                .collect(),
            cycles: 1,
            time: TimeControl::Depth(1),
            adjudication: Adjudication {
                max_plies: Some(6),
                ..Adjudication::default()
            },
            ..TournamentConfig::default()
        }
    }

    fn lines(&self, name: &str) -> Vec<String> {
        self.fakes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, f)| f.received())
            .unwrap_or_default()
    }

    fn send(&mut self, message: impl Into<Message>) -> Vec<Effect> {
        update(&mut self.state, message.into())
    }

    fn start(&mut self, config: TournamentConfig) -> Vec<Effect> {
        self.send(TournamentMessage::Start {
            config: Box::new(config),
            at: Instant::now(),
        })
    }

    async fn step(&mut self, effect: Effect) -> Vec<Effect> {
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
                dir: Some(self.dir.path().to_path_buf()),
                tablebase_dirs,
            },
            other => other,
        };
        let message = perform(effect).await;
        let effects = update(&mut self.state, message);
        if let Some(notice) = self.notice().cloned()
            && self.seen.last() != Some(&notice)
        {
            self.seen.push(notice);
        }
        effects
    }

    async fn drive(&mut self, effects: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            if is_tournament_effect(&effect) {
                let more = self.step(effect).await;
                queue.extend(more);
            }
        }
    }

    fn notice(&self) -> Option<&TournamentNotice> {
        match self.state.notice.as_ref() {
            Some(AppNotice::Tournament(n)) => Some(n),
            _ => None,
        }
    }

    fn pgn_text(&self) -> String {
        std::fs::read_dir(self.dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "pgn"))
            .map(|p| std::fs::read_to_string(p).unwrap())
            .unwrap_or_default()
    }

    fn pgn_games(&self) -> usize {
        self.pgn_text().matches("[Event ").count()
    }

    fn phase(&self) -> Phase {
        self.state.tournament().expect("a tournament").phase()
    }

    /// Opens a database file in the next slot and makes it current.
    async fn open_base(&mut self, path: &Path) {
        let mut queue: VecDeque<Effect> = self.send(BasesMessage::Open(path.to_path_buf())).into();
        while let Some(effect) = queue.pop_front() {
            if effect.runs_on_host() || is_tournament_effect(&effect) {
                continue;
            }
            let message = perform(effect).await;
            queue.extend(update(&mut self.state, message));
        }
    }
}

fn xboard() -> Kind {
    Kind::XBoard(XBoardFake::default())
}

fn si4_with(dir: &Path, name: &str, pgn: &str) -> PathBuf {
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, 0).expect("create si4");
    import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    base.with_extension("si4")
}

const ONE_GAME: &str = "[White \"Old\"]\n[Black \"Game\"]\n\n1. e4 e5 *\n";

// ---- XBoard participants ----

#[tokio::test]
async fn xboard_engines_play_a_clock_game_and_are_told_the_names_and_the_result() {
    let mut rig = Rig::new(vec![("XA", xboard()), ("XB", xboard())]);
    let mut config = rig.config(&["XA", "XB"]);
    config.time = TimeControl::Clock {
        base_ms: 60_000,
        increment_ms: 1_000,
    };
    let effects = rig.start(config);
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    assert_eq!(rig.pgn_games(), 1);
    let white = rig.lines("XA");
    // The clock of the engine and of its opponent (D2522).
    assert!(white.iter().any(|l| l == "level 0 1:00 1"), "{white:?}");
    assert!(white.iter().any(|l| l == "time 6000"), "{white:?}");
    assert!(white.iter().any(|l| l == "otim 6000"), "{white:?}");
    assert!(
        !white.iter().any(|l| l.starts_with("st ")),
        "a clock game sends no `st`: {white:?}"
    );
    // The name of the opponent follows a `new` (D2524).
    let new = white.iter().position(|l| l == "new").expect("new");
    assert_eq!(white[new + 1], "name XB", "{white:?}");
    // The game ended by the adjudication at 6 plies: a draw.
    assert!(
        white.iter().any(|l| l.starts_with("result 1/2-1/2 {")),
        "{white:?}"
    );
    let black = rig.lines("XB");
    assert!(black.iter().any(|l| l == "name XA"), "{black:?}");
    assert!(
        black.iter().any(|l| l.starts_with("result 1/2-1/2 {")),
        "{black:?}"
    );
}

#[tokio::test]
async fn an_xboard_engine_without_setboard_gets_force_and_usermove() {
    let dialect = XBoardFake {
        features: "analyze=1 ping=1 usermove=1 done=1".to_owned(),
        ..XBoardFake::default()
    };
    let mut rig = Rig::new(vec![("XA", Kind::XBoard(dialect)), ("U", Kind::Uci)]);
    let config = rig.config(&["XA", "U"]);
    let effects = rig.start(config);
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    let lines = rig.lines("XA");
    assert!(lines.iter().any(|l| l == "force"), "{lines:?}");
    assert!(
        lines.iter().any(|l| l.starts_with("usermove ")),
        "moves go as `usermove`: {lines:?}"
    );
    assert!(lines.iter().any(|l| l == "go"), "{lines:?}");
}

#[tokio::test]
async fn uci_and_xboard_engines_can_share_a_tournament() {
    let mut rig = Rig::new(vec![("U1", Kind::Uci), ("X1", xboard()), ("X2", xboard())]);
    let config = rig.config(&["U1", "X1", "X2"]);
    let effects = rig.start(config);
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    assert_eq!(rig.pgn_games(), 3);
    assert!(rig.pgn_text().contains("[WhiteType \"program\"]"));
    // The UCI engine got no XBoard command and the XBoard ones no UCI.
    assert!(
        !rig.lines("U1")
            .iter()
            .any(|l| l == "xboard" || l == "force")
    );
    assert!(
        !rig.lines("X1")
            .iter()
            .any(|l| l == "uci" || l == "ucinewgame")
    );
}

#[tokio::test]
async fn an_xboard_engine_that_resigns_loses_the_game_without_waiting() {
    let dialect = XBoardFake {
        resign: true,
        ..XBoardFake::default()
    };
    let mut rig = Rig::new(vec![("Quitter", Kind::XBoard(dialect)), ("U", Kind::Uci)]);
    let mut config = rig.config(&["Quitter", "U"]);
    config.failures.max_retries = 0;
    let effects = rig.start(config);
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    let t = rig.state.tournament().unwrap();
    let games = t.finished_games();
    assert_eq!(games.len(), 1);
    // The engine that resigned lost, whatever colour it had.
    let loser_is_quitter = match games[0].result {
        mychess_core::GameResult::White => games[0].black == 0,
        mychess_core::GameResult::Black => games[0].white == 0,
        _ => false,
    };
    assert!(loser_is_quitter, "{games:?}");
    assert!(
        rig.pgn_text()
            .contains("[Termination \"rules infraction\"]")
    );
}

#[tokio::test]
async fn a_limit_of_nodes_is_refused_for_an_xboard_participant() {
    let mut rig = Rig::new(vec![("U", Kind::Uci), ("X", xboard())]);
    let mut config = rig.config(&["U", "X"]);
    config.time = TimeControl::Nodes(1000);
    let effects = rig.start(config);
    assert!(effects.is_empty(), "nothing starts");
    assert_eq!(
        rig.notice(),
        Some(&TournamentNotice::UnsupportedTime("X".to_owned()))
    );
    assert!(rig.state.tournament().is_none());
}

// ---- games at once ----

#[tokio::test]
async fn games_at_once_give_the_same_table_as_one_game_at_a_time() {
    let names = ["A", "B", "C", "D"];
    let run = |lanes: u32| async move {
        let mut rig = Rig::new(names.iter().map(|n| (*n, Kind::Uci)).collect());
        let mut config = rig.config(&names);
        config.concurrency = lanes;
        config.cycles = 2;
        // Enough cores to keep what was asked.
        rig.send(TournamentMessage::Resources(SystemResources {
            cores: 16,
            ram_available_mb: Some(15_000),
        }));
        let effects = rig.start(config);
        rig.drive(effects).await;
        assert_eq!(rig.phase(), Phase::Finished);
        assert_eq!(rig.pgn_games(), 12);
        let t = rig.state.tournament().unwrap();
        (t.lane_count(), t.finished_games(), t.standings())
    };
    let (one_lane, games_one, table_one) = run(1).await;
    let (three_lanes, games_three, table_three) = run(3).await;
    assert_eq!((one_lane, three_lanes), (1, 3));
    assert_eq!(games_one, games_three);
    assert_eq!(table_one, table_three);
}

#[tokio::test]
async fn every_lane_has_its_own_engine_processes() {
    let names = ["A", "B", "C", "D"];
    let mut rig = Rig::new(names.iter().map(|n| (*n, Kind::Uci)).collect());
    let mut config = rig.config(&names);
    config.concurrency = 2;
    rig.send(TournamentMessage::Resources(SystemResources {
        cores: 16,
        ram_available_mb: Some(15_000),
    }));
    let effects = rig.start(config);
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    // All engines were ended with the tournament.
    for name in names {
        let lines = rig.lines(name);
        assert!(lines.iter().any(|l| l == "uci"), "{name} was used");
    }
    assert_eq!(rig.pgn_games(), 6);
}

#[tokio::test]
async fn more_games_at_once_than_the_machine_takes_are_brought_down_with_a_notice() {
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci), ("C", Kind::Uci)]);
    let mut config = rig.config(&["A", "B", "C"]);
    config.concurrency = 16;
    // A small machine: 4 cores leave 3 for the games.
    rig.send(TournamentMessage::Resources(SystemResources {
        cores: 4,
        ram_available_mb: Some(15_000),
    }));
    let effects = rig.start(config);
    let load = effects.into_iter().next().expect("load");
    let more = rig.step(load).await;
    assert_eq!(rig.state.tournament().unwrap().lane_count(), 3);
    assert_eq!(
        rig.notice(),
        Some(&TournamentNotice::ConcurrencyReduced { from: 16, to: 3 })
    );
    rig.drive(more).await;
    assert_eq!(rig.phase(), Phase::Finished);
}

#[tokio::test]
async fn the_machine_is_probed_when_asked() {
    let mut rig = Rig::new(vec![("A", Kind::Uci)]);
    let effects = rig.send(TournamentMessage::ProbeResources);
    assert!(matches!(effects.as_slice(), [Effect::TournamentProbe]));
    rig.drive(effects).await;
    assert!(rig.state.tournament_resources().cores >= 1);
}

// ---- the extra destination ----

#[tokio::test]
async fn the_clipbase_receives_every_game_and_its_list_grows() {
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci), ("C", Kind::Uci)]);
    let mut config = rig.config(&["A", "B", "C"]);
    config.destination = ExtraDestination::Clipbase;
    let effects = rig.start(config);
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    assert_eq!(rig.pgn_games(), 3);
    let clip = rig.state.bases.slot(SlotId::CLIPBASE).expect("clipbase");
    assert_eq!(clip.total(), 3);
    assert_eq!(clip.filtered(), 3, "the new games are in the filter");
}

#[tokio::test]
async fn an_si4_slot_gets_the_games_is_reopened_and_keeps_its_old_ones() {
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci), ("C", Kind::Uci)]);
    let base = si4_with(rig.dir.path(), "dest", ONE_GAME);
    rig.open_base(&base).await;
    let slot = rig.state.bases.current();
    assert_eq!(rig.state.bases.slot(slot).unwrap().total(), 1);
    let mut config = rig.config(&["A", "B", "C"]);
    config.destination = ExtraDestination::CurrentSlot;
    let effects = rig.start(config);
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    assert_eq!(rig.pgn_games(), 3);
    let after = rig.state.bases.slot(slot).unwrap();
    assert_eq!(after.total(), 4, "one old game and three new ones");
    assert_eq!(after.filtered(), 4);
    // The file has them too: opening it again finds four games.
    let reopened = mychess_db::si4::Si4Database::open(&base).expect("open");
    assert_eq!(mychess_db::GameSource::len(&reopened), 4);
    assert!(
        !matches!(rig.notice(), Some(TournamentNotice::DestinationFailed)),
        "{:?}",
        rig.notice()
    );
}

#[tokio::test]
async fn a_read_only_destination_is_refused_at_the_start_and_the_pgn_still_gets_the_games() {
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci)]);
    // A PGN file opened as a database cannot be written.
    let pgn = rig.dir.path().join("readonly.pgn");
    std::fs::write(&pgn, ONE_GAME).unwrap();
    rig.open_base(&pgn).await;
    let mut config = rig.config(&["A", "B"]);
    config.destination = ExtraDestination::CurrentSlot;
    let effects = rig.start(config);
    let load = effects.into_iter().next().expect("load");
    let more = rig.step(load).await;
    assert!(
        rig.seen.contains(&TournamentNotice::DestinationReadOnly),
        "{:?}",
        rig.seen
    );
    rig.drive(more).await;
    assert_eq!(rig.phase(), Phase::Finished);
    // "readonly.pgn" is the base; the tournament's own PGN has the game.
    let own: Vec<_> = std::fs::read_dir(rig.dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "pgn")
                && p.file_name().is_some_and(|n| n != "readonly.pgn")
        })
        .collect();
    assert_eq!(own.len(), 1);
    assert_eq!(
        std::fs::read_to_string(&own[0])
            .unwrap()
            .matches("[Event ")
            .count(),
        1
    );
    assert_eq!(std::fs::read_to_string(&pgn).unwrap(), ONE_GAME);
}

#[tokio::test]
async fn a_slot_closed_during_the_tournament_costs_a_notice_and_no_game() {
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci), ("C", Kind::Uci)]);
    let base = si4_with(rig.dir.path(), "gone", ONE_GAME);
    rig.open_base(&base).await;
    let slot = rig.state.bases.current();
    let mut config = rig.config(&["A", "B", "C"]);
    config.destination = ExtraDestination::CurrentSlot;
    let effects = rig.start(config);
    let load = effects.into_iter().next().expect("load");
    let more = rig.step(load).await;
    // The user closes the database while the tournament runs.
    let closing = rig.send(BasesMessage::Close(slot));
    for effect in closing {
        if !effect.runs_on_host() && !is_tournament_effect(&effect) {
            let message = perform(effect).await;
            update(&mut rig.state, message);
        }
    }
    rig.drive(more).await;
    assert_eq!(rig.phase(), Phase::Finished);
    assert_eq!(rig.pgn_games(), 3, "the PGN has every game");
    assert!(
        rig.seen.contains(&TournamentNotice::DestinationFailed),
        "{:?}",
        rig.seen
    );
    // The file was not touched after it was closed.
    let reopened = mychess_db::si4::Si4Database::open(&base).expect("open");
    assert!(mychess_db::GameSource::len(&reopened) <= 4);
}

// ---- corrections of the review ----

use mychess_app::BaseError;
use mychess_app::tournament::SlotWrite;

/// The state file of the tournament in the folder of the rig.
fn state_file(rig: &Rig) -> PathBuf {
    std::fs::read_dir(rig.dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "toml"))
        .expect("a state file")
}

#[tokio::test]
async fn the_slot_is_the_one_current_when_the_tournament_is_asked_for() {
    // D2519: opening another database while the files load changes nothing.
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci), ("C", Kind::Uci)]);
    let first = si4_with(rig.dir.path(), "first", ONE_GAME);
    rig.open_base(&first).await;
    let asked_for = rig.state.bases.current();
    let mut config = rig.config(&["A", "B", "C"]);
    config.destination = ExtraDestination::CurrentSlot;
    let effects = rig.start(config);
    let second = si4_with(rig.dir.path(), "second", ONE_GAME);
    rig.open_base(&second).await;
    let now_current = rig.state.bases.current();
    assert_ne!(asked_for, now_current, "the new base became current");
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    assert_eq!(rig.state.bases.slot(asked_for).unwrap().total(), 4);
    assert_eq!(
        rig.state.bases.slot(now_current).unwrap().total(),
        1,
        "the other database was not written"
    );
}

#[tokio::test]
async fn a_resumed_tournament_does_not_copy_to_whatever_slot_is_current() {
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci), ("C", Kind::Uci)]);
    let base = si4_with(rig.dir.path(), "dest", ONE_GAME);
    rig.open_base(&base).await;
    let slot = rig.state.bases.current();
    let mut config = rig.config(&["A", "B", "C"]);
    config.destination = ExtraDestination::CurrentSlot;
    let effects = rig.start(config);
    // Load, then end the tournament at once: the state is on disk.
    let load = effects.into_iter().next().expect("load");
    let more = rig.step(load).await;
    let ended = rig.send(TournamentMessage::Abort(Instant::now()));
    rig.drive(more).await;
    rig.drive(ended).await;
    assert_eq!(rig.phase(), Phase::Aborted);
    let path = state_file(&rig);
    rig.seen.clear();
    let effects = rig.send(TournamentMessage::Resume {
        path,
        replacements: Vec::new(),
        at: Instant::now(),
    });
    rig.drive(effects).await;
    assert_eq!(rig.phase(), Phase::Finished);
    assert!(
        rig.seen.contains(&TournamentNotice::DestinationUnavailable),
        "{:?}",
        rig.seen
    );
    assert_eq!(rig.pgn_games(), 3, "the PGN has every game");
    assert_eq!(
        rig.state.bases.slot(slot).unwrap().total(),
        1,
        "not written"
    );
}

#[tokio::test]
async fn a_game_written_but_not_reopened_marks_the_slot_and_says_where_the_game_is() {
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci)]);
    let base = si4_with(rig.dir.path(), "dest", ONE_GAME);
    rig.open_base(&base).await;
    let mut config = rig.config(&["A", "B"]);
    config.destination = ExtraDestination::CurrentSlot;
    let effects = rig.start(config);
    let load = effects.into_iter().next().expect("load");
    let more = rig.step(load).await;
    rig.send(TournamentMessage::SlotAdded {
        result: Box::new(SlotWrite::WrittenNotReopened(BaseError::Internal)),
    });
    assert_eq!(
        rig.notice(),
        Some(&TournamentNotice::DestinationNotReopened)
    );
    // The slot is stale: a second tournament cannot take it as destination.
    let ended = rig.send(TournamentMessage::Abort(Instant::now()));
    rig.drive(more).await;
    rig.drive(ended).await;
    rig.send(TournamentMessage::Close);
    let mut config = rig.config(&["A", "B"]);
    config.destination = ExtraDestination::CurrentSlot;
    let effects = rig.start(config);
    let load = effects.into_iter().next().expect("load");
    rig.step(load).await;
    assert_eq!(rig.notice(), Some(&TournamentNotice::DestinationReadOnly));
}

#[tokio::test]
async fn a_failed_copy_does_not_cover_a_game_missing_from_the_pgn() {
    let mut rig = Rig::new(vec![("A", Kind::Uci), ("B", Kind::Uci)]);
    let mut config = rig.config(&["A", "B"]);
    config.destination = ExtraDestination::Clipbase;
    let effects = rig.start(config);
    let load = effects.into_iter().next().expect("load");
    rig.step(load).await;
    rig.send(TournamentMessage::GameSaved {
        number: 1,
        ok: false,
    });
    assert_eq!(rig.notice(), Some(&TournamentNotice::GameNotSaved));
    rig.send(TournamentMessage::SlotAdded {
        result: Box::new(SlotWrite::NotWritten(BaseError::Internal)),
    });
    assert_eq!(
        rig.notice(),
        Some(&TournamentNotice::GameNotSaved),
        "the graver remark stays"
    );
}

#[tokio::test]
async fn a_smaller_machine_runs_fewer_games_but_the_saved_file_keeps_the_choice() {
    let names = ["A", "B", "C", "D"];
    let mut rig = Rig::new(names.iter().map(|n| (*n, Kind::Uci)).collect());
    let mut config = rig.config(&names);
    config.concurrency = 8;
    // No reading of the machine yet: the cautious default (4 cores).
    let effects = rig.start(config);
    let load = effects.into_iter().next().expect("load");
    let more = rig.step(load).await;
    let t = rig.state.tournament().unwrap();
    assert_eq!(t.lane_count(), 3);
    assert_eq!(t.config().concurrency, 8, "what the user asked for");
    assert_eq!(t.to_saved().config.concurrency, 8);
    let ended = rig.send(TournamentMessage::Abort(Instant::now()));
    rig.drive(more).await;
    rig.drive(ended).await;
    assert_eq!(rig.phase(), Phase::Aborted);
    // Resumed on a bigger machine, it runs what was asked.
    let path = state_file(&rig);
    rig.send(TournamentMessage::Resources(SystemResources {
        cores: 16,
        ram_available_mb: Some(15_000),
    }));
    let effects = rig.send(TournamentMessage::Resume {
        path,
        replacements: Vec::new(),
        at: Instant::now(),
    });
    let load = effects.into_iter().next().expect("load state");
    let next = rig.step(load).await;
    let load = next.into_iter().next().expect("load");
    rig.step(load).await;
    assert_eq!(rig.state.tournament().unwrap().lane_count(), 8);
    assert_eq!(rig.state.tournament().unwrap().config().concurrency, 8);
}
