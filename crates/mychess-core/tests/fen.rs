//! FEN reading and writing (spec rules 1-8 and acceptance criteria).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
mod common;
use common::{START_COUNT, fen, random_game, sq, start_position};
use mychess_core::{
    BoardProblem, CastlingSide, Color, EnPassantFen, FenError, IllegalPositionReason, Piece,
    Position, Role,
};
use proptest::prelude::*;
use rstest::rstest;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[test]
fn initial_position_fen_is_canonical() {
    assert_eq!(Position::new().fen(), START);
}

#[test]
fn full_fen_roundtrips_exactly() {
    let s = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 5 17";
    assert_eq!(fen(s).fen(), s);
}

#[test]
fn four_field_fen_defaults_clocks() {
    let p = fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -");
    assert_eq!(p.halfmove_clock(), 0);
    assert_eq!(p.fullmove_number(), 1);
    assert_eq!(p.fen(), START);
}

#[test]
fn two_field_fen_deduces_castling() {
    let p = fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w");
    assert_eq!(p.fen(), START);
}

#[test]
fn two_field_fen_deduces_only_rights_with_king_and_rook_in_place() {
    // White king on e1 with only the h1 rook; black king on e8 with only a8.
    let p = fen("r3k3/8/8/8/8/8/8/4K2R w");
    assert!(p.has_castling_right(Color::White, CastlingSide::KingSide));
    assert!(!p.has_castling_right(Color::White, CastlingSide::QueenSide));
    assert!(p.has_castling_right(Color::Black, CastlingSide::QueenSide));
    assert!(!p.has_castling_right(Color::Black, CastlingSide::KingSide));
    // King off e1: nothing deduced.
    let q = fen("4k3/8/8/8/8/8/8/R2K3R w");
    assert_eq!(q.fen().split(' ').nth(2), Some("-"));
}

#[test]
fn fen_without_slashes_is_accepted() {
    let flat = "rnbqkbnrpppppppp8888PPPPPPPPRNBQKBNR w KQkq - 0 1";
    assert_eq!(fen(flat).fen(), START);
}

#[test]
fn fen_with_extra_whitespace_is_accepted() {
    assert_eq!(
        fen("  rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR   w   KQkq  -  0  1 ").fen(),
        START
    );
}

