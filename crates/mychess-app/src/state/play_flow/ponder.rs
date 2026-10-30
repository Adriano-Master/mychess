//! *Ponder* in the game against an engine (spec M8-1d, D2400-D2411): the
//! opponent thinks on the player's time about the reply it expects.
//!
//! The expected reply is the `ponder` move of its `bestmove`. While the
//! player thinks, a `go ponder` search runs on the position after that
//! reply. If the player plays it, the search becomes the real one
//! (`ponderhit`); if not, the real search replaces it and whatever the old
//! one reports is dropped by its id.

use std::time::{Duration, Instant};

use mychess_core::Move;
use mychess_engine::{Bound, EngineCommand, EngineEvent, EngineWarning, Score, SearchId};

use super::{Active, AppState, Stage, Thinking};
use crate::effect::Effect;
use crate::engines::{EngineRunState, RunId};
use crate::play::{PlayPhase, Think};

/// The longest a search of *ponder* may run before it is let go (D2407).
pub(crate) const PONDER_LIMIT: Duration = Duration::from_secs(300);

/// The search that thinks on the player's time.
#[derive(Debug)]
pub(crate) struct Pondering {
    pub(super) tag: u64,
    pub(super) search: Option<SearchId>,
    /// The moves of the game when the search started (before `predicted`).
    base: Vec<Move>,
    /// The reply it expects.
    predicted: Move,
    started: Instant,
    /// The last exact evaluation of the best line (White's point of view).
    score: Option<Score>,
}

impl AppState {
    /// Starts to ponder on `predicted`, the `ponder` move of the `bestmove`
    /// the opponent just gave (D2401, D2403).
    pub(super) fn play_ponder_start(&mut self, predicted: Option<Move>, effects: &mut Vec<Effect>) {
        let Some(predicted) = predicted else {
            return;
        };
        let now = self.play_now();
        let Some(active) = self.flow.play.active.as_ref() else {
            return;
        };
        if !active.restore.ponder || active.pondering.is_some() || active.thinking.is_some() {
            return;
        }
        let run = active.run;
        let Stage::Playing(controller) = &active.stage else {
            return;
        };
        let Some(think) = controller.ponder_think(predicted, now) else {
            return;
        };
        let mut base = think.moves.clone();
        base.pop();
        let alive = self
            .engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        if !alive {
            return;
        }
        let tag = self.play_next_tag();
        let mut request = think.to_request(tag);
        self.tactical_cap(&mut request);
        request.ponder = true;
        if let Some(active) = self.flow.play.active.as_mut() {
            active.pondering = Some(Pondering {
                tag,
                search: None,
                base,
                predicted,
                started: now,
                score: None,
            });
        }
        self.play_send(run, EngineCommand::Search(request), effects);
    }

    /// The controller asks for the real search of the opponent. Returns
    /// whether the *ponder* search took the job (a hit, D2404); on a miss
    /// the *ponder* is forgotten and the caller sends the search (D2405).
    pub(super) fn play_ponder_hit(
        &mut self,
        run: RunId,
        think: &Think,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let now = self.play_now();
        let Some(active) = self.flow.play.active.as_mut() else {
            return false;
        };
        let Some(p) = active.pondering.take() else {
            return false;
        };
        let expected: Vec<Move> = p.base.iter().copied().chain([p.predicted]).collect();
        if think.moves != expected || p.search.is_none() {
            // A miss: the new search replaces this one on the engine.
            self.play_forget_ponder(run, &p);
            return false;
        }
        let (allowed, ceiling) = Self::think_allowance(think);
        active.thinking = Some(Thinking {
            tag: p.tag,
            search: p.search,
            started: now,
            last_event: now,
            allowed,
            ceiling,
            score: p.score,
        });
        self.play_send(run, EngineCommand::PonderHit, effects);
        true
    }

    /// Lets the search of `p` be reported late without effect.
    pub(super) fn play_forget_ponder(&mut self, run: RunId, p: &Pondering) {
        match p.search {
            Some(id) => self.play_remember(run, id),
            None => self.play_remember_tag(p.tag),
        }
    }

    /// Stops the *ponder* search, if any (D2406).
    pub(super) fn play_ponder_drop(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        let (run, taken) = (active.run, active.pondering.take());
        let Some(p) = taken else {
            return;
        };
        self.play_forget_ponder(run, &p);
        let alive = self
            .engine_runs
            .get(run)
            .is_some_and(|r| r.state == EngineRunState::Running);
        if alive {
            self.play_send(run, EngineCommand::Stop, effects);
        }
    }

    /// After every round of outputs: the *ponder* is kept only while the
    /// game is where it started and it is the player's turn (D2406).
    pub(super) fn play_ponder_check(&mut self, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.play.active.as_ref() else {
            return;
        };
        let Some(p) = active.pondering.as_ref() else {
            return;
        };
        let valid = match &active.stage {
            Stage::Playing(c) => {
                matches!(c.phase(), PlayPhase::Player | PlayPhase::Asking(_))
                    && c.played_moves() == p.base.as_slice()
            }
            Stage::Starting { .. } => false,
        };
        if !valid {
            self.play_ponder_drop(effects);
        }
    }

    /// A tick: a *ponder* that outlived [`PONDER_LIMIT`] is let go (D2407).
    pub(super) fn play_ponder_tick(&mut self, at: Instant, effects: &mut Vec<Effect>) {
        let too_long = self
            .flow
            .play
            .active
            .as_ref()
            .and_then(|a| a.pondering.as_ref())
            .is_some_and(|p| at.saturating_duration_since(p.started) > PONDER_LIMIT);
        if too_long {
            self.play_ponder_drop(effects);
        }
    }

    /// A refused search without an id may have been the *ponder* one.
    pub(super) fn ponder_command_failed(&mut self) {
        if let Some(active) = self.flow.play.active.as_mut()
            && active.thinking.is_none()
            && active
                .pondering
                .as_ref()
                .is_some_and(|p| p.search.is_none())
            && let Some(p) = active.pondering.take()
        {
            self.play_remember_tag(p.tag);
        }
    }

    /// The events of the *ponder* search belong to it, not to the analysis
    /// (D2408). `None`: not about it.
    pub(super) fn ponder_event(
        &mut self,
        event: &EngineEvent,
        _effects: &mut Vec<Effect>,
    ) -> Option<bool> {
        let Active {
            pondering,
            thinking,
            ..
        } = self.flow.play.active.as_mut()?;
        let p = pondering.as_mut()?;
        match event {
            EngineEvent::SearchTagged { search, tag } if *tag == p.tag => {
                p.search = Some(*search);
                Some(true)
            }
            EngineEvent::SearchStarted(id) if thinking.is_none() => match p.search {
                None => {
                    p.search = Some(*id);
                    Some(true)
                }
                Some(known) => Some(known == *id),
            },
            EngineEvent::Info(info) if p.search == Some(info.search) => {
                if info.multipv <= 1
                    && let Some((score, Bound::Exact)) = info.score
                {
                    p.score = Some(score);
                }
                Some(true)
            }
            EngineEvent::BestMove(best) if p.search == Some(best.search) => {
                // The engine ended a search of *ponder* on its own: nothing
                // is pondering any more.
                *pondering = None;
                Some(true)
            }
            EngineEvent::Warning(EngineWarning::CommandRefused {
                command: "go" | "position",
                ..
            }) if p.search.is_none() && thinking.is_none() => {
                if let Some(p) = pondering.take() {
                    self.play_remember_tag(p.tag);
                }
                Some(false)
            }
            _ => None,
        }
    }
}
