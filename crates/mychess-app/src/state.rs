//! [`AppState`] and the pure [`update`] function.
//!
//! `update` never touches the file system, the clipboard or the clock: it
//! describes what to do as [`Effect`]s (rule 65 of the M2-2 spec). The
//! controller code is split by concern in the `state/` modules.

mod actions;
mod advanced_search_flow;
mod analysis_flow;
#[cfg(test)]
mod analysis_tests;
mod annotate_flow;
#[cfg(test)]
mod annotate_tests;
mod bases_flow;
mod book_flow;
#[cfg(test)]
mod book_tests;
mod bookmarks_flow;
mod browser_flow;
mod charts_flow;
mod crosstable_flow;
mod dialogs;
mod eboard_flow;
#[cfg(test)]
mod eboard_tests;
mod eco_flow;
#[cfg(test)]
mod eco_tests;
mod engines_flow;
mod epd_flow;
#[cfg(test)]
mod epd_tests;
#[cfg(test)]
mod epd_view_tests;
mod evalbar_flow;
#[cfg(test)]
mod evalbar_tests;
mod export_docs_flow;
mod fics_flow;
#[cfg(test)]
mod fics_sound_tests;
mod file_import_flow;
mod files_flow;
mod finder_flow;
#[cfg(test)]
mod flow_tests;
mod game_commands;
mod games_export_flow;
mod graph_flow;
mod list_flow;
mod maintenance_flow;
mod manual_flow;
mod merge_flow;
mod nag_keys;
mod notation_cache;
mod online_flow;
#[cfg(test)]
mod online_tests;
mod opening_report_flow;
mod pattern_search_flow;
mod piece_tracker_flow;
mod play_flow;
#[cfg(test)]
mod play_tests;
mod player_info_flow;
mod player_report_flow;
mod prefs_flow;
mod replace;
mod report_flow;
#[cfg(test)]
mod report_tests;
mod sound_flow;
mod spelling_flow;
mod start_flow;
mod strip_cache;
mod tablebase_flow;
mod tournament_dest;
mod tournament_flow;
#[cfg(test)]
mod tournament_tests;
mod training_flow;
#[cfg(test)]
mod training_tests;
mod tree_cache_flow;
mod tree_flow;
mod tree_mask_flow;
mod tree_train_flow;

pub use game_commands::GameMode;
use std::path::PathBuf;
use std::sync::Arc;

use mychess_core::Tags;

use crate::bases::{Bases, OpenControl, SlotId};
use crate::chooser::GameChooser;
use crate::effect::Effect;
use crate::error::RequestId;
use crate::files::{GameOrigin, PgnCollection};
use crate::flow::{
    AppNotice, DiscardPrompt, MovetextOutcome, OverwritePrompt, OverwriteReason, Pending,
};
use crate::game_info::{GameInfoDialog, GameInfoView, NameBook, game_info};
use crate::host::PickKind;
use crate::import::{ImportDialog, ImportReport};
use crate::message::Message;
use crate::prefs::{Preferences, PrefsIssue};
use crate::session::{GameMessage, GameSession};
use crate::setup::SetupBoard;
use crate::shared::SharedEco;
use crate::shortcuts::Shortcuts;

/// A file dialog waiting for the user.
#[derive(Debug, Clone)]
pub(crate) struct PendingPick {
    pub(crate) request: RequestId,
    pub(crate) kind: PickKind,
    /// What to do after the file was saved.
    pub(crate) then: Option<Pending>,
}

/// The open file that a save must not replace without asking, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Protected {
    pub(crate) path: PathBuf,
    pub(crate) games: usize,
    pub(crate) reason: OverwriteReason,
}

/// What `perform` must check before it writes (see [`crate::SaveGuard`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SaveCheck {
    /// The name was altered after the user confirmed it: refuse to replace.
    pub(crate) create_only: bool,
    pub(crate) protected: Option<Protected>,
}

/// A save in flight.
#[derive(Debug, Clone)]
pub(crate) struct PendingSave {
    pub(crate) request: RequestId,
    pub(crate) revision: crate::session::Revision,
    /// What to do after the file was saved.
    pub(crate) then: Option<Pending>,
    pub(crate) path: PathBuf,
    pub(crate) check: SaveCheck,
}

/// A game of a collection being parsed.
#[derive(Debug, Clone)]
pub(crate) struct PendingParse {
    pub(crate) request: RequestId,
    pub(crate) collection: Arc<PgnCollection>,
    pub(crate) index: usize,
}

