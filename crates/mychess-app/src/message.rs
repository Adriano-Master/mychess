//! The top-level [`Message`]: every event the front-end can send to
//! [`update`](crate::update).

use std::path::PathBuf;
use std::time::Instant;

use mychess_core::PartialDate;

use crate::advanced_search::AdvancedSearchMessage;
use crate::analysis::AnalysisMessage;
use crate::annotate::AnnotateMessage;
use crate::bases::BasesMessage;
use crate::bookmarks::BookmarksMessage;
use crate::chooser::ChooserMessage;
use crate::clipboard::{ClipboardError, ClipboardPurpose};
use crate::engines::EngineMessage;
use crate::error::{FileError, RequestId};
use crate::files::{LoadedGame, SaveMode, ScanOutcome};
use crate::flow::{Command, DiscardChoice, MovetextOutcome};
use crate::game_info::GameInfoMessage;
use crate::import::ImportMessage;
use crate::nag_shortcuts::NagKey;
use crate::prefs::{LoadedPreferences, PrefsMessage, WindowGeometry};
use crate::session::GameMessage;
use crate::setup::SetupMessage;
use crate::shared::SharedEco;
use crate::shortcuts::Action;

/// An event for the application state.
///
/// One variant per feature slice, plus the answers to effects. The answers
/// carry the [`RequestId`] of the effect (an answer to an operation that is
/// no longer current is dropped).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Message {
    /// Editing the game: navigation, moves, annotation, undo
    /// (see [`GameMessage`]).
    Game(GameMessage),
    /// A command from a menu or button: new, open, save, copy, paste...
    Command(Command),
    /// A shortcut was pressed (see [`Shortcuts`](crate::Shortcuts)).
    Action(Action),
    /// An edit of the "set up position" dialog.
    Setup(SetupMessage),
    /// An event of the PGN import dialog.
    Import(ImportMessage),
    /// An event of the game information dialog.
    Info(GameInfoMessage),
    /// An event of the game chooser.
    Chooser(ChooserMessage),
    /// An event of the database slots and the Clipbase (slice M3-4).
    Bases(BasesMessage),
    /// An event of the bookmarks (slice M3-6).
    Bookmarks(BookmarksMessage),
    /// An event of the engine management (spec M4-3).
    Engine(EngineMessage),
    /// An event of the analysis window (spec M4-4).
    Analysis(AnalysisMessage),
    /// A command of the evaluation bar, the arrows and the traffic lights
    /// (spec M4-5).
    EvalBar(crate::evalbar::EvalBarMessage),
    /// An event of the automatic annotation (spec M4-6).
    Annotate(AnnotateMessage),
    /// An event of the game report (spec M4-10).
    Report(crate::report::ReportMessage),
    /// An event of the score and time graph (spec M4-7).
    Graph(crate::graphs::GraphMessage),
    /// An event of the opening tree panel (spec M5-2).
    Tree(crate::tree::TreeMessage),
    /// An event of the import of PGN files (spec M3-8).
    FileImport(crate::file_import::FileImportMessage),
    /// An event of the export of games (spec M3-9).
    GamesExport(crate::games_export::GamesExportMessage),
    /// An event of the extra export formats and of the board image (spec
    /// M7-4).
    ExportDocs(crate::export_docs::ExportDocsMessage),
    /// An event of the searches by position, material and moves (spec
    /// M6-1).
    AdvancedSearch(AdvancedSearchMessage),
    /// An event of the search for tactics and patterns (spec M6-2).
    PatternSearch(crate::pattern_search::PatternSearchMessage),
    /// An event of the ECO tools: browser, classification, "identify
    /// opening" and "find novelty" (spec M5-7b).
    Eco(crate::eco_ui::EcoMessage),
    /// An event of the book window (spec M5-6).
    Book(crate::book::BookMessage),
    /// An event of the game against an engine (spec M8-1).
    Play(crate::play::PlayMessage),
    /// An event of the tournament between engines (spec M8-2).
    Tournament(crate::tournament::TournamentMessage),
    /// An event of the online play on Lichess (spec M8-3).
    Online(crate::online::OnlineMessage),
    /// An event of the play on FICS (spec M8-9).
    Fics(crate::fics::FicsMessage),
    /// An event of the engine modes: threat, shoot-out, automove (spec
    /// M4-8).
    Mode(crate::training::ModeMessage),
    /// An event of the tactics training (spec M8-4).
    Tactics(crate::tactics::TacticsMessage),
    /// An event of the calculation exercise (spec M8-6).
    Calvar(crate::calvar::CalvarMessage),
    /// An event of the tablebase panel and of the endgame training (spec
    /// M8-8).
    Tablebase(crate::tablebase::TablebaseMessage),
    /// An event of the electronic board (spec M8-10).
    Eboard(crate::eboard::EboardMessage),
    /// An event of the sounds and of their window (spec M8-11).
    Sound(crate::sound::SoundMessage),
    /// An event of the crosstable panel (spec M6-4).
    Crosstable(crate::crosstable::CrosstableMessage),
    /// An event of the statistics panel or of the rating graph (spec M6-4).
    Charts(crate::charts::ChartsMessage),
    /// An event of the piece tracker (spec M6-4).
    PieceTracker(crate::piece_tracker::PieceTrackerMessage),
    /// An event of the opening trainer (spec M8-5).
    OpeningTrainer(crate::opening_trainer::OpeningMessage),
    /// An event of the spelling file, the name checker and "Add Elo" (spec
    /// M6-3).
    Spelling(crate::spelling_ui::SpellingMessage),
    /// An event of the player finder or the tournament finder (spec M6-3).
    Finder(crate::finder_ui::FinderMessage),
    /// An event of the merge of a game into the current one (spec M7-6).
    Merge(crate::merge_game::MergeMessage),
    /// An event of the EPD window (spec M7-5).
    Epd(crate::epd_window::EpdMessage),
    /// An event of the opening report (spec M7-2).
    OpeningReport(crate::opening_report::OpeningReportMessage),
    /// An event of the start-up: launch files, the first-run welcome and the
    /// tip of the day (spec M9-4).
    Start(crate::start::StartMessage),
    /// An event of the manual window (spec M9-5).
    Manual(crate::manual::ManualMessage),
    /// An event of the player report (spec M7-3).
    PlayerReport(crate::player_report::PlayerReportMessage),
    /// An event of the maintenance tools (spec M7-1).
    Maintenance(crate::maintenance::MaintenanceMessage),
    /// An event of the player information window (spec M6-3).
    PlayerInfo(crate::player_info_ui::PlayerInfoMessage),
    /// A change of the preferences.
    Prefs(PrefsMessage),
    /// A key that may belong to a NAG shortcut (`!` Enter, `+-`...), typed
    /// at `at`. Sent for the characters and Enter typed with the board
    /// focused and no text field focused; the keys that are not part of a
    /// sequence are replayed as move entry (`--` is the null move).
    NagKey {
        /// The key.
        key: NagKey,
        /// When it was typed (the front-end's clock).
        at: Instant,
    },
    /// The timer of a half-typed NAG sequence fired (see
    /// [`AppState::nag_sequence_deadline`](crate::AppState::nag_sequence_deadline)).
    NagTick(Instant),
    /// A key that is not a character (arrows, Escape...) was pressed: a
    /// half-typed NAG sequence is dropped and its keys replayed.
    NagInterrupt,
    /// The answer to the "discard changes?" question.
    AnswerDiscard(DiscardChoice),
    /// The answer to the "overwrite a file with several games?" question.
    AnswerOverwrite(bool),

    /// The program started: loads the preferences.
    Startup,
    /// The user asked to quit. Preferences are saved first; the host
    /// receives [`Effect::Exit`](crate::Effect::Exit) when it is safe.
    Quit,
    /// Saves the preferences if they changed (the front-end sends it two
    /// seconds after the last change of window geometry, and on close).
    FlushPreferences,
    /// The window was moved or resized. Only marks the preferences as
    /// changed.
    WindowGeometryChanged(WindowGeometry),

    // ---- answers to host effects ----
    /// The file dialog finished (`None`: cancelled).
    FilePicked(RequestId, Option<PathBuf>),
    /// The dialog that picks several files finished (an empty list:
    /// cancelled; spec M3-8).
    FilesPicked(RequestId, Vec<PathBuf>),
    /// The clipboard was read.
    ClipboardRead {
        /// Why it was read.
        purpose: ClipboardPurpose,
        /// The text, or why there is none.
        result: Result<String, ClipboardError>,
    },
    /// Writing to the clipboard failed.
    ClipboardWriteFailed,
    /// [`perform`](crate::perform) was given an effect that the front-end
    /// must run.
    HostEffectNotHandled,

    // ---- answers to effects run by `perform` ----
    /// A PGN scan finished.
    PgnScanned {
        /// The request.
        request: RequestId,
        /// The file asked for, `None` for pasted text.
        path: Option<PathBuf>,
        /// What was found.
        result: Result<ScanOutcome, FileError>,
    },
    /// A game of a collection was parsed.
    CollectionGameParsed {
        /// The request.
        request: RequestId,
        /// The game.
        result: Result<LoadedGame, FileError>,
    },
    /// A PGN file was written.
    PgnSaved {
        /// The request.
        request: RequestId,
        /// How the game was written.
        mode: SaveMode,
        /// The canonical path of the file, or the failure.
        result: Result<PathBuf, FileError>,
    },
    /// Pasted moves were parsed.
    MovetextParsed {
        /// The request.
        request: RequestId,
        /// The result.
        outcome: Box<MovetextOutcome>,
    },
    /// The preferences were read.
    PreferencesLoaded(LoadedPreferences),
    /// The preferences were written.
    PreferencesSaved(Result<(), FileError>),
    /// An opening book was read.
    EcoLoaded {
        /// The file asked for.
        path: PathBuf,
        /// The book, or the failure.
        result: Result<SharedEco, FileError>,
    },
    /// Today's date (UTC).
    Today(RequestId, PartialDate),
}

