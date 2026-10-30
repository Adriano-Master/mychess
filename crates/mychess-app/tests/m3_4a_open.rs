//! Contract tests of opening databases in slots (spec M3-4, rules 2-4, D503,
//! D518; Scid `sc_base_open`, `::file::openBase`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

mod m3_4a_common;
use std::io::Write as _;

use m3_4a_common::*;
use mychess_app::{BaseError, FileErrorKind, RequestId};

fn gzip(text: &str) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(text.as_bytes()).unwrap();
    enc.finish().unwrap()
}

fn assert_nothing_opened(rig: &Rig) {
    assert_eq!(rig.open_count(), 1, "only the Clipbase");
    assert_eq!(rig.current(), SlotId::CLIPBASE);
}

#[test]
fn a_pgn_opens_read_only_with_every_game_in_the_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "two.pgn", TWO));
    let slot = rig.state.bases.slot(sel(1)).unwrap();
    assert_eq!(slot.kind, BaseKind::Pgn);
    assert!(slot.read_only);
    assert_eq!((slot.total(), slot.filtered()), (2, 2));
    assert!(slot.path.as_ref().unwrap().is_absolute());
    assert_eq!(rig.state.bases.list_page(sel(1), 1, 10).len(), 2);
}

#[test]
fn a_gzipped_pgn_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two.pgn.gz");
    std::fs::write(&path, gzip(TWO)).unwrap();
    let mut rig = Rig::new();
    rig.open(&path);
    let slot = rig.state.bases.slot(sel(1)).expect("slot");
    assert_eq!((slot.kind, slot.total()), (BaseKind::Pgn, 2));
}

#[test]
fn an_unknown_extension_or_none_is_read_as_pgn() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "games.txt", TWO));
    rig.open(&write(dir.path(), "noext", TWO));
    rig.open(&write(dir.path(), "UPPER.PGN", TWO));
    for n in 1..=3 {
        let s = rig.state.bases.slot(sel(n)).expect("slot");
        assert_eq!((s.kind, s.total()), (BaseKind::Pgn, 2), "slot {n}");
    }
}

#[test]
fn an_si4_opens_editable_with_its_games() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "db", TWO);
    let mut rig = Rig::new();
    rig.open(&si4);
    let slot = rig.state.bases.slot(sel(1)).expect("slot");
    assert_eq!(slot.kind, BaseKind::Si4);
    // Slice 4c (D560): an SI4 file that can be written is editable.
    assert!(!slot.read_only, "SI4 is editable since slice 4c");
    assert_eq!((slot.total(), slot.filtered()), (2, 2));
    assert_eq!(
        slot.path.as_ref().unwrap().extension().unwrap(),
        "si4",
        "identified by the .si4 file"
    );
}

#[test]
fn naming_the_sn4_or_sg4_opens_the_si4_and_is_the_same_database() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "db", TWO);
    let mut rig = Rig::new();
    rig.open(&si4.with_extension("sn4"));
    let slot = rig.state.bases.slot(sel(1)).expect("slot");
    assert_eq!(slot.kind, BaseKind::Si4);
    assert_eq!(slot.path.as_ref().unwrap().extension().unwrap(), "si4");
    rig.select(9);
    rig.open(&si4.with_extension("sg4"));
    assert_eq!(rig.current(), sel(1), "already open: no second slot");
    rig.select(9);
    rig.open(&si4);
    assert_eq!(rig.current(), sel(1));
    assert_eq!(rig.open_count(), 2);
}

#[test]
fn an_si3_asks_for_an_upgrade_and_opens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let path = write(dir.path(), "old.si3", "whatever");
    rig.open(&path);
    assert_eq!(
        rig.notice(),
        Some(&BasesNotice::Failed(BaseError::Si3NeedsUpgrade))
    );
    assert_nothing_opened(&rig);
    assert!(
        rig.state.prefs.files.recent.is_empty(),
        "failed open is not a recent file"
    );
}

#[test]
fn a_pgn_without_games_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "empty.pgn", ""));
    assert_eq!(rig.notice(), Some(&BasesNotice::Failed(BaseError::NoGames)));
    assert_nothing_opened(&rig);
    rig.state.notice = None;
    rig.open(&write(dir.path(), "text.pgn", "just some text, no games\n"));
    assert_eq!(rig.notice(), Some(&BasesNotice::Failed(BaseError::NoGames)));
    assert_nothing_opened(&rig);
}

