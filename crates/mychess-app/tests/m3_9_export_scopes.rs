//! M3-9 contract: what is exported (spec
//! `docs/specs/m3-9-exportar-partidas.md`, D2001, D2007, D2008).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

mod m3_9_common;

use m3_9_common::*;
use tempfile::tempdir;

#[test]
fn d2001_the_default_is_the_selection_then_the_filter() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "b", &numbered(5));
    rig.gx(GamesExportMessage::Open);
    let view = rig.state.games_export_view().unwrap();
    assert_eq!(view.draft.scope, ExportScope::Filter);
    assert_eq!(
        (view.counts.all, view.counts.filter, view.counts.selection),
        (5, 5, 0)
    );
    assert!(view.draft.comments && view.draft.variations && view.draft.marks);
    assert!(!view.draft.only_tactics && !view.draft.append);
    rig.gx(GamesExportMessage::Close);
    rig.send(list(ListMessage::SelectNumbers {
        slot,
        numbers: vec![2, 4],
    }));
    rig.gx(GamesExportMessage::Open);
    let view = rig.state.games_export_view().unwrap();
    assert_eq!(view.draft.scope, ExportScope::Selection);
    assert_eq!(view.counts.selection, 2);
}

#[test]
fn d2001_a_scope_without_games_cannot_be_chosen() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(3));
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetScope(ExportScope::Selection));
    assert_eq!(
        rig.state.games_export_view().unwrap().draft.scope,
        ExportScope::Filter
    );
    rig.gx(GamesExportMessage::SetScope(ExportScope::All));
    assert_eq!(
        rig.state.games_export_view().unwrap().draft.scope,
        ExportScope::All
    );
}

#[test]
fn d2007_the_selection_is_written_in_the_order_of_the_database() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "b", &numbered(5));
    rig.send(list(ListMessage::SelectNumbers {
        slot,
        numbers: vec![4, 2],
    }));
    let dest = dir.path().join("out.pgn");
    rig.gx_all(&dest);
    assert_eq!(whites_in(&text_of(&dest)), ["P2", "P4"]);
    assert_eq!(
        notice(&rig),
        Some(GamesExportNotice::Exported {
            count: 2,
            name: "out.pgn".into()
        })
    );
    // A clean end closes the window (D2004).
    assert!(rig.state.games_export_view().is_none());
    assert!(!rig.state.modal_open());
}

#[test]
fn the_filter_exports_two_of_five() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "b", &numbered(5));
    // Filter to games 2 and 5: select the others and remove them.
    rig.send(list(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1, 3, 4],
    }));
    rig.send(list(ListMessage::RemoveSelected { slot }));
    rig.send(list(ListMessage::SelectNumbers {
        slot,
        numbers: vec![],
    }));
    assert_eq!(rig.current_filtered(), 2);
    let dest = dir.path().join("f.pgn");
    rig.gx_all(&dest);
    assert_eq!(whites_in(&text_of(&dest)), ["P2", "P5"]);
}

#[test]
fn all_ignores_the_filter() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "b", &numbered(4));
    rig.send(list(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1],
    }));
    rig.send(list(ListMessage::RemoveSelected { slot }));
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetScope(ExportScope::All));
    let dest = dir.path().join("all.pgn");
    let (effects, _) = rig.gx_choose(&dest);
    rig.gx_run(effects);
    assert_eq!(whites_in(&text_of(&dest)).len(), 4);
}

#[test]
fn d2008_only_tactics_keeps_the_flagged_games() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let path = make_si4(dir.path(), "t", &numbered(5));
    flag_tactics(&path, &[2, 5]);
    rig.send(BasesMessage::Open(path));
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetOnlyTactics(true));
    let dest = dir.path().join("t.pgn");
    let (effects, _) = rig.gx_choose(&dest);
    rig.gx_run(effects);
    assert_eq!(whites_in(&text_of(&dest)), ["P2", "P5"]);
}

#[test]
fn d2008_only_tactics_without_any_flagged_game_writes_nothing() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_si4(dir.path(), "b", &numbered(3));
    rig.gx(GamesExportMessage::Open);
    rig.gx(GamesExportMessage::SetOnlyTactics(true));
    let dest = dir.path().join("none.pgn");
    let (effects, _) = rig.gx_choose(&dest);
    rig.gx_run(effects);
    assert!(!dest.exists());
    assert!(!has_temp(dir.path()));
    assert_eq!(notice(&rig), Some(GamesExportNotice::NothingToExport));
}

#[test]
fn a_pgn_file_opened_as_a_base_can_be_exported() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open_pgn(dir.path(), "src.pgn", &numbered(3));
    let dest = dir.path().join("copy.pgn");
    rig.gx_all(&dest);
    assert_eq!(whites_in(&text_of(&dest)), ["P1", "P2", "P3"]);
}

#[test]
fn the_clipbase_is_read_only_source_and_works() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    let (slot, _) = rig.open_si4(dir.path(), "b", &numbered(3));
    rig.send(list(ListMessage::SelectNumbers {
        slot,
        numbers: vec![1, 3],
    }));
    rig.send(list(ListMessage::CopySelected { slot }));
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    let dest = dir.path().join("clip.pgn");
    rig.gx_all(&dest);
    assert_eq!(whites_in(&text_of(&dest)), ["P1", "P3"]);
}

#[test]
fn the_current_game_is_exported_with_its_edits() {
    let dir = tempdir().unwrap();
    let mut rig = Rig::new();
    rig.gx(GamesExportMessage::Open);
    let view = rig.state.games_export_view().unwrap();
    assert_eq!(view.draft.scope, ExportScope::Current);
    let dest = dir.path().join("cur.pgn");
    let (effects, _) = rig.gx_choose(&dest);
    rig.gx_run(effects);
    assert_eq!(text_of(&dest).matches("[Event ").count(), 1);
}
