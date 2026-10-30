//! Contract tests of the refusals of a copy between databases (spec M3-4,
//! rule 8 and D576; Scid `switcher.tcl` `copyFilter` 1139-1170, where the
//! last failing test overwrites the message of the earlier ones).
#![allow(
    clippy::assert_is_empty,
    clippy::format_push_string,
    clippy::unwrap_used,
    clippy::expect_used
)]
mod m3_4d_common;

use m3_4d_common::*;

fn empty_the_filter(rig: &mut Rig, slot: SlotId) {
    rig.send(l(ListMessage::NegateFilter { slot }));
    assert_eq!(rig.state.bases.slot(slot).unwrap().filtered(), 0);
}

fn setup() -> Pair {
    pair(&numbered(5), &numbered(3))
}

fn assert_nothing_happened(p: &Pair, total_to: u32) {
    assert_eq!(p.rig.state.bases.slot(p.to).unwrap().total(), total_to);
    assert!(p.rig.state.list_job().is_none());
    assert!(p.rig.state.copy_prompt.is_none());
}

#[test]
fn the_same_database_is_refused_and_names_it_on_both_ends() {
    let mut p = setup();
    let from = p.from;
    let effects = p.rig.raw(copy_msg(from, from, CopyScope::Filter));
    assert!(effects.is_empty(), "no job starts");
    let (reason, a, b) = refusal(&p.rig).expect("a refusal");
    assert_eq!(reason, CopyRefusal::SameBase);
    assert_eq!((a, b), (end(from, "src.pgn"), end(from, "src.pgn")));
    assert_nothing_happened(&p, 3);
}

#[test]
fn the_clipbase_onto_itself_is_the_same_database() {
    let mut p = setup();
    p.rig.raw(copy_msg(
        SlotId::CLIPBASE,
        SlotId::CLIPBASE,
        CopyScope::Filter,
    ));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::SameBase);
}

#[test]
fn a_source_that_is_not_open_is_refused_naming_the_target() {
    let mut p = setup();
    let free = sel(5);
    p.rig.raw(copy_msg(free, p.to, CopyScope::Filter));
    let (reason, _, to) = refusal(&p.rig).expect("a refusal");
    assert_eq!(reason, CopyRefusal::SourceNotOpen);
    assert_eq!(to.name, "dst.si4");
    assert_eq!(to.slot, p.to);
}

#[test]
fn a_target_that_is_not_open_is_refused_naming_the_source() {
    let mut p = setup();
    let free = sel(6);
    p.rig.raw(copy_msg(p.from, free, CopyScope::Filter));
    let (reason, from, to) = refusal(&p.rig).expect("a refusal");
    assert_eq!(reason, CopyRefusal::TargetNotOpen);
    assert_eq!(from.name, "src.pgn");
    assert_eq!(to.slot, free);
}

#[test]
fn a_closed_source_beats_a_closed_target() {
    // Scid: "source not open" is tested after "target not open".
    let mut p = setup();
    p.rig.raw(copy_msg(sel(5), sel(6), CopyScope::Filter));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::SourceNotOpen);
}

#[test]
fn the_same_closed_slot_is_the_same_database_before_anything_else() {
    let mut p = setup();
    p.rig.raw(copy_msg(sel(5), sel(5), CopyScope::Filter));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::SameBase);
}

#[test]
fn a_pgn_target_is_read_only_even_with_an_empty_source_filter() {
    let mut p = setup();
    // The .si4 (slot 2) is the source and its filter is empty; the PGN
    // (slot 1) is the target.
    empty_the_filter(&mut p.rig, p.to);
    p.rig.raw(copy_msg(p.to, p.from, CopyScope::Filter));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::TargetReadOnly);
}

#[test]
fn a_session_read_only_mark_makes_the_target_read_only_and_beats_an_empty_filter() {
    let mut p = setup();
    p.rig.send(l(ListMessage::SetReadOnly {
        slot: p.to,
        read_only: true,
    }));
    empty_the_filter(&mut p.rig, p.from);
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::TargetReadOnly);
    // Unmarked, the empty filter is what the user hears about.
    p.rig.send(l(ListMessage::SetReadOnly {
        slot: p.to,
        read_only: false,
    }));
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::EmptyFilter);
    assert_nothing_happened(&p, 3);
}

#[test]
fn an_empty_filter_is_refused_naming_both_ends() {
    let mut p = setup();
    empty_the_filter(&mut p.rig, p.from);
    let effects = p.rig.raw(copy_msg(p.from, p.to, CopyScope::Filter));
    assert!(effects.is_empty());
    let (reason, from, to) = refusal(&p.rig).expect("a refusal");
    assert_eq!(reason, CopyRefusal::EmptyFilter);
    assert_eq!((from, to), (end(p.from, "src.pgn"), end(p.to, "dst.si4")));
}

