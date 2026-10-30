//! The messages of the game list and the visual state that answers them.

use iced::{Point, Size};
use mychess_app::{
    AppState, BaseSlot, BasesMessage, CopyScope, ListMessage, Message as AppMessage, PickMode,
    PrefChange, PrefsMessage, SlotId,
};

use super::columns::{Column, ColumnEdit, edit_columns};
use super::keys::{ListAction, ListKey, max_first, visible_rows};
use super::{DEFAULT_ROWS, DEFAULT_WIDTH, MAX_ROWS};

/// What a context menu is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    /// A game of the list.
    Row(u32),
    /// A column header.
    Header(Column),
    /// The list of all the columns (the toolbar button).
    Columns,
    /// A tab of the strip of databases (slice 4d).
    Tab(SlotId),
}

/// What the context menu of a tab can do (spec M3-4d, D575 onwards).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    /// Every game back in the filter of the database.
    Reset,
    /// Negate the filter of the database.
    Negate,
    /// Mark the database read-only, or editable again (this session).
    ToggleReadOnly,
    /// Close the database (clear it, for the Clipbase).
    Close,
    /// Turn the question before a copy on or off.
    ToggleConfirmCopy,
}

/// What a drag that started on a tab or on a row carries (D576).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragPayload {
    /// The whole filter of the database whose tab was pressed.
    Filter,
    /// The selected games of the list.
    Selection,
}

/// A drag in progress: pressed on a tab or on a selected row, released
/// over a tab. Only visual state; the copy is the application's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabDrag {
    /// The database the games come from.
    pub from: SlotId,
    /// What is dragged.
    pub payload: DragPayload,
    /// The tab under the pointer, once it has one.
    pub over: Option<SlotId>,
    /// A plain press on a row that is part of a multiple selection keeps
    /// the selection (it may be the start of a drag); if the button is
    /// released on that same row the selection shrinks to it.
    pub collapse: Option<u32>,
}

impl TabDrag {
    /// Whether the pointer is over a tab other than the one the drag
    /// started on (what makes the drag visible).
    pub fn is_dragging(&self) -> bool {
        self.over.is_some_and(|over| over != self.from)
    }
}

/// An open context menu (window coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListMenu {
    /// What it is about.
    pub target: MenuTarget,
    /// Where the card opens.
    pub position: Point,
}

/// A message of the game list.
#[derive(Debug, Clone, PartialEq)]
pub enum GameListMessage {
    /// The rows area has this size.
    Viewport(Size),
    /// The pointer entered or left the panel.
    Hover(bool),
    /// The button was released on this tab with no drag in progress: a
    /// click, if the press was on the same tab (D616).
    TabClick(SlotId),
    /// The pointer entered the row of this game (the target of `B` when
    /// nothing is selected).
    RowHover(u32),
    /// The wheel moved the list by this many rows (positive: down).
    Scroll {
        /// The slot shown.
        slot: SlotId,
        /// Rows to move.
        rows: i64,
    },
    /// The scroll bar was dragged to this first position.
    ScrollTo {
        /// The slot shown.
        slot: SlotId,
        /// The position in the filter, from 1.
        first: u32,
    },
    /// A row was clicked (the modifiers held decide how the selection
    /// changes).
    Click {
        /// The slot shown.
        slot: SlotId,
        /// The game number.
        number: u32,
    },
    /// A row was double clicked: load the game.
    Open {
        /// The slot shown.
        slot: SlotId,
        /// The game number.
        number: u32,
    },
    /// A key.
    Key(ListKey),
    /// The Ctrl/Cmd and Shift keys went down or up.
    Modifiers {
        /// Ctrl (Cmd on the Mac).
        command: bool,
        /// Shift.
        shift: bool,
    },
    /// The search field gained or lost the focus (found out by asking the
    /// widget after each click, D565).
    SearchFocus(bool),
    /// The text of the search field changed.
    SearchInput(String),
    /// The "ignore case" box was set.
    IgnoreCase(bool),
    /// An action on the games.
    Act(ListAction),
    /// Move the focus to the search field (the shell runs the operation).
    FocusSearch,
    /// A header was clicked: sort.
    Header(Column),
    /// Open a context menu.
    Menu(ListMenu),
    /// Close the context menu.
    CloseMenu,
    /// Edit the columns.
    Columns(ColumnEdit),
    /// Show the list of columns (the toolbar button), at this point.
    ColumnsButton(Point),
    /// The left button went down on a tab: a drag may start.
    TabPress(SlotId),
    /// The pointer entered a tab (`Some`) or left them all (`None`).
    TabHover(Option<SlotId>),
    /// The button was released over a tab other than the one it was
    /// pressed on (or over any tab, for a drag of rows): copy.
    TabDrop(SlotId),
    /// The button went up on a row (see [`TabDrag::collapse`]).
    RowRelease {
        /// The slot shown.
        slot: SlotId,
        /// The game number.
        number: u32,
    },
    /// The button went up somewhere, or Esc was pressed: the drag ends
    /// (a drop on a tab has been handled before this).
    DragEnd,
    /// An item of the context menu of a tab.
    TabAct(SlotId, TabAction),
    /// "Copy the filter to...": from the first slot to the second (the
    /// keyboard-friendly way of doing what the drag does).
    TabCopy(SlotId, SlotId),
}

