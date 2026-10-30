//! What the window of the maintenance reads: snapshots without types of
//! `mychess-db` in their fields, and the table of what each kind of database
//! allows (spec M7-1, section 1, "Habilitação"; criterion 24).

use mychess_db::GameSource;
use mychess_db::maintenance::{DatabaseStats, GameDiff, SharedTag};
use mychess_db::si4::bytes_to_string;

use super::message::{ConfirmView, MaintJobKind};
use super::prefs::MaintenancePrefs;
use crate::bases::{BaseKind, BaseSlot, SlotId};
use crate::maintenance::Stage;
use crate::spelling_ui::Shared;
use crate::state::AppState;

/// What the buttons of the window may do for the database in a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MaintAccess {
    /// Delete, undelete, mark and unmark (an `.si4` that can be written, or
    /// the Clipbase).
    pub flags: bool,
    /// Type, description, custom flags and autoload (an `.si4` that can be
    /// written).
    pub header: bool,
    /// Compact the names and the games.
    pub compact: bool,
    /// Sort (also off for a read-only database, D3517).
    pub sort: bool,
    /// Search for duplicates and mark them.
    pub twins: bool,
    /// Check the games (any database).
    pub check: bool,
    /// Remove comments and variations.
    pub strip: bool,
    /// List the extra tags and filter by one.
    pub tag_list: bool,
    /// Add and remove extra tags.
    pub tag_edit: bool,
    /// The name editor.
    pub names: bool,
    /// The Cleaner.
    pub cleaner: bool,
    /// Repair a database that is not open (needs no database).
    pub repair: bool,
    /// Take the deleted games out of the filter (any database).
    pub filter_deleted: bool,
}

impl MaintAccess {
    /// The table of the spec for the database of `slot`.
    pub(crate) fn of(slot: Option<&BaseSlot>) -> MaintAccess {
        let Some(slot) = slot else {
            return MaintAccess {
                repair: true,
                ..MaintAccess::default()
            };
        };
        let si4 = slot.kind == BaseKind::Si4 && !slot.read_only && !slot.needs_reload;
        let clip = slot.kind == BaseKind::Clipbase;
        MaintAccess {
            flags: si4 || clip,
            header: si4,
            compact: si4,
            sort: si4,
            twins: si4 || clip,
            check: !slot.needs_reload,
            strip: si4,
            tag_list: si4 || clip,
            tag_edit: si4,
            names: si4,
            cleaner: si4,
            repair: true,
            filter_deleted: !slot.needs_reload,
        }
    }
}

/// The fields of the header of an `.si4` database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderView {
    /// The type of the database (`0..=31`, others are kept as read).
    pub base_type: u32,
    /// The English name of the type, if it is one of the 32.
    pub base_type_name: Option<&'static str>,
    /// The description.
    pub description: String,
    /// The descriptions of the six custom flags.
    pub custom_flags: [String; 6],
    /// The game that opens with the database, if any.
    pub auto_load: Option<u32>,
}

/// The pair of duplicates the checker shows.
#[derive(Debug, Clone, PartialEq)]
pub struct TwinPairView {
    /// The position of the pair (from 1).
    pub position: usize,
    /// The pairs of the last search.
    pub total: usize,
    /// The game on the left.
    pub left: u32,
    /// The game on the right.
    pub right: u32,
    /// The left game has the `D` flag.
    pub left_deleted: bool,
    /// The right game has the `D` flag.
    pub right_deleted: bool,
    /// The differences, when they are computed.
    pub diff: Option<Shared<GameDiff>>,
    /// The tags the pair would share, when they are worked out.
    pub share: Option<Shared<Vec<SharedTag>>>,
}

impl MaintenanceView {
    /// A job runs or a preview counts: the buttons that start something are
    /// off and "Stop" is on.
    #[must_use]
    pub fn working(&self) -> bool {
        self.busy.is_some() || self.previewing
    }
}

