//! The opening report window (spec M7-2c, D3621-D3632; Scid
//! `tcl/tools/optable.tcl`).
//!
//! It is one window with its own board, a toolbar, the numbered sections of
//! the report, the theory table with its footnotes and a few forms (options,
//! save, exclude, favorites). The application owns the report and the
//! favorites (`AppState::opening_report_view`); this module keeps only what
//! the screen needs: the model of the shown report (built once per report and
//! language, never per frame), the text of the search field, the form that is
//! open, the board and a footnote in focus.
//!
//! In this front-end every window is a layer of the main window; the report
//! covers it with a margin. It is not a modal question: it keeps the report
//! after it closes, and a click on a game or a count acts on the application
//! while the window stays open. While it is open the keys of the game behind
//! are blocked (`Shell::modal_layer_open`).
//!
//! - `model`: the cells of the report, with the click each one carries;
//! - `toolbar`: the buttons, the progress and the warnings;
//! - `body`: the header with the board, the sections and the theory table;
//! - `forms`: the cards (options, save, exclude, favorites);
//! - `text`: the translated words of the notices.

mod body;
pub mod files;
mod forms;
pub mod model;
pub mod text;
mod toolbar;

use std::path::PathBuf;
use std::sync::Arc;

use iced::Element;
use iced::Point;
use iced::widget::{column, container, opaque, stack};
use mychess_app::opening_report::{
    OpeningReportMessage, OpeningReportPrefs, ReportFormat, ReportPart, SharedReport,
};
use mychess_app::{AppState, Message as AppMessage};

use crate::i18n::Language;
use crate::shell::{Message, Shell};
use crate::theme;
use crate::widgets::board::BoardCaches;
use crate::widgets::board::marks::MarkBrush;

pub use body::target_id;
pub use model::{Model, Target};

/// Id of the card of the window (for tests and automation).
pub const OPREP_WINDOW_ID: &str = "oprep-window";
/// Id of the search field.
pub const OPREP_FIND_ID: &str = "oprep-find";
/// Id of the field of the name of a new favorite.
pub const OPREP_FAV_NAME_ID: &str = "oprep-fav-name";
/// Id of the field of the new name of a favorite.
pub const OPREP_FAV_RENAME_ID: &str = "oprep-fav-rename";
/// Id of the card of the options.
pub const OPREP_OPTIONS_BOX_ID: &str = "oprep-options-box";
/// Id of the card of "save".
pub const OPREP_SAVE_BOX_ID: &str = "oprep-save-box";
/// Id of the card of "exclude".
pub const OPREP_EXCLUDE_BOX_ID: &str = "oprep-exclude-box";
/// Id of the card of the favorites.
pub const OPREP_FAVORITES_BOX_ID: &str = "oprep-favorites-box";
/// Id of the container of the board.
pub const OPREP_BOARD_ID: &str = "oprep-board";

/// A count of the options, which the stepper buttons change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptCount {
    /// Oldest games, 0 to 10.
    Oldest,
    /// Newest games, 0 to 10.
    Newest,
    /// Most frequent players, 0 to 10.
    MostFrequent,
    /// Highest-rated games, 0 to 10.
    HighRating,
    /// Shortest wins, 0 to 10.
    Shortest,
    /// Move orders, 0 to 10.
    MoveOrders,
    /// Extra moves of the notes, 0 to 10.
    ExtraMoves,
    /// Games of the theory table, 0 to 10 000.
    MaxTableGames,
    /// Games kept for the statistics, 1 to 100 000.
    MaxGames,
}

/// Values the two large counts step through.
const TABLE_STEPS: [u32; 12] = [
    0, 50, 100, 200, 300, 400, 500, 750, 1000, 2000, 5000, 10_000,
];
const GAMES_STEPS: [u32; 9] = [1, 100, 500, 1000, 5000, 10_000, 25_000, 50_000, 100_000];

impl OptCount {
    /// Every count, in the order of the options form.
    pub const ALL: [OptCount; 9] = [
        OptCount::Oldest,
        OptCount::Newest,
        OptCount::MostFrequent,
        OptCount::HighRating,
        OptCount::Shortest,
        OptCount::MoveOrders,
        OptCount::ExtraMoves,
        OptCount::MaxTableGames,
        OptCount::MaxGames,
    ];