/// A database being opened (slice M3-4).
#[derive(Debug, Clone)]
pub(crate) struct PendingOpen {
    pub(crate) request: RequestId,
    pub(crate) path: PathBuf,
    pub(crate) control: OpenControl,
}

/// A search, sort, copy or flag change of the list in flight (slice 4c).
#[derive(Debug, Clone)]
pub(crate) struct PendingListJob {
    pub(crate) request: RequestId,
    pub(crate) slot: SlotId,
    /// The database the job works on; the answer is dropped if the slot no
    /// longer holds it.
    pub(crate) source: crate::bases::SharedSource,
    pub(crate) kind: crate::bases::ListJobKind,
    /// The slot a copy writes to and the database it held when the copy
    /// started (slice 4d).
    pub(crate) target: Option<(SlotId, crate::bases::SharedSource)>,
    /// The file a copy writes to, kept for the time the job outlives the
    /// closing of its slots (D588).
    pub(crate) target_path: Option<PathBuf>,
    /// A slot the job works on was closed: the job was cancelled but its
    /// thread is still winding down, so the job stays (and keeps the
    /// databases busy) until its answer arrives (D588).
    pub(crate) closing: bool,
    pub(crate) control: OpenControl,
}

/// A game of a slot being read for the editor (slice M3-4).
#[derive(Debug, Clone)]
pub(crate) struct PendingBaseGame {
    pub(crate) request: RequestId,
    pub(crate) slot: SlotId,
    pub(crate) number: u32,
    /// Set when the browser asked for the game (slice 4e).
    pub(crate) landing: Option<crate::flow::BrowserLanding>,
    /// The game is read again only to show corrected names (D3472): the
    /// answer is dropped if the editor's game was altered meanwhile.
    pub(crate) names_refresh: bool,
}

/// Bookkeeping of the asynchronous flows (not shown by the front-end).
#[derive(Debug, Default)]
pub(crate) struct Flow {
    pub(crate) next_request: u64,
    /// Source of the autoplay generations of the browser: unique across
    /// browsers, so a late tick of a closed one never matches (D608).
    pub(crate) next_browser_generation: u64,
    pub(crate) prefs_path: Option<PathBuf>,
    pub(crate) pick: Option<PendingPick>,
    pub(crate) scan: Option<RequestId>,
    pub(crate) parse: Option<PendingParse>,
    pub(crate) open_base: Option<PendingOpen>,
    pub(crate) base_game: Option<PendingBaseGame>,
    /// The list job in flight, if any (one at a time).
    pub(crate) list_job: Option<PendingListJob>,
    /// A quit is waiting for the list job to answer (D589).
    pub(crate) quit_waiting_job: bool,
    /// A quit is waiting for the step of a cancelled annotation batch
    /// that is in flight (D1550).
    pub(crate) quit_waiting_batch: bool,
    /// Slots whose altered game the user already answered "don't save"
    /// in the quit in progress (D549).
    pub(crate) quit_asked: Vec<SlotId>,
    pub(crate) save: Option<PendingSave>,
    pub(crate) movetext: Option<RequestId>,
    pub(crate) prefs_save_in_flight: bool,
    pub(crate) needs_resave: bool,
    pub(crate) exit_when_saved: bool,
    pub(crate) warned_no_config_dir: bool,
    /// The preferences file was asked for and has not been read yet.
    pub(crate) prefs_loading: bool,
    /// A save of the preferences was asked while they were loading.
    pub(crate) save_after_load: bool,
    /// Changes made while the preferences were loading; applied over the
    /// loaded values.
    pub(crate) early_changes: Vec<crate::prefs::PrefChange>,
    /// The window geometry reported while the preferences were loading.
    pub(crate) early_geometry: Option<crate::prefs::WindowGeometry>,
    /// The bookmarks file was asked for and has not been read yet.
    pub(crate) bookmarks_loading: bool,
    /// Bookmarks added while the file was being read; added again over the
    /// loaded list (D677).
    pub(crate) bookmarks_added_early: Vec<crate::bookmarks::GameBookmark>,
    pub(crate) bookmarks_save_in_flight: bool,
    pub(crate) bookmarks_resave: bool,
    /// A quit waits for the bookmarks save in flight.
    pub(crate) quit_waiting_bookmarks: bool,
    /// A bookmark to go to once its database is open (D675).
    pub(crate) bookmark_go: Option<crate::bookmarks::GameBookmark>,
    /// The search for the game of a bookmark in flight.
    pub(crate) bookmark_find: Option<PendingBookmarkFind>,
    /// The engine management (spec M4-3).
    pub(crate) engines: engines_flow::EnginesFlow,
    /// The evaluation bar, arrows and traffic lights (spec M4-5).
    pub(crate) evalbar: evalbar_flow::EvalBarFlow,
    /// The annotation run in progress, if any (spec M4-6).
    pub(crate) annotate: Option<annotate_flow::Active>,
    /// Searches of an ended annotation run that may still report (D894).
    pub(crate) annotate_stale: Vec<(crate::engines::RunId, mychess_engine::SearchId)>,
    /// An engine whose run ended with a `stop` pending for a search that
    /// had not been labelled: its next unlabelled start is dropped (D900).
    pub(crate) annotate_untagged: Option<crate::engines::RunId>,
    /// The batch over the filter in progress, if any (spec M4-6 phase B).
    pub(crate) annotate_batch: Option<annotate_flow::Batch>,
    /// The threat, shoot-out or automove in progress (spec M4-8).
    pub(crate) training: training_flow::TrainingFlow,
    /// The game against an engine (spec M8-1).
    pub(crate) play: play_flow::PlayFlow,
    /// The tournament between engines (spec M8-2).
    pub(crate) tournament: Box<tournament_flow::TournamentFlow>,
    /// The game on Lichess and the editor game it replaced (spec M8-3).
    pub(crate) online: online_flow::OnlineFlow,
    /// The game on FICS and the editor game it replaced (spec M8-9).
    pub(crate) fics: fics_flow::FicsFlow,
    /// The tactics training (spec M8-4).
    pub(crate) tactics: crate::tactics::TacticsFlow,
    /// The opening trainer (spec M8-5).
    pub(crate) opening: crate::opening_trainer::OpeningFlow,
    /// The calculation exercise (spec M8-6).
    pub(crate) calvar: crate::calvar::CalvarFlow,
    /// The tablebase panel and the endgame training (spec M8-8).
    pub(crate) tablebase: tablebase_flow::TablebaseFlow,
    /// The electronic board (spec M8-10).
    pub(crate) eboard: crate::eboard::EboardState,
    /// The sounds and their window (spec M8-11).
    pub(crate) sound: sound_flow::SoundFlow,
}

