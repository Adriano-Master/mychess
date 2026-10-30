//! The spelling file (`.ssp`), the name checker and "Add Elo" (spec
//! `docs/specs/m6-3-jogadores-torneios-spelling.md`, slice M6-3b: the
//! controller side over `mychess_db::spelling`).
//!
//! - `message`: [`SpellingMessage`], the effects and answers, the
//!   [`SpellingNotice`] with its Fluent keys, and [`Shared`], the wrapper
//!   that lets big values travel in messages;
//! - `prefs`: the `[player_tools]` table ([`PlayerToolsPrefs`]);
//! - `view`: the snapshots of the windows ([`SpellView`], [`AddEloView`],
//!   [`SpellingStatusView`]) without `mychess-db` types;
//! - `work`: what [`perform`](crate::perform) runs on a blocking thread.
//!
//! The flow (loading, the checker, the writes through the list job, the
//! cache of `PlayerStats`) is in `state/spelling_flow.rs`. The writes follow
//! `classify_run` of the ECO tools: the `FileStamp` is checked, the writer
//! is dropped before the database is read again, and the slot is reopened
//! whatever happened (spec rule 56, D3341).

mod message;
mod prefs;
mod view;
mod work;

use std::path::PathBuf;

use mychess_db::PlayerStats;

pub use message::{
    EloOutcome, LoadedSpelling, NamesOutcome, PhotoCounts, PlannedElo, ScanDone, Shared,
    SharedPhotos, SharedSpelling, SpellRow, SpellingCounts, SpellingEffect, SpellingLoadError,
    SpellingMessage, SpellingNotice,
};
pub use prefs::{
    AddEloPrefs, DEFAULT_LIST_SIZE, DEFAULT_TOURNAMENT_START, FinderPrefs, HISTORY_MAX,
    MAX_LIST_SIZE, PlayerSortKey, PlayerToolsPrefs, SpellcheckPrefs, TournamentPrefs,
    TournamentSortKey,
};
pub use view::{AddEloFlags, AddEloView, SpellFlags, SpellRowView, SpellView, SpellingStatusView};

pub(crate) use message::{EloJob, NamesJob, PlanJob, ScanJob, Target};
pub(crate) use prefs::read_player_tools;
pub(crate) use work::{changes_of, perform, rows_text};

use std::sync::Arc;

use crate::bases::{OpenControl, SharedSource, SlotId};
use crate::error::RequestId;
use crate::game_info::NameKind;

/// A change of `[player_tools]` made while the preferences file was being
/// read. It is applied again, over the file's values, when the file arrives,
/// so only the fields it touches are overwritten.
#[derive(Clone)]
pub(crate) struct ToolsDelta(pub(crate) Arc<dyn Fn(&mut PlayerToolsPrefs) + Send + Sync>);

impl std::fmt::Debug for ToolsDelta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ToolsDelta")
    }
}

/// A computation in flight.
#[derive(Debug)]
pub(crate) struct Running {
    pub(crate) request: RequestId,
    pub(crate) control: OpenControl,
}

/// The loading of the spelling file in flight.
#[derive(Debug)]
pub(crate) struct LoadRun {
    pub(crate) run: Running,
    /// The file asked for by the user (becomes the preference), as opposed
    /// to the start-up and the reload.
    pub(crate) remember: Option<PathBuf>,
    /// The file asked for (`None`: the default one), kept to start the load
    /// again when a cancelled quit had cut it.
    pub(crate) path: Option<PathBuf>,
    /// A missing file is not worth a remark (start-up).
    pub(crate) quiet: bool,
    /// The user cancelled the load (not the quit): it is not started again
    /// when a quit is given up (D3502).
    pub(crate) user_cancelled: bool,
}

/// The `PlayerStats` of one slot, made with the spelling file of
/// `fingerprint`.
#[derive(Debug)]
pub(crate) struct StatsEntry {
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    pub(crate) fingerprint: u64,
    pub(crate) stats: Shared<PlayerStats>,
}

/// The build of `PlayerStats` in flight.
#[derive(Debug)]
pub(crate) struct StatsRun {
    pub(crate) run: Running,
    pub(crate) slot: SlotId,
    pub(crate) source: SharedSource,
    pub(crate) len: u32,
    pub(crate) fingerprint: u64,
}

/// The name checker window.
#[derive(Debug)]
pub(crate) struct CheckDialog {
    pub(crate) kind: NameKind,
    pub(crate) target: Target,
    /// The `FileStamp` of the slot when the scan began.
    pub(crate) surnames: bool,
    pub(crate) ambiguous: bool,
    pub(crate) rows: Vec<SpellRow>,
    /// The rows that pass the search box, built when the list or the search
    /// change (not at every `view`).
    pub(crate) rows_view: Shared<Vec<SpellRowView>>,
    /// The rows ticked (kept with `rows_view`).
    pub(crate) ticked: usize,
    pub(crate) scanned: bool,
    pub(crate) interrupted: bool,
    /// Surnames or ambiguity changed while the scan could not run (a write
    /// was under way): the end of the write scans again.
    pub(crate) options_changed: bool,
    pub(crate) scan: Option<Running>,
    pub(crate) find: String,
    /// The confirmation of "Make corrections" is on screen.
    pub(crate) confirm: bool,
    /// How the last application ended.
    pub(crate) result: Option<SpellingNotice>,
}

