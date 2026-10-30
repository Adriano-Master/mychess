//! M8-11b contract tests of the sound packs (spec criteria 3 to 6, D2900-D2909):
//! the choice of a pack plays its clips, the fallback chain, the preference,
//! stale answers, the packs of the user (hostile folders included) and the
//! assets that are embedded.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mychess_app::sound::{
    BUNDLED_BUDGET_BYTES, Clip, FakeSink, MAX_PACK_ENTRIES, MAX_USER_PACKS, PackError, PackId,
    PackStatus, PackWarning, PlayRequest, SoundCue, SoundEffect, SoundMessage, SoundPrefs,
    builtin_clip, decode_file, embedded_bytes, embedded_files, list_packs, load_pack, perform_play,
    scan_user_packs,
};
use mychess_app::{
    AppState, Effect, Message, PrefsIssue, RequestId, load_preferences, perform,
    render_preferences, update,
};

fn sounds_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds")
}

fn asset(pack: &str, file: &str) -> Clip {
    decode_file(&sounds_dir().join(pack).join(file)).unwrap()
}

/// A WAV of `n` samples of `value` (16 bit, 8 kHz, mono).
fn wav(n: usize, value: i16) -> Vec<u8> {
    let data: Vec<u8> = std::iter::repeat_n(value.to_le_bytes(), n)
        .flatten()
        .collect();
    let len = u32::try_from(data.len()).unwrap();
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&8_000_u32.to_le_bytes());
    out.extend_from_slice(&16_000_u32.to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&data);
    out
}

fn sound_effects(effects: Vec<Effect>) -> Vec<Effect> {
    effects
        .into_iter()
        .filter(|e| matches!(e, Effect::Sound(_)))
        .collect()
}

/// Feeds the sound effects (not the play ones) back until nothing is left.
async fn settle(state: &mut AppState, effects: Vec<Effect>) {
    let mut pending = sound_effects(effects);
    while let Some(effect) = pending.pop() {
        if matches!(effect, Effect::Sound(SoundEffect::Play(_))) {
            continue;
        }
        let message = perform(effect).await;
        pending.extend(sound_effects(update(state, message)));
    }
}

fn app(prefs: SoundPrefs, dir: Option<PathBuf>) -> AppState {
    let mut state = AppState::default();
    state.set_sound_packs_dir(dir);
    drop(update(
        &mut state,
        Message::Sound(SoundMessage::Set(Box::new(SoundPrefs {
            enabled: true,
            ..prefs
        }))),
    ));
    state
}

async fn select(state: &mut AppState, id: PackId) {
    let effects = update(state, Message::Sound(SoundMessage::SelectPack(id)));
    settle(state, effects).await;
}

fn request_of(effects: &[Effect]) -> PlayRequest {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Sound(SoundEffect::Play(r)) => Some(r.clone()),
            _ => None,
        })
        .expect("a play effect")
}

fn play(state: &mut AppState, cue: SoundCue) -> PlayRequest {
    request_of(&update(state, Message::Sound(SoundMessage::Cue(cue))))
}

#[tokio::test]
async fn choosing_a_bundled_pack_plays_its_clips_through_the_sink() {
    let mut state = app(SoundPrefs::default(), None);
    // Before the choice: the synthesized sound.
    assert!(Arc::ptr_eq(
        &play(&mut state, SoundCue::Move).clips[0],
        &builtin_clip(SoundCue::Move)
    ));
    select(&mut state, PackId::Bundled("sfx".into())).await;
    assert!(matches!(
        state.sound_pack_status(),
        PackStatus::Loaded { covered: 6, .. }
    ));
    let sink = FakeSink::new();
    let request = play(&mut state, SoundCue::Move);
    perform_play(request, Some(&sink));
    let played = sink.played();
    assert_eq!(played.len(), 1);
    let want = asset("sfx", "Move.ogg");
    let got = &played[0].clips[0];
    assert_eq!(got.samples, want.samples);
    assert_eq!((got.sample_rate, got.channels), (44_100, 2));
}

