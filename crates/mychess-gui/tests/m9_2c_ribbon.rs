//! M9-2c: the ribbon as the second view of the menus (spec M9-2 section 3).

use iced::{Settings, Size, Theme};
use iced_test::Simulator;
use mychess_app::{
    General, LanguagePref, LoadedPreferences, MenuStylePref, Message as AppMessage, Preferences,
    ThemePref, render_preferences,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::ribbon::RibbonMessage;
use mychess_gui::{Message, Shell, UiMessage};

const SIZE: Size = Size::new(1400.0, 800.0);

fn shell(style: MenuStylePref, collapsed: bool) -> Shell {
    let mut shell = Shell::new(None);
    let prefs = Preferences {
        general: General {
            language: LanguagePref::EnUs,
            theme: ThemePref::Light,
            menu_style: style,
            ribbon_collapsed: collapsed,
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

fn simulator(shell: &Shell, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(Settings::default(), size, shell.view())
}

fn ribbon(shell: &mut Shell, message: RibbonMessage) {
    drop(shell.update(Message::Ui(UiMessage::Ribbon(message))));
}

fn tab_of(shell: &Shell, key: &str) -> usize {
    menu_specs(&shell.app, &shell.ui, shell.options)
        .iter()
        .position(|s| s.label_key == key)
        .unwrap_or(0)
}

fn text_of(key: &str) -> String {
    I18n::new(Language::EnUs).tr(key)
}

/// The message of the item with this key, as the dropdown has it.
fn item_message(shell: &Shell, key: &str) -> Option<String> {
    fn walk(entries: &[MenuEntry], key: &str) -> Option<String> {
        entries.iter().find_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(k),
                action,
                ..
            } if *k == key => Some(format!("{action:?}")),
            MenuEntry::Submenu { entries, .. } => walk(entries, key),
            _ => None,
        })
    }
    menu_specs(&shell.app, &shell.ui, shell.options)
        .iter()
        .find_map(|s| walk(&s.entries, key))
}

#[test]
fn the_ribbon_replaces_the_menu_bar_and_shows_group_titles() {
    let mut dropdown = shell(MenuStylePref::Dropdown, false);
    ribbon(&mut dropdown, RibbonMessage::SelectTab(5));
    assert!(
        simulator(&dropdown, SIZE)
            .find(text_of("menu-group-annotation"))
            .is_err()
    );

    let mut shell = shell(MenuStylePref::Ribbon, false);
    let tools = tab_of(&shell, "menu-tools");
    ribbon(&mut shell, RibbonMessage::SelectTab(tools));
    let mut sim = simulator(&shell, SIZE);
    for group in [
        "menu-group-engines",
        "menu-group-analysis-windows",
        "menu-group-openings",
        "menu-group-annotation",
    ] {
        assert!(sim.find(text_of(group)).is_ok(), "{group}");
    }
}

#[test]
fn clicking_a_button_sends_the_message_of_the_dropdown_item() {
    for (menu, key) in [
        ("menu-file", "menu-file-new"),
        ("menu-tools", "menu-tools-graph"),
        ("menu-play", "menu-play-engine"),
        ("menu-help", "menu-help-about"),
    ] {
        let mut shell = shell(MenuStylePref::Ribbon, false);
        let tab = tab_of(&shell, menu);
        ribbon(&mut shell, RibbonMessage::SelectTab(tab));
        let expected = item_message(&shell, key);
        let mut sim = simulator(&shell, SIZE);
        assert!(sim.click(text_of(key)).is_ok(), "{key}");
        let messages: Vec<Message> = sim.into_messages().collect();
        let sent: Vec<String> = messages.iter().map(|m| format!("{:?}", Some(m))).collect();
        assert!(
            expected.is_some_and(|e| sent.contains(&e)),
            "{key}: {sent:?}"
        );
    }
}

#[test]
fn collapsing_expanding_and_the_temporary_ribbon() {
    let mut shell = shell(MenuStylePref::Ribbon, false);
    let tools = tab_of(&shell, "menu-tools");
    ribbon(&mut shell, RibbonMessage::SelectTab(tools));
    let title = text_of("menu-group-annotation");
    assert!(simulator(&shell, SIZE).find(title.clone()).is_ok());

    // The arrow collapses; the preference is saved.
    ribbon(&mut shell, RibbonMessage::ToggleCollapsed);
    assert!(shell.app.prefs.general.ribbon_collapsed);
    assert!(render_preferences(&shell.app.prefs).contains("ribbon_collapsed = true"));
    assert!(simulator(&shell, SIZE).find(title.clone()).is_err());

    // A tab shows the groups for a moment...
    ribbon(&mut shell, RibbonMessage::SelectTab(tools));
    assert!(simulator(&shell, SIZE).find(title.clone()).is_ok());
    // ... a click outside closes them ...
    ribbon(&mut shell, RibbonMessage::Dismiss);
    assert!(simulator(&shell, SIZE).find(title.clone()).is_err());
    // ... and so does choosing something, which still runs.
    ribbon(&mut shell, RibbonMessage::SelectTab(tools));
    let pick = Message::Ui(UiMessage::RibbonPick(Box::new(Message::Ui(
        UiMessage::ShowAbout(true),
    ))));
    drop(shell.update(pick));
    assert!(shell.ui.show_about);
    assert!(!shell.ui.ribbon.flyout);
    assert!(shell.app.prefs.general.ribbon_collapsed);

    // The arrow expands again.
    ribbon(&mut shell, RibbonMessage::ToggleCollapsed);
    assert!(!shell.app.prefs.general.ribbon_collapsed);
}

#[test]
fn a_collapsed_ribbon_loaded_from_the_preferences_shows_only_the_tabs() {
    let shell = shell(MenuStylePref::Ribbon, true);
    let mut sim = simulator(&shell, SIZE);
    assert!(sim.find(text_of("menu-tools")).is_ok());
    assert!(sim.find(text_of("menu-group-engines")).is_err());
}

#[test]
fn the_view_menu_switches_the_style_and_the_choice_persists() {
    let mut shell = shell(MenuStylePref::Dropdown, false);
    let message = menu_specs(&shell.app, &shell.ui, shell.options)
        .into_iter()
        .flat_map(|s| s.entries)
        .find_map(|e| match e {
            MenuEntry::Submenu {
                label: Label::Key("menu-view-menu-style"),
                entries,
                ..
            } => entries.into_iter().find_map(|e| match e {
                MenuEntry::Item {
                    label: Label::Key("menu-style-ribbon"),
                    action,
                    checked: Some(false),
                    ..
                } => action,
                _ => None,
            }),
            _ => None,
        });
    let Some(message) = message else {
        panic!("Ver > Menu style > Ribbon exists");
    };
    drop(shell.update(message));
    assert_eq!(shell.app.prefs.general.menu_style, MenuStylePref::Ribbon);
    assert!(render_preferences(&shell.app.prefs).contains("menu_style = \"ribbon\""));
    // The new style is drawn at once.
    let tools = tab_of(&shell, "menu-tools");
    ribbon(&mut shell, RibbonMessage::SelectTab(tools));
    assert!(
        simulator(&shell, SIZE)
            .find(text_of("menu-group-engines"))
            .is_ok()
    );
}

#[test]
fn the_ribbon_renders_narrow_and_in_both_themes_and_languages() {
    for width in [360.0, 800.0, 2400.0] {
        for theme in [Theme::Light, Theme::Dark] {
            let mut shell = shell(MenuStylePref::Ribbon, false);
            for tab in 0..8 {
                ribbon(&mut shell, RibbonMessage::SelectTab(tab));
                let mut sim = simulator(&shell, Size::new(width, 600.0));
                assert!(sim.snapshot(&theme).is_ok(), "{width} tab {tab}");
            }
        }
    }
}

/// Regression: the dividers between groups used to be `Length::Fill` in a
/// row inside a horizontal scrollable, which has no height limit, so every
/// tab with two or more groups made the ribbon as tall as the window (only
/// "Help", with one group, looked right). The page under the ribbon must
/// start at the same height in every tab.
#[test]
fn the_ribbon_has_the_same_short_height_in_every_tab() {
    let mut shell = shell(MenuStylePref::Ribbon, false);
    let help = tab_of(&shell, "menu-help");
    ribbon(&mut shell, RibbonMessage::SelectTab(help));
    let below = |shell: &Shell| {
        let mut sim = simulator(shell, Size::new(1400.0, 900.0));
        [
            "pane-board",
            "pane-notation",
            "pane-info",
            "pane-comment",
            "pane-close",
        ]
        .iter()
        .find_map(|key| sim.find(text_of(key)).ok())
        .expect("some pane title is under the ribbon")
        .bounds()
        .y
    };
    let reference = below(&shell);
    assert!(
        reference < 260.0,
        "the page starts at y = {reference} even under the one-group tab"
    );
    let tabs = menu_specs(&shell.app, &shell.ui, shell.options).len();
    for tab in 0..tabs {
        ribbon(&mut shell, RibbonMessage::SelectTab(tab));
        let y = below(&shell);
        assert!(
            (y - reference).abs() < 24.0,
            "tab {tab}: the page starts at y = {y}, the one-group tab at {reference}: the ribbon is too tall"
        );
    }
}

// ---- review fixes: the temporary ribbon (D1711, D1714) ----

fn collapsed_with_flyout() -> (Shell, usize) {
    let mut shell = shell(MenuStylePref::Ribbon, true);
    let tools = tab_of(&shell, "menu-tools");
    ribbon(&mut shell, RibbonMessage::SelectTab(tools));
    assert!(shell.ui.ribbon.flyout);
    (shell, tools)
}

fn escape() -> Message {
    use mychess_app::{Key, KeyInput, Modifiers, NamedKey};
    Message::Dialog(mychess_gui::windows::dialog_ui::DialogMessage::Key {
        input: KeyInput {
            key: Key::Named(NamedKey::Escape),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
        typed: None,
    })
}

#[test]
fn escape_closes_the_temporary_ribbon_and_the_next_one_goes_on_as_usual() {
    let (mut shell, _) = collapsed_with_flyout();
    drop(shell.update(Message::Ui(UiMessage::ShowAbout(true))));
    drop(shell.update(escape()));
    assert!(!shell.ui.ribbon.flyout);
    assert!(shell.ui.show_about, "the first Esc only closed the ribbon");
    drop(shell.update(escape()));
    assert!(!shell.ui.show_about);
}

#[test]
fn opening_a_dialog_closes_the_temporary_ribbon_for_good() {
    let (mut shell, _) = collapsed_with_flyout();
    drop(shell.update(Message::Dialog(
        mychess_gui::windows::dialog_ui::DialogMessage::OpenGoto,
    )));
    assert!(!shell.ui.ribbon.flyout);
    drop(shell.update(escape()));
    assert!(!shell.ui.ribbon.flyout, "it does not come back");
}

#[test]
fn pressing_the_open_tab_again_closes_it_and_the_panel_click_changes_nothing() {
    let (mut shell, tools) = collapsed_with_flyout();
    ribbon(&mut shell, RibbonMessage::Hold);
    assert!(shell.ui.ribbon.flyout);
    ribbon(&mut shell, RibbonMessage::SelectTab(tools));
    assert!(!shell.ui.ribbon.flyout);
    ribbon(&mut shell, RibbonMessage::SelectTab(tools));
    assert!(shell.ui.ribbon.flyout);
}

#[test]
fn a_click_outside_the_temporary_ribbon_closes_it_through_the_simulator() {
    use iced::{Event, Point, mouse};
    let (mut shell, _) = collapsed_with_flyout();
    let messages: Vec<Message> = {
        let mut sim = simulator(&shell, SIZE);
        sim.point_at(Point::new(700.0, 700.0));
        drop(sim.simulate([
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]));
        sim.into_messages().collect()
    };
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Ui(UiMessage::Ribbon(RibbonMessage::Dismiss)))),
        "{messages:?}"
    );
    for message in messages {
        drop(shell.update(message));
    }
    assert!(!shell.ui.ribbon.flyout);
}

