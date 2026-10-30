//! The state of the masks (spec M5-4, D1817, D1818, D1863).

use std::path::PathBuf;
use std::sync::Arc;

use mychess_db::tree::mask::{MaskHits, MaskQuery, Outline, StmReport};

use super::message::{MaskPending, SharedMask};
use super::view::MaskCell;
use crate::bases::OpenControl;
use crate::error::RequestId;

/// The open mask: the one at a time of D1817.
#[derive(Debug, Clone)]
pub(crate) struct MaskSession {
    /// Identifies this mask for as long as it is open (it never changes
    /// with the edits, unlike `rev`): a write that comes back for another
    /// mask is not applied to this one (D1875).
    pub(crate) id: u64,
    /// The file; `None` until the first save of a new mask.
    pub(crate) path: Option<PathBuf>,
    pub(crate) mask: SharedMask,
    /// Altered since it was read or written.
    pub(crate) dirty: bool,
    pub(crate) report: Arc<StmReport>,
    /// Grows with every change of the mask: a save that comes back for an
    /// older value leaves the mask altered, and the cache of the view is
    /// valid for one value only.
    pub(crate) rev: u64,
}

impl MaskSession {
    /// The name the question about saving shows.
    pub(crate) fn name(&self) -> Option<String> {
        self.path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
    }
}

/// The question "the mask was altered: save it?" (D1817). The answer is
/// [`MaskMessage::Answer`](crate::MaskMessage::Answer); the front-end shows
/// it as a modal layer and counts it in its `modal_layer_open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskPrompt {
    /// The file name; `None` for a mask that has none yet.
    pub name: Option<String>,
    /// What happens after "Save" or "Don't save".
    pub then: MaskPending,
}

/// A file being read.
#[derive(Debug, Clone)]
pub(crate) struct PendingRead {
    pub(crate) request: RequestId,
    pub(crate) path: PathBuf,
}

/// A file being written (one at a time).
#[derive(Debug, Clone)]
pub(crate) struct PendingWrite {
    pub(crate) request: RequestId,
    /// The mask the write was started for ([`MaskSession::id`]).
    pub(crate) session: u64,
    pub(crate) path: PathBuf,
    pub(crate) rev: u64,
    /// What goes on when the file is written.
    pub(crate) then: Option<MaskPending>,
}

/// A fill with a filter in flight.
#[derive(Debug, Clone)]
pub(crate) struct RunningFill {
    pub(crate) id: u64,
    pub(crate) control: OpenControl,
    pub(crate) cancelling: bool,
}

/// What the file dialog was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PickPurpose {
    /// A mask to open.
    Open,
    /// A name to save under; what goes on after the save.
    Save(Option<MaskPending>),
}

/// The result of the search of the mask (D1832).
#[derive(Debug, Clone, PartialEq)]
pub struct MaskSearchState {
    /// What was asked.
    pub query: MaskQuery,
    /// The answer; `None` while it runs.
    pub hits: Option<Arc<MaskHits>>,
    /// The mask changed after the search ran: run it again (D1878).
    pub stale: bool,
}

/// The outline of the mask (D1833).
#[derive(Debug, Clone, PartialEq)]
pub struct MaskOutlineState {
    /// The answer; `None` while it runs.
    pub outline: Option<Arc<Outline>>,
    /// The mask changed after the outline was built (D1878).
    pub stale: bool,
}

/// A search or outline in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Asked {
    pub(crate) request: RequestId,
    /// The `rev` of the mask the job was started for.
    pub(crate) rev: u64,
}

/// A move of the mask the tree does not have, as the cache keeps it.
#[derive(Debug, Clone)]
pub(crate) struct CachedOnly {
    pub(crate) san: String,
    pub(crate) cell: MaskCell,
    /// The legal move the text names; `None` for an illegal entry.
    pub(crate) mv: Option<mychess_core::Move>,
}

/// What the mask says about the position of the table: computed once per
/// change of the mask, of the table or of the position, never per frame
/// (D1864).
#[derive(Debug, Clone)]
pub(crate) struct ViewCache {
    pub(crate) rev: u64,
    /// The address of the rows it was made for.
    pub(crate) rows_id: usize,
    /// The FEN of the table.
    pub(crate) fen: String,
    /// The SAN of each row (`None` for `[end]` and hidden rows).
    pub(crate) row_sans: Vec<Option<String>>,
    /// The cell of each row.
    pub(crate) cells: Vec<Option<MaskCell>>,
    pub(crate) only: Vec<CachedOnly>,
    pub(crate) position_comment: (String, bool),
}

/// How the quit gets on with the mask (D1873, D1874).
#[derive(Debug, Clone, Default)]
pub(crate) struct QuitState {
    /// The user answered (or there was nothing to ask): do not ask again in
    /// this quit.
    pub(crate) decided: bool,
    /// The quit waits for a job of the mask.
    pub(crate) waiting: bool,
    /// A timer is out.
    pub(crate) deadline_armed: bool,
    pub(crate) deadline_generation: u64,
    /// The wait ended by the timer: the jobs were given up.
    pub(crate) timed_out: bool,
}

/// Everything the masks keep in the state.
#[derive(Debug, Default)]
pub(crate) struct MaskUi {
    pub(crate) session: Option<MaskSession>,
    pub(crate) prompt: Option<MaskPrompt>,
    pub(crate) reading: Option<PendingRead>,
    pub(crate) writing: Option<PendingWrite>,
    pub(crate) fill: Option<RunningFill>,
    pub(crate) next_job: u64,
    pub(crate) next_rev: u64,
    pub(crate) pick: Option<(RequestId, PickPurpose)>,
    pub(crate) search: Option<MaskSearchState>,
    pub(crate) searching: Option<Asked>,
    pub(crate) outline: Option<MaskOutlineState>,
    pub(crate) outlining: Option<Asked>,
    pub(crate) cache: Option<ViewCache>,
    pub(crate) quit: QuitState,
    /// The auto-load was tried for this opening of the panel.
    pub(crate) autoload_tried: bool,
}
