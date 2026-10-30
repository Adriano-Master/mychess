//! Keyboard, shortcut map and layout contract of the game list panel (spec
//! M3-4 slice 4b: D536, D539, D541, D542, D543; rule 4 of the shortcuts of
//! the Scid `menus.tcl:1587-1619`). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::field_reassign_with_default
)]

mod m3_4b_common;

use std::collections::BTreeMap;

use iced::keyboard::key::Named;
use iced::widget::pane_grid;
use iced::{Size, keyboard, mouse};
use m3_4b_common::common::command_shift;
use m3_4b_common::*;
use mychess_app::{
    Action, BasesMessage, GameMessage, Key, KeyInput, LanguagePref, LayoutNode, LoadedPreferences,
    Message as AppMessage, Modifiers, NamedKey, Nav, PaneKind, PrefChange, Preferences,
    PrefsMessage, ShortcutIssue, Shortcuts, SplitAxis,
};
use mychess_gui::windows::game_list::{GameListMessage, wheel_rows};
use mychess_gui::windows::layout;
use mychess_gui::{Message, UiMessage, keymap};

fn key(rig: &mut Rig, input: KeyInput) {
    rig.key(input, false, None);
}

fn three_slots(rig: &mut Rig, dir: &std::path::Path) {
    for (i, name) in ["a.pgn", "b.pgn", "c.pgn"].iter().enumerate() {
        open(rig, &numbered(dir, name, &format!("F{i}"), 30));
    }
}

// ---- the keys reach the application ----

#[test]
fn the_toolkit_page_keys_are_translated_for_the_application() {
    for (named, expected) in [
        (Named::PageDown, NamedKey::PageDown),
        (Named::PageUp, NamedKey::PageUp),
    ] {
        let got = keymap::key_input(&keyboard::Key::Named(named), keyboard::Modifiers::empty());
        assert_eq!(
            got,
            Some(KeyInput {
                key: Key::Named(expected),
                modifiers: Modifiers::NONE
            }),
            "{named:?} must reach the list, or Page Up/Down never work in the app"
        );
        let with_command =
            keymap::key_input(&keyboard::Key::Named(named), keyboard::Modifiers::COMMAND);
        assert_eq!(
            with_command.map(|k| (k.key, k.modifiers.command)),
            Some((Key::Named(expected), true)),
            "Ctrl/Cmd+{named:?} is the alias of next/previous database"
        );
    }
}

#[test]
fn the_command_modifier_is_the_platform_one_and_tab_and_digits_translate() {
    let tab = keymap::key_input(
        &keyboard::Key::Named(Named::Tab),
        keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
    )
    .expect("Ctrl+Shift+Tab");
    assert_eq!(tab.modifiers, Modifiers::COMMAND_SHIFT);
    let one = keymap::key_input(
        &keyboard::Key::Character("1".into()),
        keyboard::Modifiers::COMMAND,
    )
    .expect("Ctrl+1");
    assert_eq!(one, command_input('1'));
    let grave = keymap::key_input(
        &keyboard::Key::Character("`".into()),
        keyboard::Modifiers::COMMAND,
    )
    .expect("Ctrl+`");
    assert_eq!(grave, command_input('`'));
    let table = Shortcuts::default();
    assert_eq!(table.lookup(&one), Some(Action::SelectBase(1)));
    assert_eq!(table.lookup(&grave), Some(Action::SelectBase(9)));
}

fn command_input(c: char) -> KeyInput {
    KeyInput {
        key: Key::Char(c),
        modifiers: Modifiers::COMMAND,
    }
}

// ---- database switching ----

