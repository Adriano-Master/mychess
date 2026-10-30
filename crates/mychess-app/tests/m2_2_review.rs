//! Regression tests for the review findings of slice M2-2 (save flow,
//! overwrite questions, preferences loading). See the spec section
//! "Correções após a revisão".
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    unreachable_pub
)]

mod m2_2_common;

use m2_2_common::*;
use mychess_app::{
    AppNotice, DiscardChoice, FileErrorKind, GameMessage, LoadedPreferences, OverwriteReason,
    Pending, PrefChange, PrefsMessage, SaveGuard, SaveMode, ThemePref,
};

fn saved_effect(effects: &[Effect]) -> (RequestId, SaveGuard) {
    match effects {
        [Effect::SavePgn { request, guard, .. }] => (*request, guard.clone()),
        other => panic!("one SavePgn expected: {other:?}"),
    }
}

/// A harness whose game was altered and then saved to `path` (so `Save`
/// writes there at once).
fn saved_harness(path: &std::path::Path) -> Harness {
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    h.pick(Some(path));
    assert!(matches!(h.state.origin, GameOrigin::SavedFile { .. }));
    assert!(!h.state.is_dirty());
    h
}

// ------------------------------------------------------------ finding 2

#[test]
fn save_and_continue_asks_again_when_the_game_changed_during_the_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pgn");
    let mut h = saved_harness(&path);
    h.play("e7e5");
    h.send(Command::NewGame);
    assert!(h.state.discard_prompt.is_some());
    let effects = h.raw(Message::AnswerDiscard(DiscardChoice::Save));
    let (request, _) = saved_effect(&effects);
    assert!(
        h.state.modal_open(),
        "the game is frozen while it is written"
    );
    // An edit that slips in anyway (bypassing the messages that check).
    let mv = h.state.session.position().parse_uci("g1f3").unwrap();
    drop(h.state.session.update(GameMessage::PlayMove {
        mv,
        options: mychess_app::PlayOptions::default(),
    }));
    let Effect::SavePgn { path, .. } = &effects[0] else {
        unreachable!()
    };
    let answer = Message::PgnSaved {
        request,
        mode: SaveMode::Overwrite,
        result: Ok(path.clone()),
    };
    h.send(answer);
    let prompt = h.state.discard_prompt.as_ref().expect("asked again");
    assert!(matches!(prompt.pending, Pending::Replace(_)));
    assert!(h.state.is_dirty(), "the new edit is not saved");
    assert_eq!(h.plies(), 3, "the game was not replaced");
}

#[test]
fn quit_waits_for_a_save_that_something_depends_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pgn");
    let mut h = saved_harness(&path);
    h.play("e7e5");
    h.send(Command::NewGame);
    let effects = h.raw(Message::AnswerDiscard(DiscardChoice::Save));
    assert_eq!(effects.len(), 1);
    assert!(h.raw(Message::Quit).is_empty());
    assert_eq!(h.state.notice, Some(AppNotice::SaveInProgress));
}

// ------------------------------------------------------------ finding 3

#[test]
fn a_second_save_is_refused_and_the_first_keeps_its_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pgn");
    let mut h = saved_harness(&path);
    h.play("e7e5");
    let first = h.raw(Command::Save);
    let (request, _) = saved_effect(&first);
    h.state.notice = None;
    assert!(h.raw(Command::Save).is_empty());
    assert_eq!(h.state.notice, Some(AppNotice::SaveInProgress));
    assert!(h.raw(Command::SaveAs).is_empty());
    assert!(h.raw(Command::Append).is_empty());
    // The first one fails: its error is shown.
    h.state.notice = None;
    h.send(Message::PgnSaved {
        request,
        mode: SaveMode::Overwrite,
        result: Err(mychess_app::FileError::new(
            FileErrorKind::PermissionDenied,
            Some(path),
        )),
    });
    assert!(matches!(
        h.state.notice,
        Some(AppNotice::File(ref e)) if e.kind == FileErrorKind::PermissionDenied
    ));
    assert!(h.state.is_dirty());
}

#[test]
fn quit_during_a_plain_save_joins_it_and_exits_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pgn");
    let mut h = saved_harness(&path);
    h.play("e7e5");
    let first = h.raw(Command::Save);
    h.raw(Message::Quit);
    assert!(h.state.discard_prompt.is_some());
    assert!(
        h.raw(Message::AnswerDiscard(DiscardChoice::Save))
            .is_empty()
    );
    assert!(h.state.discard_prompt.is_none());
    let answer = h.perform(first.into_iter().next().unwrap());
    let host = h.send(answer);
    assert!(
        host.iter().any(|e| matches!(e, Effect::Exit)),
        "the quit went on after the save: {host:?}"
    );
    assert!(!h.state.is_dirty());
}

