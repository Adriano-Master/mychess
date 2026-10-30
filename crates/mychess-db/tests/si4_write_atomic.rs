//! Crash consistency of the multi-file operations (compact games, compact
//! names, `.si3` upgrade) with the `FailPoint` hooks, recovery on open, and
//! hostile journals (spec M3-2b: "Atomicidade", decisions D290, D310-D312,
//! D319; review fix: journal names are restricted to the database).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::assertions_on_result_states,
    clippy::assert_is_empty,
    clippy::case_sensitive_file_extension_comparisons
)]

#[path = "common/si4_write_support.rs"]
mod support;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use mychess_db::si4::compact::{compact_games_with_hook, compact_names_with_hook};
use mychess_db::si4::journal::{pending_journal, recover};
use mychess_db::si4::si3::upgrade_with_hook;
use mychess_db::si4::{
    FailPoint, FlagOp, GameFlags, Recovery, Result, Si4Database, Si4Error, Si4Writer,
};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{ext, game_of_record_len, named, tmp};

type Files = BTreeMap<String, Vec<u8>>;

fn snapshot(dir: &Path) -> Files {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().unwrap().is_file())
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------- setups

fn setup_games(base: &Path) {
    let mut w = Si4Writer::create(base, 1).unwrap();
    for i in 0..4 {
        w.add_game(
            &named(&format!("W{i}"), "B", "E", "S", &i.to_string()),
            GameFlags::empty(),
        )
        .unwrap();
    }
    w.add_game(&game_of_record_len(5_000), GameFlags::empty())
        .unwrap();
    w.set_flag(2, GameFlags::DELETE, FlagOp::Set).unwrap();
    w.set_flag(4, GameFlags::DELETE, FlagOp::Set).unwrap();
    w.set_description("desc").unwrap();
}

fn setup_names(base: &Path) {
    let mut w = Si4Writer::create(base, 1).unwrap();
    for i in 0..4 {
        w.add_game(
            &named(&format!("W{i}"), "B", "E", "S", &i.to_string()),
            GameFlags::empty(),
        )
        .unwrap();
    }
    w.replace_game(2, &named("W9", "B", "E", "S", "1")).unwrap();
}

fn setup_si3(base: &Path) {
    let mut w = Si4Writer::create(base, 4).unwrap();
    for i in 0..3 {
        w.add_game(
            &named(&format!("W{i}"), "B", "E", "S", &i.to_string()),
            GameFlags::empty(),
        )
        .unwrap();
    }
    drop(w);
    let si4 = fs::read(ext(base, "si4")).unwrap();
    let mut old = si4[..128].to_vec();
    old[8..10].copy_from_slice(&310u16.to_be_bytes());
    for chunk in si4[182..].chunks(47) {
        old.extend_from_slice(&chunk[..6]);
        old.extend_from_slice(&chunk[7..]);
    }
    for (a, b) in [("si4", "si3"), ("sn4", "sn3"), ("sg4", "sg3")] {
        let data = if a == "si4" {
            old.clone()
        } else {
            fs::read(ext(base, a)).unwrap()
        };
        fs::write(ext(base, b), data).unwrap();
        fs::remove_file(ext(base, a)).unwrap();
    }
}

// ---------------------------------------------------------------- harness

struct Op {
    name: &'static str,
    setup: fn(&Path),
    run: fn(&Path, &mut FailPoint) -> Result<()>,
}

fn run_games(base: &Path, hook: &mut FailPoint) -> Result<()> {
    compact_games_with_hook(Si4Writer::open(base)?, hook).map(|_| ())
}
fn run_names(base: &Path, hook: &mut FailPoint) -> Result<()> {
    compact_names_with_hook(Si4Writer::open(base)?, hook).map(|_| ())
}
fn run_si3(base: &Path, hook: &mut FailPoint) -> Result<()> {
    upgrade_with_hook(base, hook).map(|_| ())
}

const OPS: [Op; 3] = [
    Op {
        name: "compact games",
        setup: setup_games,
        run: run_games,
    },
    Op {
        name: "compact names",
        setup: setup_names,
        run: run_names,
    },
    Op {
        name: "si3 upgrade",
        setup: setup_si3,
        run: run_si3,
    },
];

