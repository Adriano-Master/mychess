//! The shell: `Shell { app, options, ui }` with the MVU functions of the
//! iced application.
//!
//! `Message::App` goes to `mychess_app::update`, whose effects run as tasks
//! (host effects through [`crate::host`]); `Message::Ui` changes only the
//! visual state (and may emit `Message::App`). After every message
//! `UiState::sync` brings animations and caches up to date (spec rule 2).

mod advanced_search_glue;
mod analysis_glue;
mod appearance_glue;
mod book_glue;
mod browser_glue;
mod calvar_glue;
mod eboard_glue;
mod epd_glue;
mod export_glue;
mod fics_glue;
mod game_commands_glue;
mod games_glue;
mod maintenance_glue;
mod manual_glue;
mod mask_glue;
mod notation_glue;
mod online_entry_glue;
mod online_glue;
mod opening_report_glue;
mod opening_trainer_glue;
mod pattern_search_glue;
mod player_report_glue;
mod player_tools_glue;
mod sound_glue;
mod start_glue;

/// Opens a folder in the file manager (spec M8-11b, D2914).
pub type FolderOpener = fn(&std::path::Path) -> std::io::Result<()>;
mod panels_glue;
mod subscriptions;
mod tablebase_glue;
mod tactics_glue;
mod tournament_glue;
mod ui_state;
mod window_title;

use std::path::PathBuf;
use std::time::Instant;

use iced::widget::pane_grid;
use iced::{Element, Size, Subscription, Task, Theme, window};
use mychess_app::{
    Action, Key, KeyInput, LoadedPreferences, Message as AppMessage, Modifiers, NamedKey,
    PlayMessage,
};

pub use self::ui_state::{TabMenu, UiState};
use crate::app_api::{self, ViewChange, ViewOptions};
use crate::host;
use crate::i18n::{I18n, LanguagePref};
use crate::theme::{self, ThemeMode};
use crate::widgets::board::geometry::{CoordsMode, Orientation};
use crate::widgets::board::interaction::BoardEvent;
use crate::widgets::modal;
use crate::windows::analysis::AnalysisUiMessage;
use crate::windows::book_panel::BookQuit;
use crate::windows::browser::BrowserMessage;
use crate::windows::calvar_dialog::CalvarUiMessage;
use crate::windows::dialog_ui::{
    ActiveDialog, CHOOSER_LIST_ID, DialogMessage, Outcome as DialogOutcome, TypedKey,
};
use crate::windows::engines_dialog::EnginePick;
use crate::windows::game_list::{GameListMessage, ListAction};
use crate::windows::layout::{self, PaneKind};
use crate::windows::main_window;
use crate::windows::notation::NotationMessage;
use crate::windows::online_ui::{OnlineDialog, OnlineUiMessage};
use crate::windows::opening_trainer_dialog::OpeningUiMessage;
use crate::windows::play_dialog::PlayUiMessage;
use crate::windows::player_tools::ToolWindow;
use crate::windows::tactics_dialog::TacticsUiMessage;
use crate::windows::tournament_ui::TournamentUiMessage;

/// A message of the shell.
// Messages are short-lived values; boxing the application message would only
// add noise at every call site.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Message {
    /// For the application layer.
    App(AppMessage),
    /// For the visual state.
    Ui(UiMessage),
    /// A dialog, the move field or a typed key (see
    /// [`DialogMessage`]).
    Dialog(DialogMessage),
    /// The notation panel and the comment editor.
    Notation(NotationMessage),
    /// The game list panel (spec M3-4, slice 4b).
    Games(GameListMessage),
    /// The game browser (spec M3-4, slice 4e).
    Browser(BrowserMessage),
    /// The game against an engine: its dialog and its panel (spec M8-1b).
    PlayUi(PlayUiMessage),
    /// The tournament between engines: its form, panel and boxes (spec
    /// M8-2b).
    Tournament(TournamentUiMessage),
    /// The online play on Lichess: its windows and panel (spec M8-3c).
    Online(OnlineUiMessage),
    /// The FICS window and its board events (spec M8-9c).
    Fics(crate::windows::fics_dialog::FicsUiMessage),
    /// The dialog of the tactics training (spec M8-4c).
    TacticsUi(TacticsUiMessage),
    /// The dialog of the opening trainer (spec M8-5c).
    OpeningUi(OpeningUiMessage),
    /// The dialog and the panel of the calculation exercise (spec M8-6c).
    CalvarUi(CalvarUiMessage),
    /// The window of the advanced searches (spec M6-1c).
    AdvancedUi(crate::windows::advanced_search_dialog::AdvancedUiMessage),
    /// The windows of the players, the tournaments and the spelling file
    /// (spec M6-3c).
    PlayerTools(crate::windows::player_tools::PlayerToolsMessage),
    /// The window of the search for tactics and patterns (spec M6-2d).
    PatternUi(crate::windows::pattern_search_dialog::PatternUiMessage),
    /// The folder dialog of the sounds window (spec M8-11).
    SoundUi(crate::windows::sound_dialog::SoundUiMessage),
    /// The window of the electronic board (spec M8-10c).
    EboardUi(crate::windows::eboard_dialog::EboardUiMessage),
    /// The window of the tablebase panel (spec M8-8c).
    TablebaseUi(crate::windows::tablebase_panel::TablebaseUiMessage),
    /// The fields and dialogs of the crosstable, statistics, rating graph
    /// and piece tracker panels (spec M6-4d).
    Panels(crate::windows::data_panels::DataPanelsMessage),
    /// The tree masks: context menu, editors, search and outline (spec
    /// M5-4c).
    MaskUi(crate::windows::mask::MaskUiMessage),
    /// The book window: file dialogs and the weight fields (spec M5-6).
    Book(crate::windows::book_panel::BookUiMessage),
    /// The EPD panel: editor, search, forms and preferences (spec M7-5c).
    Epd(crate::windows::epd::EpdUiMessage),
    /// The opening report window (spec M7-2c).
    OpRep(crate::windows::opening_report::OpRepUiMessage),
    /// The player report window and its dialog (spec M7-3c).
    PlayerReportUi(crate::windows::player_report::PrUiMessage),
    /// The manual window (spec M9-5).
    ManualUi(crate::windows::manual::ManualUiMessage),
    /// The maintenance window (spec M7-1c).
    Maint(crate::windows::maintenance::MaintUiMessage),
}

/// A message that changes only the visual state.
#[derive(Debug, Clone)]
pub enum UiMessage {
    /// The board produced an event.
    Board(BoardEvent),
    /// One second went by in the "save the books, then quit" run (D1796).
    BookQuitTick,
    /// "Start and play against the engine" of the Chess960 dialog: starts the
    /// numbered position, then opens the play dialog (M9-6).
    Chess960Play,
    /// Turn the board.
    FlipBoard,
    /// Open the ECO browser (the translation glossary of the language of
    /// the interface goes with the request, spec M5-7b).
    OpenEcoBrowser,
    /// A key was pressed; the shortcut map of the application says what it
    /// means. `text_focused` is true when a widget used the key for text.
    Key {
        /// The key and its modifiers.
        input: KeyInput,
        /// Whether a text field has the focus.
        text_focused: bool,
    },
    /// An action of the shortcut map (from a key or a menu).
    Action(Action),
    /// A frame of the running animation.
    AnimationFrame(Instant),
    /// The operating system changed (or reported) its theme.
    SystemTheme(iced::theme::Mode),
    /// A session-only visual option changed.
    View(ViewChange),
    /// The coordinates choice changed (the "both sides" variant is a
    /// session option; the rest is a preference).
    Coords(CoordsMode),
    /// A panel was dragged.
    PaneDragged(pane_grid::DragEvent),
    /// A divider was dragged.
    PaneResized(pane_grid::ResizeEvent),
    /// Open or close a panel.
    TogglePane(PaneKind),
    /// Back to the default arrangement.
    ResetLayout,
    /// Close a panel (the button of its title bar, spec M2-6, D1003).
    ClosePane(PaneKind),
    /// A tab was chosen: it becomes the selected one of its group (spec
    /// M2-6, D1007), opening the panel if it was closed.
    SelectTab {
        /// The panel of the tab.
        kind: PaneKind,
    },
    /// The context menu of a tab was requested (right click, D1006).
    OpenTabMenu {
        /// The panel of the tab.
        kind: PaneKind,
        /// The pointer, in window coordinates.
        position: iced::Point,
    },
    /// Close the context menu of a tab.
    CloseTabMenu,
    /// Take a tab out of its group into a split of its own (D1006).
    DetachTab(PaneKind),
    /// The window was resized.
    WindowResized(Size),
    /// Show or hide the About box.
    ShowAbout(bool),
    /// The ribbon: a tab, the collapse arrow or a click outside (spec M9-2).
    Ribbon(crate::windows::ribbon::RibbonMessage),
    /// A message chosen in the temporary ribbon: closes it, then runs.
    RibbonPick(Box<Message>),
    /// The user asked to close the window (or chose Quit).
    CloseRequested,
    /// Time to save the preferences that changed (see
    /// `subscriptions::FLUSH_INTERVAL`).
    FlushTick,
    /// Time to redraw the progress of a database being opened.
    OpenProgressTick,
    /// Shows the native dialog that picks an engine program or an
    /// `engines.dat` (spec M4-3).
    PickEngineFile(EnginePick),
    /// The answer of that dialog (`None`: cancelled).
    EngineFilePicked(EnginePick, Option<PathBuf>),
    /// Unfolds or folds the analysis options of the Engines dialog.
    ToggleEnginePrefs,
    /// The analysis panel needs the front-end (spec M4-4).
    Analysis(AnalysisUiMessage),
}

/// The whole front-end state.
#[derive(Debug)]
pub struct Shell {
    /// The application state (the model), including the preferences.
    pub app: mychess_app::AppState,
    /// The visual options that last for the session only.
    pub options: ViewOptions,
    /// The visual state.
    pub ui: UiState,
    /// The translations of the active language.
    pub i18n: I18n,
    /// Opens a folder in the file manager (replaced in tests).
    pub folder_opener: FolderOpener,
    applied_language: LanguagePref,
    /// The last value of `AppState::online_entries` the shell reacted to
    /// (M8-3, D4520).
    seen_online_entries: u32,
}

/// A window length as whole logical pixels (saturating, never negative).
fn whole_pixels(length: f32) -> u32 {
    if length.is_finite() && length > 0.0 {
        // Rounded, positive and clamped to the `u16` range: the cast is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pixels = length.round().min(f32::from(u16::MAX)) as u32;
        pixels
    } else {
        0
    }
}

impl Shell {
    /// A shell showing a new game. The preferences file is `prefs_path`
    /// (`None`: nothing is loaded or saved); the preferences themselves
    /// arrive with `PreferencesLoaded` (see [`Shell::boot`]).
    pub fn new(prefs_path: Option<PathBuf>) -> Shell {
        let app = app_api::state_with_preferences_path(prefs_path);
        let language_pref = app_api::language_pref(&app);
        let mut shell = Shell {
            ui: UiState::new(&layout::or_default(app.prefs.layout.as_ref())),
            i18n: I18n::new(language_pref.resolve()),
            applied_language: language_pref,
            seen_online_entries: 0,
            folder_opener: sound_glue::open_in_file_manager,
            options: ViewOptions::default(),
            app,
        };
        shell.sync();
        shell
    }

    /// The initial state and the tasks that run at start: ask the system for
    /// its theme and load the preferences.
    ///
    /// `preloaded` is what [`peek_preferences`](crate::peek_preferences)
    /// read before the window opened; it makes the first frame use the
    /// saved theme, language and layout. With a `prefs_path`, `Startup`
    /// follows and the asynchronous load has the last word.
    pub fn boot(
        prefs_path: Option<PathBuf>,
        preloaded: Option<LoadedPreferences>,
    ) -> (Shell, Task<Message>) {
        Shell::boot_with(prefs_path, preloaded, None)
    }

