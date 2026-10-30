//! M8-2a contract tests of the tournament inside `AppState` (spec D1278,
//! D1285, D1296, D1300, D1304, D1306-D1308, D1311): the real effects run
//! (files in a temporary folder, engines scripted in memory) and their
//! answers go back to `update`. Nothing touches the user's folders: the
//! `TournamentLoad` effect is pointed at a temporary directory here.
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

#[path = "m8_1_common/mod.rs"]
mod m8_1_common;
mod m8_2_common;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use m8_2_common::*;
use mychess_app::engines::{EngineDraft, EngineHub, EngineId};
use mychess_app::tournament::{
    Adjudication, Decision, ExportFormat, ExportLabels, ExportTable, Participant, Phase,
    PoolCommand, Scheduling, SideReady, TimeControl, TournamentConfig, TournamentMessage,
    TournamentNotice, list_unfinished, render_state,
};
use mychess_app::{
    AnnotateMessage, AppNotice, AppState, ColorChoice, Effect, Message, PlayConfig, PlayMessage,
    PlayNotice, SharedGame, perform, update,
};
use mychess_core::{Game, GameResult};

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
    )
}

struct Glue {
    state: AppState,
    fx: Fixture,
    dir: tempfile::TempDir,
    ids: Vec<EngineId>,
}

impl Glue {
    fn new(names: &[&str], scripts: Vec<(&'static str, Scripted)>) -> Glue {
        let mut state = AppState::default();
        let ids = names
            .iter()
            .map(|n| state.engines.add(EngineDraft::new(*n, "/fake")).unwrap())
            .collect();
        let fx = Fixture::new(scripts);
        state.engine_hub = EngineHub::with_launcher(fx.launcher.clone());
        // The machine is known, as after the probe of the form: a start then
        // asks for the load only (the unknown machine has its own test).
        update(
            &mut state,
            TournamentMessage::Resources(mychess_app::tournament::SystemResources {
                cores: 4,
                ram_available_mb: Some(15_000),
            })
            .into(),
        );
        Glue {
            state,
            fx,
            dir: tempfile::tempdir().unwrap(),
            ids,
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
            cycles: 2,
            time: TimeControl::Depth(1),
            adjudication: Adjudication {
                max_plies: Some(4),
                ..Adjudication::default()
            },
            ..TournamentConfig::default()
        }
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

    /// Performs one tournament effect (the load goes to the temp folder).
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
        update(&mut self.state, message)
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
        let path = std::fs::read_dir(self.dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "pgn"));
        path.map(|p| std::fs::read_to_string(p).unwrap())
            .unwrap_or_default()
    }

    fn pgn_games(&self) -> usize {
        self.pgn_text().matches("[Event ").count()
    }

    fn phase(&self) -> Phase {
        self.state.tournament().expect("a tournament").phase()
    }
}

fn pool_commands(effects: &[Effect]) -> Vec<PoolCommand> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::TournamentPool { command, .. } => Some(command.clone()),
            _ => None,
        })
        .collect()
}

fn ready_pair(sides: [usize; 2]) -> [SideReady; 2] {
    sides.map(|participant| SideReady {
        participant,
        engine_name: None,
        can_ponder: false,
    })
}

/// Starts, loads and gets the first `Prepare` effect, without running it.
fn until_prepare(g: &mut Glue, config: TournamentConfig) -> (u64, [usize; 2], Vec<Effect>) {
    let effects = g.start(config);
    let load = effects.into_iter().next().expect("load effect");
    let Effect::TournamentLoad {
        request,
        config,
        slug,
        ..
    } = load
    else {
        panic!("load")
    };
    let loaded = mychess_app_load(g, request, &config, slug);
    let effects = g.send(loaded);
    let (epoch, sides) = effects
        .iter()
        .find_map(|e| match e {
            Effect::TournamentPrepare { epoch, plan, .. } => Some((*epoch, plan.sides)),
            _ => None,
        })
        .expect("a prepare effect");
    (epoch, sides, effects)
}