/// The search for the best game of a bookmark (D674).
#[derive(Debug, Clone)]
pub(crate) struct PendingBookmarkFind {
    pub(crate) request: RequestId,
    pub(crate) slot: SlotId,
    pub(crate) bookmark: crate::bookmarks::GameBookmark,
}

/// The whole application state (the MVU model).
///
/// Each feature slice owns fields. They are public so the front-end can
/// read them in `view`; only [`update`] mutates them.
#[derive(Debug)]
pub struct AppState {
    /// The game being edited, with cursor, history and interaction state.
    pub session: GameSession,
    /// Where the game came from (title bar, "Save").
    pub origin: GameOrigin,
    /// Whether the board is turned (visual).
    pub board_flipped: bool,
    /// The user's preferences.
    pub prefs: Preferences,
    /// Remarks about the preferences file and the shortcuts.
    pub prefs_issues: Vec<PrefsIssue>,
    /// The effective shortcut map (defaults plus the customisations).
    pub shortcuts: Shortcuts,
    /// The "set up position" dialog.
    pub setup: Option<SetupBoard>,
    /// FEN texts of the setup dialog, newest first (this session).
    pub setup_history: Vec<String>,
    /// How many online games (Lichess or FICS: played, examined or shown from
    /// the observed list, and watched games) were entered. A game that is
    /// resynchronised does not count: the front-end compares the number to
    /// close its windows once per entry (M8-3 D4520).
    pub online_entries: u32,
    /// The PGN import dialog.
    pub import: Option<ImportDialog>,
    /// The game information dialog.
    pub info_dialog: Option<GameInfoDialog>,
    /// The game chooser (the fallback of D503: with
    /// [`open_pgn_in_slots`](AppState::open_pgn_in_slots) off, a PGN with
    /// several games is shown here).
    pub chooser: Option<GameChooser>,
    /// The eight database slots and the Clipbase (slice M3-4).
    pub bases: Bases,
    /// Whether "Open" and the recent files open the PGN in a slot (D503)
    /// instead of reading it for the chooser. Off by default so the M2
    /// front-end keeps working; the front-end of slice 4b turns it on.
    pub open_pgn_in_slots: bool,
    /// The "save changes?" question.
    pub discard_prompt: Option<DiscardPrompt>,
    /// The "overwrite a file with several games?" question.
    pub overwrite_prompt: Option<OverwritePrompt>,
    /// The "copy N games from X to Y?" question (slice 4d, D580).
    pub copy_prompt: Option<crate::bases::CopyPrompt>,
    /// The bookmarks (spec M3-6).
    pub bookmarks: crate::bookmarks::Bookmarks,
    /// The bookmarks editor, when open.
    pub bookmark_editor: Option<crate::bookmarks::BookmarkEditor>,
    /// The engine list (spec M4-3, D424-D426).
    pub engines: crate::engines::EngineList,
    /// What `engines.toml` had to fix when it was read.
    pub engine_issues: Vec<crate::engines::LoadIssue>,
    /// The "Engines" dialog, when open.
    pub engine_dialog: Option<crate::engines::EngineDialog>,
    /// The engine editor, when open.
    pub engine_editor: Option<crate::engines::EngineEditor>,
    /// The executions of the engines (D433).
    pub engine_runs: crate::engines::EngineRuns,
    /// The live engines and their event channel (D435). The front-end takes
    /// the events once with [`EngineHub::events`](crate::engines::EngineHub::events).
    pub engine_hub: crate::engines::EngineHub,
    /// What the analysis window shows for each execution (spec M4-4).
    pub analysis: crate::analysis::AnalysisViews,
    /// The options of the automatic annotation: the last ones used (spec
    /// M4-6, not saved in the preferences yet).
    pub annotate_options: crate::annotate::AnnotateOptions,
    /// The informant thresholds of the annotation.
    pub annotate_informant: crate::annotate::Informant,
    /// The "Configure annotation" dialog, when open.
    pub annotate_dialog: Option<crate::annotate::AnnotateDialog>,
    /// How the last annotation run ended.
    pub annotate_report: Option<crate::annotate::AnnotateReport>,
    /// The game report panel (spec M4-10b).
    pub report: crate::report::ReportUi,
    /// The score and time graph (spec M4-7).
    pub(crate) graph: crate::graphs::GraphState,
    /// The opening tree panel (spec M5-2).
    pub(crate) tree: crate::tree::TreeUi,
    /// The book window and the opening book (spec M5-6).
    pub(crate) book: crate::book::BookUi,
    /// The ECO browser and the dialogs of the classification and the
    /// novelty (spec M5-7b).
    pub(crate) eco: crate::eco_ui::EcoUi,
    /// The import of PGN files: the file dialogs in flight and the window
    /// of the progress and the report (spec M3-8).
    pub(crate) file_import: crate::file_import::FileImportUi,
    pub(crate) games_export: crate::games_export::GamesExportUi,
    /// The extra export formats and the board image (spec M7-4).
    pub(crate) export_docs: crate::export_docs::ExportDocsUi,
    /// The EPD window: open files, selection, jobs and the analysis (spec
    /// M7-5).
    pub(crate) epd: crate::epd_window::EpdUi,
    /// The opening report (spec M7-2).
    pub(crate) opening_report: crate::opening_report::OpeningReportUi,
    /// The built-in manual window (spec M9-5).
    pub(crate) manual: crate::manual::ManualState,
    /// The start-up: launch files, welcome and tip (spec M9-4).
    pub(crate) start: crate::start::StartState,
    pub(crate) player_report: crate::player_report::PlayerReportUi,
    pub(crate) merge: crate::merge_game::MergeUi,
    /// The window of the searches by position, material and moves (spec
    /// M6-1).
    pub(crate) advanced: crate::advanced_search::AdvancedUi,
    /// The window of the search for tactics and patterns (spec M6-2).
    pub(crate) pattern: crate::pattern_search::PatternUi,
    /// The crosstable panel (spec M6-4).
    pub(crate) crosstable: crate::crosstable::CrosstableUi,
    /// The spelling file, the name checker and "Add Elo" (spec M6-3).
    pub(crate) spelling: crate::spelling_ui::SpellingUi,
    /// The maintenance tools (spec M7-1).
    pub(crate) maintenance: crate::maintenance::MaintenanceUi,
    /// The player finder and the tournament finder (spec M6-3).
    pub(crate) finder: crate::finder_ui::FinderUi,
    /// The player information window (spec M6-3).
    pub(crate) player_info: crate::player_info_ui::PlayerInfoUi,
    /// The statistics panel and the rating graph (spec M6-4).
    pub(crate) charts: crate::charts::ChartsUi,
    /// The piece tracker (spec M6-4).
    pub(crate) tracker: crate::piece_tracker::PieceTrackerUi,
    /// Playing on Lichess (spec M8-3): the session, the game, the clock and
    /// the lock of the assistance.
    pub online: Box<crate::online::OnlineState>,
    /// Playing on FICS (spec M8-9): the session, the games, the seek graph
    /// and the lock of the assistance.
    pub fics: Box<crate::fics::FicsState>,
    /// The game browser, when open (slice 4e).
    pub browser: Option<crate::bases::BrowserState>,
    /// Names seen in this session, for autocompletion.
    pub names: NameBook,
    /// The opening book, if one is loaded.
    pub eco_book: Option<SharedEco>,
    /// The collection the game was taken from (for next/previous game).
    pub collection: Option<Arc<PgnCollection>>,
    /// Tags of the game that was replaced last ("Use previous").
    pub last_tags: Option<Tags>,
    /// The last result of "paste moves", for the tree slice to insert.
    pub pasted_moves: Option<MovetextOutcome>,
    /// The report of the last file opened or text imported.
    pub open_report: Option<ImportReport>,
    /// The last remark for the status bar.
    pub notice: Option<AppNotice>,
    /// The preferences changed and wait for `FlushPreferences`.
    pub prefs_dirty: bool,
    pub(crate) flow: Flow,
    notation_cache: notation_cache::NotationCache,
    strip_cache: strip_cache::StripCache,
    nag_keys: crate::nag_shortcuts::NagShortcutBuffer,
}

