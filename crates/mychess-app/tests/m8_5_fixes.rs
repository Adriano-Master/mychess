//! Regression tests of the review of the opening trainer (spec M8-5,
//! D2253-D2260): the cut of the games of a base, the store written before it
//! was read, the Leitner boxes on a repeated line and the small remarks.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value
)]

mod m3_4a_common;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use m3_4a_common::*;
use mychess_app::opening_trainer::{
    BuildError, BuildSource, MAX_GAMES, OpeningConfig, OpeningEffect, OpeningMessage,
    OpeningNotice, OpeningPhase, OpeningStore, OpeningTrainer, Out, RepSourceKind, STORE_FILE,
    TrainSide, build, load_file, save_file,
};
use mychess_core::{Position, Square};

const PGN: &str = "[Event \"a\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 *\n";

fn typed_base(dir: &Path, name: &str, base_type: u32, pgn: &str) -> PathBuf {
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, base_type).expect("create si4");
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

fn typed(side: TrainSide) -> OpeningConfig {
    OpeningConfig {
        side,
        source: RepSourceKind::TypedBase,
        ..OpeningConfig::default()
    }
}

fn file_config(path: &Path) -> OpeningConfig {
    OpeningConfig {
        source: RepSourceKind::File,
        file: Some(path.to_path_buf()),
        ..OpeningConfig::default()
    }
}

fn start_message(config: OpeningConfig) -> Message {
    Message::OpeningTrainer(OpeningMessage::Start {
        config: Box::new(config),
        seed: 7,
        today: 100,
    })
}

fn user_move(rig: &mut Rig, from: &str, to: &str) {
    let (from, to): (Square, Square) = (from.parse().unwrap(), to.parse().unwrap());
    rig.send(GameMessage::PlaySquares {
        from,
        to,
        promotion: None,
        options: PlayOptions::default(),
    });
}

fn with_prefs(dir: &Path) -> Rig {
    let mut rig = Rig::new();
    rig.state = AppState::with_preferences_path(Some(dir.join("preferences.toml")));
    rig
}

// ---- (1) the cut of the games of a base is told (D2254) ----

#[test]
fn a_filter_with_more_games_than_the_limit_is_cut_and_the_cut_is_told() {
    let dir = tempfile::tempdir().unwrap();
    let one = "[Event \"x\"]\n[Result \"*\"]\n\n1. e4 *\n\n";
    let big = typed_base(dir.path(), "big", 17, &one.repeat(MAX_GAMES + 1));
    let mut rig = Rig::new();
    rig.open(&big);
    rig.send(start_message(typed(TrainSide::White)));
    assert_eq!(rig.state.opening_phase(), OpeningPhase::UserTurn);
    let view = rig.state.opening_view();
    assert!(view.truncated, "the panel says the repertoire was cut");
    assert_eq!(view.notice, Some(OpeningNotice::Truncated));
}

#[test]
fn a_base_within_the_limit_is_not_reported_as_cut() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&typed_base(dir.path(), "small", 17, PGN));
    rig.send(start_message(typed(TrainSide::White)));
    assert_eq!(rig.state.opening_phase(), OpeningPhase::UserTurn);
    assert!(!rig.state.opening_view().truncated);
    assert_eq!(rig.state.opening_view().notice, None);
}

// ---- (2) nothing is written before the file was read (D2255) ----

/// Runs the effects of the trainer; `LoadStore` is held back and returned,
/// and a `SaveStore` before it is delivered fails the test.
fn pump_holding_load(rig: &mut Rig, effects: Vec<Effect>) -> Vec<Effect> {
    let mut held = Vec::new();
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        match &effect {
            Effect::OpeningTrainer(OpeningEffect::LoadStore { .. }) => {
                held.push(effect);
                continue;
            }
            Effect::OpeningTrainer(OpeningEffect::SaveStore { .. }) => {
                panic!("the store was written before it was read")
            }
            Effect::OpeningTrainer(_) => {}
            _ => continue,
        }
        let answer = rig.perform(effect);
        queue.extend(rig.raw(answer));
    }
    held
}

#[test]
fn a_start_and_a_reset_before_the_read_never_touch_a_file_of_a_newer_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join(STORE_FILE);
    let original = "version = 99\n[config]\nside = \"black\"\n[[rep]]\nkey = \"k\"\npos = [[1, 5, 5, 0, 0, 3, 9]]\n";
    std::fs::write(&store, original).unwrap();
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    let mut rig = with_prefs(dir.path());

    let effects = rig.raw(start_message(file_config(&repertoire)));
    let mut held = pump_holding_load(&mut rig, effects);
    let effects = rig.raw(Message::OpeningTrainer(OpeningMessage::ResetStats(
        Box::new(file_config(&repertoire)),
    )));
    held.extend(pump_holding_load(&mut rig, effects));
    user_move(&mut rig, "e2", "e4");
    rig.send(Message::OpeningTrainer(OpeningMessage::Close));
    assert_eq!(held.len(), 1, "the file is read once");

    for effect in held {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert_eq!(std::fs::read_to_string(&store).unwrap(), original);
}

