// Scid: tcl/tools/preport.tcl `::preport::makeReportWin` 137-311 and `report`
// 517-748, src/tkscid.cpp `sc_report` 14280-14504 (`reports[REPORT_PLAYER]`),
// `sc_report_create` 14514-14611, src/optable.cpp `TopPlayers` 1576-1779 and
// `TopEcoCodes` 1787-1888. Copyright (c) 2001-2003 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License
// (see its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The player report (milestone M7, slice 3a;
//! `docs/specs/m7-3-relatorio-de-jogador.md`, decisions M7-3/D3650 to M7-3/D3699).
//!
//! [`player_report`] selects the games of one player with one colour
//! ([`games_of_player`]), optionally keeps those that reach a position, and
//! feeds them to the engine of the opening report ([`build_report_core`]), so
//! the statistics, themes, endgames and theory table are the same. What is
//! specific to the player is added to the core: the most frequent opponents
//! and the groups of ECO codes, both with the score **of the player** (M7-3/D3652,
//! every figure of the report is seen from the player, not from White).
//!
//! The report never changes a filter of the caller (M7-3/D3653): the selection is
//! private. The result does not depend on the number of threads.

mod labels;
mod select;

use mychess_core::{Color, EcoBook, Move, Position};

pub use labels::PlayerLabels;
pub use select::games_of_player;

use super::engine::{CoreInput, Shifted, build_report_core};
use super::model::{MAX_STEM_PLIES, ReportCore};
use super::options::{
    MAX_COUNT_OPTION, MAX_GAMES_LIMIT, MAX_TABLE_GAMES_LIMIT, ReportOptions, SectionSet,
};
use super::render::{self, Fmt, ReportPart};
use super::stats::{EcoGroup, PlayerRow, oriented, score_tenths};
use super::{ReportError, deepest_eco, report_stats_queries, walk_stem};
use crate::crosstable::ResultCounts;
use crate::filter_stats::StatsConfig;
use crate::player_stats::PlayerStats;
use crate::progress::{CancelToken, ProgressSink};
use crate::source::GameSource;

/// An opponent of the player: the same row as the "most frequent players" of
/// the opening report, but the score is the one of the player against it.
pub type FrequentPlayer = PlayerRow;

/// A group of ECO codes (`B90-B99`) with the score of the player.
pub type EcoGroupShare = EcoGroup;

/// How the name of the request is compared with the names of the database
/// (decision M7-3/D3651).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NameMatch {
    /// The name as it is in the name base (the case counts).
    #[default]
    Exact,
    /// The names that start with the text, ignoring the case.
    Prefix,
    /// The names that contain the text, ignoring the case (the behaviour of
    /// the Scid).
    Contains,
}

/// Where the games of the report start.
#[derive(Debug, Clone, Copy)]
pub enum ReportStart<'a> {
    /// The beginning of the game: every game of the player with the colour.
    Beginning,
    /// Only the games that reach this position (first occurrence, exact
    /// search), and the moves from the standard start to it.
    Position(&'a Position, &'a [Move]),
}

/// The options of a player report (`::preport::setOptions`; defaults from
/// `tcl/start.tcl` 820-834).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerReportOptions {
    /// The statistics table (`Stats`).
    pub stats: bool,
    /// How many of the oldest games to list (0 to 10).
    pub oldest: u8,
    /// How many of the newest games to list (0 to 10).
    pub newest: u8,
    /// How many of the most frequent opponents to list (0 to 10).
    pub opponents: u8,
    /// Lengths and frequencies of the results (`Results`).
    pub results: bool,
    /// Average rating and performance (`AvgPerf`).
    pub avg_perf: bool,
    /// How many of the games of the highest average rating to list (0 to 10).
    pub high_rating: u8,
    /// How many groups of ECO codes to list (0 to 10).
    pub eco_codes: u8,
    /// The positional themes (`Themes`).
    pub themes: bool,
    /// The endgame classes (`Endgames`).
    pub endgames: bool,
    /// The most games in the theory table (0 turns it off; at most
    /// [`MAX_TABLE_GAMES_LIMIT`]).
    pub max_table_games: u32,
    /// Extra moves of the notes of the theory table (0 to 10).
    pub extra_moves: u8,
}

impl Default for PlayerReportOptions {
    fn default() -> PlayerReportOptions {
        PlayerReportOptions {
            stats: true,
            oldest: 5,
            newest: 5,
            opponents: 6,
            results: true,
            avg_perf: true,
            high_rating: 8,
            eco_codes: 6,
            themes: true,
            endgames: true,
            max_table_games: 500,
            extra_moves: 1,
        }
    }
}