    /// Like [`Shell::boot`], handing over what the command line asked for
    /// (spec M9-4): the files to open and the welcome or the tip run once
    /// the preferences are read.
    pub fn boot_with(
        prefs_path: Option<PathBuf>,
        preloaded: Option<LoadedPreferences>,
        launch: Option<mychess_app::start::StartInfo>,
    ) -> (Shell, Task<Message>) {
        let has_path = prefs_path.is_some();
        let mut shell = Shell::new(prefs_path);
        // The user's patterns live in the data folder of the system (spec
        // M6-2, D3277); tests build a `Shell` with `new` and keep the default.
        shell.app.set_pattern_dir(mychess_app::patterns_dir());
        let mut tasks =
            vec![iced::system::theme().map(|mode| Message::Ui(UiMessage::SystemTheme(mode)))];
        if let Some(loaded) = preloaded {
            tasks.push(shell.update(Message::App(app_api::preferences_loaded(loaded))));
        }
        if let Some(info) = launch {
            // A book named on the command line shows its panel.
            if info
                .files
                .iter()
                .any(|f| matches!(f, mychess_app::start::LaunchFile::Book(_)))
            {
                // Only for this session: the saved layout stays as it is.
                shell.ui.open_pane(PaneKind::Book);
            }
            // Before `Startup`: the answer of the disk starts the launch.
            tasks.push(shell.update(Message::App(AppMessage::Start(
                mychess_app::start::StartMessage::Configure(Box::new(info)),
            ))));
        }
        if has_path {
            tasks.push(shell.update(Message::App(app_api::startup())));
            // The marker of a game in progress (D1332, D1359).
            tasks.push(shell.update(Message::App(AppMessage::Online(
                mychess_app::online::OnlineMessage::LoadMarker,
            ))));
        } else if !shell.ui.dialogs.player_tools.started {
            // No preferences file will ever be read: the spelling file still
            // loads, from the default place with the default preferences
            // (D3497).
            shell.ui.dialogs.player_tools.started = true;
            tasks.push(shell.update(Message::App(AppMessage::Spelling(
                mychess_app::SpellingMessage::LoadAtStart,
            ))));
        }
        (shell, Task::batch(tasks))
    }

    fn sync(&mut self) {
        self.sync_graph();
        self.ui.dialogs.play.rebuild_strips(&self.app);
        self.ui.dialogs.sync(&self.app);
        self.ui.dialogs.panels.drop_unfocused_text();
        // The application owns the orientation (D850); the board follows
        // it, and a change ends any animation.
        if (self.ui.orientation == Orientation::Black) != self.app.board_flipped {
            self.ui.flip();
        }
        self.ui.note_generation(self.app.session.generation());
        let view = app_api::board_view(&self.app);
        let look = app_api::appearance(&self.app, self.options);
        self.ui.sync(&view, app_api::board_marks(&self.app), &look);
        self.ui.sync_arrows(app_api::engine_arrows(&self.app));
        let badge = self.class_badge();
        self.ui.sync_badge(badge);
        self.sync_tournament(look.board);
        self.ui.notation.list.set_piece_set(look.piece_set);
        self.ui.browser.sync(&self.app, look.piece_set, look.board);
        self.ui
            .notation
            .sync(&self.app, &self.i18n, self.ui.orientation, look.board);
        if self.ui.notation.editor.take_dropped_nag_draft() {
            self.ui.status_notice = Some(mychess_app::AppNotice::NagDraftDiscarded);
            self.ui.dialogs.status.reset();
        }
    }

    /// Tells the application whether the graph panel is open (it builds the
    /// view only then) and drops the drawing cache of a graph that changed
    /// (spec M4-7, D873).
    fn sync_graph(&mut self) {
        if self.app.graph().is_open() != self.ui.is_open(PaneKind::Graph) {
            // The graph messages never produce effects.
            drop(mychess_app::update(
                &mut self.app,
                AppMessage::Graph(mychess_app::graphs::GraphMessage::SetOpen(
                    self.ui.is_open(PaneKind::Graph),
                )),
            ));
        }
        self.ui.graph.sync(self.app.graph().stamp());
        self.ui.report_graph.sync(self.app.report.stamp);
    }

    /// Tells the application whether the opening tree panel is open, which
    /// starts or stops the searches (spec M5-2, D981). Unlike the graph,
    /// opening and closing produce effects, so this returns their tasks.
    fn sync_tree(&mut self) -> Task<Message> {
        // The application asked for the panel itself ("open base as
        // tree", D1641).
        let shown = if self.app.take_tree_show_request() {
            self.change_panes(|ui| ui.open_pane(PaneKind::Tree))
        } else {
            Task::none()
        };
        let mut wanted = self.ui.is_open(PaneKind::Tree);
        // Locked by an online game: the application ignores `Open` (D1742),
        // so the panel closes instead of asking again at every update.
        let mut closed = Task::none();
        if wanted && self.app.assist_locked() && !self.app.tree().is_open() {
            closed = self.change_panes(|ui| ui.close_pane(PaneKind::Tree));
            wanted = false;
        }
        let shown = Task::batch([shown, closed]);
        if self.app.tree().is_open() == wanted {
            return shown;
        }
        let message = if wanted {
            mychess_app::TreeMessage::Open
        } else {
            mychess_app::TreeMessage::Close
        };
        Task::batch([shown, self.dispatch(AppMessage::Tree(message))])
    }

    /// Loads the translations again when the language preference changed.
    fn refresh_language(&mut self) {
        let wanted = app_api::language_pref(&self.app);
        if wanted != self.applied_language {
            self.applied_language = wanted;
            self.i18n = I18n::new(wanted.resolve());
        }
    }

    /// Sends `message` to the application and turns its effects into tasks.
    fn dispatch(&mut self, message: AppMessage) -> Task<Message> {
        // The comment of a merge is written in the language of the interface.
        let message = crate::windows::merge_dialog::localize(&self.app, &self.i18n, message);
        if app_api::is_user_action(&message) {
            self.ui.status_notice = None;
            self.ui.dialogs.play.note = None;
            self.ui.dialogs.status.reset();
        }
        let loaded = app_api::is_preferences_loaded(&message);
        let opened_base = app_api::is_base_opened(&message);
        let failed_open = app_api::failed_open_path(&self.app, &message);
        // "Don't ask again" is kept once the answer went ahead.
        let remember = if self.ui.dialogs.dont_ask && app_api::is_confirming_answer(&message) {
            app_api::session_prompt(&self.app).and_then(app_api::dont_ask_again)
        } else {
            None
        };
        self.note_analysis_message(&message);
        self.note_pattern_open(&message);
        let reveal_merge = crate::windows::merge_dialog::moves_preview(&message);
        let runs_before = self.app.engine_runs().len();
        let mut tasks = self.save_comment_draft(&message);
        let effects = mychess_app::update(&mut self.app, message);
        tasks.extend(
            effects
                .into_iter()
                .map(|effect| host::task(host::classify(effect, &self.i18n))),
        );
        if loaded {
            self.adopt_saved_layout();
            tasks.push(self.sync_comment_editor());
            // The spelling file loads once, after the preferences said which
            // one (spec M6-3c, D3466).
            if !self.ui.dialogs.player_tools.started {
                self.ui.dialogs.player_tools.started = true;
                tasks.push(self.dispatch(AppMessage::Spelling(
                    mychess_app::SpellingMessage::LoadAtStart,
                )));
            }
        }
        tasks.push(self.reveal_analysis_for_new_run(runs_before));
        if reveal_merge && let Some(task) = crate::windows::merge_dialog::reveal_first(&self.app) {
            // The new moves may be far from the top of a long game.
            tasks.push(task);
        }
        if opened_base {
            tasks.push(self.reveal_game_list());
        }
        if let Some(notice) = app_api::take_app_notice(&mut self.app) {
            self.ui.status_notice = Some(app_api::name_failed_file(notice, failed_open));
            self.ui.dialogs.status.reset();
        }
        if let Some(preference) = remember {
            self.ui.dialogs.dont_ask = false;
            tasks.push(self.dispatch(preference));
        }
        Task::batch(tasks)
    }

    /// Shows the saved arrangement of the panels after the preferences were
    /// loaded (a layout the user is already using is left alone).
    fn adopt_saved_layout(&mut self) {
        let wanted = layout::or_default(self.app.prefs.layout.as_ref());
        if wanted != self.ui.layout() {
            self.ui.set_layout(&wanted);
        }
    }

