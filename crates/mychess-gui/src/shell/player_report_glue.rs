//! The part of the shell that serves the player report window (spec M7-3c):
//! it opens the dialog, keeps the draft of the dialog, gives the application
//! the words of the documents, runs the file dialog and turns the events of
//! the forms into messages of the application.

use iced::Task;
use mychess_app::player_report::{PlayerColor, PlayerReportMessage, PlayerRequest, player_labels};
use mychess_app::{KeyInput, Message as AppMessage, PlayerInfoMessage};

use super::{Message, Shell};
use crate::windows::opening_report::files::SaveReportDialog;
use crate::windows::player_report::{PREPORT_NAME_ID, PrForm, PrUi, PrUiMessage, SaveDraft};

fn prep(message: PlayerReportMessage) -> AppMessage {
    AppMessage::PlayerReport(message)
}

impl Shell {
    /// Whether the player report window or its dialog covers the main
    /// window.
    pub(super) fn preport_layer_open(&self) -> bool {
        PrUi::layer_open(&self.app)
    }

    /// The words of the documents in the language of the interface.
    fn preport_labels(&self) -> mychess_app::player_report::PlayerLabels {
        player_labels(&|key, args| {
            if !self.i18n.has(key) {
                return None;
            }
            let mut fluent = fluent_bundle::FluentArgs::new();
            for (name, value) in args {
                fluent.set(*name, *value);
            }
            Some(self.i18n.tr_args(key, &fluent))
        })
    }

    /// Brings the window up to date with the application: the draft of the
    /// dialog is made when the dialog opens, the words go to the application
    /// when the language changes, and a closed window forgets its forms.
    pub(super) fn sync_preport(&mut self) -> Task<Message> {
        let open = self.preport_layer_open();
        if !open {
            if self.ui.dialogs.preport.was_open {
                self.ui.dialogs.preport.reset();
            }
            self.ui.dialogs.preport.was_open = false;
            self.ui.dialogs.preport.labels_language = None;
            return Task::none();
        }
        self.ui.dialogs.preport.was_open = true;
        let mut tasks = Vec::new();
        let language = self.i18n.language();
        if self.ui.dialogs.preport.labels_language != Some(language) {
            self.ui.dialogs.preport.labels_language = Some(language);
            let labels = self.preport_labels();
            tasks.push(self.dispatch(prep(PlayerReportMessage::SetLabels(Box::new(labels)))));
        }
        match self.app.player_report_dialog() {
            Some(view) => {
                if self.ui.dialogs.preport.draft.is_none() {
                    let mut request = view.request;
                    if let Some((name, color)) = self.ui.dialogs.preport.change.take() {
                        request.name = name;
                        request.color = color;
                    }
                    self.ui.dialogs.preport.draft = Some(request);
                    tasks.push(iced::widget::operation::focus(PREPORT_NAME_ID));
                }
            }
            None => self.ui.dialogs.preport.draft = None,
        }
        Task::batch(tasks)
    }

    fn draft_mut(&mut self) -> Option<&mut PlayerRequest> {
        self.ui.dialogs.preport.draft.as_mut()
    }

    /// Esc: closes the question, then the form, then the dialog, then the
    /// window.
    pub(super) fn preport_escape(&mut self) -> Task<Message> {
        if self.app.player_report_view().confirm_clipbase {
            return self.update_preport(PrUiMessage::Confirm(false));
        }
        if self.app.player_report_view().confirm_overwrite.is_some() {
            return self.update_preport(PrUiMessage::ConfirmSave(false));
        }
        if self.ui.dialogs.preport.form.take().is_some() {
            return Task::none();
        }
        if self.app.player_report().dialog_open() {
            return self.update_preport(PrUiMessage::Cancel);
        }
        self.update_preport(PrUiMessage::Close)
    }

    /// A key while the window is open: only Esc, F5 and the keys of the
    /// program window act; the game behind is blocked.
    pub(super) fn preport_key(&mut self, input: &KeyInput, text_focused: bool) -> Task<Message> {
        use mychess_app::{Action, Key, Modifiers, NamedKey};
        match (input.key, input.modifiers) {
            (Key::Named(NamedKey::Escape), Modifiers::NONE) => self.preport_escape(),
            (Key::Named(NamedKey::F(5)), Modifiers::NONE)
                if self.ui.dialogs.preport.form.is_none()
                    && !self.app.player_report().dialog_open()
                    && !self.app.player_report().is_running() =>
            {
                self.update_preport(PrUiMessage::Refresh)
            }
            _ => match self.app.shortcuts.lookup_in(input, text_focused) {
                Some(action @ (Action::Quit | Action::FullScreen)) => self.run_action(action),
                _ => Task::none(),
            },
        }
    }

