//! M7-1a3: `repair_database` (spec M7-1, rule 43, D3526, D3527, D3580-D3583).
//! Damaged databases are made by patching the bytes of healthy ones written
//! with `Si4Writer`.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::range_plus_one,
    clippy::assert_is_empty,
    clippy::case_sensitive_file_extension_comparisons
)]

#[path = "common/m7_1a2_support.rs"]
mod support;

use std::fs;
use std::path::{Path, PathBuf};

use mychess_core::{Game, GameResult};
use mychess_db::si4::{
    FailPoint, NameKind, RepairOptions, Si4Database, Si4Error, Si4Writer, repair_database,
    repair_database_with_hook,
};
use mychess_db::{CancelToken, MaintError, NoProgress};
use support::{build_games, line, snapshot};

const HEADER: usize = 182;
const ENTRY: usize = 47;

fn game(white: &str, black: &str, event: &str) -> Game {
    let mut g = line(&["e2e4", "e7e5", "g1f3"]);
    let t = g.tags_mut();
    t.white = white.into();
    t.black = black.into();
    t.event = event.into();
    t.result = GameResult::Draw;
    g
}

/// Players Alfa 0, Beta 1, Gama 2, Delta 3; events Aberto 0, Copa 1.
fn four() -> Vec<Game> {
    vec![
        game("Alfa, A", "Beta, B", "Aberto"),
        game("Gama, G", "Delta, D", "Aberto"),
        game("Alfa, A", "Delta, D", "Copa"),
        game("Beta, B", "Gama, G", "Copa"),
    ]
}

fn make() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &four());
    (dir, base)
}

fn at(n: usize) -> usize {
    HEADER + ENTRY * (n - 1)
}

fn patch(base: &Path, ext: &str, offset: usize, bytes: &[u8]) {
    let path = base.with_extension(ext);
    let mut data = fs::read(&path).unwrap();
    data[offset..offset + bytes.len()].copy_from_slice(bytes);
    fs::write(path, data).unwrap();
}

fn repair(base: &Path, drop_unreadable: bool) -> mychess_db::si4::RepairReport {
    repair_database(
        base,
        RepairOptions { drop_unreadable },
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap()
}

fn players(base: &Path, n: u32) -> (String, String) {
    let mut db = Si4Database::open(base).unwrap();
    let g = db.game(n).unwrap();
    (g.tags().white.clone(), g.tags().black.clone())
}

fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// Game 2 and 4 stop using Gama; the file still lists it with a stale count.
fn unused_gama() -> (tempfile::TempDir, PathBuf) {
    let (dir, base) = make();
    patch(&base, "si4", at(2) + 10, &[0, 0]);
    patch(&base, "si4", at(4) + 12, &[0, 0]);
    (dir, base)
}

#[test]
fn unused_names_leave_ids_are_renumbered_and_the_game_file_is_untouched() {
    let (_dir, base) = unused_gama();
    let [_, _, sg4_before] = snapshot(&base);
    let report = repair(&base, false);
    assert_eq!(report.games, 4);
    assert_eq!(report.names_removed, [1, 0, 0, 0]);
    assert!(!report.header_fixed);
    assert_eq!(report.ids_fixed, 0);
    assert_eq!(snapshot(&base)[2], sg4_before, ".sg4 must be byte for byte");
    assert_eq!(players(&base, 2), ("Alfa, A".into(), "Delta, D".into()));
    assert_eq!(players(&base, 3), ("Alfa, A".into(), "Delta, D".into()));
    assert_eq!(players(&base, 4), ("Beta, B".into(), "Alfa, A".into()));
    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.names().num_names(NameKind::Player), 3);
    // Alfa: games 1, 2, 3 (white) and 4 (black); Beta: 1 and 4.
    assert_eq!(db.names().frequency(NameKind::Player, 0), Some(4));
    assert_eq!(db.names().frequency(NameKind::Player, 1), Some(2));
}

#[test]
fn a_name_id_outside_the_name_file_becomes_the_unknown_name() {
    let (_dir, base) = make();
    patch(&base, "si4", at(3) + 15, &[0x12, 0x34]);
    assert!(Si4Database::open(&base).is_err());
    let report = repair(&base, false);
    assert_eq!((report.ids_fixed, report.games_with_bad_ids), (1, 1));
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.game(3).unwrap().tags().event, "?");
    assert_eq!(db.game(4).unwrap().tags().event, "Copa");
    assert_eq!(db.game(1).unwrap().tags().event, "Aberto");
    assert_eq!(db.game(3).unwrap().tags().white, "Alfa, A");
}

