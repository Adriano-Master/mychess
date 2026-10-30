// Derived from Scid vs PC 4.27, src/spellchk.cpp (`AddEloData`, `SetElo`,
// `GetElo`, `GetAllElo`) and src/tkscid.cpp (`sc_name_ratings`). Copyright
// (c) 1999-2003 Shane Hudson; Scid vs PC contributors. Scid is distributed
// under the GNU General Public License (see its COPYING); this file is part
// of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Elo by period from the `%Elo` lines of the spelling file, and "Add Elo"
//! (rules 60 to 74; decisions D3308 and D3330).
//!
//! A line is stored as text and interpreted on demand ([`EloTable`]). Each
//! year has up to 12 values; which month uses which value depends on the
//! year, following the release schedule of FIDE ([`period_position`]).

use std::collections::HashMap;

use mychess_core::{MAX_ELO, PartialDate};
use thiserror::Error;

use super::SpellChecker;
use crate::filter::Filter;
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, Ticker};
use crate::si4::{NameKind, Si4Error, Si4Writer, SideRating, bytes_to_string};
use crate::source::GameSource;

/// The first year with Elo data.
pub const ELO_FIRST_YEAR: u16 = 1970;

/// Values kept per year; the rest of a line is dropped.
pub const ELO_PER_YEAR: usize = 12;

/// Games between two progress reports of [`plan_add_elo`] (Scid: 1000).
const PLAN_CHECK_EVERY: u32 = 1000;

/// Changes written per `fsync` by [`apply_add_elo`].
const APPLY_CHUNK: usize = 1000;

/// Month (0 = unknown, 1..=12) to position, per period (rule 61).
const MONTHLY: [usize; 13] = [0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const NINE: [usize; 13] = [0, 0, 0, 1, 1, 2, 2, 3, 4, 5, 6, 7, 8];
const BIMONTHLY: [usize; 13] = [0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5];
const TRANSITION: [usize; 13] = [0, 0, 0, 0, 1, 1, 1, 2, 2, 3, 3, 4, 4];
const QUARTERLY: [usize; 13] = [0, 0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3];
const SEMESTER: [usize; 13] = [0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1];

/// The number of positions of the lists of `year` (12, 9, 6, 5, 4 or 2).
pub fn positions_in_year(year: u16) -> u32 {
    match year {
        2013.. => 12,
        2012 => 9,
        2010..=2011 => 6,
        2009 => 5,
        2001..=2008 => 4,
        _ => 2,
    }
}

/// The position of the list of `year` that applies to `month` (0 =
/// unknown, 1 to 12; larger values are read as January, rule 62).
///
/// ```
/// use mychess_db::spelling::elo::period_position;
///
/// assert_eq!(period_position(2013, 3), 2);
/// assert_eq!(period_position(2012, 12), 8);
/// assert_eq!(period_position(1999, 7), 1);
/// assert_eq!(period_position(2013, 0), 0);
/// ```
pub fn period_position(year: u16, month: u8) -> usize {
    let month = if month > 12 { 1 } else { usize::from(month) };
    let table = match year {
        2013.. => &MONTHLY,
        2012 => &NINE,
        2010..=2011 => &BIMONTHLY,
        2009 => &TRANSITION,
        2001..=2008 => &QUARTERLY,
        _ => &SEMESTER,
    };
    table.get(month).copied().unwrap_or(0)
}

/// The Elo lists of one player: for each year from [`ELO_FIRST_YEAR`] with
/// data, [`ELO_PER_YEAR`] values (0 = none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EloTable {
    years: Vec<(u16, [u16; ELO_PER_YEAR])>,
}