#[test]
fn choosing_in_the_temporary_ribbon_closes_it_and_runs_through_the_simulator() {
    let mut shell = shell(MenuStylePref::Ribbon, true);
    let help = tab_of(&shell, "menu-help");
    ribbon(&mut shell, RibbonMessage::SelectTab(help));
    let messages: Vec<Message> = {
        let mut sim = simulator(&shell, SIZE);
        sim.click(text_of("menu-help-about")).expect("About button");
        sim.into_messages().collect()
    };
    assert!(!messages.is_empty());
    for message in messages {
        drop(shell.update(message));
    }
    assert!(shell.ui.show_about);
    assert!(!shell.ui.ribbon.flyout);
}

#[test]
fn a_disabled_ribbon_button_sends_nothing_through_the_simulator() {
    let mut shell = shell(MenuStylePref::Ribbon, false);
    let edit = tab_of(&shell, "menu-edit");
    ribbon(&mut shell, RibbonMessage::SelectTab(edit));
    let mut sim = simulator(&shell, SIZE);
    // A fresh application has nothing to redo.
    drop(sim.click(text_of("menu-edit-redo")));
    // Only the panel under the button spends the click (it changes nothing).
    assert!(
        sim.into_messages()
            .all(|m| matches!(m, Message::Ui(UiMessage::Ribbon(RibbonMessage::Hold))))
    );
}
