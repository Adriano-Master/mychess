//! Keys of the game list and the focus of the search field (spec M3-4, slice
//! 4c: D564, D565 and the M2 shortcut contract), headless.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::format_push_string)]

mod m3_4c_common;

use m3_4c_common::*;
use mychess_app::{
    AppNotice, BasesMessage, BasesNotice, Key, ListMessage, Message as AppMessage, NamedKey,
};
use mychess_gui::Message;
use mychess_gui::windows::game_list::{GameListMessage, ListAction};
use mychess_gui::windows::layout::PaneKind;

fn first(rig: &Rig) -> u32 {
    current(rig).list.first
}

fn selection(rig: &Rig) -> Vec<u32> {
    current(rig).list.selection.iter().copied().collect()
}

fn filtered(rig: &Rig) -> u32 {
    current(rig).filtered()
}

/// A panel of 60 games with the field of the search focused.
fn focused(dir: &std::path::Path) -> Rig {
    let mut rig = panel(dir, 60);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 5,
        },
    );
    rig.shell.ui.games.search_focused = true;
    rig
}

// ---- D565: the focused search field owns its keys ----

#[test]
fn with_the_search_field_focused_the_list_keys_do_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = focused(dir.path());
    for key in [
        NamedKey::PageDown,
        NamedKey::PageUp,
        NamedKey::Home,
        NamedKey::End,
        NamedKey::Up,
        NamedKey::Down,
        NamedKey::Enter,
        NamedKey::Delete,
    ] {
        let units = key_units(&mut rig, plain(Key::Named(key)), false);
        assert_eq!(units, 0, "{key:?} must not reach the list or the board");
        assert_eq!(first(&rig), 1, "{key:?}");
        assert_eq!(selection(&rig), [5], "{key:?}");
        assert_eq!(
            filtered(&rig),
            60,
            "{key:?}: Delete must not edit the filter"
        );
    }
}

#[test]
fn the_same_keys_act_when_the_field_is_not_focused() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 5,
        },
    );
    key_units(&mut rig, plain(Key::Named(NamedKey::PageDown)), false);
    assert!(first(&rig) > 1, "Page Down scrolls");
    key_units(&mut rig, plain(Key::Named(NamedKey::Down)), false);
    assert_ne!(selection(&rig), [5], "Down moved the selection");
    let units = key_units(&mut rig, plain(Key::Named(NamedKey::Enter)), false);
    assert!(units > 0, "Enter loads the selected game");
}

#[test]
fn a_text_field_that_reports_the_focus_also_blocks_the_list_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 5,
        },
    );
    for key in [
        NamedKey::Home,
        NamedKey::Up,
        NamedKey::Delete,
        NamedKey::Enter,
    ] {
        key_units(&mut rig, plain(Key::Named(key)), true);
    }
    assert_eq!(selection(&rig), [5]);
    assert_eq!(first(&rig), 1);
    assert_eq!(filtered(&rig), 60);
}

#[test]
fn shift_tab_does_not_change_the_database_from_the_search_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    open_more(&mut rig, dir.path(), "b.pgn", 20);
    assert_eq!(rig.shell.app.bases.current(), slot(2));
    rig.shell.ui.games.search_focused = true;
    key_units(&mut rig, shift_tab(), false);
    assert_eq!(
        rig.shell.app.bases.current(),
        slot(2),
        "focus travel, not a tab switch"
    );
    rig.shell.ui.games.search_focused = false;
    key_units(&mut rig, shift_tab(), false);
    assert_eq!(
        rig.shell.app.bases.current(),
        slot(1),
        "Shift+Tab switches otherwise"
    );
}

#[test]
fn shift_tab_is_also_left_alone_when_any_text_field_has_the_focus() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    open_more(&mut rig, dir.path(), "b.pgn", 20);
    key_units(&mut rig, shift_tab(), true);
    assert_eq!(rig.shell.app.bases.current(), slot(2));
}

