//! Unit tests of the pieces of the material search.
#![allow(clippy::field_reassign_with_default)]

use super::*;

fn piece(color: Color, role: Role) -> Piece {
    Piece { color, role }
}

#[test]
fn a_flipped_pattern_mirrors_the_rank_and_swaps_the_colour() {
    let p = Pattern::on_square(piece(Color::White, Role::Pawn), File::A, Rank::Second, true);
    let f = p.flipped();
    assert_eq!(f.piece, Some(piece(Color::Black, Role::Pawn)));
    assert_eq!((f.file, f.rank), (Some(File::A), Some(Rank::Seventh)));
    assert_eq!(f.flipped(), p);
    // No rank stays no rank.
    let q = Pattern::on_file(piece(Color::Black, Role::Rook), File::C, false);
    assert_eq!(q.flipped().rank, None);
}

#[test]
fn only_no_pawn_on_a_file_excludes_home_pawns() {
    let no_white_c = Pattern::on_file(piece(Color::White, Role::Pawn), File::C, false);
    assert_eq!(no_white_c.home_pawn_bit(), Some(1 << 13));
    let no_black_h = Pattern::on_file(piece(Color::Black, Role::Pawn), File::H, false);
    assert_eq!(no_black_h.home_pawn_bit(), Some(1));
    assert_eq!(no_white_c.flipped().home_pawn_bit(), Some(1 << 5));
    let yes = Pattern::on_file(piece(Color::White, Role::Pawn), File::C, true);
    assert_eq!(yes.home_pawn_bit(), None);
    let on_rank = Pattern::on_square(piece(Color::White, Role::Pawn), File::C, Rank::Third, false);
    assert_eq!(on_rank.home_pawn_bit(), None);
    let knight = Pattern::on_file(piece(Color::White, Role::Knight), File::C, false);
    assert_eq!(knight.home_pawn_bit(), None);
}

#[test]
fn the_signature_of_the_maximums_is_not_used_for_unusual_searches() {
    let mut q = MaterialSearch::default();
    assert!(MaterialMatcher::new(&q).max_sig.is_some());
    q.white.rook = PieceRange::new(0, 3);
    assert!(MaterialMatcher::new(&q).max_sig.is_some());
    q.black.knight = PieceRange::new(0, 4);
    assert!(MaterialMatcher::new(&q).max_sig.is_none());
}

#[test]
fn the_move_range_becomes_a_ply_range() {
    let mut q = MaterialSearch::default();
    let m = MaterialMatcher::new(&q);
    assert_eq!((m.min_ply, m.max_ply), (1, 1998));
    q.moves = (0, 10);
    let m = MaterialMatcher::new(&q);
    assert_eq!((m.min_ply, m.max_ply), (0, 20));
    q.end_only = true;
    q.min_plies = 9;
    let m = MaterialMatcher::new(&q);
    assert_eq!((m.min_ply, m.need), (0, 1));
}

#[test]
fn normalising_is_idempotent_and_swaps_a_reversed_difference() {
    let mut q = MaterialSearch::default();
    q.diff = Some((10, -10));
    q.white.rook = PieceRange::new(2, 0);
    let n = q.normalized();
    assert_eq!(n.diff, Some((-10, 10)));
    assert_eq!(n.white.rook, PieceRange::new(2, 2));
    assert_eq!(n.normalized(), n);
}