/// The visual state of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct GameListUi {
    /// Whole rows that fit in the rows area.
    pub rows: usize,
    /// Width of the rows area.
    pub width: f32,
    /// Whether the pointer is over the panel: the arrow keys, Home, End and
    /// Enter belong to the list then.
    pub hovered: bool,
    /// Ctrl (Cmd on the Mac) is down.
    pub command: bool,
    /// Shift is down.
    pub shift: bool,
    /// The search field has the focus: no key of the list or of the
    /// shortcut map that text editing uses is acted on (D565).
    pub search_focused: bool,
    /// The open context menu.
    pub menu: Option<ListMenu>,
    /// The drag in progress, if any (D576).
    pub drag: Option<TabDrag>,
    /// Esc cancelled a drag and the button is still down: the release must
    /// not count as a click on the tab under the pointer (D594).
    pub drag_cancelled: bool,
    /// The tab under the pointer.
    pub hover_tab: Option<SlotId>,
    /// The row under the pointer.
    pub hover_row: Option<u32>,
    /// The tab the button went down on and has not come up yet.
    pub pressed_tab: Option<SlotId>,
}

impl Default for GameListUi {
    fn default() -> GameListUi {
        GameListUi {
            rows: DEFAULT_ROWS,
            width: DEFAULT_WIDTH,
            hovered: false,
            command: false,
            shift: false,
            search_focused: false,
            menu: None,
            drag: None,
            drag_cancelled: false,
            hover_tab: None,
            hover_row: None,
            pressed_tab: None,
        }
    }
}

pub(super) fn set_first(slot: SlotId, first: u32) -> AppMessage {
    AppMessage::Bases(BasesMessage::SetListFirst { slot, first })
}

pub(super) fn select(slot: SlotId, number: Option<u32>) -> AppMessage {
    AppMessage::Bases(BasesMessage::SelectRow { slot, number })
}

pub(super) fn list(message: ListMessage) -> AppMessage {
    AppMessage::Bases(BasesMessage::List(message))
}

/// Opens the browser on these games (nothing when there are none).
fn browse_open(slot: SlotId, numbers: Vec<u32>) -> Vec<AppMessage> {
    if numbers.is_empty() {
        return Vec::new();
    }
    vec![AppMessage::Bases(BasesMessage::Browse(
        mychess_app::BrowseMessage::Open { slot, numbers },
    ))]
}

/// The messages for an item of the context menu of a tab.
fn tab_action(app: &AppState, slot: SlotId, action: TabAction) -> Vec<AppMessage> {
    match action {
        TabAction::Reset => vec![list(ListMessage::ResetFilter { slot })],
        TabAction::Negate => vec![list(ListMessage::NegateFilter { slot })],
        TabAction::ToggleReadOnly => {
            let read_only = app.bases.slot(slot).is_some_and(|s| s.read_only);
            vec![list(ListMessage::SetReadOnly {
                slot,
                read_only: !read_only,
            })]
        }
        TabAction::Close => vec![AppMessage::Bases(BasesMessage::Close(slot))],
        TabAction::ToggleConfirmCopy => vec![AppMessage::Prefs(PrefsMessage::Set(
            PrefChange::ConfirmCopy(!app.prefs.editing.confirm_copy),
        ))],
    }
}

