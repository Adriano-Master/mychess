// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_name_info` with
// `-ratings:YEAR -elo:ELO`) and tcl/tools/graphs.tcl (the rating graph).
// Copyright (c) 1999-2004 Shane Hudson; Scid vs PC contributors. Scid is
// distributed under the GNU General Public License (see its COPYING); this
// file is part of MyChess, GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The rating history of a player (milestone M6, slice 4b; spec 2.4, rules R1
//! to R4 and decision D3375).
//!
//! For each month, the rating the player had in the **last game of the month**
//! (the highest game number) in which a rating was recorded. The rating comes
//! from the games of the database (`WhiteElo` / `BlackElo`), not from the
//! spelling file (rule R5: that source waits for the spelling item).
//!
//! ## Differences from Scid
//!
//! - A game with rating 0 (none) is ignored. Scid, with the minimum rating
//!   at 0, lets a later game without a rating erase the rating of the month
//!   (D3375).
//! - Several players are computed in one pass ([`rating_histories`]), at most
//!   [`MAX_RATING_PLAYERS`] (the nine colours of the graph).
//!
//! A game with an unknown month counts as January; a game with an unknown
//! year, or earlier than [`RatingOptions::start_year`], is dropped.

use std::collections::{BTreeMap, HashMap};

use crate::filter::Filter;
use crate::filter_stats::{StatsError, check_filter, fold_index};
use crate::progress::{CancelToken, ProgressSink};
use crate::si4::{NameKind, bytes_to_string};
use crate::source::GameSource;

/// Most players a rating graph draws (rule R4, one per colour).
pub const MAX_RATING_PLAYERS: usize = 9;

/// Who the history is of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RatingPlayer {
    /// A player name ID of the database.
    Id(u32),
    /// A name, compared with the player names of the database as text. A
    /// name that is not in the database has no games.
    Name(String),
}

/// The options of the rating graph (the menus "Start year" and "Start
/// rating").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatingOptions {
    /// Games before this year are dropped (Scid default 1900).
    pub start_year: u16,
    /// Ratings below this value are dropped (Scid default 0).
    pub start_elo: u16,
    /// Count only the games in the filter.
    pub only_filter: bool,
}

impl Default for RatingOptions {
    fn default() -> RatingOptions {
        RatingOptions {
            start_year: 1900,
            start_elo: 0,
            only_filter: false,
        }
    }
}

/// The points of one player, by month, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RatingSeries {
    /// `(year, month 1 to 12, rating)`.
    pub points: Vec<(u16, u8, u16)>,
    /// Index entries of the database that could not be read and so were
    /// left out (the same number in every series of one call; D3443).
    pub skipped: u32,
}

/// The x coordinate of a point (rule R3): `year + trunc((month-1)*100/12) /
/// 100`, so January is `.00`, April `.25`, July `.50`, December `.91`.
pub fn rating_x(year: u16, month: u8) -> f32 {
    let month = u32::from(month.clamp(1, 12));
    let hundredths = (month - 1) * 100 / 12;
    f32::from(year) + f32::from(u16::try_from(hundredths).unwrap_or(0)) / 100.0
}

impl RatingSeries {
    /// The points as `(x, rating)` for drawing.
    pub fn xy(&self) -> Vec<(f32, u16)> {
        self.points
            .iter()
            .map(|&(year, month, elo)| (rating_x(year, month), elo))
            .collect()
    }