#[test]
fn an_empty_filter_leaves_nothing_selected_for_a_copy_of_the_selection() {
    let mut p = setup();
    empty_the_filter(&mut p.rig, p.from);
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Selection));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::NothingSelected);
}

#[test]
fn an_empty_selection_is_refused_and_a_read_only_target_beats_it() {
    let mut p = setup();
    p.rig.raw(copy_msg(p.from, p.to, CopyScope::Selection));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::NothingSelected);
    // A PGN target with nothing selected: read-only wins.
    p.rig.raw(copy_msg(p.to, p.from, CopyScope::Selection));
    assert_eq!(refusal(&p.rig).unwrap().0, CopyRefusal::TargetReadOnly);
    assert_nothing_happened(&p, 3);
}

#[test]
fn every_failing_combination_reports_the_last_test_of_the_scid() {
    // Precedence: same > source closed > target closed > read-only target >
    // empty filter.
    let mut p = setup();
    empty_the_filter(&mut p.rig, p.from);
    let cases = [
        (p.from, p.from, CopyRefusal::SameBase),
        (sel(5), p.to, CopyRefusal::SourceNotOpen),
        (sel(5), sel(6), CopyRefusal::SourceNotOpen),
        (p.from, sel(6), CopyRefusal::TargetNotOpen),
        (p.to, p.from, CopyRefusal::TargetReadOnly),
        (p.from, p.to, CopyRefusal::EmptyFilter),
    ];
    for (from, to, expected) in cases {
        p.rig.clear_notice();
        p.rig.raw(copy_msg(from, to, CopyScope::Filter));
        assert_eq!(
            refusal(&p.rig).map(|r| r.0),
            Some(expected),
            "{from:?} -> {to:?}"
        );
    }
}

#[test]
fn a_refusal_changes_nothing_in_either_database() {
    let mut p = setup();
    let before_from = p.rig.filter_numbers(p.from);
    let current = p.rig.state.bases.current();
    p.rig.raw(copy_msg(p.from, p.from, CopyScope::Filter));
    p.rig.raw(copy_msg(p.to, p.from, CopyScope::Filter));
    assert_eq!(p.rig.filter_numbers(p.from), before_from);
    assert_eq!(p.rig.state.bases.current(), current);
    assert_eq!(disk(&p.path).len(), 3);
}

#[cfg(unix)]
#[test]
fn an_si4_without_write_permission_is_a_read_only_target() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    let from = rig.open_games(dir.path(), "src.pgn", &numbered(2));
    let path = make_si4(dir.path(), "locked", &pgn(&numbered(2)));
    for ext in ["si4", "sn4", "sg4"] {
        let file = path.with_extension(ext);
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions).unwrap();
    }
    // As root the permission bits do not stop a write: skip.
    if std::fs::OpenOptions::new().write(true).open(&path).is_ok() {
        eprintln!("skipped: the files are writable (root?)");
        return;
    }
    let to = rig.open(&path);
    confirm(&mut rig, false);
    assert!(rig.state.bases.slot(to).unwrap().is_locked_read_only());
    let effects = rig.raw(copy_msg(from, to, CopyScope::Filter));
    assert!(effects.is_empty());
    assert_eq!(
        refusal(&rig).map(|r| r.0),
        Some(CopyRefusal::TargetReadOnly)
    );
    // Unmarking does not unlock it (D582).
    rig.send(l(ListMessage::SetReadOnly {
        slot: to,
        read_only: false,
    }));
    assert!(rig.state.bases.slot(to).unwrap().read_only);
    rig.raw(copy_msg(from, to, CopyScope::Filter));
    assert_eq!(
        refusal(&rig).map(|r| r.0),
        Some(CopyRefusal::TargetReadOnly)
    );
}

#[test]
fn the_clipbase_as_a_target_is_never_read_only() {
    let mut p = setup();
    p.rig
        .send(copy_msg(p.from, SlotId::CLIPBASE, CopyScope::Filter));
    assert_eq!(whites(&p.rig, SlotId::CLIPBASE).len(), 5);
    assert!(refusal(&p.rig).is_none());
}

#[test]
fn a_clipbase_as_the_source_works_like_any_other() {
    let mut p = setup();
    p.rig
        .send(copy_msg(p.from, SlotId::CLIPBASE, CopyScope::Filter));
    p.rig
        .send(copy_msg(SlotId::CLIPBASE, p.to, CopyScope::Filter));
    assert_eq!(p.rig.state.bases.slot(p.to).unwrap().total(), 8);
    let on_disk = disk(&p.path);
    assert_eq!(on_disk[3].white, "P1");
}