// ------------------------------------------------------------ finding 4

#[test]
fn an_added_extension_never_replaces_an_existing_file_silently() {
    let dir = tempfile::tempdir().unwrap();
    let existing = write_file(dir.path(), "game.pgn", b"precious");
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    let effects = h.pick_raw(&dir.path().join("game"));
    let (_, guard) = saved_effect(&effects);
    assert!(guard.create_only);
    let answer = h.perform(effects.into_iter().next().unwrap());
    assert!(matches!(
        answer,
        Message::PgnSaved { result: Err(ref e), .. } if e.kind == FileErrorKind::AlreadyExists
    ));
    h.send(answer);
    assert_eq!(std::fs::read(&existing).unwrap(), b"precious");
    let prompt = h.state.overwrite_prompt.as_ref().expect("asked");
    assert_eq!(prompt.reason, OverwriteReason::ExistingFile);
    assert_eq!(prompt.path, dir.path().join("game.pgn"));
    assert!(h.state.notice.is_none());
    // "Yes" writes it.
    h.send(Message::AnswerOverwrite(true));
    assert_ne!(std::fs::read(&existing).unwrap(), b"precious");
    assert!(!h.state.is_dirty());
}

#[test]
fn a_new_name_or_a_confirmed_name_needs_no_question() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::SaveAs);
    h.pick(Some(&dir.path().join("fresh")));
    assert!(h.state.overwrite_prompt.is_none());
    assert!(dir.path().join("fresh.pgn").exists());
    // A name that already ends in .pgn was confirmed by the dialog.
    write_file(dir.path(), "old.pgn", b"old");
    h.play("e7e5");
    h.send(Command::SaveAs);
    let effects = h.pick_raw(&dir.path().join("old.pgn"));
    let (_, guard) = saved_effect(&effects);
    assert!(!guard.create_only);
}

#[test]
fn the_extension_question_keeps_what_waits_for_the_save() {
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "game.pgn", b"precious");
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::NewGame);
    h.send(Message::AnswerDiscard(DiscardChoice::Save));
    let effects = h.pick_raw(&dir.path().join("game"));
    let answer = h.perform(effects.into_iter().next().unwrap());
    h.send(answer);
    assert!(h.state.overwrite_prompt.is_some());
    assert_eq!(h.plies(), 1, "not replaced yet");
    h.send(Message::AnswerOverwrite(true));
    assert_eq!(h.plies(), 0, "the new game came after the save");
}

// ------------------------------------------------------------ finding 5/9

fn open_file(h: &mut Harness, path: &std::path::Path) {
    h.send(Command::Open);
    h.pick(Some(path));
    if h.state.chooser.is_some() {
        h.send(mychess_app::ChooserMessage::Open(0));
        h.send(mychess_app::ChooserMessage::Confirm);
    }
}

#[cfg(unix)]
#[test]
fn another_spelling_of_the_open_multi_game_file_is_caught_by_perform() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_file(dir.path(), "many.pgn", multi_pgn(3).as_bytes());
    let link = dir.path().join("alias.pgn");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    let mut h = Harness::new();
    open_file(&mut h, &file);
    h.send(Command::SaveAs);
    let effects = h.pick_raw(&link);
    let (_, guard) = saved_effect(&effects);
    assert!(guard.protect.is_some());
    let answer = h.perform(effects.into_iter().next().unwrap());
    h.send(answer);
    let prompt = h.state.overwrite_prompt.as_ref().expect("asked");
    assert_eq!(prompt.reason, OverwriteReason::MultiGame);
    assert_eq!(prompt.games, 3);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), multi_pgn(3));
}

#[test]
fn a_partly_read_file_asks_before_it_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    // Text outside the games is skipped by the reader.
    let text = format!("a note outside any game\n\n{}", multi_pgn(1));
    let file = write_file(dir.path(), "partial.pgn", text.as_bytes());
    let mut h = Harness::new();
    open_file(&mut h, &file);
    assert!(matches!(
        h.state.origin,
        GameOrigin::PgnFile {
            total: 1,
            partial: true,
            ..
        }
    ));
    h.send(Command::SaveAs);
    h.pick(Some(&file));
    let prompt = h.state.overwrite_prompt.as_ref().expect("asked");
    assert_eq!(prompt.reason, OverwriteReason::PartiallyRead);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    h.send(Message::AnswerOverwrite(false));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
}

#[test]
fn a_complete_single_game_file_is_not_partial() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_file(dir.path(), "one.pgn", multi_pgn(1).as_bytes());
    let mut h = Harness::new();
    open_file(&mut h, &file);
    assert!(matches!(
        h.state.origin,
        GameOrigin::PgnFile { partial: false, .. }
    ));
    h.send(Command::SaveAs);
    h.pick(Some(&file));
    assert!(h.state.overwrite_prompt.is_none());
}

