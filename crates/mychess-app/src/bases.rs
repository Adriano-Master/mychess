//! The eight database slots and the Clipbase (spec M3-4, slice 4a;
//! decisions D500 to D507 and D513 onwards).
//!
//! [`Bases`] holds nine positions: `0..=7` are the slots for files (shown to
//! the user as 1 to 8) and `8` is the Clipbase (shown as 9), which is always
//! there and is never closed, only cleared. Each [`BaseSlot`] has its
//! database ([`BaseSource`]: SI4, PGN in memory or Clipbase), its
//! [`Filter`], the state of its game list ([`ListState`]), the number of the
//! game being edited and, while the slot is not the current one, the game
//! that was being edited in it, so switching slots keeps each base's game,
//! even an altered one.
//!
//! # Layout
//!
//! - `source`: [`BaseSource`], [`SharedSource`], [`BaseKind`];
//! - `clipbase`: the in-memory database ([`Clipbase`]);
//! - `open`: the blocking open of a file (run by `perform`), with
//!   [`OpenControl`] for progress and cancellation;
//! - `rows`: [`GameRow`] and [`list_page`], the contract for the list of the
//!   front-end;
//! - `message`: [`BasesMessage`] and [`BasesNotice`];
//! - `error`: [`BaseError`].
//!
//! The behaviour of the messages is in `state/bases_flow.rs`.

mod browser;
#[cfg(test)]
mod browser_contract_tests;
#[cfg(test)]
mod browser_tests;
mod clipbase;
mod columns;
#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod copy_tests;
mod error;
mod jobs;
mod list_message;
#[cfg(test)]
mod list_tests;
mod message;
mod open;
#[cfg(test)]
mod review_tests;
mod rows;
mod source;
#[cfg(test)]
mod state_tests;
mod view_order;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mychess_core::{Game, NodeId};
use mychess_db::{Filter, GameSource};

pub use browser::{
    BrowseMessage, BrowseMode, BrowseRead, BrowserContent, BrowserGame, BrowserHeader,
    BrowserState, GameStep, MAX_BROWSER_GAMES, ReadyGame, TextItem, TextKind, grid_columns,
};
pub(crate) use browser::{read_browser_games, unreadable_games};
pub use clipbase::{CLIPBASE_DESCRIPTION, CLIPBASE_TYPE, Clipbase, DEFAULT_CLIPBASE_LIMIT};
pub use columns::{
    ColumnAlign, ColumnSetup, ListColumn, MAX_COLUMN_CHARS, MIN_COLUMN_CHARS, SortDir,
};
pub use error::BaseError;
pub(crate) use jobs::{copy_games_to, find_text, sort_order, toggle_deleted};
pub use list_message::{
    CopyEnd, CopyList, CopyOutcome, CopyPrompt, CopyRefusal, CopyScope, CopyTarget, FlagOutcome,
    ListJobKind, ListMessage, PickMode, Reloaded, SearchKind, SearchOutcome, SearchSpec,
};
pub use message::{BasesMessage, BasesNotice};
pub(crate) use open::open_base;
pub use open::{FileStamp, OpenControl, OpenOutcome, OpenedBase};
pub use rows::{GameRow, list_page, rows_of};
pub use source::{BaseKind, BaseSource, SharedSource};
pub(crate) use view_order::ViewOrder;

use crate::files::GameOrigin;

/// The position of a slot: `0..=7` for files, `8` for the Clipbase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(u8);

impl SlotId {
    /// The number of positions: eight files and the Clipbase.
    pub const COUNT: usize = 9;
    /// The Clipbase (`CLIPBASE_NUM`).
    pub const CLIPBASE: SlotId = SlotId(8);

    /// The slot at `index` (`0..=8`).
    pub fn new(index: usize) -> Option<SlotId> {
        u8::try_from(index)
            .ok()
            .filter(|i| usize::from(*i) < SlotId::COUNT)
            .map(SlotId)
    }

    /// The slot the user calls `number` (1 to 9; 9 is the Clipbase).
    pub fn from_number(number: usize) -> Option<SlotId> {
        number.checked_sub(1).and_then(SlotId::new)
    }

    /// The index in `0..=8`.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The number shown to the user, 1 to 9.
    pub fn number(self) -> usize {
        self.index() + 1
    }

    /// Whether this is the Clipbase.
    pub fn is_clipbase(self) -> bool {
        self == SlotId::CLIPBASE
    }

