//! M8-9c (GUI): the FICS window (menu, connection form, graph, console),
//! the keys of its Fluent files and the rules of the lock. Headless: no
//! network; the effects of the application are never run (spec
//! `docs/specs/m8-9-fics.md`, D2601, D2602, D2616, D2626).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

#[path = "m2_4_common/mod.rs"]
mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::Rig;
use iced::Size;
use mychess_app::NamedKey;
use mychess_app::fics::{FicsMessage, FicsNotice, Phase, Seek, SeekColor};
use mychess_gui::Message;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::fics_dialog::FicsUiMessage;
use mychess_gui::windows::online_ui::Secret;

const SIZE: Size = Size::new(1200.0, 900.0);

fn active(rig: &Rig) -> Option<ActiveDialog> {
    ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs)
}

fn has(rig: &Rig, label: &str) -> bool {
    rig.sim(SIZE).find(label).is_ok()
}

fn open(rig: &mut Rig) {
    rig.msg(Message::Fics(FicsUiMessage::Open));
}

fn seek(index: u32, name: &str, minutes: u32, kind: &str) -> Seek {
    Seek {
        index,
        name: name.to_owned(),
        titles: String::new(),
        rating: Some(1500),
        provisional: false,
        minutes,
        increment: 0,
        rated: false,
        kind: kind.to_owned(),
        color: SeekColor::Any,
        min_rating: 0,
        max_rating: 9999,
        automatic: true,
        formula: false,
    }
}

#[test]
fn the_menu_message_opens_the_window_and_escape_closes_it() {
    let mut rig = Rig::new();
    assert_eq!(active(&rig), None);
    open(&mut rig);
    assert_eq!(active(&rig), Some(ActiveDialog::Fics));
    assert!(rig.shell.modal_layer_open(), "the window owns the keyboard");
    assert!(has(&rig, "Free Internet Chess Server (FICS)"));
    rig.press(common::named(NamedKey::Escape));
    assert_eq!(active(&rig), None);
    assert!(!rig.shell.modal_layer_open());
}

#[test]
fn the_disconnected_window_offers_guest_login_and_warns_about_the_password() {
    let mut rig = Rig::new();
    open(&mut rig);
    assert!(has(&rig, "Connect as guest"), "D2601");
    assert!(
        has(
            &rig,
            "The FICS protocol is not encrypted: the password travels in clear text. Use a password that you use nowhere else."
        ),
        "D2602 warning"
    );
}

#[test]
fn connecting_as_a_guest_asks_the_application_to_connect() {
    let mut rig = Rig::new();
    open(&mut rig);
    let mut sim = rig.sim(SIZE);
    sim.click("Connect as guest").expect("button");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Fics(FicsUiMessage::Connect))),
        "{messages:?}"
    );
    rig.msg(Message::Fics(FicsUiMessage::Connect));
    assert_eq!(rig.shell.app.fics().phase, Phase::Connecting);
}

#[test]
fn a_named_login_stages_the_password_in_the_hub_and_forgets_it_in_the_window() {
    let mut rig = Rig::new();
    open(&mut rig);
    rig.msg(Message::Fics(FicsUiMessage::Name("alice".into())));
    rig.msg(Message::Fics(FicsUiMessage::Password(Secret::new(
        "hunter2",
    ))));
    let shown = format!("{:?}", rig.shell.ui.dialogs.fics);
    assert!(!shown.contains("hunter2"), "D2602: {shown}");
    assert!(!format!("{:?}", FicsUiMessage::Password(Secret::new("hunter2"))).contains("hunter2"));
    rig.msg(Message::Fics(FicsUiMessage::Connect));
    assert!(
        rig.shell.ui.dialogs.fics.password.is_blank(),
        "wiped at once"
    );
    assert_eq!(
        rig.shell.app.prefs.fics.login, "alice",
        "the handle is kept"
    );
    assert_eq!(rig.shell.app.fics().phase, Phase::Connecting);
}

#[test]
fn the_handle_field_takes_only_letters_and_digits() {
    let mut rig = Rig::new();
    open(&mut rig);
    rig.msg(Message::Fics(FicsUiMessage::Name("al ice;quit\n".into())));
    assert_eq!(rig.shell.ui.dialogs.fics.name, "aliceq".to_owned() + "uit");
}

#[test]
fn the_logged_in_window_shows_the_graph_and_a_picked_dot_can_be_played() {
    let mut rig = Rig::new();
    rig.shell.app.fics.phase = Phase::LoggedIn;
    rig.shell.app.fics.me = Some("alice".into());
    rig.shell.app.fics.seeks.add(seek(7, "bob", 5, "blitz"));
    rig.shell
        .app
        .fics
        .seeks
        .add(seek(8, "eve", 5, "crazyhouse"));
    open(&mut rig);
    assert!(has(&rig, "Seek graph (2)"));
    assert!(has(&rig, "Logged in as alice"));
    rig.msg(Message::Fics(FicsUiMessage::SelectSeek(Some(7))));
    assert!(has(&rig, "bob (1500) 5 + 0, blitz, Unrated"));
    let mut sim = rig.sim(SIZE);
    sim.click("Play this seek").expect("play button");
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Fics(FicsUiMessage::PlaySelected))),
        "{messages:?}"
    );
    // A variant is shown but cannot be played (D2600).
    rig.msg(Message::Fics(FicsUiMessage::SelectSeek(Some(8))));
    let mut sim = rig.sim(SIZE);
    drop(sim.click("Play this seek"));
    let messages: Vec<Message> = sim.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::Fics(FicsUiMessage::PlaySelected))),
        "the button is disabled for a variant"
    );
}

