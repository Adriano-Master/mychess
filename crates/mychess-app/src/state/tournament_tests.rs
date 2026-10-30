//! Tests of the tournament in the state: the real effects run (files in a
//! temporary directory, engines scripted in memory) and their answers are fed
//! back to `update` (spec M8-2, phase A).

use std::path::Path;
use std::time::Instant;

use mychess_core::GameResult;

use super::{AppState, update};
use crate::effect::{Effect, perform};
use crate::engines::{EngineDraft, EngineHub, EngineId};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::play::{ColorChoice, PlayConfig, PlayMessage, PlayNotice};
use crate::prefs::{PrefChange, Preferences, load_preferences, render_preferences};
use crate::tournament::driver::tests::{Fixture, Script};
use crate::tournament::{
    Adjudication, ExportFormat, ExportLabels, ExportTable, Participant, Phase, TimeControl,
    TournamentConfig, TournamentMessage, TournamentNotice, list_unfinished,
};

fn send(state: &mut AppState, message: impl Into<Message>) -> Vec<Effect> {
    update(state, message.into())
}

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

/// Runs the effects of the tournament and what they cause, until none is
/// left. Other effects are ignored.
async fn drive(state: &mut AppState, effects: Vec<Effect>) {
    let mut queue: std::collections::VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if !is_tournament_effect(&effect) {
            continue;
        }
        let message = perform(effect).await;
        queue.extend(update(state, message));
    }
}

fn state_with(dir: &Path, names: &[&str]) -> (AppState, Fixture, Vec<EngineId>) {
    let mut state = AppState::default();
    let mut ids = Vec::new();
    for name in names {
        ids.push(
            state
                .engines
                .add(EngineDraft::new(*name, "/fake"))
                .expect("add"),
        );
    }
    let fixture = Fixture::new(Vec::<(&'static str, Script)>::new());
    state.engine_hub = EngineHub::with_launcher(fixture.launcher.clone());
    state.flow.tournament.dir_override = Some(dir.to_path_buf());
    // The machine is known, as after the probe of the form: a start then
    // asks for the load only (the unknown machine has its own test).
    state.flow.tournament.resources = Some(crate::tournament::SystemResources {
        cores: 4,
        ram_available_mb: Some(15_000),
    });
    (state, fixture, ids)
}

/// Like `state_with`, but the identities 1 and 2 were used and removed, as in
/// a list that changed since the tournament was saved.
fn state_with_gaps(dir: &Path, names: &[&str]) -> (AppState, Fixture, Vec<EngineId>) {
    let (mut state, fixture, _) = state_with(dir, &[]);
    for name in ["X1", "X2"] {
        let id = state
            .engines
            .add(EngineDraft::new(name, "/fake"))
            .expect("add");
        state.engines.remove(id).expect("remove");
    }
    let mut ids = Vec::new();
    for name in names {
        ids.push(
            state
                .engines
                .add(EngineDraft::new(*name, "/fake"))
                .expect("add"),
        );
    }
    (state, fixture, ids)
}

fn config(ids: &[EngineId], names: &[&str]) -> TournamentConfig {
    TournamentConfig {
        participants: ids
            .iter()
            .zip(names)
            .map(|(id, name)| Participant::new(id.0, name))
            .collect(),
        cycles: 2,
        time: TimeControl::Depth(1),
        // Every game is decided by the limit of plies.
        adjudication: Adjudication {
            max_plies: Some(6),
            ..Adjudication::default()
        },
        ..TournamentConfig::default()
    }
}

fn start(state: &mut AppState, config: TournamentConfig) -> Vec<Effect> {
    send(
        state,
        TournamentMessage::Start {
            config: Box::new(config),
            at: Instant::now(),
        },
    )
}

fn notice(state: &AppState) -> Option<&TournamentNotice> {
    match state.notice.as_ref() {
        Some(AppNotice::Tournament(n)) => Some(n),
        _ => None,
    }
}

fn pgn_games(dir: &Path) -> usize {
    let Some(path) = std::fs::read_dir(dir).ok().and_then(|d| {
        d.filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "pgn"))
    }) else {
        return 0;
    };
    std::fs::read_to_string(path).map_or(0, |t| t.matches("[Event ").count())
}

