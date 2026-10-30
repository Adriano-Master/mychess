//! The Tools menu and the keys of the engine management (spec M4-3,
//! "Comportamento observavel" 1 and 3; D724, D743; the multiplatform rule:
//! `Modifiers::command()`). Headless.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m4_3_gui_common;

use m4_3_gui_common::*;
use mychess_app::engines::{
    Capabilities, EngineEntry, EngineIdentity, EngineMessage, EngineNotice, EngineRunState, Hotkey,
};
use mychess_app::{Action, AppNotice, Binding};
use mychess_gui::keymap::{Platform, binding_label};
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::{Message, Shell};

/// The entry with this key (or the numbered one: `key#n` is not used here).
fn tool_item(rig: &Rig, key: &str) -> MenuEntry {
    tools(rig)
        .into_iter()
        .find(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                ..
            } => *k == key,
            _ => false,
        })
        .unwrap_or_else(|| panic!("tools item {key}"))
}

fn tools(rig: &Rig) -> Vec<MenuEntry> {
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-tools")
        .expect("a Tools menu")
        .entries
}

fn f(n: u8) -> KeyInput {
    KeyInput {
        key: Key::Named(NamedKey::F(n)),
        modifiers: Modifiers::NONE,
    }
}

fn space() -> KeyInput {
    nk(NamedKey::Space)
}

fn manage() -> KeyInput {
    KeyInput {
        key: Key::Char('e'),
        modifiers: Modifiers {
            command: true,
            shift: true,
            alt: false,
        },
    }
}

fn notice(rig: &Rig) -> Option<AppNotice> {
    rig.shell
        .ui
        .status_notice
        .clone()
        .or_else(|| rig.shell.app.notice.clone())
}

fn dialog_open(rig: &Rig) -> bool {
    rig.shell.app.engine_dialog.is_some()
}

fn run_count(rig: &Rig) -> usize {
    rig.shell.app.engine_runs.len()
}

fn id_of(identity: Option<&EngineEntry>) -> EngineIdentity {
    let _ = identity;
    EngineIdentity {
        name: "Fake".to_owned(),
        author: None,
        options: vec![],
        capabilities: Capabilities::default(),
    }
}

/// Starts `id` and makes the run `Running`.
fn run_running(rig: &mut Rig, index: usize) -> mychess_app::engines::RunId {
    let id = rig.shell.app.engines.engines()[index].id;
    let run = start(rig, id);
    let identity = id_of(None);
    send(
        rig,
        EngineMessage::Started {
            run,
            identity: Box::new(identity),
            at: 1,
        },
    );
    assert_eq!(
        rig.shell.app.engine_runs.get(run).unwrap().state,
        EngineRunState::Running
    );
    run
}

/// A rig with three engines on F2, F3, F4 and the dialog closed.
fn keyed_trio() -> Rig {
    let (mut rig, ids) = trio();
    for (id, key) in ids.iter().zip(Hotkey::ALL) {
        send(&mut rig, EngineMessage::SetHotkey(*id, Some(key)));
    }
    send(&mut rig, EngineMessage::CloseDialog);
    rig
}

// ---- the menu ----

#[test]
fn the_tools_menu_lists_engines_start_1_to_3_and_the_analysis_toggle() {
    let rig = Rig::new();
    let entries = tools(&rig);
    let shape: Vec<String> = entries
        .iter()
        .map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                ..
            } => (*k).to_owned(),
            MenuEntry::Item {
                label: Label::Numbered(k, n),
                ..
            } => format!("{k}#{n}"),
            MenuEntry::Separator => "-".to_owned(),
            MenuEntry::Group(k) => format!("group:{k}"),
            _ => "?".to_owned(),
        })
        .collect();
    assert_eq!(
        shape,
        [
            "group:menu-group-engines",
            "menu-tools-engines",
            "menu-tools-start-engine#1",
            "menu-tools-start-engine#2",
            "menu-tools-start-engine#3",
            "menu-tools-toggle-analysis",
            // Spec M8-10c: the electronic board.
            "menu-tools-eboard",
            // Spec M4-4, M4-7, M8-8: the analysis windows.
            "group:menu-group-analysis-windows",
            "menu-tools-analysis-window",
            "menu-tools-graph",
            "menu-tools-tablebase",
            // Spec M5-2, M5-6, M5-7b: the openings.
            "group:menu-group-openings",
            "menu-tools-tree",
            "menu-tools-book",
            "menu-tools-eco-browser",
            // Spec M4-6, D912, M9-7: annotation, game report and EPD.
            "group:menu-group-annotation",
            "menu-tools-annotate",
            "menu-tools-report",
            "menu-tools-epd",
        ]
    );
}