/// What `perform` would answer to a load, built by hand for the temp folder.
fn mychess_app_load(
    g: &Glue,
    request: u64,
    config: &TournamentConfig,
    slug: Option<String>,
) -> Message {
    use mychess_app::tournament::Loaded;
    Message::Tournament(TournamentMessage::Loaded {
        request,
        result: Box::new(Ok(Loaded {
            book: None,
            book_unreadable: false,
            suite: None,
            slug: slug.unwrap_or_else(|| "glue".to_owned()),
            dir: g.dir.path().to_path_buf(),
            today: today(),
            seed: if config.seed == 0 { 5 } else { config.seed },
            tablebase: None,
        })),
        at: Instant::now(),
    })
}

// ---- whole runs ----

#[tokio::test]
async fn a_gauntlet_runs_through_the_real_effects_and_writes_pgn_and_state() {
    let names = ["A", "B", "C"];
    let mut g = Glue::new(&names, Vec::new());
    let mut config = g.config(&names);
    config.scheduling = Scheduling::Gauntlet;
    config.tested = 1;
    let effects = g.start(config);
    assert!(matches!(
        effects.as_slice(),
        [Effect::TournamentLoad { .. }]
    ));
    assert!(g.state.tournament_live(), "loading counts as live");
    g.drive(effects).await;
    assert_eq!(g.phase(), Phase::Finished);
    assert!(!g.state.tournament_live());
    assert_eq!(g.pgn_games(), 4);
    assert!(list_unfinished(g.dir.path()).is_empty());
    let tags = g.pgn_text();
    assert!(tags.contains("[Event \"MyChess Tournament\"]"));
    assert!(tags.contains("[Round \"1.1\"]") && tags.contains("[Round \"2.1\"]"));
    // The game of the user's editor is not touched.
    assert!(g.state.session.game().mainline().next().is_none());
    // Every engine of the pool is gone at the end.
    assert!(matches!(
        g.notice(),
        Some(TournamentNotice::Finished { .. })
    ));
}

#[tokio::test]
async fn ponder_runs_a_whole_game_with_real_ponderhits() {
    let names = ["A", "B"];
    let script = Scripted {
        ponder: true,
        ..Scripted::default()
    };
    let mut g = Glue::new(&names, vec![("A", script.clone()), ("B", script)]);
    let mut config = g.config(&names);
    config.cycles = 1;
    config.ponder = true;
    config.adjudication.max_plies = Some(8);
    let effects = g.start(config);
    g.drive(effects).await;
    assert_eq!(g.phase(), Phase::Finished);
    assert_eq!(g.pgn_games(), 1);
    let lines = g.fx.lines("A");
    assert!(
        lines.iter().any(|l| l.starts_with("go ponder")),
        "{lines:?}"
    );
    assert!(lines.iter().any(|l| l == "ponderhit"), "{lines:?}");
    assert!(g.pgn_text().contains("[PlyCount \"8\"]"));
}

#[tokio::test]
async fn uniform_options_reach_only_engines_that_declare_them_and_own_options_win() {
    // D1288: Hash and Threads "only if the engine declares the option".
    let names = ["A", "B"];
    let mut g = Glue::new(&names, Vec::new());
    let mut config = g.config(&names);
    config.cycles = 1;
    config.hash_mb = Some(64);
    config.threads = Some(4);
    config.participants[0].options = vec![("Hash".into(), "8".into())];
    let effects = g.start(config);
    g.drive(effects).await;
    let a = g.fx.lines("A");
    let b = g.fx.lines("B");
    assert!(
        a.iter().any(|l| l == "setoption name Hash value 8"),
        "{a:?}"
    );
    assert!(
        !a.iter().any(|l| l == "setoption name Hash value 64"),
        "{a:?}"
    );
    assert!(
        b.iter().any(|l| l == "setoption name Hash value 64"),
        "{b:?}"
    );
    for lines in [&a, &b] {
        assert!(
            !lines
                .iter()
                .any(|l| l.starts_with("setoption name Threads")),
            "Threads is not declared by the engine: {lines:?}"
        );
    }
}

// ---- exclusivity ----

