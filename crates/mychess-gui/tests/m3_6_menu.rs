//! File > Bookmarks, recent files and Read-only, from the specification of
//! M3-6 (D673, D679, D681, D682; Scid `bookmark.tcl`, `recent.tcl`,
//! `menus.tcl` 98-105), with real databases. Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m3_6_common;

use std::path::PathBuf;

use m3_6_common::*;
use mychess_app::{
    BasesMessage, BookmarksMessage, Command, ListMessage, Message as AppMessage, SlotId,
};
use mychess_gui::Message;
use mychess_gui::windows::menu_bar::MenuEntry;

fn bookmarks_menu(rig: &Rig) -> Vec<MenuEntry> {
    submenu(&file_entries(rig), "menu-file-bookmarks").to_vec()
}

fn recent_menu(rig: &Rig) -> Vec<MenuEntry> {
    submenu(&file_entries(rig), "menu-file-open-recent").to_vec()
}

/// Sends the message an enabled menu item carries.
fn invoke(rig: &mut Rig, found: &Found) {
    match found.message.clone().expect("an enabled item") {
        Message::App(m) => {
            rig.send(m);
        }
        other => rig.msg(other),
    }
}

/// Loads game `number` of the current database.
fn load_current(rig: &mut Rig, number: u32) {
    let slot = rig.shell.app.bases.current();
    rig.send(BasesMessage::LoadGame { slot, number });
}

/// File > Bookmarks > Add current game.
fn click_add(rig: &mut Rig) {
    let add = item(&bookmarks_menu(rig), "menu-file-bookmarks-add");
    invoke(rig, &add);
}

fn current_slot(rig: &Rig) -> SlotId {
    rig.shell.app.bases.current()
}

// ---- Add current game (D673, Scid CanAdd) ----

#[test]
fn add_current_is_off_with_only_the_clipbase_and_with_a_database_without_a_game() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    assert!(!item(&bookmarks_menu(&rig), "menu-file-bookmarks-add").enabled);
    // A database is open but no game of it is loaded yet.
    open(&mut rig, &si4(dir.path(), "a", "A", 3));
    assert!(!rig.shell.app.can_bookmark_current());
    assert!(!item(&bookmarks_menu(&rig), "menu-file-bookmarks-add").enabled);
    // Manage is always on.
    assert!(item(&bookmarks_menu(&rig), "menu-file-bookmarks-manage").enabled);
}

#[test]
fn add_current_is_on_for_a_game_of_a_si4_and_creates_a_bookmark_with_its_identity() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let path = si4(dir.path(), "a", "A", 3);
    open(&mut rig, &path);
    load_current(&mut rig, 2);
    let add = item(&bookmarks_menu(&rig), "menu-file-bookmarks-add");
    assert!(add.enabled, "a game of an open database");
    invoke(&mut rig, &add);
    let tree = &rig.shell.app.bookmarks;
    assert_eq!(tree.len(), 1);
    let bookmark = tree.roots()[0].game().expect("a game bookmark").clone();
    assert_eq!(bookmark.base, path);
    assert_eq!(bookmark.game, 2);
    assert_eq!(
        (bookmark.white.as_str(), bookmark.black.as_str()),
        ("A 2", "Opp")
    );
    assert_eq!(bookmark.year, 2001);
    assert_eq!(bookmark.round, "2");
    assert_eq!(bookmark.site, "Rome, Italy");
    // Scid AddCurrent: file name, surnames, result, city, round, year.
    assert!(bookmark.title.starts_with("a:"), "{}", bookmark.title);
    assert!(bookmark.title.contains("A 2"), "{}", bookmark.title);
    assert!(bookmark.title.contains("Opp"), "{}", bookmark.title);
    assert!(bookmark.title.contains("Rome"), "{}", bookmark.title);
    assert!(bookmark.title.contains("2001"), "{}", bookmark.title);
    assert!(
        !bookmark.title.contains("Italy"),
        "the city only: {}",
        bookmark.title
    );
}