#[test]
fn a_command_typed_in_the_console_is_sent_and_echoed() {
    let mut rig = Rig::new();
    rig.shell.app.fics.phase = Phase::LoggedIn;
    open(&mut rig);
    rig.msg(Message::Fics(FicsUiMessage::Command("who".into())));
    rig.msg(Message::Fics(FicsUiMessage::SubmitCommand));
    assert!(
        rig.shell.ui.dialogs.fics.command.is_empty(),
        "field cleared"
    );
    assert!(
        rig.shell
            .app
            .fics()
            .console
            .lines()
            .any(|l| l.text == "who"),
        "echoed"
    );
}

#[test]
fn a_command_cannot_carry_a_second_command() {
    let mut rig = Rig::new();
    rig.shell.app.fics.phase = Phase::LoggedIn;
    open(&mut rig);
    rig.msg(Message::Fics(FicsUiMessage::Command("who\nquit".into())));
    rig.msg(Message::Fics(FicsUiMessage::SubmitCommand));
    assert!(
        rig.shell
            .app
            .fics()
            .console
            .lines()
            .all(|l| l.text != "quit"),
        "D2604"
    );
}

#[test]
fn the_settings_of_the_window_reach_the_preferences() {
    let mut rig = Rig::new();
    rig.shell.app.fics.phase = Phase::LoggedIn;
    open(&mut rig);
    assert!(rig.shell.app.prefs.fics.premove);
    let mut prefs = rig.shell.app.fics().prefs.clone();
    prefs.premove = false;
    rig.app_msg(mychess_app::Message::Fics(FicsMessage::SetPrefs(Box::new(
        prefs,
    ))));
    assert!(!rig.shell.app.prefs.fics.premove, "D2628 kept in [fics]");
}

#[test]
fn the_online_menu_has_the_fics_entry() {
    let rig = Rig::new();
    let i18n = I18n::new(Language::EnUs);
    assert_eq!(i18n.tr("menu-online-fics"), "FICS...");
    drop(rig);
}

// ---- Fluent ----

fn keys(language: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/i18n")
        .join(language)
        .join("fics.ftl");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(' '))
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, _)| k.trim().to_owned())
        .collect()
}

#[test]
fn both_languages_have_the_same_fics_keys() {
    let en = keys("en-US");
    let pt = keys("pt-BR");
    assert!(en.len() > 60, "the file was read: {}", en.len());
    assert_eq!(en, pt);
}

#[test]
fn every_fics_key_the_code_asks_for_exists_in_both_languages() {
    let en = keys("en-US");
    let online = {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n/en-US/online.ftl");
        std::fs::read_to_string(path).unwrap()
    };
    for file in ["fics_dialog.rs", "fics_graph.rs"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/windows")
            .join(file);
        let src = std::fs::read_to_string(path).unwrap();
        for piece in src.split('"').skip(1).step_by(2) {
            let is_widget_id = matches!(
                piece,
                "fics-box" | "fics-command" | "fics-name" | "fics-confirm-box"
            );
            if !is_widget_id
                && piece.starts_with("fics-")
                && piece.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            {
                assert!(en.contains(piece), "missing {piece} in fics.ftl");
            }
            if piece.starts_with("online-")
                && piece.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            {
                assert!(online.contains(&format!("{piece} =")), "missing {piece}");
            }
        }
    }
}

#[test]
fn every_fics_remark_has_a_text_in_both_languages() {
    use mychess_app::fics::Disconnect;
    let notices = [
        FicsNotice::Connected { name: "a".into() },
        FicsNotice::Disconnected(Disconnect::ServerClosed),
        FicsNotice::Disconnected(Disconnect::Timeout),
        FicsNotice::Disconnected(Disconnect::ConnectFailed),
        FicsNotice::BadInput,
        FicsNotice::WrongState,
        FicsNotice::AssistanceDisabled,
        FicsNotice::MoveRejected,
        FicsNotice::GameStarted,
        FicsNotice::GameEnded,
        FicsNotice::Imported,
        FicsNotice::AlreadyImported,
        FicsNotice::NotImported,
        FicsNotice::ImportFailed,
        FicsNotice::Incomplete,
        FicsNotice::TooManyObserved,
        FicsNotice::FollowWhilePlaying,
        FicsNotice::LockReleased,
    ];
    for language in [Language::EnUs, Language::PtBr] {
        let i18n = I18n::new(language);
        for notice in &notices {
            let key = notice.message_key();
            let text = i18n.tr(key);
            assert!(!text.is_empty() && text != key, "{key} in {language:?}");
        }
    }
}
