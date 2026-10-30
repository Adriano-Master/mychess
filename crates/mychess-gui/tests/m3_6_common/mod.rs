//! Shared helpers of the `m3_6_*` GUI contract tests (spec
//! `docs/specs/m3-6-marcadores-recentes.md`: bookmarks, recent files,
//! read-only), headless.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

#[path = "../m3_4b_common/mod.rs"]
pub mod b;

use std::path::{Path, PathBuf};

pub use b::{Rig, numbered, open};
use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::{
    BookmarkId, BookmarksMessage, EditorMessage, Folder, GameBookmark, Key, KeyInput,
    LoadedPreferences, Message as AppMessage, Modifiers, NodeKind, Preferences, parse_bookmarks,
};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::{Message, Shell};

pub const SIZE: Size = Size::new(1280.0, 800.0);

/// A real `.si4` database of `count` games (`<prefix> n` as White).
pub fn si4(dir: &Path, name: &str, prefix: &str, count: u32) -> PathBuf {
    use mychess_db::si4::Si4Writer;
    use mychess_db::{CancelToken, ImportOptions, NoProgress, import_pgn};
    let mut pgn = String::new();
    for n in 1..=count {
        pgn.push_str(&format!(
            "[Event \"Ev\"]\n[Site \"Rome, Italy\"]\n[Date \"2001.02.03\"]\n[Round \"{n}\"]\n[White \"{prefix} {n}\"]\n[Black \"Opp\"]\n[Result \"*\"]\n\n1. e4 e5 2. Nf3 Nc6 *\n\n"
        ));
    }
    let base = dir.join(name);
    let mut writer = Si4Writer::create(&base, 0).expect("create si4");
    import_pgn(
        pgn.as_bytes(),
        &mut writer,
        &ImportOptions::default(),
        None,
        &mut NoProgress,
        &CancelToken::new(),
    )
    .expect("import");
    drop(writer);
    base.with_extension("si4")
}

/// A rig whose preferences file (and so `bookmarks.toml`) lives in `dir`.
/// The bookmarks start as `initial` (the text of a `bookmarks.toml`).
pub fn rig_in(dir: &Path, initial: Option<&str>) -> Rig {
    let mut rig = Rig::new();
    rig.shell = Shell::new(Some(dir.join("preferences.toml")));
    let mut prefs = Preferences::default();
    prefs.general.language = mychess_app::LanguagePref::EnUs;
    prefs.general.theme = mychess_app::ThemePref::Light;
    rig.app_msg(AppMessage::PreferencesLoaded(LoadedPreferences {
        prefs,
        issues: Vec::new(),
    }));
    let loaded = initial.map_or_else(Default::default, parse_bookmarks);
    rig.send(AppMessage::Bookmarks(BookmarksMessage::Loaded(loaded)));
    rig
}

pub fn bookmarks_file(dir: &Path) -> PathBuf {
    dir.join("bookmarks.toml")
}

pub fn game(title: &str, base: &Path, number: u32) -> NodeKind {
    NodeKind::Game(GameBookmark {
        title: title.to_owned(),
        base: base.to_path_buf(),
        game: number,
        ply: 2,
        white: format!("W {number}"),
        black: "Opp".to_owned(),
        year: 2001,
        site: "Rome".to_owned(),
        round: number.to_string(),
        result: "*".to_owned(),
    })
}

pub fn folder(title: &str) -> NodeKind {
    NodeKind::Folder(Folder {
        title: title.to_owned(),
        children: Vec::new(),
    })
}

pub fn add(rig: &mut Rig, after: Option<BookmarkId>, kind: NodeKind) -> BookmarkId {
    rig.shell
        .app
        .bookmarks
        .insert_after(after, kind)
        .expect("room in the tree")
}

pub fn titles(rig: &Rig) -> Vec<String> {
    rig.shell
        .app
        .bookmarks
        .rows()
        .into_iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.title))
        .collect()
}

pub fn edit(rig: &mut Rig, message: EditorMessage) {
    rig.send(AppMessage::Bookmarks(BookmarksMessage::Edit(message)));
}

pub fn ctrl(key: Key, shift: bool) -> KeyInput {
    KeyInput {
        key,
        modifiers: Modifiers {
            command: true,
            shift,
            alt: false,
        },
    }
}

pub fn key_char(c: char) -> Key {
    Key::Char(c)
}