    /// Applies a message and returns the follow-up work.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        self.ui.dialogs.play.now = Instant::now();
        self.ui.dialogs.tournament.now = self.ui.dialogs.play.now;
        self.ui.dialogs.online.now = self.ui.dialogs.play.now;
        self.ui.dialogs.fics.now = self.ui.dialogs.play.now;
        // A tick of the FICS clocks changes only the strips.
        if let Message::Fics(crate::windows::fics_dialog::FicsUiMessage::Tick(at)) = message {
            return self.on_fics_tick(at);
        }
        // A tick of the online clocks changes only the strips.
        if let Message::Online(OnlineUiMessage::Tick(at)) = message {
            return self.on_online_tick(at);
        }
        // A tick of the clocks of the live tab changes nothing else.
        if let Message::Tournament(TournamentUiMessage::Tick(at)) = message {
            self.ui.dialogs.tournament.now = at;
            return Task::none();
        }
        if let Message::App(AppMessage::Play(PlayMessage::Tick(at))) = message {
            return self.on_play_tick(at);
        }
        if let Message::Ui(UiMessage::AnimationFrame(now)) = message {
            // A frame only moves the eased progress, which the cached layers
            // do not depend on; the full synchronization is needed only when
            // the animation just ended (its hidden squares disappear).
            self.ui.on_frame(now);
            if !self.ui.is_animating() {
                self.sync();
            }
            return Task::none();
        }
        let task = match message {
            Message::App(message) => {
                // What was typed in a weight field is dropped when anything
                // is sent to the books (it may have changed the weight).
                if matches!(message, AppMessage::Book(_)) {
                    self.ui.dialogs.book.clear();
                }
                let loaded = app_api::is_browser_loaded(&message);
                let sound_extra = self.sound_extra(&message);
                let task = self.dispatch(message);
                let task = if loaded {
                    Task::batch([task, self.reveal_current_move()])
                } else {
                    task
                };
                match sound_extra {
                    Some(extra) => Task::batch([task, extra]),
                    None => task,
                }
            }
            Message::Ui(message) => self.update_ui(message),
            Message::Dialog(message) => self.update_dialog(message),
            Message::Notation(message) => {
                // A click may have moved the focus into or out of the
                // search field of the game list: ask the widget.
                let probe = matches!(message, NotationMessage::PointerReleased);
                if probe {
                    // The button is up: a drag of the game list ends. A drop
                    // on a tab was a message of the widget, handled before
                    // this one (D576).
                    self.ui.games.drag = None;
                    self.ui.games.drag_cancelled = false;
                    self.ui.games.pressed_tab = None;
                }
                let task = self.update_notation(message);
                if probe {
                    Task::batch([
                        task,
                        self.probe_search_focus(),
                        self.probe_analysis_focus(),
                        Self::probe_panel_focus(),
                        Self::probe_epd_focus(),
                    ])
                } else {
                    task
                }
            }
            Message::Games(message) => self.update_games(message),
            Message::Browser(message) => self.update_browser(message),
            Message::PlayUi(message) => self.update_play(message),
            Message::Tournament(message) => self.update_tournament(message),
            Message::Online(message) => self.update_online(message),
            Message::Fics(message) => self.update_fics(message),
            Message::TacticsUi(message) => self.update_tactics(message),
            Message::OpeningUi(message) => self.update_opening(message),
            Message::CalvarUi(message) => self.update_calvar(message),
            Message::AdvancedUi(message) => self.update_advanced_search(message),
            Message::PlayerTools(message) => self.update_player_tools(message),
            Message::PatternUi(message) => self.update_pattern_search(message),
            Message::TablebaseUi(message) => self.update_tablebase(message),
            Message::Panels(message) => self.update_panels(message),
            Message::SoundUi(message) => self.update_sound(message),
            Message::EboardUi(message) => self.update_eboard(message),
            Message::MaskUi(message) => self.update_mask(message),
            Message::Book(message) => self.update_book(message),
            Message::Epd(message) => self.update_epd(message),
            Message::OpRep(message) => self.update_oprep(message),
            Message::PlayerReportUi(message) => self.update_preport(message),
            Message::ManualUi(message) => self.update_manual(message),
            Message::Maint(message) => self.update_maint(message),
        };
        self.finish_update(task)
    }

    /// What follows every update: the pieces of the screen catch up with
    /// the application.
    fn finish_update(&mut self, task: Task<Message>) -> Task<Message> {
        // An online game was entered: the board becomes the main window
        // (M8-3, D4520).
        let task = if self.app.online_entries == self.seen_online_entries {
            task
        } else {
            self.seen_online_entries = self.app.online_entries;
            Task::batch([task, self.dismiss_for_online_game()])
        };
        // A start that waited for the "discard changes?" answer (D1252).
        let note_before = self.ui.dialogs.play.note;
        let resumed = match self.ui.dialogs.play.resume_pending(&self.app) {
            Some(message) => self.dispatch(message),
            None => Task::none(),
        };
        if self.ui.dialogs.play.note.is_some() && self.ui.dialogs.play.note != note_before {
            self.ui.dialogs.status.reset();
        }
        let task = Task::batch([task, resumed]);
        // The browser takes the keys: the search field of the list lets go
        // of the focus (D613).
        let release = if self.app.browser.is_some() && self.ui.games.search_focused {
            self.ui.games.search_focused = false;
            modal::unfocus_all()
        } else {
            Task::none()
        };
        let task = Task::batch([task, release]);
        self.refresh_language();
        let top_before = self.ui.dialogs.player_tools.top();
        self.sync();
        // A window of the player tools without a field of its own to type in
        // opens with no focus (a field under it would keep the keys).
        let top = self.ui.dialogs.player_tools.top();
        let task = if top != top_before
            && matches!(
                top,
                Some(
                    ToolWindow::TournamentFinder
                        | ToolWindow::PlayerInfo
                        | ToolWindow::SpellCheck
                        | ToolWindow::AddElo
                )
            ) {
            Task::batch([task, modal::unfocus_all()])
        } else {
            task
        };
        let task = Task::batch([task, self.sync_tree()]);
        let task = Task::batch([task, self.sync_tablebase()]);
        let task = Task::batch([task, self.sync_data_panels()]);
        let task = Task::batch([task, self.sync_epd()]);
        let task = Task::batch([task, self.sync_oprep()]);
        let task = Task::batch([task, self.sync_preport()]);
        self.sync_export();
        let task = Task::batch([task, self.sync_manual()]);
        let task = Task::batch([task, self.sync_maint()]);
        self.sync_book();
        let task = Task::batch([task, self.book_quit_step()]);
        self.clamp_game_list();
        self.ui.games.drop_stale_menu(&self.app);
        self.drop_stale_training_box();
        // A dialog covers the temporary ribbon; it must not come back when
        // the dialog closes.
        if ActiveDialog::of(&self.app, &self.ui.dialogs).is_some() {
            self.ui.ribbon.flyout = false;
        }
        match self.ui.dialogs.newly_opened(&self.app) {
            Some(id) => Task::batch([task, iced::widget::operation::focus(id)]),
            None => task,
        }
    }

    #[allow(clippy::too_many_lines)] // one flat match over the messages
    fn update_ui(&mut self, message: UiMessage) -> Task<Message> {
        match message {
            UiMessage::Board(event) => return self.on_board_event(event),
            UiMessage::FlipBoard => return self.dispatch(app_api::action(Action::FlipBoard)),
            UiMessage::Chess960Play => {
                let started = self.ui.dialogs.chess960.is_some();
                let out = self
                    .ui
                    .dialogs
                    .update(DialogMessage::Chess960Submit, &self.app);
                let first = self.run_outcome(out, None);
                // A number that is not valid keeps the dialog open: nothing
                // else happens.
                if !started || self.ui.dialogs.chess960.is_some() {
                    return first;
                }
                return Task::batch([first, self.update_play(PlayUiMessage::Open)]);
            }
            UiMessage::OpenEcoBrowser => {
                return self.dispatch(mychess_app::Message::Eco(
                    mychess_app::EcoMessage::OpenBrowser {
                        glossary: crate::windows::eco_text::glossary(&self.i18n),
                    },
                ));
            }
            UiMessage::Key {
                input,
                text_focused,
            } => return self.on_key(input, text_focused, None),
            UiMessage::Action(action) => return self.run_action(action),
            UiMessage::AnimationFrame(now) => self.ui.on_frame(now),
            UiMessage::SystemTheme(mode) => self.ui.system_mode = mode,
            UiMessage::View(change) => self.options.apply(change),
            UiMessage::Coords(mode) => {
                self.options.coords_both_sides = mode == CoordsMode::OutsideBothSides;
                return self.dispatch(app_api::set_coordinates(mode));
            }
            UiMessage::PaneDragged(pane_grid::DragEvent::Dropped { pane, target }) => {
                return self.change_panes(|ui| match target {
                    // Dropped on the middle of a panel: the tabs merge
                    // (spec M2-6, D1005). The board takes no part.
                    pane_grid::Target::Pane(onto, pane_grid::Region::Center) => {
                        ui.merge_panes(pane, onto);
                    }
                    // On an edge the panel splits the space, as before.
                    other => ui.drop_pane(pane, other),
                });
            }
            // The tick only brings a new frame: the progress is read from
            // the application when the view is built.
            UiMessage::PaneDragged(_) | UiMessage::OpenProgressTick => {}
            UiMessage::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.ui.panes.resize(split, ratio);
                return self.remember_layout();
            }
            UiMessage::TogglePane(kind) => return self.change_panes(|ui| ui.toggle_pane(kind)),
            UiMessage::ClosePane(kind) => return self.change_panes(|ui| ui.close_pane(kind)),
            UiMessage::SelectTab { kind } => return self.change_panes(|ui| ui.show_pane(kind)),
            UiMessage::OpenTabMenu { kind, position } => {
                self.ui.tab_menu = Some(TabMenu { kind, position });
            }
            UiMessage::CloseTabMenu => self.ui.tab_menu = None,
            UiMessage::DetachTab(kind) => return self.change_panes(|ui| ui.detach_tab(kind)),
            UiMessage::ResetLayout => {
                let hide = self.change_panes(UiState::reset_layout);
                return Task::batch([hide, self.dispatch(app_api::set_layout(None))]);
            }
            UiMessage::WindowResized(size) => return self.on_window_resized(size),
            UiMessage::ShowAbout(show) => self.ui.show_about = show,
            UiMessage::Ribbon(message) => {
                let collapsed = app_api::ribbon_collapsed(&self.app);
                if let Some(value) = self.ui.ribbon.update(message, collapsed) {
                    return self.dispatch(app_api::set_ribbon_collapsed(value));
                }
            }
            UiMessage::RibbonPick(inner) => {
                self.ui.ribbon.flyout = false;
                return self.update(*inner);
            }
            UiMessage::CloseRequested => return self.request_quit(),
            UiMessage::BookQuitTick => return self.book_quit_tick(),
            UiMessage::FlushTick => return self.dispatch(app_api::flush_preferences()),
            UiMessage::PickEngineFile(pick) => {
                let dialog = host::EngineFileDialog::new(pick, &self.i18n);
                return Task::perform(dialog.run(), move |path| {
                    Message::Ui(UiMessage::EngineFilePicked(pick, path))
                });
            }
            UiMessage::EngineFilePicked(pick, path) => return self.engine_file_picked(pick, path),
            UiMessage::ToggleEnginePrefs => {
                self.ui.dialogs.engines_prefs_open = !self.ui.dialogs.engines_prefs_open;
            }
            UiMessage::Analysis(message) => return self.update_analysis_ui(message),
        }
        Task::none()
    }

    // ---- keyboard and actions ----

    /// Runs what a dialog event asked for: messages for the application,
    /// a widget to focus, the focus to cycle, the list to scroll.
    fn run_outcome(&mut self, out: DialogOutcome, active: Option<ActiveDialog>) -> Task<Message> {
        let mut tasks = Vec::new();
        for message in out.messages {
            tasks.push(self.dispatch(message));
        }
        if let Some(id) = out.focus {
            tasks.push(iced::widget::operation::focus(id));
        }
        if let (Some(backwards), Some(active)) = (out.cycle, active) {
            tasks.push(modal::cycle_focus(active.box_id(), backwards));
        }
        if out.quit {
            // Enter on the tournament question gets here too: the unsaved
            // books are asked about last in every path (D1795).
            tasks.push(self.quit_after_books());
        }
        if let Some(offset) = out.scroll {
            tasks.push(iced::widget::operation::scroll_to(
                CHOOSER_LIST_ID,
                iced::widget::scrollable::AbsoluteOffset {
                    x: Some(0.0),
                    y: Some(offset),
                },
            ));
        }
        Task::batch(tasks)
    }

    fn update_dialog(&mut self, message: DialogMessage) -> Task<Message> {
        match message {
            DialogMessage::Key {
                input,
                text_focused,
                typed,
            } => return self.on_key(input, text_focused, typed),
            DialogMessage::StatusTick(now) => {
                let has_notice = self.has_remark();
                self.ui.dialogs.status.tick(now, has_notice);
                return Task::none();
            }
            DialogMessage::Appearance(message) => return self.update_appearance(message),
            _ => {}
        }
        let out = self.ui.dialogs.update(message, &self.app);
        self.run_outcome(out, None)
    }

    /// Whether there is a remark for the status bar, expired or not.
    fn has_remark(&self) -> bool {
        self.ui.status_notice.is_some()
            || self.ui.dialogs.play.note.is_some()
            || app_api::notice(&self.app).is_some()
    }

    /// Whether the status bar shows a remark that goes away by itself: one
    /// that expired needs no timer any more.
    pub fn has_temporary_notice(&self) -> bool {
        !self.ui.dialogs.status.expired() && self.has_remark()
    }

    /// Whether a layer is open over the window that owns the keyboard: a
    /// dialog, the About box, the game browser, a running annotation, the
    /// comment editor or an inline box of the analysis panel (training,
    /// exclusion). The keys of the panel, of the game list and the typed
    /// keys (NAG sequences, move entry) do not act while one is open.
    /// **Every new layer must be added here** (D920).
    pub fn modal_layer_open(&self) -> bool {
        ActiveDialog::of(&self.app, &self.ui.dialogs).is_some()
            || self.ui.show_about
            || self.app.browser.is_some()
            || self.app.annotation_running()
            || self.ui.comment_visible()
            || self.analysis_box_open()
            || self.app.fics().window_open
            || self.epd_layer_open()
            || self.oprep_layer_open()
            || self.preport_layer_open()
            || self.export_image_layer_open()
            || self.manual_layer_open()
            || self.maint_layer_open()
            || self.start_layer_open()
    }

    /// Enter goes to the next problem of the tactics training once the
    /// problem is over (D2143). It is asked only with no layer open and no
    /// text field focused, and not while a move is being typed (Enter would
    /// commit it).
    fn tactics_enter(&mut self, input: &KeyInput, text_focused: bool) -> Option<Task<Message>> {
        let over = matches!(
            self.app.tactics_phase(),
            mychess_app::tactics::TacticsPhase::Solved(_)
                | mychess_app::tactics::TacticsPhase::Failed(_)
        );
        (input.key == Key::Named(NamedKey::Enter)
            && input.modifiers == Modifiers::NONE
            && over
            && !text_focused
            && !self.modal_layer_open()
            && !self.ui.analysis.field_focused()
            && !(self.ui.games.search_focused && self.ui.is_visible(PaneKind::GameList))
            && self.app.session.move_entry().text().is_empty())
        .then(|| {
            self.dispatch(mychess_app::Message::Tactics(
                mychess_app::tactics::TacticsMessage::Next,
            ))
        })
    }

    /// Enter goes to the next line of the opening trainer once the line is
    /// over (D2238), under the same guards as [`Shell::tactics_enter`].
    fn opening_enter(&mut self, input: &KeyInput, text_focused: bool) -> Option<Task<Message>> {
        let over = matches!(
            self.app.opening_phase(),
            mychess_app::opening_trainer::OpeningPhase::LineDone(_)
        );
        (input.key == Key::Named(NamedKey::Enter)
            && input.modifiers == Modifiers::NONE
            && over
            && !text_focused
            && !self.modal_layer_open()
            && !self.ui.analysis.field_focused()
            && !(self.ui.games.search_focused && self.ui.is_visible(PaneKind::GameList))
            && self.app.session.move_entry().text().is_empty())
        .then(|| {
            self.dispatch(mychess_app::Message::OpeningTrainer(
                mychess_app::opening_trainer::OpeningMessage::Next,
            ))
        })
    }

    /// Enter in the training modes that move on with it: the next problem
    /// of the tactics (D2143) or the next line of the openings (D2238).
    fn training_enter(&mut self, input: &KeyInput, text_focused: bool) -> Option<Task<Message>> {
        self.tactics_enter(input, text_focused)
            .or_else(|| self.opening_enter(input, text_focused))
    }

    /// A key press. Tab may move the focus between the text fields of the
    /// EPD panel, so the focus is probed again after it (D3865).
    fn on_key(
        &mut self,
        input: KeyInput,
        text_focused: bool,
        typed: Option<char>,
    ) -> Task<Message> {
        let tab = input.key == Key::Named(NamedKey::Tab);
        let task = self.on_key_layers(input, text_focused, typed);
        if tab && self.ui.is_visible(PaneKind::Epd) {
            Task::batch([task, Self::probe_epd_focus()])
        } else {
            task
        }
    }

    #[allow(clippy::too_many_lines)] // one flat chain of the layers that take a key
    fn on_key_layers(
        &mut self,
        input: KeyInput,
        text_focused: bool,
        typed: Option<char>,
    ) -> Task<Message> {
        let escape =
            input.key == Key::Named(NamedKey::Escape) && input.modifiers == Modifiers::NONE;
        // Esc closes the temporary ribbon first (it is not a modal layer).
        if escape && self.ui.ribbon.flyout {
            self.ui.ribbon.flyout = false;
            return Task::none();
        }
        if let Some(active) = ActiveDialog::of(&self.app, &self.ui.dialogs) {
            if active == ActiveDialog::Appearance
                && let Some(task) = self.appearance_key(&input)
            {
                return task;
            }
            if let Some(task) = self.player_tools_key(active, &input, text_focused) {
                return task;
            }
            let out = self.ui.dialogs.key(active, &input, text_focused, &self.app);
            return self.run_outcome(out, Some(active));
        }
        if escape && self.epd_layer_open() {
            return self.epd_escape().unwrap_or_else(Task::none);
        }
        // A form or question of the EPD window owns the keyboard: only the
        // keys of the window itself (quit, full screen) act (D3862).
        if self.epd_layer_open() {
            return match self.app.shortcuts.lookup_in(&input, text_focused) {
                Some(action @ (Action::Quit | Action::FullScreen)) => self.run_action(action),
                _ => Task::none(),
            };
        }
        // The opening report window owns the keyboard: Esc, Ctrl/Cmd+F and
        // F5 and the keys of the program window only (spec M7-2c, D3631).
        // The questions and forms of the EPD window are drawn above it, so
        // they take the keys first (D3681).
        if self.oprep_layer_open() {
            return self.oprep_key(&input, text_focused);
        }
        // The player report window and its dialog own the keyboard too
        // (spec M7-3c).
        if self.preport_layer_open() {
            return self.preport_key(&input, text_focused);
        }
        // The window of the board image is a modal card (spec M7-4c).
        if self.export_image_layer_open() {
            return self.export_image_key(&input, text_focused);
        }
        // The welcome and the tip of the day own the keyboard (spec M9-4).
        if self.start_layer_open() {
            return self.start_key(&input, text_focused);
        }
        // The manual window owns the keyboard too (spec M9-5): Esc, F1,
        // Ctrl/Cmd+F, Alt+arrows and the keys that scroll.
        if self.manual_layer_open() {
            return self.manual_key(&input, text_focused);
        }
        // The maintenance window owns the keyboard (spec M7-1c, D4105).
        if self.maint_layer_open() {
            return self.maint_key(&input, text_focused);
        }
        if self.app.browser.is_some()
            && let Some(task) = self.browser_key(&input, text_focused)
        {
            return task;
        }
        if escape && self.close_notation_popups() {
            return Task::none();
        }
        if escape && self.ui.show_about {
            self.ui.show_about = false;
            return Task::none();
        }
        // Escape cancels a drag of the game list; the key is spent only
        // when the drag was visible (the pointer over another tab), so a
        // plain click never swallows it.
        if escape && self.ui.games.escape_drag() {
            return Task::none();
        }
        // Escape leaves the search field of the list (the widget lets go of
        // the focus itself).
        if escape && self.ui.games.search_focused && self.ui.is_visible(PaneKind::GameList) {
            self.ui.games.search_focused = false;
            return Task::none();
        }
        // The inline boxes of the analysis panel close with Esc, and the
        // key does not go further (D921).
        if escape && let Some(task) = self.analysis_escape() {
            return task;
        }
        // Esc stops an annotation run: the edits made so far stay (D469). It
        // comes after every layer that spends the key for itself (D920): the
        // "Stop" button is the explicit way.
        if escape && self.app.annotation_running() {
            return self.dispatch(mychess_app::Message::Annotate(
                mychess_app::AnnotateMessage::Cancel,
            ));
        }
        // Before the keys of the analysis panel and of the game list, which
        // take a plain Enter for themselves (D2168).
        // A text field of a data panel has the focus: `text_input` does not
        // capture Up, Down, Page Up, Page Down or an Enter without
        // `on_submit`, which would move the game, scroll the list or type
        // over the board.
        let text_focused = text_focused || self.panel_field_focused() || self.epd_field_focused();
        if let Some(task) = self.training_enter(&input, text_focused) {
            return task;
        }
        if let Some(task) = self.analysis_key(&input, text_focused, typed) {
            return task;
        }
        if let Some(task) = self.game_list_key(&input, text_focused) {
            return task;
        }
        // The search field is a text field even when the key was not
        // captured (Tab, Page Up and Page Down are not): D565.
        let text_focused = text_focused
            || (self.ui.games.search_focused && self.ui.is_visible(PaneKind::GameList));
        if let Some(action) = self.app.shortcuts.lookup_in(&input, text_focused) {
            // Behind the About box only the keys of the window itself act
            // (D920): the others would change the game it covers.
            if self.ui.show_about
                && !matches!(action, Action::Quit | Action::FullScreen | Action::Help)
            {
                return Task::none();
            }
            // The keys of the game list are `Always` in the table (the M2
            // contract gives every key with a modifier that scope), but they
            // edit the filter or write flags: they act only with the panel
            // open and no text field focused (D572).
            if is_list_action(action) && (text_focused || !self.ui.is_visible(PaneKind::GameList)) {
                return Task::none();
            }
            // Shift+Tab alone moves the focus of a text field: the shortcut
            // map gives every key with a modifier the scope `Always`
            // (D150), so this one is left out here (D554).
            if text_focused && action == Action::PreviousBase && !input.modifiers.command {
                return Task::none();
            }
            let mut tasks = vec![];
            if self.app.nag_sequence_deadline().is_some() {
                tasks.push(self.dispatch(mychess_app::Message::NagInterrupt));
            }
            tasks.push(self.run_action(action));
            return Task::batch(tasks);
        }
        if text_focused
            || input.modifiers.command
            || input.modifiers.alt
            || self.modal_layer_open()
            || self.ui.analysis.field_focused()
        {
            return Task::none();
        }
        // During a game against an engine nothing is typed into the game:
        // the keys are ignored instead of answered with "locked" (D1484).
        // The same during an online game: typed moves would write into the
        // game the server owns (D1484).
        if self.app.play_live() || self.app.online.phase.is_live() {
            return Task::none();
        }
        // Typing over the board: NAG sequences and the move entry (D181,
        // D199). The character is the one typed, not the key code, so it
        // does not depend on the layout of the keyboard.
        let key = match (input.key, typed) {
            (Key::Named(NamedKey::Enter), _) => TypedKey::Enter,
            (Key::Named(NamedKey::Backspace), _) => {
                return self.dispatch(mychess_app::Message::Game(
                    mychess_app::GameMessage::MoveEntryBackspace,
                ));
            }
            (_, Some(c)) if !c.is_control() && !c.is_whitespace() => TypedKey::Char(c),
            _ => TypedKey::Interrupt,
        };
        let out = self
            .ui
            .dialogs
            .update(DialogMessage::Typed(key, Instant::now()), &self.app);
        self.run_outcome(out, None)
    }

    /// Runs an action: the ones about the window and the panels here, the
    /// rest in the application.
    fn run_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Quit => return self.request_quit(),
            // The Information panel when the layout has one, else the
            // Information tab of the notation panel (D216).
            Action::ToggleGameInfo => {
                return self.toggle_info_tab();
            }
            Action::ShowNotation => {
                if !self.ui.is_visible(PaneKind::Notation) {
                    return self.change_panes(|ui| ui.show_pane(PaneKind::Notation));
                }
            }
            Action::ToggleGameList => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::GameList));
            }
            Action::ToggleAnalysisPane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::Analysis));
            }
            Action::ToggleGraphPane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::Graph));
            }
            Action::ToggleTreePane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::Tree));
            }
            Action::ToggleBookPane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::Book));
            }
            Action::ToggleTablebasePane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::Tablebase));
            }
            Action::ToggleCrosstablePane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::Crosstable));
            }
            Action::ToggleStatsPane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::FilterStats));
            }
            Action::ToggleRatingPane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::RatingGraph));
            }
            Action::TogglePieceTrackerPane => {
                return self.change_panes(|ui| ui.toggle_pane(PaneKind::PieceTracker));
            }
            Action::ToggleEpdPane
            | Action::EpdFirst
            | Action::EpdPrev
            | Action::EpdNext
            | Action::EpdLast => {
                if let Some(task) = self.epd_action(action) {
                    return task;
                }
            }
            // Whatever the way in (key, menu), they need the panel open and
            // the search field not being typed in (D572).
            action if is_list_action(action) && !self.list_panel_ready() => {}
            Action::ListSelectAll => {
                return self.update_games(GameListMessage::Act(ListAction::SelectVisible));
            }
            Action::ListRemoveFromFilter => {
                return self.update_games(GameListMessage::Act(ListAction::RemoveSelected));
            }
            Action::ListToggleDeleted => {
                return self.update_games(GameListMessage::Act(ListAction::ToggleDeleted));
            }
            Action::ListFocusSearch => return self.focus_search(),
            Action::FullScreen => return self.toggle_full_screen(),
            Action::Help => return self.toggle_manual(),
            Action::GotoMove => return self.update_dialog(DialogMessage::OpenGoto),
            // The commands of a game on: closed layers over the board keep
            // them off (M8-12, D3104).
            Action::ResignGame | Action::Rematch if self.modal_layer_open() => {}
            Action::ResignGame => return self.resign_game(),
            Action::Rematch => return self.rematch_game(),
            Action::PlayEngine => return self.update_play(PlayUiMessage::Open),
            Action::TacticalGame => return self.update_play(PlayUiMessage::OpenTactical),
            Action::TacticsTrainer => return self.update_tactics(TacticsUiMessage::Open),
            Action::OpeningTrainer => return self.update_opening(OpeningUiMessage::Open),
            Action::CalvarTrainer => return self.update_calvar(CalvarUiMessage::Open),
            Action::TournamentEngines => {
                return self.update_tournament(TournamentUiMessage::Open);
            }
            Action::OnlineConnect => {
                return self.update_online(OnlineUiMessage::Open(OnlineDialog::Connect));
            }
            Action::EboardDialog => {
                return self.update_eboard(crate::windows::eboard_dialog::EboardUiMessage::Open);
            }
            Action::OnlineSeek => {
                return self.update_online(OnlineUiMessage::Open(OnlineDialog::Seek));
            }
            // Delete asks first, with the count (D190).
            Action::TruncateGame => {
                let cursor = self.app.session.cursor();
                return self.dispatch(app_api::request_strip(
                    &self.app,
                    mychess_app::StripKind::TruncateAfter(cursor),
                ));
            }
            // Their windows arrive with later slices: the comment editor,
            // and the board size (the board fills its panel).
            Action::ShowCommentEditor => return self.toggle_comment_editor(),
            Action::BoardSmaller | Action::BoardLarger => {}
            other => return self.dispatch(app_api::action(other)),
        }
        Task::none()
    }

    /// The dialog and the panel of the game against an engine.
    fn update_play(&mut self, message: PlayUiMessage) -> Task<Message> {
        match message {
            PlayUiMessage::PickBook => {
                let dialog = host::BookFileDialog::new(&self.i18n);
                return Task::perform(dialog.run(), |path| {
                    Message::PlayUi(PlayUiMessage::BookPicked(path))
                });
            }
            PlayUiMessage::TacticalPickFile => {
                let title = self.i18n.tr("tactics-file-dialog-title");
                let filter = self.i18n.tr("tactics-file-filter");
                let all = self.i18n.tr("file-filter-all");
                return Task::perform(tactics_glue::pick_problems(title, filter, all), |path| {
                    Message::PlayUi(PlayUiMessage::TacticalFilePicked(path))
                });
            }
            PlayUiMessage::Open | PlayUiMessage::OpenTactical if self.app.play_live() => {
                self.ui.status_notice = Some(mychess_app::AppNotice::Play(
                    mychess_app::PlayNotice::AlreadyRunning,
                ));
                self.ui.dialogs.status.reset();
                return Task::none();
            }
            _ => {}
        }
        let out = self.ui.dialogs.play.update(message, &self.app);
        self.run_outcome(out, None)
    }

    /// A tick of the clocks (every 100 ms while they run). Only the clocks
    /// and the strips change unless the tick ended the game or the flag
    /// fell, so the full synchronization is skipped (spec M8-1b, D1259).
    fn on_play_tick(&mut self, at: Instant) -> Task<Message> {
        self.ui.dialogs.play.now = at;
        let key = |shell: &Shell| {
            (
                shell.app.session.generation(),
                app_api::play_phase(&shell.app),
                shell.ui.status_notice.is_some(),
            )
        };
        let before = key(self);
        let task = self.dispatch(AppMessage::Play(PlayMessage::Tick(at)));
        if key(self) == before {
            self.ui.dialogs.play.refresh_clocks(&self.app, at);
            return task;
        }
        self.finish_update(task)
    }

    /// The answer of an engine file dialog: the messages for the editor or
    /// the import. A command that is not UTF-8 cannot be kept (rule 9) and
    /// is reported.
    fn engine_file_picked(&mut self, pick: EnginePick, path: Option<PathBuf>) -> Task<Message> {
        let not_utf8 =
            pick == EnginePick::Command && path.as_ref().is_some_and(|p| p.to_str().is_none());
        if not_utf8 {
            self.ui.status_notice = Some(mychess_app::AppNotice::Engine(
                mychess_app::engines::EngineNotice::EntryInvalid,
            ));
            self.ui.dialogs.status.reset();
            return Task::none();
        }
        let dir_empty = self
            .app
            .engine_editor
            .as_ref()
            .is_none_or(|e| e.working_dir.trim().is_empty());
        let tasks: Vec<_> = crate::windows::engines_dialog::picked_messages(pick, path, dir_empty)
            .into_iter()
            .map(|m| self.dispatch(AppMessage::Engine(m)))
            .collect();
        Task::batch(tasks)
    }

    fn toggle_full_screen(&mut self) -> Task<Message> {
        self.ui.fullscreen = !self.ui.fullscreen;
        let mode = if self.ui.fullscreen {
            window::Mode::Fullscreen
        } else {
            window::Mode::Windowed
        };
        window::oldest().and_then(move |id| window::set_mode(id, mode))
    }

    // ---- window and preferences ----

    /// Sends the arrangement of the panels to the preferences (saved with
    /// the next flush).
    fn remember_layout(&mut self) -> Task<Message> {
        let layout = self.ui.layout_to_save();
        if layout.is_valid() {
            self.dispatch(app_api::set_layout(Some(layout)))
        } else {
            Task::none()
        }
    }

    fn window_geometry(size: Size) -> AppMessage {
        app_api::window_geometry(whole_pixels(size.width), whole_pixels(size.height))
    }

    fn on_window_resized(&mut self, size: Size) -> Task<Message> {
        let first = self.ui.window_size.is_none();
        self.ui.window_size = Some(size);
        // The first report is the size the window opened with.
        if first {
            Task::none()
        } else {
            self.dispatch(Shell::window_geometry(size))
        }
    }

    /// Records the window and the panels, then asks the application to quit.
    /// The application saves the preferences and answers `Effect::Exit` when
    /// it is safe (after the "save changes?" question, if any).
    fn request_quit(&mut self) -> Task<Message> {
        // A second request while the books are being saved aborts the run
        // (D1796); while the question is on screen it is ignored.
        if matches!(self.ui.dialogs.book.quit, Some(BookQuit::Saving { .. })) {
            self.ui.dialogs.book.quit = None;
            return Task::none();
        }
        if self.ui.dialogs.book.quit.is_some() {
            return Task::none();
        }
        // EPD files with work that is not saved ask first: the questions of
        // the tournament and of the online game below end in the quit
        // itself, so asking later would skip this one (spec M7-5c, D3866).
        if self.epd_quit_pending() {
            return Task::none();
        }
        // A tournament on is not dropped by a stray click on the close
        // button: its engines would end and the game in progress be lost
        // (the state stays on disk and can be resumed, D1307).
        // The same for an online game: closing the program does not end
        // it, the clock keeps running at the server (D1333).
        if self.app.online.phase.is_live() {
            self.ui.dialogs.online.confirm = Some(crate::windows::online_ui::OnlineConfirm::Quit);
            return Task::none();
        }
        if self.app.tournament_live() {
            self.ui.dialogs.tournament.confirm =
                Some(crate::windows::tournament_ui::TournamentConfirm::Quit);
            return Task::none();
        }
        // Books with changes that are not saved ask last (spec M5-6b).
        self.quit_after_books()
    }

    /// Saves the geometry and the layout and asks the application to quit.
    fn quit_final(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        if let Some(size) = self.ui.window_size {
            tasks.push(self.dispatch(Shell::window_geometry(size)));
        }
        tasks.push(self.remember_layout());
        tasks.push(self.dispatch(app_api::quit()));
        Task::batch(tasks)
    }

    fn on_board_event(&mut self, event: BoardEvent) -> Task<Message> {
        // During a game on FICS the moves go to the server's game (D2613).
        if self.app.fics().playing() {
            return self.on_fics_board_event(&event);
        }
        // During an online game the moves go to the server's game (D1320).
        if self.app.online.phase.is_live() {
            return self.on_online_board_event(&event);
        }
        // During a game against an engine the moves go to the game, which
        // keeps the clocks and the rules (D1241); nothing else is edited.
        if self.app.play_live() {
            return match event {
                BoardEvent::PlayMove { mv, dragged } => {
                    let (Some(from), Some(to)) = (mv.from(), mv.to()) else {
                        return Task::none();
                    };
                    if dragged {
                        self.ui.suppress_next_animation();
                    }
                    self.dispatch(AppMessage::Play(PlayMessage::PlayerMove {
                        from,
                        to,
                        promotion: mv.promotion(),
                        at: Instant::now(),
                    }))
                }
                _ => Task::none(),
            };
        }
        match event {
            BoardEvent::PlayMove { mv, dragged } => {
                if dragged {
                    self.ui.suppress_next_animation();
                }
                self.dispatch(app_api::play_move(mv))
            }
            BoardEvent::PlayNullMove => self.dispatch(app_api::play_null_move()),
            BoardEvent::ToggleMark(mark) => {
                // Marking turns the marks on (Scid `end.tcl:1523`).
                self.options.apply(ViewChange::ShowMarks(true));
                let message = app_api::toggle_mark_message(&self.app, &mark);
                self.dispatch(message)
            }
            BoardEvent::PlayEngineArrow { from, to, rank } => {
                match app_api::play_arrow(&self.app, from, to, rank) {
                    Some(message) => self.dispatch(message),
                    None => Task::none(),
                }
            }
            BoardEvent::Redraw => Task::none(),
        }
    }

    /// The window content.
    pub fn view(&self) -> Element<'_, Message> {
        main_window::view(self)
    }

    /// The window title.
    pub fn title(&self) -> String {
        window_title::window_title(
            &self.i18n,
            app_api::file_name(&self.app).as_deref(),
            app_api::is_modified(&self.app),
            self.app.eboard().phase() == mychess_app::eboard::EboardPhase::Connected,
        )
    }

    /// The badge of the class of the move under the cursor, when the
    /// report has one to show (spec M4-10, D1434): the colour comes from
    /// the tokens of the theme in use.
    pub fn class_badge(&self) -> Option<crate::widgets::board::ClassBadge> {
        let mark = self.app.board_class_mark()?;
        let tokens = theme::Tokens::of(&self.theme());
        Some(crate::widgets::board::ClassBadge {
            square: mark.square,
            glyph: theme::report_colors::class_glyph(mark.class),
            color: theme::report_colors::class_color(&tokens, mark.class),
        })
    }

    /// The iced theme.
    pub fn theme(&self) -> Theme {
        theme::resolve(
            app_api::theme_mode(&self.app),
            self.ui.system_mode,
            app_api::accent(&self.app),
        )
    }

    /// The event sources.
    pub fn subscription(&self) -> Subscription<Message> {
        subscriptions::subscription(self)
    }

    /// Whether the theme preference follows the system.
    pub fn follows_system_theme(&self) -> bool {
        app_api::theme_mode(&self.app) == ThemeMode::System
    }
}