impl Default for AppState {
    fn default() -> AppState {
        AppState {
            session: GameSession::default(),
            origin: GameOrigin::New,
            board_flipped: false,
            prefs: Preferences::default(),
            prefs_issues: Vec::new(),
            shortcuts: Shortcuts::defaults(),
            setup: None,
            setup_history: Vec::new(),
            online_entries: 0,
            import: None,
            info_dialog: None,
            chooser: None,
            bases: Bases::default(),
            open_pgn_in_slots: false,
            discard_prompt: None,
            overwrite_prompt: None,
            copy_prompt: None,
            bookmarks: crate::bookmarks::Bookmarks::new(),
            bookmark_editor: None,
            engines: crate::engines::EngineList::new(),
            engine_issues: Vec::new(),
            engine_dialog: None,
            engine_editor: None,
            engine_runs: crate::engines::EngineRuns::default(),
            engine_hub: crate::engines::EngineHub::new(),
            analysis: crate::analysis::AnalysisViews::default(),
            annotate_options: crate::annotate::AnnotateOptions::default(),
            annotate_informant: crate::annotate::Informant::default(),
            annotate_dialog: None,
            annotate_report: None,
            report: crate::report::ReportUi::default(),
            graph: crate::graphs::GraphState::default(),
            tree: crate::tree::TreeUi::default(),
            book: crate::book::BookUi::default(),
            eco: crate::eco_ui::EcoUi::default(),
            file_import: crate::file_import::FileImportUi::default(),
            games_export: crate::games_export::GamesExportUi::default(),
            export_docs: crate::export_docs::ExportDocsUi::default(),
            epd: crate::epd_window::EpdUi::default(),
            opening_report: crate::opening_report::OpeningReportUi::default(),
            manual: crate::manual::ManualState::default(),
            start: crate::start::StartState::default(),
            player_report: crate::player_report::PlayerReportUi::default(),
            merge: crate::merge_game::MergeUi::default(),
            advanced: crate::advanced_search::AdvancedUi::default(),
            pattern: crate::pattern_search::PatternUi::default(),
            crosstable: crate::crosstable::CrosstableUi::default(),
            spelling: crate::spelling_ui::SpellingUi::default(),
            maintenance: crate::maintenance::MaintenanceUi::default(),
            finder: crate::finder_ui::FinderUi::default(),
            player_info: crate::player_info_ui::PlayerInfoUi::default(),
            charts: crate::charts::ChartsUi::default(),
            tracker: crate::piece_tracker::PieceTrackerUi::default(),
            online: Box::default(),
            fics: Box::default(),
            browser: None,
            names: NameBook::default(),
            eco_book: None,
            collection: None,
            last_tags: None,
            pasted_moves: None,
            open_report: None,
            notice: None,
            prefs_dirty: false,
            flow: Flow::default(),
            notation_cache: notation_cache::NotationCache::default(),
            strip_cache: strip_cache::StripCache::default(),
            nag_keys: crate::nag_shortcuts::NagShortcutBuffer::default(),
        }
    }
}