#[tokio::test]
async fn a_missing_cue_follows_the_chain_and_then_the_synthesized_sound() {
    let mut state = app(SoundPrefs::default(), None);
    select(&mut state, PackId::Bundled("nes".into())).await;
    let check = asset("nes", "Check.ogg").samples;
    let moved = asset("nes", "Move.ogg").samples;
    let expect = [
        (SoundCue::Move, &moved),
        (SoundCue::Capture, &asset("nes", "Capture.ogg").samples),
        (SoundCue::Check, &check),
        (SoundCue::Checkmate, &check),
        (SoundCue::Castle, &moved),
        (SoundCue::Promotion, &moved),
        (SoundCue::Back, &moved),
        (SoundCue::GameEnd, &asset("nes", "Victory.ogg").samples),
        (SoundCue::LowTime, &asset("nes", "LowTime.ogg").samples),
    ];
    for (cue, samples) in expect {
        let clip = play(&mut state, cue).clips[0].clone();
        assert_eq!(&clip.samples, samples, "{cue:?}");
    }
    // The pack has no game start: the synthesized one.
    assert!(Arc::ptr_eq(
        &play(&mut state, SoundCue::GameStart).clips[0],
        &builtin_clip(SoundCue::GameStart)
    ));
}

#[tokio::test]
async fn every_bundled_pack_loads_without_remarks() {
    for pack in ["sfx", "nes", "futuristic", "piano"] {
        let loaded = load_pack(&PackId::Bundled(pack.into()), None).unwrap();
        assert!(loaded.info().warnings.is_empty(), "{pack}");
        assert_eq!(loaded.info().covered, 6, "{pack}");
        assert_eq!(
            loaded.direct(SoundCue::Move).unwrap().samples,
            asset(pack, "Move.ogg").samples,
            "{pack}"
        );
    }
}

#[tokio::test]
async fn going_back_to_the_default_drops_the_pack() {
    let mut state = app(SoundPrefs::default(), None);
    select(&mut state, PackId::Bundled("piano".into())).await;
    assert!(state.sound_loaded_pack().is_some());
    select(&mut state, PackId::BuiltinSynth).await;
    assert!(state.sound_loaded_pack().is_none());
    assert_eq!(state.sound_pack_status(), &PackStatus::None);
    assert!(Arc::ptr_eq(
        &play(&mut state, SoundCue::Check).clips[0],
        &builtin_clip(SoundCue::Check)
    ));
}

#[tokio::test]
async fn the_advanced_folder_wins_over_the_pack_cue_by_cue() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Check.wav"), wav(5, 1_000)).unwrap();
    let prefs = SoundPrefs {
        folder: dir.path().to_string_lossy().into_owned(),
        pack: PackId::Bundled("sfx".into()),
        ..SoundPrefs::default()
    };
    let mut state = AppState::default();
    state.set_sound_packs_dir(None);
    let effects = update(
        &mut state,
        Message::Sound(SoundMessage::Set(Box::new(SoundPrefs {
            enabled: true,
            ..prefs
        }))),
    );
    settle(&mut state, effects).await;
    assert_eq!(play(&mut state, SoundCue::Check).clips[0].samples.len(), 5);
    assert_eq!(
        play(&mut state, SoundCue::Move).clips[0].samples,
        asset("sfx", "Move.ogg").samples
    );
}

#[test]
fn an_unknown_pack_in_the_preferences_goes_back_to_the_default_with_a_remark() {
    for text in [
        "[sound]\npack = \"bundled:nope\"\n",
        "[sound]\npack = \"lichess\"\n",
        "[sound]\npack = \"user:../x\"\n",
        "[sound]\npack = 7\n",
    ] {
        let loaded = load_preferences(text);
        assert_eq!(loaded.prefs.sound.pack, PackId::BuiltinSynth, "{text}");
        assert!(
            loaded.issues.contains(&PrefsIssue::InvalidValue {
                key: "sound.pack".into()
            }),
            "{text}: {:?}",
            loaded.issues
        );
    }
}

