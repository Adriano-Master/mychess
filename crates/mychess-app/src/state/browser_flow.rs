//! The game browser (spec M3-4, slice 4e; D595 onwards).
//!
//! Everything here is bookkeeping on [`BrowserState`]: reading the games is
//! an [`Effect`] (`ReadBrowserGames`), the answer comes back as
//! [`BrowseMessage::Loaded`] and is dropped if the browser was closed or
//! reopened since (the request id no longer matches). While the browser is
//! open it counts as a modal layer ([`AppState::modal_open`]): the editor
//! and the list do not act under it.

use std::collections::BTreeSet;

use super::AppState;
use crate::bases::{
    BaseError, BaseSlot, BasesNotice, BrowseMessage, BrowseMode, BrowseRead, BrowserContent,
    BrowserState, GameStep, MAX_BROWSER_GAMES, OpenControl, SlotId,
};
use crate::effect::Effect;
use crate::error::RequestId;
use crate::flow::{AppNotice, BrowserLanding};
use crate::session::{GameMessage, Nav};
use mychess_db::GameSource;

impl AppState {
    /// Handles a [`BrowseMessage`].
    pub(crate) fn browse_message(&mut self, message: BrowseMessage, effects: &mut Vec<Effect>) {
        match message {
            // Answers and closing always get in.
            BrowseMessage::Loaded { request, games } => self.browser_loaded(request, games),
            BrowseMessage::AutoplayTick { generation } => self.browser_tick(generation, effects),
            BrowseMessage::Close => self.close_browser(),
            // A question about the game is open on top of the browser: it
            // gets the keys.
            _ if self.other_modal_open() => {}
            BrowseMessage::Open { slot, numbers } => self.open_browser(slot, numbers, effects),
            _ if self.browser.is_none() => {}
            BrowseMessage::Step { game, delta } => {
                if let Some(browser) = &mut self.browser {
                    browser.step(game, delta);
                }
            }
            BrowseMessage::GoToStart { game } => {
                if let Some(browser) = &mut self.browser {
                    browser.go_to(game, 0);
                }
            }
            BrowseMessage::GoToEnd { game } => {
                if let Some(browser) = &mut self.browser {
                    browser.go_to(game, usize::MAX);
                }
            }
            BrowseMessage::GoToPly { game, ply } => {
                if let Some(browser) = &mut self.browser {
                    let ply = usize::try_from(ply).unwrap_or(usize::MAX);
                    browser.go_to(Some(game), ply);
                }
            }
            BrowseMessage::Flip { game } => {
                if let Some(browser) = &mut self.browser {
                    for i in browser.targets(game) {
                        if let Some(board) = browser.games.get_mut(i) {
                            board.flipped = !board.flipped;
                        }
                    }
                }
            }
            BrowseMessage::ToggleAutoplay => self.browser_toggle_autoplay(effects),
            BrowseMessage::GoToGame(step) => self.browser_go_to_game(step, effects),
            BrowseMessage::Focus(index) => {
                if let Some(browser) = &mut self.browser
                    && index < browser.games.len()
                {
                    browser.focus = index;
                }
            }
            BrowseMessage::LoadIntoBoard { game, keep_open } => {
                self.browser_load_into_board(game, keep_open, effects);
            }
            BrowseMessage::ToggleDeleted { game } => self.browser_toggle_deleted(game, effects),
        }
    }

    /// Closes the browser, cancelling the reading in flight.
    pub(crate) fn close_browser(&mut self) {
        if let Some(browser) = self.browser.take() {
            browser.control.cancel();
        }
    }

    /// Whether a job of the list works on `slot`: reading a record takes
    /// the write lock, which a job may hold for long (D570).
    fn list_job_on(&self, slot: SlotId) -> bool {
        self.flow
            .list_job
            .as_ref()
            .is_some_and(|j| j.slot == slot || j.target.as_ref().is_some_and(|(to, _)| *to == slot))
    }