impl AppState {
    /// A state that loads and saves its preferences at `path` (usually
    /// [`preferences_path`](crate::preferences_path)); `None` runs with the
    /// defaults and saves nothing. Send [`Message::Startup`] next.
    pub fn with_preferences_path(path: Option<PathBuf>) -> AppState {
        let mut state = AppState::default();
        state.flow.prefs_path = path;
        state
    }

    /// Whether the game was altered since it was loaded or saved.
    pub fn is_dirty(&self) -> bool {
        self.session.is_dirty()
    }

    /// The information panel for the cursor position, with the preferences
    /// and the opening book of the state.
    pub fn game_info(&self) -> GameInfoView {
        game_info(
            self.session.game(),
            self.session.cursor(),
            &self.prefs.game_info,
            // The name of the opening is help: hidden during an online game.
            self.eco_book.as_deref().filter(|_| !self.assist_locked()),
        )
    }

    /// A fresh request id.
    pub(crate) fn new_request(&mut self) -> RequestId {
        self.flow.next_request = self.flow.next_request.wrapping_add(1);
        RequestId(self.flow.next_request)
    }

    /// Whether a dialog that edits or replaces the game is open (the game
    /// cannot be edited meanwhile). It stays true while the save that
    /// answered "save first?" is being written, so the game cannot change
    /// under it.
    pub fn modal_open(&self) -> bool {
        self.browser.is_some() || self.other_modal_open()
    }

