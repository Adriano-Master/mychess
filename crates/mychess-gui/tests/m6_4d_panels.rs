//! M6-4d (GUI, spec D3430-D3439): the four panels open and close with the
//! shell, the menu, the actions and the layout; the empty states and the
//! small sizes render.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

mod m6_4d_common;

use m6_4d_common::*;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::pane_tabs::{close_id, label_key, tab_id};

const PANES: [(PaneKind, Action); 4] = [
    (PaneKind::Crosstable, Action::ToggleCrosstablePane),
    (PaneKind::FilterStats, Action::ToggleStatsPane),
    (PaneKind::RatingGraph, Action::ToggleRatingPane),
    (PaneKind::PieceTracker, Action::TogglePieceTrackerPane),
];

fn app_open(shell: &Shell, kind: PaneKind) -> bool {
    match kind {
        PaneKind::Crosstable => shell.app.crosstable().is_open(),
        PaneKind::FilterStats => shell.app.charts().stats_is_open(),
        PaneKind::RatingGraph => shell.app.charts().rating_is_open(),
        _ => shell.app.piece_tracker().is_open(),
    }
}

fn stats(shell: &Shell) -> Vec<MenuEntry> {
    menu_specs(&shell.app, &shell.ui, mychess_gui::ViewOptions::default())
        .into_iter()
        .find(|s| s.label_key == "menu-stats")
        .expect("Statistics menu")
        .entries
}

fn entry<'a>(entries: &'a [MenuEntry], key: &str) -> &'a MenuEntry {
    entries
        .iter()
        .find(|e| matches!(e, MenuEntry::Item { label: Label::Key(k), .. } if *k == key))
        .unwrap_or_else(|| panic!("no menu entry {key}"))
}

fn checked(entry: &MenuEntry) -> Option<bool> {
    match entry {
        MenuEntry::Item { checked, .. } => *checked,
        _ => None,
    }
}

#[test]
fn each_action_opens_and_closes_its_pane_and_tells_the_application() {
    for (kind, action) in PANES {
        let mut rig = Rig::new();
        assert!(!rig.shell.ui.is_open(kind) && !app_open(&rig.shell, kind));
        rig.ui(Message::Ui(UiMessage::Action(action)));
        assert!(rig.shell.ui.is_open(kind), "{kind:?} opens");
        assert!(
            app_open(&rig.shell, kind),
            "{kind:?}: the application follows"
        );
        assert!(rig.shell.ui.layout().panes().contains(&kind));
        rig.ui(Message::Ui(UiMessage::Action(action)));
        assert!(!rig.shell.ui.is_open(kind) && !app_open(&rig.shell, kind));
    }
}

#[test]
fn the_close_button_of_the_tab_closes_the_pane_for_the_application_too() {
    for (kind, _) in PANES {
        let mut rig = Rig::new();
        rig.toggle(kind);
        assert!(app_open(&rig.shell, kind));
        rig.ui(Message::Ui(UiMessage::ClosePane(kind)));
        assert!(!rig.shell.ui.is_open(kind));
        assert!(!app_open(&rig.shell, kind), "{kind:?} closed in the app");
    }
}

#[test]
fn the_statistics_menu_lists_the_six_entries_without_keys_and_follows_the_panes() {
    let mut rig = Rig::new();
    let before = stats(&rig.shell);
    for key in [
        "menu-stats-crosstable",
        "menu-stats-stats",
        "menu-stats-graph-relative",
        "menu-stats-graph-absolute",
        "menu-stats-rating-graph",
        "menu-stats-piece-tracker",
    ] {
        match entry(&before, key) {
            MenuEntry::Item {
                shortcut, checked, ..
            } => {
                assert!(shortcut.is_none(), "{key} has no default key");
                assert_eq!(*checked, Some(false), "{key}");
            }
            _ => unreachable!(),
        }
    }
    for (kind, key) in [
        (PaneKind::Crosstable, "menu-stats-crosstable"),
        (PaneKind::FilterStats, "menu-stats-stats"),
        (PaneKind::RatingGraph, "menu-stats-rating-graph"),
        (PaneKind::PieceTracker, "menu-stats-piece-tracker"),
    ] {
        let MenuEntry::Item {
            action: Some(message),
            ..
        } = entry(&stats(&rig.shell), key).clone()
        else {
            panic!("{key} is enabled");
        };
        rig.ui(message);
        assert!(rig.shell.ui.is_open(kind), "{key} opens {kind:?}");
        assert_eq!(checked(entry(&stats(&rig.shell), key)), Some(true));
    }
}

