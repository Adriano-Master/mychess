//! M8-1d contract tests of the preferences of ponder and the coach (spec
//! D2400, D2419): defaults, tolerant reading, clamping, round trip and the
//! `PlayConfig` they make.
#![allow(clippy::assert_is_empty, clippy::unwrap_used, clippy::expect_used)]

use mychess_app::engines::EngineId;
use mychess_app::{
    COACH_DEPTH_RANGE, COACH_SECONDS_RANGE, PlayPrefs, Preferences, load_preferences,
    render_preferences,
};

fn play_of(text: &str) -> PlayPrefs {
    *load_preferences(text).prefs.play
}

#[test]
fn defaults_are_off_and_numbers_on() {
    let p = PlayPrefs::default();
    assert!(!p.ponder && !p.coach && !p.coach_hint);
    assert!(p.coach_numbers);
    assert_eq!((p.coach_depth, p.coach_seconds), (12, 3));
    assert_eq!(p.coach_engine, None);
}

#[test]
fn an_old_file_opens_with_the_defaults_and_no_remark() {
    let loaded = load_preferences("[play]\nmode = \"depth\"\nponder = true\n");
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    let p = &loaded.prefs.play;
    assert!(p.ponder);
    assert_eq!(
        (p.coach, p.coach_engine, p.coach_depth, p.coach_seconds),
        (false, None, 12, 3)
    );
}

#[test]
fn the_coach_fields_round_trip() {
    let play = PlayPrefs {
        ponder: true,
        coach: true,
        coach_engine: Some(9),
        coach_depth: 7,
        coach_seconds: 5,
        coach_hint: true,
        coach_numbers: false,
        tactical: mychess_app::tactical_game::TacticalPrefs::default(),
        ..PlayPrefs::default()
    };
    let prefs = Preferences {
        play: Box::new(play.clone()),
        ..Preferences::default()
    };
    let text = render_preferences(&prefs);
    assert!(text.contains("coach_engine = 9"), "{text}");
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert_eq!(*loaded.prefs.play, play);
}

#[test]
fn bad_values_fall_back_and_numbers_are_clamped() {
    let p = play_of(
        "[play]\ncoach = \"yes\"\ncoach_depth = 999\ncoach_seconds = 0\ncoach_engine = \"x\"\ncoach_hint = 3\n",
    );
    assert!(!p.coach);
    assert_eq!(p.coach_depth, COACH_DEPTH_RANGE.1);
    assert_eq!(p.coach_seconds, COACH_SECONDS_RANGE.0);
    assert_eq!(p.coach_engine, None);
    assert!(!p.coach_hint);
    assert!(
        !load_preferences("[play]\ncoach = \"yes\"\n")
            .issues
            .is_empty()
    );
}

#[test]
fn the_config_carries_ponder_and_the_coach_only_when_valid() {
    let on = PlayPrefs {
        ponder: true,
        coach: true,
        coach_engine: Some(2),
        coach_depth: 9,
        coach_seconds: 4,
        coach_hint: true,
        coach_numbers: false,
        tactical: mychess_app::tactical_game::TacticalPrefs::default(),
        ..PlayPrefs::default()
    };
    let config = on.to_config(EngineId(1));
    assert!(config.ponder);
    let coach = config.coach.expect("a coach");
    assert_eq!(coach.engine, EngineId(2));
    assert_eq!((coach.depth, coach.seconds), (9, 4));
    assert!(coach.hint && !coach.numbers);
    // The opponent's own entry cannot coach.
    assert!(on.to_config(EngineId(2)).coach.is_none());
    // Off, or no engine chosen: no coach.
    let off = PlayPrefs {
        coach: false,
        ..on.clone()
    };
    assert!(off.to_config(EngineId(1)).coach.is_none());
    let none = PlayPrefs {
        coach_engine: None,
        ..on
    };
    assert!(none.to_config(EngineId(1)).coach.is_none());
    assert!(!PlayPrefs::default().to_config(EngineId(1)).ponder);
}