    /// Every position, in order.
    pub fn all() -> impl Iterator<Item = SlotId> {
        (0..SlotId::COUNT).filter_map(SlotId::new)
    }
}

/// The state of the game list of a slot (D502). The list is drawn by the
/// front-end; the fields are kept per slot so switching slots never loses
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListState {
    /// The position in the view of the first visible row, from 1 (D520).
    pub first: u32,
    /// The text of the quick-search field.
    pub search: String,
    /// "Ignore case" of the quick search (the Scid's `findcase`).
    pub ignore_case: bool,
    /// The game the selection is anchored on (the last one clicked or
    /// reached by the keyboard): the row the arrows move, the row "remove
    /// above" and "remove below" refer to, and the game Enter loads. Always
    /// in [`ListState::selection`] when set.
    pub selected: Option<u32>,
    /// Every selected game (its number in the database), D557.
    pub selection: BTreeSet<u32>,
    /// How the view is sorted, if it is (D506). The order of the games is
    /// in the slot; this is what the header shows.
    pub sort: Option<(ListColumn, SortDir)>,
}

impl Default for ListState {
    fn default() -> ListState {
        ListState {
            first: 1,
            search: String::new(),
            ignore_case: false,
            selected: None,
            selection: BTreeSet::new(),
            sort: None,
        }
    }
}

impl ListState {
    /// Selects only `number` (or nothing).
    pub(crate) fn select_only(&mut self, number: Option<u32>) {
        self.selection.clear();
        self.selected = number;
        if let Some(n) = number {
            self.selection.insert(n);
        }
    }
}

/// The game that was being edited in a slot that is no longer the current
/// one.
#[derive(Debug, Clone)]
pub(crate) struct Parked {
    pub(crate) game: Game,
    pub(crate) origin: GameOrigin,
    pub(crate) cursor: NodeId,
    pub(crate) dirty: bool,
}

/// What a cached page was drawn for: it is shown again, when the database
/// is locked by a writer, only if the list still asks for the same page of
/// the same filter and order (D593).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageKey {
    first: u32,
    count: u32,
    games: u32,
    filtered: u32,
    sort: Option<(ListColumn, SortDir)>,
}

/// One open database.
#[derive(Debug)]
#[non_exhaustive]
pub struct BaseSlot {
    /// The database, shared with the effects that read games.
    pub source: SharedSource,
    /// The kind of database.
    pub kind: BaseKind,
    /// The canonical path of the file; `None` for the Clipbase.
    pub path: Option<PathBuf>,
    /// Games cannot be added, replaced or flagged.
    pub read_only: bool,
    /// The database cannot be written whatever the user says (a PGN file,
    /// or an `.si4` whose files are read-only): `read_only` cannot be
    /// turned off (slice 4d, D582).
    pub(crate) locked_read_only: bool,
    /// A copy wrote to the file but the database could not be opened again:
    /// the slot is out of date and refuses writes (D591).
    pub(crate) needs_reload: bool,
    /// The games of the list. Always as long as the database.
    pub(crate) filter: Filter,
    /// The state of the list.
    pub list: ListState,
    /// The number of the game loaded from this database into the editor
    /// (from 1), if the editor holds one.
    pub current_game: Option<u32>,
    /// Games of the PGN file that could not be read and were left out.
    pub(crate) skipped: u64,
    /// The PGN file had text outside its games that was not kept.
    pub(crate) ignored_text: bool,
    pub(crate) parked: Option<Parked>,
    /// The sorted order of the view, if the list is sorted (D506).
    pub(crate) order: Option<ViewOrder>,
    /// The state of the `.si4` file when it was read (D573).
    pub(crate) stamp: Option<FileStamp>,
    /// The last page given to the list, kept for when the database is
    /// locked by a writer and `view` cannot wait (D570).
    pub(crate) page_cache: Mutex<Option<(PageKey, Vec<GameRow>)>>,
    /// Counts the changes of the filter (D3421).
    filter_rev: u64,
    /// Counts the changes of the games under the slot (D3421).
    data_rev: u64,
}