    /// Like [`AppState::modal_open`] without the browser: the dialogs that
    /// the browser itself must wait for.
    pub(crate) fn other_modal_open(&self) -> bool {
        self.setup.is_some()
            || self.info_dialog.is_some()
            || self.discard_prompt.is_some()
            || self.copy_prompt.is_some()
            || self.bookmark_editor.is_some()
            || self.eco.any_open()
            || self.report.config_dialog.is_some()
            || self.file_import.is_open()
            || self.games_export.is_open()
            || self.export_docs.is_open()
            || self.merge.is_open()
            || self.start.is_open()
            || self.tree.mask.prompt.is_some()
            || self.flow.save.as_ref().is_some_and(|s| s.then.is_some())
    }
}

/// Finishes the walk of the review if `message` is an action of the user.
fn interrupt_walk(state: &mut AppState, message: &Message, effects: &mut Vec<Effect>) -> bool {
    if state.report.walk.is_some() && interrupts_walk(message) {
        state.review_walk_finish(effects);
        return true;
    }
    false
}

/// Whether `message` is an action of the user that cuts the walk of the
/// review short (the streams of engines, timers and the like do not).
fn interrupts_walk(message: &Message) -> bool {
    match message {
        Message::Game(game) => !matches!(game, GameMessage::AutoplayTick { .. }),
        Message::Report(report) => !matches!(report, crate::report::ReportMessage::WalkTick { .. }),
        Message::Command(_)
        | Message::Action(_)
        | Message::Setup(_)
        | Message::Import(_)
        | Message::Bases(_)
        | Message::Info(_)
        | Message::Chooser(_)
        | Message::Bookmarks(_)
        | Message::Tree(_)
        | Message::Book(_)
        | Message::Eco(_)
        | Message::Quit
        | Message::NagKey { .. } => true,
        _ => false,
    }
}

/// Applies `message` to `state` and returns the effects to run.
///
/// Pure and non-blocking: no I/O, no timers (those are [`Effect`]s). While
/// a modal dialog is open (see [`AppState::modal_open`]) the editing
/// messages of [`GameMessage`] are ignored, except the autoplay tick.
pub fn update(state: &mut AppState, message: Message) -> Vec<Effect> {
    let mut effects = Vec::new();
    // An action of the user finishes the walk of the review first (D1663).
    // The early returns of the body skip its refreshes, so they are done
    // here when the walk moved the cursor.
    let finished = interrupt_walk(state, &message, &mut effects);
    // The cursor before the message, for the sound of what it did (D2803).
    let sound_mark = state.sound_mark();
    effects.extend(update_body(state, message));
    if let Some(mark) = sound_mark {
        state.sound_watch(mark, &mut effects);
    }
    if finished {
        refresh_views(state, &mut effects);
    }
    // The arrows of the review follow the state whatever path `update` took
    // (D1679).
    state.report.arrows = crate::evalbar::review_arrows(state);
    effects
}

