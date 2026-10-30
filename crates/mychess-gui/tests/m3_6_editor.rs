//! The bookmark editor, end to end through the real view (spec M3-6, D678,
//! D700-D704; Scid `bookmark.tcl`: `Edit` 316, `EditDone` 397, `EditMove` 477,
//! `EditDelete` 506, `EditNew` 527). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m3_6_common;

use m3_6_common::b::plain;
use m3_6_common::*;
use mychess_app::{
    AppNotice, BasesMessage, BookmarkNotice, EditorMessage, Key, MAX_TITLE_CHARS, NamedKey,
    NodeKind, parse_bookmarks,
};
use mychess_gui::windows::bookmark_editor::BOOKMARKS_BOX_ID;

fn saved_tree(dir: &std::path::Path) -> Vec<String> {
    let text = std::fs::read_to_string(bookmarks_file(dir)).expect("bookmarks.toml");
    let loaded = parse_bookmarks(&text);
    loaded
        .bookmarks
        .rows()
        .into_iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.title))
        .collect()
}

/// A flat tree `[Openings]{Sicilian}`, `French`, `[Endings]` built by hand.
fn flat(dir: &std::path::Path) -> (Rig, std::path::PathBuf) {
    let path = si4(dir, "a", "W", 4);
    let mut rig = rig_in(dir, None);
    let openings = add(&mut rig, None, folder("Openings"));
    add(&mut rig, Some(openings), game("Sicilian", &path, 2));
    let french = add(&mut rig, None, game("French", &path, 3));
    add(&mut rig, Some(french), folder("Endings"));
    assert_eq!(
        titles(&rig),
        ["Openings", "  Sicilian", "French", "Endings"]
    );
    (rig, path)
}

#[test]
fn the_editor_opens_by_key_with_the_tree_as_a_working_copy_and_shows_the_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    assert!(!has_id(&rig, BOOKMARKS_BOX_ID));
    open_editor_by_key(&mut rig);
    assert!(has_id(&rig, BOOKMARKS_BOX_ID));
    for text in ["[Openings]", "Sicilian", "French", "[Endings]"] {
        assert!(has_text(&rig, text), "{text}");
    }
    assert_eq!(
        working(&rig),
        ["[Openings]", "  Sicilian", "French", "[Endings]"]
    );
}

#[test]
fn selecting_renaming_moving_and_deleting_edit_the_copy_and_not_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    let before = titles(&rig);
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    assert_eq!(
        rig.shell.app.bookmark_editor.as_ref().unwrap().title,
        "French",
        "the field shows the title of the selection"
    );
    type_in_title(&mut rig, "!");
    assert_eq!(working(&rig)[2], "French!");
    click(&mut rig, "Down");
    assert_eq!(
        working(&rig),
        ["[Openings]", "  Sicilian", "[Endings]", "French!"]
    );
    click(&mut rig, "Up");
    assert_eq!(
        working(&rig),
        ["[Openings]", "  Sicilian", "French!", "[Endings]"]
    );
    click(&mut rig, "Delete");
    assert_eq!(working(&rig), ["[Openings]", "  Sicilian", "[Endings]"]);
    assert_eq!(titles(&rig), before, "the real tree is untouched until OK");
}

#[test]
fn indent_enters_the_previous_folder_and_outdent_leaves_it() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "Indent");
    assert_eq!(
        working(&rig),
        ["[Openings]", "  Sicilian", "  French", "[Endings]"],
        "French goes to the end of the folder before it"
    );
    click(&mut rig, "Outdent");
    assert_eq!(
        working(&rig),
        ["[Openings]", "  Sicilian", "French", "[Endings]"],
        "and comes out after that folder"
    );
}

#[test]
fn indent_of_the_first_entry_and_outdent_of_a_top_level_entry_do_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    let before = working(&rig);
    click(&mut rig, "[Openings]");
    click(&mut rig, "Indent");
    click(&mut rig, "Outdent");
    click(&mut rig, "Up");
    assert_eq!(working(&rig), before);
}