    /// The Fluent key of the label.
    pub fn label_key(self) -> &'static str {
        match self {
            OptCount::Oldest => "oprep-opt-oldest",
            OptCount::Newest => "oprep-opt-newest",
            OptCount::MostFrequent => "oprep-opt-most-frequent",
            OptCount::HighRating => "oprep-opt-high-rating",
            OptCount::Shortest => "oprep-opt-shortest",
            OptCount::MoveOrders => "oprep-opt-move-orders",
            OptCount::ExtraMoves => "oprep-opt-extra-moves",
            OptCount::MaxTableGames => "oprep-opt-max-table",
            OptCount::MaxGames => "oprep-opt-max-games",
        }
    }

    /// The value in `prefs`.
    pub fn get(self, prefs: &OpeningReportPrefs) -> u32 {
        match self {
            OptCount::Oldest => u32::from(prefs.oldest),
            OptCount::Newest => u32::from(prefs.newest),
            OptCount::MostFrequent => u32::from(prefs.most_frequent),
            OptCount::HighRating => u32::from(prefs.high_rating),
            OptCount::Shortest => u32::from(prefs.shortest),
            OptCount::MoveOrders => u32::from(prefs.move_orders),
            OptCount::ExtraMoves => u32::from(prefs.extra_moves),
            OptCount::MaxTableGames => prefs.max_table_games,
            OptCount::MaxGames => prefs.max_games,
        }
    }

    fn steps(self) -> Option<&'static [u32]> {
        match self {
            OptCount::MaxTableGames => Some(&TABLE_STEPS),
            OptCount::MaxGames => Some(&GAMES_STEPS),
            _ => None,
        }
    }

    /// The next value up or down from `current`, inside the range.
    pub fn stepped(self, current: u32, up: bool) -> u32 {
        match self.steps() {
            None => {
                if up {
                    (current + 1).min(10)
                } else {
                    current.saturating_sub(1)
                }
            }
            Some(steps) => {
                let found = if up {
                    steps.iter().copied().find(|s| *s > current)
                } else {
                    steps.iter().rev().copied().find(|s| *s < current)
                };
                found.unwrap_or(current)
            }
        }
    }

    /// Whether `current` can still go up or down.
    pub fn can_step(self, current: u32, up: bool) -> bool {
        self.stepped(current, up) != current
    }

    /// Stores `value` in `prefs`.
    pub fn set(self, prefs: &mut OpeningReportPrefs, value: u32) {
        let small = u8::try_from(value.min(10)).unwrap_or(10);
        match self {
            OptCount::Oldest => prefs.oldest = small,
            OptCount::Newest => prefs.newest = small,
            OptCount::MostFrequent => prefs.most_frequent = small,
            OptCount::HighRating => prefs.high_rating = small,
            OptCount::Shortest => prefs.shortest = small,
            OptCount::MoveOrders => prefs.move_orders = small,
            OptCount::ExtraMoves => prefs.extra_moves = small,
            OptCount::MaxTableGames => prefs.max_table_games = value,
            OptCount::MaxGames => prefs.max_games = value,
        }
    }
}

/// An on/off option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptFlag {
    /// The statistics table.
    Stats,
    /// The popularity table.
    Popular,
    /// Average rating and performance.
    AvgPerf,
    /// Lengths and frequencies of the results.
    Results,
    /// The moves from the position.
    MovesFrom,
    /// The positional themes.
    Themes,
    /// The endgames.
    Endgames,
    /// The most frequent players with White.
    FrequentWhite,
    /// The most frequent players with Black.
    FrequentBlack,
    /// The shortest wins of White.
    ShortestWhite,
    /// The shortest wins of Black.
    ShortestBlack,
}

impl OptFlag {
    /// Every flag, in the order of the options form.
    pub const ALL: [OptFlag; 11] = [
        OptFlag::Stats,
        OptFlag::Popular,
        OptFlag::AvgPerf,
        OptFlag::Results,
        OptFlag::MovesFrom,
        OptFlag::Themes,
        OptFlag::Endgames,
        OptFlag::FrequentWhite,
        OptFlag::FrequentBlack,
        OptFlag::ShortestWhite,
        OptFlag::ShortestBlack,
    ];

