//! UCI move strings (spec rules 25-26).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
mod common;
use common::{START_COUNT, fen, random_game, start_position};
use mychess_core::{Move, Position, Role, UciError};
use proptest::prelude::*;
use rstest::rstest;

const PROMO: &str = "3r3k/4P3/8/8/8/8/8/K7 w - - 0 1";

#[test]
fn uci_of_basic_moves() {
    let p = Position::new();
    assert_eq!(p.uci(p.parse_uci("e2e4").unwrap()), "e2e4");
    assert_eq!(p.uci(p.parse_uci("g1f3").unwrap()), "g1f3");
}

#[test]
fn uci_promotion_is_written_lowercase_for_every_piece() {
    let p = fen(PROMO);
    for (role, text) in [
        (Role::Queen, "e7e8q"),
        (Role::Rook, "e7e8r"),
        (Role::Bishop, "e7e8b"),
        (Role::Knight, "e7e8n"),
    ] {
        let m = p
            .find_move("e7".parse().unwrap(), "e8".parse().unwrap(), Some(role))
            .unwrap();
        assert_eq!(p.uci(m), text);
    }
    let cap = p.parse_uci("e7d8q").unwrap();
    assert_eq!(p.uci(cap), "e7d8q");
}

#[rstest]
#[case("e7e8q", Role::Queen)]
#[case("E7E8Q", Role::Queen)]
#[case("e7e8Q", Role::Queen)]
#[case("e7e8N", Role::Knight)]
#[case("e7e8n", Role::Knight)]
#[case("E7E8R", Role::Rook)]
#[case("e7e8B", Role::Bishop)]
fn uci_reads_both_cases(#[case] text: &str, #[case] role: Role) {
    let p = fen(PROMO);
    assert_eq!(p.parse_uci(text).unwrap().promotion(), Some(role), "{text}");
}

#[test]
fn uci_reads_uppercase_plain_move() {
    assert_eq!(
        Position::new().parse_uci("E2E4").unwrap(),
        Position::new().parse_uci("e2e4").unwrap()
    );
}

#[test]
fn uci_promotion_requires_letter() {
    let p = fen(PROMO);
    assert!(matches!(
        p.parse_uci("e7e8"),
        Err(UciError::MissingPromotion { .. })
    ));
}

#[test]
fn uci_promotion_letter_on_non_promoting_move_is_rejected() {
    let p = Position::new();
    assert!(p.parse_uci("e2e4q").is_err());
}

#[rstest]
#[case("e7e8k")]
#[case("e7e8p")]
#[case("e7e8x")]
#[case("e7e8qq")]
#[case("e7e")]
#[case("e7")]
#[case("")]
#[case("   ")]
#[case("i2i4")]
#[case("e2e9")]
#[case("e2 e4")]
#[case("é2e4")]
#[case("e2e4\u{0}")]
fn uci_rejects_malformed(#[case] text: &str) {
    let p = fen(PROMO);
    assert!(p.parse_uci(text).is_err(), "{text:?}");
}

#[test]
fn uci_illegal_move_is_illegal_not_syntax() {
    let p = Position::new();
    assert!(matches!(p.parse_uci("e2e5"), Err(UciError::Illegal { .. })));
    assert!(matches!(p.parse_uci("e7e5"), Err(UciError::Illegal { .. })));
    assert!(matches!(p.parse_uci("a1a1"), Err(UciError::Illegal { .. })));
}

#[test]
fn uci_castling_is_the_two_square_king_move() {
    let p = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let short = p.parse_uci("e1g1").unwrap();
    let long = p.parse_uci("e1c1").unwrap();
    assert!(short.is_castle() && long.is_castle());
    assert_eq!(p.uci(short), "e1g1");
    assert_eq!(p.uci(long), "e1c1");
    let b = fen("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1");
    assert_eq!(b.uci(b.parse_uci("e8g8").unwrap()), "e8g8");
    assert_eq!(b.uci(b.parse_uci("e8c8").unwrap()), "e8c8");
}

