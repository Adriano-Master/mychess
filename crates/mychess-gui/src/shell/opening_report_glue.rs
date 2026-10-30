//! The part of the shell that serves the opening report window (spec M7-2c):
//! it opens the window and gives the application the words of the documents,
//! builds the model of the report once per report and language, keeps the
//! board in step, runs the file dialogs and turns the events of the forms
//! into messages of the application.

use iced::Task;
use mychess_app::opening_report::{
    BOARD_SIZE_RANGE, FavoriteMessage, OpeningReportMessage, OpeningReportPrefs, report_labels,
};
use mychess_app::{KeyInput, Message as AppMessage};

use super::{Message, Shell};
use crate::app_api;
use crate::widgets::board::BoardData;
use crate::widgets::board::geometry::Orientation;
use crate::windows::opening_report::files::{DatDialog, FolderDialog, SaveReportDialog};
use crate::windows::opening_report::model::Model;
use crate::windows::opening_report::{
    Cached, FavDraft, OPREP_FAV_NAME_ID, OPREP_FAV_RENAME_ID, OPREP_FIND_ID, OpRepForm,
    OpRepUiMessage, SaveDraft,
};

fn orep(message: OpeningReportMessage) -> AppMessage {
    AppMessage::OpeningReport(message)
}

fn fav(message: FavoriteMessage) -> AppMessage {
    orep(OpeningReportMessage::Favorite(message))
}

/// What the opening report window does with a key.
enum KeyUse {
    /// Escape.
    Escape,
    /// Ctrl/Cmd+F: the search field.
    Find,
    /// F5: update.
    Refresh,
}

impl Shell {
    /// Whether the opening report window covers the main window.
    pub(super) fn oprep_layer_open(&self) -> bool {
        crate::windows::opening_report::OpRepUi::layer_open(&self.app)
    }

