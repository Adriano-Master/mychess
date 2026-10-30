// Derived from Scid vs PC 4.27, src/crosstab.cpp and src/crosstab.h
// (`Crosstable`) and src/tkscid.cpp (`isCrosstableGame`,
// `sc_game_crosstable`). Copyright (c) 2000-2004 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The tournament table, or crosstable (milestone M6, slice 4a, decisions
//! D3350 to D3369 and D3395 to D3398 of
//! `docs/specs/m6-4-tabela-estatisticas-rastreador.md`).
//!
//! Three steps, each usable alone:
//!
//! 1. [`select_tournament`] picks the games of the tournament of a reference
//!    game ([`TournamentReference`]) out of the **whole** database (the filter
//!    does not count): same Event and Site, and a date within 3 months of the
//!    reference or the same `EventDate` ([`TournamentSelection`]).
//! 2. [`load_games`] reads those games' index entries into plain data
//!    ([`TournamentGames`]); [`Crosstable::from_games`] turns that data into a
//!    table without touching a database, so it can be tested with hand-made
//!    games. [`build_crosstable`] chains steps 1 and 2 and the table.
//! 3. The renderers [`render_text`], [`render_html`] and [`render_latex`]
//!    print the table; the interface can instead draw it from the ready-made
//!    [`Crosstable::rows`], whose cells are already in display order.
//!
//! All the arithmetic is integer: points are kept in half points, the
//! Sonneborn-Berger tie-break in quarter points, the performance and rating
//! change with the tables and the truncating divisions of the Scid.
//!
//! ## Differences from Scid
//!
//! - A tournament whose reference date has only a year takes the **whole
//!   year** (D3356); Scid stops at `Y.11.31` and loses December.
//! - The sort is **stable**: players with everything equal stay in order of
//!   first appearance (D3360); Scid's exchange sort is not stable.
//! - A candidate game with a year but no month and day belongs when its year
//!   is the year of the reference game (D3399). The Scid compares it with the
//!   year of the middle of the packed date window, which for a reference in
//!   the first or last months of a year can be the neighbouring year.
//! - The selection stops at [`MAX_TOURNAMENT_GAMES`] games, with a notice;
//!   entries that cannot be read are counted in a notice, not hidden.
//! - `Auto` picks a knockout by structure (D3399), not by the shape of the
//!   Round tags alone.
//! - Names are padded by characters, not bytes, so names with accents stay
//!   aligned in the text output.
//! - HTML and LaTeX are complete documents with escaped names (D3363); the
//!   cell of the tallies is closed, and the Swiss LaTeX table has no extra
//!   `&` at the end of the row. A two-player LaTeX table has one result
//!   column, not two.
//! - With numbered columns and 100 players or more, the hundreds digit is
//!   right (Scid repeats the tens digit).
//! - Notices (player limits) are plain text, not Tcl markup.

mod elo;
mod format;
mod select;
mod table;
mod tiebreak;

use std::fmt;

use mychess_core::{Color, PartialDate};
use thiserror::Error;

use crate::progress::{CancelToken, ProgressSink};
use crate::si4::Si4Error;
use crate::source::GameSource;

pub use elo::{fide_category, opponent_elo, performance_rating, rating_change};
pub use format::{CrosstableLabels, render_html, render_latex, render_text};
pub use select::{
    MAX_TOURNAMENT_GAMES, TournamentGame, TournamentGames, TournamentReference,
    TournamentSelection, load_games, load_games_cancellable, parse_round, select_tournament,
};

/// Most players a table holds (`CROSSTABLE_MaxPlayers`).
pub const MAX_PLAYERS: usize = 1000;

/// Round numbers go from 1 to `MAX_ROUNDS - 1` (`CROSSTABLE_MaxRounds`, 60).
pub const MAX_ROUNDS: u32 = 60;

/// Most players shown in an all-play-all table (`apaLimit`, T18).
pub const ALL_PLAY_ALL_LIMIT: usize = 300;

/// Estimated size of an all-play-all result grid (rows times columns times
/// games per pair) above which [`Notice::LargeOutput`] is added. 200 000
/// cells is a few hundred kilobytes of text: 300 players with one game per
/// pair (90 000) stay below it, three games per pair do not.
pub const ALL_PLAY_ALL_LARGE_OUTPUT: usize = 200_000;