#[allow(clippy::too_many_lines)] // one flat match over the messages reads best
fn update_body(state: &mut AppState, message: Message) -> Vec<Effect> {
    let mut effects = Vec::new();
    // The lock of the assistance during an online game (spec M8-3, D1330):
    // a message that asks for help is dropped here, not just hidden.
    if state.assist_locked() && crate::online::family_of(&message).is_some() {
        state.notice = Some(AppNotice::Online(
            crate::online::OnlineNotice::AssistanceDisabled,
        ));
        return effects;
    }
    match message {
        Message::Game(message) => {
            // The game is commanded by the annotation run (D469).
            if state.annotation_running() && !matches!(message, GameMessage::AutoplayTick { .. }) {
                state.notice = Some(AppNotice::Annotate(crate::annotate::AnnotateNotice::Busy));
                return effects;
            }
            // A problem of the tactics training judges the moves (M8-4).
            if state.tactics_intercept(&message, &mut effects) {
                return effects;
            }
            // A line of the opening trainer judges the moves (M8-5).
            if state.opening_intercept(&message, &mut effects) {
                return effects;
            }
            // The user is looking for the better move of a review (M4-10c).
            if state.review_intercept(&message, &mut effects) {
                return effects;
            }
            // The game is commanded by the game against an engine (D1240).
            if state.play_refuses_game_message(&message, &mut effects) {
                return effects;
            }
            if state.modal_open() && !matches!(message, GameMessage::AutoplayTick { .. }) {
                return effects;
            }
            // The lines of the calculation exercise are typed, not played;
            // a dialog on top of the board gets the keys first (M8-6, D2361).
            if state.calvar_intercept(&message, &mut effects) {
                return effects;
            }
            // A shoot-out or an automove owns the game's clock (D495).
            if state.training_refuses_autoplay(&message) {
                return effects;
            }
            state.training_stop_before_edit(&message, &mut effects);
            let before = state.training_before_game_message(&message);
            let board_before = state.eboard_before_game_message(&message);
            effects.extend(state.session.update(message));
            state.eboard_after_game_message(board_before, &mut effects);
            state.take_session_analysis_notice();
            state.training_after_game_message(before, &mut effects);
        }
        Message::Command(command) => state.command(command, &mut effects),
        Message::Action(action) => state.action(action, &mut effects),
        Message::Setup(message) => state.setup_message(message, &mut effects),
        Message::Import(message) => state.import_message(message, &mut effects),
        Message::Info(message) => state.info_message(message, &mut effects),
        Message::Chooser(message) => state.chooser_message(&message, &mut effects),
        Message::Bases(message) => state.bases_message(message, &mut effects),
        Message::Bookmarks(message) => state.bookmarks_message(message, &mut effects),
        Message::Engine(message) => state.engine_message(message, &mut effects),
        Message::Analysis(message) => state.analysis_message(message, &mut effects),
        Message::Annotate(message) => state.annotate_message(&message, &mut effects),
        Message::Report(message) => state.report_message(message, &mut effects),
        Message::Graph(message) => state.graph_message(message, &mut effects),
        Message::Tree(message) => state.tree_message(message, &mut effects),
        Message::Book(message) => state.book_message(message, &mut effects),
        Message::Eco(message) => state.eco_message(message, &mut effects),
        Message::FileImport(message) => state.file_import_message(message, &mut effects),
        Message::GamesExport(message) => state.games_export_message(message, &mut effects),
        Message::ExportDocs(message) => state.export_docs_message(message, &mut effects),
        Message::Epd(message) => state.epd_message(message, &mut effects),
        Message::OpeningReport(message) => state.opening_report_message(message, &mut effects),
        Message::Manual(message) => state.manual_message(message, &mut effects),
        Message::Start(message) => state.start_message(message, &mut effects),
        Message::PlayerReport(message) => state.player_report_message(message, &mut effects),
        Message::AdvancedSearch(message) => state.advanced_search_message(message, &mut effects),
        Message::PatternSearch(message) => state.pattern_message(message, &mut effects),
        Message::FilesPicked(request, paths) => {
            state.files_picked(request, paths, &mut effects);
        }
        Message::Mode(message) => state.mode_message(message, &mut effects),
        Message::Tactics(message) => state.tactics_message(message, &mut effects),
        Message::Calvar(message) => state.calvar_message(message, &mut effects),
        Message::Tablebase(message) => state.tablebase_message(message, &mut effects),
        Message::Crosstable(message) => state.crosstable_message(message, &mut effects),
        Message::Spelling(message) => state.spelling_message(message, &mut effects),
        Message::Maintenance(message) => state.maintenance_message(message, &mut effects),
        Message::Finder(message) => state.finder_message(message, &mut effects),
        Message::Merge(message) => state.merge_message(message, &mut effects),
        Message::PlayerInfo(message) => state.player_info_message(message, &mut effects),
        Message::Charts(message) => state.charts_message(message, &mut effects),
        Message::PieceTracker(message) => state.piece_tracker_message(message, &mut effects),
        Message::Eboard(message) => state.eboard_message(message, &mut effects),
        Message::Sound(message) => state.sound_message(message, &mut effects),
        Message::OpeningTrainer(message) => state.opening_message(message, &mut effects),
        Message::Play(message) => state.play_message(message, &mut effects),
        Message::Tournament(message) => state.tournament_message(message, &mut effects),
        Message::Online(message) => state.online_message(message, &mut effects),
        Message::Fics(message) => state.fics_message(message, &mut effects),
        Message::EvalBar(message) => state.evalbar_message(&message, &mut effects),
        Message::Prefs(message) => state.prefs_message(message, &mut effects),
        Message::NagKey { key, at } => state.nag_key(key, at, &mut effects),
        Message::NagTick(now) => state.nag_tick(now, &mut effects),
        Message::NagInterrupt => state.nag_interrupt(&mut effects),
        Message::AnswerDiscard(choice) => state.answer_discard(choice, &mut effects),
        Message::AnswerOverwrite(yes) => state.answer_overwrite(yes, &mut effects),
        Message::Startup => state.startup(&mut effects),
        Message::Quit => {
            state.tournament_quit(&mut effects);
            state.play_cancel_pick();
            // The score is written and the game of the user comes back
            // before the program asks about unsaved changes (D1935).
            state.tactics_stop(&mut effects);
            state.online_quit(&mut effects);
            state.fics_quit(&mut effects);
            state.eboard_quit(&mut effects);
            state.tree.cancel_search();
            state.advanced_search_cancel();
            state.pattern_search_cancel();
            state.crosstable_cancel();
            state.spelling_cancel();
            state.maintenance_cancel();
            state.finder_cancel();
            state.player_info_cancel();
            state.merge.cancel();
            state.epd_quit(&mut effects);
            state.charts_cancel();
            state.piece_tracker_cancel();
            state.opening_report_cancel();
            state.player_report_cancel();
            state.quit(&mut effects);
        }
        Message::FlushPreferences => state.flush_preferences(&mut effects),
        Message::WindowGeometryChanged(geometry) => state.window_geometry(geometry),
        Message::FilePicked(request, path) => state.file_picked(request, path, &mut effects),
        Message::ClipboardRead { purpose, result } => {
            state.clipboard_read(purpose, result, &mut effects);
        }
        Message::ClipboardWriteFailed => state.notice = Some(AppNotice::ClipboardWriteFailed),
        Message::HostEffectNotHandled => state.notice = Some(AppNotice::HostEffectNotHandled),
        Message::PgnScanned {
            request,
            path,
            result,
        } => state.pgn_scanned(request, path, result, &mut effects),
        Message::CollectionGameParsed { request, result } => {
            state.collection_game_parsed(request, result, &mut effects);
        }
        Message::PgnSaved {
            request,
            mode,
            result,
        } => state.pgn_saved(request, mode, result, &mut effects),
        Message::MovetextParsed { request, outcome } => state.movetext_parsed(request, *outcome),
        Message::PreferencesLoaded(loaded) => state.preferences_loaded(loaded, &mut effects),
        Message::PreferencesSaved(result) => state.preferences_saved(result, &mut effects),
        Message::EcoLoaded { path, result } => state.eco_loaded(&path, result),
        Message::Today(request, date) => state.today(request, date),
    }
    state.merge_guard();
    state.annotate_guard(&mut effects);
    state.report_guard(&mut effects);
    state.training_guard(&mut effects);
    state.tactics_guard(&mut effects);
    state.calvar_guard(&mut effects);
    state.pattern_search_guard(&mut effects);
    refresh_views(state, &mut effects);
    effects
}

/// The derived views and engine feeds, brought up to date after a message.
fn refresh_views(state: &mut AppState, effects: &mut Vec<Effect>) {
    state.refresh_notation();
    state.refresh_strips();
    state.refresh_engines(effects);
    state.refresh_evalbar();
    state.refresh_graph();
    state.refresh_tree(effects);
    state.refresh_eco(effects);
    state.refresh_tablebase(effects);
    state.refresh_crosstable(effects);
    state.refresh_spelling(effects);
    state.refresh_maintenance(effects);
    state.refresh_finder(effects);
    state.refresh_player_info(effects);
    state.refresh_charts(effects);
    state.refresh_piece_tracker(effects);
    state.refresh_epd(effects);
    state.refresh_opening_report();
    state.refresh_player_report();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use super::*;
    use crate::session::{GameMessage, Nav};

    #[test]
    fn update_routes_game_messages_to_the_session() {
        let mut state = AppState::default();
        let effects = update(&mut state, Message::Game(GameMessage::Navigate(Nav::Start)));
        assert!(effects.is_empty());
        assert_eq!(state.session.cursor(), state.session.game().root());
    }
}
