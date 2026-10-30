//! Contract tests of adding, replacing and flagging games, record size, `.sg4`
//! blocks, validation, `.stc` removal and crash consistency (spec M3-2a: rules
//! 5-15, 17, 37-41, 48; decisions D289, D298-D307).
#![allow(
    clippy::assertions_on_result_states,
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation
)]

#[path = "common/si4_write_support.rs"]
mod support;

use std::path::Path;

use mychess_core::{Game, Position};
use mychess_db::si4::{
    ENTRY_LEN, FlagOp, GameFlags, HEADER_LEN, MAX_RECORD_LEN, MAX_WRITE_TAGS, PreparedGame,
    Si4Database, Si4Error, Si4Writer, write::plan_append,
};
use pretty_assertions::assert_eq;
use rstest::rstest;
use support::{BLOCK, ext, game_of_record_len, game_ucis, named, read, shuffle, tmp};

fn db_game(base: &Path, n: u32) -> Game {
    Si4Database::open(base).unwrap().game(n).unwrap()
}

fn commented() -> Game {
    let mut g = named(
        "Carlsen, Magnus",
        "Nepomniachtchi, Ian",
        "WCh",
        "Dubai",
        "3",
    );
    let ids: Vec<_> = g.mainline().collect();
    g.set_comment(ids[0], "best by test").unwrap();
    g.set_initial_comment("prelude");
    g.add_nag(ids[1], mychess_core::Nag::new(3).unwrap())
        .unwrap();
    g
}

// ---------------------------------------------------------------- add

#[test]
fn add_returns_consecutive_numbers_and_games_read_back() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let games = [
        commented(),
        game_ucis(&["d2d4", "d7d5", "c2c4"]),
        Game::new(),
    ];
    for (i, g) in games.iter().enumerate() {
        assert_eq!(w.add_game(g, GameFlags::empty()).unwrap() as usize, i + 1);
    }
    drop(w);
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 3);
    for (i, g) in games.iter().enumerate() {
        let back = db.game(u32::try_from(i).unwrap() + 1).unwrap();
        assert!(g.same_content(&back), "game {}", i + 1);
    }
}

#[test]
fn add_keeps_the_si4_exactly_header_plus_47_per_game() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    for _ in 0..4 {
        w.add_game(&commented(), GameFlags::empty()).unwrap();
    }
    drop(w);
    assert_eq!(read(&base, "si4").len(), HEADER_LEN + 4 * ENTRY_LEN);
}

#[test]
fn add_applies_the_given_user_flags_and_ignores_derived_ones() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let flags = GameFlags::from_letters("D!3").unwrap() | GameFlags::PROMOTION | GameFlags::START;
    w.add_game(&game_ucis(&["e2e4"]), flags).unwrap();
    drop(w);
    let e = Si4Database::open(&base).unwrap().entry(1).unwrap();
    assert_eq!(e.flags, GameFlags::from_letters("D!3").unwrap());
}

#[test]
fn entry_offset_and_length_point_at_the_record() {
    let (_dir, base) = tmp();
    let p0 = PreparedGame::from_game(&commented(), GameFlags::empty()).unwrap();
    let p1 = PreparedGame::from_game(&game_ucis(&["e2e4"]), GameFlags::empty()).unwrap();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_prepared(&p0).unwrap();
    w.add_prepared(&p1).unwrap();
    drop(w);
    let db = Si4Database::open(&base).unwrap();
    let (e0, e1) = (db.entry(1).unwrap(), db.entry(2).unwrap());
    assert_eq!(e0.offset, 0, "offset 0 is valid");
    assert_eq!(e0.length as usize, p0.record().len());
    assert_eq!(e1.offset as usize, p0.record().len());
    let sg4 = read(&base, "sg4");
    assert_eq!(&sg4[..p0.record().len()], p0.record());
    assert_eq!(&sg4[e1.offset as usize..], p1.record());
}

