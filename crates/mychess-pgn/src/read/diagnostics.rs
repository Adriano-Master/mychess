//! Diagnostics: everything wrong with the content of a PGN that is not fatal.

use std::fmt;

use mychess_core::SanError;
use thiserror::Error;

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Severity {
    /// Informative; nothing was lost.
    Info,
    /// Something was tolerated or repaired.
    Warning,
    /// Something was lost or could not be read.
    Error,
}

/// What a [`Diagnostic`] reports. Messages are English, for logs; the GUI
/// translates from the variant.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// A tag could not be read and was dropped.
    #[error("malformed tag")]
    BadTag,
    /// A tag name or value exceeds the safety limit and was truncated.
    #[error("tag too long, truncated")]
    TagTooLong,
    /// A rating above 4000 was cut to 4000.
    #[error("rating too large, set to 4000")]
    RatingTooLarge,
    /// The `FEN` tag is not a valid position.
    #[error("invalid FEN, using the standard start position")]
    InvalidFen,
    /// Castling was accepted although the right was lost.
    #[error("illegal castling accepted")]
    IllegalCastling,
    /// `OO` (short) was impossible and was read as long castling.
    #[error("short castling read as long castling")]
    CastlingReinterpreted,
    /// The move is not legal in the position.
    #[error("illegal move: {0}")]
    IllegalMove(SanError),
    /// More than one legal move matches the text.
    #[error("ambiguous move: {0}")]
    AmbiguousMove(SanError),
    /// The move text is understood but not acceptable (promotion, strict mode).
    #[error("invalid move: {0}")]
    InvalidMove(SanError),
    /// The game has more nodes than the game tree can address.
    #[error("too many nodes in the game")]
    TooManyNodes,
    /// More extra tags than `max_tags_per_game`; the rest are dropped.
    #[error("too many tags, the rest are ignored")]
    TooManyTags,
    /// A move already exists at this point of the tree (a duplicate
    /// variation); its data was merged into the existing node.
    #[error("duplicate move merged with an existing one")]
    DuplicateVariation,
    /// An annotation symbol that is not a known NAG.
    #[error("invalid annotation symbol")]
    InvalidNagSymbol,
    /// A `$n` NAG above 255.
    #[error("NAG out of range")]
    NagOutOfRange,
    /// A NAG before the first move of its line.
    #[error("NAG without a move")]
    NagWithoutMove,
    /// `(` before the first move of the line; the variation was skipped.
    #[error("variation before the first move")]
    VariationBeforeFirstMove,
    /// `)` without a matching `(`.
    #[error("unbalanced closing parenthesis")]
    UnbalancedCloseParen,
    /// A variation still open at the end of the game.
    #[error("unclosed variation")]
    UnclosedVariation,
    /// `}` outside a comment.
    #[error("\"}}\" outside a comment")]
    StrayCloseBrace,
    /// `{` without `}` before the end of the input. As in the Scid, the
    /// comment swallows everything that follows, the next games included
    /// (the chunker cannot tell a header from comment text).
    #[error("unterminated comment")]
    UnterminatedComment,
    /// A comment above the size limit was truncated.
    #[error("comment truncated")]
    CommentTruncated,
    /// The game ended without a result token.
    #[error("result token missing")]
    MissingResult,
    /// A word that is neither a move, a number, a NAG nor a result.
    #[error("unexpected token")]
    UnexpectedToken,
    /// The result token differs from the `Result` tag; the token wins.
    #[error("result does not match the header")]
    ResultMismatch,
    /// Text that does not belong in the header.
    #[error("invalid text in the header")]
    InvalidTextInHeader,
    /// The input ended inside the header section.
    #[error("end of input in the header")]
    HeaderOnlyAtEof,
    /// Text between two games.
    #[error("text between games")]
    JunkBetweenGames,
    /// Text that is not valid UTF-8 was decoded with the fallback encoding.
    #[error("invalid UTF-8, using the fallback encoding")]
    EncodingFallback,
    /// The `FEN` tag (without a `Variant` tag) holds a position whose castling
    /// rights only make sense in Chess960; the game was read as Chess960.
    #[error("Chess960 position detected")]
    Chess960Detected,
    /// The game is of a variant that is not supported.
    #[error("unsupported variant")]
    UnsupportedVariant,
    /// The game exceeds `max_game_bytes` and was skipped.
    #[error("game too large, skipped")]
    GameTooLarge,
    /// More diagnostics than `max_diagnostics_per_game`; the rest are dropped.
    #[error("too many diagnostics")]
    TooManyDiagnostics,
}

