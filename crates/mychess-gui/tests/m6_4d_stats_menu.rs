//! The Statistics menu (a top-level menu between Search and Play) and the
//! server groups of the Online menu.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use mychess_app::charts::StatsTab;
use mychess_app::online::AccountView;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, MenuSpec, menu_specs};
use mychess_gui::{Message, Shell, UiMessage, ViewOptions};

fn specs(shell: &Shell) -> Vec<MenuSpec> {
    menu_specs(&shell.app, &shell.ui, ViewOptions::default())
}

fn menu(shell: &Shell, key: &str) -> Vec<MenuEntry> {
    specs(shell)
        .into_iter()
        .find(|s| s.label_key == key)
        .unwrap_or_else(|| panic!("no menu {key}"))
        .entries
}

fn shape(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| match e {
            MenuEntry::Group(k) => format!("group:{k}"),
            MenuEntry::Separator => "sep".to_owned(),
            MenuEntry::Item {
                label: Label::Key(k) | Label::Numbered(k, _),
                ..
            } => format!("item:{k}"),
            MenuEntry::Item {
                label: Label::Text(_),
                ..
            } => "text".to_owned(),
            MenuEntry::Submenu {
                label: Label::Key(k),
                ..
            } => format!("sub:{k}"),
            other => panic!("unexpected entry {other:?}"),
        })
        .collect()
}

const STATS: [&str; 6] = [
    "menu-stats-crosstable",
    "menu-stats-stats",
    "menu-stats-graph-relative",
    "menu-stats-graph-absolute",
    "menu-stats-rating-graph",
    "menu-stats-piece-tracker",
];

/// D3494: the player entries follow the statistics, under their own group.
/// Spec M9-7: the spell-check and "Add Elo" moved to the Database menu and
/// the two reports came here.
const PLAYERS: [&str; 3] = [
    "menu-stats-find-players",
    "menu-stats-find-tournaments",
    "menu-stats-player-info",
];

#[test]
fn the_statistics_menu_sits_between_search_and_play_with_its_entries() {
    let shell = Shell::new(None);
    let keys: Vec<&str> = specs(&shell).iter().map(|s| s.label_key).collect();
    let at = keys.iter().position(|k| *k == "menu-search").unwrap();
    assert_eq!(keys[at + 1], "menu-stats");
    assert_eq!(keys[at + 2], "menu-play");
    let mut expected = vec!["group:menu-group-statistics".to_owned()];
    expected.extend(STATS.iter().map(|k| format!("item:{k}")));
    expected.push("group:menu-group-players".to_owned());
    expected.push("item:menu-stats-find-players".to_owned());
    expected.push("item:menu-stats-find-tournaments".to_owned());
    expected.push("item:menu-stats-player-info".to_owned());
    expected.push("group:menu-group-reports".to_owned());
    expected.push("item:menu-tools-opening-report".to_owned());
    expected.push("item:menu-tools-player-report".to_owned());
    assert_eq!(shape(&menu(&shell, "menu-stats")), expected);
    for entry in menu(&shell, "menu-stats").into_iter().take(7).skip(1) {
        let MenuEntry::Item {
            checked, action, ..
        } = entry
        else {
            panic!("only items");
        };
        assert_eq!(checked, Some(false));
        assert!(action.is_some());
    }
}

#[test]
fn the_tools_menu_no_longer_holds_them() {
    let shell = Shell::new(None);
    let tools = shape(&menu(&shell, "menu-tools"));
    for key in STATS {
        assert!(!tools.contains(&format!("item:{key}")), "{key}");
    }
    assert!(!tools.contains(&"group:menu-group-statistics".to_owned()));
    // What stays: the score graph; the players moved to Statistics (D3494).
    assert!(tools.contains(&"item:menu-tools-graph".to_owned()));
    for key in PLAYERS {
        assert!(!tools.iter().any(|e| e.ends_with(key)), "{key}");
    }
    assert!(!tools.contains(&"group:menu-group-players".to_owned()));
}

#[test]
fn the_labels_exist_in_both_languages() {
    for (language, menu_label, crosstable) in [
        (Language::EnUs, "Statistics", "Crosstable"),
        (Language::PtBr, "Estatísticas", "Tabela cruzada"),
    ] {
        let i18n = I18n::new(language);
        assert_eq!(i18n.tr("menu-stats"), menu_label);
        assert_eq!(i18n.tr("menu-stats-crosstable"), crosstable);
        for key in STATS.iter().chain(PLAYERS.iter()) {
            assert_ne!(i18n.tr(key), *key, "{key}");
        }
        for key in ["menu-group-statistics", "menu-group-players"] {
            assert_ne!(i18n.tr(key), key, "{key}");
        }
    }
    assert_eq!(
        I18n::new(Language::PtBr).tr("menu-group-statistics"),
        "Estatísticas e gráficos"
    );
    assert_eq!(
        I18n::new(Language::EnUs).tr("menu-group-players"),
        "Players"
    );
    assert_eq!(
        I18n::new(Language::PtBr).tr("menu-stats-find-players"),
        "Localizador de jogadores..."
    );
}

