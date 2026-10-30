//! M7-1a2, criterion 15: `Si4Writer::strip_games`
//! (`docs/specs/m7-1-manutencao.md`, decisions D3520 and D3531, rule 39).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

#[path = "common/m7_1a2_support.rs"]
mod support;

use mychess_db::si4::{FailPoint, Si4Error, Si4Writer, StripWhat};
use mychess_db::{CancelToken, NoProgress};
use proptest::prelude::*;
use support::{build_games, entries_of, open, plain, record, rich, shape, snapshot};

fn strip(base: &std::path::Path, games: &[u32], what: StripWhat) -> mychess_db::si4::StripReport {
    let mut writer = Si4Writer::open(base).unwrap();
    writer
        .strip_games(games, what, &CancelToken::new(), &mut NoProgress)
        .unwrap()
}

fn all(n: u32) -> Vec<u32> {
    (1..=n).collect()
}

#[test]
fn comments_go_and_moves_nags_and_the_rest_stay() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[plain(), rich(), plain(), rich()]);
    let before = snapshot(&base);
    let (mut db, mut shapes_before) = (open(&base), vec![]);
    for n in 1..=4 {
        shapes_before.push(shape(&mut db, n));
    }
    let records: Vec<Vec<u8>> = (1..=4).map(|n| record(&mut db, n)).collect();
    drop(db);
    assert_eq!(shapes_before[1].comments, 3);

    let report = strip(&base, &all(4), StripWhat::Comments);
    assert_eq!(
        (report.examined, report.changed, report.unchanged),
        (4, 2, 2)
    );
    assert!(!report.interrupted);
    assert!(report.bytes_appended > 0);

    let after = snapshot(&base);
    let (b, a) = (entries_of(&before[0]), entries_of(&after[0]));
    let mut db = open(&base);
    for n in [1usize, 3] {
        // Not rewritten: same entry bytes and same record bytes.
        assert_eq!(b[n - 1], a[n - 1], "entry {n}");
        assert_eq!(record(&mut db, u32::try_from(n).unwrap()), records[n - 1]);
    }
    for n in [2u32, 4] {
        let s = shape(&mut db, n);
        let old = shapes_before[usize::try_from(n - 1).unwrap()];
        assert_eq!(s.comments, 0);
        assert_eq!(s.nags, old.nags, "NAGs intact");
        assert_eq!(s.variations, old.variations, "variations intact");
        assert_eq!(s.mainline_moves, old.mainline_moves);
        let entry = db.entry(n).unwrap();
        assert_eq!(entry.comment_count, 0);
        // The decoded game has no comment text left, in particular no command.
        let game = db.game(n).unwrap();
        assert!(
            game.mainline()
                .all(|id| game.comment(id).unwrap().is_none())
        );
    }
    // Names and the name file are untouched.
    assert_eq!(before[1], after[1]);
    // Header untouched.
    assert_eq!(before[0][..182], after[0][..182]);
}

#[test]
fn variations_go_at_every_depth_and_comments_of_the_main_line_stay() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[rich(), plain()]);
    let before_shape = shape(&mut open(&base), 1);
    assert_eq!(before_shape.variations, 2);
    let report = strip(&base, &[1, 2], StripWhat::Variations);
    assert_eq!((report.changed, report.unchanged), (1, 1));
    let mut db = open(&base);
    let s = shape(&mut db, 1);
    assert_eq!(s.variations, 0);
    assert_eq!(
        s.comments, 2,
        "the comment inside the variation went with it"
    );
    assert_eq!(s.mainline_moves, before_shape.mainline_moves);
    let entry = db.entry(1).unwrap();
    assert_eq!(entry.variation_count, 0);
    assert_eq!(entry.comment_count, 2);
    assert_eq!(db.game(1).unwrap().mainline().count(), 4);
}

#[test]
fn both_removes_comments_and_variations() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[rich()]);
    let report = strip(&base, &[1], StripWhat::Both);
    assert_eq!(report.changed, 1);
    let mut db = open(&base);
    let s = shape(&mut db, 1);
    assert_eq!((s.comments, s.variations), (0, 0));
    assert_eq!(s.nags, 2);
    let entry = db.entry(1).unwrap();
    assert_eq!((entry.comment_count, entry.variation_count), (0, 0));
    assert_eq!(entry.nag_count, 2);
}

