//! Effects: side effects described as data and executed by [`perform`] or,
//! for the host effects, by the front-end (spec M2-2, D100).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mychess_core::{PartialDate, Position};
use mychess_pgn::eco_file::{EcoFileError, read_eco_file};
use mychess_pgn::read::{DiagnosticKind, ReadOptions, parse_movetext};

use crate::bases::{
    BaseError, BasesMessage, BrowseMessage, CopyList, CopyTarget, FileStamp, ListColumn,
    ListMessage, OpenControl, SearchKind, SearchSpec, SharedSource, SlotId, copy_games_to,
    find_text, open_base, read_browser_games, sort_order, toggle_deleted, unreadable_games,
};
use crate::bookmarks::{
    Bookmarks, BookmarksMessage, GameBookmark, best_match, load_file as load_bookmarks,
    save_file as save_bookmarks,
};
use crate::clipboard::ClipboardPurpose;
use crate::engines::{
    EngineDraft, EngineEntry, EngineHub, EngineList, EngineMessage, RunDefaults, RunId, StoreError,
    ops as engine_ops,
};
use crate::error::{FileError, FileErrorKind, RequestId};
use crate::files::{
    PgnCollection, PgnSource, SaveMode, ScanLimits, ScanOutcome, save_pgn, scan_pgn,
};
use crate::flow::MovetextOutcome;
use crate::fsutil::{canonical_or_absolute, same_file};
use crate::host::PickSpec;
use crate::import::{ImportMode, ImportNotice, ImportReport};
use crate::message::Message;
use crate::prefs::{LoadedPreferences, PgnExportPrefs, Preferences, load_file, save_file};
use crate::session::GameMessage;
use crate::shared::{SharedEco, SharedGame};

/// Checks `perform` makes before it writes a PGN file; they need file
/// system access, so `update` cannot make them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveGuard {
    /// Fail with [`FileErrorKind::AlreadyExists`] if the target exists (the
    /// name was altered after the user confirmed it).
    pub create_only: bool,
    /// Fail with [`FileErrorKind::NeedsConfirmation`] if the target is this
    /// file (same canonical path, or same device and inode on Unix).
    pub protect: Option<PathBuf>,
}

impl SaveGuard {
    fn check(&self, path: &Path) -> Result<(), FileError> {
        let fail = |kind| Err(FileError::new(kind, Some(path.to_path_buf())));
        if let Some(open) = &self.protect
            && same_file(path, open)
        {
            return fail(FileErrorKind::NeedsConfirmation);
        }
        if self.create_only && std::fs::symlink_metadata(path).is_ok() {
            return fail(FileErrorKind::AlreadyExists);
        }
        Ok(())
    }
}

