//! The tactical game in the state (spec M8-7, D2450-D2468).
//!
//! The game itself is the one of [`super`]; here live the four things the
//! variant adds: the plan of the strength of the engine, the cap on its
//! depth, the drawing of a start position (an effect, with its own
//! *watchdog*) and the score at the end of the game.

use std::time::{Duration, Instant};

use mychess_engine::{EngineOption, SearchRequest};

use super::{AppState, Stage};
use crate::effect::Effect;
use crate::flow::AppNotice;
use crate::play::{PlayConfig, PlayEnd, PlayNotice, StartFrom};
use crate::tactical_game::{
    EngineCaps, Outcome, PickFailure, PickInput, PickJob, PickSource, Picked, StrengthPlan,
    TacticalView, adjust_elo, plan_strength, playable,
};
use crate::tactics::Cancel;

/// How long a drawing of a position may take before it is given up
/// (D2460).
pub(crate) const PICK_WATCHDOG: Duration = Duration::from_secs(30);

/// A drawing of a position that waits for its answer (D2460).
#[derive(Debug)]
pub(crate) struct Picking {
    request: u64,
    /// The game to start with the position drawn.
    config: PlayConfig,
    since: Instant,
    cancel: Cancel,
}

/// What a tactical game keeps beside the configuration.
#[derive(Debug, Default)]
pub(crate) struct TacticalRun {
    /// The deepest search allowed to the engine, when it has no option of
    /// strength (D2454).
    pub(super) depth_cap: Option<u32>,
    /// The Elo before and after the adjustment of the game that ended
    /// (D2456).
    pub(super) adjusted: Option<(u32, u32)>,
}

impl AppState {
    /// What the front-end shows of a tactical game: from the start of the
    /// game until it is closed (D2471). `None` for an ordinary game.
    pub fn play_tactical(&self) -> Option<TacticalView> {
        let active = self.flow.play.active.as_ref()?;
        let setup = active.config.tactical?;
        if !matches!(active.stage, Stage::Playing(_)) {
            return None;
        }
        // `elo` is the one this game was played at; the adjustment for the
        // next one is in `adjusted` (D2456).
        Some(TacticalView {
            elo: setup.elo,
            plan: setup.plan,
            adjust_step: setup.adjust_step,
            stats: self.prefs.play.tactical.stats,
            adjusted: active.tactical.adjusted,
        })
    }

    /// Whether a position is being drawn for a tactical game (D2460).
    pub fn play_picking(&self) -> bool {
        self.flow.play.picking.is_some()
    }

    /// The position of the problem on the board of the tactics training:
    /// where "the problem of the training" starts a tactical game from
    /// (D2459). `None` without a problem.
    pub fn tactical_problem_position(&self) -> Option<mychess_core::Position> {
        self.tactics_problem_position()
    }

    /// Asks for the drawing of a position and waits for it (D2460).
    pub(super) fn tactical_start_pick(
        &mut self,
        config: PlayConfig,
        source: &PickSource,
        effects: &mut Vec<Effect>,
    ) {
        let input = match source {
            PickSource::File(path) => PickInput::File(path.clone()),
            PickSource::CurrentBase => match self.tactics_current_base() {
                Some(opened) if !opened.handle.is_empty() => PickInput::Handle(opened.handle),
                _ => return self.play_notice(PlayNotice::TacticalPickEmpty),
            },
        };
        self.flow.play.request_seq = self.flow.play.request_seq.wrapping_add(1);
        let request = self.flow.play.request_seq;
        let cancel = Cancel::new();
        let seed = self.play_roll();
        self.flow.play.picking = Some(Picking {
            request,
            config,
            since: self.play_now(),
            cancel: cancel.clone(),
        });
        effects.push(Effect::PlayPick {
            request,
            job: PickJob {
                input,
                seed,
                cancel,
            },
        });
    }

    /// The answer of the drawing (D2460): an old one is dropped.
    pub(super) fn tactical_picked(
        &mut self,
        request: u64,
        result: Result<Picked, PickFailure>,
        effects: &mut Vec<Effect>,
    ) {
        if self
            .flow
            .play
            .picking
            .as_ref()
            .is_none_or(|p| p.request != request)
        {
            return;
        }
        let Some(picking) = self.flow.play.picking.take() else {
            return;
        };
        if self.assist_locked() {
            return self.tactical_locked_notice();
        }
        match result {
            Ok(picked) => {
                let mut config = picking.config;
                config.start = StartFrom::Position(Box::new(picked.position));
                self.play_start(config, None, false, effects);
            }
            Err(PickFailure::Empty) => self.play_notice(PlayNotice::TacticalPickEmpty),
            Err(PickFailure::Cancelled) => {}
            Err(_) => self.play_notice(PlayNotice::TacticalPickFailed),
        }
    }

