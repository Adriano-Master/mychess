//! M2-3 move animation (rules 54-57): pure functions over positions. The
//! material strips (rule 52) left the board widget in M2-5 (D240); their
//! logic and tests live in `mychess_app::material`.

#![allow(clippy::assert_is_empty, clippy::float_cmp, clippy::unnecessary_wraps)]

use std::time::Duration;

use mychess_core::{CastlingSide, Color, Move, Piece, Position, Role, Square};
use mychess_gui::widgets::board::animation::{
    DEFAULT_DURATION, Direction, Sprite, animation_between, ease_out_cubic, progress,
};
use proptest::prelude::*;
use rstest::rstest;

const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn pos(fen: &str) -> Position {
    Position::from_fen(fen).expect("fen")
}

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn white(role: Role) -> Piece {
    Piece {
        color: Color::White,
        role,
    }
}

fn black(role: Role) -> Piece {
    Piece {
        color: Color::Black,
        role,
    }
}

fn play(fen: &str, from: &str, to: &str) -> (Position, Position, Move) {
    let old = pos(fen);
    let mv = old
        .find_move(sq(from), sq(to), None)
        .unwrap_or_else(|| panic!("{from}{to} is not legal in {fen}"));
    let new = old.after(mv).expect("legal");
    (old, new, mv)
}

// ---------------------------------------------------------------------------
// Material (rule 52) moved to `mychess_app::material` in M2-5 (D234, D240).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Animation detection (rules 54 and 55)
// ---------------------------------------------------------------------------

#[test]
fn m2_3_default_animation_duration_is_200_ms() {
    assert_eq!(DEFAULT_DURATION, Duration::from_millis(200));
}

#[test]
fn m2_3_pawn_push_slides_the_pawn() {
    let (old, new, _) = play(START, "e2", "e4");
    let a = animation_between(&old, &new).expect("animation");
    assert_eq!(a.direction, Direction::Forward);
    assert_eq!(a.duration, Duration::from_millis(200));
    assert_eq!(
        a.sprites.as_slice(),
        [Sprite {
            piece: white(Role::Pawn),
            from: sq("e2"),
            to: sq("e4")
        }]
    );
    assert_eq!(a.hidden.as_slice(), [sq("e4")]);
}

#[test]
fn m2_3_capture_slides_the_capturing_piece_and_hides_only_its_arrival() {
    let (old, new, _) = play("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", "e4", "d5");
    let a = animation_between(&old, &new).expect("animation");
    assert_eq!(
        a.sprites.as_slice(),
        [Sprite {
            piece: white(Role::Pawn),
            from: sq("e4"),
            to: sq("d5")
        }]
    );
    assert_eq!(a.hidden.as_slice(), [sq("d5")]);
}

#[test]
fn m2_3_en_passant_slides_the_capturer_and_the_victim_just_disappears() {
    let (old, new, mv) = play("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5", "d6");
    assert!(mv.is_en_passant());
    let a = animation_between(&old, &new).expect("animation");
    assert_eq!(
        a.sprites.as_slice(),
        [Sprite {
            piece: white(Role::Pawn),
            from: sq("e5"),
            to: sq("d6")
        }]
    );
    assert_eq!(a.hidden.as_slice(), [sq("d6")]);
}

#[test]
fn m2_3_promotion_forward_slides_the_promoted_piece() {
    let old = pos("4k3/P7/8/8/8/8/8/4K3 w - - 0 1");
    for role in [Role::Queen, Role::Rook, Role::Bishop, Role::Knight] {
        let mv = old
            .legal_moves_from(sq("a7"))
            .into_iter()
            .find(|m| m.promotion() == Some(role))
            .expect("promotion");
        let new = old.after(mv).expect("legal");
        let a = animation_between(&old, &new).expect("animation");
        assert_eq!(a.direction, Direction::Forward);
        assert_eq!(
            a.sprites.as_slice(),
            [Sprite {
                piece: white(role),
                from: sq("a7"),
                to: sq("a8")
            }]
        );
        assert_eq!(a.hidden.as_slice(), [sq("a8")]);
    }
}

