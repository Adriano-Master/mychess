//! M3-8 contract: importing into a new database when the current slot
//! cannot take games (spec `docs/specs/m3-8-importar-arquivo-pgn.md`,
//! D1973-D1975).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m3_8_common;

use m3_8_common::*;
use std::path::PathBuf;
use tempfile::tempdir;

/// Starts the import, answers the file dialog and returns the dialog that
/// names the new database.
fn ask_name(rig: &mut Rig, files: &[PathBuf]) -> PickSpec {
    let effects = rig.start_import(files);
    assert!(
        !effects.iter().any(is_import_job),
        "no job before the name is known"
    );
    pick_of(&effects, PickKind::NewBase)
}

/// Names the database and runs everything that follows.
fn name_it(rig: &mut Rig, spec: &PickSpec, path: Option<PathBuf>) {
    rig.send(Message::FilePicked(spec.request, path));
}

#[test]
fn d1973_the_clipbase_does_not_take_the_import_and_asks_for_a_new_base() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    assert!(rig.state.bases.current().is_clipbase());
    let pgn = write(dir.path(), "Games.PGN", &games(2));
    let spec = ask_name(&mut rig, &[pgn]);
    assert_eq!(spec.file_name.as_deref(), Some("Games.si4"));
    assert_eq!(spec.initial_dir.as_deref(), Some(dir.path()));
}

#[test]
fn d1973_a_pgn_slot_asks_for_a_new_base_and_the_result_opens_in_a_free_slot() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let pgn_slot = rig.open_pgn(dir.path(), "view.pgn", &games(1));
    let pgn = write(dir.path(), "src.pgn.gz.txt", &games(4));
    let spec = ask_name(&mut rig, &[pgn]);
    let target = dir.path().join("fresh");
    name_it(&mut rig, &spec, Some(target.clone()));
    // D1974: the extension was added; D1975: the base is open in a new slot.
    let si4 = dir.path().join("fresh.si4");
    assert!(si4.exists());
    assert_eq!(games_on_disk(&si4), 4);
    assert_ne!(rig.state.bases.current(), pgn_slot);
    assert_eq!(rig.current_total(), 4);
    assert_eq!(rig.current_filtered(), 4);
    assert_eq!(
        std::fs::canonicalize(rig.current_path()).unwrap(),
        std::fs::canonicalize(&si4).unwrap()
    );
    assert_eq!(rig.notice(), Some(FileImportNotice::Imported { count: 4 }));
}

#[test]
fn d1973_a_read_only_si4_asks_for_a_new_base() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, path) = rig.open_si4(dir.path(), "ro", &games(2));
    rig.send(BasesMessage::List(ListMessage::SetReadOnly {
        slot,
        read_only: true,
    }));
    let pgn = write(dir.path(), "x.pgn", &games(3));
    let spec = ask_name(&mut rig, &[pgn]);
    name_it(&mut rig, &spec, Some(dir.path().join("copy.si4")));
    assert_eq!(games_on_disk(&path), 2, "the read-only base is untouched");
    assert_eq!(games_on_disk(&dir.path().join("copy.si4")), 3);
}

#[test]
fn d1974_an_existing_database_is_never_overwritten() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let taken = make_si4(dir.path(), "taken", &games(2));
    let pgn = write(dir.path(), "x.pgn", &games(3));
    let spec = ask_name(&mut rig, &[pgn]);
    name_it(&mut rig, &spec, Some(taken.clone()));
    assert_eq!(games_on_disk(&taken), 2);
    let result = rig
        .state
        .file_import_view()
        .and_then(|v| v.result)
        .expect("the refusal is shown");
    assert_eq!(result.error, Some(BaseError::AlreadyExists));
    assert_eq!(
        rig.notice(),
        Some(FileImportNotice::NotStarted(BaseError::AlreadyExists))
    );
    // Nothing was opened.
    assert!(rig.state.bases.current().is_clipbase());
}

#[test]
fn d1975_nothing_stored_removes_the_database_that_was_created() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let pgn = write(dir.path(), "empty.pgn", "");
    let spec = ask_name(&mut rig, &[pgn]);
    name_it(&mut rig, &spec, Some(dir.path().join("none.si4")));
    for ext in ["si4", "sn4", "sg4"] {
        assert!(
            !dir.path().join("none").with_extension(ext).exists(),
            "{ext} must be gone"
        );
    }
    assert!(rig.state.bases.current().is_clipbase());
    assert_eq!(rig.notice(), Some(FileImportNotice::NothingImported));
}

#[test]
fn cancelling_the_name_dialog_does_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let pgn = write(dir.path(), "x.pgn", &games(2));
    let spec = ask_name(&mut rig, &[pgn]);
    name_it(&mut rig, &spec, None);
    assert!(rig.state.file_import_view().is_none());
    assert!(rig.state.list_job().is_none());
    assert!(!rig.state.modal_open());
}

#[test]
fn an_answer_with_an_unknown_id_is_ignored() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let pgn = write(dir.path(), "x.pgn", &games(2));
    let spec = ask_name(&mut rig, &[pgn]);
    let wrong = RequestId(spec.request.0 + 1000);
    let effects = rig.raw(Message::FilePicked(wrong, Some(dir.path().join("a.si4"))));
    assert!(effects.is_empty());
    assert!(rig.state.list_job().is_none());
    // The right answer still works.
    name_it(&mut rig, &spec, Some(dir.path().join("a.si4")));
    assert_eq!(games_on_disk(&dir.path().join("a.si4")), 2);
}

#[test]
fn an_empty_choice_of_files_cancels() {
    let mut rig = Rig::new();
    let effects = rig.raw(FileImportMessage::Open);
    let spec = pick_of(&effects, PickKind::ImportPgn);
    let effects = rig.raw(Message::FilesPicked(spec.request, Vec::new()));
    assert!(effects.is_empty());
    assert!(rig.state.file_import_view().is_none());
}

#[test]
fn d1986_the_import_belongs_to_no_assistance_family() {
    use mychess_app::online::family_of;
    use mychess_app::{Action, Message};
    assert_eq!(
        family_of(&Message::FileImport(FileImportMessage::Open)),
        None
    );
    assert_eq!(family_of(&Message::FilesPicked(RequestId(1), vec![])), None);
    assert_eq!(family_of(&Message::Action(Action::ImportPgnFile)), None);
}
