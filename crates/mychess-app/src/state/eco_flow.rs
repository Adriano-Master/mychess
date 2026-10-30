//! The flow of the ECO tools (spec M5-7b, D1700-D1749): the browser, the
//! "Classify games" dialog, "Identify opening" and "Find novelty".
//!
//! Everything here is bookkeeping, which `update` may do. The work that reads
//! a database is an [`Effect`]: the statistics table of the browser
//! ([`Effect::EcoStats`]) and, as jobs of the game list, the classification
//! ([`Effect::EcoClassify`]) and the novelty search ([`Effect::EcoNovelty`]);
//! the answers of those two come back as [`crate::ListMessage`]s so that they share
//! the progress, the cancellation and the quit handling of the list jobs.

use mychess_core::PartialDate;
use mychess_db::identify_opening;

use super::AppState;
use crate::bases::{BaseError, BaseKind, ListColumn, ListJobKind, SlotId};
use crate::eco_ui::{
    Browser, BrowserScope, ClassifyChoice, ClassifyDialog, ClassifyOutcome, ClassifyResult,
    ClassifyScope, EcoBar, EcoBrowserView, EcoClassifyJob, EcoClassifyView, EcoLevel, EcoMessage,
    EcoNotice, EcoNoveltyJob, EcoNoveltyView, EcoWindow, NoveltyDialog, NoveltyOutcome, bar_parts,
    entries_for, game_from_line, grid_lines,
};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::files::GameOrigin;
use crate::flow::{AppNotice, Replacement};
use crate::host::PickKind;
use crate::session::{GameMessage, Nav};
use crate::shared::{SharedEco, SharedGame};

/// The deepest node of the ECO tree the browser goes to (`B90a`).
const MAX_PREFIX: usize = 4;

/// Whether `prefix` is a node of the ECO tree: nothing, a letter `A`-`E`,
/// then two digits, then a subcode letter `a`-`z` (D1182).
pub(super) fn valid_prefix(prefix: &str) -> bool {
    if prefix.len() > MAX_PREFIX {
        return false;
    }
    prefix.bytes().enumerate().all(|(i, b)| match i {
        0 => (b'A'..=b'E').contains(&b),
        1 | 2 => b.is_ascii_digit(),
        _ => b.is_ascii_lowercase(),
    })
}

/// The date `years` or a month back from `today` (the day is kept, as the
/// dialog of Scid does: `date:Y-1.M.D`). The day is not clamped to the length
/// of the month: 31/03 a month back is `31/02`, which the date type accepts as
/// partial data and which still selects "the last month" (D1747).
pub(super) fn date_back(today: PartialDate, year: bool) -> Option<PartialDate> {
    let (y, m, d) = (today.year()?, today.month()?, today.day());
    let (y, m) = if year {
        (y.checked_sub(1)?, m)
    } else if m <= 1 {
        (y.checked_sub(1)?, 12)
    } else {
        (y, m - 1)
    };
    PartialDate::new(Some(y), Some(m), d).ok()
}

/// Fills the list of openings of the browser from `book`.
fn refill(browser: &mut Browser, book: Option<&SharedEco>) {
    browser.book = book.cloned();
    if let Some(book) = book.filter(|b| !b.is_empty()) {
        let (rows, truncated) = entries_for(
            book,
            &browser.glossary,
            &browser.prefix,
            browser.finding.as_deref(),
        );
        browser.entries = rows;
        browser.truncated = truncated;
    } else {
        browser.entries.clear();
        browser.truncated = false;
    }
}

impl AppState {
    /// Which window of the ECO tools is open, if one is.
    pub fn eco_window(&self) -> Option<EcoWindow> {
        if self.eco.browser.is_some() {
            Some(EcoWindow::Browser)
        } else if self.eco.classify.is_some() {
            Some(EcoWindow::Classify)
        } else if self.eco.novelty.is_some() {
            Some(EcoWindow::Novelty)
        } else {
            None
        }
    }

    fn eco_notice(&mut self, notice: EcoNotice) {
        self.notice = Some(AppNotice::Eco(notice));
    }

