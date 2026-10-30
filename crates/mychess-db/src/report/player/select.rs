// Scid: src/tkscid.cpp `sc_report_create` 14514-14611 and the header search
// that tcl/tools/preport.tcl `makeReportWin` runs (`sc_search header -white
// <name>`, a case-insensitive substring), src/index.h/namebase.h (the IDs of
// the names). Copyright (c) 1999-2003 Shane Hudson; Scid vs PC contributors.
// Scid is distributed under the GNU General Public License (see its
// COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The games of a player with one colour (decision M7-3/D3651: by name ID by
//! default, with the "starts with" and "contains" modes of the Scid), and the
//! optional restriction to the games that reach a position (M7-3/D3654).

use mychess_core::{Color, Position};

use super::NameMatch;
use crate::filter::{Filter, FilterOp};
use crate::finder::{find_name_id, fold};
use crate::progress::{
    CHECK_INTERVAL, CancelToken, NoProgress, ProgressSink, SEARCH_CHECK_EVERY, Ticker,
};
use crate::report::ReportError;
use crate::search::{BoardSearch, SearchError, search_board};
use crate::si4::{NameBase, NameKind, bytes_to_string};
use crate::source::GameSource;

/// The IDs of the names of the database that `name` selects, as a table
/// indexed by ID. `None` when no name matches.
fn matching_ids(names: &NameBase, name: &str, matching: NameMatch) -> Option<Vec<bool>> {
    let count = names.num_names(NameKind::Player);
    let mut ids = vec![false; usize::try_from(count).unwrap_or(0)];
    let mut any = false;
    match matching {
        NameMatch::Exact => {
            if let Some(slot) = find_name_id(names, NameKind::Player, name)
                .and_then(|id| ids.get_mut(usize::try_from(id).ok()?))
            {
                *slot = true;
                any = true;
            }
        }
        NameMatch::Prefix | NameMatch::Contains => {
            let wanted = fold(name);
            for (id, slot) in (0..count).zip(ids.iter_mut()) {
                let Some(raw) = names.get(NameKind::Player, id) else {
                    continue;
                };
                let have = fold(&bytes_to_string(raw));
                let hit = if matching == NameMatch::Prefix {
                    have.starts_with(&wanted)
                } else {
                    have.contains(&wanted)
                };
                if hit {
                    *slot = true;
                    any = true;
                }
            }
        }
    }
    any.then_some(ids)
}

/// Whether `name` can be the player of a report: after the `trim` the caller
/// does, it has at least one letter or digit (so `""`, `"?"` and `","` are
/// refused). Note that the `trim` also removes the spaces the user typed on
/// purpose: `"Smith "` selects the same names as `"Smith"` in the "starts
/// with" mode, so it does not keep `"Smithson"` out.
pub(super) fn is_valid_name(name: &str) -> bool {
    name.chars().any(char::is_alphanumeric)
}

/// The games of the player with `color`, every one with the filter value 1
/// (ply 0, the beginning of the game), and how many entries could not be read
/// (those games are left out). Cancelling returns `None`.
fn scan<D: GameSource + ?Sized>(
    db: &D,
    name: &str,
    color: Color,
    matching: NameMatch,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Option<(Filter, u32)> {
    let games = db.len();
    let mut filter = Filter::new(games);
    filter.fill(0);
    if cancel.is_cancelled() {
        return None;
    }
    let Some(ids) = matching_ids(db.names(), name, matching) else {
        progress.progress(u64::from(games), u64::from(games));
        return Some((filter, 0));
    };
    let mut unreadable = 0u32;
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=games {
        if ticker.tick() {
            progress.progress(u64::from(n - 1), u64::from(games));
            if cancel.is_cancelled() {
                return None;
            }
        }
        let Ok(entry) = db.entry(n) else {
            unreadable = unreadable.saturating_add(1);
            continue;
        };
        let id = match color {
            Color::White => entry.white_id,
            Color::Black => entry.black_id,
        };
        let selected = usize::try_from(id)
            .ok()
            .and_then(|i| ids.get(i))
            .copied()
            .unwrap_or(false);
        if selected {
            let _ = filter.set(n, 1);
        }
    }
    progress.progress(u64::from(games), u64::from(games));
    Some((filter, unreadable))
}

/// The games of the player: the selection by name, then, with a position,
/// the games that reach it (the filter value is the ply of the first
/// occurrence plus 1), with the number of games that could not be read (an
/// entry, or a game in the search by position) and were left out.
/// `Ok(None)` when `cancel` stopped the work.
pub(super) fn select_player_games<D: GameSource + ?Sized>(
    db: &mut D,
    name: &str,
    color: Color,
    matching: NameMatch,
    position: Option<&Position>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<Option<(Filter, u32)>, ReportError> {
    let games = u64::from(db.len());
    let Some((mut filter, mut unreadable)) = scan(db, name, color, matching, progress, cancel)
    else {
        return Ok(None);
    };
    let Some(position) = position else {
        return Ok(Some((filter, unreadable)));
    };
    let mut query = BoardSearch::new(position.clone());
    query.op = FilterOp::And;
    let mut shifted = Offset {
        inner: progress,
        offset: games,
    };
    let report =
        search_board(db, &query, &mut filter, &mut shifted, cancel).map_err(|e| match e {
            SearchError::FilterSize { filter, games } => ReportError::FilterSize { filter, games },
            SearchError::UnsupportedPosition => ReportError::UnsupportedPosition,
            other => ReportError::Search(other),
        })?;
    unreadable =
        unreadable.saturating_add(u32::try_from(report.unreadable_total).unwrap_or(u32::MAX));
    // A cancelled search leaves the filter as it was: not the games that
    // reach the position.
    Ok((!report.interrupted).then_some((filter, unreadable)))
}

/// Progress moved by a fixed amount.
struct Offset<'a> {
    inner: &'a mut dyn ProgressSink,
    offset: u64,
}

impl ProgressSink for Offset<'_> {
    fn progress(&mut self, done: u64, total: u64) {
        let _ = total;
        self.inner.progress(self.offset.saturating_add(done), 0);
    }
}

/// The games of `name` with `color` as a filter (every game has the value 1),
/// the first step of the player report (`sc_search header -white|-black`).
///
/// The match is by name ID ([`NameMatch::Exact`], the case counts) or by the
/// text, ignoring the case. A name that does not exist gives an empty filter.
///
/// # Errors
///
/// [`ReportError::InvalidName`] for a name without a letter or digit (empty,
/// `?`, `,`...); [`ReportError::Cancelled`] when `cancel` is set.
pub fn games_of_player<D: GameSource + ?Sized>(
    db: &D,
    name: &str,
    color: Color,
    matching: NameMatch,
    cancel: &CancelToken,
) -> Result<Filter, ReportError> {
    let name = name.trim();
    if !is_valid_name(name) {
        return Err(ReportError::InvalidName);
    }
    scan(db, name, color, matching, &mut NoProgress, cancel)
        .map(|(filter, _)| filter)
        .ok_or(ReportError::Cancelled)
}
