//! M4-3 contract tests of the import of a Scid `engines.dat` and of the disk
//! check of the editor through `update` + `perform` (D432, D690, D727;
//! rules 10 and 12).
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string
)]

mod m4_3_common;

use m4_3_common::*;
use mychess_app::engines::{
    CommandWarning, EditorMessage, EngineMessage, EngineNotice, FileProbe, MAX_SCID_BYTES,
};
use mychess_app::{AppState, FileErrorKind};

const GOOD: &str = "engine {\n Name {A one}\n Cmd /bin/a\n Dir /bin\n UCI 1\n}\nengine {\n Name B\n Cmd b\n Dir .\n}\n";

async fn import(state: &mut AppState, bytes: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.dat");
    std::fs::write(&path, bytes).unwrap();
    drive(state, EngineMessage::ImportScid(path)).await;
}

#[tokio::test]
async fn importing_adds_engines_reports_counts_and_asks_for_a_save() {
    let mut state =
        AppState::with_preferences_path(Some(std::path::PathBuf::from("cfg").join("p.toml")));
    send(&mut state, EngineMessage::OpenDialog);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("engines.dat");
    std::fs::write(&path, GOOD).unwrap();
    let fx = send(&mut state, EngineMessage::ImportScid(path.clone()));
    assert!(
        saves(&fx).is_empty(),
        "nothing is saved before the file is read"
    );
    let mut all = settle_collect(&mut state, fx).await;
    assert_eq!(state.engines.len(), 2);
    assert_eq!(
        notice(&state),
        Some(&EngineNotice::Imported {
            added: 2,
            duplicates: 0,
            invalid: 0
        })
    );
    assert!(
        all.iter()
            .any(|e| matches!(e, mychess_app::Effect::SaveEngines { .. }))
    );
    let report = state
        .engine_dialog
        .as_ref()
        .unwrap()
        .import
        .clone()
        .unwrap();
    assert_eq!(report.added.len(), 2);
    assert!(!report.warnings.is_empty(), "B has a relative path");
    // Importing the same file again skips both.
    send(&mut state, EngineMessage::Saved(Ok(())));
    let fx = send(&mut state, EngineMessage::ImportScid(path));
    all = settle_collect(&mut state, fx).await;
    assert_eq!(state.engines.len(), 2);
    assert_eq!(
        notice(&state),
        Some(&EngineNotice::Imported {
            added: 0,
            duplicates: 2,
            invalid: 0
        })
    );
    assert!(
        !all.iter()
            .any(|e| matches!(e, mychess_app::Effect::SaveEngines { .. }))
    );
}

async fn settle_collect(
    state: &mut AppState,
    fx: Vec<mychess_app::Effect>,
) -> Vec<mychess_app::Effect> {
    // `settle` hides the effects `update` returns for answers; collect them too.
    let mut out = Vec::new();
    let mut queue: std::collections::VecDeque<_> = fx.into();
    while let Some(effect) = queue.pop_front() {
        if effect.runs_on_host() {
            continue;
        }
        let answer = mychess_app::perform(effect).await;
        let more = mychess_app::update(state, answer);
        out.extend(more.iter().cloned());
        queue.extend(more);
    }
    out
}