#[test]
fn a_bookmark_added_by_the_menu_keeps_the_existing_entries_and_is_a_top_level_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let path = si4(dir.path(), "a", "A", 3);
    add(&mut rig, None, folder("Folder"));
    add(&mut rig, None, game("Old", &path, 1));
    open(&mut rig, &path);
    load_current(&mut rig, 3);
    click_add(&mut rig);
    // Scid `AddCurrent`: the entry goes just before the first folder (D705).
    let rows = rig.shell.app.bookmarks.rows();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.depth == 0));
    assert!(!rows[0].is_folder && rows[0].title != "Old");
    assert!(rows[1].is_folder && rows[1].title == "Folder");
    assert_eq!(rows[2].title, "Old");
}

#[test]
fn add_current_is_also_on_for_a_pgn_slot_and_off_after_switching_to_the_clipbase() {
    // D673 (deviation from the Scid, which refuses `.pgn`).
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let path = numbered(dir.path(), "a.pgn", "P", 4);
    open(&mut rig, &path);
    load_current(&mut rig, 1);
    assert!(item(&bookmarks_menu(&rig), "menu-file-bookmarks-add").enabled);
    rig.send(BasesMessage::Select(SlotId::CLIPBASE));
    assert!(
        !item(&bookmarks_menu(&rig), "menu-file-bookmarks-add").enabled,
        "the Clipbase cannot be bookmarked"
    );
}

#[test]
fn add_current_is_off_for_a_game_that_is_not_from_a_slot() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    open(&mut rig, &si4(dir.path(), "a", "A", 2));
    load_current(&mut rig, 1);
    assert!(rig.shell.app.can_bookmark_current());
    // A new game replaces the game of the slot: it has no number.
    rig.send(AppMessage::Command(Command::NewGame));
    rig.send(AppMessage::Command(Command::NewGame));
    let on = item(&bookmarks_menu(&rig), "menu-file-bookmarks-add").enabled;
    assert_eq!(
        on,
        rig.shell.app.can_bookmark_current(),
        "menu follows the predicate"
    );
}

#[test]
fn adding_by_the_menu_writes_bookmarks_toml_and_a_new_session_sees_it() {
    // Acceptance: a bookmark survives restarting the program.
    let dir = tempfile::tempdir().unwrap();
    let path = si4(dir.path(), "a", "A", 3);
    {
        let mut rig = rig_in(dir.path(), None);
        open(&mut rig, &path);
        load_current(&mut rig, 3);
        click_add(&mut rig);
    }
    let text = std::fs::read_to_string(bookmarks_file(dir.path())).expect("the file was written");
    assert!(text.contains("version = 1"), "{text}");
    let rig = rig_in(dir.path(), Some(&text));
    let menu = bookmarks_menu(&rig);
    let games = text_items(&menu);
    assert_eq!(games.len(), 1, "{games:?}");
    assert!(games[0].contains("A 3"), "{games:?}");
}

// ---- the tree in the menu (D671, D679, D700-D704) ----

#[test]
fn the_tree_is_shown_as_nested_submenus_with_games_as_items_that_go() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let p = dir.path().join("x.si4");
    let top = add(&mut rig, None, game("Top", &p, 1));
    let f1 = add(&mut rig, Some(top), folder("Openings"));
    let f2 = add(&mut rig, Some(f1), folder("Sicilian"));
    let leaf = add(&mut rig, Some(f2), game("Najdorf", &p, 2));
    let menu = bookmarks_menu(&rig);
    assert!(
        matches!(menu[2], MenuEntry::Separator),
        "a separator before the tree"
    );
    let MenuEntry::Submenu { entries: l1, .. } = text_item(&menu, "Openings") else {
        panic!("a folder is a submenu")
    };
    let MenuEntry::Submenu { entries: l2, .. } = text_item(l1, "Sicilian") else {
        panic!("nested folder is a nested submenu")
    };
    let MenuEntry::Item { action, .. } = text_item(l2, "Najdorf") else {
        panic!("a game is an item")
    };
    assert_eq!(
        format!("{:?}", action.clone().unwrap()),
        format!(
            "{:?}",
            Message::App(AppMessage::Bookmarks(BookmarksMessage::Go(leaf)))
        )
    );
}

#[test]
fn an_empty_tree_has_no_separator_and_an_empty_folder_is_a_disabled_item() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let menu = bookmarks_menu(&rig);
    assert_eq!(menu.len(), 2, "Add and Manage only");
    add(&mut rig, None, folder("Empty"));
    let menu = bookmarks_menu(&rig);
    let MenuEntry::Submenu { entries, .. } = text_item(&menu, "Empty") else {
        panic!("submenu")
    };
    assert_eq!(entries.len(), 1);
    let found = item(entries, "menu-file-bookmarks-empty");
    assert!(!found.enabled, "(empty) is disabled");
}