    /// The loaded book, if it has entries.
    fn usable_book(&self) -> Option<SharedEco> {
        self.eco_book.clone().filter(|book| !book.is_empty())
    }

    fn eco_job_running(&self, kind: ListJobKind) -> bool {
        self.list_job().is_some_and(|(running, _)| running == kind)
    }

    /// Cancels the job of the list if it is of `kind`.
    fn eco_cancel(&self, kind: ListJobKind) {
        if let Some((running, control)) = self.list_job()
            && running == kind
        {
            control.cancel();
        }
    }

    /// Closes every window of the ECO tools and cancels their jobs (D1740:
    /// the online lock closes them like the other assistance).
    pub(crate) fn eco_close_all(&mut self) {
        self.eco_cancel(ListJobKind::EcoClassify);
        self.eco_cancel(ListJobKind::EcoNovelty);
        self.eco.close_all();
    }

    /// Handles an [`EcoMessage`].
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(crate) fn eco_message(&mut self, message: EcoMessage, effects: &mut Vec<Effect>) {
        match message {
            EcoMessage::OpenBrowser { glossary } => self.eco_open_browser(glossary, effects),
            EcoMessage::CloseBrowser => self.eco.browser = None,
            EcoMessage::BrowseTo(prefix) => {
                let book = self.eco_book.clone();
                if let Some(browser) = self.eco.browser.as_mut()
                    && valid_prefix(&prefix)
                {
                    browser.prefix = prefix;
                    browser.find.clear();
                    browser.finding = None;
                    refill(browser, book.as_ref());
                }
            }
            EcoMessage::BrowseUp => {
                let book = self.eco_book.clone();
                if let Some(browser) = self.eco.browser.as_mut() {
                    browser.prefix.pop();
                    browser.find.clear();
                    browser.finding = None;
                    refill(browser, book.as_ref());
                }
            }
            EcoMessage::SetFind(text) => {
                if let Some(browser) = self.eco.browser.as_mut() {
                    browser.find = text;
                }
            }
            EcoMessage::SubmitFind => {
                let book = self.eco_book.clone();
                if let Some(browser) = self.eco.browser.as_mut() {
                    let text = browser.find.trim();
                    browser.finding = (!text.is_empty()).then(|| text.to_owned());
                    refill(browser, book.as_ref());
                }
            }
            EcoMessage::SetScope(scope) => {
                if let Some(browser) = self.eco.browser.as_mut()
                    && browser.scope != scope
                {
                    browser.scope = scope;
                    self.eco_build_stats(effects);
                }
            }
            EcoMessage::ImportLine(index) => self.eco_import_line(index, effects),
            EcoMessage::LoadBookFile => self.ask_for_file(PickKind::OpenEco, None, effects),
            EcoMessage::StatsBuilt { request, table } => {
                if let Some(browser) = self.eco.browser.as_mut()
                    && browser.building == Some(request)
                {
                    browser.building = None;
                    browser.table = Some(table);
                }
            }

            EcoMessage::OpenClassify => self.eco_open_classify(effects),
            EcoMessage::CloseClassify => {
                self.eco_cancel(ListJobKind::EcoClassify);
                self.eco.classify = None;
            }
            EcoMessage::SetClassifyChoice(choice) => {
                if let Some(dialog) = self.eco.classify.as_mut() {
                    dialog.choice = choice;
                    dialog.invalid_date = false;
                    dialog.result = None;
                }
            }
            EcoMessage::SetClassifySince(text) => {
                if let Some(dialog) = self.eco.classify.as_mut() {
                    dialog.since = text;
                    dialog.invalid_date = false;
                    dialog.result = None;
                }
            }
            EcoMessage::SetClassifyExtended(extended) => {
                if let Some(dialog) = self.eco.classify.as_mut() {
                    dialog.extended = extended;
                    dialog.result = None;
                }
            }
            EcoMessage::ClassifyPreview => self.eco_start_classify(false, effects),
            EcoMessage::ClassifyRun => self.eco_start_classify(true, effects),

            EcoMessage::OpenNovelty => {
                let reference = self.bases.current();
                self.eco.close_all();
                self.eco.novelty = Some(NoveltyDialog {
                    reference,
                    older_only: false,
                    result: None,
                });
            }
            EcoMessage::CloseNovelty => {
                self.eco_cancel(ListJobKind::EcoNovelty);
                self.eco.novelty = None;
            }
            EcoMessage::SetNoveltyReference(slot) => {
                let exists = self.bases.slot(slot).is_some();
                if let Some(dialog) = self.eco.novelty.as_mut()
                    && exists
                {
                    dialog.reference = slot;
                    dialog.result = None;
                }
            }
            EcoMessage::SetNoveltyOlderOnly(older_only) => {
                if let Some(dialog) = self.eco.novelty.as_mut() {
                    dialog.older_only = older_only;
                    dialog.result = None;
                }
            }
            EcoMessage::FindNovelty => self.eco_start_novelty(effects),

            EcoMessage::Identify => self.eco_identify(effects),
        }
    }

