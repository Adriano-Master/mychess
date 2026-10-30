//! Property: for every legal move of random positions (standard and Chess960, castling
//! included) `parse_uci(uci(m)) == m`, `parse_san(san(m)) == m` and the FEN round-trips.
//!
//! Spec: `docs/specs/m1-5-chess960-fechamento.md` ("Propriedades"). The standard-chess
//! variants live in `uci.rs` and `san.rs`; the Chess960 ones are here. The strategies are
//! local (equivalent to `test_support::arb_position`) so the test runs without the
//! `test-support` feature.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mychess_core::{EnPassantFen, FenCastlingStyle, Position, SanMode};
use proptest::prelude::*;

fn playout(start: Position, choices: &[u16]) -> Vec<Position> {
    let mut out = vec![start.clone()];
    let mut pos = start;
    for c in choices {
        let moves = pos.legal_moves();
        if moves.is_empty() {
            break;
        }
        let m = moves[usize::from(*c) % moves.len()];
        // Bias towards castling so that it is exercised in long playouts.
        let m = moves
            .iter()
            .copied()
            .find(|m| m.is_castle() && c % 4 == 0)
            .unwrap_or(m);
        pos = pos.after(m).unwrap();
        out.push(pos.clone());
    }
    out
}

fn check_all_moves(pos: &Position) -> Result<usize, TestCaseError> {
    let mut castles = 0;
    for m in pos.legal_moves() {
        castles += usize::from(m.is_castle());
        let uci = pos.uci(m);
        prop_assert_eq!(pos.parse_uci(&uci), Ok(m), "uci {} in {}", uci, pos.fen());
        let san = pos.san(m).unwrap();
        prop_assert_eq!(
            pos.parse_san(&san, SanMode::Strict).map(|p| p.mv),
            Ok(m),
            "strict san {} in {}",
            san,
            pos.fen()
        );
        prop_assert_eq!(
            pos.parse_san(&san, SanMode::Tolerant).map(|p| p.mv),
            Ok(m),
            "tolerant san {} in {}",
            san,
            pos.fen()
        );
    }
    Ok(castles)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn uci_and_san_roundtrip_for_every_legal_move_from_chess960_starts(
        n in 0u16..960,
        choices in proptest::collection::vec(any::<u16>(), 0..90),
    ) {
        let start = Position::chess960_start(n).unwrap();
        for pos in playout(start, &choices) {
            check_all_moves(&pos)?;
        }
    }

    #[test]
    fn fen_roundtrips_in_every_style_along_chess960_playouts(
        n in 0u16..960,
        choices in proptest::collection::vec(any::<u16>(), 0..90),
    ) {
        let start = Position::chess960_start(n).unwrap();
        for pos in playout(start, &choices) {
            for style in [FenCastlingStyle::XFen, FenCastlingStyle::Shredder] {
                let fen = pos.fen_with_options(EnPassantFen::Adjacent, style);
                let back = Position::from_fen(&fen).unwrap();
                prop_assert_eq!(&back, &pos, "{}", fen);
                prop_assert_eq!(back.zobrist(), pos.zobrist());
            }
        }
    }

    #[test]
    fn standard_start_playouts_roundtrip_too(
        choices in proptest::collection::vec(any::<u16>(), 0..120),
    ) {
        for pos in playout(Position::new(), &choices) {
            check_all_moves(&pos)?;
            let back = Position::from_fen(&pos.fen()).unwrap();
            prop_assert_eq!(back, pos);
        }
    }
}

#[test]
fn castling_moves_appear_in_the_sampled_chess960_positions() {
    // Deterministic guard: the properties above would be weaker if castling never showed up.
    let mut seen = 0;
    for n in [0u16, 518, 100, 777, 959] {
        let choices: Vec<u16> = (0..60u16)
            .map(|i| i.wrapping_mul(7919).wrapping_add(n))
            .collect();
        for pos in playout(Position::chess960_start(n).unwrap(), &choices) {
            seen += check_all_moves(&pos).unwrap();
        }
    }
    assert!(seen > 0, "no castling move was generated in the sample");
}
