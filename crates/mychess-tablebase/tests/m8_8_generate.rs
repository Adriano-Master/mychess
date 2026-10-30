//! Contract tests of the generator of training positions (spec m8-8, D2577;
//! acceptance criterion 6).

#![allow(clippy::assert_is_empty, clippy::unwrap_used, clippy::expect_used)]

use mychess_core::{Color, Position, Role};
use mychess_tablebase::generate::{GenerateError, parse_material, random_position};
use mychess_tablebase::{material_name, piece_count};
use proptest::prelude::*;

#[test]
fn parses_the_usual_spellings() {
    let m = parse_material("KRvKP").unwrap();
    assert_eq!(m.white, [Role::Rook]);
    assert_eq!(m.black, [Role::Pawn]);
    assert_eq!(parse_material("krpvkr").unwrap().pieces(), 5);
    assert_eq!(parse_material(" KQ-KR ").unwrap().pieces(), 4);
    assert_eq!(parse_material("KBNvK").unwrap().white.len(), 2);
}

#[test]
fn rejects_bad_materials_with_a_typed_error() {
    for bad in ["", "KQK", "KQvQ", "KKvK", "KXvK", "vK", "K v K"] {
        assert!(parse_material(bad).is_err(), "{bad:?}");
    }
    assert_eq!(parse_material("KvK"), Err(GenerateError::TooFewPieces(2)));
    assert!(matches!(
        parse_material("KQRBNvKQR"),
        Err(GenerateError::TooManyPieces(_))
    ));
    assert!(matches!(
        parse_material("KPPPPPPPPPvK"),
        Err(GenerateError::TooManyPieces(_))
    ));
    assert!(matches!(
        parse_material("KQvKQQQQQ"),
        Err(GenerateError::TooManyPieces(8))
    ));
    assert!(random_position("nonsense", 1).is_err());
}

fn assert_legal_and_playable(pos: &Position) {
    assert!(!pos.legal_moves().is_empty());
    assert!(!pos.is_insufficient_material());
    // The side that does not move is not in check: with the move passed it
    // would be able to take the king.
    let fen = pos.fen();
    let (from, to) = if pos.turn() == Color::White {
        (" w ", " b ")
    } else {
        (" b ", " w ")
    };
    let flipped = fen.replacen(from, to, 1);
    assert!(!Position::from_fen(&flipped).unwrap().is_check(), "{fen}");
    let wk = pos.king_square(Color::White).unwrap();
    let bk = pos.king_square(Color::Black).unwrap();
    assert!(
        wk.file().distance(bk.file()) > 1 || wk.rank().distance(bk.rank()) > 1,
        "adjacent kings in {fen}"
    );
    for (sq, piece) in pos.pieces() {
        if piece.role == Role::Pawn {
            let r = u32::from(sq.rank());
            assert!((1..=6).contains(&r), "pawn on the edge in {fen}");
        }
    }
}

#[test]
fn the_same_seed_gives_the_same_position() {
    let a = random_position("KRvKP", 42).unwrap();
    let b = random_position("KRvKP", 42).unwrap();
    assert_eq!(a.fen(), b.fen());
    let c = random_position("KRvKP", 43).unwrap();
    assert_ne!(a.fen(), c.fen());
}

#[test]
fn the_position_has_the_material_asked_for() {
    for text in [
        "KQvK", "KRvK", "KPvK", "KBNvK", "KRvKP", "KQvKR", "KRPvKR", "KQRvKBN",
    ] {
        let pos = random_position(text, 7).unwrap();
        assert_eq!(material_name(&pos), text);
        assert_eq!(
            usize::from(piece_count(&pos)),
            parse_material(text).unwrap().pieces()
        );
        assert_legal_and_playable(&pos);
    }
}

proptest! {
    #[test]
    fn every_seed_gives_a_legal_position(seed in any::<u64>(), pick in 0usize..6) {
        let materials = ["KQvK", "KRvK", "KPvK", "KRvKP", "KQvKR", "KBNvK"];
        let pos = random_position(materials[pick], seed).unwrap();
        assert_legal_and_playable(&pos);
    }
}