#[test]
fn a_truncated_index_is_read_by_the_size_of_the_file() {
    let (_dir, base) = make();
    let [si4, _, _] = snapshot(&base);
    let cut = si4.len() - 20;
    fs::write(base.with_extension("si4"), &si4[..cut]).unwrap();
    assert!(matches!(
        Si4Database::open(&base),
        Err(Si4Error::Truncated { .. })
    ));
    let report = repair(&base, false);
    assert_eq!((report.games, report.games_read), (3, 3));
    assert!(report.header_fixed);
    assert_eq!(report.trailing_bytes, 27);
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 3);
    for n in 1..=3 {
        db.game(n).unwrap();
    }
    assert_eq!(players(&base, 3), ("Alfa, A".into(), "Delta, D".into()));
}

#[test]
fn a_header_that_counts_fewer_games_than_the_file_holds_is_fixed() {
    let (_dir, base) = make();
    patch(&base, "si4", 14, &[0, 0, 2]);
    assert_eq!(Si4Database::open(&base).unwrap().len(), 2);
    let report = repair(&base, false);
    assert!(report.header_fixed);
    assert_eq!(report.games, 4);
    assert_eq!(Si4Database::open(&base).unwrap().len(), 4);
}

#[test]
fn entries_pointing_outside_the_game_file_are_dropped_only_on_request() {
    let (_dir, base) = make();
    let [si4, _, _] = snapshot(&base);
    patch(&base, "si4", at(2), &0x7FFF_0000u32.to_be_bytes());
    let report = repair(&base, false);
    assert_eq!((report.games, report.entries_dropped), (4, 0));
    assert!(Si4Database::open(&base).unwrap().game(2).is_err());

    let (_dir2, base2) = make();
    patch(&base2, "si4", at(2), &0x7FFF_0000u32.to_be_bytes());
    let report = repair(&base2, true);
    assert_eq!((report.games, report.entries_dropped), (3, 1));
    assert_eq!(report.games_read, 4);
    let mut db = Si4Database::open(&base2).unwrap();
    assert_eq!(db.len(), 3);
    for n in 1..=3 {
        db.game(n).unwrap();
    }
    assert_eq!(players(&base2, 2), ("Alfa, A".into(), "Delta, D".into()));
    // The entries kept are the originals (game 1, 3, 4).
    let after = fs::read(base2.with_extension("si4")).unwrap();
    assert_eq!(after[at(1)..at(1) + ENTRY], si4[at(1)..at(1) + ENTRY]);
    assert_eq!(after[at(2)..at(2) + ENTRY], si4[at(3)..at(3) + ENTRY]);
}

#[test]
fn a_game_file_cut_in_the_middle_loses_only_the_games_past_the_cut() {
    let (_dir, base) = make();
    let [si4, _, sg4] = snapshot(&base);
    let offset4 = u32::from_be_bytes(si4[at(4)..at(4) + 4].try_into().unwrap()) as usize;
    fs::write(base.with_extension("sg4"), &sg4[..offset4 + 1]).unwrap();
    let report = repair(&base, true);
    assert_eq!((report.games, report.entries_dropped), (3, 1));
    let mut db = Si4Database::open(&base).unwrap();
    for n in 1..=3 {
        db.game(n).unwrap();
    }
}

#[test]
fn backups_keep_the_originals_and_are_never_overwritten() {
    let (_dir, base) = unused_gama();
    let [si4, sn4, _] = snapshot(&base);
    let first = repair(&base, false);
    assert_eq!(first.backups.len(), 2);
    assert_eq!(fs::read(&first.backups[0]).unwrap(), si4);
    assert_eq!(fs::read(&first.backups[1]).unwrap(), sn4);
    assert!(first.backups[0].to_str().unwrap().ends_with(".si4.bak"));
    let [si4_after, sn4_after, _] = snapshot(&base);
    let second = repair(&base, false);
    assert!(second.backups[0].to_str().unwrap().ends_with(".si4.bak1"));
    assert_eq!(fs::read(&second.backups[0]).unwrap(), si4_after);
    assert_eq!(fs::read(&second.backups[1]).unwrap(), sn4_after);
    // The first generation is untouched.
    assert_eq!(fs::read(&first.backups[0]).unwrap(), si4);
    let third = repair(&base, false);
    assert!(third.backups[0].to_str().unwrap().ends_with(".si4.bak2"));
}