#[test]
fn batch_adds_consecutive_games_and_reports_the_range() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&Game::new(), GameFlags::empty()).unwrap();
    let batch: Vec<PreparedGame> = (0..5)
        .map(|i| {
            PreparedGame::from_game(
                &named(&format!("P{i}"), "Q", "E", "S", "1"),
                GameFlags::empty(),
            )
            .unwrap()
        })
        .collect();
    let range = w.add_prepared_batch(&batch).unwrap();
    assert_eq!(range, 2..7);
    assert_eq!(w.len(), 6);
    let empty = w.add_prepared_batch(&[]).unwrap();
    assert_eq!(empty, 7..7);
    drop(w);
    let mut db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 6);
    assert_eq!(db.game(4).unwrap().tags().white, "P2");
}

#[test]
fn empty_batch_changes_no_file() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&commented(), GameFlags::empty()).unwrap();
    let before = (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"));
    w.add_prepared_batch(&[]).unwrap();
    drop(w);
    assert_eq!(
        before,
        (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"))
    );
}

#[test]
fn batch_result_equals_adding_one_by_one() {
    let games: Vec<Game> = (0..6)
        .map(|i| named(&format!("P{}", i % 3), "Q", "E", "S", &i.to_string()))
        .collect();
    let (_d1, one) = tmp();
    let mut w = Si4Writer::create(&one, 1).unwrap();
    for g in &games {
        w.add_game(g, GameFlags::empty()).unwrap();
    }
    drop(w);
    let (_d2, batch) = tmp();
    let mut w = Si4Writer::create(&batch, 1).unwrap();
    let prepared: Vec<_> = games
        .iter()
        .map(|g| PreparedGame::from_game(g, GameFlags::empty()).unwrap())
        .collect();
    w.add_prepared_batch(&prepared).unwrap();
    drop(w);
    for e in ["si4", "sn4", "sg4"] {
        assert_eq!(read(&one, e), read(&batch, e), ".{e}");
    }
}

// ---------------------------------------------------------------- replace

#[test]
fn replace_appends_the_new_record_and_keeps_the_old_one_as_dead_space() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&commented(), GameFlags::empty()).unwrap();
    w.add_game(&game_ucis(&["d2d4"]), GameFlags::empty())
        .unwrap();
    let sg4_before = read(&base, "sg4");
    let entry2 = support::entry_bytes(&base, 2);

    let new = game_ucis(&["c2c4", "e7e5"]);
    w.replace_game(1, &new).unwrap();
    drop(w);

    let sg4 = read(&base, "sg4");
    assert_eq!(
        &sg4[..sg4_before.len()],
        &sg4_before[..],
        "old bytes untouched"
    );
    assert!(sg4.len() > sg4_before.len());
    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.len(), 2, "numGames unchanged");
    assert_eq!(db.entry(1).unwrap().offset as usize, sg4_before.len());
    assert_eq!(
        support::entry_bytes(&base, 2),
        entry2,
        "other entries untouched"
    );
    let mut db = db;
    assert!(db.game(1).unwrap().same_content(&new));
}

#[test]
fn replace_inherits_the_19_editable_flags_including_delete() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_ucis(&["e2e4"]), GameFlags::empty())
        .unwrap();
    w.set_flag(1, GameFlags::EDITABLE, FlagOp::Set).unwrap();

    // the new game asks for other flags: they are discarded
    let p = PreparedGame::from_game(&game_ucis(&["d2d4"]), GameFlags::BLUNDER).unwrap();
    w.replace_prepared(1, &p).unwrap();
    drop(w);

    let e = Si4Database::open(&base).unwrap().entry(1).unwrap();
    assert_eq!(e.flags, GameFlags::EDITABLE);
    assert!(e.flags.contains(GameFlags::DELETE));
}

#[test]
fn replace_takes_derived_flags_from_the_new_game() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let pos = Position::from_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
    let mut promo = Game::from_position(pos);
    let root = promo.root();
    support::play(&mut promo, root, "a7a8n");
    w.add_game(&promo, GameFlags::empty()).unwrap();
    drop(w);
    let flags = Si4Database::open(&base).unwrap().entry(1).unwrap().flags;
    assert!(flags.contains(GameFlags::START | GameFlags::UNDER_PROMOTION));
    let mut w = Si4Writer::open(&base).unwrap();
    w.replace_game(1, &game_ucis(&["e2e4"])).unwrap();
    drop(w);
    let e = Si4Database::open(&base).unwrap().entry(1).unwrap();
    assert!(!e.flags.intersects(GameFlags::DERIVED));
}

