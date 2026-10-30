//! The best games of the tree (spec M5-2, D1600-D1612): the list of the
//! games that reach the position, by rating or in database order, with the
//! choices the front-end offers.

use std::collections::BTreeSet;
use std::sync::Arc;

use mychess_db::GameSource;
use mychess_db::PlayerStats;
use mychess_db::tree::{BestOptions, TreeState, best_games};

use crate::bases::{GameRow, PickMode, SharedSource, SlotId};

/// The results of the games the list accepts (Scid: the "Result" menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BestResults {
    /// Every result.
    #[default]
    All,
    /// White wins (`1-0`).
    White,
    /// Black wins (`0-1`).
    Black,
    /// White or Black wins.
    Decisive,
    /// Draws.
    Draw,
    /// Games without a result (`*`).
    Unfinished,
}

impl BestResults {
    /// Every choice, in menu order.
    pub const ALL: [BestResults; 6] = [
        BestResults::All,
        BestResults::White,
        BestResults::Black,
        BestResults::Decisive,
        BestResults::Draw,
        BestResults::Unfinished,
    ];

    pub(crate) fn engine(self) -> mychess_db::tree::ResultFilter {
        use mychess_db::tree::ResultFilter;
        match self {
            BestResults::All => ResultFilter::All,
            BestResults::White => ResultFilter::White,
            BestResults::Black => ResultFilter::Black,
            BestResults::Decisive => ResultFilter::Decisive,
            BestResults::Draw => ResultFilter::Draw,
            BestResults::Unfinished => ResultFilter::None,
        }
    }
}

/// The order of the list (Scid: "Best" and "Order").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BestOrder {
    /// By the average rating of the players, highest first (the "Best"
    /// order of Scid; the engine calls it `Best`, D1746).
    #[default]
    Rating,
    /// The last games of the filter, in database order.
    Database,
}

impl BestOrder {
    /// Every choice, in menu order.
    pub const ALL: [BestOrder; 2] = [BestOrder::Rating, BestOrder::Database];

    pub(crate) fn engine(self) -> mychess_db::tree::BestOrder {
        match self {
            BestOrder::Rating => mychess_db::tree::BestOrder::Best,
            BestOrder::Database => mychess_db::tree::BestOrder::Order,
        }
    }
}

/// A column of the list (D1603).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BestColumn {
    /// The game number.
    Number,
    /// White.
    White,
    /// White's rating.
    WhiteElo,
    /// Black.
    Black,
    /// Black's rating.
    BlackElo,
    /// The result.
    Result,
    /// The length in moves.
    Length,
    /// The date.
    Date,
    /// The event.
    Event,
    /// The ECO code.
    Eco,
    /// The moves that follow the position.
    Moves,
}

impl BestColumn {
    /// Every column, in display order.
    pub const ALL: [BestColumn; 11] = [
        BestColumn::Number,
        BestColumn::White,
        BestColumn::WhiteElo,
        BestColumn::Black,
        BestColumn::BlackElo,
        BestColumn::Result,
        BestColumn::Length,
        BestColumn::Date,
        BestColumn::Event,
        BestColumn::Eco,
        BestColumn::Moves,
    ];

    /// The columns shown at first (every one but the event).
    pub fn defaults() -> Vec<BestColumn> {
        BestColumn::ALL
            .into_iter()
            .filter(|c| *c != BestColumn::Event)
            .collect()
    }
}

/// How many games the list can have: the choices offered (the engine limit
/// is 1000).
pub const BEST_MAX_CHOICES: [u32; 6] = [20, 50, 100, 250, 500, 1000];

/// The default size of the list (Scid: `bestMax`).
pub const BEST_DEFAULT_MAX: u32 = 100;

/// An event of the best games section.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BestMessage {
    /// Opens the section if closed, closes it if open.
    Toggle,
    /// Chooses the results; searches again.
    SetResults(BestResults),
    /// Chooses the order; searches again.
    SetOrder(BestOrder),
    /// Chooses how many games; searches again.
    SetMax(u32),
    /// Shows or hides a column.
    ToggleColumn(BestColumn),
    /// Shows or hides the list of columns.
    ToggleColumnMenu,
    /// A click on the row of game `number`.
    Pick {
        /// The game number.
        number: u32,
        /// How the selection changes.
        mode: PickMode,
    },
    /// Loads game `number` at the ply of its row (double click).
    Load(u32),
    /// Opens the game browser on the selected games.
    Browse,
    /// Copies the selected games to the Clipbase.
    Copy,
}

