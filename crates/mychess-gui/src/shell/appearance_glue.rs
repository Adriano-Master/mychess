//! The appearance window inside the shell (spec M9-3): opening it from the
//! current preferences, applying the draft and its keys.

use iced::Task;
use mychess_app::{Key, KeyInput, Modifiers, NamedKey};

use super::{Message, Shell};
use crate::windows::appearance::{AppearanceDraft, AppearanceMessage, AppearanceUi};

impl Shell {
    /// What the application shows now, as a draft.
    pub(crate) fn appearance_now(&self) -> AppearanceDraft {
        AppearanceDraft::current(&self.app, self.options)
    }

    /// Handles an event of the appearance window. The choices only touch
    /// the draft; `Apply` sends what differs from the preferences and
    /// closes.
    pub(crate) fn update_appearance(&mut self, message: AppearanceMessage) -> Task<Message> {
        match message {
            AppearanceMessage::Open => {
                let now = self.appearance_now();
                self.ui.dialogs.appearance = Some(AppearanceUi::open(now));
            }
            AppearanceMessage::Close => self.ui.dialogs.appearance = None,
            AppearanceMessage::Apply => {
                let Some(window) = self.ui.dialogs.appearance.take() else {
                    return Task::none();
                };
                let changes = window
                    .draft
                    .changes_from(&window.baseline, &self.appearance_now());
                return Task::batch(changes.into_iter().map(|change| self.update(change)));
            }
            other => {
                if let Some(window) = self.ui.dialogs.appearance.as_mut() {
                    window.update(other);
                }
            }
        }
        Task::none()
    }

    /// The keys the window answers itself: `Enter` applies, the up and
    /// down arrows change the tab (D1706). `None` leaves the key to the
    /// common handling of dialogs (`Esc`, `Tab`).
    pub(crate) fn appearance_key(&mut self, input: &KeyInput) -> Option<Task<Message>> {
        if input.modifiers != Modifiers::NONE {
            return None;
        }
        match input.key {
            Key::Named(NamedKey::Enter) => Some(self.update_appearance(AppearanceMessage::Apply)),
            Key::Named(NamedKey::Up | NamedKey::Down) => {
                let delta = if input.key == Key::Named(NamedKey::Up) {
                    -1
                } else {
                    1
                };
                let tab = self.ui.dialogs.appearance.as_ref()?.tab.step(delta);
                Some(self.update_appearance(AppearanceMessage::Tab(tab)))
            }
            _ => None,
        }
    }
}