    /// The Fluent key of the label.
    pub fn label_key(self) -> &'static str {
        match self {
            OptFlag::Stats => "oprep-opt-stats",
            OptFlag::Popular => "oprep-opt-popular",
            OptFlag::AvgPerf => "oprep-opt-avg-perf",
            OptFlag::Results => "oprep-opt-results",
            OptFlag::MovesFrom => "oprep-opt-moves-from",
            OptFlag::Themes => "oprep-opt-themes",
            OptFlag::Endgames => "oprep-opt-endgames",
            OptFlag::FrequentWhite => "oprep-opt-frequent-white",
            OptFlag::FrequentBlack => "oprep-opt-frequent-black",
            OptFlag::ShortestWhite => "oprep-opt-shortest-white",
            OptFlag::ShortestBlack => "oprep-opt-shortest-black",
        }
    }

    fn slot(self, prefs: &mut OpeningReportPrefs) -> &mut bool {
        match self {
            OptFlag::Stats => &mut prefs.stats,
            OptFlag::Popular => &mut prefs.popular,
            OptFlag::AvgPerf => &mut prefs.avg_perf,
            OptFlag::Results => &mut prefs.results,
            OptFlag::MovesFrom => &mut prefs.moves_from,
            OptFlag::Themes => &mut prefs.themes,
            OptFlag::Endgames => &mut prefs.endgames,
            OptFlag::FrequentWhite => &mut prefs.most_frequent_white,
            OptFlag::FrequentBlack => &mut prefs.most_frequent_black,
            OptFlag::ShortestWhite => &mut prefs.shortest_white,
            OptFlag::ShortestBlack => &mut prefs.shortest_black,
        }
    }

    /// The value in `prefs`.
    pub fn get(self, prefs: &OpeningReportPrefs) -> bool {
        let mut copy = prefs.clone();
        *self.slot(&mut copy)
    }

    /// Stores `on` in `prefs`.
    pub fn set(self, prefs: &mut OpeningReportPrefs, on: bool) {
        *self.slot(prefs) = on;
    }
}

/// The part and format chosen to save a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveDraft {
    /// The part of the report.
    pub part: ReportPart,
    /// The format of the file.
    pub format: ReportFormat,
}

impl SaveDraft {
    fn from_prefs(prefs: &OpeningReportPrefs) -> SaveDraft {
        SaveDraft {
            part: prefs.last_part,
            format: prefs.last_format,
        }
    }
}

/// The favorites form: the fields and the choices of the batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavDraft {
    /// The name typed for a new favorite.
    pub name: String,
    /// The favorite being renamed (its place) and the name typed.
    pub rename: Option<(usize, String)>,
    /// The choices of "generate reports".
    pub batch: SaveDraft,
}

/// The form or card that covers the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpRepForm {
    /// The options, edited as a draft until OK.
    Options(Box<OpeningReportPrefs>),
    /// Part and format to save.
    Save(SaveDraft),
    /// The move left out of the theory table.
    Exclude,
    /// The manager of the favorites.
    Favorites(FavDraft),
}

/// The menu of "copy line" opened with the right button on a row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowMenu {
    /// The text copied.
    pub line: Arc<str>,
    /// Where the pointer was.
    pub at: Point,
}

/// A model and what it was built from.
#[derive(Debug)]
pub(crate) struct Cached {
    pub(crate) report: SharedReport,
    pub(crate) language: Language,
    pub(crate) model: Model,
}

/// What the window keeps on its own.
#[derive(Debug, Default)]
pub struct OpRepUi {
    /// The form or card that is open.
    pub form: Option<OpRepForm>,
    /// What is typed in the search field.
    pub find: String,
    /// The board is turned.
    pub flipped: bool,
    /// The footnote shown under the table.
    pub note_focus: Option<u32>,
    /// The menu of "copy line".
    pub row_menu: Option<RowMenu>,
    pub(crate) caches: BoardCaches,
    pub(crate) brush: MarkBrush,
    pub(crate) cached: Option<Cached>,
    /// The rows of the model that hold the text of the search field.
    pub(crate) hits: model::Hits,
    /// Pixels the theory table is scrolled down.
    pub(crate) theory_scroll: f32,
    /// The language the application was last given the words of.
    pub(crate) labels_language: Option<Language>,
    /// The window was open at the last synchronization.
    pub(crate) was_open: bool,
}