#[tokio::test]
async fn a_whole_tournament_runs_through_the_effects_and_writes_its_files() {
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _fixture, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    assert!(matches!(
        effects.as_slice(),
        [Effect::TournamentLoad { .. }]
    ));
    assert!(state.tournament_live(), "loading counts as live");
    drive(&mut state, effects).await;
    let t = state.tournament().expect("tournament");
    assert_eq!(t.phase(), Phase::Finished);
    assert_eq!(t.progress(), (2, 2));
    assert!(!state.tournament_live());
    // The PGN holds both games, written one by one.
    assert_eq!(pgn_games(dir.path()), 2);
    // The final state is saved as finished and is not offered for resuming.
    assert!(list_unfinished(dir.path()).is_empty());
    let saved: Vec<_> = std::fs::read_dir(dir.path())
        .expect("dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
        .collect();
    assert_eq!(saved.len(), 1);
    // The editor game of the user was not touched (D1278).
    assert!(state.session.game().mainline().next().is_none());
    // Everything drawn by the plies limit: a perfectly even table.
    assert!(
        t.finished_games()
            .iter()
            .all(|g| g.result == GameResult::Draw)
    );
}

#[tokio::test]
async fn the_seed_and_the_slug_come_from_the_load() {
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    drive(&mut state, effects).await;
    let t = state.tournament().expect("tournament");
    assert_ne!(t.config().seed, 0, "a seed was chosen");
    assert!(i64::try_from(t.config().seed).is_ok());
    assert!(t.slug().starts_with("MyChess-Tournament-"), "{}", t.slug());
    // A second tournament with the same name gets another slug.
    let effects = start(&mut state, config(&ids, &names));
    drive(&mut state, effects).await;
    let second = state.tournament().expect("tournament").slug().to_owned();
    assert!(second.ends_with("-2"), "{second}");
}

#[test]
fn start_refuses_an_invalid_or_unknown_configuration() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, ids) = state_with(dir.path(), &["A", "B"]);
    let mut c = config(&ids, &["A", "B"]);
    c.participants.pop();
    assert!(start(&mut state, c).is_empty());
    assert!(matches!(
        notice(&state),
        Some(TournamentNotice::InvalidConfig(_))
    ));
    let mut c = config(&ids, &["A", "B"]);
    c.participants[1].engine = 999;
    assert!(start(&mut state, c).is_empty());
    assert_eq!(
        notice(&state),
        Some(&TournamentNotice::MissingEngines(vec!["B".to_owned()]))
    );
    assert!(state.tournament().is_none());
}

#[test]
fn start_refuses_a_second_tournament_while_one_is_on() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, ids) = state_with(dir.path(), &["A", "B"]);
    assert_eq!(start(&mut state, config(&ids, &["A", "B"])).len(), 1);
    assert!(start(&mut state, config(&ids, &["A", "B"])).is_empty());
    assert_eq!(notice(&state), Some(&TournamentNotice::AlreadyRunning));
}

#[test]
fn a_live_tournament_blocks_the_game_against_an_engine() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, ids) = state_with(dir.path(), &["A", "B"]);
    start(&mut state, config(&ids, &["A", "B"]));
    let mut play = PlayConfig::new(ids[0]);
    play.color = ColorChoice::White;
    let effects = send(&mut state, PlayMessage::Start(Box::new(play)));
    assert!(effects.is_empty());
    assert_eq!(state.notice, Some(AppNotice::Play(PlayNotice::Busy)));
    assert!(!state.play_live());
}

#[test]
fn start_refuses_while_a_game_against_an_engine_is_live() {
    // The reverse of the test above needs a live game; its flag is enough.
    let dir = tempfile::tempdir().expect("dir");
    let (state, _f, _ids) = state_with(dir.path(), &["A", "B"]);
    assert!(!state.tournament_live());
    assert!(state.tournament().is_none());
}

