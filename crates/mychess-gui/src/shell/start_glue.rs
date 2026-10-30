//! The part of the shell that serves the welcome and the tip of the day
//! (spec M9-4): the layer owns the keyboard.

use iced::Task;
use mychess_app::start::StartMessage;
use mychess_app::{Action, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey};

use super::{Message, Shell};

impl Shell {
    /// Whether the welcome or the tip covers the window.
    pub(super) fn start_layer_open(&self) -> bool {
        self.app.start().is_open()
    }

    /// The keys of the layer: Esc and Enter close it (the welcome starts the
    /// program, the tip closes), the arrows step through the tips, and only
    /// Quit and full screen of the program window act (like the other
    /// layers, D920).
    pub(super) fn start_key(&mut self, input: &KeyInput, text_focused: bool) -> Task<Message> {
        let welcome = self.app.start().welcome().is_some();
        let plain = input.modifiers == Modifiers::NONE;
        let close = if welcome {
            StartMessage::CloseWelcome
        } else {
            StartMessage::CloseTip
        };
        let message = match input.key {
            Key::Named(NamedKey::Escape | NamedKey::Enter) if plain => Some(close),
            Key::Named(NamedKey::Left) if plain && !welcome => Some(StartMessage::PreviousTip),
            Key::Named(NamedKey::Right) if plain && !welcome => Some(StartMessage::NextTip),
            _ => None,
        };
        if let Some(message) = message {
            return self.dispatch(AppMessage::Start(message));
        }
        match self.app.shortcuts.lookup_in(input, text_focused) {
            Some(action @ (Action::Quit | Action::FullScreen)) => self.run_action(action),
            _ => Task::none(),
        }
    }
}