#[test]
fn an_old_file_without_the_key_loads_the_default_without_remarks() {
    let loaded = load_preferences("[sound]\nenabled = true\nvolume = 40\n");
    assert_eq!(loaded.prefs.sound.pack, PackId::BuiltinSynth);
    assert!(loaded.issues.is_empty());
    let none = load_preferences("");
    assert_eq!(none.prefs.sound.pack, PackId::BuiltinSynth);
}

#[test]
fn the_pack_choice_round_trips_in_the_file() {
    for id in ["bundled:piano", "user:My-Pack_2", "builtin:synth"] {
        let loaded = load_preferences(&format!("[sound]\npack = \"{id}\"\n"));
        assert!(loaded.issues.is_empty(), "{id}");
        let text = render_preferences(&loaded.prefs);
        assert!(text.contains(&format!("pack = \"{id}\"")), "{text}");
        assert_eq!(load_preferences(&text).prefs.sound.pack.to_string(), id);
    }
}

#[tokio::test]
async fn the_chosen_pack_is_loaded_when_the_preferences_arrive() {
    let loaded = load_preferences("[sound]\nenabled = true\npack = \"bundled:futuristic\"\n");
    let mut state = AppState::default();
    state.set_sound_packs_dir(None);
    let effects = update(&mut state, Message::PreferencesLoaded(loaded));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Sound(SoundEffect::LoadPack { id, .. }) if *id == PackId::Bundled("futuristic".into())
    )));
    assert_eq!(state.sound_pack_status(), &PackStatus::Loading);
    // Meanwhile the synthesized sound plays.
    assert!(Arc::ptr_eq(
        &play(&mut state, SoundCue::Move).clips[0],
        &builtin_clip(SoundCue::Move)
    ));
    settle(&mut state, effects).await;
    assert!(matches!(
        state.sound_pack_status(),
        PackStatus::Loaded { .. }
    ));
    assert_eq!(
        play(&mut state, SoundCue::Move).clips[0].samples,
        asset("futuristic", "Move.ogg").samples
    );
}

fn load_requests(effects: &[Effect]) -> Vec<(RequestId, PackId)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Sound(SoundEffect::LoadPack { request, id, .. }) => {
                Some((*request, id.clone()))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_old_answer_of_load_pack_is_dropped() {
    let mut state = app(SoundPrefs::default(), None);
    let first = update(
        &mut state,
        Message::Sound(SoundMessage::SelectPack(PackId::Bundled("sfx".into()))),
    );
    let second = update(
        &mut state,
        Message::Sound(SoundMessage::SelectPack(PackId::Bundled("nes".into()))),
    );
    let (old, new) = (load_requests(&first), load_requests(&second));
    assert_eq!(old.len(), 1);
    assert_eq!(new.len(), 1);
    assert_ne!(old[0].0, new[0].0);
    // The answer of the first arrives last.
    let [new_effect] = <[Effect; 1]>::try_from(sound_effects(second)).unwrap();
    let [old_effect] = <[Effect; 1]>::try_from(sound_effects(first)).unwrap();
    let new_answer = perform(new_effect).await;
    let old_answer = perform(old_effect).await;
    drop(update(&mut state, new_answer));
    drop(update(&mut state, old_answer));
    assert_eq!(
        play(&mut state, SoundCue::Move).clips[0].samples,
        asset("nes", "Move.ogg").samples,
        "the stale sfx answer must not replace nes"
    );
    // And an answer for a request nobody waits for changes nothing.
    let late = Message::Sound(SoundMessage::PackLoaded {
        request: old[0].0,
        id: PackId::Bundled("sfx".into()),
        result: Ok(Arc::new(
            load_pack(&PackId::Bundled("sfx".into()), None).unwrap(),
        )),
    });
    drop(update(&mut state, late));
    assert_eq!(state.sound_prefs().pack, PackId::Bundled("nes".into()));
    assert_eq!(
        play(&mut state, SoundCue::Move).clips[0].samples,
        asset("nes", "Move.ogg").samples
    );
}

#[tokio::test]
async fn a_user_pack_that_is_gone_goes_back_to_the_default_with_a_remark() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = app(SoundPrefs::default(), Some(dir.path().to_path_buf()));
    select(&mut state, PackId::User("vanished".into())).await;
    assert_eq!(state.sound_prefs().pack, PackId::BuiltinSynth);
    assert!(matches!(
        state.sound_pack_status(),
        PackStatus::Failed(PackError::NotFound(_))
    ));
    assert!(state.prefs_issues.contains(&PrefsIssue::InvalidValue {
        key: "sound.pack".into()
    }));
    // An invalid or unknown embedded id is not even requested.
    let effects = update(
        &mut state,
        Message::Sound(SoundMessage::SelectPack(PackId::Bundled("nope".into()))),
    );
    assert!(load_requests(&effects).is_empty());
    assert_eq!(state.sound_prefs().pack, PackId::BuiltinSynth);
}

