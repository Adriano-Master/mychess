//! Database maintenance (milestone M7, slice 1a; `docs/specs/m7-1-manutencao.md`).
//!
//! The core of the Maintenance window, without interface:
//! - [`database_stats`], [`compaction_stats`] and [`name_usage`]: the counters
//!   of `recalcFlagCounts`, `sc_base stats` and `sc_compact stats`, read from
//!   the index only;
//! - [`check_games`]: `sc_base check`, with the cause of each problem;
//! - [`find_duplicates`], [`diff_games`] and [`share_tags_preview`]: the twin
//!   games (`sc_base duplicates`), the side-by-side comparison of the twin
//!   checker and `sc_game tags share`;
//! - [`list_extra_tags`], [`filter_by_tag`] and [`games_in_scope`]: the read
//!   side of the tag and name editors;
//! - [`repair_database`] (slice a3): the repair of a damaged index and name
//!   file, with backups; the sort with up to 15 criteria returns its
//!   permutation in [`SortReport::order`](crate::si4::SortReport::order); and
//!   [`CleanerReport`], which sums the reports of the Cleaner steps;
//! - the writes that belong here live in the writer:
//!   [`Si4Writer::set_flags_batch`](crate::si4::Si4Writer::set_flags_batch),
//!   [`Si4Writer::share_tags`](crate::si4::Si4Writer::share_tags) (slice a1),
//!   [`Si4Writer::strip_games`](crate::si4::Si4Writer::strip_games),
//!   [`Si4Writer::edit_tags`](crate::si4::Si4Writer::edit_tags) and
//!   [`Si4Writer::edit_names`](crate::si4::Si4Writer::edit_names) (slice a2).
//!
//! Every long function takes a [`ProgressSink`](crate::ProgressSink) and a
//! [`CancelToken`](crate::CancelToken); progress never goes backwards, a
//! cancelled run returns what it had with `interrupted` set, and damaged
//! input becomes a count or an item of the report, never a panic.

mod check;
mod cleaner;
mod duplicates;
mod edit;
mod repair;
mod share;
mod stats;
mod twin_diff;

use crate::filter::Filter;
use crate::si4::Si4Error;

pub use crate::si4::{RepairError, RepairOptions, RepairReport};
pub use crate::si4::{SharedField, SharedTag};
pub use check::{CheckCause, CheckIssue, CheckReport, MAX_CHECK_ISSUES, check_games};
pub use cleaner::{CleanerCounts, CleanerReport, CleanerStep, StepRecord, StepStatus};
pub use duplicates::{
    DeleteWhich, DuplicateCriteria, DuplicateReport, DuplicateScan, MAX_PAIRS_PER_GROUP, TwinPair,
    find_duplicates, find_duplicates_whole_groups, find_duplicates_with_limit,
};
pub use edit::{TagCount, TagFilter, TagListing, filter_by_tag, games_in_scope, list_extra_tags};
pub use repair::repair_database;
pub use share::share_tags_preview;
pub use stats::{
    CompactionStats, DatabaseStats, NameFrequencies, NameUsage, compaction_stats, database_stats,
    name_frequencies, name_usage,
};
pub use twin_diff::{DiffToken, GameDiff, MAX_DIFF_TOKENS, diff_games};

/// Why a maintenance function could not run (damage inside the database is
/// reported in the result, not here).
#[derive(Debug, thiserror::Error)]
pub enum MaintError {
    /// The scope filter does not cover the database.
    #[error("the filter covers {filter} games but the database has {games}")]
    FilterSize {
        /// Games in the filter.
        filter: u32,
        /// Games in the database.
        games: u32,
    },
    /// A database error that stops the whole operation.
    #[error(transparent)]
    Si4(#[from] Si4Error),
    /// A repair failed after it had made backups of the originals (the
    /// error lists them).
    #[error(transparent)]
    Repair(#[from] RepairError),
    /// A game could not be written as PGN text.
    #[error("cannot write a game as pgn")]
    Write(#[from] mychess_pgn::write::WriteError),
}

/// Which games an operation looks at.
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    /// Every game.
    All,
    /// Only the games in the filter, which must cover the database.
    Filter(&'a Filter),
}

impl Scope<'_> {
    /// Checks the filter size against `games`.
    pub(crate) fn validate(self, games: u32) -> Result<(), MaintError> {
        match self {
            Scope::Filter(filter) if filter.len() != games => Err(MaintError::FilterSize {
                filter: filter.len(),
                games,
            }),
            _ => Ok(()),
        }
    }

    /// Whether game `n` (base 1) is in scope.
    pub(crate) fn contains(self, n: u32) -> bool {
        match self {
            Scope::All => true,
            Scope::Filter(filter) => filter.contains(n),
        }
    }

    /// How many games are in scope in a database of `games`.
    pub(crate) fn count(self, games: u32) -> u32 {
        match self {
            Scope::All => games,
            Scope::Filter(filter) => filter.count(),
        }
    }
}