    fn open_browser(&mut self, slot: SlotId, numbers: Vec<u32>, effects: &mut Vec<Effect>) {
        let Some(total) = self.bases.slot(slot).map(BaseSlot::total) else {
            return;
        };
        // A number above the total is the last game (Scid); zero is not a
        // game; repeated ones show once.
        let mut seen = BTreeSet::new();
        let mut wanted: Vec<u32> = numbers
            .into_iter()
            .filter(|n| *n >= 1)
            .map(|n| n.min(total))
            .filter(|n| seen.insert(*n))
            .collect();
        if wanted.is_empty() || total == 0 {
            return;
        }
        if self.list_job_on(slot) {
            self.notice = Some(AppNotice::Bases(BasesNotice::ListBusy));
            return;
        }
        // The list sends at most one game more than the browser shows; the
        // real size of the selection says how many were asked for (D609).
        let selected = self.bases.slot(slot).map_or(0, |s| s.list.selection.len());
        let asked = if wanted.len() > MAX_BROWSER_GAMES {
            wanted.len().max(selected)
        } else {
            wanted.len()
        };
        let left_out = asked.saturating_sub(MAX_BROWSER_GAMES);
        wanted.truncate(MAX_BROWSER_GAMES);
        let mode = if wanted.len() == 1 {
            BrowseMode::Single
        } else {
            BrowseMode::Grid
        };
        self.close_browser();
        self.start_browser_read(slot, mode, &wanted, left_out, false, effects);
        if left_out > 0 {
            self.notice = Some(AppNotice::Bases(BasesNotice::BrowserLimit {
                shown: u32::try_from(wanted.len()).unwrap_or(u32::MAX),
                asked: u32::try_from(asked).unwrap_or(u32::MAX),
            }));
        }
    }

    /// Installs a browser with its boards loading and asks for the games.
    fn start_browser_read(
        &mut self,
        slot: SlotId,
        mode: BrowseMode,
        numbers: &[u32],
        left_out: usize,
        flipped: bool,
        effects: &mut Vec<Effect>,
    ) {
        let Some(source) = self.bases.slot(slot).map(|s| s.source.clone()) else {
            return;
        };
        let request = self.new_request();
        let control = OpenControl::new();
        let mut browser =
            BrowserState::new(slot, mode, numbers, left_out, request, control.clone());
        for board in &mut browser.games {
            board.flipped = flipped;
        }
        self.browser = Some(browser);
        effects.push(Effect::ReadBrowserGames {
            request,
            source,
            numbers: numbers.to_vec(),
            control,
        });
    }

    fn browser_loaded(&mut self, request: RequestId, games: Vec<BrowseRead>) {
        let Some(browser) = &mut self.browser else {
            return;
        };
        if browser.request != request {
            return;
        }
        let Some(slot) = self.bases.slot(browser.slot) else {
            self.close_browser();
            return;
        };
        for (index, read) in games.into_iter().enumerate() {
            // The answer is in the order asked.
            if browser.games.get(index).map(|g| g.number) != Some(read.number) {
                continue;
            }
            let raw = slot.filter.get(read.number);
            browser.install(index, read, raw);
        }
        // A cancelled read leaves boards without a game.
        for board in &mut browser.games {
            if matches!(board.content, BrowserContent::Loading) {
                board.content = BrowserContent::Unreadable(BaseError::GameUnreadable {
                    number: board.number,
                });
            }
        }
    }

    // ---- autoplay ----

    /// A generation no other run of any browser has had (D608).
    fn next_browser_generation(&mut self) -> u64 {
        self.flow.next_browser_generation = self.flow.next_browser_generation.wrapping_add(1);
        self.flow.next_browser_generation
    }

    fn browser_toggle_autoplay(&mut self, effects: &mut Vec<Effect>) {
        let delay = self.session.autoplay().delay;
        let generation = self.next_browser_generation();
        let Some(browser) = &mut self.browser else {
            return;
        };
        if browser.mode != BrowseMode::Single {
            return;
        }
        if browser.autoplay {
            browser.autoplay = false;
            browser.generation = generation;
            return;
        }
        if browser.is_loading() {
            return;
        }
        browser.autoplay = true;
        browser.step(None, 1);
        schedule_or_finish(browser, generation, delay, effects);
    }

    fn browser_tick(&mut self, generation: u64, effects: &mut Vec<Effect>) {
        let delay = self.session.autoplay().delay;
        let next = self.next_browser_generation();
        let Some(browser) = &mut self.browser else {
            return;
        };
        if !browser.autoplay || browser.generation != generation {
            return;
        }
        browser.step(None, 1);
        schedule_or_finish(browser, next, delay, effects);
    }

    // ---- other games of the filter ----

    fn browser_go_to_game(&mut self, step: GameStep, effects: &mut Vec<Effect>) {
        let Some(browser) = &self.browser else {
            return;
        };
        if browser.mode != BrowseMode::Single {
            return;
        }
        let (slot_id, current) = (browser.slot, browser.games.first().map_or(0, |g| g.number));
        let flipped = browser.games.first().is_some_and(|g| g.flipped);
        let Some(slot) = self.bases.slot(slot_id) else {
            return;
        };
        // The filter decides; with no game before or after, the browser
        // stays where it is (Scid `::gbrowser::load`).
        let target = match step {
            GameStep::First => slot.filter.first(),
            GameStep::Last => slot.filter.last(),
            GameStep::Previous => slot.filter.prev_before(current),
            GameStep::Next => slot.filter.next_after(current),
        };
        let Some(target) = target.filter(|n| *n != current && *n >= 1 && *n <= slot.total()) else {
            return;
        };
        if self.list_job_on(slot_id) {
            self.notice = Some(AppNotice::Bases(BasesNotice::ListBusy));
            return;
        }
        self.close_browser();
        self.start_browser_read(slot_id, BrowseMode::Single, &[target], 0, flipped, effects);
    }