    // ---- the browser ----

    fn eco_open_browser(&mut self, glossary: Vec<(String, String)>, effects: &mut Vec<Effect>) {
        let slot_id = self.bases.current();
        let Some(source) = self.bases.slot(slot_id).map(|slot| slot.source.clone()) else {
            return;
        };
        self.eco.close_all();
        let mut browser = Browser {
            slot: slot_id,
            source,
            prefix: String::new(),
            scope: BrowserScope::WholeBase,
            table: None,
            building: None,
            find: String::new(),
            finding: None,
            glossary,
            entries: Vec::new(),
            truncated: false,
            book: None,
        };
        refill(&mut browser, self.eco_book.as_ref());
        self.eco.browser = Some(browser);
        self.eco_build_stats(effects);
    }

    /// Asks for the statistics table of the database of the browser.
    fn eco_build_stats(&mut self, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        let Some(browser) = self.eco.browser.as_mut() else {
            return;
        };
        let Some(slot) = self.bases.slot(browser.slot) else {
            return;
        };
        let filter = (browser.scope == BrowserScope::Filter).then(|| Box::new(slot.filter.clone()));
        browser.table = None;
        browser.building = Some(request);
        effects.push(Effect::EcoStats {
            request,
            source: slot.source.clone(),
            filter,
        });
    }

    /// Keeps the browser in line with the database and the book of the state
    /// (called after every message): a slot that holds another database (a
    /// classification wrote the file) or a book that was loaded.
    pub(crate) fn refresh_eco(&mut self, effects: &mut Vec<Effect>) {
        let Some(browser) = self.eco.browser.as_mut() else {
            return;
        };
        let Some(slot) = self.bases.slot(browser.slot) else {
            self.eco.browser = None;
            return;
        };
        let changed = slot.source != browser.source;
        if changed {
            browser.source = slot.source.clone();
        }
        if browser.book != self.eco_book {
            refill(browser, self.eco_book.as_ref());
        }
        if changed {
            self.eco_build_stats(effects);
        }
    }

    fn eco_import_line(&mut self, index: usize, effects: &mut Vec<Effect>) {
        let Some(moves) = self
            .eco
            .browser
            .as_ref()
            .and_then(|b| b.entries.get(index))
            .map(|row| row.moves.clone())
        else {
            return;
        };
        let Some(game) = game_from_line(&moves) else {
            self.eco_notice(EcoNotice::LineFailed);
            return;
        };
        // The browser covers the board: it closes, as the game that
        // replaces the old one is what the user wants to see.
        self.eco.close_all();
        self.begin_replace(
            Replacement::Loaded {
                game: SharedGame::new(game),
                origin: GameOrigin::New,
                dirty: true,
                collection: None,
            },
            effects,
        );
        // The line is shown at its end, as Scid's `importMoveList` does.
        if self.discard_prompt.is_none() {
            effects.extend(self.session.update(GameMessage::Navigate(Nav::GameEnd)));
        }
    }