/// A game of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BestRow {
    /// The columns of the game list.
    pub game: GameRow,
    /// The average rating that ranks the game (0 when it is not known).
    pub avg_elo: u32,
    /// The ply of the position in the game.
    pub ply: u8,
    /// Up to six half moves from that position, in SAN.
    pub next_moves: Vec<String>,
}

/// What the job needs to build the list (the state is in its hands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BestSpec {
    pub(crate) results: BestResults,
    pub(crate) order: BestOrder,
    pub(crate) max: u32,
    pub(crate) current_ply: Option<u8>,
}

/// What a refresh was asked about the list (part of the refresh key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BestKey {
    pub(crate) results: BestResults,
    pub(crate) order: BestOrder,
    pub(crate) max: u32,
}

/// The list of the best games, as the panel keeps it.
#[derive(Debug, Clone)]
pub struct BestUi {
    pub(crate) open: bool,
    pub(crate) results: BestResults,
    pub(crate) order: BestOrder,
    pub(crate) max: u32,
    pub(crate) columns: Vec<BestColumn>,
    pub(crate) columns_open: bool,
    pub(crate) rows: Arc<Vec<BestRow>>,
    /// The database the rows are of.
    pub(crate) slot: Option<SlotId>,
    pub(crate) selected: BTreeSet<u32>,
    /// The row a range selection starts from.
    pub(crate) anchor: Option<u32>,
}

impl Default for BestUi {
    fn default() -> BestUi {
        BestUi {
            open: false,
            results: BestResults::All,
            order: BestOrder::Rating,
            max: BEST_DEFAULT_MAX,
            columns: BestColumn::defaults(),
            columns_open: false,
            rows: Arc::new(Vec::new()),
            slot: None,
            selected: BTreeSet::new(),
            anchor: None,
        }
    }
}

impl BestUi {
    /// Whether the section is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn key(&self) -> Option<BestKey> {
        self.open.then_some(BestKey {
            results: self.results,
            order: self.order,
            max: self.max,
        })
    }

    /// Forgets the list (the panel closed or the search failed).
    pub(crate) fn clear(&mut self) {
        self.rows = Arc::new(Vec::new());
        self.slot = None;
        self.selected.clear();
        self.anchor = None;
    }

    /// Puts a new list in; the selection keeps the games that are still in.
    pub(crate) fn set_rows(&mut self, slot: SlotId, rows: Vec<BestRow>) {
        // Game numbers mean nothing across databases (D1748).
        if self.slot != Some(slot) {
            self.selected.clear();
            self.anchor = None;
        }
        self.selected
            .retain(|n| rows.iter().any(|r| r.game.number == *n));
        if self
            .anchor
            .is_some_and(|a| !rows.iter().any(|r| r.game.number == a))
        {
            self.anchor = None;
        }
        self.slot = Some(slot);
        self.rows = Arc::new(rows);
    }

    /// A click on a row (D1604).
    pub(crate) fn pick(&mut self, number: u32, mode: PickMode) {
        let Some(index) = self.rows.iter().position(|r| r.game.number == number) else {
            return;
        };
        match mode {
            PickMode::Only => {
                self.selected.clear();
                self.selected.insert(number);
                self.anchor = Some(number);
            }
            PickMode::Toggle => {
                if !self.selected.remove(&number) {
                    self.selected.insert(number);
                }
                self.anchor = Some(number);
            }
            PickMode::Range => {
                let from = self
                    .anchor
                    .and_then(|a| self.rows.iter().position(|r| r.game.number == a))
                    .unwrap_or(index);
                let (low, high) = (from.min(index), from.max(index));
                self.selected = self
                    .rows
                    .get(low..=high)
                    .unwrap_or_default()
                    .iter()
                    .map(|r| r.game.number)
                    .collect();
            }
        }
    }

    /// The selected games in the order of the list.
    pub(crate) fn selected_in_order(&self) -> Vec<u32> {
        self.rows
            .iter()
            .map(|r| r.game.number)
            .filter(|n| self.selected.contains(n))
            .collect()
    }

    /// The ply of the row of a game.
    pub(crate) fn ply_of(&self, number: u32) -> Option<u8> {
        self.rows
            .iter()
            .find(|r| r.game.number == number)
            .map(|r| r.ply)
    }

    /// Shows or hides a column; the last one stays.
    pub(crate) fn toggle_column(&mut self, column: BestColumn) {
        if let Some(at) = self.columns.iter().position(|c| *c == column) {
            if self.columns.len() > 1 {
                self.columns.remove(at);
            }
        } else {
            self.columns.push(column);
            self.columns.sort_unstable();
        }
    }
}

