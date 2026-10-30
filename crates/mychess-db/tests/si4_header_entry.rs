//! Contract tests of the `.si4` header and entries (spec M3-1, criteria
//! "Cabeçalho" and "Entrada"; `docs/formatos/si4.md` sections 2-6), with
//! databases assembled by `common/si4_builder.rs`.
#![allow(
    clippy::single_match_else,
    clippy::decimal_bitwise_operands,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_lossless
)]

#[path = "common/si4_builder.rs"]
mod si4_builder;

use mychess_core::{Eco, GameResult, RatingType};
use mychess_db::si4::{
    ENTRY_LEN, GameFlags, HEADER_LEN, Header, IndexEntry, NameKind, Si4Database, Si4Error, Si4File,
};
use pretty_assertions::assert_eq;
use rstest::rstest;
use si4_builder::{
    EntrySpec, IndexHeader, Parts, default_names, index_file, open_parts, parts_from_records, rec,
};
use std::io::Cursor;

fn header_bytes(h: &IndexHeader) -> Vec<u8> {
    h.encode(0)
}

// ------------------------------------------------------------------ header

#[test]
fn header_reads_every_field_of_the_182_bytes() {
    let h = IndexHeader {
        base_type: 15,
        num_games: Some(0x01_02_03),
        auto_load: 0x00_00_07,
        description: b"My test base".to_vec(),
        custom_flags: [
            b"alpha".to_vec(),
            b"beta".to_vec(),
            b"gamma".to_vec(),
            b"delta".to_vec(),
            b"epsilon".to_vec(),
            b"12345678".to_vec(),
        ],
        ..IndexHeader::default()
    };
    let parsed = Header::parse(&header_bytes(&h)).unwrap();
    assert_eq!(parsed.base_type(), 15);
    assert_eq!(parsed.num_games(), 0x01_02_03);
    assert_eq!(parsed.auto_load_raw(), 7);
    assert_eq!(parsed.description(), b"My test base");
    let flags = parsed.custom_flag_descriptions();
    assert_eq!(flags[0], b"alpha");
    assert_eq!(flags[4], b"epsilon");
    assert_eq!(flags[5], b"12345678", "8 bytes plus the NUL");
}

#[test]
fn header_description_stops_at_first_nul_and_keeps_raw_bytes() {
    let mut bytes = header_bytes(&IndexHeader::default());
    bytes[20..26].copy_from_slice(b"ab\0cd\xE9");
    let parsed = Header::parse(&bytes).unwrap();
    assert_eq!(parsed.description(), b"ab");
    bytes[20..24].copy_from_slice(b"\xE9\xFF\xC3\xA9");
    bytes[24] = 0;
    assert_eq!(
        Header::parse(&bytes).unwrap().description(),
        b"\xE9\xFF\xC3\xA9"
    );
}

#[test]
fn header_with_fewer_than_182_bytes_is_truncated_at_every_length() {
    let full = header_bytes(&IndexHeader::default());
    assert_eq!(full.len(), HEADER_LEN);
    for len in 0..HEADER_LEN {
        let err = Header::parse(&full[..len]).unwrap_err();
        assert!(
            matches!(
                err,
                Si4Error::Truncated {
                    file: Si4File::Index,
                    ..
                }
            ),
            "length {len}: {err:?}"
        );
    }
}

#[rstest]
#[case::wrong_text(*b"Scid.xx\0")]
#[case::byte_seven_not_zero(*b"Scid.si\x01")]
#[case::lower_case(*b"scid.si\0")]
#[case::zeros([0; 8])]
#[case::sn4_magic(*b"Scid.sn\0")]
fn header_with_bad_magic_is_rejected(#[case] magic: [u8; 8]) {
    let h = IndexHeader {
        magic,
        ..IndexHeader::default()
    };
    assert!(matches!(
        Header::parse(&header_bytes(&h)),
        Err(Si4Error::BadMagic {
            file: Si4File::Index
        })
    ));
}

