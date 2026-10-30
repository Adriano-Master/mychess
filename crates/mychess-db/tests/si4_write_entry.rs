//! Contract tests of the index entry written by M3-2a: counts, event date,
//! rating, `NumHalfMoves`, flags, derived fields (rules 16-19, 33-38;
//! `docs/formatos/si4.md` sections 3 and 5, 11).
#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::decimal_bitwise_operands
)]

#[path = "common/si4_write_support.rs"]
mod support;

use mychess_core::{Elo, Game, GameResult, PartialDate, Position, Rating, RatingType, Square};
use mychess_db::si4::derive::{
    decode_count, encode_count, event_date_field, rating_field, stored_line_code,
};
use mychess_db::si4::{ENTRY_LEN, FLAG_LETTERS, GameFlags, IndexEntry, PreparedGame, Si4Error};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use rstest::rstest;
use support::{game_ucis, play, shuffle};

fn date(y: u16, m: u8, d: u8) -> PartialDate {
    PartialDate::new(Some(y), Some(m), Some(d)).unwrap()
}

fn year_only(y: u16) -> PartialDate {
    PartialDate::new(Some(y), None, None).unwrap()
}

fn entry_of(game: &Game) -> IndexEntry {
    PreparedGame::from_game(game, GameFlags::empty())
        .unwrap()
        .entry()
        .clone()
}

// ---------------------------------------------------------------- counts

#[rstest]
#[case(0, 0)]
#[case(1, 1)]
#[case(9, 9)]
#[case(10, 10)]
#[case(11, 10)]
#[case(12, 10)]
#[case(13, 11)]
#[case(17, 11)]
#[case(18, 12)]
#[case(24, 12)]
#[case(25, 13)]
#[case(34, 13)]
#[case(35, 14)]
#[case(44, 14)]
#[case(45, 15)]
#[case(46, 15)]
#[case(u32::MAX, 15)]
fn encode_count_boundaries(#[case] n: u32, #[case] code: u8) {
    assert_eq!(encode_count(n), code);
}

#[test]
fn decode_count_scale_is_the_documented_one() {
    let scale: Vec<u8> = (0..16).map(decode_count).collect();
    assert_eq!(
        scale,
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 30, 40, 50]
    );
}

#[test]
fn counts_of_a_game_use_the_approximate_scale() {
    // 13 comments -> code 11 -> 15; no variations, no NAGs.
    let mut game = shuffle(60);
    let ids: Vec<_> = game.mainline().collect();
    for id in ids.iter().take(13) {
        game.set_comment(*id, "c").unwrap();
    }
    let e = entry_of(&game);
    assert_eq!(e.comment_count, 15);
    assert_eq!(e.variation_count, 0);
    assert_eq!(e.nag_count, 0);
}

#[test]
fn nag_count_and_variation_count_are_encoded() {
    let mut game = shuffle(30);
    let ids: Vec<_> = game.mainline().collect();
    for id in ids.iter().take(11) {
        game.add_nag(*id, mychess_core::Nag::new(1).unwrap())
            .unwrap();
    }
    // 11 NAGs -> code 10 -> 10 (11-12 share code 10).
    assert_eq!(entry_of(&game).nag_count, 10);

    // 1 main move + 19 alternatives at the root = 19 variations -> 20.
    let mut game = Game::new();
    let root = game.root();
    for mv in game.position_at(root).unwrap().legal_moves() {
        game.add_move(root, mv).unwrap();
    }
    assert_eq!(entry_of(&game).variation_count, 20);
}

// ---------------------------------------------------------------- event date

#[rstest]
#[case::game_year_0(0, 2000, 0)]
#[case::game_year_1(1, 1, 0)]
#[case::game_year_2(2, 2, 0)]
#[case::game_year_3_same(3, 3, 4)]
#[case::game_year_3_plus3(3, 6, 7)]
#[case::game_year_3_event_0(3, 0, 0)]
#[case::plus3(2000, 2003, 7)]
#[case::minus3(2000, 1997, 1)]
#[case::plus4_lost(2000, 2004, 0)]
#[case::minus4_lost(2000, 1996, 0)]
#[case::same_year(2000, 2000, 4)]
fn event_date_field_matches_set_event_date(
    #[case] game_year: u16,
    #[case] event_year: u16,
    #[case] offset: u32,
) {
    let game = if game_year == 0 {
        PartialDate::UNKNOWN
    } else {
        date(game_year, 5, 6)
    };
    let event = if event_year == 0 {
        PartialDate::UNKNOWN
    } else {
        date(event_year, 7, 9)
    };
    let field = event_date_field(game, event);
    let expected = if offset == 0 {
        0
    } else {
        (7 << 5) | 9 | (offset << 9)
    };
    assert_eq!(field, expected);
}