#[test]
fn control_digits_ignore_empty_slots_and_nine_or_backquote_reach_the_clipbase() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    three_slots(&mut rig, dir.path());
    for n in 1..=3 {
        key(&mut rig, command_input(char::from_digit(n, 10).unwrap()));
        assert_eq!(rig.shell.app.bases.current(), slot(n as usize), "Ctrl+{n}");
    }
    for c in ['4', '5', '6', '7', '8'] {
        key(&mut rig, command_input(c));
        assert_eq!(rig.shell.app.bases.current(), slot(3), "empty slot {c}");
    }
    key(&mut rig, command_input('9'));
    assert!(rig.shell.app.bases.current().is_clipbase());
    key(&mut rig, command_input('2'));
    key(&mut rig, command_input('`'));
    assert!(rig.shell.app.bases.current().is_clipbase());
}

#[test]
fn next_and_previous_database_cycle_skipping_the_empty_slots() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    three_slots(&mut rig, dir.path());
    rig.app_msg(AppMessage::Bases(BasesMessage::Close(slot(2))));
    key(&mut rig, command_input('1'));
    let tab = |modifiers| KeyInput {
        key: Key::Named(NamedKey::Tab),
        modifiers,
    };
    let page = |named, modifiers| KeyInput {
        key: Key::Named(named),
        modifiers,
    };
    let walk = |rig: &mut Rig, input: KeyInput| -> usize {
        key(rig, input);
        rig.shell.app.bases.current().number()
    };
    // Ctrl+Tab: 1 -> 3 (2 is closed) -> 9 -> 1.
    assert_eq!(walk(&mut rig, tab(Modifiers::COMMAND)), 3);
    assert_eq!(walk(&mut rig, tab(Modifiers::COMMAND)), 9);
    assert_eq!(walk(&mut rig, tab(Modifiers::COMMAND)), 1);
    // Ctrl+Shift+Tab and Shift+Tab go back.
    assert_eq!(walk(&mut rig, command_shift(Key::Named(NamedKey::Tab))), 9);
    assert_eq!(
        walk(
            &mut rig,
            tab(Modifiers {
                command: false,
                shift: true,
                alt: false
            })
        ),
        3
    );
    // The macOS alias.
    assert_eq!(
        walk(&mut rig, page(NamedKey::PageDown, Modifiers::COMMAND)),
        9
    );
    assert_eq!(
        walk(&mut rig, page(NamedKey::PageUp, Modifiers::COMMAND)),
        3
    );
}

#[test]
fn control_l_toggles_the_panel_both_ways_and_the_arrangement_follows() {
    let mut rig = Rig::new();
    key(&mut rig, command_input('l'));
    assert!(rig.shell.ui.is_open(PaneKind::GameList));
    assert!(rig.shell.ui.layout().panes().contains(&PaneKind::GameList));
    key(&mut rig, command_input('l'));
    assert!(!rig.shell.ui.is_open(PaneKind::GameList));
    assert_eq!(rig.shell.ui.layout(), LayoutNode::default_layout());
}

// ---- keys of the list ----

fn list_rig(dir: &std::path::Path, games: u32) -> Rig {
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir, "a.pgn", "A", games));
    viewport(&mut rig, 900.0, 240.0);
    rig
}

#[test]
fn page_keys_move_a_page_less_one_and_the_selection_travels_with_them() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = list_rig(dir.path(), 100);
    let id = rig.shell.app.bases.current();
    list(
        &mut rig,
        GameListMessage::Click {
            slot: id,
            number: 3,
        },
    );
    key(&mut rig, plain(Key::Named(NamedKey::PageDown)));
    assert_eq!(current_first(&rig), 10, "10 rows: a page is 9");
    assert_eq!(selected(&rig), Some(12), "the selection travelled");
    key(&mut rig, plain(Key::Named(NamedKey::PageUp)));
    assert_eq!(current_first(&rig), 1);
    assert_eq!(selected(&rig), Some(3));
    key(&mut rig, plain(Key::Named(NamedKey::PageUp)));
    assert_eq!(current_first(&rig), 1, "stops at the top");
    for _ in 0..30 {
        key(&mut rig, plain(Key::Named(NamedKey::PageDown)));
    }
    assert_eq!(
        current_first(&rig),
        91,
        "stops with the last row at the end"
    );
    assert!(selected(&rig).unwrap() <= 100);
}

