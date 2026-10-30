//! The visual state and the messages shared by the four panels of slice
//! M6-4d (spec D3430-D3439): the crosstable, the statistics and graphs of the
//! filter, the rating graph and the piece tracker. Whatever the application
//! owns (options, results, computations) is not kept here; this is only what
//! is typed, unfolded or hovered.

use std::path::PathBuf;

use mychess_app::charts::{FilterGraphKind, GraphField, StatsTab};
use mychess_app::crosstable::ExportFormat;
use mychess_core::Square;

use crate::i18n::Language;

/// The fields of the window of the piece tracker that are typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackerField {
    /// First move.
    Start,
    /// Last move.
    End,
}

/// What the panels keep on their own.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DataPanelsUi {
    /// The options of the crosstable are unfolded.
    pub cross_options_open: bool,
    /// The game of the table that was clicked (the strip with "Browse" and
    /// "Load" shows for it).
    pub cross_selected: Option<u32>,
    /// What is typed in the field of the row color.
    pub cross_color_text: Option<String>,
    /// The language the labels of the crosstable were last sent in.
    pub labels_language: Option<Language>,
    /// The options of the statistics are unfolded.
    pub stats_options_open: bool,
    /// What is typed in the axis fields (minimum, maximum, step) of the
    /// Year, Rating and Moves graphs.
    pub axis_text: [[Option<String>; 3]; 3],
    /// What is typed in the field of the rating graph.
    pub player_text: String,
    /// The options of the rating graph are unfolded.
    pub rating_options_open: bool,
    /// The square of the map under the pointer.
    pub tracker_hover: Option<Square>,
    /// What is typed in the fields of the first and last move.
    pub tracker_text: [Option<String>; 2],
    /// The text field of a panel that has the focus, if any (one of
    /// [`DataPanelsUi::FIELD_IDS`]). Up, Down, Page Up, Page Down and Enter
    /// are text there, not commands of the list or of the board.
    pub focused: Option<&'static str>,
    /// A file dialog of the export of the crosstable is open (new clicks
    /// on the export buttons are ignored meanwhile).
    pub export_pending: bool,
    /// The table the open export dialog proposed its file name for: if
    /// another table is on screen when the user answers, nothing is written
    /// (D3454).
    pub export_table: Option<mychess_app::crosstable::CrosstableToken>,
    /// Pixels the grid of the crosstable is scrolled down.
    pub cross_scroll: f32,
    /// The height of the viewport of the grid, when it has been scrolled
    /// (`0.0`: not known yet).
    pub cross_view_h: f32,
}

/// The id of the minimum field of the axis of a graph.
pub const AXIS_MIN_ID: &str = "panel-axis-min";
/// The id of the maximum field of the axis of a graph.
pub const AXIS_MAX_ID: &str = "panel-axis-max";
/// The id of the step field of the axis of a graph.
pub const AXIS_STEP_ID: &str = "panel-axis-step";
/// The id of the field of the row color of the crosstable.
pub const CROSS_COLOR_ID: &str = "panel-cross-color";
/// The id of the player field of the rating graph.
pub const RATING_PLAYER_ID: &str = "panel-rating-player";
/// The id of the field of the first move of the tracker.
pub const TRACKER_START_ID: &str = "panel-tracker-start";
/// The id of the field of the last move of the tracker.
pub const TRACKER_END_ID: &str = "panel-tracker-end";

impl DataPanelsUi {
    /// Every text field of the panels, for the focus probe.
    pub const FIELD_IDS: [&'static str; 7] = [
        AXIS_MIN_ID,
        AXIS_MAX_ID,
        AXIS_STEP_ID,
        CROSS_COLOR_ID,
        RATING_PLAYER_ID,
        TRACKER_START_ID,
        TRACKER_END_ID,
    ];

