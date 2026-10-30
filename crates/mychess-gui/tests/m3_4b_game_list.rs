//! The game list panel and the database tabs (spec M3-4, slice 4b,
//! decisions D536 to D548): sanity tests of the shell, headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::redundant_closure_for_method_calls
)]

#[path = "m2_4_common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};

use common::{Rig, command, named, plain};
use iced::{Size, widget};
use mychess_app::{
    BasesMessage, Effect, Key, Message as AppMessage, Modifiers, NamedKey, SlotId, perform,
};
use mychess_gui::windows::dialog_ui::DialogMessage;
use mychess_gui::windows::game_list::{GameListMessage, ListKey};
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::{Message, UiMessage};

/// A PGN file of `count` games, game `n` played by `White n`.
fn pgn_with(dir: &Path, name: &str, count: u32) -> PathBuf {
    let mut text = String::new();
    for n in 1..=count {
        text.push_str(&format!(
            "[Event \"Event {n}\"]\n[White \"White {n}\"]\n[Black \"Black {n}\"]\n\
             [Result \"1-0\"]\n\n1. e4 e5 2. Nf3 1-0\n\n"
        ));
    }
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write the pgn");
    path
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// Sends `message` to the application and runs its effects to completion,
/// feeding the answers back (host effects are ignored). The shell is not
/// involved, so it cannot react: use it for what the application decides.
fn send(rig: &mut Rig, message: AppMessage) {
    let rt = runtime();
    let mut queue = vec![message];
    while let Some(message) = queue.pop() {
        for effect in mychess_app::update(&mut rig.shell.app, message) {
            if !effect.runs_on_host() {
                queue.push(rt.block_on(perform(effect)));
            }
        }
    }
    rig.sync();
}

/// Opens `path` as the shell would: the request goes to the application and
/// the answer comes back through the shell, which may open the panel.
fn open(rig: &mut Rig, path: &Path) {
    let rt = runtime();
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Bases(BasesMessage::Open(path.to_path_buf())),
    );
    for effect in effects {
        assert!(matches!(effect, Effect::OpenBase { .. }));
        let answer = rt.block_on(perform(effect));
        rig.app_msg(answer);
    }
}

fn app_rig() -> Rig {
    Rig::new()
}

fn has(rig: &Rig, id: &'static str) -> bool {
    rig.sim(Size::new(1280.0, 800.0))
        .find(widget::Id::new(id))
        .is_ok()
}

#[test]
fn opening_a_multi_game_pgn_shows_the_panel_with_its_tab_and_rows() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    assert!(!has(&rig, "games-pane"), "closed until a database opens");
    open(&mut rig, &pgn_with(dir.path(), "a.pgn", 30));
    assert!(rig.shell.ui.is_open(PaneKind::GameList));
    assert!(has(&rig, "games-pane"));
    assert!(has(&rig, "games-tab-1"));
    assert!(has(&rig, "games-tab-9"), "the Clipbase is always there");
    assert!(has(&rig, "games-row-1"));
    // D552: a panel the application opened is not saved by itself.
    assert_eq!(rig.shell.app.prefs.layout, None);
    assert!(
        !rig.shell
            .ui
            .layout_to_save()
            .panes()
            .contains(&PaneKind::GameList)
    );
}

#[test]
fn a_single_game_pgn_opens_no_panel_and_its_game_loads() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "one.pgn", 1));
    assert!(
        !rig.shell.ui.is_open(PaneKind::GameList),
        "one game: no list"
    );
    let slot = rig.shell.app.bases.current();
    assert_eq!(rig.shell.app.bases.slot(slot).map(|s| s.total()), Some(1));
    // The shell drops the effect of the answer (it only runs as a task);
    // the application asks for the game itself, so run that here.
    send(
        &mut rig,
        AppMessage::Bases(BasesMessage::LoadGame { slot, number: 1 }),
    );
    assert_eq!(rig.game().tags().white, "White 1");
}

#[test]
fn only_the_visible_rows_become_widgets_and_the_viewport_decides_how_many() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "big.pgn", 500));
    // Twelve rows until the sensor reports the real size.
    assert!(has(&rig, "games-row-12"));
    assert!(!has(&rig, "games-row-13"));
    rig.msg(Message::Games(GameListMessage::Viewport(Size::new(
        900.0, 240.0,
    ))));
    assert!(has(&rig, "games-row-10"));
    assert!(!has(&rig, "games-row-11"));
}

