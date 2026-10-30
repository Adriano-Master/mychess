//! Typed NAG shortcuts applied to the game (spec M2-4, D181).

use std::time::Instant;

use super::{AppState, update};
use crate::effect::Effect;
use crate::message::Message;
use crate::nag_shortcuts::{NagKey, NagStep};
use crate::session::GameMessage;

impl AppState {
    /// When the half-typed NAG sequence times out, if there is one: the
    /// front-end sends [`Message::NagTick`] then (no timer otherwise).
    pub fn nag_sequence_deadline(&self) -> Option<Instant> {
        self.nag_keys.expires_at()
    }

    pub(crate) fn nag_key(&mut self, key: NagKey, at: Instant, effects: &mut Vec<Effect>) {
        if let Some(chars) = self.nag_keys.expire(at) {
            self.replay_keys(&chars, false, effects);
        }
        match self.nag_keys.push(key, at) {
            NagStep::Pending => {}
            NagStep::Fired(nag) => {
                effects.extend(update(self, Message::Game(GameMessage::AddNag(nag))));
            }
            NagStep::Pass { chars, enter } => self.replay_keys(&chars, enter, effects),
        }
    }

    pub(crate) fn nag_tick(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        if let Some(chars) = self.nag_keys.expire(now) {
            self.replay_keys(&chars, false, effects);
        }
    }

    pub(crate) fn nag_interrupt(&mut self, effects: &mut Vec<Effect>) {
        let chars = self.nag_keys.flush();
        self.replay_keys(&chars, false, effects);
    }

    /// Hands keys that were held back to the move entry; `--` is the null
    /// move (no sequence of the Scid starts with two minuses).
    fn replay_keys(&mut self, chars: &str, enter: bool, effects: &mut Vec<Effect>) {
        let mut messages = Vec::new();
        if chars == "--" {
            messages.push(GameMessage::PlayNullMove);
        } else {
            messages.extend(chars.chars().map(GameMessage::MoveEntryTyped));
        }
        if enter {
            messages.push(GameMessage::MoveEntryCommit);
        }
        for message in messages {
            effects.extend(update(self, Message::Game(message)));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mychess_core::{Game, Nag, Position};

    use super::*;
    use crate::nag_shortcuts::NAG_SEQUENCE_TIMEOUT;
    use crate::session::Nav;

    fn state_at_e4() -> AppState {
        let mut game = Game::new();
        let e4 = game
            .add_move(game.root(), Position::new().parse_uci("e2e4").unwrap())
            .unwrap();
        let _ = e4;
        let mut state = AppState::default();
        state.session.replace_game(game);
        update(
            &mut state,
            Message::Game(GameMessage::Navigate(Nav::GameEnd)),
        );
        state
    }

    fn key(state: &mut AppState, ch: char, at: Instant) {
        update(
            state,
            Message::NagKey {
                key: NagKey::Char(ch),
                at,
            },
        );
    }

    fn nags(state: &AppState) -> Vec<Nag> {
        let node = state.session.cursor();
        state.session.game().nags(node).unwrap().to_vec()
    }

    #[test]
    fn exclamation_enter_adds_the_nag_and_a_new_one_of_its_group_replaces_it() {
        let mut state = state_at_e4();
        let now = Instant::now();
        key(&mut state, '!', now);
        assert!(state.nag_sequence_deadline().is_some());
        update(
            &mut state,
            Message::NagKey {
                key: NagKey::Enter,
                at: now,
            },
        );
        assert_eq!(nags(&state), [Nag::new(1).unwrap()]);
        assert!(state.nag_sequence_deadline().is_none());
        // D180: `?` replaces `!` (same group).
        key(&mut state, '?', now);
        update(
            &mut state,
            Message::NagKey {
                key: NagKey::Enter,
                at: now,
            },
        );
        assert_eq!(nags(&state), [Nag::new(2).unwrap()]);
        // `+-` is another group and needs no Enter.
        key(&mut state, '+', now);
        key(&mut state, '-', now);
        assert_eq!(nags(&state), [Nag::new(2).unwrap(), Nag::new(18).unwrap()]);
    }

    #[test]
    fn the_nag_changes_the_notation() {
        let mut state = state_at_e4();
        let now = Instant::now();
        key(&mut state, '+', now);
        key(&mut state, '-', now);
        assert!(state.notation().flat_text().contains("1.e4"));
        assert!(state.notation().flat_text().contains("+-"));
    }

    #[test]
    fn two_minuses_play_the_null_move() {
        let mut state = state_at_e4();
        let now = Instant::now();
        key(&mut state, '-', now);
        key(&mut state, '-', now);
        let game = state.session.game();
        let cursor = state.session.cursor();
        assert!(game.mv(cursor).unwrap().is_null());
    }

    #[test]
    fn ordinary_characters_reach_the_move_entry() {
        let mut state = state_at_e4();
        let now = Instant::now();
        key(&mut state, 'e', now);
        assert_eq!(state.session.move_entry().text(), "e");
    }

    #[test]
    fn a_stale_half_typed_sequence_is_replayed_by_the_tick() {
        let mut state = state_at_e4();
        let start = Instant::now();
        key(&mut state, '-', start);
        let deadline = state.nag_sequence_deadline().unwrap();
        assert_eq!(deadline, start + NAG_SEQUENCE_TIMEOUT);
        update(&mut state, Message::NagTick(start + Duration::from_secs(2)));
        assert!(state.nag_sequence_deadline().is_none());
        // Nothing was added and nothing is armed.
        assert!(nags(&state).is_empty());
    }

    #[test]
    fn interrupting_replays_held_keys() {
        let mut state = state_at_e4();
        let now = Instant::now();
        key(&mut state, '!', now);
        update(&mut state, Message::NagInterrupt);
        assert!(state.nag_sequence_deadline().is_none());
        assert!(nags(&state).is_empty());
    }
}
