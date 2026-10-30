//! M5-2a GUI contract: opening and closing the opening tree panel by the
//! shortcut, the menus, the Close button, the saved layout and the layout
//! reset (spec `docs/specs/m5-2-janela-da-arvore.md`, D980-D984), with the
//! real search behind it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod m5_2_gui_common;

use m5_2_gui_common::*;
use mychess_app::{Binding, LoadedPreferences, Preferences};
use mychess_gui::keymap::{Platform, binding_label};
use mychess_gui::windows::layout::{LayoutNode, SplitAxis};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use tempfile::tempdir;

type Found<'a> = (Option<&'a Message>, Option<bool>, Option<&'a Binding>);

fn find_item<'a>(entries: &'a [MenuEntry], key: &str) -> Found<'a> {
    fn walk<'a>(entries: &'a [MenuEntry], key: &str) -> Option<Found<'a>> {
        entries.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                action,
                checked,
                shortcut,
                ..
            } if *k == key => Some((action.as_ref(), *checked, shortcut.as_ref())),
            MenuEntry::Submenu { entries, .. } => walk(entries, key),
            _ => None,
        })
    }
    walk(entries, key).unwrap_or_else(|| panic!("menu item {key}"))
}

fn menu(rig: &TreeRig, key: &str) -> Vec<MenuEntry> {
    menu_specs(&rig.rig.shell.app, &rig.rig.shell.ui, rig.rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == key)
        .unwrap_or_else(|| panic!("menu {key}"))
        .entries
}

fn in_layout(rig: &TreeRig) -> bool {
    rig.rig.shell.ui.layout().panes().contains(&PaneKind::Tree)
}

#[test]
fn the_panel_is_closed_by_default_and_the_app_does_not_search() {
    let rig = TreeRig::new();
    assert!(!rig.panel_open());
    assert!(!in_layout(&rig));
    assert!(!rig.app().tree().is_open());
    assert_eq!(rig.view().body, TreeBody::Closed);
    assert!(!rig.shows("TOTAL"));
}

#[test]
fn the_shortcut_opens_the_panel_runs_the_search_and_closes_it_again() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    assert!(rig.panel_open() && in_layout(&rig));
    assert!(rig.app().tree().is_open(), "sync_tree told the app");
    assert!(rig.rig.shell.ui.layout().is_valid());
    // The search ran for real, through the tasks of the shell.
    let view = rig.view();
    assert_eq!(view.body, TreeBody::Rows);
    assert!(!view.searching);
    assert_eq!(view.rows.len(), 3);
    assert!(rig.shows("TOTAL") && rig.shows("e4") && rig.shows("Nf3") && rig.shows("d4"));
    rig.open_panel();
    assert!(!rig.panel_open() && !in_layout(&rig));
    assert!(!rig.app().tree().is_open());
    assert_eq!(rig.view().body, TreeBody::Closed);
    assert!(!rig.shows("TOTAL"));
}

#[test]
fn the_shortcut_is_command_shift_t_and_plain_command_t_stays_the_trial_mode() {
    let rig = TreeRig::new();
    let bindings = rig.app().shortcuts.bindings(AppAction::ToggleTreePane);
    assert_eq!(bindings.len(), 1);
    assert_eq!(binding_label(&bindings[0], Platform::Linux), "Ctrl+Shift+T");
    assert_eq!(binding_label(&bindings[0], Platform::MacOs), "⇧⌘T");
    let mut rig = TreeRig::new();
    rig.key(command(Key::Char('t')));
    assert!(!rig.panel_open(), "Command+T is not the tree (D980)");
}

#[test]
fn the_tools_menu_item_shows_state_and_shortcut_and_toggles_the_panel() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    let entries = menu(&rig, "menu-tools");
    let (action, checked, shortcut) = find_item(&entries, "menu-tools-tree");
    assert_eq!(checked, Some(false));
    assert!(shortcut.is_some());
    let action = action.expect("an action").clone();
    rig.msg(action.clone());
    assert!(rig.panel_open() && rig.app().tree().is_open());
    assert_eq!(rig.view().body, TreeBody::Rows);
    let (_, checked, _) = find_item(&menu(&rig, "menu-tools"), "menu-tools-tree");
    assert_eq!(checked, Some(true));
    rig.msg(action);
    assert!(!rig.panel_open() && !rig.app().tree().is_open());
}

#[test]
fn the_view_panes_submenu_entry_toggles_the_panel_and_shows_the_check() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    let entries = menu(&rig, "menu-view");
    let (action, checked, _) = find_item(&entries, "menu-view-pane-tree");
    assert_eq!(checked, Some(false));
    let action = action.expect("an action").clone();
    assert!(matches!(
        action,
        Message::Ui(UiMessage::TogglePane(PaneKind::Tree))
    ));
    rig.msg(action.clone());
    assert!(rig.app().tree().is_open());
    let (_, checked, _) = find_item(&menu(&rig, "menu-view"), "menu-view-pane-tree");
    assert_eq!(checked, Some(true));
    rig.msg(action);
    assert!(!rig.app().tree().is_open());
}

