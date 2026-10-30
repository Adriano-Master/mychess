//! SAN reading and writing (spec rules 13-24).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    let_underscore_drop
)]
mod common;
use common::{START_COUNT, fen, random_game, sq, start_position};
use mychess_core::{CastlingSide, Move, Position, Role, SanError, SanMode, SanSuffix, SanWarning};
use proptest::prelude::*;
use rstest::rstest;

const BOTH_CASTLES_W: &str = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
const BOTH_CASTLES_B: &str = "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1";

fn tol(p: &Position, s: &str) -> Move {
    p.parse_san(s, SanMode::Tolerant)
        .unwrap_or_else(|e| panic!("{s:?} should parse in {}: {e}", p.fen()))
        .mv
}

fn uci_of(p: &Position, s: &str) -> String {
    p.uci(tol(p, s))
}

// ---------- rule 14: castling ----------

#[rstest]
#[case("O-O", "e1g1")]
#[case("O-O-O", "e1c1")]
#[case("0-0", "e1g1")]
#[case("0-0-0", "e1c1")]
#[case("o-o", "e1g1")]
#[case("o-o-o", "e1c1")]
#[case("OO", "e1g1")]
#[case("OOO", "e1c1")]
#[case("00", "e1g1")]
#[case("000", "e1c1")]
#[case("O-O+", "e1g1")]
#[case("O-O-O#", "e1c1")]
#[case("0-0+", "e1g1")]
fn tolerant_reads_white_castling_spellings(#[case] text: &str, #[case] uci: &str) {
    assert_eq!(uci_of(&fen(BOTH_CASTLES_W), text), uci);
}

#[rstest]
#[case("O-O", "e8g8")]
#[case("O-O-O", "e8c8")]
#[case("000", "e8c8")]
fn tolerant_reads_black_castling(#[case] text: &str, #[case] uci: &str) {
    assert_eq!(uci_of(&fen(BOTH_CASTLES_B), text), uci);
}

#[test]
fn oo_falls_back_to_queenside_when_kingside_is_impossible() {
    // Scid: pgnparse.cpp:1070. Only the queenside right exists.
    let p = fen("r3k2r/8/8/8/8/8/8/R3K2R w Qkq - 0 1");
    let r = p
        .parse_san("OO", SanMode::Tolerant)
        .expect("OO falls back to long castling");
    assert_eq!(p.uci(r.mv), "e1c1");
}

#[test]
fn strict_accepts_only_canonical_castling() {
    let p = fen(BOTH_CASTLES_W);
    assert!(p.parse_san("O-O", SanMode::Strict).is_ok());
    assert!(p.parse_san("O-O-O", SanMode::Strict).is_ok());
    for bad in ["0-0", "0-0-0", "o-o", "OO", "OOO"] {
        assert!(
            matches!(
                p.parse_san(bad, SanMode::Strict),
                Err(SanError::NonCanonical { .. })
            ),
            "{bad}: {:?}",
            p.parse_san(bad, SanMode::Strict)
        );
    }
}

#[test]
fn castle_moves_with_equal_squares_are_equal_and_share_san() {
    let p = fen(BOTH_CASTLES_W);
    let a = tol(&p, "O-O");
    let b = Move::Castle {
        king_from: sq("e1"),
        rook_from: sq("h1"),
    };
    assert_eq!(a, b);
    assert_eq!(a.castling_side(), Some(CastlingSide::KingSide));
    assert_eq!(p.san(a).unwrap(), p.san(b).unwrap());
    assert_eq!(p.san(b).unwrap(), "O-O");
    let long = tol(&p, "O-O-O");
    assert_eq!(long.castling_side(), Some(CastlingSide::QueenSide));
    assert_eq!(Move::Null.castling_side(), None);
}

#[test]
fn strict_rejects_space_before_check_marks() {
    let p = Position::new();
    for bad in ["e4 +", "e4 #", "e4 +!"] {
        assert!(
            matches!(
                p.parse_san(bad, SanMode::Strict),
                Err(SanError::NonCanonical { .. })
            ),
            "{bad}: {:?}",
            p.parse_san(bad, SanMode::Strict)
        );
    }
    assert!(p.parse_san("e4 +", SanMode::Tolerant).is_ok());
    assert!(p.parse_san("e4", SanMode::Strict).is_ok());
}

