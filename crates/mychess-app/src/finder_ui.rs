//! The player finder and the tournament finder (spec
//! `docs/specs/m6-3-jogadores-torneios-spelling.md`, slice M6-3b).
//!
//! - `players`: the form of the player finder, its checks and its rows;
//! - `tournaments`: the same for the tournament finder;
//! - `message`: [`FinderMessage`], the effects and what `perform` runs;
//! - `view`: the snapshots the windows draw ([`PlayerFinderView`],
//!   [`TournamentFinderView`]).
//!
//! The flow is in `state/finder_flow.rs`. The table of players is built once
//! per database (and per spelling file, which supplies the estimated Elo and
//! the country) and then queried without reading the database again
//! (rule 80); the tournaments are found by one pass over the index for every
//! "Update" (the order and the cut are part of the pass).

mod message;
mod players;
mod tournaments;
mod view;

use mychess_db::finder::{PlayerTable, TournamentQuery};

pub use message::{FinderEffect, FinderMessage, FinderNotice, Panel};
pub use players::{PlayerField, PlayerForm, PlayerRowView};
pub use tournaments::{
    StandingView, TournamentAction, TournamentField, TournamentForm, TournamentRowView,
};
pub use view::{PlayerFinderView, TournamentFinderView};

pub(crate) use message::{DEBOUNCE_MS, perform};
pub(crate) use players::PlayerFinderUi;
pub(crate) use tournaments::{Found, TournamentFinderUi};

use crate::bases::{SharedSource, SlotId};
use crate::spelling_ui::Shared;

/// What a player table is made from; a different key means a new table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    /// Counts the changes made in place (the Clipbase).
    pub(crate) version: u64,
    /// The fingerprint of the spelling checker for players, 0 without.
    pub(crate) fingerprint: u64,
}

/// The table on show and what it was built from.
#[derive(Debug)]
pub(crate) struct TableShown {
    pub(crate) key: TableKey,
    pub(crate) table: Shared<PlayerTable>,
}

/// What a tournament search is asked for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TourKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    pub(crate) version: u64,
    pub(crate) query: TournamentQuery,
}

/// The state of the two finder windows.
#[derive(Debug, Default)]
pub struct FinderUi {
    pub(crate) players: PlayerFinderUi,
    pub(crate) tournaments: TournamentFinderUi,
}

impl FinderUi {
    /// Whether the player finder is open.
    pub fn players_open(&self) -> bool {
        self.players.open
    }

    /// Whether the tournament finder is open.
    pub fn tournaments_open(&self) -> bool {
        self.tournaments.open
    }

    /// Stops what runs (the program is ending).
    pub(crate) fn cancel(&mut self) {
        self.players.refresh.cancel();
        self.tournaments.refresh.cancel();
    }
}