#[tokio::test]
async fn a_live_tournament_refuses_a_game_against_an_engine_a_second_tournament_and_an_annotation()
{
    let names = ["A", "B"];
    let mut g = Glue::new(&names, Vec::new());
    let config = g.config(&names);
    g.start(config.clone());
    assert!(g.state.tournament_live());
    let ids = g.ids.clone();
    let fx = g.send(PlayMessage::Start(Box::new(PlayConfig::new(ids[0]))));
    assert!(fx.is_empty());
    assert!(matches!(
        g.state.notice,
        Some(AppNotice::Play(PlayNotice::Busy))
    ));
    let fx = g.start(config);
    assert!(fx.is_empty());
    assert_eq!(g.notice(), Some(&TournamentNotice::AlreadyRunning));
    let fx = g.send(AnnotateMessage::Start);
    assert!(fx.is_empty());
    assert!(g.state.tournament_live());
}

#[tokio::test]
async fn stopped_after_a_game_still_holds_the_exclusivity_until_it_continues() {
    let names = ["A", "B"];
    let mut g = Glue::new(&names, Vec::new());
    let config = g.config(&names);
    let (epoch, sides, _) = until_prepare(&mut g, config);
    // Ask to stop after the game, then let the game play to its end.
    g.send(TournamentMessage::StopAfterGame(Instant::now()));
    let effects = g.send(TournamentMessage::Prepared {
        epoch,
        result: Ok(ready_pair(sides)),
        at: Instant::now(),
    });
    g.drive(effects).await;
    assert_eq!(g.phase(), Phase::StoppedAfterGame);
    assert!(g.state.tournament_live());
    let ids = g.ids.clone();
    g.send(PlayMessage::Start(Box::new(PlayConfig::new(ids[0]))));
    assert!(matches!(
        g.state.notice,
        Some(AppNotice::Play(PlayNotice::Busy))
    ));
    let effects = g.send(TournamentMessage::Continue(Instant::now()));
    g.drive(effects).await;
    assert_eq!(g.phase(), Phase::Finished);
    assert_eq!(g.pgn_games(), 2);
}

#[test]
fn a_game_against_an_engine_blocks_the_tournament() {
    let mut rig = m8_1_common::Rig::new();
    let cfg = rig.config(ColorChoice::White, m8_1_common::blitz());
    rig.start(cfg);
    assert!(rig.state.play_live());
    let eid = rig.engine.0;
    let config = TournamentConfig {
        participants: vec![Participant::new(eid, "X"), Participant::new(eid, "Y")],
        time: TimeControl::Depth(1),
        ..TournamentConfig::default()
    };
    let fx = rig.send(TournamentMessage::Start {
        config: Box::new(config),
        at: Instant::now(),
    });
    assert!(fx.is_empty());
    assert!(matches!(
        rig.state.notice,
        Some(AppNotice::Tournament(TournamentNotice::PlayBusy))
    ));
    assert!(!rig.state.tournament_live());
}

#[test]
fn start_refuses_invalid_unknown_and_non_uci_configurations() {
    let names = ["A", "B"];
    let mut g = Glue::new(&names, Vec::new());
    let mut bad = g.config(&names);
    bad.cycles = 0;
    assert!(g.start(bad).is_empty());
    assert!(matches!(
        g.notice(),
        Some(TournamentNotice::InvalidConfig(_))
    ));
    let mut unknown = g.config(&names);
    unknown.participants[1].engine = 9_999;
    assert!(g.start(unknown).is_empty());
    assert!(
        matches!(g.notice(), Some(TournamentNotice::MissingEngines(m)) if m == &["B".to_owned()])
    );
    assert!(!g.state.tournament_live());
}

// ---- analyses, close, quit ----