#[test]
fn a_missing_file_is_an_error_without_partial_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&dir.path().join("nope.pgn"));
    match rig.notice() {
        Some(BasesNotice::Failed(BaseError::File(e))) => {
            assert_eq!(e.kind, FileErrorKind::NotFound);
        }
        other => panic!("expected a NotFound failure, got {other:?}"),
    }
    assert_nothing_opened(&rig);
    // Nothing stays pending: a good file opens right after.
    rig.open(&write(dir.path(), "ok.pgn", TWO));
    assert_eq!(rig.current(), sel(1));
}

#[test]
fn a_missing_file_leaves_the_recent_list() {
    let dir = tempfile::tempdir().unwrap();
    let ghost = dir.path().join("ghost.pgn");
    let mut rig = Rig::new();
    rig.state.prefs.files.recent.push(ghost.clone());
    rig.open(&ghost);
    assert!(rig.state.prefs.files.recent.iter().all(|p| p != &ghost));
}

#[test]
fn a_directory_is_an_error_without_partial_state() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("folder.pgn");
    std::fs::create_dir(&sub).unwrap();
    let mut rig = Rig::new();
    rig.open(&sub);
    assert!(
        matches!(rig.notice(), Some(BasesNotice::Failed(_))),
        "got {:?}",
        rig.notice()
    );
    assert_nothing_opened(&rig);
}

#[test]
fn a_truncated_or_garbage_si4_is_an_error_without_partial_state() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "db", TWO);
    std::fs::write(&si4, [0u8; 7]).unwrap();
    let mut rig = Rig::new();
    rig.open(&si4);
    assert!(
        matches!(rig.notice(), Some(BasesNotice::Failed(_))),
        "got {:?}",
        rig.notice()
    );
    assert_nothing_opened(&rig);
    let junk = dir.path().join("junk.si4");
    std::fs::write(&junk, vec![0xFFu8; 4096]).unwrap();
    rig.state.notice = None;
    rig.open(&junk);
    assert!(matches!(rig.notice(), Some(BasesNotice::Failed(_))));
    assert_nothing_opened(&rig);
}

#[test]
fn binary_garbage_named_pgn_does_not_panic_nor_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bin.pgn");
    std::fs::write(&path, (0..=255u8).cycle().take(5000).collect::<Vec<_>>()).unwrap();
    let mut rig = Rig::new();
    rig.open(&path);
    assert!(
        matches!(rig.notice(), Some(BasesNotice::Failed(_))),
        "got {:?}",
        rig.notice()
    );
    assert_nothing_opened(&rig);
}

#[test]
fn cancelling_the_open_creates_no_slot_and_frees_the_flow() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let path = write(dir.path(), "two.pgn", TWO);
    let effects = rig.raw(BasesMessage::Open(path.clone()));
    assert_eq!(effects.len(), 1);
    rig.send(BasesMessage::CancelOpen);
    for e in effects {
        let a = rig.perform(e);
        rig.send(a);
    }
    assert_nothing_opened(&rig);
    // The cancel is not an error to show.
    assert!(!matches!(rig.notice(), Some(BasesNotice::Failed(e)) if *e != BaseError::Cancelled));
    rig.open(&path);
    assert_eq!(rig.current(), sel(1), "a later open works");
}

#[test]
fn cancel_open_with_nothing_in_flight_is_harmless() {
    let mut rig = Rig::new();
    rig.send(BasesMessage::CancelOpen);
    assert_nothing_opened(&rig);
}

#[test]
fn a_second_open_while_one_is_in_flight_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = write(dir.path(), "a.pgn", TWO);
    let b = write(dir.path(), "b.pgn", TWO);
    let first = rig.raw(BasesMessage::Open(a));
    assert_eq!(first.len(), 1);
    let second = rig.raw(BasesMessage::Open(b));
    assert!(second.is_empty(), "no second effect");
    assert_eq!(rig.notice(), Some(&BasesNotice::OpenInProgress));
    for e in first {
        let m = rig.perform(e);
        rig.send(m);
    }
    assert_eq!(rig.open_count(), 2);
    assert!(
        rig.state
            .bases
            .slot(sel(1))
            .unwrap()
            .path
            .as_ref()
            .unwrap()
            .ends_with("a.pgn")
    );
}

