//! M9-7 (GUI): the organization of the menus and of the ribbon (spec
//! `docs/specs/m9-7-reorganizacao-dos-menus.md`, section 3, D4600-D4614).
//! A contract test: every command sits in the menu and group the spec names,
//! no command appears twice, every key has both languages, and the group
//! "Analysis windows" holds analysis only. Headless: the menus are pure data.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::BTreeSet;

use mychess_app::Action;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, MenuSpec, menu_specs};
use mychess_gui::windows::ribbon::groups;
use mychess_gui::{Shell, ViewOptions};

type Groups = Vec<(Option<&'static str>, Vec<String>)>;
type Layout = Vec<(&'static str, Groups)>;

fn specs(shell: &Shell) -> Vec<MenuSpec> {
    menu_specs(&shell.app, &shell.ui, ViewOptions::default())
}

fn entry_key(entry: &MenuEntry) -> Option<String> {
    match entry {
        MenuEntry::Item {
            label: Label::Key(k),
            ..
        }
        | MenuEntry::Submenu {
            label: Label::Key(k),
            ..
        } => Some((*k).to_owned()),
        MenuEntry::Item {
            label: Label::Numbered(k, n),
            ..
        } => Some(format!("{k}#{n}")),
        _ => None,
    }
}

/// Menu -> groups (a separator opens an untitled group) -> top-level keys.
fn layout(shell: &Shell) -> Layout {
    specs(shell)
        .into_iter()
        .map(|spec| {
            let mut groups: Vec<(Option<&'static str>, Vec<String>)> = vec![(None, Vec::new())];
            for entry in &spec.entries {
                match entry {
                    MenuEntry::Group(k) => groups.push((Some(*k), Vec::new())),
                    MenuEntry::Separator => groups.push((None, Vec::new())),
                    other => {
                        if let Some(key) = entry_key(other) {
                            groups.last_mut().unwrap().1.push(key);
                        }
                    }
                }
            }
            groups.retain(|(_, items)| !items.is_empty());
            (spec.label_key, groups)
        })
        .collect()
}

fn expect(
    menu: &'static str,
    groups: &[(Option<&'static str>, &[&str])],
) -> (&'static str, Groups) {
    (
        menu,
        groups
            .iter()
            .map(|(g, items)| (*g, items.iter().map(|s| (*s).to_owned()).collect()))
            .collect(),
    )
}

fn menu_of<'a>(layout: &'a Layout, key: &str) -> &'a [(Option<&'static str>, Vec<String>)] {
    &layout.iter().find(|(m, _)| *m == key).expect("menu").1
}

#[test]
fn file_edit_and_database_menus_hold_the_commands_the_spec_names() {
    let shell = Shell::new(None);
    let all = layout(&shell);
    let expected = [
        expect(
            "menu-file",
            &[
                (
                    Some("menu-group-file-open"),
                    &[
                        "menu-file-new",
                        "menu-file-open",
                        "menu-file-open-recent",
                        "menu-file-bookmarks",
                        "menu-file-epd",
                        // The Clipbase is current in a fresh application:
                        // "Close database" reads "Clear the Clipbase".
                        "menu-file-clear-clipbase",
                    ],
                ),
                (
                    Some("menu-group-file-save"),
                    &["menu-file-save", "menu-file-save-as", "menu-file-append"],
                ),
                (
                    Some("menu-group-file-exchange"),
                    &[
                        "menu-file-import-pgn-file",
                        "menu-file-export-games",
                        "menu-file-export-image",
                    ],
                ),
                (None, &["menu-file-quit"]),
            ],
        ),
        expect(
            "menu-edit",
            &[
                (
                    Some("menu-group-edit-history"),
                    &["menu-edit-undo", "menu-edit-redo"],
                ),
                (
                    Some("menu-group-edit-clipboard"),
                    &[
                        "menu-edit-copy-fen",
                        "menu-edit-copy-pgn",
                        "menu-edit-paste-fen",
                        "menu-edit-import-pgn",
                    ],
                ),
                (
                    Some("menu-group-edit-game"),
                    &[
                        "menu-edit-setup-board",
                        "menu-edit-merge-game",
                        "menu-edit-strip",
                    ],
                ),
            ],
        ),
        expect(
            "menu-database",
            &[
                (
                    Some("menu-group-db-open"),
                    &["menu-file-databases", "menu-file-read-only"],
                ),
                (
                    Some("menu-group-db-clipbase"),
                    &[
                        "menu-edit-clipbase-copy",
                        "menu-edit-clipbase-paste",
                        "menu-edit-clipbase-clear",
                    ],
                ),
                (
                    Some("menu-group-db-maintenance"),
                    &[
                        "menu-tools-maintenance",
                        "menu-db-compact",
                        "menu-db-sort",
                        "menu-db-twins",
                        "menu-db-cleaner",
                        "menu-db-check",
                        "menu-tools-repair-base",
                    ],
                ),
                (
                    Some("menu-group-db-properties"),
                    &[
                        "menu-db-type",
                        "menu-db-description",
                        "menu-db-flags",
                        "menu-db-tags",
                    ],
                ),
                (
                    Some("menu-group-db-names"),
                    &[
                        "menu-tools-name-editor",
                        "menu-stats-spell",
                        "menu-db-spelling-file",
                    ],
                ),
                (
                    Some("menu-group-db-fill"),
                    &["menu-tools-eco-classify", "menu-stats-add-elo"],
                ),
            ],
        ),
    ];
    for (menu, groups) in expected {
        assert_eq!(menu_of(&all, menu), groups.as_slice(), "{menu}");
    }
}

#[test]
fn statistics_and_tools_menus_hold_the_commands_the_spec_names() {
    let shell = Shell::new(None);
    let all = layout(&shell);
    let expected = [
        expect(
            "menu-stats",
            &[
                (
                    Some("menu-group-statistics"),
                    &[
                        "menu-stats-crosstable",
                        "menu-stats-stats",
                        "menu-stats-graph-relative",
                        "menu-stats-graph-absolute",
                        "menu-stats-rating-graph",
                        "menu-stats-piece-tracker",
                    ],
                ),
                (
                    Some("menu-group-players"),
                    &[
                        "menu-stats-find-players",
                        "menu-stats-find-tournaments",
                        "menu-stats-player-info",
                    ],
                ),
                (
                    Some("menu-group-reports"),
                    &["menu-tools-opening-report", "menu-tools-player-report"],
                ),
            ],
        ),
        expect(
            "menu-tools",
            &[
                (
                    Some("menu-group-engines"),
                    &[
                        "menu-tools-engines",
                        "menu-tools-start-engine#1",
                        "menu-tools-start-engine#2",
                        "menu-tools-start-engine#3",
                        "menu-tools-toggle-analysis",
                        "menu-tools-eboard",
                    ],
                ),
                (
                    Some("menu-group-analysis-windows"),
                    &[
                        "menu-tools-analysis-window",
                        "menu-tools-graph",
                        "menu-tools-tablebase",
                    ],
                ),
                (
                    Some("menu-group-openings"),
                    &[
                        "menu-tools-tree",
                        "menu-tools-book",
                        "menu-tools-eco-browser",
                    ],
                ),
                (
                    Some("menu-group-annotation"),
                    &["menu-tools-annotate", "menu-tools-report", "menu-tools-epd"],
                ),
            ],
        ),
    ];
    for (menu, groups) in expected {
        assert_eq!(menu_of(&all, menu), groups.as_slice(), "{menu}");
    }
}

#[test]
fn the_analysis_windows_group_of_tools_holds_analysis_only() {
    let shell = Shell::new(None);
    let all = layout(&shell);
    let tools = menu_of(&all, "menu-tools");
    let (_, items) = tools
        .iter()
        .find(|(g, _)| *g == Some("menu-group-analysis-windows"))
        .expect("the group exists");
    let analysis: BTreeSet<&str> = [
        "menu-tools-analysis-window",
        "menu-tools-graph",
        "menu-tools-tablebase",
    ]
    .into();
    for key in items {
        assert!(analysis.contains(key.as_str()), "{key} is not analysis");
    }
    // The items that started the complaint left Tools for good.
    let in_tools: BTreeSet<&str> = tools
        .iter()
        .flat_map(|(_, items)| items.iter().map(String::as_str))
        .collect();
    for key in [
        "menu-tools-maintenance",
        "menu-tools-name-editor",
        "menu-tools-repair-base",
        "menu-tools-eco-classify",
        "menu-stats-spell",
        "menu-stats-add-elo",
    ] {
        assert!(!in_tools.contains(key), "{key} must not be in Tools");
    }
}

#[test]
fn the_database_menu_comes_after_game_and_before_search() {
    let shell = Shell::new(None);
    let keys: Vec<&str> = specs(&shell).iter().map(|s| s.label_key).collect();
    let at = keys.iter().position(|k| *k == "menu-database").unwrap();
    assert_eq!(keys[at - 1], "menu-game");
    assert_eq!(keys[at + 1], "menu-search");
    assert_eq!(keys.len(), 11);
}

#[test]
fn no_command_appears_twice_in_the_menus() {
    let shell = Shell::new(None);
    let mut seen = BTreeSet::new();
    for (menu, groups) in layout(&shell) {
        for (_, items) in groups {
            for key in items {
                assert!(seen.insert(key.clone()), "{key} appears twice ({menu})");
            }
        }
    }
}

#[test]
fn titled_groups_have_at_least_two_commands() {
    let shell = Shell::new(None);
    for (menu, groups) in layout(&shell) {
        for (group, items) in groups {
            if let Some(group) = group {
                // The FICS group of Online has one window (spec M9-7 D4612).
                if group == "menu-group-fics" {
                    continue;
                }
                assert!(items.len() >= 2, "{menu}/{group} has {items:?}");
            }
        }
    }
}

fn collect_keys(entries: &[MenuEntry], out: &mut BTreeSet<String>) {
    for entry in entries {
        match entry {
            MenuEntry::Group(k)
            | MenuEntry::Item {
                label: Label::Key(k) | Label::Numbered(k, _),
                ..
            } => {
                out.insert((*k).to_owned());
            }
            MenuEntry::Submenu { label, entries, .. } => {
                if let Label::Key(k) = label {
                    out.insert((*k).to_owned());
                }
                collect_keys(entries, out);
            }
            _ => {}
        }
    }
}

#[test]
fn every_menu_title_group_and_command_has_text_in_both_languages() {
    let shell = Shell::new(None);
    let mut keys = BTreeSet::new();
    for spec in specs(&shell) {
        keys.insert(spec.label_key.to_owned());
        collect_keys(&spec.entries, &mut keys);
    }
    assert!(keys.contains("menu-database") && keys.contains("menu-db-compact"));
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in &keys {
            assert!(i18n.has(key), "{key} missing in {language:?}");
            assert_ne!(i18n.tr(key), *key, "{key} untranslated in {language:?}");
        }
    }
    let pt = I18n::new(Language::PtBr);
    let en = I18n::new(Language::EnUs);
    assert_eq!(pt.tr("menu-database"), "Banco");
    assert_eq!(en.tr("menu-database"), "Database");
}

#[test]
fn dialogs_use_three_dots_and_no_item_keeps_the_unicode_ellipsis() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for key in ["menu-view-theme", "menu-view-sounds", "menu-db-compact"] {
            let text = i18n.tr(key);
            assert!(text.ends_with("..."), "{key}: {text}");
            assert!(!text.contains('\u{2026}'), "{key}: {text}");
        }
    }
}

