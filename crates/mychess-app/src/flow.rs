//! Data types of the file, clipboard and dialog flows (spec M2-2, D100-D104).

use std::path::PathBuf;
use std::sync::Arc;

use mychess_core::Position;

use crate::bases::{BasesNotice, SlotId};
use crate::clipboard::{ClipboardError, PasteFenError};
use crate::error::FileError;
use crate::files::{GameOrigin, PgnCollection};
use crate::import::ImportReport;
use crate::prefs::PrefsIssue;
use crate::shared::SharedGame;

/// A command from a menu, toolbar or shortcut that has no data of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// New game.
    NewGame,
    /// Choose a PGN file and open it.
    Open,
    /// Open the nth recent file (0 is the newest).
    OpenRecent(usize),
    /// Forget every recent file (spec M3-6, D681).
    ClearRecent,
    /// Save to the file of the game, or ask for one.
    Save,
    /// Choose a file and save the game there.
    SaveAs,
    /// Choose a file and append the game to it.
    Append,
    /// Next game of the open file.
    NextGame,
    /// Previous game of the open file.
    PrevGame,
    /// First game of the open file.
    FirstGame,
    /// Last game of the open file.
    LastGame,
    /// Copy the position as FEN.
    CopyFen,
    /// Replace the game with the position in the clipboard.
    PasteFen,
    /// Copy the game as PGN.
    CopyPgn,
    /// Parse the clipboard as moves from the cursor position (the tree
    /// slice inserts them; see `AppState::pasted_moves`).
    PasteMoves,
    /// Open the "set up position" dialog.
    SetupBoard,
    /// New game from Chess960 starting position `n` (`0..=959`, 518 is the
    /// standard arrangement); a number out of range does nothing (M9-6,
    /// D4501).
    NewChess960(u16),
}

/// A new game to put in place of the current one (D103).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Replacement {
    /// An empty game from the standard position.
    New,
    /// A game that starts from this position, with default tags.
    FromPosition(Position),
    /// A game started by the electronic board (M8-10, D2729, D2730): from
    /// the position the board shows, or the standard one, with the `Event`
    /// tag naming the board.
    Board {
        /// The position of the board, when it told one.
        position: Option<Position>,
        /// The `Event` tag.
        event: &'static str,
    },
    /// A game that was read.
    Loaded {
        /// The game.
        game: SharedGame,
        /// Where it came from.
        origin: GameOrigin,
        /// Whether it counts as altered (pasted text does).
        dirty: bool,
        /// The collection it belongs to, for next/previous game.
        collection: Option<Arc<PgnCollection>>,
    },
    /// A game of an open database (slice M3-4): like `Loaded`, and the
    /// slot remembers the number as its current game.
    FromBase {
        /// The slot it was read from (the current one).
        slot: SlotId,
        /// The game number in the database, from 1.
        number: u32,
        /// The game.
        game: SharedGame,
        /// Where it came from.
        origin: GameOrigin,
        /// Set when the game comes from the browser: the ply to show and
        /// whether the browser closes (slice 4e).
        landing: Option<BrowserLanding>,
    },
}

/// Where a game loaded from the browser lands (slice 4e, D603).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserLanding {
    /// The ply of the main line the cursor goes to.
    pub ply: usize,
    /// Close the browser once the game is in the editor.
    pub close_browser: bool,
    /// Whether the browser's board was turned: the main board is turned
    /// the same way once the game is in the editor (D603, D607).
    pub flipped: bool,
    /// The browser that asked; only that one is closed.
    pub browser: crate::error::RequestId,
}

/// What waits for the answer to "discard changes?".
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Pending {
    /// Replace the game.
    Replace(Replacement),
    /// Quit the program.
    Quit,
    /// Close a slot (the game of that slot was the one asked about).
    CloseBase(SlotId),
    /// Start a session of the tactics training, which parks the game
    /// (spec M8-4d, D2145).
    TacticsStart(Box<crate::tactics::TacticsStartRequest>),
    /// Start a calculation exercise, which parks the game (spec M8-6,
    /// D2320).
    CalvarStart(Box<crate::calvar::CalvarStartRequest>),
    /// Start a session of the opening trainer (spec M8-5, D2229).
    OpeningStart(Box<crate::opening_trainer::OpeningStartRequest>),
    /// Start the maintenance operation that waits in
    /// `MaintenanceUi::staged`, which rewrites the games of the database
    /// (spec M7-1, rule 3).
    Maintenance,
}