#[test]
fn page_keys_do_nothing_while_the_panel_is_closed() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = list_rig(dir.path(), 100);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    key(&mut rig, plain(Key::Named(NamedKey::PageDown)));
    assert_eq!(current_first(&rig), 1);
}

#[test]
fn arrows_home_end_and_enter_belong_to_the_board_unless_the_pointer_is_over_the_panel() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = list_rig(dir.path(), 50);
    let id = rig.shell.app.bases.current();
    list(
        &mut rig,
        GameListMessage::Open {
            slot: id,
            number: 2,
        },
    );
    rig.send(GameMessage::Navigate(Nav::GoToNode(rig.game().root())));
    let selected_before = selected(&rig);
    // Pointer elsewhere: Down is "forward 10 plies" and Enter nothing for the list.
    key(&mut rig, plain(Key::Named(NamedKey::Down)));
    key(&mut rig, plain(Key::Named(NamedKey::Enter)));
    key(&mut rig, plain(Key::Named(NamedKey::End)));
    assert_eq!(selected(&rig), selected_before, "list untouched");
    assert_eq!(current_first(&rig), 1);
    assert_ne!(rig.cursor(), rig.game().root(), "the board moved instead");
    // Home goes to the start of the game.
    key(&mut rig, plain(Key::Named(NamedKey::Home)));
    assert_eq!(rig.cursor(), rig.game().root());

    // Pointer over the panel: the list takes them and the board stays.
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    key(&mut rig, plain(Key::Named(NamedKey::Down)));
    assert_eq!(selected(&rig), Some(3));
    key(&mut rig, plain(Key::Named(NamedKey::Up)));
    assert_eq!(selected(&rig), Some(2));
    assert_eq!(rig.cursor(), rig.game().root(), "the board did not move");
    key(&mut rig, plain(Key::Named(NamedKey::End)));
    assert_eq!(selected(&rig), Some(50));
    assert_eq!(current_first(&rig), 41, "scrolled to keep it visible");
    key(&mut rig, plain(Key::Named(NamedKey::Home)));
    assert_eq!(selected(&rig), Some(1));
    assert_eq!(current_first(&rig), 1);
    // Enter loads the selected game.
    key(&mut rig, plain(Key::Named(NamedKey::Down)));
    let enter = rig.shell.ui.games.update(
        GameListMessage::Key(mychess_gui::windows::game_list::ListKey::Enter),
        &rig.shell.app,
    );
    for m in enter {
        rig.send(m);
    }
    assert_eq!(rig.game().tags().white, "A 2");
}

#[test]
fn no_list_key_acts_while_a_text_field_has_the_focus() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = list_rig(dir.path(), 100);
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    for named in [
        NamedKey::Down,
        NamedKey::Up,
        NamedKey::Home,
        NamedKey::End,
        NamedKey::Enter,
        NamedKey::PageDown,
        NamedKey::PageUp,
    ] {
        rig.key(plain(Key::Named(named)), true, None);
    }
    assert_eq!(current_first(&rig), 1);
    assert_eq!(selected(&rig), None);
}

#[test]
fn modified_arrows_are_not_list_keys() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = list_rig(dir.path(), 100);
    rig.msg(Message::Games(GameListMessage::Hover(true)));
    // Ctrl+Down is "next game of the filter" in the Scid, never a selection step.
    key(&mut rig, command(Key::Named(NamedKey::Down)));
    assert_eq!(selected(&rig), None);
    assert_eq!(current_first(&rig), 1);
}

