// Derived from Scid vs PC 4.27, src/position.cpp (`DoSimpleMove`, which fills
// the `simpleMoveT` fields) and src/tkscid.cpp (`sc_tree_search`). Copyright
// (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is distributed
// under the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The fields of a cache node that only the file has (decision D1041),
//! computed from the position of the entry on a best-effort basis: the piece
//! numbers come from the SI4 piece list of the position (which reproduces the
//! order of the Scid for positions reached from a start position) and castling
//! flags are deduced from the squares. The en passant square of the entry is
//! not known, so for a pawn that changes file onto an empty square the node is
//! described in a position with the en passant square set to the target
//! (decision D1063). When the position cannot be rebuilt the defaults are
//! written.

use mychess_core::{CastlingSide, Color, EcoBook, Move, Piece, Position, Role, Square};

use super::format::{NO_PIECE, NULL_SQUARE, NodeExtras, StcEntry, role_code, square_code};
use crate::si4::ListPosition;
use crate::tree::stats::score_permille;
use crate::tree::{TreeNode, same_board};

/// The FEN of an entry: castling rights from the squares, no en passant
/// (`ep` is the en passant target to write instead of `-`).
fn entry_fen(entry: &StcEntry, ep: Option<Square>) -> Option<String> {
    let mut fen = String::new();
    for rank in (0..8usize).rev() {
        let mut empty = 0;
        for file in 0..8usize {
            let code = *entry.board.get(rank * 8 + file)?;
            if code == NO_PIECE {
                empty += 1;
                continue;
            }
            if empty > 0 {
                fen.push(char::from_digit(empty, 10)?);
                empty = 0;
            }
            let letter = match code & 7 {
                1 => 'k',
                2 => 'q',
                3 => 'r',
                4 => 'b',
                5 => 'n',
                6 => 'p',
                _ => return None,
            };
            let black = match code {
                1..=6 => false,
                9..=14 => true,
                _ => return None,
            };
            fen.push(if black {
                letter
            } else {
                letter.to_ascii_uppercase()
            });
        }
        if empty > 0 {
            fen.push(char::from_digit(empty, 10)?);
        }
        if rank > 0 {
            fen.push('/');
        }
    }
    let at = |i: usize| entry.board.get(i).copied();
    let mut castling = String::new();
    // White K1 is square 4, rooks 0 and 7; black K9 is 60, rooks 56 and 63.
    if at(4) == Some(1) && at(7) == Some(3) {
        castling.push('K');
    }
    if at(4) == Some(1) && at(0) == Some(3) {
        castling.push('Q');
    }
    if at(60) == Some(9) && at(63) == Some(11) {
        castling.push('k');
    }
    if at(60) == Some(9) && at(56) == Some(11) {
        castling.push('q');
    }
    if castling.is_empty() {
        castling.push('-');
    }
    let side = if entry.to_move == Color::White {
        'w'
    } else {
        'b'
    };
    let ep = ep.map_or_else(|| "-".to_owned(), |sq| sq.to_string());
    Some(format!("{fen} {side} {castling} {ep} 0 1"))
}

/// The piece byte with the color bit.
fn piece_code(piece: Piece) -> u8 {
    role_code(piece.role) + if piece.color == Color::Black { 8 } else { 0 }
}

/// What is needed to describe the moves of one entry.
struct Context {
    /// The entry, as given.
    entry: StcEntry,
    pos: Option<Position>,
    list: Option<ListPosition>,
}

impl Context {
    fn new(entry: &StcEntry) -> Context {
        let fen = entry_fen(entry, None);
        let list = fen
            .as_deref()
            .and_then(|f| ListPosition::from_fen(f).ok())
            // The FEN order of the pieces is not the one of a game that
            // started from the standard position; use the real one there.
            .map(|from_fen| {
                let standard = ListPosition::standard();
                if same_board(&from_fen, &standard) {
                    standard
                } else {
                    from_fen
                }
            });
        Context {
            entry: entry.clone(),
            pos: fen.as_deref().and_then(|f| Position::from_fen(f).ok()),
            list,
        }
    }

    fn extras(&self, node: &TreeNode, eco_book: Option<&EcoBook>) -> NodeExtras {
        let mut extras = NodeExtras {
            score: score_permille(node.freq, u64::from(node.total)),
            ..NodeExtras::default()
        };
        let Some(base) = &self.pos else {
            extras.san = fallback_san(node);
            return extras;
        };
        // En passant needs the target square in the position to be legal.
        let ep_target = node
            .mv
            .and_then(|mv| en_passant_target(base, mv.from, mv.to));
        let ep_pos = ep_target
            .and_then(|sq| entry_fen(&self.entry, Some(sq)))
            .and_then(|fen| Position::from_fen(&fen).ok());
        let pos = ep_pos.as_ref().unwrap_or(base);
        if ep_pos.is_some() {
            extras.ep_square = ep_target.map_or(NULL_SQUARE, square_code);
        }
        let core_move = match node.mv {
            None => None,
            Some(mv) if mv.is_null() => Some(Move::Null),
            Some(mv) => pos.find_move(mv.from, mv.to, mv.promotion),
        };
        extras.san = match (node.mv, core_move) {
            (None, _) => "[end]".to_owned(),
            (Some(_), Some(m)) => pos.san(m).unwrap_or_else(|_| fallback_san(node)),
            (Some(_), None) => fallback_san(node),
        };
        // The ECO code is that of the position after the move (`[end]`: the
        // position itself).
        let after = match core_move {
            Some(m) => pos.after(m).ok(),
            None => Some(pos.clone()),
        };
        if let (Some(book), Some(after)) = (eco_book, &after) {
            extras.eco_code = book
                .lookup(after)
                .map_or(0, |entry| entry.eco().to_scid_code());
        }
        if let Some(mv) = node.mv {
            self.fill_move(pos, mv.from, mv.to, &mut extras);
        }
        extras.castle_flags = castle_flags(pos);
        extras
    }