#[test]
fn replace_zeroes_the_spare_bit_but_set_flag_preserves_it() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_ucis(&["e2e4"]), GameFlags::empty())
        .unwrap();
    drop(w);
    // turn on bit 6 of Length_High by hand
    let mut si4 = read(&base, "si4");
    si4[HEADER_LEN + 6] |= 0x40;
    std::fs::write(ext(&base, "si4"), &si4).unwrap();
    assert!(
        Si4Database::open(&base)
            .unwrap()
            .entry(1)
            .unwrap()
            .length_spare_bit
    );

    let mut w = Si4Writer::open(&base).unwrap();
    assert!(w.set_flag(1, GameFlags::CUSTOM3, FlagOp::Set).unwrap());
    assert!(w.set_flag(1, GameFlags::DELETE, FlagOp::Toggle).unwrap());
    assert_eq!(
        support::entry_bytes(&base, 1)[6] & 0x40,
        0x40,
        "bit 6 kept by flag edits"
    );

    w.replace_game(1, &game_ucis(&["d2d4"])).unwrap();
    drop(w);
    let b = support::entry_bytes(&base, 1);
    assert_eq!(b[6] & 0x40, 0, "bit 6 cleared by replace");
    assert_eq!(b[6] & 0x3F, 0b100, "custom flag 3 inherited");
}

#[rstest]
#[case(0)]
#[case(3)]
#[case(u32::MAX)]
fn replace_with_a_number_out_of_range_is_an_error_and_changes_nothing(#[case] n: u32) {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&commented(), GameFlags::empty()).unwrap();
    w.add_game(&commented(), GameFlags::empty()).unwrap();
    let before = (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"));
    let err = w.replace_game(n, &Game::new()).unwrap_err();
    assert!(
        matches!(err, Si4Error::InvalidGameNumber { number, len: 2 } if number == n),
        "{err:?}"
    );
    drop(w);
    assert_eq!(
        before,
        (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"))
    );
}

// ---------------------------------------------------------------- flags

#[test]
fn set_flag_set_clear_toggle_for_each_of_the_19_flags() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_ucis(&["e2e4"]), GameFlags::empty())
        .unwrap();
    w.add_game(&game_ucis(&["d2d4"]), GameFlags::empty())
        .unwrap();
    for letter in mychess_db::si4::FLAG_LETTERS.chars() {
        let flag = GameFlags::from_letter(letter).unwrap();
        assert!(w.set_flag(1, flag, FlagOp::Set).unwrap(), "{letter} set");
        assert!(
            !w.set_flag(1, flag, FlagOp::Set).unwrap(),
            "{letter} set again"
        );
        assert!(
            w.set_flag(1, flag, FlagOp::Clear).unwrap(),
            "{letter} clear"
        );
        assert!(
            !w.set_flag(1, flag, FlagOp::Clear).unwrap(),
            "{letter} clear again"
        );
        assert!(
            w.set_flag(1, flag, FlagOp::Toggle).unwrap(),
            "{letter} toggle on"
        );
        assert!(
            w.set_flag(1, flag, FlagOp::Toggle).unwrap(),
            "{letter} toggle off"
        );
    }
    drop(w);
    let db = Si4Database::open(&base).unwrap();
    assert!(db.entry(1).unwrap().flags.is_empty());
}

#[test]
fn set_flag_writes_only_the_flag_bytes_of_that_entry() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    for i in 0..3 {
        w.add_game(
            &named("A", "B", "E", "S", &i.to_string()),
            GameFlags::empty(),
        )
        .unwrap();
    }
    let all_before = (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"));
    w.set_flag(2, GameFlags::DELETE | GameFlags::CUSTOM1, FlagOp::Set)
        .unwrap();
    drop(w);
    let (si4_a, sn4_a, sg4_a) = all_before;
    assert_eq!(read(&base, "sn4"), sn4_a);
    assert_eq!(read(&base, "sg4"), sg4_a);
    let si4_b = read(&base, "si4");
    let mut expected = si4_a;
    let at = HEADER_LEN + ENTRY_LEN;
    expected[at + 6] |= 0x01; // custom flag 1
    expected[at + 8] |= 0x08; // D = bit 3
    assert_eq!(si4_b, expected);
}