#[test]
fn the_wheel_moves_three_rows_a_notch_through_the_widget() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = list_rig(dir.path(), 100);
    assert_eq!(wheel_rows(mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }), 3);
    let mut sim = rig.sim(Size::new(1280.0, 800.0));
    let list_bounds = sim
        .find(iced::widget::Id::new("games-list"))
        .expect("list")
        .bounds();
    sim.point_at(list_bounds.center());
    let events = [iced::Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
    })];
    sim.simulate(events);
    let messages: Vec<Message> = sim.into_messages().collect();
    let rows: Vec<i64> = messages
        .iter()
        .filter_map(|m| match m {
            Message::Games(GameListMessage::Scroll { rows, .. }) => Some(*rows),
            _ => None,
        })
        .collect();
    assert_eq!(rows, vec![3], "{messages:?}");
    for m in messages {
        if let Message::Games(m) = m {
            list(&mut rig, m);
        }
    }
    assert_eq!(current_first(&rig), 4);
}

// ---- the central map of shortcuts ----

#[test]
fn every_default_binding_belongs_to_one_action_and_the_new_ones_are_present() {
    let table = Shortcuts::default();
    let mut seen: Vec<(Key, Modifiers, Action)> = Vec::new();
    for (action, keys) in table.iter() {
        for b in keys {
            if let Some((_, _, other)) = seen
                .iter()
                .find(|(k, m, _)| *k == b.key && *m == b.modifiers)
            {
                panic!("{b} is bound to {action:?} and {other:?}");
            }
            seen.push((b.key, b.modifiers, action));
            // Every binding resolves back to its action.
            assert_eq!(
                table.lookup(&KeyInput {
                    key: b.key,
                    modifiers: b.modifiers
                }),
                Some(action),
                "{b}"
            );
        }
    }
    for n in 1..=9u8 {
        assert!(
            !table.bindings(Action::SelectBase(n)).is_empty() || n != 9,
            "slot {n} has a key"
        );
    }
    for action in [
        Action::NextBase,
        Action::PreviousBase,
        Action::ToggleGameList,
    ] {
        assert!(!table.bindings(action).is_empty(), "{action:?}");
    }
    // Existing M2 shortcuts are intact.
    assert_eq!(
        table.lookup(&command_input('o')),
        Some(Action::OpenFile),
        "Ctrl+O"
    );
    assert_eq!(
        table.lookup(&command_input('i')),
        Some(Action::ToggleGameInfo)
    );
    assert_eq!(
        table.lookup(&command_input('p')),
        Some(Action::ShowNotation)
    );
}

#[test]
fn only_next_previous_and_toggle_are_configurable_among_the_new_actions() {
    let table = Shortcuts::default();
    let set = |pairs: &[(&str, &str)]| -> (Shortcuts, Vec<ShortcutIssue>) {
        let map: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        table.clone().with_overrides(&map)
    };
    let (t, issues) = set(&[
        ("NextBase", "Command+J"),
        ("PreviousBase", "Command+K"),
        ("ToggleGameList", "Command+Shift+Q"),
    ]);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(t.lookup(&command_input('j')), Some(Action::NextBase));
    assert_eq!(t.lookup(&command_input('k')), Some(Action::PreviousBase));
    assert_eq!(
        t.lookup(&command_shift(Key::Char('q'))),
        Some(Action::ToggleGameList)
    );
    assert_eq!(t.lookup(&command_input('l')), None, "the old key is gone");

    // SelectBase has no configuration: the table keeps its keys.
    let (t, _) = set(&[("SelectBase", "Command+K"), ("SelectBase1", "Command+K")]);
    assert_eq!(t.lookup(&command_input('1')), Some(Action::SelectBase(1)));
    assert_eq!(t.lookup(&command_input('k')), None);

    // A key in use is refused.
    let (t, issues) = set(&[("ToggleGameList", "Command+G")]);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, ShortcutIssue::Conflict { .. })),
        "{issues:?}"
    );
    assert_eq!(t.lookup(&command_input('l')), Some(Action::ToggleGameList));

    // An empty text turns the action off.
    let (t, _) = set(&[("ToggleGameList", "")]);
    assert_eq!(t.lookup(&command_input('l')), None);
}

// ---- layout (D536, D213) ----

/// `[ [Board | Moves/Info/Comment] / Games ]`.
fn with_games() -> LayoutNode {
    LayoutNode::default_layout().open(PaneKind::GameList)
}

