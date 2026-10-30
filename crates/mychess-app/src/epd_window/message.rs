//! The messages, effects and remarks of the EPD window (spec M7-5, D3800).

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mychess_pgn::epd::{EpdBook, EpdDiagnostic, EpdIndex, EpdOptions};

use super::run::AnnotateConfig;
use crate::engines::RunId;
use crate::error::{FileError, RequestId};

/// The identity of an open EPD file for as long as the program runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EpdFileId(pub u64);

/// An EPD book built or changed off the interface thread and shared with the
/// state; two values are equal when they are the same allocation (messages
/// and effects must be `Clone + PartialEq`).
#[derive(Debug)]
pub struct SharedBook(pub(crate) Arc<EpdBook>);

impl SharedBook {
    /// Shares `book`.
    pub fn new(book: EpdBook) -> SharedBook {
        SharedBook(Arc::new(book))
    }

    /// The book.
    pub fn book(&self) -> &EpdBook {
        &self.0
    }
}

impl Clone for SharedBook {
    fn clone(&self) -> SharedBook {
        SharedBook(Arc::clone(&self.0))
    }
}

impl PartialEq for SharedBook {
    fn eq(&self, other: &SharedBook) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedBook {}

/// The rows of the list: the records that pass the filter, in the order of
/// the list. Shared like [`SharedBook`].
#[derive(Debug, Clone)]
pub struct SharedRows(pub(crate) Arc<Vec<EpdIndex>>);

impl PartialEq for SharedRows {
    fn eq(&self, other: &SharedRows) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SharedRows {}

/// What the list is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SortKey {
    /// The order of the file.
    #[default]
    File,
    /// The four-field FEN, as text.
    Fen,
    /// The `id` operand (records without one last).
    Id,
    /// The `ce` evaluation (records without one last).
    Eval,
}

/// The order of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EpdSort {
    /// What to order by.
    pub key: SortKey,
    /// Largest first.
    pub descending: bool,
}

/// The answer to "this file was altered, save it?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpdCloseChoice {
    /// Save, then close.
    Save,
    /// Close without saving.
    Discard,
    /// Keep the file open.
    Cancel,
}

/// What a batch job does to a copy of the book.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchOp {
    /// Sorts the operations of every record (`EpdBook::sort_operations`).
    SortAll,
    /// Removes an opcode from every record (`EpdBook::strip_opcode`).
    Strip(String),
    /// Reclaims the holes left by removed records.
    Compact,
}

/// An EPD file read (or created), as `perform` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenedEpd {
    /// The canonical path of the file.
    pub path: PathBuf,
    /// The records.
    pub book: SharedBook,
    /// The file cannot be written: the window shows it read-only.
    pub read_only: bool,
    /// The records in the order of the list (no filter).
    pub rows: SharedRows,
    /// What was found on each line that was not a plain record (at most
    /// [`MAX_REPORT_LINES`] are kept).
    pub diagnostics: Vec<EpdDiagnostic>,
    /// Diagnostics that were counted but not kept.
    pub dropped: u64,
}

/// The most diagnostics of a file that the state keeps.
pub const MAX_REPORT_LINES: usize = 1000;

/// A save that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaveFailure {
    /// The file could not be written.
    File(FileError),
    /// A record cannot be expressed in the dialect (nothing was written).
    Unwritable {
        /// The opcode of the offending operation.
        opcode: String,
        /// Why.
        kind: UnwritableKind,
    },
}

/// Why a record cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnwritableKind {
    /// A `;` outside quotes (standard dialect).
    Semicolon,
    /// A quote that is never closed (standard dialect).
    UnbalancedQuote,
    /// The line would be longer than the reader accepts (1 MiB).
    TooLong,
    /// A line break inside an operation.
    LineBreak,
    /// Any other reason (empty opcode, counters).
    Other,
}