fn fresh(op: &Op) -> (tempfile::TempDir, PathBuf) {
    let (dir, base) = tmp();
    (op.setup)(&base);
    (dir, base)
}

fn is_committed(label: &str) -> bool {
    label == "journal" || label.starts_with("step:")
}

#[test]
fn every_stage_of_every_multi_file_operation_leaves_the_old_or_the_new_database() {
    for op in &OPS {
        // reference runs: old state, new state and the list of stages
        let (old_dir, _) = fresh(op);
        let old_files = snapshot(old_dir.path());
        let (new_dir, new_base) = fresh(op);
        let mut hook = FailPoint::never();
        (op.run)(&new_base, &mut hook).unwrap();
        let labels: Vec<String> = hook.reached().to_vec();
        let new_files = snapshot(new_dir.path());
        assert!(labels.len() >= 5, "{}: stages {labels:?}", op.name);
        assert!(
            labels.iter().any(|l| l == "journal"),
            "{}: has a commit point",
            op.name
        );
        assert_ne!(
            old_files, new_files,
            "{}: operation changes something",
            op.name
        );
        assert!(
            !new_files
                .keys()
                .any(|k| k.ends_with(".tmp") || k.ends_with(".txn")),
            "{}: clean after success: {:?}",
            op.name,
            new_files.keys()
        );

        for (i, label) in labels.iter().enumerate() {
            let what = format!("{} failing at stage {i} ({label})", op.name);
            let (dir, base) = fresh(op);
            let mut hook = FailPoint::at(i);
            let err = (op.run)(&base, &mut hook).unwrap_err();
            assert!(
                matches!(err, Si4Error::InjectedFailure { index, .. } if index == i),
                "{what}: {err:?}"
            );
            assert_eq!(hook.reached().len(), i + 1, "{what}");

            let committed = is_committed(label);
            // The injected failure leaves the disk as a killed process would.
            assert_eq!(
                pending_journal(&base).is_some(),
                committed,
                "{what}: journal presence"
            );
            let before_reader = snapshot(dir.path());
            if committed {
                let err = Si4Database::open(&base).map(|_| ()).unwrap_err();
                assert!(
                    matches!(err, Si4Error::PendingJournal { .. }),
                    "{what}: {err:?}"
                );
            }
            assert_eq!(
                snapshot(dir.path()),
                before_reader,
                "{what}: the reader changes nothing"
            );

            // Recovery.
            let report = recover(&base).unwrap_or_else(|e| panic!("{what}: recover {e:?}"));
            assert_eq!(report.journal_completed, committed, "{what}");
            let after = snapshot(dir.path());
            assert!(
                !after
                    .keys()
                    .any(|k| k.ends_with(".tmp") || k.ends_with(".txn")),
                "{what}: leftovers {:?}",
                after.keys()
            );
            let expected = if committed { &new_files } else { &old_files };
            assert_eq!(
                after.keys().collect::<Vec<_>>(),
                expected.keys().collect::<Vec<_>>(),
                "{what}: file set"
            );
            for (name, bytes) in expected {
                assert!(after[name] == *bytes, "{what}: {name} differs");
            }
            // Recovery is idempotent.
            assert_eq!(recover(&base).unwrap(), Recovery::default(), "{what}");
        }
    }
}

#[test]
fn writer_open_finishes_a_committed_journal_and_cleans_orphans() {
    for op in &OPS[..2] {
        let (_d, nb) = fresh(op);
        let mut hook = FailPoint::never();
        (op.run)(&nb, &mut hook).unwrap();
        let new_files = snapshot(nb.parent().unwrap());
        let commit = hook.reached().iter().position(|l| l == "journal").unwrap();

        let (dir, base) = fresh(op);
        assert!((op.run)(&base, &mut FailPoint::at(commit)).is_err());
        assert!(pending_journal(&base).is_some());

        let w = Si4Writer::open(&base).unwrap();
        drop(w);
        assert_eq!(snapshot(dir.path()), new_files, "{}", op.name);
    }
}