/// A side effect requested by [`update`](crate::update).
///
/// The front-end runs each effect (typically as an async task) and feeds the
/// resulting [`Message`] back to `update`. [`Effect::runs_on_host`] tells
/// which effects the front-end must run itself; [`perform`] answers
/// [`Message::HostEffectNotHandled`] for those.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Effect {
    /// Reads or writes the files of the tactics training, or runs its
    /// solver (spec M8-4, D1936); answers with a
    /// [`TacticsMessage`](crate::tactics::TacticsMessage).
    Tactics(crate::tactics::TacticsEffect),
    /// Reads or writes the file of the calculation exercise, or runs its
    /// engine jobs; answers with a
    /// [`CalvarMessage`](crate::calvar::CalvarMessage).
    Calvar(crate::calvar::CalvarEffect),
    /// Opens the Syzygy folders, probes a tablebase or waits; answers with a
    /// [`TablebaseMessage`](crate::tablebase::TablebaseMessage) (spec M8-8).
    Tablebase(crate::tablebase::TablebaseEffect),
    /// Selects the games of a tournament and builds its table, waits for the
    /// debounce, writes the table to a file or renders it for the clipboard;
    /// answers with a [`CrosstableMessage`](crate::crosstable::CrosstableMessage)
    /// (spec M6-4). The host **must** run it, or the panel waits for ever.
    Crosstable(crate::crosstable::CrosstableEffect),
    /// Reads the spelling file and the photos, builds the `PlayerStats`,
    /// scans the names, plans "Add Elo" and writes the corrected names and
    /// the ratings; answers with a
    /// [`SpellingMessage`](crate::spelling_ui::SpellingMessage) (spec M6-3).
    /// The host **must** run it.
    Spelling(crate::spelling_ui::SpellingEffect),
    /// Builds the table of players, queries it, finds tournaments or waits;
    /// answers with a [`FinderMessage`](crate::finder_ui::FinderMessage) (spec
    /// M6-3). The host **must** run it.
    Finder(crate::finder_ui::FinderEffect),
    /// Counts and checks a database, runs a maintenance job (flags, header,
    /// duplicates, compaction, sort, tags, names, repair, Cleaner) or
    /// compares two games; answers with a
    /// [`MaintenanceMessage`](crate::maintenance::MaintenanceMessage) (spec
    /// M7-1). The host **must** run it.
    Maintenance(crate::maintenance::MaintenanceEffect),
    /// Reads games of a database for the merge into the current game;
    /// answers with a [`MergeMessage::Loaded`](crate::MergeMessage) (spec
    /// M7-6). The host **must** run it.
    LoadGamesForMerge {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The games, from 1.
        numbers: Vec<u32>,
    },
    /// Counts the games of a player, sets the filter from a link, opens the
    /// FIDE profile or waits; answers with a
    /// [`PlayerInfoMessage`](crate::player_info_ui::PlayerInfoMessage) (spec
    /// M6-3). The host **must** run it.
    PlayerInfo(crate::player_info_ui::PlayerInfoEffect),
    /// Counts the statistics and graphs of the filter and the rating
    /// histories; answers with a [`ChartsMessage`](crate::charts::ChartsMessage)
    /// (spec M6-4). The host **must** run it.
    Charts(crate::charts::ChartsEffect),
    /// Follows pieces through the games of the filter; answers with a
    /// [`PieceTrackerMessage`](crate::piece_tracker::PieceTrackerMessage)
    /// (spec M6-4). The host **must** run it.
    PieceTracker(crate::piece_tracker::PieceTrackerEffect),
    /// Connects, sends to or ends the session of the electronic board, or
    /// lists the serial ports; answers with an
    /// [`EboardMessage`](crate::eboard::EboardMessage) (spec M8-10).
    Eboard(crate::eboard::EboardEffect),
    /// Plays a sound or reads the sounds folder; answers with a
    /// [`SoundMessage`](crate::sound::SoundMessage) (spec M8-11).
    Sound(crate::sound::SoundEffect),
    /// Reads or writes the file of the opening trainer, or builds its
    /// repertoire; the answer is an
    /// [`OpeningMessage`](crate::opening_trainer::OpeningMessage).
    OpeningTrainer(crate::opening_trainer::OpeningEffect),

    /// Reads or writes a tree mask, or runs one of its jobs (spec M5-4b);
    /// answers with a [`TreeMessage::Mask`](crate::TreeMessage::Mask). The
    /// host **must** run it (the `perform` of the front-end does).
    Mask(crate::tree::MaskEffect),
    /// Reads, writes, orders or searches an EPD book, edits it in a batch
    /// or waits for the watchdog of the analysis of positions; answers with
    /// an [`EpdMessage`](crate::epd_window::EpdMessage) (spec M7-5). The
    /// host **must** run it.
    Epd(crate::epd_window::EpdEffect),
    /// Creates the `MyChess` folder of the user and looks for engines in it;
    /// answers a [`StartMessage`](crate::start::StartMessage) (spec M9-4).
    Start(crate::start::StartEffect),
    /// Searches the games and makes the opening report, writes its
    /// documents and the favorites, opens the HTML; answers with an
    /// [`OpeningReportMessage`](crate::opening_report::OpeningReportMessage)
    /// (spec M7-2). The host **must** run it.
    OpeningReport(crate::opening_report::OpeningReportEffect),
    /// Asks the system to open an `http(s)` address in the browser (the
    /// manual, spec M9-5); answers with a
    /// [`ManualMessage::UrlOpened`](crate::manual::ManualMessage::UrlOpened).
    /// The host **must** run it.
    OpenUrl(String),
    /// Selects the games and makes the player report, writes its documents
    /// and opens the HTML; answers with a
    /// [`PlayerReportMessage`](crate::player_report::PlayerReportMessage)
    /// (spec M7-3). The host **must** run it.
    PlayerReport(crate::player_report::PlayerReportEffect),

    /// Reads a Polyglot book on a blocking thread (spec M5-6, D1652): into
    /// a side of the book window, or as the opening book.
    BookLoad {
        /// Correlates the answer.
        request: RequestId,
        /// Where the book goes.
        target: crate::book::BookTarget,
        /// The file.
        path: PathBuf,
    },

    /// Writes a copy of a book (to `path`, or to its own file) atomically
    /// and answers with the copy (spec M5-6, D1659).
    BookSave {
        /// Correlates the answer.
        request: RequestId,
        /// The book to write.
        book: crate::book::SharedBook,
        /// The destination; `None` writes to the book's own file.
        path: Option<PathBuf>,
    },

    /// Makes a book from the games `games` of a database (spec M5-6,
    /// D1660).
    BookCreate {
        /// Correlates the answer.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The numbers of the games, in the order of the filter.
        games: Vec<u32>,
        /// The options.
        options: crate::book::BookCreateOptions,
        /// Raised to stop the work (D1791).
        cancel: crate::book::BookCancel,
    },

    /// Writes a branch of a book as PGN (spec M5-6d, D2160).
    BookExport {
        /// Correlates the answer.
        request: RequestId,
        /// The book (read only).
        book: crate::book::SharedBook,
        /// The position the branch starts at.
        start: crate::book::BranchStart,
        /// The limits.
        options: crate::book::BookExportOptions,
        /// The file, with the name already fixed.
        dest: PathBuf,
        /// The user confirmed exactly `dest`.
        confirmed: bool,
        /// Files that must not be written over (open databases).
        protected: Vec<PathBuf>,
        /// Progress and cancellation.
        control: OpenControl,
    },

    /// Merges two books (spec M5-6, D1661).
    BookMerge {
        /// Correlates the answer.
        request: RequestId,
        /// The first book.
        first: crate::book::SharedBook,
        /// The second book.
        second: crate::book::SharedBook,
        /// How.
        mode: crate::book::BookMergeMode,
    },

    /// Wait `delay`, then deliver
    /// [`GameMessage::AutoplayTick`] carrying `generation`. A stale
    /// generation is ignored by the session, so there is no cancel effect.
    ScheduleAutoplayTick {
        /// Generation of the autoplay run that asked for the tick.
        generation: u64,
        /// How long to wait before the tick.
        delay: Duration,
    },

    /// Search the opening tree of a slot on a blocking thread (spec M5-2,
    /// D962). The job owns the engine state of the slot; the answer
    /// ([`TreeMessage::Finished`](crate::TreeMessage::Finished)) gives it
    /// back. The host **must** run it, or the panel waits for ever.
    TreeSearch {
        /// What to search.
        job: Box<crate::tree::TreeJob>,
    },

    /// Save, fill or describe the cache of the trees of a slot on a blocking
    /// thread (spec M5-3b, D1766). The host **must** run it; the answer is
    /// [`TreeMessage::CacheFinished`](crate::TreeMessage::CacheFinished).
    TreeCache {
        /// What to do.
        job: Box<crate::tree::CacheJob>,
    },

    /// Empty the cache of the trees of a database that changed and remove
    /// its `.stc` file, after any job that holds the cache (spec M5-3b,
    /// D1772). The host **must** run it; the answer is
    /// [`TreeMessage::CacheDiscarded`](crate::TreeMessage::CacheDiscarded).
    TreeCacheDiscard {
        /// What to discard.
        discard: Box<crate::tree::CacheDiscard>,
    },

    /// Wait `delay`, then deliver
    /// [`TreeMessage::CacheDeadline`](crate::TreeMessage::CacheDeadline) with
    /// `generation`: the limit of the wait of the quit for the tree caches
    /// (spec M5-3b, D1780). The host **must** run it.
    TreeCacheDeadline {
        /// How long to wait.
        delay: Duration,
        /// Identity of the timer.
        generation: u64,
    },

    /// Wait `delay` and answer with
    /// [`ModeMessage::ModeTick`](crate::ModeMessage::ModeTick): the automove
    /// asks again after an engine that gave no move (spec M4-8, D493). A
    /// stale `generation` is ignored, so there is no cancel effect.
    ///
    /// The host **must** run it (the `perform` of the front-end does): it is
    /// both the retry timer of the automove and the watchdog of every search
    /// of a shoot-out or an automove (D855), so a mode whose timer is never
    /// delivered can wait for ever for a mute engine.
    ScheduleModeTick {
        /// The execution that asked.
        run: RunId,
        /// The generation of the cycle that asked.
        generation: u64,
        /// How long to wait.
        delay: Duration,
    },

    /// Wait `delay` and answer with
    /// [`AnnotateMessage::Tick`](crate::AnnotateMessage::Tick): the watchdog
    /// of the annotation (spec M4-6, D895). A stale `generation` is ignored,
    /// so there is no cancel effect.
    ScheduleAnnotateTick {
        /// The execution of the run.
        run: RunId,
        /// The generation of the timer.
        generation: u64,
        /// How long to wait.
        delay: Duration,
    },

    /// Wait `after` and answer with
    /// [`ReportMessage::WalkTick`](crate::report::ReportMessage::WalkTick): the next
    /// step of the walk of the review to its moment (spec M4-11, D1666). A
    /// stale `stamp` is ignored, so there is no cancel effect; a host that
    /// drops it only leaves the walk to be finished by the next action of the
    /// user.
    ScheduleReviewTick {
        /// How long to wait.
        after: Duration,
        /// The walk that asked.
        stamp: u64,
    },

    /// One step of the batch annotation: saves a game in the `.si4` file,
    /// reopens it and reads the next game; answer with
    /// [`AnnotateMessage::BatchStep`](crate::AnnotateMessage::BatchStep)
    /// (M4-6 phase B).
    AnnotateBatchStep {
        /// The `.si4` file.
        path: PathBuf,
        /// The game to write back, if any.
        save: Option<crate::annotate::BatchSave>,
        /// The next game to read, if any.
        next: Option<u32>,
        /// The number of games the slot was loaded with.
        expected_total: u32,
        /// The state of the file when the slot last read it.
        stamp: Option<FileStamp>,
    },

    /// Reads the Polyglot book of a game against an engine; answer with
    /// [`PlayMessage::BookLoaded`](crate::PlayMessage::BookLoaded) (M8-1,
    /// D1234).
    LoadPlayBook {
        /// The `.bin` file.
        path: PathBuf,
        /// How the move is picked.
        selection: crate::play::BookSelection,
        /// The request, given back in the answer.
        request: u64,
    },

    /// Draws a position for a tactical game from a file or from the games
    /// of the current database; answer with
    /// [`PlayMessage::Picked`](crate::PlayMessage::Picked) (M8-7, D2460).
    PlayPick {
        /// The request, given back in the answer.
        request: u64,
        /// What to read and how to draw.
        job: crate::tactical_game::PickJob,
    },

    /// Prepares a tournament between engines: checks the book and the suite
    /// of openings, reads them and finds the directory and the name of the
    /// files; answer with
    /// [`TournamentMessage::Loaded`](crate::tournament::TournamentMessage::Loaded)
    /// (M8-2).
    TournamentLoad {
        /// The request, given back in the answer.
        request: u64,
        /// The configuration.
        config: Box<crate::tournament::TournamentConfig>,
        /// The base of the file names of a tournament that is resumed.
        slug: Option<String>,
        /// The directory of the tournaments; the user's data directory when
        /// `None`.
        dir: Option<PathBuf>,
        /// The folders of Syzygy tables to open for the adjudication (D2538);
        /// empty when the rule is off or the user has no local tables.
        tablebase_dirs: Vec<PathBuf>,
    },
    /// Something for the FICS play to do (spec M8-9); the hub carries the
    /// connection and the staged password. Answer with a
    /// [`FicsMessage`](crate::fics::FicsMessage); what the server says
    /// arrives through [`FicsHub::events`](crate::fics::FicsHub::events).
    Fics {
        /// What to do.
        command: crate::fics::FicsCommand,
        /// The hub that does it.
        hub: crate::fics::FicsHub,
    },
    /// Something for the online play to do (spec M8-3); the hub carries the
    /// token, the streams and the stores. Answer with an
    /// [`OnlineMessage`](crate::online::OnlineMessage); what the streams say
    /// arrives through [`OnlineHub::events`](crate::online::OnlineHub::events).
    Online {
        /// What to do.
        command: crate::online::OnlineCommand,
        /// The hub that does it.
        hub: crate::online::OnlineHub,
    },
    /// Starts the engines of a game and sends a new game; answer with
    /// [`TournamentMessage::Prepared`](crate::tournament::TournamentMessage::Prepared).
    TournamentPrepare {
        /// The engines of the tournament.
        pool: crate::tournament::EnginePool,
        /// What to start.
        plan: Box<crate::tournament::PreparePlan>,
        /// The epoch of the request.
        epoch: u64,
    },
    /// Asks an engine of the tournament for a move; answer with
    /// [`TournamentMessage::Searched`](crate::tournament::TournamentMessage::Searched).
    /// The time is measured inside the task (M8-2, D1267).
    TournamentSearch {
        /// The engines of the tournament.
        pool: crate::tournament::EnginePool,
        /// The search.
        job: Box<crate::tournament::SearchJob>,
    },
    /// Cancels, pauses the pondering of, or ends the engines of the
    /// tournament; answer with
    /// [`TournamentMessage::PoolDone`](crate::tournament::TournamentMessage::PoolDone).
    TournamentPool {
        /// The engines of the tournament.
        pool: crate::tournament::EnginePool,
        /// The command.
        command: crate::tournament::PoolCommand,
    },
    /// Appends a game to the PGN of the tournament; answer with
    /// [`TournamentMessage::GameSaved`](crate::tournament::TournamentMessage::GameSaved).
    TournamentSaveGame {
        /// The PGN of the tournament.
        path: PathBuf,
        /// The number of the game.
        number: u32,
        /// The game.
        game: SharedGame,
    },
    /// Writes the state of the tournament atomically; answer with
    /// [`TournamentMessage::StateSaved`](crate::tournament::TournamentMessage::StateSaved).
    TournamentSaveState {
        /// The file.
        path: PathBuf,
        /// The state.
        state: Box<crate::tournament::SavedTournament>,
    },
    /// Reads the cores and the free memory of the machine; answer with
    /// [`TournamentMessage::Resources`](crate::tournament::TournamentMessage::Resources)
    /// (M8-2c, D2502).
    TournamentProbe,
    /// Adds a game to the `.si4` file of a slot with a short-lived writer and
    /// opens the database again; answer with
    /// [`TournamentMessage::SlotAdded`](crate::tournament::TournamentMessage::SlotAdded)
    /// (M8-2c, D2519).
    TournamentAddToSlot {
        /// The `.si4` file.
        path: PathBuf,
        /// The number of games the slot was loaded with.
        expected_total: u32,
        /// The state of the file when the slot last read it.
        stamp: Option<FileStamp>,
        /// The number of the game in the tournament.
        number: u32,
        /// The game.
        game: SharedGame,
    },
    /// Reads a saved tournament; answer with
    /// [`TournamentMessage::StateLoaded`](crate::tournament::TournamentMessage::StateLoaded).
    TournamentLoadState {
        /// The request, given back in the answer.
        request: u64,
        /// The file.
        path: PathBuf,
    },
    /// Lists the tournaments that can be resumed; answer with
    /// [`TournamentMessage::Resumable`](crate::tournament::TournamentMessage::Resumable).
    TournamentList {
        /// The directory; the user's data directory when `None`.
        dir: Option<PathBuf>,
    },
    /// Writes the text of a table; answer with
    /// [`TournamentMessage::Exported`](crate::tournament::TournamentMessage::Exported).
    TournamentExport {
        /// The file.
        path: PathBuf,
        /// What to write.
        text: String,
    },

    // ---- host effects (run by the front-end) ----
    /// Show a file dialog; answer with [`Message::FilePicked`].
    PickFile(PickSpec),
    /// Read the clipboard text; answer with [`Message::ClipboardRead`]
    /// (`Unavailable` when it cannot be read).
    ReadClipboard(ClipboardPurpose),
    /// Put this text in the clipboard; on failure answer with
    /// [`Message::ClipboardWriteFailed`].
    WriteClipboard(String),
    /// Close the program (everything was saved).
    Exit,

    // ---- effects run by `perform` ----
    /// Read PGN text or a file ([`Message::PgnScanned`]).
    ScanPgn {
        /// The request.
        request: RequestId,
        /// What to read.
        source: PgnSource,
    },
    /// Parse one game of a collection ([`Message::CollectionGameParsed`]).
    ParseCollectionGame {
        /// The request.
        request: RequestId,
        /// The collection.
        collection: Arc<PgnCollection>,
        /// The game (0-based).
        index: usize,
    },
    /// Write a game to a PGN file ([`Message::PgnSaved`]).
    SavePgn {
        /// The request.
        request: RequestId,
        /// The file.
        path: PathBuf,
        /// Overwrite or append.
        mode: SaveMode,
        /// The game, as it is now.
        game: SharedGame,
        /// How to write it.
        options: PgnExportPrefs,
        /// Checks to make first.
        guard: SaveGuard,
    },
    /// Parse pasted moves ([`Message::MovetextParsed`]).
    ParseMovetext {
        /// The request.
        request: RequestId,
        /// The position the moves start from.
        start: Position,
        /// The text.
        text: String,
    },
    /// Read the preferences file ([`Message::PreferencesLoaded`]).
    LoadPreferences {
        /// The file.
        path: PathBuf,
    },
    /// Write the preferences file ([`Message::PreferencesSaved`]).
    SavePreferences {
        /// The file.
        path: PathBuf,
        /// What to write.
        prefs: Box<Preferences>,
    },
    /// Read the bookmarks file ([`BookmarksMessage::Loaded`]).
    LoadBookmarks {
        /// The file.
        path: PathBuf,
    },
    /// Write the bookmarks file ([`BookmarksMessage::Saved`]).
    SaveBookmarks {
        /// The file.
        path: PathBuf,
        /// What to write.
        bookmarks: Box<Bookmarks>,
    },
    /// Find the game of a database that best matches a bookmark
    /// ([`BookmarksMessage::Found`]).
    FindBookmarkGame {
        /// The request.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The bookmark.
        bookmark: Box<GameBookmark>,
    },
    /// Read the engine list file ([`EngineMessage::Loaded`]).
    LoadEngines {
        /// The file.
        path: PathBuf,
    },
    /// Write the engine list file ([`EngineMessage::Saved`]).
    SaveEngines {
        /// The file.
        path: PathBuf,
        /// What to write.
        list: Box<EngineList>,
    },
    /// Start an engine and wait for its handshake
    /// ([`EngineMessage::Started`] or [`EngineMessage::StartFailed`]).
    StartEngine {
        /// The execution.
        run: RunId,
        /// The engine to run.
        entry: Box<EngineEntry>,
        /// The analysis preferences that apply to every engine.
        defaults: RunDefaults,
        /// Where the live engines are kept.
        hub: EngineHub,
    },
    /// Hand a command to a running engine; never waits for the engine
    /// ([`EngineMessage::CommandDone`]).
    EngineCommand {
        /// The execution.
        run: RunId,
        /// The command.
        command: Box<mychess_engine::EngineCommand>,
        /// Where the live engines are kept.
        hub: EngineHub,
    },
    /// End the engine of an execution ([`EngineMessage::Stopped`]).
    StopEngine {
        /// The execution.
        run: RunId,
        /// Where the live engines are kept.
        hub: EngineHub,
    },
    /// Read the entries of the protocol log of an engine written since
    /// `since` ([`AnalysisMessage::LogRead`](crate::AnalysisMessage::LogRead)).
    ReadEngineLog {
        /// The execution.
        run: RunId,
        /// The first entry wanted.
        since: u64,
        /// Where the live engines are kept.
        hub: EngineHub,
    },
    /// Write the protocol log of an engine to a file
    /// ([`AnalysisMessage::LogSaved`](crate::AnalysisMessage::LogSaved)).
    SaveEngineLog {
        /// The execution.
        run: RunId,
        /// The file.
        path: PathBuf,
        /// Where the live engines are kept.
        hub: EngineHub,
    },
    /// End every engine, waiting at most [`SHUTDOWN_WAIT`](crate::engines::SHUTDOWN_WAIT)
    /// ([`EngineMessage::ShutdownDone`]).
    ShutdownEngines {
        /// Where the live engines are kept.
        hub: EngineHub,
    },
    /// Start an engine of the editor, ask who it is and end it
    /// ([`EngineMessage::Tested`]).
    TestEngine {
        /// The request.
        request: RequestId,
        /// The engine as it is in the form.
        draft: Box<EngineDraft>,
        /// The analysis preferences.
        defaults: RunDefaults,
    },
    /// Find out the protocol of an engine of the editor
    /// ([`EngineMessage::Detected`]).
    DetectEngine {
        /// The request.
        request: RequestId,
        /// The engine as it is in the form.
        draft: Box<EngineDraft>,
        /// The analysis preferences.
        defaults: RunDefaults,
    },
    /// Look at the disk for the command and the folder of the editor
    /// ([`EngineMessage::Checked`]).
    CheckEngineCommand {
        /// The request.
        request: RequestId,
        /// The command.
        command: PathBuf,
        /// The working directory.
        working_dir: Option<PathBuf>,
    },
    /// List the installed engines of the `PATH` and the usual folders;
    /// runs nothing ([`EngineMessage::Found`]).
    FindInstalledEngines {
        /// The request.
        request: RequestId,
    },
    /// Probe a found executable as UCI, then XBoard
    /// ([`EngineMessage::FoundProbed`]).
    ProbeFoundEngine {
        /// The request.
        request: RequestId,
        /// The executable.
        path: PathBuf,
        /// The analysis preferences.
        defaults: RunDefaults,
    },
    /// Read an `engines.dat` of the Scid ([`EngineMessage::ScidRead`]).
    ImportScidEngines {
        /// The request.
        request: RequestId,
        /// The file.
        path: PathBuf,
    },
    /// Read an `.eco` file ([`Message::EcoLoaded`]).
    LoadEcoBook {
        /// The file.
        path: PathBuf,
    },
    /// Ask for today's date, UTC ([`Message::Today`]).
    QueryToday {
        /// The request.
        request: RequestId,
    },
    /// Wait `delay`, then deliver [`BasesMessage::QuitDeadline`] (the limit
    /// of the time a quit waits for the list job).
    QuitDeadline {
        /// How long to wait.
        delay: Duration,
    },
    /// Open a database file for a slot, unless it is one of `open_paths`
    /// (answer: [`BasesMessage::Opened`]). The load reports its progress to
    /// `control` and stops when it is cancelled.
    OpenBase {
        /// The request.
        request: RequestId,
        /// The file.
        path: PathBuf,
        /// Canonical paths of the databases already open.
        open_paths: Vec<PathBuf>,
        /// Whether a slot is free for a new database.
        allow_new: bool,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Read one game of an open database (answer: [`BasesMessage::GameLoaded`]).
    LoadBaseGame {
        /// The request.
        request: RequestId,
        /// The slot it belongs to (for the front-end's bookkeeping).
        slot: SlotId,
        /// The game number, from 1.
        number: u32,
        /// The database.
        source: SharedSource,
    },
    /// Read games for the browser (answer: [`BrowseMessage::Loaded`]).
    /// Reports progress to `control` and stops when it is cancelled.
    ReadBrowserGames {
        /// The request.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The games, in the order they are shown.
        numbers: Vec<u32>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Wait `delay`, then deliver [`BrowseMessage::AutoplayTick`] carrying
    /// `generation` (the autoplay of the browser).
    BrowserTick {
        /// Identifies the run the tick belongs to.
        generation: u64,
        /// How long to wait.
        delay: Duration,
    },
    /// A quick search of the list: find the next game or reduce the filter
    /// (answer: [`ListMessage::Searched`]). Reports progress to `control`
    /// and stops when it is cancelled.
    SearchList {
        /// The request.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// A copy of the filter of the slot.
        filter: Box<mychess_db::Filter>,
        /// What to search for.
        spec: SearchSpec,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// An advanced search of one database (spec M6-1, D3232): by position,
    /// material and patterns, or a sequence of moves. Runs on a blocking
    /// thread over a copy of the filter of `slot`, reports progress to
    /// `control` and stops when it is cancelled. The operation on the filter
    /// (Reset, AND, OR) is part of `query`. Answer:
    /// [`AdvancedSearchMessage::Finished`](crate::AdvancedSearchMessage::Finished),
    /// or [`AdvancedSearchMessage::Cancelled`](crate::AdvancedSearchMessage::Cancelled)
    /// when it was stopped. The host **must** run it, or the window waits for
    /// ever.
    AdvancedSearch {
        /// The request.
        request: RequestId,
        /// Which of the three searches.
        kind: crate::advanced_search::AdvancedKind,
        /// The slot whose filter the search changes.
        slot: SlotId,
        /// The database searched.
        source: SharedSource,
        /// A copy of the filter of the slot.
        filter: Box<mychess_db::Filter>,
        /// What to search for, with the operation on the filter.
        query: Box<mychess_db::AdvancedQuery>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// An operation on the saved advanced searches in `dir` (list, save,
    /// load, delete, or read a material `.sso` file; spec M6-1, D3249 item
    /// 6). Answer: [`AdvancedSearchMessage::SavedDone`](crate::AdvancedSearchMessage::SavedDone).
    AdvancedSaved {
        /// The request.
        request: RequestId,
        /// The folder of the saved searches.
        dir: PathBuf,
        /// What to do.
        command: crate::advanced_search::SavedCommand,
    },
    /// A search for a pattern in one database (spec M6-2, D3273): runs on a
    /// blocking thread over a copy of the filter of `slot`, reports progress
    /// to `control` and stops (within 500 ms, even in the mate solver) when
    /// it is cancelled. Answer:
    /// [`PatternSearchMessage::Finished`](crate::PatternSearchMessage::Finished),
    /// or [`PatternSearchMessage::Cancelled`](crate::PatternSearchMessage::Cancelled)
    /// when it was stopped. The host **must** run it, or the window waits for
    /// ever.
    PatternSearch {
        /// The request.
        request: RequestId,
        /// The slot whose filter the search changes.
        slot: SlotId,
        /// The database searched.
        source: SharedSource,
        /// A copy of the filter of the slot.
        filter: Box<mychess_db::Filter>,
        /// What to look for (already validated).
        pattern: Box<mychess_db::pattern::model::Pattern>,
        /// The operation on the filter.
        op: mychess_db::FilterOp,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// The occurrences of a pattern in one game (spec M6-2, D3284 and
    /// D3283: after loading a game of the filter, and "test on this
    /// game"). Answer:
    /// [`PatternSearchMessage::HitsDone`](crate::PatternSearchMessage::HitsDone).
    PatternHits {
        /// The request.
        request: RequestId,
        /// The game (a copy of the one in the editor).
        game: SharedGame,
        /// The pattern.
        pattern: Box<mychess_db::pattern::model::Pattern>,
    },
    /// An operation on the user's patterns in `dir` (list, save, load,
    /// delete, import, export; spec M6-2, D3277). Answer:
    /// [`PatternSearchMessage::SavedDone`](crate::PatternSearchMessage::SavedDone).
    PatternSaved {
        /// The request.
        request: RequestId,
        /// The folder of the patterns (unused by export).
        dir: PathBuf,
        /// What to do.
        command: crate::pattern_search::PatternCommand,
    },
    /// Compute the order of the view by a column (answer:
    /// [`ListMessage::Sorted`]).
    SortList {
        /// The request.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// The column.
        column: ListColumn,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Copy games of one database to the end of another (answer:
    /// [`ListMessage::GamesCopied`]). Reports progress to `control` and
    /// stops when it is cancelled; the games already written stay.
    CopyGames {
        /// The request.
        request: RequestId,
        /// The database the games are in.
        source: SharedSource,
        /// The `.si4` file of the source and its state when it was read, to
        /// refuse a source that changed since (D590).
        source_stamp: Option<(PathBuf, FileStamp)>,
        /// The games, in the order they are copied.
        list: CopyList,
        /// Where they go.
        target: CopyTarget,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Count the games of a database by ECO code for the browser (answer:
    /// [`EcoMessage::StatsBuilt`](crate::EcoMessage::StatsBuilt); spec
    /// M5-7b). The host **must** run it, or the browser waits for ever.
    EcoStats {
        /// The request.
        request: RequestId,
        /// The database.
        source: SharedSource,
        /// Count only the games of this filter (a copy of the one of the
        /// slot); `None`: all the games.
        filter: Option<Box<mychess_db::Filter>>,
    },
    /// Classify the games of a database by ECO code, writing the codes in
    /// the `.si4` file or only counting (answer: [`ListMessage::EcoClassified`];
    /// spec M5-7b). Reports progress to `control` and stops when it is
    /// cancelled; the codes already written stay.
    EcoClassify {
        /// The request.
        request: RequestId,
        /// What to do.
        job: Box<crate::eco_ui::EcoClassifyJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Import PGN files into a database, existing or new (answer:
    /// [`ListMessage::PgnImported`]; spec M3-8). Reports progress to
    /// `control` and stops when it is cancelled; the games already stored
    /// stay.
    ImportPgnFiles {
        /// The request.
        request: RequestId,
        /// What to do.
        job: Box<crate::file_import::FileImportJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Export games to a PGN file (answer: [`ListMessage::GamesExported`];
    /// spec M3-9). Reports progress to `control` and stops when it is
    /// cancelled; a new file is then thrown away.
    ExportGames {
        /// The request.
        request: RequestId,
        /// What to do.
        job: Box<crate::games_export::GamesExportJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Export games in a format other than PGN: HTML, the HTML viewer,
    /// LaTeX, EPD, the game list or CSV (answer:
    /// [`ListMessage::GamesExported`]; spec M7-4). Like `ExportGames`; when
    /// the file needs the user's word nothing is written and the answer says
    /// so.
    ExportDocs {
        /// The request.
        request: RequestId,
        /// What to do, with the options frozen.
        job: Box<crate::export_docs::ExportDocsJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Draw the image of a board position: the preview, or the file (answer:
    /// [`ExportDocsMessage::ImageRendered`](crate::ExportDocsMessage::ImageRendered);
    /// spec M7-4).
    RenderImage {
        /// The request.
        request: RequestId,
        /// What to do.
        job: Box<crate::export_docs::ImageJob>,
    },
    /// Open an exported HTML file with the default program (answer:
    /// [`ExportDocsMessage::FileOpened`](crate::ExportDocsMessage::FileOpened);
    /// spec M7-4, D3708).
    OpenExported {
        /// The file.
        path: PathBuf,
    },
    /// Look for the novelty of a game in a database (answer:
    /// [`ListMessage::EcoNovelty`]; spec M5-7b). Reports progress to
    /// `control` and stops when it is cancelled.
    EcoNovelty {
        /// The request.
        request: RequestId,
        /// What to do.
        job: Box<crate::eco_ui::EcoNoveltyJob>,
        /// Progress and cancellation.
        control: OpenControl,
    },
    /// Toggle the "deleted" flag of games of an SI4 file and reopen it
    /// (answer: [`ListMessage::Flagged`]).
    ToggleDeleted {
        /// The request.
        request: RequestId,
        /// The `.si4` file.
        path: PathBuf,
        /// The games.
        numbers: Vec<u32>,
        /// The number of games the slot was loaded with.
        expected_total: u32,
        /// The state of the file when the slot was loaded.
        stamp: Option<FileStamp>,
        /// Progress and cancellation.
        control: OpenControl,
    },
}

impl Effect {
    /// Whether the front-end must run this effect itself (file dialogs,
    /// clipboard, exit) instead of handing it to [`perform`].
    pub fn runs_on_host(&self) -> bool {
        matches!(
            self,
            Effect::PickFile(_)
                | Effect::ReadClipboard(_)
                | Effect::WriteClipboard(_)
                | Effect::Exit
        )
    }
}

/// Runs blocking `work` off the async executor.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    on_panic: impl FnOnce() -> T,
) -> T {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| on_panic())
}

fn internal_error(path: Option<PathBuf>) -> FileError {
    FileError::new(FileErrorKind::Io(std::io::ErrorKind::Other), path)
}

/// Executes one effect and returns the message that reports its result.
///
/// Must run inside a tokio runtime with the time driver enabled (the
/// front-end's executor). File work runs on `spawn_blocking`. Never panics
/// for a host effect: it answers [`Message::HostEffectNotHandled`].
pub async fn perform(effect: Effect) -> Message {
    // Boxed: the state machine of every arm together is large (it crossed the
    // `large_futures` limit when the tournament and play effects came), and
    // callers hold this future across their own `await`s.
    Box::pin(perform_inner(effect)).await
}

#[allow(clippy::too_many_lines)] // one flat match over the effects reads best
async fn perform_inner(effect: Effect) -> Message {
    match effect {
        Effect::Tactics(effect) => crate::tactics::perform(effect).await,
        Effect::Calvar(effect) => crate::calvar::perform(effect).await,
        Effect::Tablebase(effect) => crate::tablebase::perform(effect).await,
        Effect::Crosstable(effect) => crate::crosstable::perform(effect).await,
        Effect::Spelling(effect) => crate::spelling_ui::perform(effect).await,
        Effect::Finder(effect) => crate::finder_ui::perform(effect).await,
        Effect::Maintenance(effect) => crate::maintenance::perform(effect).await,
        Effect::PlayerInfo(effect) => crate::player_info_ui::perform(effect).await,
        Effect::LoadGamesForMerge {
            request,
            source,
            numbers,
        } => {
            let asked = numbers.clone();
            let games = blocking(
                move || crate::merge_game::load_games(&source, &numbers),
                move || {
                    asked
                        .into_iter()
                        .map(|n| (n, Err(crate::MergeLoadError::Decode)))
                        .collect()
                },
            )
            .await;
            Message::Merge(crate::MergeMessage::Loaded { request, games })
        }
        Effect::Charts(effect) => crate::charts::perform(effect).await,
        Effect::PieceTracker(effect) => crate::piece_tracker::perform(effect).await,
        Effect::Eboard(effect) => crate::eboard::perform(effect).await,
        Effect::Sound(effect) => crate::sound::perform(effect).await,
        Effect::OpeningTrainer(effect) => crate::opening_trainer::perform(effect).await,
        Effect::Mask(effect) => crate::tree::perform_mask(effect).await,
        Effect::Epd(effect) => crate::epd_window::perform(effect).await,
        Effect::Start(effect) => crate::start::perform(effect).await,
        Effect::OpeningReport(effect) => crate::opening_report::perform(effect).await,
        Effect::OpenUrl(url) => {
            let ok = tokio::task::spawn_blocking(move || crate::online::open_url(&url))
                .await
                .unwrap_or(false);
            Message::Manual(crate::manual::ManualMessage::UrlOpened(ok))
        }
        Effect::PlayerReport(effect) => crate::player_report::perform(effect).await,
        Effect::ScheduleAutoplayTick { generation, delay } => {
            tokio::time::sleep(delay).await;
            Message::Game(GameMessage::AutoplayTick { generation })
        }
        Effect::ScheduleModeTick {
            run,
            generation,
            delay,
        } => {
            tokio::time::sleep(delay).await;
            Message::Mode(crate::training::ModeMessage::ModeTick { run, generation })
        }
        Effect::ScheduleAnnotateTick {
            run,
            generation,
            delay,
        } => {
            tokio::time::sleep(delay).await;
            Message::Annotate(crate::AnnotateMessage::Tick { run, generation })
        }
        Effect::ScheduleReviewTick { after, stamp } => {
            tokio::time::sleep(after).await;
            Message::Report(crate::report::ReportMessage::WalkTick { stamp })
        }
        Effect::AnnotateBatchStep {
            path,
            save,
            next,
            expected_total,
            stamp,
        } => {
            let result = blocking(
                move || {
                    crate::annotate::batch_step(&path, save.as_ref(), next, expected_total, stamp)
                },
                || Err(BaseError::Internal),
            )
            .await;
            Message::Annotate(crate::AnnotateMessage::BatchStep(result))
        }
        Effect::LoadPlayBook {
            path,
            selection,
            request,
        } => {
            let result = blocking(
                move || crate::play::read_book(&path, selection),
                || Err(crate::play::BookFailure::Unreadable),
            )
            .await;
            Message::Play(crate::play::PlayMessage::BookLoaded { request, result })
        }
        Effect::PlayPick { request, job } => {
            let cancel = job.cancel.clone();
            let result = blocking(
                move || crate::tactical_game::run_pick(&job),
                || Err(crate::tactical_game::PickFailure::Unreadable),
            )
            .await;
            // A drawing that was cancelled while it ran says so.
            let result = match result {
                Ok(_) if cancel.is_cancelled() => Err(crate::tactical_game::PickFailure::Cancelled),
                other => other,
            };
            Message::Play(crate::play::PlayMessage::Picked { request, result })
        }
        effect @ (Effect::BookLoad { .. }
        | Effect::BookSave { .. }
        | Effect::BookCreate { .. }
        | Effect::BookExport { .. }
        | Effect::BookMerge { .. }) => crate::book::perform_book(effect).await,
        effect @ (Effect::TournamentLoad { .. }
        | Effect::TournamentPrepare { .. }
        | Effect::TournamentSearch { .. }
        | Effect::TournamentPool { .. }
        | Effect::TournamentSaveGame { .. }
        | Effect::TournamentSaveState { .. }
        | Effect::TournamentProbe
        | Effect::TournamentAddToSlot { .. }
        | Effect::TournamentLoadState { .. }
        | Effect::TournamentList { .. }
        | Effect::TournamentExport { .. }) => {
            // Boxed: the futures of the engine tasks are large.
            Box::pin(crate::tournament::perform_tournament(effect)).await
        }
        Effect::Fics { command, hub } => {
            Message::Fics(crate::fics::perform_fics(command, &hub).await)
        }
        Effect::Online { command, hub } => {
            Message::Online(crate::online::perform_online(command, &hub).await)
        }
        Effect::TreeSearch { job } => {
            let (generation, slot, epoch) = (job.generation, job.slot, job.epoch);
            let answer = blocking(
                move || crate::tree::run_search(*job),
                move || crate::tree::TreeAnswer::internal_failure(generation, slot, epoch),
            )
            .await;
            Message::Tree(crate::tree::TreeMessage::Finished(Box::new(answer)))
        }
        Effect::TreeCache { job } => {
            let (id, slot, autosave, cache) = (job.id, job.slot, job.autosave, job.cache.clone());
            let answer = blocking(
                move || crate::tree::run_cache_job(*job),
                move || {
                    crate::tree::CacheAnswer::internal_failure(id, slot, autosave, cache.clone())
                },
            )
            .await;
            Message::Tree(crate::tree::TreeMessage::CacheFinished(Box::new(answer)))
        }
        Effect::TreeCacheDeadline { delay, generation } => {
            tokio::time::sleep(delay).await;
            Message::Tree(crate::tree::TreeMessage::CacheDeadline(generation))
        }
        Effect::TreeCacheDiscard { discard } => {
            let file = discard.file.clone();
            let file = blocking(
                move || crate::tree::run_cache_discard(*discard),
                move || file,
            )
            .await;
            Message::Tree(crate::tree::TreeMessage::CacheDiscarded(file))
        }
        Effect::PickFile(_)
        | Effect::ReadClipboard(_)
        | Effect::WriteClipboard(_)
        | Effect::Exit => Message::HostEffectNotHandled,
        Effect::ScanPgn { request, source } => {
            let path = match &source {
                PgnSource::File(path) => Some(path.clone()),
                PgnSource::Text(_) => None,
            };
            let fallback_path = path.clone();
            let result = blocking(
                move || run_scan(source),
                move || Err(internal_error(fallback_path)),
            )
            .await;
            Message::PgnScanned {
                request,
                path,
                result,
            }
        }
        Effect::ParseCollectionGame {
            request,
            collection,
            index,
        } => {
            let result = blocking(
                move || parse_collection_game(&collection, index),
                || Err(internal_error(None)),
            )
            .await;
            Message::CollectionGameParsed { request, result }
        }
        Effect::SavePgn {
            request,
            path,
            mode,
            game,
            options,
            guard,
        } => {
            let fallback = path.clone();
            let result = blocking(
                move || {
                    guard.check(&path)?;
                    save_pgn(&path, mode, &game, &options)?;
                    Ok(canonical_or_absolute(&path))
                },
                move || Err(internal_error(Some(fallback))),
            )
            .await;
            Message::PgnSaved {
                request,
                mode,
                result,
            }
        }
        Effect::ParseMovetext {
            request,
            start,
            text,
        } => {
            let outcome = blocking(
                move || parse_moves(&text, &start),
                || MovetextOutcome {
                    game: SharedGame::new(mychess_core::Game::new()),
                    report: ImportReport::default(),
                },
            )
            .await;
            Message::MovetextParsed {
                request,
                outcome: Box::new(outcome),
            }
        }
        Effect::LoadPreferences { path } => Message::PreferencesLoaded(
            blocking(move || load_file(&path), LoadedPreferences::default).await,
        ),
        Effect::SavePreferences { path, prefs } => {
            let fallback = path.clone();
            let result = blocking(
                move || save_file(&path, &prefs),
                move || Err(internal_error(Some(fallback))),
            )
            .await;
            Message::PreferencesSaved(result)
        }
        Effect::LoadBookmarks { path } => Message::Bookmarks(BookmarksMessage::Loaded(
            blocking(move || load_bookmarks(&path), Default::default).await,
        )),
        Effect::SaveBookmarks { path, bookmarks } => {
            let fallback = path.clone();
            let result = blocking(
                move || save_bookmarks(&path, &bookmarks),
                move || Err(internal_error(Some(fallback))),
            )
            .await;
            Message::Bookmarks(BookmarksMessage::Saved(result))
        }
        Effect::FindBookmarkGame {
            request,
            source,
            bookmark,
        } => {
            let result = blocking(
                move || Ok(best_match(&*source.read(), &bookmark)),
                || Err(BaseError::Internal),
            )
            .await;
            Message::Bookmarks(BookmarksMessage::Found { request, result })
        }
        Effect::LoadEngines { path } => Message::Engine(EngineMessage::Loaded(Box::new(
            blocking(
                move || crate::engines::load_engine_file(&path),
                || Err(StoreError::File(internal_error(None))),
            )
            .await,
        ))),
        Effect::SaveEngines { path, list } => {
            let fallback = path.clone();
            let result = blocking(
                move || crate::engines::save_engine_file(&path, &list),
                move || Err(StoreError::File(internal_error(Some(fallback)))),
            )
            .await;
            Message::Engine(EngineMessage::Saved(result))
        }
        Effect::StartEngine {
            run,
            entry,
            defaults,
            hub,
        } => engine_ops::start(&hub, run, &entry, &defaults).await,
        Effect::EngineCommand { run, command, hub } => engine_ops::command(&hub, run, *command),
        Effect::StopEngine { run, hub } => engine_ops::stop(&hub, run).await,
        Effect::ReadEngineLog { run, since, hub } => engine_ops::read_log(&hub, run, since),
        Effect::SaveEngineLog { run, path, hub } => engine_ops::save_log(&hub, run, path).await,
        Effect::ShutdownEngines { hub } => engine_ops::shutdown(&hub).await,
        Effect::TestEngine {
            request,
            draft,
            defaults,
        } => engine_ops::test(request, &draft, &defaults).await,
        Effect::DetectEngine {
            request,
            draft,
            defaults,
        } => engine_ops::detect(request, &draft, &defaults).await,
        Effect::CheckEngineCommand {
            request,
            command,
            working_dir,
        } => engine_ops::check(request, command, working_dir).await,
        Effect::FindInstalledEngines { request } => engine_ops::find(request).await,
        Effect::ProbeFoundEngine {
            request,
            path,
            defaults,
        } => engine_ops::probe_found(request, path, &defaults).await,
        Effect::ImportScidEngines { request, path } => engine_ops::read_scid(request, path).await,
        Effect::LoadEcoBook { path } => {
            let fallback = path.clone();
            let echo = path.clone();
            let result = blocking(
                move || load_eco(&path),
                move || Err(internal_error(Some(fallback))),
            )
            .await;
            Message::EcoLoaded { path: echo, result }
        }
        Effect::QueryToday { request } => Message::Today(request, today_utc()),
        Effect::OpenBase {
            request,
            path,
            open_paths,
            allow_new,
            control,
        } => {
            let result = blocking(
                move || open_base(&path, &open_paths, allow_new, &control),
                || Err(BaseError::Internal),
            )
            .await;
            Message::Bases(BasesMessage::Opened { request, result })
        }
        Effect::QuitDeadline { delay } => {
            tokio::time::sleep(delay).await;
            Message::Bases(BasesMessage::QuitDeadline)
        }
        Effect::LoadBaseGame {
            request,
            slot: _,
            number,
            source,
        } => {
            let result = blocking(
                move || {
                    mychess_db::GameSource::game(&mut *source.lock(), number)
                        .map(SharedGame::new)
                        .map_err(|error| {
                            tracing::warn!(game = number, %error, "cannot read a game of a database");
                            BaseError::GameUnreadable { number }
                        })
                },
                move || Err(BaseError::GameUnreadable { number }),
            )
            .await;
            Message::Bases(BasesMessage::GameLoaded { request, result })
        }
        Effect::BrowserTick { generation, delay } => {
            tokio::time::sleep(delay).await;
            Message::Bases(BasesMessage::Browse(BrowseMessage::AutoplayTick {
                generation,
            }))
        }
        Effect::ReadBrowserGames {
            request,
            source,
            numbers,
            control,
        } => {
            let asked = numbers.clone();
            let games = blocking(
                move || read_browser_games(&source, &numbers, &control),
                move || unreadable_games(&asked),
            )
            .await;
            Message::Bases(BasesMessage::Browse(BrowseMessage::Loaded {
                request,
                games,
            }))
        }
        Effect::SearchList {
            request,
            source,
            filter,
            spec,
            control,
        } => {
            let kind: SearchKind = spec.kind;
            let result = blocking(
                move || find_text(&source, &filter, &spec, &control),
                || Err(BaseError::Internal),
            )
            .await;
            Message::Bases(BasesMessage::List(ListMessage::Searched {
                request,
                kind,
                result,
            }))
        }
        Effect::AdvancedSearch {
            request,
            kind: _,
            slot: _,
            source,
            filter,
            query,
            control,
        } => {
            let work_control = control.clone();
            let result = blocking(
                move || crate::advanced_search::run_search(source, *filter, &query, &work_control),
                || Err(crate::advanced_search::SearchFailure::Internal),
            )
            .await;
            Message::AdvancedSearch(match result {
                Ok(done) if control.is_cancelled() || done.report.interrupted => {
                    crate::advanced_search::AdvancedSearchMessage::Cancelled { request }
                }
                result => crate::advanced_search::AdvancedSearchMessage::Finished {
                    request,
                    result: result.map(Box::new),
                },
            })
        }
        Effect::AdvancedSaved {
            request,
            dir,
            command,
        } => {
            let result = blocking(
                move || crate::advanced_search::run_saved(&dir, command),
                || Err(crate::advanced_search::SavedFailure::Internal),
            )
            .await;
            Message::AdvancedSearch(crate::advanced_search::AdvancedSearchMessage::SavedDone {
                request,
                result,
            })
        }
        Effect::PatternSearch {
            request,
            slot: _,
            source,
            filter,
            pattern,
            op,
            control,
        } => {
            let work_control = control.clone();
            let result = blocking(
                move || {
                    crate::pattern_search::run_search(source, *filter, &pattern, op, &work_control)
                },
                || Err(crate::pattern_search::PatternFailure::Internal),
            )
            .await;
            Message::PatternSearch(match result {
                Ok(done) if control.is_cancelled() || done.report.interrupted => {
                    crate::pattern_search::PatternSearchMessage::Cancelled { request }
                }
                result => crate::pattern_search::PatternSearchMessage::Finished {
                    request,
                    result: result.map(Box::new),
                },
            })
        }
        Effect::PatternHits {
            request,
            game,
            pattern,
        } => {
            let result = blocking(
                move || crate::pattern_search::run_hits(&game, &pattern),
                || Err(crate::pattern_search::PatternFailure::Internal),
            )
            .await;
            Message::PatternSearch(crate::pattern_search::PatternSearchMessage::HitsDone {
                request,
                result,
            })
        }
        Effect::PatternSaved {
            request,
            dir,
            command,
        } => {
            let result = blocking(
                move || crate::pattern_search::run_saved(&dir, command),
                || Err(crate::pattern_search::SavedFailure::Internal),
            )
            .await;
            Message::PatternSearch(crate::pattern_search::PatternSearchMessage::SavedDone {
                request,
                result,
            })
        }
        Effect::SortList {
            request,
            source,
            column,
            control,
        } => {
            let result = blocking(
                move || sort_order(&source, column, &control),
                || Err(BaseError::Internal),
            )
            .await;
            Message::Bases(BasesMessage::List(ListMessage::Sorted { request, result }))
        }
        Effect::CopyGames {
            request,
            source,
            source_stamp,
            list,
            target,
            control,
        } => {
            let result = blocking(
                move || copy_games_to(&source, source_stamp.as_ref(), &list, &target, &control),
                || Err(BaseError::Internal),
            )
            .await;
            match &result {
                Ok(outcome) => tracing::debug!(
                    copied = outcome.copied,
                    truncated = outcome.truncated,
                    error = ?outcome.error,
                    "copy between databases ended"
                ),
                Err(error) => tracing::warn!(?error, "copy between databases failed"),
            }
            Message::Bases(BasesMessage::List(ListMessage::GamesCopied {
                request,
                result,
            }))
        }
        Effect::EcoStats {
            request,
            source,
            filter,
        } => {
            let table = blocking(
                move || crate::eco_ui::stats_run(&source, filter.as_deref()),
                || crate::eco_ui::SharedStats::new(mychess_db::EcoStatsTable::default()),
            )
            .await;
            Message::Eco(crate::eco_ui::EcoMessage::StatsBuilt { request, table })
        }
        Effect::EcoClassify {
            request,
            job,
            control,
        } => {
            let result = blocking(
                move || crate::eco_ui::classify_run(&job, &control),
                || Err(BaseError::Internal),
            )
            .await;
            Message::Bases(BasesMessage::List(ListMessage::EcoClassified {
                request,
                result,
            }))
        }
        Effect::ImportPgnFiles {
            request,
            job,
            control,
        } => {
            let result = blocking(
                move || crate::file_import::import_run(&job, &control).map(Box::new),
                || Err(BaseError::Internal),
            )
            .await;
            match &result {
                Ok(outcome) => tracing::debug!(
                    added = outcome.report.games_added,
                    skipped = outcome.report.games_skipped,
                    error = ?outcome.error,
                    "PGN import ended"
                ),
                Err(error) => tracing::warn!(?error, "PGN import failed"),
            }
            Message::Bases(BasesMessage::List(ListMessage::PgnImported {
                request,
                result,
            }))
        }
        Effect::ExportGames {
            request,
            job,
            control,
        } => {
            let result = blocking(
                move || crate::games_export::export_run(&job, &control).map(Box::new),
                || Err(BaseError::Internal),
            )
            .await;
            match &result {
                Ok(outcome) => tracing::debug!(
                    written = outcome.report.written,
                    skipped = outcome.report.skipped,
                    interrupted = outcome.report.interrupted,
                    "games export ended"
                ),
                Err(error) => tracing::warn!(?error, "games export failed"),
            }
            Message::Bases(BasesMessage::List(ListMessage::GamesExported {
                request,
                result,
            }))
        }
        Effect::ExportDocs {
            request,
            job,
            control,
        } => {
            let result = blocking(
                move || crate::export_docs::docs_run(&job, &control).map(Box::new),
                || Err(BaseError::Internal),
            )
            .await;
            match &result {
                Ok(outcome) => tracing::debug!(
                    written = outcome.report.written,
                    skipped = outcome.report.skipped,
                    interrupted = outcome.report.interrupted,
                    "documents export ended"
                ),
                Err(error) => tracing::warn!(?error, "documents export failed"),
            }
            Message::Bases(BasesMessage::List(ListMessage::GamesExported {
                request,
                result,
            }))
        }
        Effect::RenderImage { request, job } => {
            let result = tokio::task::spawn_blocking(move || {
                if job.dest.is_none() {
                    crate::export_docs::preview_png(&job).map(crate::ImageOutcome::Preview)
                } else {
                    crate::export_docs::image_run(&job)
                }
            })
            .await
            .unwrap_or(Err(crate::ImageFailure::Internal));
            if let Err(error) = &result {
                tracing::warn!(?error, "board image failed");
            }
            Message::ExportDocs(crate::ExportDocsMessage::ImageRendered { request, result })
        }
        Effect::OpenExported { path } => {
            let ok = tokio::task::spawn_blocking(move || crate::online::open_path(&path))
                .await
                .unwrap_or(false);
            Message::ExportDocs(crate::ExportDocsMessage::FileOpened { ok })
        }
        Effect::EcoNovelty {
            request,
            job,
            control,
        } => {
            let result = blocking(
                move || crate::eco_ui::novelty_run(&job, &control),
                || Err(BaseError::Internal),
            )
            .await;
            Message::Bases(BasesMessage::List(ListMessage::EcoNovelty {
                request,
                result,
            }))
        }
        Effect::ToggleDeleted {
            request,
            path,
            numbers,
            expected_total,
            stamp,
            control,
        } => {
            let result = blocking(
                move || toggle_deleted(&path, &numbers, expected_total, stamp, &control),
                || Err(BaseError::Internal),
            )
            .await;
            Message::Bases(BasesMessage::List(ListMessage::Flagged { request, result }))
        }
    }
}

fn run_scan(source: PgnSource) -> Result<ScanOutcome, FileError> {
    let file = match &source {
        PgnSource::File(path) => Some(path.clone()),
        PgnSource::Text(_) => None,
    };
    let (load, notices) = scan_pgn(source, &ScanLimits::default(), &ReadOptions::default())?;
    Ok(ScanOutcome {
        load,
        notices,
        path: file.map(|p| canonical_or_absolute(&p)),
    })
}

fn parse_collection_game(
    collection: &PgnCollection,
    index: usize,
) -> Result<crate::files::LoadedGame, FileError> {
    let path = collection.path().map(Path::to_path_buf);
    let parsed = collection
        .parse_game(index, &ReadOptions::default())
        .ok_or_else(|| FileError::new(FileErrorKind::NotFound, path))?;
    Ok(crate::files::LoadedGame {
        number: parsed.number,
        report: ImportReport {
            mode: ImportMode::FullGame,
            notices: parsed
                .diagnostics
                .iter()
                .map(ImportNotice::from_diagnostic)
                .collect(),
        },
        game: SharedGame::new(parsed.game),
    })
}

/// Parses a game of a collection, for the tests of the controller.
#[cfg(test)]
pub(crate) fn perform_parse_for_tests(
    collection: &PgnCollection,
    index: usize,
) -> crate::files::LoadedGame {
    parse_collection_game(collection, index).expect("game")
}

fn parse_moves(text: &str, start: &Position) -> MovetextOutcome {
    let parsed = parse_movetext(text, start, &ReadOptions::default());
    let notices = parsed
        .diagnostics
        .iter()
        .filter(|d| {
            !matches!(
                d.kind,
                DiagnosticKind::MissingResult | DiagnosticKind::HeaderOnlyAtEof
            )
        })
        .map(ImportNotice::from_diagnostic)
        .collect();
    MovetextOutcome {
        game: SharedGame::new(parsed.game),
        report: ImportReport {
            mode: ImportMode::MovesOnly,
            notices,
        },
    }
}

fn load_eco(path: &Path) -> Result<SharedEco, FileError> {
    match read_eco_file(path) {
        Ok(report) => Ok(SharedEco::new(report.book)),
        Err(EcoFileError::Io { source, .. }) => {
            Err(FileError::from_io(&source, Some(path.to_path_buf())))
        }
        Err(EcoFileError::TooLarge { .. }) => Err(FileError::new(
            FileErrorKind::TooLarge { limit: 16 << 20 },
            Some(path.to_path_buf()),
        )),
        Err(_) => Err(FileError::new(
            FileErrorKind::Io(std::io::ErrorKind::InvalidData),
            Some(path.to_path_buf()),
        )),
    }
}

/// Today's date in UTC (D136).
pub(crate) fn today_utc() -> PartialDate {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok());
    seconds.map_or(PartialDate::UNKNOWN, date_from_unix_seconds)
}

/// The UTC calendar date of a Unix time (days-from-civil algorithm of
/// Howard Hinnant, valid for the whole range of `i64` seconds we accept).
pub(crate) fn date_from_unix_seconds(seconds: i64) -> PartialDate {
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let part = |value: i64| u8::try_from(value).ok();
    PartialDate::new(u16::try_from(year).ok(), part(month), part(day))
        .unwrap_or(PartialDate::UNKNOWN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::PgnLoad;

    #[tokio::test(start_paused = true)]
    async fn autoplay_tick_effect_yields_tick_message_with_generation() {
        let effect = Effect::ScheduleAutoplayTick {
            generation: 7,
            delay: Duration::from_secs(5),
        };
        let message = perform(effect).await;
        assert_eq!(
            message,
            Message::Game(GameMessage::AutoplayTick { generation: 7 })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn mode_tick_effect_yields_tick_message_with_run_and_generation() {
        let effect = Effect::ScheduleModeTick {
            run: RunId(3),
            generation: 9,
            delay: Duration::from_secs(1),
        };
        let message = perform(effect).await;
        assert_eq!(
            message,
            Message::Mode(crate::training::ModeMessage::ModeTick {
                run: RunId(3),
                generation: 9
            })
        );
    }

    #[tokio::test]
    async fn host_effects_are_not_handled_by_perform() {
        let host = [
            Effect::ReadClipboard(ClipboardPurpose::Fen),
            Effect::WriteClipboard("x".to_owned()),
            Effect::Exit,
        ];
        for effect in host {
            assert!(effect.runs_on_host());
            assert_eq!(perform(effect).await, Message::HostEffectNotHandled);
        }
        assert!(
            !Effect::QueryToday {
                request: RequestId(1)
            }
            .runs_on_host()
        );
    }

    #[tokio::test]
    async fn scanning_text_answers_with_the_request() {
        let message = perform(Effect::ScanPgn {
            request: RequestId(3),
            source: PgnSource::Text("1. e4 e5".to_owned()),
        })
        .await;
        let Message::PgnScanned {
            request,
            path,
            result: Ok(outcome),
        } = message
        else {
            panic!("expected a scan answer");
        };
        assert_eq!(request, RequestId(3));
        assert_eq!(path, None);
        assert!(matches!(outcome.load, PgnLoad::Single(_)));
    }

    #[tokio::test]
    async fn preferences_round_trip_through_the_effects() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cfg").join("preferences.toml");
        let mut prefs = Preferences::default();
        prefs.board.theme = "wood".to_owned();
        let saved = perform(Effect::SavePreferences {
            path: path.clone(),
            prefs: Box::new(prefs.clone()),
        })
        .await;
        assert_eq!(saved, Message::PreferencesSaved(Ok(())));
        let Message::PreferencesLoaded(loaded) = perform(Effect::LoadPreferences { path }).await
        else {
            panic!("expected loaded preferences");
        };
        assert_eq!(loaded.prefs, prefs);
    }

    fn save_effect(path: PathBuf, guard: SaveGuard) -> Effect {
        Effect::SavePgn {
            request: RequestId(1),
            path,
            mode: SaveMode::Overwrite,
            game: SharedGame::new(mychess_core::Game::new()),
            options: PgnExportPrefs::default(),
            guard,
        }
    }

    fn save_error(message: Message) -> FileErrorKind {
        let Message::PgnSaved {
            result: Err(error), ..
        } = message
        else {
            panic!("expected a failed save: {message:?}");
        };
        error.kind
    }

    #[tokio::test]
    async fn create_only_refuses_an_existing_target_and_keeps_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("x.pgn");
        std::fs::write(&path, "old").expect("write");
        let guard = SaveGuard {
            create_only: true,
            protect: None,
        };
        let message = perform(save_effect(path.clone(), guard.clone())).await;
        assert_eq!(save_error(message), FileErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).expect("read"), b"old");
        let fresh = dir.path().join("fresh.pgn");
        let message = perform(save_effect(fresh.clone(), guard)).await;
        assert!(matches!(message, Message::PgnSaved { result: Ok(_), .. }));
        assert!(fresh.exists());
    }

    #[tokio::test]
    async fn protect_refuses_the_open_file_under_any_spelling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let open = dir.path().join("games.pgn");
        std::fs::write(&open, "old").expect("write");
        std::fs::create_dir(dir.path().join("sub")).expect("dir");
        let spelled = dir.path().join("sub").join("..").join("games.pgn");
        let guard = SaveGuard {
            create_only: false,
            protect: Some(open.clone()),
        };
        let message = perform(save_effect(spelled, guard.clone())).await;
        assert_eq!(save_error(message), FileErrorKind::NeedsConfirmation);
        assert_eq!(std::fs::read(&open).expect("read"), b"old");
        // Another file is fine.
        let other = dir.path().join("other.pgn");
        let message = perform(save_effect(other, guard)).await;
        assert!(matches!(message, Message::PgnSaved { result: Ok(_), .. }));
    }

    #[tokio::test]
    async fn today_is_a_plausible_date() {
        let Message::Today(request, date) = perform(Effect::QueryToday {
            request: RequestId(9),
        })
        .await
        else {
            panic!("expected a date");
        };
        assert_eq!(request, RequestId(9));
        assert!(date.year().is_some_and(|y| y >= 2024));
    }

    #[test]
    fn unix_time_converts_to_a_utc_date() {
        let ymd = |s: i64| {
            let d = date_from_unix_seconds(s);
            (d.year(), d.month(), d.day())
        };
        assert_eq!(ymd(0), (Some(1970), Some(1), Some(1)));
        assert_eq!(ymd(86_399), (Some(1970), Some(1), Some(1)));
        assert_eq!(ymd(951_782_400), (Some(2000), Some(2), Some(29)));
        assert_eq!(ymd(1_709_164_800), (Some(2024), Some(2), Some(29)));
        assert_eq!(ymd(1_709_251_199), (Some(2024), Some(2), Some(29)));
        assert_eq!(ymd(1_709_251_200), (Some(2024), Some(3), Some(1)));
        assert_eq!(ymd(-1), (Some(1969), Some(12), Some(31)));
    }
}