#[rstest]
#[case(GameFlags::START)]
#[case(GameFlags::PROMOTION)]
#[case(GameFlags::UNDER_PROMOTION)]
#[case(GameFlags::empty())]
fn set_flag_refuses_derived_flags_and_the_empty_set(#[case] flag: GameFlags) {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_ucis(&["e2e4"]), GameFlags::empty())
        .unwrap();
    let err = w.set_flag(1, flag, FlagOp::Set).unwrap_err();
    assert!(matches!(err, Si4Error::NotEditableFlag), "{err:?}");
}

#[test]
fn set_flag_with_a_bad_game_number_is_an_error() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_ucis(&["e2e4"]), GameFlags::empty())
        .unwrap();
    for n in [0, 2] {
        let err = w.set_flag(n, GameFlags::DELETE, FlagOp::Set).unwrap_err();
        assert!(matches!(err, Si4Error::InvalidGameNumber { .. }), "{err:?}");
    }
}

#[test]
fn set_flag_on_derived_bits_keeps_them_when_editing_user_flags() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let pos = Position::from_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
    let mut g = Game::from_position(pos);
    let root = g.root();
    support::play(&mut g, root, "a7a8n");
    w.add_game(&g, GameFlags::empty()).unwrap();
    w.set_flag(1, GameFlags::TACTICS, FlagOp::Set).unwrap();
    drop(w);
    let f = Si4Database::open(&base).unwrap().entry(1).unwrap().flags;
    assert!(f.contains(
        GameFlags::START | GameFlags::PROMOTION | GameFlags::UNDER_PROMOTION | GameFlags::TACTICS
    ));
}

// ---------------------------------------------------------------- record size

#[test]
fn record_of_131071_bytes_is_written_and_read_back() {
    let (_dir, base) = tmp();
    let game = game_of_record_len(MAX_RECORD_LEN);
    let p = PreparedGame::from_game(&game, GameFlags::empty()).unwrap();
    assert_eq!(p.record().len(), 131_071);
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_prepared(&p).unwrap();
    drop(w);

    let e = Si4Database::open(&base).unwrap().entry(1).unwrap();
    assert_eq!(e.length, 131_071);
    let b = support::entry_bytes(&base, 1);
    assert_eq!(
        b[6] & 0x80,
        0x80,
        "bit 16 of the length lives in Length_High"
    );
    assert_eq!(u16::from_be_bytes([b[4], b[5]]), 0xFFFF);
    assert_eq!(read(&base, "sg4").len(), 131_071);
    assert!(db_game(&base, 1).same_content(&game));
}

#[test]
fn record_of_131072_bytes_is_refused_without_writing() {
    let game = game_of_record_len(131_072);
    let err = PreparedGame::from_game(&game, GameFlags::empty()).unwrap_err();
    assert!(
        matches!(
            err,
            Si4Error::RecordTooLarge {
                size: 131_072,
                limit: 131_071
            }
        ),
        "{err:?}"
    );
}

#[test]
fn much_larger_record_is_refused() {
    let game = game_of_record_len(300_000);
    assert!(matches!(
        PreparedGame::from_game(&game, GameFlags::empty()).unwrap_err(),
        Si4Error::RecordTooLarge { .. }
    ));
}

#[test]
fn record_of_131070_bytes_is_accepted_too() {
    let p = PreparedGame::from_game(&game_of_record_len(131_070), GameFlags::empty()).unwrap();
    assert_eq!(p.record().len(), 131_070);
}

// ---------------------------------------------------------------- blocks

#[rstest]
#[case(0, 100, 0, 0)]
#[case(131_072, 1, 131_072, 0)]
#[case(131_071, 1, 131_071, 0)]
#[case(131_071, 2, 131_072, 1)]
#[case(100_000, 31_072, 100_000, 0)]
#[case(100_000, 31_073, 131_072, 31_072)]
#[case(262_144, 5, 262_144, 0)]
fn plan_append_never_crosses_a_block(
    #[case] file_len: u64,
    #[case] record: usize,
    #[case] offset: u64,
    #[case] padding: u64,
) {
    assert_eq!(plan_append(file_len, record), (offset, padding));
}

