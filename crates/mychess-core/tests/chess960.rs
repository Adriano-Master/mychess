//! Chess960 through the public API: numbering, FEN (X-FEN and Shredder),
//! castling rules, UCI/SAN forms, round-trips and perft.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]

mod common;

use common::{perft, sq};
use mychess_core::{
    CastlingMode, CastlingSide, Chess960Error, Color, EnPassantFen, FenCastlingStyle, FenError,
    FenMode, Game, Move, Position, SanMode, SanWarning, Variant,
};

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn fen960(s: &str) -> Position {
    Position::from_fen_with(s, FenMode::Chess960)
        .unwrap_or_else(|e| panic!("bad test FEN {s:?}: {e}"))
}

fn castles(pos: &Position) -> Vec<Move> {
    pos.legal_moves()
        .into_iter()
        .filter(|m| m.is_castle())
        .collect()
}

// --- numbering -------------------------------------------------------------

#[test]
fn scharnagl_vectors_match_the_spec() {
    let vectors = [
        (
            0,
            "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1",
        ),
        (
            1,
            "bqnbnrkr/pppppppp/8/8/8/8/PPPPPPPP/BQNBNRKR w KQkq - 0 1",
        ),
        (
            2,
            "bqnnrbkr/pppppppp/8/8/8/8/PPPPPPPP/BQNNRBKR w KQkq - 0 1",
        ),
        (
            100,
            "qbbnrnkr/pppppppp/8/8/8/8/PPPPPPPP/QBBNRNKR w KQkq - 0 1",
        ),
        (518, START),
        (
            959,
            "rkrnnqbb/pppppppp/8/8/8/8/PPPPPPPP/RKRNNQBB w KQkq - 0 1",
        ),
    ];
    for (n, fen) in vectors {
        let pos = Position::chess960_start(n).unwrap();
        assert_eq!(pos.fen(), fen, "position {n}");
        assert_eq!(pos.castling_mode(), CastlingMode::Chess960);
        assert_eq!(pos.variant(), Variant::Chess960);
        assert_eq!(pos.chess960_number(), Some(n));
    }
}

#[test]
fn all_960_positions_are_distinct_valid_and_invertible() {
    let mut seen = std::collections::HashSet::new();
    for n in 0..960u16 {
        let pos = Position::chess960_start(n).unwrap();
        assert!(seen.insert(pos.fen()), "duplicate {n}");
        assert_eq!(pos.chess960_number(), Some(n));
        assert_eq!(pos.pieces().count(), 32);
        assert!(!pos.is_check());
        assert_eq!(pos.halfmove_clock(), 0);
        assert_eq!(pos.fullmove_number(), 1);
        for color in Color::ALL {
            for side in [CastlingSide::KingSide, CastlingSide::QueenSide] {
                assert!(pos.castling_rook(color, side).is_some(), "{n}");
            }
        }
    }
    assert_eq!(seen.len(), 960);
}

#[test]
fn number_960_is_an_error() {
    assert_eq!(
        Position::chess960_start(960),
        Err(Chess960Error::NumberOutOfRange(960))
    );
    assert!(Position::chess960_start(u16::MAX).is_err());
}

#[test]
fn standard_start_is_number_518_and_equal_to_the_960_one() {
    let std_start = Position::new();
    let p518 = Position::chess960_start(518).unwrap();
    assert_eq!(std_start.chess960_number(), Some(518));
    assert_eq!(std_start, p518);
    assert_eq!(std_start.zobrist(), p518.zobrist());
    assert_eq!(std_start.variant(), Variant::Standard);
    assert_eq!(p518.variant(), Variant::Chess960);
}

// --- FEN -------------------------------------------------------------------

#[test]
fn shredder_of_position_0_is_hfhf() {
    let pos = Position::chess960_start(0).unwrap();
    assert_eq!(
        pos.fen_with_style(FenCastlingStyle::Shredder),
        "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1"
    );
    assert_eq!(
        pos.fen(),
        "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1"
    );
    assert_eq!(
        Position::from_fen("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w HFhf - 0 1").unwrap(),
        pos
    );
}