#[test]
fn castling_dash_means_none_and_order_is_free() {
    let none = fen("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
    assert_eq!(none.fen(), "r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
    let shuffled = fen("r3k2r/8/8/8/8/8/8/R3K2R w qkQK - 0 1");
    assert_eq!(shuffled.fen(), "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
}

#[test]
fn castling_rights_inconsistent_with_board_are_dropped() {
    let p = fen("4k3/8/8/8/8/8/8/4K3 w KQkq - 0 1");
    assert_eq!(p.fen(), "4k3/8/8/8/8/8/8/4K3 w - - 0 1");
}

#[rstest]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR W KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR B KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR white")]
fn rejects_invalid_or_missing_turn(#[case] s: &str) {
    assert!(
        matches!(Position::from_fen(s), Err(FenError::InvalidTurn { .. })),
        "{s}"
    );
}

#[test]
fn rejects_empty_input() {
    assert!(Position::from_fen("").is_err());
    assert!(Position::from_fen("   ").is_err());
}

#[rstest]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNX w KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/9/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/0/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")]
#[case("rnbqkbnr/ppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR/8 w KQkq - 0 1")]
#[case("rnbqkbnrp/ppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBN w KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNRR w KQkq - 0 1")]
#[case("rnbqkbnr/pppppppp/é/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")]
fn rejects_malformed_board(#[case] s: &str) {
    assert!(
        matches!(Position::from_fen(s), Err(FenError::InvalidBoard { .. })),
        "{s}: {:?}",
        Position::from_fen(s)
    );
}

#[test]
fn board_error_reports_unexpected_character() {
    let e =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNX w - - 0 1").unwrap_err();
    match e {
        FenError::InvalidBoard { problem, .. } => {
            assert!(
                matches!(problem, BoardProblem::UnexpectedChar { ch: 'X', .. }),
                "{problem:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn board_error_reports_too_many_and_too_few_squares() {
    let many =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR1 w - - 0 1").unwrap_err();
    assert!(
        matches!(
            many,
            FenError::InvalidBoard {
                problem: BoardProblem::TooManySquares | BoardProblem::MisplacedSlash { .. },
                ..
            }
        ),
        "{many:?}"
    );
    let few =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBN w - - 0 1").unwrap_err();
    assert!(
        matches!(
            few,
            FenError::InvalidBoard {
                problem: BoardProblem::TooFewSquares { count: 63 },
                ..
            }
        ),
        "{few:?}"
    );
}

#[test]
fn accepts_leading_trailing_and_repeated_slashes_at_rank_boundaries() {
    // Spec rule 1 / Scid: a slash is fine whenever the count is a multiple of 8.
    for s in [
        "/rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "rnbqkbnr//pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "//rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR/ w KQkq - 0 1",
    ] {
        assert_eq!(fen(s).fen(), START, "{s}");
    }
}

#[test]
fn rejects_misplaced_slash() {
    let e =
        Position::from_fen("rnbqkbn/rpppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1").unwrap_err();
    assert!(
        matches!(
            e,
            FenError::InvalidBoard {
                problem: BoardProblem::MisplacedSlash { .. },
                ..
            }
        ),
        "{e:?}"
    );
}

fn illegal(s: &str) -> IllegalPositionReason {
    match Position::from_fen(s) {
        Err(FenError::IllegalPosition { reason }) => reason,
        other => panic!("{s}: expected IllegalPosition, got {other:?}"),
    }
}

#[test]
fn rejects_missing_white_king() {
    assert_eq!(
        illegal("4k3/8/8/8/8/8/8/8 w - - 0 1"),
        IllegalPositionReason::MissingKing(Color::White)
    );
}

#[test]
fn rejects_missing_black_king() {
    assert_eq!(
        illegal("8/8/8/8/8/8/8/4K3 w - - 0 1"),
        IllegalPositionReason::MissingKing(Color::Black)
    );
}

#[test]
fn rejects_two_kings_of_one_color() {
    assert_eq!(
        illegal("4k3/8/8/8/8/8/8/3KK3 w - - 0 1"),
        IllegalPositionReason::TooManyKings(Color::White)
    );
    assert_eq!(
        illegal("3kk3/8/8/8/8/8/8/4K3 w - - 0 1"),
        IllegalPositionReason::TooManyKings(Color::Black)
    );
}

#[test]
fn rejects_more_than_sixteen_pieces() {
    // White: king + 16 pawns... use 8 pawns + 8 knights + king = 17 pieces.
    let s = "4k3/8/8/8/8/NNNNNNNN/PPPPPPPP/4K3 w - - 0 1";
    assert_eq!(
        illegal(s),
        IllegalPositionReason::TooManyPieces(Color::White)
    );
}

#[test]
fn rejects_adjacent_kings() {
    assert_eq!(
        illegal("8/8/8/3kK3/8/8/8/8 w - - 0 1"),
        IllegalPositionReason::AdjacentKings
    );
    assert_eq!(
        illegal("8/8/8/8/8/8/3k4/3K4 b - - 0 1"),
        IllegalPositionReason::AdjacentKings
    );
}

#[rstest]
#[case("P3k3/8/8/8/8/8/8/4K3 w - - 0 1")]
#[case("4k3/8/8/8/8/8/8/p3K3 w - - 0 1")]
#[case("4k3/8/8/8/8/8/8/P3K3 w - - 0 1")]
#[case("p3k3/8/8/8/8/8/8/4K3 w - - 0 1")]
fn rejects_pawn_on_back_rank(#[case] s: &str) {
    assert_eq!(illegal(s), IllegalPositionReason::PawnOnBackRank, "{s}");
}

#[test]
fn rejects_side_not_to_move_giving_check() {
    // White rook gives check to the black king, but it is White's turn.
    assert_eq!(
        illegal("4k3/8/8/8/8/8/8/4RK2 w - - 0 1"),
        IllegalPositionReason::OpponentInCheck
    );
}

#[test]
fn accepts_side_to_move_in_check() {
    let p = fen("4k3/8/8/8/8/8/8/4RK2 b - - 0 1");
    assert!(p.is_check());
}

#[test]
fn accepts_double_check_even_if_unreachable() {
    // Spec choice: "impossible" checks are accepted (implementer note).
    let p = fen("4k3/8/8/8/1B6/8/8/4R1K1 b - - 0 1");
    assert!(p.is_check());
}

#[rstest]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQxq - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KK1 - 0 1")]
#[case("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w kqKQ- - 0 1")]
fn rejects_invalid_castling_field(#[case] s: &str) {
    assert!(
        matches!(Position::from_fen(s), Err(FenError::InvalidCastling { .. })),
        "{s}: {:?}",
        Position::from_fen(s)
    );
}

#[rstest]
#[case("e4")]
#[case("i3")]
#[case("e")]
#[case("e33")]
#[case("e6x")]
#[case("E3")]
#[case("3e")]
#[case("e1")]
#[case("e8")]
#[case("e2")]
fn rejects_invalid_en_passant_field(#[case] ep: &str) {
    let s = format!("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq {ep} 0 1");
    assert!(
        matches!(
            Position::from_fen(&s),
            Err(FenError::InvalidEnPassant { .. })
        ),
        "{s}: {:?}",
        Position::from_fen(&s)
    );
}

#[test]
fn en_passant_without_capturable_pawn_is_silently_dropped() {
    let p = fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq e6 0 1");
    assert_eq!(p.ep_square(), None);
    assert_eq!(p.fen(), START);
}

#[test]
fn en_passant_with_capturable_pawn_is_kept() {
    let s = "rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3";
    let p = fen(s);
    assert_eq!(p.ep_square(), Some(sq("d6")));
    assert_eq!(p.fen(), s);
}

#[test]
fn en_passant_square_on_wrong_side_rank_is_dropped_or_rejected_not_kept() {
    // Black to move with target on rank 6 does not make sense (spec rule 5):
    // the field is silently discarded.
    let p = Position::from_fen("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR b KQkq d6 0 3")
        .expect("a misplaced en passant square is not an error");
    assert_eq!(p.ep_square(), None);
}

#[rstest]
#[case("0", 0)]
#[case("5", 5)]
#[case("99", 99)]
fn halfmove_clock_is_read(#[case] hm: &str, #[case] expected: u32) {
    let p = fen(&format!("4k3/8/8/8/8/8/8/4K3 w - - {hm} 1"));
    assert_eq!(p.halfmove_clock(), expected);
}

#[test]
fn negative_halfmove_clock_is_treated_as_zero() {
    assert_eq!(fen("4k3/8/8/8/8/8/8/4K3 w - - -3 1").halfmove_clock(), 0);
}

#[rstest]
#[case("4k3/8/8/8/8/8/8/4K3 w - - 0 1", 0, 1)]
#[case("4k3/8/8/8/8/8/8/4K3 b - - 0 1", 1, 1)]
#[case("4k3/8/8/8/8/8/8/4K3 w - - 0 10", 18, 10)]
#[case("4k3/8/8/8/8/8/8/4K3 b - - 0 10", 19, 10)]
#[case("4k3/8/8/8/8/8/8/4K3 w - - 0 0", 0, 1)]
#[case("4k3/8/8/8/8/8/8/4K3 b - - 0 0", 1, 1)]
#[case("4k3/8/8/8/8/8/8/4K3 w - - 0 -4", 0, 1)]
#[case("4k3/8/8/8/8/8/8/4K3 b - - 0 -4", 1, 1)]
#[case("4k3/8/8/8/8/8/8/4K3 w", 0, 1)]
#[case("4k3/8/8/8/8/8/8/4K3 b", 1, 1)]
#[case("4k3/8/8/8/8/8/8/4K3 w - - 0", 0, 1)]
fn move_number_maps_to_ply(#[case] s: &str, #[case] ply: u32, #[case] fullmove: u32) {
    let p = fen(s);
    assert_eq!(p.ply(), ply, "{s}");
    assert_eq!(p.fullmove_number(), fullmove, "{s}");
}

#[test]
fn fen_output_move_number_is_ply_div_two_plus_one() {
    let p = fen("4k3/8/8/8/8/8/8/4K3 b - - 0 10");
    assert!(p.fen().ends_with(" 0 10"));
}

#[test]
fn non_numeric_clock_fields_are_errors() {
    // Divergence from Scid (atoi would accept): documented in the report.
    assert!(matches!(
        Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - x 1"),
        Err(FenError::InvalidHalfmoveClock { .. })
    ));
    assert!(matches!(
        Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 y"),
        Err(FenError::InvalidMoveNumber { .. })
    ));
}

#[test]
fn huge_numeric_fields_do_not_panic() {
    let _ = Position::from_fen(
        "4k3/8/8/8/8/8/8/4K3 w - - 99999999999999999999999 99999999999999999999999",
    );
    let p = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 4294967295");
    if let Ok(p) = p {
        let _ = p.fen();
        let _ = p.ply();
    }
}

#[test]
fn fen_after_e4_has_no_en_passant_without_adjacent_pawn() {
    let mut p = Position::new();
    let m = p.parse_san("e4", mychess_core::SanMode::Strict).unwrap().mv;
    p.play(m).unwrap();
    assert_eq!(
        p.fen(),
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1"
    );
    assert_eq!(
        p.fen_with(EnPassantFen::Always),
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
    );
}

#[rstest]
#[case("d4")]
#[case("f4")]
fn fen_after_e4_with_adjacent_black_pawn_has_e3(#[case] pawn: &str) {
    let base = match pawn {
        "d4" => "rnbqkbnr/ppp1pppp/8/8/3p4/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        _ => "rnbqkbnr/ppppp1pp/8/8/5p2/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    };
    let mut p = fen(base);
    let m = p.parse_san("e4", mychess_core::SanMode::Strict).unwrap().mv;
    p.play(m).unwrap();
    assert!(p.fen().split(' ').nth(3) == Some("e3"), "{}", p.fen());
    assert_eq!(p.fen(), p.fen_with(EnPassantFen::Adjacent));
    assert_eq!(p.fen(), p.fen_with(EnPassantFen::Always));
}

#[test]
fn fen_prints_target_when_adjacent_pawn_is_pinned_pseudo_legal() {
    // D1 (Scid compatible): an adjacent enemy pawn is enough, even when the
    // capture itself would be illegal (d4 is pinned on the d-file).
    let mut p = fen("3k4/8/8/8/3p4/8/4P3/3RK3 w - - 0 1");
    p.play(p.find_move(sq("e2"), sq("e4"), None).unwrap())
        .unwrap();
    assert_eq!(p.fen().split(' ').nth(3), Some("e3"), "{}", p.fen());
}

#[test]
fn piece_at_matches_fen_placement() {
    let p = Position::new();
    assert_eq!(
        p.piece_at(sq("e1")),
        Some(Piece {
            color: Color::White,
            role: Role::King
        })
    );
    assert_eq!(
        p.piece_at(sq("d8")),
        Some(Piece {
            color: Color::Black,
            role: Role::Queen
        })
    );
    assert_eq!(p.piece_at(sq("e4")), None);
}

#[test]
fn fen_roundtrip_of_castling_and_promotion_positions() {
    for s in [
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    ] {
        assert_eq!(fen(s).fen(), s);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn fen_roundtrips_for_random_games(start in 0..START_COUNT, choices in proptest::collection::vec(any::<u16>(), 0..120)) {
        let game = random_game(&start_position(start), &choices);
        for (pos, _) in &game {
            let text = pos.fen();
            let again = Position::from_fen(&text).expect("own FEN must parse");
            prop_assert_eq!(again.fen(), text.clone());
            prop_assert_eq!(again.legal_moves().len(), pos.legal_moves().len(), "{}", text);
            prop_assert_eq!(again.zobrist(), pos.zobrist());
            let always = pos.fen_with(EnPassantFen::Always);
            let a = Position::from_fen(&always).expect("always-FEN must parse");
            prop_assert_eq!(a.fen(), text.clone());
        }
    }

    #[test]
    fn from_fen_never_panics_on_arbitrary_text(s in "\\PC{0,200}") {
        let _ = Position::from_fen(&s);
    }

    #[test]
    fn from_fen_never_panics_on_fenlike_text(s in "[rnbqkpRNBQKP1-8/ wbKQkq\\-a-h36 0-9x]{0,120}") {
        let _ = Position::from_fen(&s);
    }
}

#[test]
fn position_equality_and_hash_ignore_the_clocks() {
    use std::collections::HashSet;
    let a = fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1");
    let b = fen("4k3/8/8/8/8/8/4P3/4K3 w - - 17 40");
    assert_eq!(a, b);
    assert_ne!(a.halfmove_clock(), b.halfmove_clock());
    assert_eq!([a, b].into_iter().collect::<HashSet<_>>().len(), 1);
}