#[test]
fn escape_leaves_the_search_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = focused(dir.path());
    key_units(&mut rig, plain(Key::Named(NamedKey::Escape)), false);
    assert!(!rig.shell.ui.games.search_focused);
    // And the list keys work again.
    key_units(&mut rig, plain(Key::Named(NamedKey::PageDown)), false);
    assert!(first(&rig) > 1);
}

#[test]
fn ctrl_f_over_the_list_focuses_the_search_field_and_does_not_flip_the_board() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    let flipped = rig.shell.ui.orientation;
    let units = key_units(&mut rig, command(Key::Char('f')), false);
    assert!(rig.shell.ui.games.search_focused);
    assert!(units > 0, "the focus operation runs");
    assert_eq!(
        rig.shell.ui.orientation, flipped,
        "FlipBoard is not triggered"
    );
}

#[test]
fn ctrl_f_off_the_list_still_flips_the_board() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    let flipped = rig.shell.ui.orientation;
    key_units(&mut rig, command(Key::Char('f')), false);
    assert!(!rig.shell.ui.games.search_focused);
    assert_ne!(rig.shell.ui.orientation, flipped, "the M2 meaning (D154)");
}

#[test]
fn the_focus_shortcut_works_anywhere_while_the_panel_is_open_and_does_nothing_when_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    key_units(&mut rig, with(Key::Char('j'), true, true), false);
    assert!(rig.shell.ui.games.search_focused, "Command+Shift+J");
    rig.shell.ui.games.search_focused = false;
    rig.shell.ui.close_pane(PaneKind::GameList);
    key_units(&mut rig, with(Key::Char('j'), true, true), false);
    assert!(!rig.shell.ui.games.search_focused, "no panel, no focus");
}

// ---- pointer-only keys ----

#[test]
fn ctrl_a_selects_the_visible_games_only_with_the_pointer_over_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    let rows = rig.shell.ui.games.rows;
    assert!(rows > 3 && rows < 60);
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    key_units(&mut rig, command(Key::Char('a')), false);
    assert_eq!(
        selection(&rig),
        Vec::<u32>::new(),
        "off the list it is AddVariation"
    );
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    key_units(&mut rig, command(Key::Char('a')), false);
    let expected: Vec<u32> = (1..=u32::try_from(rows).unwrap()).collect();
    assert_eq!(selection(&rig), expected, "exactly the visible rows");
}

#[test]
fn the_central_select_all_shortcut_works_without_the_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    let rows = u32::try_from(rig.shell.ui.games.rows).unwrap();
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    key_units(&mut rig, with(Key::Char('a'), true, true), false);
    assert_eq!(u32::try_from(selection(&rig).len()).unwrap(), rows);
}

#[test]
fn select_visible_follows_the_scroll_position_and_the_sort() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    let rows = u32::try_from(rig.shell.ui.games.rows).unwrap();
    rig.send(BasesMessage::SetListFirst {
        slot: slot(1),
        first: 11,
    });
    list(&mut rig, GameListMessage::Act(ListAction::SelectVisible));
    let expected: Vec<u32> = (11..11 + rows).collect();
    assert_eq!(selection(&rig), expected);
    // Sorted by White ("White 1", "White 10"..): the visible rows of the view.
    list(
        &mut rig,
        GameListMessage::Header(mychess_gui::windows::game_list::Column::White),
    );
    let visible: Vec<u32> = rig
        .shell
        .app
        .bases
        .list_page(slot(1), 1, rows)
        .iter()
        .map(|r| r.number)
        .collect();
    list(&mut rig, GameListMessage::Act(ListAction::SelectVisible));
    let mut want = visible;
    want.sort_unstable();
    assert_eq!(selection(&rig), want);
}

