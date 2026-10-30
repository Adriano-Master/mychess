//! The visual state of the dialogs and its own `update` (spec M2-4, D186,
//! D187).
//!
//! The *domain* state of most dialogs lives in the application (the tags
//! draft, the set-up board, the import text, the game chooser, the prompts
//! of the session). What is here is only what the screen needs: the text of
//! "go to move", the highlighted suggestion, a mouse press on the set-up
//! board, the editor of the import text and the scroll of the game list.
//!
//! [`DialogUi::update`] and [`DialogUi::key`] never touch the application:
//! they return the messages to send in an [`Outcome`].

use std::time::Instant;

use iced::widget::text_editor;
use mychess_app::engines::{
    EditorMessage as EngineEditorMessage, EngineMessage, OptionsMessage, StartMode,
};
use mychess_app::{
    AnnotateMessage, AppState, BookmarksMessage, DiscardChoice, EcoMessage, EditorMessage,
    GameInfoMessage, GameMessage, ImportMessage, Key, KeyInput, Message as AppMessage, Modifiers,
    NagKey, NamedKey, Nav, SetupMessage, TagField,
};
use mychess_core::Square;

use crate::app_api;
use crate::widgets::autocomplete::{self, MAX_ROWS};
use crate::widgets::board::pieces::PieceSet;
use crate::windows::advanced_search_dialog::{self as advanced_dialog, AdvancedSearchUi};
use crate::windows::calvar_dialog::{self as calvar_dialog, CalvarUi};
use crate::windows::chess960_dialog::{CHESS960_FIELD_ID, Chess960Draft};
use crate::windows::mask::MaskUi;
use crate::windows::mask::dialogs as mask_dialogs;
use crate::windows::mask::state::MaskDialog;
use crate::windows::online_ui::{OnlineActive, OnlineUi};
use crate::windows::opening_trainer_dialog::{self as opening_dialog, OpeningUi};
use crate::windows::pattern_search_dialog::{
    self as pattern_dialog, PatternDialog, PatternSearchUi,
};
use crate::windows::play_dialog::{PlayUi, PlayUiMessage};
use crate::windows::play_panel::{self, PlayBox};
use crate::windows::player_tools::{PlayerToolsUi, ToolWindow};
use crate::windows::prompts;
use crate::windows::tactics_dialog::{self as tactics_dialog, TacticsUi};
use crate::windows::tournament_ui::{TournamentUi, TournamentUiMessage};

/// Height of a row of the game chooser, in logical pixels.
pub const CHOOSER_ROW_HEIGHT: f32 = 26.0;
/// Height of the list of the game chooser.
pub const CHOOSER_LIST_HEIGHT: f32 = 340.0;

/// Id of the "go to move" field.
pub const GOTO_FIELD_ID: &str = "goto-field";
/// Id of the import text editor.
pub const PASTE_EDITOR_ID: &str = "paste-editor";
/// Id of the filter field of the game chooser.
pub const CHOOSER_FILTER_ID: &str = "chooser-filter";
/// Id of the list of the game chooser.
pub const CHOOSER_LIST_ID: &str = "chooser-list";
/// Id of the FEN field of the set-up dialog.
pub const SETUP_FEN_ID: &str = "setup-fen";

/// The dialog that has the keyboard, if any. Only one is shown at a time;
/// the order of the checks in `of` is the order of precedence (a game list
/// opened by an import covers the import dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveDialog {
    /// "Save the changes?".
    Discard,
    /// "Replace the file?".
    Overwrite,
    /// "Copy N games from X to Y?" (spec M3-4d).
    CopyGames,
    /// A question of the editing session.
    Prompt,
    /// Set up position.
    Setup,
    /// Game information (the tags).
    Info,
    /// Paste PGN.
    Import,
    /// "Importing PGN files": progress and report (spec M3-8).
    ImportFile,
    /// "Export games": options, progress and report (spec M3-9).
    ExportGames,
    /// The merge of a game into the current one (spec M7-6c).
    Merge,
    /// Go to move number.
    Goto,
    /// "New Chess960 game" (spec M9-6).
    Chess960,
    /// The list of the games of a file.
    Chooser,
    /// The variation picker.
    Variations,
    /// The bookmark editor (spec M3-6, D678).
    Bookmarks,
    /// The "Configure annotation" dialog (spec M4-6).
    Annotate,
    /// The ECO browser (spec M5-7b).
    EcoBrowser,
    /// "ECO-classify games" (spec M5-7b).
    EcoClassify,
    /// "Find novelty" (spec M5-7b).
    EcoNovelty,
    /// "Play against the engine" (spec M8-1b).
    PlayConfig,
    /// "Resign / restart / quit the game?" in the middle of a game (D1480).
    PlayConfirm,
    /// The question of the book or the line of a game against the engine.
    PlayAsk,
    /// The end of a game against the engine.
    PlayEnd,
    /// The engine of the game was lost.
    PlayLost,
    /// The form of the tournament between engines (spec M8-2b).
    TournamentForm,
    /// The list of tournaments that can be resumed.
    TournamentResume,
    /// A confirmation of the tournament.
    TournamentConfirm,
    /// The question about a participant that failed too often.
    TournamentDecision,
    /// "Connect to Lichess" (spec M8-3c).
    OnlineConnect,
    /// "Find a game" on Lichess, or the wait for an opponent.
    OnlineSeek,
    /// "Challenge a user", or the wait for the answer.
    OnlineChallenge,
    /// The challenges received.
    OnlineIncoming,
    /// "Watch a game" (spec M8-3d).
    OnlineWatch,
    /// The piece of a premove that promotes (spec D3013).
    OnlinePromotion,
    /// A confirmation of the online play.
    OnlineConfirm,
    /// "Resign?" in a game on FICS (D3109).
    FicsConfirm,
    /// The "Engines" dialog (spec M4-3).
    Engines,
    /// The engine editor, over the Engines dialog.
    EngineEditor,
    /// The UCI options, over the Engines dialog or the editor.
    EngineOptions,
    /// The question that confirms the deletion of an engine.
    EngineDelete,
    /// The appearance window (spec M9-3).
    Appearance,
    /// "Save the opening books before quitting?" (spec M5-6b, D1792).
    BookQuit,
    /// The configuration of the tactics training (spec M8-4c).
    TacticsSetup,
    /// The configuration of the opening trainer (spec M8-5c).
    OpeningSetup,
    /// The configuration of the calculation exercise (spec M8-6c).
    CalvarSetup,
    /// The advanced searches (spec M6-1c).
    AdvancedSearch,
    /// The player finder (spec M6-3c).
    PlayerFinder,
    /// The tournament finder (spec M6-3c).
    TournamentFinder,
    /// The player information (spec M6-3c).
    PlayerInfo,
    /// The name checker (spec M6-3c).
    SpellCheck,
    /// "Add Elo ratings" (spec M6-3c).
    AddElo,
    /// The search for tactics and patterns (spec M6-2d).
    PatternSearch,
    /// The question about an expensive search for patterns (D3285).
    PatternCost,
    /// The sounds window (spec M8-11).
    Sound,
    /// The FICS window (spec M8-9c).
    Fics,
    /// The electronic board window (spec M8-10c).
    Eboard,
    /// The "Configure report" dialog (spec M4-10, D2100).
    ReportConfig,
    /// "Save the mask?" (spec M5-4c, D1817).
    MaskPrompt,
    /// "Fill the mask with the line" (spec M5-4c, D1826).
    MaskFillLine,
    /// The comment editor of the masks (spec M5-4c).
    MaskComment,
    /// The search of the mask (spec M5-4c, D1832).
    MaskSearch,
    /// The outline of the mask (spec M5-4c, D1833).
    MaskOutline,
}

impl ActiveDialog {
    /// The dialog on top for this state.
    #[allow(clippy::too_many_lines)] // one flat chain of the layers, by priority
    pub fn of(app: &AppState, ui: &DialogUi) -> Option<ActiveDialog> {
        // The question about the books is the last step of a quit: it is
        // never left behind another dialog (D1797).
        if ui.book.quit == Some(crate::windows::book_panel::BookQuit::Asking) {
            Some(ActiveDialog::BookQuit)
        } else if app.discard_prompt.is_some() {
            Some(ActiveDialog::Discard)
        } else if app.overwrite_prompt.is_some() {
            Some(ActiveDialog::Overwrite)
        } else if app.copy_prompt.is_some() {
            Some(ActiveDialog::CopyGames)
        } else if app.mask_prompt().is_some() {
            Some(ActiveDialog::MaskPrompt)
        } else if app_api::session_prompt(app).is_some() {
            Some(ActiveDialog::Prompt)
        } else if app.bookmark_editor.is_some() {
            Some(ActiveDialog::Bookmarks)
        } else if app.annotate_dialog.is_some() {
            Some(ActiveDialog::Annotate)
        } else if crate::windows::merge_dialog::is_open(app) {
            Some(ActiveDialog::Merge)
        } else if app.report.config_dialog.is_some() {
            Some(ActiveDialog::ReportConfig)
        } else if let Some(window) = app.eco_window() {
            Some(match window {
                mychess_app::EcoWindow::Browser => ActiveDialog::EcoBrowser,
                mychess_app::EcoWindow::Classify => ActiveDialog::EcoClassify,
                mychess_app::EcoWindow::Novelty => ActiveDialog::EcoNovelty,
            })
        } else if let Some(engines) = app.engine_dialog.as_ref() {
            Some(if engines.options.is_some() {
                ActiveDialog::EngineOptions
            } else if engines.confirm_delete.is_some() {
                ActiveDialog::EngineDelete
            } else if app.engine_editor.is_some() {
                ActiveDialog::EngineEditor
            } else {
                ActiveDialog::Engines
            })
        } else if let Some(active) = ui.online.active() {
            Some(match active {
                OnlineActive::Connect => ActiveDialog::OnlineConnect,
                OnlineActive::Seek => ActiveDialog::OnlineSeek,
                OnlineActive::Challenge => ActiveDialog::OnlineChallenge,
                OnlineActive::Incoming => ActiveDialog::OnlineIncoming,
                OnlineActive::Watch => ActiveDialog::OnlineWatch,
                OnlineActive::Promotion => ActiveDialog::OnlinePromotion,
                OnlineActive::Confirm => ActiveDialog::OnlineConfirm,
            })
        } else if ui.fics.confirm_resign {
            Some(ActiveDialog::FicsConfirm)
        } else if ui.tournament.confirm.is_some() {
            Some(ActiveDialog::TournamentConfirm)
        } else if ui.tournament.decision_visible(app) {
            Some(ActiveDialog::TournamentDecision)
        } else if ui.tournament.resume.is_some() {
            Some(ActiveDialog::TournamentResume)
        } else if ui.tournament.draft.is_some() {
            Some(ActiveDialog::TournamentForm)
        } else if ui.play.confirm.is_some() {
            Some(ActiveDialog::PlayConfirm)
        } else if let Some(kind) = ui.play_box(app) {
            Some(match kind {
                PlayBox::Ask => ActiveDialog::PlayAsk,
                PlayBox::End => ActiveDialog::PlayEnd,
                PlayBox::Lost => ActiveDialog::PlayLost,
            })
        } else if ui.play.draft.is_some() {
            Some(ActiveDialog::PlayConfig)
        } else if let Some(dialog) = ui.mask.dialog(app) {
            Some(match dialog {
                MaskDialog::Prompt => ActiveDialog::MaskPrompt,
                MaskDialog::FillLine => ActiveDialog::MaskFillLine,
                MaskDialog::Comment => ActiveDialog::MaskComment,
                MaskDialog::Search => ActiveDialog::MaskSearch,
                MaskDialog::Outline => ActiveDialog::MaskOutline,
            })
        } else if app.tactics_dialog_open() {
            Some(ActiveDialog::TacticsSetup)
        } else if app.opening_dialog_open() {
            Some(ActiveDialog::OpeningSetup)
        } else if app.calvar_dialog_open() {
            Some(ActiveDialog::CalvarSetup)
        } else if let Some(dialog) = ui.pattern.dialog(app) {
            Some(match dialog {
                PatternDialog::Window => ActiveDialog::PatternSearch,
                PatternDialog::Cost => ActiveDialog::PatternCost,
            })
        } else if let Some(window) = ui.player_tools.top() {
            Some(match window {
                ToolWindow::PlayerFinder => ActiveDialog::PlayerFinder,
                ToolWindow::TournamentFinder => ActiveDialog::TournamentFinder,
                ToolWindow::PlayerInfo => ActiveDialog::PlayerInfo,
                ToolWindow::SpellCheck => ActiveDialog::SpellCheck,
                ToolWindow::AddElo => ActiveDialog::AddElo,
            })
        } else if app.advanced_search().is_open() {
            Some(ActiveDialog::AdvancedSearch)
        } else if ui.eboard.is_some() {
            Some(ActiveDialog::Eboard)
        } else if app.sound_dialog_open() {
            Some(ActiveDialog::Sound)
        } else if app.fics().window_open {
            Some(ActiveDialog::Fics)
        } else if app.chooser.is_some() {
            Some(ActiveDialog::Chooser)
        } else if app.setup.is_some() {
            Some(ActiveDialog::Setup)
        } else if app.info_dialog.is_some() {
            Some(ActiveDialog::Info)
        } else if app.import.is_some() {
            Some(ActiveDialog::Import)
        } else if app.file_import_view().is_some() {
            Some(ActiveDialog::ImportFile)
        } else if app.games_export_view().is_some() {
            Some(ActiveDialog::ExportGames)
        } else if ui.goto.is_some() {
            Some(ActiveDialog::Goto)
        } else if ui.chess960.is_some() {
            Some(ActiveDialog::Chess960)
        } else if app.session.chooser().is_some() {
            Some(ActiveDialog::Variations)
        } else if ui.appearance.is_some() {
            Some(ActiveDialog::Appearance)
        } else {
            None
        }
    }

