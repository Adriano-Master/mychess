//! The messages, notices and answers of the advanced searches (spec M6-1).

use std::path::PathBuf;
use std::time::Duration;

use mychess_core::{Color, File, Piece, Rank};
use mychess_db::{AdvancedQuery, Filter, MaterialSearch, SearchError, SearchReport};

use super::prefs::{BishopsKind, BoardKind, CheckKind, SearchOp, SideKind};
use crate::bases::SlotId;
use crate::error::RequestId;

/// Which of the three searches (the tabs of the window).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AdvancedKind {
    /// By position (Scid: Search > Current Board).
    #[default]
    Position,
    /// By material and patterns (Scid: Search > Material/Pattern).
    Material,
    /// By a sequence of moves (Scid: Search > Moves).
    Moves,
}

impl AdvancedKind {
    /// The tabs, in the order the window shows them.
    pub const ALL: [AdvancedKind; 3] = [
        AdvancedKind::Position,
        AdvancedKind::Material,
        AdvancedKind::Moves,
    ];

    pub(crate) fn index(self) -> usize {
        match self {
            AdvancedKind::Position => 0,
            AdvancedKind::Material => 1,
            AdvancedKind::Moves => 2,
        }
    }
}

/// A row of the material box (Scid: Queen, Rook, Bishop, Knight, Minor,
/// Pawn).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterialPiece {
    /// Queens.
    Queen,
    /// Rooks.
    Rook,
    /// Bishops.
    Bishop,
    /// Knights.
    Knight,
    /// Minor pieces: bishops plus knights.
    Minor,
    /// Pawns.
    Pawn,
}

impl MaterialPiece {
    /// The rows, in the order of the window.
    pub const ALL: [MaterialPiece; 6] = [
        MaterialPiece::Queen,
        MaterialPiece::Rook,
        MaterialPiece::Bishop,
        MaterialPiece::Knight,
        MaterialPiece::Minor,
        MaterialPiece::Pawn,
    ];
}

/// The largest count of a piece the window lets the user type (`0..=9`).
pub const MAX_PIECE_COUNT: u8 = 9;
/// The largest absolute material difference the window lets the user type.
pub const MAX_MATERIAL_DIFF: i16 = 99;
/// The number of pattern rows of the window (the engine takes more).
pub const PATTERN_ROWS: usize = 10;

