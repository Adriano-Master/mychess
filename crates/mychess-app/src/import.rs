//! The PGN import dialog and its structured report (spec M2-2, D119-D122;
//! Scid: `tcl/tools/import.tcl`).

use mychess_pgn::read::{Diagnostic, DiagnosticKind, Severity};

use crate::error::RequestId;

/// What limit stopped a scan (D124).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScanLimit {
    /// More than this many bytes (decompressed).
    Bytes(u64),
    /// More than this many games.
    Games(usize),
}

/// Whether the text had a header or was read as bare moves (D120).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportMode {
    /// A complete PGN game.
    #[default]
    FullGame,
    /// Only moves; the game starts from the standard position.
    MovesOnly,
}

/// What an [`ImportNotice`] reports.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportIssue {
    /// A diagnostic of the PGN reader, as it is.
    Pgn(DiagnosticKind),
    /// The text has no move and no result.
    NothingToImport,
    /// Text that belongs to no game was skipped.
    TextOutsideGames {
        /// How many bytes.
        bytes: u64,
    },
    /// Reading stopped at a limit; the collection is incomplete.
    Truncated {
        /// Games kept.
        games_read: u64,
        /// The limit that was hit.
        limit: ScanLimit,
    },
}

impl ImportIssue {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            ImportIssue::Pgn(kind) => pgn_message_key(kind),
            ImportIssue::NothingToImport => "import-issue-nothing-to-import",
            ImportIssue::TextOutsideGames { .. } => "import-issue-text-outside-games",
            ImportIssue::Truncated { .. } => "import-issue-truncated",
        }
    }
}

/// The Fluent key of a reader diagnostic (`pgn-diag-<variant>`).
pub fn pgn_message_key(kind: &DiagnosticKind) -> &'static str {
    use DiagnosticKind as K;
    match kind {
        K::BadTag => "pgn-diag-bad-tag",
        K::TagTooLong => "pgn-diag-tag-too-long",
        K::RatingTooLarge => "pgn-diag-rating-too-large",
        K::InvalidFen => "pgn-diag-invalid-fen",
        K::IllegalCastling => "pgn-diag-illegal-castling",
        K::CastlingReinterpreted => "pgn-diag-castling-reinterpreted",
        K::IllegalMove(_) => "pgn-diag-illegal-move",
        K::AmbiguousMove(_) => "pgn-diag-ambiguous-move",
        K::InvalidMove(_) => "pgn-diag-invalid-move",
        K::TooManyNodes => "pgn-diag-too-many-nodes",
        K::TooManyTags => "pgn-diag-too-many-tags",
        K::DuplicateVariation => "pgn-diag-duplicate-variation",
        K::InvalidNagSymbol => "pgn-diag-invalid-nag-symbol",
        K::NagOutOfRange => "pgn-diag-nag-out-of-range",
        K::NagWithoutMove => "pgn-diag-nag-without-move",
        K::VariationBeforeFirstMove => "pgn-diag-variation-before-first-move",
        K::UnbalancedCloseParen => "pgn-diag-unbalanced-close-paren",
        K::UnclosedVariation => "pgn-diag-unclosed-variation",
        K::StrayCloseBrace => "pgn-diag-stray-close-brace",
        K::UnterminatedComment => "pgn-diag-unterminated-comment",
        K::CommentTruncated => "pgn-diag-comment-truncated",
        K::MissingResult => "pgn-diag-missing-result",
        K::UnexpectedToken => "pgn-diag-unexpected-token",
        K::ResultMismatch => "pgn-diag-result-mismatch",
        K::InvalidTextInHeader => "pgn-diag-invalid-text-in-header",
        K::HeaderOnlyAtEof => "pgn-diag-header-only-at-eof",
        K::JunkBetweenGames => "pgn-diag-junk-between-games",
        K::EncodingFallback => "pgn-diag-encoding-fallback",
        K::Chess960Detected => "pgn-diag-chess960-detected",
        K::UnsupportedVariant => "pgn-diag-unsupported-variant",
        K::GameTooLarge => "pgn-diag-game-too-large",
        K::TooManyDiagnostics => "pgn-diag-too-many-diagnostics",
        _ => "pgn-diag-unknown",
    }
}

/// One remark about an import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportNotice {
    /// How serious it is.
    pub severity: Severity,
    /// What happened.
    pub issue: ImportIssue,
    /// Line (1-based) in the text or file, when known.
    pub line: Option<u64>,
    /// Column (1-based, in bytes), when known.
    pub column: Option<u32>,
    /// The offending text (clipped), when there is one.
    pub excerpt: Option<String>,
}

impl ImportNotice {
    /// A notice without a position.
    pub fn new(severity: Severity, issue: ImportIssue) -> ImportNotice {
        ImportNotice {
            severity,
            issue,
            line: None,
            column: None,
            excerpt: None,
        }
    }

    /// Converts a reader diagnostic.
    pub fn from_diagnostic(diagnostic: &Diagnostic) -> ImportNotice {
        ImportNotice {
            severity: diagnostic.severity,
            issue: ImportIssue::Pgn(diagnostic.kind.clone()),
            line: Some(diagnostic.line),
            column: Some(diagnostic.column),
            excerpt: diagnostic.text.clone(),
        }
    }

    /// Whether the notice keeps the import dialog open (D122).
    pub fn is_problem(&self) -> bool {
        self.severity >= Severity::Warning
    }
}

/// The outcome of one import, as data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    /// How the text was read.
    pub mode: ImportMode,
    /// Everything worth telling, in reading order.
    pub notices: Vec<ImportNotice>,
}

impl ImportReport {
    /// Whether some notice is a warning or an error.
    pub fn has_problems(&self) -> bool {
        self.notices.iter().any(ImportNotice::is_problem)
    }
}

/// State of the import dialog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportDialog {
    /// The PGN text.
    pub text: String,
    /// The scan in flight; an answer with another id is dropped.
    pub request: Option<RequestId>,
    /// The report of the last import.
    pub report: Option<ImportReport>,
}

/// Events of the import dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportMessage {
    /// Opens the dialog and asks for the clipboard to pre-fill it.
    Open,
    /// The text was edited.
    TextEdited(String),
    /// Inserts the current game as PGN text (width 70).
    PasteCurrentGame,
    /// Empties the text and the report.
    Clear,
    /// Imports the text (a [`ScanPgn`](crate::Effect::ScanPgn) effect).
    Import,
    /// Closes the dialog and forgets the request in flight.
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_are_warnings_and_errors_only() {
        let info = ImportNotice::new(
            Severity::Info,
            ImportIssue::Pgn(DiagnosticKind::JunkBetweenGames),
        );
        let err = ImportNotice::new(Severity::Error, ImportIssue::NothingToImport);
        let ok = ImportReport {
            mode: ImportMode::FullGame,
            notices: vec![info.clone()],
        };
        assert!(!ok.has_problems());
        let bad = ImportReport {
            mode: ImportMode::FullGame,
            notices: vec![info, err],
        };
        assert!(bad.has_problems());
    }

    #[test]
    fn unknown_kinds_have_a_key() {
        assert_eq!(
            ImportIssue::Pgn(DiagnosticKind::BadTag).message_key(),
            "pgn-diag-bad-tag"
        );
        assert_eq!(
            ImportIssue::NothingToImport.message_key(),
            "import-issue-nothing-to-import"
        );
    }
}
