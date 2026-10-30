//! The crosstable panel as the front-end draws it: plain data, no
//! `mychess-db` types (spec M6-4, D3350).

use mychess_core::{Color, PartialDate};
use mychess_db::{
    Crosstable, CrosstableClash, CrosstableOutcome, CrosstableSort, PlayerRow, TableMode,
};

use super::message::{CrossLabels, CrossWarning, CrosstableNotice};
use super::prefs::CrosstablePrefs;

/// The format actually used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossMode {
    /// Every player against every other.
    AllPlayAll,
    /// One column per round.
    Swiss,
    /// One block per round.
    Knockout,
}

/// The result of a game for the player of the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossOutcome {
    /// Won (`1`, Swiss `+`).
    Win,
    /// Lost (`0`, `-`).
    Loss,
    /// Drawn (`½`, `=`).
    Draw,
    /// No result (`*`).
    Unknown,
}

/// One game in a cell; a click on it loads the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClashView {
    /// The number of the game in the database, from 1.
    pub game: u32,
    /// The result for the player of the row.
    pub outcome: CrossOutcome,
    /// The colour the player of the row had.
    pub color: Color,
    /// The numeric round (0: none).
    pub round: u32,
    /// The row of the opponent, from 0.
    pub opponent: usize,
    /// This is the game in the editor (the Scid paints it green).
    pub current: bool,
}

/// A cell of the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrossCellView {
    /// All-play-all: the player against themselves.
    Diagonal,
    /// All-play-all: the games against the player of that column (none:
    /// empty, drawn as `.`).
    Games(Vec<ClashView>),
    /// Swiss: the game of that round (none: the player did not play).
    Round(Option<ClashView>),
}

/// The performance of a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrossPerformance {
    /// The performance rating.
    pub rating: u32,
    /// The rating change; `None` when the player has no rating.
    pub change: Option<i32>,
}

/// A row: a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossRowView {
    /// The position, from 1.
    pub rank: usize,
    /// The name.
    pub name: String,
    /// The rating, 0 if none.
    pub elo: u32,
    /// The title.
    pub title: String,
    /// The country.
    pub country: String,
    /// The age in years.
    pub age: Option<u32>,
    /// The points as text (`1.5`).
    pub score: String,
    /// Games played.
    pub games: u32,
    /// The tie-break as text (Sonneborn-Berger or Buchholz); empty for a
    /// knockout.
    pub tiebreak: String,
    /// Games won.
    pub won: u32,
    /// Games lost.
    pub lost: u32,
    /// Games drawn.
    pub drawn: u32,
    /// The performance, when it is shown.
    pub performance: Option<CrossPerformance>,
    /// A blank line goes before this row (groups of equal score).
    pub group_break: bool,
    /// The cells of the grid: one per player (all-play-all) or per round
    /// (Swiss); none for a knockout.
    pub cells: Vec<CrossCellView>,
}

/// A line of a knockout round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossMatchView {
    /// The row of the left player.
    pub first: usize,
    /// The row of the right player.
    pub second: usize,
    /// The games of the round, from the left player's side.
    pub games: Vec<ClashView>,
    /// Points of the left player as text.
    pub score: String,
}

/// A block of a knockout table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossRoundView {
    /// The round, from 1.
    pub round: u32,
    /// The matches.
    pub matches: Vec<CrossMatchView>,
}

/// The columns that exist.
// One flag per column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct CrossColumnsView {
    /// Ratings and performances.
    pub ratings: bool,
    /// Titles.
    pub titles: bool,
    /// Countries.
    pub countries: bool,
    /// Ages.
    pub ages: bool,
    /// The tie-break.
    pub tiebreaks: bool,
    /// The tallies.
    pub tallies: bool,
}

/// A table, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossTableView {
    /// The Event.
    pub event: String,
    /// The Site.
    pub site: String,
    /// The earliest date seen.
    pub first_date: PartialDate,
    /// The latest date seen.
    pub last_date: PartialDate,
    /// The format used.
    pub mode: CrossMode,
    /// The columns.
    pub columns: CrossColumnsView,
    /// The rows, in order.
    pub rows: Vec<CrossRowView>,
    /// Players in the tournament (can be more than the rows).
    pub total_players: usize,
    /// Games in the tournament.
    pub game_count: u32,
    /// White wins, black wins, draws and games without result.
    pub results: (u32, u32, u32, u32),
    /// The most games between two players.
    pub max_clashes: u32,
    /// The highest numeric round.
    pub max_round: u32,
    /// The average rating, 0 if none.
    pub average_elo: u32,
    /// The FIDE category (all-play-all), 0 if none.
    pub category: u32,
    /// The knockout rounds.
    pub knockout: Vec<CrossRoundView>,
    /// The warnings.
    pub warnings: Vec<CrossWarning>,
}

/// What the body of the panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrosstableBody {
    /// The panel is closed.
    Closed,
    /// No table: see the notice.
    Empty(CrosstableNotice),
    /// Computing the first table; progress in the view.
    Computing,
    /// A table.
    Table(std::sync::Arc<CrossTableView>),
}