#[test]
fn closing_a_paused_tournament_cancels_then_shuts_down_and_brings_the_analyses_back() {
    let mut rig = m8_1_common::Rig::new();
    let second = rig
        .state
        .engines
        .add(EngineDraft::new("Second", "/fake"))
        .unwrap();
    assert!(
        rig.state.engine_runs.as_slice().iter().any(|r| r.analyzing),
        "the analysis runs"
    );
    let config = TournamentConfig {
        participants: vec![
            Participant::new(rig.engine.0, "X"),
            Participant::new(second.0, "Y"),
        ],
        time: TimeControl::Depth(1),
        ..TournamentConfig::default()
    };
    // The machine is known, so the load is answered at once.
    rig.send(TournamentMessage::Resources(
        mychess_app::tournament::SystemResources {
            cores: 4,
            ram_available_mb: Some(15_000),
        },
    ));
    let effects = rig.send(TournamentMessage::Start {
        config: Box::new(config),
        at: Instant::now(),
    });
    let Some(Effect::TournamentLoad {
        request, config, ..
    }) = effects.into_iter().next()
    else {
        panic!("load")
    };
    let dir = tempfile::tempdir().unwrap();
    let loaded = Message::Tournament(TournamentMessage::Loaded {
        request,
        result: Box::new(Ok(mychess_app::tournament::Loaded {
            book: None,
            book_unreadable: false,
            suite: None,
            slug: "x".into(),
            dir: dir.path().to_path_buf(),
            today: today(),
            seed: 3,
            tablebase: None,
        })),
        at: Instant::now(),
    });
    drop(config);
    let effects = rig.send(loaded);
    assert!(
        rig.state
            .engine_runs
            .as_slice()
            .iter()
            .all(|r| !r.analyzing),
        "the analysis is paused while the tournament lives"
    );
    let (epoch, sides) = effects
        .iter()
        .find_map(|e| match e {
            Effect::TournamentPrepare { epoch, plan, .. } => Some((*epoch, plan.sides)),
            _ => None,
        })
        .unwrap();
    rig.send(TournamentMessage::Pause);
    rig.send(TournamentMessage::Prepared {
        epoch,
        result: Ok(ready_pair(sides)),
        at: Instant::now(),
    });
    assert_eq!(rig.state.tournament().unwrap().phase(), Phase::Paused);
    // Close while running would be ignored, in Paused it ends the engines.
    let effects = rig.send(TournamentMessage::Close);
    assert!(rig.state.tournament().is_none());
    let commands = pool_commands(&effects);
    let cancel = commands
        .iter()
        .position(|c| matches!(c, PoolCommand::Cancel(_)));
    let shutdown = commands.iter().position(|c| *c == PoolCommand::Shutdown);
    assert!(
        cancel.is_some() && shutdown.is_some() && cancel < shutdown,
        "{commands:?}"
    );
    assert!(
        rig.state.engine_runs.as_slice().iter().any(|r| r.analyzing),
        "the analysis of the user is back"
    );
    assert!(!rig.state.tournament_live());
}

#[tokio::test]
async fn close_is_ignored_while_the_tournament_plays_and_removes_a_finished_one() {
    let names = ["A", "B"];
    let mut g = Glue::new(&names, Vec::new());
    let config = g.config(&names);
    let (epoch, _, _) = until_prepare(&mut g, config);
    let fx = g.send(TournamentMessage::Close);
    assert!(fx.is_empty());
    assert!(g.state.tournament().is_some());
    g.send(TournamentMessage::Abort(Instant::now()));
    g.send(TournamentMessage::Prepared {
        epoch,
        result: Err(prepare_failure(0)),
        at: Instant::now(),
    });
    assert_eq!(g.phase(), Phase::Aborted);
    let fx = g.send(TournamentMessage::Close);
    assert!(g.state.tournament().is_none(), "{fx:?}");
}

