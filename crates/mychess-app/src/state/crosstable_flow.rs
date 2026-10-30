//! The glue of the crosstable panel with [`AppState`] (spec M6-4, slice a):
//! the panel follows the game on screen, recomputes when its Event, Site or
//! dates change (debounced), and answers "Set filter", the export and the
//! clipboard.
//!
//! Everything here is bookkeeping, which `update` may do. The selection of
//! the games and the table are the effect [`CrosstableEffect::Compute`],
//! run by `perform` on a blocking thread (D3352).

use super::AppState;
use crate::bases::{OpenControl, SlotId};
use crate::crosstable::{
    CrossKey, CrosstableBody, CrosstableDone, CrosstableEffect, CrosstableFailure,
    CrosstableMessage, CrosstableNotice, CrosstablePrefs, CrosstableSpec, CrosstableView,
    DEBOUNCE_MS, ExportFormat, Shown, TagsRef, is_hex_color, table_view,
};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::AppNotice;
use crate::panel_calc::Step;
use crate::prefs::{PrefChange, PrefsMessage};

impl AppState {
    /// The state of the crosstable panel.
    pub fn crosstable(&self) -> &crate::crosstable::CrosstableUi {
        &self.crosstable
    }

    /// The panel, ready to draw.
    pub fn crosstable_view(&self) -> CrosstableView {
        let ui = &self.crosstable;
        let stale = ui
            .shown
            .as_ref()
            .is_some_and(|s| ui.refresh.last_key.as_ref() != Some(&s.key));
        let body = if !ui.open {
            CrosstableBody::Closed
        } else if let Some(shown) = &ui.shown {
            CrosstableBody::Table(std::sync::Arc::clone(&shown.view))
        } else if let Some(notice) = ui.empty {
            CrosstableBody::Empty(notice)
        } else if ui.refresh.running() {
            CrosstableBody::Computing
        } else {
            // Nothing on show and nothing being computed: only a cancelled
            // computation leaves the panel here (D3422).
            CrosstableBody::Empty(CrosstableNotice::Cancelled)
        };
        CrosstableView {
            body,
            stale,
            progress: ui.refresh.progress(),
            prefs: (*self.prefs.crosstable).clone(),
            labels: ui.labels.clone(),
            notice: ui.notice,
            following: ui.pinned.is_none(),
        }
    }

    /// The database the table on show was computed for (M6-4d: the click on
    /// a result lists the game of *that* database, D3433).
    pub fn crosstable_slot(&self) -> Option<SlotId> {
        self.crosstable.shown.as_ref().map(|s| s.key.slot)
    }

    /// The name proposed for the file of an export: Event and Site of the
    /// table with the extension of the format. `None` without a table.
    pub fn crosstable_export_name(&self, format: ExportFormat) -> Option<String> {
        let shown = self.crosstable.shown.as_ref()?;
        let table = &shown.done.table;
        let stem = crate::files::sanitize_file_name(&format!("{} {}", table.event, table.site));
        Some(format!("{stem}.{}", format.extension()))
    }

    /// The identity of the table on screen (see [`crate::crosstable::CrosstableToken`]).
    pub fn crosstable_token(&self) -> Option<crate::crosstable::CrosstableToken> {
        let shown = self.crosstable.shown.as_ref()?;
        Some(crate::crosstable::CrosstableToken(std::sync::Arc::clone(
            &shown.done.table,
        )))
    }

    /// Writes the table on screen to `path`; the effect carries the shared
    /// table, not a copy (D3451).
    fn crosstable_export(
        &mut self,
        format: ExportFormat,
        path: std::path::PathBuf,
        effects: &mut Vec<Effect>,
    ) {
        let Some(shown) = &self.crosstable.shown else {
            self.crosstable_notice(CrosstableNotice::NoTable);
            return;
        };
        effects.push(Effect::Crosstable(CrosstableEffect::Export {
            path,
            table: std::sync::Arc::clone(&shown.done.table),
            labels: Box::new(self.crosstable.labels.clone()),
            format,
        }));
    }

    /// Stops the computation in flight (the program is ending).
    pub(crate) fn crosstable_cancel(&mut self) {
        self.crosstable.cancel();
    }

