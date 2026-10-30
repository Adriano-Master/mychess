//! The extra destination of the games of a tournament: the Clipbase or the
//! slot that was current when the tournament started (spec M8-2c,
//! D2517-D2519).
//!
//! The PGN of the tournament always gets the game first (D1279); what happens
//! here is the copy, and a failure never loses the game: the user is told and
//! the tournament goes on. The Clipbase is in memory and takes the game in
//! `update`, like the copy of the editor. An `.si4` slot is written by a
//! short-lived writer in a task, one game at a time, and reopened; the slot
//! changes as after a copy between databases (`games_copied`).

use std::collections::VecDeque;

use mychess_core::Game;
use mychess_db::GameSource;

use super::AppState;
use crate::bases::{BaseKind, SharedSource, SlotId};
use crate::effect::Effect;
use crate::tournament::{ExtraDestination, SlotWrite, TournamentNotice};

/// A game waiting to be written to the slot.
#[derive(Debug)]
struct Waiting {
    number: u32,
    game: Game,
}

/// The state of the extra destination.
#[derive(Debug, Default)]
pub(crate) struct DestFlow {
    /// The slot that gets the copies; `None`: no extra destination.
    slot: Option<SlotId>,
    /// The database the slot held when the tournament started: a slot that
    /// holds another one (closed and opened again) is left alone.
    source: Option<SharedSource>,
    queue: VecDeque<Waiting>,
    busy: bool,
    /// The user was told the destination failed.
    told: bool,
}

impl DestFlow {
    /// Whether a write to the slot is in flight.
    pub(super) fn is_busy(&self) -> bool {
        self.busy
    }

    /// A write to the slot is in flight (tests).
    #[cfg(test)]
    pub(super) fn mark_busy(&mut self) {
        self.busy = true;
    }

    /// Drops the games that wait for the slot (the PGN has them).
    pub(super) fn drop_queue(&mut self) {
        self.queue.clear();
    }

    /// Chooses the slot without the checks of the setup (tests).
    #[cfg(test)]
    pub(super) fn set_slot_for_test(&mut self, id: SlotId) {
        self.slot = Some(id);
    }

    /// A game waits for the slot (tests).
    #[cfg(test)]
    pub(super) fn queue_for_test(&mut self) {
        self.queue.push_back(Waiting {
            number: 1,
            game: Game::new(),
        });
    }

    /// Games wait for the slot (tests).
    #[cfg(test)]
    pub(super) fn has_queue(&self) -> bool {
        !self.queue.is_empty()
    }

    /// The deadline of the quit gave the write in flight up.
    pub(super) fn give_up(&mut self) {
        self.busy = false;
        self.queue.clear();
    }
}

impl AppState {
    /// The slot a tournament copies to, taken **when the tournament is
    /// asked for** (D2519): the slot that is current at that moment, not the
    /// one that is current when the files are ready. `None` when nothing was
    /// asked.
    pub(crate) fn tournament_dest_capture(
        &self,
        wanted: ExtraDestination,
    ) -> Option<(SlotId, SharedSource)> {
        let id = match wanted {
            ExtraDestination::None => return None,
            ExtraDestination::Clipbase => SlotId::CLIPBASE,
            ExtraDestination::CurrentSlot => self.bases.current(),
        };
        self.bases.slot(id).map(|slot| (id, slot.source.clone()))
    }

    /// Chooses the destination when the tournament exists (D2517, D2519).
    /// `captured` is what [`AppState::tournament_dest_capture`] saw at the
    /// start; a slot that is not the same database any more, or none, is
    /// reported once and the games go to the PGN only.
    pub(crate) fn tournament_dest_setup(
        &mut self,
        wanted: ExtraDestination,
        captured: Option<(SlotId, SharedSource)>,
    ) {
        // A write in flight is still out there: its answer must not be taken
        // for the answer of a new one (D2534).
        let busy = self.flow.tournament.dest.busy;
        self.flow.tournament.dest = DestFlow {
            busy,
            ..DestFlow::default()
        };
        if wanted == ExtraDestination::None {
            return;
        }
        let Some((id, source)) = captured else {
            self.dest_notice(TournamentNotice::DestinationUnavailable);
            return;
        };
        let found = self.bases.slot(id).map(|slot| {
            (
                slot.source == source,
                !(slot.read_only || slot.is_locked_read_only() || slot.needs_reload),
            )
        });
        match found {
            Some((true, true)) => {
                self.flow.tournament.dest.slot = Some(id);
                self.flow.tournament.dest.source = Some(source);
            }
            Some((true, false)) => self.dest_notice(TournamentNotice::DestinationReadOnly),
            _ => self.dest_notice(TournamentNotice::DestinationUnavailable),
        }
    }

    fn dest_notice(&mut self, notice: TournamentNotice) {
        self.notice = Some(crate::flow::AppNotice::Tournament(notice));
    }

    /// Copies a game that was saved in the PGN to the destination.
    pub(crate) fn tournament_dest_add(
        &mut self,
        number: u32,
        game: &Game,
        effects: &mut Vec<Effect>,
    ) {
        let Some(id) = self.flow.tournament.dest.slot else {
            return;
        };
        if self.flow.tournament.quitting {
            return;
        }
        if id.is_clipbase() {
            self.tournament_dest_clipbase(id, game);
        } else {
            self.flow.tournament.dest.queue.push_back(Waiting {
                number,
                game: game.clone(),
            });
            self.tournament_dest_pump(effects);
        }
    }