impl BaseSlot {
    /// A slot for a database just opened: every game in the filter.
    pub(crate) fn from_opened(opened: OpenedBase) -> BaseSlot {
        let (len, locked) = {
            let source = opened.source.read();
            (
                GameSource::len(&*source),
                source.is_read_only() || opened.read_only,
            )
        };
        BaseSlot {
            source: opened.source,
            kind: opened.kind,
            path: Some(opened.path),
            read_only: locked,
            locked_read_only: locked,
            needs_reload: false,
            filter: Filter::new(len),
            list: ListState::default(),
            current_game: None,
            skipped: opened.skipped,
            ignored_text: opened.ignored_text,
            parked: None,
            order: None,
            stamp: opened.stamp,
            page_cache: Mutex::new(None),
            filter_rev: 0,
            data_rev: 0,
        }
    }

    /// An empty Clipbase.
    pub(crate) fn clipbase(limit: u32) -> Result<BaseSlot, BaseError> {
        let clip = Clipbase::new(limit)?;
        Ok(BaseSlot {
            source: SharedSource::new(BaseSource::Clipbase(Box::new(clip))),
            kind: BaseKind::Clipbase,
            path: None,
            read_only: false,
            locked_read_only: false,
            needs_reload: false,
            filter: Filter::new(0),
            list: ListState::default(),
            current_game: None,
            skipped: 0,
            ignored_text: false,
            parked: None,
            order: None,
            stamp: None,
            page_cache: Mutex::new(None),
            filter_rev: 0,
            data_rev: 0,
        })
    }

    /// A counter that grows every time the filter changes (a search, the
    /// list commands, a reset). The panels that compute over the filter key
    /// on it instead of hashing the filter on every `update` (D3421). It is
    /// bumped by `BaseSlot::filter_changed` and `BaseSlot::clear_sort`;
    /// code that writes `filter` must call one of them.
    pub fn filter_rev(&self) -> u64 {
        self.filter_rev
    }

    /// A counter that grows every time the games under the slot change
    /// (appended, replaced, reloaded), even when the total stays the same.
    pub fn data_rev(&self) -> u64 {
        self.data_rev
    }

    /// The filter of the list (read-only; changed by `update` only).
    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    /// Games of the PGN file that could not be read and were left out.
    pub fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Whether the file holds something the database did not keep: games
    /// that were skipped or text outside the games. Replacing such a file
    /// loses that part (spec M2-2, `PartiallyRead`).
    pub fn is_partial(&self) -> bool {
        self.skipped > 0 || self.ignored_text
    }

    /// Whether the database can never be written (a PGN file, an `.si4`
    /// whose files are read-only): the "read-only" mark cannot be turned
    /// off.
    pub fn is_locked_read_only(&self) -> bool {
        self.locked_read_only
    }

    /// The number of games in the database.
    pub fn total(&self) -> u32 {
        self.filter.len()
    }

    /// The number of games in the filter.
    pub fn filtered(&self) -> u32 {
        self.filter.count()
    }

    /// Whether the game that was being edited here was altered. Only
    /// meaningful for a slot that is not current; for the current one ask
    /// [`AppState::is_dirty`](crate::AppState::is_dirty).
    pub fn parked_dirty(&self) -> bool {
        self.parked.as_ref().is_some_and(|p| p.dirty)
    }

    /// The first row scrolled to the filter: `1..=max(1, filtered)`.
    pub(crate) fn clamp_first(&mut self) {
        let last = self.filter.count().max(1);
        self.list.first = self.list.first.clamp(1, last);
    }

    /// How the view is sorted, if it is.
    pub fn sort(&self) -> Option<(ListColumn, SortDir)> {
        self.list.sort
    }

    /// The number of the game at `position` of the view (from 1), in the
    /// order the list shows: the database order, or the sorted one.
    pub fn nth_visible(&self, position: u32) -> Option<u32> {
        match &self.order {
            Some(order) => order.nth(position),
            None => self.filter.nth(position),
        }
    }

    /// The position (from 1) of game `number` in the view, if the game is
    /// in the filter.
    pub fn position_of(&self, number: u32) -> Option<u32> {
        if !self.filter.contains(number) {
            return None;
        }
        match &self.order {
            Some(order) => order.position_of(number),
            None => Some(self.filter.rank(number)),
        }
    }

    /// The numbers of the games at positions `from..=to` of the view
    /// (clamped), in view order.
    pub fn visible_range(&self, from: u32, to: u32) -> Vec<u32> {
        let from = from.max(1);
        if let Some(order) = &self.order {
            return order.range(from, to);
        }
        let mut out = Vec::new();
        let mut next = self.filter.nth(from);
        let mut position = from;
        while let Some(number) = next {
            if position > to {
                break;
            }
            out.push(number);
            position += 1;
            next = self.filter.next_after(number);
        }
        out
    }

