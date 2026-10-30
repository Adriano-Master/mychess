//! The `[maintenance]` table of the preferences (spec M7-1, section "Dados e
//! formatos"; decision D3530). Every value out of its range goes back to the
//! default of its own field when the file is read or a change is applied.

use mychess_db::maintenance::{DeleteWhich, DuplicateCriteria};
use mychess_db::si4::{MAX_SORT_KEYS, SortKey};
use serde::{Serialize, Serializer};

use crate::panel_calc::named_enum;

named_enum! {
    /// Which of the two games of a pair of duplicates is deleted ("Qual
    /// apagar" of the Scid).
    pub enum DeleteChoice {
        /// The one with fewer half moves (the lower number on a tie).
        #[default]
        Shorter => "shorter",
        /// The one with the lower number.
        LowerNumber => "lower",
        /// The one with the higher number.
        HigherNumber => "higher",
    }
}

named_enum! {
    /// The games a batch operation looks at.
    pub enum ScopeChoice {
        /// Every game.
        All => "all",
        /// The games of the filter.
        #[default]
        Filter => "filter",
    }
}

named_enum! {
    /// The games the name editor looks at.
    pub enum NameScopeChoice {
        /// Every game.
        #[default]
        All => "all",
        /// The games of the filter.
        Filter => "filter",
        /// The games of the tournament of the open game.
        Tournament => "tournament",
    }
}

named_enum! {
    /// A criterion of the sort of the database (the names of the Scid
    /// `sortCriteriaNames`, without `Random`).
    pub enum SortCriterion {
        /// The full date.
        Date => "date",
        /// The year.
        #[default]
        Year => "year",
        /// The event.
        Event => "event",
        /// The site.
        Site => "site",
        /// The round.
        Round => "round",
        /// The White player.
        White => "white",
        /// The Black player.
        Black => "black",
        /// The ECO code.
        Eco => "eco",
        /// The result.
        Result => "result",
        /// The number of half moves.
        Length => "length",
        /// The average rating.
        Rating => "rating",
        /// The rating of White.
        WhiteElo => "welo",
        /// The rating of Black.
        BlackElo => "belo",
        /// The country of the site.
        Country => "country",
        /// The month.
        Month => "month",
        /// The delete flag.
        Deleted => "deleted",
        /// The event date.
        EventDate => "eventdate",
        /// The number of variations.
        Variations => "variations",
        /// The number of comments.
        Comments => "comments",
    }
}

macro_rules! serialize_by_name {
    ($($ty:ty),+) => {$(
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }
    )+};
}
serialize_by_name!(DeleteChoice, ScopeChoice, NameScopeChoice, SortCriterion);

impl SortCriterion {
    /// The criterion as the database sorts by it.
    pub(crate) fn key(self) -> SortKey {
        match self {
            SortCriterion::Date => SortKey::Date,
            SortCriterion::Year => SortKey::Year,
            SortCriterion::Event => SortKey::Event,
            SortCriterion::Site => SortKey::Site,
            SortCriterion::Round => SortKey::Round,
            SortCriterion::White => SortKey::White,
            SortCriterion::Black => SortKey::Black,
            SortCriterion::Eco => SortKey::Eco,
            SortCriterion::Result => SortKey::Result,
            SortCriterion::Length => SortKey::Length,
            SortCriterion::Rating => SortKey::Rating,
            SortCriterion::WhiteElo => SortKey::WhiteElo,
            SortCriterion::BlackElo => SortKey::BlackElo,
            SortCriterion::Country => SortKey::Country,
            SortCriterion::Month => SortKey::Month,
            SortCriterion::Deleted => SortKey::Deleted,
            SortCriterion::EventDate => SortKey::EventDate,
            SortCriterion::Variations => SortKey::Variations,
            SortCriterion::Comments => SortKey::Comments,
        }
    }
}

/// The most criteria one sort takes (the Scid refuses the 16th).
pub const MAX_SORT_CRITERIA: usize = MAX_SORT_KEYS;

