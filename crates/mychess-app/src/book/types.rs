//! The plain types of the book window (spec M5-6, D1651-D1668).

use std::sync::Arc;

use mychess_book::{Book, BookError, BuildOptions, MergeMode};

/// A book shared between the state and the effects. `Book` has no
/// `PartialEq`, so two shared books are equal when they are the same
/// allocation.
#[derive(Clone)]
pub struct SharedBook(Arc<Book>);

impl std::fmt::Debug for SharedBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SharedBook({} entries)", self.0.len())
    }
}

impl PartialEq for SharedBook {
    fn eq(&self, other: &SharedBook) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl SharedBook {
    pub(crate) fn new(book: Book) -> SharedBook {
        SharedBook(Arc::new(book))
    }

    pub(crate) fn get(&self) -> &Book {
        &self.0
    }

    /// The book to edit: copies it first if an effect still holds it.
    pub(crate) fn edit(&mut self) -> &mut Book {
        Arc::make_mut(&mut self.0)
    }
}

/// The flag that stops a book being made from games (D1791). Two flags are
/// always equal: what identifies a job is its `RequestId`.
#[derive(Debug, Clone, Default)]
pub struct BookCancel(mychess_db::CancelToken);

impl BookCancel {
    /// A flag that is not raised.
    pub fn new() -> BookCancel {
        BookCancel::default()
    }

    /// Asks the work to stop.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// Whether [`BookCancel::cancel`] was called.
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

impl PartialEq for BookCancel {
    fn eq(&self, _: &BookCancel) -> bool {
        true
    }
}

/// One of the two books of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BookSide {
    /// The left book ("book 1").
    First,
    /// The right book ("book 2").
    Second,
}

impl BookSide {
    /// Both sides, left first.
    pub const ALL: [BookSide; 2] = [BookSide::First, BookSide::Second];

    /// The index in the pair of slots.
    pub fn index(self) -> usize {
        match self {
            BookSide::First => 0,
            BookSide::Second => 1,
        }
    }

    /// The other book.
    #[must_use]
    pub fn other(self) -> BookSide {
        match self {
            BookSide::First => BookSide::Second,
            BookSide::Second => BookSide::First,
        }
    }
}

/// Why a book operation failed (D1668). Never carries a path or a message
/// of the operating system: the front-end translates the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookIssue {
    /// The file does not exist.
    NotFound,
    /// The file cannot be read or is not a book.
    Unreadable,
    /// The file is too short to hold a record.
    TooShort,
    /// The file is larger than the limit.
    TooLarge,
    /// The book is read-only.
    ReadOnly,
    /// The book has no file yet.
    NoPath,
    /// The file could not be written.
    WriteFailed,
    /// Chess960 and set-up positions cannot be in a Polyglot book.
    Unsupported,
    /// A move of the line is illegal.
    IllegalMove,
    /// The weight is zero.
    ZeroWeight,
    /// The weight is above the maximum.
    WeightTooLarge,
    /// The position would have more moves than the format allows.
    TooManyMoves,
    /// The weight is not a number.
    NotANumber,
    /// The move is already in the book.
    AlreadyThere,
    /// There is nothing after the cursor to add or remove.
    NothingToAdd,
    /// The branch has too many lines or moves.
    BranchTooBig,
    /// The side has changes that are not saved.
    UnsavedChanges,
    /// A book is missing for the operation.
    NoBook,
    /// The database to read the games from is missing or empty.
    NoGames,
    /// A job of the same book is running.
    Busy,
    /// The work was stopped by the user.
    Cancelled,
    /// The work failed in a way that has no better name.
    Internal,
    /// The book has no move to export from the position (D2160).
    EmptyBranch,
    /// The destination is the file of an open database or book (D2166).
    ProtectedFile,
    /// The name of the file is not valid Unicode, so it cannot be made safe
    /// (D2172).
    BadFileName,
    /// The destination exists and was not the name the user confirmed
    /// (D2166).
    FileExists,
}

