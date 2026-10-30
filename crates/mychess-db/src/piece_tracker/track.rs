// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_base_piecetrack`).
// Copyright notices of src/tkscid.cpp: Copyright (c) 1999-2004 Shane Hudson;
// Copyright (c) 2006-2007 Pascal Georges; Copyright (c) 2009-2013 Stevenaaus.
// Scid is distributed under the GNU General Public License (see its COPYING);
// this file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The following of the pieces through one game (rules P1, P4 to P8).

use std::ops::ControlFlow;

use mychess_core::{Color, File, Role, Square};

use super::TrackMode;
use crate::si4::{ListPosition, RecordError, walk_mainline};

/// The plies `(first, last)` counted for the moves `start..=end` of the
/// window (rule P1, `tkscid.cpp` 1748-1757): `first = max(2 start, 2) - 1`,
/// `last = max(2 end, 2 start, 2)`. Move 0 is read as move 1 and an `end`
/// before `start` as `start`.
///
/// ```
/// use mychess_db::piece_track_plies;
///
/// assert_eq!(piece_track_plies(1, 20), (1, 40));
/// assert_eq!(piece_track_plies(0, 0), (1, 2));
/// assert_eq!(piece_track_plies(5, 3), (9, 10));
/// ```
pub fn piece_track_plies(start: u16, end: u16) -> (u32, u32) {
    let (start, end) = (u32::from(start), u32::from(end));
    let first = (2 * start).max(2) - 1;
    let last = (2 * end).max(2 * start).max(2);
    (first, last)
}

/// The mask bit of a square (a1 is bit 0, h8 is bit 63).
pub(crate) fn bit(square: Square) -> u64 {
    1u64 << square.to_u32()
}

/// Follows the squares of `start_mask` through the main line of `record`
/// and adds to `counts` (rules P4 to P6). `first` and `last` are the plies
/// from [`piece_track_plies`]. Nothing is added if the record is damaged
/// before the walk ends.
///
/// - A move whose destination holds a followed piece captures it (en passant:
///   the pawn behind the destination); the followed piece that moves from the
///   origin goes to the destination (a promotion keeps it followed).
/// - Castling is the move of the king **and** of the rook (rule P7 / D3390).
/// - A null move changes nothing but is a ply.
/// - Games mode: the destination of a followed piece is counted once per
///   game, from ply `first`. Time mode: after each ply from `first`, every
///   square that still holds a followed piece is counted (halving is up to
///   the caller).
/// - The walk ends at ply `last`, at the end of the game or when no piece is
///   followed any more (the ply of the last capture is not counted).
pub(crate) fn track_game(
    record: &[u8],
    first: u32,
    last: u32,
    mode: TrackMode,
    start_mask: u64,
    counts: &mut [u64; 64],
) -> Result<(), RecordError> {
    let mut local = [0u64; 64];
    let mut mask = start_mask;
    let mut moved_to = 0u64;
    let mut ply = 0u32;
    walk_mainline(record, |pos: &ListPosition, next| {
        let Some(mv) = next else {
            return ControlFlow::Break(());
        };
        ply += 1;
        if mv.from != mv.to {
            let piece = pos.piece_at(mv.from);
            // A capture removes the followed piece it lands on.
            let captured = if pos.piece_at(mv.to).is_some() {
                Some(mv.to)
            } else if let Some(pawn) = piece.filter(|p| p.role == Role::Pawn)
                && mv.from.file() != mv.to.file()
            {
                // En passant: the pawn is behind the destination.
                mv.to
                    .offset(if pawn.color == Color::White { -8 } else { 8 })
            } else {
                None
            };
            if let Some(square) = captured {
                mask &= !bit(square);
            }
            // Scid: castling is a king move from the e-file to the c- or
            // g-file (`sc_base_piecetrack`). Chess960 kings elsewhere on the
            // board are not recognised; that is inherited on purpose because
            // games with a `START` position are skipped before this point
            // (rule P3), so the king always starts on e1/e8 here.
            let rook = piece
                .filter(|p| p.role == Role::King && mv.from.file() == File::E)
                .and_then(|_| castle_rook(mv.to));
            let moves = [Some((mv.from, mv.to)), rook];
            for (from, to) in moves.into_iter().flatten() {
                if mask & bit(from) == 0 {
                    continue;
                }
                mask = (mask & !bit(from)) | bit(to);
                if mode == TrackMode::Games && ply >= first && moved_to & bit(to) == 0 {
                    moved_to |= bit(to);
                    if let Some(slot) = local.get_mut(to.to_usize()) {
                        *slot += 1;
                    }
                }
            }
        }
        if mask == 0 {
            return ControlFlow::Break(());
        }
        if mode == TrackMode::Time && ply >= first {
            for (index, slot) in local.iter_mut().enumerate() {
                if mask >> index & 1 == 1 {
                    *slot += 1;
                }
            }
        }
        if ply >= last {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    for (total, add) in counts.iter_mut().zip(local) {
        *total += add;
    }
    Ok(())
}

/// The rook move of a king move from the e-file to `to` on the c- or g-file,
/// as Scid recognises castling.
fn castle_rook(to: Square) -> Option<(Square, Square)> {
    match to.file() {
        File::G => Some((to.offset(1)?, to.offset(-1)?)),
        File::C => Some((to.offset(-2)?, to.offset(1)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plies_follow_the_conversion_of_scid() {
        assert_eq!(piece_track_plies(1, 20), (1, 40));
        assert_eq!(piece_track_plies(0, 0), (1, 2));
        assert_eq!(piece_track_plies(1, 1), (1, 2));
        assert_eq!(piece_track_plies(0, 5), (1, 10));
        assert_eq!(piece_track_plies(5, 3), (9, 10));
        assert_eq!(piece_track_plies(999, 999), (1997, 1998));
    }

    #[test]
    fn castling_rook_squares() {
        let sq = |s: &str| s.parse::<Square>().expect("square");
        assert_eq!(castle_rook(sq("g1")), Some((sq("h1"), sq("f1"))));
        assert_eq!(castle_rook(sq("c1")), Some((sq("a1"), sq("d1"))));
        assert_eq!(castle_rook(sq("g8")), Some((sq("h8"), sq("f8"))));
        assert_eq!(castle_rook(sq("c8")), Some((sq("a8"), sq("d8"))));
        assert_eq!(castle_rook(sq("e2")), None);
    }
}