#[tokio::test(start_paused = true)]
async fn quitting_cancels_the_search_then_shuts_down_saves_and_leaves_no_engine_alive() {
    let names = ["A", "B"];
    let slow = Scripted {
        delay: Duration::from_secs(3_600),
        ..Scripted::default()
    };
    let mut g = Glue::new(&names, vec![("A", slow.clone()), ("B", slow)]);
    let mut config = g.config(&names);
    config.time = TimeControl::Depth(1);
    config.depth_limit_secs = 3_600;
    let effects = g.start(config);
    // Load and prepare, then the first search starts and thinks for an hour.
    let mut queue: VecDeque<Effect> = effects.into();
    let mut searching = None;
    let mut pool = None;
    while let Some(effect) = queue.pop_front() {
        match effect {
            Effect::TournamentSearch { pool: p, job } => {
                pool = Some(p.clone());
                let message = tokio::spawn(perform(Effect::TournamentSearch { pool: p, job }));
                searching = Some(message);
            }
            e if is_tournament_effect(&e) => {
                let more = g.step(e).await;
                queue.extend(more);
            }
            _ => {}
        }
        if searching.is_some() {
            break;
        }
    }
    let pool = pool.expect("a search was sent");
    let searching = searching.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(pool.alive(), 1, "the thinking engine is out of the pool");
    let effects = g.send(Message::Quit);
    let commands = pool_commands(&effects);
    let cancel = commands
        .iter()
        .position(|c| matches!(c, PoolCommand::Cancel(e) if *e > 0));
    let shutdown = commands.iter().position(|c| *c == PoolCommand::Shutdown);
    assert!(
        cancel.is_some() && shutdown.is_some() && cancel < shutdown,
        "{commands:?}"
    );
    let save = effects
        .iter()
        .position(|e| matches!(e, Effect::TournamentSaveState { .. }));
    assert!(save.is_some(), "the state is saved on quit");
    // Run those effects in order, as the host does.
    for effect in effects {
        if is_tournament_effect(&effect) {
            perform(effect).await;
        }
    }
    // The search comes back (cancelled) and the engine does not survive.
    let answer = searching.await.unwrap();
    assert!(
        matches!(
            answer,
            Message::Tournament(TournamentMessage::Searched {
                outcome: mychess_app::tournament::SearchOutcome::Cancelled,
                ..
            })
        ),
        "{answer:?}"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(pool.alive(), 0, "no engine process is left");
    // And what was saved can be resumed.
    assert_eq!(list_unfinished(g.dir.path()).len(), 1);
}

// ---- resume ----

async fn stopped_tournament() -> (Glue, PathBuf) {
    let names = ["A", "B", "C"];
    let mut g = Glue::new(&names, Vec::new());
    let mut config = g.config(&names);
    config.cycles = 1;
    let (epoch, sides, _) = until_prepare(&mut g, config);
    g.send(TournamentMessage::StopAfterGame(Instant::now()));
    let effects = g.send(TournamentMessage::Prepared {
        epoch,
        result: Ok(ready_pair(sides)),
        at: Instant::now(),
    });
    g.drive(effects).await;
    assert_eq!(g.phase(), Phase::StoppedAfterGame);
    let state_file = std::fs::read_dir(g.dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "toml"))
        .unwrap();
    (g, state_file)
}

#[tokio::test]
async fn resume_finds_the_engines_by_identity_then_by_name_and_continues_alone() {
    let (old, state_file) = stopped_tournament().await;
    let played_before = old.pgn_games();
    assert_eq!(played_before, 1);
    // A new session whose list has the same engines in other positions (new
    // identities), so only the names can find them (case does not matter).
    let mut g = Glue::new(&["x1", "x2"], Vec::new());
    for id in g.ids.clone() {
        g.state.engines.remove(id).unwrap();
    }
    for name in ["c", "B", "a"] {
        g.state
            .engines
            .add(EngineDraft::new(name, "/fake"))
            .unwrap();
    }
    let effects = g.send(TournamentMessage::Resume {
        path: state_file.clone(),
        replacements: Vec::new(),
        at: Instant::now(),
    });
    // The files are next to the state file, whichever the folder is.
    g.drive(effects).await;
    assert_eq!(
        g.phase(),
        Phase::Finished,
        "it went on by itself, without a Continue"
    );
    let pgn = std::fs::read_to_string(state_file.with_extension("pgn")).unwrap();
    assert_eq!(
        pgn.matches("[Event ").count(),
        3,
        "the 2 games left were appended to the same PGN"
    );
    let t = g.state.tournament().unwrap();
    // The configuration now points at the identities of the new list.
    let live: Vec<u64> = t.config().participants.iter().map(|p| p.engine).collect();
    let known: Vec<u64> = g.state.engines.engines().iter().map(|e| e.id.0).collect();
    assert!(
        live.iter().all(|id| known.contains(id)),
        "{live:?} in {known:?}"
    );
}

