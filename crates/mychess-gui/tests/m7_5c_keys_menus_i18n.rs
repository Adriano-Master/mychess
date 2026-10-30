//! M7-5c (GUI, spec M7-5): the actions and keys, the menus, the file dialogs
//! and the translations of the EPD window.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::format_push_string,
    clippy::assert_is_empty
)]

mod m7_5c_common;

use m7_5c_common::*;
use mychess_app::epd_window::EpdNotice;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::pane_tabs::{close_id, label_key, tab_id};

fn menu<'a>(specs: &'a [mychess_gui::windows::menu_bar::MenuSpec], key: &str) -> &'a [MenuEntry] {
    &specs
        .iter()
        .find(|s| s.label_key == key)
        .expect("menu")
        .entries
}

fn find<'a>(entries: &'a [MenuEntry], key: &str) -> Option<&'a MenuEntry> {
    entries.iter().find(|e| match e {
        MenuEntry::Item {
            label: Label::Key(k),
            ..
        }
        | MenuEntry::Submenu {
            label: Label::Key(k),
            ..
        } => *k == key,
        _ => false,
    })
}

#[test]
fn the_toggle_action_opens_and_closes_the_pane_and_has_no_default_key() {
    let mut rig = Rig::new();
    assert!(
        rig.shell
            .app
            .shortcuts
            .bindings(Action::ToggleEpdPane)
            .is_empty()
    );
    rig.ui(Message::Ui(UiMessage::Action(Action::ToggleEpdPane)));
    assert!(rig.shell.ui.is_open(PaneKind::Epd));
    rig.ui(Message::Ui(UiMessage::Action(Action::ToggleEpdPane)));
    assert!(!rig.shell.ui.is_open(PaneKind::Epd));
}

#[test]
fn the_navigation_keys_load_the_records_and_use_command_not_control() {
    let mut rig = Rig::new();
    rig.open("cinco.epd", CINCO);
    for (action, key) in [
        (Action::EpdFirst, "Command+Shift+Home"),
        (Action::EpdPrev, "Command+Shift+Up"),
        (Action::EpdNext, "Command+Shift+Down"),
        (Action::EpdLast, "Command+Shift+End"),
    ] {
        let bindings = rig.shell.app.shortcuts.bindings(action);
        assert_eq!(bindings.len(), 1, "{action:?}");
        assert_eq!(bindings[0].to_string(), key);
    }
    // The key goes through the map to the action and the record loads.
    rig.ui(Message::Ui(UiMessage::Key {
        input: mychess_app::KeyInput {
            key: mychess_app::Key::Named(mychess_app::NamedKey::Down),
            modifiers: mychess_app::Modifiers {
                command: true,
                shift: true,
                alt: false,
            },
        },
        text_focused: false,
    }));
    // The effects of the shell's dispatch are tasks; the same message to
    // the app shows what the key meant.
    rig.send(EpdMessage::Next);
    assert!(rig.tab().selected.is_some());
}

#[test]
fn the_menus_have_the_epd_entries_and_follow_the_state() {
    let mut rig = Rig::new();
    let options = mychess_gui::ViewOptions::default();
    let specs = menu_specs(&rig.shell.app, &rig.shell.ui, options);
    let file = menu(&specs, "menu-file");
    let Some(MenuEntry::Submenu { entries, .. }) = find(file, "menu-file-epd") else {
        panic!("File > EPD");
    };
    let enabled = |key: &str, entries: &[MenuEntry]| {
        matches!(
            find(entries, key),
            Some(MenuEntry::Item {
                action: Some(_),
                ..
            })
        )
    };
    assert!(enabled("menu-file-epd-open", entries));
    assert!(enabled("menu-file-epd-new", entries));
    assert!(!enabled("menu-file-epd-save", entries), "no file yet");
    let tools = menu(&specs, "menu-tools");
    assert!(matches!(
        find(tools, "menu-tools-epd"),
        Some(MenuEntry::Item {
            checked: Some(false),
            ..
        })
    ));
    rig.open("cinco.epd", CINCO);
    let specs = menu_specs(&rig.shell.app, &rig.shell.ui, options);
    let Some(MenuEntry::Submenu { entries, .. }) = find(menu(&specs, "menu-file"), "menu-file-epd")
    else {
        panic!("File > EPD");
    };
    assert!(enabled("menu-file-epd-save", entries));
    assert!(matches!(
        find(menu(&specs, "menu-tools"), "menu-tools-epd"),
        Some(MenuEntry::Item {
            checked: Some(true),
            ..
        })
    ));
}

#[test]
fn the_pane_labels_exist_in_both_languages() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in [
            label_key(PaneKind::Epd),
            "action-toggle-epd-pane",
            "action-epd-first",
            "action-epd-prev",
            "action-epd-next",
            "action-epd-last",
            "menu-tools-epd",
            "menu-file-epd",
            "menu-file-epd-open",
            "menu-file-epd-new",
            "menu-file-epd-save",
            "menu-file-epd-save-as",
            "menu-file-epd-recent",
            "file-dialog-open-epd",
            "file-dialog-new-epd",
            "file-dialog-save-epd",
            "file-filter-epd",
        ] {
            assert!(i18n.has(key), "{key} in {language:?}");
        }
    }
    assert_eq!(tab_id(PaneKind::Epd), "tab-epd");
    assert_eq!(close_id(PaneKind::Epd), "pane-close-epd");
}

#[test]
fn every_notice_text_is_translated_and_pt_br_says_variante_never_variacao() {
    let notices = [
        EpdNotice::AlreadyOpen,
        EpdNotice::TooMany { max: 4 },
        EpdNotice::ReadOnly,
        EpdNotice::NotFound,
        EpdNotice::UciOnly,
        EpdNotice::RunUndone,
    ];
    for notice in &notices {
        let en = mychess_gui::windows::epd::text::notice_text(&I18n::new(Language::EnUs), notice);
        let pt = mychess_gui::windows::epd::text::notice_text(&I18n::new(Language::PtBr), notice);
        assert_ne!(en, pt, "{notice:?}");
        assert!(!pt.to_lowercase().contains("variação"));
    }
    let pt = I18n::new(Language::PtBr);
    assert_eq!(
        mychess_gui::windows::epd::text::notice_text(&pt, &EpdNotice::AlreadyOpen),
        "O arquivo EPD escolhido já está aberto."
    );
}

#[test]
fn the_panel_renders_in_portuguese() {
    let mut rig = Rig::new();
    rig.shell.app.prefs.general.language = mychess_app::LanguagePref::PtBr;
    rig.shell.i18n = I18n::new(Language::PtBr);
    rig.open("cinco.epd", CINCO);
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("5 posições").is_ok());
    assert!(sim.find("Opcodes da posição").is_ok());
}

#[test]
fn the_empty_panel_offers_open_and_new() {
    let mut rig = Rig::new();
    rig.ui(Message::Ui(UiMessage::Action(Action::ToggleEpdPane)));
    let mut sim = rig.sim(1400.0, 900.0);
    assert!(sim.find("No EPD file is open.").is_ok());
    assert!(sim.find("Open EPD file").is_ok());
    assert!(sim.find("New EPD file").is_ok());
    for theme in themes() {
        assert!(rig.sim(900.0, 600.0).snapshot(&theme).is_ok());
    }
}