/// The criteria and options of the search for duplicate games (the defaults
/// are those of `start.tcl` 773-791, except `undelete_first`, D3524).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TwinPrefs {
    /// Names compared whole (else by their first four characters).
    pub exact_names: bool,
    /// The same colours (else the players may be swapped).
    pub same_colors: bool,
    /// The same event.
    pub same_event: bool,
    /// The same site.
    pub same_site: bool,
    /// The same round.
    pub same_round: bool,
    /// The same year.
    pub same_year: bool,
    /// The same month.
    pub same_month: bool,
    /// The same day.
    pub same_day: bool,
    /// The same result.
    pub same_result: bool,
    /// The same basic ECO code.
    pub same_eco: bool,
    /// The same moves.
    pub same_moves: bool,
    /// Skip the games of fewer than five moves.
    pub skip_short: bool,
    /// Undelete every game before the search (off: D3524).
    pub undelete_first: bool,
    /// Make the filter the games marked by the search.
    pub set_filter: bool,
    /// Never delete a game with comments.
    pub keep_commented: bool,
    /// Never delete a game with variations.
    pub keep_with_variations: bool,
    /// The game that stays gets the ratings of the deleted one.
    pub copy_ratings: bool,
    /// Look only at the games of the filter.
    pub only_filter: bool,
    /// Which of the two to delete.
    pub delete: DeleteChoice,
}

impl Default for TwinPrefs {
    fn default() -> TwinPrefs {
        TwinPrefs {
            exact_names: true,
            same_colors: true,
            same_event: false,
            same_site: true,
            same_round: true,
            same_year: true,
            same_month: true,
            same_day: false,
            same_result: false,
            same_eco: false,
            same_moves: true,
            skip_short: true,
            undelete_first: false,
            set_filter: true,
            keep_commented: true,
            keep_with_variations: true,
            copy_ratings: false,
            only_filter: false,
            delete: DeleteChoice::Shorter,
        }
    }
}

impl TwinPrefs {
    /// The criteria the database takes.
    pub fn criteria(&self) -> DuplicateCriteria {
        DuplicateCriteria {
            exact_names: self.exact_names,
            same_colors: self.same_colors,
            same_event: self.same_event,
            same_site: self.same_site,
            same_round: self.same_round,
            same_year: self.same_year,
            same_month: self.same_month,
            same_day: self.same_day,
            same_result: self.same_result,
            same_eco: self.same_eco,
            same_moves: self.same_moves,
            skip_short: self.skip_short,
            keep_commented: self.keep_commented,
            keep_with_variations: self.keep_with_variations,
            copy_ratings: self.copy_ratings,
            delete: match self.delete {
                DeleteChoice::Shorter => DeleteWhich::Shorter,
                DeleteChoice::LowerNumber => DeleteWhich::LowerNumber,
                DeleteChoice::HigherNumber => DeleteWhich::HigherNumber,
            },
        }
    }

    /// Whether the criteria are weak enough to deserve the first warning of
    /// the Scid (`twinCriteriaOK`): the moves are not compared and one of
    /// colours, event, site, year or month is off.
    pub fn weak_without_moves(&self) -> bool {
        let strict = self.same_colors
            && self.same_event
            && self.same_site
            && self.same_year
            && self.same_month;
        !self.same_moves && !strict
    }

    /// Whether fewer than two of site, round and year are on (the second
    /// warning of the Scid).
    pub fn weak_few_fields(&self) -> bool {
        [self.same_site, self.same_round, self.same_year]
            .into_iter()
            .filter(|on| *on)
            .count()
            < 2
    }
}

/// The steps the Cleaner runs (`start.tcl` 256-265: the first seven on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CleanerPrefs {
    /// Spelling of the player names.
    pub players: bool,
    /// Spelling of the event names.
    pub events: bool,
    /// Spelling of the site names.
    pub sites: bool,
    /// Spelling of the round names.
    pub rounds: bool,
    /// ECO classification.
    pub eco: bool,
    /// Add Elo ratings.
    pub elo: bool,
    /// Mark the duplicates for deletion.
    pub twins: bool,
    /// Compact the name file.
    pub compact_names: bool,
    /// Compact the game file.
    pub compact_games: bool,
    /// Fill the tree cache.
    pub tree: bool,
}

impl Default for CleanerPrefs {
    fn default() -> CleanerPrefs {
        CleanerPrefs {
            players: true,
            events: true,
            sites: true,
            rounds: true,
            eco: true,
            elo: true,
            twins: true,
            compact_names: false,
            compact_games: false,
            tree: false,
        }
    }
}