#[test]
fn delete_removes_the_selection_from_the_filter_over_the_list_and_truncates_off_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 3,
        },
    );
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    key_units(&mut rig, plain(Key::Named(NamedKey::Delete)), false);
    assert_eq!(filtered(&rig), 60, "off the list Delete is TruncateGame");
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    key_units(&mut rig, plain(Key::Named(NamedKey::Delete)), false);
    assert_eq!(filtered(&rig), 59);
    assert!(!current(&rig).filter().contains(3));
}

#[test]
fn shift_delete_removes_from_the_filter_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 7,
        },
    );
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    key_units(
        &mut rig,
        with(Key::Named(NamedKey::Delete), false, true),
        false,
    );
    assert_eq!(filtered(&rig), 59);
}

#[test]
fn ctrl_delete_and_ctrl_backspace_toggle_the_deleted_flag_over_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 2,
        },
    );
    // A PGN refuses the flag: the proof the key reached `ToggleDeleted`.
    for key in [NamedKey::Delete, NamedKey::Backspace] {
        clear_notice(&mut rig);
        key_units(&mut rig, command(Key::Named(key)), false);
        assert_eq!(
            notice(&rig),
            Some(AppNotice::Bases(BasesNotice::ReadOnly)),
            "Ctrl+{key:?} over the list"
        );
    }
}

#[test]
fn the_central_toggle_deleted_shortcut_works_without_the_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 20);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 2,
        },
    );
    rig.msg(Message::Games(GameListMessage::Hover(false)));
    clear_notice(&mut rig);
    // Command+Delete is the mapped ListToggleDeleted.
    key_units(&mut rig, command(Key::Named(NamedKey::Delete)), false);
    assert_eq!(notice(&rig), Some(AppNotice::Bases(BasesNotice::ReadOnly)));
}

#[test]
fn the_pointer_keys_do_nothing_when_the_panel_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 5,
        },
    );
    rig.shell.ui.close_pane(PaneKind::GameList);
    key_units(&mut rig, command(Key::Char('a')), false);
    key_units(&mut rig, plain(Key::Named(NamedKey::PageDown)), false);
    assert_eq!(selection(&rig), [5], "no select-all");
    assert_eq!(first(&rig), 1, "no scrolling");
}

fn closed_panel_with_a_selection(dir: &std::path::Path) -> Rig {
    // A closed panel is not on screen: acting on its hidden selection (or,
    // for Command+Delete, writing flags to the file) would surprise the user.
    let mut rig = panel(dir, 60);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 5,
        },
    );
    rig.shell.ui.close_pane(PaneKind::GameList);
    rig
}

#[test]
fn command_shift_a_does_not_select_rows_while_the_panel_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = closed_panel_with_a_selection(dir.path());
    key_units(&mut rig, with(Key::Char('a'), true, true), false);
    assert_eq!(selection(&rig), [5]);
}

#[test]
fn shift_delete_does_not_edit_the_filter_while_the_panel_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = closed_panel_with_a_selection(dir.path());
    key_units(
        &mut rig,
        with(Key::Named(NamedKey::Delete), false, true),
        false,
    );
    assert_eq!(filtered(&rig), 60);
}

#[test]
fn command_delete_does_not_reach_the_flag_code_while_the_panel_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = closed_panel_with_a_selection(dir.path());
    clear_notice(&mut rig);
    key_units(&mut rig, command(Key::Named(NamedKey::Delete)), false);
    assert_eq!(notice(&rig), None);
}

#[test]
fn a_filter_job_started_with_the_keyboard_does_not_block_the_next_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = panel(dir.path(), 60);
    list(
        &mut rig,
        GameListMessage::Click {
            slot: slot(1),
            number: 4,
        },
    );
    // A job pending: Delete over the list must say "busy" and change nothing.
    let _held = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::List(ListMessage::SortBy {
            slot: slot(1),
            column: mychess_app::ListColumn::White,
        })),
    );
    key_units(&mut rig, plain(Key::Named(NamedKey::Delete)), false);
    assert_eq!(filtered(&rig), 60);
    assert_eq!(notice(&rig), Some(AppNotice::Bases(BasesNotice::ListBusy)));
}