impl PlayerReportOptions {
    /// The options with every value limited to its range.
    #[must_use]
    pub fn normalized(&self) -> PlayerReportOptions {
        let count = |v: u8| v.min(MAX_COUNT_OPTION);
        PlayerReportOptions {
            oldest: count(self.oldest),
            newest: count(self.newest),
            opponents: count(self.opponents),
            high_rating: count(self.high_rating),
            eco_codes: count(self.eco_codes),
            max_table_games: self.max_table_games.min(MAX_TABLE_GAMES_LIMIT),
            extra_moves: count(self.extra_moves),
            ..self.clone()
        }
    }

    /// The options of the shared engine and renderers: the sections the
    /// player report has, up to 100 000 games (M7-3/D3655) and no excluded move.
    #[must_use]
    pub fn to_report_options(&self) -> ReportOptions {
        let o = self.normalized();
        let mut sections = SectionSet::empty();
        sections.set(SectionSet::STATS, o.stats);
        sections.set(SectionSet::AVG_PERF, o.avg_perf);
        sections.set(SectionSet::RESULTS, o.results);
        sections.set(SectionSet::THEMES, o.themes);
        sections.set(SectionSet::ENDGAMES, o.endgames);
        ReportOptions {
            sections,
            oldest: o.oldest,
            newest: o.newest,
            most_frequent: o.opponents,
            most_frequent_white: true,
            most_frequent_black: true,
            high_rating: o.high_rating,
            shortest: 0,
            move_orders: 0,
            max_table_games: o.max_table_games,
            max_games: MAX_GAMES_LIMIT,
            extra_moves: o.extra_moves,
            ..ReportOptions::default()
        }
    }

    /// Whether the section 1 ("Statistics and history") is shown.
    pub fn has_stats_section(&self) -> bool {
        self.stats || self.oldest > 0 || self.newest > 0 || self.opponents > 0 || self.results
    }

    /// Whether the section 2 ("Ratings and performance") is shown.
    pub fn has_ratings_section(&self) -> bool {
        self.avg_perf || self.high_rating > 0
    }

    /// Whether the section 3 ("Moves and themes") is shown.
    pub fn has_moves_section(&self) -> bool {
        self.themes || self.eco_codes > 0 || self.endgames
    }
}

/// Everything [`player_report`] needs besides the database.
#[derive(Debug)]
pub struct PlayerReportRequest<'a> {
    /// The player as typed (surname first, `Surname, Given`).
    pub name: &'a str,
    /// The colour the player has in the games of the report.
    pub color: Color,
    /// How the name is compared with the names of the database.
    pub matching: NameMatch,
    /// The beginning of the game or a position.
    pub start: ReportStart<'a>,
    /// The options.
    pub options: &'a PlayerReportOptions,
    /// The ECO book, for the code in the header (with a position).
    pub eco_book: Option<&'a EcoBook>,
    /// The estimated ratings.
    pub player_stats: &'a PlayerStats,
    /// The lines of the statistics block; `None` is the default of the
    /// statistics window for the year of `today`.
    pub stats_config: Option<&'a StatsConfig>,
    /// Today (the clock is read by the caller).
    pub today: mychess_core::PartialDate,
}

/// Wins, draws and losses of the player (and the games without result).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScoreCounts {
    /// Games the player won.
    pub wins: u32,
    /// Drawn games.
    pub draws: u32,
    /// Games the player lost.
    pub losses: u32,
    /// Games without a result (`*`).
    pub unknown: u32,
}

impl ScoreCounts {
    fn from_core(total: &ResultCounts, color: Color) -> ScoreCounts {
        let seen = oriented(total, color);
        ScoreCounts {
            wins: seen.white,
            draws: seen.draws,
            losses: seen.black,
            unknown: seen.unknown,
        }
    }

    /// Games with a result.
    pub fn finished(&self) -> u32 {
        self.wins
            .saturating_add(self.draws)
            .saturating_add(self.losses)
    }

    /// The score of the player in tenths of a per cent: `(2 wins + draws) *
    /// 500 / finished`; 0 without games (the `*` games are left out).
    pub fn score_tenths(&self) -> u32 {
        score_tenths(&ResultCounts {
            white: self.wins,
            black: self.losses,
            draws: self.draws,
            unknown: self.unknown,
        })
    }

    /// The score in per cent, rounded half up.
    pub fn score_percent(&self) -> u32 {
        self.score_tenths().saturating_add(5) / 10
    }
}

