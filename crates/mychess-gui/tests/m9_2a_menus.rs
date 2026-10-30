//! M9-2a (GUI): the Play menu and the reorganized Tools menu (spec
//! `docs/specs/m9-2-menu-em-faixa.md`, sections 2.1 and 2.2, D1620-D1631).
//! Contract tests derived from the spec; headless (the specs of the menus
//! are pure data).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::time::Instant;

use mychess_app::engines::{EngineDraft, EngineMessage};
use mychess_app::tournament::{
    Adjudication, Participant, TimeControl, TournamentConfig, TournamentMessage,
};
use mychess_app::{
    Action, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey, PlayColorPref, PlayModePref,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::dialog_ui::{ActiveDialog, DialogMessage};
use mychess_gui::windows::layout::PaneKind;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, MenuSpec, menu_specs};
use mychess_gui::windows::play_dialog::PlayUiMessage;
use mychess_gui::{Message, Shell, UiMessage};

fn specs(shell: &Shell) -> Vec<MenuSpec> {
    menu_specs(&shell.app, &shell.ui, shell.options)
}

fn menu(shell: &Shell, key: &str) -> Vec<MenuEntry> {
    specs(shell)
        .into_iter()
        .find(|s| s.label_key == key)
        .unwrap_or_else(|| panic!("menu {key}"))
        .entries
}

/// `group:<key>`, `sep`, `item:<key>[#n]`, `sub:<key>` per entry.
fn shape(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| match e {
            MenuEntry::Group(k) => format!("group:{k}"),
            MenuEntry::Separator => "sep".to_owned(),
            MenuEntry::Item {
                label: Label::Key(k),
                ..
            } => format!("item:{k}"),
            MenuEntry::Item {
                label: Label::Numbered(k, n),
                ..
            } => format!("item:{k}#{n}"),
            MenuEntry::Submenu {
                label: Label::Key(k),
                ..
            } => format!("sub:{k}"),
            other => panic!("unexpected entry {other:?}"),
        })
        .collect()
}

fn find<'a>(entries: &'a [MenuEntry], key: &str) -> &'a MenuEntry {
    entries
        .iter()
        .find(|e| matches!(e, MenuEntry::Item { label: Label::Key(k), .. } if *k == key))
        .unwrap_or_else(|| panic!("item {key}"))
}

fn enabled(entry: &MenuEntry) -> bool {
    match entry {
        MenuEntry::Item { action, .. } => action.is_some(),
        _ => panic!("not an item"),
    }
}

fn checked(entry: &MenuEntry) -> Option<bool> {
    match entry {
        MenuEntry::Item { checked, .. } => *checked,
        _ => panic!("not an item"),
    }
}

fn shortcut_text(entry: &MenuEntry) -> Option<String> {
    match entry {
        MenuEntry::Item { shortcut, .. } => shortcut.map(|b| b.to_string()),
        _ => panic!("not an item"),
    }
}

fn shell_with_engines() -> Shell {
    let mut shell = Shell::new(None);
    for name in ["Alpha", "Beta"] {
        shell
            .app
            .engines
            .add(EngineDraft::new(name, "/fake/engine"))
            .expect("engine added");
    }
    shell
}

fn send(shell: &mut Shell, message: Message) {
    drop(shell.update(message));
}

fn play_live_shell() -> Shell {
    let mut shell = shell_with_engines();
    send(&mut shell, Message::PlayUi(PlayUiMessage::Open));
    send(
        &mut shell,
        Message::PlayUi(PlayUiMessage::Color(PlayColorPref::White)),
    );
    send(
        &mut shell,
        Message::PlayUi(PlayUiMessage::Mode(PlayModePref::TimeBonus)),
    );
    send(&mut shell, Message::PlayUi(PlayUiMessage::Submit));
    assert!(shell.app.play_live(), "the game is starting");
    shell
}

fn start_tournament(shell: &mut Shell) {
    let config = TournamentConfig {
        participants: shell
            .app
            .engines
            .engines()
            .iter()
            .map(|e| Participant::new(e.id.0, &e.name))
            .collect(),
        time: TimeControl::Depth(1),
        adjudication: Adjudication {
            max_plies: Some(6),
            ..Adjudication::default()
        },
        ..TournamentConfig::default()
    };
    drop(mychess_app::update(
        &mut shell.app,
        AppMessage::Tournament(TournamentMessage::Start {
            config: Box::new(config),
            at: Instant::now(),
        }),
    ));
    assert!(shell.app.tournament_live(), "loading counts as live");
}