    /// Whether the dialog stays when an online game is entered (M8-3,
    /// D4520): questions about the data of the user (save, overwrite, copy,
    /// the session prompts, the mask and tournament questions, the quit of
    /// the books), jobs that run (import, export) and the windows that only
    /// their own buttons close.
    pub fn survives_online_entry(self, app: &AppState) -> bool {
        match self {
            ActiveDialog::Discard
            | ActiveDialog::Overwrite
            | ActiveDialog::CopyGames
            | ActiveDialog::Prompt
            | ActiveDialog::BookQuit
            | ActiveDialog::MaskPrompt
            | ActiveDialog::TournamentConfirm
            | ActiveDialog::TournamentDecision
            | ActiveDialog::PlayLost
            | ActiveDialog::PlayerFinder
            | ActiveDialog::TournamentFinder
            | ActiveDialog::PlayerInfo
            | ActiveDialog::SpellCheck
            | ActiveDialog::AddElo => true,
            ActiveDialog::ImportFile => import_running(app),
            ActiveDialog::ExportGames => {
                export_running(app)
                    || crate::windows::export_games_dialog::escape_confirm_message(app).is_some()
            }
            ActiveDialog::AdvancedSearch => app.advanced_search().is_running(),
            ActiveDialog::PatternSearch => app.pattern_search().is_running(),
            _ => false,
        }
    }

    /// The window of the player tools this dialog is, if it is one.
    pub fn tool_window(self) -> Option<ToolWindow> {
        match self {
            ActiveDialog::PlayerFinder => Some(ToolWindow::PlayerFinder),
            ActiveDialog::TournamentFinder => Some(ToolWindow::TournamentFinder),
            ActiveDialog::PlayerInfo => Some(ToolWindow::PlayerInfo),
            ActiveDialog::SpellCheck => Some(ToolWindow::SpellCheck),
            ActiveDialog::AddElo => Some(ToolWindow::AddElo),
            _ => None,
        }
    }

    /// The id of the container of the dialog (the focus scope).
    pub fn box_id(self) -> &'static str {
        match self {
            ActiveDialog::Discard => crate::windows::dialogs::DISCARD_BOX_ID,
            ActiveDialog::Overwrite => crate::windows::dialogs::OVERWRITE_BOX_ID,
            ActiveDialog::CopyGames => crate::windows::dialogs::COPY_BOX_ID,
            ActiveDialog::Prompt => prompts::REPLACE_BOX_ID,
            ActiveDialog::Setup => crate::windows::setup_board::SETUP_BOX_ID,
            ActiveDialog::Info => crate::windows::header_dialog::HEADER_BOX_ID,
            ActiveDialog::Import => crate::windows::paste_pgn::PASTE_BOX_ID,
            ActiveDialog::ImportFile => crate::windows::import_file_dialog::IMPORT_FILE_BOX_ID,
            ActiveDialog::ExportGames => crate::windows::export_games_dialog::EXPORT_GAMES_BOX_ID,
            ActiveDialog::Merge => crate::windows::merge_dialog::MERGE_BOX_ID,
            ActiveDialog::Goto => crate::windows::goto_move::GOTO_BOX_ID,
            ActiveDialog::Chess960 => crate::windows::chess960_dialog::CHESS960_BOX_ID,
            ActiveDialog::Chooser => crate::windows::game_chooser::CHOOSER_BOX_ID,
            ActiveDialog::Variations => crate::windows::variation_picker::VARIATIONS_BOX_ID,
            ActiveDialog::Bookmarks => crate::windows::bookmark_editor::BOOKMARKS_BOX_ID,
            ActiveDialog::Annotate => crate::windows::annotate_dialog::ANNOTATE_BOX_ID,
            ActiveDialog::ReportConfig => crate::windows::report_dialog::REPORT_CONFIG_BOX_ID,
            ActiveDialog::EcoBrowser => crate::windows::eco_browser::ECO_BROWSER_BOX_ID,
            ActiveDialog::EcoClassify => crate::windows::eco_dialogs::ECO_CLASSIFY_BOX_ID,
            ActiveDialog::EcoNovelty => crate::windows::eco_dialogs::ECO_NOVELTY_BOX_ID,
            ActiveDialog::PlayConfig => crate::windows::play_dialog::PLAY_BOX_ID,
            ActiveDialog::PlayConfirm => play_panel::PLAY_CONFIRM_BOX_ID,
            ActiveDialog::PlayAsk => play_panel::PLAY_ASK_BOX_ID,
            ActiveDialog::PlayEnd => play_panel::PLAY_END_BOX_ID,
            ActiveDialog::PlayLost => play_panel::PLAY_LOST_BOX_ID,
            ActiveDialog::TournamentForm => crate::windows::tournament_dialog::TOURNAMENT_BOX_ID,
            ActiveDialog::TournamentResume => {
                crate::windows::tournament_dialog::TOURNAMENT_RESUME_BOX_ID
            }
            ActiveDialog::TournamentConfirm => {
                crate::windows::tournament_dialog::TOURNAMENT_CONFIRM_BOX_ID
            }
            ActiveDialog::TournamentDecision => {
                crate::windows::tournament_dialog::TOURNAMENT_DECISION_BOX_ID
            }
            ActiveDialog::OnlineConnect => crate::windows::online_dialog::ONLINE_CONNECT_BOX_ID,
            ActiveDialog::OnlineSeek => crate::windows::online_dialog::ONLINE_SEEK_BOX_ID,
            ActiveDialog::OnlineChallenge => crate::windows::online_dialog::ONLINE_CHALLENGE_BOX_ID,
            ActiveDialog::OnlineIncoming => crate::windows::online_dialog::ONLINE_INCOMING_BOX_ID,
            ActiveDialog::OnlineWatch => crate::windows::online_dialog::ONLINE_WATCH_BOX_ID,
            ActiveDialog::OnlinePromotion => crate::windows::online_dialog::ONLINE_PROMOTION_BOX_ID,
            ActiveDialog::OnlineConfirm => crate::windows::online_dialog::ONLINE_CONFIRM_BOX_ID,
            ActiveDialog::Engines => crate::windows::engines_dialog::ENGINES_BOX_ID,
            ActiveDialog::EngineEditor => crate::windows::engine_editor::EDITOR_BOX_ID,
            ActiveDialog::EngineOptions => crate::windows::engine_options::OPTIONS_BOX_ID,
            ActiveDialog::EngineDelete => crate::windows::engines_dialog::ENGINES_DELETE_BOX_ID,
            ActiveDialog::Appearance => crate::windows::appearance_view::APPEARANCE_BOX_ID,
            ActiveDialog::BookQuit => crate::windows::book_panel::BOOK_QUIT_BOX_ID,
            ActiveDialog::TacticsSetup => tactics_dialog::TACTICS_BOX_ID,
            ActiveDialog::OpeningSetup => opening_dialog::OPENING_BOX_ID,
            ActiveDialog::CalvarSetup => calvar_dialog::CALVAR_BOX_ID,
            ActiveDialog::PlayerFinder => ToolWindow::PlayerFinder.box_id(),
            ActiveDialog::TournamentFinder => ToolWindow::TournamentFinder.box_id(),
            ActiveDialog::PlayerInfo => ToolWindow::PlayerInfo.box_id(),
            ActiveDialog::SpellCheck => ToolWindow::SpellCheck.box_id(),
            ActiveDialog::AddElo => ToolWindow::AddElo.box_id(),
            ActiveDialog::AdvancedSearch => advanced_dialog::ADVANCED_BOX_ID,
            ActiveDialog::PatternSearch => pattern_dialog::PATTERN_BOX_ID,
            ActiveDialog::PatternCost => pattern_dialog::PATTERN_COST_BOX_ID,
            ActiveDialog::Sound => crate::windows::sound_dialog::SOUND_BOX_ID,
            ActiveDialog::Fics => crate::windows::fics_dialog::FICS_BOX_ID,
            ActiveDialog::FicsConfirm => crate::windows::fics_dialog::FICS_CONFIRM_BOX_ID,
            ActiveDialog::Eboard => crate::windows::eboard_dialog::EBOARD_BOX_ID,
            ActiveDialog::MaskPrompt => mask_dialogs::MASK_PROMPT_BOX_ID,
            ActiveDialog::MaskFillLine => mask_dialogs::MASK_FILL_BOX_ID,
            ActiveDialog::MaskComment => mask_dialogs::MASK_COMMENT_BOX_ID,
            ActiveDialog::MaskSearch => mask_dialogs::MASK_SEARCH_BOX_ID,
            ActiveDialog::MaskOutline => mask_dialogs::MASK_OUTLINE_BOX_ID,
        }
    }
}

