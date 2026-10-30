//! M8-1d review fixes (GUI, spec D2432): the coach line and the ponder mark
//! of the panel follow the view of the application, which hides them while
//! the assistance is locked.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::time::Instant;

use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::engines::{EngineDraft, EngineMessage};
use mychess_app::online::{MarkerInfo, OnlineMessage};
use mychess_app::{Message as AppMessage, PlayPhase};
use mychess_engine::{Capabilities, EngineIdentity};
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::windows::play_dialog::PlayUiMessage;
use mychess_gui::windows::play_panel::bar;
use mychess_gui::{Message, Shell};

fn send(shell: &mut Shell, message: Message) {
    drop(shell.update(message));
}

#[test]
fn the_coach_line_disappears_from_the_panel_when_the_assistance_is_locked() {
    let mut shell = Shell::new(None);
    let a = shell
        .app
        .engines
        .add(EngineDraft::new("Alpha", "alpha"))
        .expect("added");
    let b = shell
        .app
        .engines
        .add(EngineDraft::new("Bravo", "bravo"))
        .expect("added");
    send(&mut shell, Message::PlayUi(PlayUiMessage::Open));
    send(&mut shell, Message::PlayUi(PlayUiMessage::Engine(a)));
    send(&mut shell, Message::PlayUi(PlayUiMessage::Coach(true)));
    send(&mut shell, Message::PlayUi(PlayUiMessage::CoachEngine(b)));
    send(&mut shell, Message::PlayUi(PlayUiMessage::Submit));
    let run = shell.app.engine_runs.of_engine(a).expect("run").id;
    send(
        &mut shell,
        Message::App(AppMessage::Engine(EngineMessage::Started {
            run,
            identity: Box::new(EngineIdentity {
                name: "Alpha".to_owned(),
                author: None,
                options: Vec::new(),
                capabilities: Capabilities::default(),
            }),
            at: 1_700_000_000,
        })),
    );
    assert!(
        shell
            .app
            .play_phase()
            .is_some_and(|p| p == PlayPhase::Player)
    );
    let i18n = I18n::new(Language::EnUs);
    let view = shell.app.play_view(Instant::now()).expect("a game");
    assert!(view.coach.is_some());
    let mut sim = Simulator::with_size(
        Settings::default(),
        Size::new(1000.0, 200.0),
        bar(&view, &i18n),
    );
    assert!(sim.find("Coach: watching").is_ok());

    // A marker of an online game shows up in the middle of the game.
    send(
        &mut shell,
        Message::App(AppMessage::Online(OnlineMessage::MarkerRead(Some(
            MarkerInfo {
                game_id: "abc12345".to_owned(),
                date: "2026-10-26".to_owned(),
            },
        )))),
    );
    let view = shell.app.play_view(Instant::now()).expect("a game");
    assert!(view.coach.is_none() && !view.pondering);
    let mut sim = Simulator::with_size(
        Settings::default(),
        Size::new(1000.0, 200.0),
        bar(&view, &i18n),
    );
    assert!(sim.find("Coach: watching").is_err());
    // The game itself is still on the panel.
    assert!(sim.find("Your turn").is_ok());
}
