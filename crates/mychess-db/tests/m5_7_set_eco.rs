//! Contract tests of `Si4Writer::set_eco` and `set_eco_batch` (M5-7a, D1187,
//! D1200): only bytes 23-24 (big-endian) of the 47-byte entry change, in place,
//! idempotently, and the result survives a reopen.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

#[path = "common/m5_7_support.rs"]
mod support;

use mychess_core::{Eco, GameResult};
use mychess_db::si4::{FlagOp, GameFlags, Si4Writer};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{code, eco, eco_offset, index_bytes, open, spec, write_db};

fn sample(dir: &std::path::Path) -> std::path::PathBuf {
    write_db(
        dir,
        &[
            spec(&["e4", "e5"]).result(GameResult::White),
            spec(&["d4", "d5"]).eco("D00").result(GameResult::Draw),
            spec(&["c4"]).result(GameResult::Black).deleted(),
        ],
    )
}

fn changed_offsets(a: &[u8], b: &[u8]) -> Vec<usize> {
    assert_eq!(a.len(), b.len(), "file length changed");
    (0..a.len()).filter(|&i| a[i] != b[i]).collect()
}

#[test]
fn set_eco_changes_only_the_two_eco_bytes_big_endian() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let before = index_bytes(&base);
    let sg4_before = std::fs::read(base.with_extension("sg4")).unwrap();
    let sn4_before = std::fs::read(base.with_extension("sn4")).unwrap();
    // Act: code 258 = 0x0102 so the two bytes differ.
    let value = Eco::from_scid_code(0x0102).unwrap();
    let mut writer = Si4Writer::open(&base).unwrap();
    assert!(writer.set_eco(1, Some(value)).unwrap());
    drop(writer);
    // Assert
    let after = index_bytes(&base);
    assert_eq!(
        changed_offsets(&before, &after),
        [eco_offset(1), eco_offset(1) + 1]
    );
    assert_eq!(&after[eco_offset(1)..eco_offset(1) + 2], &[0x01, 0x02]);
    assert_eq!(
        sg4_before,
        std::fs::read(base.with_extension("sg4")).unwrap()
    );
    assert_eq!(
        sn4_before,
        std::fs::read(base.with_extension("sn4")).unwrap()
    );
}

#[test]
fn set_eco_on_the_last_game_and_extreme_codes() {
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let mut writer = Si4Writer::open(&base).unwrap();
    writer.set_eco(3, Some(eco("E99z4"))).unwrap();
    writer.set_eco(1, Some(eco("A00"))).unwrap();
    drop(writer);
    let bytes = index_bytes(&base);
    assert_eq!(
        &bytes[eco_offset(3)..eco_offset(3) + 2],
        &65_500u16.to_be_bytes()
    );
    assert_eq!(
        &bytes[eco_offset(1)..eco_offset(1) + 2],
        &1u16.to_be_bytes()
    );
    // The file did not grow.
    assert_eq!(bytes.len(), 182 + 47 * 3);
}

#[test]
fn set_eco_is_idempotent_and_reports_whether_it_changed() {
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let mut writer = Si4Writer::open(&base).unwrap();
    assert!(writer.set_eco(1, Some(eco("B90a"))).unwrap());
    let once = std::fs::read(base.with_extension("si4")).unwrap();
    assert!(!writer.set_eco(1, Some(eco("B90a"))).unwrap());
    // Setting "none" on a game that has none is also not a change.
    assert!(!writer.set_eco(1 + 2, None).unwrap());
    drop(writer);
    assert_eq!(once, index_bytes(&base));
}

#[test]
fn set_eco_none_clears_the_code_and_leaves_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    // Game 2 starts with D00.
    assert!(writer.set_eco(2, None).unwrap());
    drop(writer);
    let after = index_bytes(&base);
    assert_eq!(&after[eco_offset(2)..eco_offset(2) + 2], &[0, 0]);
    let diff = changed_offsets(&before, &after);
    assert!(
        diff.iter()
            .all(|&i| i == eco_offset(2) || i == eco_offset(2) + 1)
    );
    assert_eq!(open(&base).entry(2).unwrap().eco(), None);
}

#[rstest]
#[case::zero(0)]
#[case::one_past_the_end(4)]
#[case::far(1_000_000)]
#[case::max(u32::MAX)]
fn set_eco_rejects_game_numbers_out_of_range_without_touching_the_file(#[case] number: u32) {
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    assert!(writer.set_eco(number, Some(eco("C20"))).is_err());
    assert!(writer.set_eco_batch(&[(number, Some(eco("C20")))]).is_err());
    drop(writer);
    assert_eq!(before, index_bytes(&base));
}