/// A player report (decision M7-3/D3699).
#[derive(Debug, Clone)]
pub struct PlayerReport {
    /// The shared data. Its lines, counts and table are in the
    /// White-and-Black form of the games; use [`PlayerReport::player_score`]
    /// and the fields below for the point of view of the player.
    pub core: ReportCore,
    /// The name as typed (trimmed).
    pub name: String,
    /// The colour of the player.
    pub color: Color,
    /// How the name was matched.
    pub matching: NameMatch,
    /// The moves from the start to the position, in SAN, at most
    /// [`MAX_STEM_PLIES`] (the last ones); empty for the beginning.
    pub stem: Vec<String>,
    /// How many half moves of the start of the line are not in `stem`.
    pub stem_omitted: usize,
    /// The reference position; `None` for the beginning of the game.
    pub position: Option<Position>,
    /// The ECO code of the deepest book position on the line, if any.
    pub eco: Option<String>,
    /// The most frequent opponents (up to 10), with the score of the player
    /// against each.
    pub opponents: Vec<FrequentPlayer>,
    /// The most frequent groups of ECO codes (up to 10), with the score of
    /// the player.
    pub eco_groups: Vec<EcoGroupShare>,
    /// Wins, draws and losses of the player over all the report games.
    pub player_score: ScoreCounts,
}

impl PlayerReport {
    /// Games in the report (`*` included).
    pub fn game_count(&self) -> u32 {
        self.core.report_games()
    }

    /// The number of the first move of the theory table.
    pub fn first_move_number(&self) -> usize {
        usize::midpoint(self.stem.len() + self.stem_omitted, 2)
    }
}

/// The name in the natural order (decision M7-3/D3657): `Surname, Given` becomes
/// `Given Surname`; text after a second comma stays after a comma
/// (`Silva, João, Jr` is `João Silva, Jr`); a name without comma is kept.
///
/// ```
/// use mychess_db::report::natural_name;
///
/// assert_eq!(natural_name("Silva, João, Jr"), "João Silva, Jr");
/// assert_eq!(natural_name("Carlsen"), "Carlsen");
/// ```
pub fn natural_name(name: &str) -> String {
    let mut parts = name.split(',').map(str::trim);
    let first = parts.next().unwrap_or_default();
    let Some(given) = parts.next() else {
        return first.to_owned();
    };
    let mut out = match (first.is_empty(), given.is_empty()) {
        (_, true) => first.to_owned(),
        (true, false) => given.to_owned(),
        (false, false) => format!("{given} {first}"),
    };
    for extra in parts.filter(|p| !p.is_empty()) {
        out.push_str(", ");
        out.push_str(extra);
    }
    out
}

/// The most opponents and groups of ECO codes kept in the report: the
/// largest value of the count options.
const KEPT: usize = MAX_COUNT_OPTION as usize;

/// Makes the player report (`sc_report player create`).
///
/// Progress runs from 0 to a fixed total once, and never goes back; it ends
/// at the total. When `cancel` is set the report is what was done so far
/// (empty if the selection of the games was not finished), with
/// `core.interrupted` set.
///
/// # Errors
///
/// [`ReportError::InvalidName`] for a name without a letter or digit (empty,
/// `?`, `,`...); [`ReportError::InvalidStem`] and [`ReportError::StemMismatch`] when the
/// line does not lead to the position; [`ReportError::UnsupportedPosition`].
pub fn player_report<D: GameSource + ?Sized>(
    db: &mut D,
    req: &PlayerReportRequest<'_>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<PlayerReport, ReportError> {
    let name = req.name.trim();
    if !select::is_valid_name(name) {
        return Err(ReportError::InvalidName);
    }
    let options = req.options.normalized();
    let ropts = options.to_report_options();

    // The line to the position (the start is the standard one).
    let (stem, omitted, position, eco, turn, stem_last) = match req.start {
        ReportStart::Beginning => (Vec::new(), 0, None, None, Color::White, None),
        ReportStart::Position(position, moves) => {
            let (positions, mut sans) = walk_stem(None, moves, position)?;
            let eco = deepest_eco(req.eco_book, &positions);
            let stem_last = (position.turn() == Color::Black)
                .then(|| sans.last().map(|s| s.replace(['-', '='], "")))
                .flatten();
            let omitted = sans.len().saturating_sub(MAX_STEM_PLIES);
            let kept = sans.split_off(omitted);
            (
                kept,
                omitted,
                Some(position.clone()),
                eco,
                position.turn(),
                stem_last,
            )
        }
    };

    // Progress: the selection, the search by position and the engine, in a
    // total that is an upper bound (the engine reads at most twice the games
    // of the database).
    let games = u64::from(db.len());
    let selection_steps = if position.is_some() { 2 * games } else { games };
    let total = selection_steps.saturating_add(games.saturating_mul(2));

    let mut report = PlayerReport {
        core: ReportCore {
            database: super::model::DatabaseTotals {
                games: db.len(),
                ..super::model::DatabaseTotals::default()
            },
            interrupted: true,
            ..ReportCore::default()
        },
        name: name.to_owned(),
        color: req.color,
        matching: req.matching,
        stem,
        stem_omitted: omitted,
        position: position.clone(),
        eco,
        opponents: Vec::new(),
        eco_groups: Vec::new(),
        player_score: ScoreCounts::default(),
    };

    let filter = {
        let mut shifted = Shifted {
            inner: &mut *progress,
            offset: 0,
            total,
        };
        select::select_player_games(
            db,
            name,
            req.color,
            req.matching,
            position.as_ref(),
            &mut shifted,
            cancel,
        )?
    };
    let Some((filter, unreadable)) = filter else {
        // Cancelled while selecting: nothing to report yet.
        return Ok(report);
    };

    let queries = report_stats_queries(req.stats_config, req.today);
    let mut core = {
        let mut shifted = Shifted {
            inner: &mut *progress,
            offset: selection_steps,
            total,
        };
        build_report_core(
            db,
            &CoreInput {
                filter: &filter,
                turn,
                stem_last: stem_last.as_deref(),
                exclude: None,
                options: &ropts,
                player_stats: req.player_stats,
                stats_queries: &queries,
                today: req.today,
            },
            &mut shifted,
            cancel,
        )?
    };
    progress.progress(total, total);

    // Games left out of the selection because they could not be read (rule 11)
    // join the ones the report itself skipped.
    core.skipped = core.skipped.saturating_add(unreadable);
    report.opponents = core.top_players_scored(!req.color, req.color, KEPT);
    report.eco_groups = core.top_eco_codes_scored(KEPT, req.color);
    report.player_score = ScoreCounts::from_core(&core.total, req.color);
    report.core = core;
    Ok(report)
}