#[test]
fn standard_start_in_shredder_stays_standard() {
    let pos =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w HAha - 0 1").unwrap();
    assert_eq!(pos.castling_mode(), CastlingMode::Standard);
    assert_eq!(pos.fen(), START);
    assert_eq!(
        pos.fen_with_style(FenCastlingStyle::Shredder),
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w HAha - 0 1"
    );
}

#[test]
fn auto_picks_chess960_only_when_needed() {
    // Rook off its corner: needs 960.
    let p = Position::from_fen("4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1").unwrap();
    assert_eq!(p.castling_mode(), CastlingMode::Chess960);
    // King off e1.
    let p = Position::from_fen("4k3/8/8/8/8/8/8/6KR w H - 0 1").unwrap();
    assert_eq!(p.castling_mode(), CastlingMode::Chess960);
    // Standard-expressible.
    let p = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    assert_eq!(p.castling_mode(), CastlingMode::Standard);
    // No rights at all.
    let p = Position::from_fen("4k3/8/8/8/8/8/8/6K1 w - - 0 1").unwrap();
    assert_eq!(p.castling_mode(), CastlingMode::Standard);
    // Explicit modes override.
    let p = Position::from_fen_with(START, FenMode::Chess960).unwrap();
    assert_eq!(p.castling_mode(), CastlingMode::Chess960);
    let p = Position::from_fen_with("4k3/8/8/8/8/8/8/6KR w H - 0 1", FenMode::Standard).unwrap();
    assert_eq!(p.castling_mode(), CastlingMode::Standard);
    assert!(!p.has_castling_right(Color::White, CastlingSide::KingSide));
}

#[test]
fn xfen_writes_a_letter_only_for_non_outermost_rooks() {
    // Rooks b1 and d1, king e1; only d1 has the right.
    let p = Position::from_fen("4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1").unwrap();
    assert_eq!(p.fen(), "4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1");
    // The outermost kingside rook is K, the non-outermost queenside one is D.
    let p = Position::from_fen("4k3/8/8/8/8/8/8/1R1RK2R w DH - 0 1").unwrap();
    assert_eq!(p.fen(), "4k3/8/8/8/8/8/8/1R1RK2R w KD - 0 1");
}

#[test]
fn castling_field_reading_rules() {
    // Letters in any order, repeated letters ignored.
    let a = fen960("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w fhHF - 0 1");
    assert_eq!(a, Position::chess960_start(0).unwrap());
    let a = fen960("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KKQq - 0 1");
    assert!(a.has_castling_right(Color::White, CastlingSide::KingSide));
    // A letter naming no rook of the colour is dropped silently.
    let a = fen960("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w E - 0 1");
    assert!(!a.has_castling_right(Color::White, CastlingSide::KingSide));
    assert!(!a.has_castling_right(Color::White, CastlingSide::QueenSide));
    // Anything else is an error.
    for bad in ["x", "KQkqx", "I", "i", "1", "K-"] {
        let fen = format!("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w {bad} - 0 1");
        assert!(
            matches!(
                Position::from_fen(&fen),
                Err(FenError::InvalidCastling { .. })
            ),
            "{bad}"
        );
    }
}

#[test]
fn missing_castling_field_in_960_gives_the_outermost_rooks() {
    let p = Position::from_fen_with(
        "bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w",
        FenMode::Chess960,
    )
    .unwrap();
    assert_eq!(p, Position::chess960_start(0).unwrap());
    let p = Position::from_fen_with("rkr5/8/8/8/8/8/8/4K2R w", FenMode::Chess960).unwrap();
    assert_eq!(
        p.castling_rook(Color::Black, CastlingSide::KingSide),
        Some(sq("c8"))
    );
    assert_eq!(
        p.castling_rook(Color::Black, CastlingSide::QueenSide),
        Some(sq("a8"))
    );
    assert_eq!(
        p.castling_rook(Color::White, CastlingSide::KingSide),
        Some(sq("h1"))
    );
}

