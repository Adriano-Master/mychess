//! Contract tests of the `.sn4` written by M3-2a (rules 7-9, 14-15, 25-32;
//! decisions D279, D280, D284, D286, D300; `docs/formatos/si4.md` section 7).
//! The file is decoded by an independent decoder (`support::parse_sn4`).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/si4_write_support.rs"]
mod support;

use std::cmp::Ordering;

use mychess_core::Game;
use mychess_db::si4::names_write::{
    MAX_NAME_LEN, NAME_MAX_ID, NameBuilder, name_order, truncate_name,
};
use mychess_db::si4::{
    GameFlags, NameBase, NameKind, PreparedGame, Si4Database, Si4Error, Si4Writer,
};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{named, parse_sn4, read, scid_key, tmp};

const PLAYER: usize = 0;
const EVENT: usize = 1;
const SITE: usize = 2;
const ROUND: usize = 3;

fn players_of(base: &std::path::Path) -> Vec<Vec<u8>> {
    parse_sn4(&read(base, "sn4")).sections[PLAYER]
        .iter()
        .map(|r| r.name.clone())
        .collect()
}

// ---------------------------------------------------------------- order

#[rstest]
#[case("", "a", Ordering::Less)]
#[case("a", "b", Ordering::Less)]
#[case("ab", "abc", Ordering::Less)]
// "ab" comes AFTER "ab\xC3\xA9": the NUL terminator (0) beats a signed negative byte.
#[case("ab", "ab\u{e9}", Ordering::Greater)]
// within the same first byte, UTF-8 bytes (>= 0x80, negative) sort before ASCII
#[case("a\u{e9}", "ab", Ordering::Less)]
// the first byte is unsigned: 0xC3 (195) sorts after every ASCII first byte
#[case("\u{e9}", "z", Ordering::Greater)]
#[case("Z", "a", Ordering::Less)]
fn name_order_follows_rule_26(#[case] a: &str, #[case] b: &str, #[case] expected: Ordering) {
    assert_eq!(name_order(a.as_bytes(), b.as_bytes()), expected);
    assert_eq!(name_order(b.as_bytes(), a.as_bytes()), expected.reverse());
}

#[test]
fn name_order_equal_names_compare_equal() {
    assert_eq!(name_order(b"same", b"same"), Ordering::Equal);
    assert_eq!(name_order(b"", b""), Ordering::Equal);
}

#[test]
fn written_players_are_in_scid_order_with_stable_insertion_ids() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let names = ["b", "ab", "ab\u{e9}", "", "\u{e9}", "Zed", "abc"];
    for (i, n) in names.iter().enumerate() {
        w.add_game(&named(n, n, "E", "S", &i.to_string()), GameFlags::empty())
            .unwrap();
    }
    drop(w);

    let sn4 = parse_sn4(&read(&base, "sn4"));
    let file_order: Vec<&[u8]> = sn4.sections[PLAYER]
        .iter()
        .map(|r| r.name.as_slice())
        .collect();
    let expected: Vec<&[u8]> = vec![b"", b"Zed", b"ab\xC3\xA9", b"ab", b"abc", b"b", b"\xC3\xA9"];
    assert_eq!(file_order, expected);

    // IDs are explicit (insertion order), not the file position.
    let db = Si4Database::open(&base).unwrap();
    for (i, n) in names.iter().enumerate() {
        let e = db.entry(u32::try_from(i).unwrap() + 1).unwrap();
        assert_eq!(
            db.names().get(NameKind::Player, e.white_id).unwrap(),
            n.as_bytes()
        );
        assert_eq!(e.white_id, e.black_id);
        assert_eq!(
            sn4.by_name(PLAYER, n.as_bytes()).unwrap().id,
            u64::from(e.white_id)
        );
    }
}

// ---------------------------------------------------------------- layout

#[test]
fn single_game_sn4_has_the_exact_front_coded_size() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&named("A", "B", "E", "S", "1"), GameFlags::empty())
        .unwrap();
    drop(w);
    let data = read(&base, "sn4");
    // header 36; players A (2+1+1+1) and B (2+1+1+1+1); event, site, round 5 each
    assert_eq!(data.len(), 36 + 5 + 6 + 5 + 5 + 5);
    let sn4 = parse_sn4(&data);
    assert_eq!(sn4.num_names, [2, 1, 1, 1]);
    assert_eq!(sn4.max_freq, [1, 1, 1, 1]);
    assert_eq!(sn4.time_stamp, 0);
}

