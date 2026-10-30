//! Headless tests of the whole window.

use iced::{Settings, Size, Theme, widget};
use iced_test::Simulator;
use mychess_app::{
    General, LanguagePref, LoadedPreferences, Message as AppMessage, PrefChange, Preferences,
    PrefsMessage, ThemePref,
};
use mychess_gui::widgets::board::geometry::Orientation;
use mychess_gui::{Message, Shell, UiMessage, ViewChange};

/// A shell whose preferences were "loaded" with a fixed language and theme:
/// `System` would depend on the machine.
fn shell() -> Shell {
    let mut shell = Shell::new(None);
    let prefs = Preferences {
        general: General {
            language: LanguagePref::EnUs,
            theme: ThemePref::Light,
            ..General::default()
        },
        ..Preferences::default()
    };
    drop(shell.update(Message::App(AppMessage::PreferencesLoaded(
        LoadedPreferences {
            prefs,
            issues: Vec::new(),
        },
    ))));
    shell
}

fn set(change: PrefChange) -> Message {
    Message::App(AppMessage::Prefs(PrefsMessage::Set(change)))
}

fn simulator(shell: &Shell, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(Settings::default(), size, shell.view())
}

#[test]
fn the_window_renders_in_three_sizes_and_both_themes() {
    for size in [
        Size::new(800.0, 560.0),
        Size::new(1280.0, 800.0),
        Size::new(2560.0, 1440.0),
    ] {
        for theme in [Theme::Light, Theme::Dark] {
            let shell = shell();
            let mut sim = simulator(&shell, size);
            assert!(sim.snapshot(&theme).is_ok(), "{size:?} {theme:?}");
        }
    }
}

#[test]
fn the_two_default_panes_and_the_status_bar_exist() {
    let shell = shell();
    let mut sim = simulator(&shell, Size::new(1280.0, 800.0));
    for id in ["board-pane", "notation-pane", "status-bar"] {
        assert!(sim.find(widget::Id::new(id)).is_ok(), "{id}");
    }
}

#[test]
fn pane_titles_and_texts_are_translated() {
    let mut shell = shell();
    {
        let mut sim = simulator(&shell, Size::new(1280.0, 800.0));
        assert!(sim.find("Board").is_ok());
        assert!(sim.find("Moves").is_ok(), "tab of the notation panel");
        assert!(sim.find("Ready").is_ok());
    }
    drop(shell.update(set(PrefChange::Language(LanguagePref::PtBr))));
    let mut sim = simulator(&shell, Size::new(1280.0, 800.0));
    assert!(sim.find("Tabuleiro").is_ok());
    assert!(sim.find("Lances").is_ok(), "tab of the notation panel");
    assert!(sim.find("Pronto").is_ok());
}

#[test]
fn closing_the_notation_pane_and_the_status_bar_removes_them() {
    let mut shell = shell();
    drop(shell.update(Message::Ui(UiMessage::TogglePane(
        mychess_gui::windows::layout::PaneKind::Notation,
    ))));
    drop(shell.update(Message::Ui(UiMessage::View(ViewChange::StatusBar(false)))));
    let mut sim = simulator(&shell, Size::new(1280.0, 800.0));
    assert!(sim.find(widget::Id::new("notation-pane")).is_err());
    assert!(sim.find(widget::Id::new("status-bar")).is_err());
    assert!(sim.find(widget::Id::new("board-pane")).is_ok());
}

#[test]
fn flipping_through_the_game_menu_item_message_turns_the_board() {
    let mut shell = shell();
    let specs = mychess_gui::windows::menu_bar::menu_specs(&shell.app, &shell.ui, shell.options);
    let flip = specs
        .iter()
        .flat_map(|spec| spec.entries.iter())
        .find_map(|entry| match entry {
            mychess_gui::windows::menu_bar::MenuEntry::Item {
                label: mychess_gui::windows::menu_bar::Label::Key("menu-game-flip"),
                action,
                ..
            } => action.clone(),
            _ => None,
        })
        .expect("flip item with an action");
    drop(shell.update(flip));
    assert_eq!(shell.ui.orientation, Orientation::Black);
}

#[test]
fn the_menu_bar_titles_are_visible() {
    let shell = shell();
    let mut sim = simulator(&shell, Size::new(1280.0, 800.0));
    for title in ["File", "Edit", "Game", "View", "Help"] {
        assert!(sim.find(title).is_ok(), "{title}");
    }
}

#[test]
fn about_box_opens_and_closes() {
    let mut shell = shell();
    drop(shell.update(Message::Ui(UiMessage::ShowAbout(true))));
    {
        let mut sim = simulator(&shell, Size::new(1280.0, 800.0));
        assert!(sim.find(widget::Id::new("about-box")).is_ok());
    }
    drop(shell.update(Message::Ui(UiMessage::ShowAbout(false))));
    let mut sim = simulator(&shell, Size::new(1280.0, 800.0));
    assert!(sim.find(widget::Id::new("about-box")).is_err());
}