#[test]
fn fen_roundtrip_in_both_styles_for_all_start_positions() {
    for n in 0..960u16 {
        let pos = Position::chess960_start(n).unwrap();
        for style in [FenCastlingStyle::XFen, FenCastlingStyle::Shredder] {
            let fen = pos.fen_with_style(style);
            for mode in [FenMode::Auto, FenMode::Chess960] {
                let back = Position::from_fen_with(&fen, mode).unwrap();
                assert_eq!(back, pos, "{fen}");
                assert_eq!(back.chess960_number(), Some(n));
            }
        }
    }
}

// --- castling rules ---------------------------------------------------------

#[test]
fn rule_10_example_rook_stays_in_place() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1").unwrap();
    let m = pos.parse_san("O-O-O", SanMode::Strict).unwrap().mv;
    assert_eq!(pos.uci(m), "e1d1");
    assert_eq!(pos.parse_uci("e1d1"), Ok(m));
    let after = pos.after(m).unwrap();
    assert_eq!(after.king_square(Color::White), Some(sq("c1")));
    assert_eq!(after.fen(), "4k3/8/8/8/8/8/8/1RKR4 b - - 1 1");
    assert_eq!(pos.san(m).unwrap(), "O-O-O");
}

#[test]
fn rule_10_example_king_stays_in_place() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/6KR w H - 0 1").unwrap();
    let m = pos.parse_san("O-O", SanMode::Strict).unwrap().mv;
    assert_eq!(pos.uci(m), "g1h1");
    assert_eq!(pos.san(m).unwrap(), "O-O");
    assert_eq!(
        pos.after(m).unwrap().fen(),
        "4k3/8/8/8/8/8/8/5RK1 b - - 1 1"
    );
}

#[test]
fn rule_10_example_king_and_rook_cross() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/1KR5 w C - 0 1").unwrap();
    let m = pos.parse_san("O-O", SanMode::Strict).unwrap().mv;
    assert_eq!(pos.uci(m), "b1c1");
    assert_eq!(
        pos.after(m).unwrap().fen(),
        "4k3/8/8/8/8/8/8/5RK1 b - - 1 1"
    );
}

#[test]
fn castling_is_refused_through_attacked_squares() {
    // The black rook on f8 attacks f1, which the king crosses.
    let pos = fen960("4kr2/8/8/8/8/8/8/4K2R w H - 0 1");
    assert!(castles(&pos).is_empty());
    // Without the attacker it is legal.
    let pos = fen960("4k3/8/8/8/8/8/8/4K2R w H - 0 1");
    assert_eq!(castles(&pos).len(), 1);
}

#[test]
fn castling_is_refused_when_the_rook_is_pinned_on_the_first_rank() {
    // Queen-side: king c1, rook b1, black rook a1 pins it. After castling the
    // rook would leave b1 and expose the king.
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/rRK5 w B - 0 1").unwrap();
    assert_eq!(pos.castling_mode(), CastlingMode::Chess960);
    assert!(castles(&pos).is_empty());
    // With a white piece also between, nothing is pinned and it is legal.
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/rRKR4 w B - 0 1").unwrap();
    assert_eq!(
        castles(&pos).len(),
        0,
        "d1 occupied by a non-castling piece"
    );
}

#[test]
fn castling_is_refused_when_the_path_is_blocked() {
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/R2QK3 w A - 0 1").unwrap();
    assert!(castles(&pos).is_empty());
}

#[test]
fn rights_are_lost_by_king_move_rook_move_and_rook_capture() {
    // King move.
    let pos = fen960("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1");
    let after = pos.after(pos.parse_uci("e1d1").unwrap()).unwrap();
    assert_eq!(
        after.castling_rook(Color::White, CastlingSide::KingSide),
        None
    );
    assert_eq!(
        after.castling_rook(Color::White, CastlingSide::QueenSide),
        None
    );
    // Rook move: only that side.
    let after = pos.after(pos.parse_uci("h1h2").unwrap()).unwrap();
    assert_eq!(
        after.castling_rook(Color::White, CastlingSide::KingSide),
        None
    );
    assert_eq!(
        after.castling_rook(Color::White, CastlingSide::QueenSide),
        Some(sq("a1"))
    );
    // Rook captured by the opponent.
    let pos = fen960("4k2r/8/8/8/8/8/8/R3K2R b KQk - 0 1");
    let after = pos.after(pos.parse_uci("h8h1").unwrap()).unwrap();
    assert_eq!(
        after.castling_rook(Color::White, CastlingSide::KingSide),
        None
    );
    assert_eq!(
        after.castling_rook(Color::White, CastlingSide::QueenSide),
        Some(sq("a1"))
    );
    assert_eq!(
        after.castling_rook(Color::Black, CastlingSide::KingSide),
        None
    );
}

