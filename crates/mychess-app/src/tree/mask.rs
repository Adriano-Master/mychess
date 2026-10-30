//! The tree masks in the application (spec
//! `docs/specs/m5-4-mascaras-de-arvore.md`, slice M5-4b, decisions D1860 to
//! D1879): one open mask at a time, with its colors, NAGs, markers and
//! comments laid over the table of the tree.
//!
//! - `prefs`: [`MaskPrefs`] (`[tree_mask]` in `preferences.toml`);
//! - `message`: [`MaskMessage`], the [`MaskEffect`]s and the notices;
//! - `ui`: the state of the masks and the question about saving;
//! - `view`: [`MaskView`], [`MaskCell`] and [`MaskOnlyRow`], the snapshots the
//!   front-end draws;
//! - `run`: the work the effects do.
//!
//! The flow itself is in `state/tree_mask_flow.rs`. The model and the file
//! are in `mychess_db::tree::mask`; the types the front-end needs are
//! re-exported here.

mod message;
mod prefs;
mod run;
mod ui;
mod view;

pub use message::{
    MaskEdit, MaskEffect, MaskFillAnswer, MaskFillIssue, MaskFillJob, MaskIssue, MaskLoaded,
    MaskMessage, MaskNotice, MaskPending, MaskScope, MaskTarget, SharedMask,
};
pub use mychess_db::tree::mask::{
    FillReport, FillSide, MAX_PREVIEW_BYTES, Marker, MarkerSlot, MaskColor, MaskHit, MaskHits,
    MaskKey, MaskNag, MaskQuery, Outline, OutlineNode, OutlineStep, StmReport, StmWarning,
    StmWarningKind,
};
pub use prefs::{MAX_RECENT_MASKS, MaskPrefs};
pub(crate) use run::perform_mask;
pub(crate) use ui::{
    Asked, CachedOnly, MaskSession, MaskUi, PendingRead, PendingWrite, PickPurpose, RunningFill,
    ViewCache,
};
pub use ui::{MaskOutlineState, MaskPrompt, MaskSearchState};
pub(crate) use view::preview;
pub use view::{MaskBusy, MaskCell, MaskJobKind, MaskOnlyRow, MaskView};
