//! M8-7: the score reaches the disk (spec D2463, D2464). A game that ends
//! after the preferences were read asks for a save with the new score; one
//! that ends while they are being read waits and survives the load; the Elo
//! of the preferences moves only when the adjustment is on.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m8_1_common;
mod m8_7_common;

use std::path::PathBuf;
use std::time::Instant;

use m8_1_common::identity;
use m8_7_common::*;
use mychess_app::engines::{EngineDraft, EngineMessage};
use mychess_app::tactical_game::TacticalSetup;
use mychess_app::{
    AppState, ColorChoice, Effect, LoadedPreferences, Message, PlayConfig, PlayMessage,
    Preferences, TimeMode, update,
};

fn state_with_path() -> (AppState, PlayConfig) {
    let mut state =
        AppState::with_preferences_path(Some(PathBuf::from("cfg").join("preferences.toml")));
    let engine = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add engine");
    let mut config = PlayConfig::new(engine);
    config.color = ColorChoice::White;
    config.mode = TimeMode::MoveTime(3);
    config.tactical = Some(TacticalSetup {
        elo: 1500,
        adjust_step: Some(50),
        plan: None,
    });
    (state, from_fen(config, MATE_IN_ONE))
}

/// Starts the game (the engine comes up) and mates in one.
fn win(state: &mut AppState, config: PlayConfig) -> Vec<Effect> {
    let effects = update(state, Message::Play(PlayMessage::Start(Box::new(config))));
    let run = effects
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("the engine is asked for");
    update(
        state,
        Message::Engine(EngineMessage::Started {
            run,
            identity: Box::new(identity(full_options())),
            at: 1_700_000_000,
        }),
    );
    update(
        state,
        Message::Play(PlayMessage::PlayerMove {
            from: "d1".parse().unwrap(),
            to: "d8".parse().unwrap(),
            promotion: None,
            at: Instant::now(),
        }),
    )
}

fn saved(effects: &[Effect]) -> Vec<Preferences> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::SavePreferences { prefs, .. } => Some((**prefs).clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_game_that_ends_after_the_load_saves_the_new_score() {
    let (mut state, config) = state_with_path();
    update(&mut state, Message::Startup);
    update(
        &mut state,
        Message::PreferencesLoaded(LoadedPreferences::default()),
    );
    let effects = win(&mut state, config);
    let saves = saved(&effects);
    assert_eq!(saves.len(), 1, "{effects:?}");
    let tactical = &saves[0].play.tactical;
    assert_eq!((tactical.stats.games, tactical.stats.wins), (1, 1));
    assert_eq!(tactical.elo, 1550);
}

#[test]
fn a_game_that_ends_while_the_file_is_read_waits_and_survives_the_load() {
    let (mut state, config) = state_with_path();
    let startup = update(&mut state, Message::Startup);
    assert!(
        startup
            .iter()
            .any(|e| matches!(e, Effect::LoadPreferences { .. }))
    );
    let effects = win(&mut state, config);
    assert!(
        saved(&effects).is_empty(),
        "writing now would replace the file with the defaults"
    );
    assert_eq!(state.prefs.play.tactical.stats.wins, 1);
    // The file has an old score and another engine settled in `[play]`.
    let mut file = Preferences::default();
    file.play.tactical.stats.games = 4;
    file.play.tactical.stats.wins = 2;
    file.play.player_name = "Ana".to_owned();
    let effects = update(
        &mut state,
        Message::PreferencesLoaded(LoadedPreferences {
            prefs: file,
            issues: Vec::new(),
        }),
    );
    let tactical = &state.prefs.play.tactical;
    assert_eq!(
        (tactical.stats.games, tactical.stats.wins),
        (5, 3),
        "the game counts on top of the score of the file"
    );
    assert_eq!(tactical.elo, 1550);
    assert_eq!(
        state.prefs.play.player_name, "Ana",
        "the rest of [play] is the file's"
    );
    let saves = saved(&effects);
    assert_eq!(saves.len(), 1, "the save that waited goes out: {effects:?}");
    assert_eq!(saves[0].play.tactical.elo, 1550);
}

#[test]
fn the_elo_of_the_preferences_moves_only_with_the_adjustment_on() {
    let (mut state, mut config) = state_with_path();
    update(&mut state, Message::Startup);
    update(
        &mut state,
        Message::PreferencesLoaded(LoadedPreferences::default()),
    );
    state.prefs.play.tactical.elo = 1700;
    if let Some(setup) = config.tactical.as_mut() {
        setup.adjust_step = None;
    }
    let effects = win(&mut state, config);
    assert_eq!(state.prefs.play.tactical.elo, 1700, "left alone");
    assert_eq!(state.prefs.play.tactical.stats.wins, 1, "the score counts");
    assert_eq!(saved(&effects).len(), 1);
}

#[test]
fn a_play_change_while_the_file_is_read_keeps_the_score_and_the_elo_of_the_file() {
    use mychess_app::{PrefChange, PrefsMessage};
    let (mut state, _) = state_with_path();
    update(&mut state, Message::Startup);
    // The dialog sends the whole `[play]` from the defaults before the load.
    let mut play = state.prefs.play.as_ref().clone();
    play.player_name = "Bia".to_owned();
    update(
        &mut state,
        Message::Prefs(PrefsMessage::Set(PrefChange::Play(Box::new(play)))),
    );
    let mut file = Preferences::default();
    file.play.tactical.stats.games = 4;
    file.play.tactical.stats.wins = 2;
    file.play.tactical.elo = 1800;
    update(
        &mut state,
        Message::PreferencesLoaded(LoadedPreferences {
            prefs: file,
            issues: Vec::new(),
        }),
    );
    let tactical = &state.prefs.play.tactical;
    assert_eq!((tactical.stats.games, tactical.stats.wins), (4, 2));
    assert_eq!(tactical.elo, 1800);
    assert_eq!(state.prefs.play.player_name, "Bia", "the change still wins");
}