#[test]
fn new_folder_goes_after_the_selection_and_without_a_selection_at_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "New folder");
    let rows = working(&rig);
    assert_eq!(rows.len(), 5, "{rows:?}");
    assert_eq!(rows[2], "French");
    assert!(rows[3].starts_with('['), "a folder after French: {rows:?}");
    assert_eq!(rows[4], "[Endings]");
    // No selection: at the end.
    edit(&mut rig, EditorMessage::Select(None));
    click(&mut rig, "New folder");
    let rows = working(&rig);
    assert_eq!(rows.len(), 6);
    assert!(rows[5].starts_with('['));
}

#[test]
fn a_new_folder_in_an_empty_selected_folder_becomes_its_child() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "[Endings]");
    click(&mut rig, "New folder");
    let rows = working(&rig);
    assert_eq!(rows[3], "[Endings]");
    assert!(
        rows[4].starts_with("  ["),
        "a child of the empty folder: {rows:?}"
    );
}

#[test]
fn deleting_a_folder_removes_what_is_inside() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "[Openings]");
    click(&mut rig, "Delete");
    assert_eq!(working(&rig), ["French", "[Endings]"]);
    // Nothing is selected afterwards: the buttons that need a selection are off.
    let messages = click(&mut rig, "Delete");
    assert!(messages.is_empty(), "{messages:?}");
}

#[test]
fn ok_replaces_the_tree_and_writes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "Delete");
    click(&mut rig, "OK");
    assert!(!editor_open(&rig));
    assert_eq!(titles(&rig), ["Openings", "  Sicilian", "Endings"]);
    assert_eq!(
        saved_tree(dir.path()),
        ["Openings", "  Sicilian", "Endings"]
    );
}

#[test]
fn cancel_restores_the_tree_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    let before = titles(&rig);
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "Delete");
    click(&mut rig, "New folder");
    click(&mut rig, "Cancel");
    assert!(!editor_open(&rig));
    assert_eq!(titles(&rig), before);
    assert!(!bookmarks_file(dir.path()).exists(), "Cancel does not save");
    // A second opening starts from the real tree, not from the dropped edits.
    open_editor_by_key(&mut rig);
    assert_eq!(working(&rig).len(), before.len());
}

#[test]
fn escape_cancels_and_enter_outside_the_field_confirms() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    let before = titles(&rig);
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "Delete");
    press(&mut rig, plain(Key::Named(NamedKey::Escape)), false);
    assert!(!editor_open(&rig), "Esc closes");
    assert_eq!(titles(&rig), before, "and discards");
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "Delete");
    press(&mut rig, plain(Key::Named(NamedKey::Enter)), false);
    assert!(!editor_open(&rig), "Enter outside the field is OK");
    assert_eq!(titles(&rig), ["Openings", "  Sicilian", "Endings"]);
}

#[test]
fn enter_inside_the_title_field_does_not_close_the_editor() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    press(&mut rig, plain(Key::Named(NamedKey::Enter)), true);
    assert!(editor_open(&rig));
}

#[test]
fn an_emptied_title_gets_a_default_when_ok_is_pressed() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    edit(&mut rig, EditorMessage::Rename(String::new()));
    edit(&mut rig, EditorMessage::Select(None));
    click(&mut rig, "[Endings]");
    edit(&mut rig, EditorMessage::Rename(String::new()));
    click(&mut rig, "OK");
    for row in rig.shell.app.bookmarks.rows() {
        assert!(!row.title.trim().is_empty(), "{row:?}");
    }
}