/// The "this game has been altered, save it?" question.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscardPrompt {
    /// What happens after "Save" or "Don't save".
    pub pending: Pending,
}

/// The answer to a [`DiscardPrompt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardChoice {
    /// Save first; if saving fails or is cancelled, nothing else happens.
    Save,
    /// Discard the changes.
    DontSave,
    /// Do nothing.
    Cancel,
}

/// Why saving over a file needs the user's consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OverwriteReason {
    /// The file holds several games and all of them would be replaced.
    MultiGame,
    /// The file was only partly read (a limit was hit or text outside the
    /// games was skipped); what was not read would be lost.
    PartiallyRead,
    /// The file exists and was not the one the user picked: the `.pgn`
    /// extension was added to the chosen name.
    ExistingFile,
}

/// "This file has several games; replace all of them with this one?"
#[derive(Debug, Clone, PartialEq)]
pub struct OverwritePrompt {
    /// The file.
    pub path: PathBuf,
    /// How many games it holds (0 when not known).
    pub games: usize,
    /// Why the question is asked.
    pub reason: OverwriteReason,
    pub(crate) then: Option<Pending>,
}

/// The result of parsing pasted moves (D123).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovetextOutcome {
    /// The moves as a game that starts from the position at the cursor.
    pub game: SharedGame,
    /// What went wrong while reading.
    pub report: ImportReport,
}

/// A remark for the status bar or a message box, as data.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AppNotice {
    /// A file operation failed.
    File(FileError),
    /// The clipboard could not be used.
    Clipboard(ClipboardError),
    /// The clipboard text is not a FEN; the game is unchanged.
    PasteFen(PasteFenError),
    /// The clipboard could not be written.
    ClipboardWriteFailed,
    /// The front-end was asked to run an effect it does not know.
    HostEffectNotHandled,
    /// Chess960 positions cannot be set up by hand. No longer raised since
    /// the editor takes Chess960 (M9-6); kept so the text stays translated.
    SetupChess960Unsupported,
    /// A remark about the preferences.
    Prefs(PrefsIssue),
    /// A save was asked while another one is still being written.
    SaveInProgress,
    /// The comment editor refused text because the comment would pass the
    /// size limit.
    CommentTooLong,
    /// The annotation field held an unknown token and was dropped when the
    /// cursor moved.
    NagDraftDiscarded,
    /// A remark about the database slots or the Clipbase.
    Bases(BasesNotice),
    /// A remark about the bookmarks (spec M3-6).
    Bookmark(crate::bookmarks::BookmarkNotice),
    /// A remark about the engines (spec M4-3).
    Engine(crate::engines::EngineNotice),
    /// A remark about the analysis window (spec M4-4).
    Analysis(crate::analysis::AnalysisNotice),
    /// A remark about the automatic annotation (spec M4-6).
    Annotate(crate::annotate::AnnotateNotice),
    /// A remark about the threat, the shoot-out or the automove (spec
    /// M4-8).
    Mode(crate::training::ModeNotice),
    /// A remark about the game against an engine (spec M8-1).
    Play(crate::play::PlayNotice),
    /// A remark about the tournament between engines (spec M8-2).
    Tournament(crate::tournament::TournamentNotice),
    /// A remark about the online play (spec M8-3).
    Online(crate::online::OnlineNotice),
    /// A remark about the play on FICS (spec M8-9).
    Fics(crate::fics::FicsNotice),
    /// A remark about the tablebases (spec M8-8).
    Tablebase(crate::tablebase::TablebaseNotice),
    /// A remark about the crosstable panel (spec M6-4).
    Crosstable(crate::crosstable::CrosstableNotice),
    /// A remark about the spelling file, the name checker and "Add Elo"
    /// (spec M6-3).
    Spelling(crate::spelling_ui::SpellingNotice),
    /// A remark about the finders of players and tournaments (spec M6-3).
    Finder(crate::finder_ui::FinderNotice),
    /// A remark about the EPD window (spec M7-5).
    Epd(crate::epd_window::EpdNotice),
    /// A remark about the opening report (spec M7-2).
    OpeningReport(crate::opening_report::OpeningReportNotice),
    /// A remark about the player report (spec M7-3).
    PlayerReport(crate::player_report::PlayerReportNotice),
    /// A remark about the start-up (spec M9-4).
    Start(crate::start::StartNotice),
    /// A remark about the maintenance tools (spec M7-1).
    Maintenance(crate::maintenance::MaintNotice),
    /// A remark about the merge of a game (spec M7-6).
    Merge(crate::merge_game::MergeNotice),
    /// A remark about the player information (spec M6-3).
    PlayerInfo(crate::player_info_ui::PlayerInfoNotice),
    /// A remark about the statistics and graphs (spec M6-4).
    Charts(crate::charts::ChartsNotice),
    /// A remark about the piece tracker (spec M6-4).
    PieceTracker(crate::piece_tracker::PieceTrackerNotice),
    /// A remark about the electronic board (spec M8-10).
    Eboard(crate::eboard::EboardNotice),
    /// A remark about the ECO tools (spec M5-7b).
    Eco(crate::eco_ui::EcoNotice),
    /// A remark about the import of PGN files (spec M3-8).
    FileImport(crate::file_import::FileImportNotice),
    /// A remark about the export of games (spec M3-9).
    GamesExport(crate::games_export::GamesExportNotice),
    /// A remark about the tree masks (spec M5-4b).
    Mask(crate::tree::MaskNotice),
    /// A remark about the advanced searches (spec M6-1).
    AdvancedSearch(crate::advanced_search::AdvancedSearchNotice),
    /// A remark about the search for tactics and patterns (spec M6-2).
    PatternSearch(crate::pattern_search::PatternSearchNotice),
    /// A database file could not be opened for a reason that does not name
    /// the file (empty, not a PGN): the front-end adds the file it was
    /// opening (spec M3-4, D553).
    BaseFile {
        /// The file being opened.
        path: PathBuf,
        /// Why it failed.
        error: crate::bases::BaseError,
    },
}