    /// Brings everything derived from the filter back in line after it
    /// changed: the selection keeps only games that are still in, the
    /// position stays in range and the sorted view is rebuilt.
    pub(crate) fn filter_changed(&mut self) {
        self.filter_rev = self.filter_rev.wrapping_add(1);
        let filter = &self.filter;
        self.list.selection.retain(|n| filter.contains(*n));
        if self.list.selected.is_some_and(|n| !filter.contains(n)) {
            self.list.selected = self.list.selection.iter().next_back().copied();
        }
        if let Some(order) = &mut self.order {
            order.rebuild(&self.filter);
        }
        self.clamp_first();
    }

    /// Notes that the content under the slot changed without the total or
    /// the order changing (names or ratings rewritten in place): the panels
    /// that compute over the games key on [`BaseSlot::data_rev`].
    pub(crate) fn data_changed(&mut self) {
        self.data_rev = self.data_rev.wrapping_add(1);
    }

    /// Drops the sorted view (the database changed under it).
    pub(crate) fn clear_sort(&mut self) {
        self.filter_rev = self.filter_rev.wrapping_add(1);
        self.data_changed();
        self.order = None;
        self.list.sort = None;
    }
}

/// The slots and the Clipbase.
#[derive(Debug)]
pub struct Bases {
    slots: [Option<BaseSlot>; SlotId::COUNT],
    current: SlotId,
    clipbase_limit: u32,
}

impl Default for Bases {
    fn default() -> Bases {
        Bases::with_clipbase_limit(DEFAULT_CLIPBASE_LIMIT)
    }
}

impl Bases {
    /// Only the Clipbase, current, with a limit of `limit` games.
    ///
    /// Building the empty Clipbase cannot fail (it is a fixed header and an
    /// empty name base). If it ever did, the position would stay empty and
    /// the Clipbase commands would report [`BaseError::Internal`]; nothing
    /// panics.
    pub fn with_clipbase_limit(limit: u32) -> Bases {
        // Building the empty Clipbase cannot fail; if it ever did, the
        // position stays empty and the Clipbase commands report an error.
        let clipbase = BaseSlot::clipbase(limit).ok();
        let mut slots: [Option<BaseSlot>; SlotId::COUNT] = Default::default();
        if let Some(slot) = slots.get_mut(SlotId::CLIPBASE.index()) {
            *slot = clipbase;
        }
        Bases {
            slots,
            current: SlotId::CLIPBASE,
            clipbase_limit: limit,
        }
    }

    /// The current slot (the one whose game is in the editor).
    pub fn current(&self) -> SlotId {
        self.current
    }

    /// The slot at `id`, if it is open.
    pub fn slot(&self, id: SlotId) -> Option<&BaseSlot> {
        self.slots.get(id.index()).and_then(Option::as_ref)
    }

    pub(crate) fn slot_mut(&mut self, id: SlotId) -> Option<&mut BaseSlot> {
        self.slots.get_mut(id.index()).and_then(Option::as_mut)
    }

    /// The open slots, in order of position (files first, Clipbase last).
    pub fn iter(&self) -> impl Iterator<Item = (SlotId, &BaseSlot)> {
        SlotId::all().filter_map(|id| self.slot(id).map(|slot| (id, slot)))
    }

    /// The current slot's data (the Clipbase until a file is opened).
    pub fn current_slot(&self) -> Option<&BaseSlot> {
        self.slot(self.current)
    }

    /// The first free slot for a file, `0..=7`.
    pub fn first_free(&self) -> Option<SlotId> {
        SlotId::all()
            .filter(|id| !id.is_clipbase())
            .find(|id| self.slot(*id).is_none())
    }

    /// The slot that holds the file `path` (compared as given: pass the
    /// canonical path).
    pub fn find_by_path(&self, path: &Path) -> Option<SlotId> {
        self.iter()
            .find(|(_, slot)| slot.path.as_deref() == Some(path))
            .map(|(id, _)| id)
    }

    /// The paths of the files that are open.
    pub fn open_paths(&self) -> Vec<PathBuf> {
        self.iter().filter_map(|(_, s)| s.path.clone()).collect()
    }

    /// The limit of games of the Clipbase.
    pub fn clipbase_limit(&self) -> u32 {
        self.clipbase_limit
    }