#[test]
fn set_eco_round_trips_through_reopen_and_keeps_other_fields_and_game() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let db = open(&base);
    let entries_before: Vec<_> = (1..=3).map(|n| db.entry(n).unwrap()).collect();
    let games_before: Vec<_> = (1..=3).map(|n| open(&base).game(n).unwrap()).collect();
    drop(db);
    // Act
    let mut writer = Si4Writer::open(&base).unwrap();
    writer.set_eco(1, Some(eco("C20b"))).unwrap();
    writer.set_eco(3, Some(eco("A00a"))).unwrap();
    drop(writer);
    // Assert
    let db = open(&base);
    for n in 1..=3u32 {
        let mut expected = entries_before[usize::try_from(n).unwrap() - 1].clone();
        match n {
            1 => expected.eco_code = code("C20b"),
            3 => expected.eco_code = code("A00a"),
            _ => {}
        }
        assert_eq!(db.entry(n).unwrap(), expected, "entry {n}");
    }
    assert_eq!(db.entry(1).unwrap().eco(), Some(eco("C20b")));
    assert!(db.entry(3).unwrap().flags.contains(GameFlags::DELETE));
    let mut db = db;
    for n in 1..=3u32 {
        let game = db.game(n).unwrap();
        let before = &games_before[usize::try_from(n).unwrap() - 1];
        let moves = |g: &mychess_core::Game| -> Vec<_> {
            g.mainline().map(|id| g.mv(id).unwrap()).collect()
        };
        assert_eq!(moves(&game), moves(before), "game {n}");
    }
}

#[test]
fn set_eco_preserves_flags_set_by_set_flag_before_and_after() {
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let mut writer = Si4Writer::open(&base).unwrap();
    writer.set_flag(1, GameFlags::USER, FlagOp::Set).unwrap();
    writer.set_eco(1, Some(eco("B01"))).unwrap();
    writer.set_flag(1, GameFlags::DELETE, FlagOp::Set).unwrap();
    drop(writer);
    let entry = open(&base).entry(1).unwrap();
    assert!(entry.flags.contains(GameFlags::USER | GameFlags::DELETE));
    assert_eq!(entry.eco(), Some(eco("B01")));
}

// ------------------------------------------------------------ batch

#[test]
fn batch_writes_many_entries_and_counts_the_changed_ones() {
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    let changed = writer
        .set_eco_batch(&[
            (1, Some(eco("C20"))),
            (2, Some(eco("D00"))), // already D00: not a change
            (3, Some(eco("A45"))),
        ])
        .unwrap();
    drop(writer);
    assert_eq!(changed, 2);
    let after = index_bytes(&base);
    let allowed = [
        eco_offset(1),
        eco_offset(1) + 1,
        eco_offset(3),
        eco_offset(3) + 1,
    ];
    assert!(
        changed_offsets(&before, &after)
            .iter()
            .all(|i| allowed.contains(i))
    );
    assert_eq!(
        support::codes(&base),
        [code("C20"), code("D00"), code("A45")]
    );
}

#[test]
fn batch_with_a_repeated_game_keeps_the_last_value() {
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let mut writer = Si4Writer::open(&base).unwrap();
    writer
        .set_eco_batch(&[
            (1, Some(eco("C20"))),
            (1, Some(eco("C21"))),
            (1, None),
            (1, Some(eco("B01"))),
        ])
        .unwrap();
    drop(writer);
    assert_eq!(support::codes(&base)[0], code("B01"));
}

#[test]
fn empty_batch_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let before = index_bytes(&base);
    let mut writer = Si4Writer::open(&base).unwrap();
    assert_eq!(writer.set_eco_batch(&[]).unwrap(), 0);
    drop(writer);
    assert_eq!(before, index_bytes(&base));
}

#[test]
fn batch_with_an_invalid_game_fails_and_keeps_what_was_written_before() {
    // The spec (D1187/D1200) requires an error for an invalid index; the
    // writer documents that earlier entries stay (each is complete alone).
    let dir = tempfile::tempdir().unwrap();
    let base = sample(dir.path());
    let mut writer = Si4Writer::open(&base).unwrap();
    let result = writer.set_eco_batch(&[
        (1, Some(eco("C20"))),
        (99, Some(eco("C21"))),
        (3, Some(eco("A45"))),
    ]);
    drop(writer);
    assert!(result.is_err());
    let codes = support::codes(&base);
    assert_eq!(codes[0], code("C20"));
    assert_eq!(codes[2], 0, "games after the failure are not written");
    // The file is still a valid database.
    assert_eq!(open(&base).len(), 3);
}

#[test]
fn batch_equals_one_by_one_set_eco() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = sample(dir_a.path());
    let b = sample(dir_b.path());
    let changes = [
        (1, Some(eco("A00a"))),
        (2, None),
        (3, Some(eco("E99z4"))),
        (1, Some(eco("B90a2"))),
    ];
    let mut wa = Si4Writer::open(&a).unwrap();
    wa.set_eco_batch(&changes).unwrap();
    drop(wa);
    let mut wb = Si4Writer::open(&b).unwrap();
    for (n, e) in changes {
        wb.set_eco(n, e).unwrap();
    }
    drop(wb);
    assert_eq!(index_bytes(&a), index_bytes(&b));
}

#[test]
fn set_eco_on_a_database_without_games_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let base = write_db(dir.path(), &[]);
    let mut writer = Si4Writer::open(&base).unwrap();
    assert!(writer.set_eco(1, Some(eco("C20"))).is_err());
}