#[test]
fn repairing_twice_is_idempotent() {
    let (_dir, base) = unused_gama();
    patch(&base, "si4", at(3) + 15, &[0x12, 0x34]);
    repair(&base, false);
    let after_first = snapshot(&base);
    let report = repair(&base, false);
    assert_eq!(snapshot(&base), after_first);
    assert_eq!(report.names_removed, [0; 4]);
    assert_eq!(report.ids_fixed, 0);
    assert!(!report.header_fixed);
}

#[test]
fn repairing_a_healthy_database_keeps_every_game_and_entry() {
    let (_dir, base) = make();
    let [si4, _, sg4] = snapshot(&base);
    let before: Vec<_> = (1..=4).map(|n| players(&base, n)).collect();
    let report = repair(&base, false);
    assert_eq!(report.games, 4);
    assert_eq!(report.names_removed, [0; 4]);
    let [si4_after, _, sg4_after] = snapshot(&base);
    assert_eq!(si4_after, si4);
    assert_eq!(sg4_after, sg4);
    let after: Vec<_> = (1..=4).map(|n| players(&base, n)).collect();
    assert_eq!(after, before);
}

#[test]
fn a_database_open_in_a_writer_is_refused_and_nothing_changes() {
    let (dir, base) = unused_gama();
    let before = snapshot(&base);
    let names = dir_names(dir.path());
    let writer = Si4Writer::open(&base).unwrap();
    let error = repair_database(
        &base,
        RepairOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(
        matches!(error.source, Si4Error::AlreadyOpen { .. }),
        "{error}"
    );
    let wrapped = mychess_db::repair_database(
        &base,
        RepairOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(
        wrapped,
        MaintError::Si4(Si4Error::AlreadyOpen { .. })
    ));
    drop(writer);
    assert_eq!(snapshot(&base), before);
    // Only the lock file of the writer-less open may differ; no backup.
    assert!(
        dir_names(dir.path())
            .iter()
            .all(|n| !n.contains("bak") && !n.ends_with(".tmp"))
    );
    assert_eq!(dir_names(dir.path()), names);
}

#[test]
fn an_unusable_header_or_name_file_is_an_error_with_no_backup() {
    let (dir, base) = make();
    let before = snapshot(&base);
    patch(&base, "si4", 0, b"XXXX");
    let broken = snapshot(&base);
    let error = repair_database(
        &base,
        RepairOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(error.source, Si4Error::BadMagic { .. }), "{error}");
    assert_eq!(snapshot(&base), broken);
    fs::write(base.with_extension("si4"), &before[0]).unwrap();
    fs::write(base.with_extension("sn4"), &before[1][..20]).unwrap();
    assert!(
        repair_database(
            &base,
            RepairOptions::default(),
            &mut NoProgress,
            &CancelToken::new()
        )
        .is_err()
    );
    assert!(dir_names(dir.path()).iter().all(|n| !n.contains("bak")));
}

fn copy_db(from: &Path, to_dir: &Path) -> PathBuf {
    for ext in ["si4", "sn4", "sg4"] {
        fs::copy(from.with_extension(ext), to_dir.join(format!("a2.{ext}"))).unwrap();
    }
    to_dir.join("a2")
}

#[test]
fn a_failure_at_any_stage_leaves_a_database_that_opens_and_never_a_mix() {
    let (dir, base) = unused_gama();
    let original = snapshot(&base);
    // Learn the stages from a run that does not fail.
    let probe_dir = tempfile::tempdir().unwrap();
    let probe = copy_db(&base, probe_dir.path());
    let mut hook = FailPoint::never();
    repair_database_with_hook(
        &probe,
        RepairOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
        &mut hook,
    )
    .unwrap();
    let labels: Vec<String> = hook.reached().to_vec();
    assert!(labels.iter().any(|l| l == "backup:sn4"));
    let journal_at = labels.iter().position(|l| l == "journal").unwrap();
    let repaired = snapshot(&probe);
    drop(dir);

    for (i, label) in labels.iter().enumerate() {
        let work = tempfile::tempdir().unwrap();
        let base = copy_db(&probe_dir.path().join("a2"), work.path());
        // Start again from the damaged originals.
        for (ext, bytes) in ["si4", "sn4", "sg4"].iter().zip(&original) {
            fs::write(base.with_extension(ext), bytes).unwrap();
        }
        let error = repair_database_with_hook(
            &base,
            RepairOptions::default(),
            &mut NoProgress,
            &CancelToken::new(),
            &mut FailPoint::at(i),
        )
        .unwrap_err();
        assert!(
            matches!(error.source, Si4Error::InjectedFailure { .. }),
            "{label}"
        );
        if i < journal_at {
            assert_eq!(snapshot(&base), original, "{label}: original intact");
        }
        if label != "backup:si4" && i > 0 && labels[..i].iter().any(|l| l == "backup:si4") {
            assert_eq!(
                fs::read(base.with_extension("si4.bak")).unwrap(),
                original[0],
                "{label}: the backup is complete"
            );
        }
        // The next writer finishes or discards what was left.
        let writer = Si4Writer::open(&base).unwrap();
        drop(writer);
        let now = snapshot(&base);
        assert!(now == original || now == repaired, "{label}: old or new");
        let names = dir_names(work.path());
        assert!(
            names
                .iter()
                .all(|n| !n.ends_with(".tmp") && !n.ends_with(".txn") && !n.ends_with(".lock")),
            "{label}: {names:?}"
        );
        let mut db = Si4Database::open(&base).unwrap();
        for n in 1..=4 {
            db.game(n).unwrap();
        }
    }
}

fn many(n: usize) -> Vec<Game> {
    (0..n)
        .map(|i| game(&format!("P{i}, X"), &format!("Q{}, Y", i % 7), "Aberto"))
        .collect()
}

#[test]
fn cancelling_changes_nothing_and_removes_the_backups_it_made() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &many(600));
    patch(&base, "si4", at(5) + 10, &[0, 0]);
    let before = snapshot(&base);
    let names = dir_names(dir.path());
    // Cancelled in the second pass, after the backups.
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let report = repair_database(
        &base,
        RepairOptions::default(),
        &mut |done: u64, _total: u64| {
            if done > 600 {
                trigger.cancel();
            }
        },
        &cancel,
    )
    .unwrap();
    assert!(report.interrupted);
    assert!(report.backups.is_empty());
    assert_eq!(snapshot(&base), before);
    assert_eq!(dir_names(dir.path()), names);
    // Cancelled before anything.
    let cancel = CancelToken::new();
    cancel.cancel();
    let report =
        repair_database(&base, RepairOptions::default(), &mut NoProgress, &cancel).unwrap();
    assert!(report.interrupted);
    assert_eq!(snapshot(&base), before);
    assert_eq!(dir_names(dir.path()), names);
    // And the same database still repairs afterwards.
    let report = repair(&base, false);
    assert!(!report.interrupted);
    assert_eq!(report.games, 600);
}