#[test]
fn bookmark_titles_with_unicode_markup_and_length_are_shown_as_they_are() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let p = dir.path().join("x.si4");
    let long = "L\u{e9}vy \u{2013} \u{4e2d}\u{6587} \u{1f3c1} ".repeat(30);
    add(&mut rig, None, game(&long, &p, 1));
    add(&mut rig, None, game("{ $x } &amp; <b>", &p, 1));
    let texts = text_items(&bookmarks_menu(&rig));
    assert!(
        texts
            .iter()
            .any(|t| *t == long.chars().take(200).collect::<String>() || *t == long)
    );
    assert!(texts.iter().any(|t| t == "{ $x } &amp; <b>"));
}

#[test]
fn eight_levels_of_folders_nest_in_the_menu_without_losing_a_level() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let p = dir.path().join("x.si4");
    let mut id = add(&mut rig, None, folder("F1"));
    for depth in 2..=8 {
        id = add(&mut rig, Some(id), folder(&format!("F{depth}")));
    }
    let mut entries = bookmarks_menu(&rig);
    for depth in 1..=7 {
        let name = format!("F{depth}");
        let MenuEntry::Submenu { entries: inner, .. } = text_item(&entries, &name).clone() else {
            panic!("{name} is a submenu")
        };
        entries = inner;
    }
    let MenuEntry::Submenu { entries: last, .. } = text_item(&entries, "F8") else {
        panic!("F8 is a submenu")
    };
    assert!(!item(last, "menu-file-bookmarks-empty").enabled);
    // A game inside the eighth folder would be a ninth level: refused.
    let refused = rig
        .shell
        .app
        .bookmarks
        .insert_after(Some(id), game("deep", &p, 1))
        .is_err();
    assert!(refused, "no ninth level");
}

#[test]
fn bookmark_shortcuts_in_the_menu_come_from_the_central_map() {
    let dir = tempfile::tempdir().unwrap();
    let rig = rig_in(dir.path(), None);
    let menu = bookmarks_menu(&rig);
    assert!(item(&menu, "menu-file-bookmarks-add").shortcut);
    assert!(item(&menu, "menu-file-bookmarks-manage").shortcut);
}

// ---- Read-only (D682, Scid menus.tcl 98, 1634-1700) ----

#[test]
fn read_only_is_off_for_the_clipbase() {
    let dir = tempfile::tempdir().unwrap();
    let rig = rig_in(dir.path(), None);
    assert!(rig.shell.app.bases.current().is_clipbase());
    let found = item(&database_menu_entries(&rig), "menu-file-read-only");
    assert!(!found.enabled);
    assert_ne!(found.checked, Some(true));
}

#[test]
fn read_only_toggles_for_a_si4_and_shows_the_check_mark() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    open(&mut rig, &si4(dir.path(), "a", "A", 3));
    let found = item(&database_menu_entries(&rig), "menu-file-read-only");
    assert!(found.enabled, "a writable si4");
    assert_eq!(found.checked, Some(false));
    invoke(&mut rig, &found);
    assert!(rig.shell.app.bases.current_slot().unwrap().read_only);
    let found = item(&database_menu_entries(&rig), "menu-file-read-only");
    assert_eq!(found.checked, Some(true), "checked after SetReadOnly");
    assert!(found.enabled, "it can be turned off again");
    invoke(&mut rig, &found);
    assert_eq!(
        item(&database_menu_entries(&rig), "menu-file-read-only").checked,
        Some(false)
    );
}

#[test]
fn read_only_set_by_the_list_message_is_reflected_in_the_menu() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    open(&mut rig, &si4(dir.path(), "a", "A", 3));
    let slot = current_slot(&rig);
    rig.send(BasesMessage::List(ListMessage::SetReadOnly {
        slot,
        read_only: true,
    }));
    assert_eq!(
        item(&database_menu_entries(&rig), "menu-file-read-only").checked,
        Some(true)
    );
}