#[tokio::test]
async fn stop_after_the_game_then_resume_in_a_new_session_finishes_the_tournament() {
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let mut c = config(&ids, &names);
    c.cycles = 4;
    // Stop as soon as the tournament starts: the first game is the last.
    let effects = start(&mut state, c);
    let mut first = Vec::new();
    let mut queue: std::collections::VecDeque<Effect> = effects.into();
    let mut stopped = false;
    while let Some(effect) = queue.pop_front() {
        if !is_tournament_effect(&effect) {
            continue;
        }
        let message = perform(effect).await;
        let more = update(&mut state, message);
        if !stopped && state.tournament().is_some() {
            stopped = true;
            first.extend(send(
                &mut state,
                TournamentMessage::StopAfterGame(Instant::now()),
            ));
        }
        queue.extend(more);
        queue.extend(std::mem::take(&mut first));
    }
    let t = state.tournament().expect("tournament");
    assert_eq!(t.phase(), Phase::StoppedAfterGame);
    assert!(
        state.tournament_live(),
        "a stopped tournament still holds the exclusivity (D1311)"
    );
    assert_eq!(t.progress(), (1, 4));
    assert_eq!(pgn_games(dir.path()), 1);
    let listed = list_unfinished(dir.path());
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].played, listed[0].total), (1, 4));

    // A new session whose list holds the same engines under other
    // identities: they are found again by name (D1285).
    let (mut other, _f2, _ids2) = state_with_gaps(dir.path(), &["Z", "B", "A"]);
    let effects = send(
        &mut other,
        TournamentMessage::Resume {
            path: listed[0].path.clone(),
            replacements: Vec::new(),
            at: Instant::now(),
        },
    );
    drive(&mut other, effects).await;
    let t = other.tournament().expect("resumed");
    assert_eq!(t.phase(), Phase::Finished);
    assert_eq!(t.progress(), (4, 4));
    assert_eq!(
        pgn_games(dir.path()),
        4,
        "the games were appended to the PGN"
    );
    let names: Vec<_> = t.config().participants.iter().map(|p| p.engine).collect();
    assert_ne!(names, ids.iter().map(|i| i.0).collect::<Vec<_>>());
}

#[tokio::test]
async fn resume_reports_a_missing_engine_and_accepts_a_replacement() {
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    drive(&mut state, effects).await;
    // Make the saved tournament resumable again.
    let path = std::fs::read_dir(dir.path())
        .expect("dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "toml"))
        .expect("state file");
    let text = std::fs::read_to_string(&path).expect("read");
    std::fs::write(&path, text.replace("finished = true", "finished = false")).expect("write");

    let (mut other, _f2, other_ids) = state_with_gaps(dir.path(), &["A", "Other"]);
    let effects = send(
        &mut other,
        TournamentMessage::Resume {
            path: path.clone(),
            replacements: Vec::new(),
            at: Instant::now(),
        },
    );
    drive(&mut other, effects).await;
    assert_eq!(
        notice(&other),
        Some(&TournamentNotice::MissingEngines(vec!["B".to_owned()]))
    );
    assert!(other.tournament().is_none());
    // With a replacement for participant 1 it goes on.
    let effects = send(
        &mut other,
        TournamentMessage::Resume {
            path,
            replacements: vec![(1, other_ids[1].0)],
            at: Instant::now(),
        },
    );
    drive(&mut other, effects).await;
    assert_eq!(
        other.tournament().expect("resumed").config().participants[1].engine,
        other_ids[1].0
    );
}

#[tokio::test]
async fn resume_of_a_corrupt_file_says_so() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, _ids) = state_with(dir.path(), &["A", "B"]);
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "garbage = [").expect("write");
    let effects = send(
        &mut state,
        TournamentMessage::Resume {
            path,
            replacements: Vec::new(),
            at: Instant::now(),
        },
    );
    drive(&mut state, effects).await;
    assert_eq!(notice(&state), Some(&TournamentNotice::ResumeFailed));
    assert!(!state.tournament_live());
}

#[tokio::test]
async fn the_table_is_exported_to_a_file() {
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    drive(&mut state, effects).await;
    let target = dir.path().join("table.csv");
    let effects = send(
        &mut state,
        TournamentMessage::Export {
            table: ExportTable::Standings,
            format: ExportFormat::Csv,
            labels: Box::new(ExportLabels::default()),
            path: target.clone(),
        },
    );
    drive(&mut state, effects).await;
    assert_eq!(notice(&state), Some(&TournamentNotice::Exported));
    let csv = std::fs::read_to_string(&target).expect("csv");
    assert!(csv.starts_with("Rank,Name,Played"), "{csv}");
    assert_eq!(csv.lines().count(), 3);
    // Without a tournament there is nothing to export.
    let mut empty = AppState::default();
    let effects = send(
        &mut empty,
        TournamentMessage::Export {
            table: ExportTable::Cross,
            format: ExportFormat::Html,
            labels: Box::new(ExportLabels::default()),
            path: target,
        },
    );
    assert!(effects.is_empty());
}