#[rstest]
#[case(0, true)]
#[case(300, true)]
#[case(399, true)]
#[case(401, false)]
#[case(500, false)]
#[case(0xFFFF, false)]
fn header_old_and_new_versions_are_distinct_errors(#[case] version: u16, #[case] old: bool) {
    let h = IndexHeader {
        version,
        ..IndexHeader::default()
    };
    let err = Header::parse(&header_bytes(&h)).unwrap_err();
    match (old, &err) {
        (true, Si4Error::OldVersion { found }) | (false, Si4Error::NewerVersion { found }) => {
            assert_eq!(*found, version);
        }
        _ => panic!("version {version}: unexpected {err:?}"),
    }
}

#[test]
fn header_version_400_is_accepted() {
    assert!(Header::parse(&header_bytes(&IndexHeader::default())).is_ok());
}

#[rstest]
#[case(0, Some(1))]
#[case(1, None)]
#[case(2, Some(1))]
#[case(3, Some(2))]
#[case(10, Some(9))]
#[case(11, Some(10))] // limited to the number of games (10)
#[case(0xFF_FFFF, Some(10))]
fn auto_load_interprets_the_raw_value(#[case] raw: u32, #[case] expected: Option<u32>) {
    let h = IndexHeader {
        auto_load: raw,
        ..IndexHeader::default()
    };
    let entries: Vec<Vec<u8>> = (0..10).map(|_| EntrySpec::new(0, 3).encode()).collect();
    let index = index_file(&h, &entries);
    let parsed = Header::parse(&index).unwrap();
    assert_eq!(parsed.auto_load_raw(), raw);
    assert_eq!(parsed.auto_load(), expected, "raw {raw}");
}

#[rstest]
#[case(0, Some("Unknown"))]
#[case(1, Some("Temporary database"))]
#[case(2, Some("Clipbase"))]
#[case(3, Some("PGN file"))]
#[case(4, Some("My games"))]
#[case(5, Some("Large database"))]
#[case(6, Some("Correspondence chess"))]
#[case(7, Some("Computer Chess"))]
#[case(8, Some("Index of games"))]
#[case(9, Some("Player collection"))]
#[case(10, Some("Tournament"))]
#[case(11, Some("Internet"))]
#[case(12, Some("GM games"))]
#[case(13, Some("IM games"))]
#[case(14, Some("Blitz games"))]
#[case(15, Some("Tactics"))]
#[case(16, Some("End games"))]
#[case(17, Some("Openings for White"))]
#[case(18, Some("Openings for Black"))]
#[case(19, Some("Openings for either color"))]
#[case(20, Some("Theory: 1.c4"))]
#[case(21, Some("Theory: 1.d4"))]
#[case(22, Some("Theory: 1.d4 d5"))]
#[case(23, Some("Theory: QGD"))]
#[case(24, Some("Theory: Slav"))]
#[case(25, Some("Theory: 1.d4 Nf6"))]
#[case(26, Some("Theory: 1.e4"))]
#[case(27, Some("Theory: Sicilian"))]
#[case(28, Some("Theory: Sicilian 1.e4 c5 2.Nd3 d6 3.d4"))]
#[case(29, Some("Theory: Caro-Kann"))]
#[case(30, Some("Theory: French"))]
#[case(31, Some("Theory: Open Games"))]
#[case(32, None)]
#[case(0xDEAD_BEEF, None)]
fn base_type_names_follow_the_table_and_raw_value_is_preserved(
    #[case] raw: u32,
    #[case] name: Option<&str>,
) {
    let h = IndexHeader {
        base_type: raw,
        ..IndexHeader::default()
    };
    let parsed = Header::parse(&header_bytes(&h)).unwrap();
    assert_eq!(parsed.base_type(), raw);
    assert_eq!(parsed.base_type_name(), name);
}