fn cmd_shift(c: char) -> KeyInput {
    KeyInput {
        key: Key::Char(c),
        modifiers: Modifiers {
            command: true,
            shift: true,
            alt: false,
        },
    }
}

fn press(shell: &mut Shell, input: KeyInput) {
    send(
        shell,
        Message::Dialog(DialogMessage::Key {
            input,
            text_focused: false,
            typed: None,
        }),
    );
}

// ---- structure (section 2.1) ----

#[test]
fn the_eleven_menus_come_in_the_order_of_the_spec() {
    let shell = Shell::new(None);
    let keys: Vec<&str> = specs(&shell).iter().map(|s| s.label_key).collect();
    assert_eq!(
        keys,
        [
            "menu-file",
            "menu-edit",
            "menu-game",
            // Spec M9-7 D4600: the Database menu.
            "menu-database",
            "menu-search",
            "menu-stats",
            "menu-play",
            "menu-view",
            "menu-tools",
            "menu-online",
            "menu-help"
        ]
    );
}

#[test]
fn the_play_menu_has_the_two_groups_and_four_items_of_the_spec() {
    let shell = Shell::new(None);
    assert_eq!(
        shape(&menu(&shell, "menu-play")),
        [
            // The Chess960 start comes before the first group.
            "item:menu-play-chess960",
            "group:menu-group-vs-engine",
            "item:menu-play-engine",
            // Spec M8-7: the tactical game.
            "item:menu-play-tactical",
            "group:menu-group-tournament",
            "item:menu-play-tournament",
            "item:menu-play-tournament-resume",
            "item:menu-play-tournament-panel",
            // Spec M8-4c: the tactics training.
            "group:menu-group-training",
            "item:menu-play-tactics",
            "item:menu-play-opening-trainer",
            "item:menu-play-calvar",
        ]
    );
}

#[test]
fn the_tools_menu_keeps_engines_analysis_openings_and_annotation_after_m9_7() {
    let shell = Shell::new(None);
    assert_eq!(
        shape(&menu(&shell, "menu-tools")),
        [
            "group:menu-group-engines",
            "item:menu-tools-engines",
            "item:menu-tools-start-engine#1",
            "item:menu-tools-start-engine#2",
            "item:menu-tools-start-engine#3",
            "item:menu-tools-toggle-analysis",
            "item:menu-tools-eboard",
            "group:menu-group-analysis-windows",
            "item:menu-tools-analysis-window",
            "item:menu-tools-graph",
            "item:menu-tools-tablebase",
            "group:menu-group-openings",
            "item:menu-tools-tree",
            "item:menu-tools-book",
            "item:menu-tools-eco-browser",
            "group:menu-group-annotation",
            "item:menu-tools-annotate",
            "item:menu-tools-report",
            "item:menu-tools-epd",
        ]
    );
}

#[test]
fn no_item_exists_twice_across_menus_among_play_and_tools() {
    let shell = Shell::new(None);
    let mut seen = BTreeSet::new();
    for key in ["menu-play", "menu-tools"] {
        for line in shape(&menu(&shell, key)) {
            if line.starts_with("item:") {
                assert!(seen.insert(line.clone()), "{line} appears twice");
            }
        }
    }
}

#[test]
fn the_online_menu_is_unchanged_and_right_after_tools() {
    let shell = Shell::new(None);
    let specs = specs(&shell);
    let at = specs
        .iter()
        .position(|s| s.label_key == "menu-tools")
        .unwrap();
    assert_eq!(specs[at + 1].label_key, "menu-online");
    let shape = shape(&specs[at + 1].entries);
    for key in [
        "item:menu-online-connect",
        "item:menu-online-fics",
        "item:menu-online-seek",
        "item:menu-online-challenge",
        "item:menu-online-disconnect",
        "item:menu-online-remove-lock",
    ] {
        assert!(shape.iter().any(|s| s == key), "{key} in {shape:?}");
    }
}

// ---- Group (section 2.2, D1628) ----

