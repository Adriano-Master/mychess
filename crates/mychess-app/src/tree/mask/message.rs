//! Messages, effects and notices of the tree masks (spec M5-4, D1860 to
//! D1879).

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use mychess_db::Filter;
use mychess_db::tree::mask::{
    FillReport, FillSide, Marker, MarkerSlot, MaskColor, MaskHits, MaskKey, MaskNag, MaskQuery,
    Outline, OutlineBudget, StmError, StmReport, TreeMask,
};

use crate::bases::{OpenControl, SharedSource};
use crate::error::RequestId;
use crate::flow::DiscardChoice;

/// A mask shared between the state and the jobs that read it. The state
/// edits it with `Arc::make_mut`, so a job that holds a copy never sees an
/// edit (D1863).
#[derive(Clone)]
pub struct SharedMask(pub(crate) Arc<TreeMask>);

impl SharedMask {
    /// Wraps a mask.
    pub fn new(mask: TreeMask) -> SharedMask {
        SharedMask(Arc::new(mask))
    }

    /// The mask.
    pub fn mask(&self) -> &TreeMask {
        &self.0
    }
}

impl fmt::Debug for SharedMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print a million positions.
        write!(f, "SharedMask({} positions)", self.0.len())
    }
}

impl PartialEq for SharedMask {
    fn eq(&self, other: &SharedMask) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Why a mask file could not be read or written. Plain data, so a message
/// can carry it; the front-end translates from the variant
/// ([`MaskIssue::message_key`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaskIssue {
    /// The file is not a mask (empty, binary, other text).
    NotAMask,
    /// The file has more than the one `set` command: nothing was executed
    /// (D1801).
    Foreign,
    /// The text is malformed (unbalanced braces...).
    Syntax,
    /// The file is bigger than the limit.
    TooLarge,
    /// The destination is read-only.
    ReadOnly,
    /// The operating system refused (the kind of the error).
    Io(std::io::ErrorKind),
    /// The worker thread failed.
    Internal,
}

impl MaskIssue {
    /// The Fluent key of the reason.
    pub fn message_key(&self) -> &'static str {
        match self {
            MaskIssue::NotAMask => "tree-mask-issue-not-a-mask",
            MaskIssue::Foreign => "tree-mask-issue-foreign",
            MaskIssue::Syntax => "tree-mask-issue-syntax",
            MaskIssue::TooLarge => "tree-mask-issue-too-large",
            MaskIssue::ReadOnly => "tree-mask-issue-read-only",
            MaskIssue::Io(std::io::ErrorKind::NotFound) => "tree-mask-issue-not-found",
            MaskIssue::Io(std::io::ErrorKind::PermissionDenied) => "tree-mask-issue-denied",
            MaskIssue::Io(_) => "tree-mask-issue-io",
            MaskIssue::Internal => "tree-mask-issue-internal",
        }
    }
}

impl From<&StmError> for MaskIssue {
    fn from(error: &StmError) -> MaskIssue {
        match error {
            StmError::Io { source, .. } => MaskIssue::Io(source.kind()),
            StmError::TooLarge { .. } => MaskIssue::TooLarge,
            StmError::ReadOnly { .. } => MaskIssue::ReadOnly,
            StmError::NotAMask => MaskIssue::NotAMask,
            StmError::Syntax { .. } => MaskIssue::Syntax,
            StmError::ForeignCommand { .. } => MaskIssue::Foreign,
            _ => MaskIssue::Internal,
        }
    }
}

/// How far an edit reaches (D1822).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaskScope {
    /// Only the move.
    #[default]
    One,
    /// The move and the moves of the line of the cursor: for every edit
    /// except "remove", the moves that led to the position (back to the
    /// start); for "remove", the moves that follow (to the end of the line).
    /// All of it counts as one change.
    Line,
}

/// The move an edit is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskTarget {
    /// The row `index` of [`TreeView::rows`](crate::TreeView::rows).
    Row(usize),
    /// The row `index` of [`MaskView::only_rows`](crate::MaskView::only_rows).
    MaskOnly(usize),
    /// A move by its text in the position of the cursor (the "add this
    /// move" submenu, which lists [`AppState::mask_legal_moves`](crate::AppState::mask_legal_moves)).
    San(String),
}

/// What an edit does to a move (D1821).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskEdit {
    /// Adds the move to the mask (nothing if it is there).
    Add,
    /// Removes the move.
    Remove,
    /// Sets the comment of the move.
    Comment(String),
    /// Sets the NAG (`None` for none).
    Nag(Option<MaskNag>),
    /// Sets the color.
    Color(MaskColor),
    /// Sets one of the two markers (`None` for none).
    Marker {
        /// Which one.
        slot: MarkerSlot,
        /// The marker.
        marker: Option<Marker>,
    },
}

/// What waits for the answer of the "save the mask?" question (D1817,
/// D1818).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaskPending {
    /// Start a new, empty mask.
    New,
    /// Open this file.
    Open(PathBuf),
    /// Close the mask.
    Close,
    /// Quit the program.
    Quit,
}