#[test]
fn every_tools_item_is_enabled_even_with_an_empty_list() {
    let rig = Rig::new();
    for entry in tools(&rig) {
        if let MenuEntry::Item { action, label, .. } = entry {
            assert!(action.is_some(), "{label:?} is off");
        }
    }
}

fn shortcuts(rig: &Rig) -> Vec<Option<Binding>> {
    tools(rig)
        .into_iter()
        .filter_map(|e| match e {
            MenuEntry::Item { shortcut, .. } => Some(shortcut),
            _ => None,
        })
        .collect()
}

#[test]
fn the_shortcuts_shown_come_from_the_central_map_and_follow_the_platform() {
    let rig = Rig::new();
    let s = shortcuts(&rig);
    let app = &rig.shell.app;
    assert_eq!(
        s[0],
        app.shortcuts
            .bindings(Action::ManageEngines)
            .first()
            .copied()
    );
    for n in 1..=3u8 {
        assert_eq!(
            s[usize::from(n)],
            app.shortcuts
                .bindings(Action::StartEngine(n))
                .first()
                .copied()
        );
    }
    assert_eq!(
        s[4],
        app.shortcuts
            .bindings(Action::ToggleEngineAnalysis)
            .first()
            .copied()
    );
    let label = |b: Option<Binding>, p| binding_label(&b.expect("bound"), p);
    for p in [Platform::Linux, Platform::Windows, Platform::MacOs] {
        assert_eq!(label(s[1], p), "F2");
        assert_eq!(label(s[2], p), "F3");
        assert_eq!(label(s[3], p), "F4");
    }
    assert_eq!(label(s[0], Platform::Linux), "Ctrl+Shift+E");
    assert_eq!(label(s[0], Platform::Windows), "Ctrl+Shift+E");
    let mac = label(s[0], Platform::MacOs);
    assert!(mac.contains('\u{2318}') && !mac.contains("Ctrl"), "{mac}");
}

#[test]
fn the_manage_shortcut_is_a_command_modifier_never_a_fixed_control() {
    let rig = Rig::new();
    let b = rig.shell.app.shortcuts.bindings(Action::ManageEngines)[0];
    assert!(b.modifiers.command && b.modifiers.shift);
}

#[test]
fn the_engines_item_opens_the_dialog() {
    let mut rig = Rig::new();
    let MenuEntry::Item {
        action: Some(message),
        ..
    } = tool_item(&rig, "menu-tools-engines")
    else {
        panic!("an item")
    };
    match message {
        Message::App(m) => {
            rig.send(m);
        }
        other => rig.msg(other),
    }
    assert!(dialog_open(&rig));
}

#[test]
fn the_start_items_act_like_the_function_keys() {
    let mut rig = keyed_trio();
    let MenuEntry::Item {
        action: Some(Message::App(m)),
        ..
    } = tools(&rig)
        .into_iter()
        .find(|e| {
            matches!(
                e,
                MenuEntry::Item {
                    label: Label::Numbered(_, 2),
                    ..
                }
            )
        })
        .expect("start engine 2")
    else {
        panic!("start engine 2")
    };
    rig.app_msg(m);
    let lc0 = rig.shell.app.engines.engines()[1].id;
    assert!(rig.shell.app.engine_runs.of_engine(lc0).is_some());
    assert_eq!(run_count(&rig), 1);
}

#[test]
fn the_toggle_item_without_a_running_engine_says_none_is_running() {
    let mut rig = Rig::new();
    let MenuEntry::Item {
        action: Some(Message::App(m)),
        ..
    } = tool_item(&rig, "menu-tools-toggle-analysis")
    else {
        panic!("toggle")
    };
    rig.app_msg(m);
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::NoEngineRunning))
    );
}

#[test]
fn the_menu_bar_names_the_menu_in_both_languages() {
    assert_eq!(en().tr("menu-tools"), "Tools");
    assert_eq!(pt().tr("menu-tools"), "Ferramentas");
    assert_eq!(pt().tr("menu-tools-engines"), "Engines...");
}

// ---- F2-F4 ----