fn find_item<'a>(entries: &'a [MenuEntry], key: &str) -> Option<&'a MenuEntry> {
    entries.iter().find_map(|e| match e {
        MenuEntry::Item {
            label: Label::Key(k),
            ..
        } if *k == key => Some(e),
        MenuEntry::Submenu { entries, .. } => find_item(entries, key),
        _ => None,
    })
}

#[test]
fn the_shortcuts_stay_with_the_items_that_moved_menu() {
    let shell = Shell::new(None);
    let all = specs(&shell);
    let find = |key: &str| {
        all.iter()
            .find_map(|s| find_item(&s.entries, key))
            .unwrap_or_else(|| panic!("{key} in some menu"))
    };
    for (key, action) in [
        ("menu-tools-analysis-window", Action::ToggleAnalysisPane),
        ("menu-tools-graph", Action::ToggleGraphPane),
        ("menu-tools-tree", Action::ToggleTreePane),
        ("menu-tools-book", Action::ToggleBookPane),
        ("menu-tools-tablebase", Action::ToggleTablebasePane),
        ("menu-tools-eboard", Action::EboardDialog),
        ("menu-tools-engines", Action::ManageEngines),
        ("menu-file-next-base", Action::NextBase),
        ("menu-file-previous-base", Action::PreviousBase),
        ("menu-stats-find-players", Action::PlayerFinder),
        ("menu-stats-find-tournaments", Action::TournamentFinder),
        ("menu-file-quit", Action::Quit),
        ("menu-file-open", Action::OpenFile),
        ("menu-file-save", Action::Save),
    ] {
        let MenuEntry::Item { shortcut, .. } = find(key) else {
            panic!("{key} is an item");
        };
        let expected = shell.app.shortcuts.bindings(action).first().copied();
        assert!(expected.is_some(), "{key}: the action has a default key");
        assert_eq!(*shortcut, expected, "{key}");
    }
}

