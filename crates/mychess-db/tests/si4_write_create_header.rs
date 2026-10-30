//! Contract tests of database creation, the header and the writer lock
//! (spec M3-2a: rules 1-4, 16-24; decisions D282, D283, D291, D298, D303, D307).
#![allow(
    clippy::assertions_on_result_states,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::needless_pass_by_value
)]

#[path = "common/si4_write_support.rs"]
mod support;

use mychess_core::Game;
use mychess_db::si4::{GameFlags, HEADER_LEN, MAX_GAMES, Si4Database, Si4Error, Si4Writer};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{ext, game_ucis, named, read, tmp};

fn empty_header(base_type: u32) -> Vec<u8> {
    let mut h = vec![0u8; HEADER_LEN];
    h[..8].copy_from_slice(b"Scid.si\0");
    h[8..10].copy_from_slice(&400u16.to_be_bytes());
    h[10..14].copy_from_slice(&base_type.to_be_bytes());
    h[19] = 2; // raw autoload: first game
    h
}

#[rstest]
#[case(0)]
#[case(1)]
#[case(3)]
fn create_writes_exact_bytes_of_an_empty_database(#[case] base_type: u32) {
    let (_dir, base) = tmp();
    let writer = Si4Writer::create(&base, base_type).unwrap();
    drop(writer);

    assert_eq!(read(&base, "si4"), empty_header(base_type));
    let mut sn4 = vec![0u8; 36];
    sn4[..8].copy_from_slice(b"Scid.sn\0");
    assert_eq!(read(&base, "sn4"), sn4);
    assert_eq!(read(&base, "sg4").len(), 0);
}

#[test]
fn created_database_opens_with_zero_games_and_no_name_question_mark() {
    let (_dir, base) = tmp();
    drop(Si4Writer::create(&base, 1).unwrap());
    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 0);
    assert_eq!(db.header().base_type(), 1);
    assert_eq!(db.header().auto_load_raw(), 2);
    assert!(db.header().description().is_empty());
    assert!(
        db.header()
            .custom_flag_descriptions()
            .iter()
            .all(Vec::is_empty)
    );
    // Rule 2: a new base has no "?" pre-registered.
    let sn4 = support::parse_sn4(&read(&base, "sn4"));
    assert_eq!(sn4.num_names, [0; 4]);
    assert_eq!(sn4.time_stamp, 0);
}

#[test]
fn create_accepts_any_of_the_three_file_paths() {
    let (_dir, base) = tmp();
    drop(Si4Writer::create(&ext(&base, "si4"), 0).unwrap());
    assert!(ext(&base, "si4").exists());
    assert!(ext(&base, "sn4").exists());
    assert!(ext(&base, "sg4").exists());
}

#[rstest]
#[case("si4")]
#[case("sn4")]
#[case("sg4")]
fn create_refuses_when_a_file_of_the_database_exists(#[case] existing: &str) {
    let (dir, base) = tmp();
    std::fs::write(ext(&base, existing), b"precious").unwrap();

    let err = Si4Writer::create(&base, 1).unwrap_err();

    assert!(matches!(err, Si4Error::AlreadyExists { .. }), "{err:?}");
    assert_eq!(
        read(&base, existing),
        b"precious",
        "existing file untouched"
    );
    for other in ["si4", "sn4", "sg4"] {
        if other != existing {
            assert!(!ext(&base, other).exists(), "{other} must not be created");
        }
    }
    assert!(support::leftovers(dir.path()).is_empty());
}

#[test]
fn create_leaves_no_temporary_files() {
    let (dir, base) = tmp();
    drop(Si4Writer::create(&base, 1).unwrap());
    assert!(support::leftovers(dir.path()).is_empty());
}

#[test]
fn create_removes_a_stale_stc_cache() {
    let (_dir, base) = tmp();
    std::fs::write(ext(&base, "stc"), b"cache").unwrap();
    drop(Si4Writer::create(&base, 1).unwrap());
    assert!(!ext(&base, "stc").exists());
}

