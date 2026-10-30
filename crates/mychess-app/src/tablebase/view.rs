//! The snapshot of the tablebase panel that the front-end draws (spec M8-8,
//! D2571, D2587). Numbers and enumerations only: the front-end formats them
//! with Fluent.

use mychess_core::Color;
use mychess_tablebase::{Category, Counts, Source, TablebaseError};

use super::training::{Grade, TrainingOutcome};

/// What the panel shows in place of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TablebaseStatus {
    /// Neither the local tables nor the online tablebase is turned on.
    NotConfigured,
    /// The online tablebase and the training wait for the end of an online
    /// game (D2570).
    Locked,
    /// The folders are being read.
    Loading,
    /// A query is in flight, and nothing is shown yet.
    Waiting,
    /// The list is for the position on the board.
    Ready,
    /// The list is for another position (the answer is on its way, or the
    /// query failed).
    Stale,
}

/// A warning line (D2566).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IssueView {
    /// No table was found in the folders.
    NoTables,
    /// The position has more pieces than the source covers.
    TooManyPieces {
        /// Pieces on the board.
        pieces: u8,
        /// Most pieces covered.
        max: u8,
    },
    /// Two kings only (or fewer than three pieces).
    TooFewPieces,
    /// A castling right is left.
    Castling,
    /// The table of this material is not there.
    MissingTable {
        /// The material, for instance `KRvKP`.
        material: String,
    },
    /// A table file is corrupted.
    Corrupt {
        /// The material.
        material: String,
    },
    /// No connection, or the client could not start.
    Network,
    /// The server did not answer in time.
    TimedOut,
    /// The server asked to wait.
    RateLimited {
        /// Seconds left.
        seconds: u64,
    },
    /// The server failed.
    Server,
    /// The answer made no sense.
    Invalid,
    /// An internal failure.
    Internal,
}

impl IssueView {
    /// The Fluent key of the line.
    #[must_use]
    pub fn message_key(&self) -> &'static str {
        match self {
            IssueView::NoTables => "tablebase-issue-no-tables",
            IssueView::TooManyPieces { .. } => "tablebase-issue-too-many-pieces",
            IssueView::TooFewPieces => "tablebase-issue-too-few-pieces",
            IssueView::Castling => "tablebase-issue-castling",
            IssueView::MissingTable { .. } => "tablebase-issue-missing-table",
            IssueView::Corrupt { .. } => "tablebase-issue-corrupt",
            IssueView::Network => "tablebase-issue-network",
            IssueView::TimedOut => "tablebase-issue-timed-out",
            IssueView::RateLimited { .. } => "tablebase-issue-rate-limited",
            IssueView::Server => "tablebase-issue-server",
            IssueView::Invalid => "tablebase-issue-invalid",
            IssueView::Internal => "tablebase-issue-internal",
        }
    }
}

/// The line that tells why the position has no list.
pub(crate) fn issue_of(error: &TablebaseError) -> IssueView {
    match error {
        TablebaseError::NoTables => IssueView::NoTables,
        TablebaseError::TooManyPieces { pieces, max } => IssueView::TooManyPieces {
            pieces: *pieces,
            max: *max,
        },
        TablebaseError::TooFewPieces { .. } => IssueView::TooFewPieces,
        TablebaseError::Castling => IssueView::Castling,
        TablebaseError::MissingTable { material } => IssueView::MissingTable {
            material: material.clone(),
        },
        TablebaseError::Corrupt { material } => IssueView::Corrupt {
            material: material.clone(),
        },
        TablebaseError::Network(_) | TablebaseError::Io { .. } => IssueView::Network,
        TablebaseError::Timeout => IssueView::TimedOut,
        TablebaseError::RateLimited { retry_after } => IssueView::RateLimited {
            seconds: retry_after.as_secs().max(1),
        },
        TablebaseError::Server(_) | TablebaseError::NotFound => IssueView::Server,
        TablebaseError::Protocol(_) => IssueView::Invalid,
        _ => IssueView::Internal,
    }
}

/// One folder of the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryView {
    /// The folder as the user configured it.
    pub path: String,
    /// `.rtbw` files found; `None` while the folders were not read.
    pub wdl_files: Option<usize>,
    /// `.rtbz` files found.
    pub dtz_files: Option<usize>,
    /// The folder does not exist or cannot be read.
    pub unreadable: bool,
    /// Files with the name of a table that were not accepted.
    pub bad_files: usize,
    /// Table files whose extension is in upper case: they are ignored and
    /// must be renamed to lower case (D2581).
    pub uppercase_files: usize,
}

/// What the opened tables hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSummary {
    /// `.rtbw` files.
    pub wdl_files: usize,
    /// `.rtbz` files.
    pub dtz_files: usize,
    /// The largest table, in pieces.
    pub max_pieces: u8,
}

/// A legal move and its result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    /// The move in SAN.
    pub san: String,
    /// The result for the side that plays it.
    pub category: Category,
    /// Distance to zeroing, in plies; positive when the mover wins.
    pub dtz: Option<i32>,
    /// The distance may be one ply off.
    pub dtz_rounded: bool,
    /// Distance to mate, in plies.
    pub dtm: Option<i32>,
    /// The move belongs to the best group.
    pub best: bool,
    /// The move captures or moves a pawn.
    pub zeroing: bool,
    /// The move mates.
    pub checkmate: bool,
}

/// The training in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingView {
    /// The side of the user.
    pub user: Color,
    /// Moves of the user so far.
    pub user_moves: u16,
    /// Times the user made the position worth less.
    pub slips: u16,
    /// What the position was worth for the user at the start.
    pub start: Option<Grade>,
    /// It is the move of the tablebase.
    pub tablebase_to_move: bool,
}

/// What the panel draws.
#[derive(Debug, Clone, PartialEq)]
pub struct TablebaseView {
    /// The panel is open.
    pub open: bool,
    /// What takes the place of the list, if anything.
    pub status: TablebaseStatus,
    /// The local tables are turned on.
    pub local_enabled: bool,
    /// The online tablebase is turned on.
    pub online_enabled: bool,
    /// The folders.
    pub directories: Vec<DirectoryView>,
    /// What the opened tables hold.
    pub summary: Option<LocalSummary>,
    /// Where the list came from.
    pub source: Option<Source>,
    /// The result of the position, for the side to move.
    pub position: Option<mychess_tablebase::Evaluation>,
    /// The counts of the results of the moves.
    pub counts: Counts,
    /// The moves, best first (empty while the training hides them).
    pub rows: Vec<RowView>,
    /// Some row has a distance to mate.
    pub has_dtm: bool,
    /// The list is not for the position on the board.
    pub stale: bool,
    /// The warning line.
    pub issue: Option<IssueView>,
    /// The training in progress.
    pub training: Option<TrainingView>,
    /// The result of the last training.
    pub outcome: Option<TrainingOutcome>,
}

impl TablebaseView {
    /// Whether the training hides the results.
    #[must_use]
    pub fn results_hidden(&self) -> bool {
        self.training.is_some()
    }
}