#[test]
fn names_are_front_coded_against_the_previous_record_of_the_file() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(
        &named("Smith, A", "Smith, B", "E", "S", "1"),
        GameFlags::empty(),
    )
    .unwrap();
    w.add_game(
        &named("Smyth", "Smith, A", "E", "S", "1"),
        GameFlags::empty(),
    )
    .unwrap();
    drop(w);
    let sn4 = parse_sn4(&read(&base, "sn4"));
    let names: Vec<_> = sn4.sections[PLAYER]
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(
        names,
        vec![
            b"Smith, A".to_vec(),
            b"Smith, B".to_vec(),
            b"Smyth".to_vec()
        ]
    );
    // the first record of a section has no prefix byte
    assert_eq!(sn4.prefixes[PLAYER], vec![7, 2]);
}

#[test]
fn frequencies_count_real_uses_and_white_equals_black_counts_twice() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&named("A", "A", "E", "S", "1"), GameFlags::empty())
        .unwrap();
    w.add_game(&named("A", "B", "E", "T", "2"), GameFlags::empty())
        .unwrap();
    drop(w);
    let sn4 = parse_sn4(&read(&base, "sn4"));
    assert_eq!(sn4.by_name(PLAYER, b"A").unwrap().freq, 3);
    assert_eq!(sn4.by_name(PLAYER, b"B").unwrap().freq, 1);
    assert_eq!(sn4.by_name(EVENT, b"E").unwrap().freq, 2);
    assert_eq!(sn4.by_name(SITE, b"S").unwrap().freq, 1);
    assert_eq!(sn4.by_name(ROUND, b"2").unwrap().freq, 1);
    assert_eq!(sn4.max_freq, [3, 2, 1, 1]);
}

#[test]
fn empty_tag_name_is_stored_as_the_empty_name() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&named("", "B", "E", "S", "1"), GameFlags::empty())
        .unwrap();
    drop(w);
    assert_eq!(players_of(&base), vec![b"".to_vec(), b"B".to_vec()]);
}

// ---------------------------------------------------------------- long names

#[test]
fn name_over_255_bytes_is_cut_to_255_and_reported() {
    let long = "a".repeat(300);
    let g = named(&long, "B", "E", "S", "1");
    let p = PreparedGame::from_game(&g, GameFlags::empty()).unwrap();
    assert!(p.names_truncated());
    assert_eq!(p.names()[0], vec![b'a'; MAX_NAME_LEN]);

    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_prepared(&p).unwrap();
    drop(w);
    assert!(players_of(&base).contains(&vec![b'a'; 255]));
}

#[test]
fn truncation_never_splits_a_utf8_sequence() {
    let text = "\u{e9}".repeat(200); // 400 bytes
    let (kept, cut) = truncate_name(&text);
    assert!(cut);
    assert_eq!(kept.len(), 254);
    assert!(kept.chars().all(|c| c == '\u{e9}'));
    assert_eq!(truncate_name("short"), ("short", false));
    let exact = "x".repeat(255);
    assert_eq!(truncate_name(&exact), (exact.as_str(), false));
}

#[test]
fn name_of_exactly_255_bytes_is_kept_whole() {
    let name = "n".repeat(255);
    let p = PreparedGame::from_game(&named(&name, "B", "E", "S", "1"), GameFlags::empty()).unwrap();
    assert!(!p.names_truncated());
    assert_eq!(p.names()[0].len(), 255);
}

#[test]
fn nul_in_a_name_is_invalid_text() {
    let err = PreparedGame::from_game(&named("a\0b", "B", "E", "S", "1"), GameFlags::empty())
        .unwrap_err();
    assert!(matches!(err, Si4Error::InvalidText { .. }), "{err:?}");
}

// ---------------------------------------------------------------- maxFrequency

fn same_name_game(white: &str, round: &str) -> Game {
    named(white, &format!("B{round}"), "?", "?", round)
}