    /// Gives the drawing up: on `Quit`, and when a new game replaces it.
    pub(super) fn tactical_cancel_pick(&mut self) {
        if let Some(picking) = self.flow.play.picking.take() {
            picking.cancel.cancel();
        }
    }

    /// The *watchdog* of the drawing, on each tick (D2460).
    pub(super) fn tactical_pick_tick(&mut self, at: Instant) {
        let late = self
            .flow
            .play
            .picking
            .as_ref()
            .is_some_and(|p| at.saturating_duration_since(p.since) > PICK_WATCHDOG);
        if late {
            self.tactical_cancel_pick();
            self.play_notice(PlayNotice::TacticalPickFailed);
        }
    }

    /// Works out what the engine will be told and records it in the
    /// configuration (D2452). Returns the notice to give, if any.
    pub(super) fn tactical_plan(
        config: &mut PlayConfig,
        options: &[EngineOption],
    ) -> (Option<u32>, Option<PlayNotice>) {
        let Some(setup) = config.tactical.as_mut() else {
            return (None, None);
        };
        let plan = plan_strength(EngineCaps::from_options(options), setup.elo);
        setup.plan = Some(plan);
        config.strength = plan.as_strength();
        let notice = match plan {
            StrengthPlan::Elo { clamped: true, .. } => Some(PlayNotice::TacticalEloClamped),
            StrengthPlan::Depth { .. } => Some(PlayNotice::TacticalDepthLimited),
            _ => None,
        };
        (plan.depth_cap(), notice)
    }

    /// Cuts the depth of a search of the engine when the plan asks for it
    /// (D2454): the cap works next to the limit of the mode.
    pub(super) fn tactical_cap(&self, request: &mut SearchRequest) {
        let Some(cap) = self
            .flow
            .play
            .active
            .as_ref()
            .and_then(|a| a.tactical.depth_cap)
        else {
            return;
        };
        request.limits.depth = Some(request.limits.depth.map_or(cap, |d| d.min(cap)));
    }

    /// The end of the game: the score and the adjustment of the Elo
    /// (D2456, D2463). A game without a result does not count.
    pub(super) fn tactical_ended(&mut self, end: PlayEnd, effects: &mut Vec<Effect>) {
        let Some(active) = self.flow.play.active.as_mut() else {
            return;
        };
        let (Some(setup), Stage::Playing(controller)) = (active.config.tactical, &active.stage)
        else {
            return;
        };
        let outcome = match (end.result(), controller.player_color()) {
            (mychess_core::GameResult::White, mychess_core::Color::White)
            | (mychess_core::GameResult::Black, mychess_core::Color::Black) => Outcome::Win,
            (mychess_core::GameResult::Draw, _) => Outcome::Draw,
            (mychess_core::GameResult::White | mychess_core::GameResult::Black, _) => Outcome::Loss,
            _ => return,
        };
        let faced = setup.plan.map_or(setup.elo, |p| p.effective_elo(setup.elo));
        let next = match setup.adjust_step {
            Some(step) => adjust_elo(setup.elo, outcome, step),
            None => setup.elo,
        };
        active.tactical.adjusted = (next != setup.elo).then_some((setup.elo, next));
        // Through the preferences message, as a delta, so that a game that
        // ends while the file is still being read counts on top of the
        // score of the file (D2463, D2473).
        self.prefs_message(
            crate::prefs::PrefsMessage::Set(crate::prefs::PrefChange::RecordTactical {
                outcome,
                faced,
                new_elo: setup.adjust_step.is_some().then_some(next),
            }),
            effects,
        );
    }

    /// The configuration of a restart: the Elo that the finished game
    /// adjusted (D2456).
    pub(super) fn tactical_restart_config(&self, config: &mut PlayConfig) {
        let next = self
            .flow
            .play
            .active
            .as_ref()
            .and_then(|a| a.tactical.adjusted);
        if let (Some((_, next)), Some(setup)) = (next, config.tactical.as_mut()) {
            setup.elo = next;
        }
    }

    /// The refusal for a start position that has no move (D2458).
    pub(super) fn tactical_position_ok(config: &PlayConfig) -> bool {
        match &config.start {
            StartFrom::Position(position) => playable(position),
            _ => true,
        }
    }

    /// The notice of the lock of the assistance for a start (D2467).
    pub(super) fn tactical_locked_notice(&mut self) {
        self.notice = Some(AppNotice::Online(
            crate::online::OnlineNotice::AssistanceDisabled,
        ));
    }
}