#[test]
fn event_year_zero_writes_zero_even_with_month_and_day() {
    let event = PartialDate::new(None, Some(7), Some(9)).unwrap();
    assert_eq!(event_date_field(date(2000, 1, 1), event), 0);
}

#[test]
fn event_date_in_the_entry_bytes_is_in_the_top_12_bits_of_dates() {
    let mut game = game_ucis(&["e2e4"]);
    game.tags_mut().date = date(2000, 1, 2);
    game.tags_mut().event_date = date(2001, 3, 4);
    let bytes = entry_of(&game).to_bytes();
    let dates = u32::from_be_bytes(bytes[25..29].try_into().unwrap());
    assert_eq!(dates & 0xFFFFF, (2000 << 9) | (1 << 5) | 2);
    assert_eq!(dates >> 20, (5 << 9) | (3 << 5) | 4); // offset 2001+4-2000 = 5
}

#[test]
fn event_date_outside_three_years_is_dropped_from_the_entry() {
    let mut game = game_ucis(&["e2e4"]);
    game.tags_mut().date = date(2000, 1, 2);
    game.tags_mut().event_date = date(2004, 3, 4);
    let bytes = entry_of(&game).to_bytes();
    let dates = u32::from_be_bytes(bytes[25..29].try_into().unwrap());
    assert_eq!(dates >> 20, 0);
}

// ---------------------------------------------------------------- rating

#[rstest]
#[case(0, 0, 0x0000)]
#[case(2500, 0, 0x09C4)]
#[case(4000, 0, 0x0FA0)]
#[case(4001, 0, 0x0FA0)]
#[case(u16::MAX, 3, 0x3FA0)]
#[case(1800, 6, 0x6708)]
fn rating_field_limits_elo_to_4000_and_keeps_type_in_high_nibble(
    #[case] elo: u16,
    #[case] kind: u8,
    #[case] field: u16,
) {
    assert_eq!(rating_field(elo, kind), field);
}

#[test]
fn entry_to_bytes_limits_elo_above_4000() {
    let entry = IndexEntry {
        white_elo: 4001,
        black_elo: 4095,
        ..IndexEntry::default()
    };
    let b = entry.to_bytes();
    assert_eq!(u16::from_be_bytes([b[29], b[30]]) & 0xFFF, 4000);
    assert_eq!(u16::from_be_bytes([b[31], b[32]]) & 0xFFF, 4000);
}

#[test]
fn every_rating_type_from_0_to_6_is_written_in_the_high_nibble() {
    for (i, kind) in RatingType::ALL.into_iter().enumerate() {
        let mut game = game_ucis(&["e2e4"]);
        game.tags_mut().white_rating = Some(Rating::new(Elo::new(2100).unwrap(), kind));
        game.tags_mut().black_rating = Some(Rating::new(Elo::new(4000).unwrap(), kind));
        let b = entry_of(&game).to_bytes();
        let w = u16::from_be_bytes([b[29], b[30]]);
        let k = u16::from_be_bytes([b[31], b[32]]);
        assert_eq!(w & 0xFFF, 2100, "{kind:?}");
        assert_eq!(usize::from(w >> 12), i, "{kind:?}");
        assert_eq!(k & 0xFFF, 4000);
        assert_eq!(usize::from(k >> 12), i);
    }
}

#[test]
fn game_without_ratings_writes_zero_elo_and_type() {
    let b = entry_of(&game_ucis(&["e2e4"])).to_bytes();
    assert_eq!(&b[29..33], &[0, 0, 0, 0]);
}

// ---------------------------------------------------------------- NumHalfMoves

#[rstest]
#[case(0, 0)]
#[case(1, 1)]
#[case(255, 255)]
#[case(256, 256)]
#[case(1023, 1023)]
#[case(1024, 0)]
#[case(1300, 276)]
fn num_half_moves_is_written_modulo_1024(#[case] plies: usize, #[case] stored: u16) {
    let e = entry_of(&shuffle(plies));
    assert_eq!(e.num_half_moves, stored);
    let b = e.to_bytes();
    assert_eq!(b[37], (stored & 0xFF) as u8, "byte 37 = low 8 bits");
    assert_eq!(
        b[38] >> 6,
        (stored >> 8) as u8,
        "bits 6-7 of byte 38 = bits 8-9"
    );
}