#[test]
fn record_that_does_not_fit_starts_the_next_block_with_a_hole_of_zeros() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_of_record_len(100_000), GameFlags::empty())
        .unwrap();
    w.add_game(&game_of_record_len(40_000), GameFlags::empty())
        .unwrap();
    drop(w);

    let sg4 = read(&base, "sg4");
    assert_eq!(
        sg4.len(),
        BLOCK + 40_000,
        "file ends at the end of the last record"
    );
    assert!(sg4[100_000..BLOCK].iter().all(|&b| b == 0), "hole is zeros");
    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.entry(1).unwrap().offset, 0);
    assert_eq!(db.entry(2).unwrap().offset as usize, BLOCK);
    // no record crosses a block
    let mut db = db;
    assert_eq!(
        db.game(2).unwrap().initial_comment().unwrap().len(),
        40_000 - 5
    );
}

#[test]
fn record_that_fits_exactly_fills_the_block_and_the_next_opens_a_new_one() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_of_record_len(131_069), GameFlags::empty())
        .unwrap();
    w.add_game(&Game::new(), GameFlags::empty()).unwrap(); // 3 bytes, ends at 131072
    assert_eq!(read(&base, "sg4").len(), BLOCK);
    // a file that is an exact multiple of the block counts as a full block
    w.add_game(&Game::new(), GameFlags::empty()).unwrap();
    drop(w);
    let sg4 = read(&base, "sg4");
    assert_eq!(sg4.len(), BLOCK + 3);
    let db = Si4Database::open(&base).unwrap();
    assert_eq!(db.entry(2).unwrap().offset, 131_069);
    assert_eq!(
        db.entry(3).unwrap().offset as usize,
        BLOCK,
        "no padding needed"
    );
}

#[test]
fn reopened_writer_computes_the_append_position_from_the_file_size() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_of_record_len(100_000), GameFlags::empty())
        .unwrap();
    drop(w);
    let mut w = Si4Writer::open(&base).unwrap();
    w.add_game(&game_of_record_len(40_000), GameFlags::empty())
        .unwrap();
    drop(w);
    assert_eq!(
        Si4Database::open(&base).unwrap().entry(2).unwrap().offset as usize,
        BLOCK
    );
}

#[test]
fn replace_at_a_block_boundary_appends_in_the_next_block() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_of_record_len(100_000), GameFlags::empty())
        .unwrap();
    w.replace_game(1, &game_of_record_len(50_000)).unwrap();
    drop(w);
    let e = Si4Database::open(&base).unwrap().entry(1).unwrap();
    assert_eq!(e.offset as usize, BLOCK);
    assert_eq!(read(&base, "sg4").len(), BLOCK + 50_000);
}

// ---------------------------------------------------------------- validation

fn with_tags(n: usize, name_len: usize, value_len: usize) -> Game {
    let mut g = game_ucis(&["e2e4"]);
    for i in 0..n {
        let mut name = format!("T{i}");
        while name.len() < name_len {
            name.push('x');
        }
        g.tags_mut()
            .set_extra(&name, &"v".repeat(value_len))
            .unwrap();
    }
    g
}

#[test]
fn forty_extra_tags_are_accepted_and_forty_one_refused() {
    let ok = PreparedGame::from_game(&with_tags(MAX_WRITE_TAGS, 5, 3), GameFlags::empty());
    assert!(ok.is_ok());
    let err = PreparedGame::from_game(&with_tags(MAX_WRITE_TAGS + 1, 5, 3), GameFlags::empty())
        .unwrap_err();
    assert!(matches!(err, Si4Error::InvalidTag { .. }), "{err:?}");
}

#[rstest]
#[case(240, 1, true)]
#[case(241, 1, false)]
#[case(10, 255, true)]
#[case(10, 256, false)]
#[case(240, 255, true)]
fn tag_name_and_value_lengths_are_limited(
    #[case] name_len: usize,
    #[case] value_len: usize,
    #[case] accepted: bool,
) {
    let r = PreparedGame::from_game(&with_tags(1, name_len, value_len), GameFlags::empty());
    assert_eq!(r.is_ok(), accepted, "{:?}", r.err());
    if let Err(e) = r {
        assert!(matches!(e, Si4Error::InvalidTag { .. }), "{e:?}");
    }
}

