//! The player strips kept in the state (spec M2-5, D231).
//!
//! Same pattern as `notation_cache.rs`: [`AppState::refresh_strips`] runs at
//! the end of every [`update`](super::update). Two levels keep a cursor move
//! cheap:
//!
//! - what depends on the content of the game (names, titles, ratings and the
//!   clock slot, which scans every comment) is rebuilt only when the game
//!   ([`GameSession::changes`](crate::GameSession::changes)) changes;
//! - what depends on the cursor (captured pieces, advantage, clocks, side to
//!   move) is recomputed when the cursor or the material mode changes, by one
//!   walk from the cursor to the root.

use mychess_core::NodeId;

use super::AppState;
use crate::material::MaterialMode;
use crate::player_strip::PlayerStrips;

/// What the identity part was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GameKey {
    /// The root names the game (a replaced game has another root even if the
    /// change counter happens to repeat).
    root: NodeId,
    changes: u64,
}

/// What the cursor part was computed from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CursorKey {
    cursor: NodeId,
    mode: MaterialMode,
}

/// The cached strips.
#[derive(Debug, Default)]
pub(crate) struct StripCache {
    strips: PlayerStrips,
    game_key: Option<GameKey>,
    cursor_key: Option<CursorKey>,
    /// How many times the identity part was rebuilt (for tests).
    identity_builds: u64,
    /// How many times the cursor part was recomputed (for tests).
    cursor_builds: u64,
}

impl AppState {
    /// The player strips for the cursor position and the material mode.
    ///
    /// Up to date after every [`update`](crate::update). If the session was
    /// changed directly (`state.session.update(..)`), call
    /// [`AppState::refresh_strips`] first.
    pub fn player_strips(&self) -> &PlayerStrips {
        &self.strip_cache.strips
    }

    /// Recomputes the strips if what they depend on changed. Cheap (a
    /// comparison) when nothing did.
    pub fn refresh_strips(&mut self) {
        let game = self.session.game();
        let game_key = GameKey {
            root: game.root(),
            changes: self.session.changes(),
        };
        let cursor_key = CursorKey {
            cursor: self.session.cursor(),
            mode: self.prefs.players.material,
        };
        let cache = &mut self.strip_cache;
        let game_changed = cache.game_key.as_ref() != Some(&game_key);
        if !game_changed && cache.cursor_key.as_ref() == Some(&cursor_key) {
            return;
        }
        if game_changed {
            cache.strips = PlayerStrips::identity(game);
            cache.identity_builds += 1;
        }
        cache
            .strips
            .refresh_cursor(game, cursor_key.cursor, cursor_key.mode);
        cache.cursor_builds += 1;
        cache.game_key = Some(game_key);
        cache.cursor_key = Some(cursor_key);
    }
}

#[cfg(test)]
mod tests {
    use mychess_core::{Color, Game, Position, Role};

    use super::*;
    use crate::message::Message;
    use crate::prefs::{PlayersPrefs, PrefChange, PrefsMessage};
    use crate::session::{GameMessage, Nav};
    use crate::state::update;

    fn state_with_game() -> AppState {
        let mut game = Game::new();
        let pos = Position::new();
        let mut node = game.root();
        let mut position = pos;
        for uci in ["e2e4", "d7d5", "e4d5"] {
            let mv = position.parse_uci(uci).expect("uci");
            node = game.add_move(node, mv).expect("add");
            position = game.position_at(node).expect("node").clone();
        }
        game.tags_mut().white = "Alice".to_owned();
        let mut state = AppState::default();
        state.session.replace_game(game);
        state.refresh_strips();
        state
    }

    fn send(state: &mut AppState, message: GameMessage) {
        update(state, Message::Game(message));
    }

    #[test]
    fn new_state_has_default_strips() {
        let state = AppState::default();
        assert_eq!(state.player_strips().white.color, Color::White);
        assert!(!state.player_strips().clock_slot);
    }

    #[test]
    fn replaced_game_is_shown() {
        let state = state_with_game();
        assert_eq!(state.player_strips().white.name.as_deref(), Some("Alice"));
        assert_eq!(state.strip_cache.identity_builds, 1);
    }

    #[test]
    fn moving_the_cursor_recomputes_only_the_cursor_part() {
        let mut state = state_with_game();
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        assert_eq!(state.player_strips().white.captured, vec![(Role::Pawn, 1)]);
        let (identity, cursor) = (
            state.strip_cache.identity_builds,
            state.strip_cache.cursor_builds,
        );
        send(&mut state, GameMessage::Navigate(Nav::Start));
        assert!(state.player_strips().white.captured.is_empty());
        assert_eq!(state.strip_cache.identity_builds, identity);
        assert_eq!(state.strip_cache.cursor_builds, cursor + 1);
        // Nothing changed: no work at all.
        state.refresh_strips();
        assert_eq!(state.strip_cache.cursor_builds, cursor + 1);
    }

    #[test]
    fn editing_the_game_rebuilds_the_identity() {
        let mut state = state_with_game();
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        let before = state.strip_cache.identity_builds;
        let node = state.session.cursor();
        send(
            &mut state,
            GameMessage::SetComment {
                node,
                text: "[%clk 0:01:00]".to_owned(),
            },
        );
        assert_eq!(state.strip_cache.identity_builds, before + 1);
        assert!(state.player_strips().clock_slot);
        assert_eq!(state.player_strips().white.clock, Some(60));
    }

    #[test]
    fn changing_the_material_mode_recomputes_the_cursor_part() {
        let mut state = state_with_game();
        send(&mut state, GameMessage::Navigate(Nav::GameEnd));
        let identity = state.strip_cache.identity_builds;
        let players = PlayersPrefs {
            material: MaterialMode::Off,
            ..PlayersPrefs::default()
        };
        update(
            &mut state,
            Message::Prefs(PrefsMessage::Set(PrefChange::Players(players))),
        );
        assert!(state.player_strips().white.captured.is_empty());
        assert_eq!(state.player_strips().white.advantage, None);
        assert_eq!(state.strip_cache.identity_builds, identity);
    }
}