/// What the window of the advanced searches can be asked (spec M6-1).
///
/// The options that outlive the window (the operation of each tab and the
/// last options, `[advanced_search]` of the preferences) are changed by the
/// `Set*` messages that name them; what is typed is kept by the window until
/// the program ends.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AdvancedSearchMessage {
    /// Opens the window on a tab (the menu items of Search).
    Open(AdvancedKind),
    /// Closes the window. A search in flight goes on (it is stopped by
    /// [`AdvancedSearchMessage::Stop`]).
    Close,
    /// Shows a tab.
    SelectTab(AdvancedKind),
    /// The operation on the filter of a tab (Reset, AND, OR; D3204).
    SetOp(AdvancedKind, SearchOp),
    /// "Search in database": the slot whose filter the search changes, or
    /// `None` for the current one (D3213). A slot that is not open is
    /// ignored.
    SetReference(AdvancedKind, Option<SlotId>),

    // ---- position ----
    /// What the position search compares.
    SetBoardKind(BoardKind),
    /// "Look in variations".
    SetBoardVariations(bool),
    /// "Ignore colors" of the position search.
    SetBoardIgnoreColors(bool),
    /// The position to search is the one on the board (the default).
    UseCurrentPosition,
    /// The position to search is this FEN (checked when the search starts).
    SetFen(String),

    // ---- material and patterns ----
    /// The range of a piece for one colour (clamped to `0..=9`).
    SetRange {
        /// The colour.
        color: Color,
        /// The row.
        piece: MaterialPiece,
        /// Fewest pieces.
        min: u8,
        /// Most pieces.
        max: u8,
    },
    /// The material difference (White minus Black), or no restriction.
    SetDiff(Option<(i16, i16)>),
    /// The colour of the bishops' squares.
    SetBishops(BishopsKind),
    /// The range of move numbers (`0..=999`).
    SetMoveRange(u16, u16),
    /// "Match for at least" this many half moves (`0..=99`).
    SetMinPlies(u16),
    /// "End pos only".
    SetEndOnly(bool),
    /// "Ignore colors" of the material search.
    SetMaterialIgnoreColors(bool),
    /// "Zero": only the two kings.
    MaterialZero,
    /// "Any": any material, no difference restriction.
    MaterialAny,
    /// "Current Board": the pieces on the board, kings left out.
    MaterialCurrentBoard,
    /// A common ending, by its index in `AdvancedSearchView::endings`.
    ApplyEnding(usize),
    /// A common pattern, by its index in `AdvancedSearchView::patterns`.
    ApplyPatternPreset(usize),
    /// "Clear": every pattern row empty.
    ClearPatterns,
    /// The piece of a pattern row (`None`: the row is empty).
    SetPatternPiece {
        /// The row, from 0.
        index: usize,
        /// The piece.
        piece: Option<Piece>,
    },
    /// The file of a pattern row (`None`: any file).
    SetPatternFile {
        /// The row, from 0.
        index: usize,
        /// The file.
        file: Option<File>,
    },
    /// The rank of a pattern row (`None`: any rank).
    SetPatternRank {
        /// The row, from 0.
        index: usize,
        /// The rank.
        rank: Option<Rank>,
    },
    /// The sense of a pattern row: `true` = the piece must be there.
    SetPatternPresent {
        /// The row, from 0.
        index: usize,
        /// Yes or No.
        present: bool,
    },

    // ---- moves ----
    /// The words to look for (SAN, `?` = any move).
    SetMovesText(String),
    /// Who plays the first move.
    SetMovesSide(SideKind),
    /// The check test.
    SetMovesCheck(CheckKind),
    /// "Look in variations" of the search by moves.
    SetMovesVariations(bool),

    // ---- running ----
    /// Searches with the tab on show. Starting a search while another runs
    /// stops the old one (D3232).
    Start,
    /// Stops the search in flight; the filter is left as it was (D3202).
    Stop,
    /// A tick of the front-end while a search runs: reads the progress.
    Progress,
    /// Answer of the search (see [`Effect::AdvancedSearch`](crate::Effect)).
    Finished {
        /// The request.
        request: RequestId,
        /// The new filter and the report, or why it could not run.
        result: Result<Box<AdvancedDone>, SearchFailure>,
    },
    /// The search was stopped; the filter was not changed.
    Cancelled {
        /// The request.
        request: RequestId,
    },

    // ---- saved searches ----
    /// Lists the saved searches again.
    RefreshSaved,
    /// Saves the search of the tab on show under a name.
    SaveAs(String),
    /// Loads a saved search into its tab and shows it.
    LoadSaved(String),
    /// Deletes a saved search.
    DeleteSaved(String),
    /// Reads a material `.sso` file of the Scid into the material tab.
    ImportSso(PathBuf),
    /// Answer of a saved-search operation.
    SavedDone {
        /// The request.
        request: RequestId,
        /// What happened.
        result: Result<SavedOutcome, SavedFailure>,
    },
}

/// The result of a search that ran to the end.
#[derive(Debug, Clone, PartialEq)]
pub struct AdvancedDone {
    /// The filter after the search (the copy that was searched).
    pub filter: Filter,
    /// The report of the search.
    pub report: SearchReport,
}

/// Why a search did not produce a filter.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SearchFailure {
    /// The engine refused the search.
    Search(SearchError),
    /// The search thread failed.
    Internal,
}

/// The line under the window: how the last search went (D3241).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchSummary {
    /// Which search.
    pub kind: AdvancedKind,
    /// The slot whose filter changed.
    pub slot: SlotId,
    /// Games in the filter now.
    pub matched: u32,
    /// Games in the filter before (AND) or in the database (the others).
    pub start_count: u32,
    /// Time spent.
    pub elapsed: Duration,
    /// Games that could not be read (they did not match).
    pub unreadable: u64,
}