    /// Whether there is no point.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// The player name IDs whose name is `name` (as text).
fn ids_of_name<D: GameSource + ?Sized>(db: &D, name: &str) -> Vec<u32> {
    let names = db.names();
    (0..names.num_names(NameKind::Player))
        .filter(|&id| {
            names
                .get(NameKind::Player, id)
                .is_some_and(|raw| bytes_to_string(raw) == name)
        })
        .collect()
}

/// Per player: `(year, month)` to the `(rating)` of the last game.
type Months = BTreeMap<(u16, u8), u16>;

/// The history of every player of `players` (the first
/// [`MAX_RATING_PLAYERS`]), in one pass over the index. `result[i]` is the
/// series of `players[i]`; a player without games has an empty series.
///
/// `filter` is looked at only when `options.only_filter` is set.
///
/// # Errors
///
/// [`StatsError::FilterSize`] if the filter is used and does not cover `db`;
/// [`StatsError::Cancelled`] if `cancel` is set.
pub fn rating_histories<D: GameSource + ?Sized>(
    db: &D,
    players: &[RatingPlayer],
    options: RatingOptions,
    filter: Option<&Filter>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Vec<RatingSeries>, StatsError> {
    let players = players.get(..MAX_RATING_PLAYERS).unwrap_or(players);
    let filter = if options.only_filter { filter } else { None };
    if let Some(filter) = filter {
        check_filter(db, filter)?;
    }
    // name ID -> index of the player that asked for it.
    let mut slot_of: HashMap<u32, usize> = HashMap::new();
    for (slot, player) in players.iter().enumerate() {
        let ids = match player {
            RatingPlayer::Id(id) => vec![*id],
            RatingPlayer::Name(name) => ids_of_name(db, name),
        };
        for id in ids {
            slot_of.entry(id).or_insert(slot);
        }
    }
    let count = players.len();
    let (months, unreadable) = fold_index(
        db,
        progress,
        cancel,
        || vec![Months::new(); count],
        |months: &mut Vec<Months>, n, entry| {
            if filter.is_some_and(|f| !f.contains(n)) {
                return;
            }
            // Scid: white first, then black ("else if"): a player that is
            // on both sides counts as White. Two requested players in the
            // same game each get their side.
            let white = slot_of.get(&entry.white_id).copied();
            let black = if entry.black_id == entry.white_id {
                None
            } else {
                slot_of.get(&entry.black_id).copied()
            };
            if let Some(slot) = white {
                record(months, slot, entry.white_elo, entry, options);
            }
            if let Some(slot) = black.filter(|&s| Some(s) != white) {
                record(months, slot, entry.black_elo, entry, options);
            }
        },
        |total, part| {
            for (sum, other) in total.iter_mut().zip(part) {
                // Later blocks hold later games: they overwrite.
                sum.extend(other);
            }
        },
    )?;
    Ok(months
        .into_iter()
        .map(|m| RatingSeries {
            points: m
                .into_iter()
                .map(|((year, month), elo)| (year, month, elo))
                .collect(),
            skipped: unreadable,
        })
        .collect())
}

fn record(
    months: &mut [Months],
    slot: usize,
    elo: u16,
    entry: &crate::si4::IndexEntry,
    options: RatingOptions,
) {
    if elo == 0 || elo < options.start_elo {
        return;
    }
    let Some(year) = entry
        .date
        .year()
        .filter(|&y| y != 0 && y >= options.start_year)
    else {
        return;
    };
    let month = entry
        .date
        .month()
        .filter(|m| (1..=12).contains(m))
        .unwrap_or(1);
    if let Some(map) = months.get_mut(slot) {
        map.insert((year, month), elo);
    }
}

/// The history of one player (see the module doc).
///
/// # Errors
///
/// Same as [`rating_histories`].
pub fn rating_history<D: GameSource + ?Sized>(
    db: &D,
    player: &RatingPlayer,
    options: RatingOptions,
    filter: Option<&Filter>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<RatingSeries, StatsError> {
    Ok(rating_histories(
        db,
        std::slice::from_ref(player),
        options,
        filter,
        progress,
        cancel,
    )?
    .into_iter()
    .next()
    .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_coordinate_is_the_hundredths_of_the_year() {
        assert_eq!(rating_x(2000, 1), 2000.0);
        assert_eq!(rating_x(2000, 2), 2000.08);
        assert_eq!(rating_x(2000, 3), 2000.16);
        assert_eq!(rating_x(2000, 4), 2000.25);
        assert_eq!(rating_x(2000, 7), 2000.5);
        assert_eq!(rating_x(2000, 10), 2000.75);
        assert_eq!(rating_x(2000, 12), 2000.91);
        assert_eq!(rating_x(2000, 0), 2000.0);
    }

    #[test]
    fn the_series_gives_xy_pairs() {
        let series = RatingSeries {
            points: vec![(2001, 4, 2500), (2002, 12, 2510)],
            skipped: 0,
        };
        assert_eq!(series.xy(), [(2001.25, 2500), (2002.91, 2510)]);
        assert!(!series.is_empty());
    }
}