#[test]
fn scrolling_moves_the_window_of_rows() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "big.pgn", 100));
    rig.msg(Message::Games(GameListMessage::Viewport(Size::new(
        900.0, 240.0,
    ))));
    let slot = rig.shell.app.bases.current();
    rig.msg(Message::Games(GameListMessage::Scroll { slot, rows: 20 }));
    assert!(!has(&rig, "games-row-1"));
    assert!(has(&rig, "games-row-21"));
    assert!(has(&rig, "games-row-30"));
    assert!(!has(&rig, "games-row-31"));
    rig.msg(Message::Games(GameListMessage::Scroll { slot, rows: 9999 }));
    assert!(has(&rig, "games-row-100"), "the last game can be reached");
}

#[test]
fn tabs_switch_the_list_and_keep_the_position_of_each() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "a.pgn", 50));
    open(&mut rig, &pgn_with(dir.path(), "b.pgn", 20));
    rig.msg(Message::Games(GameListMessage::Viewport(Size::new(
        900.0, 240.0,
    ))));
    let second = SlotId::new(1).unwrap();
    assert_eq!(
        rig.shell.app.bases.current(),
        second,
        "the newest is current"
    );
    let first = SlotId::new(0).unwrap();
    rig.msg(Message::Games(GameListMessage::Scroll {
        slot: second,
        rows: 5,
    }));
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(first)));
    assert_eq!(rig.shell.app.bases.current(), first);
    assert!(has(&rig, "games-row-1"), "the list of a.pgn starts at 1");
    rig.msg(Message::Games(GameListMessage::Scroll {
        slot: first,
        rows: 10,
    }));
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(second)));
    assert!(has(&rig, "games-row-6"), "b.pgn kept its position");
    rig.app_msg(AppMessage::Bases(BasesMessage::Select(first)));
    assert!(has(&rig, "games-row-11"), "and so did a.pgn");
}

#[test]
fn clicking_a_tab_selects_that_database() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "a.pgn", 10));
    open(&mut rig, &pgn_with(dir.path(), "b.pgn", 10));
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click("a.pgn").expect("the tab of a.pgn");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(messages.iter().any(|m| matches!(
        m,
        Message::Games(GameListMessage::TabClick(slot)) if *slot == SlotId::new(0).unwrap()
    )));
}

#[test]
fn a_double_click_on_a_row_asks_for_the_game() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "a.pgn", 10));
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    sim.click(widget::Id::new("games-row-3")).expect("row 3");
    sim.click(widget::Id::new("games-row-3"))
        .expect("row 3 again");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Games(GameListMessage::Open { number: 3, .. }))),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Games(GameListMessage::Click { number: 3, .. })))
    );
}

fn key(rig: &mut Rig, input: mychess_app::KeyInput) {
    rig.msg(Message::Dialog(DialogMessage::Key {
        input,
        text_focused: false,
        typed: None,
    }));
}

#[test]
fn control_digits_and_tab_walk_the_open_databases() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "a.pgn", 10));
    open(&mut rig, &pgn_with(dir.path(), "b.pgn", 10));
    let slot = |n| SlotId::from_number(n).unwrap();
    key(&mut rig, command(Key::Char('1')));
    assert_eq!(rig.shell.app.bases.current(), slot(1));
    key(&mut rig, command(Key::Char('9')));
    assert_eq!(rig.shell.app.bases.current(), slot(9));
    key(&mut rig, command(Key::Char('`')));
    assert_eq!(rig.shell.app.bases.current(), slot(9));
    key(&mut rig, command(Key::Char('5')));
    assert_eq!(
        rig.shell.app.bases.current(),
        slot(9),
        "an empty slot is ignored"
    );
    key(&mut rig, command(Key::Named(NamedKey::Tab)));
    assert_eq!(rig.shell.app.bases.current(), slot(1), "wraps to the first");
    key(&mut rig, command(Key::Named(NamedKey::Tab)));
    assert_eq!(rig.shell.app.bases.current(), slot(2));
    let shift_tab = mychess_app::KeyInput {
        key: Key::Named(NamedKey::Tab),
        modifiers: Modifiers {
            command: false,
            shift: true,
            alt: false,
        },
    };
    key(&mut rig, shift_tab);
    assert_eq!(rig.shell.app.bases.current(), slot(1));
    key(
        &mut rig,
        mychess_app::KeyInput {
            key: Key::Named(NamedKey::Tab),
            modifiers: Modifiers::COMMAND_SHIFT,
        },
    );
    assert_eq!(
        rig.shell.app.bases.current(),
        slot(9),
        "back from the first"
    );
}