#[test]
fn reader_ignores_orphan_tmp_files_and_deletes_nothing() {
    let (dir, base) = tmp();
    setup_games(&base);
    for e in ["si4.tmp", "sn4.tmp", "sg4.tmp", "txn.tmp"] {
        fs::write(ext(&base, e), b"orphan").unwrap();
    }
    let before = snapshot(dir.path());
    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 5);
    drop(db);
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn writer_open_removes_the_four_kinds_of_orphan_tmp_files() {
    let (dir, base) = tmp();
    setup_games(&base);
    let clean = snapshot(dir.path());
    for e in ["si4.tmp", "sn4.tmp", "sg4.tmp", "txn.tmp"] {
        fs::write(ext(&base, e), b"orphan").unwrap();
    }
    drop(Si4Writer::open(&base).unwrap());
    assert_eq!(snapshot(dir.path()), clean);
}

#[test]
fn recover_counts_orphans_and_does_nothing_on_a_clean_database() {
    let (_dir, base) = tmp();
    setup_games(&base);
    assert_eq!(recover(&base).unwrap(), Recovery::default());
    fs::write(ext(&base, "sn4.tmp"), b"x").unwrap();
    fs::write(ext(&base, "sg4.tmp"), b"x").unwrap();
    let r = recover(&base).unwrap();
    assert!(!r.journal_completed);
    assert_eq!(r.orphans_removed, 2);
}

// ---------------------------------------------------------------- lock vs recovery

#[test]
fn open_does_not_delete_tmp_files_while_another_holder_has_the_lock() {
    // Another process in the middle of a transaction holds the lock of the
    // .si4 and has .tmp files on disk: a concurrent open must not remove them.
    let (dir, base) = tmp();
    setup_games(&base);
    let holder = File::options()
        .read(true)
        .write(true)
        .open(ext(&base, "si4"))
        .unwrap();
    holder.try_lock().unwrap();
    for e in ["sn4.tmp", "sg4.tmp", "si4.tmp"] {
        fs::write(ext(&base, e), b"in progress").unwrap();
    }
    let before = snapshot(dir.path());

    let err = Si4Writer::open(&base).map(|_| ()).unwrap_err();
    assert!(matches!(err, Si4Error::AlreadyOpen { .. }), "{err:?}");
    let err = recover(&base).unwrap_err();
    assert!(matches!(err, Si4Error::AlreadyOpen { .. }), "{err:?}");

    assert_eq!(snapshot(dir.path()), before, "temporary files survive");
    drop(holder);
}

#[test]
fn open_does_not_complete_a_journal_while_another_holder_has_the_lock() {
    let (dir, base) = tmp();
    let op = &OPS[0];
    (op.setup)(&base);
    let mut hook = FailPoint::never();
    let (_d, nb) = fresh(op);
    (op.run)(&nb, &mut hook).unwrap();
    let commit = hook.reached().iter().position(|l| l == "journal").unwrap();
    assert!((op.run)(&base, &mut FailPoint::at(commit)).is_err());
    let before = snapshot(dir.path());

    let holder = File::options()
        .read(true)
        .write(true)
        .open(ext(&base, "si4"))
        .unwrap();
    holder.try_lock().unwrap();
    assert!(matches!(
        Si4Writer::open(&base),
        Err(Si4Error::AlreadyOpen { .. })
    ));
    assert_eq!(snapshot(dir.path()), before);
}

// ---------------------------------------------------------------- hostile journals

const OUTSIDE: &[u8] = b"not part of the database";

struct Hostile {
    dir: tempfile::TempDir,
    base: PathBuf,
}

/// A database in `dir/db/games`, a neighbour file `dir/db/outro.pgn` and
/// `dir/victim.txt` above it.
fn hostile_setup() -> Hostile {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("db");
    fs::create_dir(&sub).unwrap();
    let base = sub.join("games");
    setup_games(&base);
    fs::write(sub.join("outro.pgn"), OUTSIDE).unwrap();
    fs::write(sub.join("x.pdf"), OUTSIDE).unwrap();
    fs::write(dir.path().join("victim.txt"), OUTSIDE).unwrap();
    Hostile { dir, base }
}

fn journal_text(steps: &[String]) -> String {
    let mut s = String::from("mychess-txn 1\nop evil\n");
    for step in steps {
        s.push_str(step);
        s.push('\n');
    }
    s.push_str("end\n");
    s
}

