//! M7-1a1, criterion 4: `Si4Writer::set_flags_batch`
//! (`docs/specs/m7-1-manutencao.md`).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

#[path = "common/m7_1a1_support.rs"]
mod support;

use mychess_db::si4::{FlagOp, GameFlags, Si4Error, Si4Writer};
use proptest::prelude::*;
use support::{build, entries_of, index_bytes, open, s, twin_games};

fn five() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &twin_games());
    (dir, base)
}

#[test]
fn marks_three_games_and_leaves_every_other_entry_byte_identical() {
    let (_dir, base) = five();
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    let changed = writer
        .set_flags_batch(&[1, 3, 5], GameFlags::TACTICS, FlagOp::Set)
        .unwrap();
    assert_eq!(changed, 3);
    let after = index_bytes(&base);
    assert_eq!(before.len(), after.len());
    assert_eq!(before[..182], after[..182], "header untouched");
    let (b, a) = (entries_of(&before), entries_of(&after));
    for n in 0..5 {
        if [0, 2, 4].contains(&n) {
            assert_ne!(b[n], a[n]);
            // Only the high byte of the flag word (byte 7: T is bit 10) differs.
            let diff: Vec<usize> = (0..47).filter(|&i| b[n][i] != a[n][i]).collect();
            assert_eq!(diff, [7], "game {}", n + 1);
        } else {
            assert_eq!(b[n], a[n], "game {}", n + 1);
        }
    }
    let db = open(&base);
    assert!(db.entry(1).unwrap().flags.contains(GameFlags::TACTICS));
    assert!(!db.entry(2).unwrap().flags.contains(GameFlags::TACTICS));
}

#[test]
fn a_second_writer_is_refused_while_the_first_one_edits() {
    let (_dir, base) = five();
    let mut first = Si4Writer::open(&base).unwrap();
    assert!(matches!(
        Si4Writer::open(&base),
        Err(Si4Error::AlreadyOpen { .. })
    ));
    // The refusal does not disturb the writer that holds the lock.
    assert_eq!(
        first
            .set_flags_batch(&[1], GameFlags::TACTICS, FlagOp::Set)
            .unwrap(),
        1
    );
}

#[test]
fn running_it_again_changes_nothing() {
    let (_dir, base) = five();
    let mut writer = Si4Writer::open(&base).unwrap();
    writer
        .set_flags_batch(&[1, 3, 5], GameFlags::TACTICS, FlagOp::Set)
        .unwrap();
    let once = index_bytes(&base);
    let again = writer
        .set_flags_batch(&[1, 3, 5], GameFlags::TACTICS, FlagOp::Set)
        .unwrap();
    assert_eq!(again, 0);
    assert_eq!(index_bytes(&base), once);
}

#[test]
fn derived_flags_are_refused_without_writing() {
    let (_dir, base) = five();
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    for flag in [
        GameFlags::START,
        GameFlags::PROMOTION,
        GameFlags::UNDER_PROMOTION,
        GameFlags::empty(),
        GameFlags::TACTICS | GameFlags::START,
    ] {
        let error = writer.set_flags_batch(&[1], flag, FlagOp::Set).unwrap_err();
        assert!(matches!(error, Si4Error::NotEditableFlag), "{flag:?}");
    }
    assert_eq!(index_bytes(&base), before);
}

#[test]
fn custom_flags_use_length_high_and_keep_its_top_bits() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[s(&["e2e4"]), s(&["e2e4"])]);
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    assert_eq!(
        writer
            .set_flags_batch(&[2], GameFlags::CUSTOM6, FlagOp::Set)
            .unwrap(),
        1
    );
    let after = index_bytes(&base);
    let (b, a) = (entries_of(&before), entries_of(&after));
    assert_eq!(b[0], a[0]);
    assert_eq!(a[1][6] & 0xC0, b[1][6] & 0xC0);
    assert_eq!(a[1][6] & 0x3F, 0x20);
    assert!(
        open(&base)
            .entry(2)
            .unwrap()
            .flags
            .contains(GameFlags::CUSTOM6)
    );
}

