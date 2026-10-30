// Derived from Scid vs PC 4.27, src/tkscid.cpp (`isCrosstableGame`,
// `sc_game_crosstable`) and src/date.cpp (`date_AddMonths`). Copyright (c)
// 1999-2004 Shane Hudson; Scid vs PC contributors. Scid is distributed under
// the GNU General Public License (see its COPYING); this file is part of
// MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! Which games belong to the tournament of a reference game (spec 2.1,
//! decisions D3356 to D3358).

use std::collections::HashMap;

use mychess_core::{GameResult, PartialDate};

use super::CrosstableError;
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::{GameFlags, IndexEntry, NameKind, bytes_to_string};
use crate::source::GameSource;

/// Most games a tournament selection holds. A selection that would pass it
/// stops there and sets [`TournamentSelection::truncated`]: a "tournament"
/// of a whole database (empty Event and Site, say) must not build a table
/// of millions of games.
pub const MAX_TOURNAMENT_GAMES: usize = 100_000;

/// Months before and after the reference date that still count.
const MONTHS_AROUND: i32 = 3;

/// The game a tournament is found from: its Event and Site (compared by
/// exact name, byte for byte as the Scid does), its date and its `EventDate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentReference {
    /// The Event name.
    pub event: String,
    /// The Site name.
    pub site: String,
    /// The stored bytes of the Event name. Empty means "the UTF-8 bytes of
    /// `event`" (a reference made by hand); [`TournamentReference::from_game`]
    /// fills it so that two names that decode to the same text but differ in
    /// bytes (UTF-8 and Windows-1252) stay different names.
    pub event_raw: Vec<u8>,
    /// The stored bytes of the Site name; see `event_raw`.
    pub site_raw: Vec<u8>,
    /// The date of the game.
    pub date: PartialDate,
    /// The `EventDate` of the game ([`PartialDate::UNKNOWN`] if none).
    pub event_date: PartialDate,
}

impl TournamentReference {
    /// The reference taken from game `number` (1-based) of `db`.
    ///
    /// # Errors
    ///
    /// [`CrosstableError::InvalidGame`] if there is no such game and
    /// [`CrosstableError::Read`] if its index entry is damaged.
    pub fn from_game<D: GameSource + ?Sized>(
        db: &D,
        number: u32,
    ) -> Result<TournamentReference, CrosstableError> {
        if number == 0 || number > db.len() {
            return Err(CrosstableError::InvalidGame(number));
        }
        let entry = db.entry(number).map_err(|source| CrosstableError::Read {
            game: number,
            source,
        })?;
        let names = db.names();
        let raw = |kind, id| names.get(kind, id).map(<[u8]>::to_vec).unwrap_or_default();
        let event_raw = raw(NameKind::Event, entry.event_id);
        let site_raw = raw(NameKind::Site, entry.site_id);
        Ok(TournamentReference {
            event: bytes_to_string(&event_raw).into_owned(),
            site: bytes_to_string(&site_raw).into_owned(),
            event_raw,
            site_raw,
            date: entry.date,
            event_date: entry.event_date,
        })
    }
}

/// The games of a tournament, by number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentSelection {
    /// The reference the games were selected for.
    pub reference: TournamentReference,
    /// The numbers of the games (1-based), in increasing order.
    pub games: Vec<u32>,
    /// Entries that could not be read and were skipped.
    pub unreadable: u32,
    /// More than [`MAX_TOURNAMENT_GAMES`] games belong; `games` holds the
    /// first ones.
    pub truncated: bool,
}

/// One game of the tournament, as the table needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentGame {
    /// The number of the game in the database (1-based).
    pub number: u32,
    /// The name ID of White.
    pub white: u32,
    /// The name ID of Black.
    pub black: u32,
    /// The result.
    pub result: GameResult,
    /// The numeric round (0 when the Round tag is not a number), see
    /// [`parse_round`].
    pub round: u32,
    /// The Round tag looks like `N.M` (round and board): the mark of a
    /// knockout for the `Auto` format (D3395).
    pub sub_round: bool,
    /// The date of the game.
    pub date: PartialDate,
    /// The Elo of White in this game (0 if none).
    pub white_elo: u16,
    /// The Elo of Black in this game (0 if none).
    pub black_elo: u16,
}

