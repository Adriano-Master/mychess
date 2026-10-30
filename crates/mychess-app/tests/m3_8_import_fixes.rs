//! M3-8 review fixes, through the public API (spec
//! `docs/specs/m3-8-importar-arquivo-pgn.md`, D1993-D1996). The cases that
//! need a closed slot or a job stopped at a known point are unit tests in
//! `src/file_import/tests.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m3_8_common;

use m3_8_common::*;
use tempfile::tempdir;

#[test]
fn d1996_a_source_that_is_a_file_of_the_database_is_skipped_and_reported() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (_, path) = rig.open_si4(dir.path(), "base", &games(2));
    let own = path.with_extension("sg4");
    let good = write(dir.path(), "good.pgn", &games(3));
    rig.import(&[own, good]);
    assert_eq!(games_on_disk(&path), 5);
    let result = rig
        .state
        .file_import_view()
        .and_then(|v| v.result)
        .expect("an error line keeps the window");
    assert_eq!(result.report.file_errors, [(0, BaseError::SourceIsTarget)]);
}

#[test]
fn d1993_a_stop_before_anything_is_stored_leaves_no_window_and_no_database() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let pgn = write(dir.path(), "x.pgn", &games(3));
    let effects = rig.start_import(&[pgn]);
    let spec = pick_of(&effects, PickKind::NewBase);
    let target = dir.path().join("fresh.si4");
    let effects = rig.raw(Message::FilePicked(spec.request, Some(target.clone())));
    rig.send(BasesMessage::List(ListMessage::CancelJob));
    for effect in effects {
        let answer = rig.perform(effect);
        rig.send(answer);
    }
    assert!(!target.exists());
    assert!(rig.state.file_import_view().is_none());
}