// ---------------------------------------------------------------- lock

#[test]
fn second_writer_is_refused_while_the_first_lives() {
    let (_dir, base) = tmp();
    let first = Si4Writer::create(&base, 1).unwrap();

    let err = Si4Writer::open(&base).unwrap_err();

    assert!(matches!(err, Si4Error::AlreadyOpen { .. }), "{err:?}");
    drop(first);
    // After the first writer is gone the lock is free.
    drop(Si4Writer::open(&base).unwrap());
}

#[test]
fn failed_second_open_does_not_disturb_the_first_writer() {
    let (_dir, base) = tmp();
    let mut first = Si4Writer::create(&base, 1).unwrap();
    first.add_game(&Game::new(), GameFlags::empty()).unwrap();
    assert!(Si4Writer::open(&base).is_err());
    first.add_game(&Game::new(), GameFlags::empty()).unwrap();
    assert_eq!(first.len(), 2);
    drop(first);
    // Windows (D303): the .si4 cannot be read while a writer lives.
    assert_eq!(Si4Database::open(&base).unwrap().len(), 2);
}

// ---------------------------------------------------------------- header

fn header_after(edit: impl FnOnce(&mut Si4Writer)) -> Vec<u8> {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    edit(&mut w);
    drop(w);
    read(&base, "si4")
}

#[test]
fn description_is_cut_at_107_bytes_and_keeps_byte_127_zero() {
    let bytes = header_after(|w| w.set_description(&"x".repeat(200)).unwrap());
    assert_eq!(bytes.len(), HEADER_LEN);
    assert!(bytes[20..127].iter().all(|&b| b == b'x'));
    assert_eq!(bytes[127], 0);
}

#[test]
fn description_ends_at_the_first_newline_and_drops_it() {
    let bytes = header_after(|w| w.set_description("first\nsecond").unwrap());
    assert_eq!(&bytes[20..25], b"first");
    assert!(bytes[25..128].iter().all(|&b| b == 0), "newline not stored");
}

#[test]
fn description_replacement_erases_the_old_longer_text() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.set_description("a long description to be replaced")
        .unwrap();
    w.set_description("short").unwrap();
    drop(w);
    let bytes = read(&base, "si4");
    assert_eq!(&bytes[20..25], b"short");
    assert!(bytes[25..128].iter().all(|&b| b == 0));
}

#[test]
fn description_keeps_utf8_bytes_as_given() {
    let bytes = header_after(|w| w.set_description("Partidas \u{e9}picas").unwrap());
    let text = "Partidas \u{e9}picas".as_bytes();
    assert_eq!(&bytes[20..20 + text.len()], text);
}

#[rstest]
#[case(0, 1)]
#[case(1, 2)]
#[case(5, 6)]
#[case(MAX_GAMES, 16_777_215)]
fn autoload_stores_n_plus_one(#[case] game: u32, #[case] raw: u32) {
    let bytes = header_after(|w| w.set_auto_load(game).unwrap());
    assert_eq!(support::be(&bytes[17..20]) as u32, raw);
}

#[test]
fn autoload_above_max_games_is_rejected_and_header_is_unchanged() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let before = read(&base, "si4");
    let err = w.set_auto_load(MAX_GAMES + 1).unwrap_err();
    assert!(matches!(err, Si4Error::InvalidAutoLoad { .. }), "{err:?}");
    drop(w);
    assert_eq!(read(&base, "si4"), before);
}

#[rstest]
#[case(0)]
#[case(4)]
#[case(31)]
#[case(32)]
#[case(0xDEAD_BEEF)]
fn base_type_is_stored_as_given(#[case] t: u32) {
    let bytes = header_after(|w| w.set_base_type(t).unwrap());
    assert_eq!(&bytes[10..14], &t.to_be_bytes());
}