/// The data a table is built from: the games and the player names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TournamentGames {
    /// The reference (gives the Event, Site and the first date seen).
    pub reference: TournamentReference,
    /// The games in the order of their numbers; the order decides the first
    /// appearance of each player.
    pub games: Vec<TournamentGame>,
    /// Player names by name ID.
    pub names: HashMap<u32, String>,
    /// Entries that could not be read and were left out (from the selection
    /// and from [`load_games`]); the table says so in a notice.
    pub unreadable: u32,
    /// The selection was cut at [`MAX_TOURNAMENT_GAMES`].
    pub truncated: bool,
}

/// The number at the start of a Round tag, as the `strtoul` of the Scid reads
/// it: leading white space and a `+` are skipped, digits are read, anything
/// else gives 0. A minus sign gives 0 (the Scid would wrap to a huge value,
/// which is never a valid round either). The flag says the number is followed
/// by `.` and a digit (`3.1`).
///
/// ```
/// use mychess_db::crosstable::parse_round;
///
/// assert_eq!(parse_round(b"3"), (3, false));
/// assert_eq!(parse_round(b"3.1"), (3, true));
/// assert_eq!(parse_round(b"?"), (0, false));
/// assert_eq!(parse_round(b" 12 "), (12, false));
/// ```
pub fn parse_round(text: &[u8]) -> (u32, bool) {
    let mut rest = text;
    while let [first, tail @ ..] = rest {
        if first.is_ascii_whitespace() {
            rest = tail;
        } else {
            break;
        }
    }
    if let [b'+', tail @ ..] = rest {
        rest = tail;
    }
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    let value = rest.iter().take(digits).fold(0u32, |acc, b| {
        acc.saturating_mul(10).saturating_add(u32::from(b - b'0'))
    });
    let after = rest.get(digits..).unwrap_or(&[]);
    let sub = digits > 0 && matches!(after, [b'.', d, ..] if d.is_ascii_digit());
    (value, sub)
}

/// `date_AddMonths` on the packed date `year << 9 | month << 5 | day`. A year
/// that would go below zero stops at zero.
fn add_months(date: PartialDate, months: i32) -> u32 {
    let mut year = i64::from(date.year().unwrap_or(0));
    let mut month = i64::from(date.month().unwrap_or(0));
    let day = i64::from(date.day().unwrap_or(0));
    let mut left = months;
    while left < 0 {
        if month <= 1 {
            year -= 1;
            month = 12;
        } else {
            month -= 1;
        }
        left += 1;
    }
    while left > 0 {
        month += 1;
        if month > 12 {
            year += 1;
            month = 1;
        }
        left -= 1;
    }
    let packed = year.max(0) << 9 | month << 5 | day;
    u32::try_from(packed).unwrap_or(u32::MAX)
}

/// The date test of `isCrosstableGame`, with the year-only rule of D3356.
struct DateWindow {
    event_date: PartialDate,
    year: Option<u16>,
    year_only: bool,
    first: u32,
    last: u32,
}

impl DateWindow {
    fn new(reference: &TournamentReference) -> DateWindow {
        let date = reference.date;
        DateWindow {
            event_date: reference.event_date,
            year: date.year(),
            year_only: date.month().is_none() && date.day().is_none(),
            first: add_months(date, -MONTHS_AROUND),
            last: add_months(date, MONTHS_AROUND),
        }
    }

    fn accepts(&self, date: PartialDate, event_date: PartialDate) -> bool {
        if !self.event_date.is_unknown() && event_date == self.event_date {
            return true;
        }
        if self.year_only || (date.month().is_none() && date.day().is_none()) {
            return date.year() == self.year;
        }
        let packed = date.to_scid_u32();
        packed >= self.first && packed <= self.last
    }
}

/// The IDs of the names of `kind` whose bytes are exactly `wanted`.
fn name_ids<D: GameSource + ?Sized>(db: &D, kind: NameKind, wanted: &[u8]) -> Vec<u32> {
    let names = db.names();
    (0..names.num_names(kind))
        .filter(|&id| names.get(kind, id) == Some(wanted))
        .collect()
}

/// The bytes a name is looked up by: the stored ones, or the UTF-8 of the
/// text for a reference made by hand.
fn wanted<'a>(raw: &'a [u8], text: &'a str) -> &'a [u8] {
    if raw.is_empty() { text.as_bytes() } else { raw }
}