impl From<crate::pattern_search::PatternSearchMessage> for Message {
    fn from(message: crate::pattern_search::PatternSearchMessage) -> Message {
        Message::PatternSearch(message)
    }
}

impl From<AdvancedSearchMessage> for Message {
    fn from(message: AdvancedSearchMessage) -> Message {
        Message::AdvancedSearch(message)
    }
}

impl From<GameMessage> for Message {
    fn from(message: GameMessage) -> Message {
        Message::Game(message)
    }
}

impl From<Command> for Message {
    fn from(command: Command) -> Message {
        Message::Command(command)
    }
}

impl From<Action> for Message {
    fn from(action: Action) -> Message {
        Message::Action(action)
    }
}

impl From<SetupMessage> for Message {
    fn from(message: SetupMessage) -> Message {
        Message::Setup(message)
    }
}

impl From<ImportMessage> for Message {
    fn from(message: ImportMessage) -> Message {
        Message::Import(message)
    }
}

impl From<GameInfoMessage> for Message {
    fn from(message: GameInfoMessage) -> Message {
        Message::Info(message)
    }
}

impl From<ChooserMessage> for Message {
    fn from(message: ChooserMessage) -> Message {
        Message::Chooser(message)
    }
}

impl From<PrefsMessage> for Message {
    fn from(message: PrefsMessage) -> Message {
        Message::Prefs(message)
    }
}