// ------------------------------------------------------------ finding 10

#[test]
fn a_late_scan_answer_does_not_replace_the_open_question() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_file(dir.path(), "one.pgn", multi_pgn(1).as_bytes());
    let mut h = Harness::new();
    h.play("e2e4");
    h.send(Command::Open);
    let effects = h.pick_raw(&file);
    let scan = effects.into_iter().next().unwrap();
    h.raw(Message::Quit);
    let Some(prompt) = &h.state.discard_prompt else {
        panic!("the quit asks first");
    };
    assert_eq!(prompt.pending, Pending::Quit);
    let answer = h.perform(scan);
    h.send(answer);
    let prompt = h.state.discard_prompt.as_ref().expect("still asking");
    assert_eq!(prompt.pending, Pending::Quit);
    assert_eq!(h.plies(), 1);
}

// ------------------------------------------------------------ finding 8

#[test]
fn nothing_is_written_before_the_preferences_were_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    std::fs::write(&path, "[board]\ntheme = \"wood\"\n").unwrap();
    let mut h = Harness::with_prefs_path(Some(path.clone()));
    let startup = h.raw(Message::Startup);
    // The bookmarks and the engine list are read at startup too.
    let load = startup
        .into_iter()
        .find(|e| matches!(e, Effect::LoadPreferences { .. }))
        .expect("the preferences are read");
    // The user changes something and asks to quit before the answer.
    let effects = h.raw(PrefsMessage::Set(PrefChange::Theme(ThemePref::Dark)));
    assert!(effects.is_empty(), "no save yet: {effects:?}");
    assert!(h.raw(Message::Quit).is_empty());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[board]\ntheme = \"wood\"\n",
        "the file is untouched"
    );
    // The read finishes: the change is applied over the loaded values and
    // saved, and then the program exits.
    let loaded = h.perform(load);
    let host = h.send(loaded);
    assert_eq!(h.state.prefs.board.theme, "wood");
    assert_eq!(h.state.prefs.general.theme, ThemePref::Dark);
    assert!(host.iter().any(|e| matches!(e, Effect::Exit)), "{host:?}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("wood") && text.contains("dark"), "{text}");
}

#[test]
fn window_geometry_reported_while_loading_survives() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    std::fs::write(&path, "[window]\nwidth = 900\nheight = 700\n").unwrap();
    let mut h = Harness::with_prefs_path(Some(path));
    let load = h.raw(Message::Startup).into_iter().next().unwrap();
    h.raw(Message::WindowGeometryChanged(
        mychess_app::WindowGeometry {
            width: 1000,
            height: 800,
            x: Some(1),
            y: Some(2),
            maximized: false,
        },
    ));
    let loaded = h.perform(load);
    h.raw(loaded);
    assert_eq!(h.state.prefs.window.width, Some(1000));
    assert!(h.state.prefs_dirty);
}

#[test]
fn saving_works_as_before_once_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.toml");
    let mut h = Harness::with_prefs_path(Some(path));
    h.raw(Message::PreferencesLoaded(LoadedPreferences::default()));
    let effects = h.raw(PrefsMessage::Set(PrefChange::Theme(ThemePref::Light)));
    assert!(matches!(
        effects.as_slice(),
        [Effect::SavePreferences { .. }]
    ));
}

#[test]
fn a_failing_save_reports_once() {
    let mut h = Harness::new();
    for _ in 0..3 {
        h.raw(Message::PreferencesSaved(Err(mychess_app::FileError::new(
            FileErrorKind::PermissionDenied,
            None,
        ))));
    }
    let count = h
        .state
        .prefs_issues
        .iter()
        .filter(|i| matches!(i, mychess_app::PrefsIssue::SaveFailed(_)))
        .count();
    assert_eq!(count, 1);
}

// ------------------------------------------------------------ finding 11

#[test]
fn only_a_missing_file_leaves_the_recent_list() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_file(dir.path(), "g.pgn", multi_pgn(1).as_bytes());
    let mut h = Harness::new();
    open_file(&mut h, &file);
    assert_eq!(h.state.prefs.files.recent.iter().count(), 1);
    // The file still exists but holds no game now.
    std::fs::write(&file, "nothing here\n").unwrap();
    h.send(Command::OpenRecent(0));
    assert!(matches!(
        h.state.notice,
        Some(AppNotice::File(ref e)) if matches!(e.kind, FileErrorKind::NoGamesFound { .. })
    ));
    assert_eq!(h.state.prefs.files.recent.iter().count(), 1);
    std::fs::remove_file(&file).unwrap();
    h.send(Command::OpenRecent(0));
    assert_eq!(h.state.prefs.files.recent.iter().count(), 0);
}