    fn crosstable_notice(&mut self, notice: CrosstableNotice) {
        self.crosstable.notice = Some(notice);
        self.notice = Some(AppNotice::Crosstable(notice));
    }

    fn crosstable_prefs(
        &mut self,
        change: impl FnOnce(&mut CrosstablePrefs),
        effects: &mut Vec<Effect>,
    ) {
        let mut prefs = (*self.prefs.crosstable).clone();
        change(&mut prefs);
        let prefs = prefs.sanitized();
        if prefs != *self.prefs.crosstable {
            self.prefs_message(PrefsMessage::Set(PrefChange::Crosstable(prefs)), effects);
            // A change of the options is not a burst of typing: no wait.
            self.crosstable.refresh.immediate = true;
        }
    }

    /// The game of the editor the table of `slot` marks: only when that is
    /// the current database.
    fn crosstable_current(&self, slot: SlotId) -> Option<u32> {
        self.bases
            .slot(slot)
            .filter(|_| slot == self.bases.current())
            .and_then(|s| s.current_game)
    }

    /// Converts the table on show again when the game it marks changed.
    fn crosstable_refresh_view(&mut self) {
        let Some(shown) = &self.crosstable.shown else {
            return;
        };
        let current = self.crosstable_current(shown.key.slot);
        if current != shown.current
            && let Some(shown) = &mut self.crosstable.shown
        {
            shown.view = std::sync::Arc::new(table_view(&shown.done.table, current));
            shown.current = current;
        }
    }

    /// Whether the key asked for last is still what the state asks for:
    /// the same test as building the key and comparing, without the
    /// allocations (D3421).
    fn crosstable_unchanged(&self, last: &CrossKey) -> bool {
        let pinned = self.crosstable.pinned;
        let id = pinned.map_or_else(|| self.bases.current(), |(id, _)| id);
        let Some(slot) = self.bases.slot(id).filter(|s| !s.needs_reload) else {
            return false;
        };
        let same_game = if let Some((_, game)) = pinned {
            last.spec.game == game && last.spec.tags.is_none()
        } else {
            let t = self.session.game().tags();
            slot.current_game == Some(last.spec.game)
                && last.spec.tags.as_ref().is_some_and(|l| {
                    l.event == t.event
                        && l.site == t.site
                        && l.date == t.date
                        && l.event_date == t.event_date
                })
        };
        same_game
            && last.slot == id
            && last.source == slot.source
            && last.len == slot.total()
            && last.data_rev == slot.data_rev()
            && last.spec.options == self.prefs.crosstable.options()
    }

    /// What the table is asked for now, or why there is nothing to ask.
    fn crosstable_key(&self) -> Result<CrossKey, CrosstableNotice> {
        let pinned = self.crosstable.pinned;
        let id = pinned.map_or_else(|| self.bases.current(), |(id, _)| id);
        let slot = self
            .bases
            .slot(id)
            .filter(|s| !s.needs_reload)
            .ok_or(CrosstableNotice::NoGame)?;
        let (game, tags) = if let Some((_, game)) = pinned {
            (game, None)
        } else {
            let game = slot.current_game.ok_or(CrosstableNotice::NoGame)?;
            let t = self.session.game().tags();
            let tags = TagsRef {
                event: t.event.clone(),
                site: t.site.clone(),
                date: t.date,
                event_date: t.event_date,
            };
            (game, Some(tags))
        };
        Ok(CrossKey {
            slot: id,
            source: slot.source.clone(),
            len: slot.total(),
            data_rev: slot.data_rev(),
            spec: CrosstableSpec {
                game,
                tags,
                options: self.prefs.crosstable.options(),
            },
        })
    }