impl From<BasesMessage> for Message {
    fn from(message: BasesMessage) -> Message {
        Message::Bases(message)
    }
}

impl From<BookmarksMessage> for Message {
    fn from(message: BookmarksMessage) -> Message {
        Message::Bookmarks(message)
    }
}

impl From<AnalysisMessage> for Message {
    fn from(message: AnalysisMessage) -> Message {
        Message::Analysis(message)
    }
}

impl From<EngineMessage> for Message {
    fn from(message: EngineMessage) -> Message {
        Message::Engine(message)
    }
}

impl From<crate::graphs::GraphMessage> for Message {
    fn from(message: crate::graphs::GraphMessage) -> Message {
        Message::Graph(message)
    }
}

impl From<crate::tree::TreeMessage> for Message {
    fn from(message: crate::tree::TreeMessage) -> Message {
        Message::Tree(message)
    }
}

impl From<crate::eco_ui::EcoMessage> for Message {
    fn from(message: crate::eco_ui::EcoMessage) -> Message {
        Message::Eco(message)
    }
}

impl From<crate::file_import::FileImportMessage> for Message {
    fn from(message: crate::file_import::FileImportMessage) -> Message {
        Message::FileImport(message)
    }
}

impl From<crate::export_docs::ExportDocsMessage> for Message {
    fn from(message: crate::export_docs::ExportDocsMessage) -> Message {
        Message::ExportDocs(message)
    }
}