/// The actions that belong to the game list.
fn is_list_action(action: Action) -> bool {
    matches!(
        action,
        Action::ListSelectAll
            | Action::ListRemoveFromFilter
            | Action::ListToggleDeleted
            | Action::ListFocusSearch
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use std::time::Duration;

    use mychess_app::{DiscardChoice, General, Preferences};

    use super::*;
    use crate::i18n::Language;
    use crate::theme::board_theme::BoardThemeId;

    fn sq(name: &str) -> mychess_core::Square {
        name.parse().expect("square")
    }

    fn loaded(prefs: Preferences) -> LoadedPreferences {
        LoadedPreferences {
            prefs,
            issues: Vec::new(),
        }
    }

    /// A shell whose preferences were "loaded" with a fixed language:
    /// `System` would depend on the machine.
    fn shell() -> Shell {
        let mut shell = Shell::new(None);
        let prefs = Preferences {
            general: General {
                language: mychess_app::LanguagePref::EnUs,
                theme: mychess_app::ThemePref::Light,
                ..General::default()
            },
            ..Preferences::default()
        };
        send(
            &mut shell,
            Message::App(app_api::preferences_loaded(loaded(prefs))),
        );
        shell
    }

    fn send(shell: &mut Shell, message: Message) {
        drop(shell.update(message));
    }

    fn play(shell: &mut Shell, from: &str, to: &str) {
        let mv = shell
            .app
            .session
            .position()
            .find_move(sq(from), sq(to), None)
            .expect("legal");
        send(shell, Message::App(app_api::play_move(mv)));
    }

    #[test]
    fn animation_frames_advance_the_clock_and_the_last_one_ends_the_animation() {
        let mut shell = shell();
        play(&mut shell, "e2", "e4");
        assert!(shell.ui.is_animating());
        let t0 = Instant::now();
        let frame = |ms| Message::Ui(UiMessage::AnimationFrame(t0 + Duration::from_millis(ms)));
        send(&mut shell, frame(0));
        send(&mut shell, frame(50));
        assert!(shell.ui.is_animating());
        send(&mut shell, frame(10_000));
        assert!(!shell.ui.is_animating());
    }

    fn key(c: char, modifiers: Modifiers) -> Message {
        Message::Ui(UiMessage::Key {
            input: KeyInput {
                key: Key::Char(c),
                modifiers,
            },
            text_focused: false,
        })
    }

    fn named(key: NamedKey) -> Message {
        Message::Ui(UiMessage::Key {
            input: KeyInput {
                key: Key::Named(key),
                modifiers: Modifiers::NONE,
            },
            text_focused: false,
        })
    }

    /// `[ Board | Information ]`: the default with Moves and Comment closed.
    fn info_only() -> mychess_app::LayoutNode {
        mychess_app::LayoutNode::default_layout()
            .close(PaneKind::Notation)
            .close(PaneKind::Comment)
    }

    #[test]
    fn boot_adopts_the_preloaded_preferences_in_the_first_frame() {
        let prefs = Preferences {
            general: General {
                language: mychess_app::LanguagePref::PtBr,
                theme: mychess_app::ThemePref::Dark,
                ..General::default()
            },
            layout: Some(mychess_app::LayoutNode::default_layout()),
            ..Preferences::default()
        };
        let (shell, _task) = Shell::boot(None, Some(loaded(prefs)));
        assert_eq!(shell.i18n.language(), Language::PtBr);
        assert_eq!(
            shell.theme(),
            theme::resolve(
                ThemeMode::Dark,
                iced::theme::Mode::None,
                theme::AccentId::Mychess
            )
        );
        assert_eq!(shell.title(), "Sem título - MyChess");
        assert_eq!(shell.ui.layout(), mychess_app::LayoutNode::default_layout());
        assert!(shell.ui.is_open(PaneKind::Info));
        assert!(shell.ui.is_open(PaneKind::Notation));
    }

    #[test]
    fn a_path_makes_boot_ask_the_application_to_load_the_preferences() {
        // `Startup` with a path only describes a `LoadPreferences` effect;
        // nothing is read while the task is not run.
        let (shell, _task) = Shell::boot(Some(PathBuf::from("prefs.toml")), None);
        assert!(shell.app.prefs_issues.is_empty());
        let (shell, _task) = Shell::boot(None, None);
        assert!(
            shell.app.prefs_issues.is_empty(),
            "without a path nothing is loaded, and nothing is reported"
        );
    }

    #[test]
    fn a_layout_that_arrives_later_replaces_the_default_one() {
        let mut shell = shell();
        assert_eq!(shell.ui.layout(), mychess_app::LayoutNode::default_layout());
        let prefs = Preferences {
            layout: Some(info_only()),
            ..Preferences::default()
        };
        send(
            &mut shell,
            Message::App(app_api::preferences_loaded(loaded(prefs))),
        );
        assert_eq!(shell.ui.layout(), info_only());
        assert!(!shell.ui.is_open(PaneKind::Notation));
    }

    #[test]
    fn an_invalid_saved_layout_is_ignored() {
        let prefs = Preferences {
            layout: Some(mychess_app::LayoutNode::pane(PaneKind::Info)),
            ..Preferences::default()
        };
        let (shell, _task) = Shell::boot(None, Some(loaded(prefs)));
        assert_eq!(shell.ui.layout(), mychess_app::LayoutNode::default_layout());
    }

    #[test]
    fn menu_choices_change_the_preferences_of_the_application() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::App(app_api::set_theme(ThemeMode::Dark)),
        );
        assert_eq!(
            shell.theme(),
            theme::resolve(
                ThemeMode::Dark,
                iced::theme::Mode::None,
                theme::AccentId::Mychess
            )
        );
        send(
            &mut shell,
            Message::App(app_api::set_language(LanguagePref::PtBr)),
        );
        assert_eq!(shell.i18n.language(), Language::PtBr);
        assert_eq!(shell.title(), "Sem título - MyChess");
        send(
            &mut shell,
            Message::App(app_api::set_board_theme(BoardThemeId::ScidBrown)),
        );
        assert_eq!(shell.app.prefs.board.theme, "scid-brown");
        let look = app_api::appearance(&shell.app, shell.options);
        assert_eq!(look.board.theme, BoardThemeId::ScidBrown);
        send(&mut shell, Message::App(app_api::set_animation_ms(0)));
        assert_eq!(
            app_api::appearance(&shell.app, shell.options).animation_ms,
            0
        );
    }

    #[test]
    fn both_sides_coordinates_are_a_session_option_over_outside() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::Coords(CoordsMode::OutsideBothSides)),
        );
        assert_eq!(
            shell.app.prefs.board.coordinates,
            mychess_app::CoordinatesPref::Outside
        );
        let look = app_api::appearance(&shell.app, shell.options);
        assert_eq!(look.board.coords, CoordsMode::OutsideBothSides);
        send(
            &mut shell,
            Message::Ui(UiMessage::Coords(CoordsMode::Inside)),
        );
        let look = app_api::appearance(&shell.app, shell.options);
        assert_eq!(look.board.coords, CoordsMode::Inside);
        send(
            &mut shell,
            Message::Ui(UiMessage::Coords(CoordsMode::Outside)),
        );
        let look = app_api::appearance(&shell.app, shell.options);
        assert_eq!(look.board.coords, CoordsMode::Outside);
    }

    #[test]
    fn layout_changes_are_sent_to_the_preferences_to_be_saved_with_the_next_flush() {
        let mut shell = shell();
        assert_eq!(shell.app.prefs.layout, None);
        send(
            &mut shell,
            Message::Ui(UiMessage::TogglePane(PaneKind::Notation)),
        );
        assert_eq!(
            shell.app.prefs.layout,
            Some(mychess_app::LayoutNode::default_layout().close(PaneKind::Notation))
        );
        assert!(shell.app.prefs_dirty, "waits for FlushPreferences");
        send(&mut shell, Message::Ui(UiMessage::ResetLayout));
        assert_eq!(shell.app.prefs.layout, None);
        assert!(shell.ui.is_open(PaneKind::Notation));
    }

    #[test]
    fn the_first_window_size_is_not_a_change_but_later_ones_are() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::WindowResized(Size::new(1280.0, 800.0))),
        );
        assert_eq!(shell.app.prefs.window.width, None);
        send(
            &mut shell,
            Message::Ui(UiMessage::WindowResized(Size::new(1000.4, 700.6))),
        );
        assert_eq!(shell.app.prefs.window.width, Some(1000));
        assert_eq!(shell.app.prefs.window.height, Some(701));
        assert!(shell.app.prefs_dirty);
        // Without a preferences file there is nothing to write; the remark
        // about it is only logged, never shown.
        send(&mut shell, Message::Ui(UiMessage::FlushTick));
        assert!(app_api::prefs_remark(&shell.app).is_none());
    }

    #[test]
    fn whole_pixels_never_wraps() {
        assert_eq!(whole_pixels(-5.0), 0);
        assert_eq!(whole_pixels(f32::NAN), 0);
        assert_eq!(whole_pixels(f32::INFINITY), 0);
        assert_eq!(whole_pixels(1e20), u32::from(u16::MAX));
        assert_eq!(whole_pixels(799.5), 800);
    }

    #[test]
    fn keys_go_through_the_shortcut_map_of_the_application() {
        let mut shell = shell();
        send(&mut shell, key('f', Modifiers::COMMAND));
        assert_eq!(shell.ui.orientation, Orientation::Black);
        play(&mut shell, "e2", "e4");
        send(&mut shell, named(NamedKey::Left));
        assert_eq!(shell.app.session.cursor(), shell.app.session.game().root());
        send(&mut shell, named(NamedKey::Right));
        assert_ne!(shell.app.session.cursor(), shell.app.session.game().root());
    }

    #[test]
    fn custom_shortcuts_replace_the_default_ones() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::App(AppMessage::Prefs(mychess_app::PrefsMessage::Set(
                mychess_app::PrefChange::Shortcut {
                    action: "FlipBoard".to_owned(),
                    binding: Some("Command+Shift+Q".to_owned()),
                },
            ))),
        );
        send(&mut shell, key('f', Modifiers::COMMAND));
        assert_eq!(shell.ui.orientation, Orientation::White, "old key is free");
        send(&mut shell, key('q', Modifiers::COMMAND_SHIFT));
        assert_eq!(shell.ui.orientation, Orientation::Black);
    }

    #[test]
    fn keys_that_belong_to_a_text_field_are_not_shortcuts() {
        let mut shell = shell();
        play(&mut shell, "e2", "e4");
        send(
            &mut shell,
            Message::Ui(UiMessage::Key {
                input: KeyInput {
                    key: Key::Named(NamedKey::Left),
                    modifiers: Modifiers::NONE,
                },
                text_focused: true,
            }),
        );
        assert_ne!(shell.app.session.cursor(), shell.app.session.game().root());
    }

    #[test]
    fn actions_of_the_window_are_handled_by_the_gui() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::Action(Action::ToggleGameInfo)),
        );
        assert!(
            shell.ui.is_visible(PaneKind::Info),
            "the action selects the Information tab (M2-6, D1007)"
        );
        send(
            &mut shell,
            Message::Ui(UiMessage::Action(Action::ToggleGameInfo)),
        );
        assert!(shell.ui.is_visible(PaneKind::Notation));
        send(&mut shell, Message::Ui(UiMessage::ShowAbout(true)));
        assert!(shell.ui.show_about);
        send(&mut shell, named(NamedKey::Escape));
        assert!(!shell.ui.show_about, "Escape closes the About box");
        send(&mut shell, Message::Ui(UiMessage::Action(Action::Help)));
        assert!(
            !shell.ui.show_about,
            "F1 no longer opens the About box (D4065)"
        );
        assert!(shell.app.manual().is_open(), "F1 opens the manual");
        send(&mut shell, named(NamedKey::Escape));
        assert!(!shell.app.manual().is_open(), "Escape closes the manual");
        send(
            &mut shell,
            Message::Ui(UiMessage::Action(Action::FullScreen)),
        );
        assert!(shell.ui.fullscreen);
        send(
            &mut shell,
            Message::Ui(UiMessage::Action(Action::FullScreen)),
        );
        assert!(!shell.ui.fullscreen);
    }

    #[test]
    fn a_modified_game_asks_before_new_game_and_cancel_keeps_it() {
        let mut shell = shell();
        play(&mut shell, "e2", "e4");
        assert!(app_api::is_modified(&shell.app));
        send(&mut shell, key('n', Modifiers::COMMAND_SHIFT));
        assert!(app_api::discard_prompt(&shell.app).is_some());
        send(&mut shell, named(NamedKey::Escape));
        assert!(app_api::discard_prompt(&shell.app).is_none());
        assert!(app_api::is_modified(&shell.app), "Escape cancels");
        send(&mut shell, key('n', Modifiers::COMMAND_SHIFT));
        send(
            &mut shell,
            Message::App(app_api::answer_discard(DiscardChoice::DontSave)),
        );
        assert!(app_api::discard_prompt(&shell.app).is_none());
        assert!(
            !app_api::is_modified(&shell.app),
            "the new game replaced it"
        );
    }

    #[test]
    fn the_board_ignores_moves_while_a_question_is_open() {
        let mut shell = shell();
        play(&mut shell, "e2", "e4");
        send(&mut shell, Message::Ui(UiMessage::CloseRequested));
        assert!(app_api::discard_prompt(&shell.app).is_some());
        assert!(!app_api::board_view(&shell.app).accepts_moves);
        // Other keys do nothing while the question is open.
        send(&mut shell, named(NamedKey::Left));
        assert_ne!(shell.app.session.cursor(), shell.app.session.game().root());
        send(
            &mut shell,
            Message::App(app_api::answer_discard(DiscardChoice::Cancel)),
        );
        assert!(app_api::board_view(&shell.app).accepts_moves);
    }

    #[test]
    fn closing_a_clean_window_records_the_layout_and_geometry_before_quitting() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::WindowResized(Size::new(1280.0, 800.0))),
        );
        let split = first_split(&shell);
        send(
            &mut shell,
            Message::Ui(UiMessage::PaneResized(pane_grid::ResizeEvent {
                split,
                ratio: 0.5,
            })),
        );
        send(&mut shell, Message::Ui(UiMessage::CloseRequested));
        assert_eq!(shell.app.prefs.window.width, Some(1280));
        assert!(shell.app.prefs.layout.is_some());
    }

    fn tab_kinds(shell: &Shell, of: PaneKind) -> Vec<PaneKind> {
        shell
            .ui
            .panes
            .iter()
            .find(|(_, g)| g.contains(of))
            .map(|(_, g)| g.tabs.clone())
            .unwrap_or_default()
    }

    fn pane_of(shell: &Shell, kind: PaneKind) -> pane_grid::Pane {
        *shell
            .ui
            .panes
            .iter()
            .find(|(_, g)| g.contains(kind))
            .map(|(pane, _)| pane)
            .expect("the panel is open")
    }

    #[test]
    fn the_close_button_closes_one_tab_and_the_menu_brings_it_back() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::ClosePane(PaneKind::Info)),
        );
        assert_eq!(
            tab_kinds(&shell, PaneKind::Notation),
            [PaneKind::Notation, PaneKind::Comment]
        );
        assert!(shell.app.prefs.layout.is_some(), "the layout is remembered");
        send(
            &mut shell,
            Message::Ui(UiMessage::TogglePane(PaneKind::Info)),
        );
        assert!(shell.ui.is_visible(PaneKind::Info), "reopened and selected");
        assert_eq!(tab_kinds(&shell, PaneKind::Info).len(), 3);
        // The board has no close button: asking anyway changes nothing.
        let before = shell.ui.layout();
        send(
            &mut shell,
            Message::Ui(UiMessage::ClosePane(PaneKind::Board)),
        );
        assert_eq!(shell.ui.layout(), before);
    }

    #[test]
    fn closing_the_last_tab_of_a_group_gives_the_space_to_the_neighbour() {
        let mut shell = shell();
        for kind in [PaneKind::Notation, PaneKind::Info, PaneKind::Comment] {
            send(&mut shell, Message::Ui(UiMessage::ClosePane(kind)));
        }
        assert_eq!(
            shell.ui.layout(),
            mychess_app::LayoutNode::pane(PaneKind::Board)
        );
    }

    #[test]
    fn a_tab_click_selects_it_and_is_saved() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::SelectTab {
                kind: PaneKind::Info,
            }),
        );
        assert!(shell.ui.is_visible(PaneKind::Info));
        let saved = shell.app.prefs.layout.clone().expect("saved");
        assert_eq!(
            saved
                .group_of(PaneKind::Info)
                .map(|(tabs, active)| tabs[active]),
            Some(PaneKind::Info)
        );
        // Comment: the editor opens and the draft is saved on the way out.
        send(
            &mut shell,
            Message::Ui(UiMessage::SelectTab {
                kind: PaneKind::Comment,
            }),
        );
        assert!(shell.ui.comment_visible());
        assert!(shell.modal_layer_open(), "the editor owns the keyboard");
        send(
            &mut shell,
            Message::Ui(UiMessage::SelectTab {
                kind: PaneKind::Notation,
            }),
        );
        assert!(!shell.ui.comment_visible());
    }

    #[test]
    fn hiding_the_analysis_with_its_box_open_frees_the_modal_layer() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)),
        );
        assert!(shell.ui.is_visible(PaneKind::Analysis));
        // Group it with the moves, then select the moves: hidden, not closed.
        let analysis = pane_of(&shell, PaneKind::Analysis);
        let notation = pane_of(&shell, PaneKind::Notation);
        send(
            &mut shell,
            Message::Ui(UiMessage::PaneDragged(pane_grid::DragEvent::Dropped {
                pane: analysis,
                target: pane_grid::Target::Pane(notation, pane_grid::Region::Center),
            })),
        );
        shell.ui.analysis.training = Some((mychess_app::engines::RunId(1), "1.0".to_owned()));
        shell.ui.analysis.hovered = true;
        assert!(shell.modal_layer_open());
        send(
            &mut shell,
            Message::Ui(UiMessage::SelectTab {
                kind: PaneKind::Notation,
            }),
        );
        assert!(shell.ui.is_open(PaneKind::Analysis));
        assert!(!shell.ui.is_visible(PaneKind::Analysis));
        assert!(shell.ui.analysis.training.is_none());
        assert!(!shell.ui.analysis.hovered);
        assert!(!shell.analysis_box_open());
        assert!(!shell.modal_layer_open());
    }

    /// Types `text` in the comment editor, which must be on screen.
    fn type_in_editor(shell: &mut Shell, text: &str) {
        use iced::widget::text_editor::{Action, Edit};
        for c in text.chars() {
            drop(shell.ui.notation.editor.update(
                crate::windows::comment_editor::EditorMessage::Action(Action::Edit(Edit::Insert(
                    c,
                ))),
                &shell.app,
            ));
        }
    }

    /// A shell whose comment editor is on screen with a dirty draft.
    fn shell_with_draft() -> Shell {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::SelectTab {
                kind: PaneKind::Comment,
            }),
        );
        assert!(shell.ui.comment_visible());
        type_in_editor(&mut shell, "draft");
        shell
    }

    fn initial_comment(shell: &Shell) -> Option<String> {
        shell
            .app
            .session
            .game()
            .initial_comment()
            .map(str::to_owned)
    }

    #[test]
    fn every_way_of_hiding_the_comment_saves_the_draft() {
        // Closing the tab.
        let mut shell = shell_with_draft();
        send(
            &mut shell,
            Message::Ui(UiMessage::ClosePane(PaneKind::Comment)),
        );
        assert_eq!(initial_comment(&shell).as_deref(), Some("draft"));
        // Selecting another tab.
        let mut shell = shell_with_draft();
        send(
            &mut shell,
            Message::Ui(UiMessage::SelectTab {
                kind: PaneKind::Notation,
            }),
        );
        assert_eq!(initial_comment(&shell).as_deref(), Some("draft"));
        // The shortcut.
        let mut shell = shell_with_draft();
        send(&mut shell, key('e', Modifiers::COMMAND));
        assert!(!shell.ui.comment_visible());
        assert_eq!(initial_comment(&shell).as_deref(), Some("draft"));
        // Merging another panel over the group of the comment.
        let mut shell = shell_with_draft();
        send(
            &mut shell,
            Message::Ui(UiMessage::DetachTab(PaneKind::Comment)),
        );
        send(
            &mut shell,
            Message::Ui(UiMessage::TogglePane(PaneKind::GameList)),
        );
        let games = pane_of(&shell, PaneKind::GameList);
        let comment = pane_of(&shell, PaneKind::Comment);
        send(
            &mut shell,
            Message::Ui(UiMessage::PaneDragged(pane_grid::DragEvent::Dropped {
                pane: games,
                target: pane_grid::Target::Pane(comment, pane_grid::Region::Center),
            })),
        );
        assert!(!shell.ui.comment_visible());
        assert_eq!(initial_comment(&shell).as_deref(), Some("draft"));
    }

    #[test]
    fn a_drop_that_nests_too_deep_is_refused() {
        let mut shell = shell();
        // Open and detach every panel, dropping each on an edge of the last
        // one, until the depth limit would be exceeded.
        let kinds = [
            PaneKind::GameList,
            PaneKind::Analysis,
            PaneKind::Graph,
            PaneKind::Tree,
        ];
        for kind in kinds {
            send(&mut shell, Message::Ui(UiMessage::TogglePane(kind)));
        }
        for _ in 0..12 {
            let moved = pane_of(&shell, PaneKind::GameList);
            let onto = pane_of(&shell, PaneKind::Tree);
            send(
                &mut shell,
                Message::Ui(UiMessage::PaneDragged(pane_grid::DragEvent::Dropped {
                    pane: moved,
                    target: pane_grid::Target::Pane(
                        onto,
                        pane_grid::Region::Edge(pane_grid::Edge::Left),
                    ),
                })),
            );
            assert!(
                shell.ui.layout().is_valid(),
                "the screen never shows an invalid arrangement"
            );
        }
    }

    #[test]
    fn dropping_on_the_center_merges_and_on_an_edge_splits() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::TogglePane(PaneKind::GameList)),
        );
        assert_eq!(tab_kinds(&shell, PaneKind::GameList), [PaneKind::GameList]);
        let games = pane_of(&shell, PaneKind::GameList);
        let notation = pane_of(&shell, PaneKind::Notation);
        send(
            &mut shell,
            Message::Ui(UiMessage::PaneDragged(pane_grid::DragEvent::Dropped {
                pane: games,
                target: pane_grid::Target::Pane(notation, pane_grid::Region::Center),
            })),
        );
        assert_eq!(
            tab_kinds(&shell, PaneKind::Notation),
            [
                PaneKind::Notation,
                PaneKind::Info,
                PaneKind::Comment,
                PaneKind::GameList
            ]
        );
        assert!(shell.ui.is_visible(PaneKind::GameList));
        assert!(shell.ui.layout().is_valid());
        // Separate it again, then drop it on the edge of the notation.
        send(
            &mut shell,
            Message::Ui(UiMessage::DetachTab(PaneKind::GameList)),
        );
        assert_eq!(tab_kinds(&shell, PaneKind::GameList), [PaneKind::GameList]);
        let games = pane_of(&shell, PaneKind::GameList);
        let notation = pane_of(&shell, PaneKind::Notation);
        send(
            &mut shell,
            Message::Ui(UiMessage::PaneDragged(pane_grid::DragEvent::Dropped {
                pane: games,
                target: pane_grid::Target::Pane(
                    notation,
                    pane_grid::Region::Edge(pane_grid::Edge::Bottom),
                ),
            })),
        );
        assert_eq!(tab_kinds(&shell, PaneKind::GameList), [PaneKind::GameList]);
        assert_eq!(tab_kinds(&shell, PaneKind::Notation).len(), 3);
    }

    #[test]
    fn dropping_on_or_from_the_board_is_ignored() {
        let mut shell = shell();
        let board = pane_of(&shell, PaneKind::Board);
        let notation = pane_of(&shell, PaneKind::Notation);
        let before = shell.ui.layout();
        for (pane, onto) in [(notation, board), (board, notation)] {
            send(
                &mut shell,
                Message::Ui(UiMessage::PaneDragged(pane_grid::DragEvent::Dropped {
                    pane,
                    target: pane_grid::Target::Pane(onto, pane_grid::Region::Center),
                })),
            );
            assert_eq!(shell.ui.layout(), before);
        }
    }

    #[test]
    fn the_tab_menu_detaches_a_tab_and_reset_brings_the_default_back() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Ui(UiMessage::OpenTabMenu {
                kind: PaneKind::Info,
                position: iced::Point::new(10.0, 10.0),
            }),
        );
        assert!(shell.ui.tab_menu.is_some());
        send(
            &mut shell,
            Message::Ui(UiMessage::DetachTab(PaneKind::Info)),
        );
        assert!(shell.ui.tab_menu.is_none());
        assert_eq!(tab_kinds(&shell, PaneKind::Info), [PaneKind::Info]);
        send(&mut shell, Message::Ui(UiMessage::ResetLayout));
        assert_eq!(shell.ui.layout(), mychess_app::LayoutNode::default_layout());
        assert_eq!(shell.app.prefs.layout, None);
    }

    #[test]
    fn the_command_bar_belongs_to_the_first_group_with_the_move_list() {
        let mut shell = shell();
        assert_eq!(
            shell.ui.command_bar_owner(),
            Some(pane_of(&shell, PaneKind::Notation))
        );
        send(
            &mut shell,
            Message::Ui(UiMessage::ClosePane(PaneKind::Notation)),
        );
        assert_eq!(shell.ui.command_bar_owner(), None, "no move list, no bar");
    }

    fn first_split(shell: &Shell) -> pane_grid::Split {
        match shell.ui.panes.layout() {
            pane_grid::Node::Split { id, .. } => *id,
            pane_grid::Node::Pane(_) => panic!("the default layout has splits"),
        }
    }

    #[test]
    fn notices_of_the_application_reach_the_status_line_until_the_next_action() {
        let mut shell = shell();
        let error = mychess_app::FileError::new(
            mychess_app::FileErrorKind::NotFound,
            Some(PathBuf::from("x").join("gone.pgn")),
        );
        send(
            &mut shell,
            Message::App(AppMessage::PgnScanned {
                request: mychess_app::RequestId(99),
                path: None,
                result: Err(error),
            }),
        );
        // A stale request is ignored by the application: nothing shown.
        assert!(shell.ui.status_notice.is_none());
        send(
            &mut shell,
            Message::App(AppMessage::ClipboardRead {
                purpose: mychess_app::ClipboardPurpose::Fen,
                result: Err(mychess_app::ClipboardError::Unavailable),
            }),
        );
        assert!(matches!(
            shell.ui.status_notice,
            Some(mychess_app::AppNotice::Clipboard(
                mychess_app::ClipboardError::Unavailable
            ))
        ));
        assert!(shell.app.notice.is_none(), "the shell takes the remark");
        play(&mut shell, "e2", "e4");
        assert!(shell.ui.status_notice.is_none(), "an action replaces it");
    }

    #[test]
    fn open_file_waits_for_the_dialog_without_blocking_the_shell() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::App(app_api::command(mychess_app::Command::Open)),
        );
        // The dialog is a host effect; the game is untouched meanwhile.
        play(&mut shell, "e2", "e4");
        assert!(app_api::is_modified(&shell.app));
    }

    #[test]
    fn flipping_changes_the_orientation_and_back() {
        let mut shell = shell();
        send(&mut shell, Message::Ui(UiMessage::FlipBoard));
        assert_eq!(shell.ui.orientation, Orientation::Black);
        send(
            &mut shell,
            Message::Ui(UiMessage::Action(Action::FlipBoard)),
        );
        assert_eq!(shell.ui.orientation, Orientation::White);
    }

    #[test]
    fn a_board_move_reaches_the_game_and_marks_it_modified() {
        let mut shell = shell();
        let mv = shell
            .app
            .session
            .position()
            .find_move(sq("e2"), sq("e4"), None)
            .expect("legal");
        send(
            &mut shell,
            Message::Ui(UiMessage::Board(BoardEvent::PlayMove {
                mv,
                dragged: false,
            })),
        );
        assert!(app_api::is_modified(&shell.app));
        assert!(shell.ui.is_animating());
        assert!(shell.title().starts_with('*'));
    }

    #[test]
    fn a_dragged_move_is_not_animated() {
        let mut shell = shell();
        let mv = shell
            .app
            .session
            .position()
            .find_move(sq("e2"), sq("e4"), None)
            .expect("legal");
        send(
            &mut shell,
            Message::Ui(UiMessage::Board(BoardEvent::PlayMove { mv, dragged: true })),
        );
        assert!(!shell.ui.is_animating());
    }

    #[test]
    fn marking_writes_the_comment_and_turns_marks_on() {
        let mut shell = shell();
        shell.options.show_marks = false;
        let mark = mychess_core::BoardMark::arrow(sq("d4"), sq("f6"), "green");
        send(
            &mut shell,
            Message::Ui(UiMessage::Board(BoardEvent::ToggleMark(mark.clone()))),
        );
        assert!(shell.options.show_marks);
        assert_eq!(shell.ui.marks(), [mark]);
    }

    #[test]
    fn theme_follows_the_system_only_in_system_mode() {
        let mut shell = Shell::new(None);
        send(
            &mut shell,
            Message::App(app_api::set_theme(ThemeMode::System)),
        );
        send(
            &mut shell,
            Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)),
        );
        assert!(shell.follows_system_theme());
        assert_eq!(
            shell.theme(),
            theme::resolve(
                ThemeMode::Dark,
                iced::theme::Mode::None,
                theme::AccentId::Mychess
            )
        );
        send(
            &mut shell,
            Message::App(app_api::set_theme(ThemeMode::Light)),
        );
        assert!(!shell.follows_system_theme());
        assert_eq!(
            shell.theme(),
            theme::resolve(
                ThemeMode::Light,
                iced::theme::Mode::None,
                theme::AccentId::Mychess
            )
        );
    }

    #[test]
    fn dialogs_stay_open_until_the_user_answers() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::App(app_api::command(mychess_app::Command::SetupBoard)),
        );
        assert!(shell.app.setup.is_some(), "no automatic cancel any more");
        assert_eq!(
            ActiveDialog::of(&shell.app, &shell.ui.dialogs),
            Some(ActiveDialog::Setup)
        );
        send(&mut shell, named(NamedKey::Escape));
        assert!(shell.app.setup.is_none(), "Esc cancels the dialog");
        // The shortcut opens it again.
        send(&mut shell, key('b', Modifiers::COMMAND_SHIFT));
        assert!(shell.app.setup.is_some());
    }

    #[test]
    fn the_replace_prompt_waits_for_an_answer_and_five_keys_answer_it() {
        let mut shell = shell();
        play(&mut shell, "e2", "e4");
        play(&mut shell, "e7", "e5");
        send(
            &mut shell,
            Message::App(app_api::navigate(mychess_app::Nav::Start)),
        );
        play(&mut shell, "d2", "d4");
        assert!(app_api::session_prompt(&shell.app).is_some());
        assert!(
            !app_api::board_view(&shell.app).accepts_moves || shell.app.session.prompt().is_some()
        );
        send(&mut shell, named(NamedKey::Escape));
        assert!(app_api::session_prompt(&shell.app).is_none(), "Esc cancels");
        play(&mut shell, "d2", "d4");
        send(&mut shell, key('v', Modifiers::NONE));
        assert!(app_api::session_prompt(&shell.app).is_none());
        let root = shell.app.session.game().root();
        assert_eq!(
            shell.app.session.game().children(root).map(<[_]>::len),
            Ok(2)
        );
    }

    #[test]
    fn a_multi_game_file_opens_the_chooser_and_waits_for_the_user() {
        let mut shell = shell();
        let games = "[White \"A\"]\n\n1. e4 *\n\n[White \"B\"]\n\n1. d4 *\n";
        send(
            &mut shell,
            Message::App(AppMessage::Import(mychess_app::ImportMessage::Open)),
        );
        send(
            &mut shell,
            Message::App(AppMessage::Import(mychess_app::ImportMessage::TextEdited(
                games.to_owned(),
            ))),
        );
        let effects = mychess_app::update(
            &mut shell.app,
            AppMessage::Import(mychess_app::ImportMessage::Import),
        );
        for effect in effects {
            let answer = crate::test_support::run_effect(effect);
            send(&mut shell, Message::App(answer));
        }
        assert!(shell.app.chooser.is_some(), "the chooser stays open");
        assert_eq!(
            ActiveDialog::of(&shell.app, &shell.ui.dialogs),
            Some(ActiveDialog::Chooser)
        );
        send(&mut shell, named(NamedKey::Escape));
        assert!(shell.app.chooser.is_none());
    }

    #[test]
    fn the_chess960_dialog_starts_a_game_and_blocks_the_board_meanwhile() {
        let mut shell = shell();
        send(&mut shell, Message::Dialog(DialogMessage::OpenChess960));
        assert!(shell.modal_layer_open());
        send(
            &mut shell,
            Message::Dialog(DialogMessage::Chess960Edited("0".to_owned())),
        );
        send(&mut shell, named(NamedKey::Enter));
        assert!(!shell.modal_layer_open());
        let position = shell.app.session.position();
        assert_eq!(position.chess960_number(), Some(0));
        assert_eq!(
            shell.app.session.game().variant(),
            mychess_core::Variant::Chess960
        );
    }

    #[test]
    fn go_to_move_opens_a_dialog_and_enter_navigates() {
        let mut shell = shell();
        for (a, b) in [("e2", "e4"), ("e7", "e5"), ("g1", "f3")] {
            play(&mut shell, a, b);
        }
        send(&mut shell, Message::Ui(UiMessage::Action(Action::GotoMove)));
        assert!(shell.ui.dialogs.goto.is_some());
        send(
            &mut shell,
            Message::Dialog(DialogMessage::GotoEdited("1".to_owned())),
        );
        send(&mut shell, named(NamedKey::Enter));
        assert!(shell.ui.dialogs.goto.is_none());
        let cursor = shell.app.session.cursor();
        assert_eq!(shell.app.session.game().ply(cursor).ok(), Some(1));
    }

    #[test]
    fn typing_over_the_board_feeds_the_move_entry_and_an_exact_move_is_played() {
        let mut shell = shell();
        let typed = |c: char| {
            Message::Dialog(DialogMessage::Key {
                input: KeyInput {
                    key: Key::Char(c),
                    modifiers: Modifiers::NONE,
                },
                text_focused: false,
                typed: Some(c),
            })
        };
        send(&mut shell, typed('e'));
        assert_eq!(shell.app.session.move_entry().text(), "e");
        send(&mut shell, typed('4'));
        assert!(app_api::is_modified(&shell.app), "e4 is the only candidate");
        assert_eq!(shell.app.session.move_entry().text(), "");
        // Enter with a half-typed move that is ambiguous plays nothing.
        send(&mut shell, typed('N'));
        assert_eq!(shell.app.session.move_entry().text(), "N");
        send(
            &mut shell,
            Message::Dialog(DialogMessage::Key {
                input: KeyInput {
                    key: Key::Named(NamedKey::Enter),
                    modifiers: Modifiers::NONE,
                },
                text_focused: false,
                typed: None,
            }),
        );
        assert_eq!(shell.app.session.move_entry().text(), "N");
    }

    #[test]
    fn keys_typed_in_a_text_field_do_not_reach_the_move_entry() {
        let mut shell = shell();
        send(
            &mut shell,
            Message::Dialog(DialogMessage::Key {
                input: KeyInput {
                    key: Key::Char('e'),
                    modifiers: Modifiers::NONE,
                },
                text_focused: true,
                typed: Some('e'),
            }),
        );
        assert_eq!(shell.app.session.move_entry().text(), "");
    }

    // ---- engine management (spec M4-3) ----

    fn engine_notice(shell: &Shell) -> Option<mychess_app::engines::EngineNotice> {
        match &shell.ui.status_notice {
            Some(mychess_app::AppNotice::Engine(notice)) => Some(notice.clone()),
            _ => None,
        }
    }

    fn key_with(key: Key, modifiers: Modifiers, text_focused: bool) -> Message {
        Message::Ui(UiMessage::Key {
            input: KeyInput { key, modifiers },
            text_focused,
        })
    }

    #[test]
    fn space_toggles_the_analysis_only_when_no_text_field_has_the_focus() {
        use mychess_app::engines::EngineNotice;
        let mut shell = shell();
        send(
            &mut shell,
            key_with(Key::Named(NamedKey::Space), Modifiers::NONE, true),
        );
        assert_eq!(engine_notice(&shell), None, "a text field keeps the space");
        send(
            &mut shell,
            key_with(Key::Named(NamedKey::Space), Modifiers::NONE, false),
        );
        assert_eq!(engine_notice(&shell), Some(EngineNotice::NoEngineRunning));
    }

    #[test]
    fn a_function_key_without_an_engine_says_so_and_the_shortcut_opens_the_dialog() {
        use mychess_app::engines::{EngineNotice, Hotkey};
        let mut shell = shell();
        send(
            &mut shell,
            key_with(Key::Named(NamedKey::F(3)), Modifiers::NONE, false),
        );
        assert_eq!(
            engine_notice(&shell),
            Some(EngineNotice::NoEngineOnKey(Hotkey::F3))
        );
        let open = Modifiers {
            command: true,
            shift: true,
            alt: false,
        };
        send(&mut shell, key_with(Key::Char('e'), open, false));
        assert!(shell.app.engine_dialog.is_some());
        assert_eq!(
            ActiveDialog::of(&shell.app, &shell.ui.dialogs),
            Some(ActiveDialog::Engines)
        );
    }

    #[test]
    fn the_engines_dialog_takes_the_keys_and_escape_closes_the_top_layer() {
        use mychess_app::engines::{EngineDraft, EngineMessage};
        let mut shell = shell();
        let a = shell
            .app
            .engines
            .add(EngineDraft::new("Alpha", "/bin/a"))
            .expect("add");
        let b = shell
            .app
            .engines
            .add(EngineDraft::new("Beta", "/bin/b"))
            .expect("add");
        send(
            &mut shell,
            Message::App(AppMessage::Engine(EngineMessage::OpenDialog)),
        );
        let selected = |s: &Shell| s.app.engine_dialog.as_ref().and_then(|d| d.selected);
        assert_eq!(selected(&shell), Some(a));
        // Typing a letter jumps to the engine; the move entry stays empty.
        send(&mut shell, key('b', Modifiers::NONE));
        assert_eq!(selected(&shell), Some(b));
        assert_eq!(shell.app.session.move_entry().text(), "");
        send(&mut shell, named(NamedKey::Up));
        assert_eq!(selected(&shell), Some(a));
        // New opens the editor on top; Escape closes it, then the dialog.
        send(
            &mut shell,
            Message::App(AppMessage::Engine(EngineMessage::New)),
        );
        assert_eq!(
            ActiveDialog::of(&shell.app, &shell.ui.dialogs),
            Some(ActiveDialog::EngineEditor)
        );
        send(&mut shell, named(NamedKey::Escape));
        assert!(shell.app.engine_editor.is_none() && shell.app.engine_dialog.is_some());
        send(&mut shell, named(NamedKey::Escape));
        assert!(shell.app.engine_dialog.is_none());
    }

    #[test]
    fn a_picked_engine_command_fills_the_editor() {
        use mychess_app::engines::{EngineDraft, EngineMessage};
        let mut shell = shell();
        shell
            .app
            .engines
            .add(EngineDraft::new("A", "/a"))
            .expect("add");
        send(
            &mut shell,
            Message::App(AppMessage::Engine(EngineMessage::OpenDialog)),
        );
        send(
            &mut shell,
            Message::App(AppMessage::Engine(EngineMessage::New)),
        );
        let path = PathBuf::from("engines").join("fish");
        send(
            &mut shell,
            Message::Ui(UiMessage::EngineFilePicked(
                EnginePick::Command,
                Some(path.clone()),
            )),
        );
        let editor = shell.app.engine_editor.as_ref().expect("editor");
        assert_eq!(editor.command, path.to_string_lossy());
        assert_eq!(editor.working_dir, "engines");
        send(
            &mut shell,
            Message::Ui(UiMessage::EngineFilePicked(EnginePick::Command, None)),
        );
        assert!(
            shell.app.engine_editor.is_some(),
            "cancelled: nothing changes"
        );
    }
}