    fn fill_move(&self, pos: &Position, from: Square, to: Square, extras: &mut NodeExtras) {
        let mover = pos.turn();
        let Some(piece) = pos.piece_at(from) else {
            return;
        };
        extras.moving_piece = piece_code(piece);
        extras.captured_square = square_code(to);
        let target = pos.piece_at(to).filter(|p| p.color != mover);
        // A pawn changing file onto an empty square is en passant: the
        // captured pawn stands behind the destination.
        let mut captured_square = to;
        let mut captured = target;
        if en_passant_target(pos, from, to).is_some() {
            let behind = if mover == Color::White { -8 } else { 8 };
            if let Some(sq) = to.offset(behind) {
                captured_square = sq;
                captured = pos.piece_at(sq);
            }
        }
        if let Some(captured) = captured {
            extras.captured_piece = piece_code(captured);
            extras.captured_square = square_code(captured_square);
        }
        if let Some(list) = &self.list {
            extras.piece_num = list.mover_index(from).unwrap_or(0);
            if captured.is_some() {
                extras.captured_num = list
                    .piece_squares(!mover)
                    .iter()
                    .position(|&sq| sq == captured_square)
                    .and_then(|i| u8::try_from(i).ok())
                    .unwrap_or(0);
            }
        }
    }
}

/// `to` if the move `from`-`to` is an en passant capture in `pos`: a pawn that
/// changes file onto an empty square.
fn en_passant_target(pos: &Position, from: Square, to: Square) -> Option<Square> {
    let pawn = pos.piece_at(from).is_some_and(|p| p.role == Role::Pawn);
    (pawn && from.file() != to.file() && pos.piece_at(to).is_none()).then_some(to)
}

/// White queen side 1, king side 2, black queen side 4, king side 8.
fn castle_flags(pos: &Position) -> u8 {
    let mut flags = 0;
    for (color, side, bit) in [
        (Color::White, CastlingSide::QueenSide, 1),
        (Color::White, CastlingSide::KingSide, 2),
        (Color::Black, CastlingSide::QueenSide, 4),
        (Color::Black, CastlingSide::KingSide, 8),
    ] {
        if pos.has_castling_right(color, side) {
            flags |= bit;
        }
    }
    flags
}

/// The squares of the move, when the position is unknown or the move is not
/// legal there.
fn fallback_san(node: &TreeNode) -> String {
    match node.mv {
        None => "[end]".to_owned(),
        Some(mv) => format!("{}{}", mv.from, mv.to),
    }
}

/// The extras of every node of `entry`.
pub(super) fn entry_extras(entry: &StcEntry, eco_book: Option<&EcoBook>) -> Vec<NodeExtras> {
    let ctx = Context::new(entry);
    entry
        .nodes
        .iter()
        .map(|node| ctx.extras(node, eco_book))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::Filter;
    use crate::si4::RawMove;
    use crate::tree::stc::format::{CompressedFilter, NULL_SQUARE, board_key};

    fn entry(pos: &Position, nodes: Vec<TreeNode>) -> StcEntry {
        StcEntry {
            to_move: pos.turn(),
            board: board_key(pos),
            total_count: 1,
            nodes,
            filter: CompressedFilter::compress(&Filter::new(1)),
        }
    }

    fn node(from: Square, to: Square) -> TreeNode {
        TreeNode {
            mv: Some(RawMove {
                from,
                to,
                promotion: None,
            }),
            total: 3,
            freq: [3, 0, 0, 0],
            elo_sum: 0,
            elo_count: 0,
            perf_sum: 0,
            perf_count: 0,
            year_sum: 0,
            year_count: 0,
        }
    }

    #[test]
    fn describes_the_first_move_of_the_start_position() {
        let pos = Position::new();
        let mut end = node(Square::E2, Square::E4);
        end.mv = None;
        let extras = entry_extras(&entry(&pos, vec![node(Square::E2, Square::E4), end]), None);
        let [e4, end] = extras.as_slice() else {
            panic!("two nodes");
        };
        assert_eq!(e4.san, "e4");
        // Pawn e2 is index 8 + 4 = 12 in the standard list; white pawn = 6.
        assert_eq!((e4.piece_num, e4.moving_piece), (12, 6));
        assert_eq!((e4.captured_piece, e4.ep_square), (NO_PIECE, NULL_SQUARE));
        assert_eq!(e4.castle_flags, 15);
        assert_eq!(e4.score, 1000);
        assert_eq!(end.san, "[end]");
    }

    #[test]
    fn en_passant_gets_its_own_san_and_victim() {
        // White pawn e5, black pawn d5 (just moved): exd6 e.p.
        let pos = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").expect("fen");
        let extras = entry_extras(&entry(&pos, vec![node(Square::E5, Square::D6)]), None);
        assert_eq!(extras[0].san, "exd6");
        assert_eq!(extras[0].captured_piece, 14);
        assert_eq!(extras[0].captured_square, square_code(Square::D5));
        assert_eq!(extras[0].ep_square, square_code(Square::D6));
    }

    #[test]
    fn captures_record_the_victim() {
        let pos = Position::from_fen("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1").expect("fen");
        let extras = entry_extras(&entry(&pos, vec![node(Square::E4, Square::D5)]), None);
        assert_eq!(extras[0].san, "exd5");
        assert_eq!(extras[0].captured_piece, 14);
        assert_eq!(extras[0].captured_square, square_code(Square::D5));
    }
}