#[tokio::test]
async fn a_pack_that_failed_with_a_read_error_is_read_again_on_demand() {
    let mut state = app(SoundPrefs::default(), None);
    let id = PackId::Bundled("nes".into());
    let first = update(
        &mut state,
        Message::Sound(SoundMessage::SelectPack(id.clone())),
    );
    let [(request, _)] = <[_; 1]>::try_from(load_requests(&first)).unwrap();
    // A transient read error: the choice stays, the status is Failed.
    drop(update(
        &mut state,
        Message::Sound(SoundMessage::PackLoaded {
            request,
            id: id.clone(),
            result: Err(PackError::Io(std::io::ErrorKind::TimedOut)),
        }),
    ));
    assert!(matches!(state.sound_pack_status(), PackStatus::Failed(_)));
    assert_eq!(state.sound_prefs().pack, id);
    // Choosing the same pack again asks for a new load.
    let again = update(
        &mut state,
        Message::Sound(SoundMessage::SelectPack(id.clone())),
    );
    assert_eq!(load_requests(&again).len(), 1);
    assert_eq!(*state.sound_pack_status(), PackStatus::Loading);
    settle(&mut state, again).await;
    assert!(matches!(
        state.sound_pack_status(),
        PackStatus::Loaded { .. }
    ));
    // A loaded pack is not read again by a repeated choice.
    let third = update(&mut state, Message::Sound(SoundMessage::SelectPack(id)));
    assert!(load_requests(&third).is_empty());
}

#[tokio::test]
async fn a_user_pack_is_listed_chosen_and_played() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("Mine");
    std::fs::create_dir(&pack).unwrap();
    std::fs::write(pack.join("MOVE.wav"), wav(7, 8_192)).unwrap();
    std::fs::write(pack.join("Capture.wav"), wav(9, 8_192)).unwrap();
    std::fs::write(
        pack.join("pack.toml"),
        "name = \"My sounds\"\nauthor = \"Me\"\nlicense = \"CC0-1.0\"\n[cues]\nfuture_cue = \"x.wav\"\n",
    )
    .unwrap();
    let mut state = app(SoundPrefs::default(), Some(root.path().to_path_buf()));
    assert_eq!(state.sound_packs().len(), 5, "the synth and the embedded");
    let effects = update(&mut state, Message::Sound(SoundMessage::ScanPacks));
    settle(&mut state, effects).await;
    let listed: Vec<_> = state
        .sound_packs()
        .iter()
        .map(|p| p.id.to_string())
        .collect();
    assert_eq!(listed.last().map(String::as_str), Some("user:Mine"));
    let info = state.sound_packs().last().unwrap();
    assert_eq!(
        (
            info.name.as_str(),
            info.author.as_str(),
            info.license.as_str()
        ),
        ("My sounds", "Me", "CC0-1.0")
    );
    assert_eq!(info.covered, 2);
    assert!(info.warnings.is_empty(), "unknown cue keys are ignored");
    select(&mut state, PackId::User("Mine".into())).await;
    assert_eq!(play(&mut state, SoundCue::Move).clips[0].samples.len(), 7);
    // Capture is the pack's own; Check follows the chain to the move.
    assert_eq!(
        play(&mut state, SoundCue::Capture).clips[0].samples.len(),
        9
    );
    assert_eq!(play(&mut state, SoundCue::Check).clips[0].samples.len(), 7);
}

