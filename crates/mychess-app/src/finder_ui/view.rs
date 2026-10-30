//! Snapshots of the finder windows, ready to draw.

use super::players::{PlayerForm, PlayerRowView};
use super::tournaments::{TournamentForm, TournamentRowView};
use crate::spelling_ui::{PlayerSortKey, Shared, TournamentSortKey};

/// The player finder window.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerFinderView {
    /// The file name of the database, for the title.
    pub file_name: Option<String>,
    /// The players of the database (all of them, not only the rows).
    pub players: usize,
    /// The texts of the form.
    pub form: PlayerForm,
    /// The rows (shared, cheap to clone).
    pub rows: Shared<Vec<PlayerRowView>>,
    /// The rows have the country column.
    pub country_column: bool,
    /// The column of the order.
    pub sort: PlayerSortKey,
    /// The order is descending.
    pub descending: bool,
    /// The table is being built: games done and total.
    pub building: Option<(u64, u64)>,
    /// Some Elo come from the spelling file (a hint for the column).
    pub estimated: bool,
    /// There is no database to list.
    pub no_base: bool,
}

/// The tournament finder window.
#[derive(Debug, Clone, PartialEq)]
pub struct TournamentFinderView {
    /// The file name of the database, for the title.
    pub file_name: Option<String>,
    /// The player the list was opened for, for the title.
    pub player: Option<String>,
    /// The texts of the form.
    pub form: TournamentForm,
    /// The rows (shared); `None` before the first answer.
    pub rows: Option<Shared<Vec<TournamentRowView>>>,
    /// The column of the order.
    pub sort: TournamentSortKey,
    /// The search is running: games done and total.
    pub running: Option<(u64, u64)>,
    /// There is no database to search.
    pub no_base: bool,
}