    // ---- loading into the main board ----

    fn browser_load_into_board(&mut self, game: usize, keep_open: bool, effects: &mut Vec<Effect>) {
        let Some(browser) = &self.browser else {
            return;
        };
        let Some(ready) = browser.games.get(game).and_then(|g| g.ready()) else {
            return;
        };
        let (slot, number, ply) = (browser.slot, browser.games[game].number, ready.ply());
        let (flipped, browser_id) = (browser.games[game].flipped, browser.request);
        let in_range = self
            .bases
            .slot(slot)
            .is_some_and(|s| number >= 1 && number <= s.total());
        if !in_range {
            return;
        }
        // Like a double click on the list: the base of the game is
        // activated first.
        self.select_slot(slot);
        let landing = BrowserLanding {
            ply,
            close_browser: !keep_open,
            flipped,
            browser: browser_id,
        };
        self.request_base_game(slot, number, Some(landing), effects);
    }

    /// Puts the cursor of the editor at the ply the browser showed, asks
    /// the front-end to turn the main board like the browser's and closes
    /// the browser that asked if it was asked to (called when the game is
    /// in the editor).
    ///
    /// The effects of the navigation are dropped: moving the cursor of a
    /// game never schedules anything unless autoplay runs, and loading a
    /// game stops it.
    pub(crate) fn land_browser_game(&mut self, landing: BrowserLanding) {
        let ply = u32::try_from(landing.ply).unwrap_or(u32::MAX);
        drop(
            self.session
                .update(GameMessage::Navigate(Nav::GoToMainlinePly(ply))),
        );
        self.board_flipped = landing.flipped;
        if landing.close_browser
            && self.browser.as_ref().map(|b| b.request) == Some(landing.browser)
        {
            self.close_browser();
        }
    }

    // ---- the "deleted" mark ----

    fn browser_toggle_deleted(&mut self, game: usize, effects: &mut Vec<Effect>) {
        let Some(browser) = &self.browser else {
            return;
        };
        // The mark is read with the games: wait for them.
        if browser.is_loading() {
            return;
        }
        let Some(number) = browser.games.get(game).map(|g| g.number) else {
            return;
        };
        let slot_id = browser.slot;
        let Some(slot) = self.bases.slot_mut(slot_id) else {
            return;
        };
        // The flag job works on the selection of the list: point it at the
        // game under the pointer for the moment it takes the numbers, then
        // give the selection back.
        let saved = (slot.list.selection.clone(), slot.list.selected);
        slot.list.selection = BTreeSet::from([number]);
        slot.list.selected = Some(number);
        self.list_toggle_deleted(slot_id, effects);
        if let Some(slot) = self.bases.slot_mut(slot_id) {
            slot.list.selection = saved.0;
            slot.list.selected = saved.1;
        }
    }

    /// Reads the "deleted" mark of the boards again from the index of
    /// `slot`, after a flag job reloaded the database.
    pub(crate) fn refresh_browser_flags(&mut self, slot_id: SlotId) {
        let Some(browser) = &mut self.browser else {
            return;
        };
        if browser.slot != slot_id {
            return;
        }
        let Some(slot) = self.bases.slot(slot_id) else {
            return;
        };
        let Some(source) = slot.source.try_read() else {
            // The marks shown stay as they were until the next flag job.
            tracing::warn!(
                slot = slot_id.number(),
                "browser marks not refreshed: database busy"
            );
            return;
        };
        for board in &mut browser.games {
            if let Ok(entry) = source.entry(board.number) {
                board.deleted = entry.flags.contains(mychess_db::si4::GameFlags::DELETE);
            }
        }
    }
}

/// Stops at the end of the game; otherwise schedules the next step.
fn schedule_or_finish(
    browser: &mut BrowserState,
    generation: u64,
    delay: std::time::Duration,
    effects: &mut Vec<Effect>,
) {
    if browser.at_end() {
        browser.autoplay = false;
        browser.generation = generation;
    } else {
        browser.generation = generation;
        effects.push(Effect::BrowserTick {
            generation: browser.generation,
            delay,
        });
    }
}