#[test]
fn uci_castling_without_right_is_illegal() {
    let p = fen("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
    assert!(matches!(p.parse_uci("e1g1"), Err(UciError::Illegal { .. })));
}

#[test]
fn uci_rook_move_is_not_confused_with_castling() {
    let p = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let m = p.parse_uci("h1g1").unwrap();
    assert!(!m.is_castle());
}

#[rstest]
#[case("0000")]
#[case("--")]
fn uci_null_move(#[case] text: &str) {
    assert_eq!(Position::new().parse_uci(text).unwrap(), Move::Null);
}

#[test]
fn uci_writes_null_move_as_zeros() {
    assert_eq!(Position::new().uci(Move::Null), "0000");
}

#[test]
fn uci_null_move_in_check_is_refused() {
    let p = fen("4k3/8/8/8/8/8/8/r3K3 w - - 0 1");
    assert!(matches!(
        p.parse_uci("0000"),
        Err(UciError::NullMoveInCheck { .. })
    ));
    assert!(matches!(
        p.parse_uci("--"),
        Err(UciError::NullMoveInCheck { .. })
    ));
}

#[test]
fn play_null_move_in_check_is_refused_and_position_unchanged() {
    let mut p = fen("4k3/8/8/8/8/8/8/r3K3 w - - 0 1");
    let before = p.fen();
    assert!(p.play(Move::Null).is_err());
    assert_eq!(p.fen(), before);
}

#[test]
fn null_move_swaps_turn_clears_ep_keeps_castling_and_advances_counters() {
    let mut p = fen("r3k2r/ppp1pppp/8/3pP3/8/8/PPPP1PPP/R3K2R w KQkq d6 3 7");
    p.play(Move::Null).unwrap();
    assert_eq!(
        p.fen(),
        "r3k2r/ppp1pppp/8/3pP3/8/8/PPPP1PPP/R3K2R b KQkq - 4 7"
    );
    p.play(Move::Null).unwrap();
    assert_eq!(
        p.fen(),
        "r3k2r/ppp1pppp/8/3pP3/8/8/PPPP1PPP/R3K2R w KQkq - 5 8"
    );
    assert_eq!(p.ply(), 12 + 2);
}

#[test]
fn null_move_via_after_leaves_original_untouched() {
    let p = Position::new();
    let q = p.after(Move::Null).unwrap();
    assert_eq!(p.fen(), Position::new().fen());
    assert_eq!(q.turn(), mychess_core::Color::Black);
}

#[test]
fn play_illegal_move_leaves_position_unchanged() {
    let mut p = Position::new();
    let bogus = Move::Normal {
        role: Role::Pawn,
        from: "e2".parse().unwrap(),
        to: "e5".parse().unwrap(),
        capture: None,
        promotion: None,
    };
    assert!(p.play(bogus).is_err());
    assert_eq!(p.fen(), Position::new().fen());
}

#[test]
fn clocks_after_moves() {
    let mut p = Position::new();
    for (u, clock, full) in [
        ("g1f3", 1, 1),
        ("g8f6", 2, 2),
        ("e2e4", 0, 2),
        ("f6e4", 0, 3),
        ("f3e5", 1, 3),
    ] {
        let m = p.parse_uci(u).unwrap();
        p.play(m).unwrap();
        assert_eq!(
            (p.halfmove_clock(), p.fullmove_number()),
            (clock, full),
            "after {u}"
        );
    }
}

#[test]
fn castling_rights_are_lost_by_king_rook_moves_and_rook_capture() {
    use mychess_core::{CastlingSide::*, Color::*};
    let base = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
    let mut p = fen(base);
    p.play(p.parse_uci("h1h2").unwrap()).unwrap();
    assert!(!p.has_castling_right(White, KingSide) && p.has_castling_right(White, QueenSide));
    let mut p = fen(base);
    p.play(p.parse_uci("e1e2").unwrap()).unwrap();
    assert!(!p.has_castling_right(White, KingSide) && !p.has_castling_right(White, QueenSide));
    assert!(p.has_castling_right(Black, KingSide));
    // capture of the rook on a8 removes Black's queenside right
    let mut p = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    p.play(p.parse_uci("a1a8").unwrap()).unwrap();
    assert!(!p.has_castling_right(Black, QueenSide) && p.has_castling_right(Black, KingSide));
}

#[test]
fn castling_through_attacked_square_or_out_of_check_is_illegal() {
    let through = fen("4k3/8/8/8/8/8/5r2/R3K2R w KQ - 0 1");
    assert!(through.parse_uci("e1g1").is_err());
    assert!(through.parse_uci("e1c1").is_ok());
    let in_check = fen("4k3/8/8/8/8/8/4r3/R3K2R w KQ - 0 1");
    assert!(in_check.parse_uci("e1g1").is_err());
    assert!(in_check.parse_uci("e1c1").is_err());
}

#[test]
fn castling_moves_the_rook() {
    let mut p = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    p.play(p.parse_uci("e1c1").unwrap()).unwrap();
    assert_eq!(p.fen(), "r3k2r/8/8/8/8/8/8/2KR3R b kq - 1 1");
}

#[test]
fn en_passant_removes_the_captured_pawn() {
    let mut p = fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
    let m = p.parse_uci("e5d6").unwrap();
    assert!(m.is_en_passant());
    p.play(m).unwrap();
    assert_eq!(p.fen(), "4k3/8/3P4/8/8/8/8/4K3 b - - 0 1");
}

#[test]
fn en_passant_that_exposes_the_king_is_not_generated() {
    // Classic horizontal pin: K a5, P b5, p c5 (just moved), r h5.
    let p = fen("4k3/8/8/KPp4r/8/8/8/8 w - c6 0 1");
    assert!(p.parse_uci("b5c6").is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn uci_roundtrips_for_every_legal_move(start in 0..START_COUNT, choices in proptest::collection::vec(any::<u16>(), 0..100)) {
        for (pos, _) in random_game(&start_position(start), &choices) {
            for m in pos.legal_moves() {
                let text = pos.uci(m);
                prop_assert_eq!(pos.parse_uci(&text), Ok(m), "{} {}", pos.fen(), text);
                prop_assert_eq!(pos.parse_uci(&text.to_uppercase()), Ok(m));
            }
        }
    }

    #[test]
    fn legal_moves_are_unique_and_playable(start in 0..START_COUNT, choices in proptest::collection::vec(any::<u16>(), 0..100)) {
        for (pos, _) in random_game(&start_position(start), &choices) {
            let moves = pos.legal_moves();
            let mut texts: Vec<_> = moves.iter().map(|m| pos.uci(*m)).collect();
            texts.sort();
            texts.dedup();
            prop_assert_eq!(texts.len(), moves.len());
            for m in &moves {
                prop_assert!(pos.is_legal(*m));
                prop_assert!(pos.after(*m).is_ok());
            }
            if let Some(king) = pos.king_square(pos.turn()) {
                let _ = king;
            }
            let by_square: usize = (0..64u32).map(|i| pos.legal_moves_from(mychess_core::Square::new(i)).len()).sum();
            prop_assert_eq!(by_square, moves.len());
        }
    }

    #[test]
    fn parse_uci_never_panics(s in "\\PC{0,100}") {
        let _ = Position::new().parse_uci(&s);
        let _ = fen(PROMO).parse_uci(&s);
    }

    #[test]
    fn parse_uci_never_panics_on_ucilike_text(s in "[a-hA-H1-8qrbnQRBN0\\-]{0,8}") {
        let _ = Position::new().parse_uci(&s);
        let _ = fen(PROMO).parse_uci(&s);
    }
}