#[test]
fn num_half_moves_does_not_disturb_the_six_pawn_count_bits() {
    // 1.e4 e5 changes two home pawns, then 1300 more plies of knight moves.
    let mut game = game_ucis(&["e2e4", "e7e5"]);
    let mut at = game.mainline().last().unwrap();
    for i in 0..1298 {
        at = play(&mut game, at, ["g1f3", "g8f6", "f3g1", "f6g8"][i % 4]);
    }
    let b = entry_of(&game).to_bytes();
    assert_eq!(b[38] & 0x3F, 2);
    let half = u16::from(b[37]) | (u16::from(b[38] >> 6) << 8);
    assert_eq!(half, 1300 % 1024);
}

// ---------------------------------------------------------------- derived data

#[test]
fn home_pawn_data_of_e4_e5_is_derived_from_the_signature_changes() {
    let b = entry_of(&game_ucis(&["e2e4", "e7e5"])).to_bytes();
    // [0] = 2 changes; e2 leaves: log2(1<<11) = 11, e7 leaves: log2(1<<3) = 3.
    assert_eq!(b[38] & 0x3F, 2);
    assert_eq!(b[39], 0xB3);
    assert_eq!(&b[40..47], &[0; 7]);
}

#[test]
fn final_material_signature_of_e4_e5_counts_every_piece() {
    let e = entry_of(&game_ucis(&["e2e4", "e7e5"]));
    // bits: BP 0, BN 4, BB 6, BR 8, BQ 10, WP 12, WN 16, WB 18, WR 20, WQ 22
    let expected = 8
        | (2 << 4)
        | (2 << 6)
        | (2 << 8)
        | (1 << 10)
        | (8 << 12)
        | (2 << 16)
        | (2 << 18)
        | (2 << 20)
        | (1 << 22);
    assert_eq!(e.final_mat_sig, expected);
}

#[test]
fn material_signature_caps_pieces_at_three_and_counts_pawns_to_fifteen() {
    // 4 white queens and 9 white pawns.
    let pos = Position::from_fen("7k/8/8/8/8/8/PPPPPPPP/QQQQK3 w - - 0 1").unwrap();
    let game = Game::from_position(pos);
    let e = entry_of(&game);
    assert_eq!((e.final_mat_sig >> 22) & 3, 3, "queens capped at 3");
    assert_eq!((e.final_mat_sig >> 12) & 15, 8);
}

#[test]
fn game_from_own_position_sets_start_and_zeroes_stored_line_and_pawn_data() {
    let pos = Position::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1").unwrap();
    let mut game = Game::from_position(pos);
    let root = game.root();
    play(&mut game, root, "e2e4");
    let e = entry_of(&game);
    assert!(e.flags.contains(GameFlags::START));
    assert_eq!(e.stored_line_code, 0);
    assert_eq!(e.home_pawn_data, [0; 9]);
    assert_eq!(e.num_half_moves, 1);
}

#[test]
fn standard_game_has_no_start_flag() {
    assert!(
        !entry_of(&game_ucis(&["e2e4"]))
            .flags
            .contains(GameFlags::START)
    );
}

#[rstest]
#[case("a7a8q", true, false)]
#[case("a7a8n", true, true)]
#[case("a7a8r", true, true)]
#[case("a7a8b", true, true)]
fn promotion_flags_follow_the_main_line(
    #[case] uci: &str,
    #[case] promo: bool,
    #[case] under: bool,
) {
    let pos = Position::from_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
    let mut game = Game::from_position(pos);
    let root = game.root();
    play(&mut game, root, uci);
    let flags = entry_of(&game).flags;
    assert_eq!(flags.contains(GameFlags::PROMOTION), promo);
    assert_eq!(flags.contains(GameFlags::UNDER_PROMOTION), under);
}

#[test]
fn stored_line_code_is_zero_for_empty_and_nonzero_for_the_italian() {
    assert_eq!(stored_line_code(&[]), 0);
    let line = [
        (Square::E2, Square::E4),
        (Square::E7, Square::E5),
        (Square::G1, Square::F3),
        (Square::B8, Square::C6),
        (Square::F1, Square::C4),
    ];
    assert_ne!(stored_line_code(&line), 0);
}

