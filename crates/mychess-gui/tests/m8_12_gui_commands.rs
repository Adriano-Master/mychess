//! M8-12 (GUI): the Resign and Rematch buttons of the notation panel, the
//! items of the Game menu and the keys of both languages (spec
//! `docs/specs/m8-12-desistir-e-revanche.md`, D3100-D3108).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use mychess_app::engines::{EngineDraft, EngineMessage};
use mychess_app::{
    Action, DiscardChoice, Message as AppMessage, PlayColorPref, PlayModePref, PlayPhase,
};
use mychess_engine::{Capabilities, EngineIdentity};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::game_commands::{GAME_COMMANDS_ID, REMATCH_BUTTON_ID, RESIGN_BUTTON_ID};
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::play_dialog::PlayUiMessage;
use mychess_gui::{Message, Shell, UiMessage, ViewOptions};

fn send(shell: &mut Shell, message: Message) {
    drop(shell.update(message));
}

fn ui(shell: &mut Shell, message: PlayUiMessage) {
    send(shell, Message::PlayUi(message));
}

fn started() -> Shell {
    let mut shell = Shell::new(None);
    shell
        .app
        .engines
        .add(EngineDraft::new("Fake", "fake-engine"))
        .expect("engine added");
    ui(&mut shell, PlayUiMessage::Open);
    ui(&mut shell, PlayUiMessage::Color(PlayColorPref::White));
    ui(&mut shell, PlayUiMessage::Mode(PlayModePref::TimeBonus));
    ui(&mut shell, PlayUiMessage::Submit);
    ready(&mut shell);
    shell
}

fn ready(shell: &mut Shell) {
    let engine = shell.app.engines.engines().first().expect("engine").id;
    let run = shell.app.engine_runs.of_engine(engine).expect("run").id;
    send(
        shell,
        Message::App(AppMessage::Engine(EngineMessage::Started {
            run,
            identity: Box::new(EngineIdentity {
                name: "Fake".to_owned(),
                author: None,
                options: Vec::new(),
                capabilities: Capabilities::default(),
            }),
            at: 1_700_000_000,
        })),
    );
}

fn present(shell: &Shell, id: &'static str) -> bool {
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        mychess_gui::windows::main_window::view(shell),
    );
    sim.find(iced::widget::Id::new(id)).is_ok()
}

fn click(shell: &Shell, id: &'static str) -> Vec<Message> {
    let mut sim = iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(1300.0, 900.0),
        mychess_gui::windows::main_window::view(shell),
    );
    sim.click(iced::widget::Id::new(id)).expect("the button");
    sim.into_messages().collect()
}

/// `(exists, enabled)` of an item of the Game menu.
fn game_item(shell: &Shell, key: &'static str) -> (bool, bool) {
    let specs = menu_specs(&shell.app, &shell.ui, ViewOptions::default());
    for spec in specs.iter().filter(|s| s.label_key == "menu-game") {
        for entry in &spec.entries {
            if let MenuEntry::Item {
                label: Label::Key(k),
                action,
                ..
            } = entry
                && *k == key
            {
                return (true, action.is_some());
            }
        }
    }
    (false, false)
}

#[test]
fn outside_a_game_the_buttons_are_hidden_and_the_items_disabled() {
    let shell = Shell::new(None);
    assert!(!present(&shell, GAME_COMMANDS_ID));
    assert!(!present(&shell, RESIGN_BUTTON_ID));
    assert_eq!(game_item(&shell, "menu-game-resign"), (true, false));
    assert_eq!(game_item(&shell, "menu-game-rematch"), (true, false));
}

#[test]
fn the_actions_do_nothing_outside_a_game() {
    let mut shell = Shell::new(None);
    send(
        &mut shell,
        Message::Ui(UiMessage::Action(Action::ResignGame)),
    );
    send(&mut shell, Message::Ui(UiMessage::Action(Action::Rematch)));
    assert!(shell.ui.dialogs.play.confirm.is_none());
    assert!(!shell.app.play_live());
}