#[test]
fn extra_tags_survive_the_round_trip_in_order() {
    let (_dir, base) = tmp();
    let mut g = game_ucis(&["e2e4"]);
    for (k, v) in [
        ("Annotator", "me"),
        ("Zebra", "z"),
        ("Opening", "Sicilian"),
        ("Aaa", "a"),
    ] {
        g.tags_mut().set_extra(k, v).unwrap();
    }
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&g, GameFlags::empty()).unwrap();
    drop(w);
    assert!(db_game(&base, 1).same_content(&g));
}

#[test]
fn nul_in_a_comment_is_invalid_text() {
    let mut g = game_ucis(&["e2e4"]);
    let id = g.mainline().next().unwrap();
    if g.set_comment(id, "bad\0comment").is_ok() {
        let err = PreparedGame::from_game(&g, GameFlags::empty()).unwrap_err();
        assert!(matches!(err, Si4Error::InvalidText { .. }), "{err:?}");
    }
}

#[test]
fn failed_validation_leaves_the_database_untouched() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&commented(), GameFlags::empty()).unwrap();
    let before = (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"));
    assert!(
        w.add_game(&with_tags(41, 5, 3), GameFlags::empty())
            .is_err()
    );
    assert!(
        w.add_game(&game_of_record_len(131_072), GameFlags::empty())
            .is_err()
    );
    assert_eq!(w.len(), 1);
    drop(w);
    assert_eq!(
        before,
        (read(&base, "si4"), read(&base, "sn4"), read(&base, "sg4"))
    );
}

// ---------------------------------------------------------------- .stc

#[test]
fn stc_cache_is_removed_by_add_and_replace_but_not_by_flag_or_header_edits() {
    let (_dir, base) = tmp();
    let stc = ext(&base, "stc");
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&game_ucis(&["e2e4"]), GameFlags::empty())
        .unwrap();

    std::fs::write(&stc, b"cache").unwrap();
    w.set_flag(1, GameFlags::DELETE, FlagOp::Set).unwrap();
    w.set_description("x").unwrap();
    w.set_base_type(2).unwrap();
    w.set_auto_load(1).unwrap();
    w.set_custom_flag_description(1, "a").unwrap();
    assert!(stc.exists(), "flags and header edits keep the cache");

    w.add_game(&game_ucis(&["d2d4"]), GameFlags::empty())
        .unwrap();
    assert!(!stc.exists(), "add removes it");

    std::fs::write(&stc, b"cache").unwrap();
    w.replace_game(1, &game_ucis(&["c2c4"])).unwrap();
    assert!(!stc.exists(), "replace removes it");

    std::fs::write(&stc, b"cache").unwrap();
    let batch = [PreparedGame::from_game(&Game::new(), GameFlags::empty()).unwrap()];
    w.add_prepared_batch(&batch).unwrap();
    assert!(!stc.exists(), "batch removes it");
}

#[test]
fn missing_stc_is_not_an_error() {
    let (_dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    assert!(!ext(&base, "stc").exists());
    w.add_game(&Game::new(), GameFlags::empty()).unwrap();
    w.replace_game(1, &Game::new()).unwrap();
}

#[test]
fn no_temporary_files_remain_after_any_operation() {
    let (dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&commented(), GameFlags::empty()).unwrap();
    w.replace_game(1, &commented()).unwrap();
    w.set_flag(1, GameFlags::DELETE, FlagOp::Set).unwrap();
    w.set_description("d").unwrap();
    assert!(support::leftovers(dir.path()).is_empty());
}

// ---------------------------------------------------------------- crash consistency

/// Copies the three files of `from` over `to` (only the given extensions).
fn copy_exts(from: &Path, to: &Path, exts: &[&str]) {
    for e in exts {
        std::fs::copy(ext(from, e), ext(to, e)).unwrap();
    }
}

fn snapshot_dir(src: &Path) -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, base) = tmp();
    copy_exts(src, &base, &["si4", "sn4", "sg4"]);
    (dir, base)
}