#[test]
fn max_frequency_is_a_historic_high_water_mark_after_replace() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    for r in ["1", "2", "3"] {
        w.add_game(&same_name_game("X", r), GameFlags::empty())
            .unwrap();
    }
    assert_eq!(w.names().max_frequency(NameKind::Player), 3);

    // Replacing game 1 by a game of Y: X goes 3 -> 2, the marker stays at 3.
    w.replace_game(1, &same_name_game("Y", "1")).unwrap();
    assert_eq!(w.names().max_frequency(NameKind::Player), 3);
    drop(w);
    let sn4 = parse_sn4(&read(&base, "sn4"));
    assert_eq!(sn4.max_freq[PLAYER], 3);
    assert_eq!(sn4.by_name(PLAYER, b"X").unwrap().freq, 2);

    // Reopened: the header is the floor, so the marker stays.
    let mut w = Si4Writer::open(&base).unwrap();
    assert_eq!(w.names().max_frequency(NameKind::Player), 3);
    w.add_game(&same_name_game("Z", "4"), GameFlags::empty())
        .unwrap();
    drop(w);
    assert_eq!(parse_sn4(&read(&base, "sn4")).max_freq[PLAYER], 3);
}

#[test]
fn replace_with_the_same_name_raises_the_marker_transiently() {
    // Rule 14: the new names get +1 before the old ones get -1.
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    for r in ["1", "2", "3"] {
        w.add_game(&same_name_game("X", r), GameFlags::empty())
            .unwrap();
    }
    w.replace_game(1, &same_name_game("X", "1")).unwrap();
    drop(w);
    let sn4 = parse_sn4(&read(&base, "sn4"));
    assert_eq!(sn4.by_name(PLAYER, b"X").unwrap().freq, 3, "real count");
    assert_eq!(sn4.max_freq[PLAYER], 4, "3 -> 4 -> 3 raises the marker");
}

#[test]
fn replaced_name_keeps_its_id_and_stays_with_frequency_zero() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&named("Old", "B", "E", "S", "1"), GameFlags::empty())
        .unwrap();
    w.replace_game(1, &named("New", "B", "E", "S", "1"))
        .unwrap();
    drop(w);
    let sn4 = parse_sn4(&read(&base, "sn4"));
    let old = sn4.by_name(PLAYER, b"Old").expect("name kept in the .sn4");
    assert_eq!(old.freq, 0);
    let new = sn4.by_name(PLAYER, b"New").unwrap();
    assert_ne!(old.id, new.id, "ids are not reused");
    assert_eq!(sn4.num_names[PLAYER], 3, "Old, B, New (plus no recycling)");
}

#[test]
fn time_stamp_of_the_name_file_is_preserved_by_later_writes() {
    let (_dir, base) = tmp();
    drop(Si4Writer::create(&base, 1).unwrap());
    let mut sn4 = read(&base, "sn4");
    sn4[8..12].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
    std::fs::write(support::ext(&base, "sn4"), &sn4).unwrap();

    let mut w = Si4Writer::open(&base).unwrap();
    w.add_game(&named("A", "B", "E", "S", "1"), GameFlags::empty())
        .unwrap();
    w.replace_game(1, &named("C", "B", "E", "S", "1")).unwrap();
    drop(w);

    assert_eq!(parse_sn4(&read(&base, "sn4")).time_stamp, 0xDEAD_BEEF);
}

#[test]
fn a_new_database_has_time_stamp_zero_after_adding_games() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&Game::new(), GameFlags::empty()).unwrap();
    drop(w);
    assert_eq!(parse_sn4(&read(&base, "sn4")).time_stamp, 0);
}

#[test]
fn reopening_recounts_frequencies_from_the_entries() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&named("A", "B", "E", "S", "1"), GameFlags::empty())
        .unwrap();
    drop(w);
    // Corrupt the stored frequencies: the writer ignores them (no -fast).
    let mut sn4 = read(&base, "sn4");
    let sn4_parsed = parse_sn4(&sn4);
    assert_eq!(sn4_parsed.by_name(PLAYER, b"A").unwrap().freq, 1);
    sn4[36 + 2] = 9; // frequency byte of the first player
    std::fs::write(support::ext(&base, "sn4"), &sn4).unwrap();

    let w = Si4Writer::open(&base).unwrap();
    assert_eq!(w.names().frequency(NameKind::Player, 0), Some(1));
}

// ---------------------------------------------------------------- widths

fn batch_of(n: usize, make: impl Fn(usize) -> Game) -> Vec<PreparedGame> {
    (0..n)
        .map(|i| PreparedGame::from_game(&make(i), GameFlags::empty()).unwrap())
        .collect()
}