impl UnwritableKind {
    /// Classifies the reason of `EpdError::Unwritable`.
    pub(crate) fn of(reason: &str) -> UnwritableKind {
        if reason.contains("semicolon") {
            UnwritableKind::Semicolon
        } else if reason.contains("quote") {
            UnwritableKind::UnbalancedQuote
        } else if reason.contains("length") {
            UnwritableKind::TooLong
        } else if reason.contains("line break") {
            UnwritableKind::LineBreak
        } else {
            UnwritableKind::Other
        }
    }
}

/// A file written.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedEpd {
    /// The path written.
    pub path: PathBuf,
    /// The book without holes, when there were any (`None`: unchanged).
    pub book: Option<SharedBook>,
    /// `(old, new)` of the records whose handle changed with the compaction.
    pub moved: Vec<(EpdIndex, EpdIndex)>,
}

/// A batch job done.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchDone {
    /// The changed book; `None` when nothing changed.
    pub book: Option<SharedBook>,
    /// Records that changed.
    pub changed: usize,
    /// `(old, new)` of the records whose handle changed (compaction).
    pub moved: Vec<(EpdIndex, EpdIndex)>,
}

/// An event of the EPD window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EpdMessage {
    // ---- files ----
    /// "Open EPD...": asks for the file.
    Open,
    /// "New EPD...": asks for the name and creates the file.
    New,
    /// Opens this file (a recent one, one dropped, or "Open" with an
    /// `.epd`).
    OpenPath(PathBuf),
    /// Stops waiting for the file being opened (its answer is dropped).
    CancelOpen,
    /// Writes the active file.
    Save,
    /// Asks for a name and writes the active file there.
    SaveAs,
    /// Closes a file; asks first when it was altered.
    Close(EpdFileId),
    /// The answer to the "save before closing?" question.
    AnswerClose(EpdCloseChoice),
    /// Makes this file the active one (its tab).
    Activate(EpdFileId),
    /// Forgets the recent files.
    ClearRecent,

    // ---- the list ----
    /// Orders the list.
    SetSort(EpdSort),
    /// Shows only the records whose text contains this (empty: all).
    SetFilter(String),
    /// Selects a record without touching the board.
    Select(EpdIndex),
    /// Selects a record and loads its position as a new game.
    GoTo(EpdIndex),
    /// The first record of the list, loaded.
    First,
    /// The last record of the list, loaded.
    Last,
    /// The record after the selected one, loaded.
    Next,
    /// The record before the selected one, loaded.
    Prev,
    /// A record picked by this number (the front-end draws it), loaded.
    Random(u64),

    // ---- the record ----
    /// The text of the editor changed (kept as a draft).
    Edit(String),
    /// Writes the draft into the record.
    Commit,
    /// Adds the position of the board (or selects the record that has it).
    AddPosition,
    /// Asks to remove the selected record.
    RemovePosition,
    /// The answer to "remove this record?".
    ConfirmRemove(bool),
    /// Sorts the operations of the selected record.
    SortRecord,
    /// Sorts the operations of every record.
    SortAll,
    /// Removes an opcode from every record.
    StripOpcode(String),
    /// Writes the analysis of the engine into the selected record. `None`
    /// takes the first analysis that has a line.
    PasteAnalysis(Option<RunId>),
    /// Loads the deepest position of the game that the file has.
    Deepest,

    // ---- searching ----
    /// Looks for this text from the selected record on (`true`: forward).
    Find(String, bool),

    // ---- the analysis ----
    /// Starts the analysis of the positions.
    Annotate(AnnotateConfig),
    /// Stops it.
    StopAnnotate,
    /// Puts the records back as they were before the last analysis.
    UndoRun,
    /// The watchdog of the analysis fired.
    Tick {
        /// The execution of the engine.
        run: RunId,
        /// Tells a stale timer from the current one.
        generation: u64,
    },

    // ---- answers of effects ----
    /// A file was read or created.
    Opened {
        /// The request.
        request: RequestId,
        /// The file, or why not.
        result: Result<Box<OpenedEpd>, FileError>,
    },
    /// A file was written.
    Saved {
        /// The request.
        request: RequestId,
        /// What was done, or why not.
        result: Result<SavedEpd, SaveFailure>,
    },
    /// The rows of a list were computed.
    Rows {
        /// The request.
        request: RequestId,
        /// The rows.
        rows: SharedRows,
    },
    /// A search ended.
    Found {
        /// The request.
        request: RequestId,
        /// The record and the span of the text, if any.
        hit: Option<(EpdIndex, Range<usize>)>,
    },
    /// A batch job ended.
    Batch {
        /// The request.
        request: RequestId,
        /// What it did, or why not.
        result: Result<BatchDone, FileError>,
    },
}

