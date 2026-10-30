//! The game list panel (spec M3-4, slice 4b; decisions D536 to D548): a strip
//! of tabs, one per open database, and under it the list of the games of
//! the filter of the selected one.
//!
//! The list is virtualized (D509): `view` asks `Bases::list_page` for the
//! visible rows only, so a database of a million games costs the same as a
//! small one. The number of visible rows comes from a `sensor` around the
//! rows ([`GameListMessage::Viewport`]). Which database and which row are
//! shown, selected or current is state of the application (`ListState`);
//! this module only keeps what the screen needs ([`GameListUi`]).

//!
//! Layout: `columns` (the 22 columns and their editing), `keys` (keys, actions
//! and scroll arithmetic), `update` (messages and visual state), `body` (tabs,
//! header and rows), `toolbar` and `menus`.

mod body;
mod columns;
#[cfg(test)]
mod drag_tests;
mod keys;
mod menus;
#[cfg(test)]
mod tests;
mod toolbar;
mod update;

pub use body::view;
pub use columns::{Column, ColumnEdit, columns_for_width, edit_columns, visible_setups};
pub use keys::{ListAction, ListKey, max_first, visible_rows, wheel_rows};
pub use menus::overlay;
pub use update::{
    DragPayload, GameListMessage, GameListUi, ListMenu, MenuTarget, TabAction, TabDrag,
};

/// Height of a row of the list, and of the header, in logical pixels.
pub const ROW_HEIGHT: f32 = 24.0;
/// Rows assumed until the sensor reports the real size.
const DEFAULT_ROWS: usize = 12;
/// Width assumed until the sensor reports the real size.
const DEFAULT_WIDTH: f32 = 900.0;
/// Rows a turn of the wheel scrolls (the Scid scrolls one; three is the
/// usual feel of a modern list).
const WHEEL_ROWS: f32 = 3.0;
/// The panel is wide enough for the two Elo columns from this width on.
const ELO_MIN_WIDTH: f32 = 760.0;
/// Width of the scroll bar.
const SCROLLBAR_WIDTH: f32 = 12.0;
/// Characters of a file name shown in a tab before it is cut.
const TAB_NAME_CHARS: usize = 18;
/// The largest list the screen could show: bounds a bogus viewport.
const MAX_ROWS: usize = 400;

/// Id of the search field (not a Fluent key).
pub const SEARCH_ID: &str = "glist-search";

/// Width of a context menu card.
const MENU_WIDTH: f32 = 300.0;
