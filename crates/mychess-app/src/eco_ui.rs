//! The ECO tools of the interface (spec `docs/specs/m5-7-eco-e-novidade.md`,
//! slice M5-7b): the ECO browser, the "Classify games" dialog, "Identify
//! opening" and "Find novelty", over the engine of `mychess_db::eco`.
//!
//! - `message`: [`EcoMessage`], [`EcoNotice`], the choices of the dialogs, the
//!   jobs that [`Effect::EcoStats`](crate::Effect::EcoStats),
//!   [`Effect::EcoClassify`](crate::Effect::EcoClassify) and
//!   [`Effect::EcoNovelty`](crate::Effect::EcoNovelty) carry and their
//!   outcomes;
//! - `ui`: [`EcoUi`], what the three windows hold (the application keeps all
//!   of it, the front-end only draws);
//! - `view`: the ready-to-draw snapshots ([`EcoBrowserView`],
//!   [`EcoClassifyView`], [`EcoNoveltyView`]) and the pure arithmetic of the
//!   bars;
//! - `line`: the game that "import this line" creates from a book entry;
//! - `work`: the blocking work behind the effects.
//!
//! The flow (messages, answers of the list jobs, hooks of `update`) is in
//! `state/eco_flow.rs`. Classifying and the novelty search are jobs of the
//! list (`ListJobKind::EcoClassify` and `EcoNovelty`), so they share the
//! progress, the cancellation, the "one job at a time" rule and the quit
//! handling of the other list jobs.

mod line;
mod message;
mod ui;
mod view;
mod work;

pub(crate) use line::game_from_line;
pub use message::{
    BrowserScope, ClassifyChoice, ClassifyOutcome, ClassifyScope, EcoClassifyJob, EcoMessage,
    EcoNotice, EcoNoveltyJob, NoveltyOutcome, SharedStats,
};
pub(crate) use ui::{Browser, ClassifyDialog, NoveltyDialog};
pub use ui::{ClassifyResult, EcoUi, EcoWindow};
pub(crate) use view::entries_for;
pub use view::{
    BAR_UNITS, EcoBar, EcoBrowserView, EcoClassifyView, EcoEntryRow, EcoGridLine, EcoLevel,
    EcoNoveltyView, bar_parts, grid_lines,
};
pub(crate) use work::{classify_run, novelty_run, stats_run};