/// Why a table could not be built.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CrosstableError {
    /// The reference game number does not exist (Scid: `Invalid game number`).
    #[error("invalid game number {0}")]
    InvalidGame(u32),
    /// The index entry of the reference game could not be read.
    #[error("cannot read game {game}: {source}")]
    Read {
        /// The game number.
        game: u32,
        /// The reason.
        source: Si4Error,
    },
    /// Fewer than two players: there is no table (Scid: `No crosstable for
    /// this game.`). Also the answer when the Event or Site of the reference
    /// is not in the database.
    #[error("no crosstable for this game")]
    NoTable,
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

/// The format of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrosstableFormat {
    /// Pick one (`BestMode`, plus the knockout rule of D3395).
    #[default]
    Auto,
    /// Every player against every other, one column per player.
    AllPlayAll,
    /// One column per round.
    Swiss,
    /// Knockout: one block per round with a line for each match.
    Knockout,
}

/// The format actually used (`Auto` resolved).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableMode {
    /// All-play-all.
    AllPlayAll,
    /// Swiss.
    Swiss,
    /// Knockout.
    Knockout,
}

/// The order of the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrosstableSort {
    /// By points, then fewer games, then the optional tie-breaks.
    #[default]
    Score,
    /// By name, ascending, by bytes.
    Name,
    /// By country, ascending, by bytes.
    Country,
    /// By Elo, highest first.
    Rating,
}

/// The options of the table (the "Options" menu of the Scid window). The
/// default is the one of the Scid.
// Each flag is one independent menu item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosstableOptions {
    /// The format of the table.
    pub format: CrosstableFormat,
    /// The order of the rows.
    pub sort: CrosstableSort,
    /// Show the age in years (needs the birth date from a bio provider).
    pub show_ages: bool,
    /// Show the nationality (needs a bio provider).
    pub show_countries: bool,
    /// Show the `(+won -lost =drawn)` tallies.
    pub show_tallies: bool,
    /// Show the Elo column and the performance (needs some Elo).
    pub show_ratings: bool,
    /// Show the titles (needs a bio provider).
    pub show_titles: bool,
    /// Show the tie-break score (`(Tie)`): Sonneborn-Berger or Buchholz.
    pub show_tiebreaks: bool,
    /// Include the games marked for deletion.
    pub include_deleted: bool,
    /// Swiss tables: show `w`/`b` after the opponent number.
    pub swiss_colors: bool,
    /// All-play-all: number the columns instead of naming them.
    pub numbered_columns: bool,
    /// Sort ties by the score against the players on the same points.
    pub tiebreak_head_to_head: bool,
    /// Sort ties by the number of wins.
    pub tiebreak_wins: bool,
    /// Three points for a win (and one for a draw) instead of 1 and 1/2.
    pub three_points: bool,
    /// Leave a blank row between groups of equal score (sort by score only).
    pub separate_score_groups: bool,
    /// The decimal separator of the scores.
    pub decimal_point: char,
}

impl Default for CrosstableOptions {
    fn default() -> CrosstableOptions {
        CrosstableOptions {
            format: CrosstableFormat::Auto,
            sort: CrosstableSort::Score,
            show_ages: true,
            show_countries: true,
            show_tallies: true,
            show_ratings: true,
            show_titles: true,
            show_tiebreaks: false,
            include_deleted: false,
            swiss_colors: true,
            numbered_columns: false,
            tiebreak_head_to_head: false,
            tiebreak_wins: false,
            three_points: false,
            separate_score_groups: false,
            decimal_point: '.',
        }
    }
}

/// What to compute: which tournament, and how to show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosstableRequest {
    /// The game the tournament is found from (usually the current game).
    pub reference: TournamentReference,
    /// The options of the table.
    pub options: CrosstableOptions,
}

/// Title, country and birth date of a player (the Scid reads them from the
/// spelling file). Without a provider these columns do not exist (D3365).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerBio {
    /// `gm`, `im`, `wgm`... or empty.
    pub title: String,
    /// The three-letter country code or empty.
    pub country: String,
    /// The birth date; [`PartialDate::UNKNOWN`] if unknown.
    pub birth: PartialDate,
}

/// Gives title, country and birth date of a player by name.
pub trait PlayerBioProvider: Send + Sync {
    /// The data of the player named `name`, if known.
    fn bio(&self, name: &str) -> Option<PlayerBio>;
}

/// The result of a game from the point of view of one player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The player won (`1`, Swiss `+`).
    Win,
    /// The player lost (`0`, Swiss `-`).
    Loss,
    /// A draw (`=`, shown `½`).
    Draw,
    /// No result (`*`).
    Unknown,
}