#[test]
fn f2_f3_f4_start_the_engine_of_each_key_and_press_again_ends_it() {
    let mut rig = keyed_trio();
    for (n, index) in [(2u8, 0usize), (3, 1), (4, 2)] {
        rig.press(f(n));
        let id = rig.shell.app.engines.engines()[index].id;
        assert!(rig.shell.app.engine_runs.of_engine(id).is_some(), "F{n}");
    }
    assert_eq!(run_count(&rig), 3);
    rig.press(f(3));
    let lc0 = rig.shell.app.engines.engines()[1].id;
    assert_eq!(
        rig.shell.app.engine_runs.of_engine(lc0).unwrap().state,
        EngineRunState::Stopping
    );
}

#[test]
fn a_key_without_an_engine_only_says_so() {
    let mut rig = Rig::new();
    rig.press(f(3));
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::NoEngineOnKey(Hotkey::F3)))
    );
    assert_eq!(run_count(&rig), 0);
    assert!(!dialog_open(&rig));
}

#[test]
fn the_remark_for_an_empty_key_is_shown_in_the_status_bar() {
    let mut rig = Rig::new();
    rig.press(f(4));
    rig.sync();
    // The shell moves the application's remark to the bar on the next
    // application message.
    rig.app_msg(mychess_app::Message::Engine(EngineMessage::CancelDelete));
    assert!(c::has_text(&rig, "No engine has the key F4."));
}

#[test]
fn f2_works_with_a_text_field_focused_since_it_is_not_text() {
    let mut rig = keyed_trio();
    rig.key(f(2), true, None);
    assert_eq!(run_count(&rig), 1);
}

// ---- the space bar ----

#[test]
fn space_without_a_running_engine_says_none_is_running() {
    let mut rig = keyed_trio();
    rig.press(space());
    assert_eq!(
        notice(&rig),
        Some(AppNotice::Engine(EngineNotice::NoEngineRunning))
    );
}

#[test]
fn space_with_a_text_field_focused_does_nothing() {
    let mut rig = keyed_trio();
    rig.key(space(), true, Some(' '));
    assert_eq!(notice(&rig), None);
    run_running(&mut rig, 0);
    let run = rig.shell.app.engine_runs.as_slice()[0].id;
    let before = rig.shell.app.engine_runs.get(run).unwrap().analyzing;
    rig.key(space(), true, Some(' '));
    assert_eq!(
        rig.shell.app.engine_runs.get(run).unwrap().analyzing,
        before
    );
}

#[test]
fn space_toggles_the_analysis_of_the_first_running_engine_in_list_order() {
    let mut rig = keyed_trio();
    // Started in the opposite order of the list.
    let third = run_running(&mut rig, 2);
    let first = run_running(&mut rig, 0);
    let analyzing = |rig: &Rig, run| rig.shell.app.engine_runs.get(run).unwrap().analyzing;
    assert!(analyzing(&rig, first) && analyzing(&rig, third));
    rig.press(space());
    assert!(
        !analyzing(&rig, first),
        "the first of the list was switched off"
    );
    assert!(analyzing(&rig, third));
    rig.press(space());
    assert!(analyzing(&rig, first));
}

// ---- modals swallow the keys (D743) ----

fn modal_states() -> Vec<(&'static str, Rig)> {
    let mut out = Vec::new();
    let mut r = keyed_trio();
    send(&mut r, EngineMessage::OpenDialog);
    out.push(("list", r));
    let mut r = keyed_trio();
    send(&mut r, EngineMessage::OpenDialog);
    send(&mut r, EngineMessage::New);
    out.push(("editor", r));
    let mut r = keyed_trio();
    send(&mut r, EngineMessage::OpenDialog);
    let id = r.shell.app.engines.engines()[0].id;
    send(&mut r, EngineMessage::Delete(id));
    out.push(("delete", r));
    let mut r = keyed_trio();
    send(&mut r, EngineMessage::OpenDialog);
    let id = r.shell.app.engines.engines()[0].id;
    send(&mut r, EngineMessage::OpenOptions(id));
    out.push(("options", r));
    out
}

#[test]
fn function_keys_and_space_do_nothing_while_an_engine_dialog_is_open() {
    for (name, mut rig) in modal_states() {
        for (label, input) in [("F2", f(2)), ("F3", f(3)), ("F4", f(4)), ("Space", space())] {
            for focused in [false, true] {
                rig.key(input, focused, None);
            }
            assert_eq!(run_count(&rig), 0, "{name}: {label} started an engine");
            assert_eq!(notice(&rig), None, "{name}: {label} raised a remark");
        }
        assert!(
            dialog_open(&rig),
            "{name}: closed by a key it should ignore"
        );
    }
}

