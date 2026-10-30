//! The context menus of the game list (spec M3-4c, D563).

use fluent_bundle::FluentArgs;
use iced::{Element, Size};
use mychess_app::{BaseKind, ColumnAlign, ListColumn, SlotId};

use super::MENU_WIDTH;
use super::columns::{Column, ColumnEdit};
use super::keys::ListAction;
use super::toolbar::enabled;
use super::update::{GameListMessage, MenuTarget, TabAction};
use crate::shell::{Message, Shell};
use crate::widgets::context_menu::{self, Entry};

// ---- context menus ----

pub(super) fn item(label: String, message: Option<GameListMessage>) -> Entry<'static, Message> {
    Entry::Item {
        label,
        message: message.map(Message::Games),
    }
}

pub(super) fn row_entries(shell: &Shell, number: u32) -> Vec<Entry<'static, Message>> {
    let i18n = &shell.i18n;
    let id = shell.app.bases.current();
    let Some(slot) = shell.app.bases.slot(id) else {
        return Vec::new();
    };
    let on = enabled(id, slot);
    let busy = shell.app.list_job().is_some();
    let go = |ok: bool, action: ListAction| (ok && !busy).then_some(GameListMessage::Act(action));
    let mut entries = vec![
        item(
            i18n.tr("games-menu-load"),
            go(true, ListAction::Load(number)),
        ),
        item(
            i18n.tr("games-menu-browse"),
            go(true, ListAction::BrowseRow(number)),
        ),
        item(
            i18n.tr("games-menu-calvar"),
            go(!shell.app.assist_locked(), ListAction::Calvar(number)),
        ),
        item(
            i18n.tr("merge-action"),
            go(
                crate::windows::merge_dialog::can_merge(&shell.app),
                ListAction::Merge(number),
            ),
        ),
        Entry::Separator,
        item(
            i18n.tr("games-menu-select-all"),
            go(true, ListAction::SelectVisible),
        ),
        item(
            i18n.tr("games-menu-remove"),
            go(on.selection, ListAction::RemoveSelected),
        ),
        item(
            i18n.tr("games-menu-remove-above"),
            go(true, ListAction::RemoveAbove(number)),
        ),
        item(
            i18n.tr("games-menu-remove-below"),
            go(true, ListAction::RemoveBelow(number)),
        ),
        item(
            i18n.tr("games-menu-keep"),
            go(on.selection, ListAction::Keep),
        ),
        Entry::Separator,
        item(
            i18n.tr("games-menu-copy"),
            go(on.copy, ListAction::CopySelected),
        ),
        item(
            i18n.tr("games-menu-delete"),
            go(on.flag, ListAction::ToggleDeleted),
        ),
        Entry::Separator,
        item(i18n.tr("games-menu-reset"), go(true, ListAction::Reset)),
        item(i18n.tr("games-menu-negate"), go(true, ListAction::Negate)),
    ];
    if slot.kind == BaseKind::Si4 {
        let key = if slot.read_only {
            "games-menu-editable"
        } else {
            "games-menu-read-only"
        };
        entries.push(Entry::Separator);
        entries.push(item(i18n.tr(key), go(true, ListAction::ToggleReadOnly)));
    }
    entries
}

pub(super) fn header_entries(shell: &Shell, column: Column) -> Vec<Entry<'static, Message>> {
    let i18n = &shell.i18n;
    let columns = &shell.app.prefs.game_list.columns;
    let edit = |e: ColumnEdit| Some(GameListMessage::Columns(e));
    let can_hide = columns.len() > 1;
    vec![
        item(
            i18n.tr("games-menu-sort"),
            column
                .list_column()
                .is_sortable()
                .then_some(GameListMessage::Header(column)),
        ),
        Entry::Separator,
        item(
            i18n.tr("games-menu-align-left"),
            edit(ColumnEdit::Align(column, ColumnAlign::Left)),
        ),
        item(
            i18n.tr("games-menu-align-center"),
            edit(ColumnEdit::Align(column, ColumnAlign::Center)),
        ),
        item(
            i18n.tr("games-menu-align-right"),
            edit(ColumnEdit::Align(column, ColumnAlign::Right)),
        ),
        Entry::Separator,
        item(i18n.tr("games-menu-wider"), edit(ColumnEdit::Wider(column))),
        item(
            i18n.tr("games-menu-narrower"),
            edit(ColumnEdit::Narrower(column)),
        ),
        item(
            i18n.tr("games-menu-move-left"),
            edit(ColumnEdit::MoveLeft(column)),
        ),
        item(
            i18n.tr("games-menu-move-right"),
            edit(ColumnEdit::MoveRight(column)),
        ),
        Entry::Separator,
        item(
            i18n.tr("games-menu-hide"),
            can_hide.then_some(GameListMessage::Columns(ColumnEdit::Toggle(column))),
        ),
        item(i18n.tr("games-menu-reset-columns"), edit(ColumnEdit::Reset)),
    ]
}

