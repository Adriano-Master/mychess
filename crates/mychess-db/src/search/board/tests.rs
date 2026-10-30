//! Unit tests of the helpers of the position search.

use mychess_core::Square;

use super::*;

fn list(fen: &str) -> ListPosition {
    ListPosition::from_fen(fen).unwrap()
}

#[test]
fn the_flipped_position_mirrors_the_ranks_and_swaps_colours_and_turn() {
    let pos = list("4k3/8/8/8/8/8/4P3/R3K3 w - - 0 1");
    let other = flipped(&pos).unwrap();
    assert_eq!(other.turn(), Color::Black);
    assert_eq!(
        other.piece_at(Square::E7),
        Some(Piece {
            color: Color::Black,
            role: Role::Pawn
        })
    );
    assert_eq!(
        other.piece_at(Square::A8),
        Some(Piece {
            color: Color::Black,
            role: Role::Rook
        })
    );
    assert_eq!(
        other.piece_at(Square::E1),
        Some(Piece {
            color: Color::White,
            role: Role::King
        })
    );
    // Flipping twice gives the position back.
    let back = flipped(&other).unwrap();
    assert!(
        Square::ALL
            .iter()
            .all(|&s| back.piece_at(s) == pos.piece_at(s))
    );
    assert_eq!(back.turn(), pos.turn());
}

#[test]
fn counts_say_when_a_later_position_can_no_longer_match() {
    let full = Counts::of(&ListPosition::standard());
    let fewer = Counts::of(&list("4k3/8/8/8/8/8/PPPPPPPP/4K3 w - - 0 1"));
    assert!(fewer.fewer_than(&full));
    assert!(!full.fewer_than(&fewer));
    assert!(!full.fewer_than(&full));
}

#[test]
fn the_filter_value_is_the_ply_plus_one_limited_to_255() {
    assert_eq!(filter_value(0), 1);
    assert_eq!(filter_value(253), 254);
    assert_eq!(filter_value(254), 255);
    assert_eq!(filter_value(1000), 255);
}

#[test]
fn files_compare_the_pawns_of_each_colour_per_file() {
    let a = Target::new(&list("4k3/8/8/8/4P3/8/8/4K3 w - - 0 1"));
    let b = list("4k3/8/8/8/8/4P3/8/4K3 w - - 0 1");
    let c = list("4k3/8/8/8/8/3P4/8/4K3 w - - 0 1");
    assert!(a.same_squares(BoardMatch::Files, &b));
    assert!(!a.same_squares(BoardMatch::Files, &c));
    assert!(!a.same_squares(BoardMatch::Pawns, &b));
    assert!(!a.same_squares(BoardMatch::Exact, &b));
    assert!(a.same_squares(BoardMatch::Material, &c));
}
