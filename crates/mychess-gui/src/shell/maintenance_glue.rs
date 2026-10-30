//! The part of the shell that serves the maintenance window (spec M7-1c):
//! it opens the window and its forms, keeps the text of the fields, turns the
//! events of the forms into messages of the application, runs the file
//! dialog of the repair and takes the keys of the window.

use std::path::PathBuf;

use iced::Task;
use mychess_app::maintenance::{MaintenanceMessage, MaintenancePrefs};
use mychess_app::{BasesMessage, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey};

use super::{Message, Shell};
use crate::widgets::modal;
use crate::windows::maintenance::{
    MAINT_AUTOLOAD_ID, MAINT_CUSTOM_ID, MAINT_DESCRIPTION_ID, MAINT_NAME_OLD_ID, MaintEdit,
    MaintForm, MaintUi, MaintUiMessage, ScopeField, common_sort,
};

fn maint(message: MaintenanceMessage) -> AppMessage {
    AppMessage::Maintenance(message)
}

impl Shell {
    /// Whether the maintenance window covers the main window.
    pub(super) fn maint_layer_open(&self) -> bool {
        MaintUi::layer_open(&self.app)
    }

    fn maint_prefs(&self) -> MaintenancePrefs {
        (*self.app.prefs.maintenance).clone()
    }

    /// Brings the window up to date with the application: a closed window
    /// forgets its fields, and a search for duplicates that marked games
    /// opens the checker (once).
    pub(super) fn sync_maint(&mut self) -> Task<Message> {
        if !self.maint_layer_open() {
            if self.ui.dialogs.maint.was_open {
                let prefs = self.maint_prefs();
                self.ui.dialogs.maint.reset(&prefs);
                return modal::unfocus_all();
            }
            return Task::none();
        }
        self.ui.dialogs.maint.was_open = true;
        let marked = matches!(
            self.app.maintenance().last_report(),
            Some(mychess_app::maintenance::MaintReport::Marked { .. })
        );
        if !marked {
            self.ui.dialogs.maint.marked_seen = false;
        } else if !self.ui.dialogs.maint.marked_seen {
            self.ui.dialogs.maint.marked_seen = true;
            let view = self.app.maintenance_view();
            if view.twin_pairs > 0 && !view.checker_open {
                return self.dispatch(maint(MaintenanceMessage::OpenChecker));
            }
        }
        Task::none()
    }