#[tokio::test]
async fn opening_the_user_folder_creates_it_only_then_and_rescans() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("sounds");
    let mut state = app(SoundPrefs::default(), Some(dir.clone()));
    assert!(!dir.exists(), "nothing is created on start-up or scan");
    let effects = update(&mut state, Message::Sound(SoundMessage::ScanPacks));
    settle(&mut state, effects).await;
    assert!(!dir.exists());
    assert!(state.sound_user_folder().is_none());
    let effects = update(
        &mut state,
        Message::Sound(SoundMessage::OpenUserFolderRequested),
    );
    settle(&mut state, effects).await;
    assert!(dir.is_dir());
    assert_eq!(state.sound_user_folder(), Some(dir.as_path()));
}

// ---- hostile packs of the user ----------------------------------------

#[test]
fn malicious_folder_names_are_ignored_with_a_remark() {
    let root = tempfile::tempdir().unwrap();
    let mut bad = vec![
        "a b".to_owned(),
        "caf\u{e9}".to_owned(),
        "x".repeat(65),
        "dots..here".to_owned(),
        "semi;colon".to_owned(),
        "$(rm)".to_owned(),
    ];
    // Characters that are not valid in file names on Windows.
    #[cfg(unix)]
    bad.push("back\\slash".to_owned());
    for name in &bad {
        std::fs::create_dir(root.path().join(name)).unwrap();
    }
    std::fs::create_dir(root.path().join("good-1.0_x")).unwrap();
    let scan = scan_user_packs(root.path()).unwrap();
    assert_eq!(scan.packs.len(), 1);
    assert_eq!(scan.packs[0].id, PackId::User("good-1.0_x".into()));
    assert_eq!(scan.warnings.len(), bad.len(), "{:?}", scan.warnings);
    assert!(
        scan.warnings
            .iter()
            .all(|w| matches!(w, PackWarning::BadFolderName(_)))
    );
    // And such an id cannot be loaded even if forged.
    for name in ["..", ".", ".hidden", "trailing.", "../x", "a/b", "a\\b", ""] {
        assert!(load_pack(&PackId::User(name.into()), Some(root.path())).is_err());
    }
}

#[test]
fn a_file_mapped_outside_the_pack_is_refused() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("secret.wav"), wav(3, 100)).unwrap();
    let pack = root.path().join("p");
    std::fs::create_dir(&pack).unwrap();
    std::fs::write(
        pack.join("pack.toml"),
        "[cues]\nmove = \"../secret.wav\"\ncapture = \"..\\\\secret.wav\"\ncheck = \"sub/x.wav\"\n",
    )
    .unwrap();
    let loaded = load_pack(&PackId::User("p".into()), Some(root.path())).unwrap();
    assert_eq!(loaded.info().covered, 0);
    let bad = loaded
        .info()
        .warnings
        .iter()
        .filter(|w| matches!(w, PackWarning::BadFileName(_)))
        .count();
    assert_eq!(bad, 3);
}