#[rstest]
#[case(1, "abcdefgh", "abcdefgh")]
#[case(2, "abcdefghij", "abcdefgh")]
#[case(6, "ab", "ab")]
// 7 ASCII bytes + a 2-byte char would need 9 bytes: cut at the boundary (D282).
#[case(3, "aaaaaaa\u{e9}", "aaaaaaa")]
#[case(4, "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}", "\u{e9}\u{e9}\u{e9}\u{e9}")]
fn custom_flag_description_is_at_most_8_bytes_on_a_char_boundary(
    #[case] number: u8,
    #[case] text: &str,
    #[case] expected: &str,
) {
    let bytes = header_after(|w| w.set_custom_flag_description(number, text).unwrap());
    let at = 128 + 9 * (usize::from(number) - 1);
    let slot = &bytes[at..at + 9];
    assert_eq!(&slot[..expected.len()], expected.as_bytes());
    assert!(
        slot[expected.len()..].iter().all(|&b| b == 0),
        "zero padded, NUL at 9th"
    );
    // other slots untouched
    for other in 1..=6u8 {
        if other != number {
            let o = 128 + 9 * (usize::from(other) - 1);
            assert!(bytes[o..o + 9].iter().all(|&b| b == 0));
        }
    }
}

#[test]
fn custom_flag_description_shorter_text_clears_the_old_one() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.set_custom_flag_description(1, "abcdefgh").unwrap();
    w.set_custom_flag_description(1, "xy").unwrap();
    drop(w);
    assert_eq!(&read(&base, "si4")[128..137], b"xy\0\0\0\0\0\0\0");
}

#[rstest]
#[case(0)]
#[case(7)]
#[case(255)]
fn custom_flag_number_outside_one_to_six_is_an_error(#[case] number: u8) {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let err = w.set_custom_flag_description(number, "x").unwrap_err();
    assert!(
        matches!(err, Si4Error::InvalidCustomFlag(n) if n == number),
        "{err:?}"
    );
}

#[test]
fn editing_one_header_field_changes_only_that_field() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.set_description("desc").unwrap();
    w.set_custom_flag_description(2, "mine").unwrap();
    w.set_auto_load(7).unwrap();
    drop(w);
    let before = read(&base, "si4");

    let mut w = Si4Writer::open(&base).unwrap();
    w.set_base_type(9).unwrap();
    drop(w);
    let after = read(&base, "si4");

    let mut expected = before.clone();
    expected[10..14].copy_from_slice(&9u32.to_be_bytes());
    assert_eq!(after, expected);
}

#[test]
fn header_edits_do_not_touch_entries_names_or_games() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&named("A", "B", "E", "S", "1"), GameFlags::empty())
        .unwrap();
    let (sn4, sg4) = (read(&base, "sn4"), read(&base, "sg4"));
    let entries = read(&base, "si4")[HEADER_LEN..].to_vec();

    w.set_description("hello").unwrap();
    w.set_base_type(5).unwrap();
    w.set_auto_load(1).unwrap();
    w.set_custom_flag_description(1, "x").unwrap();
    drop(w);

    assert_eq!(read(&base, "sn4"), sn4);
    assert_eq!(read(&base, "sg4"), sg4);
    assert_eq!(read(&base, "si4")[HEADER_LEN..], entries[..]);
}

#[test]
fn header_keeps_number_of_games_updated_after_each_add() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    for i in 1..=3u32 {
        let n = w
            .add_game(&game_ucis(&["e2e4"]), GameFlags::empty())
            .unwrap();
        assert_eq!(n, i);
        assert_eq!(w.len(), i);
        assert_eq!(support::be(&read(&base, "si4")[14..17]) as u32, i);
    }
}

#[test]
fn header_edits_survive_adding_games_afterwards() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 7).unwrap();
    w.set_description("kept").unwrap();
    w.set_custom_flag_description(3, "cf").unwrap();
    w.add_game(&Game::new(), GameFlags::empty()).unwrap();
    drop(w);
    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.header().base_type(), 7);
    assert_eq!(db.header().description(), b"kept");
    assert_eq!(db.header().custom_flag_descriptions()[2], b"cf");
    assert_eq!(db.len(), 1);
}
