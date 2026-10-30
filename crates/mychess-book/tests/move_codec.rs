//! Contract tests for the 16-bit Polyglot move code (spec D1104).

mod common;

use common::mv;
use mychess_book::{code_to_text, decode_move, encode_move};
use mychess_core::{Move, Position};

/// Builds a code from the spec layout: bits 0-2 to-file, 3-5 to-rank, 6-8
/// from-file, 9-11 from-rank, 12-14 promotion.
fn raw(from: (u16, u16), to: (u16, u16), promo: u16) -> u16 {
    to.0 | (to.1 << 3) | (from.0 << 6) | (from.1 << 9) | (promo << 12)
}

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).expect("fen")
}

#[test]
fn simple_move_uses_the_documented_bit_layout() {
    let start = Position::new();
    assert_eq!(
        encode_move(mv(&start, "e2e4")),
        Some(raw((4, 1), (4, 3), 0))
    );
    assert_eq!(
        encode_move(mv(&start, "g1f3")),
        Some(raw((6, 0), (5, 2), 0))
    );
    assert_eq!(
        encode_move(mv(&start, "a2a3")),
        Some(raw((0, 1), (0, 2), 0))
    );
    // e2e4 spelled out: to=e4 -> 4 | 3<<3 = 0x1c; from=e2 -> (4|1<<3)<<6.
    assert_eq!(raw((4, 1), (4, 3), 0), 0x1c | (0x0c << 6));
}

#[test]
fn white_promotions_n_b_r_q_and_captures() {
    let p = pos("r3k3/1P6/8/8/8/8/8/4K3 w - - 0 1");
    for (suffix, bits) in [("n", 1u16), ("b", 2), ("r", 3), ("q", 4)] {
        let push = mv(&p, &format!("b7b8{suffix}"));
        let capture = mv(&p, &format!("b7a8{suffix}"));
        assert_eq!(
            encode_move(push),
            Some(raw((1, 6), (1, 7), bits)),
            "b7b8{suffix}"
        );
        assert_eq!(
            encode_move(capture),
            Some(raw((1, 6), (0, 7), bits)),
            "b7a8{suffix}"
        );
        assert_eq!(decode_move(&p, raw((1, 6), (1, 7), bits)), Some(push));
        assert_eq!(decode_move(&p, raw((1, 6), (0, 7), bits)), Some(capture));
    }
}

#[test]
fn black_promotions_n_b_r_q() {
    let p = pos("4k3/8/8/8/8/8/6p1/4K2R b - - 0 1");
    for (suffix, bits) in [("n", 1u16), ("b", 2), ("r", 3), ("q", 4)] {
        let push = mv(&p, &format!("g2g1{suffix}"));
        let capture = mv(&p, &format!("g2h1{suffix}"));
        assert_eq!(encode_move(push), Some(raw((6, 1), (6, 0), bits)));
        assert_eq!(encode_move(capture), Some(raw((6, 1), (7, 0), bits)));
        assert_eq!(decode_move(&p, raw((6, 1), (6, 0), bits)), Some(push));
    }
}

#[test]
fn exd8_queen_promotion_code_from_the_spec() {
    // 0x4d3b = Performance.bin promotion: e7 -> d8 = queen.
    let p = pos("3n4/4P3/8/8/8/8/8/k6K w - - 0 1");
    assert_eq!(0x4d3b, raw((4, 6), (3, 7), 4));
    let m = decode_move(&p, 0x4d3b).expect("legal exd8=Q");
    assert_eq!(encode_move(m), Some(0x4d3b));
    assert_eq!(code_to_text(0x4d3b).as_deref(), Some("e7d8q"));
}

#[test]
fn castling_is_encoded_as_king_takes_own_rook_for_both_colours_and_sides() {
    let both = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let white_short = mv(&both, "e1g1");
    let white_long = mv(&both, "e1c1");
    assert!(matches!(white_short, Move::Castle { .. }));
    assert_eq!(encode_move(white_short), Some(0x0107)); // e1h1
    assert_eq!(encode_move(white_long), Some(0x0100)); // e1a1
    assert_eq!(decode_move(&both, 0x0107), Some(white_short));
    assert_eq!(decode_move(&both, 0x0100), Some(white_long));

    let black = pos("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1");
    let black_short = mv(&black, "e8g8");
    let black_long = mv(&black, "e8c8");
    assert_eq!(encode_move(black_short), Some(raw((4, 7), (7, 7), 0))); // e8h8
    assert_eq!(encode_move(black_long), Some(raw((4, 7), (0, 7), 0))); // e8a8
    assert_eq!(encode_move(black_short), Some(0x0f3f));
    assert_eq!(encode_move(black_long), Some(0x0f38));
    assert_eq!(decode_move(&black, 0x0f3f), Some(black_short));
    assert_eq!(decode_move(&black, 0x0f38), Some(black_long));
}