#[test]
fn progress_never_goes_backwards_and_ends_at_the_total() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &many(700));
    let mut calls: Vec<(u64, u64)> = Vec::new();
    repair_database(
        &base,
        RepairOptions {
            drop_unreadable: true,
        },
        &mut |done: u64, total: u64| calls.push((done, total)),
        &CancelToken::new(),
    )
    .unwrap();
    assert!(calls.len() > 3);
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0), "{calls:?}");
    assert!(calls.iter().all(|&(d, t)| d <= t));
    assert_eq!(*calls.last().unwrap(), (2100, 2100));
}

#[test]
fn a_pending_journal_is_finished_before_the_repair() {
    // A repair killed right after the commit leaves a journal; repairing again
    // rolls it forward first instead of failing.
    let (_dir, base) = unused_gama();
    let error = repair_database_with_hook(
        &base,
        RepairOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
        &mut FailPoint::at_label("journal"),
    )
    .unwrap_err();
    assert!(matches!(error.source, Si4Error::InjectedFailure { .. }));
    let report = repair(&base, false);
    assert_eq!(report.games, 4);
    assert_eq!(
        report.names_removed, [0; 4],
        "already repaired by the journal"
    );
    assert_eq!(Si4Database::open(&base).unwrap().len(), 4);
}