impl EloTable {
    /// Interprets `%Elo` lines (the text after `%Elo `), in order: a later
    /// line only overwrites the positions it defines (rule 73). Years
    /// outside `[1970, last_year]` are dropped; a character that does not
    /// belong to the grammar stops the reading of that line, keeping what
    /// was read (rule 60).
    pub fn parse<'a>(lines: impl IntoIterator<Item = &'a str>, last_year: u16) -> EloTable {
        let mut table = EloTable::default();
        for line in lines {
            table.add_line(line, last_year);
        }
        table.years.sort_unstable_by_key(|&(year, _)| year);
        table
    }

    fn set(&mut self, year: u16, position: usize, value: u16) {
        if position >= ELO_PER_YEAR {
            return;
        }
        let at = if let Some(at) = self.years.iter().position(|&(y, _)| y == year) {
            at
        } else {
            self.years.push((year, [0; ELO_PER_YEAR]));
            self.years.len() - 1
        };
        if let Some(slot) = self
            .years
            .get_mut(at)
            .and_then(|(_, values)| values.get_mut(position))
        {
            *slot = value;
        }
    }

    fn add_line(&mut self, line: &str, last_year: u16) {
        let b = line.as_bytes();
        let mut pos = 0usize;
        let digits = |from: usize| {
            b.get(from..)
                .map_or(0, |s| s.iter().take_while(|c| c.is_ascii_digit()).count())
        };
        let number = |from: usize, len: usize| -> Option<u16> {
            let text = line.get(from..from + len)?;
            if len == 0 || len > 4 {
                return None;
            }
            text.parse::<u16>().ok()
        };
        loop {
            while b
                .get(pos)
                .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
            {
                pos += 1;
            }
            let n = digits(pos);
            let Some(year) = number(pos, n) else {
                return;
            };
            pos += n;
            if b.get(pos) != Some(&b':') {
                return;
            }
            pos += 1;
            let mut index = 0usize;
            loop {
                let value = match b.get(pos) {
                    Some(c) if c.is_ascii_digit() => {
                        let n = digits(pos);
                        let Some(v) = number(pos, n) else {
                            return;
                        };
                        pos += n;
                        v
                    }
                    Some(b'?') => {
                        pos += 1;
                        0
                    }
                    Some(b' ') => break,
                    // Invalid data, or the end of the line.
                    _ => return,
                };
                if (ELO_FIRST_YEAR..=last_year).contains(&year) {
                    self.set(year, index, value);
                }
                index += 1;
                if b.get(pos) == Some(&b',') {
                    pos += 1;
                }
            }
        }
    }

    /// The values of `year`, if the table has any line for it.
    pub fn year(&self, year: u16) -> Option<&[u16; ELO_PER_YEAR]> {
        self.years
            .iter()
            .find(|&&(y, _)| y == year)
            .map(|(_, values)| values)
    }

    /// The years with data and their values, in year order.
    pub fn years(&self) -> impl Iterator<Item = (u16, &[u16; ELO_PER_YEAR])> {
        self.years.iter().map(|(y, v)| (*y, v))
    }

    /// The rating for a game on `date` (rule 62).
    ///
    /// The position of the period is tried first, then the first position of
    /// the year; if the month is unknown and `exact` is false, the first
    /// non-zero position of the year.
    pub fn at(&self, date: PartialDate, exact: bool, last_year: u16) -> Option<u16> {
        let year = date.year()?;
        if !(ELO_FIRST_YEAR..=last_year).contains(&year) {
            return None;
        }
        let month = date.month().unwrap_or(0);
        let values = self.year(year)?;
        let position = period_position(year, month);
        let mut elo = values.get(position).copied().unwrap_or(0);
        if elo == 0 {
            elo = values[0];
        }
        if elo == 0 && month == 0 && !exact {
            elo = values.iter().copied().find(|&v| v != 0).unwrap_or(0);
        }
        (elo != 0).then_some(elo)
    }
}

impl SpellChecker {
    /// The Elo table of the correct name `name` (exact; aliases have none),
    /// interpreted from its `%Elo` lines. `None` if it has no `%Elo` line.
    pub fn elo_table(&self, name: &str) -> Option<EloTable> {
        let index = *self.exact.get(name)?;
        let lines = self.side.elo.get(&index)?;
        Some(EloTable::parse(
            lines.iter().map(AsRef::as_ref),
            self.last_year,
        ))
    }

