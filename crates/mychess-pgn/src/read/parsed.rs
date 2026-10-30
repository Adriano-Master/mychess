//! The result of reading one game.

use std::ops::Range;

use mychess_core::Game;

use super::diagnostics::Diagnostic;

/// Why the reading of a game stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Termination {
    /// A result token (`1-0`, `0-1`, `1/2-1/2`, `*`...) ended the game.
    ResultToken,
    /// A `[` at the start of a line (the next game's header) ended the game
    /// without a result token.
    NextGameHeader,
    /// The input ended.
    EndOfInput,
    /// The game is of an unsupported variant; only the header was read.
    UnsupportedVariant,
    /// The reading of the moves stopped at `max_nodes_per_game`; the rest of
    /// the game was not read.
    NodeLimit,
    /// The game was larger than `max_game_bytes`; only the header was read.
    Oversize,
}

/// A game read from a PGN stream, with what went wrong while reading it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ParsedGame {
    /// The game.
    pub game: Game,
    /// Number of the game in the file (1-based; games with errors count).
    pub number: u64,
    /// Line (1-based) of the first header tag, in the decompressed stream.
    pub first_line: u64,
    /// Byte range in the decompressed stream, from the first header tag to
    /// the end of the game.
    pub byte_range: Range<u64>,
    /// Why the reading stopped.
    pub termination: Termination,
    /// Text found between the previous game and this one (clipped to
    /// `max_comment_len`). Only filled with `PreGameText::AsComment`; it is
    /// `None` with `Ignore`, where the text is only reported by a
    /// `JunkBetweenGames` diagnostic.
    pub pre_game_text: Option<String>,
    /// Everything wrong with the content, in reading order.
    pub diagnostics: Vec<Diagnostic>,
}