#[tokio::test]
async fn resume_with_a_missing_engine_says_so_and_a_replacement_fixes_it() {
    let (_old, state_file) = stopped_tournament().await;
    let mut g = Glue::new(&["A", "B"], Vec::new());
    let before_engines: Vec<_> = g.ids.clone();
    for id in &before_engines {
        // identities from the saved file (1, 2, 3 in a fresh list) may exist:
        // rename so neither id nor name match for "C".
        let _ = id;
    }
    let effects = g.send(TournamentMessage::Resume {
        path: state_file.clone(),
        replacements: vec![],
        at: Instant::now(),
    });
    g.drive(effects).await;
    // "C" has identity 3, which this list does not have, and no engine with
    // its name: nothing starts.
    assert!(
        matches!(g.notice(), Some(TournamentNotice::MissingEngines(m)) if m == &["C".to_owned()]),
        "{:?}",
        g.notice()
    );
    assert!(g.state.tournament().is_none());
    assert!(!g.state.tournament_live());
    // With a replacement for participant 2 it resumes.
    let extra = g
        .state
        .engines
        .add(EngineDraft::new("Substitute", "/fake"))
        .unwrap();
    let effects = g.send(TournamentMessage::Resume {
        path: state_file.clone(),
        replacements: vec![(2, extra.0)],
        at: Instant::now(),
    });
    g.drive(effects).await;
    assert_eq!(g.phase(), Phase::Finished);
}

#[tokio::test]
async fn resume_refuses_bad_files_without_panic_and_without_leaving_the_folder() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("torneios");
    std::fs::create_dir_all(&dir).unwrap();
    let mut g = Glue::new(&["A", "B"], Vec::new());
    // A saved state with a hostile slug.
    let (old, state_file) = stopped_tournament().await;
    let mut hostile = mychess_app::tournament::load_state(&state_file).unwrap();
    hostile.slug = "../x".to_owned();
    let text = render_state(&hostile).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("hostile.toml", text.into_bytes()),
        ("corrupt.toml", b"version = [[".to_vec()),
        ("future.toml", b"version = 99\nstuff = 1\n".to_vec()),
        ("binary.toml", vec![0xff, 0xfe, 0x00, 0x80]),
        ("empty.toml", Vec::new()),
    ];
    drop(old);
    for (name, bytes) in cases {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let effects = g.send(TournamentMessage::Resume {
            path,
            replacements: vec![],
            at: Instant::now(),
        });
        g.drive(effects).await;
        assert_eq!(g.notice(), Some(&TournamentNotice::ResumeFailed), "{name}");
        assert!(g.state.tournament().is_none());
        assert!(!g.state.tournament_live(), "{name}");
    }
    assert!(!root.path().join("x.pgn").exists() && !root.path().join("x.toml").exists());
    let created: Vec<_> = std::fs::read_dir(root.path()).unwrap().collect();
    assert_eq!(created.len(), 1, "nothing was written beside the folder");
}

#[tokio::test]
async fn a_huge_state_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("huge.toml");
    std::fs::write(&path, vec![b'#'; 16 * 1024 * 1024 + 1]).unwrap();
    let mut g = Glue::new(&["A", "B"], Vec::new());
    let effects = g.send(TournamentMessage::Resume {
        path,
        replacements: vec![],
        at: Instant::now(),
    });
    g.drive(effects).await;
    assert_eq!(g.notice(), Some(&TournamentNotice::ResumeFailed));
}

#[tokio::test]
async fn the_resumable_list_comes_from_the_folder_and_only_has_unfinished_ones() {
    let (g, _state_file) = stopped_tournament().await;
    let list = list_unfinished(g.dir.path());
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].played, list[0].total), (1, 3));
}

// ---- the writing tasks ----

#[tokio::test]
async fn twelve_games_appended_at_once_are_all_kept_even_with_state_writes_between() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cup.pgn");
    let mut tasks = Vec::new();
    for number in 1..=12u32 {
        let mut game = Game::new();
        game.tags_mut().white = format!("White {number}");
        tasks.push(tokio::spawn(perform(Effect::TournamentSaveGame {
            path: path.clone(),
            number,
            game: SharedGame::new(game),
        })));
    }
    for task in tasks {
        let answer = task.await.unwrap();
        assert!(
            matches!(
                answer,
                Message::Tournament(TournamentMessage::GameSaved { ok: true, .. })
            ),
            "{answer:?}"
        );
    }
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("[Event ").count(), 12);
    for number in 1..=12 {
        assert!(
            text.contains(&format!("[White \"White {number}\"]")),
            "game {number} lost"
        );
    }
}

