//! Closing the windows when an online game is entered (spec M8-3, D4520).
//!
//! The application counts the entries (`AppState::online_entries`: Lichess
//! and FICS games played, examined, shown from the observed list or watched;
//! a resynchronised game is not an entry). Once per entry the shell sends
//! `Esc` to the topmost layer until none is left, so each layer closes the
//! way it already does for the user. It stops, leaving what is left open,
//! at anything that holds data or work of the user: a question (save,
//! overwrite, copy), a job that runs (import, export, report, maintenance,
//! search) and the windows that only their own buttons close.

use iced::Task;
use mychess_app::{Key, KeyInput, Modifiers, NamedKey};

use super::{Message, Shell};
use crate::widgets::modal;
use crate::windows::dialog_ui::ActiveDialog;

/// More than any stack of layers the program can open.
const MAX_LAYERS: usize = 16;

impl Shell {
    /// Whether the topmost layer must stay: a question about the data of the
    /// user or a job that runs.
    fn online_entry_blocked(&self) -> bool {
        if ActiveDialog::of(&self.app, &self.ui.dialogs)
            .is_some_and(|dialog| dialog.survives_online_entry(&self.app))
        {
            return true;
        }
        self.app.annotation_running()
            || self.app.maintenance_view().working()
            || self.oprep_running()
            || self.app.player_report().is_running()
    }

    /// Closes the windows over the board and gives it the focus.
    pub(super) fn dismiss_for_online_game(&mut self) -> Task<Message> {
        self.ui.show_about = false;
        self.ui.ribbon.flyout = false;
        let escape = KeyInput {
            key: Key::Named(NamedKey::Escape),
            modifiers: Modifiers::NONE,
        };
        let mut tasks = Vec::new();
        for _ in 0..MAX_LAYERS {
            if !self.modal_layer_open() || self.online_entry_blocked() {
                break;
            }
            tasks.push(self.on_key_layers(escape, false, None));
        }
        tasks.push(modal::unfocus_all());
        Task::batch(tasks)
    }
}