#[test]
fn m2_3_promotion_backward_slides_the_pawn_back() {
    let old = pos("4k3/P7/8/8/8/8/8/4K3 w - - 0 1");
    let mv = old
        .legal_moves_from(sq("a7"))
        .into_iter()
        .find(|m| m.promotion() == Some(Role::Queen))
        .expect("promotion");
    let promoted = old.after(mv).expect("legal");
    let a = animation_between(&promoted, &old).expect("animation");
    assert_eq!(a.direction, Direction::Backward);
    assert_eq!(
        a.sprites.as_slice(),
        [Sprite {
            piece: white(Role::Pawn),
            from: sq("a8"),
            to: sq("a7")
        }]
    );
    assert_eq!(a.hidden.as_slice(), [sq("a7")]);
}

#[test]
fn m2_3_backward_move_reverses_the_sprite() {
    let (old, new, _) = play(START, "g1", "f3");
    let a = animation_between(&new, &old).expect("animation");
    assert_eq!(a.direction, Direction::Backward);
    assert_eq!(
        a.sprites.as_slice(),
        [Sprite {
            piece: white(Role::Knight),
            from: sq("f3"),
            to: sq("g1")
        }]
    );
    assert_eq!(a.hidden.as_slice(), [sq("g1")]);
}

#[test]
fn m2_3_backward_capture_brings_the_capturer_home_and_the_victim_is_already_there() {
    let (old, new, _) = play("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1", "e4", "d5");
    let a = animation_between(&new, &old).expect("animation");
    assert_eq!(a.direction, Direction::Backward);
    assert_eq!(
        a.sprites.as_slice(),
        [Sprite {
            piece: white(Role::Pawn),
            from: sq("d5"),
            to: sq("e4")
        }]
    );
    // d5 (where the victim reappears) is not hidden: it is drawn from the start.
    assert!(!a.hidden.contains(&sq("d5")));
    assert_eq!(old.piece_at(sq("d5")), Some(black(Role::Pawn)));
}

fn castle_sprites(fen: &str, king_from: &str, rook_from: &str) -> (Vec<Sprite>, Vec<Square>) {
    let old = pos(fen);
    let mv = old
        .legal_moves()
        .into_iter()
        .find(|m| matches!(m, Move::Castle { king_from: k, rook_from: r } if *k == sq(king_from) && *r == sq(rook_from)))
        .expect("castle");
    let new = old.after(mv).expect("legal");
    let a = animation_between(&old, &new).expect("animation");
    assert_eq!(a.direction, Direction::Forward);
    (a.sprites.to_vec(), a.hidden.to_vec())
}

#[rstest]
#[case(
    "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
    "e1",
    "h1",
    "g1",
    "f1",
    Color::White
)]
#[case(
    "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
    "e1",
    "a1",
    "c1",
    "d1",
    Color::White
)]
#[case(
    "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1",
    "e8",
    "h8",
    "g8",
    "f8",
    Color::Black
)]
#[case(
    "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1",
    "e8",
    "a8",
    "c8",
    "d8",
    Color::Black
)]
fn m2_3_castling_slides_king_and_rook(
    #[case] fen: &str,
    #[case] king_from: &str,
    #[case] rook_from: &str,
    #[case] king_to: &str,
    #[case] rook_to: &str,
    #[case] color: Color,
) {
    let (sprites, hidden) = castle_sprites(fen, king_from, rook_from);
    assert_eq!(sprites.len(), 2);
    assert!(sprites.contains(&Sprite {
        piece: Piece {
            color,
            role: Role::King
        },
        from: sq(king_from),
        to: sq(king_to)
    }));
    assert!(sprites.contains(&Sprite {
        piece: Piece {
            color,
            role: Role::Rook
        },
        from: sq(rook_from),
        to: sq(rook_to)
    }));
    assert_eq!(hidden.len(), 2);
    assert!(hidden.contains(&sq(king_to)) && hidden.contains(&sq(rook_to)));
}

