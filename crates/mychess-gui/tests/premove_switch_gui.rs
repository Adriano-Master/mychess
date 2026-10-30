//! The premove switch under the board and the premove of the castling
//! (spec `docs/specs/m8-3-lichess-online.md`, D4450-D4455). Headless: no
//! network, the effects are never run.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "m2_4_common/mod.rs"]
mod common;
#[path = "m8_3_gui_common/mod.rs"]
mod online_common;

use std::time::Instant;

use common::Rig;
use iced::{Settings, Size};
use iced_test::Simulator;
use mychess_app::{Message as AppMessage, PlayMessage, PrefChange, PrefsMessage};
use mychess_core::Square;
use mychess_gui::Message;
use mychess_gui::i18n::{I18n, Language};
use mychess_gui::widgets::board::premove::PremoveEvent;
use mychess_gui::windows::online_ui::OnlineUiMessage;
use mychess_gui::windows::play_panel::premove_line;

const SIZE: Size = Size::new(1400.0, 900.0);

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

/// Alice (White) to castle short, with Black to move.
fn black_to_move(rig: &mut Rig) {
    online_common::start_game(
        rig,
        "alice",
        "startpos",
        "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6 d2d3",
        Instant::now(),
    );
    let position = online_common::game_mut(rig).position().clone();
    rig.set_game(mychess_core::Game::from_position(position));
}

#[test]
fn the_lichess_bar_shows_the_premove_switch_and_it_turns_the_preference_off() {
    let mut rig = Rig::new();
    black_to_move(&mut rig);
    assert!(rig.shell.app.prefs.online.premove, "on by default");
    assert!(rig.shell.ui.dialogs.online.premove_rule.get().is_some());
    let mut sim = rig.sim(SIZE);
    sim.click("Premove").expect("the switch is on screen");
    let messages: Vec<Message> = sim.into_messages().collect();
    let off = messages.iter().any(|m| match m {
        Message::App(AppMessage::Prefs(PrefsMessage::Set(PrefChange::Online(p)))) => !p.premove,
        _ => false,
    });
    assert!(off, "{messages:?}");
}

#[test]
fn with_the_preference_off_the_board_takes_no_premove() {
    let mut rig = Rig::new();
    black_to_move(&mut rig);
    rig.shell.app.prefs.online.premove = false;
    rig.sync();
    assert!(rig.shell.ui.dialogs.online.premove_rule.get().is_none());
}

#[test]
fn castling_can_be_premoved_on_lichess_and_cancelled() {
    let mut rig = Rig::new();
    black_to_move(&mut rig);
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Set {
            from: sq("e1"),
            to: sq("g1"),
        },
    )));
    let kept = rig.shell.app.online.premove.get().expect("kept");
    assert_eq!((kept.from, kept.to), (sq("e1"), sq("g1")));
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Cancel,
    )));
    assert!(!rig.shell.app.online.premove.is_set());
}

#[test]
fn the_play_line_sends_the_switch_and_shows_when_a_premove_waits() {
    let i18n = I18n::new(Language::EnUs);
    let element = premove_line(true, true, &i18n, |on| {
        Message::App(AppMessage::Play(PlayMessage::EnablePremove(on)))
    });
    let mut sim = Simulator::with_size(Settings::default(), SIZE, element);
    assert!(sim.find("Premove set (Esc cancels it)").is_ok());
    sim.click("Premove").expect("the switch");
    let sent: Vec<Message> = sim.into_messages().collect();
    assert!(
        sent.iter().any(|m| matches!(
            m,
            Message::App(AppMessage::Play(PlayMessage::EnablePremove(false)))
        )),
        "{sent:?}"
    );
}

#[test]
fn a_premove_event_without_any_game_is_ignored() {
    let mut rig = Rig::new();
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Set {
            from: sq("e2"),
            to: sq("e4"),
        },
    )));
    assert!(!rig.shell.app.online.premove.is_set());
    assert!(rig.shell.app.play_premove().is_none());
}
