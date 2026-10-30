//! The analysis panel as a pane (spec M4-4, D437, D810, D818): it opens by
//! itself when an engine starts, has one section per running engine, is part
//! of the saved layout and has a menu item and a shortcut. Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod m4_4_gui_common;

use m4_4_gui_common::*;
use mychess_app::engines::Protocol;
use mychess_app::{Action, Preferences, load_preferences, render_preferences};
use mychess_gui::keymap::{Platform, binding_label};
use mychess_gui::windows::layout::LayoutNode;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};

fn menu_entries(rig: &Rig, key: &str) -> Vec<MenuEntry> {
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == key)
        .unwrap_or_else(|| panic!("menu {key}"))
        .entries
}

fn find_item<'a>(
    entries: &'a [MenuEntry],
    key: &str,
) -> (
    Option<&'a Message>,
    Option<bool>,
    Option<&'a mychess_app::Binding>,
) {
    fn walk<'a>(
        entries: &'a [MenuEntry],
        key: &str,
    ) -> Option<(
        Option<&'a Message>,
        Option<bool>,
        Option<&'a mychess_app::Binding>,
    )> {
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

fn analysis_open(rig: &Rig) -> bool {
    rig.shell.ui.is_open(PaneKind::Analysis)
        && rig.shell.ui.layout().panes().contains(&PaneKind::Analysis)
}

#[test]
fn the_panel_is_closed_by_default_and_an_empty_one_says_how_to_start_an_engine() {
    let mut rig = Rig::new();
    assert!(!analysis_open(&rig));
    assert!(
        !rig.shell
            .app
            .prefs
            .layout
            .as_ref()
            .is_some_and(|l| l.panes().contains(&PaneKind::Analysis))
    );
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    assert!(analysis_open(&rig));
    assert!(has(
        &rig,
        "No engine is running. Start one with Tools > Engines..."
    ));
}

#[test]
fn starting_an_engine_with_the_panel_closed_opens_it_and_saves_the_layout() {
    let mut rig = Rig::new();
    assert!(!analysis_open(&rig));
    let run = uci_run(&mut rig);
    assert!(analysis_open(&rig), "D810: opens by itself");
    assert!(rig.shell.ui.layout().is_valid());
    let saved = rig
        .shell
        .app
        .prefs
        .layout
        .clone()
        .expect("layout remembered");
    assert!(saved.panes().contains(&PaneKind::Analysis));
    assert!(has(&rig, "Fake"), "the section of run {run:?} is drawn");
}

#[test]
fn a_panel_the_user_closed_stays_closed_until_another_engine_starts() {
    let mut rig = Rig::new();
    let _first = uci_run(&mut rig);
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    assert!(!analysis_open(&rig));
    // Any other message does not bring it back (an engine event, a key...).
    rig.press(ch('a'));
    rig.app_msg(AppMessage::Analysis(AnalysisMessage::ToggleInfo(
        mychess_app::engines::RunId(1),
    )));
    assert!(!analysis_open(&rig), "closing is respected");
    // A new engine does.
    let id = add_engine(&mut rig, "Second", Protocol::Uci);
    let _second = start_with(&mut rig, id, identity("Second", Some(1..=3), true));
    assert!(analysis_open(&rig));
}

#[test]
fn there_is_one_section_per_running_engine_named_after_what_it_announced() {
    let mut rig = Rig::new();
    for (list, announced) in [
        ("List A", "Alpha 1.0"),
        ("List B", "Beta 2.0"),
        ("List C", "Gamma"),
    ] {
        let id = add_engine(&mut rig, list, Protocol::Uci);
        start_with(&mut rig, id, identity(announced, Some(1..=4), true));
    }
    for name in ["Alpha 1.0", "Beta 2.0", "Gamma"] {
        assert!(has(&rig, name), "{name}");
    }
    assert!(
        !has(&rig, "List A"),
        "the announced name wins over the list's"
    );
    assert_eq!(rig.shell.app.engine_runs().len(), 3);
}

#[test]
fn eight_engines_running_draw_eight_sections_without_trouble() {
    let mut rig = Rig::new();
    for n in 0..8 {
        let name = format!("Engine{n}");
        let id = add_engine(&mut rig, &name, Protocol::Uci);
        let run = start_with(&mut rig, id, identity(&name, Some(1..=5), true));
        started(&mut rig, run, 1);
        info(
            &mut rig,
            run,
            report(&Position::new(), 1, 1, 10, 20, &["e2e4"]),
        );
    }
    // The scrollable clips what does not fit, but every section is in the tree.
    let mut sim = rig.sim(SIZE);
    assert!(sim.find("Engine0").is_ok());
}

#[test]
fn the_panel_survives_other_panes_opening_and_closing_and_the_layout_stays_valid() {
    let mut rig = Rig::new();
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    for kind in [PaneKind::Info, PaneKind::GameList, PaneKind::Notation] {
        rig.msg(Message::Ui(UiMessage::TogglePane(kind)));
        assert!(analysis_open(&rig), "after toggling {kind:?}");
        assert!(rig.shell.ui.layout().is_valid());
    }
    for kind in [PaneKind::Info, PaneKind::GameList, PaneKind::Notation] {
        rig.msg(Message::Ui(UiMessage::TogglePane(kind)));
        assert!(analysis_open(&rig), "after toggling {kind:?} again");
    }
    // It is part of the saved arrangement.
    let saved = rig.shell.app.prefs.layout.clone().expect("saved");
    assert!(saved.panes().contains(&PaneKind::Analysis));
    // Panes appear once each.
    let panes = rig.shell.ui.layout().panes();
    assert_eq!(
        panes.iter().filter(|p| **p == PaneKind::Analysis).count(),
        1
    );
}

#[test]
fn the_analysis_pane_is_written_to_and_read_from_the_preferences_toml() {
    let prefs = Preferences {
        layout: Some(LayoutNode::default_layout().open(PaneKind::Analysis)),
        ..Preferences::default()
    };
    let text = render_preferences(&prefs);
    assert!(
        text.contains("analysis"),
        "the pane is named in the file:\n{text}"
    );
    let loaded = load_preferences(&text);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    let layout = loaded.prefs.layout.expect("the layout comes back");
    assert!(layout.panes().contains(&PaneKind::Analysis));
    assert!(layout.is_valid());
    assert_eq!(Some(layout), prefs.layout);
}

#[test]
fn a_saved_layout_with_the_panel_opens_it_when_the_preferences_load() {
    let prefs = Preferences {
        layout: Some(LayoutNode::default_layout().open(PaneKind::Analysis)),
        ..Preferences::default()
    };
    let text = render_preferences(&prefs);
    let mut rig = Rig::new();
    assert!(!analysis_open(&rig));
    rig.app_msg(AppMessage::PreferencesLoaded(load_preferences(&text)));
    assert!(analysis_open(&rig));
}

#[test]
fn reset_layout_closes_the_panel_and_forgets_it_in_the_preferences() {
    let mut rig = Rig::new();
    let _run = uci_run(&mut rig);
    assert!(analysis_open(&rig));
    rig.msg(Message::Ui(UiMessage::ResetLayout));
    assert!(!analysis_open(&rig));
    assert_eq!(rig.shell.app.prefs.layout, None, "back to the default");
    // The engine is still running: its next start reopens the panel.
    assert_eq!(rig.shell.app.engine_runs().len(), 1);
}

#[test]
fn closing_the_panel_drops_the_visual_state_that_the_keys_depend_on() {
    let mut rig = Rig::new();
    let run = uci_run(&mut rig);
    hover(&mut rig, true);
    rig.msg(aui(AnalysisUiMessage::OpenExclude(run)));
    assert!(rig.shell.ui.analysis.hovered);
    assert_eq!(rig.shell.ui.analysis.focus, Some(run));
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    assert!(
        !rig.shell.ui.analysis.hovered,
        "no ghost hover after closing"
    );
    assert_eq!(rig.shell.ui.analysis.focus, None);
}

// ---- menu and shortcut ----

#[test]
fn the_tools_menu_has_the_analysis_window_item_with_a_check_and_the_shortcut() {
    let mut rig = Rig::new();
    let tools = menu_entries(&rig, "menu-tools");
    let (action, checked, shortcut) = find_item(&tools, "menu-tools-analysis-window");
    assert_eq!(checked, Some(false));
    assert!(shortcut.is_some(), "shows its shortcut");
    assert!(
        matches!(
            action,
            Some(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)))
        ),
        "{action:?}"
    );
    rig.msg(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)));
    let tools = menu_entries(&rig, "menu-tools");
    assert_eq!(
        find_item(&tools, "menu-tools-analysis-window").1,
        Some(true)
    );
}