    /// The words of the documents in the language of the interface.
    fn oprep_labels(&self) -> mychess_app::opening_report::ReportLabels {
        report_labels(&|key, args| {
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

    /// Brings the window up to date with the application: the words go to
    /// the application when the language changed, the model is built when
    /// the report changed, the board follows the position and a closed
    /// window forgets its forms.
    pub(super) fn sync_oprep(&mut self) -> Task<Message> {
        if !self.app.opening_report().is_open() {
            if self.ui.dialogs.oprep.was_open {
                self.ui.dialogs.oprep.reset();
            }
            self.ui.dialogs.oprep.was_open = false;
            return Task::none();
        }
        self.ui.dialogs.oprep.was_open = true;
        let mut tasks = Vec::new();
        let language = self.i18n.language();
        if self.ui.dialogs.oprep.labels_language != Some(language) {
            self.ui.dialogs.oprep.labels_language = Some(language);
            let labels = self.oprep_labels();
            tasks.push(self.dispatch(orep(OpeningReportMessage::SetLabels(Box::new(labels)))));
        }
        // The light accessor: the full view writes the SAN of every move.
        let shown = self
            .app
            .opening_report_shown()
            .map(|(shared, options, name)| (shared, options.clone(), name.to_owned()));
        let Some((shared, options, name)) = shown else {
            self.ui.dialogs.oprep.cached = None;
            self.ui.dialogs.oprep.hits = crate::windows::opening_report::model::Hits::default();
            // A form about a report that is gone cannot stay.
            if matches!(self.ui.dialogs.oprep.form, Some(OpRepForm::Exclude)) {
                self.ui.dialogs.oprep.form = None;
            }
            return Task::batch(tasks);
        };
        let rebuild = self
            .ui
            .dialogs
            .oprep
            .cached
            .as_ref()
            .is_none_or(|c| c.report != shared || c.language != language);
        if rebuild {
            let labels = self.oprep_labels();
            let generator = format!("MyChess {}", env!("CARGO_PKG_VERSION"));
            let model = Model::build(&shared, &options, &labels, &name, &generator);
            self.ui.dialogs.oprep.cached = Some(Cached {
                report: shared.clone(),
                language,
                model,
            });
            self.ui.dialogs.oprep.refind();
        }
        // The board: its caches are taken out so the piece set of the
        // interface can be borrowed meanwhile.
        let look = app_api::appearance(&self.app, self.options);
        let mut caches = std::mem::take(&mut self.ui.dialogs.oprep.caches);
        let data = BoardData {
            position: &shared.position,
            last_move: None,
            marks: &[],
            orientation: if self.ui.dialogs.oprep.flipped {
                Orientation::Black
            } else {
                Orientation::White
            },
            options: look.board,
            accepts_moves: false,
            brush: &self.ui.dialogs.oprep.brush,
            pieces: self.ui.pieces(),
            animation: None,
        };
        caches.sync_data(&data);
        self.ui.dialogs.oprep.caches = caches;
        Task::batch(tasks)
    }

    /// Esc: closes the menu of a row, then the form, then the window.
    pub(super) fn oprep_escape(&mut self) -> Task<Message> {
        let ui = &mut self.ui.dialogs.oprep;
        if ui.row_menu.take().is_some() {
            return Task::none();
        }
        if let Some(OpRepForm::Favorites(draft)) = &mut ui.form
            && draft.rename.take().is_some()
        {
            return Task::none();
        }
        if ui.form.take().is_some() {
            return Task::none();
        }
        self.update_oprep(OpRepUiMessage::Close)
    }

    /// A report or the batch of the favorites is being made.
    pub(super) fn oprep_running(&self) -> bool {
        let view = self.app.opening_report_view();
        matches!(
            view.phase,
            mychess_app::opening_report::ReportPhase::Generating { .. }
        ) || view.batch.is_some()
    }

    /// A key while the window is open: only Esc, Ctrl/Cmd+F and F5 (D3631)
    /// and the keys of the program window act; the game behind is blocked.
    pub(super) fn oprep_key(&mut self, input: &KeyInput, text_focused: bool) -> Task<Message> {
        use mychess_app::{Action, Key, Modifiers, NamedKey};
        let wanted = match (input.key, input.modifiers) {
            (Key::Named(NamedKey::Escape), Modifiers::NONE) => Some(KeyUse::Escape),
            (Key::Char('f'), m) if m.command && !m.shift && !m.alt => Some(KeyUse::Find),
            (Key::Named(NamedKey::F(5)), Modifiers::NONE) => Some(KeyUse::Refresh),
            _ => None,
        };
        match wanted {
            Some(KeyUse::Escape) => self.oprep_escape(),
            Some(KeyUse::Find) if self.ui.dialogs.oprep.form.is_none() => {
                self.update_oprep(OpRepUiMessage::FocusFind)
            }
            // The same conditions as the "Update" button: not while a
            // report or the batch of the favorites is being made.
            Some(KeyUse::Refresh) if self.ui.dialogs.oprep.form.is_none() => {
                if self.oprep_running() {
                    Task::none()
                } else {
                    self.update_oprep(OpRepUiMessage::Refresh)
                }
            }
            Some(_) => Task::none(),
            None => match self.app.shortcuts.lookup_in(input, text_focused) {
                Some(action @ (Action::Quit | Action::FullScreen)) => self.run_action(action),
                _ => Task::none(),
            },
        }
    }

    fn drop_fav_rename(&mut self) {
        if let Some(OpRepForm::Favorites(draft)) = &mut self.ui.dialogs.oprep.form {
            draft.rename = None;
        }
    }

    fn orep_set_prefs(&mut self, change: impl FnOnce(&mut OpeningReportPrefs)) -> Task<Message> {
        let mut prefs = (*self.app.prefs.opening_report).clone();
        change(&mut prefs);
        self.dispatch(orep(OpeningReportMessage::SetPrefs(Box::new(prefs))))
    }

    /// An event of the opening report window.
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(super) fn update_oprep(&mut self, message: OpRepUiMessage) -> Task<Message> {
        use OpRepUiMessage as M;
        match message {
            M::Open => {
                self.ui.dialogs.oprep.reset();
                // Generate first: it starts the reading of the favorites
                // (D3655); the words follow (they also go when the
                // language changes, see `sync_oprep`).
                let generate = self.dispatch(orep(OpeningReportMessage::Generate));
                let labels = self.oprep_labels();
                self.ui.dialogs.oprep.labels_language = Some(self.i18n.language());
                let words = self.dispatch(orep(OpeningReportMessage::SetLabels(Box::new(labels))));
                Task::batch([generate, words])
            }
            M::Close => {
                // Closing stops the generation of the favorites too; the
                // application cancels the job itself.
                let stop = if self.app.opening_report_view().batch.is_some() {
                    self.dispatch(orep(OpeningReportMessage::Stop))
                } else {
                    Task::none()
                };
                self.ui.dialogs.oprep.reset();
                let close = self.dispatch(orep(OpeningReportMessage::Close));
                Task::batch([stop, close])
            }
            M::Refresh => self.dispatch(orep(OpeningReportMessage::Refresh)),
            M::Stop => self.dispatch(orep(OpeningReportMessage::Stop)),
            M::Noop => Task::none(),
            M::Flip => {
                self.ui.dialogs.oprep.flipped = !self.ui.dialogs.oprep.flipped;
                Task::none()
            }
            M::BoardSize(up) => {
                let (lo, hi) = (*BOARD_SIZE_RANGE.start(), *BOARD_SIZE_RANGE.end());
                self.orep_set_prefs(|p| {
                    p.board_size = if up {
                        p.board_size.saturating_add(5).min(hi)
                    } else {
                        p.board_size.saturating_sub(5).max(lo)
                    };
                })
            }
            M::OpenOptions => {
                self.ui.dialogs.oprep.row_menu = None;
                self.ui.dialogs.oprep.form = Some(OpRepForm::Options(Box::new(
                    (*self.app.prefs.opening_report).clone(),
                )));
                Task::none()
            }
            M::OpenSave => {
                self.ui.dialogs.oprep.row_menu = None;
                self.ui.dialogs.oprep.form = Some(OpRepForm::Save(
                    crate::windows::opening_report::OpRepUi::save_draft(
                        &self.app.prefs.opening_report,
                    ),
                ));
                Task::none()
            }
            M::OpenExclude => {
                self.ui.dialogs.oprep.row_menu = None;
                self.ui.dialogs.oprep.form = Some(OpRepForm::Exclude);
                Task::none()
            }
            M::OpenFavorites => {
                self.ui.dialogs.oprep.row_menu = None;
                self.ui.dialogs.oprep.form = Some(OpRepForm::Favorites(FavDraft {
                    name: String::new(),
                    rename: None,
                    batch: crate::windows::opening_report::OpRepUi::save_draft(
                        &self.app.prefs.opening_report,
                    ),
                }));
                iced::widget::operation::focus(OPREP_FAV_NAME_ID)
            }
            M::CloseForm => {
                self.ui.dialogs.oprep.form = None;
                Task::none()
            }
            M::Find(text) => {
                self.ui.dialogs.oprep.find = text;
                self.ui.dialogs.oprep.refind();
                Task::none()
            }
            M::TheoryScrolled(offset) => {
                self.ui.dialogs.oprep.theory_scroll = offset;
                Task::none()
            }
            M::FocusFind => iced::widget::operation::focus(OPREP_FIND_ID),
            M::NoteFocus(n) => {
                self.ui.dialogs.oprep.note_focus = n;
                Task::none()
            }
            M::RowMenu { line, at } => {
                self.ui.dialogs.oprep.row_menu =
                    Some(crate::windows::opening_report::RowMenu { line, at });
                Task::none()
            }
            M::CloseRowMenu => {
                self.ui.dialogs.oprep.row_menu = None;
                Task::none()
            }
            M::CopyLine => match self.ui.dialogs.oprep.row_menu.take() {
                Some(menu) => iced::clipboard::write(menu.line.to_string()),
                None => Task::none(),
            },
            M::Count(count, up) => {
                if let Some(OpRepForm::Options(draft)) = &mut self.ui.dialogs.oprep.form {
                    let next = count.stepped(count.get(draft), up);
                    count.set(draft, next);
                }
                Task::none()
            }
            M::Flag(flag, on) => {
                if let Some(OpRepForm::Options(draft)) = &mut self.ui.dialogs.oprep.form {
                    flag.set(draft, on);
                }
                Task::none()
            }
            M::OptionsDefaults => {
                if let Some(OpRepForm::Options(draft)) = &mut self.ui.dialogs.oprep.form {
                    let keep = draft.clone();
                    **draft = OpeningReportPrefs {
                        last_part: keep.last_part,
                        last_format: keep.last_format,
                        last_dir: keep.last_dir,
                        board_size: keep.board_size,
                        ..OpeningReportPrefs::default()
                    };
                }
                Task::none()
            }
            M::OptionsOk => {
                let Some(OpRepForm::Options(draft)) = self.ui.dialogs.oprep.form.take() else {
                    return Task::none();
                };
                // Apply, and make the report again when something changed.
                if *draft == *self.app.prefs.opening_report {
                    return Task::none();
                }
                let set = self.dispatch(orep(OpeningReportMessage::SetPrefs(draft)));
                let refresh = self.dispatch(orep(OpeningReportMessage::Refresh));
                Task::batch([set, refresh])
            }
            M::SavePart(part) => {
                if let Some(OpRepForm::Save(draft)) = &mut self.ui.dialogs.oprep.form {
                    draft.part = part;
                }
                Task::none()
            }
            M::SaveFormat(format) => {
                if let Some(OpRepForm::Save(draft)) = &mut self.ui.dialogs.oprep.form {
                    draft.format = format;
                }
                Task::none()
            }
            M::SaveChoose => {
                let Some(OpRepForm::Save(draft)) = &self.ui.dialogs.oprep.form else {
                    return Task::none();
                };
                let dialog = SaveReportDialog::new(
                    &self.i18n,
                    draft.format,
                    self.app.prefs.opening_report.last_dir.clone(),
                );
                Task::perform(dialog.run(), |path| Message::OpRep(M::SavePicked(path)))
            }
            M::SavePicked(path) => {
                let Some(OpRepForm::Save(SaveDraft { part, format })) = self.ui.dialogs.oprep.form
                else {
                    return Task::none();
                };
                let Some(path) = path else {
                    return Task::none();
                };
                self.ui.dialogs.oprep.form = None;
                self.dispatch(orep(OpeningReportMessage::Save { path, part, format }))
            }
            M::Exclude(choice) => {
                self.ui.dialogs.oprep.form = None;
                self.dispatch(orep(OpeningReportMessage::SetExclude(choice)))
            }
            M::FavName(text) => {
                if let Some(OpRepForm::Favorites(draft)) = &mut self.ui.dialogs.oprep.form {
                    draft.name = text;
                }
                Task::none()
            }
            M::FavAdd => {
                let Some(OpRepForm::Favorites(draft)) = &mut self.ui.dialogs.oprep.form else {
                    return Task::none();
                };
                let name = draft.name.trim().to_owned();
                if name.is_empty() {
                    return Task::none();
                }
                draft.name.clear();
                self.dispatch(fav(FavoriteMessage::Add { name }))
            }
            M::FavOpen(index) => {
                self.ui.dialogs.oprep.form = None;
                self.dispatch(orep(OpeningReportMessage::OpenFavorite(index)))
            }
            M::FavRenameStart(index) => {
                let name = self
                    .app
                    .opening_report_view()
                    .favorites
                    .get(index)
                    .map(|f| f.name.clone());
                if let (Some(OpRepForm::Favorites(draft)), Some(name)) =
                    (&mut self.ui.dialogs.oprep.form, name)
                {
                    draft.rename = Some((index, name));
                    return iced::widget::operation::focus(OPREP_FAV_RENAME_ID);
                }
                Task::none()
            }
            M::FavRenameInput(text) => {
                if let Some(OpRepForm::Favorites(FavDraft {
                    rename: Some((_, name)),
                    ..
                })) = &mut self.ui.dialogs.oprep.form
                {
                    *name = text;
                }
                Task::none()
            }
            M::FavRenameGo => {
                let Some(OpRepForm::Favorites(draft)) = &mut self.ui.dialogs.oprep.form else {
                    return Task::none();
                };
                let Some((index, name)) = draft.rename.take() else {
                    return Task::none();
                };
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return Task::none();
                }
                self.dispatch(fav(FavoriteMessage::Rename { index, name }))
            }
            M::FavRenameCancel => {
                if let Some(OpRepForm::Favorites(draft)) = &mut self.ui.dialogs.oprep.form {
                    draft.rename = None;
                }
                Task::none()
            }
            // The rename field holds the place of its favorite: a change of
            // the places would send the new name to another one.
            M::FavDelete(index) => {
                self.drop_fav_rename();
                self.dispatch(fav(FavoriteMessage::Delete(index)))
            }
            M::FavUp(index) => {
                self.drop_fav_rename();
                self.dispatch(fav(FavoriteMessage::MoveUp(index)))
            }
            M::FavDown(index) => {
                self.drop_fav_rename();
                self.dispatch(fav(FavoriteMessage::MoveDown(index)))
            }
            M::FavImportChoose => {
                let dialog = DatDialog::new(&self.i18n);
                Task::perform(dialog.run(), |path| Message::OpRep(M::FavDatPicked(path)))
            }
            M::FavDatPicked(path) => match path {
                Some(path) => {
                    self.drop_fav_rename();
                    self.dispatch(fav(FavoriteMessage::Import(path)))
                }
                None => Task::none(),
            },
            M::FavPart(part) => {
                if let Some(OpRepForm::Favorites(draft)) = &mut self.ui.dialogs.oprep.form {
                    draft.batch.part = part;
                }
                Task::none()
            }
            M::FavFormat(format) => {
                if let Some(OpRepForm::Favorites(draft)) = &mut self.ui.dialogs.oprep.form {
                    draft.batch.format = format;
                }
                Task::none()
            }
            M::FavBatchChoose => {
                let dialog =
                    FolderDialog::new(&self.i18n, self.app.prefs.opening_report.last_dir.clone());
                Task::perform(dialog.run(), |path| Message::OpRep(M::FavDirPicked(path)))
            }
            M::FavDirPicked(dir) => {
                let Some(OpRepForm::Favorites(draft)) = &self.ui.dialogs.oprep.form else {
                    return Task::none();
                };
                let batch = draft.batch;
                let Some(dir) = dir else {
                    return Task::none();
                };
                // The progress is on the window, not behind the form.
                self.ui.dialogs.oprep.form = None;
                self.dispatch(fav(FavoriteMessage::GenerateAll {
                    dir,
                    part: batch.part,
                    format: batch.format,
                }))
            }
        }
    }
}