    // ---- identify opening ----

    fn eco_identify(&mut self, effects: &mut Vec<Effect>) {
        if self.modal_open() {
            return;
        }
        if self.annotation_running() {
            self.notice = Some(AppNotice::Annotate(crate::annotate::AnnotateNotice::Busy));
            return;
        }
        let Some(book) = self.usable_book() else {
            self.eco_notice(EcoNotice::NoBook);
            return;
        };
        match identify_opening(&book, self.session.game()) {
            Some(found) => {
                effects.extend(
                    self.session
                        .update(GameMessage::Navigate(Nav::GoToMainlinePly(found.ply))),
                );
                self.eco_notice(EcoNotice::Opening {
                    eco: found.eco.to_string(),
                    name: found.name,
                    ply: found.ply,
                });
            }
            None => self.eco_notice(EcoNotice::NoOpening),
        }
    }

    // ---- classify ----

    fn eco_open_classify(&mut self, effects: &mut Vec<Effect>) {
        let slot = self.bases.current();
        self.eco.close_all();
        let request = self.new_request();
        self.eco.classify = Some(ClassifyDialog {
            slot,
            choice: ClassifyChoice::default(),
            since: String::new(),
            extended: true,
            today: None,
            pending_today: Some(request),
            invalid_date: false,
            result: None,
        });
        effects.push(Effect::QueryToday { request });
    }

    /// The answer of the clock, if the "Classify games" dialog asked for it.
    pub(crate) fn eco_today(&mut self, request: RequestId, date: PartialDate) -> bool {
        let Some(dialog) = self.eco.classify.as_mut() else {
            return false;
        };
        if dialog.pending_today != Some(request) {
            return false;
        }
        dialog.pending_today = None;
        dialog.today = Some(date);
        true
    }

    fn eco_start_classify(&mut self, write: bool, effects: &mut Vec<Effect>) {
        let Some(dialog) = self.eco.classify.as_ref() else {
            return;
        };
        let (slot_id, choice, extended) = (dialog.slot, dialog.choice, dialog.extended);
        let (today, since_text) = (dialog.today, dialog.since.clone());
        let Some(book) = self.usable_book() else {
            self.eco_notice(EcoNotice::NoBook);
            return;
        };
        if self.list_busy() {
            return;
        }
        let Some(slot) = self.bases.slot(slot_id) else {
            return;
        };
        if write && !(slot.kind == BaseKind::Si4 && !slot.read_only && !slot.needs_reload) {
            self.eco_notice(EcoNotice::ReadOnly);
            return;
        }
        let scope = match choice {
            ClassifyChoice::All => Some(ClassifyScope::All),
            ClassifyChoice::NoCode => Some(ClassifyScope::NoCode),
            ClassifyChoice::Filter => Some(ClassifyScope::Filter(Box::new(slot.filter.clone()))),
            ClassifyChoice::LastYear | ClassifyChoice::LastMonth => today
                .and_then(|t| date_back(t, choice == ClassifyChoice::LastYear))
                .map(ClassifyScope::Since),
            ClassifyChoice::Since => {
                let date = PartialDate::parse_lenient(&since_text);
                date.year().map(|_| ClassifyScope::Since(date))
            }
        };
        let Some(scope) = scope else {
            if let Some(dialog) = self.eco.classify.as_mut() {
                dialog.invalid_date = true;
            }
            return;
        };
        let job = EcoClassifyJob {
            source: slot.source.clone(),
            path: slot.path.clone(),
            write,
            stamp: slot.stamp,
            expected_total: slot.total(),
            book,
            scope,
            extended,
        };
        if let Some(dialog) = self.eco.classify.as_mut() {
            dialog.result = None;
        }
        self.start_job(
            slot_id,
            ListJobKind::EcoClassify,
            move |request, control| Effect::EcoClassify {
                request,
                job: Box::new(job),
                control,
            },
            effects,
        );
    }