/// The section, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct BestView {
    /// The columns shown, in order.
    pub columns: Vec<BestColumn>,
    /// The list of columns is open.
    pub columns_open: bool,
    /// The accepted results.
    pub results: BestResults,
    /// The order.
    pub order: BestOrder,
    /// How many games at most.
    pub max: u32,
    /// The games.
    pub rows: Arc<Vec<BestRow>>,
    /// The selected game numbers.
    pub selected: BTreeSet<u32>,
    /// The database the games are of is the Clipbase (copying makes no
    /// sense).
    pub in_clipbase: bool,
    /// The database the rows are of (the one the numbers mean something
    /// in); the tree may be locked on another one than the current.
    pub slot: Option<SlotId>,
}

/// Builds the list for the games of the last search, from the state the job
/// holds. The database is locked per call, never for the whole work (D986);
/// a game that cannot be read is left out.
pub(crate) fn compute(source: &SharedSource, state: &TreeState, spec: BestSpec) -> Vec<BestRow> {
    let stats = PlayerStats::from_database(&*source.read());
    let options = BestOptions {
        max: spec.max,
        results: spec.results.engine(),
        order: spec.order.engine(),
        current_ply: spec.current_ply,
        current_game: None,
    };
    let mut locking = super::message::LockingSource::new(source.clone());
    let Ok(games) = best_games(&mut locking, state, &stats, &options) else {
        return Vec::new();
    };
    let guard = source.read();
    games
        .into_iter()
        .filter_map(|best| {
            let entry = GameSource::entry(&*guard, best.game).ok()?;
            Some(BestRow {
                game: GameRow::from_entry(&*guard, best.game, &entry),
                avg_elo: best.avg_elo,
                ply: best.ply,
                next_moves: best.next_moves,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(number: u32) -> BestRow {
        BestRow {
            game: GameRow::unreadable(number),
            avg_elo: 0,
            ply: 0,
            next_moves: Vec::new(),
        }
    }

    fn ui_with(numbers: &[u32]) -> BestUi {
        let mut ui = BestUi::default();
        ui.set_rows(SlotId::CLIPBASE, numbers.iter().map(|n| row(*n)).collect());
        ui
    }

    #[test]
    fn a_plain_click_selects_only_that_game() {
        let mut ui = ui_with(&[5, 3, 9]);
        ui.pick(3, PickMode::Only);
        ui.pick(9, PickMode::Only);
        assert_eq!(ui.selected_in_order(), [9]);
    }

    #[test]
    fn toggle_adds_and_removes() {
        let mut ui = ui_with(&[5, 3, 9]);
        ui.pick(3, PickMode::Only);
        ui.pick(9, PickMode::Toggle);
        assert_eq!(ui.selected_in_order(), [3, 9]);
        ui.pick(3, PickMode::Toggle);
        assert_eq!(ui.selected_in_order(), [9]);
    }

    #[test]
    fn a_range_follows_the_order_of_the_list_not_the_numbers() {
        let mut ui = ui_with(&[5, 3, 9, 1]);
        ui.pick(5, PickMode::Only);
        ui.pick(9, PickMode::Range);
        assert_eq!(ui.selected_in_order(), [5, 3, 9]);
    }

    #[test]
    fn a_new_list_keeps_only_the_games_still_in_it() {
        let mut ui = ui_with(&[5, 3, 9]);
        ui.pick(3, PickMode::Only);
        ui.pick(9, PickMode::Toggle);
        ui.set_rows(SlotId::CLIPBASE, vec![row(9), row(2)]);
        assert_eq!(ui.selected_in_order(), [9]);
    }

    #[test]
    fn the_last_column_cannot_be_hidden_and_order_is_kept() {
        let mut ui = BestUi {
            columns: vec![BestColumn::White],
            ..BestUi::default()
        };
        ui.toggle_column(BestColumn::White);
        assert_eq!(ui.columns, [BestColumn::White]);
        ui.toggle_column(BestColumn::Number);
        assert_eq!(ui.columns, [BestColumn::Number, BestColumn::White]);
        ui.toggle_column(BestColumn::Number);
        assert_eq!(ui.columns, [BestColumn::White]);
    }

    #[test]
    fn the_default_columns_leave_out_the_event() {
        let columns = BestColumn::defaults();
        assert_eq!(columns.len(), 10);
        assert!(!columns.contains(&BestColumn::Event));
    }

    #[test]
    fn the_key_exists_only_while_open() {
        let mut ui = BestUi::default();
        assert!(ui.key().is_none());
        ui.open = true;
        assert_eq!(ui.key().map(|k| k.max), Some(BEST_DEFAULT_MAX));
    }
}