/// Selects the games of the tournament of `reference` out of the whole
/// database; the filter is not used.
///
/// A game belongs when its Event and Site are the reference's (exact names)
/// and its `EventDate` is the reference's (when the reference has one) or its
/// date is within three months of the reference's (compared with month and
/// day, so `2020.05.15` reaches `2020.02.15` to `2020.08.15`); with a
/// reference date of only a year, any game of that year. A game with unknown
/// month and day belongs when its year is the reference's. Games with the
/// same player on both sides are ignored, and so are games marked for
/// deletion unless `include_deleted`. A reference whose Event or Site is not
/// in the name base gives no games.
///
/// At most [`MAX_TOURNAMENT_GAMES`] games are taken; past that the scan stops
/// and [`TournamentSelection::truncated`] is set.
///
/// Progress is reported every 5000 games or 100 ms, where `cancel` is looked
/// at too.
///
/// # Errors
///
/// [`CrosstableError::Cancelled`] when `cancel` is set.
pub fn select_tournament<D: GameSource + ?Sized>(
    db: &D,
    reference: &TournamentReference,
    include_deleted: bool,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<TournamentSelection, CrosstableError> {
    let mut selection = TournamentSelection {
        reference: reference.clone(),
        games: Vec::new(),
        unreadable: 0,
        truncated: false,
    };
    let events = name_ids(
        db,
        NameKind::Event,
        wanted(&reference.event_raw, &reference.event),
    );
    let sites = name_ids(
        db,
        NameKind::Site,
        wanted(&reference.site_raw, &reference.site),
    );
    if events.is_empty() || sites.is_empty() {
        return Ok(selection);
    }
    let window = DateWindow::new(reference);
    let total = db.len();
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=total {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(total));
            if cancel.is_cancelled() {
                return Err(CrosstableError::Cancelled);
            }
        }
        let Ok(entry) = db.entry(n) else {
            selection.unreadable = selection.unreadable.saturating_add(1);
            continue;
        };
        if belongs(&entry, &events, &sites, &window, include_deleted) {
            if selection.games.len() >= MAX_TOURNAMENT_GAMES {
                selection.truncated = true;
                break;
            }
            selection.games.push(n);
        }
    }
    progress.progress(u64::from(total), u64::from(total));
    Ok(selection)
}

fn belongs(
    entry: &IndexEntry,
    events: &[u32],
    sites: &[u32],
    window: &DateWindow,
    include_deleted: bool,
) -> bool {
    if entry.flags.contains(GameFlags::DELETE) && !include_deleted {
        return false;
    }
    events.contains(&entry.event_id)
        && sites.contains(&entry.site_id)
        && window.accepts(entry.date, entry.event_date)
        && entry.white_id != entry.black_id
}

/// Reads the index entries of the selected games into [`TournamentGames`].
/// A game whose entry cannot be read is left out and counted in
/// [`TournamentGames::unreadable`].
pub fn load_games<D: GameSource + ?Sized>(
    db: &D,
    selection: &TournamentSelection,
) -> TournamentGames {
    match load_games_cancellable(db, selection, &CancelToken::new()) {
        Ok(games) => games,
        // A token nobody else holds is never set.
        Err(_) => TournamentGames {
            reference: selection.reference.clone(),
            games: Vec::new(),
            names: HashMap::new(),
            unreadable: selection.unreadable,
            truncated: selection.truncated,
        },
    }
}