#[test]
fn a_group_is_only_a_divider_without_action_or_clickable_label() {
    let shell = Shell::new(None);
    for spec in specs(&shell) {
        for entry in &spec.entries {
            // The variant carries nothing but the key of the title: matching
            // it exhaustively shows there is no action to run.
            if let MenuEntry::Group(key) = entry {
                assert!(key.starts_with("menu-group-"), "{key}");
            }
        }
    }
    let groups: Vec<&str> = specs(&shell)
        .iter()
        .flat_map(|s| s.entries.iter())
        .filter_map(|e| match e {
            MenuEntry::Group(k) => Some(*k),
            _ => None,
        })
        .collect();
    // Spec M9-7: File 3, Edit 3, Database 6, Statistics 3 (graphs, players,
    // reports), Play 3, Tools 4 and the two servers of Online.
    assert_eq!(groups.len(), 24);
}

#[test]
fn group_titles_and_new_keys_exist_in_both_languages_and_old_keys_are_gone() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in [
            "menu-play",
            "menu-play-engine",
            "menu-play-tournament",
            "menu-play-tournament-resume",
            "menu-play-tournament-panel",
            "menu-group-vs-engine",
            "menu-group-tournament",
            "menu-group-engines",
            "menu-group-analysis-windows",
            "menu-group-annotation",
        ] {
            assert!(i18n.has(key), "{key} in {language:?}");
            assert_ne!(i18n.tr(key), key);
        }
        for old in [
            "menu-tools-play-engine",
            "menu-tools-tournament",
            "menu-tools-tournament-resume",
            "menu-tools-tournament-panel",
        ] {
            assert!(!i18n.has(old), "{old} must be gone in {language:?}");
        }
    }
}

// ---- conditions (section 2.1: "some/desabilita enquanto app.play_live()") ----

#[test]
fn idle_play_items_are_enabled_and_the_panel_is_unchecked() {
    let shell = shell_with_engines();
    let play = menu(&shell, "menu-play");
    assert!(enabled(find(&play, "menu-play-engine")));
    assert!(enabled(find(&play, "menu-play-tournament")));
    assert!(enabled(find(&play, "menu-play-tournament-resume")));
    let panel = find(&play, "menu-play-tournament-panel");
    assert!(enabled(panel));
    assert_eq!(checked(panel), Some(false));
}

#[test]
fn while_a_game_against_the_engine_is_live_play_and_tournament_are_disabled() {
    let shell = play_live_shell();
    let play = menu(&shell, "menu-play");
    assert!(!enabled(find(&play, "menu-play-engine")));
    assert!(!enabled(find(&play, "menu-play-tournament")));
    assert!(!enabled(find(&play, "menu-play-tournament-resume")));
    // The panel of the tournament stays available.
    assert!(enabled(find(&play, "menu-play-tournament-panel")));
}

#[test]
fn a_disabled_play_item_keeps_its_shortcut_label() {
    let shell = play_live_shell();
    let play = menu(&shell, "menu-play");
    assert_eq!(
        shortcut_text(find(&play, "menu-play-engine")).as_deref(),
        Some("Command+Shift+P")
    );
}

#[test]
fn resume_is_disabled_with_a_live_tournament_and_the_panel_still_works() {
    let mut shell = shell_with_engines();
    start_tournament(&mut shell);
    let play = menu(&shell, "menu-play");
    assert!(!enabled(find(&play, "menu-play-tournament-resume")));
    assert!(enabled(find(&play, "menu-play-tournament-panel")));
    assert!(enabled(find(&play, "menu-play-engine")) || shell.app.play_live());
}

#[test]
fn the_panel_item_is_checked_exactly_while_the_pane_is_open() {
    let mut shell = shell_with_engines();
    start_tournament(&mut shell);
    let panel_checked = |shell: &Shell| {
        let play = menu(shell, "menu-play");
        let entry = find(&play, "menu-play-tournament-panel");
        assert!(enabled(entry));
        checked(entry)
    };
    let before = panel_checked(&shell);
    assert_eq!(before, Some(shell.ui.is_open(PaneKind::Tournament)));
    send(
        &mut shell,
        Message::Ui(UiMessage::TogglePane(PaneKind::Tournament)),
    );
    let during = panel_checked(&shell);
    assert_eq!(during, Some(shell.ui.is_open(PaneKind::Tournament)));
    assert_ne!(before, during, "toggling flips the mark");
    send(
        &mut shell,
        Message::Ui(UiMessage::TogglePane(PaneKind::Tournament)),
    );
    assert_eq!(panel_checked(&shell), before);
}