/// Sends a key the way the subscription does; the shell's task is dropped.
pub fn press(rig: &mut Rig, input: KeyInput, text_focused: bool) {
    rig.msg(Message::Dialog(DialogMessage::Key {
        input,
        text_focused,
        typed: None,
    }));
}

// ---- the menus ----

pub fn file_entries(rig: &Rig) -> Vec<MenuEntry> {
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-file")
        .expect("a File menu")
        .entries
}

/// The entries of the Database menu (spec M9-7: "Read-only" moved there).
pub fn database_menu_entries(rig: &Rig) -> Vec<MenuEntry> {
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-database")
        .expect("a Database menu")
        .entries
}

pub fn submenu<'a>(entries: &'a [MenuEntry], key: &str) -> &'a [MenuEntry] {
    entries
        .iter()
        .find_map(|e| match e {
            MenuEntry::Submenu {
                label: Label::Key(k),
                entries,
                ..
            } if *k == key => Some(entries.as_slice()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("submenu {key}"))
}

pub struct Found {
    pub enabled: bool,
    pub checked: Option<bool>,
    pub shortcut: bool,
    pub message: Option<Message>,
}

pub fn item(entries: &[MenuEntry], key: &str) -> Found {
    entries
        .iter()
        .find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                action,
                checked,
                shortcut,
                ..
            } if *k == key => Some(Found {
                enabled: action.is_some(),
                checked: *checked,
                shortcut: shortcut.is_some(),
                message: action.clone(),
            }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("item {key}"))
}

pub fn text_items(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item {
                label: Label::Text(t),
                ..
            } => Some(t.clone()),
            _ => None,
        })
        .collect()
}

pub fn text_item<'a>(entries: &'a [MenuEntry], text: &str) -> &'a MenuEntry {
    entries
        .iter()
        .find(|e| matches!(e, MenuEntry::Item { label: Label::Text(t), .. } | MenuEntry::Submenu { label: Label::Text(t), .. } if t == text))
        .unwrap_or_else(|| panic!("entry {text:?}"))
}

// ---- the editor, through the real view ----

/// Feeds shell messages the way the runtime would: application messages run
/// with their effects, the rest go to the shell.
pub fn apply(rig: &mut Rig, messages: Vec<Message>) {
    for message in messages {
        match message {
            Message::App(m) => {
                rig.send(m);
            }
            other => rig.msg(other),
        }
    }
}

/// Clicks the widget with this text in the view and applies what it sent;
/// returns the messages.
pub fn click(rig: &mut Rig, label: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let mut sim = rig.sim(SIZE);
        sim.click(label)
            .unwrap_or_else(|_| panic!("a widget with the text {label:?}"));
        sim.into_messages().collect()
    };
    apply(rig, messages.clone());
    messages
}

/// Types into the title field of the editor.
pub fn type_in_title(rig: &mut Rig, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let mut sim = rig.sim(SIZE);
        sim.click(iced::widget::Id::new(
            mychess_gui::windows::bookmark_editor::BOOKMARKS_TITLE_ID,
        ))
        .expect("the title field");
        sim.typewrite(text);
        sim.into_messages().collect()
    };
    apply(rig, messages.clone());
    messages
}

pub fn has_text(rig: &Rig, text: &str) -> bool {
    rig.sim(SIZE).find(text).is_ok()
}

pub fn has_id(rig: &Rig, id: &'static str) -> bool {
    rig.sim(SIZE).find(iced::widget::Id::new(id)).is_ok()
}

pub fn editor_open(rig: &Rig) -> bool {
    rig.shell.app.bookmark_editor.is_some()
}

/// The rows of the working copy, indented by depth, folders in brackets.
pub fn working(rig: &Rig) -> Vec<String> {
    rig.shell
        .app
        .bookmark_editor
        .as_ref()
        .expect("the editor is open")
        .working
        .rows()
        .into_iter()
        .map(|r| {
            let title = if r.is_folder {
                format!("[{}]", r.title)
            } else {
                r.title
            };
            format!("{}{title}", "  ".repeat(r.depth))
        })
        .collect()
}

pub fn open_editor_by_key(rig: &mut Rig) {
    press(rig, ctrl(key_char('d'), true), false);
    assert!(editor_open(rig), "Ctrl+Shift+D opens the editor");
}

/// The remark the user would see: the shell moves the application's notice
/// to the status bar after each message.
pub fn notice(rig: &Rig) -> Option<mychess_app::AppNotice> {
    rig.shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone())
}