#[test]
fn castling_destination_form_is_not_accepted() {
    let both = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    assert_eq!(decode_move(&both, raw((4, 0), (6, 0), 0)), None); // e1g1
    assert_eq!(decode_move(&both, raw((4, 0), (2, 0), 0)), None); // e1c1
}

#[test]
fn castling_code_without_the_right_is_not_a_legal_move() {
    let no_rights = pos("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
    // e1h1 would be "king takes own rook": not a legal move without the right.
    assert_eq!(decode_move(&no_rights, 0x0107), None);
    // With only the queen side, the king-side code must not resolve.
    let q_only = pos("r3k2r/8/8/8/8/8/8/R3K2R w Q - 0 1");
    assert_eq!(decode_move(&q_only, 0x0107), None);
    assert!(decode_move(&q_only, 0x0100).is_some());
}

#[test]
fn castling_text_form_is_king_takes_rook() {
    assert_eq!(code_to_text(0x0107).as_deref(), Some("e1h1"));
    assert_eq!(code_to_text(0x0100).as_deref(), Some("e1a1"));
    assert_eq!(code_to_text(0x0f3f).as_deref(), Some("e8h8"));
    assert_eq!(code_to_text(0x0f38).as_deref(), Some("e8a8"));
}

#[test]
fn zero_means_absent_and_never_decodes() {
    let start = Position::new();
    assert_eq!(decode_move(&start, 0), None);
    assert_eq!(code_to_text(0), None);
    assert_eq!(encode_move(Move::Null), None);
    // a1a1 is the all-zero code: no legal move may encode to it.
    for m in start.legal_moves() {
        assert_ne!(encode_move(m), Some(0));
    }
}

#[test]
fn invalid_codes_do_not_decode_or_panic() {
    let start = Position::new();
    for code in [0xffff, 0x0abc, 0x7000, 0x1c, 0x4d3b, raw((4, 1), (4, 3), 7)] {
        assert_eq!(decode_move(&start, code), None, "{code:#06x}");
    }
    // Promotion bits on a non-promotion move, and an unknown promotion piece.
    let e2e4 = raw((4, 1), (4, 3), 0);
    assert_eq!(decode_move(&start, e2e4), Some(mv(&start, "e2e4")));
    for promo in 1..=7 {
        assert_eq!(
            decode_move(&start, e2e4 | (promo << 12)),
            None,
            "promo {promo}"
        );
    }
    // Promotion without promotion bits is not the legal promotion move.
    let p = pos("3n4/4P3/8/8/8/8/8/k6K w - - 0 1");
    assert_eq!(decode_move(&p, raw((4, 6), (4, 7), 0)), None);
    for bits in [5u16, 6, 7] {
        assert_eq!(decode_move(&p, raw((4, 6), (4, 7), bits)), None);
    }
    // The top bit is unused and must not alias a move.
    assert_eq!(decode_move(&start, e2e4 | 0x8000), None);
}

#[test]
fn en_passant_is_origin_to_target_square() {
    let p = pos("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1");
    let ep = mv(&p, "e5d6");
    assert!(matches!(ep, Move::EnPassant { .. }));
    assert_eq!(encode_move(ep), Some(raw((4, 4), (3, 5), 0)));
    assert_eq!(decode_move(&p, raw((4, 4), (3, 5), 0)), Some(ep));
}

#[test]
fn chess960_positions_never_decode() {
    let p = Position::chess960_start(518).expect("start");
    assert_ne!(p.castling_mode(), Position::new().castling_mode());
    assert_eq!(decode_move(&p, raw((4, 1), (4, 3), 0)), None);
    let q = Position::chess960_start(0).expect("start");
    assert_eq!(decode_move(&q, raw((1, 1), (1, 2), 0)), None);
}

#[test]
fn legal_moves_of_varied_positions_round_trip() {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1",
        "r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R b KQkq - 0 1",
        "r3k2r/1P4P1/8/8/8/8/1p4p1/R3K2R w KQkq - 0 1",
        "r3k2r/1P4P1/8/8/8/8/1p4p1/R3K2R b kq - 0 1",
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        "4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1",
        "4k3/8/8/K2pP2r/8/8/8/8 w - d6 0 1",
        "8/P7/8/8/8/8/7p/k6K w - - 0 1",
    ];
    for fen in fens {
        let p = pos(fen);
        for m in p.legal_moves() {
            let code = encode_move(m).expect("encodable");
            assert_eq!(decode_move(&p, code), Some(m), "{fen}: {m:?}");
        }
    }
}