impl GameListUi {
    /// Closes the context menu of a tab whose database is gone.
    pub fn drop_stale_menu(&mut self, app: &AppState) {
        if let Some(ListMenu {
            target: MenuTarget::Tab(id),
            ..
        }) = self.menu
            && app.bases.slot(id).is_none()
        {
            self.menu = None;
        }
    }

    /// Esc during a drag. Returns whether the key was spent on it: only a
    /// visible drag is cancelled (the button may still be down, so the
    /// release is remembered as dead, D594); a plain click in progress
    /// leaves the key to others.
    pub fn escape_drag(&mut self) -> bool {
        let visible = self.drag.is_some_and(|d| d.is_dragging());
        if visible {
            self.drag_cancelled = true;
        }
        self.drag = None;
        visible
    }

    /// Called when the panel opens or closes: a closed panel has no hovered
    /// row, no focused search field and no menu (D572).
    pub fn close_with_panel(&mut self, open: bool) {
        if !open {
            self.hovered = false;
            self.search_focused = false;
            self.menu = None;
            self.drag = None;
            self.drag_cancelled = false;
            self.hover_tab = None;
            self.hover_row = None;
        }
    }

    /// Applies a message: the visual part here, the rest as messages for
    /// the application (the list position, the selection, loading a game).
    #[allow(clippy::needless_pass_by_value, clippy::too_many_lines)] // one flat match
    pub fn update(&mut self, message: GameListMessage, app: &AppState) -> Vec<AppMessage> {
        match message {
            GameListMessage::Viewport(size) => {
                self.rows = visible_rows(size.height).min(MAX_ROWS);
                self.width = if size.width.is_finite() {
                    size.width
                } else {
                    DEFAULT_WIDTH
                };
                Vec::new()
            }
            GameListMessage::RowHover(number) => {
                self.hover_row = Some(number);
                Vec::new()
            }
            GameListMessage::Hover(on) => {
                self.hovered = on;
                if !on {
                    self.hover_row = None;
                }
                Vec::new()
            }
            GameListMessage::Scroll { slot, rows } => self.scroll_by(app, slot, rows),
            GameListMessage::ScrollTo { slot, first } => {
                self.scroll_to(app, slot, i64::from(first))
            }
            GameListMessage::Click { slot, number } => {
                self.menu = None;
                // A press on a row may start a drag of the selection. A
                // plain press on a row of a multiple selection leaves the
                // selection alone until the button comes up on the row.
                let inside = app.bases.slot(slot).is_some_and(|s| {
                    s.list.selection.len() > 1 && s.list.selection.contains(&number)
                });
                let plain = !self.shift && !self.command;
                self.drag = Some(TabDrag {
                    from: slot,
                    payload: DragPayload::Selection,
                    over: None,
                    collapse: (plain && inside).then_some(number),
                });
                if plain && inside {
                    return Vec::new();
                }
                let mode = if self.shift {
                    PickMode::Range
                } else if self.command {
                    PickMode::Toggle
                } else {
                    PickMode::Only
                };
                vec![list(ListMessage::Pick { slot, number, mode })]
            }
            GameListMessage::Open { slot, number } => {
                vec![
                    select(slot, Some(number)),
                    AppMessage::Bases(BasesMessage::LoadGame { slot, number }),
                ]
            }
            GameListMessage::Key(key) => self.key(app, key),
            GameListMessage::Modifiers { command, shift } => {
                self.command = command;
                self.shift = shift;
                Vec::new()
            }
            GameListMessage::SearchFocus(on) => {
                self.search_focused = on;
                Vec::new()
            }
            GameListMessage::SearchInput(text) => {
                let slot = app.bases.current();
                vec![AppMessage::Bases(BasesMessage::SetSearchText {
                    slot,
                    text,
                })]
            }
            GameListMessage::IgnoreCase(ignore_case) => {
                let slot = app.bases.current();
                vec![list(ListMessage::SetIgnoreCase { slot, ignore_case })]
            }
            GameListMessage::Act(action) => {
                self.menu = None;
                self.act(app, action)
            }
            // The shell focuses the field; nothing for the application.
            GameListMessage::FocusSearch => {
                self.search_focused = true;
                Vec::new()
            }
            GameListMessage::Header(column) => {
                self.menu = None;
                let slot = app.bases.current();
                vec![list(ListMessage::SortBy {
                    slot,
                    column: column.list_column(),
                })]
            }
            GameListMessage::Menu(menu) => {
                self.menu = Some(menu);
                // A right click on a game outside the selection selects it,
                // like a file manager does.
                match menu.target {
                    MenuTarget::Row(number) => {
                        let slot = app.bases.current();
                        let inside = app
                            .bases
                            .slot(slot)
                            .is_some_and(|s| s.list.selection.contains(&number));
                        if inside {
                            Vec::new()
                        } else {
                            vec![select(slot, Some(number))]
                        }
                    }
                    _ => Vec::new(),
                }
            }
            GameListMessage::CloseMenu => {
                self.menu = None;
                Vec::new()
            }
            GameListMessage::TabPress(slot) => {
                self.pressed_tab = Some(slot);
                self.menu = None;
                self.drag_cancelled = false;
                self.drag = Some(TabDrag {
                    from: slot,
                    payload: DragPayload::Filter,
                    over: Some(slot),
                    collapse: None,
                });
                Vec::new()
            }
            GameListMessage::TabHover(tab) => {
                self.hover_tab = tab;
                if let Some(drag) = &mut self.drag {
                    drag.over = tab;
                }
                Vec::new()
            }
            GameListMessage::TabClick(id) => {
                self.drag = None;
                if self.pressed_tab.take() == Some(id) {
                    vec![AppMessage::Bases(BasesMessage::Select(id))]
                } else {
                    Vec::new()
                }
            }
            GameListMessage::TabDrop(to) => {
                self.pressed_tab = None;
                let Some(drag) = self.drag.take() else {
                    return Vec::new();
                };
                self.hover_tab = Some(to);

                // Dropping on the tab it came from only selects (a click)
                // for a tab, and does nothing for rows.
                if drag.from == to {
                    return match drag.payload {
                        DragPayload::Filter => vec![AppMessage::Bases(BasesMessage::Select(to))],
                        DragPayload::Selection => Vec::new(),
                    };
                }
                let scope = match drag.payload {
                    DragPayload::Filter => CopyScope::Filter,
                    DragPayload::Selection => CopyScope::Selection,
                };
                vec![list(ListMessage::CopyGames {
                    from: drag.from,
                    to,
                    scope,
                })]
            }
            GameListMessage::RowRelease { slot, number } => {
                let collapse = self.drag.take().and_then(|d| d.collapse);
                if collapse == Some(number) {
                    vec![list(ListMessage::Pick {
                        slot,
                        number,
                        mode: PickMode::Only,
                    })]
                } else {
                    Vec::new()
                }
            }
            GameListMessage::DragEnd => {
                self.pressed_tab = None;
                self.drag = None;
                self.drag_cancelled = false;
                Vec::new()
            }
            GameListMessage::TabCopy(from, to) => {
                self.menu = None;
                vec![list(ListMessage::CopyGames {
                    from,
                    to,
                    scope: CopyScope::Filter,
                })]
            }
            GameListMessage::TabAct(slot, action) => {
                self.menu = None;
                tab_action(app, slot, action)
            }
            GameListMessage::ColumnsButton(position) => {
                self.menu = Some(ListMenu {
                    target: MenuTarget::Columns,
                    position,
                });
                Vec::new()
            }
            GameListMessage::Columns(edit) => {
                // Showing and hiding columns keeps the list of columns
                // open; the other edits end the menu of their header.
                let keeps_open = matches!(
                    (&edit, self.menu),
                    (
                        ColumnEdit::Toggle(_) | ColumnEdit::Reset,
                        Some(ListMenu {
                            target: MenuTarget::Columns,
                            ..
                        })
                    )
                );
                if !keeps_open {
                    self.menu = None;
                }
                let columns = edit_columns(&app.prefs.game_list, &edit);
                vec![AppMessage::Prefs(PrefsMessage::Set(PrefChange::GameList(
                    columns,
                )))]
            }
        }
    }