/// A mask read from a file.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskLoaded {
    /// The mask.
    pub mask: SharedMask,
    /// What the reader found (D1814).
    pub report: StmReport,
}

/// An event of the masks. Sent inside
/// [`TreeMessage::Mask`](crate::TreeMessage::Mask).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MaskMessage {
    /// A new, empty mask with no file (the file is named by the first
    /// save, D1816). Asks first when the open one is altered.
    New,
    /// Opens a file (`None` asks for it with the file dialog). Asks first
    /// when the open mask is altered.
    Open(Option<PathBuf>),
    /// Writes the mask (asks for a name if it has none).
    Save,
    /// Asks for a name and writes the mask there.
    SaveAs,
    /// Closes the mask. Asks first when it is altered.
    Close,
    /// The answer to the "save the mask?" question
    /// ([`AppState::mask_prompt`](crate::AppState::mask_prompt)).
    Answer(DiscardChoice),
    /// Edits one move of the position of the cursor (D1821, D1822).
    Edit {
        /// The move.
        target: MaskTarget,
        /// What to do.
        edit: MaskEdit,
        /// How far.
        scope: MaskScope,
    },
    /// Sets the comment of the position of the cursor.
    SetPositionComment(String),
    /// A click on a row only the mask has ([`MaskView::only_rows`](crate::MaskView::only_rows)):
    /// plays the move by the normal flow of a move of the user. Nothing for
    /// an illegal entry.
    PlayOnlyRow(usize),
    /// Copies the moves from the start of the game to the cursor into the
    /// mask (D1826); the side is the one that plays the moves.
    FillLine(FillSide),
    /// Copies the whole game into the mask.
    FillGame,
    /// Copies the games of the filter of the base into the mask, in the
    /// background.
    FillFilter(FillSide),
    /// Stops the fill that runs; what was done stays.
    CancelFill,
    /// Searches the mask (D1832).
    Search(Box<MaskQuery>),
    /// Closes the result of the search.
    CloseSearch,
    /// Builds the outline of the lines of the mask from the position of the
    /// cursor (D1833).
    BuildOutline,
    /// Closes the outline.
    CloseOutline,
    /// Opens a new game from the position of a key (the double click of the
    /// search and of the outline); asks first when the game is altered.
    GoTo(MaskKey),
    /// "Show markers" (D1831).
    SetShowMarkers(bool),
    /// "Show tool tips".
    SetTooltips(bool),
    /// "Load the last mask when the tree opens" (D1820).
    SetAutoLoad(bool),
    /// Takes a file out of the recent list.
    RemoveRecent(PathBuf),

    // ---- answers to the effects ----
    /// A file was read.
    Opened {
        /// The request.
        request: RequestId,
        /// The mask, or why not.
        result: Box<Result<MaskLoaded, MaskIssue>>,
    },
    /// A file was written.
    Saved {
        /// The request.
        request: RequestId,
        /// The size of the file, or why not.
        result: Result<u64, MaskIssue>,
    },
    /// The fill with a filter ended.
    FillFinished(Box<MaskFillAnswer>),
    /// A search ended.
    Searched {
        /// The request.
        request: RequestId,
        /// The result.
        hits: Box<MaskHits>,
    },
    /// An outline was built.
    Outlined {
        /// The request.
        request: RequestId,
        /// The result.
        outline: Box<Outline>,
    },
    /// The time the quit waits for the mask is over; carries the
    /// generation of the timer (D1874).
    Deadline(u64),
}

/// The fill with a filter, as the job runs it (D1826).
#[derive(Debug, Clone)]
pub struct MaskFillJob {
    pub(crate) id: u64,
    pub(crate) mask: SharedMask,
    pub(crate) source: SharedSource,
    pub(crate) filter: Filter,
    pub(crate) side: FillSide,
    pub(crate) control: OpenControl,
}

impl MaskFillJob {
    /// Asks the job to stop; it answers with what it did.
    pub fn cancel(&self) {
        self.control.cancel();
    }
}

impl PartialEq for MaskFillJob {
    fn eq(&self, other: &MaskFillJob) -> bool {
        self.id == other.id
    }
}

/// Why a fill with a filter did not run to the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MaskFillIssue {
    /// The filter was made for another number of games.
    FilterSize,
    /// The worker thread failed.
    Internal,
}

/// The answer to a [`MaskFillJob`].
#[derive(Debug, Clone, PartialEq)]
pub struct MaskFillAnswer {
    pub(crate) id: u64,
    /// The mask with what the job added.
    pub(crate) mask: SharedMask,
    pub(crate) result: Result<FillReport, MaskFillIssue>,
}