    /// Tells the user a copy failed, once. A worse remark that is on screen
    /// (a game missing from the PGN, a state not saved) is not covered by
    /// it: the PGN has every game anyway (D2517).
    fn tournament_dest_failed(&mut self, notice: TournamentNotice) {
        if self.flow.tournament.dest.told {
            return;
        }
        self.flow.tournament.dest.told = true;
        let graver = matches!(
            self.notice,
            Some(crate::flow::AppNotice::Tournament(
                TournamentNotice::GameNotSaved | TournamentNotice::StateNotSaved
            ))
        );
        if !graver {
            self.dest_notice(notice);
        }
    }

    /// The game enters the Clipbase and its filter, as in the copy of the
    /// editor (D2518).
    fn tournament_dest_clipbase(&mut self, id: SlotId, game: &Game) {
        let same = self.flow.tournament.dest.source.as_ref();
        let pushed = self.bases.slot_mut(id).and_then(|slot| {
            if same.is_some_and(|s| *s != slot.source) {
                return None;
            }
            let pushed = slot
                .source
                .lock()
                .clipbase_mut()
                .map(|clip| clip.push_game(game));
            if matches!(pushed, Some(Ok(_))) {
                slot.filter.append(1);
                slot.clear_sort();
            }
            pushed
        });
        if !matches!(pushed, Some(Ok(_))) {
            self.tournament_dest_failed(TournamentNotice::DestinationFailed);
        }
    }

    /// Stops copying: the queue is dropped (the PGN has the games).
    fn tournament_dest_stop(&mut self, notice: TournamentNotice) {
        self.flow.tournament.dest.slot = None;
        self.flow.tournament.dest.queue.clear();
        self.tournament_dest_failed(notice);
    }

    /// Starts the next write to the `.si4` slot, if none is going on.
    pub(crate) fn tournament_dest_pump(&mut self, effects: &mut Vec<Effect>) {
        if self.flow.tournament.dest.busy {
            return;
        }
        let Some(id) = self.flow.tournament.dest.slot else {
            self.flow.tournament.dest.queue.clear();
            return;
        };
        let Some(next) = self.flow.tournament.dest.queue.pop_front() else {
            return;
        };
        let busy_list = self.flow.list_job.as_ref().is_some_and(|job| {
            job.slot == id || job.target.as_ref().is_some_and(|(t, _)| *t == id)
        });
        let prepared = self.bases.slot(id).and_then(|slot| {
            let same = self.flow.tournament.dest.source.as_ref() == Some(&slot.source);
            let writable = !(slot.read_only || slot.needs_reload);
            (same && writable && slot.kind == BaseKind::Si4 && !busy_list)
                .then(|| {
                    slot.path
                        .clone()
                        .map(|path| (path, slot.total(), slot.stamp))
                })
                .flatten()
        });
        let Some((path, expected_total, stamp)) = prepared else {
            // The slot is not what it was (closed, replaced, busy with a
            // copy): the copy is skipped, the PGN has the game (D2519).
            self.tournament_dest_stop(TournamentNotice::DestinationFailed);
            return;
        };
        self.flow.tournament.dest.busy = true;
        effects.push(Effect::TournamentAddToSlot {
            path,
            expected_total,
            stamp,
            number: next.number,
            game: crate::shared::SharedGame::new(next.game),
        });
    }

    /// The answer of a write to the slot (D2519).
    pub(crate) fn tournament_dest_written(&mut self, result: SlotWrite, effects: &mut Vec<Effect>) {
        self.flow.tournament.dest.busy = false;
        let Some(id) = self.flow.tournament.dest.slot else {
            return;
        };
        match result {
            SlotWrite::Reopened(reloaded) => {
                let same = self.flow.tournament.dest.source.as_ref();
                let swapped = self.bases.slot_mut(id).is_some_and(|slot| {
                    if same != Some(&slot.source) {
                        return false;
                    }
                    let total = GameSource::len(&*reloaded.source.read());
                    let old_total = slot.filter.len();
                    slot.source = reloaded.source.clone();
                    slot.stamp = reloaded.stamp;
                    // The new game enters the filter, like `append(1)` of the
                    // Scid; the sorted view knows nothing of it.
                    let grown = total == old_total.saturating_add(1) && slot.filter.append(1);
                    if !grown {
                        // Another number of games than expected: the file
                        // changed behind our back (D582).
                        slot.filter = mychess_db::Filter::new(total);
                        slot.list.select_only(None);
                    }
                    slot.clear_sort();
                    slot.clamp_first();
                    true
                });
                if swapped {
                    self.flow.tournament.dest.source = Some(reloaded.source);
                } else {
                    self.tournament_dest_stop(TournamentNotice::DestinationFailed);
                    return;
                }
            }
            SlotWrite::WrittenNotReopened(error) => {
                // The game is on disk, but the slot shows the old file: it
                // refuses writes until it is closed and opened (D591).
                tracing::warn!(?error, "the destination cannot be opened again");
                let same = self.flow.tournament.dest.source.as_ref();
                if let Some(slot) = self.bases.slot_mut(id)
                    && same == Some(&slot.source)
                {
                    slot.needs_reload = true;
                }
                self.tournament_dest_stop(TournamentNotice::DestinationNotReopened);
                return;
            }
            SlotWrite::NotWritten(error) => {
                tracing::warn!(?error, "a tournament game could not be copied to the slot");
                self.tournament_dest_stop(TournamentNotice::DestinationFailed);
                return;
            }
        }
        self.tournament_dest_pump(effects);
    }
}
