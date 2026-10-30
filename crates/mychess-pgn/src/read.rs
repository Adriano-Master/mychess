//! Tolerant, streaming PGN reader modelled on Scid's `pgnparse.cpp` (ADR 0006,
//! `docs/specs/m1-3-pgn.md`).
//!
//! # Layers
//!
//! 1. [`GameChunker`] cuts a byte stream (plain or gzip, with or without a
//!    BOM, any line-break style) into [`RawGame`]s without parsing them.
//! 2. [`RawGame::parse`] turns one `RawGame` into a [`ParsedGame`]: a pure
//!    function, so callers may parse in parallel and keep the order.
//! 3. [`PgnReader`] chains the two as an iterator; [`parse_pgn_text`] and
//!    [`parse_movetext`] cover pasted text and loose moves.
//!
//! Nothing about the *content* is fatal: problems become [`Diagnostic`]s.
//! [`PgnError`] is only for I/O and gzip failures.
//!
//! Two deliberate simplifications of the spec: line breaks are not
//! normalised in a separate pass (the chunker and the lexers treat `\r\n`,
//! `\r` and `\n` alike on the original bytes, so offsets, lines and columns
//! are those of the file), and there is no `memchr` dependency (the slice
//! searches of `core` are enough for now; see the report).

mod chars;
mod chunker;
mod diagnostics;
mod error;
mod glyph;
mod header;
mod input;
mod movetext;
mod options;
mod parsed;
mod parser;
mod reader;
#[cfg(test)]
mod tests;
mod text;

pub use chunker::{GameChunker, RawGame};
pub use diagnostics::{Diagnostic, DiagnosticKind, Severity};
pub use error::PgnError;
pub use options::{
    CommentWhitespace, PlayerNames, PreGameText, ReadOptions, SemicolonComments, UnsupportedVariant,
};
pub use parsed::{ParsedGame, Termination};
pub use reader::{MovetextParse, PgnReader, parse_movetext, parse_pgn_text};