/// What [`perform`](crate::perform) does for the EPD window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EpdEffect {
    /// Reads a file, or creates an empty one; answers [`EpdMessage::Opened`].
    Open {
        /// Correlates the answer.
        request: RequestId,
        /// The file.
        path: PathBuf,
        /// The dialect.
        options: EpdOptions,
        /// Create the file (empty, replacing any) instead of reading it.
        create: bool,
    },
    /// Writes a book atomically; answers [`EpdMessage::Saved`].
    Save {
        /// Correlates the answer.
        request: RequestId,
        /// The file.
        path: PathBuf,
        /// The records.
        book: SharedBook,
        /// The dialect.
        options: EpdOptions,
        /// Reclaim the holes first.
        compact: bool,
    },
    /// Orders and filters the records; answers [`EpdMessage::Rows`].
    Rows {
        /// Correlates the answer.
        request: RequestId,
        /// The records.
        book: SharedBook,
        /// The order.
        sort: EpdSort,
        /// The text the records must contain (empty: all).
        filter: String,
    },
    /// Looks for text; answers [`EpdMessage::Found`].
    Find {
        /// Correlates the answer.
        request: RequestId,
        /// The records.
        book: SharedBook,
        /// The record to start after.
        from: Option<EpdIndex>,
        /// The text.
        needle: String,
        /// The direction.
        forward: bool,
    },
    /// Changes a copy of the book; answers [`EpdMessage::Batch`].
    Batch {
        /// Correlates the answer.
        request: RequestId,
        /// The records.
        book: SharedBook,
        /// What to do.
        op: BatchOp,
    },
    /// Waits, then answers [`EpdMessage::Tick`].
    Tick {
        /// The execution of the engine.
        run: RunId,
        /// Tells a stale timer from the current one.
        generation: u64,
        /// How long to wait.
        delay: Duration,
    },
}

/// A remark about the EPD window, as data (the front-end translates it).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EpdNotice {
    /// The limit of open files was reached.
    TooMany {
        /// The limit.
        max: usize,
    },
    /// The file is already open.
    AlreadyOpen,
    /// A file could not be opened or created.
    OpenFailed(FileError),
    /// A file could not be written.
    SaveFailed(FileError),
    /// A record cannot be written; nothing was touched.
    Unwritable {
        /// The offending opcode.
        opcode: String,
        /// Why.
        kind: UnwritableKind,
    },
    /// The file is read-only: it cannot be edited or saved.
    ReadOnly,
    /// A job or the analysis holds the file; try again.
    Busy,
    /// An analysis is running on the file.
    RunActive,
    /// Lines of the file were ignored or repaired (see the report).
    SkippedLines {
        /// How many.
        count: u64,
    },
    /// A line of the record has an opcode that is not standard (kept).
    NonStandardOpcode {
        /// The opcode.
        opcode: String,
    },
    /// An opcode was removed from some records.
    Stripped {
        /// The opcode.
        opcode: String,
        /// The records that changed.
        count: usize,
    },
    /// The operations of some records were sorted.
    Sorted {
        /// The records that changed.
        count: usize,
    },
    /// The text was not found.
    NotFound,
    /// No position of the game is in the file.
    NoDeepest,
    /// No EPD file is open.
    NoFile,
    /// No record is selected.
    NoRecord,
    /// There is no analysis of the position to paste.
    NoAnalysis,
    /// No engine is available.
    NoEngine,
    /// The engine is not a UCI engine.
    UciOnly,
    /// The engine is in use by another feature.
    EngineBusy,
    /// The engine ended, refused the search or went silent.
    EngineFailed,
    /// There is no position to analyse in the scope.
    NothingToAnalyze,
    /// The analysis ended.
    RunDone {
        /// Positions analysed.
        done: u32,
        /// Positions where the best move was found.
        found: u32,
        /// Positions that have `bm` or `am`.
        counted: u32,
        /// Records whose `bm` or `am` has a move that is not legal (they
        /// count as no result).
        illegal: u32,
    },
    /// The analysis was stopped.
    RunInterrupted {
        /// Positions analysed.
        done: u32,
        /// Positions in the scope.
        total: u32,
    },
    /// A job on the file failed inside (not an I/O error).
    JobFailed,
    /// The last analysis was undone.
    RunUndone,
    /// There is no analysis to undo.
    NoRunToUndo,
}