    /// The id of the axis field `field`.
    pub fn axis_id(field: GraphField) -> &'static str {
        match field {
            GraphField::Min => AXIS_MIN_ID,
            GraphField::Max => AXIS_MAX_ID,
            GraphField::Step => AXIS_STEP_ID,
        }
    }

    /// Drops the typed text of the fields that do not have the focus: what
    /// is shown is then the value of the application again.
    pub fn drop_unfocused_text(&mut self) {
        for fields in &mut self.axis_text {
            for (i, text) in fields.iter_mut().enumerate() {
                let id = [AXIS_MIN_ID, AXIS_MAX_ID, AXIS_STEP_ID][i];
                if self.focused != Some(id) {
                    *text = None;
                }
            }
        }
        if self.focused != Some(CROSS_COLOR_ID) {
            self.cross_color_text = None;
        }
        for (i, id) in [TRACKER_START_ID, TRACKER_END_ID].into_iter().enumerate() {
            if self.focused != Some(id) {
                self.tracker_text[i] = None;
            }
        }
    }

    /// The index of the typed axis fields of `kind` (the decades have none).
    pub fn axis_index(kind: FilterGraphKind) -> Option<usize> {
        match kind {
            FilterGraphKind::Year => Some(0),
            FilterGraphKind::Elo => Some(1),
            FilterGraphKind::Moves => Some(2),
            FilterGraphKind::Decade => None,
        }
    }

    /// The index of a field in the arrays of typed text.
    pub fn field_index(field: GraphField) -> usize {
        match field {
            GraphField::Min => 0,
            GraphField::Max => 1,
            GraphField::Step => 2,
        }
    }

    /// Forgets the typed axis fields (the Defaults button).
    pub fn clear_axis_text(&mut self) {
        self.axis_text = Default::default();
    }
}

/// An event of the panels that only the shell can handle (a typed field, a
/// dialog, a menu entry that sends two messages).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DataPanelsMessage {
    /// Unfolds or folds the options of the crosstable.
    ToggleCrossOptions,
    /// A result of the table was clicked (`None`: the strip is closed).
    SelectGame(Option<u32>),
    /// "Browse": lists the clicked game in the browser.
    BrowseGame(u32),
    /// Asks for the file of an export.
    PickExport(ExportFormat),
    /// The file dialog answered (`None`: cancelled).
    ExportPicked(ExportFormat, Option<PathBuf>),
    /// The text of the row color changed.
    RowColorEdit(String),
    /// Unfolds or folds the options of the statistics.
    ToggleStatsOptions,
    /// An axis field of the graph on show was edited (the value goes to the
    /// application on Enter or when the field loses the focus).
    AxisEdit(FilterGraphKind, GraphField, String),
    /// The grid of the crosstable was scrolled: pixels from the top and the
    /// height of the viewport.
    CrossScrolled {
        /// Pixels scrolled down.
        offset: f32,
        /// The height of the viewport.
        height: f32,
    },
    /// Enter in an axis field: the typed value is applied.
    AxisSubmit(FilterGraphKind, GraphField),
    /// Enter in the field of the row color: the typed color is applied.
    RowColorSubmit,
    /// The answer of the focus probe for a text field of the panels.
    FieldFocus(&'static str, bool),
    /// "Defaults" of the axes of the graph.
    ResetAxes,
    /// Opens the statistics panel (if closed) on a tab; the entries
    /// "Relative graph" and "Absolute graph" of the menu.
    OpenStatsTab(StatsTab),
    /// The text of the player field of the rating graph changed.
    PlayerEdit(String),
    /// Adds the typed player.
    AddPlayer,
    /// Unfolds or folds the options of the rating graph.
    ToggleRatingOptions,
    /// The pointer moved over the map of the tracker.
    TrackerHover(Option<Square>),
    /// A field of the window of the tracker was edited.
    TrackerEdit(TrackerField, String),
}

/// Keeps the digits of `text`, at most `max` of them (the fields of the
/// Scid take four).
pub fn digits(text: &str, max: usize) -> String {
    text.chars()
        .filter(char::is_ascii_digit)
        .take(max)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfocused_fields_forget_their_typed_text() {
        let mut ui = DataPanelsUi {
            focused: Some(AXIS_MAX_ID),
            ..DataPanelsUi::default()
        };
        ui.axis_text[0] = [Some("1".into()), Some("2".into()), None];
        ui.cross_color_text = Some("#fff".into());
        ui.tracker_text = [Some("3".into()), None];
        ui.drop_unfocused_text();
        assert_eq!(ui.axis_text[0], [None, Some("2".into()), None]);
        assert_eq!(ui.cross_color_text, None);
        assert_eq!(ui.tracker_text, [None, None]);
    }

    #[test]
    fn typed_numbers_keep_only_digits() {
        assert_eq!(digits("1a9b95x", 4), "1995");
        assert_eq!(digits("123456", 4), "1234");
        assert_eq!(digits("", 4), "");
    }

    #[test]
    fn the_decades_have_no_axis_fields() {
        assert_eq!(DataPanelsUi::axis_index(FilterGraphKind::Decade), None);
        assert_eq!(DataPanelsUi::axis_index(FilterGraphKind::Moves), Some(2));
        assert_eq!(DataPanelsUi::field_index(GraphField::Step), 2);
    }
}