#[test]
fn index_shorter_than_declared_games_is_truncated() {
    let names = default_names();
    let entries: Vec<Vec<u8>> = (0..3)
        .map(|_| EntrySpec::new(0, 3).encode())
        .collect::<Vec<_>>();
    let good = index_file(&IndexHeader::default(), &entries);
    let sg4 = vec![0, 0, 15];
    let open = |index: Vec<u8>| {
        Si4Database::from_parts(index, &names, Cursor::new(sg4.clone()), "x.sg4".into())
    };
    assert!(open(good.clone()).is_ok(), "182 + 47 * 3 bytes is enough");
    assert_eq!(good.len(), HEADER_LEN + 3 * ENTRY_LEN);
    for cut in 1..=ENTRY_LEN {
        let err = open(good[..good.len() - cut].to_vec()).unwrap_err();
        assert!(
            matches!(
                err,
                Si4Error::Truncated {
                    file: Si4File::Index,
                    ..
                }
            ),
            "cut {cut}: {err:?}"
        );
    }
    // Declared count larger than the entries present.
    let h = IndexHeader {
        num_games: Some(4),
        ..IndexHeader::default()
    };
    assert!(matches!(
        open(index_file(&h, &entries)),
        Err(Si4Error::Truncated { .. })
    ));
    // Trailing bytes are tolerated (Scid does not check the size).
    let mut longer = good;
    longer.extend_from_slice(&[0xAA; 10]);
    assert_eq!(open(longer).unwrap().len(), 3);
}

#[test]
fn huge_declared_game_count_in_a_tiny_index_is_an_error_not_an_allocation() {
    let h = IndexHeader {
        num_games: Some(16_777_214),
        ..IndexHeader::default()
    };
    let index = index_file(&h, &[]);
    let r = Si4Database::from_parts(
        index,
        &default_names(),
        Cursor::new(Vec::<u8>::new()),
        "x.sg4".into(),
    );
    assert!(matches!(r, Err(Si4Error::Truncated { .. })));
}

#[test]
fn empty_database_opens_with_no_games() {
    let p = parts_from_records(&[]);
    let db = open_parts(&p).unwrap();
    assert_eq!((db.len(), db.is_empty()), (0, true));
    assert!(matches!(
        db.entry(1),
        Err(Si4Error::InvalidGameNumber { number: 1, len: 0 })
    ));
}

// ------------------------------------------------------------------ entry

fn parse(spec: &EntrySpec) -> IndexEntry {
    IndexEntry::parse(&spec.encode()).unwrap()
}

#[test]
fn entry_has_47_bytes_and_shorter_input_is_an_error() {
    let bytes = EntrySpec::new(0, 3).encode();
    assert_eq!(bytes.len(), ENTRY_LEN);
    for len in 0..ENTRY_LEN {
        assert!(IndexEntry::parse(&bytes[..len]).is_err(), "length {len}");
    }
}

#[test]
fn entry_decodes_ids_with_their_high_bits_in_the_right_places() {
    let mut s = EntrySpec::new(0x1234_5678, 0x0_ABCD);
    s.white = 0xABCDE; // 20 bits
    s.black = 0x12345;
    s.event = 0x5A5A5; // 19 bits
    s.site = 0x3C3C3;
    s.round = 0x2B2B2; // 18 bits
    let e = parse(&s);
    assert_eq!(
        (e.white_id, e.black_id, e.event_id, e.site_id, e.round_id),
        (0xABCDE, 0x12345, 0x5A5A5, 0x3C3C3, 0x2B2B2)
    );
    assert_eq!((e.offset, e.length), (0x1234_5678, 0xABCD));
}

#[test]
fn entry_decodes_the_maximum_ids_offset_and_length() {
    let mut s = EntrySpec::new(u32::MAX, 131_071);
    s.white = 0xF_FFFF;
    s.black = 0xF_FFFF;
    s.event = 0x7_FFFF;
    s.site = 0x7_FFFF;
    s.round = 0x3_FFFF;
    let e = parse(&s);
    assert_eq!(e.offset, u32::MAX);
    assert_eq!(e.length, 131_071);
    assert_eq!(e.white_id, 1_048_575);
    assert_eq!(e.black_id, 1_048_575);
    assert_eq!(e.event_id, 524_287);
    assert_eq!(e.site_id, 524_287);
    assert_eq!(e.round_id, 262_143);
}

#[test]
fn entry_length_uses_bit_seven_of_length_high_as_bit_16() {
    for length in [0u32, 1, 0xFFFF, 0x1_0000, 0x1_0001, 131_071] {
        assert_eq!(parse(&EntrySpec::new(0, length)).length, length);
    }
    // Raw check: Length_High = 0x80 and Length_Low = 0 is 65536.
    let mut raw = EntrySpec::new(0, 0).encode();
    raw[6] = 0x80;
    let e = IndexEntry::parse(&raw).unwrap();
    assert_eq!(e.length, 65_536);
    assert!(e.flags.is_empty(), "bit 7 is not a flag");
}