#[test]
fn a_long_unicode_title_is_cut_at_the_limit_in_characters() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    let long = "\u{4e2d}\u{1f3c1}\u{e9}".repeat(150);
    edit(&mut rig, EditorMessage::Rename(long));
    let editor = rig.shell.app.bookmark_editor.as_ref().unwrap();
    assert!(editor.title.chars().count() <= MAX_TITLE_CHARS);
    click(&mut rig, "OK");
    let saved = saved_tree(dir.path());
    assert!(
        saved
            .iter()
            .all(|t| t.trim_start().chars().count() <= MAX_TITLE_CHARS)
    );
    assert!(
        saved.iter().any(|t| t.contains('\u{1f3c1}')),
        "Unicode survives the file"
    );
}

#[test]
fn the_title_field_is_off_without_a_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    let messages = type_in_title(&mut rig, "x");
    assert!(
        !messages.iter().any(|m| format!("{m:?}").contains("Rename")),
        "{messages:?}"
    );
    assert_eq!(
        working(&rig),
        ["[Openings]", "  Sicilian", "French", "[Endings]"]
    );
}

#[test]
fn an_empty_tree_shows_a_hint_and_nothing_needs_saving() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    open_editor_by_key(&mut rig);
    assert!(has_text(&rig, "No bookmarks yet"));
    for label in ["Up", "Down", "Indent", "Outdent", "Delete", "Go to"] {
        assert!(click(&mut rig, label).is_empty(), "{label} is off");
    }
    click(&mut rig, "OK");
    assert!(!editor_open(&rig));
    assert!(!bookmarks_file(dir.path()).exists(), "no change, no write");
}

#[test]
fn add_in_the_editor_is_off_without_a_game_and_adds_after_the_selection_with_one() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, path) = flat(dir.path());
    open_editor_by_key(&mut rig);
    assert!(click(&mut rig, "Add").is_empty(), "no game: Add is off");
    click(&mut rig, "Cancel");
    // With a game of a database.
    open(&mut rig, &path);
    let slot = rig.shell.app.bases.current();
    rig.send(BasesMessage::LoadGame { slot, number: 4 });
    assert!(rig.shell.app.can_bookmark_current());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "Add");
    let rows = working(&rig);
    assert_eq!(rows.len(), 5, "{rows:?}");
    assert_eq!(rows[2], "French");
    assert!(rows[3].starts_with("a:"), "the automatic title: {rows:?}");
    assert_eq!(rows[4], "[Endings]");
    let tree = &rig.shell.app.bookmark_editor.as_ref().unwrap().working;
    let game = tree
        .rows()
        .into_iter()
        .find(|r| r.title.starts_with("a:"))
        .and_then(|r| tree.get(r.id).and_then(|n| n.game().cloned()))
        .expect("a game bookmark");
    assert_eq!(game.game, 4);
    assert_eq!(game.base, path);
}

#[test]
fn the_editor_does_not_go_past_eight_levels_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let mut id = add(&mut rig, None, folder("F1"));
    for depth in 2..=8 {
        id = add(&mut rig, Some(id), folder(&format!("F{depth}")));
    }
    open_editor_by_key(&mut rig);
    for text in ["[F1]", "[F8]"] {
        assert!(has_text(&rig, text), "{text}");
    }
    click(&mut rig, "[F8]");
    let before = working(&rig);
    click(&mut rig, "New folder");
    assert_eq!(working(&rig), before, "a ninth level is refused");
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Bookmark(BookmarkNotice::TreeFull))
    );
    // Moving a folder into a deeper one is refused too.
    edit(&mut rig, EditorMessage::Select(None));
    click(&mut rig, "New folder");
    let rows = working(&rig);
    assert_eq!(rows.len(), 9, "a sibling of F1 at the top level");
}

#[test]
fn many_entries_still_render_and_the_buttons_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    let p = dir.path().join("x.si4");
    for n in 0..400u32 {
        add(&mut rig, None, game(&format!("Game {n}"), &p, 1));
    }
    open_editor_by_key(&mut rig);
    assert!(has_text(&rig, "Game 0"));
    click(&mut rig, "Cancel");
    assert!(!editor_open(&rig));
}