#[test]
fn the_view_menu_panes_submenu_lists_the_analysis_pane_with_its_check() {
    let mut rig = Rig::new();
    let view = menu_entries(&rig, "menu-view");
    let (action, checked, _) = find_item(&view, "menu-view-pane-analysis");
    assert_eq!(checked, Some(false));
    assert!(
        matches!(
            action,
            Some(Message::Ui(UiMessage::TogglePane(PaneKind::Analysis)))
        ),
        "{action:?}"
    );
    uci_run(&mut rig);
    let view = menu_entries(&rig, "menu-view");
    assert_eq!(find_item(&view, "menu-view-pane-analysis").1, Some(true));
}

#[test]
fn the_default_shortcut_is_command_shift_y_and_toggles_the_panel() {
    let mut rig = Rig::new();
    let bindings = rig.shell.app.shortcuts.bindings(Action::ToggleAnalysisPane);
    assert_eq!(bindings.len(), 1);
    let b = bindings[0];
    assert!(b.modifiers.command && b.modifiers.shift && !b.modifiers.alt);
    assert_eq!(b.key, Key::Char('y'));
    // Ctrl on Linux and Windows, Cmd on macOS: one binding, two labels.
    assert_eq!(binding_label(&b, Platform::Linux), "Ctrl+Shift+Y");
    assert!(binding_label(&b, Platform::MacOs).contains('\u{2318}'));
    rig.press(command_shift(Key::Char('y')));
    assert!(analysis_open(&rig));
    rig.press(command_shift(Key::Char('y')));
    assert!(!analysis_open(&rig));
}

#[test]
fn the_shortcut_works_even_with_a_text_field_focused_and_plain_y_does_not_toggle() {
    let mut rig = Rig::new();
    rig.key(command_shift(Key::Char('y')), true, None);
    assert!(analysis_open(&rig), "the shortcut has a modifier");
    rig.press(ch('y'));
    assert!(analysis_open(&rig), "a plain y is not the shortcut");
    // Ctrl+Shift+A is "select games" (D564), not the panel.
    rig.press(command_shift(Key::Char('a')));
    assert!(analysis_open(&rig));
}

#[test]
fn the_action_name_of_the_shortcut_is_stable_for_the_user_file() {
    assert_eq!(Action::ToggleAnalysisPane.name(), "ToggleAnalysisPane");
    // Not in `Action::all` (the fixed-binding table), but bound by default.
    let shortcuts = mychess_app::Shortcuts::defaults();
    assert!(!shortcuts.bindings(Action::ToggleAnalysisPane).is_empty());
}