#[rstest]
#[case(255, 1)]
#[case(256, 2)]
#[case(65_535, 2)]
#[case(65_536, 3)]
fn frequency_width_follows_max_frequency(#[case] uses: usize, #[case] width: usize) {
    let (_dir, base) = tmp();
    let one = PreparedGame::from_game(&named("A", "?", "?", "?", "?"), GameFlags::empty()).unwrap();
    let games = vec![one; uses];
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_prepared_batch(&games).unwrap();
    drop(w);

    let data = read(&base, "sn4");
    let sn4 = parse_sn4(&data); // asserts that the widths consume every byte
    assert_eq!(sn4.max_freq[PLAYER], uses as u64);
    assert_eq!(sn4.by_name(PLAYER, b"A").unwrap().freq, uses as u64);
    // Two player records ("?" and "A"), 2-byte ids: record bytes are
    // id(2) + freq(width) + len(1) [+ prefix(1)] + name(1).
    let players = (2 + width + 1 + 1) + (2 + width + 1 + 1 + 1);
    let others = 3 * (2 + width + 1 + 1);
    assert_eq!(data.len(), 36 + players + others);
}

#[rstest]
#[case(65_535, 2)]
#[case(65_536, 3)]
fn id_width_follows_the_number_of_names(#[case] names: usize, #[case] id_width: usize) {
    let (_dir, base) = tmp();
    let games = batch_of(names, |i| {
        named(&format!("p{i}"), &format!("p{i}"), "E", "S", "1")
    });
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_prepared_batch(&games).unwrap();
    assert_eq!(w.names().num_names(NameKind::Player) as usize, names);
    drop(w);

    let data = read(&base, "sn4");
    let sn4 = parse_sn4(&data);
    assert_eq!(sn4.num_names[PLAYER] as usize, names);
    // IDs are exactly 0..names
    let mut ids: Vec<u64> = sn4.sections[PLAYER].iter().map(|r| r.id).collect();
    ids.sort_unstable();
    assert!(ids.iter().enumerate().all(|(i, &id)| id == i as u64));
    // `parse_sn4` consumed every byte with the width of the specification.
    let _ = id_width;
    let _ = &data;
    // And a fresh reader accepts it.
    assert_eq!(Si4Database::open(&base).unwrap().len() as usize, names);
}