    fn act(&self, app: &AppState, action: ListAction) -> Vec<AppMessage> {
        let slot = app.bases.current();
        let Some(state) = app.bases.slot(slot) else {
            return Vec::new();
        };
        let one = |m| vec![list(m)];
        match action {
            ListAction::Find => one(ListMessage::Find { slot }),
            ListAction::FilterText => one(ListMessage::FilterByText { slot }),
            ListAction::Reset => one(ListMessage::ResetFilter { slot }),
            ListAction::Negate => one(ListMessage::NegateFilter { slot }),
            ListAction::RemoveSelected => one(ListMessage::RemoveSelected { slot }),
            ListAction::RemoveAbove(number) => one(ListMessage::RemoveAbove { slot, number }),
            ListAction::RemoveBelow(number) => one(ListMessage::RemoveBelow { slot, number }),
            ListAction::Keep => one(ListMessage::KeepSelected { slot }),
            ListAction::CopySelected => one(ListMessage::CopySelected { slot }),
            ListAction::ToggleDeleted => one(ListMessage::ToggleDeleted { slot }),
            ListAction::Cancel => one(ListMessage::CancelJob),
            ListAction::SelectVisible => {
                let rows = u32::try_from(self.rows).unwrap_or(u32::MAX);
                let to = state.list.first.saturating_add(rows.saturating_sub(1));
                one(ListMessage::SelectNumbers {
                    slot,
                    numbers: state.visible_range(state.list.first, to),
                })
            }
            ListAction::Load(number) => {
                vec![AppMessage::Bases(BasesMessage::LoadGame { slot, number })]
            }
            ListAction::Browse => {
                // D619: a selection of two or more is the grid; else the row
                // under the pointer; else the selected row; else the game
                // being edited.
                let hover = if state.list.selection.len() >= 2 {
                    None
                } else {
                    self.hover_row
                };
                browse_open(slot, state.browse_targets(hover))
            }
            ListAction::BrowseRow(number) => browse_open(slot, state.browse_targets(Some(number))),
            ListAction::Calvar(number) => vec![AppMessage::Calvar(
                mychess_app::calvar::CalvarMessage::StartFromGame {
                    slot,
                    number,
                    now: crate::windows::calvar_dialog::wall_clock(),
                },
            )],
            ListAction::Merge(number) => {
                vec![crate::windows::merge_dialog::open_message_app(slot, number)]
            }
            ListAction::ToggleReadOnly => one(ListMessage::SetReadOnly {
                slot,
                read_only: !state.read_only,
            }),
        }
    }

