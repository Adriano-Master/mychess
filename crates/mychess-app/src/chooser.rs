//! The game chooser: the list shown when a PGN has several games (spec
//! M2-2, D125).

use std::sync::Arc;

use crate::files::PgnCollection;

/// State of the game chooser. It holds indices only; the front-end draws a
/// virtual list from `collection.header(index)`.
#[derive(Debug, Clone, PartialEq)]
pub struct GameChooser {
    /// The games.
    pub collection: Arc<PgnCollection>,
    /// The filter text.
    pub filter: String,
    /// Indices of the games that match the filter, in file order.
    pub visible: Vec<usize>,
    /// The highlighted game (an index into the collection).
    pub selected: Option<usize>,
}

/// Events of the game chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChooserMessage {
    /// The filter text changed.
    Filter(String),
    /// Highlights a game (a collection index; ignored when filtered out).
    Select(usize),
    /// Moves the highlight by this many rows among the visible games.
    Move(isize),
    /// Opens a game (double click).
    Open(usize),
    /// Opens the highlighted game (Enter).
    Confirm,
    /// Closes the chooser (Esc).
    Cancel,
}

impl GameChooser {
    /// A chooser on `collection`, nothing filtered, first game highlighted.
    pub fn new(collection: Arc<PgnCollection>) -> GameChooser {
        let visible: Vec<usize> = (0..collection.len()).collect();
        GameChooser {
            selected: visible.first().copied(),
            collection,
            filter: String::new(),
            visible,
        }
    }

    /// Applies the edits that only touch the list. Returns the game to
    /// open for [`Open`](ChooserMessage::Open) and
    /// [`Confirm`](ChooserMessage::Confirm).
    pub(crate) fn update(&mut self, message: &ChooserMessage) -> Option<usize> {
        match message {
            ChooserMessage::Filter(text) => {
                text.clone_into(&mut self.filter);
                self.visible = self.collection.filter(text);
                if self.selected.is_none_or(|s| !self.visible.contains(&s)) {
                    self.selected = self.visible.first().copied();
                }
                None
            }
            ChooserMessage::Select(index) => {
                if self.visible.contains(index) {
                    self.selected = Some(*index);
                }
                None
            }
            ChooserMessage::Move(delta) => {
                let at = self
                    .selected
                    .and_then(|s| self.visible.iter().position(|v| *v == s))
                    .unwrap_or(0);
                let last = self.visible.len().saturating_sub(1);
                let target = at.saturating_add_signed(*delta).min(last);
                self.selected = self.visible.get(target).copied();
                None
            }
            ChooserMessage::Open(index) => (*index < self.collection.len()).then_some(*index),
            ChooserMessage::Confirm => self.selected,
            ChooserMessage::Cancel => None,
        }
    }
}
