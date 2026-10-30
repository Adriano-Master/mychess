//! Which "Resign" and "Rematch" the application can run now (spec M8-12,
//! D3100-D3104). The front-end shows one button and one menu item for all
//! the modes of play; this says which mode they act on, without any I/O.

use super::AppState;
use crate::play::PlayPhase;

/// The mode of play a command of the game acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GameMode {
    /// The game against an engine, the tactical game included.
    Engine,
    /// A game on Lichess.
    Online,
    /// A game on FICS.
    Fics,
}

impl AppState {
    /// The mode whose game the user can resign now: a game of his that is
    /// on. Tournaments, trainings and an observed game have no resignation.
    #[must_use]
    pub fn resign_mode(&self) -> Option<GameMode> {
        if self.online.phase.is_live() {
            let game = self.online.game.as_ref()?;
            // A game that has hardly begun is aborted, not resigned.
            return (!game.is_over() && !self.online.can_abort()).then_some(GameMode::Online);
        }
        if self.fics.playing() {
            return Some(GameMode::Fics);
        }
        matches!(
            self.play_phase(),
            Some(
                PlayPhase::Player
                    | PlayPhase::Opponent
                    | PlayPhase::Asking(_)
                    | PlayPhase::Paused { lost: false }
            )
        )
        .then_some(GameMode::Engine)
    }

    /// The mode whose finished game the user can play again now. FICS has
    /// none: its client has no `rematch` command (D3103).
    ///
    /// Anything else that holds the board (a game against an engine that is
    /// on, a tournament, a game on FICS) wins over the finished Lichess
    /// game that is still shown, so the Lichess rematch never competes with
    /// a game in course (D3110).
    #[must_use]
    pub fn rematch_mode(&self) -> Option<GameMode> {
        if self.play_live() || self.tournament_live() || self.fics.playing() {
            return None;
        }
        if self.online.can_rematch() && self.online.game.is_some() {
            return Some(GameMode::Online);
        }
        if self.online.phase.is_busy() {
            return None;
        }
        matches!(self.play_phase(), Some(PlayPhase::Ended(_))).then_some(GameMode::Engine)
    }
}
