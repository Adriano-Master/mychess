//! M8-3 / M8-9 (D4520): entering an online game closes the windows over the
//! board, keeps the questions about the data of the user, closes nothing on a
//! resynchronisation, and gives the board the focus. Headless.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

#[path = "m2_4_common/mod.rs"]
mod common;
#[path = "m8_3_gui_common/mod.rs"]
mod online_common;

use common::Rig;
use mychess_app::fics::FicsMessage;
use mychess_app::{Action, Message as AppMessage};
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::online_ui::{OnlineDialog, OnlineUiMessage};
use mychess_gui::{Message, UiMessage};

fn enter(rig: &mut Rig) {
    rig.shell.app.online_entries += 1;
    // Any message: the shell reacts after every update.
    rig.msg(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
}

fn open_online(rig: &mut Rig, dialog: OnlineDialog) {
    rig.msg(Message::Online(OnlineUiMessage::Open(dialog)));
}

#[test]
fn m8_3_d4520_each_online_window_closes_when_a_game_is_entered() {
    for dialog in [
        OnlineDialog::Seek,
        OnlineDialog::Challenge,
        OnlineDialog::Incoming,
        OnlineDialog::Watch,
    ] {
        let mut rig = Rig::new();
        online_common::connect(&mut rig);
        open_online(&mut rig, dialog);
        assert!(rig.shell.modal_layer_open(), "{dialog:?} is open");
        enter(&mut rig);
        assert!(!rig.shell.modal_layer_open(), "{dialog:?} closed");
    }
}

#[test]
fn m8_3_d4520_the_fics_window_and_the_manual_and_about_close_too() {
    let mut rig = Rig::new();
    rig.app_msg(AppMessage::Fics(FicsMessage::OpenWindow));
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::Fics)
    );
    enter(&mut rig);
    assert!(!rig.shell.modal_layer_open());

    let mut rig = Rig::new();
    rig.msg(Message::Ui(UiMessage::Action(Action::Help)));
    assert!(rig.shell.app.manual().is_open());
    rig.msg(Message::Ui(UiMessage::ShowAbout(true)));
    enter(&mut rig);
    assert!(!rig.shell.app.manual().is_open());
    assert!(!rig.shell.ui.show_about);
    assert!(!rig.shell.modal_layer_open());
}

#[test]
fn m8_3_d4520_stacked_windows_all_close() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    rig.msg(Message::Ui(UiMessage::Action(Action::Help)));
    open_online(&mut rig, OnlineDialog::Seek);
    rig.app_msg(AppMessage::Fics(FicsMessage::OpenWindow));
    enter(&mut rig);
    assert!(!rig.shell.modal_layer_open());
    assert!(!rig.shell.app.manual().is_open());
}

#[test]
fn m8_3_d4520_no_entry_no_closing() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    open_online(&mut rig, OnlineDialog::Seek);
    // A resynchronisation leaves the counter alone: nothing closes.
    rig.msg(Message::Ui(UiMessage::SystemTheme(iced::theme::Mode::Dark)));
    assert!(rig.shell.modal_layer_open());
    enter(&mut rig);
    assert!(!rig.shell.modal_layer_open());
    // The windows opened after the entry stay: one reaction per entry.
    open_online(&mut rig, OnlineDialog::Seek);
    rig.msg(Message::Ui(UiMessage::SystemTheme(
        iced::theme::Mode::Light,
    )));
    assert!(rig.shell.modal_layer_open());
}

#[test]
fn m8_3_d4520_a_question_about_the_data_of_the_user_stays() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    // An altered game and "New game": the question comes first.
    rig.app_msg(AppMessage::Game(mychess_app::GameMessage::PlayMove {
        mv: rig.shell.app.session.position().parse_uci("e2e4").unwrap(),
        options: mychess_app::PlayOptions::default(),
    }));
    rig.app_msg(AppMessage::Command(mychess_app::Command::NewGame));
    assert!(rig.shell.app.discard_prompt.is_some());
    enter(&mut rig);
    assert!(
        rig.shell.app.discard_prompt.is_some(),
        "the save question is not answered for the user"
    );
    assert!(rig.shell.modal_layer_open());
}