    /// The message that brings the list of the current slot back inside
    /// its range, if it is outside (the window grew, a tab was selected).
    pub fn clamp(&self, app: &AppState) -> Option<AppMessage> {
        let slot = app.bases.current();
        let state = app.bases.slot(slot)?;
        let last = max_first(state.filtered(), self.rows);
        (state.list.first > last).then(|| set_first(slot, last))
    }

    /// The position to scroll to for a wanted first row, if it differs
    /// from the current one.
    fn scroll_target(&self, state: &BaseSlot, first: i64) -> Option<u32> {
        let last = i64::from(max_first(state.filtered(), self.rows));
        let target = u32::try_from(first.clamp(1, last)).unwrap_or(1);
        (target != state.list.first).then_some(target)
    }

    fn scroll_to(&self, app: &AppState, slot: SlotId, first: i64) -> Vec<AppMessage> {
        let Some(state) = app.bases.slot(slot) else {
            return Vec::new();
        };
        self.scroll_target(state, first)
            .map(|target| set_first(slot, target))
            .into_iter()
            .collect()
    }

    fn scroll_by(&self, app: &AppState, slot: SlotId, rows: i64) -> Vec<AppMessage> {
        let Some(state) = app.bases.slot(slot) else {
            return Vec::new();
        };
        self.scroll_to(app, slot, i64::from(state.list.first).saturating_add(rows))
    }