#[test]
fn the_manage_shortcut_does_not_stack_a_second_dialog_or_reset_the_open_one() {
    for (name, mut rig) in modal_states() {
        let before = ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs);
        rig.press(manage());
        assert_eq!(
            ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
            before,
            "{name}"
        );
    }
}

#[test]
fn the_manage_shortcut_opens_the_dialog_when_nothing_is_open() {
    let mut rig = keyed_trio();
    rig.press(manage());
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Engines)
    );
}

#[test]
fn the_precedence_is_options_then_delete_then_editor_then_list_and_escape_peels_them() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::Edit(ids[0]));
    send(&mut rig, EngineMessage::Delete(ids[1]));
    send(&mut rig, EngineMessage::OpenOptions(ids[0]));
    let top = |rig: &Rig| ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs);
    assert_eq!(top(&rig), Some(ActiveDialog::EngineOptions));
    rig.press(nk(NamedKey::Escape));
    assert_eq!(top(&rig), Some(ActiveDialog::EngineDelete));
    rig.press(nk(NamedKey::Escape));
    assert_eq!(top(&rig), Some(ActiveDialog::EngineEditor));
    rig.press(nk(NamedKey::Escape));
    assert_eq!(top(&rig), Some(ActiveDialog::Engines));
    rig.press(nk(NamedKey::Escape));
    assert_eq!(top(&rig), None);
    assert_eq!(names(&rig).len(), 3, "nothing was deleted by the Escapes");
}

#[test]
fn letters_typed_in_the_editor_or_options_do_not_move_the_list_selection() {
    let (mut rig, ids) = trio();
    send(&mut rig, EngineMessage::Edit(ids[0]));
    rig.press(ch('c'));
    assert_eq!(selected(&rig), Some(ids[0]));
}

#[test]
fn a_letter_when_no_dialog_is_open_does_not_touch_the_engine_list() {
    let mut rig = keyed_trio();
    let before = names(&rig);
    rig.press(ch('s'));
    assert_eq!(names(&rig), before);
    assert!(!dialog_open(&rig));
}

#[test]
fn closing_the_main_dialog_closes_the_editor_too() {
    let (mut rig, _) = trio();
    send(&mut rig, EngineMessage::New);
    send(&mut rig, EngineMessage::CloseDialog);
    assert!(rig.shell.app.engine_editor.is_none());
    assert!(!dialog_open(&rig));
}

#[test]
fn a_fresh_shell_has_no_engine_dialog() {
    let shell = Shell::new(None);
    assert!(shell.app.engine_dialog.is_none());
}

// ---- the Play menu (spec M9-2a) ----

fn play_menu(rig: &Rig) -> Vec<MenuEntry> {
    menu_specs(&rig.shell.app, &rig.shell.ui, rig.shell.options)
        .into_iter()
        .find(|s| s.label_key == "menu-play")
        .expect("a Play menu")
        .entries
}

#[test]
fn the_tools_menu_no_longer_has_the_play_and_tournament_items() {
    let rig = Rig::new();
    for entry in tools(&rig) {
        if let MenuEntry::Item {
            label: Label::Key(k),
            ..
        } = entry
        {
            assert!(
                !k.starts_with("menu-tools-tournament") && k != "menu-tools-play-engine",
                "{k}"
            );
        }
    }
}

#[test]
fn the_play_menu_has_the_engine_and_the_tournament_groups() {
    let rig = Rig::new();
    let shape: Vec<String> = play_menu(&rig)
        .iter()
        .map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                ..
            } => (*k).to_owned(),
            MenuEntry::Group(k) => format!("group:{k}"),
            _ => "?".to_owned(),
        })
        .collect();
    assert_eq!(
        shape,
        [
            // The Chess960 start comes before the first group.
            "menu-play-chess960",
            "group:menu-group-vs-engine",
            "menu-play-engine",
            "menu-play-tactical",
            "group:menu-group-tournament",
            "menu-play-tournament",
            "menu-play-tournament-resume",
            "menu-play-tournament-panel",
            "group:menu-group-training",
            "menu-play-tactics",
            "menu-play-opening-trainer",
            "menu-play-calvar",
        ]
    );
}