/// Whether the import of PGN files is still running: the one predicate of
/// Esc, Enter and the buttons of its window (the report is shown when it is
/// not).
fn import_running(app: &AppState) -> bool {
    app.file_import_view().is_some_and(|v| v.running)
}

/// Whether the export of games is still running or waiting for its file
/// dialog (Esc stops or closes; the report is shown when it is not).
fn export_running(app: &AppState) -> bool {
    app.games_export_view()
        .is_some_and(|v| matches!(v.step, mychess_app::GamesExportStepView::Running { .. }))
}

/// The field of the game information dialog that `field` stands for.
pub fn field_id(field: TagField) -> &'static str {
    match field {
        TagField::Black => "hdr-black",
        TagField::Event => "hdr-event",
        TagField::Site => "hdr-site",
        TagField::Round => "hdr-round",
        TagField::Date => "hdr-date-year",
        TagField::EventDate => "hdr-event-date-year",
        TagField::WhiteRating => "hdr-white-elo",
        TagField::BlackRating => "hdr-black-elo",
        TagField::Eco => "hdr-eco",
        _ => "hdr-white",
    }
}

/// The text of "go to move".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GotoDraft {
    /// What the user typed.
    pub text: String,
    /// The last try was not a move number.
    pub invalid: bool,
}

/// Which move "go to move" aims at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GotoTarget {
    /// The move number.
    pub number: u32,
    /// Black's move (`12...`).
    pub black: bool,
}

impl GotoDraft {
    /// Reads `12`, `12.`, `12...` and `12b` (Black's move).
    pub fn parse(&self) -> Option<GotoTarget> {
        let text = self.text.trim();
        let (digits, black) = if let Some(rest) = text.strip_suffix("...") {
            (rest, true)
        } else if let Some(rest) = text.strip_suffix(['b', 'B']) {
            (rest, true)
        } else {
            (text.strip_suffix('.').unwrap_or(text), false)
        };
        let number: u32 = digits.trim().parse().ok()?;
        (number > 0).then_some(GotoTarget { number, black })
    }
}

/// The highlighted suggestion and the pending focus of the tags dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderUi {
    /// The row that `Enter` picks.
    pub highlight: Option<usize>,
    /// The list was closed with `Esc` or a pick, until the next typing.
    pub hidden: bool,
    /// The field to focus when the dialog opens.
    pub pending_focus: Option<TagField>,
}

/// Mouse state of the set-up board and the text of its counters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupUi {
    /// The square where the left button went down.
    pub pressed: Option<Square>,
    /// The text of the move number field.
    pub fullmove: String,
    /// The text of the half-move clock field.
    pub halfmove: String,
    seen: Option<(u16, u16)>,
}

/// The import text editor (the application keeps the plain text).
#[derive(Debug, Default)]
pub struct PasteUi {
    /// The widget state.
    pub content: text_editor::Content,
    synced: String,
}

/// The scroll of the game list.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ChooserUi {
    /// Pixels scrolled from the top.
    pub scroll: f32,
}

/// How long a temporary remark stays in the status bar (spec M2-4, D204).
pub const NOTICE_LIFETIME: std::time::Duration = std::time::Duration::from_secs(8);

/// The clock of the temporary remarks of the status bar. The time comes
/// from the ticks of the subscription (injected, never read here).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusClock {
    since: Option<Instant>,
    expired: bool,
}

impl StatusClock {
    /// A new remark may have appeared: start counting again.
    pub fn reset(&mut self) {
        *self = StatusClock::default();
    }

    /// A tick of the timer; `has_notice` tells whether a remark is on show.
    pub fn tick(&mut self, now: Instant, has_notice: bool) {
        if !has_notice {
            self.reset();
            return;
        }
        let since = *self.since.get_or_insert(now);
        if now.saturating_duration_since(since) >= NOTICE_LIFETIME {
            self.expired = true;
        }
    }

    /// Whether the remark on show should be hidden.
    pub fn expired(self) -> bool {
        self.expired
    }
}

/// What the dialogs need from the screen, apart from the application.
#[derive(Debug, Default)]
pub struct DialogUi {
    /// The clock of the temporary remarks of the status bar.
    pub status: StatusClock,
    /// The "don't ask again" box of the open question is ticked.
    pub dont_ask: bool,
    /// "Go to move", when open.
    pub goto: Option<GotoDraft>,
    /// "New Chess960 game", when open (spec M9-6).
    pub chess960: Option<Chess960Draft>,
    /// The tags dialog.
    pub header: HeaderUi,
    /// The set-up dialog.
    pub setup: SetupUi,
    /// The import dialog.
    pub paste: PasteUi,
    /// The game chooser.
    pub chooser: ChooserUi,
    /// The pieces of the set-up board and its palette.
    pub pieces: PieceSet,
    /// The analysis options of the Engines dialog are unfolded.
    pub engines_prefs_open: bool,
    /// What the user has typed in the numeric fields of those options, kept
    /// as text so that a field can be empty while it is edited (D751).
    pub engines_prefs_text: [Option<String>; 2],
    /// The informant thresholds of the annotation dialog are unfolded.
    pub annotate_informant_open: bool,
    /// The game against an engine (spec M8-1b).
    pub play: PlayUi,
    /// The tournament between engines (spec M8-2b).
    pub tournament: TournamentUi,
    /// The configuration of the tactics training (spec M8-4c).
    pub tactics: TacticsUi,
    /// The configuration of the opening trainer (spec M8-5c).
    pub opening: OpeningUi,
    /// The configuration of the calculation exercise and the field of
    /// moves of its panel (spec M8-6c).
    pub calvar: CalvarUi,
    /// The visual state of the advanced searches (spec M6-1c).
    pub advanced: AdvancedSearchUi,
    /// The windows of the players, tournaments and spelling (spec M6-3c).
    pub player_tools: PlayerToolsUi,
    /// The visual state of the search for tactics and patterns (spec M6-2d).
    pub pattern: PatternSearchUi,
    /// The electronic board window, when open (spec M8-10c).
    pub eboard: Option<crate::windows::eboard_dialog::EboardUi>,
    /// The tablebase panel (spec M8-8c).
    pub tablebase: crate::windows::tablebase_panel::TablebaseUi,
    /// The crosstable, statistics, rating graph and piece tracker panels
    /// (spec M6-4d).
    pub panels: crate::windows::data_panels::DataPanelsUi,
    /// The tree masks: context menu, editors, search and outline (spec
    /// M5-4c).
    pub mask: MaskUi,
    /// The online play on Lichess (spec M8-3c).
    pub online: OnlineUi,
    /// The play on FICS (spec M8-9c).
    pub fics: crate::windows::fics_dialog::FicsUi,
    /// The book window: what is typed in its weight fields (spec M5-6).
    pub book: crate::windows::book_panel::BookPanelUi,
    /// The EPD panel: editor, scroll, search and forms (spec M7-5c).
    pub epd: crate::windows::epd::EpdUi,
    /// The opening report window: model, forms, board (spec M7-2c).
    pub oprep: crate::windows::opening_report::OpRepUi,
    /// The player report window: draft of the dialog, forms (spec M7-3c).
    pub preport: crate::windows::player_report::PrUi,
    /// The window of the board image: assets and preview (spec M7-4c).
    pub export_image: crate::windows::export_image::ExportImageUi,
    /// The manual window: the chapter on the screen (spec M9-5).
    pub manual: crate::windows::manual::ManualUi,
    /// The maintenance window: forms and fields (spec M7-1c).
    pub maint: crate::windows::maintenance::MaintUi,
    /// The appearance window, when open (spec M9-3).
    pub appearance: Option<crate::windows::appearance::AppearanceUi>,
    last_active: Option<ActiveDialog>,
}

/// Events of the dialogs that change only the visual state (and may send
/// messages to the application).
#[derive(Debug, Clone)]
pub enum DialogMessage {
    /// The "don't ask again" box of the open question was (un)ticked.
    DontAskAgain(bool),
    /// An event of the appearance window (spec M9-3); the shell handles it
    /// because applying sends messages of the shell as well.
    Appearance(crate::windows::appearance::AppearanceMessage),
    /// Opens "go to move".
    OpenGoto,
    /// The text of "go to move" changed.
    GotoEdited(String),
    /// Go to the move typed.
    GotoSubmit,
    /// Closes "go to move".
    CloseGoto,
    /// Opens "New Chess960 game" (spec M9-6).
    OpenChess960,
    /// The number of "New Chess960 game" changed.
    Chess960Edited(String),
    /// Draws a random position.
    Chess960Random,
    /// Starts the game with the position typed.
    Chess960Submit,
    /// Closes "New Chess960 game".
    CloseChess960,
    /// Opens the tags dialog, focusing a field.
    OpenHeader(Option<TagField>),
    /// A text field of the tags dialog changed.
    HeaderText(TagField, String),
    /// A suggestion was clicked.
    HeaderPick(TagField, usize),
    /// `Enter` in a field of the tags dialog.
    HeaderSubmit,
    /// The left button went down on a set-up square.
    SetupPress(Square),
    /// The left button went up on a set-up square.
    SetupRelease(Square),
    /// The left button went up anywhere in the set-up dialog.
    SetupAreaRelease,
    /// A numeric field of the analysis options of the Engines dialog was
    /// edited (or submitted: then the text is the one it already shows and
    /// nothing changes, so that Enter never reaches the dialog, D750).
    EnginesPrefsText(PrefsNumber, String),
    /// The move number of the set-up dialog was edited.
    SetupFullmove(String),
    /// The half-move clock of the set-up dialog was edited.
    SetupHalfmove(String),
    /// Unfolds or folds the informant thresholds of the annotation dialog.
    ToggleAnnotateInformant,
    /// The import editor produced an action.
    PasteEdit(text_editor::Action),
    /// The game list scrolled to this offset.
    ChooserScrolled(f32),
    /// The text of the move field changed: these are the keystrokes the
    /// move-entry buffer needs (see `nav_bar::entry_edit`).
    MoveEntryKeys(Vec<GameMessage>),
    /// A key typed with the board focused and no text field focused: it may
    /// start a NAG sequence or be part of a move (D181, D199).
    Typed(TypedKey, Instant),
    /// The timer of a half-typed NAG sequence fired.
    NagTick(Instant),
    /// A tick of the timer that expires the remarks of the status bar.
    StatusTick(Instant),
    /// A key press, as the subscription reports it: the key for the
    /// shortcut map, and the text it typed (if any) for the move entry.
    Key {
        /// The key and modifiers.
        input: KeyInput,
        /// A widget used the key for text.
        text_focused: bool,
        /// The character typed, when it is printable.
        typed: Option<char>,
    },
}