#[test]
fn capturing_an_unrelated_rook_keeps_the_right_of_the_inner_one() {
    // The right belongs to b1; the a1 rook has none. Capturing a1 keeps it.
    let pos = fen960("r3k3/8/8/8/8/8/8/RR2K3 b B - 0 1");
    let after = pos.after(pos.parse_uci("a8a1").unwrap()).unwrap();
    assert_eq!(
        after.castling_rook(Color::White, CastlingSide::QueenSide),
        Some(sq("b1"))
    );
}

// --- UCI --------------------------------------------------------------------

#[test]
fn uci_writing_depends_on_the_castling_mode() {
    let std = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let m = std.parse_uci("e1g1").unwrap();
    assert_eq!(std.uci(m), "e1g1");
    assert_eq!(m.to_uci(CastlingMode::Chess960), "e1h1");
    let c960 = fen960("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    assert_eq!(c960.uci(m), "e1h1");
}

#[test]
fn parse_uci_in_960_accepts_the_three_forms() {
    let pos = fen960("4k3/8/8/8/8/8/8/4K2R w H - 0 1");
    let castle = pos.parse_uci("e1h1").unwrap();
    assert!(castle.is_castle());
    // King to its destination square (engines in standard mode).
    assert_eq!(pos.parse_uci("e1g1"), Ok(castle));
    // Ordinary move.
    let step = pos.parse_uci("e1e2").unwrap();
    assert!(!step.is_castle());
    // Upper case works too.
    assert_eq!(pos.parse_uci("E1H1"), Ok(castle));
}

#[test]
fn parse_uci_prefers_the_ordinary_king_move_over_castling_by_destination() {
    // King f1, rook h1: f1g1 is an ordinary move, f1h1 is castling (king
    // lands on g1, rook on f1).
    let pos = fen960("4k3/8/8/8/8/8/8/5K1R w H - 0 1");
    let step = pos.parse_uci("f1g1").unwrap();
    assert!(!step.is_castle());
    let castle = pos.parse_uci("f1h1").unwrap();
    assert!(castle.is_castle());
    assert_eq!(castle.to(), Some(sq("g1")));
    assert_ne!(step, castle);
    // SAN keeps them apart too.
    assert_eq!(pos.san(step).unwrap(), "Kg1");
    assert_eq!(pos.san(castle).unwrap(), "O-O");
}

#[test]
fn king_takes_own_rook_is_castling_even_when_both_squares_are_adjacent() {
    let pos = fen960("4k3/8/8/8/8/8/8/5KR1 w G - 0 1");
    // King f1, rook g1: castling swaps them.
    let castle = pos.parse_uci("f1g1").unwrap();
    assert!(castle.is_castle());
    assert_eq!(
        pos.after(castle).unwrap().fen(),
        "4k3/8/8/8/8/8/8/5RK1 b - - 1 1"
    );
}

// --- SAN --------------------------------------------------------------------

#[test]
fn tolerant_san_restores_a_lost_right_in_960() {
    let pos = fen960("4k3/8/8/8/8/8/8/R3K3 w - - 0 1");
    assert!(pos.parse_san("O-O-O", SanMode::Strict).is_err());
    let parsed = pos.parse_san("O-O-O", SanMode::Tolerant).unwrap();
    assert_eq!(parsed.warnings, vec![SanWarning::IllegalCastling]);
    assert!(parsed.mv.is_castle());
    // No rook on the kingside: still an error.
    assert!(pos.parse_san("O-O", SanMode::Tolerant).is_err());
    // A king off the first rank gets no help.
    let off = fen960("4k3/8/8/8/8/8/4K3/R7 w - - 0 1");
    assert!(off.parse_san("O-O-O", SanMode::Tolerant).is_err());
}

#[test]
fn tolerant_castling_spellings_work_in_960() {
    let pos = fen960("4k3/8/8/8/8/8/8/1R1RK3 w D - 0 1");
    for text in ["O-O-O", "0-0-0", "OOO", "o-o-o"] {
        let parsed = pos.parse_san(text, SanMode::Tolerant).unwrap();
        assert!(parsed.mv.is_castle(), "{text}");
    }
}

#[test]
fn game_inherits_the_variant_and_castles_in_the_tree() {
    let start = Position::chess960_start(0).unwrap();
    assert_eq!(Game::from_position(start).variant(), Variant::Chess960);
    assert_eq!(Game::new().variant(), Variant::Standard);

    let pos = fen960("4k3/8/8/8/8/8/8/6KR w H - 0 1");
    let mut game = Game::from_position(pos.clone());
    let m = pos.parse_uci("g1h1").unwrap();
    let node = game.add_move(game.root(), m).unwrap();
    assert_eq!(
        game.position_at(node).unwrap().castling_mode(),
        CastlingMode::Chess960
    );
    assert_eq!(game.variant(), Variant::Chess960);
}

// --- round-trips on random positions ------------------------------------------

/// Small deterministic generator (no external randomness).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

#[test]
fn san_uci_fen_roundtrip_on_100_random_960_positions() {
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    let mut castling_seen = 0;
    for i in 0..100u16 {
        let n = u16::try_from(rng.next() % 960).unwrap();
        let mut pos = Position::chess960_start(n).unwrap();
        let plies = 4 + rng.next() % 40;
        for _ in 0..plies {
            let moves = pos.legal_moves();
            if moves.is_empty() {
                break;
            }
            // Prefer castling when available so it is exercised.
            let m = moves
                .iter()
                .copied()
                .find(|m| m.is_castle() && rng.next().is_multiple_of(2))
                .unwrap_or(moves[usize::try_from(rng.next()).unwrap() % moves.len()]);
            pos = pos.after(m).unwrap();
        }
        for style in [FenCastlingStyle::XFen, FenCastlingStyle::Shredder] {
            let fen = pos.fen_with_options(EnPassantFen::Adjacent, style);
            let back = Position::from_fen(&fen).unwrap();
            assert_eq!(back, pos, "game {i}: {fen}");
            assert_eq!(back.fen_with_style(style), fen);
        }
        for m in pos.legal_moves() {
            castling_seen += usize::from(m.is_castle());
            let san = pos.san(m).unwrap();
            assert_eq!(pos.parse_san(&san, SanMode::Strict).unwrap().mv, m, "{san}");
            let uci = pos.uci(m);
            assert_eq!(pos.parse_uci(&uci), Ok(m), "{uci}");
        }
    }
    assert!(castling_seen > 0, "the sample never offered castling");
}

// --- perft ----------------------------------------------------------------------

const PERFT_960: [(&str, [u64; 5]); 4] = [
    (
        "bqnb1rkr/pp3ppp/3ppn2/2p5/5P2/P2P4/NPP1P1PP/BQ1BNRKR w HFhf - 2 9",
        [21, 528, 12_189, 326_672, 8_146_062],
    ),
    (
        "2nnrbkr/p1qppppp/8/1ppb4/6PP/3PP3/PPP2P2/BQNNRBKR w HEhe - 1 9",
        [21, 807, 18_002, 667_366, 16_253_601],
    ),
    (
        "b1q1rrkb/pppppppp/3nn3/8/P7/1PPP4/4PPPP/BQNNRKRB w GE - 1 9",
        [20, 479, 10_471, 273_318, 6_417_013],
    ),
    (
        "qbbnnrkr/2pp2pp/p7/1p2pp2/8/P3PP2/1PPP1KPP/QBBNNR1R w hf - 0 9",
        [22, 593, 13_440, 382_958, 9_183_776],
    ),
];

fn check_perft(id: usize, max_depth: usize) {
    let (fen, counts) = PERFT_960[id];
    let pos = Position::from_fen(fen).unwrap();
    assert_eq!(pos.castling_mode(), CastlingMode::Chess960, "{fen}");
    for depth in 1..=max_depth {
        assert_eq!(
            perft(&pos, u32::try_from(depth).unwrap()),
            counts[depth - 1],
            "id {id} depth {depth}"
        );
    }
}

#[test]
fn perft_960_position_0_to_depth_5() {
    check_perft(0, 5);
}

#[test]
fn perft_960_position_1_to_depth_5() {
    check_perft(1, 5);
}

#[test]
fn perft_960_position_2_to_depth_5() {
    check_perft(2, 5);
}

#[test]
fn perft_960_position_3_to_depth_5() {
    check_perft(3, 5);
}

#[test]
fn perft_standard_start_is_unchanged() {
    assert_eq!(perft(&Position::new(), 4), 197_281);
    assert_eq!(perft(&Position::chess960_start(518).unwrap(), 4), 197_281);
}

#[test]
#[ignore = "slow: perft depth 6 of Chess960 position 0 (run with --ignored)"]
fn perft_960_position_0_depth_6() {
    let pos = Position::from_fen(PERFT_960[0].0).unwrap();
    assert_eq!(perft(&pos, 6), 227_689_589);
}

#[test]
fn lenient_play_restores_the_right_of_the_castling_rook_only() {
    // Same move on a board without rights: the outermost rook gets the right.
    let with_rights = fen960("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1");
    let without = fen960("4k3/8/8/8/8/8/8/R3K2R w - - 0 1");
    let m = with_rights.parse_uci("e1h1").unwrap();
    assert!(!without.is_legal(m));
    let mut p = without.clone();
    p.play_lenient(m).unwrap();
    assert_eq!(p.fen(), "4k3/8/8/8/8/8/8/R4RK1 b - - 1 1");

    // The move names the inner rook (d1) but the outermost one (a1) would
    // get the right: the move stays refused and the position is untouched.
    let inner = fen960("4k3/8/8/8/8/8/8/R2RK3 w D - 0 1");
    let m = inner.parse_uci("e1d1").unwrap();
    assert!(m.is_castle());
    let mut p = fen960("4k3/8/8/8/8/8/8/R2RK3 w - - 0 1");
    let before = p.clone();
    assert!(p.play_lenient(m).is_err());
    assert_eq!(p, before);
}

#[test]
fn tolerant_san_castling_is_refused_when_an_inner_rook_blocks_the_outer_one() {
    let pos = fen960("4k3/8/8/8/8/8/8/R3K1RR w - - 0 1");
    assert!(pos.parse_san("O-O", SanMode::Tolerant).is_err());
    let pos = fen960("4k3/8/8/8/8/8/8/R2RK3 w - - 0 1");
    assert!(pos.parse_san("O-O-O", SanMode::Tolerant).is_err());
}

// --- Auto mode with stale KQkq ------------------------------------------------

#[test]
fn auto_keeps_standard_when_a_stale_kqkq_does_not_resolve() {
    for fen in [
        "r4rk1/pppq1ppp/2np1n2/2b1p3/2B1P3/2NP1N2/PPPQ1PPP/R4RK1 w KQkq - 0 9",
        "r4rk1/pppq1ppp/2np1n2/2b1p3/2B1P3/2NP1N2/PPPQ1PPP/R4RK1 w KQ - 0 9",
    ] {
        let pos = Position::from_fen(fen).unwrap();
        assert_eq!(pos.castling_mode(), CastlingMode::Standard, "{fen}");
        assert_eq!(pos.variant(), Variant::Standard);
        assert!(pos.fen().contains(" w - - "), "{}", pos.fen());
        // The standard castling move of a king on g1 does not exist.
        assert!(pos.legal_moves().iter().all(|m| !m.is_castle()));
    }
}

#[test]
fn auto_stays_standard_when_the_kqkq_is_expressible() {
    let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    assert_eq!(pos.castling_mode(), CastlingMode::Standard);
    let castle = castles(&pos);
    assert_eq!(castle.len(), 2);
    assert!(
        castle
            .iter()
            .any(|m| m.to_uci(CastlingMode::Standard) == "e1g1")
    );
    // A single stale letter in an otherwise coherent field also falls back.
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/R4K2 w KQ - 0 1").unwrap();
    assert_eq!(pos.castling_mode(), CastlingMode::Standard);
    assert!(pos.fen().contains(" w - - "));
}

#[test]
fn auto_switches_to_960_only_when_every_letter_resolves() {
    let p0 =
        Position::from_fen("bbqnnrkr/pppppppp/8/8/8/8/PPPPPPPP/BBQNNRKR w KQkq - 0 1").unwrap();
    assert_eq!(p0.castling_mode(), CastlingMode::Chess960);
    assert_eq!(p0.chess960_number(), Some(0));
    // King on g1, outermost rooks a1 and h1 for both letters: resolves.
    let all = Position::from_fen("4k3/8/8/8/8/8/8/R4RKR w KQ - 0 1").unwrap();
    assert_eq!(all.castling_mode(), CastlingMode::Chess960);
    // Same board, but black has `k` with no rook on its side: all or nothing,
    // so the whole field is read as standard and the rights are dropped.
    let partial = Position::from_fen("4k3/8/8/8/8/8/8/R4RKR w KQk - 0 1").unwrap();
    assert_eq!(partial.castling_mode(), CastlingMode::Standard);
    assert!(partial.fen().contains(" w - - "));
}

#[test]
fn game_same_content_distinguishes_the_variant() {
    let std_game = Game::from_position(Position::new());
    let p518 = Game::from_position(Position::chess960_start(518).unwrap());
    assert_eq!(std_game.variant(), Variant::Standard);
    assert_eq!(p518.variant(), Variant::Chess960);
    assert!(!std_game.same_content(&p518));
    assert!(!p518.same_content(&std_game));
    let mut a = std_game.clone();
    let mut b = p518.clone();
    for g in [&mut a, &mut b] {
        let root = g.root();
        let m = g.start_position().parse_uci("e2e4").unwrap();
        g.add_move(root, m).unwrap();
    }
    assert!(!a.same_content(&b));
    assert!(a.same_content(&a.clone()));
}

#[test]
fn x_fen_roundtrip_asserts_the_mode() {
    for n in [0u16, 100, 518, 959] {
        let pos = Position::chess960_start(n).unwrap();
        let explicit = Position::from_fen_with(&pos.fen(), FenMode::Chess960).unwrap();
        assert_eq!(explicit.castling_mode(), CastlingMode::Chess960);
        assert_eq!(explicit, pos);
        let auto = Position::from_fen(&pos.fen()).unwrap();
        assert_eq!(auto, pos);
        // 518 reads back as standard in Auto, the others stay 960.
        let expected = if n == 518 {
            CastlingMode::Standard
        } else {
            CastlingMode::Chess960
        };
        assert_eq!(auto.castling_mode(), expected, "{n}");
    }
}

// --- compatibility with the Scid patch ---------------------------------------

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_chess960_krn_table_matches_scid_patch() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/scid_vs_pc-4.27");
    assert!(
        dir.is_dir(),
        "Scid vs PC reference not found at {} (see docs/referencia-scid.md)",
        dir.display()
    );
    let patch = std::fs::read_to_string(dir.join("patches/chess960-new.patch"))
        .expect("read patches/chess960-new.patch");
    let start = patch
        .find("proc ::chess960::numberToFEN")
        .expect("numberToFEN in the patch");
    let line = patch[start..]
        .lines()
        .find(|l| l.contains("set KRN"))
        .expect("KRN list in numberToFEN");
    let krn: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    assert_eq!(krn.len(), 10, "{line}");

    // Position n = 96 * k + 16 * q + 4 * b2 + b1; with the bishops on b/a and
    // the queen on the first free square (q = 0, b1 = b2 = 0) the remaining
    // five files, c to h minus the queen, hold the KRN pattern k in order.
    for (k, pattern) in krn.iter().enumerate() {
        let n = u16::try_from(96 * k).unwrap();
        let pos = Position::chess960_start(n).unwrap();
        let fen = pos.fen();
        let rank = fen.rsplit('/').next().unwrap().split(' ').next().unwrap();
        // Rank 1 is "BBQ" + pattern in upper case for b1 = b2 = q = 0.
        assert_eq!(rank, format!("BBQ{pattern}"), "KRN index {k}");
    }
}