impl Outcome {
    /// The character of the Scid: `1`, `0`, `=` or `*`.
    pub fn result_char(self) -> char {
        match self {
            Outcome::Win => '1',
            Outcome::Loss => '0',
            Outcome::Draw => '=',
            Outcome::Unknown => '*',
        }
    }

    /// The mark of the Swiss table: `+`, `-`, `=` or `*`.
    pub fn swiss_char(self) -> char {
        match self {
            Outcome::Win => '+',
            Outcome::Loss => '-',
            Outcome::Draw => '=',
            Outcome::Unknown => '*',
        }
    }

    /// Half points of the Scid's `RESULT_SCORE`: win 2, draw 1, loss 0 and no
    /// result 1 (used only by the knockout lines).
    pub fn knockout_half_points(self) -> u32 {
        match self {
            Outcome::Win => 2,
            Outcome::Draw | Outcome::Unknown => 1,
            Outcome::Loss => 0,
        }
    }
}

/// One game between two players, seen from one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clash {
    /// The number of the game in the database (1-based).
    pub game: u32,
    /// The result for the player of the row.
    pub outcome: Outcome,
    /// The colour the player of the row had.
    pub color: Color,
    /// The numeric round (0 when the Round tag is not a number).
    pub round: u32,
    /// The opponent, as a 0-based position in the sorted table.
    pub opponent: usize,
}

/// The performance of a player (T8, T9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Performance {
    /// Average Elo of the rated opponents, limited to a margin of 350 of the
    /// player's own Elo (`OpponentElo`).
    pub average_opponent: u32,
    /// Percentage score against rated opponents, rounded.
    pub percentage: u32,
    /// The performance rating.
    pub rating: u32,
    /// The rating change; `None` when the player has no Elo.
    pub change: Option<i32>,
}

/// One row of the table: a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRow {
    /// The position in the table, from 1.
    pub rank: usize,
    /// The name ID in the database.
    pub player: u32,
    /// The name.
    pub name: String,
    /// The highest Elo in the games of the tournament; 0 if none.
    pub elo: u32,
    /// The title ("" if unknown).
    pub title: String,
    /// The country ("" if unknown).
    pub country: String,
    /// The age in years, when known and positive.
    pub age: Option<u32>,
    /// Points in half points (with three points for a win: 6 per win, 2 per
    /// draw).
    pub score_half: u32,
    /// Games played, including those without result.
    pub games: u32,
    /// Sonneborn-Berger in quarter points (all-play-all), or Buchholz in
    /// half points (Swiss); 0 for knockout.
    pub tiebreak: u32,
    /// Head-to-head score against players on the same points.
    pub head_to_head: u32,
    /// Games won.
    pub won: u32,
    /// Games lost.
    pub lost: u32,
    /// Games drawn.
    pub drawn: u32,
    /// Games with a result against a rated opponent (0: no performance).
    pub rated_games: u32,
    /// The performance, when it is shown (0 < rating < 5000).
    pub performance: Option<Performance>,
    /// All-play-all: for each column (the sorted position of the opponent),
    /// the games against that opponent in order. Empty for other formats.
    pub versus: Vec<Vec<Clash>>,
    /// Swiss: for each round `1..=max_round` (index 0 is round 1), the last
    /// game of the round. Empty for other formats.
    pub by_round: Vec<Option<Clash>>,
}

/// One line of a knockout round: two players and their games of the round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnockoutMatch {
    /// Row index of the player shown on the left (the one who had White in
    /// their first game).
    pub first: usize,
    /// Row index of the opponent, shown on the right.
    pub second: usize,
    /// The games of the round between them, from the left player's side.
    pub games: Vec<Clash>,
    /// Points of the left player in half points (`*` counts one half).
    pub score_half: u32,
}

/// One block of a knockout table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnockoutRound {
    /// The round number, from 1.
    pub round: u32,
    /// The matches, in the order of the rows.
    pub matches: Vec<KnockoutMatch>,
}

/// A warning that goes with the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// More than [`MAX_PLAYERS`] players: the table is incomplete.
    PlayerLimit,
    /// All-play-all with more than [`ALL_PLAY_ALL_LIMIT`] players: only the
    /// first ones are shown.
    TooManyPlayers,
    /// The tournament has more than [`MAX_TOURNAMENT_GAMES`] games: only the
    /// first ones were used.
    GameLimit,
    /// This many index entries could not be read and were left out.
    Unreadable(u32),
    /// The all-play-all grid is very large (more than
    /// [`ALL_PLAY_ALL_LARGE_OUTPUT`] cells); the Swiss format is shorter.
    LargeOutput,
}

