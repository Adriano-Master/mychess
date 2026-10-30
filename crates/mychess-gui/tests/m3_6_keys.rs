//! The bookmark shortcuts end to end: key, shell, application state (spec
//! M3-6, D679: `Command+D` adds, `Command+Shift+D` manages; the Scid has no
//! key). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m3_6_common;

use m3_6_common::b::plain;
use m3_6_common::*;
use mychess_app::{Action, AppNotice, BasesMessage, BookmarkNotice, Key, NamedKey, Shortcuts};

fn with_game(dir: &std::path::Path) -> Rig {
    let mut rig = rig_in(dir, None);
    open(&mut rig, &si4(dir, "a", "A", 3));
    let slot = rig.shell.app.bases.current();
    rig.send(BasesMessage::LoadGame { slot, number: 2 });
    rig
}

#[test]
fn ctrl_d_bookmarks_the_current_game_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_game(dir.path());
    press(&mut rig, ctrl(key_char('d'), false), false);
    let tree = &rig.shell.app.bookmarks;
    assert_eq!(tree.len(), 1);
    assert_eq!(tree.roots()[0].game().unwrap().game, 2);
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Bookmark(BookmarkNotice::Added))
    );
    assert!(has_text(&rig, "Bookmark added"), "the status bar shows it");
}

#[test]
fn ctrl_d_on_the_clipbase_refuses_with_a_notice_and_adds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    press(&mut rig, ctrl(key_char('d'), false), false);
    assert!(rig.shell.app.bookmarks.is_empty());
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Bookmark(BookmarkNotice::CannotAdd))
    );
    assert!(has_text(
        &rig,
        "Only a game of an open database can be bookmarked"
    ));
}

#[test]
fn ctrl_d_twice_adds_two_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_game(dir.path());
    press(&mut rig, ctrl(key_char('d'), false), false);
    press(&mut rig, ctrl(key_char('d'), false), false);
    assert_eq!(rig.shell.app.bookmarks.len(), 2);
}

#[test]
fn ctrl_shift_d_opens_the_editor_and_shows_the_box() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_game(dir.path());
    press(&mut rig, ctrl(key_char('d'), true), false);
    assert!(editor_open(&rig));
    assert!(has_id(
        &rig,
        mychess_gui::windows::bookmark_editor::BOOKMARKS_BOX_ID
    ));
}

#[test]
fn plain_d_and_shift_d_do_not_trigger_the_bookmark_actions() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_game(dir.path());
    press(&mut rig, plain(key_char('d')), false);
    press(&mut rig, plain(key_char('d')), true);
    assert!(rig.shell.app.bookmarks.is_empty());
    assert!(!editor_open(&rig));
}

#[test]
fn the_editor_does_not_open_while_another_dialog_is_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_game(dir.path());
    // The game information dialog is a modal (Ctrl+I in the central map).
    rig.send(mychess_app::Message::Info(
        mychess_app::GameInfoMessage::Open,
    ));
    assert!(rig.shell.app.info_dialog.is_some());
    press(&mut rig, ctrl(key_char('d'), true), false);
    assert!(!editor_open(&rig), "one modal at a time");
}

#[test]
fn the_bookmark_actions_are_in_the_central_map_with_the_documented_keys() {
    let table = Shortcuts::defaults();
    let labels: Vec<String> = [Action::AddBookmark, Action::ManageBookmarks]
        .into_iter()
        .flat_map(|a| table.bindings(a).to_vec())
        .map(|b| mychess_gui::keymap::binding_label(&b, mychess_gui::keymap::Platform::Linux))
        .collect();
    assert_eq!(labels, ["Ctrl+D", "Ctrl+Shift+D"]);
    let mac: Vec<String> = [Action::AddBookmark, Action::ManageBookmarks]
        .into_iter()
        .flat_map(|a| table.bindings(a).to_vec())
        .map(|b| mychess_gui::keymap::binding_label(&b, mychess_gui::keymap::Platform::MacOs))
        .collect();
    assert!(
        mac.iter().all(|l| !l.contains("Ctrl")),
        "Cmd on macOS: {mac:?}"
    );
}

#[test]
fn the_menu_shows_the_same_shortcut_labels_as_the_central_map() {
    use mychess_gui::windows::menu_bar::MenuEntry;
    let dir = tempfile::tempdir().unwrap();
    let rig = rig_in(dir.path(), None);
    let menu = submenu(&file_entries(&rig), "menu-file-bookmarks").to_vec();
    let mut seen = Vec::new();
    for entry in &menu {
        if let MenuEntry::Item {
            shortcut: Some(b), ..
        } = entry
        {
            seen.push(mychess_gui::keymap::binding_label(
                b,
                mychess_gui::keymap::Platform::Linux,
            ));
        }
    }
    assert_eq!(seen, ["Ctrl+D", "Ctrl+Shift+D"]);
}

#[test]
fn escape_without_the_editor_does_not_touch_the_bookmarks() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = with_game(dir.path());
    press(&mut rig, ctrl(key_char('d'), false), false);
    press(&mut rig, plain(Key::Named(NamedKey::Escape)), false);
    assert_eq!(rig.shell.app.bookmarks.len(), 1);
}