    /// The rating of the player `name` for a game on `date` (rule 62).
    /// `exact` disables the search of the other positions of the year when
    /// the month is unknown.
    ///
    /// ```
    /// use mychess_core::PartialDate;
    /// use mychess_db::si4::NameKind;
    /// use mychess_db::spelling::{ReadOptions, SpellingSet};
    ///
    /// let opts = ReadOptions { current_year: Some(2020), ..ReadOptions::default() };
    /// let set = SpellingSet::read_bytes(
    ///     b"@PLAYER\nAlfa, A\n%Elo 2013:2300,2301,2305\n",
    ///     &opts,
    /// )?;
    /// let date = PartialDate::parse_lenient("2013.03.10");
    /// assert_eq!(set.checker(NameKind::Player).elo_at("Alfa, A", date, false), Some(2305));
    /// # Ok::<(), mychess_db::spelling::SpellingError>(())
    /// ```
    pub fn elo_at(&self, name: &str, date: PartialDate, exact: bool) -> Option<u16> {
        self.elo_table(name)?.at(date, exact, self.last_year)
    }

    /// The Elo of a player over time, for a graph (rule 64): pairs
    /// `(year + position * 100 / n / 100, Elo)` for every non-zero position
    /// with Elo at least `min_elo` and year at least `start_year`, where `n`
    /// is the number of positions of the year. The name is first corrected
    /// with [`SpellChecker::correct`].
    pub fn elo_history(&self, name: &str, start_year: u16, min_elo: u16) -> Vec<(f32, u16)> {
        let Some(correct) = self.correct(name) else {
            return Vec::new();
        };
        let Some(table) = self.elo_table(correct) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (year, values) in table.years() {
            if year < start_year {
                continue;
            }
            let n = positions_in_year(year);
            for (position, &elo) in values.iter().enumerate() {
                if elo == 0 || elo < min_elo {
                    continue;
                }
                let x = u32::try_from(position).unwrap_or(0) * 100 / n;
                let text = format!("{year}.{x:02}");
                out.push((text.parse::<f32>().unwrap_or(f32::from(year)), elo));
            }
        }
        out
    }
}

/// The options of "Add Elo" (rule 66).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EloOptions {
    /// Replace ratings that are already there (default: no).
    pub overwrite: bool,
    /// Only the games of the filter (default: yes).
    pub only_filter: bool,
    /// Also games without a month (default: yes).
    pub no_month: bool,
    /// Write the changes; false only counts (default: yes).
    pub update: bool,
}

impl Default for EloOptions {
    fn default() -> EloOptions {
        EloOptions {
            overwrite: false,
            only_filter: true,
            no_month: true,
            update: true,
        }
    }
}

/// What "Add Elo" did or would do (rule 70).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EloReport {
    /// Ratings that change (value or type).
    pub ratings: u32,
    /// Games with at least one change.
    pub games: u32,
    /// The run was cancelled; the counts are partial.
    pub interrupted: bool,
}

/// The new ratings of one game: `(Elo, rating type)` per side; the type is 0
/// (`Elo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EloChange {
    /// The game number (1-based).
    pub game: u32,
    /// The new White rating.
    pub white: Option<(u16, u8)>,
    /// The new Black rating.
    pub black: Option<(u16, u8)>,
}

/// The result of [`plan_add_elo`]: the writes to do and the counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EloPlan {
    /// The changes, in game order.
    pub changes: Vec<EloChange>,
    /// The counts.
    pub report: EloReport,
    /// Whether the options asked to write.
    pub update: bool,
}

/// Why "Add Elo" cannot run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AddEloError {
    /// The checker is not the one for players.
    #[error("the spelling checker is not for player names")]
    NotPlayers,
    /// The spelling file has no `%Elo` lines (the interface says
    /// `spell-no-elo`).
    #[error("the spelling file has no Elo ratings")]
    NoEloData,
    /// The filter does not cover the database.
    #[error("the filter has {filter} games but the database has {games}")]
    FilterSize {
        /// Length of the filter.
        filter: u32,
        /// Number of games.
        games: u32,
    },
}