fn assert_rejected(h: &Hostile, text: &str, what: &str) {
    let journal = ext(&h.base, "txn");
    fs::write(&journal, text).unwrap();
    let dir = h.base.parent().unwrap();
    let before = snapshot(dir);
    let outer_before = fs::read(h.dir.path().join("victim.txt")).unwrap();

    let e1 = Si4Writer::open(&h.base).map(|_| ()).unwrap_err();
    assert!(
        matches!(e1, Si4Error::CorruptJournal { .. }),
        "{what}: open {e1:?}"
    );
    let e2 = recover(&h.base).unwrap_err();
    assert!(
        matches!(e2, Si4Error::CorruptJournal { .. }),
        "{what}: recover {e2:?}"
    );
    let e3 = Si4Database::open(&h.base).map(|_| ()).unwrap_err();
    assert!(
        matches!(e3, Si4Error::PendingJournal { .. }),
        "{what}: reader {e3:?}"
    );

    assert_eq!(
        snapshot(dir),
        before,
        "{what}: no file of the directory changed"
    );
    assert!(journal.exists(), "{what}: the journal is kept");
    assert_eq!(
        fs::read(h.dir.path().join("victim.txt")).unwrap(),
        outer_before
    );
    fs::remove_file(&journal).unwrap();
}

#[test]
fn journal_cannot_remove_a_neighbouring_file() {
    let h = hostile_setup();
    assert_rejected(
        &h,
        &journal_text(&["remove\toutro.pgn".into()]),
        "remove outro.pgn",
    );
    assert_eq!(
        fs::read(h.base.parent().unwrap().join("outro.pgn")).unwrap(),
        OUTSIDE
    );
}

#[test]
fn journal_cannot_overwrite_the_database_with_a_foreign_file() {
    let h = hostile_setup();
    assert_rejected(
        &h,
        &journal_text(&["rename\tx.pdf\tgames.si4".into()]),
        "x.pdf -> games.si4",
    );
    assert!(h.base.parent().unwrap().join("x.pdf").exists());
}

#[test]
fn journal_cannot_rename_onto_a_foreign_name() {
    let h = hostile_setup();
    assert_rejected(
        &h,
        &journal_text(&["rename\tgames.sn4.tmp\toutro.pgn".into()]),
        "to foreign",
    );
}

#[rstest]
#[case::dotdot("remove\t..")]
#[case::dotdot_path("remove\t../victim.txt")]
#[case::dotdot_prefixed("remove\tgames/../../victim.txt")]
#[case::dot("remove\t.")]
#[case::empty("remove\t")]
#[case::colon("remove\tgames.si4:stream")]
#[case::colon_drive("remove\tC:games.sn4")]
#[case::bad_extension_exe("remove\tgames.exe")]
#[case::bad_extension_pgn("remove\tgames.pgn")]
#[case::no_extension("remove\tgames")]
#[case::other_stem("remove\tgamesX.sn4")]
#[case::other_stem_dot("remove\tother.sn4")]
#[case::separator("remove\tsub/games.sn4")]
#[case::rename_from_foreign("rename\tvictim.txt\tgames.sn4")]
#[case::rename_to_dotdot("rename\tgames.sn4.tmp\t../victim.txt")]
#[case::unknown_verb("copy\tgames.sn4\tgames.sg4")]
#[case::missing_field("rename\tgames.sn4.tmp")]
#[case::extra_field("remove\tgames.sn4.tmp\tgames.sn4")]
fn journal_with_a_hostile_step_is_corrupt(#[case] step: &str) {
    let h = hostile_setup();
    assert_rejected(&h, &journal_text(&[step.to_owned()]), step);
}

#[test]
fn journal_with_an_absolute_path_is_corrupt() {
    let h = hostile_setup();
    let abs = h.dir.path().join("victim.txt");
    let step = format!("remove\t{}", abs.display());
    assert_rejected(&h, &journal_text(&[step]), "absolute remove");
    let step = format!("rename\tgames.sn4.tmp\t{}", abs.display());
    assert_rejected(&h, &journal_text(&[step]), "absolute rename");
}