#[test]
fn an_old_opened_answer_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let a = write(dir.path(), "a.pgn", TWO);
    let b = write(dir.path(), "b.pgn", TWO);
    let ea = rig.raw(BasesMessage::Open(a));
    let old: Vec<Message> = ea.into_iter().map(|e| rig.perform(e)).collect();
    // The request of `a` is answered (and consumed) ...
    for m in old.clone() {
        rig.send(m);
    }
    assert_eq!(rig.open_count(), 2);
    // ... then b is in flight and a late duplicate of a's answer arrives.
    let eb = rig.raw(BasesMessage::Open(b));
    for m in old {
        rig.send(m);
    }
    assert_eq!(
        rig.open_count(),
        2,
        "the stale answer must not install anything"
    );
    for e in eb {
        let m = rig.perform(e);
        rig.send(m);
    }
    assert_eq!(rig.open_count(), 3);
    assert!(
        rig.state
            .bases
            .slot(sel(2))
            .unwrap()
            .path
            .as_ref()
            .unwrap()
            .ends_with("b.pgn")
    );
}

#[test]
fn an_answer_with_an_unknown_request_is_ignored() {
    let mut rig = Rig::new();
    rig.send(BasesMessage::Opened {
        request: RequestId(987_654),
        result: Err(BaseError::NoGames),
    });
    assert!(rig.notice().is_none());
    rig.send(BasesMessage::GameLoaded {
        request: RequestId(987_655),
        result: Err(BaseError::GameUnreadable { number: 1 }),
    });
    assert!(rig.notice().is_none());
    assert_nothing_opened(&rig);
}

#[test]
fn an_old_game_loaded_answer_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "two.pgn", TWO));
    let e1 = rig.raw(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    let e2 = rig.raw(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    for e in e1 {
        let m = rig.perform(e);
        rig.send(m);
    }
    assert_eq!(rig.plies(), 0, "the answer for game 1 is outdated");
    for e in e2 {
        let m = rig.perform(e);
        rig.send(m);
    }
    assert_eq!(rig.white(), "C");
    assert_eq!(rig.state.bases.slot(sel(1)).unwrap().current_game, Some(2));
}

#[test]
fn load_game_with_a_bad_number_or_slot_is_ignored_without_effects() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "two.pgn", TWO));
    rig.select(9);
    for (slot, number) in [
        (sel(1), 0),
        (sel(1), 3),
        (sel(1), u32::MAX),
        (sel(4), 1),
        (SlotId::CLIPBASE, 1),
    ] {
        let effects = rig.raw(BasesMessage::LoadGame { slot, number });
        assert!(
            effects.is_empty(),
            "({}, {number}) must do nothing",
            slot.number()
        );
        assert_eq!(rig.current(), SlotId::CLIPBASE, "and must not switch slots");
    }
}

#[test]
fn a_game_read_from_si4_is_not_overwritten_by_save() {
    let dir = tempfile::tempdir().unwrap();
    let si4 = make_si4(dir.path(), "db", TWO);
    let mut rig = Rig::new();
    rig.open(&si4);
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 1,
    });
    assert_eq!(rig.white(), "A");
    assert_eq!(rig.state.session.game().tags().black, "B");
    assert_eq!(rig.plies(), 2);
    assert_eq!(
        rig.state.origin,
        GameOrigin::New,
        "SI4 games are saved elsewhere (D519)"
    );
    assert!(!rig.state.is_dirty());
}

#[test]
fn a_game_read_from_a_pgn_slot_remembers_its_index_and_total() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    rig.open(&write(dir.path(), "two.pgn", TWO));
    rig.send(BasesMessage::LoadGame {
        slot: sel(1),
        number: 2,
    });
    assert_eq!(rig.white(), "C");
    assert!(
        matches!(
            rig.state.origin,
            GameOrigin::PgnFile {
                index: 1,
                total: 2,
                ..
            }
        ),
        "got {:?}",
        rig.state.origin
    );
    assert!(!rig.state.is_dirty());
}

#[test]
fn opening_registers_the_recent_file_and_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = Rig::new();
    let path = write(dir.path(), "two.pgn", TWO);
    rig.open(&path);
    assert_eq!(rig.state.prefs.files.recent.len(), 1);
    assert!(rig.state.prefs_dirty);
    // Activating an open file does not duplicate the entry.
    rig.select(9);
    rig.open(&path);
    assert_eq!(rig.state.prefs.files.recent.len(), 1);
}