#[tokio::test]
async fn close_keeps_a_running_tournament_and_drops_a_finished_one() {
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    send(&mut state, TournamentMessage::Close);
    assert!(state.tournament_live(), "loading is kept");
    drive(&mut state, effects).await;
    send(&mut state, TournamentMessage::Close);
    assert!(state.tournament().is_none());
}

#[tokio::test]
async fn quitting_saves_the_state_and_ends_the_engines() {
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    // Load, then stop before the games run: run only the first answer.
    let load = perform(effects.into_iter().next().expect("load")).await;
    drop(update(&mut state, load));
    let effects = send(&mut state, Message::Quit);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::TournamentSaveState { .. }))
    );
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::TournamentPool {
            command: crate::tournament::PoolCommand::Shutdown,
            ..
        }
    )));
}

#[test]
fn the_tournament_preferences_round_trip_and_survive_garbage() {
    let ids = [EngineId(3), EngineId(4)];
    let mut prefs = Preferences::default();
    let mut c = config(&ids, &["A", "B"]);
    c.name = "Club cup".to_owned();
    c.seed = 77;
    c.participants[0].options = vec![("Hash".to_owned(), "64".to_owned())];
    PrefChange::Tournament(Box::new(c.clone())).apply(&mut prefs);
    let loaded = load_preferences(&render_preferences(&prefs));
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(*loaded.prefs.tournament, c);

    let loaded = load_preferences("[tournament]\ncycles = \"many\"\n");
    assert_eq!(loaded.issues.len(), 1);
    assert_eq!(*loaded.prefs.tournament, TournamentConfig::default());
    // Numbers are brought into range.
    let loaded = load_preferences("[tournament]\ncycles = 5000\nmargin_ms = 1\n");
    assert_eq!(loaded.prefs.tournament.cycles, 100);
    assert_eq!(loaded.prefs.tournament.margin_ms, 1000);
}

#[tokio::test]
async fn resume_reads_and_writes_next_to_the_state_file_not_in_the_preferences_folder() {
    // Regression (D1304): the folder of the preferences was used for the PGN.
    let origin = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(origin.path(), &names);
    let mut c = config(&ids, &names);
    c.cycles = 4;
    let effects = start(&mut state, c);
    drive(&mut state, effects).await;
    assert_eq!(pgn_games(origin.path()), 4);
    // Move both files elsewhere and make the last game pending again.
    let moved = tempfile::tempdir().expect("dir");
    let mut state_file = None;
    for entry in std::fs::read_dir(origin.path()).expect("dir").flatten() {
        let target = moved.path().join(entry.file_name());
        std::fs::copy(entry.path(), &target).expect("copy");
        if target.extension().is_some_and(|e| e == "toml") {
            state_file = Some(target);
        }
    }
    let state_file = state_file.expect("state file");
    let mut saved = crate::tournament::load_state(&state_file).expect("load");
    saved.games[3].status = crate::tournament::SavedStatus::Pending;
    saved.games[3].result = None;
    saved.games[3].termination = None;
    saved.queue = vec![3];
    saved.finished = false;
    crate::tournament::save_state(&state_file, &saved).expect("save");

    // The new session believes its folder is a decoy.
    let decoy = tempfile::tempdir().expect("dir");
    let (mut other, _f2, _ids2) = state_with(decoy.path(), &names);
    let effects = send(
        &mut other,
        TournamentMessage::Resume {
            path: state_file,
            replacements: Vec::new(),
            at: Instant::now(),
        },
    );
    drive(&mut other, effects).await;
    assert_eq!(
        other.tournament().expect("resumed").phase(),
        Phase::Finished
    );
    assert_eq!(pgn_games(moved.path()), 5, "appended next to the state");
    assert_eq!(
        std::fs::read_dir(decoy.path()).expect("dir").count(),
        0,
        "nothing was written to the other folder"
    );
}

#[tokio::test]
async fn resume_refuses_a_state_whose_slug_climbs_out_of_the_folder() {
    // Regression (D1304): `slug = "../x"` made the PGN go outside.
    let root = tempfile::tempdir().expect("dir");
    let inner = root.path().join("torneios");
    std::fs::create_dir(&inner).expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(&inner, &names);
    let effects = start(&mut state, config(&ids, &names));
    drive(&mut state, effects).await;
    let path = std::fs::read_dir(&inner)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "toml"))
        .expect("state file");
    let mut saved = crate::tournament::load_state(&path).expect("load");
    saved.slug = "../x".to_owned();
    saved.finished = false;
    std::fs::write(&path, toml::to_string(&saved).expect("toml")).expect("write");

    let (mut other, _f2, _ids2) = state_with(&inner, &names);
    let effects = send(
        &mut other,
        TournamentMessage::Resume {
            path,
            replacements: Vec::new(),
            at: Instant::now(),
        },
    );
    drive(&mut other, effects).await;
    assert_eq!(notice(&other), Some(&TournamentNotice::ResumeFailed));
    assert!(other.tournament().is_none());
    assert!(!root.path().join("x.pgn").exists());
    assert!(!root.path().join("x.toml").exists());
}