/// The state of the window for the slot it follows.
#[derive(Debug, Clone, PartialEq)]
pub struct MaintenanceView {
    /// The window is open.
    pub open: bool,
    /// The slot.
    pub slot: SlotId,
    /// The name of the file (empty for the Clipbase).
    pub file_name: String,
    /// The kind of database; `None` when the slot is empty.
    pub kind: Option<BaseKind>,
    /// The database is marked read-only (or cannot be written).
    pub read_only: bool,
    /// Games in the database.
    pub games: u32,
    /// Games in the filter.
    pub filtered: u32,
    /// What the buttons may do.
    pub access: MaintAccess,
    /// The kind of the job that runs.
    pub busy: Option<MaintJobKind>,
    /// The preview that counts for a confirmation runs (it can be stopped).
    pub previewing: bool,
    /// The statistics (an `.si4` only).
    pub stats: Option<Shared<DatabaseStats>>,
    /// The statistics are being counted.
    pub stats_busy: bool,
    /// The header (an `.si4` only).
    pub header: Option<HeaderView>,
    /// The question on screen.
    pub confirm: Option<ConfirmView>,
    /// The preferences of the tools.
    pub prefs: MaintenancePrefs,
    /// Pairs of the last search for duplicates that still hold.
    pub twin_pairs: usize,
    /// The checker of duplicates is open.
    pub checker_open: bool,
}

impl AppState {
    /// What the buttons of the window may do for the slot `id`.
    pub fn maintenance_access(&self, id: SlotId) -> MaintAccess {
        MaintAccess::of(self.bases.slot(id))
    }

    /// The state of the maintenance tools.
    pub fn maintenance(&self) -> &super::MaintenanceUi {
        &self.maintenance
    }

    /// The kind of the maintenance job in flight, if one runs. The list
    /// reports it as `ListJobKind::Flags`.
    pub fn maintenance_job(&self) -> Option<MaintJobKind> {
        self.maintenance.running.as_ref().map(|r| r.kind)
    }

    /// The window, for the slot it follows (the current one).
    pub fn maintenance_view(&self) -> MaintenanceView {
        let id = self.bases.current();
        let slot = self.bases.slot(id);
        let ui = &self.maintenance;
        let stats = ui
            .stats
            .as_ref()
            .filter(|s| {
                slot.is_some_and(|slot| {
                    s.slot == id && s.source == slot.source && s.data_rev == slot.data_rev()
                })
            })
            .map(|s| s.stats.clone());
        MaintenanceView {
            open: ui.open,
            slot: id,
            file_name: slot
                .and_then(|s| s.path.as_deref())
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            kind: slot.map(|s| s.kind),
            read_only: slot.is_some_and(|s| s.read_only),
            games: slot.map_or(0, BaseSlot::total),
            filtered: slot.map_or(0, BaseSlot::filtered),
            access: MaintAccess::of(slot),
            busy: self.maintenance_job(),
            previewing: ui.preview.is_some(),
            stats,
            stats_busy: ui.stats_run.is_some(),
            header: slot.and_then(header_of),
            confirm: ui
                .staged
                .as_ref()
                .filter(|s| s.stage == Stage::Confirming)
                .and_then(|s| s.confirm.clone()),
            prefs: (*self.prefs.maintenance).clone(),
            twin_pairs: self.maint_twins().map_or(0, |t| t.scan.pairs.len()),
            checker_open: ui.checker.is_some(),
        }
    }

    /// The pair of duplicates the checker shows.
    pub fn twin_pair_view(&self) -> Option<TwinPairView> {
        let twins = self.maint_twins()?;
        let checker = self.maintenance.checker.as_ref()?;
        let pair = twins.scan.pairs.get(checker.index)?;
        let slot = self.bases.slot(twins.slot)?;
        let deleted = |n: u32| {
            slot.source.try_read().is_some_and(|db| {
                GameSource::entry(&*db, n)
                    .is_ok_and(|e| e.flags.contains(mychess_db::si4::GameFlags::DELETE))
            })
        };
        Some(TwinPairView {
            position: checker.index + 1,
            total: twins.scan.pairs.len(),
            left: pair.first,
            right: pair.second,
            left_deleted: deleted(pair.first),
            right_deleted: deleted(pair.second),
            diff: checker.diff.clone(),
            share: checker.share.clone(),
        })
    }
}

fn header_of(slot: &BaseSlot) -> Option<HeaderView> {
    if slot.kind != BaseKind::Si4 {
        return None;
    }
    let db = slot.source.try_read()?;
    let header = GameSource::header(&*db);
    let flags = header.custom_flag_descriptions();
    let text = |i: usize| {
        flags
            .get(i)
            .map(|b| bytes_to_string(b).into_owned())
            .unwrap_or_default()
    };
    Some(HeaderView {
        base_type: header.base_type(),
        base_type_name: header.base_type_name(),
        description: bytes_to_string(header.description()).into_owned(),
        custom_flags: [text(0), text(1), text(2), text(3), text(4), text(5)],
        auto_load: header.auto_load(),
    })
}
