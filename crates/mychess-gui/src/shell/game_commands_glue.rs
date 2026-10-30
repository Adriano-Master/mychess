//! The commands "Resign" and "Rematch" for every mode of play (spec M8-12,
//! D3100-D3104): the button of the notation panel, the item of the Game
//! menu and the ribbon all send one action, and this routes it to the mode
//! that is on. Each mode keeps its own confirmation.

use iced::Task;
use mychess_app::GameMode;

use super::{Message, Shell};
use crate::windows::fics_dialog::FicsUiMessage;
use crate::windows::online_ui::OnlineUiMessage;
use crate::windows::play_dialog::PlayUiMessage;

impl Shell {
    /// Resigns the game of the user in the mode that is on; nothing when
    /// there is none. Against an engine and on Lichess the confirmation of
    /// the mode asks first; FICS has none (D3103).
    pub(super) fn resign_game(&mut self) -> Task<Message> {
        match self.app.resign_mode() {
            Some(GameMode::Engine) => self.update_play(PlayUiMessage::Resign),
            Some(GameMode::Online) => self.update_online(OnlineUiMessage::Resign),
            Some(GameMode::Fics) => self.update_fics(FicsUiMessage::Resign),
            _ => Task::none(),
        }
    }

    /// Plays the finished game again with the colours swapped, in the mode
    /// that was on; nothing when there is none.
    pub(super) fn rematch_game(&mut self) -> Task<Message> {
        match self.app.rematch_mode() {
            Some(GameMode::Engine) => self.update_play(PlayUiMessage::Rematch),
            Some(GameMode::Online) => self.update_online(OnlineUiMessage::Rematch),
            _ => Task::none(),
        }
    }
}