impl From<crate::games_export::GamesExportMessage> for Message {
    fn from(message: crate::games_export::GamesExportMessage) -> Message {
        Message::GamesExport(message)
    }
}

impl From<crate::report::ReportMessage> for Message {
    fn from(message: crate::report::ReportMessage) -> Message {
        Message::Report(message)
    }
}

impl From<AnnotateMessage> for Message {
    fn from(message: AnnotateMessage) -> Message {
        Message::Annotate(message)
    }
}

impl From<crate::tournament::TournamentMessage> for Message {
    fn from(message: crate::tournament::TournamentMessage) -> Message {
        Message::Tournament(message)
    }
}

impl From<crate::fics::FicsMessage> for Message {
    fn from(message: crate::fics::FicsMessage) -> Message {
        Message::Fics(message)
    }
}

impl From<crate::online::OnlineMessage> for Message {
    fn from(message: crate::online::OnlineMessage) -> Message {
        Message::Online(message)
    }
}

impl From<crate::book::BookMessage> for Message {
    fn from(message: crate::book::BookMessage) -> Message {
        Message::Book(message)
    }
}

impl From<crate::opening_trainer::OpeningMessage> for Message {
    fn from(message: crate::opening_trainer::OpeningMessage) -> Message {
        Message::OpeningTrainer(message)
    }
}

impl From<crate::sound::SoundMessage> for Message {
    fn from(message: crate::sound::SoundMessage) -> Message {
        Message::Sound(message)
    }
}

impl From<crate::eboard::EboardMessage> for Message {
    fn from(message: crate::eboard::EboardMessage) -> Message {
        Message::Eboard(message)
    }
}

impl From<crate::tablebase::TablebaseMessage> for Message {
    fn from(message: crate::tablebase::TablebaseMessage) -> Message {
        Message::Tablebase(message)
    }
}

impl From<crate::finder_ui::FinderMessage> for Message {
    fn from(message: crate::finder_ui::FinderMessage) -> Message {
        Message::Finder(message)
    }
}

impl From<crate::merge_game::MergeMessage> for Message {
    fn from(message: crate::merge_game::MergeMessage) -> Message {
        Message::Merge(message)
    }
}

impl From<crate::player_info_ui::PlayerInfoMessage> for Message {
    fn from(message: crate::player_info_ui::PlayerInfoMessage) -> Message {
        Message::PlayerInfo(message)
    }
}

impl From<crate::spelling_ui::SpellingMessage> for Message {
    fn from(message: crate::spelling_ui::SpellingMessage) -> Message {
        Message::Spelling(message)
    }
}

impl From<crate::crosstable::CrosstableMessage> for Message {
    fn from(message: crate::crosstable::CrosstableMessage) -> Message {
        Message::Crosstable(message)
    }
}

impl From<crate::charts::ChartsMessage> for Message {
    fn from(message: crate::charts::ChartsMessage) -> Message {
        Message::Charts(message)
    }
}

impl From<crate::piece_tracker::PieceTrackerMessage> for Message {
    fn from(message: crate::piece_tracker::PieceTrackerMessage) -> Message {
        Message::PieceTracker(message)
    }
}

impl From<crate::calvar::CalvarMessage> for Message {
    fn from(message: crate::calvar::CalvarMessage) -> Message {
        Message::Calvar(message)
    }
}

impl From<crate::tactics::TacticsMessage> for Message {
    fn from(message: crate::tactics::TacticsMessage) -> Message {
        Message::Tactics(message)
    }
}

impl From<crate::play::PlayMessage> for Message {
    fn from(message: crate::play::PlayMessage) -> Message {
        Message::Play(message)
    }
}

impl From<crate::maintenance::MaintenanceMessage> for Message {
    fn from(message: crate::maintenance::MaintenanceMessage) -> Message {
        Message::Maintenance(message)
    }
}

impl From<crate::player_report::PlayerReportMessage> for Message {
    fn from(message: crate::player_report::PlayerReportMessage) -> Message {
        Message::PlayerReport(message)
    }
}

impl From<crate::opening_report::OpeningReportMessage> for Message {
    fn from(message: crate::opening_report::OpeningReportMessage) -> Message {
        Message::OpeningReport(message)
    }
}