#[tokio::test]
async fn a_file_without_engines_says_nothing_to_import_and_a_missing_file_fails_cleanly() {
    let mut state = AppState::default();
    import(&mut state, b"# nothing here\nset a 1\n").await;
    assert_eq!(notice(&state), Some(&EngineNotice::NothingToImport));
    assert!(state.engines.is_empty());
    drive(
        &mut state,
        EngineMessage::ImportScid(std::path::PathBuf::from("definitely").join("missing.dat")),
    )
    .await;
    match notice(&state) {
        Some(EngineNotice::ImportFailed(e)) => assert_eq!(e.kind, FileErrorKind::NotFound),
        other => panic!("expected ImportFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn a_latin1_file_with_invalid_utf8_is_read_lossily_not_rejected() {
    let mut bytes = b"engine {\n Name {Caf".to_vec();
    bytes.push(0xe9);
    bytes.extend_from_slice(b"}\n Cmd /c\n Dir /d\n}\n");
    let mut state = AppState::default();
    import(&mut state, &bytes).await;
    assert_eq!(state.engines.len(), 1);
    assert!(state.engines.engines()[0].name.starts_with("Caf"));
}

#[tokio::test]
async fn a_file_over_4_mib_is_read_only_up_to_the_limit_and_the_first_engines_survive() {
    let mut bytes = GOOD.as_bytes().to_vec();
    bytes.extend(std::iter::repeat_n(b'#', MAX_SCID_BYTES + 1000));
    let mut state = AppState::default();
    import(&mut state, &bytes).await;
    assert_eq!(state.engines.len(), 2);
}

#[tokio::test]
async fn a_stale_import_answer_is_ignored() {
    let mut state = AppState::default();
    let fx = send(
        &mut state,
        EngineMessage::ImportScid(std::path::PathBuf::from("x.dat")),
    );
    drop(fx);
    send(
        &mut state,
        EngineMessage::ScidRead {
            request: mychess_app::RequestId(424_242),
            result: Box::new(Ok(mychess_app::engines::parse_scid_engines_dat(GOOD))),
        },
    );
    assert!(state.engines.is_empty());
}

// ---- disk check of the editor ----

#[tokio::test]
async fn the_check_effect_warns_about_a_missing_command_and_folder_without_blocking_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::default();
    send(&mut state, EngineMessage::New);
    let missing = dir.path().join("nope").join("engine");
    for m in [
        EditorMessage::SetName("N".into()),
        EditorMessage::SetCommand(missing.to_string_lossy().into_owned()),
        EditorMessage::SetWorkingDir(dir.path().join("nodir").to_string_lossy().into_owned()),
    ] {
        send(&mut state, EngineMessage::Editor(m));
    }
    let fx = send(&mut state, EngineMessage::Editor(EditorMessage::Check));
    assert_eq!(fx.len(), 1);
    settle(&mut state, fx).await;
    let w = &state.engine_editor.as_ref().unwrap().warnings;
    assert!(w.contains(&CommandWarning::CommandNotFound), "{w:?}");
    assert!(w.contains(&CommandWarning::WorkingDirMissing), "{w:?}");
    send(&mut state, EngineMessage::Editor(EditorMessage::Save));
    assert_eq!(state.engines.len(), 1, "warnings do not block");
}

#[tokio::test]
async fn editing_the_command_clears_the_old_warnings_and_a_stale_check_answer_is_dropped() {
    let mut state = AppState::default();
    send(&mut state, EngineMessage::New);
    send(
        &mut state,
        EngineMessage::Editor(EditorMessage::SetCommand("zzz-nonexistent".into())),
    );
    let fx = send(&mut state, EngineMessage::Editor(EditorMessage::Check));
    // The user edits before the answer arrives.
    send(
        &mut state,
        EngineMessage::Editor(EditorMessage::SetCommand("other".into())),
    );
    settle(&mut state, fx).await;
    assert!(state.engine_editor.as_ref().unwrap().warnings.is_empty());
    // An empty command: no effect at all.
    send(
        &mut state,
        EngineMessage::Editor(EditorMessage::SetCommand("  ".into())),
    );
    assert!(send(&mut state, EngineMessage::Editor(EditorMessage::Check)).is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn a_file_without_the_execute_bit_is_flagged_not_executable() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("engine");
    std::fs::write(&file, "x").unwrap();
    let mut state = AppState::default();
    send(&mut state, EngineMessage::New);
    send(
        &mut state,
        EngineMessage::Editor(EditorMessage::SetCommand(
            file.to_string_lossy().into_owned(),
        )),
    );
    let fx = send(&mut state, EngineMessage::Editor(EditorMessage::Check));
    settle(&mut state, fx).await;
    let w = &state.engine_editor.as_ref().unwrap().warnings;
    assert!(w.contains(&CommandWarning::NotExecutable), "{w:?}");
    let _ = std::marker::PhantomData::<&dyn FileProbe>;
}
