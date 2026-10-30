//! The EPD panel (spec M7-5c, D3801, D3824-D3826, D3850-D3859): one tab per
//! open file (up to four), the virtualized list of positions, the editor of
//! the opcodes of the selected one, the toolbar, the status line and the
//! forms (analysis, strip, ignored lines, the questions).
//!
//! The application owns the files (`AppState::epd_view`); this module keeps
//! only what the screen needs: the editor content, the scroll of the list,
//! what is typed in the search field and the form that is open.
//!
//! - `text`: the translated words of the notices and the diagnostics;
//! - `toolbar`: the tabs, the buttons and the filter row;
//! - `list`: the virtualized list;
//! - `editor`: the text of the record and the status line;
//! - `forms`: the modal cards.

mod editor;
mod forms;
mod list;
pub mod text;
mod toolbar;

use iced::widget::{column, container, text as label, text_editor};
use iced::{Element, Length};
use mychess_app::engines::EngineId;
use mychess_app::epd_window::{
    AnnotateConfig, AnnotateMode, EpdFileId, EpdPrefs, EpdTabView, EpdView, RunScope, SECONDS_RANGE,
};
use mychess_app::{AppState, Message as AppMessage, PrefChange, PrefsMessage};

use crate::shell::{Message, Shell};
use crate::windows::panel_kit::{BODY_TEXT, action, muted};
use crate::windows::panes;

pub use forms::overlay;
pub use list::ROW_HEIGHT;

/// Pixels from the top of the list to the top of row `row` (from 0).
pub fn row_top(row: usize) -> f32 {
    // Row counts stay far below 2^24.
    #[allow(clippy::cast_precision_loss)]
    let top = row as f32 * ROW_HEIGHT;
    top
}

/// Id of the body of the panel (for tests and automation).
pub const EPD_PANE_ID: &str = "epd-pane";
/// Id of the scrollable list of positions.
pub const EPD_LIST_ID: &str = "epd-list";
/// Id of the text editor of the record.
pub const EPD_EDITOR_ID: &str = "epd-editor";
/// Id of the filter field.
pub const EPD_FILTER_ID: &str = "epd-filter";
/// Id of the search field.
pub const EPD_FIND_ID: &str = "epd-find";
/// Id of the field of the opcode to strip.
pub const EPD_STRIP_ID: &str = "epd-strip-field";
/// Id of the field of the seconds of the analysis.
pub const EPD_SECONDS_ID: &str = "epd-seconds-field";
/// Id of the card of the analysis form.
pub const EPD_ANALYZE_BOX_ID: &str = "epd-analyze-box";
/// Id of the card of the strip form.
pub const EPD_STRIP_BOX_ID: &str = "epd-strip-box";
/// Id of the card of the list of ignored lines.
pub const EPD_REPORT_BOX_ID: &str = "epd-report-box";
/// Id of the card of the "save before closing?" question.
pub const EPD_CLOSE_BOX_ID: &str = "epd-close-box";
/// Id of the card of the "remove this position?" question.
pub const EPD_REMOVE_BOX_ID: &str = "epd-remove-box";
/// Id of the card of the "quit with unsaved files?" question.
pub const EPD_QUIT_BOX_ID: &str = "epd-quit-box";
/// Id of the card of the "replace the undo?" question.
pub const EPD_UNDO_BOX_ID: &str = "epd-undo-box";

/// Every text field of the panel and of its forms, for the focus probe.
pub const EPD_FIELD_IDS: [&str; 5] = [
    EPD_EDITOR_ID,
    EPD_FILTER_ID,
    EPD_FIND_ID,
    EPD_STRIP_ID,
    EPD_SECONDS_ID,
];

/// The choices of the form of the analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeDraft {
    /// The engine; `None` takes the running one or the first.
    pub engine: Option<EngineId>,
    /// What is typed in the field of the seconds.
    pub seconds: String,
    /// What the analysis does.
    pub mode: AnnotateMode,
    /// Which positions.
    pub scope: RunScope,
}

impl AnalyzeDraft {
    /// The draft the preferences propose.
    pub fn from_prefs(prefs: &EpdPrefs) -> AnalyzeDraft {
        AnalyzeDraft {
            engine: prefs.annotate_engine.map(EngineId),
            seconds: prefs.annotate_seconds.to_string(),
            mode: prefs.annotate_mode,
            scope: RunScope::All,
        }
    }

    /// The seconds typed, when they are in range (1-300).
    pub fn seconds_value(&self) -> Option<u32> {
        self.seconds
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|s| SECONDS_RANGE.contains(s))
    }

    /// The configuration to send, when the seconds are valid.
    pub fn config(&self) -> Option<AnnotateConfig> {
        Some(AnnotateConfig {
            engine: self.engine,
            seconds: self.seconds_value()?,
            mode: self.mode,
            scope: self.scope,
        })
    }
}

/// The form or question that covers the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EpdForm {
    /// "Analyze EPD".
    Analyze(AnalyzeDraft),
    /// The new analysis replaces the undo of the last one: confirm.
    ConfirmUndo(AnnotateConfig),
    /// "Strip opcodes": the opcode typed.
    Strip(String),
    /// The lines of the file that were ignored when it was read.
    Report(EpdFileId),
    /// Quitting with files altered.
    ConfirmQuit,
}