#[test]
fn read_only_is_checked_and_off_for_a_pgn() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    open(&mut rig, &numbered(dir.path(), "a.pgn", "P", 3));
    let found = item(&database_menu_entries(&rig), "menu-file-read-only");
    assert_eq!(found.checked, Some(true), "a PGN always opens read-only");
    assert!(!found.enabled, "and it cannot be changed");
}

#[test]
fn read_only_refers_to_the_current_slot_when_switching() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    open(&mut rig, &si4(dir.path(), "a", "A", 2));
    let first = current_slot(&rig);
    rig.send(BasesMessage::List(ListMessage::SetReadOnly {
        slot: first,
        read_only: true,
    }));
    open(&mut rig, &si4(dir.path(), "b", "B", 2));
    assert_ne!(current_slot(&rig), first);
    assert_eq!(
        item(&database_menu_entries(&rig), "menu-file-read-only").checked,
        Some(false)
    );
    rig.send(BasesMessage::Select(first));
    assert_eq!(
        item(&database_menu_entries(&rig), "menu-file-read-only").checked,
        Some(true)
    );
}

// ---- recent files (D681, Scid recent.tcl) ----

fn push_recent(rig: &mut Rig, paths: &[PathBuf]) {
    for p in paths {
        rig.shell.app.prefs.files.recent.push(p.clone());
    }
}

fn recent_labels_of(rig: &Rig) -> Vec<String> {
    text_items(&recent_menu(rig))
}

#[test]
fn recent_files_are_numbered_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    push_recent(
        &mut rig,
        &[dir.path().join("old.pgn"), dir.path().join("new.si4")],
    );
    assert_eq!(recent_labels_of(&rig), ["1: new.si4", "2: old.pgn"]);
}

#[test]
fn reopening_a_recent_file_moves_it_to_the_top_without_repeating_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let names = ["a.pgn", "b.pgn", "c.pgn"];
    let paths: Vec<PathBuf> = names.iter().map(|n| dir.path().join(n)).collect();
    push_recent(&mut rig, &paths);
    push_recent(&mut rig, &paths[..1]);
    assert_eq!(recent_labels_of(&rig), ["1: a.pgn", "2: c.pgn", "3: b.pgn"]);
}

#[test]
fn the_list_shows_at_most_ten_files_numbered_one_to_ten() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let paths: Vec<PathBuf> = (1..=13)
        .map(|n| dir.path().join(format!("f{n}.pgn")))
        .collect();
    push_recent(&mut rig, &paths);
    let labels = recent_labels_of(&rig);
    assert_eq!(labels.len(), 10, "{labels:?}");
    assert_eq!(labels[0], "1: f13.pgn");
    assert_eq!(labels[9], "10: f4.pgn");
}

#[test]
fn files_with_the_same_name_get_their_folder_and_the_others_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    push_recent(
        &mut rig,
        &[
            dir.path().join("one").join("games.pgn"),
            dir.path().join("solo.pgn"),
            dir.path().join("two").join("games.pgn"),
            dir.path().join("three").join("games.pgn"),
        ],
    );
    let labels = recent_labels_of(&rig);
    assert_eq!(labels[0], "1: games.pgn \u{2014} three/");
    assert_eq!(labels[1], "2: games.pgn \u{2014} two/");
    assert_eq!(labels[2], "3: solo.pgn");
    assert_eq!(labels[3], "4: games.pgn \u{2014} one/");
}

#[test]
fn names_that_differ_only_by_case_never_panic_and_each_keep_a_numbered_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    push_recent(
        &mut rig,
        &[dir.path().join("Games.PGN"), dir.path().join("games.pgn")],
    );
    let labels = recent_labels_of(&rig);
    assert!(!labels.is_empty() && labels.len() <= 2, "{labels:?}");
    assert!(labels[0].starts_with("1: "), "{labels:?}");
    // The commands point at distinct positions of the list.
    let entries = recent_menu(&rig);
    let commands: Vec<String> = entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item {
                label: mychess_gui::windows::menu_bar::Label::Text(_),
                action: Some(a),
                ..
            } => Some(format!("{a:?}")),
            _ => None,
        })
        .collect();
    let unique: std::collections::BTreeSet<_> = commands.iter().collect();
    assert_eq!(unique.len(), commands.len(), "{commands:?}");
}