#[test]
fn in_a_game_resign_shows_and_the_button_sends_the_action() {
    let shell = started();
    assert!(present(&shell, RESIGN_BUTTON_ID));
    assert!(!present(&shell, REMATCH_BUTTON_ID));
    assert_eq!(game_item(&shell, "menu-game-resign"), (true, true));
    assert_eq!(game_item(&shell, "menu-game-rematch"), (true, false));
    let sent = click(&shell, RESIGN_BUTTON_ID);
    assert!(
        matches!(
            sent.as_slice(),
            [Message::Ui(UiMessage::Action(Action::ResignGame))]
        ),
        "{sent:?}"
    );
}

#[test]
fn resigning_asks_once_and_then_the_game_ends() {
    let mut shell = started();
    send(
        &mut shell,
        Message::Ui(UiMessage::Action(Action::ResignGame)),
    );
    assert!(shell.ui.dialogs.play.confirm.is_some(), "the mode asks");
    assert_eq!(shell.app.play_phase(), Some(PlayPhase::Player));
    ui(&mut shell, PlayUiMessage::Confirm(true));
    assert!(matches!(shell.app.play_phase(), Some(PlayPhase::Ended(_))));
    assert!(
        shell.ui.dialogs.play.confirm.is_none(),
        "no second question"
    );
}

#[test]
fn the_rematch_after_the_end_swaps_the_colours() {
    let mut shell = started();
    send(
        &mut shell,
        Message::Ui(UiMessage::Action(Action::ResignGame)),
    );
    ui(&mut shell, PlayUiMessage::Confirm(true));
    // The end box is modal: the item and the button of the panel are off
    // the way, the action does nothing under it, the box has its own button.
    assert!(shell.modal_layer_open());
    assert_eq!(game_item(&shell, "menu-game-rematch"), (true, true));
    send(&mut shell, Message::Ui(UiMessage::Action(Action::Rematch)));
    assert!(matches!(shell.app.play_phase(), Some(PlayPhase::Ended(_))));
    // The resigned game is not saved: the question of the restart comes
    // first (D3102). Cancelling drops the rematch with a remark.
    ui(&mut shell, PlayUiMessage::Rematch);
    assert!(shell.app.discard_prompt.is_some());
    send(
        &mut shell,
        Message::App(AppMessage::AnswerDiscard(DiscardChoice::Cancel)),
    );
    assert!(matches!(shell.app.play_phase(), Some(PlayPhase::Ended(_))));
    assert_eq!(shell.ui.dialogs.play.note, Some("play-rematch-dropped"));
    ui(&mut shell, PlayUiMessage::Rematch);
    send(
        &mut shell,
        Message::App(AppMessage::AnswerDiscard(DiscardChoice::DontSave)),
    );
    assert!(shell.app.play_live());
    ready(&mut shell);
    let view = shell
        .app
        .play_view(std::time::Instant::now())
        .expect("a game");
    assert_eq!(view.player_color, mychess_core::Color::Black);
    assert!(!shell.modal_layer_open());
    assert!(present(&shell, RESIGN_BUTTON_ID));
}

#[test]
fn the_rematch_button_shows_when_the_game_is_over() {
    let mut shell = started();
    ui(&mut shell, PlayUiMessage::Resign);
    ui(&mut shell, PlayUiMessage::Confirm(true));
    assert!(present(&shell, REMATCH_BUTTON_ID));
    assert!(!present(&shell, RESIGN_BUTTON_ID));
}

#[test]
fn the_keys_exist_in_both_languages() {
    for language in Language::ALL {
        let i18n = I18n::new(language);
        for key in [
            "action-resign-game",
            "action-rematch",
            "menu-game-resign",
            "menu-game-rematch",
            "game-cmd-resign",
            "game-cmd-resign-tip",
            "game-cmd-rematch",
            "game-cmd-rematch-tip",
            "play-rematch-dropped",
        ] {
            assert!(i18n.has(key), "{key} in {language:?}");
        }
    }
    let pt = I18n::new(Language::PtBr);
    assert_eq!(pt.tr("game-cmd-resign"), "Desistir");
    assert_eq!(pt.tr("game-cmd-rematch"), "Revanche");
    let en = I18n::new(Language::EnUs);
    assert_eq!(en.tr("menu-game-resign"), "Resign");
}