#[test]
fn the_pane_kind_is_called_games_in_the_toml_and_comes_back() {
    let mut prefs = Preferences::default();
    prefs.layout = Some(with_games());
    let text = mychess_app::render_preferences(&prefs);
    assert!(
        text.contains("pane = \"games\"") || text.contains("pane = 'games'"),
        "{text}"
    );
    let back = mychess_app::load_preferences(&text);
    assert!(back.issues.is_empty(), "{:?}", back.issues);
    assert_eq!(back.prefs.layout, prefs.layout);
}

#[test]
fn a_layout_saved_before_the_panel_still_loads_and_stays_valid() {
    for old in [
        LayoutNode::default_layout(),
        LayoutNode::legacy_default_layout(),
        LayoutNode::pane(PaneKind::Board),
    ] {
        let mut prefs = Preferences::default();
        prefs.layout = Some(old.clone());
        let text = mychess_app::render_preferences(&prefs);
        // The pane is named by the `pane` key as a quoted TOML string; a bare
        // `games` key (the tactical statistics) or a value such as the
        // piece tracker `mode = "games"` is not the panel.
        assert!(
            !text.contains("pane = \"games\"") && !text.contains("pane = 'games'"),
            "an old layout has no panel: {text}"
        );
        let back = mychess_app::load_preferences(&text);
        assert!(back.issues.is_empty(), "{:?}", back.issues);
        let loaded = back.prefs.layout.clone().expect("a layout");
        assert!(loaded.is_valid());
        // Shown by the shell: no panel appears by itself.
        let mut rig = Rig::new();
        rig.app_msg(AppMessage::PreferencesLoaded(back));
        assert!(!rig.shell.ui.is_open(PaneKind::GameList));
        assert!(!has_id(&rig, "games-pane"));
    }
}

#[test]
fn an_unknown_pane_name_in_the_toml_falls_back_to_the_default_layout() {
    let text = "[layout]\ntype = \"split\"\naxis = \"horizontal\"\nratio = 0.6\n\
                [layout.a]\ntype = \"pane\"\npane = \"board\"\n\
                [layout.b]\ntype = \"pane\"\npane = \"games2\"\n";
    let loaded = mychess_app::load_preferences(text);
    assert!(
        loaded
            .prefs
            .layout
            .as_ref()
            .is_none_or(LayoutNode::is_valid)
    );
}

#[test]
fn the_games_pane_twice_or_without_the_board_is_invalid() {
    let pane = |k| Box::new(LayoutNode::pane(k));
    let twice = LayoutNode::Split {
        axis: SplitAxis::Vertical,
        ratio: 0.5,
        a: Box::new(with_games()),
        b: pane(PaneKind::GameList),
    };
    assert!(!twice.is_valid());
    let no_board = LayoutNode::Split {
        axis: SplitAxis::Vertical,
        ratio: 0.5,
        a: pane(PaneKind::Notation),
        b: pane(PaneKind::GameList),
    };
    assert!(!no_board.is_valid());
    assert_eq!(
        layout::or_default(Some(&twice)),
        LayoutNode::default_layout()
    );
}

#[test]
fn opening_the_game_list_wraps_any_layout_and_leaves_the_rest_alone() {
    for without in [
        LayoutNode::default_layout(),
        LayoutNode::legacy_default_layout(),
        LayoutNode::pane(PaneKind::Board),
        LayoutNode::default_layout().close(PaneKind::Notation),
    ] {
        assert!(without.is_valid());
        let with = without.open(PaneKind::GameList);
        assert!(with.is_valid(), "{without:?}");
        let panes = with.panes();
        assert_eq!(panes.last(), Some(&PaneKind::GameList), "at the bottom");
        assert_eq!(
            panes.iter().filter(|p| **p == PaneKind::GameList).count(),
            1
        );
        let LayoutNode::Split { axis, ratio, a, b } = with else {
            panic!("a split");
        };
        assert_eq!(axis, SplitAxis::Vertical, "full width, under the rest");
        assert!((ratio - 0.58).abs() < 1e-3, "{ratio}");
        assert_eq!(*a, without);
        assert_eq!(*b, LayoutNode::pane(PaneKind::GameList));
    }
}

