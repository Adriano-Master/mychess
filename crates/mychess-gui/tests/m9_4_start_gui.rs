//! M9-4 (GUI, headless): the first-run welcome and the tip of the day: texts
//! in both languages, Esc and Enter, the keyboard trap and the status
//! notices of the start-up.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::start::{LayoutMode, StartInfo, StartMessage, StartNotice, TIP_COUNT, tip_key};
use mychess_app::{
    AppNotice, Key, KeyInput, Message as AppMessage, Modifiers, NamedKey, load_preferences,
};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::shell::UiMessage;
use mychess_gui::{Message, Shell};

const SIZE: Size = Size::new(1000.0, 700.0);

fn info(root: Option<std::path::PathBuf>) -> StartInfo {
    StartInfo {
        root,
        mode: LayoutMode::Normal,
        warnings: Vec::new(),
        files: Vec::new(),
        skip_tip: false,
    }
}

/// A shell that went through the launch with `prefs_text` as the file.
fn launched(dir: &std::path::Path, prefs_text: &str, info: StartInfo) -> Shell {
    let mut shell = Shell::new(Some(dir.join("preferences.toml")));
    for message in [
        AppMessage::Startup,
        AppMessage::Start(StartMessage::Configure(Box::new(info))),
        AppMessage::PreferencesLoaded(load_preferences(prefs_text)),
    ] {
        drop(mychess_app::update(&mut shell.app, message));
    }
    // Any message of the shell catches the language and the layers up with
    // the application (the loaded preferences went in behind its back).
    drop(shell.update(Message::Ui(UiMessage::CloseTabMenu)));
    shell
}

fn key(key: NamedKey) -> Message {
    Message::Ui(UiMessage::Key {
        input: KeyInput {
            key: Key::Named(key),
            modifiers: Modifiers::NONE,
        },
        text_focused: false,
    })
}

fn has_text(shell: &Shell, text: &str) -> bool {
    let mut sim = Simulator::with_size(Settings::default(), SIZE, shell.view());
    sim.find(text).is_ok()
}

#[test]
fn the_first_run_shows_the_welcome_over_the_window() {
    let dir = tempfile::tempdir().expect("tempdir");
    let shell = launched(
        dir.path(),
        "[general]\nlanguage = \"en-US\"\n",
        info(Some(dir.path().join("MyChess"))),
    );
    assert!(shell.app.start().welcome().is_some());
    assert!(shell.modal_layer_open(), "the welcome traps the keyboard");
    assert!(has_text(&shell, "Welcome to MyChess"));
    assert!(has_text(&shell, "Get started"));
}

#[test]
fn the_welcome_is_in_portuguese_too() {
    let dir = tempfile::tempdir().expect("tempdir");
    let shell = launched(
        dir.path(),
        "[general]\nlanguage = \"pt-BR\"\n",
        info(Some(dir.path().join("MyChess"))),
    );
    assert!(has_text(&shell, "Bem-vindo ao MyChess"));
    assert!(has_text(&shell, "Começar"));
}

#[test]
fn enter_and_escape_close_the_welcome_and_it_does_not_come_back() {
    for closing in [NamedKey::Enter, NamedKey::Escape] {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut shell = launched(dir.path(), "", info(Some(dir.path().join("MyChess"))));
        drop(shell.update(key(closing)));
        assert!(shell.app.start().welcome().is_none(), "{closing:?}");
        assert!(shell.app.prefs.start.welcome_done, "{closing:?}");
        assert!(!shell.modal_layer_open(), "{closing:?}");
    }
}

#[test]
fn the_keys_of_the_game_do_not_act_behind_the_welcome() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut shell = launched(dir.path(), "", info(None));
    let before = shell.app.session.cursor();
    drop(shell.update(key(NamedKey::Left)));
    drop(shell.update(key(NamedKey::Home)));
    assert_eq!(shell.app.session.cursor(), before);
    assert!(shell.app.start().welcome().is_some(), "arrows keep it open");
}

#[test]
fn a_missing_home_folder_is_explained() {
    let dir = tempfile::tempdir().expect("tempdir");
    let shell = launched(dir.path(), "[general]\nlanguage = \"en-US\"\n", info(None));
    assert!(has_text(
        &shell,
        "The program could not find your home folder, so the MyChess folder was not created. Everything else works."
    ));
}

#[test]
fn the_tip_of_the_day_opens_after_the_welcome_and_steps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut shell = launched(
        dir.path(),
        "[general]\nlanguage = \"en-US\"\n[start]\nwelcome_done = true\n",
        info(None),
    );
    assert_eq!(shell.app.start().tip(), Some(0));
    assert!(shell.modal_layer_open());
    assert!(has_text(&shell, "Tip of the day"));
    assert!(has_text(&shell, "Tip 1 of 10"));
    drop(shell.update(key(NamedKey::Right)));
    assert_eq!(shell.app.start().tip(), Some(1));
    drop(shell.update(key(NamedKey::Left)));
    drop(shell.update(key(NamedKey::Left)));
    assert_eq!(shell.app.start().tip(), Some(TIP_COUNT - 1));
    drop(shell.update(key(NamedKey::Escape)));
    assert!(shell.app.start().tip().is_none());
    assert!(!shell.modal_layer_open());
}

#[test]
fn the_tip_can_be_turned_off_from_its_own_box() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut shell = launched(dir.path(), "[start]\nwelcome_done = true\n", info(None));
    drop(
        shell.update(Message::App(AppMessage::Start(StartMessage::SetShowTip(
            false,
        )))),
    );
    assert!(!shell.app.prefs.start.show_tip);
}

#[test]
fn every_tip_exists_in_both_languages_and_has_text() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for index in 0..TIP_COUNT {
            let key = tip_key(index);
            let text = i18n.tr(key);
            assert_ne!(text, key, "{language:?} lacks {key}");
            assert!(text.len() > 20, "{language:?} {key} is too short");
        }
    }
}

#[test]
fn the_menu_has_the_tip_of_the_day() {
    let shell = Shell::new(None);
    let specs = mychess_gui::windows::menu_bar::menu_specs(
        &shell.app,
        &shell.ui,
        mychess_gui::ViewOptions::default(),
    );
    let help = specs.last().expect("a help menu");
    assert!(help.entries.iter().any(|entry| matches!(
        entry,
        mychess_gui::windows::menu_bar::MenuEntry::Item {
            label: mychess_gui::windows::menu_bar::Label::Key("menu-help-tip"),
            action: Some(_),
            ..
        }
    )));
}

#[test]
fn start_notices_are_translated_with_their_path() {
    let en = I18n::new(Language::EnUs);
    let pt = I18n::new(Language::PtBr);
    let notices = [
        StartNotice::PortableNotWritable(std::path::PathBuf::from("pasta com espaço")),
        StartNotice::PortableUnavailable,
        StartNotice::NoHomeFolder,
        StartNotice::FolderProblem(std::path::PathBuf::from("Usuário")),
    ];
    for notice in &notices {
        for i18n in [&en, &pt] {
            let text = mychess_gui::windows::status_bar::app_notice_text(
                i18n,
                &AppNotice::Start(notice.clone()),
            );
            assert_ne!(text, notice.message_key());
            if let Some(path) = notice.path() {
                assert!(text.contains(&path.display().to_string()), "{text}");
            }
        }
    }
}