/// [`load_games`] that looks at `cancel` every few thousand games.
///
/// # Errors
///
/// [`CrosstableError::Cancelled`] when `cancel` is set.
pub fn load_games_cancellable<D: GameSource + ?Sized>(
    db: &D,
    selection: &TournamentSelection,
    cancel: &CancelToken,
) -> Result<TournamentGames, CrosstableError> {
    let names = db.names();
    let mut players: HashMap<u32, String> = HashMap::new();
    let mut games = Vec::with_capacity(selection.games.len().min(MAX_TOURNAMENT_GAMES));
    let mut unreadable = selection.unreadable;
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for &number in &selection.games {
        if ticker.tick() && cancel.is_cancelled() {
            return Err(CrosstableError::Cancelled);
        }
        let Ok(entry) = db.entry(number) else {
            unreadable = unreadable.saturating_add(1);
            continue;
        };
        for id in [entry.white_id, entry.black_id] {
            players.entry(id).or_insert_with(|| {
                names
                    .get(NameKind::Player, id)
                    .map(|b| bytes_to_string(b).into_owned())
                    .unwrap_or_default()
            });
        }
        let (round, sub_round) = names
            .get(NameKind::Round, entry.round_id)
            .map_or((0, false), parse_round);
        games.push(TournamentGame {
            number,
            white: entry.white_id,
            black: entry.black_id,
            result: entry.result,
            round,
            sub_round,
            date: entry.date,
            white_elo: entry.white_elo,
            black_elo: entry.black_elo,
        });
    }
    Ok(TournamentGames {
        reference: selection.reference.clone(),
        games,
        names: players,
        unreadable,
        truncated: selection.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(text: &str) -> PartialDate {
        PartialDate::parse_lenient(text)
    }

    fn reference(date: &str, event_date: &str) -> TournamentReference {
        TournamentReference {
            event: "E".into(),
            site: "S".into(),
            date: d(date),
            event_date: d(event_date),
            event_raw: Vec::new(),
            site_raw: Vec::new(),
        }
    }

    #[test]
    fn round_numbers_follow_strtoul() {
        assert_eq!(parse_round(b"3"), (3, false));
        assert_eq!(parse_round(b"3.1"), (3, true));
        assert_eq!(parse_round(b"3."), (3, false));
        assert_eq!(parse_round(b"-3"), (0, false));
        assert_eq!(parse_round(b"?"), (0, false));
        assert_eq!(parse_round(b""), (0, false));
        assert_eq!(parse_round(b"+7"), (7, false));
        assert_eq!(parse_round(b"99999999999999"), (u32::MAX, false));
    }

    #[test]
    fn add_months_crosses_years_and_keeps_the_day() {
        assert_eq!(
            add_months(d("2020.01.15"), -3),
            (2019 << 9) | (10 << 5) | 15
        );
        assert_eq!(add_months(d("2020.11.15"), 3), (2021 << 9) | (2 << 5) | 15);
        assert_eq!(add_months(d("2020.05.15"), 3), (2020 << 9) | (8 << 5) | 15);
    }

    #[test]
    fn window_is_three_months_either_side_with_the_day_kept() {
        let w = DateWindow::new(&reference("2020.05.15", "????.??.??"));
        assert!(w.accepts(d("2020.02.15"), PartialDate::UNKNOWN));
        assert!(w.accepts(d("2020.08.15"), PartialDate::UNKNOWN));
        assert!(!w.accepts(d("2020.02.14"), PartialDate::UNKNOWN));
        assert!(!w.accepts(d("2020.08.16"), PartialDate::UNKNOWN));
    }

    #[test]
    fn year_only_reference_takes_the_whole_year_including_december() {
        let w = DateWindow::new(&reference("2020.??.??", "????.??.??"));
        assert!(w.accepts(d("2020.12.31"), PartialDate::UNKNOWN));
        assert!(w.accepts(d("2020.01.01"), PartialDate::UNKNOWN));
        assert!(w.accepts(d("2020.??.??"), PartialDate::UNKNOWN));
        assert!(!w.accepts(d("2021.01.01"), PartialDate::UNKNOWN));
    }

    #[test]
    fn candidate_with_unknown_month_and_day_matches_by_year() {
        let w = DateWindow::new(&reference("2020.05.15", "????.??.??"));
        assert!(w.accepts(d("2020.??.??"), PartialDate::UNKNOWN));
        assert!(!w.accepts(d("2019.??.??"), PartialDate::UNKNOWN));
    }

    #[test]
    fn event_date_matches_regardless_of_the_game_date() {
        let w = DateWindow::new(&reference("2020.05.15", "2020.05.01"));
        assert!(w.accepts(d("1999.01.01"), d("2020.05.01")));
        assert!(!w.accepts(d("1999.01.01"), d("2020.05.02")));
        // An unknown reference EventDate never matches by itself.
        let w = DateWindow::new(&reference("2020.05.15", "????.??.??"));
        assert!(!w.accepts(d("1999.01.01"), PartialDate::UNKNOWN));
    }
}