#[test]
fn nothing_to_strip_leaves_every_file_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[plain(), plain()]);
    let before = snapshot(&base);
    for what in [StripWhat::Comments, StripWhat::Variations, StripWhat::Both] {
        let report = strip(&base, &[1, 2], what);
        assert_eq!((report.changed, report.unchanged), (0, 2), "{what:?}");
        assert_eq!(report.bytes_appended, 0);
    }
    assert_eq!(snapshot(&base), before);
}

#[test]
fn the_tree_cache_is_removed_only_when_something_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[plain(), rich()]);
    let cache = base.with_extension("stc");
    std::fs::write(&cache, b"cache").unwrap();
    strip(&base, &[1], StripWhat::Both);
    assert!(cache.exists());
    strip(&base, &[2], StripWhat::Both);
    assert!(!cache.exists());
}

#[test]
fn stripping_twice_changes_nothing_the_second_time() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[rich(), plain(), rich()]);
    strip(&base, &all(3), StripWhat::Both);
    let once = snapshot(&base);
    let again = strip(&base, &all(3), StripWhat::Both);
    assert_eq!(again.changed, 0);
    assert_eq!(snapshot(&base), once);
}

#[test]
fn empty_database_empty_list_and_zero_are_fine() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[]);
    let before = snapshot(&base);
    let report = strip(&base, &[], StripWhat::Both);
    assert_eq!((report.examined, report.changed), (0, 0));
    let report = strip(&base, &[0, 0], StripWhat::Both);
    assert_eq!(report.examined, 0);
    assert_eq!(snapshot(&base), before);
}

#[test]
fn a_game_number_out_of_range_fails_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[rich(), rich()]);
    let before = snapshot(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    let error = writer
        .strip_games(
            &[1, 3],
            StripWhat::Both,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        Si4Error::InvalidGameNumber { number: 3, len: 2 }
    ));
    drop(writer);
    assert_eq!(snapshot(&base), before);
}

#[test]
fn only_the_listed_games_are_touched_and_repeats_count_once() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[rich(), rich(), rich()]);
    let before = snapshot(&base);
    let report = strip(&base, &[2, 2, 2], StripWhat::Comments);
    assert_eq!((report.examined, report.changed), (1, 1));
    let after = snapshot(&base);
    let (b, a) = (entries_of(&before[0]), entries_of(&after[0]));
    assert_eq!(b[0], a[0]);
    assert_ne!(b[1], a[1]);
    assert_eq!(b[2], a[2]);
}

#[test]
fn a_cancelled_run_leaves_the_database_exactly_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<_> = (0..700).map(|_| rich()).collect();
    let base = build_games(dir.path(), &games);
    let before = snapshot(&base);

    // Already cancelled.
    let token = CancelToken::new();
    token.cancel();
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .strip_games(&all(700), StripWhat::Both, &token, &mut NoProgress)
        .unwrap();
    assert!(report.interrupted);
    assert_eq!(report.changed, 0);
    drop(writer);
    assert_eq!(snapshot(&base), before);

    // Cancelled from the progress callback, after hundreds of games were
    // queued: the appended records are cut away.
    let token = CancelToken::new();
    let trigger = token.clone();
    // By the number of games read, not by the call: the ticker also fires
    // on a timer, so the number of calls is not deterministic.
    let mut sink = move |done: u64, _total: u64| {
        if done >= 256 {
            trigger.cancel();
        }
    };
    let mut writer = Si4Writer::open(&base).unwrap();
    let report = writer
        .strip_games(&all(700), StripWhat::Both, &token, &mut sink)
        .unwrap();
    assert!(report.interrupted);
    assert_eq!(report.changed, 0);
    assert!(report.examined > 256, "it was cancelled in the middle");
    drop(writer);
    assert_eq!(snapshot(&base), before, "nothing partial");
    // The database still works.
    assert_eq!(open(&base).len(), 700);
}

#[test]
fn progress_never_goes_back_and_ends_at_the_total() {
    let dir = tempfile::tempdir().unwrap();
    let games: Vec<_> = (0..600).map(|_| rich()).collect();
    let base = build_games(dir.path(), &games);
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut sink = |done: u64, total: u64| seen.push((done, total));
    let mut writer = Si4Writer::open(&base).unwrap();
    writer
        .strip_games(&all(600), StripWhat::Both, &CancelToken::new(), &mut sink)
        .unwrap();
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
    assert_eq!(seen.last(), Some(&(600, 600)));
    assert!(seen.iter().all(|&(d, t)| d <= t && t == 600));
}