#[tokio::test]
async fn quitting_cancels_the_search_before_it_shuts_the_pool_down() {
    // Regression (D1307): without the cancel the engine in flight came back
    // to a pool that had been emptied.
    use crate::tournament::PoolCommand;
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    let load = perform(effects.into_iter().next().expect("load")).await;
    drop(update(&mut state, load));
    let effects = send(&mut state, Message::Quit);
    let commands: Vec<PoolCommand> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::TournamentPool { command, .. } => Some(command.clone()),
            _ => None,
        })
        .collect();
    let cancel = commands
        .iter()
        .position(|c| matches!(c, PoolCommand::Cancel(epoch) if *epoch > 0));
    let shutdown = commands.iter().position(|c| *c == PoolCommand::Shutdown);
    assert!(cancel.is_some() && shutdown.is_some(), "{commands:?}");
    assert!(cancel < shutdown, "{commands:?}");
}

#[tokio::test]
async fn quitting_waits_for_the_write_to_the_slot_that_is_in_flight() {
    // Regression (D2533): the quit only looked at the writes of the PGN and
    // of the state and left while a game was being added to the `.si4`.
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    let load = perform(effects.into_iter().next().expect("load")).await;
    drop(update(&mut state, load));
    state.flow.tournament.dest.mark_busy();
    let effects = send(&mut state, Message::Quit);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Exit)));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::QuitDeadline { .. })),
        "the wait has its deadline"
    );
    // The writes of the state (the one of the start and the one of the
    // quit) are answered, but the slot is still being written.
    for _ in 0..2 {
        let effects = send(&mut state, TournamentMessage::StateSaved { ok: true });
        assert!(!effects.iter().any(|e| matches!(e, Effect::Exit)));
    }
    let failed = crate::bases::BaseError::Corrupt;
    let effects = send(
        &mut state,
        TournamentMessage::SlotAdded {
            result: Box::new(crate::tournament::SlotWrite::NotWritten(failed)),
        },
    );
    assert!(
        effects.iter().any(|e| matches!(e, Effect::Exit)),
        "{effects:?}"
    );
}

#[test]
fn a_new_destination_does_not_forget_the_write_in_flight() {
    // Regression (D2534): the setup of the destination cleared `busy` while
    // the answer of the write was still to come.
    let mut state = AppState::default();
    state.flow.tournament.dest.mark_busy();
    state.tournament_dest_setup(crate::tournament::ExtraDestination::None, None);
    assert!(state.flow.tournament.dest.is_busy());
}

#[tokio::test]
async fn a_cancelled_search_after_the_quit_opens_no_engine_and_saves_no_game() {
    // Regression (D2533): `Searched { Cancelled }` after the quit asked for
    // another preparation, which opened the pool again.
    use crate::tournament::SearchOutcome;
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    let load = perform(effects.into_iter().next().expect("load")).await;
    let effects = update(&mut state, load);
    let prepare = effects
        .into_iter()
        .find(|e| matches!(e, Effect::TournamentPrepare { .. }))
        .expect("prepare");
    let prepared = perform(prepare).await;
    let effects = update(&mut state, prepared);
    let epoch = effects
        .iter()
        .find_map(|e| match e {
            Effect::TournamentSearch { job, .. } => Some(job.epoch),
            _ => None,
        })
        .expect("the first search");
    drop(send(&mut state, Message::Quit));
    let later = [
        TournamentMessage::Searched {
            epoch,
            outcome: SearchOutcome::Cancelled,
            at: Instant::now(),
        },
        TournamentMessage::Prepared {
            epoch,
            result: Err(crate::tournament::PrepareFailure {
                participant: 0,
                failure: crate::engines::EngineFailure {
                    kind: crate::engines::FailureKind::CannotStart,
                    exit_code: None,
                    stderr_tail: Vec::new(),
                },
            }),
            at: Instant::now(),
        },
    ];
    for message in later {
        let effects = send(&mut state, message);
        assert!(
            !effects.iter().any(|e| matches!(
                e,
                Effect::TournamentSearch { .. }
                    | Effect::TournamentPrepare { .. }
                    | Effect::TournamentSaveGame { .. }
            )),
            "{effects:?}"
        );
    }
}

