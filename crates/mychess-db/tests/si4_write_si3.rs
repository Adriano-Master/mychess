//! Contract tests of the `.si3` upgrade (spec M3-2b: rules 45-47, decision
//! D317). The `.si3` fixtures are built byte by byte here.
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::assertions_on_result_states,
    clippy::assert_is_empty
)]

#[path = "common/si4_write_support.rs"]
mod support;

use mychess_db::si4::si3::{convert_index, upgrade};
use mychess_db::si4::{GameFlags, HEADER_LEN, Si4Database, Si4Error, Si4Writer};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{ext, named, read, tmp};

fn si3_header(version: u16, base_type: u32, games: u32, autoload: u32, desc: &[u8]) -> Vec<u8> {
    let mut h = vec![0u8; 128];
    h[..8].copy_from_slice(b"Scid.si\0");
    h[8..10].copy_from_slice(&version.to_be_bytes());
    h[10..14].copy_from_slice(&base_type.to_be_bytes());
    h[14..17].copy_from_slice(&games.to_be_bytes()[1..]);
    h[17..20].copy_from_slice(&autoload.to_be_bytes()[1..]);
    h[20..20 + desc.len()].copy_from_slice(desc);
    h
}

fn si3_entry(seed: u8) -> Vec<u8> {
    (0..46u8)
        .map(|i| seed.wrapping_mul(31).wrapping_add(i))
        .collect()
}

fn si3(version: u16, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut v = si3_header(
        version,
        7,
        u32::try_from(entries.len()).unwrap(),
        3,
        b"old base",
    );
    for e in entries {
        v.extend_from_slice(e);
    }
    v
}

#[test]
fn convert_index_rewrites_header_and_inserts_a_zero_after_the_sixth_byte() {
    let entries = vec![si3_entry(1), si3_entry(2), si3_entry(3)];
    let input = si3(310, &entries);

    let out = convert_index(&input).unwrap();

    assert_eq!(out.len(), HEADER_LEN + 3 * 47);
    assert_eq!(&out[..8], b"Scid.si\0", "magic copied");
    assert_eq!(&out[8..10], &400u16.to_be_bytes(), "version 400");
    assert_eq!(
        &out[10..128],
        &input[10..128],
        "type, count, autoload and description raw"
    );
    assert!(
        out[128..182].iter().all(|&b| b == 0),
        "54 bytes of flag descriptions are zero"
    );
    for (i, e) in entries.iter().enumerate() {
        let o = &out[HEADER_LEN + 47 * i..][..47];
        assert_eq!(&o[..6], &e[..6]);
        assert_eq!(o[6], 0, "Length_High");
        assert_eq!(&o[7..], &e[6..]);
    }
}

#[test]
fn convert_index_ignores_bytes_after_the_last_entry() {
    let mut input = si3(300, &[si3_entry(5)]);
    let clean = convert_index(&input).unwrap();
    input.extend_from_slice(&[0xEE; 20]);
    assert_eq!(convert_index(&input).unwrap(), clean);
}

#[test]
fn convert_index_of_zero_games_gives_just_the_header() {
    let out = convert_index(&si3(399, &[])).unwrap();
    assert_eq!(out.len(), HEADER_LEN);
}

#[test]
fn convert_index_rejects_a_bad_magic() {
    let mut input = si3(310, &[si3_entry(1)]);
    input[0] = b'X';
    assert!(matches!(
        convert_index(&input),
        Err(Si4Error::BadMagic { .. })
    ));
    input[0] = b'S';
    input[7] = 1; // the 8th magic byte must be 0
    assert!(matches!(
        convert_index(&input),
        Err(Si4Error::BadMagic { .. })
    ));
}

#[rstest]
#[case(0)]
#[case(1)]
#[case(7)]
#[case(127)]
fn convert_index_rejects_a_truncated_header(#[case] len: usize) {
    let input = si3(310, &[]);
    assert!(matches!(
        convert_index(&input[..len]),
        Err(Si4Error::Truncated { .. })
    ));
}

#[rstest]
#[case(1)]
#[case(45)]
#[case(46)]
#[case(47)]
fn convert_index_rejects_missing_entry_bytes(#[case] cut: usize) {
    let entries = vec![si3_entry(1), si3_entry(2)];
    let input = si3(310, &entries);
    let r = convert_index(&input[..input.len() - cut]);
    assert!(matches!(r, Err(Si4Error::Truncated { .. })), "{r:?}");
}

#[test]
fn convert_index_declared_huge_count_is_an_error_not_an_allocation() {
    let input = si3_header(310, 0, 0xFF_FFFF, 2, b"");
    assert!(matches!(
        convert_index(&input),
        Err(Si4Error::Truncated { .. })
    ));
}

#[rstest]
#[case(0)]
#[case(299)]
fn convert_index_rejects_too_old_versions(#[case] v: u16) {
    assert!(matches!(
        convert_index(&si3(v, &[])),
        Err(Si4Error::OldVersion { found }) if found == v
    ));
}

#[rstest]
#[case(400)]
#[case(401)]
#[case(65_535)]
fn convert_index_rejects_version_400_and_newer(#[case] v: u16) {
    assert!(matches!(
        convert_index(&si3(v, &[])),
        Err(Si4Error::NewerVersion { found }) if found == v
    ));
}

#[rstest]
#[case(300)]
#[case(350)]
#[case(399)]
fn convert_index_accepts_versions_300_to_399(#[case] v: u16) {
    assert!(convert_index(&si3(v, &[si3_entry(1)])).is_ok());
}

