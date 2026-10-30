//! Sanity tests of the panels with a close button and dynamic tabs (spec
//! M2-6, phase B), headless with `iced_test`: the real title bars of the
//! main window in, messages out.

use iced::{Settings, Size, widget};
use iced_test::Simulator;
use mychess_app::{
    General, LanguagePref, LoadedPreferences, Message as AppMessage, Preferences, ThemePref,
};
use mychess_gui::windows::layout::{LayoutNode, PaneKind};
use mychess_gui::windows::nav_bar::MOVE_FIELD_ID;
use mychess_gui::windows::{main_window, pane_tabs};
use mychess_gui::{Message, Shell, UiMessage};

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

fn sim(shell: &Shell) -> Simulator<'_, Message> {
    Simulator::with_size(
        Settings::default(),
        Size::new(1280.0, 800.0),
        main_window::view(shell),
    )
}

fn has(shell: &Shell, id: &'static str) -> bool {
    sim(shell).find(widget::Id::new(id)).is_ok()
}

fn send(shell: &mut Shell, message: Message) {
    drop(shell.update(message));
}

fn click(shell: &mut Shell, id: &'static str) {
    let messages: Vec<Message> = {
        let mut sim = sim(shell);
        sim.click(widget::Id::new(id)).expect("the widget");
        sim.into_messages().collect()
    };
    assert!(!messages.is_empty(), "{id} sent nothing");
    for message in messages {
        send(shell, message);
    }
}

#[test]
fn the_default_layout_shows_three_tabs_with_a_close_button_each_and_none_on_the_board() {
    let shell = shell();
    for kind in [PaneKind::Notation, PaneKind::Info, PaneKind::Comment] {
        assert!(has(&shell, pane_tabs::tab_id(kind)), "{kind:?} tab");
        assert!(has(&shell, pane_tabs::close_id(kind)), "{kind:?} close");
    }
    assert!(!has(&shell, pane_tabs::close_id(PaneKind::Board)));
    assert!(
        has(&shell, "notation-pane"),
        "only the active body is built"
    );
    assert!(!has(&shell, "info-pane"));
    assert!(!has(&shell, "comment-pane"));
}

#[test]
fn the_close_button_of_a_tab_closes_only_that_tab() {
    let mut shell = shell();
    click(&mut shell, pane_tabs::close_id(PaneKind::Info));
    assert!(!shell.ui.is_open(PaneKind::Info));
    assert!(shell.ui.is_open(PaneKind::Notation) && shell.ui.is_open(PaneKind::Comment));
    assert!(!has(&shell, pane_tabs::tab_id(PaneKind::Info)));
    // The menu brings it back, as a tab, selected.
    send(
        &mut shell,
        Message::Ui(UiMessage::TogglePane(PaneKind::Info)),
    );
    assert!(has(&shell, "info-pane"));
    assert!(has(&shell, pane_tabs::tab_id(PaneKind::Info)));
}

#[test]
fn a_panel_alone_in_its_group_has_the_close_button_at_the_end_of_the_title_bar() {
    let mut shell = shell();
    send(
        &mut shell,
        Message::Ui(UiMessage::DetachTab(PaneKind::Info)),
    );
    assert!(
        !has(&shell, pane_tabs::tab_id(PaneKind::Info)),
        "no tab bar"
    );
    assert!(has(&shell, pane_tabs::close_id(PaneKind::Info)));
    click(&mut shell, pane_tabs::close_id(PaneKind::Info));
    assert!(!shell.ui.is_open(PaneKind::Info));
}

#[test]
fn clicking_a_tab_selects_it_and_the_choice_is_saved() {
    let mut shell = shell();
    click(&mut shell, pane_tabs::tab_id(PaneKind::Info));
    assert!(shell.ui.is_visible(PaneKind::Info));
    assert!(has(&shell, "info-pane") && !has(&shell, "notation-pane"));
    let saved = shell.app.prefs.layout.clone().expect("saved");
    let text = mychess_app::render_preferences(&Preferences {
        layout: Some(saved.clone()),
        ..Preferences::default()
    });
    let back = mychess_app::load_preferences(&text).prefs.layout;
    assert_eq!(back, Some(saved.clone()), "written and read back");
    // A new run restores the groups and the selected tab.
    let mut other = Shell::new(None);
    let prefs = Preferences {
        layout: Some(saved),
        ..Preferences::default()
    };
    send(
        &mut other,
        Message::App(AppMessage::PreferencesLoaded(LoadedPreferences {
            prefs,
            issues: Vec::new(),
        })),
    );
    assert!(other.ui.is_visible(PaneKind::Info));
    assert_eq!(other.ui.layout(), shell.ui.layout());
}

#[test]
fn the_command_bar_stays_under_any_tab_of_the_group_with_the_move_list() {
    let mut shell = shell();
    assert!(has(&shell, MOVE_FIELD_ID));
    for kind in [PaneKind::Info, PaneKind::Comment] {
        click(&mut shell, pane_tabs::tab_id(kind));
        assert!(has(&shell, MOVE_FIELD_ID), "bar under {kind:?}");
    }
    click_text(&mut shell, "Moves");
    assert!(has(&shell, MOVE_FIELD_ID));
    // Without the move list there is no bar.
    send(
        &mut shell,
        Message::Ui(UiMessage::ClosePane(PaneKind::Notation)),
    );
    assert!(!has(&shell, MOVE_FIELD_ID));
}

#[test]
fn a_game_list_merged_into_the_notation_group_keeps_the_bar_and_becomes_a_tab() {
    let mut shell = shell();
    send(
        &mut shell,
        Message::Ui(UiMessage::TogglePane(PaneKind::GameList)),
    );
    let layout = shell.ui.layout();
    let merged = layout.merge(PaneKind::GameList, PaneKind::Notation);
    shell.ui.set_layout(&merged);
    assert!(has(&shell, pane_tabs::tab_id(PaneKind::GameList)));
    assert!(has(&shell, MOVE_FIELD_ID));
    assert_eq!(
        shell
            .ui
            .layout()
            .group_of(PaneKind::GameList)
            .map(|(t, _)| t.len()),
        Some(4)
    );
}

#[test]
fn the_tab_menu_offers_detach_and_separates_the_tab() {
    let mut shell = shell();
    send(
        &mut shell,
        Message::Ui(UiMessage::OpenTabMenu {
            kind: PaneKind::Comment,
            position: iced::Point::new(900.0, 100.0),
        }),
    );
    assert!(sim(&shell).find("Detach to a new panel").is_ok());
    click_text(&mut shell, "Detach to a new panel");
    assert!(shell.ui.tab_menu.is_none());
    assert_eq!(
        shell
            .ui
            .layout()
            .group_of(PaneKind::Comment)
            .map(|(t, _)| t.len()),
        Some(1)
    );
    assert_eq!(
        shell.ui.layout().panes().len(),
        LayoutNode::default_layout().panes().len()
    );
}

fn click_text(shell: &mut Shell, label: &'static str) {
    let messages: Vec<Message> = {
        let mut sim = sim(shell);
        sim.click(label).expect("the label");
        sim.into_messages().collect()
    };
    for message in messages {
        send(shell, message);
    }
}