impl BookIssue {
    /// The reason of a failure of the book crate.
    pub fn of(error: &BookError) -> BookIssue {
        match error {
            BookError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                BookIssue::NotFound
            }
            BookError::Io { .. } => BookIssue::WriteFailed,
            BookError::TooShort { .. } => BookIssue::TooShort,
            BookError::TooLarge { .. } => BookIssue::TooLarge,
            BookError::ReadOnly => BookIssue::ReadOnly,
            BookError::NoPath => BookIssue::NoPath,
            BookError::IllegalMove { .. } => BookIssue::IllegalMove,
            BookError::ZeroWeight => BookIssue::ZeroWeight,
            BookError::WeightTooLarge { .. } => BookIssue::WeightTooLarge,
            BookError::TooManyMoves { .. } => BookIssue::TooManyMoves,
            BookError::DuplicateMove => BookIssue::AlreadyThere,
            BookError::UnsupportedPosition => BookIssue::Unsupported,
            _ => BookIssue::Unreadable,
        }
    }

    /// Like [`BookIssue::of`] for a read: an I/O failure other than "not
    /// found" means the file is unreadable, not unwritable.
    pub(crate) fn of_read(error: &BookError) -> BookIssue {
        match BookIssue::of(error) {
            BookIssue::WriteFailed => BookIssue::Unreadable,
            other => other,
        }
    }

    /// The Fluent key of the text.
    pub fn key(self) -> &'static str {
        match self {
            BookIssue::NotFound => "book-issue-not-found",
            BookIssue::Unreadable => "book-issue-unreadable",
            BookIssue::TooShort => "book-issue-too-short",
            BookIssue::TooLarge => "book-issue-too-large",
            BookIssue::ReadOnly => "book-issue-read-only",
            BookIssue::NoPath => "book-issue-no-path",
            BookIssue::WriteFailed => "book-issue-write-failed",
            BookIssue::Unsupported => "book-issue-unsupported",
            BookIssue::IllegalMove => "book-issue-illegal-move",
            BookIssue::ZeroWeight => "book-issue-zero-weight",
            BookIssue::WeightTooLarge => "book-issue-weight-too-large",
            BookIssue::TooManyMoves => "book-issue-too-many-moves",
            BookIssue::NotANumber => "book-issue-not-a-number",
            BookIssue::AlreadyThere => "book-issue-already-there",
            BookIssue::NothingToAdd => "book-issue-nothing-to-add",
            BookIssue::BranchTooBig => "book-issue-branch-too-big",
            BookIssue::UnsavedChanges => "book-issue-unsaved-changes",
            BookIssue::NoBook => "book-issue-no-book",
            BookIssue::NoGames => "book-issue-no-games",
            BookIssue::Busy => "book-issue-busy",
            BookIssue::Cancelled => "book-issue-cancelled",
            BookIssue::Internal => "book-issue-internal",
            BookIssue::EmptyBranch => "book-issue-empty-branch",
            BookIssue::ProtectedFile => "book-issue-protected-file",
            BookIssue::FileExists => "book-issue-file-exists",
            BookIssue::BadFileName => "book-issue-bad-name",
        }
    }
}

/// How two books are merged (D1661).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BookMergeMode {
    /// A position that is in both keeps the first book's moves (the
    /// `polyglot merge-book` rule).
    #[default]
    KeepFirst,
    /// The weights of the same move are added.
    SumWeights,
}

impl BookMergeMode {
    /// The mode of the book crate.
    pub(crate) fn core(self) -> MergeMode {
        match self {
            BookMergeMode::KeepFirst => MergeMode::KeepFirst,
            BookMergeMode::SumWeights => MergeMode::SumWeights,
        }
    }
}

/// The largest `max_ply` accepted (the default of `polyglot make-book`).
pub const MAX_PLY_LIMIT: u32 = 1024;
/// The largest number of games a move must be seen in.
pub const MIN_GAMES_LIMIT: u32 = 100_000;

/// The options of "create a book from the filter" (D1660, `book_make.cpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookCreateOptions {
    /// Only the first `max_ply` plies of each game are used.
    pub max_ply: u32,
    /// A move seen in fewer games is left out.
    pub min_games: u32,
    /// The least expected score of a move, in percent.
    pub min_score: u8,
    /// Keep only the moves of White.
    pub only_white: bool,
    /// Keep only the moves of Black.
    pub only_black: bool,
    /// Every move gets weight 1.
    pub uniform: bool,
}

impl Default for BookCreateOptions {
    fn default() -> BookCreateOptions {
        BookCreateOptions {
            max_ply: MAX_PLY_LIMIT,
            min_games: 3,
            min_score: 0,
            only_white: false,
            only_black: false,
            uniform: false,
        }
    }
}

impl BookCreateOptions {
    /// The options with every number inside its range. White-only and
    /// black-only exclude each other: the one turned on last would win, so
    /// when both arrive the white one is kept.
    #[must_use]
    pub fn normalized(mut self) -> BookCreateOptions {
        self.max_ply = self.max_ply.clamp(1, MAX_PLY_LIMIT);
        self.min_games = self.min_games.clamp(1, MIN_GAMES_LIMIT);
        self.min_score = self.min_score.min(100);
        if self.only_white && self.only_black {
            self.only_black = false;
        }
        self
    }

