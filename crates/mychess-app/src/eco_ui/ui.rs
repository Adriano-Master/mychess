//! What the three windows of the ECO tools hold (spec M5-7b). The
//! application keeps it all; the front-end draws the snapshots of
//! `view.rs` and sends [`EcoMessage`](super::EcoMessage)s.

use mychess_core::PartialDate;
use mychess_db::ClassifyReport;

use super::message::{BrowserScope, ClassifyChoice, NoveltyOutcome, SharedStats};
use super::view::EcoEntryRow;
use crate::bases::{BaseError, SharedSource, SlotId};
use crate::error::RequestId;
use crate::shared::SharedEco;

/// The ECO browser.
#[derive(Debug)]
pub(crate) struct Browser {
    pub(crate) slot: SlotId,
    /// The database the table was built from (rebuilt when the slot holds
    /// another one, for instance after a classification).
    pub(crate) source: SharedSource,
    /// The node of the ECO tree: `""`, `B`, `B9`, `B90` or `B90a`.
    pub(crate) prefix: String,
    pub(crate) scope: BrowserScope,
    pub(crate) table: Option<SharedStats>,
    /// The request of the table being built.
    pub(crate) building: Option<RequestId>,
    /// The text of the search field.
    pub(crate) find: String,
    /// The search the list shows (`None`: the summary of the node).
    pub(crate) finding: Option<String>,
    pub(crate) glossary: Vec<(String, String)>,
    pub(crate) entries: Vec<EcoEntryRow>,
    pub(crate) truncated: bool,
    /// The book the entries were made from.
    pub(crate) book: Option<SharedEco>,
}

/// How the last run of the classification dialog ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyResult {
    /// It was only a count.
    pub preview: bool,
    /// The counters.
    pub report: ClassifyReport,
    /// Why it stopped early, if it did.
    pub error: Option<BaseError>,
}

/// The "Classify games" dialog.
#[derive(Debug)]
pub(crate) struct ClassifyDialog {
    pub(crate) slot: SlotId,
    pub(crate) choice: ClassifyChoice,
    /// The text of the date of [`ClassifyChoice::Since`].
    pub(crate) since: String,
    pub(crate) extended: bool,
    pub(crate) today: Option<PartialDate>,
    pub(crate) pending_today: Option<RequestId>,
    /// The date typed is not a date (shown until the next change).
    pub(crate) invalid_date: bool,
    pub(crate) result: Option<ClassifyResult>,
}

/// The "Find novelty" dialog.
#[derive(Debug)]
pub(crate) struct NoveltyDialog {
    pub(crate) reference: SlotId,
    pub(crate) older_only: bool,
    pub(crate) result: Option<NoveltyOutcome>,
}

/// A window of the ECO tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcoWindow {
    /// The ECO browser.
    Browser,
    /// The "Classify games" dialog.
    Classify,
    /// The "Find novelty" dialog.
    Novelty,
}

/// The ECO tools: at most one window of each, and only one open at a time
/// (opening one closes the others).
#[derive(Debug, Default)]
pub struct EcoUi {
    pub(crate) browser: Option<Browser>,
    pub(crate) classify: Option<ClassifyDialog>,
    pub(crate) novelty: Option<NoveltyDialog>,
}

impl EcoUi {
    /// Whether one of the windows is open.
    pub fn any_open(&self) -> bool {
        self.browser.is_some() || self.classify.is_some() || self.novelty.is_some()
    }

    pub(crate) fn close_all(&mut self) {
        self.browser = None;
        self.classify = None;
        self.novelty = None;
    }
}