#[test]
fn what_was_played_before_the_read_is_added_to_the_score_of_the_file_not_over_it() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join(STORE_FILE);
    let mut old = OpeningStore::default();
    old.use_rep("other.si4|w");
    old.stat_mut("other.si4|w", 5).unwrap().seen = 7;
    save_file(&store_path, &old).unwrap();
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    let mut rig = with_prefs(dir.path());

    let effects = rig.raw(start_message(file_config(&repertoire)));
    let held = pump_holding_load(&mut rig, effects);
    user_move(&mut rig, "e2", "e4");
    rig.send(Message::OpeningTrainer(OpeningMessage::Close));
    for effect in held {
        let answer = rig.perform(effect);
        rig.send(answer);
    }

    let loaded = load_file(&store_path).store;
    assert_eq!(
        loaded.stat("other.si4|w", 5).map(|s| s.seen),
        Some(7),
        "the old score survived"
    );
    let key = format!("{}|w", mychess_app::tactics::source_key(&repertoire));
    assert_eq!(
        loaded.recorded(&key),
        2,
        "and the new one was added (two user positions)"
    );
}

#[test]
fn a_reset_before_the_read_also_clears_what_the_file_brings() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join(STORE_FILE);
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    let key = format!("{}|w", mychess_app::tactics::source_key(&repertoire));
    let mut old = OpeningStore::default();
    old.use_rep(&key);
    old.stat_mut(&key, 5).unwrap().seen = 7;
    save_file(&store_path, &old).unwrap();
    let mut rig = with_prefs(dir.path());

    let effects = rig.raw(Message::OpeningTrainer(OpeningMessage::Open));
    let held = pump_holding_load(&mut rig, effects);
    let effects = rig.raw(Message::OpeningTrainer(OpeningMessage::ResetStats(
        Box::new(file_config(&repertoire)),
    )));
    assert!(pump_holding_load(&mut rig, effects).is_empty());
    for effect in held {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert_eq!(load_file(&store_path).store.recorded(&key), 0);
}

#[test]
fn without_a_preferences_path_nothing_is_ever_written() {
    let dir = tempfile::tempdir().unwrap();
    let repertoire = write(dir.path(), "rep.pgn", PGN);
    let mut rig = Rig::new();
    let effects = rig.raw(start_message(file_config(&repertoire)));
    let wrote = |effects: &[Effect]| {
        effects
            .iter()
            .any(|e| matches!(e, Effect::OpeningTrainer(OpeningEffect::SaveStore { .. })))
    };
    assert!(!wrote(&effects));
    rig.send(Message::OpeningTrainer(OpeningMessage::Close));
    assert!(!dir.path().join(STORE_FILE).exists());
}

// ---- the machine, driven by hand ----

fn session(config: OpeningConfig, text: &str) -> (OpeningTrainer, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "rep.pgn", text);
    let config = OpeningConfig {
        file: Some(path.clone()),
        source: RepSourceKind::File,
        ..config
    };
    let mut m = OpeningTrainer::new();
    let keys = ["rep|w".to_owned(), "rep|b".to_owned()];
    let outs = m.start(config, 1, 100, Ok((BuildSource::File(path), keys)));
    let Some(Out::Effect(OpeningEffect::Build { request, job })) = outs.first().cloned() else {
        panic!("a build is asked for");
    };
    m.built(request, build(&job).map(Box::new));
    (m, dir)
}

#[test]
fn repeating_a_line_the_same_day_counts_as_one_answer_for_the_boxes() {
    let (mut m, _d) = session(OpeningConfig::default(), PGN);
    let start = Position::new();
    let e4 = start.parse_uci("e2e4").unwrap();
    for _ in 0..3 {
        m.user_move(e4);
        m.retry();
    }
    let stat = m
        .store()
        .stat("rep|w", start.zobrist().value())
        .copied()
        .unwrap();
    assert_eq!(stat.box_no, 1, "one answer, one box");
    assert_eq!(
        stat.good, 3,
        "the counter of right answers still counts them"
    );
}

#[test]
fn a_build_that_failed_ends_the_session_with_a_reason_not_in_silence() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "rep.pgn", PGN);
    let mut m = OpeningTrainer::new();
    let keys = [String::new(), String::new()];
    let outs = m.start(
        OpeningConfig {
            file: Some(path.clone()),
            source: RepSourceKind::File,
            ..OpeningConfig::default()
        },
        1,
        100,
        Ok((BuildSource::File(path), keys)),
    );
    let Some(Out::Effect(OpeningEffect::Build { request, .. })) = outs.first().cloned() else {
        panic!("build");
    };
    m.built(request, Err(BuildError::Failed));
    assert_eq!(m.phase(), OpeningPhase::Closed);
    assert_eq!(m.notice(), Some(OpeningNotice::SourceUnreadable));
    assert!(m.dialog_open());
}

#[test]
fn closing_without_a_session_leaves_an_open_dialog_alone() {
    let mut m = OpeningTrainer::new();
    m.open_dialog();
    assert!(m.close().is_empty());
    assert!(m.dialog_open());
}

#[test]
fn stopping_the_tactics_training_does_not_close_the_dialog_of_the_trainer() {
    let mut rig = Rig::new();
    rig.send(Message::OpeningTrainer(OpeningMessage::Open));
    assert!(rig.state.opening_dialog_open());
    rig.send(Message::Tactics(
        mychess_app::tactics::TacticsMessage::Close,
    ));
    assert!(rig.state.opening_dialog_open());
}