#[test]
fn journal_with_a_valid_step_then_an_invalid_one_executes_nothing() {
    let h = hostile_setup();
    let tmp_sn4 = ext(&h.base, "sn4.tmp");
    fs::write(&tmp_sn4, b"replacement names").unwrap();
    let steps = [
        "rename\tgames.sn4.tmp\tgames.sn4".to_owned(),
        "remove\tgames.sg4.tmp".to_owned(),
        "remove\toutro.pgn".to_owned(),
    ];
    let sn4 = fs::read(ext(&h.base, "sn4")).unwrap();
    assert_rejected(&h, &journal_text(&steps), "valid then invalid");
    assert!(tmp_sn4.exists(), "the valid first step was not executed");
    assert_eq!(fs::read(ext(&h.base, "sn4")).unwrap(), sn4);
}

#[rstest]
#[case::no_end("mychess-txn 1\nop x\nremove\tgames.sn4.tmp\n")]
#[case::truncated_mid_line("mychess-txn 1\nop x\nrename\tgames.sn4.tmp\tgames.s")]
#[case::empty("")]
#[case::only_magic("mychess-txn 1\n")]
#[case::wrong_version("mychess-txn 2\nop x\nend\n")]
#[case::wrong_magic("other 1\nop x\nend\n")]
fn truncated_or_unknown_journal_is_corrupt_and_kept(#[case] text: &str) {
    let h = hostile_setup();
    assert_rejected(&h, text, text);
}

#[test]
fn journal_that_is_not_utf8_is_corrupt() {
    let h = hostile_setup();
    let journal = ext(&h.base, "txn");
    fs::write(&journal, b"mychess-txn 1\nop x\nremove\t\xFF\xFE\nend\n").unwrap();
    let err = recover(&h.base).unwrap_err();
    assert!(matches!(err, Si4Error::CorruptJournal { .. }), "{err:?}");
    assert!(journal.exists());
}

#[test]
fn journal_whose_from_and_to_are_both_missing_is_corrupt() {
    let h = hostile_setup();
    fs::remove_file(ext(&h.base, "sg4")).unwrap();
    // neither games.sg4.tmp nor games.sg4 exist: not "already done", just wrong
    let text = journal_text(&["rename\tgames.sg4.tmp\tgames.sg4".into()]);
    let journal = ext(&h.base, "txn");
    fs::write(&journal, text).unwrap();
    let err = recover(&h.base).unwrap_err();
    assert!(matches!(err, Si4Error::CorruptJournal { .. }), "{err:?}");
    assert!(journal.exists());
}

#[test]
fn journal_step_already_done_is_skipped_when_the_destination_exists() {
    let h = hostile_setup();
    let text = journal_text(&[
        "rename\tgames.sn4.tmp\tgames.sn4".into(),
        "remove\tgames.sg4.tmp".into(),
    ]);
    let journal = ext(&h.base, "txn");
    fs::write(&journal, text).unwrap();
    let sn4 = fs::read(ext(&h.base, "sn4")).unwrap();

    let r = recover(&h.base).unwrap();

    assert!(r.journal_completed);
    assert_eq!(r.steps_applied, 0);
    assert!(!journal.exists());
    assert_eq!(fs::read(ext(&h.base, "sn4")).unwrap(), sn4);
}

#[test]
fn well_formed_journal_rolls_forward_its_steps() {
    let h = hostile_setup();
    let new_names = mychess_db::si4::NameBuilder::new().to_bytes();
    fs::write(ext(&h.base, "sn4.tmp"), &new_names).unwrap();
    fs::write(ext(&h.base, "sg4.tmp"), b"junk").unwrap();
    let text = journal_text(&[
        "rename\tgames.sn4.tmp\tgames.sn4".into(),
        "remove\tgames.sg4.tmp".into(),
    ]);
    fs::write(ext(&h.base, "txn"), text).unwrap();

    let r = recover(&h.base).unwrap();

    assert_eq!((r.journal_completed, r.steps_applied), (true, 2));
    assert_eq!(fs::read(ext(&h.base, "sn4")).unwrap(), new_names);
    assert!(!ext(&h.base, "sg4.tmp").exists());
    assert!(!ext(&h.base, "txn").exists());
    assert_eq!(
        fs::read(h.base.parent().unwrap().join("outro.pgn")).unwrap(),
        OUTSIDE
    );
}