fn assert_state(base: &Path, expected: &[Game], what: &str) {
    // Reader.
    let mut db = Si4Database::open(base).unwrap_or_else(|e| panic!("{what}: reader {e:?}"));
    assert_eq!(db.len() as usize, expected.len(), "{what}: game count");
    for (i, g) in expected.iter().enumerate() {
        let back = db.game(u32::try_from(i).unwrap() + 1).unwrap();
        assert!(g.same_content(&back), "{what}: game {}", i + 1);
    }
    drop(db);
    // Writer: opens, counts the same, and can keep working.
    let mut w = Si4Writer::open(base).unwrap_or_else(|e| panic!("{what}: writer {e:?}"));
    assert_eq!(w.len() as usize, expected.len(), "{what}: writer count");
    let extra = named("After", "Crash", "E", "S", "9");
    let n = w.add_game(&extra, GameFlags::empty()).unwrap();
    assert_eq!(n as usize, expected.len() + 1);
    drop(w);
    let mut db = Si4Database::open(base).unwrap();
    assert!(
        db.game(n).unwrap().same_content(&extra),
        "{what}: game added after"
    );
    for (i, g) in expected.iter().enumerate() {
        assert!(
            g.same_content(&db.game(u32::try_from(i).unwrap() + 1).unwrap()),
            "{what}: kept"
        );
    }
}

/// States of the disk after each step of the protocol (spec "Protocolo de
/// gravação segura"): `.sg4`, then `.sn4`, then the entry, then the header.
/// There is no fault-injection hook in the API, so the intermediate states
/// are composed from the files before and after the operation.
#[test]
fn disk_after_each_step_of_add_is_the_old_or_the_new_database() {
    let old_games = [commented(), named("Mid", "End", "Ev", "Si", "2")];
    let new_game = named("Brand", "New", "Fresh", "Place", "7");

    let (_a_dir, a_base) = tmp();
    let mut w = Si4Writer::create(&a_base, 1).unwrap();
    for g in &old_games {
        w.add_game(g, GameFlags::empty()).unwrap();
    }
    drop(w);
    let (_b_dir, b_base) = snapshot_dir(&a_base);
    let mut w = Si4Writer::open(&b_base).unwrap();
    w.add_game(&new_game, GameFlags::empty()).unwrap();
    drop(w);
    let mut new_games = old_games.to_vec();
    new_games.push(new_game);

    // step 2 done: record appended
    let (_d1, s1) = snapshot_dir(&a_base);
    copy_exts(&b_base, &s1, &["sg4"]);
    assert_state(&s1, &old_games, "after .sg4 append");

    // step 3 done: new .sn4 renamed in
    let (_d2, s2) = snapshot_dir(&a_base);
    copy_exts(&b_base, &s2, &["sg4", "sn4"]);
    assert_state(&s2, &old_games, "after .sn4 replace");

    // step 4 done: entry N+1 written, header still counts N
    let (_d3, s3) = snapshot_dir(&b_base);
    let mut si4 = read(&s3, "si4");
    si4[..HEADER_LEN].copy_from_slice(&read(&a_base, "si4")[..HEADER_LEN]);
    std::fs::write(ext(&s3, "si4"), si4).unwrap();
    assert_state(&s3, &old_games, "after entry write");

    // step 5 done: header committed
    let (_d4, s4) = snapshot_dir(&b_base);
    assert_state(&s4, &new_games, "after header write");
}

#[test]
fn disk_after_each_step_of_replace_is_the_old_or_the_new_database() {
    let old_games = [commented(), named("Mid", "End", "Ev", "Si", "2")];
    let replacement = named("Other", "Pair", "Else", "Where", "5");

    let (_a_dir, a_base) = tmp();
    let mut w = Si4Writer::create(&a_base, 1).unwrap();
    for g in &old_games {
        w.add_game(g, GameFlags::empty()).unwrap();
    }
    drop(w);
    let (_b_dir, b_base) = snapshot_dir(&a_base);
    let mut w = Si4Writer::open(&b_base).unwrap();
    w.replace_game(1, &replacement).unwrap();
    drop(w);
    let new_games = [replacement, old_games[1].clone()];

    let (_d1, s1) = snapshot_dir(&a_base);
    copy_exts(&b_base, &s1, &["sg4"]);
    assert_state(&s1, &old_games, "after .sg4 append");

    let (_d2, s2) = snapshot_dir(&a_base);
    copy_exts(&b_base, &s2, &["sg4", "sn4"]);
    assert_state(&s2, &old_games, "after .sn4 replace");

    let (_d3, s3) = snapshot_dir(&b_base);
    assert_state(&s3, &new_games, "after entry write");
}

