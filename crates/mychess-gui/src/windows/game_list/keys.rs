//! The keys and actions of the game list, and the scroll arithmetic (spec
//! M3-4, D541, D564).

use iced::mouse::ScrollDelta;
use mychess_app::{Key, KeyInput, Modifiers, NamedKey};

use super::{MAX_ROWS, ROW_HEIGHT, WHEEL_ROWS};

/// How many whole rows fit in `height` logical pixels (at least one, so
/// the paging keys always move).
pub fn visible_rows(height: f32) -> usize {
    if !height.is_finite() || height <= 0.0 {
        return 1;
    }
    // Finite, positive and clamped before the cast: it is exact.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rows = (height.min(MAX_ROWS as f32 * ROW_HEIGHT) / ROW_HEIGHT).floor() as usize;
    rows.max(1)
}

/// The last position the first row can have so that the list is still full:
/// `max(1, filtered - rows + 1)`. The Scid stops one row earlier
/// (`total - rows`), which leaves the last game out of reach; here the last
/// game can be the last row (D541).
pub fn max_first(filtered: u32, rows: usize) -> u32 {
    let rows = u32::try_from(rows).unwrap_or(u32::MAX);
    filtered.saturating_sub(rows).saturating_add(1).max(1)
}

/// What the wheel did, in whole rows to move (positive: towards the end of
/// the list).
pub fn wheel_rows(delta: ScrollDelta) -> i64 {
    let (amount, per_unit) = match delta {
        ScrollDelta::Lines { y, .. } => (y, WHEEL_ROWS),
        ScrollDelta::Pixels { y, .. } => (y, 1.0 / ROW_HEIGHT),
    };
    if !amount.is_finite() {
        return 0;
    }
    // A finite delta of a mouse: rounded and bounded, so the cast is exact.
    #[allow(clippy::cast_possible_truncation)]
    let rows = (-amount * per_unit).clamp(-1.0e6, 1.0e6).round() as i64;
    rows
}

/// A key the list reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    /// Select the previous game.
    Up,
    /// Select the next game.
    Down,
    /// A page up.
    PageUp,
    /// A page down.
    PageDown,
    /// The start of the list.
    Home,
    /// The end of the list.
    End,
    /// Load the selected game.
    Enter,
    /// Select the visible games (Ctrl/Cmd+A, the Scid's key; D564).
    SelectAll,
    /// Remove the selected games from the filter (Delete).
    Remove,
    /// Toggle the deleted mark of the selected games (Ctrl/Cmd+Delete).
    ToggleDeleted,
    /// Go to the search field (Ctrl/Cmd+F).
    FocusSearch,
    /// Browse the selected games (`B`).
    Browse,
}

impl ListKey {
    /// The list key for a key press, if it is one: the plain keys and the
    /// Ctrl/Cmd chords of the Scid's game list.
    pub fn of(input: &KeyInput) -> Option<ListKey> {
        if input.modifiers == Modifiers::COMMAND {
            return match input.key {
                Key::Char('a') => Some(ListKey::SelectAll),
                Key::Char('f') => Some(ListKey::FocusSearch),
                // The Delete key of the Mac is Backspace.
                Key::Named(NamedKey::Delete | NamedKey::Backspace) => Some(ListKey::ToggleDeleted),
                _ => None,
            };
        }
        if input.modifiers != Modifiers::NONE {
            return None;
        }
        match input.key {
            Key::Named(NamedKey::Up) => Some(ListKey::Up),
            Key::Named(NamedKey::Down) => Some(ListKey::Down),
            Key::Named(NamedKey::PageUp) => Some(ListKey::PageUp),
            Key::Named(NamedKey::PageDown) => Some(ListKey::PageDown),
            Key::Named(NamedKey::Home) => Some(ListKey::Home),
            Key::Named(NamedKey::End) => Some(ListKey::End),
            Key::Named(NamedKey::Enter) => Some(ListKey::Enter),
            // The Delete key of the Mac is Backspace; over the list it is
            // "remove from the filter" too (the board has no use for it
            // there).
            Key::Named(NamedKey::Delete | NamedKey::Backspace) => Some(ListKey::Remove),
            Key::Char('b') => Some(ListKey::Browse),
            _ => None,
        }
    }

    /// Whether the key works only while the pointer is over the list: the
    /// others are free of other meaning, so they work while the panel is
    /// open.
    pub fn needs_pointer(self) -> bool {
        !matches!(self, ListKey::PageUp | ListKey::PageDown)
    }
}

/// An action of the toolbar, the menus and the keys of the list. Each one
/// acts on the current database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListAction {
    /// "Find": the next game that contains the text.
    Find,
    /// "Filter": keep the games that contain the text.
    FilterText,
    /// "Reset": all games in the filter.
    Reset,
    /// "Negate" the filter.
    Negate,
    /// Remove the selected games from the filter.
    RemoveSelected,
    /// Remove the games above this one from the filter.
    RemoveAbove(u32),
    /// Remove the games below this one from the filter.
    RemoveBelow(u32),
    /// "Set filter": keep only the selected games.
    Keep,
    /// Copy the selected games to the Clipbase.
    CopySelected,
    /// Toggle the deleted mark of the selected games.
    ToggleDeleted,
    /// Stop the search, sort or copy in flight.
    Cancel,
    /// Select the games that are visible.
    SelectVisible,
    /// Load this game into the board.
    Load(u32),
    /// Mark the database read-only or editable.
    ToggleReadOnly,
    /// Browse the selected games (one, or a grid of several).
    Browse,
    /// Browse this row's game, or the selection when the row is in it.
    BrowseRow(u32),
    /// Calculate variants from this game (spec M8-6d).
    Calvar(u32),
    /// Merge this game into the game of the editor (spec M7-6c).
    Merge(u32),
}