    fn focus_of(form: MaintForm) -> Option<&'static str> {
        match form {
            MaintForm::Description => Some(MAINT_DESCRIPTION_ID),
            MaintForm::Autoload => Some(MAINT_AUTOLOAD_ID),
            MaintForm::Names => Some(MAINT_NAME_OLD_ID),
            MaintForm::CustomFlags => Some(MAINT_CUSTOM_ID),
            _ => None,
        }
    }

    /// Opens a form with the fields filled from the database.
    fn maint_open_form(&mut self, form: MaintForm) -> Task<Message> {
        let mut tasks = Vec::new();
        if !self.maint_layer_open() {
            let prefs = self.maint_prefs();
            self.ui.dialogs.maint.reset(&prefs);
            tasks.push(self.dispatch(maint(MaintenanceMessage::Open)));
        }
        let view = self.app.maintenance_view();
        let state = &mut self.ui.dialogs.maint;
        state.form = Some(form);
        state.sort_keys.clone_from(&view.prefs.sort_keys);
        state.sort_reverse = view.prefs.sort_reverse;
        if let Some(header) = &view.header {
            match form {
                MaintForm::Autoload => {
                    state.autoload = header.auto_load.unwrap_or(0).to_string();
                }
                MaintForm::Description => state.description.clone_from(&header.description),
                MaintForm::CustomFlags => state.custom.clone_from(&header.custom_flags),
                _ => {}
            }
        }
        tasks.push(match Self::focus_of(form) {
            Some(id) => iced::widget::operation::focus(id),
            None => modal::unfocus_all(),
        });
        Task::batch(tasks)
    }

    fn maint_edit(&mut self, edit: MaintEdit) {
        let state = &mut self.ui.dialogs.maint;
        match edit {
            MaintEdit::SortAdd(key) => {
                if state.sort_keys.len() < mychess_app::maintenance::MAX_SORT_CRITERIA {
                    state.sort_keys.push(key);
                }
            }
            MaintEdit::SortRemoveLast => {
                state.sort_keys.pop();
            }
            MaintEdit::SortClear => state.sort_keys.clear(),
            MaintEdit::SortCommon(index) => state.sort_keys = common_sort(index),
            MaintEdit::SortReverse(on) => state.sort_reverse = on,
            MaintEdit::SortCompact(on) => state.sort_compact = on,
            MaintEdit::Strip(what) => state.strip = what,
            MaintEdit::TagSelect(tag) => state.tags.selected = tag,
            MaintEdit::TagName(text) => state.tags.name = text,
            MaintEdit::TagValue(text) => state.tags.value = text,
            MaintEdit::NamesKind(kind) => state.names.kind = kind,
            MaintEdit::NamesOld(text) => state.names.old = text,
            MaintEdit::NamesNew(text) => state.names.new = text,
            MaintEdit::NamesElo(text) => state.names.elo = text,
            MaintEdit::NamesPattern(on) => state.names.pattern = on,
            MaintEdit::Autoload(text) => state.autoload = text,
            MaintEdit::Description(text) => state.description = text,
            MaintEdit::Custom(index, text) => {
                if let Some(slot) = state.custom.get_mut(index) {
                    *slot = text;
                }
            }
            MaintEdit::RepairDrop(on) => state.repair_drop = on,
        }
    }

    /// An event of the maintenance window.
    pub(super) fn update_maint(&mut self, message: MaintUiMessage) -> Task<Message> {
        match message {
            MaintUiMessage::Open => {
                let prefs = self.maint_prefs();
                self.ui.dialogs.maint.reset(&prefs);
                self.dispatch(maint(MaintenanceMessage::Open))
            }
            MaintUiMessage::Close => {
                let close = self.dispatch(maint(MaintenanceMessage::Close));
                let prefs = self.maint_prefs();
                self.ui.dialogs.maint.reset(&prefs);
                Task::batch([close, modal::unfocus_all()])
            }
            MaintUiMessage::OpenForm(form) => self.maint_open_form(form),
            MaintUiMessage::OpenStrip(what) => {
                self.ui.dialogs.maint.strip = what;
                self.maint_open_form(MaintForm::Strip)
            }
            MaintUiMessage::CloseForm => {
                self.ui.dialogs.maint.form = None;
                modal::unfocus_all()
            }
            MaintUiMessage::SelectFlag(flag) => {
                self.ui.dialogs.maint.flag = flag;
                Task::none()
            }
            MaintUiMessage::App(message) => self.dispatch(maint(message)),
            MaintUiMessage::Run(request) => {
                self.dispatch(maint(MaintenanceMessage::Start(request)))
            }
            MaintUiMessage::RunClose(request) => {
                self.ui.dialogs.maint.form = None;
                let start = self.dispatch(maint(MaintenanceMessage::Start(request)));
                Task::batch([start, modal::unfocus_all()])
            }
            MaintUiMessage::Scope(field, scope) => {
                let mut prefs = self.maint_prefs();
                match field {
                    ScopeField::Strip => prefs.strip_scope = scope,
                    ScopeField::Tags => prefs.tag_scope = scope,
                }
                self.dispatch(maint(MaintenanceMessage::SetPrefs(Box::new(prefs))))
            }
            MaintUiMessage::Prefs(prefs) => {
                self.dispatch(maint(MaintenanceMessage::SetPrefs(prefs)))
            }
            MaintUiMessage::Edit(edit) => {
                self.maint_edit(edit);
                Task::none()
            }
            MaintUiMessage::PickRepairFile => {
                let title = self.i18n.tr("maint-repair-dialog-title");
                let filter = self.i18n.tr("maint-repair-file-filter");
                Task::perform(pick_si4(title, filter), |path| {
                    Message::Maint(MaintUiMessage::RepairFilePicked(path))
                })
            }
            MaintUiMessage::RepairFilePicked(path) => {
                if path.is_some() {
                    self.ui.dialogs.maint.repair_path = path;
                }
                Task::none()
            }
            MaintUiMessage::SetReadOnly(read_only) => {
                let slot = self.app.bases.current();
                self.dispatch(AppMessage::Bases(BasesMessage::List(
                    mychess_app::ListMessage::SetReadOnly { slot, read_only },
                )))
            }
            MaintUiMessage::Tool(message) => self.dispatch(*message),
            MaintUiMessage::LoadGame(number) => {
                let slot = self.app.bases.current();
                self.dispatch(AppMessage::Bases(BasesMessage::LoadGame { slot, number }))
            }
            MaintUiMessage::Noop => Task::none(),
        }
    }

    /// A key while the window is open: `Esc` closes the topmost card, `Enter`
    /// runs its default button, the keys of the checker act on the pair, and
    /// the keys of the program window (quit, full screen) still work.
    pub(super) fn maint_key(&mut self, input: &KeyInput, text_focused: bool) -> Task<Message> {
        let view = self.app.maintenance_view();
        let plain = input.modifiers == Modifiers::NONE;
        if input.key == Key::Named(NamedKey::Escape) && plain {
            let message = if view.confirm.is_some() {
                MaintUiMessage::App(MaintenanceMessage::Confirm(false))
            } else if view.checker_open {
                MaintUiMessage::App(MaintenanceMessage::CloseChecker)
            } else if self.ui.dialogs.maint.form.is_some() {
                MaintUiMessage::CloseForm
            } else if view.working() {
                // The window stays: it holds "Stop" (review M4).
                MaintUiMessage::App(MaintenanceMessage::Stop)
            } else {
                MaintUiMessage::Close
            };
            return self.update_maint(message);
        }
        if input.key == Key::Named(NamedKey::Enter) && plain {
            return match crate::windows::maintenance::default_message(self) {
                Some(message) => self.update(message),
                None => Task::none(),
            };
        }
        let checker = view.checker_open
            && view.confirm.is_none()
            && self.ui.dialogs.maint.form.is_none()
            && !text_focused;
        if checker && let Some(message) = checker_key(input) {
            return self.dispatch(maint(message));
        }
        match self.app.shortcuts.lookup_in(input, text_focused) {
            Some(action @ (mychess_app::Action::Quit | mychess_app::Action::FullScreen)) => {
                self.run_action(action)
            }
            // The manual opens above the window, at the chapter of the
            // maintenance (spec M9-5, D4081).
            Some(mychess_app::Action::Help) => self.toggle_manual(),
            _ => Task::none(),
        }
    }
}