#[test]
fn orphan_tmp_files_do_not_stop_the_database_and_are_replaced_by_the_next_write() {
    let (dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&commented(), GameFlags::empty()).unwrap();
    drop(w);
    std::fs::write(dir.path().join("games.sn4.tmp"), b"garbage").unwrap();
    std::fs::write(dir.path().join("games.si4.tmp"), b"garbage").unwrap();

    assert_eq!(Si4Database::open(&base).unwrap().len(), 1);
    let mut w = Si4Writer::open(&base).unwrap();
    w.add_game(&commented(), GameFlags::empty()).unwrap();
    drop(w);
    assert_eq!(Si4Database::open(&base).unwrap().len(), 2);
    assert!(
        !dir.path().join("games.sn4.tmp").exists(),
        "the .sn4 temporary is consumed by the rename"
    );
}

/// Fault injection that works on every platform: a directory squatting on the
/// temporary name makes the `.sn4` replacement fail after the record was
/// appended to the `.sg4`.
#[test]
fn failure_replacing_the_name_file_leaves_the_old_database_and_a_coherent_writer() {
    let (dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let first = commented();
    w.add_game(&first, GameFlags::empty()).unwrap();
    let (si4, sn4) = (read(&base, "si4"), read(&base, "sn4"));

    let squat = dir.path().join("games.sn4.tmp");
    std::fs::create_dir(&squat).unwrap();
    let err = w
        .add_game(&named("Never", "Seen", "E", "S", "1"), GameFlags::empty())
        .unwrap_err();
    assert!(matches!(err, Si4Error::Io { .. }), "{err:?}");

    // On disk: index and names are the old ones.
    assert_eq!(read(&base, "si4"), si4);
    assert_eq!(read(&base, "sn4"), sn4);
    // In memory: the writer did not keep the failed game or its names.
    assert_eq!(w.len(), 1);
    assert_eq!(
        w.names().find(mychess_db::si4::NameKind::Player, b"Never"),
        None
    );

    // The fault goes away; the same writer works.
    std::fs::remove_dir(&squat).unwrap();
    let again = named("Second", "Try", "E", "S", "1");
    assert_eq!(w.add_game(&again, GameFlags::empty()).unwrap(), 2);
    drop(w);
    let mut db = Si4Database::open(&base).unwrap();
    assert!(db.game(1).unwrap().same_content(&first));
    assert!(db.game(2).unwrap().same_content(&again));
    assert_eq!(db.len(), 2);
}

#[test]
fn failure_replacing_the_name_file_during_replace_keeps_the_old_game() {
    let (dir, base) = tmp();
    let mut w = Si4Writer::create(&base, 1).unwrap();
    let first = commented();
    w.add_game(&first, GameFlags::empty()).unwrap();
    let entry = support::entry_bytes(&base, 1);
    std::fs::create_dir(dir.path().join("games.sn4.tmp")).unwrap();

    assert!(w.replace_game(1, &named("X", "Y", "Z", "W", "1")).is_err());
    assert_eq!(support::entry_bytes(&base, 1), entry);
    assert_eq!(
        w.names().frequency(mychess_db::si4::NameKind::Player, 0),
        Some(1),
        "frequencies rolled back"
    );
    drop(w);
    assert!(db_game(&base, 1).same_content(&first));
}

#[test]
fn failure_creating_the_index_leaves_no_half_database() {
    let (dir, base) = tmp();
    std::fs::create_dir(dir.path().join("games.si4.tmp")).unwrap();
    assert!(Si4Writer::create(&base, 1).is_err());
    for e in ["si4", "sn4", "sg4"] {
        assert!(!ext(&base, e).exists(), ".{e} must not remain");
    }
}

// ---------------------------------------------------------------- misc

#[test]
fn shuffle_game_of_many_plies_round_trips() {
    let (_dir, base) = tmp();
    let g = shuffle(1300);
    let mut w = Si4Writer::create(&base, 1).unwrap();
    w.add_game(&g, GameFlags::empty()).unwrap();
    drop(w);
    assert!(db_game(&base, 1).same_content(&g));
}
