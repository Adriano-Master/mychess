//! The part of the shell that serves the book window: the file dialogs and
//! the text of the weight fields (spec M5-6).

use iced::Task;
use mychess_app::{BookMessage, BookSide, Message as AppMessage};

use super::{Message, Shell};
use crate::host::{BookFileDialog, BookPgnDialog, BookSaveDialog};
use crate::windows::book_panel::{BookQuit, BookUiMessage, QuitChoice};
use crate::windows::layout::PaneKind;

/// How long the save before quitting waits for a job (D1796).
const QUIT_SAVE_SECONDS: u8 = 5;

impl Shell {
    /// Tells the application whether the book panel is open.
    pub(super) fn sync_book(&mut self) {
        let open = self.ui.is_open(PaneKind::Book);
        if self.app.book().is_open() != open {
            // Opening and closing the panel never produce effects.
            drop(mychess_app::update(
                &mut self.app,
                AppMessage::Book(BookMessage::SetOpen(open)),
            ));
        }
    }

    /// Quits, unless a book has changes that are not saved: then the
    /// question is shown first (D1792).
    pub(super) fn quit_after_books(&mut self) -> Task<Message> {
        if self.ui.dialogs.book.quit.is_some() {
            // The question or the saving is already under way.
            return Task::none();
        }
        if self.app.book_has_unsaved() {
            self.ui.dialogs.book.quit = Some(BookQuit::Asking);
            return Task::none();
        }
        self.quit_now()
    }

    /// Takes the next step of the "save, then quit" run: waits for the job
    /// in flight, saves the next changed book (asking for a file when it
    /// has none) and quits when all are clean. A failure or a stale save
    /// stops the run and shows the panel with the reason (D1792).
    pub(super) fn book_quit_step(&mut self) -> Task<Message> {
        let Some(BookQuit::Saving { pending, .. }) = self.ui.dialogs.book.quit else {
            return Task::none();
        };
        if matches!(pending, Some((_, true))) || self.app.book().is_saving() {
            return Task::none();
        }
        if let Some((side, _)) = pending
            && self.app.book().is_dirty(side)
        {
            self.ui.dialogs.book.quit = None;
            self.ui.show_pane(PaneKind::Book);
            return self.remember_layout();
        }
        let Some(side) = BookSide::ALL
            .into_iter()
            .find(|&side| self.app.book().is_dirty(side))
        else {
            self.ui.dialogs.book.quit = None;
            return self.quit_now();
        };
        let has_file = self.app.book_view().sides[side.index()]
            .as_ref()
            .is_some_and(|book| book.path.is_some());
        if has_file {
            self.ui.dialogs.book.quit = Some(BookQuit::Saving {
                pending: Some((side, false)),
                ticks: 0,
            });
            self.dispatch(AppMessage::Book(BookMessage::Save { side }))
        } else {
            self.ui.dialogs.book.quit = Some(BookQuit::Saving {
                pending: Some((side, true)),
                ticks: 0,
            });
            self.update_book(BookUiMessage::SaveAs(side))
        }
    }

    /// Quits, after stopping what the books are doing: a book being made is
    /// dropped (nothing is written), and an export is asked to stop and
    /// waited for, for at most [`QUIT_SAVE_SECONDS`] (D2175).
    pub(super) fn quit_now(&mut self) -> Task<Message> {
        if self.app.book().is_creating() {
            drop(mychess_app::update(
                &mut self.app,
                AppMessage::Book(BookMessage::CancelCreate),
            ));
        }
        if self.app.book().is_exporting() {
            drop(mychess_app::update(
                &mut self.app,
                AppMessage::Book(BookMessage::CancelExport),
            ));
            self.ui.dialogs.book.quit = Some(BookQuit::Draining { ticks: 0 });
            return Task::none();
        }
        self.quit_final()
    }

    /// One second of the save run: a job that does not finish in
    /// [`QUIT_SAVE_SECONDS`] aborts the run and shows the panel (D1796). The
    /// time the file dialog is open does not count.
    pub(super) fn book_quit_tick(&mut self) -> Task<Message> {
        if let Some(BookQuit::Draining { ticks }) = self.ui.dialogs.book.quit {
            let ticks = ticks.saturating_add(1);
            if !self.app.book().is_exporting() || ticks >= QUIT_SAVE_SECONDS {
                return self.quit_final();
            }
            self.ui.dialogs.book.quit = Some(BookQuit::Draining { ticks });
            return Task::none();
        }
        let Some(BookQuit::Saving { pending, ticks }) = self.ui.dialogs.book.quit else {
            return Task::none();
        };
        if matches!(pending, Some((_, true))) || !self.app.book().is_saving() {
            return Task::none();
        }
        let ticks = ticks.saturating_add(1);
        if ticks >= QUIT_SAVE_SECONDS {
            self.ui.dialogs.book.quit = None;
            self.ui.show_pane(PaneKind::Book);
            return self.remember_layout();
        }
        self.ui.dialogs.book.quit = Some(BookQuit::Saving { pending, ticks });
        Task::none()
    }