#[test]
fn oo_reinterpreted_as_long_castling_warns() {
    let p = fen("r3k2r/8/8/8/8/8/8/R3K2R w Qkq - 0 1");
    let r = p.parse_san("OO", SanMode::Tolerant).unwrap();
    assert_eq!(p.uci(r.mv), "e1c1");
    assert!(r.warnings.contains(&SanWarning::CastlingReinterpreted));
    // A genuine long castling or a legal short one carries no such warning.
    let r = p.parse_san("OOO", SanMode::Tolerant).unwrap();
    assert!(!r.warnings.contains(&SanWarning::CastlingReinterpreted));
    let q = fen(BOTH_CASTLES_W);
    let r = q.parse_san("OO", SanMode::Tolerant).unwrap();
    assert!(r.warnings.is_empty());
    // With hyphens the fallback does not apply (stricter than the Scid).
    assert!(p.parse_san("O-O", SanMode::Tolerant).map_or(true, |r| {
        r.mv.castling_side() == Some(CastlingSide::KingSide)
    }));
}

#[test]
fn castling_with_lost_right_is_accepted_with_warning_in_tolerant() {
    let p = fen("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
    let r = p
        .parse_san("O-O", SanMode::Tolerant)
        .expect("tolerant accepts");
    assert_eq!(p.uci(r.mv), "e1g1");
    assert!(
        r.warnings.contains(&SanWarning::IllegalCastling),
        "{:?}",
        r.warnings
    );
    let r = p
        .parse_san("O-O-O", SanMode::Tolerant)
        .expect("tolerant accepts");
    assert_eq!(p.uci(r.mv), "e1c1");
    assert!(r.warnings.contains(&SanWarning::IllegalCastling));
}

#[test]
fn castling_with_lost_right_is_rejected_in_strict() {
    let p = fen("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
    assert!(matches!(
        p.parse_san("O-O", SanMode::Strict),
        Err(SanError::IllegalCastling { .. })
    ));
}

#[test]
fn legal_castling_has_no_warning() {
    let p = fen(BOTH_CASTLES_W);
    assert!(
        p.parse_san("O-O", SanMode::Tolerant)
            .unwrap()
            .warnings
            .is_empty()
    );
}

#[test]
fn tolerant_castling_play_lenient_moves_both_pieces() {
    let p = fen("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
    let m = tol(&p, "O-O");
    let mut q = p.clone();
    q.play_lenient(m).expect("lenient play");
    assert_eq!(
        q.fen(),
        "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1".replace(" kq", " -")
    );
    // strict play must refuse it
    assert!(p.clone().play(m).is_err());
}

#[rstest]
#[case("r3k2r/8/8/8/8/8/8/R3KB1R w - - 0 1")] // path blocked
#[case("r3k2r/8/8/8/8/8/8/R3K3 w - - 0 1")] // rook missing
#[case("r3k2r/8/8/8/8/8/4r3/R3K2R w - - 0 1")] // in check
fn tolerant_never_accepts_castling_the_board_forbids(#[case] f: &str) {
    let p = fen(f);
    assert!(p.parse_san("O-O", SanMode::Tolerant).is_err(), "{f}");
}

#[test]
fn tolerant_rejects_castling_when_king_is_not_home() {
    let p = fen("r3k2r/8/8/8/8/8/8/R2K3R w - - 0 1");
    assert!(p.parse_san("O-O", SanMode::Tolerant).is_err());
}

// ---------- rule 15: pawns ----------

#[test]
fn pawn_push_and_coordinate_forms() {
    let p = Position::new();
    assert_eq!(uci_of(&p, "e4"), "e2e4");
    assert_eq!(uci_of(&p, "e2e4"), "e2e4");
    assert_eq!(uci_of(&p, "Pe4"), "e2e4");
    assert_eq!(uci_of(&p, "e3"), "e2e3");
    assert_eq!(uci_of(&p, "e2-e4"), "e2e4");
}

#[rstest]
#[case("exd5")]
#[case("ed")]
#[case("ed5")]
#[case("exd")]
#[case("e4xd5")]
#[case("e4d5")]
#[case("e:d5")]
#[case("Pexd5")]
fn pawn_capture_forms(#[case] text: &str) {
    let p = fen("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2");
    assert_eq!(uci_of(&p, text), "e4d5", "{text}");
}

#[rstest]
#[case("exd6")]
#[case("exd6 e.p.")]
#[case("exd6ep")]
#[case("exd6 ep")]
#[case("ed")]
fn en_passant_forms(#[case] text: &str) {
    let p = fen("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3");
    let m = tol(&p, text);
    assert!(m.is_en_passant(), "{text}: {m:?}");
    assert_eq!(p.uci(m), "e5d6");
}

#[test]
fn en_passant_san_has_no_ep_suffix() {
    let p = fen("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3");
    let m = p.parse_san("exd6", SanMode::Strict).unwrap().mv;
    assert_eq!(p.san_with(m, SanSuffix::None).unwrap(), "exd6");
}

#[test]
fn ed_picks_the_only_legal_capture() {
    // Two white pawns could "take on d": c4xd5 and e4xd5; "ed" names the e-pawn.
    let p = fen("4k3/8/8/3p4/2P1P3/8/8/4K3 w - - 0 1");
    assert_eq!(uci_of(&p, "ed"), "e4d5");
    assert_eq!(uci_of(&p, "cd"), "c4d5");
}

#[test]
fn suffix_and_annotation_characters_are_ignored() {
    let p = Position::new();
    for t in [
        "e4!", "e4?", "e4!?", "e4?!", "e4!!", "e4??", "Nf3!", "e4+", "e4#",
    ] {
        assert!(p.parse_san(t, SanMode::Tolerant).is_ok(), "{t}");
    }
    assert_eq!(uci_of(&p, "Nf3!?"), "g1f3");
}

#[test]
fn check_suffix_is_not_validated() {
    let p = Position::new();
    assert!(p.parse_san("Nf3#", SanMode::Tolerant).is_ok());
    assert!(p.parse_san("e4+", SanMode::Tolerant).is_ok());
}

// ---------- rule 16: promotion ----------

#[rstest]
#[case("e8=Q", Role::Queen)]
#[case("e8Q", Role::Queen)]
#[case("e8q", Role::Queen)]
#[case("e8=R", Role::Rook)]
#[case("e8=B", Role::Bishop)]
#[case("e8=N", Role::Knight)]
#[case("e8n", Role::Knight)]
#[case("e8=N+", Role::Knight)]
#[case("e7e8=N", Role::Knight)]
#[case("e7e8q", Role::Queen)]
fn promotion_forms(#[case] text: &str, #[case] role: Role) {
    let p = fen("8/4P2k/8/8/8/8/8/K7 w - - 0 1");
    assert_eq!(tol(&p, text).promotion(), Some(role), "{text}");
}

#[test]
fn promotion_with_capture() {
    let p = fen("3rr2k/4P3/8/8/8/8/8/K7 w - - 0 1");
    let m = tol(&p, "exd8=N");
    assert_eq!(m.promotion(), Some(Role::Knight));
    assert!(m.is_capture());
    assert_eq!(tol(&p, "exd8Q").promotion(), Some(Role::Queen));
}

#[test]
fn promotion_without_letter_defaults_to_queen() {
    // Scid position.cpp:2471
    let p = fen("8/4P2k/8/8/8/8/8/K7 w - - 0 1");
    assert_eq!(tol(&p, "e8").promotion(), Some(Role::Queen));
}

#[test]
fn promotion_without_letter_in_strict_is_rejected() {
    // Implementer note: MissingPromotion is never emitted; NonCanonical instead.
    let p = fen("8/4P2k/8/8/8/8/8/K7 w - - 0 1");
    assert!(matches!(
        p.parse_san("e8", SanMode::Strict),
        Err(SanError::NonCanonical { .. } | SanError::MissingPromotion { .. })
    ));
}

#[rstest]
#[case("e8=K")]
#[case("e8=P")]
#[case("e8=X")]
#[case("e8=")]
#[case("e8=Qq")]
fn invalid_promotion_pieces_are_errors(#[case] text: &str) {
    let p = fen("8/4P2k/8/8/8/8/8/K7 w - - 0 1");
    assert!(p.parse_san(text, SanMode::Tolerant).is_err(), "{text}");
}

#[test]
fn promotion_piece_on_non_promoting_move_is_error() {
    let p = Position::new();
    assert!(p.parse_san("e4=Q", SanMode::Tolerant).is_err());
    assert!(p.parse_san("e4Q", SanMode::Tolerant).is_err());
    assert!(p.parse_san("Nf3=Q", SanMode::Tolerant).is_err());
}

#[test]
fn san_writes_promotion_with_equals() {
    let p = fen("8/4P2k/8/8/8/8/8/K7 w - - 0 1");
    let m = p.find_move(sq("e7"), sq("e8"), Some(Role::Knight)).unwrap();
    assert_eq!(p.san_with(m, SanSuffix::None).unwrap(), "e8=N");
    let q = p.find_move(sq("e7"), sq("e8"), Some(Role::Queen)).unwrap();
    assert_eq!(p.san(q).unwrap(), "e8=Q");
}

#[test]
fn san_promotion_with_capture_and_check() {
    let p = fen("3rr3/4P3/8/8/8/8/8/K6k w - - 0 1");
    let m = p.find_move(sq("e7"), sq("d8"), Some(Role::Queen)).unwrap();
    assert_eq!(p.san_with(m, SanSuffix::None).unwrap(), "exd8=Q");
}

// ---------- rule 17: pieces ----------

#[rstest]
#[case("Nf3", "g1f3")]
#[case("Ng1f3", "g1f3")]
#[case("Ng1-f3", "g1f3")]
#[case("Ngf3", "g1f3")]
#[case("N1f3", "g1f3")]
#[case("Nb1c3", "b1c3")]
#[case("Nc3", "b1c3")]
fn piece_moves_from_initial_position(#[case] text: &str, #[case] uci: &str) {
    assert_eq!(uci_of(&Position::new(), text), uci, "{text}");
}

#[test]
fn piece_capture_forms() {
    let p = fen("4k3/8/8/4p3/8/5N2/8/4K3 w - - 0 1");
    for t in ["Nxe5", "Nf3xe5", "Nf3-e5", "Nfxe5", "Ne5", "N3xe5"] {
        // "N3xe5": rank 3 disambiguator is legal even if redundant
        assert_eq!(uci_of(&p, t), "f3e5", "{t}");
    }
}

#[test]
fn disambiguation_by_file_rank_and_both_is_read() {
    // Knights on b1 and f1 can both reach d2.
    let p = fen("4k3/8/8/8/8/8/8/1N2KN2 w - - 0 1");
    assert_eq!(uci_of(&p, "Nbd2"), "b1d2");
    assert_eq!(uci_of(&p, "Nfd2"), "f1d2");
    assert_eq!(uci_of(&p, "Nb1d2"), "b1d2");
    // both knights stand on rank 1, so a rank-only hint cannot resolve it
    assert!(matches!(
        p.parse_san("N1d2", SanMode::Tolerant),
        Err(SanError::Ambiguous { .. })
    ));
}

#[test]
fn ambiguous_move_reports_ambiguous_not_illegal() {
    let p = fen("4k3/8/8/8/8/8/8/1N2KN2 w - - 0 1");
    for mode in [SanMode::Strict, SanMode::Tolerant] {
        let e = p.parse_san("Nd2", mode).unwrap_err();
        assert!(matches!(e, SanError::Ambiguous { .. }), "{mode:?}: {e:?}");
    }
}

#[test]
fn rank_disambiguation_is_ambiguous_when_both_share_rank() {
    // Rooks a1 and h1 can both reach d1: "R1d1" doesn't resolve it.
    let p = fen("4k3/8/8/8/8/8/4K3/R6R w - - 0 1");
    assert!(matches!(
        p.parse_san("R1d1", SanMode::Tolerant),
        Err(SanError::Ambiguous { .. })
    ));
    assert_eq!(uci_of(&p, "Rad1"), "a1d1");
}

#[test]
fn illegal_move_reports_illegal() {
    let p = Position::new();
    for t in [
        "Nf6", "e5", "Qd4", "Ke2", "Ra3", "exd5", "Bb5", "h5", "a1a2",
    ] {
        let e = p.parse_san(t, SanMode::Tolerant).unwrap_err();
        assert!(
            matches!(e, SanError::Illegal { .. }),
            "{t}: expected Illegal, got {e:?}"
        );
    }
}

#[test]
fn moving_a_pinned_piece_is_illegal() {
    let p = fen("4r1k1/8/8/8/8/8/4N3/4K3 w - - 0 1");
    assert!(matches!(
        p.parse_san("Nc3", SanMode::Tolerant),
        Err(SanError::Illegal { .. })
    ));
}

#[test]
fn moving_into_check_is_illegal() {
    let p = fen("4k3/8/8/8/8/8/r7/4K3 w - - 0 1");
    assert!(matches!(
        p.parse_san("Ke2", SanMode::Tolerant),
        Err(SanError::Illegal { .. })
    ));
}

#[test]
fn error_carries_text_and_fen() {
    let p = Position::new();
    match p.parse_san("Nf6", SanMode::Tolerant).unwrap_err() {
        SanError::Illegal { text, fen } => {
            assert_eq!(text, "Nf6");
            assert_eq!(fen, p.fen());
        }
        e => panic!("{e:?}"),
    }
}

// ---------- rule 18: lower-case piece letters ----------

#[test]
fn lowercase_b_pawn_capture_preferred_over_bishop() {
    // Both b2xc3 (pawn) and Bxc3 possible: pawn wins.
    let p = fen("4k3/8/8/8/8/2p5/1P6/2B1K3 w - - 0 1");
    let m = tol(&p, "bxc3");
    assert_eq!(p.uci(m), "b2c3");
}

#[test]
fn lowercase_bxc3_falls_back_to_bishop_in_tolerant() {
    // Scid rule 37: no legal pawn capture, so the bishop takes.
    let p = fen("4k3/8/8/8/8/2n5/8/B3K3 w - - 0 1");
    let m = tol(&p, "bxc3");
    assert_eq!(p.uci(m), "a1c3");
}

#[test]
fn lowercase_piece_letter_is_rejected_in_strict() {
    let p = fen("4k3/8/8/8/8/2n5/8/B3K3 w - - 0 1");
    assert!(p.parse_san("bxc3", SanMode::Strict).is_err());
    assert!(p.parse_san("Bxc3", SanMode::Strict).is_ok());
}

// ---------- rule 19: null move ----------

#[rstest]
#[case("--")]
#[case("null")]
#[case("Z0")]
#[case("0000")]
fn null_move_spellings(#[case] text: &str) {
    let p = Position::new();
    assert_eq!(tol(&p, text), Move::Null, "{text}");
}

#[test]
fn null_move_in_check_is_refused_by_san() {
    let p = fen("4k3/8/8/8/8/8/8/r3K3 w - - 0 1");
    assert!(p.is_check());
    let e = p.parse_san("--", SanMode::Tolerant).unwrap_err();
    assert!(matches!(e, SanError::NullMoveInCheck { .. }), "{e:?}");
}

#[test]
fn san_of_null_move_is_double_dash() {
    assert_eq!(Position::new().san(Move::Null).unwrap(), "--");
}

// ---------- rule 20: errors and hostile input ----------

#[test]
fn empty_and_whitespace_input_are_errors() {
    let p = Position::new();
    assert!(matches!(
        p.parse_san("", SanMode::Tolerant),
        Err(SanError::Empty)
    ));
    assert!(p.parse_san("   ", SanMode::Tolerant).is_err());
    assert!(p.parse_san("\t\n", SanMode::Strict).is_err());
}

#[test]
fn huge_input_is_rejected_quickly_and_error_text_is_clipped() {
    let p = Position::new();
    let big = "N".repeat(1_000_000);
    let e = p.parse_san(&big, SanMode::Tolerant).unwrap_err();
    assert!(e.to_string().len() < 2000, "error message must be bounded");
    let big2 = format!("{}e4", "x".repeat(100_000));
    let e = p.parse_san(&big2, SanMode::Tolerant).unwrap_err();
    assert!(e.to_string().len() < 2000);
}

#[rstest]
#[case("é4")]
#[case("♘f3")]
#[case("e4\u{0}")]
#[case("\u{feff}e4")]
#[case("Nf3 Nf6")]
#[case("e4 e5")]
#[case("1.e4")]
#[case("O-O-O-O")]
#[case("O-")]
#[case("-")]
#[case("+")]
#[case("x")]
#[case("Nxx")]
#[case("Zz")]
#[case("i9")]
#[case("a9")]
#[case("🙂")]
fn junk_is_an_error_not_a_panic(#[case] text: &str) {
    let p = Position::new();
    assert!(p.parse_san(text, SanMode::Tolerant).is_err(), "{text:?}");
    assert!(p.parse_san(text, SanMode::Strict).is_err(), "{text:?}");
}

// ---------- strict vs tolerant ----------

#[rstest]
#[case("Pe4")]
#[case("e2e4")]
#[case("e2-e4")]
#[case("Ng1-f3")]
fn strict_rejects_noncanonical_forms(#[case] text: &str) {
    let p = Position::new();
    assert!(
        matches!(
            p.parse_san(text, SanMode::Strict),
            Err(SanError::NonCanonical { .. })
        ),
        "{text}: {:?}",
        p.parse_san(text, SanMode::Strict)
    );
    assert!(p.parse_san(text, SanMode::Tolerant).is_ok());
}

#[test]
fn strict_rejects_missing_or_spurious_capture_marker() {
    let p = fen("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2");
    assert!(p.parse_san("exd5", SanMode::Strict).is_ok());
    assert!(matches!(
        p.parse_san("ed", SanMode::Strict),
        Err(SanError::NonCanonical { .. })
    ));
    // spurious x on a quiet move
    assert!(p.parse_san("Nxf3", SanMode::Strict).is_err());
    assert!(p.parse_san("Nxf3", SanMode::Tolerant).is_ok());
    // missing x on a capture
    let q = fen("4k3/8/8/4p3/8/5N2/8/4K3 w - - 0 1");
    assert!(p.parse_san("Nf3", SanMode::Strict).is_ok());
    assert!(q.parse_san("Ne5", SanMode::Strict).is_err());
    assert!(q.parse_san("Ne5", SanMode::Tolerant).is_ok());
}

#[test]
fn strict_rejects_redundant_disambiguation() {
    let p = Position::new();
    assert!(p.parse_san("Ng1f3", SanMode::Strict).is_err());
    assert!(p.parse_san("Ngf3", SanMode::Strict).is_err());
    assert!(p.parse_san("Nf3", SanMode::Strict).is_ok());
}

#[test]
fn strict_requires_equals_in_promotion_or_documents_otherwise() {
    let p = fen("8/4P2k/8/8/8/8/8/K7 w - - 0 1");
    assert!(p.parse_san("e8=Q", SanMode::Strict).is_ok());
    assert!(matches!(
        p.parse_san("e8Q", SanMode::Strict),
        Err(SanError::NonCanonical { .. })
    ));
    assert!(matches!(
        p.parse_san("e8q", SanMode::Strict),
        Err(SanError::NonCanonical { .. })
    ));
}

#[test]
fn strict_requires_uppercase_o_in_castling_and_null_forms() {
    let p = Position::new();
    assert!(p.parse_san("--", SanMode::Strict).is_ok());
    assert!(p.parse_san("null", SanMode::Strict).is_err());
    assert!(p.parse_san("Z0", SanMode::Strict).is_err());
}

#[test]
fn strict_accepts_check_and_mate_suffixes() {
    let p = fen("rnbqkbnr/pppp1ppp/8/4p3/6P1/5P2/PPPPP2P/RNBQKBNR b KQkq - 0 2");
    assert!(p.parse_san("Qh4#", SanMode::Strict).is_ok());
    assert!(p.parse_san("Qh4", SanMode::Strict).is_ok());
}

// ---------- SAN output ----------

#[test]
fn san_output_basic_moves() {
    let p = Position::new();
    let mv = |u: &str| p.parse_uci(u).unwrap();
    assert_eq!(p.san(mv("e2e4")).unwrap(), "e4");
    assert_eq!(p.san(mv("g1f3")).unwrap(), "Nf3");
    assert_eq!(p.san(mv("b1c3")).unwrap(), "Nc3");
}

#[test]
fn san_output_castling_uses_capital_o() {
    let p = fen(BOTH_CASTLES_W);
    assert_eq!(p.san(tol(&p, "0-0")).unwrap(), "O-O");
    assert_eq!(
        p.san_with(tol(&p, "0-0-0"), SanSuffix::None).unwrap(),
        "O-O-O"
    );
}

#[test]
fn san_output_castle_with_check_has_plus() {
    // Rook lands on f1 and checks the king on f8.
    let p = fen("5k2/8/8/8/8/8/8/4K2R w K - 0 1");
    let m = tol(&p, "O-O");
    assert_eq!(p.san(m).unwrap(), "O-O+");
    assert_eq!(p.san_with(m, SanSuffix::None).unwrap(), "O-O");
}

#[test]
fn san_output_check_and_mate_suffixes() {
    // Scholar's mate
    let mut p = Position::new();
    for t in ["e4", "e5", "Bc4", "Nc6", "Qh5", "Nf6"] {
        let m = tol(&p, t);
        p.play(m).unwrap();
    }
    let m = tol(&p, "Qxf7");
    assert_eq!(p.san(m).unwrap(), "Qxf7#");
    assert_eq!(p.san_with(m, SanSuffix::CheckOrMate).unwrap(), "Qxf7#");
    assert_eq!(p.san_with(m, SanSuffix::Check).unwrap(), "Qxf7+");
    assert_eq!(p.san_with(m, SanSuffix::None).unwrap(), "Qxf7");
    let check_only = tol(&p, "Bxf7");
    assert_eq!(p.san(check_only).unwrap(), "Bxf7+");
}

#[test]
fn double_check_is_written_with_single_plus() {
    let p = fen("4k3/8/8/8/4N3/8/8/4R1K1 w - - 0 1");
    let m = tol(&p, "Nd6");
    let after = p.after(m).unwrap();
    assert!(after.is_check());
    assert_eq!(p.san(m).unwrap(), "Nd6+");
    assert!(!p.san(m).unwrap().contains("++"));
}

#[test]
fn three_queens_disambiguation_uses_file_then_rank_then_both() {
    let p = fen("6k1/8/8/8/8/Q7/8/Q1Q4K w - - 0 1");
    let mv = |u: &str| p.parse_uci(u).unwrap();
    assert_eq!(p.san_with(mv("a1b2"), SanSuffix::None).unwrap(), "Qa1b2");
    assert_eq!(p.san_with(mv("a3b2"), SanSuffix::None).unwrap(), "Q3b2");
    assert_eq!(p.san_with(mv("c1b2"), SanSuffix::None).unwrap(), "Qcb2");
    // and they all read back
    for (san, u) in [("Qa1b2", "a1b2"), ("Q3b2", "a3b2"), ("Qcb2", "c1b2")] {
        assert_eq!(p.uci(tol(&p, san)), u);
        assert_eq!(p.uci(p.parse_san(san, SanMode::Strict).unwrap().mv), u);
    }
}

#[test]
fn two_pieces_disambiguation_prefers_file() {
    let p = fen("4k3/8/8/8/8/8/8/1N2KN2 w - - 0 1");
    assert_eq!(
        p.san_with(p.parse_uci("b1d2").unwrap(), SanSuffix::None)
            .unwrap(),
        "Nbd2"
    );
    assert_eq!(
        p.san_with(p.parse_uci("f1d2").unwrap(), SanSuffix::None)
            .unwrap(),
        "Nfd2"
    );
}

#[test]
fn two_pieces_on_same_file_use_rank() {
    let p = fen("7k/R7/8/8/8/8/8/R3K3 w - - 0 1");
    assert_eq!(
        p.san_with(p.parse_uci("a1a4").unwrap(), SanSuffix::None)
            .unwrap(),
        "R1a4"
    );
    assert_eq!(
        p.san_with(p.parse_uci("a7a4").unwrap(), SanSuffix::None)
            .unwrap(),
        "R7a4"
    );
}

#[test]
fn pinned_piece_does_not_force_disambiguation() {
    // Scid fix of 2017 (position.cpp:2194): only legal moves count.
    let p = fen("5r1k/8/8/8/8/5N2/8/1N3K2 w - - 0 1");
    let m = p.parse_uci("b1d2").unwrap();
    assert!(p.legal_moves().iter().all(|x| x.from() != Some(sq("f3"))));
    assert_eq!(p.san_with(m, SanSuffix::None).unwrap(), "Nd2");
}

#[test]
fn pawn_capture_always_names_the_file() {
    let p = fen("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2");
    assert_eq!(p.san(p.parse_uci("e4d5").unwrap()).unwrap(), "exd5");
}

#[test]
fn san_of_illegal_move_is_error() {
    let p = Position::new();
    let bogus = Move::Normal {
        role: Role::Queen,
        from: sq("d1"),
        to: sq("d5"),
        capture: None,
        promotion: None,
    };
    assert!(p.san(bogus).is_err());
    let castle = Move::Castle {
        king_from: sq("e1"),
        rook_from: sq("h1"),
    };
    assert!(p.san(castle).is_err());
}

// ---------- properties ----------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn san_roundtrips_for_every_legal_move(start in 0..START_COUNT, choices in proptest::collection::vec(any::<u16>(), 0..100)) {
        let game = random_game(&start_position(start), &choices);
        for (pos, _) in &game {
            for m in pos.legal_moves() {
                let text = pos.san(m).expect("legal move has SAN");
                for mode in [SanMode::Strict, SanMode::Tolerant] {
                    let parsed = pos.parse_san(&text, mode);
                    prop_assert_eq!(parsed.as_ref().map(|r| r.mv), Ok(m), "{} {} {:?}", pos.fen(), text, mode);
                }
                for sfx in [SanSuffix::None, SanSuffix::Check] {
                    let t = pos.san_with(m, sfx).unwrap();
                    prop_assert_eq!(pos.parse_san(&t, SanMode::Strict).map(|r| r.mv), Ok(m), "{} {}", pos.fen(), t);
                }
            }
        }
    }

    #[test]
    fn san_suffix_matches_resulting_position(start in 0..START_COUNT, choices in proptest::collection::vec(any::<u16>(), 0..80)) {
        let game = random_game(&start_position(start), &choices);
        for (pos, _) in &game {
            for m in pos.legal_moves() {
                let text = pos.san(m).unwrap();
                let after = pos.after(m).unwrap();
                let expected = if after.is_checkmate() { Some('#') } else if after.is_check() { Some('+') } else { None };
                prop_assert_eq!(text.chars().last().filter(|c| *c == '#' || *c == '+'), expected, "{} {}", pos.fen(), text);
            }
        }
    }

    #[test]
    fn parse_san_never_panics_on_arbitrary_text(s in "\\PC{0,300}") {
        let p = Position::new();
        let _ = p.parse_san(&s, SanMode::Tolerant);
        let _ = p.parse_san(&s, SanMode::Strict);
    }

    #[test]
    fn parse_san_never_panics_on_sanlike_text(s in "[KQRBNPabcdefghxO0o1-8=+#!?:.\\- ZNul_pe]{0,40}") {
        for p in [Position::new(), fen(BOTH_CASTLES_W), fen("8/4P2k/8/8/8/8/8/K7 w - - 0 1")] {
            let _ = p.parse_san(&s, SanMode::Tolerant);
            let _ = p.parse_san(&s, SanMode::Strict);
        }
    }

    #[test]
    fn parse_san_never_panics_on_random_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let s = String::from_utf8_lossy(&bytes).into_owned();
        let p = Position::new();
        let _ = p.parse_san(&s, SanMode::Tolerant);
    }

    #[test]
    fn parse_san_never_panics_on_very_long_text(n in 1000usize..200_000, ch in prop::sample::select(vec!['N', 'x', 'e', '4', 'O', '-', '=', 'é'])) {
        let s: String = std::iter::repeat_n(ch, n).collect();
        let p = Position::new();
        prop_assert!(p.parse_san(&s, SanMode::Tolerant).is_err());
    }
}