    pub(crate) fn build_options(self) -> BuildOptions {
        BuildOptions {
            max_ply: self.max_ply,
            min_game: self.min_games,
            min_score: f64::from(self.min_score),
            only_white: self.only_white,
            only_black: self.only_black,
            uniform: self.uniform,
        }
    }
}

/// What the window says about the last operation (D1651).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookNotice {
    /// It failed.
    Issue(BookIssue),
    /// A book was written.
    Saved,
    /// A book was created from the games of the filter.
    Created {
        /// Games used.
        used: u64,
        /// Games left out (illegal move or set-up start).
        skipped: u64,
        /// Entries of the new book.
        entries: usize,
    },
    /// The books were merged.
    Merged {
        /// Entries of the result.
        written: usize,
        /// Entries of the second book that were left out.
        skipped: usize,
    },
    /// Moves were added to the book.
    Added(usize),
    /// Moves were removed from the book.
    Removed(usize),
    /// The moves of a position were copied from the other book.
    Copied,
    /// The opening book was read.
    OpeningLoaded,
    /// The making of a book was stopped; nothing was kept (D1791).
    CreateCancelled,
    /// A branch was written as PGN (D2160).
    Exported {
        /// Lines (games) written.
        lines: usize,
        /// The limit of lines cut the branch short.
        truncated: bool,
    },
    /// The export was stopped; no file was written or changed (D2165).
    ExportCancelled,
    /// Stop was asked and the job has not answered yet (D2170).
    ExportStopping,
    /// Stop came too late: the job had already written the file (D2171).
    ExportFinishedAnyway {
        /// Lines (games) written.
        lines: usize,
    },
}

impl BookNotice {
    /// The Fluent key of the text.
    pub fn key(self) -> &'static str {
        match self {
            BookNotice::Issue(issue) => issue.key(),
            BookNotice::Saved => "book-notice-saved",
            BookNotice::Created { .. } => "book-notice-created",
            BookNotice::Merged { .. } => "book-notice-merged",
            BookNotice::Added(_) => "book-notice-added",
            BookNotice::Removed(_) => "book-notice-removed",
            BookNotice::Copied => "book-notice-copied",
            BookNotice::OpeningLoaded => "book-notice-opening-loaded",
            BookNotice::CreateCancelled => "book-notice-create-cancelled",
            BookNotice::Exported {
                truncated: false, ..
            } => "book-notice-exported",
            BookNotice::Exported {
                truncated: true, ..
            } => "book-notice-exported-truncated",
            BookNotice::ExportCancelled => "book-notice-export-cancelled",
            BookNotice::ExportStopping => "book-notice-export-stopping",
            BookNotice::ExportFinishedAnyway { .. } => "book-notice-export-finished",
        }
    }
}

/// The deepest line of an export, in plies (D2161).
pub const EXPORT_PLY_LIMIT: u32 = 200;
/// The most lines of an export (D2161).
pub const EXPORT_LINE_LIMIT: u32 = 5_000;

/// The options of "export the branch to PGN" (D2161).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookExportOptions {
    /// A line has at most this many plies.
    pub max_plies: u32,
    /// The export stops after this many lines.
    pub max_lines: u32,
    /// A move with a smaller weight is left out; `0` keeps every move.
    pub min_weight: u16,
}

impl Default for BookExportOptions {
    fn default() -> BookExportOptions {
        BookExportOptions {
            max_plies: 12,
            max_lines: 200,
            min_weight: 0,
        }
    }
}

impl BookExportOptions {
    /// The options with every number inside its range.
    #[must_use]
    pub fn normalized(mut self) -> BookExportOptions {
        self.max_plies = self.max_plies.clamp(1, EXPORT_PLY_LIMIT);
        self.max_lines = self.max_lines.clamp(1, EXPORT_LINE_LIMIT);
        self.min_weight = self.min_weight.min(mychess_book::MAX_WEIGHT);
        self
    }
}

/// The position an export starts from. Two starts are equal when their FEN
/// is (`Position` has no `PartialEq`).
#[derive(Debug, Clone)]
pub struct BranchStart(pub mychess_core::Position);

impl PartialEq for BranchStart {
    fn eq(&self, other: &BranchStart) -> bool {
        self.0.fen() == other.0.fen()
    }
}