#[test]
fn the_same_name_in_the_same_folder_name_still_opens_the_right_entry() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    push_recent(
        &mut rig,
        &[
            dir.path().join("a").join("x").join("f.pgn"),
            dir.path().join("b").join("x").join("f.pgn"),
        ],
    );
    let entries = recent_menu(&rig);
    let commands: Vec<String> = entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item {
                label: mychess_gui::windows::menu_bar::Label::Text(_),
                action: Some(a),
                ..
            } => Some(format!("{a:?}")),
            _ => None,
        })
        .collect();
    assert_eq!(commands.len(), 2);
    assert_ne!(commands[0], commands[1], "OpenRecent(0) and OpenRecent(1)");
}

#[test]
fn recent_labels_with_unicode_and_odd_names_are_kept_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    push_recent(
        &mut rig,
        &[
            dir.path().join("partidas \u{e7}\u{e3}o \u{4e2d}.pgn"),
            dir.path().join("sp ace & {x}.pgn"),
        ],
    );
    assert_eq!(
        recent_labels_of(&rig),
        [
            "1: sp ace & {x}.pgn",
            "2: partidas \u{e7}\u{e3}o \u{4e2d}.pgn"
        ]
    );
}

#[test]
fn clear_list_is_offered_only_with_a_non_empty_list_and_empties_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let empty = recent_menu(&rig);
    assert!(
        !empty.iter().any(|e| matches!(
            e,
            MenuEntry::Item {
                label: mychess_gui::windows::menu_bar::Label::Key("menu-file-open-recent-clear"),
                ..
            }
        )),
        "no Clear list without files"
    );
    assert!(!item(&empty, "menu-file-open-recent-none").enabled);
    push_recent(&mut rig, &[dir.path().join("a.pgn")]);
    let clear = item(&recent_menu(&rig), "menu-file-open-recent-clear");
    assert!(clear.enabled);
    invoke(&mut rig, &clear);
    assert!(rig.shell.app.prefs.files.recent.is_empty());
    assert!(recent_labels_of(&rig).is_empty());
}

#[test]
fn clearing_the_list_is_saved_in_the_preferences_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let a = numbered(dir.path(), "a.pgn", "A", 2);
    open(&mut rig, &a);
    rig.send(AppMessage::Command(Command::ClearRecent));
    let text = std::fs::read_to_string(dir.path().join("preferences.toml")).unwrap_or_default();
    assert!(
        !text.contains("a.pgn"),
        "the cleared list was written: {text}"
    );
}

// ---- going to a bookmark from the menu (D675) ----

fn go_item(rig: &Rig, title: &str) -> Found {
    let menu = bookmarks_menu(rig);
    let MenuEntry::Item { action, .. } = text_item(&menu, title) else {
        panic!("an item")
    };
    Found {
        enabled: action.is_some(),
        checked: None,
        shortcut: false,
        message: action.clone(),
    }
}

#[test]
fn a_bookmark_item_opens_a_closed_database_and_lands_on_the_game_and_ply() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let path = si4(dir.path(), "a", "A", 4);
    add(&mut rig, None, game("Game three", &path, 3));
    let go = go_item(&rig, "Game three");
    invoke(&mut rig, &go);
    let slot = rig.shell.app.bases.find_by_path(&path).expect("opened");
    assert_eq!(rig.shell.app.bases.current(), slot);
    assert_eq!(
        rig.shell.app.bases.slot(slot).unwrap().current_game,
        Some(3)
    );
    assert_eq!(rig.shell.app.current_bookmark().unwrap().ply, 2);
}

#[test]
fn a_bookmark_item_switches_to_the_slot_that_already_has_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let a = si4(dir.path(), "a", "A", 4);
    let b = si4(dir.path(), "b", "B", 4);
    open(&mut rig, &a);
    open(&mut rig, &b);
    let slot_a = rig.shell.app.bases.find_by_path(&a).unwrap();
    assert_ne!(rig.shell.app.bases.current(), slot_a);
    let slots_before = rig.shell.app.bases.iter().count();
    add(&mut rig, None, game("In A", &a, 4));
    let go = go_item(&rig, "In A");
    invoke(&mut rig, &go);
    assert_eq!(rig.shell.app.bases.current(), slot_a);
    assert_eq!(
        rig.shell.app.bases.iter().count(),
        slots_before,
        "not reopened"
    );
    assert_eq!(
        rig.shell.app.bases.slot(slot_a).unwrap().current_game,
        Some(4)
    );
}