#[test]
fn the_database_commands_follow_the_state_of_the_current_database() {
    // Fresh application: the Clipbase is current.
    let shell = Shell::new(None);
    let all = specs(&shell);
    let db = &all
        .iter()
        .find(|s| s.label_key == "menu-database")
        .unwrap()
        .entries;
    let enabled = |key: &str| match find_item(db, key).unwrap() {
        MenuEntry::Item { action, .. } => action.is_some(),
        _ => unreachable!(),
    };
    // Read-only cannot change for the Clipbase; copying to it is pointless.
    assert!(!enabled("menu-file-read-only"));
    assert!(!enabled("menu-edit-clipbase-copy"));
    assert!(!enabled("menu-edit-clipbase-paste"));
    assert!(enabled("menu-edit-clipbase-clear"));
    // The maintenance forms open the window on their own.
    for key in [
        "menu-tools-maintenance",
        "menu-db-compact",
        "menu-db-sort",
        "menu-db-twins",
        "menu-db-cleaner",
        "menu-db-check",
        "menu-tools-repair-base",
        "menu-db-type",
        "menu-db-description",
        "menu-db-flags",
        "menu-db-tags",
        "menu-tools-name-editor",
        "menu-tools-eco-classify",
        "menu-stats-add-elo",
    ] {
        assert!(enabled(key), "{key}");
    }
}

#[test]
fn the_ribbon_shows_the_groups_of_the_database_tab_in_order() {
    let shell = Shell::new(None);
    let all = specs(&shell);
    let db = all.iter().find(|s| s.label_key == "menu-database").unwrap();
    let titles: Vec<_> = groups(&db.entries).iter().map(|g| g.title).collect();
    assert_eq!(
        titles,
        [
            Some("menu-group-db-open"),
            Some("menu-group-db-clipbase"),
            Some("menu-group-db-maintenance"),
            Some("menu-group-db-properties"),
            Some("menu-group-db-names"),
            Some("menu-group-db-fill"),
        ]
    );
    // Every command of the dropdown is a button of the ribbon.
    let buttons: usize = groups(&db.entries).iter().map(|g| g.buttons.len()).sum();
    let commands = layout(&shell)
        .into_iter()
        .find(|(m, _)| *m == "menu-database")
        .unwrap()
        .1
        .iter()
        .map(|(_, items)| items.len())
        .sum::<usize>();
    assert_eq!(buttons, commands);
}
