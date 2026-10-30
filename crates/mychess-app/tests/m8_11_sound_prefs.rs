//! M8-11 contract tests of the `[sound]` preferences (spec D2804) and of the
//! play effect with a fake sink (D2808).
#![allow(clippy::assert_is_empty, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use mychess_app::sound::{
    Clip, FakeSink, PlayRequest, SoundEffect, SoundMessage, SoundPrefs, SoundSink, perform_play,
};
use mychess_app::{Effect, Message, PrefsIssue, load_preferences, perform, render_preferences};

#[test]
fn the_defaults_are_off() {
    let loaded = load_preferences("");
    assert_eq!(*loaded.prefs.sound, SoundPrefs::default());
    assert!(!loaded.prefs.sound.enabled);
    assert!(loaded.issues.is_empty());
}

#[test]
fn the_table_round_trips() {
    let mut loaded = load_preferences("");
    *loaded.prefs.sound = SoundPrefs {
        enabled: true,
        volume: 35,
        on_new_move: false,
        on_forward: true,
        on_back: true,
        tick_only: true,
        announce: true,
        folder: "C:\\Sons de xadrez".to_owned(),
        pack: mychess_app::sound::PackId::Bundled("sfx".to_owned()),
    };
    let text = render_preferences(&loaded.prefs);
    assert!(text.contains("[sound]"), "{text}");
    let again = load_preferences(&text);
    assert!(again.issues.is_empty(), "{:?}", again.issues);
    assert_eq!(again.prefs.sound, loaded.prefs.sound);
}

#[test]
fn the_volume_is_limited_with_a_remark_and_wrong_types_keep_the_defaults() {
    let loaded = load_preferences("[sound]\nvolume = 400\nenabled = \"yes\"\nfolder = 3\n");
    assert_eq!(loaded.prefs.sound.volume, 100);
    assert!(!loaded.prefs.sound.enabled);
    assert!(loaded.prefs.sound.folder.is_empty());
    assert!(
        loaded
            .issues
            .iter()
            .any(|i| matches!(i, PrefsIssue::Clamped { key } if key == "sound.volume"))
    );
    let invalid = loaded
        .issues
        .iter()
        .filter(|i| matches!(i, PrefsIssue::InvalidValue { .. }))
        .count();
    assert_eq!(invalid, 2);
}

#[test]
fn a_negative_volume_is_limited_to_zero() {
    let loaded = load_preferences("[sound]\nvolume = -5\n");
    assert_eq!(loaded.prefs.sound.volume, 0);
}

fn request() -> PlayRequest {
    PlayRequest {
        clips: vec![Arc::new(Clip {
            sample_rate: 8_000,
            channels: 1,
            samples: vec![0.3, -0.3],
        })],
        gain: 1.0,
    }
}

#[test]
fn a_sink_receives_the_clips_in_order() {
    let sink = FakeSink::new();
    perform_play(request(), Some(&sink as &dyn SoundSink));
    assert_eq!(sink.played(), vec![request()]);
}

#[tokio::test]
async fn the_play_effect_without_a_device_answers_played() {
    let message = perform(Effect::Sound(SoundEffect::Play(request()))).await;
    assert_eq!(message, Message::Sound(SoundMessage::Played));
}