#[test]
fn the_close_button_of_the_panel_sends_the_pane_toggle_and_closes_it() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    // The button has no text: find it by the tooltip-less icon is not
    // possible, so press the widgets of the toolbar until the toggle shows.
    let messages: Vec<Message> = {
        let mut sim = rig.sim();
        // The bar sits at the top of the panel, the button at its right end.
        let bounds = sim.find("Short display").expect("toolbar").bounds();
        let row_y = bounds.y + bounds.height / 2.0;
        let mut x = bounds.x + bounds.width;
        let end = SIZE.width;
        while x < end {
            sim.point_at(iced::Point::new(x, row_y));
            drop(sim.simulate([
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )),
            ]));
            x += 3.0;
        }
        sim.into_messages().collect()
    };
    assert!(
        sent(
            &messages,
            &Message::Ui(UiMessage::TogglePane(PaneKind::Tree))
        ),
        "no Close button found along the toolbar row: {messages:?}"
    );
    rig.ui(UiMessage::TogglePane(PaneKind::Tree));
    assert!(!rig.panel_open() && !rig.app().tree().is_open());
}

#[test]
fn the_panel_docks_to_the_right_with_the_rest_at_56_percent() {
    let mut rig = TreeRig::new();
    rig.open_panel();
    match rig.rig.shell.ui.layout() {
        LayoutNode::Split { axis, ratio, b, .. } => {
            assert_eq!(axis, SplitAxis::Horizontal, "side by side");
            assert!((ratio - 0.56).abs() < 0.01, "D984: 0.56, got {ratio}");
            assert_eq!(*b, LayoutNode::pane(PaneKind::Tree));
        }
        other @ LayoutNode::Group { .. } => panic!("{other:?}"),
    }
}

#[test]
fn the_pane_survives_the_other_panes_and_the_layout_stays_valid() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    for kind in [
        PaneKind::Info,
        PaneKind::Analysis,
        PaneKind::Graph,
        PaneKind::GameList,
    ] {
        rig.ui(UiMessage::TogglePane(kind));
        assert!(in_layout(&rig), "{kind:?} on");
        assert!(rig.rig.shell.ui.layout().is_valid(), "{kind:?} on");
        assert!(rig.app().tree().is_open(), "{kind:?} on");
    }
    for kind in [
        PaneKind::Info,
        PaneKind::Analysis,
        PaneKind::Graph,
        PaneKind::GameList,
    ] {
        rig.ui(UiMessage::TogglePane(kind));
        assert!(in_layout(&rig), "{kind:?} off");
        assert!(rig.rig.shell.ui.layout().is_valid(), "{kind:?} off");
    }
    assert!(rig.app().tree().is_open());
    assert_eq!(rig.view().body, TreeBody::Rows);
}

#[test]
fn the_pane_goes_into_the_saved_layout_and_survives_a_round_trip() {
    let mut rig = TreeRig::new();
    rig.open_panel();
    let saved = rig.app().prefs.layout.clone().expect("layout remembered");
    assert!(saved.panes().contains(&PaneKind::Tree));
    let text = mychess_app::render_preferences(&rig.app().prefs);
    let loaded = mychess_app::load_preferences(&text);
    assert_eq!(loaded.prefs.layout, Some(saved));
}

#[test]
fn a_saved_layout_with_the_tree_restores_the_pane_and_starts_the_search() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    let wanted = LayoutNode::default_layout().open(PaneKind::Tree);
    let mut prefs: Preferences = rig.app().prefs.clone();
    prefs.layout = Some(wanted);
    rig.app_msg(AppMessage::PreferencesLoaded(LoadedPreferences {
        prefs,
        issues: Vec::new(),
    }));
    assert!(
        rig.panel_open() && in_layout(&rig),
        "PaneKind::Tree restored"
    );
    assert!(rig.app().tree().is_open(), "the app was told");
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert_eq!(rig.view().rows.len(), 3);
    assert!(rig.shows("TOTAL"));
}

#[test]
fn resetting_the_layout_closes_the_pane_and_the_app_follows() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::with_std(dir.path());
    rig.open_panel();
    rig.ui(UiMessage::ResetLayout);
    assert!(!rig.panel_open() && !in_layout(&rig));
    assert!(!rig.app().tree().is_open());
    assert_eq!(rig.view().body, TreeBody::Closed);
    assert!(!rig.shows("TOTAL"));
}

#[test]
fn opening_without_a_base_says_the_base_is_not_available() {
    let mut rig = TreeRig::new();
    rig.open_panel();
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    assert!(rig.shows("Base not available"));
    assert!(!rig.shows("TOTAL"));
}

#[test]
fn opening_a_base_after_the_panel_fills_it_and_closing_the_base_empties_it() {
    let dir = tempdir().unwrap();
    let mut rig = TreeRig::new();
    rig.open_panel();
    rig.open_pgn(dir.path(), "std.pgn", STD);
    assert_eq!(rig.view().body, TreeBody::Rows);
    assert!(rig.shows("TOTAL"));
    let slot = rig.app().bases.current();
    rig.app_msg(BasesMessage::Close(slot));
    assert_eq!(rig.view().body, TreeBody::Unavailable);
    assert!(rig.shows("Base not available"));
}
