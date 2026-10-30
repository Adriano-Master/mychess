//! The player report window and its dialog (spec M7-3c, decisions
//! M7-3/D4200-D4299; Scid `tcl/tools/preport.tcl`).
//!
//! The application owns the report, the dialog's first choices and the job
//! ([`mychess_app::player_report`]); this module draws them and keeps only
//! what the screen needs: the draft of the dialog (the application only
//! seeds it), the form to save and the language the application was last
//! given the words of.
//!
//! Like every window of this front-end it is a layer of the main window: the
//! report covers it with a margin, the dialog and the questions are cards
//! over it. While one is open the keys of the game behind are blocked
//! ([`crate::shell::Shell::modal_layer_open`]).
//!
//! - `dialog`: the dialog "Generate player report", the question about the
//!   Clipbase and the save form;
//! - `view`: the title bar, the buttons, the progress and the report.

mod dialog;
pub mod view;

use std::path::PathBuf;

use iced::Element;
use iced::widget::{column, container, opaque, stack};
use mychess_app::player_report::{
    NameMatching, PlayerColor, PlayerRequest, PlayerStart, ReportFormat, ReportPart,
};
use mychess_app::{AppState, Message as AppMessage};

use crate::i18n::Language;
use crate::shell::{Message, Shell};
use crate::theme;

/// Id of the card of the window (for tests and automation).
pub const PREPORT_WINDOW_ID: &str = "preport-window";
/// Id of the card of the dialog.
pub const PREPORT_DIALOG_ID: &str = "preport-dialog";
/// Id of the field of the name of the player.
pub const PREPORT_NAME_ID: &str = "preport-name";
/// Id of the card of the question about the Clipbase.
pub const PREPORT_CONFIRM_ID: &str = "preport-confirm";
/// The question "replace the file?" of the save (D4211).
pub const PREPORT_OVERWRITE_ID: &str = "preport-overwrite";
/// Id of the card of "save".
pub const PREPORT_SAVE_BOX_ID: &str = "preport-save-box";

/// The part and format chosen to save a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveDraft {
    /// The part of the report.
    pub part: ReportPart,
    /// The format of the file.
    pub format: ReportFormat,
}

/// The form that covers the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrForm {
    /// Part and format to save.
    Save(SaveDraft),
}

/// What the window keeps on its own.
#[derive(Debug, Default)]
pub struct PrUi {
    /// The choices on the dialog, a draft until "OK"; `Some` while the
    /// application has the dialog open.
    pub draft: Option<PlayerRequest>,
    /// The form that is open.
    pub form: Option<PrForm>,
    /// The player and colour of the report on the window, to fill the dialog
    /// "Change..." opens.
    pub(crate) change: Option<(String, PlayerColor)>,
    /// The language the application was last given the words of.
    pub(crate) labels_language: Option<Language>,
    /// Pixels the report is scrolled down (kept across redraws).
    pub(crate) was_open: bool,
}

impl PrUi {
    /// Whether the window or its dialog covers the main window. Cheap: it is
    /// asked many times per frame.
    pub fn layer_open(app: &AppState) -> bool {
        let report = app.player_report();
        report.is_open() || report.dialog_open()
    }

    /// Forgets the state of a window that closed.
    pub(crate) fn reset(&mut self) {
        self.draft = None;
        self.form = None;
        self.change = None;
    }
}

/// An event of the window that only the front-end handles, or that needs the
/// state of the front-end to become a message of the application.
#[derive(Debug, Clone)]
pub enum PrUiMessage {
    /// "Player Report" of the Tools menu or the button of the player
    /// information: opens the dialog (`player`: the one to fill in).
    Open(Option<String>),
    /// The name field changed.
    Name(String),
    /// A recent name was clicked.
    Recent(String),
    /// A colour was chosen.
    Color(PlayerColor),
    /// A way of matching the name was chosen.
    Matching(NameMatching),
    /// A start position was chosen.
    Start(PlayerStart),
    /// The Clipbase box.
    Clipbase(bool),
    /// "White (name)" or "Black (name)": fills the name and the colour.
    FromGame(PlayerColor),
    /// "OK" of the dialog (or `Enter` in the name field).
    Generate,
    /// "Cancel" of the dialog.
    Cancel,
    /// The answer to "the Clipbase will be emptied".
    Confirm(bool),
    /// The answer to "replace the file?" (the extension hit an existing
    /// file, D4211).
    ConfirmSave(bool),
    /// "Change...": the dialog again, with the choices of the report.
    Change,
    /// Closes the window.
    Close,
    /// "Update".
    Refresh,
    /// "Stop" of the progress.
    Stop,
    /// Opens the save form.
    OpenSave,
    /// Closes the form that is open.
    CloseForm,
    /// A part chosen in the save form.
    SavePart(ReportPart),
    /// A format chosen in the save form.
    SaveFormat(ReportFormat),
    /// "Save as...": shows the file dialog.
    SaveChoose,
    /// The answer of the file dialog (`None`: cancelled).
    SavePicked(Option<PathBuf>),
}

pub(crate) fn ui(message: PrUiMessage) -> Message {
    Message::PlayerReportUi(message)
}

pub(crate) fn prep(message: mychess_app::player_report::PlayerReportMessage) -> Message {
    Message::App(AppMessage::PlayerReport(message))
}

/// The layer to put over the window, if the report window or its dialog is
/// open.
pub fn layer(shell: &Shell) -> Option<Element<'_, Message>> {
    if !PrUi::layer_open(&shell.app) {
        return None;
    }
    let report = shell.app.player_report_view();
    let mut layers = stack![];
    if report.open {
        let card = container(
            column![
                view::title_bar(shell, &report),
                view::tools(shell, &report),
                view::banners(shell, &report),
                view::content(shell, &report),
                view::footer(shell),
            ]
            .spacing(8),
        )
        .id(PREPORT_WINDOW_ID)
        .padding(14)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .style(theme::pane_body);
        layers = layers.push(opaque(
            container(card)
                .padding(18)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill)
                .style(theme::scrim),
        ));
    }
    if let Some(form) = dialog::overlay(shell, &report) {
        layers = layers.push(form);
    }
    Some(layers.into())
}