/// The menu of a tab of the strip of databases (Scid `switcher.tcl`):
/// reset and negate the filter, mark read-only, close (clear, for the
/// Clipbase); and the question before a copy, which is global.
pub(super) fn tab_entries(shell: &Shell, id: SlotId) -> Vec<Entry<'static, Message>> {
    let i18n = &shell.i18n;
    let Some(slot) = shell.app.bases.slot(id) else {
        return Vec::new();
    };
    let busy = shell.app.list_job().is_some();
    let act =
        |ok: bool, action: TabAction| (ok && !busy).then_some(GameListMessage::TabAct(id, action));
    // Marking read-only: only for an SI4 database, and not again once it
    // cannot be written anyway (a PGN file or a file without permission:
    // the Scid greys the entry for a read-only base that is not a PGN).
    let can_mark = slot.kind == BaseKind::Si4 && !slot.is_locked_read_only();
    let mark = if slot.read_only { '\u{2713}' } else { ' ' };
    let confirm = if shell.app.prefs.editing.confirm_copy {
        '\u{2713}'
    } else {
        ' '
    };
    let close_key = if id.is_clipbase() {
        "games-menu-clear-clipbase"
    } else {
        "games-menu-close-base"
    };
    let mut entries = vec![
        item(i18n.tr("games-menu-reset"), act(true, TabAction::Reset)),
        item(i18n.tr("games-menu-negate"), act(true, TabAction::Negate)),
        Entry::Separator,
        item(
            format!("{mark}  {}", i18n.tr("games-menu-tab-read-only")),
            act(can_mark, TabAction::ToggleReadOnly),
        ),
        item(
            format!("{confirm}  {}", i18n.tr("games-menu-confirm-copy")),
            Some(GameListMessage::TabAct(id, TabAction::ToggleConfirmCopy)),
        ),
        Entry::Separator,
    ];
    // "Copy the filter to...": the keyboard way of dragging the tab. Every
    // other open database is offered; the ones that cannot take the games
    // (read-only, or nothing in the filter) are greyed.
    let can_copy = !busy && slot.filtered() > 0;
    let mut offered = false;
    for (other, target) in shell.app.bases.iter().filter(|(other, _)| *other != id) {
        let mut args = FluentArgs::new();
        args.set("name", super::body::tab_name(i18n, other, target));
        let writable = !target.read_only && target.kind != BaseKind::Pgn;
        entries.push(item(
            i18n.tr_args("games-menu-copy-filter-to", &args),
            (can_copy && writable).then_some(GameListMessage::TabCopy(id, other)),
        ));
        offered = true;
    }
    if offered {
        entries.push(Entry::Separator);
    }
    entries.push(item(
        i18n.tr(close_key),
        Some(GameListMessage::TabAct(id, TabAction::Close)),
    ));
    entries
}

pub(super) fn column_list_entries(shell: &Shell) -> Vec<Entry<'static, Message>> {
    let i18n = &shell.i18n;
    let prefs = &shell.app.prefs.game_list;
    let mut entries = vec![Entry::Heading(i18n.tr("games-menu-columns"))];
    for list_column in ListColumn::ALL {
        let column = Column::of(list_column);
        let shown = prefs.shows(list_column);
        let mark = if shown { '\u{2713}' } else { ' ' };
        let only_one = shown && prefs.columns.len() == 1;
        entries.push(item(
            format!("{mark}  {}", i18n.tr(column.label_key())),
            (!only_one).then_some(GameListMessage::Columns(ColumnEdit::Toggle(column))),
        ));
    }
    entries.push(Entry::Separator);
    entries.push(item(
        i18n.tr("games-menu-reset-columns"),
        Some(GameListMessage::Columns(ColumnEdit::Reset)),
    ));
    entries
}

/// The layer to push on the window `stack`: the context menu of the list,
/// if one is open and the panel is.
pub fn overlay(shell: &Shell) -> Option<Element<'_, Message>> {
    let menu = shell.ui.games.menu.filter(|_| {
        shell
            .ui
            .is_visible(crate::windows::layout::PaneKind::GameList)
    })?;
    let entries = match menu.target {
        MenuTarget::Row(number) => row_entries(shell, number),
        MenuTarget::Header(column) => header_entries(shell, column),
        MenuTarget::Columns => column_list_entries(shell),
        MenuTarget::Tab(id) => tab_entries(shell, id),
    };
    if entries.is_empty() {
        return None;
    }
    let window = shell.ui.window_size.unwrap_or(Size::new(1280.0, 800.0));
    let size = context_menu::shown_size(context_menu::card_size(&entries, MENU_WIDTH), window);
    let at = context_menu::place(menu.position, size, window);
    let card = context_menu::card(entries, MENU_WIDTH, window.height);
    Some(context_menu::layer(
        card,
        at,
        Message::Games(GameListMessage::CloseMenu),
    ))
}