/// A key that feeds the typed-move and NAG-shortcut buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedKey {
    /// A printable character (the text of the event, not the key code).
    Char(char),
    /// Enter.
    Enter,
    /// Any other key: a half-typed NAG sequence is dropped.
    Interrupt,
}

/// What an event asks the shell to do.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Messages for the application, in order.
    pub messages: Vec<AppMessage>,
    /// A widget to focus.
    pub focus: Option<&'static str>,
    /// Move the focus inside the dialog (`true`: backwards).
    pub cycle: Option<bool>,
    /// Scroll the game list to this offset.
    pub scroll: Option<f32>,
    /// Leave the program (the shell saves the window first).
    pub quit: bool,
}

impl Outcome {
    pub(crate) fn send(message: AppMessage) -> Outcome {
        Outcome {
            messages: vec![message],
            ..Outcome::default()
        }
    }
}

/// A numeric field of the analysis options of the Engines dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsNumber {
    /// Size of the protocol log.
    LogLines,
    /// Longest PV shown.
    MaxPly,
}

impl PrefsNumber {
    fn slot(self) -> usize {
        match self {
            PrefsNumber::LogLines => 0,
            PrefsNumber::MaxPly => 1,
        }
    }
}

fn digits_only(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).take(4).collect()
}

impl DialogUi {
    /// What the game against an engine shows, at the time of the last
    /// update (the views never read the clock).
    pub fn play_view(&self, app: &AppState) -> Option<mychess_app::PlayView> {
        app_api::play_view(app, self.play.now)
    }

    /// The box the game against an engine asks for, if any.
    pub fn play_box(&self, app: &AppState) -> Option<PlayBox> {
        self.play_view(app)
            .and_then(|view| play_panel::box_for(&view))
    }

    /// Whether the suggestions of the tags dialog are on screen.
    pub fn suggestions_visible(&self, app: &AppState) -> bool {
        !self.header.hidden
            && app
                .info_dialog
                .as_ref()
                .is_some_and(|d| !d.suggestions.is_empty())
    }

    fn suggestion_count(app: &AppState) -> usize {
        app.info_dialog
            .as_ref()
            .map_or(0, |d| d.suggestions.len().min(MAX_ROWS))
    }