#[test]
fn entry_preserves_bit_six_of_length_high_and_does_not_make_it_a_flag() {
    let mut s = EntrySpec::new(0, 10);
    s.spare = true;
    let e = parse(&s);
    assert!(e.length_spare_bit);
    assert!(e.flags.is_empty());
    assert_eq!(e.length, 10);
}

#[test]
fn entry_has_22_flags_in_the_documented_bits() {
    for bit in 0..16u32 {
        let mut s = EntrySpec::new(0, 3);
        s.flags = 1 << bit;
        assert_eq!(parse(&s).flags.bits(), 1 << bit, "Flags bit {bit}");
    }
    for bit in 0..6u32 {
        let mut s = EntrySpec::new(0, 3);
        s.custom_flags = 1 << bit;
        assert_eq!(
            parse(&s).flags.bits(),
            1 << (16 + bit),
            "Length_High bit {bit}"
        );
    }
    let mut all = EntrySpec::new(0, 3);
    all.flags = 0xFFFF;
    all.custom_flags = 0x3F;
    let e = parse(&all);
    assert_eq!(e.flags.bits(), 0x3F_FFFF);
    assert_eq!(e.flags, GameFlags::all());
    assert!(
        e.flags
            .contains(GameFlags::START | GameFlags::CUSTOM6 | GameFlags::DELETE)
    );
}

#[test]
fn entry_flag_names_match_the_documented_bits() {
    let table = [
        (GameFlags::START, 0),
        (GameFlags::PROMOTION, 1),
        (GameFlags::UNDER_PROMOTION, 2),
        (GameFlags::DELETE, 3),
        (GameFlags::WHITE_OPENING, 4),
        (GameFlags::BLACK_OPENING, 5),
        (GameFlags::MIDDLEGAME, 6),
        (GameFlags::ENDGAME, 7),
        (GameFlags::NOVELTY, 8),
        (GameFlags::PAWN_STRUCTURE, 9),
        (GameFlags::TACTICS, 10),
        (GameFlags::KINGSIDE, 11),
        (GameFlags::QUEENSIDE, 12),
        (GameFlags::BRILLIANCY, 13),
        (GameFlags::BLUNDER, 14),
        (GameFlags::USER, 15),
        (GameFlags::CUSTOM1, 16),
        (GameFlags::CUSTOM6, 21),
    ];
    for (flag, bit) in table {
        assert_eq!(flag.bits(), 1 << bit, "{flag:?}");
    }
}

#[rstest]
#[case(0, GameResult::Unknown)]
#[case(1, GameResult::White)]
#[case(2, GameResult::Black)]
#[case(3, GameResult::Draw)]
fn entry_result_codes_0_to_3(#[case] code: u8, #[case] expected: GameResult) {
    let mut s = EntrySpec::new(0, 3);
    s.result = code;
    assert_eq!(parse(&s).result, expected);
}

#[test]
fn entry_result_codes_4_to_15_are_errors() {
    for code in 4..=15u8 {
        let mut s = EntrySpec::new(0, 3);
        s.result = code;
        assert!(IndexEntry::parse(&s.encode()).is_err(), "result {code}");
    }
}

#[test]
fn invalid_result_fails_only_that_game_not_the_open() {
    let mut bad = EntrySpec::new(0, 3);
    bad.result = 9;
    let entries = vec![
        EntrySpec::new(0, 3).encode(),
        EntrySpec::new(0, 3).encode(),
        bad.encode(),
    ];
    let mut db = Si4Database::from_parts(
        index_file(&IndexHeader::default(), &entries),
        &default_names(),
        Cursor::new(vec![0, 0, 15]),
        "x.sg4".into(),
    )
    .unwrap();
    assert_eq!(db.len(), 3);
    assert!(db.entry(2).is_ok());
    assert!(matches!(
        db.entry(3),
        Err(Si4Error::CorruptIndex { game: 3, .. })
    ));
    assert!(matches!(
        db.game(3),
        Err(Si4Error::CorruptIndex { game: 3, .. })
    ));
    assert!(matches!(
        db.raw_game(3),
        Err(Si4Error::CorruptIndex { game: 3, .. })
    ));
    assert!(matches!(
        db.decode_mainline(3),
        Err(Si4Error::CorruptIndex { game: 3, .. })
    ));
    assert!(db.game(1).is_ok());
}