impl OpRepUi {
    /// Whether the window covers the main window. Cheap: it is asked many
    /// times per frame.
    pub fn layer_open(app: &AppState) -> bool {
        app.opening_report().is_open()
    }

    /// The model of the report on the window, if one is built.
    pub fn model(&self) -> Option<&Model> {
        self.cached.as_ref().map(|c| &c.model)
    }

    /// The rows of the model that hold the text of the search field.
    pub fn hits(&self) -> &model::Hits {
        &self.hits
    }

    /// Forgets the state of a window that closed.
    pub(crate) fn reset(&mut self) {
        self.form = None;
        self.find.clear();
        self.flipped = false;
        self.note_focus = None;
        self.row_menu = None;
        // The model and the board caches of a report nobody sees are
        // dropped; the next report builds them again.
        self.cached = None;
        self.caches = BoardCaches::default();
        self.hits = model::Hits::default();
        self.theory_scroll = 0.0;
    }

    /// Finds again the rows that hold the search text (after the text or
    /// the model changed).
    pub(crate) fn refind(&mut self) {
        self.hits = self
            .cached
            .as_ref()
            .map(|c| c.model.search(&self.find))
            .unwrap_or_default();
    }

    /// The draft of the save form.
    pub(crate) fn save_draft(prefs: &OpeningReportPrefs) -> SaveDraft {
        SaveDraft::from_prefs(prefs)
    }
}

/// An event of the window that only the front-end handles, or that needs
/// the state of the front-end to become a message of the application.
#[derive(Debug, Clone)]
pub enum OpRepUiMessage {
    /// "Opening report" of the Tools menu: generates, sends the words.
    Open,
    /// Closes the window.
    Close,
    /// "Update".
    Refresh,
    /// "Stop" of the progress.
    Stop,
    /// Nothing (a click on the board that only the widget uses).
    Noop,
    /// The board is clicked: it turns.
    Flip,
    /// Makes the board bigger (`true`) or smaller.
    BoardSize(bool),
    /// Opens the options form.
    OpenOptions,
    /// Opens the save form.
    OpenSave,
    /// Opens the exclude form.
    OpenExclude,
    /// Opens the favorites form.
    OpenFavorites,
    /// Closes the form that is open.
    CloseForm,
    /// The search field changed.
    Find(String),
    /// Puts the focus on the search field.
    FocusFind,
    /// A footnote marker was clicked.
    NoteFocus(Option<u32>),
    /// The right button on a row of the table.
    RowMenu {
        /// The line of the row.
        line: Arc<str>,
        /// The pointer in window coordinates.
        at: Point,
    },
    /// Closes the menu of the row.
    CloseRowMenu,
    /// The theory table was scrolled to this many pixels.
    TheoryScrolled(f32),
    /// "Copy line".
    CopyLine,
    /// A stepper of the options form.
    Count(OptCount, bool),
    /// A box of the options form.
    Flag(OptFlag, bool),
    /// "Defaults" of the options form.
    OptionsDefaults,
    /// "OK" of the options form.
    OptionsOk,
    /// A part chosen in the save form.
    SavePart(ReportPart),
    /// A format chosen in the save form.
    SaveFormat(ReportFormat),
    /// "Save..." of the save form: shows the file dialog.
    SaveChoose,
    /// The answer of the file dialog (`None`: cancelled).
    SavePicked(Option<PathBuf>),
    /// A move chosen to leave out of the table (`None`: none).
    Exclude(Option<String>),
    /// The name field of the favorites form.
    FavName(String),
    /// "Add" a favorite.
    FavAdd,
    /// Generates the report of favorite `n`.
    FavOpen(usize),
    /// Starts renaming favorite `n`.
    FavRenameStart(usize),
    /// The rename field.
    FavRenameInput(String),
    /// Confirms the rename.
    FavRenameGo,
    /// Gives up the rename.
    FavRenameCancel,
    /// Removes favorite `n`.
    FavDelete(usize),
    /// Moves favorite `n` up.
    FavUp(usize),
    /// Moves favorite `n` down.
    FavDown(usize),
    /// "Import reports.dat": shows the file dialog.
    FavImportChoose,
    /// The answer of that dialog.
    FavDatPicked(Option<PathBuf>),
    /// The part of the batch.
    FavPart(ReportPart),
    /// The format of the batch.
    FavFormat(ReportFormat),
    /// "Generate reports": shows the folder dialog.
    FavBatchChoose,
    /// The answer of the folder dialog.
    FavDirPicked(Option<PathBuf>),
}