#[test]
fn the_panel_item_sends_the_toggle_of_the_tournament_pane() {
    let shell = Shell::new(None);
    let play = menu(&shell, "menu-play");
    let MenuEntry::Item {
        action: Some(message),
        ..
    } = find(&play, "menu-play-tournament-panel")
    else {
        panic!("enabled");
    };
    assert!(matches!(
        message,
        Message::Ui(UiMessage::TogglePane(PaneKind::Tournament))
    ));
}

#[test]
fn the_analysis_windows_of_tools_are_checkable_and_follow_the_panes() {
    let mut shell = Shell::new(None);
    for (key, pane) in [
        ("menu-tools-analysis-window", PaneKind::Analysis),
        ("menu-tools-graph", PaneKind::Graph),
        ("menu-tools-tree", PaneKind::Tree),
        ("menu-tools-report", PaneKind::Report),
    ] {
        let tools = menu(&shell, "menu-tools");
        let before = checked(find(&tools, key));
        assert_eq!(before, Some(shell.ui.is_open(pane)), "{key}");
        send(&mut shell, Message::Ui(UiMessage::TogglePane(pane)));
        let tools = menu(&shell, "menu-tools");
        assert_eq!(checked(find(&tools, key)), Some(shell.ui.is_open(pane)));
        assert_ne!(checked(find(&tools, key)), before, "{key} flips");
    }
}

// ---- shortcuts (D1626) ----

#[test]
fn the_shortcuts_stay_bound_to_the_moved_and_kept_items() {
    let shell = Shell::new(None);
    let play = menu(&shell, "menu-play");
    let tools = menu(&shell, "menu-tools");
    assert_eq!(
        shortcut_text(find(&play, "menu-play-engine")).as_deref(),
        Some("Command+Shift+P")
    );
    assert_eq!(
        shortcut_text(find(&play, "menu-play-tournament")).as_deref(),
        Some("Command+Shift+R")
    );
    assert_eq!(
        shortcut_text(find(&tools, "menu-tools-engines")).as_deref(),
        Some("Command+Shift+E")
    );
    let starts: Vec<Option<String>> = tools
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item {
                label: Label::Numbered("menu-tools-start-engine", _),
                ..
            } => Some(shortcut_text(e)),
            _ => None,
        })
        .collect();
    assert_eq!(
        starts,
        [
            Some("F2".to_owned()),
            Some("F3".to_owned()),
            Some("F4".to_owned())
        ]
    );
    // The actions themselves keep their default bindings.
    for (action, text) in [
        (Action::PlayEngine, "Command+Shift+P"),
        (Action::TournamentEngines, "Command+Shift+R"),
        (Action::ManageEngines, "Command+Shift+E"),
        (Action::StartEngine(1), "F2"),
        (Action::StartEngine(2), "F3"),
        (Action::StartEngine(3), "F4"),
    ] {
        let binding = shell.app.shortcuts.bindings(action).first().copied();
        assert_eq!(binding.map(|b| b.to_string()).as_deref(), Some(text));
    }
}

#[test]
fn command_shift_p_opens_the_play_dialog_and_command_shift_r_the_tournament_form() {
    let mut shell = shell_with_engines();
    press(&mut shell, cmd_shift('p'));
    assert!(shell.ui.dialogs.play.draft.is_some(), "play form open");
    press(
        &mut shell,
        KeyInput {
            key: Key::Named(NamedKey::Escape),
            modifiers: Modifiers::NONE,
        },
    );
    assert!(shell.ui.dialogs.play.draft.is_none());
    press(&mut shell, cmd_shift('r'));
    assert_eq!(
        ActiveDialog::of(&shell.app, &shell.ui.dialogs),
        Some(ActiveDialog::TournamentForm)
    );
}

#[test]
fn command_shift_e_opens_the_engines_dialog() {
    let mut shell = shell_with_engines();
    press(&mut shell, cmd_shift('e'));
    assert!(shell.app.engine_dialog.is_some());
}

#[test]
fn the_start_engine_items_send_the_hotkey_toggles() {
    let shell = shell_with_engines();
    let tools = menu(&shell, "menu-tools");
    let sent: Vec<bool> = tools
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item {
                label: Label::Numbered("menu-tools-start-engine", _),
                action,
                ..
            } => Some(matches!(
                action,
                Some(Message::App(AppMessage::Engine(
                    EngineMessage::ToggleHotkey(_)
                )))
            )),
            _ => None,
        })
        .collect();
    assert_eq!(sent, [true, true, true]);
}