proptest! {
    #[test]
    fn convert_index_preserves_every_entry_byte(
        entries in prop::collection::vec(prop::collection::vec(any::<u8>(), 46), 0..20),
        version in 300u16..400,
    ) {
        let out = convert_index(&si3(version, &entries)).unwrap();
        prop_assert_eq!(out.len(), HEADER_LEN + 47 * entries.len());
        for (i, e) in entries.iter().enumerate() {
            let o = &out[HEADER_LEN + 47 * i..][..47];
            let mut expected = e[..6].to_vec();
            expected.push(0);
            expected.extend_from_slice(&e[6..]);
            prop_assert_eq!(o, expected.as_slice());
        }
    }
}

// ---------------------------------------------------------------- upgrade

/// Builds an `.si3` database equivalent to a fresh SI4 one: the `.sn3` and
/// `.sg3` have the same format as `.sn4`/`.sg4`.
fn legacy(base: &std::path::Path) -> (Vec<u8>, usize) {
    let mut w = Si4Writer::create(base, 4).unwrap();
    w.set_description("legacy").unwrap();
    w.set_auto_load(2).unwrap();
    for i in 0..3 {
        w.add_game(
            &named(&format!("W{i}"), "B", "E", "S", &i.to_string()),
            GameFlags::empty(),
        )
        .unwrap();
    }
    drop(w);
    let si4 = read(base, "si4");
    let mut old = si4[..128].to_vec();
    old[8..10].copy_from_slice(&310u16.to_be_bytes());
    for chunk in si4[HEADER_LEN..].chunks(47) {
        old.extend_from_slice(&chunk[..6]);
        old.extend_from_slice(&chunk[7..]);
    }
    for (a, b) in [("si4", "si3"), ("sn4", "sn3"), ("sg4", "sg3")] {
        let data = if a == "si4" {
            old.clone()
        } else {
            read(base, a)
        };
        std::fs::write(ext(base, b), data).unwrap();
        std::fs::remove_file(ext(base, a)).unwrap();
    }
    (si4, 3)
}

#[test]
fn upgrade_creates_the_si4_renames_the_other_files_and_removes_the_si3() {
    let (_dir, base) = tmp();
    let (expected_si4, games) = legacy(&base);
    let sn3 = read(&base, "sn3");
    let sg3 = read(&base, "sg3");

    let report = upgrade(&base).unwrap();

    assert_eq!(report.games as usize, games);
    assert_eq!(
        read(&base, "si4"),
        expected_si4,
        "header 182 B and entries 47 B"
    );
    assert_eq!(read(&base, "sn4"), sn3, ".sn3 renamed");
    assert_eq!(read(&base, "sg4"), sg3, ".sg3 renamed");
    for gone in ["si3", "sn3", "sg3"] {
        assert!(!ext(&base, gone).exists(), ".{gone} must be gone");
    }
    assert!(support::leftovers(base.parent().unwrap()).is_empty());
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 3);
    assert_eq!(db.header().base_type(), 4);
    assert_eq!(db.header().description(), b"legacy");
    assert_eq!(db.game(2).unwrap().tags().white, "W1");
}

#[test]
fn upgrade_accepts_a_path_with_any_of_the_old_extensions() {
    let (_dir, base) = tmp();
    legacy(&base);
    upgrade(&ext(&base, "si3")).unwrap();
    assert!(ext(&base, "si4").exists());
}

#[rstest]
#[case("si4")]
#[case("sn4")]
#[case("sg4")]
fn upgrade_refuses_when_a_new_file_already_exists_and_changes_nothing(#[case] existing: &str) {
    let (_dir, base) = tmp();
    legacy(&base);
    std::fs::write(ext(&base, existing), b"mine").unwrap();
    let before: Vec<_> = ["si3", "sn3", "sg3"]
        .iter()
        .map(|e| read(&base, e))
        .collect();

    let err = upgrade(&base).unwrap_err();

    assert!(matches!(err, Si4Error::AlreadyExists { .. }), "{err:?}");
    assert_eq!(read(&base, existing), b"mine");
    let after: Vec<_> = ["si3", "sn3", "sg3"]
        .iter()
        .map(|e| read(&base, e))
        .collect();
    assert_eq!(before, after);
    assert!(support::leftovers(base.parent().unwrap()).is_empty());
}

#[rstest]
#[case("sn3")]
#[case("sg3")]
#[case("si3")]
fn upgrade_fails_when_an_old_file_is_missing_without_creating_anything(#[case] missing: &str) {
    let (_dir, base) = tmp();
    legacy(&base);
    std::fs::remove_file(ext(&base, missing)).unwrap();
    assert!(upgrade(&base).is_err());
    for e in ["si4", "sn4", "sg4"] {
        assert!(!ext(&base, e).exists(), ".{e} must not appear");
    }
    assert!(support::leftovers(base.parent().unwrap()).is_empty());
}

#[test]
fn upgrade_of_a_bad_si3_leaves_every_file_alone() {
    let (_dir, base) = tmp();
    legacy(&base);
    let mut bad = read(&base, "si3");
    bad[0] = b'X';
    std::fs::write(ext(&base, "si3"), &bad).unwrap();
    assert!(matches!(upgrade(&base), Err(Si4Error::BadMagic { .. })));
    assert_eq!(read(&base, "si3"), bad);
    assert!(ext(&base, "sn3").exists() && ext(&base, "sg3").exists());
    assert!(!ext(&base, "si4").exists());

    let truncated = &bad[..bad.len() - 10];
    let mut ok = truncated.to_vec();
    ok[0] = b'S';
    std::fs::write(ext(&base, "si3"), &ok).unwrap();
    assert!(matches!(upgrade(&base), Err(Si4Error::Truncated { .. })));
    assert!(!ext(&base, "si4").exists());
}