    /// An event of the panel that the application does not know.
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(super) fn update_book(&mut self, message: BookUiMessage) -> Task<Message> {
        match message {
            BookUiMessage::QuitAnswer(choice) => {
                if self.ui.dialogs.book.quit != Some(BookQuit::Asking) {
                    return Task::none();
                }
                match choice {
                    QuitChoice::Save => {
                        self.ui.dialogs.book.quit = Some(BookQuit::Saving {
                            pending: None,
                            ticks: 0,
                        });
                        self.book_quit_step()
                    }
                    QuitChoice::Discard => {
                        self.ui.dialogs.book.quit = None;
                        self.quit_now()
                    }
                    QuitChoice::Cancel => {
                        self.ui.dialogs.book.quit = None;
                        Task::none()
                    }
                }
            }
            // The dialog of the "save, then quit" run was dismissed: the
            // run stops.
            BookUiMessage::SavedAs(_, None)
                if matches!(self.ui.dialogs.book.quit, Some(BookQuit::Saving { .. })) =>
            {
                self.ui.dialogs.book.quit = None;
                Task::none()
            }
            BookUiMessage::SavedAs(side, Some(path))
                if matches!(self.ui.dialogs.book.quit, Some(BookQuit::Saving { .. })) =>
            {
                self.ui.dialogs.book.quit = Some(BookQuit::Saving {
                    pending: Some((side, false)),
                    ticks: 0,
                });
                self.dispatch(AppMessage::Book(BookMessage::SaveAs { side, path }))
            }
            BookUiMessage::Open(side) => {
                let dialog = BookFileDialog::for_window(&self.i18n);
                Task::perform(dialog.run(), move |path| {
                    Message::Book(BookUiMessage::Opened(side, path))
                })
            }
            BookUiMessage::Opened(side, Some(path)) => {
                self.dispatch(AppMessage::Book(BookMessage::Load { side, path }))
            }
            BookUiMessage::Save(side) => {
                let has_file = self.app.book_view().sides[side.index()]
                    .as_ref()
                    .is_some_and(|book| book.path.is_some());
                if has_file {
                    self.dispatch(AppMessage::Book(BookMessage::Save { side }))
                } else {
                    self.update_book(BookUiMessage::SaveAs(side))
                }
            }
            BookUiMessage::SaveAs(side) => {
                let dialog = BookSaveDialog::new(&self.i18n);
                Task::perform(dialog.run(), move |path| {
                    Message::Book(BookUiMessage::SavedAs(side, path))
                })
            }
            BookUiMessage::SavedAs(side, Some(path)) => {
                self.dispatch(AppMessage::Book(BookMessage::SaveAs { side, path }))
            }
            BookUiMessage::ExportBranch(side) => {
                let dialog = BookPgnDialog::new(&self.i18n);
                Task::perform(dialog.run(), move |path| {
                    Message::Book(BookUiMessage::ExportChosen(side, path))
                })
            }
            BookUiMessage::ExportChosen(side, Some(path)) => {
                self.dispatch(AppMessage::Book(BookMessage::ExportBranch { side, path }))
            }
            BookUiMessage::Opened(_, None)
            | BookUiMessage::SavedAs(_, None)
            | BookUiMessage::ExportChosen(_, None) => Task::none(),
            // Show the second book, or close it. A book with changes is not
            // closed here: the application asks first (the book stays on
            // screen until the answer), so nothing is lost (D2041).
            BookUiMessage::ToggleCompare => {
                let second = BookSide::Second;
                let present = self.app.book_view().sides[second.index()].is_some();
                let ui = &mut self.ui.dialogs.book;
                if !(ui.compare || present) {
                    ui.compare = true;
                    return Task::none();
                }
                ui.compare = false;
                if present {
                    self.dispatch(AppMessage::Book(BookMessage::Close { side: second }))
                } else {
                    Task::none()
                }
            }
            BookUiMessage::ToggleTools => {
                let ui = &mut self.ui.dialogs.book;
                ui.tools_open = !ui.tools_open;
                Task::none()
            }
            BookUiMessage::WeightInput { side, mv, text } => {
                let at = (self.app.session.cursor(), self.app.session.generation());
                self.ui.dialogs.book.input(at, side, mv, text);
                Task::none()
            }
        }
    }
}