/// Work for [`perform`](crate::perform): files, and the jobs that scan the
/// mask or the games.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MaskEffect {
    /// Reads a file; the answer is [`MaskMessage::Opened`].
    Read {
        /// The request.
        request: RequestId,
        /// The file.
        path: PathBuf,
    },
    /// Writes a file atomically; the answer is [`MaskMessage::Saved`].
    Write {
        /// The request.
        request: RequestId,
        /// The file.
        path: PathBuf,
        /// The mask as it was when the save was asked.
        mask: SharedMask,
    },
    /// Fills from the games of a filter; the answer is
    /// [`MaskMessage::FillFinished`]. The host must run it.
    Fill {
        /// The job.
        job: Box<MaskFillJob>,
    },
    /// Searches the mask; the answer is [`MaskMessage::Searched`].
    Search {
        /// The request.
        request: RequestId,
        /// The mask.
        mask: SharedMask,
        /// What to look for.
        query: Box<MaskQuery>,
    },
    /// Builds the outline; the answer is [`MaskMessage::Outlined`].
    Outline {
        /// The request.
        request: RequestId,
        /// The mask.
        mask: SharedMask,
        /// Where the lines start.
        position: Box<mychess_core::Position>,
        /// The limits.
        budget: OutlineBudget,
    },
    /// Waits `delay` and answers [`MaskMessage::Deadline`]: the limit of the
    /// wait of the quit (D1874).
    Deadline {
        /// How long.
        delay: std::time::Duration,
        /// Identity of the timer.
        generation: u64,
    },
}

/// A remark about the masks for the status bar, as data (D1866). The
/// arguments the text needs are named in each variant.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MaskNotice {
    /// The file could not be read; the open mask is unchanged. Arguments:
    /// `path`, `reason` (the text of [`MaskIssue::message_key`]).
    OpenFailed {
        /// The file.
        path: PathBuf,
        /// Why.
        issue: MaskIssue,
    },
    /// The file could not be written; the mask stays altered. Arguments:
    /// `path`, `reason`.
    SaveFailed {
        /// The file.
        path: PathBuf,
        /// Why.
        issue: MaskIssue,
    },
    /// The mask was written.
    Saved,
    /// The file was read with remarks. Argument: `count`.
    Warnings {
        /// How many warnings (listed or not).
        count: usize,
    },
    /// There is no open mask.
    NoMask,
    /// Another job of the mask is running; stop it first.
    Busy,
    /// Masks hold standard chess only (D1835).
    NotStandard,
    /// A fill ended. Arguments: `moves`, `games`, `skipped`.
    Filled {
        /// Moves looked at.
        moves: u64,
        /// Games used.
        games: u64,
        /// Games skipped.
        skipped: u64,
        /// The user stopped it.
        cancelled: bool,
    },
    /// A fill could not run (the base is empty or its filter is stale).
    FillUnavailable,
    /// The mask has the largest number of positions; some data was left
    /// out (D1850).
    Full,
    /// The position of a search hit or outline node cannot be shown.
    CannotShowPosition,
    /// The game is too big to be copied into the mask in one step (D1879).
    GameTooLarge,
}

impl MaskNotice {
    /// The Fluent key of the message.
    pub fn message_key(&self) -> &'static str {
        match self {
            MaskNotice::OpenFailed { .. } => "tree-mask-open-failed",
            MaskNotice::SaveFailed { .. } => "tree-mask-save-failed",
            MaskNotice::Saved => "tree-mask-saved",
            MaskNotice::Warnings { .. } => "tree-mask-warnings",
            MaskNotice::NoMask => "tree-mask-no-mask",
            MaskNotice::Busy => "tree-mask-busy",
            MaskNotice::NotStandard => "tree-mask-not-standard",
            MaskNotice::Filled {
                cancelled: false, ..
            } => "tree-mask-filled",
            MaskNotice::Filled {
                cancelled: true, ..
            } => "tree-mask-fill-cancelled",
            MaskNotice::FillUnavailable => "tree-mask-fill-unavailable",
            MaskNotice::Full => "tree-mask-full",
            MaskNotice::CannotShowPosition => "tree-mask-cannot-show",
            MaskNotice::GameTooLarge => "tree-mask-game-too-large",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_come_from_the_file_errors() {
        assert_eq!(MaskIssue::from(&StmError::NotAMask), MaskIssue::NotAMask);
        assert_eq!(
            MaskIssue::from(&StmError::ForeignCommand { offset: 3 }),
            MaskIssue::Foreign
        );
        assert_eq!(
            MaskIssue::from(&StmError::Syntax {
                offset: 1,
                reason: "x"
            }),
            MaskIssue::Syntax
        );
        let io = StmError::Io {
            path: PathBuf::from("m.stm"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        };
        assert_eq!(
            MaskIssue::from(&io),
            MaskIssue::Io(std::io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn a_shared_mask_compares_by_identity_and_prints_small() {
        let a = SharedMask::new(TreeMask::new());
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(a, SharedMask::new(TreeMask::new()));
        assert_eq!(format!("{a:?}"), "SharedMask(0 positions)");
    }
}
