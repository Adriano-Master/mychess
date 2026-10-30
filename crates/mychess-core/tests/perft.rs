//! Perft through the `mychess-core` encapsulation: proves that no legal move
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
//! (en passant, castling, promotion) is lost or invented.
mod common;
use common::{fen, perft};
use mychess_core::Position;
use rstest::rstest;

const KIWIPETE: &str = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
const POS3: &str = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
const POS4: &str = "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1";
const POS4_MIRRORED: &str = "r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1";
const POS5: &str = "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8";
const POS6: &str = "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10";

#[rstest]
#[case(1, 20)]
#[case(2, 400)]
#[case(3, 8902)]
#[case(4, 197_281)]
fn perft_initial_position(#[case] depth: u32, #[case] nodes: u64) {
    assert_eq!(perft(&Position::new(), depth), nodes);
}

#[test]
#[ignore = "slow: 4.8M nodes"]
fn perft_initial_position_depth_5() {
    assert_eq!(perft(&Position::new(), 5), 4_865_609);
}

#[rstest]
#[case(1, 48)]
#[case(2, 2039)]
#[case(3, 97_862)]
fn perft_kiwipete(#[case] depth: u32, #[case] nodes: u64) {
    assert_eq!(perft(&fen(KIWIPETE), depth), nodes);
}

#[test]
#[ignore = "slow: 4M nodes"]
fn perft_kiwipete_depth_4() {
    assert_eq!(perft(&fen(KIWIPETE), 4), 4_085_603);
}

#[rstest]
#[case(1, 14)]
#[case(2, 191)]
#[case(3, 2812)]
#[case(4, 43_238)]
fn perft_position_3_en_passant_pins(#[case] depth: u32, #[case] nodes: u64) {
    assert_eq!(perft(&fen(POS3), depth), nodes);
}

#[rstest]
#[case(1, 6)]
#[case(2, 264)]
#[case(3, 9467)]
fn perft_position_4_promotions_and_castling(#[case] depth: u32, #[case] nodes: u64) {
    assert_eq!(perft(&fen(POS4), depth), nodes);
}

#[rstest]
#[case(1, 6)]
#[case(2, 264)]
#[case(3, 9467)]
fn perft_position_4_mirrored(#[case] depth: u32, #[case] nodes: u64) {
    assert_eq!(perft(&fen(POS4_MIRRORED), depth), nodes);
}

#[rstest]
#[case(1, 44)]
#[case(2, 1486)]
#[case(3, 62_379)]
fn perft_position_5(#[case] depth: u32, #[case] nodes: u64) {
    assert_eq!(perft(&fen(POS5), depth), nodes);
}

#[rstest]
#[case(1, 46)]
#[case(2, 2079)]
fn perft_position_6(#[case] depth: u32, #[case] nodes: u64) {
    assert_eq!(perft(&fen(POS6), depth), nodes);
}
