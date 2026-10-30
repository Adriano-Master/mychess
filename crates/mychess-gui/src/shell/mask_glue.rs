//! The part of the shell that serves the masks of the tree (spec M5-4c):
//! the events of the visual state become messages for the application.

use iced::Task;

use super::{Message, Shell};
use crate::windows::mask::MaskUiMessage;

impl Shell {
    /// An event of the context menu, the editors, the search or the outline
    /// of the masks.
    pub(super) fn update_mask(&mut self, message: MaskUiMessage) -> Task<Message> {
        let out = self.ui.dialogs.mask.update(message, &self.app);
        self.run_outcome(out, None)
    }
}