    /// Changes the limit of games of the Clipbase (5,000,000 by default).
    /// Games already in stay.
    pub fn set_clipbase_limit(&mut self, limit: u32) {
        self.clipbase_limit = limit;
        if let Some(slot) = self.slot(SlotId::CLIPBASE)
            && let Some(clip) = slot.source.lock().clipbase_mut()
        {
            clip.set_limit(limit);
        }
    }

    /// The next open slot after `from` going forward (`forward`) or back,
    /// wrapping around and skipping the empty ones. Returns `from` if it is
    /// the only open one.
    pub fn cycle(&self, from: SlotId, forward: bool) -> SlotId {
        let count = SlotId::COUNT;
        (1..count)
            .filter_map(|step| {
                let offset = if forward { step } else { count - step };
                SlotId::new((from.index() + offset) % count)
            })
            .find(|id| self.slot(*id).is_some())
            .unwrap_or(from)
    }

    /// Rows `first..first + count` (positions in the filter, from 1) of the
    /// list of a slot. Empty for a slot that is not open. See [`list_page`].
    pub fn list_page(&self, id: SlotId, first: u32, count: u32) -> Vec<GameRow> {
        let Some(slot) = self.slot(id) else {
            return Vec::new();
        };
        // `view` never waits: if a writer holds the database the last page
        // given is shown instead (D570).
        let key = PageKey {
            first,
            count,
            games: slot.filter.len(),
            filtered: slot.filter.count(),
            sort: slot.list.sort,
        };
        let Some(source) = slot.source.try_read() else {
            return slot
                .page_cache
                .lock()
                .map_or_else(|e| e.into_inner().clone(), |g| g.clone())
                .filter(|(cached, _)| *cached == key)
                .map(|(_, rows)| rows)
                .unwrap_or_default();
        };
        let rows = match &slot.order {
            _ if count == 0 => Vec::new(),
            Some(order) if slot.filter.len() == GameSource::len(&*source) => {
                let to = first.saturating_add(count.saturating_sub(1));
                rows_of(&*source, order.range(first.max(1), to))
            }
            _ => list_page(&*source, &slot.filter, first, count),
        };
        drop(source);
        match slot.page_cache.lock() {
            Ok(mut cache) => *cache = Some((key, rows.clone())),
            Err(poisoned) => *poisoned.into_inner() = Some((key, rows.clone())),
        }
        rows
    }

    pub(crate) fn install(&mut self, id: SlotId, slot: BaseSlot) {
        if let Some(place) = self.slots.get_mut(id.index()) {
            *place = Some(slot);
        }
    }

    pub(crate) fn remove(&mut self, id: SlotId) -> Option<BaseSlot> {
        self.slots.get_mut(id.index()).and_then(Option::take)
    }

    pub(crate) fn set_current(&mut self, id: SlotId) {
        self.current = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_ids_are_nine_with_the_clipbase_last() {
        assert_eq!(SlotId::all().count(), 9);
        assert_eq!(SlotId::from_number(9), Some(SlotId::CLIPBASE));
        assert_eq!(SlotId::from_number(0), None);
        assert_eq!(SlotId::from_number(10), None);
        assert_eq!(SlotId::new(0).map(SlotId::number), Some(1));
        assert!(SlotId::CLIPBASE.is_clipbase());
    }

    #[test]
    fn a_new_state_has_only_the_current_clipbase() {
        let bases = Bases::default();
        assert_eq!(bases.current(), SlotId::CLIPBASE);
        assert_eq!(bases.iter().count(), 1);
        assert_eq!(bases.first_free(), SlotId::new(0));
        let clip = bases.slot(SlotId::CLIPBASE).expect("clipbase");
        assert_eq!(clip.kind, BaseKind::Clipbase);
        assert!(!clip.read_only);
        assert_eq!(clip.path, None);
        assert_eq!((clip.total(), clip.filtered()), (0, 0));
        assert_eq!(bases.cycle(SlotId::CLIPBASE, true), SlotId::CLIPBASE);
    }

    #[test]
    fn clipbase_limit_reaches_the_database() {
        let mut bases = Bases::default();
        assert_eq!(bases.clipbase_limit(), DEFAULT_CLIPBASE_LIMIT);
        bases.set_clipbase_limit(3);
        let slot = bases.slot(SlotId::CLIPBASE).expect("clipbase");
        let limit = slot.source.lock().clipbase_mut().map(|c| c.limit());
        assert_eq!(limit, Some(3));
    }
}