    /// The answer of a classification (or a count).
    pub(crate) fn list_eco_classified(
        &mut self,
        request: RequestId,
        result: Result<ClassifyOutcome, BaseError>,
    ) {
        let Some(job) = self.finish_job(request) else {
            return;
        };
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => return self.job_failed(error),
        };
        if outcome.write
            && let Some(slot) = self.bases.slot_mut(job.slot)
        {
            match outcome.source {
                Some(source) => {
                    // The database is read again, whatever happened: the
                    // slot shows what is on disk. The codes changed, so an
                    // order by ECO is stale (the number of games is not).
                    slot.source = source;
                    slot.stamp = outcome.stamp;
                    if slot.list.sort.is_some_and(|(c, _)| c == ListColumn::Eco) {
                        slot.clear_sort();
                    }
                    slot.clamp_first();
                }
                // The file changed but cannot be read: the slot keeps the
                // old database and refuses writes until it is opened again.
                None => slot.needs_reload |= outcome.report.written > 0,
            }
        }
        let report = outcome.report;
        if let Some(dialog) = self.eco.classify.as_mut() {
            dialog.result = Some(ClassifyResult {
                preview: !outcome.write,
                report,
                error: outcome.error.clone(),
            });
        }
        self.eco_notice(EcoNotice::Classified {
            classified: report.classified,
            changed: report.changed,
            written: report.written,
            preview: !outcome.write,
            stopped: report.cancelled,
        });
        if let Some(error) = outcome.error
            && error != BaseError::Cancelled
        {
            self.job_failed(error);
        }
    }

    // ---- novelty ----

    fn eco_start_novelty(&mut self, effects: &mut Vec<Effect>) {
        let Some(dialog) = self.eco.novelty.as_ref() else {
            return;
        };
        let (reference, older_only) = (dialog.reference, dialog.older_only);
        if self.session.game().mainline().next().is_none() {
            // Nothing to be new about (D1203).
            if let Some(dialog) = self.eco.novelty.as_mut() {
                dialog.result = Some(NoveltyOutcome::NoNovelty);
            }
            return;
        }
        if self.list_busy() {
            return;
        }
        let Some(slot) = self.bases.slot(reference) else {
            return;
        };
        // The game of the editor is the one that came from this database,
        // when it did: it must not be found "played before" by itself.
        let current_game = if reference == self.bases.current() {
            slot.current_game
        } else {
            None
        };
        let job = EcoNoveltyJob {
            source: slot.source.clone(),
            path: slot.path.clone().filter(|_| slot.kind == BaseKind::Si4),
            game: SharedGame::new(self.session.game().clone()),
            book: self.usable_book(),
            older_only,
            current_game,
        };
        if let Some(dialog) = self.eco.novelty.as_mut() {
            dialog.result = None;
        }
        self.start_job(
            reference,
            ListJobKind::EcoNovelty,
            move |request, control| Effect::EcoNovelty {
                request,
                job: Box::new(job),
                control,
            },
            effects,
        );
    }

    /// The answer of a novelty search: tells the result and, when a novelty
    /// was found, goes to the position before its move (as Scid does).
    pub(crate) fn list_eco_novelty(
        &mut self,
        request: RequestId,
        result: Result<NoveltyOutcome, BaseError>,
        effects: &mut Vec<Effect>,
    ) {
        if self.finish_job(request).is_none() {
            return;
        }
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => return self.job_failed(error),
        };
        let notice = match &outcome {
            NoveltyOutcome::Found {
                ply,
                san,
                move_number,
                color,
            } => {
                // Only while the dialog still covers the game: it cannot
                // have been edited since the search began.
                if self.eco.novelty.is_some() {
                    effects.extend(self.session.update(GameMessage::Navigate(
                        Nav::GoToMainlinePly(ply.saturating_sub(1)),
                    )));
                }
                EcoNotice::Novelty {
                    move_number: *move_number,
                    color: *color,
                    san: san.clone(),
                }
            }
            NoveltyOutcome::NoNovelty => EcoNotice::NoNovelty,
            NoveltyOutcome::NonStandardStart => EcoNotice::NoveltyNonStandard,
            NoveltyOutcome::Interrupted => EcoNotice::NoveltyStopped,
            _ => EcoNotice::NoveltyFailed,
        };
        if let Some(dialog) = self.eco.novelty.as_mut() {
            dialog.result = Some(outcome);
        }
        self.eco_notice(notice);
    }

    // ---- views ----

    /// The browser, ready to draw.
    pub fn eco_browser_view(&self) -> Option<EcoBrowserView<'_>> {
        let browser = self.eco.browser.as_ref()?;
        let slot = self.bases.slot(browser.slot)?;
        let (mut stats, mut classified) = (mychess_db::EcoStats::default(), 0);
        let mut children = Vec::new();
        if let Some(table) = &browser.table {
            stats = table.get(&browser.prefix).unwrap_or_default();
            classified = table.get("").map(|s| s.count).unwrap_or_default();
            children = table.children(&browser.prefix).unwrap_or_default();
        }
        let max = children
            .iter()
            .map(|c| c.stats.count)
            .max()
            .unwrap_or(0)
            .max(1);
        let bars = children
            .into_iter()
            .map(|child| {
                let (height, parts) = bar_parts(&child.stats, max);
                let label = if browser.prefix.len() == 3 {
                    child
                        .prefix
                        .chars()
                        .last()
                        .map(String::from)
                        .unwrap_or_default()
                } else {
                    child.prefix.clone()
                };
                EcoBar {
                    prefix: child.prefix,
                    label,
                    stats: child.stats,
                    height,
                    parts,
                }
            })
            .collect();
        Some(EcoBrowserView {
            slot: browser.slot,
            total_games: slot.total(),
            classified,
            prefix: &browser.prefix,
            level: EcoLevel::of(&browser.prefix),
            stats,
            bars,
            grid: grid_lines(max),
            max,
            entries: &browser.entries,
            truncated: browser.truncated,
            book_loaded: self.eco_book.as_ref().is_some_and(|b| !b.is_empty()),
            building: browser.building.is_some(),
            find: &browser.find,
            finding: browser.finding.as_deref(),
            scope: browser.scope,
            filter_active: slot.filtered() < slot.total(),
        })
    }

    /// The "Classify games" dialog, ready to draw.
    pub fn eco_classify_view(&self) -> Option<EcoClassifyView<'_>> {
        let dialog = self.eco.classify.as_ref()?;
        let slot = self.bases.slot(dialog.slot)?;
        Some(EcoClassifyView {
            slot: dialog.slot,
            total_games: slot.total(),
            filter_games: slot.filtered(),
            choice: dialog.choice,
            since: &dialog.since,
            invalid_date: dialog.invalid_date,
            extended: dialog.extended,
            today_known: dialog.today.is_some(),
            book_loaded: self.eco_book.as_ref().is_some_and(|b| !b.is_empty()),
            can_write: slot.kind == BaseKind::Si4 && !slot.read_only && !slot.needs_reload,
            running: self.eco_job_running(ListJobKind::EcoClassify),
            result: dialog.result.as_ref(),
        })
    }

    /// The "Find novelty" dialog, ready to draw.
    pub fn eco_novelty_view(&self) -> Option<EcoNoveltyView<'_>> {
        let dialog = self.eco.novelty.as_ref()?;
        Some(EcoNoveltyView {
            reference: dialog.reference,
            references: self.bases.iter().map(|(id, _)| id).collect(),
            older_only: dialog.older_only,
            game_date: self.session.game().tags().date,
            has_moves: self.session.game().mainline().next().is_some(),
            running: self.eco_job_running(ListJobKind::EcoNovelty),
            result: dialog.result.as_ref(),
        })
    }

    /// The slot a window of the ECO tools looks at (for the title).
    pub fn eco_slot(&self) -> Option<SlotId> {
        self.eco
            .browser
            .as_ref()
            .map(|b| b.slot)
            .or_else(|| self.eco.classify.as_ref().map(|d| d.slot))
            .or_else(|| self.eco.novelty.as_ref().map(|d| d.reference))
    }
}
