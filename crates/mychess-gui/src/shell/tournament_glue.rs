//! The part of the shell that serves the tournament between engines: the
//! synchronization of its visual state, the file dialogs, the clipboard and
//! the confirmation before the program quits (spec M8-2b, D1360-D1379).

use iced::Task;
use mychess_app::tournament::{ExportTable, TournamentMessage, TournamentNotice};
use mychess_app::{AppNotice, Message as AppMessage};

use super::{Message, Shell};
use crate::host::{self, TournamentFileDialog};
use crate::widgets::board::BoardOptions;
use crate::windows::layout::PaneKind;
use crate::windows::tournament_draft::DraftEdit;
use crate::windows::tournament_panel::PAIRINGS_LIST_ID;
use crate::windows::tournament_text::export_labels;
use crate::windows::tournament_ui::{TournamentConfirm, TournamentUiMessage};

impl Shell {
    /// Brings the visual state of the tournament up to date with the
    /// application; the panel appears by itself when a tournament starts.
    pub(super) fn sync_tournament(&mut self, options: BoardOptions) {
        let dialogs = &mut self.ui.dialogs;
        let out = dialogs.tournament.sync(&self.app, &dialogs.pieces, options);
        if out.show_pane {
            self.ui.show_pane(PaneKind::Tournament);
        }
    }

    fn tournament_notice(&mut self, notice: TournamentNotice) {
        self.ui.status_notice = Some(AppNotice::Tournament(notice));
        self.ui.dialogs.status.reset();
    }

    /// An event of the form, the panel or the boxes of the tournament.
    pub(super) fn update_tournament(&mut self, message: TournamentUiMessage) -> Task<Message> {
        use TournamentUiMessage as M;
        match &message {
            M::Open => {
                if self.app.tournament_live() {
                    // A tournament is on: what the user wants is to see it.
                    self.ui.show_pane(PaneKind::Tournament);
                    return self.remember_layout();
                }
                if self.app.play_live() {
                    self.tournament_notice(TournamentNotice::PlayBusy);
                    return Task::none();
                }
            }
            M::PickBook => {
                let dialog = host::BookFileDialog::new(&self.i18n);
                return Task::perform(dialog.run(), |path| {
                    Message::Tournament(M::BookPicked(path))
                });
            }
            M::PickSuite => {
                let dialog = TournamentFileDialog::suite(&self.i18n);
                return Task::perform(dialog.run(), |path| {
                    Message::Tournament(M::SuitePicked(path))
                });
            }
            M::BookPicked(path) => {
                return match path {
                    Some(path) => {
                        self.update_tournament(M::Edit(DraftEdit::Book(Some(path.clone()))))
                    }
                    None => Task::none(),
                };
            }
            M::SuitePicked(path) => {
                return match path {
                    Some(path) => {
                        self.update_tournament(M::Edit(DraftEdit::Suite(Some(path.clone()))))
                    }
                    None => Task::none(),
                };
            }
            M::Save(table) => {
                let format = self.ui.dialogs.tournament.export_format;
                let table = *table;
                let dialog = TournamentFileDialog::table(&self.i18n, table, format);
                return Task::perform(dialog.run(), move |path| {
                    Message::Tournament(M::Saved(table, format, path))
                });
            }
            M::Saved(table, format, Some(path)) => {
                return self.dispatch(AppMessage::Tournament(TournamentMessage::Export {
                    table: *table,
                    format: *format,
                    labels: Box::new(export_labels(&self.i18n)),
                    path: path.clone(),
                }));
            }
            M::Saved(_, _, None) => return Task::none(),
            M::Copy(table) => return self.copy_table(*table),
            M::Confirm(true)
                if self.ui.dialogs.tournament.confirm == Some(TournamentConfirm::Quit) =>
            {
                self.ui.dialogs.tournament.confirm = None;
                return self.quit_after_books();
            }
            _ => {}
        }
        let cell = matches!(message, M::Cell(..));
        let out = self.ui.dialogs.tournament.update(message, &self.app);
        let mut tasks = vec![self.run_outcome(out, None)];
        if cell {
            tasks.push(iced::widget::operation::scroll_to(
                PAIRINGS_LIST_ID,
                iced::widget::scrollable::AbsoluteOffset {
                    x: Some(0.0),
                    y: Some(self.ui.dialogs.tournament.scroll),
                },
            ));
        }
        Task::batch(tasks)
    }

    /// Puts a table on the clipboard in the format chosen (D1287).
    fn copy_table(&mut self, table: ExportTable) -> Task<Message> {
        let format = self.ui.dialogs.tournament.export_format;
        let labels = export_labels(&self.i18n);
        if let Some(text) = self.app.tournament_export_text(table, format, &labels) {
            self.tournament_notice(TournamentNotice::Exported);
            host::task(host::HostWork::WriteClipboard(text))
        } else {
            self.tournament_notice(TournamentNotice::NotApplicable);
            Task::none()
        }
    }
}