    /// An event of the player report window.
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(super) fn update_preport(&mut self, message: PrUiMessage) -> Task<Message> {
        use PrUiMessage as M;
        match message {
            M::Open(player) => {
                self.ui.dialogs.preport.draft = None;
                // The information card is above the report layer: it
                // closes first so that the dialog is the top card (D4210).
                // Only the click on an opponent keeps it open (D4203).
                let close = if self.app.player_info().is_open() {
                    self.dispatch(AppMessage::PlayerInfo(PlayerInfoMessage::Close))
                } else {
                    Task::none()
                };
                let open = self.dispatch(prep(PlayerReportMessage::OpenDialog { player }));
                Task::batch([close, open])
            }
            M::Name(text) => {
                if let Some(d) = self.draft_mut() {
                    d.name = text;
                }
                Task::none()
            }
            M::Recent(name) => {
                if let Some(d) = self.draft_mut() {
                    d.name = name;
                }
                Task::none()
            }
            M::Color(color) => {
                if let Some(d) = self.draft_mut() {
                    d.color = color;
                }
                Task::none()
            }
            M::Matching(matching) => {
                if let Some(d) = self.draft_mut() {
                    d.matching = matching;
                }
                Task::none()
            }
            M::Start(start) => {
                if let Some(d) = self.draft_mut() {
                    d.start = start;
                }
                Task::none()
            }
            M::Clipbase(on) => {
                if let Some(d) = self.draft_mut() {
                    d.copy_to_clipbase = on;
                }
                Task::none()
            }
            M::FromGame(color) => {
                let Some(view) = self.app.player_report_dialog() else {
                    return Task::none();
                };
                let name = match color {
                    PlayerColor::White => view.white_name,
                    PlayerColor::Black => view.black_name,
                };
                if let (Some(d), Some(name)) = (self.draft_mut(), name) {
                    d.name = name;
                    d.color = color;
                }
                Task::none()
            }
            M::Generate => {
                let Some(draft) = self.ui.dialogs.preport.draft.clone() else {
                    return Task::none();
                };
                self.dispatch(prep(PlayerReportMessage::Generate(Box::new(draft))))
            }
            M::Cancel => {
                self.ui.dialogs.preport.draft = None;
                self.dispatch(prep(PlayerReportMessage::CloseDialog))
            }
            M::Confirm(yes) => self.dispatch(prep(PlayerReportMessage::ConfirmClipbase(yes))),
            M::ConfirmSave(yes) => self.dispatch(prep(PlayerReportMessage::ConfirmSave(yes))),
            M::Change => {
                let view = self.app.player_report_view();
                let kept = view
                    .report
                    .map(|r| (r.name.clone(), PlayerColor::from(r.color)));
                self.ui.dialogs.preport.draft = None;
                self.ui.dialogs.preport.change.clone_from(&kept);
                self.dispatch(prep(PlayerReportMessage::OpenDialog {
                    player: kept.map(|(name, _)| name),
                }))
            }
            M::Close => {
                self.ui.dialogs.preport.reset();
                self.dispatch(prep(PlayerReportMessage::Close))
            }
            M::Refresh => self.dispatch(prep(PlayerReportMessage::Refresh)),
            M::Stop => self.dispatch(prep(PlayerReportMessage::Stop)),
            M::OpenSave => {
                let prefs = self.app.player_report_view().prefs;
                self.ui.dialogs.preport.form = Some(PrForm::Save(SaveDraft {
                    part: prefs.last_part,
                    format: prefs.last_format,
                }));
                Task::none()
            }
            M::CloseForm => {
                self.ui.dialogs.preport.form = None;
                Task::none()
            }
            M::SavePart(part) => {
                if let Some(PrForm::Save(d)) = &mut self.ui.dialogs.preport.form {
                    d.part = part;
                }
                Task::none()
            }
            M::SaveFormat(format) => {
                if let Some(PrForm::Save(d)) = &mut self.ui.dialogs.preport.form {
                    d.format = format;
                }
                Task::none()
            }
            M::SaveChoose => {
                let Some(PrForm::Save(draft)) = self.ui.dialogs.preport.form else {
                    return Task::none();
                };
                let mut dialog = SaveReportDialog::new(
                    &self.i18n,
                    draft.format,
                    self.app.player_report_view().prefs.last_dir.clone(),
                );
                dialog.title = self.i18n.tr("preport-ui-save-dialog");
                dialog.file_name = format!("player-report.{}", draft.format.extension());
                Task::perform(dialog.run(), |path| {
                    Message::PlayerReportUi(PrUiMessage::SavePicked(path))
                })
            }
            M::SavePicked(path) => {
                let Some(PrForm::Save(SaveDraft { part, format })) = self.ui.dialogs.preport.form
                else {
                    return Task::none();
                };
                let Some(path) = path else {
                    return Task::none();
                };
                self.ui.dialogs.preport.form = None;
                self.dispatch(prep(PlayerReportMessage::Save { path, part, format }))
            }
        }
    }
}