/// What a saved-search operation asks for
/// ([`Effect::AdvancedSaved`](crate::Effect)).
#[derive(Debug, Clone, PartialEq)]
pub enum SavedCommand {
    /// List the saved searches.
    List,
    /// Save a search (replacing one with the same name).
    Save(Box<mychess_db::SavedAdvanced>),
    /// Load the search with this name.
    Load(String),
    /// Delete the search with this name.
    Delete(String),
    /// Read a material `.sso` file.
    ImportSso(PathBuf),
}

/// What a saved-search operation did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SavedOutcome {
    /// The saved searches, by name, and the files that could not be read.
    Listed {
        /// The searches.
        entries: Vec<SavedEntry>,
        /// Files that could not be read.
        skipped: usize,
    },
    /// A search was saved.
    Saved(String),
    /// A search was loaded.
    Loaded(Box<AdvancedQuery>),
    /// A search was deleted (`false`: there was none).
    Deleted(String, bool),
    /// A `.sso` file was read.
    Imported {
        /// The material search.
        search: Box<MaterialSearch>,
        /// What was ignored.
        warnings: usize,
    },
}

/// A saved search in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedEntry {
    /// The name.
    pub name: String,
    /// The creation date, `yyyy-mm-dd`, if the file has one.
    pub created: Option<String>,
}

/// Why a saved-search operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SavedFailure {
    /// There is no search with this name.
    NotFound,
    /// The name is not acceptable.
    InvalidName,
    /// The file is not a search the program understands.
    InvalidFile,
    /// A file operation failed.
    Io,
    /// The work thread failed.
    Internal,
}

impl SavedFailure {
    pub(crate) fn from_error(error: &mychess_db::SavedSearchError) -> SavedFailure {
        use mychess_db::SavedSearchError as E;
        match error {
            E::NotFound { .. } => SavedFailure::NotFound,
            E::InvalidName { .. } | E::InvalidDate { .. } => SavedFailure::InvalidName,
            E::Io { .. } | E::TooLarge { .. } => SavedFailure::Io,
            _ => SavedFailure::InvalidFile,
        }
    }
}

/// A remark about the advanced searches for the status bar or the line of
/// the window. The front-end has a Fluent message for each key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdvancedSearchNotice {
    /// The FEN of the position tab is not valid.
    InvalidFen,
    /// A pattern is not valid (it names a piece but no file and no rank).
    InvalidPattern(&'static str),
    /// The range of move numbers is reversed or over 999.
    InvalidMoveRange {
        /// First move number.
        min: u16,
        /// Last move number.
        max: u16,
    },
    /// No moves were typed.
    EmptyMoveList,
    /// The first word is the wildcard `?`.
    FirstMoveIsWildcard,
    /// A word is not a move in SAN.
    BadMoveToken(String),
    /// A word has a `+` or `#` that the check test does not include.
    CheckTestTooLow(String),
    /// A word with `+` or `#` raised the check test (D3226).
    CheckTestRaised(CheckKind),
    /// The position cannot be searched.
    UnsupportedPosition,
    /// The database to search is not open.
    NoDatabase,
    /// The database or its filter changed while the search ran: the result
    /// was dropped.
    Stale,
    /// The search was stopped; the filter is as it was.
    Interrupted,
    /// Some games could not be read; they did not match.
    Unreadable {
        /// How many.
        count: u64,
    },
    /// The search failed for a reason that is not the user's.
    Failed,
    /// There is no folder for the saved searches.
    NoSavedFolder,
    /// A search was saved.
    Saved(String),
    /// A saved search was deleted.
    Deleted(String),
    /// There is no saved search with this name.
    SavedNotFound(String),
    /// The name of a saved search is not acceptable.
    InvalidSavedName,
    /// A saved search could not be read or written.
    SavedFailed,
    /// A `.sso` file was imported.
    Imported {
        /// How many statements were ignored.
        warnings: usize,
    },
    /// A `.sso` file could not be imported.
    ImportFailed,
}