pub(crate) fn ui(message: OpRepUiMessage) -> Message {
    Message::OpRep(message)
}

pub(crate) fn orep(message: OpeningReportMessage) -> Message {
    Message::App(AppMessage::OpeningReport(message))
}

/// The layer to put over the window, if the report window is open.
pub fn layer(shell: &Shell) -> Option<Element<'_, Message>> {
    if !OpRepUi::layer_open(&shell.app) {
        return None;
    }
    let view = shell.app.opening_report_view();
    let card = container(
        column![
            toolbar::title_bar(shell, &view),
            toolbar::tools(shell, &view),
            toolbar::banners(shell, &view),
            body::content(shell, &view),
            toolbar::footer(shell),
        ]
        .spacing(8),
    )
    .id(OPREP_WINDOW_ID)
    .padding(14)
    .width(iced::Length::Fill)
    .height(iced::Length::Fill)
    .style(theme::pane_body);
    let backdrop = opaque(
        container(card)
            .padding(18)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .style(theme::scrim),
    );
    let mut layers = stack![backdrop];
    if let Some(form) = forms::overlay(shell, &view) {
        layers = layers.push(form);
    }
    if let Some(menu) = body::row_menu_layer(shell) {
        layers = layers.push(menu);
    }
    Some(layers.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_counts_step_inside_zero_to_ten() {
        assert_eq!(OptCount::Oldest.stepped(5, true), 6);
        assert_eq!(OptCount::Oldest.stepped(10, true), 10);
        assert_eq!(OptCount::Oldest.stepped(0, false), 0);
        assert!(!OptCount::Oldest.can_step(10, true));
        assert!(OptCount::Oldest.can_step(10, false));
    }

    #[test]
    fn large_counts_step_through_their_presets_from_any_value() {
        assert_eq!(OptCount::MaxTableGames.stepped(500, true), 750);
        assert_eq!(OptCount::MaxTableGames.stepped(500, false), 400);
        assert_eq!(OptCount::MaxTableGames.stepped(450, true), 500);
        assert_eq!(OptCount::MaxTableGames.stepped(450, false), 400);
        assert_eq!(OptCount::MaxTableGames.stepped(10_000, true), 10_000);
        assert_eq!(OptCount::MaxTableGames.stepped(0, false), 0);
        assert_eq!(OptCount::MaxGames.stepped(1, false), 1);
        assert_eq!(OptCount::MaxGames.stepped(100_000, true), 100_000);
        assert_eq!(OptCount::MaxGames.stepped(10_000, true), 25_000);
    }

    #[test]
    fn a_count_is_stored_in_its_own_field() {
        let mut prefs = OpeningReportPrefs::default();
        for (i, count) in OptCount::ALL.into_iter().enumerate() {
            let value = if matches!(count, OptCount::MaxTableGames | OptCount::MaxGames) {
                1000 + u32::try_from(i).unwrap_or(0)
            } else {
                u32::try_from(i).unwrap_or(0)
            };
            count.set(&mut prefs, value);
            assert_eq!(count.get(&prefs), value, "{count:?}");
        }
    }

    #[test]
    fn a_flag_is_stored_in_its_own_field() {
        let mut prefs = OpeningReportPrefs::default();
        for flag in OptFlag::ALL {
            flag.set(&mut prefs, false);
            assert!(!flag.get(&prefs), "{flag:?}");
        }
        assert!(!prefs.most_frequent_black && !prefs.stats && !prefs.shortest_white);
        OptFlag::Themes.set(&mut prefs, true);
        assert!(prefs.themes && !prefs.endgames);
    }
}