impl Notice {
    /// The text of the Scid, in English.
    pub fn english(self) -> String {
        match self {
            Notice::PlayerLimit => {
                format!("Player limit of {MAX_PLAYERS} reached. Table is incomplete")
            }
            Notice::TooManyPlayers => {
                "Warning: Too many players for all-play-all; try displaying as a swiss tournament."
                    .to_owned()
            }
            Notice::GameLimit => {
                format!("Game limit of {MAX_TOURNAMENT_GAMES} reached. Table is incomplete")
            }
            Notice::Unreadable(count) => {
                format!("Warning: {count} unreadable games were left out. Table is incomplete")
            }
            Notice::LargeOutput => {
                "Warning: very large all-play-all table; try displaying as a swiss tournament."
                    .to_owned()
            }
        }
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.english())
    }
}

/// Games by result, over the whole tournament.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResultCounts {
    /// White wins.
    pub white: u32,
    /// Black wins.
    pub black: u32,
    /// Draws.
    pub draws: u32,
    /// Games without result.
    pub unknown: u32,
}

/// Which columns exist (the Scid's `Print*` flags).
// One flag per optional column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Columns {
    /// The Elo column, and the performance.
    pub ratings: bool,
    /// The title column.
    pub titles: bool,
    /// The country column.
    pub countries: bool,
    /// The age column.
    pub ages: bool,
    /// The tie-break score.
    pub tiebreaks: bool,
    /// The `(+ - =)` tallies.
    pub tallies: bool,
}

/// A computed table, ready to draw or to render as text, HTML or LaTeX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crosstable {
    /// The Event of the reference game.
    pub event: String,
    /// The Site of the reference game.
    pub site: String,
    /// The earliest date seen (Scid: `firstSeenDate`, which starts at the
    /// reference date and includes unknown dates).
    pub first_date: PartialDate,
    /// The latest date seen.
    pub last_date: PartialDate,
    /// The format actually used.
    pub mode: TableMode,
    /// The options the table was built with.
    pub options: CrosstableOptions,
    /// The rows, in order. For all-play-all at most
    /// [`ALL_PLAY_ALL_LIMIT`]; otherwise every player.
    pub rows: Vec<PlayerRow>,
    /// How many players the tournament has (can be more than `rows.len()`).
    pub total_players: usize,
    /// Games in the tournament, including those without result.
    pub game_count: u32,
    /// Games by result.
    pub results: ResultCounts,
    /// The most games between any two players (columns per opponent).
    pub max_clashes: u32,
    /// The highest numeric round (1 to 59) seen; 0 if none.
    pub max_round: u32,
    /// The average Elo of the rated players; 0 if none.
    pub average_elo: u32,
    /// The FIDE category of the average (all-play-all only); 0 for none.
    pub category: u32,
    /// The columns that exist.
    pub columns: Columns,
    /// Knockout only: the blocks of rounds `1..=max_round`.
    pub knockout: Vec<KnockoutRound>,
    /// Warnings.
    pub notices: Vec<Notice>,
    /// Width of the longest name, in characters, over all players.
    pub longest_name: usize,
    /// Width of the position number: 2, or 3 with more than 99 rows.
    pub number_width: usize,
    /// Length of the line of dashes of the text output.
    pub line_width: usize,
}

/// Selects the tournament of `request.reference` in `db`, reads its games
/// and builds the table.
///
/// # Errors
///
/// [`CrosstableError::NoTable`] with fewer than two players (including the
/// case where no game matches), [`CrosstableError::Cancelled`] when `cancel`
/// is set during the scan of the index or the building of the table.
pub fn build_crosstable<D: GameSource + ?Sized>(
    db: &D,
    request: &CrosstableRequest,
    bio: Option<&dyn PlayerBioProvider>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Crosstable, CrosstableError> {
    let selection = select_tournament(
        db,
        &request.reference,
        request.options.include_deleted,
        progress,
        cancel,
    )?;
    let games = load_games_cancellable(db, &selection, cancel)?;
    Crosstable::from_games_cancellable(&games, &request.options, bio, cancel)
}

#[cfg(test)]
mod tests;