/// The panel, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosstableView {
    /// What the body shows.
    pub body: CrosstableBody,
    /// The table on show is not the one for the game on screen (another
    /// is being computed).
    pub stale: bool,
    /// `(done, total)` of the computation in flight.
    pub progress: Option<(u64, u64)>,
    /// The options.
    pub prefs: CrosstablePrefs,
    /// The words (the front-end sent them).
    pub labels: CrossLabels,
    /// The last remark.
    pub notice: Option<CrosstableNotice>,
    /// The panel follows the game on screen (otherwise it was opened on a
    /// game of the locator).
    pub following: bool,
}

fn outcome(o: CrosstableOutcome) -> CrossOutcome {
    match o {
        CrosstableOutcome::Win => CrossOutcome::Win,
        CrosstableOutcome::Loss => CrossOutcome::Loss,
        CrosstableOutcome::Draw => CrossOutcome::Draw,
        CrosstableOutcome::Unknown => CrossOutcome::Unknown,
    }
}

fn clash(c: &CrosstableClash, current: Option<u32>) -> ClashView {
    ClashView {
        game: c.game,
        outcome: outcome(c.outcome),
        color: c.color,
        round: c.round,
        opponent: c.opponent,
        current: current == Some(c.game),
    }
}

/// Points in half points as `1.5`.
pub(crate) fn points_text(half: u32) -> String {
    format!("{}.{}", half / 2, if half % 2 == 1 { '5' } else { '0' })
}

fn tiebreak_text(mode: TableMode, tiebreak: u32) -> String {
    match mode {
        // Quarter points.
        TableMode::AllPlayAll => format!("{}.{:02}", tiebreak / 4, (tiebreak % 4) * 25),
        // Half points.
        TableMode::Swiss => points_text(tiebreak),
        TableMode::Knockout => String::new(),
    }
}

fn row(
    table: &Crosstable,
    index: usize,
    r: &PlayerRow,
    previous: Option<u32>,
    current: Option<u32>,
) -> CrossRowView {
    let group_break = table.options.separate_score_groups
        && table.options.sort == CrosstableSort::Score
        && previous.is_some_and(|p| p != r.score_half);
    let cells = match table.mode {
        TableMode::AllPlayAll => r
            .versus
            .iter()
            .enumerate()
            .map(|(column, games)| {
                if column == index {
                    CrossCellView::Diagonal
                } else {
                    CrossCellView::Games(games.iter().map(|c| clash(c, current)).collect())
                }
            })
            .collect(),
        TableMode::Swiss => r
            .by_round
            .iter()
            .map(|c| CrossCellView::Round(c.as_ref().map(|c| clash(c, current))))
            .collect(),
        TableMode::Knockout => Vec::new(),
    };
    CrossRowView {
        rank: r.rank,
        name: r.name.clone(),
        elo: r.elo,
        title: r.title.clone(),
        country: r.country.clone(),
        age: r.age,
        score: points_text(r.score_half),
        games: r.games,
        tiebreak: tiebreak_text(table.mode, r.tiebreak),
        won: r.won,
        lost: r.lost,
        drawn: r.drawn,
        performance: r.performance.map(|p| CrossPerformance {
            rating: p.rating,
            change: p.change,
        }),
        group_break,
        cells,
    }
}

/// The table as the front-end draws it. `current` is the number of the game
/// in the editor, if it belongs to the database of the table.
pub(crate) fn table_view(table: &Crosstable, current: Option<u32>) -> CrossTableView {
    let mut previous = None;
    let rows = table
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let view = row(table, i, r, previous, current);
            previous = Some(r.score_half);
            view
        })
        .collect();
    let c = table.columns;
    CrossTableView {
        event: table.event.clone(),
        site: table.site.clone(),
        first_date: table.first_date,
        last_date: table.last_date,
        mode: match table.mode {
            TableMode::AllPlayAll => CrossMode::AllPlayAll,
            TableMode::Swiss => CrossMode::Swiss,
            TableMode::Knockout => CrossMode::Knockout,
        },
        columns: CrossColumnsView {
            ratings: c.ratings,
            titles: c.titles,
            countries: c.countries,
            ages: c.ages,
            tiebreaks: c.tiebreaks,
            tallies: c.tallies,
        },
        rows,
        total_players: table.total_players,
        game_count: table.game_count,
        results: (
            table.results.white,
            table.results.black,
            table.results.draws,
            table.results.unknown,
        ),
        max_clashes: table.max_clashes,
        max_round: table.max_round,
        average_elo: table.average_elo,
        category: table.category,
        knockout: table
            .knockout
            .iter()
            .map(|round| CrossRoundView {
                round: round.round,
                matches: round
                    .matches
                    .iter()
                    .map(|m| CrossMatchView {
                        first: m.first,
                        second: m.second,
                        games: m.games.iter().map(|c| clash(c, current)).collect(),
                        score: points_text(m.score_half),
                    })
                    .collect(),
            })
            .collect(),
        warnings: table
            .notices
            .iter()
            .map(|n| CrossWarning::from_engine(*n))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_have_one_decimal() {
        assert_eq!(points_text(3), "1.5");
        assert_eq!(points_text(4), "2.0");
        assert_eq!(points_text(0), "0.0");
    }

    #[test]
    fn tiebreaks_follow_their_units() {
        // Sonneborn-Berger in quarter points: 5 -> 1.25.
        assert_eq!(tiebreak_text(TableMode::AllPlayAll, 5), "1.25");
        // Buchholz in half points: 4 -> 2.0.
        assert_eq!(tiebreak_text(TableMode::Swiss, 4), "2.0");
        assert_eq!(tiebreak_text(TableMode::Knockout, 4), "");
    }
}