    /// Applies an event.
    #[allow(clippy::too_many_lines)] // one flat table of events
    pub fn update(&mut self, message: DialogMessage, app: &AppState) -> Outcome {
        match message {
            DialogMessage::DontAskAgain(on) => {
                self.dont_ask = on;
                Outcome::default()
            }
            DialogMessage::OpenGoto => {
                self.goto = Some(GotoDraft::default());
                Outcome {
                    focus: Some(GOTO_FIELD_ID),
                    ..Outcome::default()
                }
            }
            DialogMessage::GotoEdited(text) => {
                self.goto = Some(GotoDraft {
                    text,
                    invalid: false,
                });
                Outcome::default()
            }
            DialogMessage::GotoSubmit => self.goto_submit(),
            DialogMessage::CloseGoto => {
                self.goto = None;
                Outcome::default()
            }
            DialogMessage::OpenChess960 => {
                self.chess960 = Some(Chess960Draft::default());
                Outcome {
                    focus: Some(CHESS960_FIELD_ID),
                    ..Outcome::default()
                }
            }
            DialogMessage::Chess960Edited(text) => {
                self.chess960 = Some(Chess960Draft {
                    text,
                    invalid: false,
                });
                Outcome::default()
            }
            DialogMessage::Chess960Random => {
                // Only a draw for a position: the low bits of the clock do.
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos());
                self.chess960 = Some(Chess960Draft::random(
                    u64::try_from(seed & u128::from(u64::MAX)).unwrap_or(0),
                ));
                Outcome::default()
            }
            DialogMessage::Chess960Submit => self.chess960_submit(),
            DialogMessage::CloseChess960 => {
                self.chess960 = None;
                Outcome::default()
            }
            DialogMessage::OpenHeader(field) => {
                self.header = HeaderUi {
                    pending_focus: Some(field.unwrap_or(TagField::White)),
                    ..HeaderUi::default()
                };
                Outcome::send(AppMessage::Info(GameInfoMessage::Open))
            }
            DialogMessage::HeaderText(field, text) => {
                self.header.hidden = false;
                self.header.highlight = None;
                Outcome::send(AppMessage::Info(GameInfoMessage::SetText(field, text)))
            }
            DialogMessage::HeaderPick(field, index) => self.pick_suggestion(field, index, app),
            DialogMessage::HeaderSubmit => {
                let picked = self
                    .header
                    .highlight
                    .filter(|_| self.suggestions_visible(app));
                match (
                    picked,
                    app.info_dialog.as_ref().and_then(|d| d.suggestion_field),
                ) {
                    (Some(index), Some(field)) => self.pick_suggestion(field, index, app),
                    _ => Outcome::send(AppMessage::Info(GameInfoMessage::Apply)),
                }
            }
            DialogMessage::SetupPress(square) => {
                self.setup.pressed = Some(square);
                Outcome::default()
            }
            DialogMessage::SetupRelease(square) => self.setup_release(Some(square), app),
            DialogMessage::SetupAreaRelease => self.setup_release(None, app),
            DialogMessage::EnginesPrefsText(field, text) => self.prefs_text(field, &text, app),
            DialogMessage::SetupFullmove(text) => {
                self.setup.fullmove = digits_only(&text);
                match self.setup.fullmove.parse::<u16>() {
                    Ok(n) if n > 0 => {
                        Outcome::send(AppMessage::Setup(SetupMessage::SetFullmove(n)))
                    }
                    _ => Outcome::default(),
                }
            }
            DialogMessage::SetupHalfmove(text) => {
                self.setup.halfmove = digits_only(&text);
                match self.setup.halfmove.parse::<u16>() {
                    Ok(n) => Outcome::send(AppMessage::Setup(SetupMessage::SetHalfmove(n))),
                    _ => Outcome::default(),
                }
            }
            DialogMessage::ToggleAnnotateInformant => {
                self.annotate_informant_open = !self.annotate_informant_open;
                Outcome::default()
            }
            DialogMessage::PasteEdit(action) => {
                let edits = action.is_edit();
                self.paste.content.perform(action);
                if edits {
                    let text = self.paste.content.text();
                    self.paste.synced.clone_from(&text);
                    Outcome::send(AppMessage::Import(ImportMessage::TextEdited(text)))
                } else {
                    Outcome::default()
                }
            }
            DialogMessage::ChooserScrolled(offset) => {
                self.chooser.scroll = offset.max(0.0);
                Outcome::default()
            }
            DialogMessage::MoveEntryKeys(keys) => Outcome {
                messages: keys.into_iter().map(AppMessage::Game).collect(),
                ..Outcome::default()
            },
            DialogMessage::Typed(key, at) => Outcome::send(typed_message(key, at)),
            DialogMessage::NagTick(now) => Outcome::send(AppMessage::NagTick(now)),
            // The shell handles these: they need the shortcut map and the
            // notices.
            // The shell handles the appearance window before it gets here.
            DialogMessage::Appearance(_)
            | DialogMessage::StatusTick(_)
            | DialogMessage::Key { .. } => Outcome::default(),
        }
    }

    fn chess960_submit(&mut self) -> Outcome {
        let Some(draft) = self.chess960.as_mut() else {
            return Outcome::default();
        };
        let Some(number) = draft.number() else {
            draft.invalid = true;
            return Outcome::default();
        };
        self.chess960 = None;
        Outcome::send(app_api::command(mychess_app::Command::NewChess960(number)))
    }

    fn goto_submit(&mut self) -> Outcome {
        let Some(draft) = self.goto.as_mut() else {
            return Outcome::default();
        };
        let Some(target) = draft.parse() else {
            draft.invalid = true;
            return Outcome::default();
        };
        self.goto = None;
        let nav = if target.black {
            Nav::GoToBlackMoveNumber(target.number)
        } else {
            Nav::GoToMoveNumber(target.number)
        };
        Outcome::send(app_api::navigate(nav))
    }

    fn pick_suggestion(&mut self, field: TagField, index: usize, app: &AppState) -> Outcome {
        let name = app
            .info_dialog
            .as_ref()
            .and_then(|d| d.suggestions.get(index))
            .map(|m| m.name.clone());
        self.header.hidden = true;
        self.header.highlight = None;
        match name {
            Some(name) => Outcome::send(AppMessage::Info(GameInfoMessage::SetText(field, name))),
            None => Outcome::default(),
        }
    }

    /// A left-button release: a click when it ends where it began, a drop
    /// when it ends elsewhere (`None`: outside the board).
    fn setup_release(&mut self, at: Option<Square>, app: &AppState) -> Outcome {
        let Some(from) = self.setup.pressed.take() else {
            return Outcome::default();
        };
        let Some(board) = app.setup.as_ref() else {
            return Outcome::default();
        };
        let message = match at {
            Some(to) if to == from => SetupMessage::ClickSquare(from),
            _ if board.piece_at(from).is_none() => return Outcome::default(),
            to => SetupMessage::Drop { from, to },
        };
        Outcome::send(AppMessage::Setup(message))
    }

    /// Brings the visual state up to date with the application: the import
    /// editor follows the text, the counters follow the board, closed
    /// dialogs forget their state.
    pub fn sync(&mut self, app: &AppState) {
        self.play.sync(app);
        self.tactics.sync(app);
        self.opening.sync(app);
        self.calvar.sync(app);
        self.advanced.sync(app);
        self.player_tools.sync(app);
        self.pattern.sync(app);
        self.mask.sync(app);
        self.online.sync(app);
        self.fics.sync(app);
        if app_api::session_prompt(app).is_none() {
            self.dont_ask = false;
        }
        match app.import.as_ref() {
            Some(dialog) => {
                if dialog.text != self.paste.synced {
                    self.paste.content = text_editor::Content::with_text(&dialog.text);
                    self.paste.synced.clone_from(&dialog.text);
                }
            }
            None => {
                if !self.paste.synced.is_empty() || !self.paste.content.is_empty() {
                    self.paste = PasteUi::default();
                }
            }
        }
        match app.setup.as_ref() {
            Some(board) => {
                let now = (board.fullmove_number, board.halfmove_clock);
                if self.setup.seen != Some(now) {
                    self.setup.seen = Some(now);
                    self.setup.fullmove = now.0.to_string();
                    self.setup.halfmove = now.1.to_string();
                }
            }
            None => self.setup = SetupUi::default(),
        }
        if app.info_dialog.is_none() && self.header.pending_focus.is_none() {
            self.header = HeaderUi::default();
        }
        if app.chooser.is_none() {
            self.chooser = ChooserUi::default();
        }
        if app.annotate_dialog.is_none() {
            self.annotate_informant_open = false;
        }
    }

    /// The widget to focus when a dialog has just opened (`None` the rest
    /// of the time). Call after every update.
    pub fn newly_opened(&mut self, app: &AppState) -> Option<&'static str> {
        let now = ActiveDialog::of(app, self);
        let changed = now != self.last_active;
        self.last_active = now;
        if !changed {
            return None;
        }
        match now? {
            ActiveDialog::Goto => Some(GOTO_FIELD_ID),
            ActiveDialog::Chess960 => Some(CHESS960_FIELD_ID),
            ActiveDialog::EcoBrowser => Some(crate::windows::eco_browser::ECO_FIND_ID),
            ActiveDialog::Info => {
                let field = self.header.pending_focus.take().unwrap_or(TagField::White);
                Some(field_id(field))
            }
            ActiveDialog::Import => Some(PASTE_EDITOR_ID),
            ActiveDialog::Chooser => Some(CHOOSER_FILTER_ID),
            ActiveDialog::EngineEditor => Some(crate::windows::engine_editor::EDITOR_NAME_ID),
            ActiveDialog::OnlineConnect => OnlineUi::focus_of(OnlineActive::Connect),
            ActiveDialog::OnlineSeek => OnlineUi::focus_of(OnlineActive::Seek),
            ActiveDialog::OnlineChallenge => OnlineUi::focus_of(OnlineActive::Challenge),
            ActiveDialog::OnlineWatch => OnlineUi::focus_of(OnlineActive::Watch),
            ActiveDialog::MaskComment => Some(mask_dialogs::MASK_COMMENT_EDITOR_ID),
            ActiveDialog::PlayerFinder => Some(crate::windows::player_tools::PLAYER_PREFIX_ID),
            _ => None,
        }
    }

    /// The first row index and count of the rows to build for a list of
    /// `total` rows scrolled by [`ChooserUi::scroll`].
    pub fn chooser_window(&self, total: usize) -> (usize, usize) {
        visible_rows(self.chooser.scroll, total)
    }

    /// Where to scroll the list so that visible row `row` is on screen.
    pub fn reveal_row(&self, row: usize) -> Option<f32> {
        // Row indices are small (at most the number of games, 50 000 by
        // design); `f32` holds them exactly.
        #[allow(clippy::cast_precision_loss)]
        let top = row as f32 * CHOOSER_ROW_HEIGHT;
        let bottom = top + CHOOSER_ROW_HEIGHT;
        if top < self.chooser.scroll {
            Some(top)
        } else if bottom > self.chooser.scroll + CHOOSER_LIST_HEIGHT {
            Some(bottom - CHOOSER_LIST_HEIGHT)
        } else {
            None
        }
    }

    /// A numeric field of the analysis options was edited: keep the text and
    /// store the number (nothing is sent when it did not change).
    fn prefs_text(&mut self, field: PrefsNumber, text: &str, app: &AppState) -> Outcome {
        let digits: String = text.chars().filter(char::is_ascii_digit).take(6).collect();
        let parsed = digits.parse::<u32>().unwrap_or(0);
        self.engines_prefs_text[field.slot()] = Some(digits);
        let mut prefs = app.prefs.analysis.as_ref().to_owned();
        let current = match field {
            PrefsNumber::LogLines => &mut prefs.log_lines,
            PrefsNumber::MaxPly => &mut prefs.max_ply,
        };
        if *current == parsed {
            return Outcome::default();
        }
        *current = parsed;
        Outcome::send(engine(EngineMessage::SetAnalysisPrefs(prefs)))
    }

    /// A key pressed while `active` is on top. Escape cancels, Enter runs
    /// the default answer (unless a text field used it), Tab cycles the
    /// focus inside the dialog and the arrows move the list of the dialog.
    #[allow(clippy::too_many_lines)] // one flat table of keys per dialog
    pub fn key(
        &mut self,
        active: ActiveDialog,
        input: &KeyInput,
        text_focused: bool,
        app: &AppState,
    ) -> Outcome {
        let plain = input.modifiers == Modifiers::NONE;
        match input.key {
            Key::Named(NamedKey::Tab) => Outcome {
                cycle: Some(input.modifiers.shift),
                ..Outcome::default()
            },
            Key::Named(NamedKey::Escape) if plain => self.escape(active, app),
            Key::Named(NamedKey::Enter) if plain && !text_focused => self.enter(active, app),
            Key::Named(NamedKey::Up) if plain => self.arrow(active, -1, app),
            Key::Named(NamedKey::Down) if plain => self.arrow(active, 1, app),
            Key::Named(NamedKey::Right) if plain && active == ActiveDialog::Variations => {
                Outcome::send(AppMessage::Game(GameMessage::ChooserConfirm(None)))
            }
            Key::Named(NamedKey::Left) if plain && active == ActiveDialog::Variations => {
                Outcome::send(AppMessage::Game(GameMessage::CloseVariationChooser))
            }
            Key::Char(c) if plain && !text_focused => match active {
                ActiveDialog::Prompt => app_api::session_prompt(app)
                    .and_then(|prompt| prompts::key_answer(prompt, input))
                    .map_or_else(Outcome::default, Outcome::send),
                ActiveDialog::Engines => {
                    Outcome::send(AppMessage::Engine(EngineMessage::SelectInitial(c)))
                }
                ActiveDialog::Variations => c
                    .to_digit(10)
                    .and_then(|d| usize::try_from(d).ok())
                    .map_or_else(Outcome::default, |d| {
                        Outcome::send(AppMessage::Game(GameMessage::ChooserConfirm(Some(d))))
                    }),
                _ => Outcome::default(),
            },
            _ => Outcome::default(),
        }
    }

    #[allow(clippy::too_many_lines)] // one flat table of dialogs
    fn escape(&mut self, active: ActiveDialog, app: &AppState) -> Outcome {
        match active {
            ActiveDialog::Discard => Outcome::send(app_api::answer_discard(DiscardChoice::Cancel)),
            ActiveDialog::Overwrite => Outcome::send(app_api::answer_overwrite(false)),
            ActiveDialog::CopyGames => Outcome::send(app_api::answer_copy(false)),
            ActiveDialog::Prompt => app_api::session_prompt(app)
                .map_or_else(Outcome::default, |p| {
                    Outcome::send(prompts::cancel_answer(p))
                }),
            ActiveDialog::Setup => Outcome::send(AppMessage::Setup(SetupMessage::Cancel)),
            ActiveDialog::Info => {
                if self.suggestions_visible(app) {
                    self.header.hidden = true;
                    self.header.highlight = None;
                    Outcome::default()
                } else {
                    Outcome::send(AppMessage::Info(GameInfoMessage::Cancel))
                }
            }
            ActiveDialog::Import => Outcome::send(AppMessage::Import(ImportMessage::Close)),
            // Esc stops the job while it runs and closes the report after
            // (D1985).
            ActiveDialog::ImportFile => Outcome::send(if import_running(app) {
                crate::windows::import_file_dialog::stop_message_app()
            } else {
                crate::windows::import_file_dialog::close_message_app()
            }),
            // Esc stops the job while it runs and closes the window
            // otherwise (D2017).
            // A question about an existing file takes the Esc first (M7-4c).
            ActiveDialog::ExportGames => Outcome::send(
                if let Some(no) = crate::windows::export_games_dialog::escape_confirm_message(app) {
                    no
                } else if export_running(app) {
                    crate::windows::export_games_dialog::stop_message_app()
                } else {
                    crate::windows::export_games_dialog::close_message_app()
                },
            ),
            ActiveDialog::Merge => {
                Outcome::send(crate::windows::merge_dialog::cancel_message_app())
            }
            ActiveDialog::Goto => {
                self.goto = None;
                Outcome::default()
            }
            ActiveDialog::Chess960 => {
                self.chess960 = None;
                Outcome::default()
            }
            ActiveDialog::Chooser => {
                Outcome::send(AppMessage::Chooser(mychess_app::ChooserMessage::Cancel))
            }
            ActiveDialog::Variations => {
                Outcome::send(AppMessage::Game(GameMessage::CloseVariationChooser))
            }
            ActiveDialog::Bookmarks => Outcome::send(bookmark_edit(EditorMessage::Cancel)),
            ActiveDialog::Annotate => Outcome::send(annotate(AnnotateMessage::Close)),
            ActiveDialog::ReportConfig => Outcome::send(AppMessage::Report(
                mychess_app::report::ReportMessage::CloseConfig,
            )),
            ActiveDialog::EcoBrowser => Outcome::send(eco(EcoMessage::CloseBrowser)),
            ActiveDialog::EcoClassify => Outcome::send(eco(EcoMessage::CloseClassify)),
            ActiveDialog::EcoNovelty => Outcome::send(eco(EcoMessage::CloseNovelty)),
            ActiveDialog::PlayConfig => self.play.update(PlayUiMessage::Cancel, app),
            // Esc is "No": the game goes on.
            ActiveDialog::PlayConfirm => self.play.update(PlayUiMessage::Confirm(false), app),
            ActiveDialog::PlayAsk => self.play.update(PlayUiMessage::Answer(false), app),
            ActiveDialog::PlayEnd => self.play.update(PlayUiMessage::Quit, app),
            // The only ways out are the buttons: leaving needs a choice. The
            // keys of the player tools are served by `Shell::player_tools_key`.
            ActiveDialog::PlayLost
            | ActiveDialog::PlayerFinder
            | ActiveDialog::TournamentFinder
            | ActiveDialog::PlayerInfo
            | ActiveDialog::SpellCheck
            | ActiveDialog::AddElo => Outcome::default(),
            // Esc is "No" in the questions; the decision box waits (the
            // tournament does not go on without an answer, D1369).
            ActiveDialog::TournamentForm => {
                self.tournament.update(TournamentUiMessage::Cancel, app)
            }
            ActiveDialog::TournamentResume => self
                .tournament
                .update(TournamentUiMessage::ResumeClose, app),
            ActiveDialog::TournamentConfirm => self
                .tournament
                .update(TournamentUiMessage::Confirm(false), app),
            ActiveDialog::TournamentDecision => self
                .tournament
                .update(TournamentUiMessage::DecisionLater, app),
            ActiveDialog::OnlineConnect
            | ActiveDialog::OnlineSeek
            | ActiveDialog::OnlineChallenge
            | ActiveDialog::OnlineIncoming
            | ActiveDialog::OnlineWatch
            | ActiveDialog::OnlinePromotion
            | ActiveDialog::OnlineConfirm => self.online_key(active, true, app),
            ActiveDialog::Engines => Outcome::send(engine(EngineMessage::CloseDialog)),
            ActiveDialog::EngineEditor => {
                Outcome::send(engine(EngineMessage::Editor(EngineEditorMessage::Cancel)))
            }
            ActiveDialog::EngineOptions => {
                Outcome::send(engine(EngineMessage::Options(OptionsMessage::Close)))
            }
            ActiveDialog::EngineDelete => Outcome::send(engine(EngineMessage::CancelDelete)),
            ActiveDialog::Appearance => {
                self.appearance = None;
                Outcome::default()
            }
            // Esc is "Cancel": the program stays open (D1792).
            ActiveDialog::BookQuit => {
                self.book.quit = None;
                Outcome::default()
            }
            ActiveDialog::TacticsSetup => Outcome::send(tactics_dialog::cancel_message()),
            ActiveDialog::OpeningSetup => Outcome::send(opening_dialog::cancel_message()),
            ActiveDialog::CalvarSetup => Outcome::send(calvar_dialog::cancel_message()),
            // Esc stops a search in flight; else it closes the window.
            ActiveDialog::AdvancedSearch => Outcome::send(if app.advanced_search().is_running() {
                advanced_dialog::stop_message()
            } else {
                advanced_dialog::close_message()
            }),
            // Esc stops a search for patterns in flight; else it closes the
            // window. On the question about the cost it is "No".
            ActiveDialog::PatternSearch => Outcome::send(if app.pattern_search().is_running() {
                pattern_dialog::stop_message()
            } else {
                pattern_dialog::close_message()
            }),
            ActiveDialog::PatternCost => Outcome::send(pattern_dialog::cost_message(false)),
            ActiveDialog::Sound => Outcome::send(crate::windows::sound_dialog::cancel_message()),
            ActiveDialog::Fics => Outcome::send(crate::windows::fics_dialog::cancel_message()),
            // Esc is "No": the game goes on.
            ActiveDialog::FicsConfirm => self.fics.update(
                crate::windows::fics_dialog::FicsUiMessage::ConfirmResign(false),
                app,
            ),
            ActiveDialog::Eboard => {
                self.eboard = None;
                Outcome::default()
            }
            ActiveDialog::MaskPrompt => self.mask.escape(MaskDialog::Prompt),
            ActiveDialog::MaskFillLine => self.mask.escape(MaskDialog::FillLine),
            ActiveDialog::MaskComment => self.mask.escape(MaskDialog::Comment),
            ActiveDialog::MaskSearch => self.mask.escape(MaskDialog::Search),
            ActiveDialog::MaskOutline => self.mask.escape(MaskDialog::Outline),
        }
    }

    #[allow(clippy::too_many_lines)] // one flat table of dialogs
    fn enter(&mut self, active: ActiveDialog, app: &AppState) -> Outcome {
        match active {
            ActiveDialog::Discard => {
                Outcome::send(app_api::answer_discard(DiscardChoice::DontSave))
            }
            ActiveDialog::Overwrite => Outcome::send(app_api::answer_overwrite(false)),
            // Return is "go" in the Scid's dialog too (`copyFilter` binds it
            // to the Copy button), though a copy into an `.si4` is not
            // something Esc can take back.
            ActiveDialog::CopyGames => Outcome::send(app_api::answer_copy(true)),
            ActiveDialog::Prompt => app_api::session_prompt(app)
                .map_or_else(Outcome::default, |p| {
                    Outcome::send(prompts::default_answer(p))
                }),
            ActiveDialog::Setup => Outcome::send(AppMessage::Setup(SetupMessage::Ok)),
            ActiveDialog::Info => self.update(DialogMessage::HeaderSubmit, app),
            ActiveDialog::Merge => Outcome::send(crate::windows::merge_dialog::apply_message_app()),
            ActiveDialog::Goto => self.goto_submit(),
            ActiveDialog::Chess960 => self.chess960_submit(),
            ActiveDialog::Chooser => {
                Outcome::send(AppMessage::Chooser(mychess_app::ChooserMessage::Confirm))
            }
            ActiveDialog::Variations => {
                Outcome::send(AppMessage::Game(GameMessage::ChooserConfirm(None)))
            }
            ActiveDialog::Bookmarks => Outcome::send(bookmark_edit(EditorMessage::Ok)),
            // Enter is OK in the Scid's dialog (`okAnnotation`).
            ActiveDialog::Annotate => Outcome::send(annotate(AnnotateMessage::Start)),
            ActiveDialog::ReportConfig => Outcome::send(AppMessage::Report(
                mychess_app::report::ReportMessage::ConfigStart,
            )),
            // Enter searches in the browser, as in the field of the search;
            // classifying rewrites codes, so only the button does it.
            ActiveDialog::EcoBrowser => Outcome::send(eco(EcoMessage::SubmitFind)),
            ActiveDialog::EcoNovelty => Outcome::send(eco(EcoMessage::FindNovelty)),
            // Enter is "Play" in the Scid's dialog; in the questions it is
            // the affirmative answer, and it closes the end box.
            ActiveDialog::PlayConfig => self.play.update(PlayUiMessage::Submit, app),
            ActiveDialog::PlayConfirm => self.play.update(PlayUiMessage::Confirm(true), app),
            ActiveDialog::PlayAsk => self.play.update(PlayUiMessage::Answer(true), app),
            ActiveDialog::PlayEnd => self.play.update(PlayUiMessage::Quit, app),
            ActiveDialog::TournamentForm => {
                self.tournament.update(TournamentUiMessage::Submit, app)
            }
            ActiveDialog::TournamentConfirm => self
                .tournament
                .update(TournamentUiMessage::Confirm(true), app),
            // An answer about a participant is a deliberate choice.
            ActiveDialog::ImportFile => {
                if import_running(app) {
                    Outcome::default()
                } else {
                    Outcome::send(crate::windows::import_file_dialog::close_message_app())
                }
            }
            // Enter chooses the destination in the options and closes the
            // report; while the job runs it does nothing (D2017).
            ActiveDialog::ExportGames => match app.games_export_view().map(|v| v.step) {
                Some(mychess_app::GamesExportStepView::Options)
                    if crate::windows::export_games_dialog::enter_confirm_message(app)
                        .is_some() =>
                {
                    Outcome::send(
                        crate::windows::export_games_dialog::enter_confirm_message(app)
                            .unwrap_or_else(crate::windows::export_games_dialog::choose_message_app),
                    )
                }
                Some(mychess_app::GamesExportStepView::Options) => Outcome::send(
                    crate::windows::export_games_dialog::choose_message_app(),
                ),
                Some(mychess_app::GamesExportStepView::Done(_)) => {
                    Outcome::send(crate::windows::export_games_dialog::close_message_app())
                }
                _ => Outcome::default(),
            },
            ActiveDialog::Import
            | ActiveDialog::PlayerFinder
            | ActiveDialog::TournamentFinder
            | ActiveDialog::PlayerInfo
            | ActiveDialog::SpellCheck
            | ActiveDialog::AddElo
            | ActiveDialog::PlayLost
            | ActiveDialog::TournamentResume
            | ActiveDialog::TournamentDecision
            // Applying sends messages of the shell: `Shell::appearance_key`
            // answers Enter before it gets here.
            | ActiveDialog::Appearance
            // The fields of these send their own lines; Enter does nothing.
            | ActiveDialog::Eboard
            | ActiveDialog::Fics
            | ActiveDialog::EcoClassify => Outcome::default(),
            // Resigning is deliberate: the safe answer is the default.
            ActiveDialog::FicsConfirm => self
                .fics
                .update(crate::windows::fics_dialog::FicsUiMessage::ConfirmResign(false), app),
            // Enter takes the safe answer, "Cancel" (D1792).
            ActiveDialog::BookQuit => {
                self.book.quit = None;
                Outcome::default()
            }
            // Enter starts, as the button does.
            ActiveDialog::TacticsSetup => {
                Outcome::send(self.tactics.start_message(&app.tactics_view()))
            }
            ActiveDialog::OpeningSetup => {
                Outcome::send(self.opening.start_message(&app.opening_view()))
            }
            ActiveDialog::CalvarSetup => {
                Outcome::send(self.calvar.start_message(&app.calvar_view()))
            }
            // Enter searches, as the button does.
            ActiveDialog::AdvancedSearch => Outcome::send(advanced_dialog::start_message()),
            ActiveDialog::PatternSearch => Outcome::send(pattern_dialog::start_message()),
            ActiveDialog::PatternCost => Outcome::send(pattern_dialog::cost_message(true)),
            // Enter closes: the changes were already applied.
            ActiveDialog::Sound => Outcome::send(crate::windows::sound_dialog::cancel_message()),
            ActiveDialog::OnlineConnect
            | ActiveDialog::OnlineSeek
            | ActiveDialog::OnlineChallenge
            | ActiveDialog::OnlineIncoming
            | ActiveDialog::OnlineWatch
            | ActiveDialog::OnlinePromotion
            | ActiveDialog::OnlineConfirm => self.online_key(active, false, app),
            // Enter or a double click starts the selected engine (Scid).
            ActiveDialog::Engines => app
                .engine_dialog
                .as_ref()
                .and_then(|d| d.selected)
                .map_or_else(Outcome::default, |id| {
                    Outcome::send(engine(EngineMessage::Start(id, StartMode::Analyze)))
                }),
            ActiveDialog::EngineEditor => {
                Outcome::send(engine(EngineMessage::Editor(EngineEditorMessage::Save)))
            }
            ActiveDialog::EngineOptions => {
                Outcome::send(engine(EngineMessage::Options(OptionsMessage::Save)))
            }
            // Like the Scid's "Confirm delete", the safe answer is the
            // default.
            ActiveDialog::EngineDelete => Outcome::send(engine(EngineMessage::CancelDelete)),
            // Enter takes the default answer of each (saving, all moves,
            // saving the comment, searching or going to the row).
            ActiveDialog::MaskPrompt => self.mask.enter(MaskDialog::Prompt, app),
            ActiveDialog::MaskFillLine => self.mask.enter(MaskDialog::FillLine, app),
            ActiveDialog::MaskComment => self.mask.enter(MaskDialog::Comment, app),
            ActiveDialog::MaskSearch => self.mask.enter(MaskDialog::Search, app),
            ActiveDialog::MaskOutline => self.mask.enter(MaskDialog::Outline, app),
        }
    }

    /// Esc (`escape`) or Enter in a window of the online play.
    fn online_key(&mut self, active: ActiveDialog, escape: bool, app: &AppState) -> Outcome {
        let which = match active {
            ActiveDialog::OnlineConnect => OnlineActive::Connect,
            ActiveDialog::OnlineSeek => OnlineActive::Seek,
            ActiveDialog::OnlineChallenge => OnlineActive::Challenge,
            ActiveDialog::OnlineIncoming => OnlineActive::Incoming,
            ActiveDialog::OnlineWatch => OnlineActive::Watch,
            ActiveDialog::OnlinePromotion => OnlineActive::Promotion,
            _ => OnlineActive::Confirm,
        };
        if escape {
            self.online.escape(which, app)
        } else {
            self.online.enter(which, app)
        }
    }

    fn arrow(&mut self, active: ActiveDialog, delta: isize, app: &AppState) -> Outcome {
        match active {
            ActiveDialog::Info if self.suggestions_visible(app) => {
                self.header.highlight = autocomplete::move_highlight(
                    self.header.highlight,
                    delta,
                    Self::suggestion_count(app),
                );
                Outcome::default()
            }
            ActiveDialog::Chooser => {
                let mut out = Outcome::send(AppMessage::Chooser(
                    mychess_app::ChooserMessage::Move(delta),
                ));
                out.scroll = app.chooser.as_ref().and_then(|chooser| {
                    let at = chooser
                        .selected
                        .and_then(|s| chooser.visible.iter().position(|v| *v == s))
                        .unwrap_or(0);
                    let last = chooser.visible.len().saturating_sub(1);
                    let target = at.saturating_add_signed(delta).min(last);
                    self.reveal_row(target)
                });
                out
            }
            ActiveDialog::Variations => {
                Outcome::send(AppMessage::Game(GameMessage::ChooserMove(delta)))
            }
            // The numeric fields of the analysis options use no arrow key,
            // but a field in use must not move the selection of the list
            // either (D751).
            ActiveDialog::Engines if self.engines_prefs_open => Outcome::default(),
            ActiveDialog::Engines => {
                let engines = app.engines.engines();
                let at = app
                    .engine_dialog
                    .as_ref()
                    .and_then(|d| d.selected)
                    .and_then(|id| app.engines.position(id));
                let target = match at {
                    Some(at) => at
                        .saturating_add_signed(delta)
                        .min(engines.len().saturating_sub(1)),
                    None => 0,
                };
                engines.get(target).map_or_else(Outcome::default, |e| {
                    Outcome::send(engine(EngineMessage::Select(Some(e.id))))
                })
            }
            _ => Outcome::default(),
        }
    }
}