#[test]
fn injected_failures_before_the_entries_leave_the_database_as_it_was() {
    for label in ["rewrite:appended", "rewrite:entries"] {
        let dir = tempfile::tempdir().unwrap();
        let base = build_games(dir.path(), &[rich(), plain(), rich()]);
        let before = snapshot(&base);
        let mut writer = Si4Writer::open(&base).unwrap();
        writer.set_fail_point(FailPoint::at_label(label));
        let error = writer
            .strip_games(
                &all(3),
                StripWhat::Both,
                &CancelToken::new(),
                &mut NoProgress,
            )
            .unwrap_err();
        assert!(matches!(error, Si4Error::InjectedFailure { .. }), "{label}");
        // The writer is still usable and the second try works.
        let report = writer
            .strip_games(
                &all(3),
                StripWhat::Both,
                &CancelToken::new(),
                &mut NoProgress,
            )
            .unwrap();
        assert_eq!(report.changed, 2, "{label}");
        drop(writer);
        let after = snapshot(&base);
        assert_ne!(after, before);
        let mut db = open(&base);
        for n in 1..=3 {
            assert!(db.game(n).is_ok());
        }
        // Without the second try the files would be the old ones.
        let dir2 = tempfile::tempdir().unwrap();
        let base2 = build_games(dir2.path(), &[rich(), plain(), rich()]);
        let mut w2 = Si4Writer::open(&base2).unwrap();
        w2.set_fail_point(FailPoint::at_label(label));
        w2.strip_games(
            &all(3),
            StripWhat::Both,
            &CancelToken::new(),
            &mut NoProgress,
        )
        .unwrap_err();
        drop(w2);
        assert_eq!(snapshot(&base2), before, "{label}: files as before");
        assert!(!base2.with_extension("txn").exists());
    }
}

#[test]
fn a_damaged_record_is_counted_and_the_others_still_change() {
    let dir = tempfile::tempdir().unwrap();
    let base = build_games(dir.path(), &[rich(), rich(), rich()]);
    // Damage the record of game 2: the comment strings end the record, so
    // cutting its length in the entry makes the decoding fail.
    let db = open(&base);
    let entry = db.entry(2).unwrap();
    drop(db);
    let mut index = std::fs::read(base.with_extension("si4")).unwrap();
    let at = 182 + 47; // entry 2
    let short = 3u16;
    index[at + 4..at + 6].copy_from_slice(&short.to_be_bytes());
    index[at + 6] &= 0x7F;
    std::fs::write(base.with_extension("si4"), &index).unwrap();
    assert!(entry.length > 3);
    let report = strip(&base, &all(3), StripWhat::Comments);
    assert_eq!(report.unreadable, 1);
    assert_eq!(report.changed, 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn strip_is_idempotent_and_keeps_moves_and_nags(kinds in prop::collection::vec(0u8..3, 0..8), what in 0u8..3) {
        let dir = tempfile::tempdir().unwrap();
        let games: Vec<_> = kinds.iter().map(|k| if *k == 0 { plain() } else { rich() }).collect();
        let base = build_games(dir.path(), &games);
        let what = [StripWhat::Comments, StripWhat::Variations, StripWhat::Both][usize::from(what)];
        let count = u32::try_from(games.len()).unwrap();
        let mut db = open(&base);
        let before: Vec<_> = (1..=count).map(|n| shape(&mut db, n)).collect();
        drop(db);
        strip(&base, &all(count), what);
        let once = snapshot(&base);
        let again = strip(&base, &all(count), what);
        prop_assert_eq!(again.changed, 0);
        prop_assert_eq!(snapshot(&base), once);
        let mut db = open(&base);
        for (i, old) in before.iter().enumerate() {
            let n = u32::try_from(i + 1).unwrap();
            let s = shape(&mut db, n);
            prop_assert_eq!(s.nags, old.nags);
            prop_assert_eq!(s.mainline_moves, old.mainline_moves);
            if matches!(what, StripWhat::Comments | StripWhat::Both) {
                prop_assert_eq!(s.comments, 0);
            }
            if matches!(what, StripWhat::Variations | StripWhat::Both) {
                prop_assert_eq!(s.variations, 0);
            }
        }
    }
}