    fn page(&self) -> i64 {
        i64::try_from(self.rows.saturating_sub(1).max(1)).unwrap_or(1)
    }

    fn key(&self, app: &AppState, key: ListKey) -> Vec<AppMessage> {
        let slot = app.bases.current();
        let Some(state) = app.bases.slot(slot) else {
            return Vec::new();
        };
        match key {
            ListKey::Enter => state
                .list
                .selected
                .map(|number| AppMessage::Bases(BasesMessage::LoadGame { slot, number }))
                .into_iter()
                .collect(),
            ListKey::Up => self.move_selection(slot, state, -1),
            ListKey::Down => self.move_selection(slot, state, 1),
            ListKey::PageUp => self.move_page(slot, state, -self.page()),
            ListKey::PageDown => self.move_page(slot, state, self.page()),
            ListKey::Home => self.select_rank(slot, state, 1),
            ListKey::End => self.select_rank(slot, state, i64::from(state.filtered())),
            ListKey::SelectAll => self.act(app, ListAction::SelectVisible),
            ListKey::Remove => self.act(app, ListAction::RemoveSelected),
            ListKey::ToggleDeleted => self.act(app, ListAction::ToggleDeleted),
            ListKey::Browse => self.act(app, ListAction::Browse),
            // Needs the shell to run the focus operation.
            ListKey::FocusSearch => Vec::new(),
        }
    }

    /// The position in the view of the selected game, if there is one.
    fn selected_rank(state: &BaseSlot) -> Option<i64> {
        let number = state.list.selected?;
        state.position_of(number).map(i64::from)
    }

    /// Selects the game at `rank` (clamped) and scrolls it into view.
    fn select_rank(&self, slot: SlotId, state: &BaseSlot, rank: i64) -> Vec<AppMessage> {
        let count = i64::from(state.filtered());
        if count == 0 {
            return Vec::new();
        }
        let rank = rank.clamp(1, count);
        let Some(number) = u32::try_from(rank)
            .ok()
            .and_then(|rank| state.nth_visible(rank))
        else {
            return Vec::new();
        };
        let mut out = vec![select(slot, Some(number))];
        let first = i64::from(state.list.first);
        let rows = i64::try_from(self.rows).unwrap_or(1);
        let wanted = if rank < first {
            Some(rank)
        } else if rank >= first + rows {
            Some(rank - rows + 1)
        } else {
            None
        };
        if let Some(target) = wanted.and_then(|wanted| self.scroll_target(state, wanted)) {
            out.push(set_first(slot, target));
        }
        out
    }

    fn move_selection(&self, slot: SlotId, state: &BaseSlot, step: i64) -> Vec<AppMessage> {
        let from = Self::selected_rank(state).unwrap_or_else(|| {
            // Nothing selected: the first press selects the first visible
            // row, going down or up.
            let first = i64::from(state.list.first);
            if step > 0 { first - 1 } else { first + 1 }
        });
        self.select_rank(slot, state, from.saturating_add(step))
    }

    fn move_page(&self, slot: SlotId, state: &BaseSlot, step: i64) -> Vec<AppMessage> {
        let mut out = Vec::new();
        let first = i64::from(state.list.first).saturating_add(step);
        if let Some(target) = self.scroll_target(state, first) {
            out.push(set_first(slot, target));
        }
        // The selection travels with the page, as in a file manager.
        if let Some(rank) = Self::selected_rank(state) {
            let count = i64::from(state.filtered());
            let moved = rank.saturating_add(step).clamp(1, count.max(1));
            if let Some(number) = u32::try_from(moved)
                .ok()
                .and_then(|rank| state.nth_visible(rank))
            {
                out.push(select(slot, Some(number)));
            }
        }
        out
    }
}