#[cfg(unix)]
#[test]
fn a_symlink_to_outside_the_pack_is_ignored_and_one_inside_is_followed() {
    // On Windows creating symlinks needs a privilege; there the same
    // protection is covered by `a_file_mapped_outside_the_pack_is_refused`
    // and by the canonical-path check, which is the same code.
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.wav"), wav(3, 100)).unwrap();
    let pack = root.path().join("p");
    std::fs::create_dir(&pack).unwrap();
    std::fs::write(pack.join("real.wav"), wav(4, 100)).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.wav"), pack.join("Move.wav")).unwrap();
    std::os::unix::fs::symlink(pack.join("real.wav"), pack.join("Check.wav")).unwrap();
    // A link to a directory is not a sound either.
    std::os::unix::fs::symlink(outside.path(), pack.join("Capture.wav")).unwrap();
    let loaded = load_pack(&PackId::User("p".into()), Some(root.path())).unwrap();
    assert!(loaded.direct(SoundCue::Move).is_none());
    assert!(loaded.direct(SoundCue::Capture).is_none());
    assert_eq!(loaded.direct(SoundCue::Check).unwrap().samples.len(), 4);
    let outside_links = loaded
        .info()
        .warnings
        .iter()
        .filter(|w| matches!(w, PackWarning::LinkOutside(_)))
        .count();
    assert_eq!(outside_links, 2, "{:?}", loaded.info().warnings);
}

#[cfg(unix)]
#[test]
fn a_symlinked_pack_manifest_to_outside_is_ignored() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("m.toml"), "name = \"Stolen\"\n").unwrap();
    let pack = root.path().join("p");
    std::fs::create_dir(&pack).unwrap();
    std::os::unix::fs::symlink(outside.path().join("m.toml"), pack.join("pack.toml")).unwrap();
    let scan = scan_user_packs(root.path()).unwrap();
    assert_eq!(scan.packs[0].name, "p");
}

#[test]
fn a_folder_with_three_hundred_files_is_limited_and_does_not_fail() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("big");
    std::fs::create_dir(&pack).unwrap();
    for i in 0..300 {
        std::fs::write(pack.join(format!("f{i:03}.wav")), wav(2, 1)).unwrap();
    }
    std::fs::write(pack.join("pack.toml"), "name = \"Big\"\n").unwrap();
    let scan = scan_user_packs(root.path()).unwrap();
    assert_eq!(scan.packs[0].name, "Big", "the manifest is read anyway");
    assert!(
        scan.packs[0]
            .warnings
            .contains(&PackWarning::TooManyEntries)
    );
    const { assert!(MAX_PACK_ENTRIES < 300) };
    let loaded = load_pack(&PackId::User("big".into()), Some(root.path())).unwrap();
    assert!(loaded.info().covered <= SoundCue::ALL.len());
}

#[test]
fn a_huge_manifest_is_ignored_and_the_pack_keeps_working() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("p");
    std::fs::create_dir(&pack).unwrap();
    std::fs::write(pack.join("move.wav"), wav(3, 5)).unwrap();
    let mut huge = String::from("name = \"Huge\"\n[cues]\n");
    huge.push_str(&"# padding padding padding padding\n".repeat(40_000));
    std::fs::write(pack.join("pack.toml"), huge).unwrap();
    let loaded = load_pack(&PackId::User("p".into()), Some(root.path())).unwrap();
    assert_eq!(loaded.info().name, "p");
    assert!(
        loaded
            .info()
            .warnings
            .contains(&PackWarning::ManifestTooLarge)
    );
    assert_eq!(loaded.info().covered, 1);
}

#[test]
fn a_corrupt_sound_costs_only_its_own_cue() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("p");
    std::fs::create_dir(&pack).unwrap();
    std::fs::write(pack.join("move.wav"), wav(3, 5)).unwrap();
    std::fs::write(pack.join("capture.ogg"), b"OggS garbage garbage garbage").unwrap();
    std::fs::write(pack.join("Check.mp3"), [0xFF, 0xFB, 0x10, 0x00, 1, 2, 3]).unwrap();
    let loaded = load_pack(&PackId::User("p".into()), Some(root.path())).unwrap();
    assert_eq!(loaded.info().covered, 1);
    let undecodable = loaded
        .info()
        .warnings
        .iter()
        .filter(|w| matches!(w, PackWarning::Undecodable { .. }))
        .count();
    assert_eq!(undecodable, 2);
    // The chain still serves them with the move.
    assert!(loaded.resolve(SoundCue::Capture).is_some());
}