#[test]
fn the_player_entries_send_their_messages() {
    use mychess_app::{Action, Message as AppMessage, SpellingMessage};
    let shell = Shell::new(None);
    let entries = menu(&shell, "menu-stats");
    let find = |key: &str| {
        entries.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                action,
                ..
            } if *k == key => action.clone(),
            _ => None,
        })
    };
    for (key, action) in [
        ("menu-stats-find-players", Action::PlayerFinder),
        ("menu-stats-find-tournaments", Action::TournamentFinder),
        ("menu-stats-player-info", Action::PlayerInfo),
    ] {
        assert!(
            matches!(find(key), Some(Message::Ui(UiMessage::Action(a))) if a == action),
            "{key}"
        );
    }
    // Spec M9-7: the spell-check submenu and "Add Elo" are in Database.
    let db = menu(&shell, "menu-database");
    assert!(matches!(
        db.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key("menu-stats-add-elo"),
                action,
                ..
            } => action.clone(),
            _ => None,
        }),
        Some(Message::App(AppMessage::Spelling(
            SpellingMessage::OpenAddElo
        )))
    ));
    let Some(MenuEntry::Submenu { entries: sub, .. }) = db.iter().find(|e| {
        matches!(
            e,
            MenuEntry::Submenu {
                label: Label::Key("menu-stats-spell"),
                ..
            }
        )
    }) else {
        panic!("no spell submenu");
    };
    assert_eq!(sub.len(), 4);
}

#[test]
fn clicking_the_entries_sends_the_right_messages_and_marks_them() {
    let mut shell = Shell::new(None);
    let entries = menu(&shell, "menu-stats");
    let action_of = |key: &str| {
        entries
            .iter()
            .find_map(|e| match e {
                MenuEntry::Item {
                    label: Label::Key(k),
                    action,
                    ..
                } if *k == key => action.clone(),
                _ => None,
            })
            .unwrap()
    };
    for (key, kind) in [
        ("menu-stats-crosstable", mychess_app::PaneKind::Crosstable),
        ("menu-stats-stats", mychess_app::PaneKind::FilterStats),
        (
            "menu-stats-rating-graph",
            mychess_app::PaneKind::RatingGraph,
        ),
        (
            "menu-stats-piece-tracker",
            mychess_app::PaneKind::PieceTracker,
        ),
    ] {
        assert!(matches!(
            action_of(key),
            Message::Ui(UiMessage::TogglePane(k)) if k == kind
        ));
    }
    for (key, tab) in [
        ("menu-stats-graph-relative", StatsTab::Relative),
        ("menu-stats-graph-absolute", StatsTab::Absolute),
    ] {
        assert!(matches!(
            action_of(key),
            Message::Panels(mychess_gui::windows::data_panels::DataPanelsMessage::OpenStatsTab(t))
                if t == tab
        ));
    }
    drop(shell.update(action_of("menu-stats-crosstable")));
    let checked = menu(&shell, "menu-stats")
        .into_iter()
        .filter(|e| {
            matches!(
                e,
                MenuEntry::Item {
                    checked: Some(true),
                    ..
                }
            )
        })
        .count();
    assert_eq!(checked, 1);
}

// ---- the groups of the Online menu ----

#[test]
fn the_online_menu_has_a_group_per_server_without_an_account() {
    let shell = Shell::new(None);
    assert_eq!(
        shape(&menu(&shell, "menu-online")),
        [
            "group:menu-group-lichess",
            "item:menu-online-connect",
            "item:menu-online-seek",
            "item:menu-online-challenge",
            "item:menu-online-incoming",
            "item:menu-online-watch",
            "item:menu-online-stop-watch",
            "item:menu-online-disconnect",
            "group:menu-group-fics",
            "item:menu-online-fics",
            "sep",
            "item:menu-online-remove-lock",
        ]
    );
}

#[test]
fn the_account_line_comes_first_inside_the_lichess_group() {
    let mut shell = Shell::new(None);
    shell.app.online.account = Some(AccountView {
        login: "alice".into(),
        id: "alice".into(),
        title: None,
        ratings: vec![("blitz".into(), 1800, false)],
        can_challenge: true,
    });
    let shape = shape(&menu(&shell, "menu-online"));
    assert_eq!(shape[0], "group:menu-group-lichess");
    assert_eq!(shape[1], "text");
    assert_eq!(shape[2], "item:menu-online-connect");
    let fics = shape
        .iter()
        .position(|s| s == "group:menu-group-fics")
        .unwrap();
    assert_eq!(shape[fics + 1], "item:menu-online-fics");
    assert_eq!(shape[fics + 2], "sep");
}

#[test]
fn the_group_titles_exist_in_both_languages() {
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        assert_eq!(i18n.tr("menu-group-lichess"), "Lichess");
        assert_eq!(i18n.tr("menu-group-fics"), "FICS");
    }
}