impl CheckDialog {
    /// Rebuilds the cached view of the rows (after the list, a row or the
    /// search box changed).
    pub(crate) fn rebuild_rows(&mut self) {
        let find = self.find.trim().to_lowercase();
        let rows = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                find.is_empty()
                    || r.old.to_lowercase().contains(&find)
                    || r.new.to_lowercase().contains(&find)
            })
            .map(|(index, row)| SpellRowView {
                index,
                row: row.clone(),
            })
            .collect();
        self.rows_view = Shared::new(rows);
        self.ticked = self.rows.iter().filter(|r| r.selected).count();
    }
}

/// The "Add Elo" dialog.
#[derive(Debug)]
pub(crate) struct AddEloDialog {
    pub(crate) slot: SlotId,
    pub(crate) overwrite: bool,
    pub(crate) only_filter: bool,
    /// The plan, made for the database of `target`.
    pub(crate) planned: Option<(Target, PlannedElo)>,
    /// The plan in flight.
    pub(crate) planning: Option<(Running, Target)>,
    /// The write in flight (a list job).
    pub(crate) writing: Option<RequestId>,
    pub(crate) result: Option<SpellingNotice>,
}

/// The state of the spelling file, the checker and "Add Elo".
#[derive(Debug)]
pub struct SpellingUi {
    pub(crate) set: Option<SharedSpelling>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) counts: Option<SpellingCounts>,
    pub(crate) load: Option<LoadRun>,
    /// The file dialog in flight.
    pub(crate) pick: Option<RequestId>,
    pub(crate) photos: SharedPhotos,
    pub(crate) photo_counts: Option<PhotoCounts>,
    pub(crate) photos_run: Option<Running>,
    /// The load the quit cut and whose answer already came: a quit given up
    /// starts it again (D3502).
    pub(crate) cut_load: Option<LoadRun>,
    /// The photos the quit cut (or did not start) and whose answer already
    /// came (D3502).
    pub(crate) photos_cut: bool,
    pub(crate) stats: Vec<StatsEntry>,
    pub(crate) stats_run: Option<StatsRun>,
    pub(crate) check: Option<CheckDialog>,
    pub(crate) add_elo: Option<AddEloDialog>,
    /// The write in flight (the request of the list job).
    pub(crate) write: Option<RequestId>,
    /// The kind of name the write in flight corrects.
    pub(crate) write_kind: Option<NameKind>,
    /// Changes of `[player_tools]` waiting for the preferences file.
    pub(crate) early_tools: Vec<ToolsDelta>,
    /// The data directory (`None`: the system's), settable for tests.
    pub(crate) data_dir: Option<PathBuf>,
    /// Counts the changes made to a database in place (the Clipbase), which
    /// keep its identity.
    pub(crate) version: u64,
    /// The program is ending: nothing starts again.
    pub(crate) quitting: bool,
    /// The statistics that were stopped, not to be started again at once.
    pub(crate) stats_stopped: Option<(SlotId, SharedSource, u32, u64)>,
    /// The build of statistics a write asked to stop, still running until it
    /// notices (D3498): the write waits for its answer.
    pub(crate) stats_draining: Option<RequestId>,
    /// The write that waits for `stats_draining` to end.
    pub(crate) write_deferred: Option<DeferredWrite>,
}

/// A write of names or ratings waiting for the reads of its database to end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferredWrite {
    Names,
    Elo,
}

impl Default for SpellingUi {
    fn default() -> SpellingUi {
        SpellingUi {
            set: None,
            path: None,
            counts: None,
            load: None,
            pick: None,
            photos: Shared::new(mychess_db::spelling::photo::PhotoLibrary::new()),
            photo_counts: None,
            photos_run: None,
            cut_load: None,
            photos_cut: false,
            stats: Vec::new(),
            stats_run: None,
            check: None,
            add_elo: None,
            write: None,
            write_kind: None,
            early_tools: Vec::new(),
            data_dir: None,
            version: 0,
            quitting: false,
            stats_stopped: None,
            stats_draining: None,
            write_deferred: None,
        }
    }
}

impl SpellingUi {
    /// The spelling file loaded, if any.
    pub fn spelling(&self) -> Option<&SharedSpelling> {
        self.set.as_ref()
    }

    /// Whether a spelling file is loaded.
    pub fn is_loaded(&self) -> bool {
        self.set.is_some()
    }

    /// Whether the name checker window is open (a cheap test for the
    /// front-end; [`AppState::spell_view`](crate::AppState::spell_view)
    /// builds the rows).
    pub fn is_check_open(&self) -> bool {
        self.check.is_some()
    }

    /// Whether the "Add Elo ratings" dialog is open.
    pub fn is_add_elo_open(&self) -> bool {
        self.add_elo.is_some()
    }

    /// Whether the file is being read.
    pub fn is_loading(&self) -> bool {
        self.load.is_some()
    }

    /// Stops what runs (the program is ending).
    pub(crate) fn cancel(&self) {
        let runs = [
            self.load.as_ref().map(|l| &l.run),
            self.photos_run.as_ref(),
            self.stats_run.as_ref().map(|s| &s.run),
            self.check.as_ref().and_then(|c| c.scan.as_ref()),
            self.add_elo
                .as_ref()
                .and_then(|a| a.planning.as_ref().map(|(run, _)| run)),
        ];
        for run in runs.into_iter().flatten() {
            run.control.cancel();
        }
    }
}