/// What the panel keeps on its own.
#[derive(Debug)]
pub struct EpdUi {
    /// The text of the editor of the record.
    pub content: text_editor::Content,
    /// Pixels the list is scrolled down.
    pub scroll: f32,
    /// The height of the viewport of the list, when it has been scrolled
    /// (`0.0`: not known yet).
    pub view_h: f32,
    /// What is typed in the search field.
    pub find_input: String,
    /// The form or question that is open.
    pub form: Option<EpdForm>,
    /// The text field of the panel that has the focus, if any.
    pub focused: Option<&'static str>,
    /// The user confirmed quitting with unsaved files.
    pub quit_confirmed: bool,
    /// The form that was open when the quit question replaced it.
    pub(crate) form_before_quit: Option<EpdForm>,
    /// Whether a form was open at the last synchronization.
    pub(crate) form_was_open: bool,
    /// Tabs seen by the last synchronization: a bigger count opens the panel.
    pub(crate) seen_tabs: usize,
    /// The row last scrolled into view, with its file.
    pub(crate) revealed: Option<(EpdFileId, usize)>,
}

impl Default for EpdUi {
    fn default() -> EpdUi {
        EpdUi {
            content: text_editor::Content::new(),
            scroll: 0.0,
            view_h: 0.0,
            find_input: String::new(),
            form: None,
            focused: None,
            quit_confirmed: false,
            form_before_quit: None,
            form_was_open: false,
            seen_tabs: 0,
            revealed: None,
        }
    }
}

impl EpdUi {
    /// Whether a form or question covers the window (a modal layer).
    pub fn layer_open(&self, app: &AppState) -> bool {
        self.form.is_some() || app.epd_layer_open()
    }

    /// Makes the editor show `text` (the cursor goes to the start).
    pub(crate) fn set_content(&mut self, text: &str) {
        self.content = text_editor::Content::with_text(text);
    }
}

/// A message of the panel that only the front-end handles.
#[derive(Debug, Clone)]
pub enum EpdUiMessage {
    /// The text editor produced an action.
    Editor(text_editor::Action),
    /// The list was scrolled.
    Scrolled {
        /// Pixels down.
        offset: f32,
        /// The height of the viewport.
        height: f32,
    },
    /// The search field changed.
    FindInput(String),
    /// Search from the selected position on (`true`: forward).
    Find(bool),
    /// Load a position drawn at random.
    Random,
    /// Open the form of the analysis.
    OpenAnalyze,
    /// Open the form of the opcode to strip.
    OpenStrip,
    /// Open the list of ignored lines of the active file.
    OpenReport,
    /// Close the form that is open.
    CloseForm,
    /// The engine of the analysis form.
    AnalyzeEngine(Option<EngineId>),
    /// The seconds field of the analysis form.
    AnalyzeSeconds(String),
    /// The mode of the analysis form.
    AnalyzeMode(AnnotateMode),
    /// The scope of the analysis form.
    AnalyzeScope(RunScope),
    /// "Start" of the analysis form.
    AnalyzeStart,
    /// The answer to "replace the undo of the last analysis?".
    ConfirmUndo(bool),
    /// The opcode field of the strip form.
    StripInput(String),
    /// "Remove" of the strip form.
    StripGo,
    /// The preference "turn the board to the side to move".
    SetFlip(bool),
    /// The preference "write in the dialect of the Scid".
    SetDialectScid(bool),
    /// A text field gained or lost the focus (the answer of the probe).
    Focus(&'static str, bool),
    /// The answer to "quit with unsaved EPD files?".
    ConfirmQuit(bool),
}

/// The message of the application for an event of the EPD window.
pub(crate) fn epd(message: mychess_app::epd_window::EpdMessage) -> Message {
    Message::App(AppMessage::Epd(message))
}

/// The message that stores the preferences of the window.
pub(crate) fn set_prefs(prefs: EpdPrefs) -> AppMessage {
    AppMessage::Prefs(PrefsMessage::Set(PrefChange::Epd(Box::new(prefs))))
}

/// The tab of the active file.
pub(crate) fn active_tab(view: &EpdView) -> Option<&EpdTabView> {
    view.active
        .and_then(|id| view.tabs.iter().find(|t| t.id == id))
        .or_else(|| view.tabs.first())
}

/// The panel with no file open: the two ways in and the recent files.
fn empty<'a>(shell: &'a Shell, view: &EpdView) -> Element<'a, Message> {
    let i18n = &shell.i18n;
    let mut page = column![
        label(i18n.tr("epd-empty")).size(15),
        muted(i18n.tr("epd-empty-hint")),
        iced::widget::row![
            action(
                i18n.tr("epd-open"),
                Some(epd(mychess_app::epd_window::EpdMessage::Open))
            ),
            action(
                i18n.tr("epd-new"),
                Some(epd(mychess_app::epd_window::EpdMessage::New))
            ),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .padding(16);
    if !view.recent.is_empty() {
        page = page.push(label(i18n.tr("epd-recent")).size(BODY_TEXT));
        for path in view.recent.iter().take(5) {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            page = page.push(action(
                name,
                Some(epd(mychess_app::epd_window::EpdMessage::OpenPath(
                    path.clone(),
                ))),
            ));
        }
    }
    if view.opening {
        page = page.push(muted(i18n.tr("epd-opening")));
    }
    page.into()
}

/// The panel.
pub fn view(shell: &Shell) -> Element<'_, Message> {
    let view = shell.app.epd_view();
    let body: Element<'_, Message> = match active_tab(&view) {
        None => empty(shell, &view),
        Some(tab) => {
            let tab = tab.clone();
            column![
                toolbar::tabs(shell, &view),
                toolbar::buttons(shell, &tab),
                toolbar::filter_row(shell, &tab),
                list::view(shell, &tab),
                editor::view(shell, &tab),
                editor::status(shell, &tab),
            ]
            .spacing(6)
            .padding(8)
            .height(Length::Fill)
            .into()
        }
    };
    panes::body(container(body).width(Length::Fill), EPD_PANE_ID)
}
