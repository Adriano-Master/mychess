//! The part of the shell that serves the four data panels of slice M6-4d
//! (crosstable, statistics and graphs of the filter, rating graph, piece
//! tracker): the panes that follow the application, the labels and the year
//! the application asks the front-end for, and the events of their fields
//! (spec D3430-D3439).

use iced::Task;
use mychess_app::Message as AppMessage;
use mychess_app::charts::{ChartsMessage, FilterGraphKind, GraphField};
use mychess_app::crosstable::CrosstableMessage;
use mychess_app::piece_tracker::PieceTrackerMessage;
use mychess_app::{BasesMessage, BrowseMessage};

use super::{Message, Shell};
use crate::host::CrosstableSaveDialog;
use crate::windows::crosstable_panel;
use crate::windows::data_panels::{
    CROSS_COLOR_ID, DataPanelsMessage, DataPanelsUi, TRACKER_END_ID, TRACKER_START_ID,
    TrackerField, digits,
};
use crate::windows::layout::PaneKind;
use crate::windows::panel_kit::{current_year, parse_hex};
use crate::windows::ptracker_panel::edit_move;

impl Shell {
    /// An event of the panels that only the shell handles.
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(super) fn update_panels(&mut self, message: DataPanelsMessage) -> Task<Message> {
        match message {
            DataPanelsMessage::ToggleCrossOptions => {
                let ui = &mut self.ui.dialogs.panels;
                ui.cross_options_open = !ui.cross_options_open;
                Task::none()
            }
            DataPanelsMessage::SelectGame(game) => {
                self.ui.dialogs.panels.cross_selected = game;
                Task::none()
            }
            DataPanelsMessage::BrowseGame(number) => {
                let Some(slot) = self.app.crosstable_slot() else {
                    return Task::none();
                };
                self.dispatch(AppMessage::Bases(BasesMessage::Browse(
                    BrowseMessage::Open {
                        slot,
                        numbers: vec![number],
                    },
                )))
            }
            DataPanelsMessage::PickExport(_) if self.ui.dialogs.panels.export_pending => {
                Task::none()
            }
            DataPanelsMessage::PickExport(format) => {
                self.ui.dialogs.panels.export_pending = true;
                self.ui.dialogs.panels.export_table = self.app.crosstable_token();
                let name = self.app.crosstable_export_name(format).unwrap_or_default();
                let dialog = CrosstableSaveDialog::new(&self.i18n, format, name);
                Task::perform(dialog.run(), move |path| {
                    Message::Panels(DataPanelsMessage::ExportPicked(format, path))
                })
            }
            DataPanelsMessage::ExportPicked(format, Some(path)) => {
                self.ui.dialogs.panels.export_pending = false;
                // The name was proposed for the table of the token: the app
                // refuses to write another one (D3454).
                let message = match self.ui.dialogs.panels.export_table.take() {
                    Some(token) => CrosstableMessage::ExportOf {
                        token,
                        format,
                        path,
                    },
                    None => CrosstableMessage::Export { format, path },
                };
                self.dispatch(AppMessage::Crosstable(message))
            }
            DataPanelsMessage::ExportPicked(_, None) => {
                self.ui.dialogs.panels.export_pending = false;
                self.ui.dialogs.panels.export_table = None;
                Task::none()
            }
            DataPanelsMessage::RowColorEdit(text) => {
                let ui = &mut self.ui.dialogs.panels;
                ui.focused = Some(CROSS_COLOR_ID);
                let valid = parse_hex(&text).is_some();
                ui.cross_color_text = Some(text.clone());
                if valid {
                    self.dispatch(AppMessage::Crosstable(CrosstableMessage::SetRowColor(text)))
                } else {
                    Task::none()
                }
            }
            DataPanelsMessage::RowColorSubmit => {
                match self.ui.dialogs.panels.cross_color_text.take() {
                    Some(text) if parse_hex(&text).is_some() => {
                        self.dispatch(AppMessage::Crosstable(CrosstableMessage::SetRowColor(text)))
                    }
                    _ => Task::none(),
                }
            }
            DataPanelsMessage::ToggleStatsOptions => {
                let ui = &mut self.ui.dialogs.panels;
                ui.stats_options_open = !ui.stats_options_open;
                Task::none()
            }
            DataPanelsMessage::AxisEdit(kind, field, text) => {
                let text = digits(&text, 4);
                if let Some(k) = DataPanelsUi::axis_index(kind) {
                    let ui = &mut self.ui.dialogs.panels;
                    ui.axis_text[k][DataPanelsUi::field_index(field)] = Some(text);
                    ui.focused = Some(DataPanelsUi::axis_id(field));
                }
                Task::none()
            }
            DataPanelsMessage::CrossScrolled { offset, height } => {
                let ui = &mut self.ui.dialogs.panels;
                ui.cross_scroll = if offset.is_finite() {
                    offset.max(0.0)
                } else {
                    0.0
                };
                ui.cross_view_h = if height.is_finite() {
                    height.max(0.0)
                } else {
                    0.0
                };
                Task::none()
            }
            DataPanelsMessage::AxisSubmit(kind, field) => self.apply_axis_text(kind, field),
            DataPanelsMessage::FieldFocus(id, on) => {
                let ui = &mut self.ui.dialogs.panels;
                if on {
                    ui.focused = Some(id);
                    return Task::none();
                }
                if ui.focused != Some(id) {
                    return Task::none();
                }
                ui.focused = None;
                // An axis field applies what was typed when it loses the
                // focus; the others already sent their value.
                let kind = self.app.prefs.filter_graph.kind;
                let field = [GraphField::Min, GraphField::Max, GraphField::Step]
                    .into_iter()
                    .find(|f| DataPanelsUi::axis_id(*f) == id);
                let task = match field {
                    Some(field) => self.apply_axis_text(kind, field),
                    None => Task::none(),
                };
                self.ui.dialogs.panels.drop_unfocused_text();
                task
            }
            DataPanelsMessage::ResetAxes => {
                self.ui.dialogs.panels.clear_axis_text();
                self.dispatch(AppMessage::Charts(ChartsMessage::ResetGraphAxes))
            }
            DataPanelsMessage::OpenStatsTab(tab) => {
                let open = self.change_panes(|ui| ui.open_pane(PaneKind::FilterStats));
                let select = self.dispatch(AppMessage::Charts(ChartsMessage::SelectTab(tab)));
                Task::batch([open, select])
            }
            DataPanelsMessage::PlayerEdit(text) => {
                self.ui.dialogs.panels.player_text = text;
                Task::none()
            }
            DataPanelsMessage::AddPlayer => {
                let name = std::mem::take(&mut self.ui.dialogs.panels.player_text);
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return Task::none();
                }
                self.dispatch(AppMessage::Charts(ChartsMessage::AddPlayer(name)))
            }
            DataPanelsMessage::ToggleRatingOptions => {
                let ui = &mut self.ui.dialogs.panels;
                ui.rating_options_open = !ui.rating_options_open;
                Task::none()
            }
            DataPanelsMessage::TrackerHover(square) => {
                self.ui.dialogs.panels.tracker_hover = square;
                Task::none()
            }
            DataPanelsMessage::TrackerEdit(field, text) => {
                let (text, value) = edit_move(&text);
                let panels = &mut self.ui.dialogs.panels;
                panels.focused = Some(match field {
                    TrackerField::Start => TRACKER_START_ID,
                    TrackerField::End => TRACKER_END_ID,
                });
                match field {
                    TrackerField::Start => {
                        panels.tracker_text[0] = Some(text);
                        // The last move may follow the first.
                        panels.tracker_text[1] = None;
                    }
                    TrackerField::End => panels.tracker_text[1] = Some(text),
                }
                match value {
                    Some(value) => self.dispatch(AppMessage::PieceTracker(match field {
                        TrackerField::Start => PieceTrackerMessage::SetStart(value),
                        TrackerField::End => PieceTrackerMessage::SetEnd(value),
                    })),
                    None => Task::none(),
                }
            }
        }
    }

    /// Sends the value typed in an axis field to the application, if it is
    /// a number. The typed text stays (the field is still being edited)
    /// until the field loses the focus.
    fn apply_axis_text(&mut self, kind: FilterGraphKind, field: GraphField) -> Task<Message> {
        let Some(k) = DataPanelsUi::axis_index(kind) else {
            return Task::none();
        };
        let typed = self.ui.dialogs.panels.axis_text[k][DataPanelsUi::field_index(field)].clone();
        match typed.and_then(|t| t.parse::<i32>().ok()) {
            Some(value) => self.dispatch(AppMessage::Charts(ChartsMessage::SetGraphField {
                kind,
                field,
                value,
            })),
            None => Task::none(),
        }
    }

    /// Whether a text field of a visible data panel has the focus: its keys
    /// are text (Up, Down, Page Up, Page Down, Enter), not commands of the
    /// list of games or of the board.
    pub(super) fn panel_field_focused(&self) -> bool {
        self.ui.dialogs.panels.focused.is_some()
            && [
                PaneKind::Crosstable,
                PaneKind::FilterStats,
                PaneKind::RatingGraph,
                PaneKind::PieceTracker,
            ]
            .into_iter()
            .any(|kind| self.ui.is_visible(kind))
    }

    /// Asks the widgets which text field of the panels has the focus. Run
    /// after every click (`text_input` has no "focus lost" event).
    pub(super) fn probe_panel_focus() -> Task<Message> {
        let tasks: Vec<_> = DataPanelsUi::FIELD_IDS
            .into_iter()
            .map(|id| {
                iced::widget::operation::is_focused(id)
                    .map(move |on| Message::Panels(DataPanelsMessage::FieldFocus(id, on)))
            })
            .collect();
        Task::batch(tasks)
    }

    /// Tells the application which of the four panels are open, which starts
    /// or stops their computations (D3430), sends the translated words of
    /// the table (D3413) and the current year (D3415).
    pub(super) fn sync_data_panels(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let wanted = |shell: &Shell, kind| shell.ui.is_open(kind);
        // The crosstable.
        let open = wanted(self, PaneKind::Crosstable);
        if open != self.app.crosstable().is_open() {
            let message = if open {
                CrosstableMessage::Open
            } else {
                CrosstableMessage::Close
            };
            tasks.push(self.dispatch(AppMessage::Crosstable(message)));
        }
        let language = self.i18n.language();
        if open && self.ui.dialogs.panels.labels_language != Some(language) {
            self.ui.dialogs.panels.labels_language = Some(language);
            let labels = crosstable_panel::labels(&self.i18n);
            tasks.push(
                self.dispatch(AppMessage::Crosstable(CrosstableMessage::SetLabels(
                    Box::new(labels),
                ))),
            );
        }
        // The statistics and the graphs of the filter.
        let open = wanted(self, PaneKind::FilterStats);
        if open != self.app.charts().stats_is_open() {
            if open {
                tasks.push(
                    self.dispatch(AppMessage::Charts(ChartsMessage::SetYear(current_year()))),
                );
                tasks.push(self.dispatch(AppMessage::Charts(ChartsMessage::OpenStats)));
            } else {
                tasks.push(self.dispatch(AppMessage::Charts(ChartsMessage::CloseStats)));
            }
        }
        // The rating graph.
        let open = wanted(self, PaneKind::RatingGraph);
        if open != self.app.charts().rating_is_open() {
            if open {
                tasks.push(
                    self.dispatch(AppMessage::Charts(ChartsMessage::SetYear(current_year()))),
                );
                tasks.push(self.dispatch(AppMessage::Charts(ChartsMessage::OpenRating)));
            } else {
                tasks.push(self.dispatch(AppMessage::Charts(ChartsMessage::CloseRating)));
            }
        }
        // The piece tracker.
        let open = wanted(self, PaneKind::PieceTracker);
        if open != self.app.piece_tracker().is_open() {
            let message = if open {
                PieceTrackerMessage::Open
            } else {
                PieceTrackerMessage::Close
            };
            tasks.push(self.dispatch(AppMessage::PieceTracker(message)));
        }
        Task::batch(tasks)
    }
}