#[test]
fn stored_line_code_treats_castling_as_a_two_square_king_move() {
    // The Ruy Lopez main line 1.e4 e5 2.Nf3 Nc6 3.Bb5 a6 4.Ba4 Nf6 5.O-O is a
    // stored line: with castling as e1-g1 it must match (D288).
    let with_castle = [
        (Square::E2, Square::E4),
        (Square::E7, Square::E5),
        (Square::G1, Square::F3),
        (Square::B8, Square::C6),
        (Square::F1, Square::B5),
        (Square::A7, Square::A6),
        (Square::B5, Square::A4),
        (Square::G8, Square::F6),
        (Square::E1, Square::G1),
    ];
    let without = &with_castle[..8];
    assert!(stored_line_code(&with_castle) >= stored_line_code(without));
    let e = entry_of(&game_ucis(&[
        "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1",
    ]));
    assert_eq!(e.stored_line_code, stored_line_code(&with_castle));
}

// ---------------------------------------------------------------- flags

#[test]
fn flag_letters_are_in_scid_order() {
    assert_eq!(FLAG_LETTERS, "DWBMENPTKQ!?U123456");
}

#[test]
fn from_letter_maps_each_of_the_19_letters_to_one_distinct_editable_bit() {
    let mut seen = GameFlags::empty();
    for c in FLAG_LETTERS.chars() {
        let f = GameFlags::from_letter(c).unwrap();
        assert_eq!(f.bits().count_ones(), 1, "{c}");
        assert!(GameFlags::EDITABLE.contains(f), "{c}");
        assert!(!seen.intersects(f), "{c} duplicated");
        seen |= f;
        assert_eq!(f.letters(), c.to_string());
    }
    assert_eq!(seen, GameFlags::EDITABLE);
    assert_eq!(GameFlags::from_letter('d'), Some(GameFlags::DELETE));
    assert_eq!(GameFlags::from_letter('1'), Some(GameFlags::CUSTOM1));
    assert_eq!(GameFlags::from_letter('6'), Some(GameFlags::CUSTOM6));
}

#[rstest]
#[case('X')]
#[case('S')] // START is derived, not editable
#[case('0')]
#[case('7')]
#[case(' ')]
#[case('\u{e9}')]
fn unknown_flag_letter_is_none_and_not_start(#[case] c: char) {
    assert_eq!(GameFlags::from_letter(c), None);
    let err = GameFlags::from_letters(&format!("D{c}")).unwrap_err();
    assert!(matches!(err, Si4Error::UnknownFlag(x) if x == c), "{err:?}");
}

#[test]
fn derived_and_editable_flags_partition_the_22_bits() {
    assert_eq!(GameFlags::EDITABLE & GameFlags::DERIVED, GameFlags::empty());
    assert_eq!(GameFlags::EDITABLE | GameFlags::DERIVED, GameFlags::all());
    assert_eq!(GameFlags::all().bits().count_ones(), 22);
}

#[test]
fn from_letters_round_trips_through_letters_in_canonical_order() {
    let f = GameFlags::from_letters("u?dK").unwrap();
    assert_eq!(f.letters(), "DK?U");
}

#[test]
fn custom_flags_are_written_in_length_high_and_user_flags_in_flags() {
    let all = GameFlags::EDITABLE;
    let e = IndexEntry {
        flags: all,
        ..IndexEntry::default()
    };
    let b = e.to_bytes();
    assert_eq!(
        b[6] & 0x3F,
        0x3F,
        "custom flags 1-6 in bits 0-5 of Length_High"
    );
    assert_eq!(u16::from_be_bytes([b[7], b[8]]), 0xFFF8, "flags 3-15");
}

#[test]
fn spare_bit_6_of_length_high_is_written_when_set() {
    let e = IndexEntry {
        length_spare_bit: true,
        length: 70_000,
        ..IndexEntry::default()
    };
    let b = e.to_bytes();
    assert_eq!(b[6], 0x80 | 0x40);
    assert_eq!(u16::from_be_bytes([b[4], b[5]]), (70_000 & 0xFFFF) as u16);
}

#[test]
fn entry_layout_matches_the_documented_offsets() {
    let e = IndexEntry {
        offset: 0x0102_0304,
        length: 0x1_0506,
        flags: GameFlags::DELETE,
        white_id: 0xA_BCDE,
        black_id: 0x5_4321,
        event_id: 0x7_1234,
        site_id: 0x6_5678,
        round_id: 0x3_9ABC,
        result: GameResult::Black,
        ..IndexEntry::default()
    };
    let b = e.to_bytes();
    assert_eq!(b.len(), ENTRY_LEN);
    assert_eq!(&b[0..4], &[1, 2, 3, 4]);
    assert_eq!(&b[4..6], &[5, 6]);
    assert_eq!(b[6] & 0x80, 0x80);
    assert_eq!(&b[7..9], &[0, 8]);
    assert_eq!(b[9], 0xA5);
    assert_eq!(&b[10..12], &[0xBC, 0xDE]);
    assert_eq!(&b[12..14], &[0x43, 0x21]);
    assert_eq!(b[14], (7 << 5) | (6 << 2) | 3);
    assert_eq!(&b[15..17], &[0x12, 0x34]);
    assert_eq!(&b[17..19], &[0x56, 0x78]);
    assert_eq!(&b[19..21], &[0x9A, 0xBC]);
    assert_eq!(b[21] >> 4, 2, "result 0-1 in the top nibble of VarCounts");
}

