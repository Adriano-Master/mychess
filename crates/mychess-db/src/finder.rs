// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_name_plist`,
// `sc_base_tournaments`, `SortNamebaseNodes`, `SortTourney`) and
// tcl/windows/pfinder.tcl, tcl/windows/tourney.tcl. Copyright (c) 1999-2003
// Shane Hudson; Scid vs PC contributors. Scid is distributed under the GNU
// General Public License (see its COPYING); this file is part of MyChess,
// GPL-3.0-or-later (docs/adr/0004-licenca.md).

//! The player finder and the tournament finder (milestone M6, slice 3a;
//! `docs/specs/m6-3-jogadores-torneios-spelling.md`, sections G and I).
//!
//! - [`players`]: [`PlayerTable`] (one pass over the index, kept by the
//!   application) and its queries by name prefix, games, Elo and country.
//! - [`tournaments`]: [`find_tournaments`], which groups the games of a
//!   database into tournaments and ranks the players of each.
//!
//! Both run on any [`GameSource`](crate::GameSource), report progress and can
//! be cancelled.

use std::cmp::Ordering;

use thiserror::Error;

use crate::si4::{NameBase, NameKind, bytes_to_string};

pub mod players;
pub mod tournaments;

pub use players::{
    CountryFilter, PlayerFound, PlayerQuery, PlayerRow, PlayerSort, PlayerTable, UNKNOWN_COUNTRY,
};
pub use tournaments::{Standing, TournamentQuery, TournamentRow, TournamentSort, find_tournaments};

/// The operation was cancelled before it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("the operation was cancelled")]
pub struct Cancelled;

/// Simple case folding of a string (each character lowercased when that
/// gives one character).
pub(crate) fn fold(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        })
        .collect()
}

/// `strCaseCompare` then `strCompare`: the order of names in the lists.
pub(crate) fn compare_names(a: &str, b: &str) -> Ordering {
    fold(a).cmp(&fold(b)).then_with(|| a.cmp(b))
}

/// `strAlphaContains`: `needle` occurs in `hay` ignoring case and spaces.
/// An empty needle (or only spaces) always matches.
pub(crate) fn alpha_contains(hay: &str, needle: &str) -> bool {
    let strip = |s: &str| -> String { fold(s).chars().filter(|c| *c != ' ').collect() };
    let needle = strip(needle);
    needle.is_empty() || strip(hay).contains(needle.as_str())
}

/// The ID of the name with this text (UTF-8, or Windows-1252 in old
/// databases), the first one in ID order.
pub(crate) fn find_name_id(names: &NameBase, kind: NameKind, text: &str) -> Option<u32> {
    (0..names.num_names(kind)).find(|&id| {
        names
            .get(kind, id)
            .is_some_and(|raw| bytes_to_string(raw) == text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(compare_names("abc", "ABD"), Ordering::Less);
        assert_eq!(compare_names("ABC", "abc"), Ordering::Less);
        assert!(alpha_contains("Smith, John", "th,j"));
        assert!(alpha_contains("Smith", ""));
        assert!(alpha_contains("Smith", "   "));
        assert!(!alpha_contains("Smith", "x"));
    }
}