#[test]
fn the_shell_arrangement_is_the_old_default_when_closed_and_the_split_when_open() {
    let mut rig = Rig::new();
    assert_eq!(rig.shell.ui.layout(), LayoutNode::default_layout());
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert_eq!(rig.shell.ui.layout(), with_games());
    assert_eq!(rig.shell.app.prefs.layout, Some(with_games()), "remembered");
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    assert_eq!(rig.shell.ui.layout(), LayoutNode::default_layout());
}

#[test]
fn toggling_the_other_panes_keeps_the_games_panel_and_a_valid_layout() {
    let mut rig = Rig::new();
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::GameList)));
    let steps = [
        PaneKind::Info,
        PaneKind::Notation,
        PaneKind::Info,
        PaneKind::Notation,
        PaneKind::Notation,
    ];
    for kind in steps {
        rig.msg(Message::Ui(UiMessage::TogglePane(kind)));
        let layout = rig.shell.ui.layout();
        assert!(layout.is_valid(), "{layout:?}");
        assert!(rig.shell.ui.is_open(PaneKind::GameList));
        assert!(layout.panes().contains(&PaneKind::GameList));
        assert!(layout.panes().contains(&PaneKind::Board));
    }
}

#[test]
fn a_saved_layout_with_the_panel_opens_it_at_startup_and_a_resize_is_remembered() {
    let mut prefs = Preferences::default();
    prefs.general.language = LanguagePref::EnUs;
    prefs.layout = Some(with_games());
    let mut rig = Rig::new();
    rig.app_msg(AppMessage::PreferencesLoaded(LoadedPreferences {
        prefs,
        issues: Vec::new(),
    }));
    assert!(rig.shell.ui.is_open(PaneKind::GameList));
    assert!(has_id(&rig, "games-pane"));

    // Drag the separator between the top panels and the list.
    let split = match rig.shell.ui.panes.layout() {
        pane_grid::Node::Split { id, axis, .. } => {
            assert_eq!(*axis, pane_grid::Axis::Horizontal);
            *id
        }
        pane_grid::Node::Pane(_) => panic!("a split"),
    };
    rig.msg(Message::Ui(UiMessage::PaneResized(
        pane_grid::ResizeEvent { split, ratio: 0.7 },
    )));
    let saved = rig.shell.app.prefs.layout.clone().expect("saved");
    let LayoutNode::Split { ratio, .. } = saved else {
        panic!("a split");
    };
    assert!((ratio - 0.7).abs() < 1e-3, "{ratio}");
    // And it round-trips through the file.
    let text = mychess_app::render_preferences(&rig.shell.app.prefs);
    let back = mychess_app::load_preferences(&text);
    assert_eq!(back.prefs.layout, rig.shell.app.prefs.layout);
}

#[test]
fn the_view_with_the_panel_open_builds_in_every_pane_combination() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 40));
    for kind in [
        PaneKind::Info,
        PaneKind::Notation,
        PaneKind::Info,
        PaneKind::Notation,
    ] {
        rig.msg(Message::Ui(UiMessage::TogglePane(kind)));
        assert!(has_id(&rig, "games-pane"), "after toggling {kind:?}");
    }
}

#[test]
fn switching_the_language_at_run_time_retranslates_the_open_panel() {
    let dir = tempfile::tempdir().expect("dir");
    let mut rig = Rig::new();
    open(&mut rig, &numbered(dir.path(), "a.pgn", "A", 12));
    viewport(&mut rig, 900.0, 240.0);
    assert!(has_text(&rig, "White") && has_text(&rig, "read-only"));
    rig.app_msg(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Language(
        LanguagePref::PtBr,
    ))));
    assert!(has_text(&rig, "Brancas") && has_text(&rig, "somente leitura"));
    assert!(!has_text(&rig, "White"));
}