/// A message of the engine management for the application.
fn engine(message: EngineMessage) -> AppMessage {
    AppMessage::Engine(message)
}

/// A message of the annotation for the application.
fn annotate(message: AnnotateMessage) -> AppMessage {
    AppMessage::Annotate(message)
}

fn eco(message: EcoMessage) -> AppMessage {
    AppMessage::Eco(message)
}

/// A message of the bookmark editor for the application.
fn bookmark_edit(message: EditorMessage) -> AppMessage {
    AppMessage::Bookmarks(BookmarksMessage::Edit(message))
}

/// The first row and the number of rows to build for a list of `total`
/// rows scrolled by `scroll` pixels: the visible ones plus a margin, so
/// that fast scrolling never shows a gap.
pub fn visible_rows(scroll: f32, total: usize) -> (usize, usize) {
    const MARGIN: usize = 3;
    let scroll = if scroll.is_finite() {
        scroll.max(0.0)
    } else {
        0.0
    };
    // The quotient is a small non-negative number; the cast cannot wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let first = (scroll / CHOOSER_ROW_HEIGHT).floor() as usize;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fit = (CHOOSER_LIST_HEIGHT / CHOOSER_ROW_HEIGHT).ceil() as usize;
    let first = first.saturating_sub(MARGIN).min(total);
    let count = (fit + 2 * MARGIN + 1).min(total - first);
    (first, count)
}

