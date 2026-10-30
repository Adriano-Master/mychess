//! The player information window (spec
//! `docs/specs/m6-3-jogadores-torneios-spelling.md`, slice M6-3b, section 3).
//!
//! - `message`: [`PlayerInfoMessage`], the effects, the Scid's link codes
//!   and the work `perform` runs;
//! - `view`: [`PlayerInfoView`], the snapshot the window draws (header from
//!   the spelling file, statistics from the database, photo).
//!
//! The flow is in `state/player_info_flow.rs`. The figures are counted in one
//! pass over the index; they are worked out again, after a short wait, when
//! the player, the filter, the opponent (the other name of the game on
//! screen), the database or the spelling file change (D3318).

mod message;
mod view;

pub use message::{
    FilterJob, InfoDone, InfoJob, PlayerInfoEffect, PlayerInfoMessage, PlayerInfoNotice,
};
pub use view::{
    InfoHeader, InfoStats, OpeningView, PhotoFormat, PlayerInfoView, PlayerPhoto, SplitView,
    TallyView,
};

pub(crate) use message::{DEBOUNCE_MS, parse_link, perform};

use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::error::RequestId;
use crate::panel_calc::Refresh;
use crate::spelling_ui::SharedPhotos;

/// What a count is asked for; a different key means a new count.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InfoKey {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    pub(crate) version: u64,
    pub(crate) name: String,
    pub(crate) opponent: Option<String>,
    /// A digest of the filter.
    pub(crate) filter: crate::panel_calc::FilterStamp,
    /// The fingerprint of the spelling checker for players, 0 without.
    pub(crate) fingerprint: u64,
    pub(crate) photos: SharedPhotos,
}

/// The figures on show and what they were counted from.
#[derive(Debug)]
pub(crate) struct InfoShown {
    pub(crate) key: InfoKey,
    pub(crate) done: InfoDone,
}

/// The filter link in flight.
#[derive(Debug)]
pub(crate) struct FilterRun {
    pub(crate) request: RequestId,
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    /// The fingerprint of the filter when the job began: a different one on
    /// the answer means a newer filter, which the answer must not replace.
    pub(crate) filter_fingerprint: crate::panel_calc::FilterStamp,
    pub(crate) control: OpenControl,
}

/// The state of the player information window.
#[derive(Debug, Default)]
pub struct PlayerInfoUi {
    pub(crate) open: bool,
    /// The player on show, or the last one looked up.
    pub(crate) name: String,
    pub(crate) refresh: Refresh<InfoKey>,
    pub(crate) shown: Option<InfoShown>,
    pub(crate) filter_run: Option<FilterRun>,
}

impl PlayerInfoUi {
    /// Whether the window is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The player on show (the last one looked up when closed).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Stops what runs (the program is ending).
    pub(crate) fn cancel(&mut self) {
        self.refresh.cancel();
        if let Some(run) = self.filter_run.take() {
            run.control.cancel();
        }
    }
}