#[test]
fn entry_counts_use_the_non_linear_scale() {
    let scale = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 30, 40, 50];
    for (code, expected) in scale.iter().enumerate() {
        let mut s = EntrySpec::new(0, 3);
        s.variations = code as u8;
        s.comments = code as u8;
        s.nags = code as u8;
        let e = parse(&s);
        assert_eq!(
            (e.variation_count, e.comment_count, e.nag_count),
            (*expected, *expected, *expected),
            "code {code}"
        );
    }
}

#[test]
fn entry_counts_do_not_leak_into_each_other() {
    let mut s = EntrySpec::new(0, 3);
    s.variations = 1;
    s.comments = 2;
    s.nags = 3;
    s.result = 2;
    let e = parse(&s);
    assert_eq!((e.variation_count, e.comment_count, e.nag_count), (1, 2, 3));
    assert_eq!(e.result, GameResult::Black);
}

#[test]
fn entry_elo_is_limited_to_4000_and_the_type_is_decoded() {
    for (raw, expected) in [
        (0u16, 0u16),
        (1, 1),
        (2750, 2750),
        (4000, 4000),
        (4001, 4000),
        (4095, 4000),
    ] {
        let mut s = EntrySpec::new(0, 3);
        s.white_elo = raw;
        s.black_elo = raw;
        let e = parse(&s);
        assert_eq!(
            (e.white_elo, e.black_elo),
            (expected, expected),
            "raw {raw}"
        );
    }
}

#[test]
fn entry_rating_types_0_to_6_are_known_and_7_to_15_are_reserved() {
    let kinds = [
        RatingType::Elo,
        RatingType::Rating,
        RatingType::Rapid,
        RatingType::Iccf,
        RatingType::Uscf,
        RatingType::Dwz,
        RatingType::Ecf,
    ];
    for t in 0..=15u8 {
        let mut s = EntrySpec::new(0, 3);
        s.white_elo = 2500;
        s.white_type = t;
        s.black_elo = 1800;
        s.black_type = t;
        let e = parse(&s);
        assert_eq!(
            (e.white_rating_type, e.black_rating_type),
            (t, t),
            "raw value kept"
        );
        match kinds.get(usize::from(t)) {
            Some(kind) => {
                let w = e.white_rating().unwrap();
                assert_eq!((w.value.get(), w.kind), (2500, *kind));
                assert_eq!(e.black_rating().unwrap().kind, *kind);
            }
            None => {
                assert!(e.white_rating().is_none(), "type {t} is reserved");
                assert!(e.black_rating().is_none());
            }
        }
    }
}

#[test]
fn entry_zero_elo_gives_no_rating() {
    let mut s = EntrySpec::new(0, 3);
    s.white_elo = 0;
    s.black_elo = 0;
    let e = parse(&s);
    assert!(e.white_rating().is_none() && e.black_rating().is_none());
}

fn date_bits(year: u32, month: u32, day: u32) -> u32 {
    (year << 9) | (month << 5) | day
}

#[test]
fn entry_game_date_unknown_fields_are_zero() {
    for (bits, text) in [
        (date_bits(0, 0, 0), "????.??.??"),
        (date_bits(1999, 0, 0), "1999.??.??"),
        (date_bits(1999, 12, 0), "1999.12.??"),
        (date_bits(1999, 12, 31), "1999.12.31"),
        (date_bits(0, 5, 7), "????.05.07"),
        (date_bits(2047, 12, 31), "2047.12.31"),
        (date_bits(1, 1, 1), "0001.01.01"),
    ] {
        let mut s = EntrySpec::new(0, 3);
        s.game_date = bits;
        assert_eq!(parse(&s).date.to_string(), text, "bits {bits:#x}");
    }
}