#[test]
fn m2_3_castling_backward_slides_both_pieces_home() {
    let old = pos("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    let mv = old
        .legal_moves()
        .into_iter()
        .find(|m| m.castling_side() == Some(CastlingSide::KingSide))
        .expect("O-O");
    let new = old.after(mv).expect("legal");
    let a = animation_between(&new, &old).expect("animation");
    assert_eq!(a.direction, Direction::Backward);
    assert_eq!(a.sprites.len(), 2);
    assert!(a.sprites.contains(&Sprite {
        piece: white(Role::King),
        from: sq("g1"),
        to: sq("e1")
    }));
    assert!(a.sprites.contains(&Sprite {
        piece: white(Role::Rook),
        from: sq("f1"),
        to: sq("h1")
    }));
}

#[test]
fn m2_3_chess960_castling_slides_king_and_rook() {
    // King g1, rook f1 ("F"): O-O-O puts the king on c1 and the rook on d1.
    let (sprites, hidden) = castle_sprites("4k3/8/8/8/8/8/8/5RK1 w F - 0 1", "g1", "f1");
    assert!(sprites.contains(&Sprite {
        piece: white(Role::King),
        from: sq("g1"),
        to: sq("c1")
    }));
    assert!(sprites.contains(&Sprite {
        piece: white(Role::Rook),
        from: sq("f1"),
        to: sq("d1")
    }));
    assert!(hidden.contains(&sq("c1")) && hidden.contains(&sq("d1")));
}

#[test]
fn m2_3_chess960_castling_where_the_king_stays_still_slides_the_rook() {
    let (sprites, _) = castle_sprites("4k3/8/8/8/8/8/8/6KR w H - 0 1", "g1", "h1");
    assert!(sprites.contains(&Sprite {
        piece: white(Role::Rook),
        from: sq("h1"),
        to: sq("f1")
    }));
}

#[test]
fn m2_3_identical_positions_do_not_animate() {
    let p = pos(START);
    assert!(animation_between(&p, &p.clone()).is_none());
}

#[test]
fn m2_3_changing_only_the_side_to_move_does_not_animate() {
    let a = pos("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    let b = pos("4k3/8/8/8/8/8/8/4K3 b - - 0 1");
    assert!(
        animation_between(&a, &b).is_none(),
        "null move does not animate"
    );
    assert!(animation_between(&b, &a).is_none());
}

#[test]
fn m2_3_jumps_do_not_animate() {
    let p0 = pos(START);
    let (_, p1, _) = play(START, "e2", "e4");
    let (_, p2, _) = play(
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        "e7",
        "e5",
    );
    let (_, p3, _) = play(
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2",
        "g1",
        "f3",
    );
    assert!(animation_between(&p0, &p2).is_none(), "two moves forward");
    assert!(animation_between(&p2, &p0).is_none(), "two moves back");
    assert!(animation_between(&p0, &p3).is_none(), "three moves");
    assert!(animation_between(&p1, &p3).is_none());
    let unrelated = pos("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    assert!(animation_between(&p0, &unrelated).is_none());
    assert!(animation_between(&unrelated, &p0).is_none());
}

#[test]
fn m2_3_a_different_position_with_one_wrong_piece_does_not_animate() {
    let (old, _, _) = play(START, "e2", "e4");
    // Pawn went to e4 but a knight also moved.
    let other = pos("rnbqkbnr/pppppppp/8/8/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 1");
    assert!(animation_between(&pos(START), &other).is_none());
    let _ = old;
}

const FENS: [&str; 6] = [
    START,
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
];

#[test]
fn m2_3_every_legal_move_animates_forward_and_backward_consistently() {
    for fen in FENS {
        let old = pos(fen);
        for mv in old.legal_moves() {
            let new = old.after(mv).expect("legal");
            let fwd = animation_between(&old, &new)
                .unwrap_or_else(|| panic!("no forward animation for {mv:?} in {fen}"));
            assert_eq!(fwd.direction, Direction::Forward, "{mv:?}");
            let to = mv.to().expect("to");
            assert!(
                fwd.sprites.iter().any(|s| s.to == to),
                "{mv:?}: the moved piece must slide to {to}"
            );
            for s in &fwd.sprites {
                assert_eq!(new.piece_at(s.to), Some(s.piece), "sprite is the NEW piece");
                assert!(fwd.hidden.contains(&s.to), "arrival hidden");
            }
            assert_eq!(fwd.hidden.len(), fwd.sprites.len());
            assert!(fwd.sprites.len() <= 2);
            assert_eq!(
                fwd.sprites.len(),
                if mv.is_castle() { 2 } else { 1 },
                "{mv:?}"
            );
            if let (Some(from), false) = (mv.from(), mv.is_castle()) {
                assert_eq!(fwd.sprites[0].from, from, "{mv:?}");
            }
            let back = animation_between(&new, &old)
                .unwrap_or_else(|| panic!("no backward animation for {mv:?} in {fen}"));
            assert_eq!(back.direction, Direction::Backward, "{mv:?}");
            assert_eq!(back.sprites.len(), fwd.sprites.len());
            for s in &back.sprites {
                assert_eq!(
                    old.piece_at(s.to),
                    Some(s.piece),
                    "backward sprite is the piece of the position shown"
                );
                assert!(back.hidden.contains(&s.to));
            }
        }
    }
}

proptest! {
    #[test]
    fn m2_3_prop_two_plies_apart_never_animates(picks in proptest::collection::vec(0usize..1000, 2..3), start in 0usize..6) {
        let mut cur = pos(FENS[start]);
        let first = cur.clone();
        for pick in &picks {
            let moves = cur.legal_moves();
            if moves.is_empty() { return Ok(()); }
            cur = cur.after(moves[pick % moves.len()]).expect("legal");
        }
        // Two plies apart: either unrelated boards or (rarely) a transposed
        // board that equals one ply away; the former must be None.
        let one_ply = first.legal_moves().into_iter().any(|m| {
            first.after(m).is_ok_and(|a| a.zobrist() == cur.zobrist())
        });
        if !one_ply {
            let back = cur.legal_moves().into_iter().any(|m| {
                cur.after(m).is_ok_and(|a| a.zobrist() == first.zobrist())
            });
            if !back {
                prop_assert!(animation_between(&first, &cur).is_none());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Timing (rule 57): injected clock, easing
// ---------------------------------------------------------------------------

#[rstest]
#[case(0, 200, 0.0)]
#[case(100, 200, 0.5)]
#[case(50, 200, 0.25)]
#[case(200, 200, 1.0)]
#[case(1000, 200, 1.0)]
#[case(0, 0, 1.0)]
#[case(5, 0, 1.0)]
fn m2_3_progress_is_linear_and_clamped(
    #[case] elapsed: u64,
    #[case] total: u64,
    #[case] expected: f32,
) {
    let got = progress(Duration::from_millis(elapsed), Duration::from_millis(total));
    assert!((got - expected).abs() < 1e-6, "{got} != {expected}");
}

#[test]
fn m2_3_progress_survives_extreme_durations() {
    let p = progress(Duration::MAX, Duration::from_nanos(1));
    assert!((p - 1.0).abs() < f32::EPSILON);
    let p = progress(Duration::from_nanos(1), Duration::MAX);
    assert!((0.0..=1.0).contains(&p));
}

#[rstest]
#[case(0.0, 0.0)]
#[case(0.5, 0.875)]
#[case(1.0, 1.0)]
#[case(0.25, 0.578_125)]
#[case(-1.0, 0.0)]
#[case(-1000.0, 0.0)]
#[case(2.0, 1.0)]
#[case(f32::INFINITY, 1.0)]
#[case(f32::NEG_INFINITY, 0.0)]
fn m2_3_ease_out_cubic_values(#[case] t: f32, #[case] expected: f32) {
    assert!((ease_out_cubic(t) - expected).abs() < 1e-6, "{t}");
}

#[test]
fn m2_3_ease_out_cubic_of_nan_stays_in_range() {
    let v = ease_out_cubic(f32::NAN);
    assert!((0.0..=1.0).contains(&v), "{v}");
}

proptest! {
    #[test]
    fn m2_3_prop_ease_is_monotone_and_bounded(a in -2.0f32..3.0, b in -2.0f32..3.0) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let (e_lo, e_hi) = (ease_out_cubic(lo), ease_out_cubic(hi));
        prop_assert!((0.0..=1.0).contains(&e_lo) && (0.0..=1.0).contains(&e_hi));
        prop_assert!(e_lo <= e_hi + 1e-6);
        // ease-out is above the identity on [0, 1].
        if (0.0..=1.0).contains(&lo) {
            prop_assert!(e_lo >= lo - 1e-6);
        }
    }

    #[test]
    fn m2_3_prop_progress_is_bounded_and_monotone(e1 in 0u64..5000, e2 in 0u64..5000, d in 0u64..2000) {
        let dur = Duration::from_millis(d);
        let (lo, hi) = if e1 <= e2 { (e1, e2) } else { (e2, e1) };
        let (p1, p2) = (progress(Duration::from_millis(lo), dur), progress(Duration::from_millis(hi), dur));
        prop_assert!((0.0..=1.0).contains(&p1) && (0.0..=1.0).contains(&p2));
        prop_assert!(p1 <= p2);
    }
}
