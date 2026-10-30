//! M8-2d contract tests (spec D2538-D2541): the load of the tournament opens
//! the Syzygy folders of the preferences only when the rule and the
//! preferences ask for it; with nothing readable the games go on without the
//! rule and the user is told once.
#![allow(
    clippy::large_futures,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use mychess_app::engines::{EngineDraft, EngineHub, EngineId, Launcher};
use mychess_app::tournament::{
    Adjudication, Participant, SystemResources, TablebaseProbe, TbOutcome, TimeControl,
    TournamentConfig, TournamentMessage, TournamentNotice, TournamentTablebase, outcome_of,
};
use mychess_app::{AppNotice, AppState, Effect, perform, update};
use mychess_core::Position;
use mychess_engine::testing::ScriptedEngine;
use mychess_engine::{EngineConfig, EngineError};
use mychess_tablebase::{Category, LocalTablebase};

struct Rig {
    state: AppState,
    ids: Vec<EngineId>,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let mut state = AppState::default();
        let mut ids = Vec::new();
        let mut fakes = Vec::new();
        for name in ["A", "B"] {
            ids.push(state.engines.add(EngineDraft::new(name, "/fake")).unwrap());
            fakes.push((
                name.to_owned(),
                ScriptedEngine::builder().name(name).build(),
            ));
        }
        let shared = Arc::new(fakes);
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
            ids,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn set_prefs(&mut self, local: bool, directories: Vec<String>) {
        self.state.prefs.tablebase.local = local;
        self.state.prefs.tablebase.directories = directories;
    }

    fn config(&self, tablebase: bool) -> TournamentConfig {
        TournamentConfig {
            participants: self
                .ids
                .iter()
                .zip(["A", "B"])
                .map(|(id, n)| Participant::new(id.0, n))
                .collect(),
            cycles: 1,
            time: TimeControl::Depth(1),
            adjudication: Adjudication {
                max_plies: Some(6),
                tablebase,
                tb_pieces: 5,
                ..Adjudication::default()
            },
            ..TournamentConfig::default()
        }
    }

    /// Starts and returns the load effect.
    fn start_load(&mut self, config: TournamentConfig) -> Effect {
        let effects = update(
            &mut self.state,
            TournamentMessage::Start {
                config: Box::new(config),
                at: Instant::now(),
            }
            .into(),
        );
        effects
            .into_iter()
            .find(|e| matches!(e, Effect::TournamentLoad { .. }))
            .expect("a load effect")
    }

    fn notice(&self) -> Option<&TournamentNotice> {
        match self.state.notice.as_ref() {
            Some(AppNotice::Tournament(n)) => Some(n),
            _ => None,
        }
    }
}

fn dirs_of(effect: &Effect) -> Vec<PathBuf> {
    match effect {
        Effect::TournamentLoad { tablebase_dirs, .. } => tablebase_dirs.clone(),
        _ => panic!("not a load"),
    }
}

#[test]
fn the_load_carries_the_folders_only_with_the_rule_and_the_preferences_on() {
    let folder = tempfile::tempdir().unwrap();
    let text = folder.path().to_string_lossy().into_owned();
    // (rule, local, has folders) -> folders expected
    for (rule, local, with_dirs) in [
        (true, true, true),
        (false, true, true),
        (true, false, true),
        (true, true, false),
    ] {
        let mut rig = Rig::new();
        let dirs = if with_dirs {
            vec![text.clone()]
        } else {
            Vec::new()
        };
        rig.set_prefs(local, dirs);
        let config = rig.config(rule);
        let effect = rig.start_load(config);
        let got = dirs_of(&effect);
        if rule && local && with_dirs {
            assert_eq!(got, vec![PathBuf::from(&text)]);
        } else {
            assert!(got.is_empty(), "{rule} {local} {with_dirs}: {got:?}");
        }
    }
}

#[tokio::test]
async fn an_empty_folder_loads_no_tables_and_warns_while_the_tournament_lives() {
    let tables = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.set_prefs(true, vec![tables.path().to_string_lossy().into_owned()]);
    let config = rig.config(true);
    let Effect::TournamentLoad {
        request,
        config,
        slug,
        dir: _,
        tablebase_dirs,
    } = rig.start_load(config)
    else {
        unreachable!()
    };
    assert_eq!(tablebase_dirs.len(), 1);
    let message = perform(Effect::TournamentLoad {
        request,
        config,
        slug,
        dir: Some(rig.dir.path().to_path_buf()),
        tablebase_dirs,
    })
    .await;
    let loaded_has_none = format!("{message:?}");
    assert!(
        loaded_has_none.contains("tablebase: None"),
        "{loaded_has_none}"
    );
    update(&mut rig.state, message);
    assert_eq!(rig.notice(), Some(&TournamentNotice::TablebaseUnavailable));
    assert!(rig.state.tournament_live());
}

#[tokio::test]
async fn without_the_rule_no_tablebase_notice_appears() {
    let tables = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.set_prefs(true, vec![tables.path().to_string_lossy().into_owned()]);
    let config = rig.config(false);
    let Effect::TournamentLoad {
        request,
        config,
        slug,
        tablebase_dirs,
        ..
    } = rig.start_load(config)
    else {
        unreachable!()
    };
    assert!(tablebase_dirs.is_empty());
    let message = perform(Effect::TournamentLoad {
        request,
        config,
        slug,
        dir: Some(rig.dir.path().to_path_buf()),
        tablebase_dirs,
    })
    .await;
    assert!(format!("{message:?}").contains("tablebase: None"));
    update(&mut rig.state, message);
    assert_ne!(rig.notice(), Some(&TournamentNotice::TablebaseUnavailable));
    assert!(rig.state.tournament_live());
}

#[test]
fn availability_needs_local_on_and_at_least_one_folder() {
    for (local, dirs, expected) in [
        (true, vec!["x".to_owned()], true),
        (true, Vec::new(), false),
        (false, vec!["x".to_owned()], false),
        (false, Vec::new(), false),
    ] {
        let mut state = AppState::default();
        state.prefs.tablebase.local = local;
        state.prefs.tablebase.directories = dirs;
        assert_eq!(state.tournament_tablebase_available(), expected);
    }
}

#[test]
fn every_category_maps_to_its_outcome_and_the_uncertain_ones_to_none() {
    assert_eq!(outcome_of(Category::Win), Some(TbOutcome::Win));
    assert_eq!(outcome_of(Category::CursedWin), Some(TbOutcome::CursedWin));
    assert_eq!(outcome_of(Category::Draw), Some(TbOutcome::Draw));
    assert_eq!(
        outcome_of(Category::BlessedLoss),
        Some(TbOutcome::BlessedLoss)
    );
    assert_eq!(outcome_of(Category::Loss), Some(TbOutcome::Loss));
    assert_eq!(outcome_of(Category::MaybeWin), None);
    assert_eq!(outcome_of(Category::MaybeLoss), None);
    assert_eq!(outcome_of(Category::Unknown), None);
}

#[test]
fn an_empty_folder_gives_a_probe_that_decides_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (tables, _) = LocalTablebase::open(&[dir.path().to_path_buf()]).unwrap();
    let tb = TournamentTablebase::new(tables);
    assert_eq!(tb.max_pieces(), 0);
    let krk = Position::from_fen("8/8/8/8/8/4k3/8/R3K3 w - - 0 1").unwrap();
    assert_eq!(tb.probe(&krk), None);
    assert_eq!(tb.probe(&Position::new()), None);
}