/// The keys of the checker of duplicates (`1`, `2`, `t`, `U`, `s`, arrows).
fn checker_key(input: &KeyInput) -> Option<MaintenanceMessage> {
    use mychess_app::maintenance::TwinSide;
    let shift = input.modifiers.shift && !input.modifiers.command && !input.modifiers.alt;
    let plain = input.modifiers == Modifiers::NONE;
    match input.key {
        Key::Char('1') if plain => Some(MaintenanceMessage::TwinToggle(TwinSide::Left)),
        Key::Char('2') if plain => Some(MaintenanceMessage::TwinToggle(TwinSide::Right)),
        Key::Char('t') if plain => Some(MaintenanceMessage::TwinToggleBoth),
        Key::Char('s') if plain => Some(MaintenanceMessage::TwinShare),
        Key::Char('U' | 'u') if shift => Some(MaintenanceMessage::TwinUndeleteAll),
        Key::Named(NamedKey::Left) if plain => Some(MaintenanceMessage::TwinStep(-1)),
        Key::Named(NamedKey::Right) if plain => Some(MaintenanceMessage::TwinStep(1)),
        _ => None,
    }
}

/// The file dialog of the repair: the index of a database.
async fn pick_si4(title: String, filter: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title.as_str())
        .add_filter(filter.as_str(), &["si4", "SI4", "Si4"])
        .pick_file()
        .await
        .map(|handle| handle.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> KeyInput {
        KeyInput { key, modifiers }
    }

    #[test]
    fn the_checker_keys_follow_the_scid() {
        assert_eq!(
            checker_key(&key(Key::Char('1'), Modifiers::NONE)),
            Some(MaintenanceMessage::TwinToggle(
                mychess_app::maintenance::TwinSide::Left
            ))
        );
        assert_eq!(
            checker_key(&key(Key::Char('t'), Modifiers::NONE)),
            Some(MaintenanceMessage::TwinToggleBoth)
        );
        assert_eq!(checker_key(&key(Key::Char('x'), Modifiers::NONE)), None);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            checker_key(&key(Key::Char('U'), shift)),
            Some(MaintenanceMessage::TwinUndeleteAll)
        );
    }
}