    /// Starts a computation if the table on screen is not the one for the
    /// game now. Called at the end of every `update`.
    pub(crate) fn refresh_crosstable(&mut self, effects: &mut Vec<Effect>) {
        if !self.crosstable.open {
            return;
        }
        self.crosstable_refresh_view();
        let refresh = &self.crosstable.refresh;
        if refresh.is_current(|last| self.crosstable_unchanged(last)) {
            return;
        }
        match self.crosstable_key() {
            Err(notice) => {
                let ui = &mut self.crosstable;
                ui.refresh.decide(None, false);
                ui.shown = None;
                ui.empty = Some(notice);
            }
            Ok(key) => {
                let has_result = self.crosstable.shown.is_some();
                match self
                    .crosstable
                    .refresh
                    .decide(Some(key.clone()), has_result)
                {
                    Step::Idle => {}
                    Step::Start => self.crosstable_start(key, effects),
                    Step::Debounce => {
                        let request = self.new_request();
                        self.crosstable.refresh.delay = Some(request);
                        effects.push(Effect::Crosstable(CrosstableEffect::Delay {
                            request,
                            ms: DEBOUNCE_MS,
                        }));
                    }
                }
            }
        }
    }

    fn crosstable_start(&mut self, key: CrossKey, effects: &mut Vec<Effect>) {
        let request = self.new_request();
        let control = OpenControl::new();
        self.crosstable.empty = None;
        self.crosstable
            .refresh
            .begin(request, control.clone(), key.clone());
        effects.push(Effect::Crosstable(CrosstableEffect::Compute {
            request,
            source: key.source,
            spec: Box::new(key.spec),
            control,
        }));
    }