// ---- the real error path (not a simulated crash): the temporaries are
// removed, the originals and the backups stay -------------------------------

fn blocked_by_a_directory(blocker_ext: &str) {
    let (dir, base) = unused_gama();
    let original = snapshot(&base);
    let blocker = dir.path().join(format!("a2.{blocker_ext}.tmp"));
    // `File::create` of the staged file fails: the same exit as a full disk.
    fs::create_dir(&blocker).unwrap();
    let error = repair_database(
        &base,
        RepairOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    assert!(matches!(error.source, Si4Error::Io { .. }), "{error:?}");
    assert_eq!(snapshot(&base), original, "originals intact");
    // The error tells where the originals were saved.
    assert_eq!(error.backups.len(), 2, "{error:?}");
    assert!(error.backups[0].ends_with("a2.si4.bak"));
    assert!(error.backups[1].ends_with("a2.sn4.bak"));
    assert_eq!(fs::read(&error.backups[0]).unwrap(), original[0]);
    assert_eq!(fs::read(&error.backups[1]).unwrap(), original[1]);
    let names = dir_names(dir.path());
    assert!(
        !names.iter().any(|n| n.ends_with(".txn")),
        "no journal: {names:?}"
    );
    assert!(blocker.is_dir(), "the blocker is not ours to touch");
    for other in ["si4", "sn4"] {
        if other != blocker_ext {
            let leftover = dir.path().join(format!("a2.{other}.tmp"));
            assert!(!leftover.exists(), "{other}: staged file removed");
        }
    }
    // And the maintenance wrapper keeps the backups in its error.
    fs::remove_dir(&blocker).unwrap();
}

#[test]
fn a_real_failure_staging_the_names_keeps_originals_and_backups() {
    blocked_by_a_directory("sn4");
}

#[test]
fn a_real_failure_staging_the_index_keeps_originals_and_backups() {
    blocked_by_a_directory("si4");
}

#[test]
fn the_maintenance_wrapper_reports_the_backups_of_a_failed_repair() {
    let (dir, base) = unused_gama();
    fs::create_dir(dir.path().join("a2.si4.tmp")).unwrap();
    let error = mychess_db::repair_database(
        &base,
        RepairOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap_err();
    match error {
        MaintError::Repair(e) => assert_eq!(e.backups.len(), 2),
        other => panic!("{other:?}"),
    }
}

// ---- backups: never replace, clean up, orphans -----------------------------

#[test]
fn a_taken_names_backup_moves_both_backups_to_the_next_generation() {
    let (dir, base) = unused_gama();
    let original = snapshot(&base);
    fs::write(dir.path().join("a2.sn4.bak"), b"someone else").unwrap();
    let report = repair(&base, false);
    assert_eq!(report.backups.len(), 2);
    assert!(report.backups[0].ends_with("a2.si4.bak1"));
    assert!(report.backups[1].ends_with("a2.sn4.bak1"));
    assert_eq!(fs::read(&report.backups[0]).unwrap(), original[0]);
    assert_eq!(
        fs::read(dir.path().join("a2.sn4.bak")).unwrap(),
        b"someone else"
    );
    assert!(
        !dir.path().join("a2.si4.bak").exists(),
        "no half generation is left"
    );
}

#[test]
fn orphan_backup_temporaries_are_removed_by_the_next_open() {
    let (dir, base) = make();
    for name in ["a2.si4.bak.tmp", "a2.sn4.bak.tmp", "a2.si4.bak3.tmp"] {
        fs::write(dir.path().join(name), b"partial").unwrap();
    }
    fs::write(dir.path().join("a2.si4.bak3"), b"a real backup").unwrap();
    drop(Si4Writer::open(&base).unwrap());
    let names = dir_names(dir.path());
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");
    assert!(names.iter().any(|n| n == "a2.si4.bak3"), "kept: {names:?}");
}

// ---- the file wins over the header ----------------------------------------

#[test]
fn bytes_beyond_the_header_count_become_entries_and_are_reported() {
    let (_dir, base) = make();
    let [si4, _, _] = snapshot(&base);
    // Header: 4 games; the file holds 2 more whole entries and 10 stray bytes.
    let mut data = si4.clone();
    data.extend_from_slice(&si4[at(1)..at(3)]);
    data.extend_from_slice(&[0xEE; 10]);
    fs::write(base.with_extension("si4"), &data).unwrap();
    let report = repair(&base, false);
    assert_eq!(report.games_read, 6);
    assert_eq!(report.entries_beyond_header, 2);
    assert_eq!(report.trailing_bytes, 10);
    assert!(report.header_fixed);
    assert_eq!(report.games, 6);
}

#[test]
fn unreadable_entries_beyond_the_header_are_dropped_on_request() {
    let (_dir, base) = make();
    let [si4, _, _] = snapshot(&base);
    let mut data = si4.clone();
    // Two entries of garbage that point outside the game file.
    data.extend_from_slice(&[0x11; 2 * ENTRY]);
    fs::write(base.with_extension("si4"), &data).unwrap();
    let report = repair(&base, true);
    assert_eq!(report.entries_beyond_header, 2);
    assert_eq!(
        (report.games_read, report.entries_dropped, report.games),
        (6, 2, 4)
    );
    let mut db = Si4Database::open(&base).unwrap();
    for n in 1..=4 {
        db.game(n).unwrap();
    }
}

#[test]
fn a_header_that_says_more_than_the_file_has_no_entries_beyond() {
    let (_dir, base) = make();
    let [si4, _, _] = snapshot(&base);
    fs::write(base.with_extension("si4"), &si4[..si4.len() - ENTRY]).unwrap();
    let report = repair(&base, false);
    assert_eq!((report.games_read, report.entries_beyond_header), (3, 0));
    assert!(report.header_fixed);
}

// ---- more than 65 536 names: the high bits of the IDs ----------------------

/// 70 000 games, game `n` played by `P{n-1:05}` (all different).
fn many_names(dir: &Path) -> PathBuf {
    let base = dir.join("big");
    let mut writer = Si4Writer::create(&base, 1).unwrap();
    for chunk in 0..7 {
        let prepared: Vec<_> = (0..10_000)
            .map(|i| {
                let mut g = line(&["e2e4"]);
                g.tags_mut().white = format!("P{:05}", chunk * 10_000 + i);
                mychess_db::si4::PreparedGame::from_game(&g, mychess_db::si4::GameFlags::empty())
                    .unwrap()
            })
            .collect();
        writer.add_prepared_batch(&prepared).unwrap();
    }
    assert_eq!(writer.names().num_names(NameKind::Player), 70_001); // + "Beta, B"
    base
}

fn assert_whites_follow_the_games(base: &Path, first: &str) {
    let mut db = Si4Database::open(base).unwrap();
    assert_eq!(db.len(), 70_000);
    for n in [2u32, 100, 65_535, 65_536, 65_537, 65_538, 69_999, 70_000] {
        assert_eq!(
            db.game(n).unwrap().tags().white,
            format!("P{:05}", n - 1),
            "game {n}"
        );
    }
    assert_eq!(db.game(1).unwrap().tags().white, first);
    assert_eq!(db.game(1).unwrap().tags().black, "Beta, B");
}

#[test]
fn repair_renumbers_more_than_65536_names_without_scrambling_them() {
    let dir = tempfile::tempdir().unwrap();
    let base = many_names(dir.path());
    // Game 1 stops using P00000, so every ID after it moves down by one and
    // the IDs around 65 536 cross the 16-bit boundary.
    patch(&base, "si4", HEADER + 10, &[0, 5]);
    let report = repair_database(
        &base,
        RepairOptions::default(),
        &mut NoProgress,
        &CancelToken::new(),
    )
    .unwrap();
    assert_eq!(report.names_removed[0], 1);
    // Name ID 5 is `P00004` (ID 1 is the black player of game 1).
    assert_whites_follow_the_games(&base, "P00004");
}

#[test]
fn compact_names_renumbers_more_than_65536_names_without_scrambling_them() {
    let dir = tempfile::tempdir().unwrap();
    let base = many_names(dir.path());
    let mut writer = Si4Writer::open(&base).unwrap();
    let mut g = line(&["e2e4"]);
    g.tags_mut().white = "P00005".into();
    writer.replace_game(1, &g).unwrap();
    let (writer, report) = mychess_db::si4::compact_names(writer).unwrap();
    assert_eq!(report.removed[0], 1);
    drop(writer);
    assert_whites_follow_the_games(&base, "P00005");
}