#[test]
fn the_scan_of_a_missing_directory_is_empty_and_of_a_file_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        scan_user_packs(&root.path().join("none"))
            .unwrap()
            .packs
            .is_empty()
    );
    let file = root.path().join("f");
    std::fs::write(&file, b"x").unwrap();
    assert!(scan_user_packs(&file).is_err());
}

#[test]
fn only_sixty_four_user_packs_are_listed() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..(MAX_USER_PACKS + 5) {
        std::fs::create_dir(root.path().join(format!("p{i:03}"))).unwrap();
    }
    let scan = scan_user_packs(root.path()).unwrap();
    assert_eq!(scan.packs.len(), MAX_USER_PACKS);
    assert!(scan.warnings.contains(&PackWarning::TooManyPacks));
}

// ---- the embedded assets ------------------------------------------------

/// `(file, bytes)` rows of a `CREDITS.md` table.
fn credits_files(text: &str) -> Vec<(String, u64)> {
    text.lines()
        .filter(|l| l.starts_with("| `") && l.contains("Ogg Vorbis"))
        .map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            (
                cells[1].trim_matches('`').to_owned(),
                cells[3].parse().unwrap(),
            )
        })
        .collect()
}

const PACKS: [&str; 4] = ["sfx", "nes", "futuristic", "piano"];
const EMBEDDED_STEMS: [&str; 5] = ["Move", "Capture", "Check", "LowTime", "Victory"];

#[test]
fn the_embedded_files_fit_the_budget_and_are_only_the_ones_used() {
    let files = embedded_files();
    assert_eq!(files.len(), PACKS.len() * EMBEDDED_STEMS.len());
    let mut sum = 0;
    for (folder, name, bytes) in &files {
        // Each embedded file is one of the credited ones, with the bytes of
        // the file in assets/ (so also the size credited).
        let credits =
            std::fs::read_to_string(sounds_dir().join(folder).join("CREDITS.md")).unwrap();
        let listed = credits_files(&credits);
        let (_, size) = listed
            .iter()
            .find(|(f, _)| f == name)
            .unwrap_or_else(|| panic!("{folder}/{name} is not in its CREDITS.md"));
        assert_eq!(bytes.len() as u64, *size, "{folder}/{name}");
        assert_eq!(
            std::fs::read(sounds_dir().join(folder).join(name)).unwrap(),
            *bytes,
            "{folder}/{name}"
        );
        assert!(EMBEDDED_STEMS.iter().any(|s| format!("{s}.ogg") == *name));
        sum += bytes.len();
    }
    assert_eq!(embedded_bytes(), sum);
    assert!(
        embedded_bytes() <= BUNDLED_BUDGET_BYTES,
        "{} bytes",
        embedded_bytes()
    );
    assert_eq!(BUNDLED_BUDGET_BYTES, 1_536 * 1024);
}