/// The application message for a key typed over the board (NAG shortcuts and
/// move entry).
fn typed_message(key: TypedKey, at: Instant) -> AppMessage {
    match key {
        TypedKey::Char(ch) => AppMessage::NagKey {
            key: NagKey::Char(ch),
            at,
        },
        TypedKey::Enter => AppMessage::NagKey {
            key: NagKey::Enter,
            at,
        },
        TypedKey::Interrupt => AppMessage::NagInterrupt,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)]
    use std::time::Duration;

    use mychess_app::update;
    use mychess_core::{Color, Piece, Role};

    use super::*;

    fn press(key: Key) -> KeyInput {
        KeyInput {
            key,
            modifiers: Modifiers::NONE,
        }
    }

    fn app_with_info() -> AppState {
        let mut app = AppState::default();
        update(&mut app, AppMessage::Info(GameInfoMessage::Open));
        app
    }

    #[test]
    fn a_remark_expires_after_eight_seconds_of_ticks_and_a_new_one_restarts_the_clock() {
        let t0 = Instant::now();
        let mut clock = StatusClock::default();
        clock.tick(t0, true);
        clock.tick(t0 + Duration::from_secs(7), true);
        assert!(!clock.expired());
        clock.tick(t0 + Duration::from_secs(8), true);
        assert!(clock.expired());
        clock.reset();
        assert!(!clock.expired());
        clock.tick(t0 + Duration::from_secs(20), true);
        assert!(!clock.expired(), "counting starts at the first tick");
        clock.tick(t0 + Duration::from_secs(30), false);
        assert!(!clock.expired(), "no remark, nothing to expire");
    }

    #[test]
    fn typed_keys_become_nag_keys_and_interrupts() {
        let app = AppState::default();
        let mut ui = DialogUi::default();
        let at = Instant::now();
        let out = ui.update(DialogMessage::Typed(TypedKey::Char('!'), at), &app);
        assert_eq!(
            out.messages,
            [AppMessage::NagKey {
                key: NagKey::Char('!'),
                at
            }]
        );
        let out = ui.update(DialogMessage::Typed(TypedKey::Enter, at), &app);
        assert_eq!(
            out.messages,
            [AppMessage::NagKey {
                key: NagKey::Enter,
                at
            }]
        );
        let out = ui.update(DialogMessage::Typed(TypedKey::Interrupt, at), &app);
        assert_eq!(out.messages, [AppMessage::NagInterrupt]);
        let out = ui.update(
            DialogMessage::MoveEntryKeys(vec![GameMessage::MoveEntryClear]),
            &app,
        );
        assert_eq!(
            out.messages,
            [AppMessage::Game(GameMessage::MoveEntryClear)]
        );
    }

    #[test]
    fn goto_reads_numbers_with_a_black_suffix() {
        let parse = |t: &str| {
            GotoDraft {
                text: t.to_owned(),
                invalid: false,
            }
            .parse()
        };
        assert_eq!(
            parse("12"),
            Some(GotoTarget {
                number: 12,
                black: false
            })
        );
        assert_eq!(
            parse(" 7. "),
            Some(GotoTarget {
                number: 7,
                black: false
            })
        );
        assert_eq!(
            parse("12..."),
            Some(GotoTarget {
                number: 12,
                black: true
            })
        );
        assert_eq!(
            parse("3b"),
            Some(GotoTarget {
                number: 3,
                black: true
            })
        );
        for bad in ["", "0", "-1", "abc", "1.5", "99999999999"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn goto_submit_navigates_or_keeps_the_dialog_with_an_error() {
        let app = AppState::default();
        let mut ui = DialogUi::default();
        let out = ui.update(DialogMessage::OpenGoto, &app);
        assert_eq!(out.focus, Some(GOTO_FIELD_ID));
        ui.update(DialogMessage::GotoEdited("x".to_owned()), &app);
        let out = ui.update(DialogMessage::GotoSubmit, &app);
        assert!(out.messages.is_empty());
        assert!(ui.goto.as_ref().is_some_and(|d| d.invalid));
        ui.update(DialogMessage::GotoEdited("5".to_owned()), &app);
        let out = ui.update(DialogMessage::GotoSubmit, &app);
        assert_eq!(out.messages, [app_api::navigate(Nav::GoToMoveNumber(5))]);
        assert!(ui.goto.is_none());
        ui.update(DialogMessage::OpenGoto, &app);
        ui.update(DialogMessage::GotoEdited("5...".to_owned()), &app);
        let out = ui.update(DialogMessage::GotoSubmit, &app);
        assert_eq!(
            out.messages,
            [app_api::navigate(Nav::GoToBlackMoveNumber(5))]
        );
    }

    fn keys(active: ActiveDialog, app: &AppState, ui: &mut DialogUi) -> [Vec<AppMessage>; 2] {
        let esc = ui.key(active, &press(Key::Named(NamedKey::Escape)), false, app);
        let enter = ui.key(active, &press(Key::Named(NamedKey::Enter)), false, app);
        [esc.messages, enter.messages]
    }

    #[test]
    fn the_boxes_of_the_game_answer_escape_and_enter() {
        use mychess_app::PlayMessage as P;
        let app = AppState::default();
        let mut ui = DialogUi::default();
        // The question: Esc = No, Enter = Yes.
        let [esc, enter] = keys(ActiveDialog::PlayAsk, &app, &mut ui);
        assert!(matches!(
            esc.as_slice(),
            [AppMessage::Play(P::Answer { yes: false, .. })]
        ));
        assert!(matches!(
            enter.as_slice(),
            [AppMessage::Play(P::Answer { yes: true, .. })]
        ));
        // The end box: both close it.
        let [esc, enter] = keys(ActiveDialog::PlayEnd, &app, &mut ui);
        assert!(matches!(esc.as_slice(), [AppMessage::Play(P::Quit(_))]));
        assert!(matches!(enter.as_slice(), [AppMessage::Play(P::Quit(_))]));
        // The lost engine: neither key leaves.
        let [esc, enter] = keys(ActiveDialog::PlayLost, &app, &mut ui);
        assert!(esc.is_empty() && enter.is_empty());
    }

    #[test]
    fn the_confirmation_answers_escape_no_and_enter_yes() {
        use crate::windows::play_dialog::PlayConfirm;
        use mychess_app::PlayMessage as P;
        let app = AppState::default();
        let mut ui = DialogUi::default();
        ui.play.confirm = Some(PlayConfirm::Resign);
        assert_eq!(ActiveDialog::of(&app, &ui), Some(ActiveDialog::PlayConfirm));
        let esc = ui.key(
            ActiveDialog::PlayConfirm,
            &press(Key::Named(NamedKey::Escape)),
            false,
            &app,
        );
        assert!(esc.messages.is_empty());
        assert!(ui.play.confirm.is_none());
        ui.play.confirm = Some(PlayConfirm::Resign);
        let enter = ui.key(
            ActiveDialog::PlayConfirm,
            &press(Key::Named(NamedKey::Enter)),
            false,
            &app,
        );
        assert!(matches!(
            enter.messages.as_slice(),
            [AppMessage::Play(P::Resign(_))]
        ));
    }

    #[test]
    fn escape_closes_goto_and_enter_submits_it() {
        let app = AppState::default();
        let mut ui = DialogUi::default();
        ui.update(DialogMessage::OpenGoto, &app);
        assert_eq!(ActiveDialog::of(&app, &ui), Some(ActiveDialog::Goto));
        ui.update(DialogMessage::GotoEdited("2".to_owned()), &app);
        let out = ui.key(
            ActiveDialog::Goto,
            &press(Key::Named(NamedKey::Enter)),
            false,
            &app,
        );
        assert_eq!(out.messages, [app_api::navigate(Nav::GoToMoveNumber(2))]);
        ui.update(DialogMessage::OpenGoto, &app);
        ui.key(
            ActiveDialog::Goto,
            &press(Key::Named(NamedKey::Escape)),
            true,
            &app,
        );
        assert!(ui.goto.is_none());
    }

    #[test]
    fn enter_in_a_text_field_is_left_to_the_field() {
        let app = AppState::default();
        let mut ui = DialogUi::default();
        ui.update(DialogMessage::OpenGoto, &app);
        let out = ui.key(
            ActiveDialog::Goto,
            &press(Key::Named(NamedKey::Enter)),
            true,
            &app,
        );
        assert_eq!(out, Outcome::default());
    }

    #[test]
    fn tab_cycles_inside_the_dialog_in_both_directions() {
        let app = AppState::default();
        let mut ui = DialogUi::default();
        let tab = press(Key::Named(NamedKey::Tab));
        assert_eq!(
            ui.key(ActiveDialog::Goto, &tab, false, &app).cycle,
            Some(false)
        );
        let back = KeyInput {
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            ..tab
        };
        assert_eq!(
            ui.key(ActiveDialog::Goto, &back, false, &app).cycle,
            Some(true)
        );
    }

    #[test]
    fn opening_the_header_asks_for_the_field_and_focuses_it_once() {
        let mut app = AppState::default();
        let mut ui = DialogUi::default();
        let out = ui.update(DialogMessage::OpenHeader(Some(TagField::Black)), &app);
        for message in out.messages {
            update(&mut app, message);
        }
        assert_eq!(ui.newly_opened(&app), Some("hdr-black"));
        assert_eq!(ui.newly_opened(&app), None, "only when it opens");
    }

    #[test]
    fn typing_a_name_shows_suggestions_that_escape_hides_before_cancel() {
        let mut app = app_with_info();
        app.names
            .observe(mychess_app::NameKind::Player, "Carlsen, M.");
        let mut ui = DialogUi::default();
        let out = ui.update(
            DialogMessage::HeaderText(TagField::White, "car".to_owned()),
            &app,
        );
        for message in out.messages {
            update(&mut app, message);
        }
        assert!(ui.suggestions_visible(&app));
        let down = press(Key::Named(NamedKey::Down));
        ui.key(ActiveDialog::Info, &down, true, &app);
        assert_eq!(ui.header.highlight, Some(0));
        let esc = press(Key::Named(NamedKey::Escape));
        let out = ui.key(ActiveDialog::Info, &esc, true, &app);
        assert!(out.messages.is_empty(), "the first Esc closes the list");
        assert!(!ui.suggestions_visible(&app));
        let out = ui.key(ActiveDialog::Info, &esc, true, &app);
        assert_eq!(out.messages, [AppMessage::Info(GameInfoMessage::Cancel)]);
    }

    #[test]
    fn enter_on_a_highlighted_suggestion_picks_it_instead_of_applying() {
        let mut app = app_with_info();
        app.names
            .observe(mychess_app::NameKind::Player, "Carlsen, M.");
        let mut ui = DialogUi::default();
        let out = ui.update(
            DialogMessage::HeaderText(TagField::White, "car".to_owned()),
            &app,
        );
        for message in out.messages {
            update(&mut app, message);
        }
        let none = ui.update(DialogMessage::HeaderSubmit, &app);
        assert_eq!(none.messages, [AppMessage::Info(GameInfoMessage::Apply)]);
        ui.header.highlight = Some(0);
        let out = ui.update(DialogMessage::HeaderSubmit, &app);
        assert_eq!(
            out.messages,
            [AppMessage::Info(GameInfoMessage::SetText(
                TagField::White,
                "Carlsen, M.".to_owned()
            ))]
        );
        assert!(ui.header.hidden);
    }

    #[test]
    fn a_press_and_release_on_one_square_is_a_click_and_elsewhere_a_drop() {
        let mut app = AppState::default();
        update(&mut app, AppMessage::Setup(SetupMessage::Clear));
        update(
            &mut app,
            AppMessage::Command(mychess_app::Command::SetupBoard),
        );
        let mut ui = DialogUi::default();
        ui.update(DialogMessage::SetupPress(Square::E4), &app);
        let out = ui.update(DialogMessage::SetupRelease(Square::E4), &app);
        assert_eq!(
            out.messages,
            [AppMessage::Setup(SetupMessage::ClickSquare(Square::E4))]
        );
        // Squares with pieces: the clear board has the kings.
        ui.update(DialogMessage::SetupPress(Square::G2), &app);
        let out = ui.update(DialogMessage::SetupRelease(Square::G3), &app);
        assert_eq!(
            out.messages,
            [AppMessage::Setup(SetupMessage::Drop {
                from: Square::G2,
                to: Some(Square::G3)
            })]
        );
        ui.update(DialogMessage::SetupPress(Square::G2), &app);
        let out = ui.update(DialogMessage::SetupAreaRelease, &app);
        assert_eq!(
            out.messages,
            [AppMessage::Setup(SetupMessage::Drop {
                from: Square::G2,
                to: None
            })]
        );
        // Dragging from an empty square does nothing.
        ui.update(DialogMessage::SetupPress(Square::A5), &app);
        let out = ui.update(DialogMessage::SetupRelease(Square::A6), &app);
        assert!(out.messages.is_empty());
        let _ = (
            Color::White,
            Piece {
                color: Color::White,
                role: Role::King,
            },
        );
    }

    #[test]
    fn the_counters_keep_digits_and_send_only_valid_numbers() {
        let mut app = AppState::default();
        update(
            &mut app,
            AppMessage::Command(mychess_app::Command::SetupBoard),
        );
        let mut ui = DialogUi::default();
        ui.sync(&app);
        assert_eq!(ui.setup.fullmove, "1");
        let out = ui.update(DialogMessage::SetupFullmove("4x2".to_owned()), &app);
        assert_eq!(ui.setup.fullmove, "42");
        assert_eq!(
            out.messages,
            [AppMessage::Setup(SetupMessage::SetFullmove(42))]
        );
        let out = ui.update(DialogMessage::SetupFullmove(String::new()), &app);
        assert!(
            out.messages.is_empty(),
            "an empty field waits for the next digit"
        );
        let out = ui.update(DialogMessage::SetupHalfmove("0".to_owned()), &app);
        assert_eq!(
            out.messages,
            [AppMessage::Setup(SetupMessage::SetHalfmove(0))]
        );
    }

    #[test]
    fn the_import_editor_follows_the_text_of_the_application() {
        let mut app = AppState::default();
        update(&mut app, AppMessage::Import(ImportMessage::Open));
        let mut ui = DialogUi::default();
        ui.sync(&app);
        update(
            &mut app,
            AppMessage::Import(ImportMessage::TextEdited("1. e4 e5".to_owned())),
        );
        ui.sync(&app);
        assert_eq!(ui.paste.content.text(), "1. e4 e5");
        let out = ui.update(
            DialogMessage::PasteEdit(text_editor::Action::Edit(text_editor::Edit::Insert('!'))),
            &app,
        );
        let [AppMessage::Import(ImportMessage::TextEdited(text))] = out.messages.as_slice() else {
            panic!("an edit is sent: {:?}", out.messages);
        };
        assert!(text.contains('!'));
        update(&mut app, AppMessage::Import(ImportMessage::Close));
        ui.sync(&app);
        assert!(ui.paste.content.is_empty());
    }

    #[test]
    fn the_game_list_builds_only_the_visible_window() {
        assert_eq!(visible_rows(0.0, 0), (0, 0));
        let (first, count) = visible_rows(0.0, 50_000);
        assert_eq!(first, 0);
        assert!(count < 30, "{count} rows built for a 340 px list");
        let (first, count) = visible_rows(26.0 * 1000.0, 50_000);
        assert_eq!(first, 997);
        assert!(count < 30);
        let (first, count) = visible_rows(26.0 * 1_000_000.0, 50_000);
        assert_eq!((first, count), (50_000, 0), "scrolled past the end");
        assert_eq!(visible_rows(f32::NAN, 10).0, 0);
    }

    #[test]
    fn revealing_a_row_scrolls_only_when_it_is_off_screen() {
        let ui = DialogUi::default();
        assert_eq!(ui.reveal_row(0), None);
        assert_eq!(ui.reveal_row(5), None);
        let below = ui.reveal_row(100).expect("off screen");
        assert!(below > 0.0);
        let mut ui = DialogUi::default();
        ui.chooser.scroll = 26.0 * 50.0;
        assert_eq!(ui.reveal_row(10), Some(26.0 * 10.0));
    }

    #[test]
    fn the_order_of_precedence_puts_questions_first() {
        let mut app = AppState::default();
        let ui = DialogUi::default();
        assert_eq!(ActiveDialog::of(&app, &ui), None);
        update(
            &mut app,
            AppMessage::Command(mychess_app::Command::SetupBoard),
        );
        assert_eq!(ActiveDialog::of(&app, &ui), Some(ActiveDialog::Setup));
        update(&mut app, AppMessage::Import(ImportMessage::Open));
        assert_eq!(ActiveDialog::of(&app, &ui), Some(ActiveDialog::Setup));
    }

    #[test]
    fn numeric_fields_of_the_engines_dialog_keep_their_text_and_ignore_the_arrows() {
        let app = AppState::default();
        let mut ui = DialogUi::default();
        let out = ui.update(
            DialogMessage::EnginesPrefsText(PrefsNumber::MaxPly, "4x2".to_owned()),
            &app,
        );
        assert_eq!(ui.engines_prefs_text[1].as_deref(), Some("42"));
        assert_eq!(out.messages.len(), 1);
        // The same text again changes nothing (the inert submit).
        let mut app = app;
        app.prefs.analysis.max_ply = 42;
        let out = ui.update(
            DialogMessage::EnginesPrefsText(PrefsNumber::MaxPly, "42".to_owned()),
            &app,
        );
        assert!(out.messages.is_empty());
        // Emptying the field is kept as text and stores 0.
        let out = ui.update(
            DialogMessage::EnginesPrefsText(PrefsNumber::MaxPly, String::new()),
            &app,
        );
        assert_eq!(ui.engines_prefs_text[1].as_deref(), Some(""));
        assert_eq!(out.messages.len(), 1);
        ui.engines_prefs_open = true;
        assert!(ui.arrow(ActiveDialog::Engines, 1, &app).messages.is_empty());
    }

    #[test]
    fn chess960_dialog_opens_on_518_submits_and_keeps_errors() {
        let app = AppState::default();
        let mut ui = DialogUi::default();
        let out = ui.update(DialogMessage::OpenChess960, &app);
        assert_eq!(out.focus, Some(CHESS960_FIELD_ID));
        assert_eq!(ActiveDialog::of(&app, &ui), Some(ActiveDialog::Chess960));
        ui.update(DialogMessage::Chess960Edited("960".to_owned()), &app);
        let out = ui.update(DialogMessage::Chess960Submit, &app);
        assert!(out.messages.is_empty());
        assert!(ui.chess960.as_ref().is_some_and(|d| d.invalid));
        ui.update(DialogMessage::Chess960Edited("42".to_owned()), &app);
        let out = ui.update(DialogMessage::Chess960Submit, &app);
        assert_eq!(
            out.messages,
            [app_api::command(mychess_app::Command::NewChess960(42))]
        );
        assert!(ui.chess960.is_none());
    }

    #[test]
    fn chess960_random_fills_a_valid_number_and_escape_closes() {
        let app = AppState::default();
        let mut ui = DialogUi::default();
        ui.update(DialogMessage::OpenChess960, &app);
        ui.update(DialogMessage::Chess960Random, &app);
        assert!(
            ui.chess960
                .as_ref()
                .and_then(Chess960Draft::number)
                .is_some()
        );
        ui.key(
            ActiveDialog::Chess960,
            &press(Key::Named(NamedKey::Escape)),
            true,
            &app,
        );
        assert!(ui.chess960.is_none());
    }
}