/// Works out the ratings "Add Elo" would add (rules 65 to 69), without
/// writing. Reads the index only. Cancelling returns what was found so far
/// with `report.interrupted` set. Only real changes are counted and
/// listed (D3330).
///
/// # Errors
///
/// [`AddEloError`] when the checker is not for players, has no `%Elo`
/// data, or the filter has the wrong size.
pub fn plan_add_elo<D: GameSource + ?Sized>(
    db: &D,
    checker: &SpellChecker,
    filter: Option<&Filter>,
    opts: EloOptions,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> Result<EloPlan, AddEloError> {
    if checker.kind() != NameKind::Player {
        return Err(AddEloError::NotPlayers);
    }
    if !checker.has_elo_data() {
        return Err(AddEloError::NoEloData);
    }
    let games = db.len();
    if let Some(f) = filter
        && opts.only_filter
        && f.len() != games
    {
        return Err(AddEloError::FilterSize {
            filter: f.len(),
            games,
        });
    }
    let filter = if opts.only_filter { filter } else { None };
    let mut tables: HashMap<u32, Option<EloTable>> = HashMap::new();
    let mut plan = EloPlan {
        changes: Vec::new(),
        report: EloReport::default(),
        update: opts.update,
    };
    let mut ticker = Ticker::new(PLAN_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                plan.report.interrupted = true;
                return Ok(plan);
            }
        }
        if filter.is_some_and(|f| !f.contains(n)) {
            continue;
        }
        let Ok(entry) = db.entry(n) else {
            continue;
        };
        if entry.date.month().is_none() && !opts.no_month {
            continue;
        }
        let mut change = EloChange {
            game: n,
            white: None,
            black: None,
        };
        for (id, old, old_type, slot) in [
            (
                entry.white_id,
                entry.white_elo,
                entry.white_rating_type,
                &mut change.white,
            ),
            (
                entry.black_id,
                entry.black_elo,
                entry.black_rating_type,
                &mut change.black,
            ),
        ] {
            if !opts.overwrite && old != 0 {
                continue;
            }
            let exact = opts.overwrite && old != 0;
            let table = tables.entry(id).or_insert_with(|| {
                let name = db.names().get(NameKind::Player, id).map(bytes_to_string)?;
                checker.elo_table(&name)
            });
            let Some(elo) = table
                .as_ref()
                .and_then(|t| t.at(entry.date, exact, checker.last_year()))
            else {
                continue;
            };
            let elo = elo.min(MAX_ELO);
            if elo != old || old_type != 0 {
                *slot = Some((elo, 0));
                plan.report.ratings = plan.report.ratings.saturating_add(1);
            }
        }
        if change.white.is_some() || change.black.is_some() {
            plan.report.games = plan.report.games.saturating_add(1);
            plan.changes.push(change);
        }
    }
    progress.progress(u64::from(games), u64::from(games));
    Ok(plan)
}

/// Writes a plan with [`Si4Writer::set_elo_batch`], in chunks, checking
/// `cancel` between chunks (what is written stays valid). Does nothing if the
/// plan was made with `update` off. Returns the counts of what was written
/// and the first error, if any.
pub fn apply_add_elo(
    writer: &mut Si4Writer,
    plan: &EloPlan,
    cancel: &CancelToken,
    progress: &mut dyn ProgressSink,
) -> (EloReport, Result<(), Si4Error>) {
    let mut report = EloReport {
        interrupted: plan.report.interrupted,
        ..EloReport::default()
    };
    if !plan.update {
        return (plan.report, Ok(()));
    }
    let total = u64::try_from(plan.changes.len()).unwrap_or(u64::MAX);
    let mut done = 0u64;
    for chunk in plan.changes.chunks(APPLY_CHUNK) {
        if cancel.is_cancelled() {
            report.interrupted = true;
            break;
        }
        let batch: Vec<(u32, SideRating, SideRating)> =
            chunk.iter().map(|c| (c.game, c.white, c.black)).collect();
        let (_, result) = writer.set_elo_batch(&batch);
        if let Err(error) = result {
            return (report, Err(error));
        }
        for c in chunk {
            let sides = u32::from(c.white.is_some()) + u32::from(c.black.is_some());
            report.ratings = report.ratings.saturating_add(sides);
            report.games = report.games.saturating_add(1);
        }
        done += u64::try_from(chunk.len()).unwrap_or(0);
        progress.progress(done, total);
    }
    (report, Ok(()))
}