impl EpdNotice {
    /// The Fluent key of the message (arguments: see the field names).
    pub fn message_key(&self) -> &'static str {
        match self {
            EpdNotice::TooMany { .. } => "epd-too-many",
            EpdNotice::AlreadyOpen => "epd-already-open",
            EpdNotice::OpenFailed(_) => "epd-error-open",
            EpdNotice::SaveFailed(_) => "epd-error-save",
            EpdNotice::Unwritable { kind, .. } => match kind {
                UnwritableKind::Semicolon => "epd-unwritable-semicolon",
                UnwritableKind::UnbalancedQuote => "epd-unwritable-quote",
                UnwritableKind::TooLong => "epd-unwritable-too-long",
                UnwritableKind::LineBreak => "epd-unwritable-line-break",
                UnwritableKind::Other => "epd-unwritable",
            },
            EpdNotice::ReadOnly => "epd-read-only-refused",
            EpdNotice::Busy => "epd-busy",
            EpdNotice::RunActive => "epd-run-active",
            EpdNotice::SkippedLines { .. } => "epd-skipped-lines",
            EpdNotice::NonStandardOpcode { .. } => "epd-nonstandard",
            EpdNotice::Stripped { .. } => "epd-stripped",
            EpdNotice::Sorted { .. } => "epd-sorted",
            EpdNotice::NotFound => "epd-not-found",
            EpdNotice::NoDeepest => "epd-no-deepest",
            EpdNotice::NoFile => "epd-no-file",
            EpdNotice::NoRecord => "epd-no-record",
            EpdNotice::NoAnalysis => "epd-no-analysis",
            EpdNotice::NoEngine => "epd-no-engine",
            EpdNotice::UciOnly => "epd-uci-only",
            EpdNotice::EngineBusy => "epd-engine-busy",
            EpdNotice::EngineFailed => "epd-engine-failed",
            EpdNotice::NothingToAnalyze => "epd-nothing-to-analyze",
            EpdNotice::RunDone { .. } => "epd-result",
            EpdNotice::RunInterrupted { .. } => "epd-interrupted",
            EpdNotice::JobFailed => "epd-job-failed",
            EpdNotice::RunUndone => "epd-run-undone",
            EpdNotice::NoRunToUndo => "epd-no-run-to-undo",
        }
    }
}

impl From<EpdMessage> for crate::Message {
    fn from(message: EpdMessage) -> crate::Message {
        crate::Message::Epd(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwritable_reasons_are_classified() {
        assert_eq!(
            UnwritableKind::of("semicolon outside quotes"),
            UnwritableKind::Semicolon
        );
        assert_eq!(
            UnwritableKind::of("unbalanced quote"),
            UnwritableKind::UnbalancedQuote
        );
        assert_eq!(
            UnwritableKind::of("record line exceeds the reader's line length limit"),
            UnwritableKind::TooLong
        );
        assert_eq!(
            UnwritableKind::of("line break in operation"),
            UnwritableKind::LineBreak
        );
        assert_eq!(
            UnwritableKind::of("opcode is empty or contains white space"),
            UnwritableKind::Other
        );
    }

    #[test]
    fn shared_books_compare_by_allocation() {
        let a = SharedBook::new(EpdBook::new());
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(a, SharedBook::new(EpdBook::new()));
    }
}