    /// Handles a [`CrosstableMessage`].
    #[allow(clippy::too_many_lines)] // one flat match over the messages
    pub(crate) fn crosstable_message(
        &mut self,
        message: CrosstableMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            CrosstableMessage::Toggle => {
                let open = !self.crosstable.open;
                self.crosstable_set_open(open);
            }
            CrosstableMessage::Open => self.crosstable_set_open(true),
            CrosstableMessage::Close => self.crosstable_set_open(false),
            CrosstableMessage::Refresh => {
                self.crosstable.pinned = None;
                self.crosstable.refresh.force();
            }
            CrosstableMessage::OpenGame { slot, game } => {
                if self
                    .bases
                    .slot(slot)
                    .is_some_and(|s| game >= 1 && game <= s.total())
                {
                    self.crosstable.pinned = Some((slot, game));
                    self.crosstable_set_open(true);
                } else {
                    self.crosstable_notice(CrosstableNotice::InvalidGame);
                }
            }
            CrosstableMessage::SetFormat(format) => {
                self.crosstable_prefs(|p| p.format = format, effects);
            }
            CrosstableMessage::SetSort(sort) => {
                self.crosstable_prefs(|p| p.sort = sort, effects);
            }
            CrosstableMessage::SetFlag(flag, on) => {
                self.crosstable_prefs(|p| p.set(flag, on), effects);
            }
            CrosstableMessage::SetRowColor(color) => {
                if is_hex_color(&color) {
                    self.crosstable_prefs(|p| p.row_color = color, effects);
                }
            }
            CrosstableMessage::SetLabels(labels) => self.crosstable.labels = *labels,
            CrosstableMessage::LoadGame(game) => self.crosstable_load_game(game, effects),
            CrosstableMessage::SetFilter(round) => self.crosstable_filter(round, true),
            CrosstableMessage::AddToFilter(round) => self.crosstable_filter(round, false),
            CrosstableMessage::ExportOf {
                token,
                format,
                path,
            } => {
                if self.crosstable_token().as_ref() == Some(&token) {
                    self.crosstable_export(format, path, effects);
                } else {
                    self.crosstable_notice(CrosstableNotice::ExportStale);
                }
            }
            CrosstableMessage::Export { format, path } => {
                self.crosstable_export(format, path, effects);
            }
            CrosstableMessage::Copy => {
                match self
                    .crosstable
                    .shown
                    .as_ref()
                    .map(|s| std::sync::Arc::clone(&s.done.table))
                {
                    Some(table) => {
                        effects.push(Effect::Crosstable(CrosstableEffect::Render {
                            table,
                            labels: Box::new(self.crosstable.labels.clone()),
                        }));
                    }
                    None => self.crosstable_notice(CrosstableNotice::NoTable),
                }
            }
            CrosstableMessage::Rendered(text) => {
                if !text.is_empty() {
                    effects.push(Effect::WriteClipboard(text));
                    self.crosstable_notice(CrosstableNotice::Copied);
                }
            }
            CrosstableMessage::Exported { ok } => self.crosstable_notice(if ok {
                CrosstableNotice::Exported
            } else {
                CrosstableNotice::ExportFailed
            }),
            CrosstableMessage::Cancel => self.crosstable.user_cancel(),
            CrosstableMessage::Computed { request, result } => {
                self.crosstable_computed(request, *result);
            }
            CrosstableMessage::DelayElapsed(request) => {
                if self.crosstable.open
                    && self.crosstable.refresh.delay_elapsed(request)
                    && let Ok(key) = self.crosstable_key()
                {
                    self.crosstable.refresh.last_key = Some(key.clone());
                    self.crosstable_start(key, effects);
                }
            }
        }
    }

    fn crosstable_set_open(&mut self, open: bool) {
        self.crosstable.open = open;
        if open {
            self.crosstable.refresh.force();
        } else {
            self.crosstable.cancel();
            self.crosstable.refresh.last_key = None;
        }
    }

    fn crosstable_computed(
        &mut self,
        request: RequestId,
        result: Result<CrosstableDone, CrosstableFailure>,
    ) {
        let Some(calc) = self.crosstable.refresh.finish(request) else {
            return;
        };
        let current = self.crosstable_current(calc.key.slot);
        let ui = &mut self.crosstable;
        match result {
            Ok(done) => {
                ui.empty = None;
                ui.shown = Some(Shown {
                    view: std::sync::Arc::new(table_view(&done.table, current)),
                    current,
                    key: calc.key,
                    done,
                });
            }
            Err(CrosstableFailure::Cancelled) => {
                // Stopped from outside (not by the user's Cancel, which
                // drops the answer): no table, no computation (D3422).
                if ui.shown.is_none() {
                    ui.empty = Some(CrosstableNotice::Cancelled);
                }
            }
            Err(failure) => {
                ui.shown = None;
                ui.empty = Some(match failure {
                    CrosstableFailure::NoTable => CrosstableNotice::NoTable,
                    CrosstableFailure::InvalidGame => CrosstableNotice::InvalidGame,
                    _ => CrosstableNotice::Failed,
                });
            }
        }
    }

    /// The click on a result: the game of the table's database goes to the
    /// editor.
    fn crosstable_load_game(&mut self, game: u32, effects: &mut Vec<Effect>) {
        let Some(shown) = &self.crosstable.shown else {
            return;
        };
        if !shown.done.games.iter().any(|(n, _)| *n == game) {
            return;
        }
        let id: SlotId = shown.key.slot;
        self.advanced_load_first(id, game, 0, effects);
    }

    /// "Set filter" and "Add to filter" (D3369): on the database of the
    /// table, even when it is not the current one.
    fn crosstable_filter(&mut self, round: Option<u32>, clear: bool) {
        if self.list_busy() {
            self.crosstable_notice(CrosstableNotice::Busy);
            return;
        }
        let Some(shown) = &self.crosstable.shown else {
            self.crosstable_notice(CrosstableNotice::NoTable);
            return;
        };
        let (id, source, len, data_rev) = (
            shown.key.slot,
            shown.key.source.clone(),
            shown.key.len,
            shown.key.data_rev,
        );
        let numbers: Vec<u32> = shown
            .done
            .games
            .iter()
            .filter(|(_, r)| round.is_none_or(|wanted| *r == wanted))
            .map(|(n, _)| *n)
            .collect();
        let Some(slot) = self.bases.slot_mut(id) else {
            self.crosstable_notice(CrosstableNotice::Stale);
            return;
        };
        if slot.source != source
            || slot.total() != len
            || slot.data_rev() != data_rev
            || slot.needs_reload
        {
            self.crosstable_notice(CrosstableNotice::Stale);
            return;
        }
        if clear {
            slot.filter.fill(0);
        }
        for n in numbers {
            slot.filter.set(n, 1);
        }
        slot.filter_changed();
        slot.list.first = 1;
        self.crosstable_notice(if clear {
            CrosstableNotice::FilterSet
        } else {
            CrosstableNotice::FilterAdded
        });
    }
}

#[cfg(test)]
#[path = "crosstable_flow_tests.rs"]
mod tests;