#[test]
fn entry_game_date_with_an_impossible_month_becomes_unknown_without_panic() {
    for month in 13..=15 {
        let mut s = EntrySpec::new(0, 3);
        s.game_date = date_bits(1999, month, 5);
        let d = parse(&s).date;
        assert_eq!(d.year(), Some(1999));
        assert_eq!(d.month(), None, "month {month}");
    }
}

#[test]
fn entry_event_date_offset_zero_means_no_event_date() {
    let mut s = EntrySpec::new(0, 3);
    s.event_date = (5 << 5) | 0x11; // month and day, but off = 0
    assert!(parse(&s).event_date.is_unknown());
}

#[test]
fn entry_event_date_offsets_1_to_7_are_minus_3_to_plus_3_years() {
    for off in 1..=7u32 {
        let mut s = EntrySpec::new(0, 3);
        s.game_date = date_bits(1999, 6, 15);
        s.event_date = (off << 9) | (3 << 5) | 0x1C;
        let d = parse(&s).event_date;
        let expected_year = 1999 + off - 4;
        assert_eq!(
            d.year(),
            Some(u16::try_from(expected_year).unwrap()),
            "off {off}"
        );
        assert_eq!((d.month(), d.day()), (Some(3), Some(28)));
    }
}

#[test]
fn entry_event_date_with_unknown_month_and_day() {
    let mut s = EntrySpec::new(0, 3);
    s.game_date = date_bits(2001, 1, 1);
    s.event_date = 5 << 9; // off 5 = +1 year, month and day unknown
    let d = parse(&s).event_date;
    assert_eq!((d.year(), d.month(), d.day()), (Some(2002), None, None));
}

#[test]
fn entry_eco_basic_and_extended_codes() {
    // basic = 1 + 131 * (100 * (letter - 'A') + number)
    let cases = [
        (0u16, None),
        (1, Some("A00")),
        (1 + 131 * 99, Some("A99")),
        (1 + 131 * 100, Some("B00")),
        (1 + 131 * 242, Some("C42")),
        (1 + 131 * 499, Some("E99")),
        // extension: + 1 + 5 * (letter - 'a') + digit
        (1 + 131 * 242 + 1, Some("C42a")),
        (1 + 131 * 242 + 4, Some("C42a3")),
        (1 + 131 * 242 + 1 + 5, Some("C42b")),
        (1 + 131 * 242 + 1 + 5 * 25 + 4, Some("C42z4")),
    ];
    for (code, expected) in cases {
        let mut s = EntrySpec::new(0, 3);
        s.eco = code;
        let e = parse(&s);
        assert_eq!(e.eco_code, code);
        assert_eq!(
            e.eco().map(|eco| eco.to_string()).as_deref(),
            expected,
            "code {code}"
        );
    }
}

#[test]
fn entry_eco_codes_beyond_letter_e_are_not_valid_ecos_and_never_panic() {
    let mut s = EntrySpec::new(0, 3);
    for code in [1 + 131 * 500, 0xFFFF, 0xFFFE] {
        s.eco = code;
        let e = parse(&s);
        assert!(e.eco().is_none(), "code {code}");
    }
    // Whole range: decoding and re-encoding agree whenever a code is valid.
    for code in 0..=u16::MAX {
        if let Some(eco) = Eco::from_scid_code(code) {
            assert_eq!(eco.to_scid_code(), code);
        }
    }
}

#[test]
fn entry_material_signature_bit_positions() {
    // BP bit 0 (4 bits), BN 4, BB 6, BR 8, BQ 10, WP 12 (4 bits), WN 16, WB 18, WR 20, WQ 22.
    let sig: u32 = 5
        | (1 << 4)
        | (2 << 6)
        | (3 << 8)
        | (1 << 10)
        | (9 << 12)
        | (3 << 16)
        | (2 << 18)
        | (1 << 20)
        | (3 << 22);
    let mut s = EntrySpec::new(0, 3);
    s.final_mat = sig;
    s.stored_line = 0xA7;
    let e = parse(&s);
    let m = e.final_material();
    assert_eq!(
        (
            m.black_pawns,
            m.black_knights,
            m.black_bishops,
            m.black_rooks,
            m.black_queens
        ),
        (5, 1, 2, 3, 1)
    );
    assert_eq!(
        (
            m.white_pawns,
            m.white_knights,
            m.white_bishops,
            m.white_rooks,
            m.white_queens
        ),
        (9, 3, 2, 1, 3)
    );
    assert_eq!(e.final_mat_sig, sig);
    assert_eq!(e.stored_line_code, 0xA7);
}

