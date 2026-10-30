//! The state of the book window (spec M5-6, D1651).

use std::path::PathBuf;

use super::types::{
    BookCancel, BookCreateOptions, BookExportOptions, BookIssue, BookMergeMode, BookNotice,
    BookSide, SharedBook,
};
use crate::bases::OpenControl;
use crate::error::RequestId;
use crate::play::PlayBook;

/// One of the two books of the window.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BookSlot {
    /// The book (shared with the effect that is saving it, if any).
    pub(crate) book: SharedBook,
    /// Grows with every edit, so a save that comes back late is recognised.
    pub(crate) revision: u64,
    /// The book has no file for its contents yet (made from games, merged):
    /// it counts as changed even before the first edit.
    pub(crate) unsaved: bool,
}

impl BookSlot {
    /// A slot for a book just read or just made.
    pub(crate) fn new(book: SharedBook, unsaved: bool) -> BookSlot {
        BookSlot {
            book,
            revision: 0,
            unsaved,
        }
    }

    /// Whether closing the book would lose something.
    pub(crate) fn is_dirty(&self) -> bool {
        self.unsaved || self.book.get().is_dirty()
    }
}

/// A job in flight.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Job {
    Load {
        request: RequestId,
        side: BookSide,
    },
    Opening {
        request: RequestId,
        path: PathBuf,
    },
    Save {
        request: RequestId,
        side: BookSide,
        revision: u64,
    },
    Create {
        request: RequestId,
        side: BookSide,
        cancel: BookCancel,
    },
    Merge {
        request: RequestId,
    },
    Export {
        request: RequestId,
        control: OpenControl,
        /// Stop was asked; the answer has not come yet (D2170).
        stopping: bool,
    },
}

impl Job {
    pub(crate) fn request(&self) -> RequestId {
        match self {
            Job::Load { request, .. }
            | Job::Opening { request, .. }
            | Job::Save { request, .. }
            | Job::Create { request, .. }
            | Job::Merge { request }
            | Job::Export { request, .. } => *request,
        }
    }
}

/// What the window knows (D1651).
#[derive(Debug, Default)]
pub struct BookUi {
    pub(crate) open: bool,
    pub(crate) slots: [Option<BookSlot>; 2],
    /// The opening book of the annotation and the report.
    pub(crate) opening: Option<PlayBook>,
    /// Why the book of the preference could not be read.
    pub(crate) opening_issue: Option<BookIssue>,
    pub(crate) merge_mode: BookMergeMode,
    pub(crate) create: BookCreateOptions,
    /// The options of the export of a branch (D2161).
    pub(crate) export: BookExportOptions,
    pub(crate) notice: Option<BookNotice>,
    pub(crate) jobs: Vec<Job>,
    /// The side whose close waits for the answer of the question.
    pub(crate) confirm_close: Option<BookSide>,
}

impl BookUi {
    /// Whether the panel is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether a job is in flight.
    pub fn is_busy(&self) -> bool {
        !self.jobs.is_empty()
    }

    pub(crate) fn slot(&self, side: BookSide) -> Option<&BookSlot> {
        self.slots[side.index()].as_ref()
    }

    pub(crate) fn slot_mut(&mut self, side: BookSide) -> Option<&mut BookSlot> {
        self.slots[side.index()].as_mut()
    }

    /// Whether the book of `side` has changes that are not saved.
    pub fn is_dirty(&self, side: BookSide) -> bool {
        self.slot(side).is_some_and(BookSlot::is_dirty)
    }

    /// Removes and returns the job of `request`, if it is in flight.
    pub(crate) fn take_job(&mut self, request: RequestId) -> Option<Job> {
        let at = self.jobs.iter().position(|j| j.request() == request)?;
        Some(self.jobs.remove(at))
    }

    /// Whether a job that writes or replaces a book is in flight (save,
    /// load or merge; not the opening-book reader or the creation, which
    /// cannot make a saved book dirty or clean, D1796).
    pub fn is_saving(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| matches!(j, Job::Save { .. } | Job::Load { .. } | Job::Merge { .. }))
    }

    /// Whether a book is being made from games.
    pub fn is_creating(&self) -> bool {
        self.jobs.iter().any(|j| matches!(j, Job::Create { .. }))
    }

    /// The progress `(done, total)` of the export, when one is running.
    pub fn export_progress(&self) -> Option<(u64, u64)> {
        self.jobs.iter().find_map(|j| match j {
            Job::Export { control, .. } => Some(control.progress()),
            _ => None,
        })
    }

    /// Whether an export was asked to stop and has not answered yet.
    pub fn is_export_stopping(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| matches!(j, Job::Export { stopping: true, .. }))
    }

    /// Whether an export job is in flight (also while it stops).
    pub fn is_exporting(&self) -> bool {
        self.export_progress().is_some()
    }

    /// Whether either book has changes that are not saved (the question
    /// before the program quits, D1792).
    pub fn has_unsaved(&self) -> bool {
        BookSide::ALL.iter().any(|&side| self.is_dirty(side))
    }

    /// Whether a job of the same kind for `side` is in flight.
    pub(crate) fn busy_side(&self, side: BookSide) -> bool {
        self.jobs.iter().any(|j| match j {
            Job::Load { side: s, .. } | Job::Save { side: s, .. } | Job::Create { side: s, .. } => {
                *s == side
            }
            Job::Merge { .. } => true,
            Job::Opening { .. } | Job::Export { .. } => false,
        })
    }
}
