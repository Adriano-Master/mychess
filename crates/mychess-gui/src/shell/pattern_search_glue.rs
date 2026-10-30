//! The part of the shell that serves the window of the search for tactics
//! and patterns: the numeric fields, the list of conditions to add, the
//! hidden window, the names of a pattern taken from a ready-made one and the
//! file dialogs of the import and the export (spec M6-2d).

use iced::Task;
use mychess_app::pattern_search::model::Condition;
use mychess_app::pattern_search::{
    ConditionKind, ParamValue, PatternEdit, PatternSearchMessage, PatternSearchView, PatternTab,
};
use mychess_app::{Action, Message as AppMessage};

use super::{Message, Shell};
use crate::host::PatternFileDialog;
use crate::windows::pattern_search_dialog::{NumField, PatternUiMessage};

impl Shell {
    fn pattern_message(&mut self, message: PatternSearchMessage) -> Task<Message> {
        self.dispatch(AppMessage::PatternSearch(message))
    }

    /// The name and the description a pattern is saved or exported with:
    /// the ones typed in the "Custom" tab, the translated ones of the
    /// ready-made pattern in the other (D3299 item 33).
    fn pattern_names(&self, view: &PatternSearchView) -> (String, String) {
        if view.tab == PatternTab::Advanced {
            return (view.draft.name.clone(), view.draft.description.clone());
        }
        let Some(entry) = view
            .presets
            .iter()
            .find(|e| view.selected_preset.as_deref() == Some(e.id))
        else {
            return (String::new(), String::new());
        };
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("n", i64::from(view.params.n));
        (
            self.i18n.tr_args(entry.name_key, &args),
            self.i18n.tr(&format!("{}-desc", entry.name_key)),
        )
    }

    /// The group that receives a new condition: the card selected if it is
    /// an AND / OR group, else the nearest one above it. A first card that
    /// is not a group is wrapped in one first.
    fn pattern_add(&mut self, kind: ConditionKind) -> Task<Message> {
        let Some(view) = self.app.pattern_search_view() else {
            return Task::none();
        };
        let mut parent = view.selection.clone().unwrap_or_default();
        let mut tasks = Vec::new();
        loop {
            let group = view
                .tree
                .iter()
                .find(|n| n.path == parent)
                .is_some_and(|n| matches!(n.kind, ConditionKind::All | ConditionKind::Any));
            if group {
                break;
            }
            if parent.is_empty() {
                tasks.push(
                    self.pattern_message(PatternSearchMessage::Edit(PatternEdit::Wrap {
                        path: Vec::new(),
                        kind: mychess_app::pattern_search::GroupKind::All,
                    })),
                );
                break;
            }
            parent.pop();
        }
        tasks.push(
            self.pattern_message(PatternSearchMessage::Edit(PatternEdit::Add {
                parent,
                kind,
            })),
        );
        Task::batch(tasks)
    }

    fn pattern_number(&mut self, field: NumField, typed: &str) -> Task<Message> {
        let Some(view) = self.app.pattern_search_view() else {
            return Task::none();
        };
        let Some(number) = self.ui.dialogs.pattern.type_into(field.clone(), typed) else {
            return Task::none();
        };
        let mut scope = view.scope.clone();
        let message = match field {
            NumField::MoveMin => {
                scope.min_move = u16::try_from(number.clamp(0, 999)).unwrap_or(999);
                PatternSearchMessage::SetScope(scope)
            }
            NumField::MoveMax => {
                scope.max_move = u16::try_from(number.clamp(0, 999)).unwrap_or(999);
                PatternSearchMessage::SetScope(scope)
            }
            NumField::Hold => {
                scope.hold = u8::try_from(number.clamp(1, 40)).unwrap_or(40);
                PatternSearchMessage::SetScope(scope)
            }
            NumField::ParamNodes => PatternSearchMessage::SetParam(ParamValue::MaxNodes(
                u32::try_from(number).unwrap_or(u32::MAX),
            )),
            NumField::CardNodes(path) => {
                let Some(node) = view.tree.iter().find(|n| n.path == path) else {
                    return Task::none();
                };
                let Condition::Mate(mut mate) = node.condition.clone() else {
                    return Task::none();
                };
                mate.max_nodes = u32::try_from(number).unwrap_or(u32::MAX);
                PatternSearchMessage::Edit(PatternEdit::Replace {
                    path,
                    condition: Box::new(Condition::Mate(mate)),
                })
            }
        };
        self.pattern_message(message)
    }

    /// An event of the window of the search for patterns.
    pub(super) fn update_pattern_search(&mut self, message: PatternUiMessage) -> Task<Message> {
        match message {
            PatternUiMessage::Send(message) => {
                // A preset, a tab or a load changes values under the drafts.
                self.ui.dialogs.pattern.clear_drafts();
                self.pattern_message(message)
            }
            PatternUiMessage::Number(field, typed) => self.pattern_number(field, &typed),
            PatternUiMessage::ToggleAdd => {
                self.ui.dialogs.pattern.toggle_add();
                Task::none()
            }
            PatternUiMessage::Add(kind) => self.pattern_add(kind),
            PatternUiMessage::Hide(hidden) => {
                self.ui.dialogs.pattern.set_hidden(hidden);
                Task::none()
            }
            PatternUiMessage::Customize => {
                let Some(view) = self.app.pattern_search_view() else {
                    return Task::none();
                };
                let (name, description) = self.pattern_names(&PatternSearchView {
                    tab: PatternTab::Presets,
                    ..view
                });
                self.ui.dialogs.pattern.clear_drafts();
                self.pattern_message(PatternSearchMessage::Customize { name, description })
            }
            PatternUiMessage::Save => {
                let Some(view) = self.app.pattern_search_view() else {
                    return Task::none();
                };
                let (name, description) = self.pattern_names(&view);
                self.pattern_message(PatternSearchMessage::SaveAs { name, description })
            }
            PatternUiMessage::PickImport => {
                let dialog = PatternFileDialog::new(&self.i18n, false);
                Task::perform(dialog.run(), |path| {
                    Message::PatternUi(PatternUiMessage::ImportPicked(path))
                })
            }
            PatternUiMessage::ImportPicked(Some(path)) => {
                self.pattern_message(PatternSearchMessage::Import(path))
            }
            PatternUiMessage::PickExport => {
                let dialog = PatternFileDialog::new(&self.i18n, true);
                Task::perform(dialog.run(), |path| {
                    Message::PatternUi(PatternUiMessage::ExportPicked(path))
                })
            }
            PatternUiMessage::ExportPicked(Some(path)) => {
                let Some(view) = self.app.pattern_search_view() else {
                    return Task::none();
                };
                let (name, description) = self.pattern_names(&view);
                self.pattern_message(PatternSearchMessage::Export {
                    path,
                    name,
                    description,
                })
            }
            PatternUiMessage::ImportPicked(None) | PatternUiMessage::ExportPicked(None) => {
                Task::none()
            }
        }
    }

    /// Opening the window (by the action or the message) brings it back
    /// from behind the bar.
    pub(super) fn note_pattern_open(&mut self, message: &AppMessage) {
        let opens = matches!(
            message,
            AppMessage::Action(Action::SearchPattern)
                | AppMessage::PatternSearch(PatternSearchMessage::Open)
        );
        if opens {
            self.ui.dialogs.pattern.set_hidden(false);
        }
    }
}