#[test]
#[ignore = "slow (262144 games): run with --release -- --ignored"]
fn round_name_limit_refuses_the_whole_batch_and_writes_nothing() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let before = (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"));
    let over = NAME_MAX_ID[3] as usize + 1;
    let games = batch_of(over, |i| named("A", "B", "E", "S", &format!("r{i}")));
    let err = w.add_prepared_batch(&games).unwrap_err();
    assert!(
        matches!(
            err,
            Si4Error::NameLimit {
                kind: NameKind::Round,
                limit: 262_143
            }
        ),
        "{err:?}"
    );
    drop(w);
    assert_eq!(
        before,
        (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"))
    );
}

#[test]
#[ignore = "slow (262143 games): run with --release -- --ignored"]
fn at_the_name_limit_even_an_existing_name_is_refused() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let full = NAME_MAX_ID[3] as usize;
    let games = batch_of(full, |i| named("A", "B", "E", "S", &format!("r{i}")));
    w.add_prepared_batch(&games).unwrap();
    assert_eq!(w.names().num_names(NameKind::Round) as usize, full);
    // rule 8: the limit is tested before looking the name up
    let err = w
        .add_game(&named("A", "B", "E", "S", "r0"), GameFlags::empty())
        .unwrap_err();
    assert!(
        matches!(
            err,
            Si4Error::NameLimit {
                kind: NameKind::Round,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(w.len() as usize, full, "nothing was added");
}

// ---------------------------------------------------------------- NameBuilder

#[test]
fn builder_frequency_never_goes_below_zero_and_marker_never_goes_down() {
    let mut b = NameBuilder::new();
    let id = b.add_name(NameKind::Player, b"X").unwrap();
    b.inc_frequency(NameKind::Player, id, -1);
    assert_eq!(b.frequency(NameKind::Player, id), Some(0));
    b.inc_frequency(NameKind::Player, id, 5);
    b.inc_frequency(NameKind::Player, id, -4);
    assert_eq!(b.frequency(NameKind::Player, id), Some(1));
    assert_eq!(b.max_frequency(NameKind::Player), 5);
}

#[test]
fn builder_add_name_returns_the_existing_id_and_cuts_at_255() {
    let mut b = NameBuilder::new();
    let a = b.add_name(NameKind::Event, b"Open").unwrap();
    assert_eq!(b.add_name(NameKind::Event, b"Open").unwrap(), a);
    // the same bytes in another kind are another name
    let s = b.add_name(NameKind::Site, b"Open").unwrap();
    assert_eq!(b.find(NameKind::Site, b"Open"), Some(s));
    let long = vec![b'z'; 400];
    let id = b.add_name(NameKind::Round, &long).unwrap();
    assert_eq!(b.get(NameKind::Round, id).unwrap().len(), 255);
}

#[test]
fn builder_rollback_undoes_names_frequencies_and_marker() {
    let mut b = NameBuilder::new();
    let a = b.add_name(NameKind::Player, b"A").unwrap();
    b.inc_frequency(NameKind::Player, a, 1);
    let snapshot = b.to_bytes();

    b.begin();
    b.add_name(NameKind::Player, b"B").unwrap();
    b.inc_frequency(NameKind::Player, a, 7);
    b.rollback();

    assert_eq!(b.to_bytes(), snapshot);
    assert_eq!(b.max_frequency(NameKind::Player), 1);
    assert_eq!(b.find(NameKind::Player, b"B"), None);

    b.begin();
    b.add_name(NameKind::Player, b"B").unwrap();
    b.commit();
    b.rollback(); // nothing to undo after commit
    assert!(b.find(NameKind::Player, b"B").is_some());
}

#[test]
fn builder_from_name_base_reproduces_the_bytes() {
    let mut b = NameBuilder::new();
    for (k, n) in [
        (NameKind::Player, "Zed"),
        (NameKind::Player, "ab"),
        (NameKind::Site, "x"),
    ] {
        let id = b.add_name(k, n.as_bytes()).unwrap();
        b.inc_frequency(k, id, 2);
    }
    b.set_time_stamp(77);
    let bytes = b.to_bytes();
    let again = NameBuilder::from_name_base(&NameBase::parse(&bytes).unwrap());
    // Names, IDs, the marker and the time stamp come from the file; the
    // frequencies are recounted from the entries (D300), so they start at 0.
    let (before, after) = (parse_sn4(&bytes), parse_sn4(&again.to_bytes()));
    assert_eq!(again.time_stamp(), 77);
    assert_eq!(after.max_freq, before.max_freq);
    assert_eq!(after.num_names, before.num_names);
    for t in 0..4 {
        let names = |s: &support::Sn4| {
            s.sections[t]
                .iter()
                .map(|r| (r.id, r.name.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&after), names(&before));
        assert!(after.sections[t].iter().all(|r| r.freq == 0));
    }
}

proptest! {
    #[test]
    fn random_byte_names_are_written_sorted_and_read_back(
        names in prop::collection::btree_set(
            prop::collection::vec(prop::sample::select(vec![1u8, 0x41, 0x61, 0x7F, 0x80, 0xC3, 0xA9, 0xFF]), 0..6),
            0..40,
        )
    ) {
        let mut b = NameBuilder::new();
        let mut ids = Vec::new();
        for n in &names {
            let id = b.add_name(NameKind::Player, n).unwrap();
            b.inc_frequency(NameKind::Player, id, 1);
            ids.push((id, n.clone()));
        }
        let bytes = b.to_bytes();
        let sn4 = parse_sn4(&bytes);

        let mut expected: Vec<Vec<u8>> = names.iter().cloned().collect();
        expected.sort_by_key(|n| scid_key(n));
        let got: Vec<Vec<u8>> = sn4.sections[PLAYER].iter().map(|r| r.name.clone()).collect();
        prop_assert_eq!(&got, &expected);
        for (id, n) in &ids {
            prop_assert_eq!(sn4.name(PLAYER, u64::from(*id)).map(|r| &r.name), Some(n));
        }
        // the reader of M3-1 accepts it and returns the same names
        let base = NameBase::parse(&bytes).unwrap();
        for (id, n) in &ids {
            prop_assert_eq!(base.get(NameKind::Player, *id), Some(n.as_slice()));
        }
    }
}