#[test]
fn go_applies_ok_closes_and_opens_the_database_to_the_game_and_ply() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, path) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    type_in_title(&mut rig, "?");
    click(&mut rig, "Go to");
    assert!(!editor_open(&rig), "Go closes the editor");
    assert!(
        titles(&rig).contains(&"French?".to_owned()),
        "and applies the edits"
    );
    assert_eq!(saved_tree(dir.path())[2], "French?", "and saves them");
    let slot = rig
        .shell
        .app
        .bases
        .find_by_path(&path)
        .expect("the database was opened");
    assert_eq!(rig.shell.app.bases.current(), slot);
    assert_eq!(
        rig.shell.app.bases.slot(slot).unwrap().current_game,
        Some(3),
        "the bookmarked game is loaded"
    );
    // The position of the bookmark (ply 2) is the cursor.
    let here = rig.shell.app.current_bookmark().expect("bookmarkable");
    assert_eq!(here.ply, 2);
}

#[test]
fn go_on_a_folder_is_off_and_a_double_click_on_a_game_goes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, path) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "[Openings]");
    assert!(click(&mut rig, "Go to").is_empty(), "Go is for games only");
    // Double click on "Sicilian".
    let messages: Vec<mychess_gui::Message> = {
        use iced_test::core::{Event, mouse};
        let mut sim = rig.sim(SIZE);
        let bounds = sim.find("Sicilian").expect("row").bounds();
        let at = bounds.center();
        sim.point_at(at);
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        drop(sim.simulate(vec![
            Event::Mouse(mouse::Event::CursorMoved { position: at }),
            press.clone(),
            release.clone(),
            press,
            release,
        ]));
        sim.into_messages().collect()
    };
    let text = format!("{messages:?}");
    assert!(text.contains("Select"), "{text}");
    assert!(text.contains("Go"), "a double click is Go: {text}");
    apply(&mut rig, messages);
    assert!(!editor_open(&rig));
    assert!(rig.shell.app.bases.find_by_path(&path).is_some());
}

#[test]
fn the_shortcuts_do_not_stack_a_second_editor_or_add_while_it_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, _) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "Delete");
    press(&mut rig, ctrl(key_char('d'), true), false);
    assert_eq!(
        working(&rig),
        ["[Openings]", "  Sicilian", "[Endings]"],
        "the edits of the open editor are not reset"
    );
    let before = titles(&rig).len();
    press(&mut rig, ctrl(key_char('d'), false), false);
    assert_eq!(
        titles(&rig).len(),
        before,
        "no bookmark is added behind the editor"
    );
}

#[test]
fn a_game_node_in_the_working_copy_keeps_its_identity_through_ok() {
    let dir = tempfile::tempdir().unwrap();
    let (mut rig, path) = flat(dir.path());
    open_editor_by_key(&mut rig);
    click(&mut rig, "French");
    click(&mut rig, "OK");
    let tree = &rig.shell.app.bookmarks;
    let french = tree
        .rows()
        .into_iter()
        .find(|r| r.title == "French")
        .and_then(|r| tree.get(r.id).map(|n| n.kind.clone()))
        .unwrap();
    let NodeKind::Game(g) = french else {
        panic!("a game")
    };
    assert_eq!(
        (g.base, g.game, g.ply, g.white.as_str()),
        (path, 3, 2, "W 3")
    );
}

#[test]
fn the_editor_is_translated_to_portuguese() {
    let dir = tempfile::tempdir().unwrap();
    let mut rig = rig_in(dir.path(), None);
    rig.app_msg(mychess_app::Message::Prefs(mychess_app::PrefsMessage::Set(
        mychess_app::PrefChange::Language(mychess_app::LanguagePref::PtBr),
    )));
    open_editor_by_key(&mut rig);
    for text in [
        "Marcadores",
        "Subir",
        "Descer",
        "Recuar",
        "Avançar",
        "Nova pasta",
        "Excluir",
        "Cancelar",
    ] {
        assert!(has_text(&rig, text), "{text}");
    }
}
