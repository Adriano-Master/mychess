//! Which navigation buttons are enabled (spec M2-4, D202), after the
//! Scid's `updateBoard2` (`tcl/main.tcl:866-905`).

use crate::game_info::in_variation;
use crate::session::GameSession;

/// The state of the navigation bar, computed from the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // one flag per button, as the bar shows them
pub struct NavState {
    /// The cursor is at the start of the game: "back" and "start" are off.
    pub at_start: bool,
    /// The cursor has no continuation: "forward" and "end" are off.
    pub at_end: bool,
    /// The cursor is inside a variation: "exit variation" is on.
    pub in_variation: bool,
    /// "Add variation" is on: there is a move to hang it from.
    pub can_add_variation: bool,
    /// Alternatives to the main continuation at the cursor (0 when the
    /// continuation is unique): "enter variation" is on when positive.
    pub variation_count: usize,
    /// Trial mode is on.
    pub trial: bool,
    /// Undo would do something.
    pub can_undo: bool,
    /// Redo would do something.
    pub can_redo: bool,
}

impl NavState {
    /// Reads the state of `session`.
    pub fn of(session: &GameSession) -> NavState {
        let game = session.game();
        let cursor = session.cursor();
        let continuations = game.children(cursor).map_or(0, <[_]>::len);
        let at_start = cursor == game.root();
        NavState {
            at_start,
            at_end: continuations == 0,
            in_variation: in_variation(game, cursor),
            can_add_variation: continuations > 0 || !at_start,
            variation_count: continuations.saturating_sub(1),
            trial: session.in_trial(),
            can_undo: session.can_undo(),
            can_redo: session.can_redo(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{GameMessage, Nav};
    use mychess_core::{Game, Position};

    fn session_with_variation() -> GameSession {
        let mut game = Game::new();
        let pos = Position::new();
        let e4 = game
            .add_move(game.root(), pos.parse_uci("e2e4").unwrap())
            .unwrap();
        game.add_move(game.root(), pos.parse_uci("d2d4").unwrap())
            .unwrap();
        game.add_move(e4, game.position_at(e4).unwrap().parse_uci("e7e5").unwrap())
            .unwrap();
        GameSession::new(game)
    }

    #[test]
    fn start_of_a_game_with_a_variation() {
        let session = session_with_variation();
        let nav = NavState::of(&session);
        assert!(nav.at_start && !nav.at_end && !nav.in_variation);
        assert!(nav.can_add_variation);
        assert_eq!(nav.variation_count, 1);
        assert!(!nav.trial && !nav.can_undo && !nav.can_redo);
    }

    #[test]
    fn empty_game_cannot_add_a_variation() {
        let session = GameSession::default();
        let nav = NavState::of(&session);
        assert!(nav.at_start && nav.at_end);
        assert!(!nav.can_add_variation);
        assert_eq!(nav.variation_count, 0);
    }

    #[test]
    fn inside_a_variation_at_its_leaf() {
        let mut session = session_with_variation();
        session.update(GameMessage::Navigate(Nav::EnterVariation(1)));
        let nav = NavState::of(&session);
        assert!(nav.in_variation && nav.at_end && !nav.at_start);
        assert!(nav.can_add_variation);
    }
}