#[test]
fn entry_material_signature_extremes() {
    let mut s = EntrySpec::new(0, 3);
    s.final_mat = 0xFF_FFFF;
    s.stored_line = 0xFF;
    let e = parse(&s);
    let m = e.final_material();
    assert_eq!((m.white_pawns, m.black_pawns), (15, 15));
    assert_eq!((m.white_queens, m.black_knights), (3, 3));
    assert_eq!(e.final_mat_sig, 0xFF_FFFF);
    assert_eq!(e.stored_line_code, 0xFF);
}

#[test]
fn entry_num_half_moves_uses_ten_bits_across_two_bytes() {
    for hm in [0u16, 1, 255, 256, 511, 512, 1000, 1023] {
        let mut s = EntrySpec::new(0, 3);
        s.half_moves = hm;
        s.home_pawn[0] = 63;
        let e = parse(&s);
        assert_eq!(e.num_half_moves, hm);
        assert_eq!(e.home_pawn_data[0], 63, "low six bits of byte 38");
    }
}

#[test]
fn entry_home_pawn_data_is_preserved_raw_and_changes_are_nibbles() {
    let mut s = EntrySpec::new(0, 3);
    s.half_moves = 700;
    s.home_pawn = [3, 0x12, 0x30, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45];
    let e = parse(&s);
    assert_eq!(e.home_pawn_data, s.home_pawn);
    assert_eq!(e.home_pawn_change_count(), 3);
    // first change in the high nibble of byte 1
    assert_eq!(e.home_pawn_changes().collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(e.num_half_moves, 700);
}

#[test]
fn entry_home_pawn_change_count_is_at_most_16() {
    let mut s = EntrySpec::new(0, 3);
    s.home_pawn = [63, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let e = parse(&s);
    assert!(e.home_pawn_change_count() <= 16);
    assert!(e.home_pawn_changes().count() <= 16);
}

#[test]
fn entries_roundtrip_through_a_database_for_every_field() {
    let mut s = EntrySpec::new(0, 5);
    s.white = 1;
    s.black = 0;
    s.flags = 0x8001;
    s.custom_flags = 0b10_0001;
    s.variations = 2;
    s.comments = 11;
    s.nags = 15;
    s.result = 3;
    s.game_date = date_bits(1985, 7, 9);
    s.white_elo = 2810;
    s.white_type = 5;
    s.half_moves = 321;
    let mut p: Parts = parts_from_records(&[rec(&[0xCF, 0xCF, 15], &[])]);
    let mut entries = vec![s.encode()];
    entries.push(EntrySpec::new(0, 5).encode());
    p.index = index_file(&IndexHeader::default(), &entries);
    let db = open_parts(&p).unwrap();
    assert_eq!(db.len(), 2);
    let e = db.entry(1).unwrap();
    assert_eq!((e.white_id, e.black_id), (1, 0));
    assert!(
        e.flags
            .contains(GameFlags::START | GameFlags::USER | GameFlags::CUSTOM1 | GameFlags::CUSTOM6)
    );
    assert_eq!(
        (e.variation_count, e.comment_count, e.nag_count),
        (2, 15, 50)
    );
    assert_eq!(e.result, GameResult::Draw);
    assert_eq!(e.date.to_string(), "1985.07.09");
    assert_eq!(
        (e.white_elo, e.white_rating_type, e.num_half_moves),
        (2810, 5, 321)
    );
    assert_eq!(db.entries().count(), 2);
    assert!(db.entries().all(|r| r.is_ok()));
    assert!(matches!(
        db.entry(0),
        Err(Si4Error::InvalidGameNumber { number: 0, .. })
    ));
    assert!(matches!(
        db.entry(3),
        Err(Si4Error::InvalidGameNumber { number: 3, len: 2 })
    ));
    assert_eq!(db.names().num_names(NameKind::Player), 2);
}
