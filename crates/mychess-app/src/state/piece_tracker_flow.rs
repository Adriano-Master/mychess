//! The glue of the piece tracker with [`AppState`] (spec M6-4, slice c): the
//! panel follows the filter and the database of the current slot, counts off
//! the interface thread (debounced) and keeps the partial counts when the
//! user stops it (D3408).

use mychess_db::PieceTrackQuery;

use super::AppState;
use crate::bases::OpenControl;
use crate::effect::Effect;
use crate::flow::AppNotice;
use crate::panel_calc::{FilterStamp, Step};
use crate::piece_tracker::{
    DEBOUNCE_MS, PieceTrackerEffect, PieceTrackerFailure, PieceTrackerMessage, PieceTrackerNotice,
    PieceTrackerPrefs, PieceTrackerView, TrackKey, build_view, initial_piece, sister_squares,
};
use crate::prefs::{PrefChange, PrefsMessage};

impl AppState {
    /// The state of the piece tracker.
    pub fn piece_tracker(&self) -> &crate::piece_tracker::PieceTrackerUi {
        &self.tracker
    }

    /// The panel, ready to draw.
    pub fn piece_tracker_view(&self) -> PieceTrackerView {
        let ui = &self.tracker;
        let slot = self.bases.current_slot();
        let result = ui.shown.as_ref().map(|(_, r)| r);
        let stale = ui.open
            && (ui.refresh.running()
                || ui
                    .shown
                    .as_ref()
                    .is_none_or(|(k, _)| ui.refresh.last_key.as_ref() != Some(k)));
        build_view(
            ui.open,
            &self.prefs.ptracker,
            result,
            stale && result.is_some(),
            ui.refresh.progress(),
            slot.is_none_or(|s| s.needs_reload),
            ui.issue,
        )
    }

    /// Stops the count (the program is ending).
    pub(crate) fn piece_tracker_cancel(&mut self) {
        self.tracker.cancel();
    }

    /// Whether `last` is the key the state asks for, compared in place
    /// (D3421).
    fn tracker_unchanged(&self, last: &TrackKey) -> bool {
        let id = self.bases.current();
        let Some(slot) = self.bases.slot(id).filter(|s| !s.needs_reload) else {
            return false;
        };
        let prefs = &self.prefs.ptracker;
        last.slot == id
            && last.source == slot.source
            && last.len == slot.total()
            && last.filter == FilterStamp::of(slot)
            && last.squares == prefs.squares
            && last.start_move == prefs.start_move
            && last.end_move == prefs.end_move
            && last.mode == prefs.mode
    }

    fn tracker_key(&self) -> Option<TrackKey> {
        let id = self.bases.current();
        let slot = self.bases.slot(id).filter(|s| !s.needs_reload)?;
        let prefs = &self.prefs.ptracker;
        Some(TrackKey {
            slot: id,
            source: slot.source.clone(),
            len: slot.total(),
            filter: FilterStamp::of(slot),
            squares: prefs.squares.clone(),
            start_move: prefs.start_move,
            end_move: prefs.end_move,
            mode: prefs.mode,
        })
    }

    /// Starts a count if the map on screen is not for the state now. Called
    /// at the end of every `update`.
    pub(crate) fn refresh_piece_tracker(&mut self, effects: &mut Vec<Effect>) {
        if !self.tracker.open {
            return;
        }
        if self
            .tracker
            .refresh
            .is_current(|last| self.tracker_unchanged(last))
        {
            return;
        }
        let key = self.tracker_key();
        let has = self.tracker.shown.is_some();
        match self.tracker.refresh.decide(key.clone(), has) {
            Step::Idle => {}
            Step::Start => {
                if let Some(key) = key.clone() {
                    self.tracker_start(key, effects);
                }
            }
            Step::Debounce => {
                let request = self.new_request();
                self.tracker.refresh.delay = Some(request);
                effects.push(Effect::PieceTracker(PieceTrackerEffect::Delay {
                    request,
                    ms: DEBOUNCE_MS,
                }));
            }
        }
        if key.is_none() {
            self.tracker.shown = None;
            self.tracker.issue = None;
        }
    }