/// The report as plain text.
pub fn render_player_text(
    report: &PlayerReport,
    options: &PlayerReportOptions,
    labels: &PlayerLabels,
    part: ReportPart,
) -> String {
    render::render_player(Fmt::Text, report, options, labels, part)
}

/// The report as a complete HTML document. Every name is escaped.
pub fn render_player_html(
    report: &PlayerReport,
    options: &PlayerReportOptions,
    labels: &PlayerLabels,
    part: ReportPart,
) -> String {
    render::render_player(Fmt::Html, report, options, labels, part)
}

/// The report as a complete LaTeX document. Every name is escaped.
pub fn render_player_latex(
    report: &PlayerReport,
    options: &PlayerReportOptions,
    labels: &PlayerLabels,
    part: ReportPart,
) -> String {
    render::render_player(Fmt::Latex, report, options, labels, part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_need_a_letter_or_a_digit() {
        use super::select::is_valid_name;
        for bad in ["", "?", ",", " , ", "-.-", "  "] {
            assert!(!is_valid_name(bad), "{bad:?}");
        }
        for good in ["Alfa, A", "Álvaro", "x", "007", "?a"] {
            assert!(is_valid_name(good), "{good:?}");
        }
    }

    #[test]
    fn natural_name_handles_commas() {
        assert_eq!(natural_name("Silva, João, Jr"), "João Silva, Jr");
        assert_eq!(natural_name("Alfa, A"), "A Alfa");
        assert_eq!(natural_name("Madonna"), "Madonna");
        assert_eq!(natural_name("Silva,"), "Silva");
        assert_eq!(natural_name(", João"), "João");
        assert_eq!(natural_name(""), "");
    }

    #[test]
    fn options_are_limited_and_sections_follow() {
        let o = PlayerReportOptions {
            oldest: 99,
            max_table_games: u32::MAX,
            ..PlayerReportOptions::default()
        };
        let n = o.normalized();
        assert_eq!(n.oldest, 10);
        assert_eq!(n.max_table_games, MAX_TABLE_GAMES_LIMIT);
        let r = o.to_report_options();
        assert_eq!(r.max_games, MAX_GAMES_LIMIT);
        assert!(r.sections.contains(SectionSet::STATS));
        assert!(!r.sections.contains(SectionSet::POPULAR));
        assert!(PlayerReportOptions::default().has_moves_section());
    }

    #[test]
    fn score_counts_round_half_up() {
        let s = ScoreCounts {
            wins: 1,
            draws: 1,
            losses: 0,
            unknown: 3,
        };
        assert_eq!(s.score_tenths(), 750);
        assert_eq!(s.score_percent(), 75);
        assert_eq!(ScoreCounts::default().score_percent(), 0);
    }
}