#[test]
fn list_keys_need_the_pointer_except_the_page_keys() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "a.pgn", 50));
    rig.msg(Message::Games(GameListMessage::Viewport(Size::new(
        900.0, 240.0,
    ))));
    let first = |rig: &Rig| rig.shell.app.bases.current_slot().unwrap().list.first;
    key(&mut rig, named(NamedKey::PageDown));
    assert_eq!(first(&rig), 10, "Page Down works anywhere");
    // Down is "forward 10 plies" over the board.
    key(&mut rig, named(NamedKey::Home));
    assert_eq!(first(&rig), 10, "Home goes to the game start, not the list");
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    key(&mut rig, named(NamedKey::Home));
    assert_eq!(first(&rig), 1);
    let selected = |rig: &Rig| rig.shell.app.bases.current_slot().unwrap().list.selected;
    assert_eq!(selected(&rig), Some(1), "Home selects the first game");
    key(&mut rig, named(NamedKey::Down));
    assert_eq!(selected(&rig), Some(2));
    key(&mut rig, plain(Key::Named(NamedKey::End)));
    assert_eq!(
        rig.shell.app.bases.current_slot().unwrap().list.selected,
        Some(50)
    );
    rig.msg(Message::Games(GameListMessage::Key(ListKey::Home)));
    assert_eq!(first(&rig), 1);
}

#[test]
fn the_panel_toggles_from_the_menu_message_and_the_shortcut() {
    let mut rig = app_rig();
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
    key(&mut rig, command(Key::Char('l')));
    assert!(rig.shell.ui.is_open(PaneKind::GameList));
    assert!(has(&rig, "games-pane"));
    assert!(has(&rig, "games-tab-9"), "even the lone Clipbase has a tab");
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
    assert!(!has(&rig, "games-pane"));
}

#[test]
fn opening_shows_progress_and_a_button_that_cancels() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    let path = pgn_with(dir.path(), "a.pgn", 5);
    // The shell turns the effect into a task that is never run here, so the
    // open stays in flight.
    rig.app_msg(AppMessage::Bases(BasesMessage::Open(path)));
    assert!(rig.shell.app.base_opening().is_some());
    {
        let mut sim = rig.sim(Size::new(1280.0, 800.0));
        assert!(sim.find("Opening a.pgn...").is_ok());
        sim.click("Cancel").expect("the cancel button");
        let messages: Vec<Message> = sim.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::App(AppMessage::Bases(BasesMessage::CancelOpen))))
        );
    }
    rig.app_msg(AppMessage::Bases(BasesMessage::CancelOpen));
    assert!(rig.shell.app.base_opening().unwrap().is_cancelled());
}

#[test]
fn the_clipbase_menu_items_follow_the_current_database() {
    use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    let enabled = |rig: &Rig, key: &str| -> bool {
        let specs = menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options);
        specs
            .iter()
            .flat_map(|spec| spec.entries.iter())
            .find_map(|entry| match entry {
                MenuEntry::Item {
                    label: Label::Key(k),
                    action,
                    ..
                } if *k == key => Some(action.is_some()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{key} is in the menu"))
    };
    assert!(!enabled(&rig, "menu-edit-clipbase-copy"), "already in it");
    assert!(enabled(&rig, "menu-edit-clipbase-clear"));
    open(&mut rig, &pgn_with(dir.path(), "a.pgn", 10));
    assert!(enabled(&rig, "menu-edit-clipbase-copy"));
    assert!(!enabled(&rig, "menu-edit-clipbase-paste"), "empty Clipbase");
    send(&mut rig, AppMessage::Bases(BasesMessage::ClipbaseCopy));
    assert!(enabled(&rig, "menu-edit-clipbase-paste"));
    assert!(has(&rig, "games-pane"));
}

#[test]
fn closing_a_tab_goes_back_to_the_clipbase() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = app_rig();
    open(&mut rig, &pgn_with(dir.path(), "a.pgn", 10));
    let slot = rig.shell.app.bases.current();
    assert!(!slot.is_clipbase());
    rig.app_msg(AppMessage::Bases(BasesMessage::Close(slot)));
    assert!(rig.shell.app.bases.current().is_clipbase());
    assert!(!has(&rig, "games-tab-1"));
}