    fn tracker_start(&mut self, key: TrackKey, effects: &mut Vec<Effect>) {
        let Some(filter) = self
            .bases
            .current_slot()
            .map(|s| Box::new(s.filter().clone()))
        else {
            return;
        };
        let request = self.new_request();
        let control = OpenControl::new();
        let query = PieceTrackQuery {
            squares: key.squares.clone(),
            start_move: key.start_move,
            end_move: key.end_move,
            mode: key.mode.engine(),
        };
        self.tracker.issue = None;
        self.tracker
            .refresh
            .begin(request, control.clone(), key.clone());
        effects.push(Effect::PieceTracker(PieceTrackerEffect::Track {
            request,
            source: key.source,
            filter,
            query: Box::new(query),
            control,
        }));
    }

    fn tracker_prefs(
        &mut self,
        change: impl FnOnce(&mut PieceTrackerPrefs),
        effects: &mut Vec<Effect>,
    ) {
        let mut next = (*self.prefs.ptracker).clone();
        change(&mut next);
        let next = next.sanitized();
        if next != *self.prefs.ptracker {
            // A click on the piece, the mode or the window starts at once
            // (D3410, D3423); the colour scheme only paints.
            let old = &self.prefs.ptracker;
            let counts = (&old.squares, old.start_move, old.end_move, old.mode)
                != (&next.squares, next.start_move, next.end_move, next.mode);
            self.prefs_message(PrefsMessage::Set(PrefChange::PieceTracker(next)), effects);
            if counts {
                self.tracker.refresh.immediate = true;
            }
        }
    }

    /// Handles a [`PieceTrackerMessage`].
    pub(crate) fn piece_tracker_message(
        &mut self,
        message: PieceTrackerMessage,
        effects: &mut Vec<Effect>,
    ) {
        match message {
            PieceTrackerMessage::Toggle => {
                let open = !self.tracker.open;
                self.tracker_set_open(open);
            }
            PieceTrackerMessage::Open => self.tracker_set_open(true),
            PieceTrackerMessage::Close => self.tracker_set_open(false),
            PieceTrackerMessage::SelectPiece { square, sister } => {
                if initial_piece(square).is_none() {
                    self.tracker.issue = Some(PieceTrackerNotice::NotAPiece);
                    self.notice = Some(AppNotice::PieceTracker(PieceTrackerNotice::NotAPiece));
                    return;
                }
                let squares = if sister {
                    sister_squares(square)
                } else {
                    vec![square]
                };
                self.tracker_prefs(|p| p.squares = squares, effects);
            }
            PieceTrackerMessage::SetStart(n) => {
                self.tracker_prefs(
                    |p| {
                        p.start_move = n;
                        p.end_move = p.end_move.max(n);
                    },
                    effects,
                );
            }
            PieceTrackerMessage::SetEnd(n) => {
                self.tracker_prefs(|p| p.end_move = n.max(p.start_move), effects);
            }
            PieceTrackerMessage::SetMode(mode) => self.tracker_prefs(|p| p.mode = mode, effects),
            PieceTrackerMessage::SetScheme(scheme) => {
                self.tracker_prefs(|p| p.scheme = scheme, effects);
            }
            PieceTrackerMessage::Refresh => self.tracker.refresh.force(),
            PieceTrackerMessage::Stop => self.tracker.refresh.stop(),
            PieceTrackerMessage::Computed { request, result } => {
                let Some(calc) = self.tracker.refresh.finish(request) else {
                    return;
                };
                match *result {
                    Ok(result) => self.tracker.shown = Some((calc.key, result)),
                    Err(PieceTrackerFailure::FilterSize | PieceTrackerFailure::Internal) => {
                        self.tracker.shown = None;
                        self.tracker.issue = Some(PieceTrackerNotice::Failed);
                    }
                }
            }
            PieceTrackerMessage::DelayElapsed(request) => {
                if self.tracker.open
                    && self.tracker.refresh.delay_elapsed(request)
                    && let Some(key) = self.tracker_key()
                {
                    self.tracker.refresh.last_key = Some(key.clone());
                    self.tracker_start(key, effects);
                }
            }
        }
    }

    fn tracker_set_open(&mut self, open: bool) {
        self.tracker.open = open;
        if open {
            self.tracker.refresh.force();
        } else {
            self.tracker.cancel();
            self.tracker.refresh.last_key = None;
        }
    }
}