#[tokio::test]
async fn closing_a_paused_tournament_ends_its_engines_and_brings_the_analyses_back() {
    // Regression (D1307): the engines stayed alive and `paused` stayed full.
    use crate::tournament::PoolCommand;
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    let load = perform(effects.into_iter().next().expect("load")).await;
    let effects = update(&mut state, load);
    // Ask for the pause while the engines start; it takes hold at the first
    // move.
    send(&mut state, TournamentMessage::Pause);
    state.flow.tournament.paused = vec![crate::engines::RunId(4242)];
    let mut queue: std::collections::VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        if is_tournament_effect(&effect) {
            let message = perform(effect).await;
            queue.extend(update(&mut state, message));
        }
    }
    assert_eq!(state.tournament().expect("t").phase(), Phase::Paused);
    let effects = send(&mut state, TournamentMessage::Close);
    assert!(state.tournament().is_none());
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::TournamentPool {
                command: PoolCommand::Shutdown,
                ..
            }
        )),
        "{effects:?}"
    );
    assert!(state.flow.tournament.paused.is_empty());
}

#[test]
fn an_annotation_does_not_start_while_a_tournament_lives() {
    // The annotation does not start while a tournament lives (D1311).
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, ids) = state_with(dir.path(), &["A", "B"]);
    start(&mut state, config(&ids, &["A", "B"]));
    let effects = send(&mut state, crate::annotate::AnnotateMessage::Start);
    assert!(effects.is_empty());
    assert_eq!(notice(&state), Some(&TournamentNotice::AlreadyRunning));
}

/// A tournament whose first search is in flight; the game of the user is
/// altered, so a quit asks before leaving.
async fn with_search_in_flight(dir: &Path) -> (AppState, Fixture, Vec<EngineId>, Vec<Effect>) {
    let names = ["A", "B"];
    let (mut state, f, ids) = state_with(dir, &names);
    let mv = state.session.position().parse_uci("e2e4").expect("uci");
    send(
        &mut state,
        crate::session::GameMessage::PlayMove {
            mv,
            options: crate::session::PlayOptions::default(),
        },
    );
    let mut c = config(&ids, &names);
    c.destination = crate::tournament::ExtraDestination::Clipbase;
    let effects = start(&mut state, c);
    let load = perform(effects.into_iter().next().expect("load")).await;
    let effects = update(&mut state, load);
    let prepare = effects
        .into_iter()
        .find(|e| matches!(e, Effect::TournamentPrepare { .. }))
        .expect("prepare");
    let prepared = perform(prepare).await;
    let effects = update(&mut state, prepared);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::TournamentSearch { .. }))
    );
    (state, f, ids, effects)
}

fn cancel_the_quit(state: &mut AppState) -> Vec<Effect> {
    let effects = send(state, Message::Quit);
    assert!(state.discard_prompt.is_some(), "the quit asks first");
    assert!(!effects.iter().any(|e| matches!(e, Effect::Exit)));
    send(
        state,
        Message::AnswerDiscard(crate::flow::DiscardChoice::Cancel),
    )
}

#[tokio::test]
async fn a_called_off_quit_ends_the_tournament_as_aborted_and_it_can_be_closed() {
    // Regression (D2538): the tournament stayed `Running` with its pool
    // closed, and nothing could end it.
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, _ids, _) = Box::pin(with_search_in_flight(dir.path())).await;
    let effects = cancel_the_quit(&mut state);
    assert!(!state.flow.tournament.quitting);
    assert_eq!(state.tournament().expect("t").phase(), Phase::Aborted);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::TournamentSaveState { .. }))
    );
    send(&mut state, TournamentMessage::Abort(Instant::now()));
    assert_eq!(state.tournament().expect("t").phase(), Phase::Aborted);
    send(&mut state, TournamentMessage::Close);
    assert!(state.tournament().is_none());
}