#[test]
fn each_asset_folder_holds_exactly_the_credited_files() {
    // Nothing else (say, a sound of an excluded set such as lisp or
    // standard) may sit next to the credited ones: it would be shipped
    // with the licence of the others.
    for pack in PACKS {
        let dir = sounds_dir().join(pack);
        let credits = std::fs::read_to_string(dir.join("CREDITS.md")).unwrap();
        let mut expected: Vec<String> = credits_files(&credits)
            .into_iter()
            .map(|(f, _)| f)
            .collect();
        assert_eq!(expected.len(), 8, "{pack}");
        for stem in [
            "Move",
            "Capture",
            "Check",
            "LowTime",
            "Victory",
            "Defeat",
            "Draw",
            "GenericNotify",
        ] {
            assert!(expected.contains(&format!("{stem}.ogg")), "{pack}: {stem}");
        }
        expected.push("CREDITS.md".into());
        expected.push("pack.toml".into());
        expected.sort();
        let mut found: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        found.sort();
        assert_eq!(found, expected, "{pack}");
    }
    let mut folders: Vec<String> = std::fs::read_dir(sounds_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    folders.sort();
    let mut known: Vec<String> = PACKS.iter().map(|p| (*p).to_owned()).collect();
    known.sort();
    assert_eq!(folders, known);
}

#[test]
fn the_asset_sounds_hold_no_symlinks() {
    fn walk(dir: &Path, seen: &mut usize) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(
                !kind.is_symlink(),
                "{} is a symlink",
                entry.path().display()
            );
            *seen += 1;
            if kind.is_dir() {
                walk(&entry.path(), seen);
            }
        }
    }
    let mut seen = 0;
    walk(&sounds_dir(), &mut seen);
    assert!(seen > PACKS.len());
}

#[test]
fn the_licenses_row_of_each_pack_has_the_sum_of_its_credited_bytes() {
    let licenses = std::fs::read_to_string(sounds_dir().join("../LICENSES.md")).unwrap();
    let expected = [
        ("sfx", 245_719_u64),
        ("nes", 160_860),
        ("futuristic", 510_448),
        ("piano", 216_499),
    ];
    for (pack, total) in expected {
        let credits = std::fs::read_to_string(sounds_dir().join(pack).join("CREDITS.md")).unwrap();
        let sum: u64 = credits_files(&credits).iter().map(|(_, n)| n).sum();
        assert_eq!(sum, total, "{pack}: sum of CREDITS.md");
        let on_disk: u64 = credits_files(&credits)
            .iter()
            .map(|(f, _)| {
                std::fs::metadata(sounds_dir().join(pack).join(f))
                    .unwrap()
                    .len()
            })
            .sum();
        assert_eq!(on_disk, total, "{pack}: files on disk");
        let row = licenses
            .lines()
            .find(|l| l.contains(&format!("`sounds/{pack}/*.ogg`")))
            .unwrap_or_else(|| panic!("{pack}: no row in LICENSES.md"));
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        assert_eq!(cells[3], total.to_string(), "{pack}: bytes in LICENSES.md");
    }
}

#[test]
fn every_bundled_pack_has_credits_a_manifest_and_a_licenses_row() {
    let licenses = std::fs::read_to_string(sounds_dir().join("../LICENSES.md")).unwrap();
    let section = licenses
        .split("## Sons")
        .nth(1)
        .expect("a Sons section in assets/LICENSES.md");
    let bundled: Vec<_> = list_packs()
        .into_iter()
        .filter_map(|p| match p.id {
            PackId::Bundled(folder) => Some((folder, p.license)),
            _ => None,
        })
        .collect();
    assert_eq!(bundled.len(), 4);
    for (folder, license) in bundled {
        let dir = sounds_dir().join(&folder);
        let credits = std::fs::read_to_string(dir.join("CREDITS.md"))
            .unwrap_or_else(|_| panic!("{folder}: CREDITS.md"));
        assert!(credits.contains("AGPL-3.0-or-later"), "{folder}");
        assert!(credits.contains("Enigmahack"), "{folder}");
        let manifest = std::fs::read_to_string(dir.join("pack.toml"))
            .unwrap_or_else(|_| panic!("{folder}: pack.toml"));
        assert!(manifest.contains("name ="), "{folder}");
        assert!(
            section.contains(&format!("`sounds/{folder}/")),
            "{folder}: no row in assets/LICENSES.md"
        );
        assert_eq!(license, "AGPL-3.0-or-later", "{folder}");
    }
}
