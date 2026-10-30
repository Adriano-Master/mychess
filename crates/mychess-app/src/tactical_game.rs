//! The tactical game: a game against an engine of limited strength (spec
//! M8-7, D2450-D2499).
//!
//! It is **a variant of the game against an engine** (the `play` module), not a
//! second engine of games: a [`PlayConfig`](crate::PlayConfig) with
//! `tactical = Some(TacticalSetup)`. What this module adds is pure:
//!
//! - `strength`: from one Elo to what the engine is told (`UCI_Elo`,
//!   `Skill Level` or a cap on the depth) and the adjustment of the Elo
//!   after a game (D2452-D2456);
//! - `stats`: the score (D2463);
//! - `start`: a typed FEN and a position drawn from a file or a database
//!   (D2458-D2460);
//! - `prefs`: the `[play.tactical]` table (D2464).
//!
//! The glue with the engine is `state::play_flow::tactical`.

mod prefs;
mod start;
mod stats;
mod strength;

pub use crate::play::TACTICAL_EVENT;
pub use prefs::{MAX_FEN_PREF_CHARS, TacticalPrefs, TacticalStartPref};
pub use start::{
    MAX_FEN_CHARS, MAX_PICK_PLY, MIN_PICK_PLY, PICK_TRIES, PickFailure, PickInput, PickJob,
    PickSource, Picked, StartProblem, parse_start_fen, pick_in_game, playable, run_pick,
};
pub use stats::TacticalStats;
pub use strength::{
    ADJUST_STEPS, DEFAULT_ADJUST_STEP, DEFAULT_TACTICAL_ELO, DEPTH_BY_ELO, EngineCaps,
    MAX_DEPTH_CAP, Outcome, SKILL_ELO_CEIL, SKILL_ELO_FLOOR, StrengthPlan, TACTICAL_ELO_RANGE,
    adjust_elo, clamp_elo, depth_for_elo, plan_strength, skill_for_elo,
};

/// The part of a [`PlayConfig`](crate::PlayConfig) that makes a game
/// tactical (D2450).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TacticalSetup {
    /// The Elo wanted for the engine.
    pub elo: u32,
    /// The step of the change after a game, when the adjustment is on
    /// (D2456).
    pub adjust_step: Option<u32>,
    /// What the engine was told, once the game began and the engine
    /// declared its options (D2452).
    pub plan: Option<StrengthPlan>,
}

impl TacticalSetup {
    /// The setup with the Elo inside its range.
    #[must_use]
    pub fn normalized(mut self) -> TacticalSetup {
        self.elo = clamp_elo(self.elo);
        self.adjust_step = self.adjust_step.map(|s| s.clamp(1, 1000));
        self
    }
}

/// What the front-end shows of a tactical game (D2471).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TacticalView {
    /// The Elo wanted (after a game with the adjustment on, the one of the
    /// next game).
    pub elo: u32,
    /// What the engine was told; `None` while the game starts.
    pub plan: Option<StrengthPlan>,
    /// The step of the adjustment, if it is on.
    pub adjust_step: Option<u32>,
    /// The score.
    pub stats: TacticalStats,
    /// After the game: the Elo before and after the adjustment.
    pub adjusted: Option<(u32, u32)>,
}