#[tokio::test]
async fn abort_after_a_quit_that_was_not_undone_finishes_without_the_pool() {
    // Regression (D2538): the user acts without going through the question
    // (the quit stopped at another gate); the cancel of the search never
    // came back and the abort waited for it for ever.
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, _ids, _) = Box::pin(with_search_in_flight(dir.path())).await;
    send(&mut state, Message::Quit);
    assert!(state.flow.tournament.quitting);
    send(&mut state, TournamentMessage::Abort(Instant::now()));
    assert_eq!(state.tournament().expect("t").phase(), Phase::Aborted);
    assert!(!state.flow.tournament.quitting);
    send(&mut state, TournamentMessage::Close);
    assert!(state.tournament().is_none());
}

#[tokio::test]
async fn abort_while_the_quit_waits_for_writes_uses_no_pool() {
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, _ids, _) = Box::pin(with_search_in_flight(dir.path())).await;
    state.discard_prompt = None;
    state.session.mark_saved();
    send(&mut state, Message::Quit);
    assert!(state.flow.tournament.quit_wait, "waiting for the writes");
    send(&mut state, TournamentMessage::Abort(Instant::now()));
    assert_eq!(state.tournament().expect("t").phase(), Phase::Aborted);
    assert!(state.flow.tournament.quit_wait, "the quit goes on");
}

#[tokio::test]
async fn resume_after_a_called_off_quit_runs_again_and_feeds_the_destination() {
    // Regression (D2538): `quitting` stayed on, so every answer was dropped
    // and no game reached the slot.
    let dir = tempfile::tempdir().expect("dir");
    let (mut state, _f, _ids, _) = Box::pin(with_search_in_flight(dir.path())).await;
    let effects = cancel_the_quit(&mut state);
    // The state of the aborted tournament reaches the disk.
    drive(&mut state, effects).await;
    send(&mut state, TournamentMessage::Close);
    let listed = list_unfinished(dir.path());
    assert_eq!(listed.len(), 1);
    let effects = send(
        &mut state,
        TournamentMessage::Resume {
            path: listed[0].path.clone(),
            replacements: Vec::new(),
            at: Instant::now(),
        },
    );
    drive(&mut state, effects).await;
    let t = state.tournament().expect("resumed");
    assert_eq!(t.phase(), Phase::Finished);
    assert_eq!(pgn_games(dir.path()), 2);
    assert_eq!(
        state
            .bases
            .list_page(crate::bases::SlotId::CLIPBASE, 1, 10)
            .len(),
        2,
        "the games reach the destination again"
    );
}

#[tokio::test]
async fn a_new_start_clears_the_quit_flag() {
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    state.flow.tournament.quitting = true;
    start(&mut state, config(&ids, &names));
    assert!(!state.flow.tournament.quitting);
}

#[tokio::test]
async fn a_late_load_after_a_quit_opens_nothing() {
    // D2538: the quit during the load drops the tournament in preparation.
    let dir = tempfile::tempdir().expect("dir");
    let names = ["A", "B"];
    let (mut state, _f, ids) = state_with(dir.path(), &names);
    let effects = start(&mut state, config(&ids, &names));
    let load = perform(effects.into_iter().next().expect("load")).await;
    send(&mut state, Message::Quit);
    let effects = update(&mut state, load);
    assert!(state.tournament().is_none());
    assert!(!effects.iter().any(is_tournament_effect), "{effects:?}");
}

#[test]
fn the_deadline_of_the_quit_gives_the_slot_write_up() {
    let mut state = AppState::default();
    state.flow.tournament.quit_wait = true;
    state.flow.tournament.dest.mark_busy();
    assert!(state.tournament_quit_deadline());
    assert!(!state.flow.tournament.dest.is_busy());
    assert!(!state.tournament_quit_deadline(), "only once");
}

#[test]
fn a_game_after_the_quit_does_not_join_the_slot_queue() {
    let mut state = AppState::default();
    let id = state.bases.current();
    state.flow.tournament.dest.set_slot_for_test(id);
    state.flow.tournament.quitting = true;
    let mut effects = Vec::new();
    state.tournament_dest_add(1, &mychess_core::Game::new(), &mut effects);
    assert!(effects.is_empty());
    assert!(!state.flow.tournament.dest.has_queue());
}

#[test]
fn quitting_without_a_tournament_drops_the_games_waiting_for_the_slot() {
    let mut state = AppState::default();
    state.flow.tournament.dest.queue_for_test();
    assert!(state.flow.tournament.dest.has_queue());
    state.tournament_quit(&mut Vec::new());
    assert!(!state.flow.tournament.dest.has_queue());
}