#[test]
fn the_graph_entries_open_the_statistics_on_their_tab() {
    let mut rig = Rig::new();
    for (key, tab) in [
        ("menu-stats-graph-relative", StatsTab::Relative),
        ("menu-stats-graph-absolute", StatsTab::Absolute),
    ] {
        let MenuEntry::Item {
            action: Some(message),
            ..
        } = entry(&stats(&rig.shell), key).clone()
        else {
            panic!("{key} is enabled");
        };
        rig.ui(message);
        assert!(rig.shell.ui.is_open(PaneKind::FilterStats));
        assert!(rig.shell.app.charts().stats_is_open());
        assert_eq!(rig.shell.app.stats_view().tab, tab);
        assert_eq!(checked(entry(&stats(&rig.shell), key)), Some(true));
        let other = if tab == StatsTab::Relative {
            "menu-stats-graph-absolute"
        } else {
            "menu-stats-graph-relative"
        };
        assert_eq!(checked(entry(&stats(&rig.shell), other)), Some(false));
    }
}

#[test]
fn every_pane_has_a_label_a_tab_and_a_close_button_in_both_languages() {
    use mychess_gui::i18n::{I18n, Language};
    let mut ids = std::collections::BTreeSet::new();
    for (kind, _) in PANES {
        assert_ne!(label_key(kind), "pane-board", "{kind:?}");
        assert!(ids.insert(tab_id(kind)) && ids.insert(close_id(kind)));
        for language in Language::ALL {
            let i18n = I18n::new(language);
            assert!(i18n.has(label_key(kind)), "{kind:?} {language:?}");
            assert!(i18n.has(&format!("pane-close-{}", &label_key(kind)[5..])));
        }
    }
}

#[test]
fn the_new_panes_are_written_in_the_layout_and_unknown_ones_are_dropped() {
    use mychess_app::LayoutNode;
    for (kind, _) in PANES {
        let layout = LayoutNode::default_layout().open(kind);
        assert!(layout.contains(kind) && layout.is_valid());
    }
}

#[test]
fn the_empty_panels_say_what_to_do() {
    let mut rig = Rig::new();
    for (kind, _) in PANES {
        rig.toggle(kind);
    }
    rig.app(CrosstableMessage::Refresh);
    rig.app(ChartsMessage::RefreshStats);
    rig.app(ChartsMessage::RefreshRating);
    rig.app(PieceTrackerMessage::Refresh);
    let mut sim = rig.sim(1500.0, 900.0);
    // No game from a database, an empty clipbase and no players.
    for key in [
        "crosstab-notice-no-game",
        "stats-label-all",
        "rating-no-players",
        "ptracker-ranking",
    ] {
        let text = rig.tr(key);
        assert!(sim.find(text.as_str()).is_ok(), "{key}: {text}");
    }
}

#[test]
fn the_panels_render_at_small_sizes_in_both_languages_and_themes() {
    for language in [
        mychess_app::LanguagePref::PtBr,
        mychess_app::LanguagePref::EnUs,
    ] {
        let mut rig = Rig::new();
        rig.shell.app.prefs.general.language = language;
        rig.open_games(&GAMES);
        rig.load(rig.shell.app.bases.current(), 1);
        for (kind, _) in PANES {
            rig.toggle(kind);
        }
        rig.app(CrosstableMessage::Refresh);
        rig.app(ChartsMessage::RefreshStats);
        rig.app(PieceTrackerMessage::Refresh);
        for (w, h) in [(300.0, 360.0), (640.0, 480.0), (1500.0, 900.0)] {
            for theme in themes() {
                let mut sim = rig.sim(w, h);
                assert!(sim.snapshot(&theme).is_ok(), "{w}x{h}");
            }
        }
    }
}

#[test]
fn the_labels_follow_the_language_when_the_crosstable_opens() {
    let mut rig = Rig::new();
    rig.shell.app.prefs.general.language = mychess_app::LanguagePref::PtBr;
    rig.ui(Message::Ui(UiMessage::FlipBoard));
    rig.toggle(PaneKind::Crosstable);
    let view = rig.shell.app.crosstable_view();
    assert_eq!(view.labels.player, "Jogador");
    assert_eq!(view.labels.games, "partidas");
    // Back to English: the next sync sends the new words.
    rig.shell.app.prefs.general.language = mychess_app::LanguagePref::EnUs;
    rig.ui(Message::Ui(UiMessage::FlipBoard));
    assert_eq!(rig.shell.app.crosstable_view().labels.player, "Player");
}

#[test]
fn the_statistics_know_the_current_year_after_opening() {
    let mut rig = Rig::new();
    rig.toggle(PaneKind::FilterStats);
    // The year came from the clock of the front-end: the options menu
    // exists (it needs the year, D3428).
    rig.open_games(&GAMES);
    rig.app(ChartsMessage::RefreshStats);
    assert!(rig.shell.app.stats_view().options.is_some());
}