#[test]
fn delete_is_a_flag_like_the_others_and_independent_of_marks() {
    let (_dir, base) = five();
    let mut writer = Si4Writer::open(&base).unwrap();
    writer
        .set_flags_batch(&[2], GameFlags::DELETE, FlagOp::Set)
        .unwrap();
    writer
        .set_flags_batch(&[2], GameFlags::WHITE_OPENING, FlagOp::Set)
        .unwrap();
    writer
        .set_flags_batch(&[2], GameFlags::WHITE_OPENING, FlagOp::Clear)
        .unwrap();
    let flags = open(&base).entry(2).unwrap().flags;
    assert!(
        flags.contains(GameFlags::DELETE),
        "marking does not undelete"
    );
    assert!(!flags.contains(GameFlags::WHITE_OPENING));
}

#[test]
fn empty_list_zero_and_duplicates_are_harmless_and_out_of_range_fails_early() {
    let (_dir, base) = five();
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    assert_eq!(
        writer
            .set_flags_batch(&[], GameFlags::USER, FlagOp::Set)
            .unwrap(),
        0
    );
    assert_eq!(
        writer
            .set_flags_batch(&[0], GameFlags::USER, FlagOp::Set)
            .unwrap(),
        0
    );
    // Toggle with a repeated number counts the game once.
    assert_eq!(
        writer
            .set_flags_batch(&[2, 2, 2], GameFlags::USER, FlagOp::Toggle)
            .unwrap(),
        1
    );
    assert!(
        open(&base)
            .entry(2)
            .unwrap()
            .flags
            .contains(GameFlags::USER)
    );
    // A bad number anywhere stops before the first write.
    let error = writer
        .set_flags_batch(&[1, 6], GameFlags::KINGSIDE, FlagOp::Set)
        .unwrap_err();
    assert!(matches!(
        error,
        Si4Error::InvalidGameNumber { number: 6, len: 5 }
    ));
    assert!(
        !open(&base)
            .entry(1)
            .unwrap()
            .flags
            .contains(GameFlags::KINGSIDE)
    );
    drop(writer);
    assert_ne!(index_bytes(&base), before);
}

#[test]
fn the_empty_database_accepts_an_empty_batch() {
    let dir = tempfile::tempdir().unwrap();
    let base = build(dir.path(), &[]);
    let mut writer = Si4Writer::open(&base).unwrap();
    assert_eq!(
        writer
            .set_flags_batch(&[], GameFlags::USER, FlagOp::Set)
            .unwrap(),
        0
    );
    assert!(
        writer
            .set_flags_batch(&[1], GameFlags::USER, FlagOp::Set)
            .is_err()
    );
}

fn editable_flag() -> impl Strategy<Value = GameFlags> {
    (3u32..22).prop_map(|bit| GameFlags::from_bits_retain(1 << bit))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn set_is_idempotent_and_clear_restores_a_flagless_database(
        flag in editable_flag(),
        picks in proptest::collection::vec(1u32..=8, 0..12),
    ) {
        let dir = tempfile::tempdir().unwrap();
        let games: Vec<_> = (0..8).map(|_| s(&["e2e4"])).collect();
        let base = build(dir.path(), &games);
        let original = index_bytes(&base);
        let mut writer = Si4Writer::open(&base).unwrap();
        let mut distinct = picks.clone();
        distinct.sort_unstable();
        distinct.dedup();
        let first = writer.set_flags_batch(&picks, flag, FlagOp::Set).unwrap();
        prop_assert_eq!(first as usize, distinct.len());
        let after_set = index_bytes(&base);
        prop_assert_eq!(writer.set_flags_batch(&picks, flag, FlagOp::Set).unwrap(), 0);
        prop_assert_eq!(index_bytes(&base), after_set);
        prop_assert_eq!(writer.set_flags_batch(&picks, flag, FlagOp::Clear).unwrap() as usize, distinct.len());
        prop_assert!(index_bytes(&base) == original);
        // Toggle twice is the identity too.
        writer.set_flags_batch(&picks, flag, FlagOp::Toggle).unwrap();
        writer.set_flags_batch(&picks, flag, FlagOp::Toggle).unwrap();
        prop_assert!(index_bytes(&base) == original);
    }
}