impl DiagnosticKind {
    /// The severity of this kind of diagnostic.
    pub fn severity(&self) -> Severity {
        use DiagnosticKind::{
            AmbiguousMove, BadTag, CastlingReinterpreted, Chess960Detected, CommentTruncated,
            DuplicateVariation, EncodingFallback, GameTooLarge, HeaderOnlyAtEof, IllegalCastling,
            IllegalMove, InvalidFen, InvalidMove, InvalidNagSymbol, InvalidTextInHeader,
            JunkBetweenGames, MissingResult, NagOutOfRange, NagWithoutMove, RatingTooLarge,
            ResultMismatch, StrayCloseBrace, TagTooLong, TooManyDiagnostics, TooManyNodes,
            TooManyTags, UnbalancedCloseParen, UnclosedVariation, UnexpectedToken,
            UnsupportedVariant, UnterminatedComment, VariationBeforeFirstMove,
        };
        match self {
            JunkBetweenGames | EncodingFallback | Chess960Detected => Severity::Info,
            TagTooLong
            | RatingTooLarge
            | IllegalCastling
            | CastlingReinterpreted
            | DuplicateVariation
            | InvalidNagSymbol
            | NagOutOfRange
            | NagWithoutMove
            | UnbalancedCloseParen
            | UnclosedVariation
            | StrayCloseBrace
            | CommentTruncated
            | MissingResult
            | ResultMismatch
            | InvalidTextInHeader
            | HeaderOnlyAtEof
            | UnsupportedVariant
            | TooManyTags
            | TooManyDiagnostics => Severity::Warning,
            BadTag
            | InvalidFen
            | IllegalMove(_)
            | AmbiguousMove(_)
            | InvalidMove(_)
            | TooManyNodes
            | VariationBeforeFirstMove
            | UnterminatedComment
            | UnexpectedToken
            | GameTooLarge => Severity::Error,
        }
    }
}

/// One remark about the content of a PGN game.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Diagnostic {
    /// How serious it is (`kind.severity()`).
    pub severity: Severity,
    /// What happened.
    pub kind: DiagnosticKind,
    /// Number of the game (1-based, in file order).
    pub game: u64,
    /// Line (1-based) of the first byte of the token or tag, counted in the
    /// decompressed stream with `\n`, `\r\n` and `\r` as line breaks.
    pub line: u64,
    /// Column (1-based, in bytes) in that line.
    pub column: u32,
    /// Byte offset from the start of the decompressed stream.
    pub offset: u64,
    /// The offending text (clipped), when there is one.
    pub text: Option<String>,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "(game {}, line {}:{}) {}",
            self.game, self.line, self.column, self.kind
        )?;
        if let Some(text) = &self.text {
            write!(f, ": {text}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severities_follow_the_spec_table() {
        assert_eq!(DiagnosticKind::BadTag.severity(), Severity::Error);
        assert_eq!(DiagnosticKind::RatingTooLarge.severity(), Severity::Warning);
        assert_eq!(DiagnosticKind::JunkBetweenGames.severity(), Severity::Info);
        assert_eq!(DiagnosticKind::UnexpectedToken.severity(), Severity::Error);
        assert!(Severity::Info < Severity::Warning && Severity::Warning < Severity::Error);
    }

    #[test]
    fn display_includes_position_and_text() {
        let d = Diagnostic {
            severity: Severity::Error,
            kind: DiagnosticKind::UnexpectedToken,
            game: 3,
            line: 7,
            column: 2,
            offset: 40,
            text: Some("xyz".to_owned()),
        };
        assert_eq!(d.to_string(), "(game 3, line 7:2) unexpected token: xyz");
        assert_eq!(
            DiagnosticKind::StrayCloseBrace.to_string(),
            "\"}\" outside a comment"
        );
    }
}