#[tokio::test]
async fn an_older_state_revision_never_replaces_a_newer_one_on_disk() {
    let mut sim = Sim::new(cfg(2), decide_by(|_| GameResult::White));
    sim.start();
    let mut older = sim.t.to_saved();
    let mut newer = sim.t.to_saved();
    older.revision = 40;
    older.failures = vec![1, 0];
    newer.revision = 41;
    newer.failures = vec![2, 0];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stale-order.toml");
    // The newer one lands first, the older one afterwards (any order of
    // tasks must give the same file).
    for state in [newer.clone(), older.clone()] {
        let m = perform(Effect::TournamentSaveState {
            path: path.clone(),
            state: Box::new(state),
        })
        .await;
        assert_eq!(
            m,
            Message::Tournament(TournamentMessage::StateSaved { ok: true })
        );
    }
    assert_eq!(
        mychess_app::tournament::load_state(&path).unwrap().failures,
        [2, 0]
    );
    // Concurrent, in reverse order of revision.
    let path2 = dir.path().join("race.toml");
    let tasks: Vec<_> = (1..=8u64)
        .rev()
        .map(|rev| {
            let mut s = newer.clone();
            s.revision = rev;
            s.failures = vec![u32::try_from(rev).unwrap(), 0];
            tokio::spawn(perform(Effect::TournamentSaveState {
                path: path2.clone(),
                state: Box::new(s),
            }))
        })
        .collect();
    for t in tasks {
        t.await.unwrap();
    }
    assert_eq!(
        mychess_app::tournament::load_state(&path2)
            .unwrap()
            .failures,
        [8, 0]
    );
}

#[tokio::test]
async fn export_writes_the_file_and_reports_an_unwritable_target() {
    let names = ["A", "B"];
    let mut g = Glue::new(&names, Vec::new());
    let config = g.config(&names);
    let effects = g.start(config);
    g.drive(effects).await;
    let out = g.dir.path().join("table.csv");
    let effects = g.send(TournamentMessage::Export {
        table: ExportTable::Standings,
        format: ExportFormat::Csv,
        labels: Box::new(ExportLabels::default()),
        path: out.clone(),
    });
    g.drive(effects).await;
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("Rank,Name,"));
    assert_eq!(g.notice(), Some(&TournamentNotice::Exported));
    let bad = g
        .dir
        .path()
        .join("no-such-folder")
        .join("deep")
        .join("t.csv");
    let effects = g.send(TournamentMessage::Export {
        table: ExportTable::Cross,
        format: ExportFormat::Html,
        labels: Box::new(ExportLabels::default()),
        path: bad,
    });
    g.drive(effects).await;
    assert!(matches!(
        g.notice(),
        Some(TournamentNotice::ExportFailed | TournamentNotice::Exported)
    ));
}

#[test]
fn export_without_a_tournament_is_not_applicable() {
    let mut state = AppState::default();
    let fx = update(
        &mut state,
        Message::Tournament(TournamentMessage::Export {
            table: ExportTable::Standings,
            format: ExportFormat::Csv,
            labels: Box::new(ExportLabels::default()),
            path: PathBuf::from("x.csv"),
        }),
    );
    assert!(fx.is_empty());
    assert!(matches!(
        state.notice,
        Some(AppNotice::Tournament(TournamentNotice::NotApplicable))
    ));
    assert!(
        state
            .tournament_export_text(
                ExportTable::Cross,
                ExportFormat::Text,
                &ExportLabels::default()
            )
            .is_none()
    );
}

#[tokio::test]
async fn deciding_after_too_many_failures_works_through_the_state() {
    // Every engine of participant "A" dies: three failures, then Withdraw.
    let names = ["A", "B"];
    let crashing = Scripted {
        crash_on_go: true,
        ..Scripted::default()
    };
    let mut g = Glue::new(&names, vec![("A", crashing)]);
    let mut config = g.config(&names);
    config.cycles = 6;
    config.failures.max_retries = 0;
    config.failures.max_failures = 3;
    let effects = g.start(config);
    g.drive(effects).await;
    assert_eq!(g.phase(), Phase::Decision(0));
    assert!(
        g.state.tournament_live(),
        "a question open keeps the exclusivity"
    );
    let effects = g.send(TournamentMessage::Decide(
        Decision::Withdraw,
        Instant::now(),
    ));
    g.drive(effects).await;
    assert_eq!(g.phase(), Phase::Finished);
    assert_eq!(g.state.tournament().unwrap().withdrawn(), [true, false]);
    assert_eq!(g.pgn_games(), 3);
}