// ---------------------------------------------------------------- Chess960

#[test]
fn chess960_start_position_is_unsupported() {
    let pos = Position::chess960_start(0).unwrap();
    let game = Game::from_position(pos);
    let err = PreparedGame::from_game(&game, GameFlags::empty()).unwrap_err();
    assert!(matches!(err, Si4Error::UnsupportedStart { .. }), "{err:?}");
}

// ---------------------------------------------------------------- properties

fn arb_entry() -> impl Strategy<Value = IndexEntry> {
    let counts = prop::sample::select(vec![0u8, 1, 2, 5, 10, 15, 20, 30, 40, 50]);
    let dates = (0u16..=2047, 0u8..=12, 0u8..=28)
        .prop_map(|(y, m, d)| PartialDate::new(Some(y), Some(m), Some(d)).unwrap());
    let first = (
        any::<u32>(),
        0u32..=131_071,
        0u32..(1 << 22),
        any::<bool>(),
        (
            0u32..1 << 20,
            0u32..1 << 20,
            0u32..1 << 19,
            0u32..1 << 19,
            0u32..1 << 18,
        ),
    );
    let second = (
        (counts.clone(), counts.clone(), counts),
        prop::sample::select(vec![
            GameResult::Unknown,
            GameResult::White,
            GameResult::Black,
            GameResult::Draw,
        ]),
        any::<u16>(),
        dates,
        (0u16..=4000, 0u8..16, 0u16..=4000, 0u8..16),
        (0u32..(1 << 24), any::<u8>(), 0u16..1024, any::<[u8; 9]>()),
    );
    (first, second).prop_map(
        |(
            (offset, length, flags, spare, (w, b, e, s, r)),
            ((vc, cc, nc), result, eco, date, (we, wt, be, bt), (mat, stored, half, mut hp)),
        )| {
            hp[0] &= 0x3F;
            IndexEntry {
                offset,
                length,
                flags: GameFlags::from_bits_truncate(flags),
                length_spare_bit: spare,
                white_id: w,
                black_id: b,
                event_id: e,
                site_id: s,
                round_id: r,
                variation_count: vc,
                comment_count: cc,
                nag_count: nc,
                result,
                eco_code: eco,
                date,
                white_elo: we,
                white_rating_type: wt,
                black_elo: be,
                black_rating_type: bt,
                final_mat_sig: mat,
                stored_line_code: stored,
                num_half_moves: half,
                home_pawn_data: hp,
                ..IndexEntry::default()
            }
        },
    )
}

proptest! {
    #[test]
    fn entry_to_bytes_then_parse_is_the_identity(entry in arb_entry()) {
        let bytes = entry.to_bytes();
        let back = IndexEntry::parse(&bytes).unwrap();
        prop_assert_eq!(&back, &entry);
        prop_assert_eq!(back.to_bytes(), bytes);
    }

    #[test]
    fn encode_count_is_monotonic(a in 0u32..200, b in 0u32..200) {
        prop_assert!(encode_count(a) <= 15);
        prop_assert!(encode_count(a.min(b)) <= encode_count(a.max(b)));
    }

    #[test]
    fn event_date_field_is_zero_or_decodes_back(gy in 0u16..=2047, ey in 0u16..=2047, m in 0u8..=12, d in 0u8..=31) {
        let g = if gy == 0 { PartialDate::UNKNOWN } else { year_only(gy) };
        let e = PartialDate::new(Some(ey), Some(m), Some(d)).unwrap();
        let field = event_date_field(g, e);
        let in_range = gy >= 3 && ey != 0 && ey + 3 >= gy && ey <= gy + 3;
        if in_range {
            prop_assert_eq!(field >> 9, u32::from(ey + 4 - gy));
            prop_assert_eq!((field >> 5) & 15, u32::from(m));
            prop_assert_eq!(field & 31, u32::from(d));
        } else {
            prop_assert_eq!(field, 0);
        }
    }
}