/// Everything the maintenance tools remember, in `[maintenance]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MaintenancePrefs {
    /// The criteria of the duplicates.
    pub twins: TwinPrefs,
    /// The steps of the Cleaner.
    pub cleaner: CleanerPrefs,
    /// The criteria of the last sort (at most 15).
    pub sort_keys: Vec<SortCriterion>,
    /// The last sort was in reverse.
    pub sort_reverse: bool,
    /// "Remove comments/variations": all or the filter.
    pub strip_scope: ScopeChoice,
    /// "Extra tags": all or the filter.
    pub tag_scope: ScopeChoice,
    /// The name editor: all, the filter or the tournament.
    pub name_editor_scope: NameScopeChoice,
    /// Compact the database after an operation that rewrites games.
    pub compact_after_rewrite: bool,
}

/// The criteria of "Common sorts" the Scid proposes first.
pub fn default_sort_keys() -> Vec<SortCriterion> {
    vec![
        SortCriterion::Year,
        SortCriterion::Month,
        SortCriterion::Site,
        SortCriterion::Event,
        SortCriterion::Date,
        SortCriterion::Round,
    ]
}

impl Default for MaintenancePrefs {
    fn default() -> MaintenancePrefs {
        MaintenancePrefs {
            twins: TwinPrefs::default(),
            cleaner: CleanerPrefs::default(),
            sort_keys: default_sort_keys(),
            sort_reverse: false,
            strip_scope: ScopeChoice::Filter,
            tag_scope: ScopeChoice::Filter,
            name_editor_scope: NameScopeChoice::All,
            compact_after_rewrite: false,
        }
    }
}

impl MaintenancePrefs {
    /// The value with every field out of its range back at the default (a
    /// wrong value never makes an error): the sort has between one and
    /// fifteen criteria.
    #[must_use]
    pub fn sanitized(mut self) -> MaintenancePrefs {
        if self.sort_keys.is_empty() {
            self.sort_keys = default_sort_keys();
        }
        self.sort_keys.truncate(MAX_SORT_CRITERIA);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_those_of_the_scid_but_for_the_undelete() {
        let twins = TwinPrefs::default();
        assert!(twins.exact_names && twins.same_colors && twins.same_site);
        assert!(twins.same_round && twins.same_year && twins.same_month && twins.same_moves);
        assert!(!twins.same_event && !twins.same_day && !twins.same_result && !twins.same_eco);
        assert!(twins.skip_short && twins.set_filter);
        assert!(twins.keep_commented && twins.keep_with_variations);
        assert!(!twins.undelete_first, "D3524");
        let cleaner = CleanerPrefs::default();
        assert!(cleaner.players && cleaner.events && cleaner.sites && cleaner.rounds);
        assert!(cleaner.eco && cleaner.elo && cleaner.twins);
        assert!(!cleaner.compact_names && !cleaner.compact_games && !cleaner.tree);
    }

    #[test]
    fn a_sort_of_sixteen_criteria_is_cut_to_fifteen_and_an_empty_one_is_reset() {
        let long = MaintenancePrefs {
            sort_keys: vec![SortCriterion::Date; 20],
            ..MaintenancePrefs::default()
        }
        .sanitized();
        assert_eq!(long.sort_keys.len(), MAX_SORT_CRITERIA);
        let empty = MaintenancePrefs {
            sort_keys: Vec::new(),
            ..MaintenancePrefs::default()
        }
        .sanitized();
        assert_eq!(empty.sort_keys, default_sort_keys());
    }

    #[test]
    fn the_names_of_the_criteria_are_those_of_the_database() {
        for criterion in SortCriterion::ALL {
            assert_eq!(criterion.name(), criterion.key().name());
        }
    }

    #[test]
    fn the_weak_criteria_warnings_follow_the_scid() {
        let mut twins = TwinPrefs::default();
        assert!(!twins.weak_without_moves());
        assert!(!twins.weak_few_fields());
        twins.same_moves = false;
        // Event is off by default: no moves and a weak set of fields.
        assert!(twins.weak_without_moves());
        twins.same_site = false;
        twins.same_round = false;
        assert!(twins.weak_few_fields());
    }
}