impl AdvancedSearchNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            AdvancedSearchNotice::InvalidFen => "advsearch-notice-invalid-fen",
            AdvancedSearchNotice::InvalidPattern(_) => "advsearch-notice-invalid-pattern",
            AdvancedSearchNotice::InvalidMoveRange { .. } => "advsearch-notice-invalid-move-range",
            AdvancedSearchNotice::EmptyMoveList => "advsearch-notice-empty-moves",
            AdvancedSearchNotice::FirstMoveIsWildcard => "advsearch-notice-first-wildcard",
            AdvancedSearchNotice::BadMoveToken(_) => "advsearch-notice-bad-move",
            AdvancedSearchNotice::CheckTestTooLow(_) => "advsearch-notice-check-test-too-low",
            AdvancedSearchNotice::CheckTestRaised(_) => "advsearch-notice-check-test-raised",
            AdvancedSearchNotice::UnsupportedPosition => "advsearch-notice-unsupported-position",
            AdvancedSearchNotice::NoDatabase => "advsearch-notice-no-database",
            AdvancedSearchNotice::Stale => "advsearch-notice-stale",
            AdvancedSearchNotice::Interrupted => "advsearch-notice-interrupted",
            AdvancedSearchNotice::Unreadable { .. } => "advsearch-notice-unreadable",
            AdvancedSearchNotice::Failed => "advsearch-notice-failed",
            AdvancedSearchNotice::NoSavedFolder => "advsearch-notice-no-saved-folder",
            AdvancedSearchNotice::Saved(_) => "advsearch-notice-saved",
            AdvancedSearchNotice::Deleted(_) => "advsearch-notice-deleted",
            AdvancedSearchNotice::SavedNotFound(_) => "advsearch-notice-saved-not-found",
            AdvancedSearchNotice::InvalidSavedName => "advsearch-notice-invalid-saved-name",
            AdvancedSearchNotice::SavedFailed => "advsearch-notice-saved-failed",
            AdvancedSearchNotice::Imported { .. } => "advsearch-notice-imported",
            AdvancedSearchNotice::ImportFailed => "advsearch-notice-import-failed",
        }
    }

    /// Whether the remark is a refusal of the form (the window marks the
    /// field) and not just information.
    pub fn is_form_error(&self) -> bool {
        matches!(
            self,
            AdvancedSearchNotice::InvalidFen
                | AdvancedSearchNotice::InvalidPattern(_)
                | AdvancedSearchNotice::InvalidMoveRange { .. }
                | AdvancedSearchNotice::EmptyMoveList
                | AdvancedSearchNotice::FirstMoveIsWildcard
                | AdvancedSearchNotice::BadMoveToken(_)
                | AdvancedSearchNotice::CheckTestTooLow(_)
                | AdvancedSearchNotice::UnsupportedPosition
        )
    }
}

impl From<&SearchError> for AdvancedSearchNotice {
    fn from(error: &SearchError) -> AdvancedSearchNotice {
        match error {
            SearchError::UnsupportedPosition => AdvancedSearchNotice::UnsupportedPosition,
            SearchError::InvalidPattern { reason } => AdvancedSearchNotice::InvalidPattern(reason),
            SearchError::InvalidMoveRange { min, max } => AdvancedSearchNotice::InvalidMoveRange {
                min: *min,
                max: *max,
            },
            SearchError::EmptyMoveList => AdvancedSearchNotice::EmptyMoveList,
            SearchError::FirstMoveIsWildcard => AdvancedSearchNotice::FirstMoveIsWildcard,
            SearchError::BadMoveToken { token } => {
                AdvancedSearchNotice::BadMoveToken(token.clone())
            }
            SearchError::CheckTestTooLow { token } => {
                AdvancedSearchNotice::CheckTestTooLow(token.clone())
            }
            // The size of the filter is the app's own invariant: it means
            // the database changed under the search.
            SearchError::FilterSize { .. } => AdvancedSearchNotice::Stale,
            _ => AdvancedSearchNotice::Failed,
        }
    }
}
