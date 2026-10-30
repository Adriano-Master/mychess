// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_base_tag` 2677-2829) and
// tcl/file/maint.tcl (`extraTags` 2046-2265). Copyright (c) 1999-2004 Shane
// Hudson; Scid vs PC contributors. Scid is distributed under the GNU General
// Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The read side of the tag and name editors (decision D3521, rules 36 and 37
//! of `docs/specs/m7-1-manutencao.md`): the list of extra tags with their
//! counts, the filter of the games that have a tag, and the conversion of a
//! [`Scope`] into the list of games the writers take
//! ([`Si4Writer::strip_games`](crate::si4::Si4Writer::strip_games),
//! [`Si4Writer::edit_tags`](crate::si4::Si4Writer::edit_tags),
//! [`Si4Writer::edit_names`](crate::si4::Si4Writer::edit_names)). Nothing is
//! written here.

use std::collections::HashMap;

use super::{MaintError, Scope};
use crate::filter::Filter;
use crate::progress::{CHECK_INTERVAL, CancelToken, ProgressSink, SEARCH_CHECK_EVERY, Ticker};
use crate::si4::{bytes_to_string, extra_tags_of_record};
use crate::source::GameSource;

/// An extra tag and the number of games that have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    /// The tag name.
    pub name: String,
    /// Games that have the tag (once per game, not per occurrence).
    pub games: u32,
}

/// The result of [`list_extra_tags`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagListing {
    /// The tags, ordered by name without regard to case, then with it;
    /// `SetUp` (derived from `FEN`) is left out.
    pub tags: Vec<TagCount>,
    /// Games looked at.
    pub examined: u32,
    /// Games whose record could not be read (not counted in `tags`).
    pub unreadable: u32,
    /// The run was cancelled: the list and the counts are partial.
    pub interrupted: bool,
}

/// The result of [`filter_by_tag`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagFilter {
    /// The new filter: the games of the scope that have the tag (ply 1),
    /// nothing else. When interrupted it holds what was found so far.
    pub filter: Filter,
    /// Games in the new filter.
    pub matched: u32,
    /// Games looked at.
    pub examined: u32,
    /// Games whose record could not be read.
    pub unreadable: u32,
    /// The run was cancelled.
    pub interrupted: bool,
}

/// The numbers (base 1) of the games in `scope`, in increasing order, for the
/// writers that take a list.
///
/// # Errors
///
/// [`MaintError::FilterSize`] if the filter does not cover `games`.
pub fn games_in_scope(scope: Scope<'_>, games: u32) -> Result<Vec<u32>, MaintError> {
    scope.validate(games)?;
    Ok(match scope {
        Scope::All => (1..=games).collect(),
        Scope::Filter(filter) => filter.iter_games().collect(),
    })
}

/// The extra tag names of game `n` (raw bytes), `None` when it is unreadable.
fn tags_of<D: GameSource + ?Sized>(db: &mut D, n: u32) -> Option<Vec<String>> {
    if db.is_scid_database() {
        let record = db.record_bytes(n).ok()?;
        let tags = extra_tags_of_record(record).ok()?;
        Some(
            tags.iter()
                .map(|(name, _)| bytes_to_string(name).into_owned())
                .collect(),
        )
    } else {
        let game = db.game(n).ok()?;
        Some(
            game.tags()
                .extras()
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        )
    }
}

/// Lists the extra tags used in the games of `scope`, with the number of
/// games of each (`sc_base tag list`).
///
/// # Errors
///
/// [`MaintError::FilterSize`] if the filter does not cover the database.
pub fn list_extra_tags<D: GameSource + ?Sized>(
    db: &mut D,
    scope: Scope<'_>,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<TagListing, MaintError> {
    let games = db.len();
    scope.validate(games)?;
    let total = u64::from(scope.count(games));
    let mut counts: HashMap<String, u32> = HashMap::new();
    let mut listing = TagListing::default();
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=games {
        if !scope.contains(n) {
            continue;
        }
        if ticker.tick() {
            progress.progress(u64::from(listing.examined), total);
            if cancel.is_cancelled() {
                listing.interrupted = true;
                break;
            }
        }
        listing.examined += 1;
        match tags_of(db, n) {
            Some(names) => {
                for name in names {
                    let slot = counts.entry(name).or_insert(0);
                    *slot = slot.saturating_add(1);
                }
            }
            None => listing.unreadable += 1,
        }
    }
    listing.tags = counts
        .into_iter()
        .filter(|(name, _)| name != "SetUp")
        .map(|(name, games)| TagCount { name, games })
        .collect();
    listing.tags.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    if !listing.interrupted {
        progress.progress(total, total);
    }
    Ok(listing)
}

/// Builds the filter of the games of `scope` that have the extra tag `tag`
/// (exact name, case-sensitive; `sc_base tag find`). Games outside the scope
/// are out of the new filter.
///
/// # Errors
///
/// [`MaintError::FilterSize`] if the filter does not cover the database.
pub fn filter_by_tag<D: GameSource + ?Sized>(
    db: &mut D,
    scope: Scope<'_>,
    tag: &str,
    progress: &mut dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<TagFilter, MaintError> {
    let games = db.len();
    scope.validate(games)?;
    let total = u64::from(scope.count(games));
    let mut filter = Filter::new(games);
    filter.fill(0);
    let mut result = TagFilter {
        filter: Filter::new(0),
        matched: 0,
        examined: 0,
        unreadable: 0,
        interrupted: false,
    };
    let mut ticker = Ticker::new(SEARCH_CHECK_EVERY, CHECK_INTERVAL);
    for n in 1..=games {
        if !scope.contains(n) {
            continue;
        }
        if ticker.tick() {
            progress.progress(u64::from(result.examined), total);
            if cancel.is_cancelled() {
                result.interrupted = true;
                break;
            }
        }
        result.examined += 1;
        match tags_of(db, n) {
            Some(names) => {
                if names.iter().any(|name| name == tag) {
                    filter.set(n, 1);
                }
            }
            None => result.unreadable += 1,
        }
    }
    result.matched = filter.count();
    result.filter = filter;
    if !result.interrupted {
        progress.progress(total, total);
    }
    Ok(result)
}
