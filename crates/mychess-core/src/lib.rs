//! Chess domain model for MyChess.
//!
//! This is the innermost layer of the architecture (`docs/arquitetura.md`): it
//! knows chess and nothing about files, processes or user interfaces.
//!
//! Contents so far (milestone M1, slice 1):
//! - [`Position`]: legal moves and game-ending rules for standard chess,
//!   built on the `shakmaty` crate but exposing only `mychess-core` types
//!   (plus a few plain `shakmaty` value types, re-exported below);
//! - notation: tolerant FEN reading and FEN writing
//!   ([`Position::from_fen`], [`Position::fen`]), SAN reading in
//!   [`SanMode::Strict`] and [`SanMode::Tolerant`] modes and SAN writing
//!   ([`Position::parse_san`], [`Position::san`]), UCI move strings
//!   ([`Position::parse_uci`], [`Position::uci`]);
//! - [`Move`], including the null move;
//! - [`GameEnd`] detection and stable [`ZobristHash`] values.
//!
//! Also (milestone M1, slice 2, first half): the building blocks of a game
//! record, ready for the game tree that comes next:
//! - [`Nag`]: numeric annotation glyphs, with Scid's symbol parsing and
//!   display table;
//! - [`PartialDate`], [`Eco`] (Scid's extended codes and internal numbers),
//!   and [`Tags`] with [`GameResult`], [`Rating`] and [`Elo`];
//! - commands embedded in comments: [`strip_mark_codes`],
//!   [`embedded_commands`], [`toggle_mark`], [`eval_of`], [`clock_of`],
//!   [`emt_of`].
//!
//! Second half of slice 2: [`Game`], the game tree in an arena addressed by
//! [`NodeId`], with variations, comments and NAGs.
//!
//! Chess960 (milestone M1, slice 5): [`Position::chess960_start`] and
//! [`Position::chess960_number`] (Scharnagl numbering, 518 is the standard
//! arrangement), [`FenMode`] and [`FenCastlingStyle`] for X-FEN and
//! Shredder-FEN, [`Position::castling_rook`] and [`Game::variant`]. The castling
//! mode is stored in each [`Position`]; castling moves are identified by king
//! and rook origin squares, so one [`Move`] type serves both rule sets.
//!
//! ```
//! use mychess_core::{CastlingMode, Position, Variant};
//!
//! let pos = Position::chess960_start(518)?;
//! assert_eq!(pos.variant(), Variant::Chess960);
//! assert_eq!(pos.castling_mode(), CastlingMode::Chess960);
//! assert_eq!(pos, Position::new()); // same arrangement, same position
//! # Ok::<(), mychess_core::Chess960Error>(())
//! ```
//!
//! The `test-support` feature (off by default) exposes `proptest` strategies
//! in a `test_support` module for the tests of other crates.
//!
//! Dependency rule: this crate depends on no other `mychess-*` crate.

mod chess960;
mod comments;
mod date;
mod eco;
mod eco_book;
mod error;
mod fen;
mod game;
mod game_end;
pub mod mate;
mod merge;
mod moves;
mod nag;
mod position;
mod san;
pub mod tactics;
mod tags;
mod uci;

#[cfg(feature = "test-support")]
pub mod test_support;

pub use chess960::Variant;
pub use comments::{
    BoardMark, DEFAULT_MARK_COLOR, EmbeddedCommand, MATE_SCORE, MarkKind, chessbase_color,
    clock_of, embedded_commands, emt_of, eval_of, strip_mark_codes, toggle_mark,
};
pub use date::{DateError, MAX_YEAR, PartialDate};
pub use eco::{Eco, EcoError};
pub use eco_book::{
    ClassifyOptions, CorruptReason, EcoBook, EcoBookError, EcoDiagnostic, EcoEntry, EcoMatch,
    EcoParseOptions, EcoReport, OnError, translate_eco_name,
};
pub use error::{
    BoardProblem, Chess960Error, FenError, IllegalPositionReason, MoveError, SanError, UciError,
};
pub use fen::{EnPassantFen, FenCastlingStyle, FenMode};
pub use game::{Ancestors, Descendants, Game, GameError, Mainline, NodeId};
pub use game_end::GameEnd;
pub use merge::{MergeCommentParts, MergeError, MergeOptions, MergeReport, merge_comment_parts};
pub use moves::Move;
pub use nag::{Nag, NagError};
pub use position::{Position, ZobristHash};
pub use san::{SanMode, SanParse, SanSuffix, SanWarning};
pub use tags::{Elo, GameResult, MAX_ELO, Rating, RatingType, TagError, Tags};

// Plain value types shared with `shakmaty`.
pub use shakmaty::{CastlingMode, CastlingSide, Color, File, Piece, Rank, Role, Square};