impl AppNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            AppNotice::File(error) => error.message_key(),
            AppNotice::Clipboard(error) => error.message_key(),
            AppNotice::PasteFen(error) => error.message_key(),
            AppNotice::ClipboardWriteFailed => "clipboard-error-write-failed",
            AppNotice::HostEffectNotHandled => "app-notice-host-effect-not-handled",
            AppNotice::SetupChess960Unsupported => "setup-chess960-unsupported",
            AppNotice::Prefs(issue) => issue.message_key(),
            AppNotice::SaveInProgress => "app-notice-save-in-progress",
            AppNotice::CommentTooLong => "status-comment-too-long",
            AppNotice::NagDraftDiscarded => "status-nag-draft-discarded",
            AppNotice::Bases(notice) => notice.message_key(),
            AppNotice::Bookmark(notice) => notice.message_key(),
            AppNotice::Engine(notice) => notice.message_key(),
            AppNotice::Analysis(notice) => notice.message_key(),
            AppNotice::Annotate(notice) => notice.message_key(),
            AppNotice::Mode(notice) => notice.message_key(),
            AppNotice::Play(notice) => notice.message_key(),
            AppNotice::Tournament(notice) => notice.message_key(),
            AppNotice::Online(notice) => notice.message_key(),
            AppNotice::Fics(notice) => notice.message_key(),
            AppNotice::Tablebase(notice) => notice.message_key(),
            AppNotice::Crosstable(notice) => notice.message_key(),
            AppNotice::Spelling(notice) => notice.message_key(),
            AppNotice::Finder(notice) => notice.message_key(),
            AppNotice::Epd(notice) => notice.message_key(),
            AppNotice::OpeningReport(notice) => notice.message_key(),
            AppNotice::PlayerReport(notice) => notice.message_key(),
            AppNotice::Maintenance(notice) => notice.message_key(),
            AppNotice::Start(notice) => notice.message_key(),
            AppNotice::Merge(notice) => notice.message_key(),
            AppNotice::PlayerInfo(notice) => notice.message_key(),
            AppNotice::Charts(notice) => notice.message_key(),
            AppNotice::PieceTracker(notice) => notice.message_key(),
            AppNotice::Eboard(notice) => notice.message_key(),
            AppNotice::Eco(notice) => notice.message_key(),
            AppNotice::FileImport(notice) => notice.message_key(),
            AppNotice::GamesExport(notice) => notice.message_key(),
            AppNotice::Mask(notice) => notice.message_key(),
            AppNotice::AdvancedSearch(notice) => notice.message_key(),
            AppNotice::PatternSearch(notice) => notice.message_key(),
            AppNotice::BaseFile { error, .. } => error.message_key(),
        }
    }
}
