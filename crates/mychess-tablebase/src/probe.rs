//! The two ways to ask a tablebase and the checks both share (spec m8-8,
//! D2552, D2592).

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use mychess_core::{CastlingSide, Color, Position, Role};

use crate::category::PositionReport;
use crate::error::{Result, TablebaseError};

/// A boxed future, as the traits below return (no `async-trait` needed).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A source that answers at once, from memory or from local files (it may
/// block on file reads: call it from `spawn_blocking`).
///
/// This is also the hook for later uses (adjudication of tournaments, marks in
/// the analysis, D2592): they only need an `Arc<dyn PositionProbe>`.
pub trait PositionProbe: Send + Sync + fmt::Debug {
    /// The result of the position and of every legal move.
    ///
    /// # Errors
    /// [`TablebaseError`]: positions the source does not cover, missing or
    /// corrupted tables.
    fn probe(&self, position: &Position) -> Result<PositionReport>;

    /// The most pieces (kings included) the source covers; `0` when it
    /// covers nothing.
    fn max_pieces(&self) -> u8;
}

/// A source that needs the network.
pub trait OnlineProbe: Send + Sync + fmt::Debug {
    /// The result of the position and of every legal move.
    ///
    /// # Errors
    /// [`TablebaseError`]: positions the server does not cover, network
    /// failures, 429, answers that do not make sense.
    fn probe<'a>(&'a self, position: &'a Position) -> BoxFuture<'a, Result<PositionReport>>;
}

/// The most pieces the online tablebase covers.
pub const ONLINE_MAX_PIECES: u8 = 7;

/// The number of pieces on the board, kings included (saturating at 255).
#[must_use]
pub fn piece_count(position: &Position) -> u8 {
    u8::try_from(position.pieces().count()).unwrap_or(u8::MAX)
}

/// The key of a position for caches and for "did the position change?"
/// (D2586): the FEN without the move number, so the board, the side to move,
/// the castling rights, the en passant square and the 50-move clock count.
#[must_use]
pub fn position_key(position: &Position) -> String {
    let fen = position.fen();
    match fen.rsplit_once(' ') {
        Some((head, _move_number)) => head.to_owned(),
        None => fen,
    }
}

/// Whether the position can be looked up at all in a source that covers up to
/// `max_pieces` pieces; returns the piece count.
///
/// # Errors
/// [`TablebaseError::Castling`], [`TablebaseError::TooFewPieces`] or
/// [`TablebaseError::TooManyPieces`].
pub fn check_applicable(position: &Position, max_pieces: u8) -> Result<u8> {
    for color in [Color::White, Color::Black] {
        for side in [CastlingSide::KingSide, CastlingSide::QueenSide] {
            if position.has_castling_right(color, side) {
                return Err(TablebaseError::Castling);
            }
        }
    }
    let pieces = piece_count(position);
    if pieces < 3 {
        return Err(TablebaseError::TooFewPieces { pieces });
    }
    if pieces > max_pieces {
        return Err(TablebaseError::TooManyPieces {
            pieces,
            max: max_pieces,
        });
    }
    Ok(pieces)
}

/// The material of the position in the usual notation, `KQvKR`: the king,
/// then queens, rooks, bishops, knights and pawns, white then black. It is
/// the name of the Syzygy files (without the extension).
#[must_use]
pub fn material_name(position: &Position) -> String {
    let side = |color: Color| {
        let mut text = String::from("K");
        for (role, letter) in [
            (Role::Queen, 'Q'),
            (Role::Rook, 'R'),
            (Role::Bishop, 'B'),
            (Role::Knight, 'N'),
            (Role::Pawn, 'P'),
        ] {
            let count = position
                .pieces()
                .filter(|(_, piece)| piece.color == color && piece.role == role)
                .count();
            text.extend(std::iter::repeat_n(letter, count.min(16)));
        }
        text
    };
    format!("{}v{}", side(Color::White), side(Color::Black))
}
