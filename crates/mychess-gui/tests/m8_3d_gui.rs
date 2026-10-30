//! M8-3d (GUI): the takeback, claim-draw and rematch buttons, the window to
//! watch a game, the log-in through the browser and the piece chooser of a
//! premove that promotes (spec `docs/specs/m8-3-lichess-online.md`,
//! D3000-D3015). Headless: no network, the effects are never run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

#[path = "m2_4_common/mod.rs"]
mod common;
#[path = "m8_3_gui_common/mod.rs"]
mod online_common;

use std::time::Instant;

use common::Rig;
use iced::Size;
use mychess_app::online::{EndKind, OnlineOutcome, OnlinePhase, OpponentGone, PlayingState};
use mychess_core::{GameResult, Role, Square};
use mychess_gui::Message;
use mychess_gui::ViewOptions;
use mychess_gui::widgets::board::premove::PremoveEvent;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::online_ui::{OnlineDialog, OnlineUiMessage};

const SIZE: Size = Size::new(1400.0, 900.0);

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn has(rig: &Rig, label: &str) -> bool {
    rig.sim(SIZE).find(label).is_ok()
}

fn click(rig: &Rig, label: &str) -> Vec<Message> {
    let mut sim = rig.sim(SIZE);
    sim.click(label)
        .unwrap_or_else(|e| panic!("no button {label:?}: {e:?}"));
    sim.into_messages().collect()
}

fn sent(messages: &[Message], wanted: &OnlineUiMessage) -> bool {
    messages
        .iter()
        .any(|m| matches!(m, Message::Online(got) if format!("{got:?}") == format!("{wanted:?}")))
}

fn menu_item_enabled(rig: &Rig, key: &str) -> Option<bool> {
    let specs = menu_specs(&rig.shell.app, &rig.shell.ui, ViewOptions::default());
    let menu = specs.iter().find(|s| s.label_key == "menu-online")?;
    menu.entries.iter().find_map(|e| match e {
        MenuEntry::Item {
            label: Label::Key(k) | Label::Numbered(k, _),
            action,
            ..
        } if *k == key => Some(action.is_some()),
        _ => None,
    })
}

// ---- takeback ----

#[test]
fn the_takeback_button_appears_after_a_move_of_the_user_and_sends_the_proposal() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert!(!has(&rig, "Ask for a takeback"), "no move of his own yet");
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    assert!(has(&rig, "Ask for a takeback"));
    assert!(sent(
        &click(&rig, "Ask for a takeback"),
        &OnlineUiMessage::ProposeTakeback
    ));
}

#[test]
fn the_takeback_button_is_gone_while_the_opponent_proposes_one() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    online_common::game_mut(&mut rig).btakeback = true;
    rig.sync();
    assert!(!has(&rig, "Ask for a takeback"));
    assert!(has(&rig, "Accept takeback"));
}

// ---- claim a draw ----

#[test]
fn the_claim_draw_button_waits_for_the_opponent_to_be_claimable() {
    let mut rig = Rig::new();
    let t0 = Instant::now();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", t0);
    rig.shell.app.online.opponent_gone = Some(OpponentGone {
        claim_secs: Some(60),
        since: t0,
    });
    rig.sync();
    assert!(has(&rig, "Claim a draw"));
    let early = {
        let mut sim = rig.sim(SIZE);
        sim.click("Claim a draw").ok();
        sim.into_messages().collect::<Vec<_>>()
    };
    assert!(early.is_empty(), "too early");
    rig.shell.app.online.opponent_gone = Some(OpponentGone {
        claim_secs: Some(0),
        since: t0,
    });
    rig.sync();
    assert!(sent(
        &click(&rig, "Claim a draw"),
        &OnlineUiMessage::ClaimDraw
    ));
}

// ---- rematch ----

#[test]
fn a_finished_game_offers_the_rematch() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    assert!(!has(&rig, "Rematch"), "not while the game is on");
    rig.shell.app.online.phase = OnlinePhase::Playing(PlayingState::Finished);
    rig.shell.app.online.finished = Some(OnlineOutcome {
        result: GameResult::White,
        kind: EndKind::Resign,
        icon: None,
    });
    rig.sync();
    assert!(has(&rig, "Rematch"));
    assert!(sent(&click(&rig, "Rematch"), &OnlineUiMessage::Rematch));
}

// ---- watching ----

#[test]
fn the_watch_item_opens_a_window_that_takes_a_link() {
    let mut rig = Rig::new();
    assert_eq!(
        menu_item_enabled(&rig, "menu-online-watch"),
        Some(true),
        "works without a login"
    );
    assert_eq!(
        menu_item_enabled(&rig, "menu-online-stop-watch"),
        Some(false)
    );
    rig.msg(Message::Online(OnlineUiMessage::Open(OnlineDialog::Watch)));
    assert!(has(&rig, "Watch a game"));
    // Nothing typed: the main button does nothing.
    rig.msg(Message::Online(OnlineUiMessage::Submit));
    assert!(!rig.shell.app.online.is_watching());
    rig.msg(Message::Online(OnlineUiMessage::WatchInput(
        "https://lichess.org/abcd1234".to_owned(),
    )));
    assert_eq!(
        rig.shell.ui.dialogs.online.watch_input,
        "https://lichess.org/abcd1234"
    );
}

#[test]
fn watching_is_not_offered_while_a_game_of_the_user_is_on() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert_eq!(menu_item_enabled(&rig, "menu-online-watch"), Some(false));
}

#[test]
fn a_bad_reference_is_refused_and_nothing_is_watched() {
    let mut rig = Rig::new();
    rig.msg(Message::Online(OnlineUiMessage::Open(OnlineDialog::Watch)));
    rig.msg(Message::Online(OnlineUiMessage::WatchInput(
        "not a game".to_owned(),
    )));
    rig.msg(Message::Online(OnlineUiMessage::Submit));
    assert!(!rig.shell.app.online.is_watching());
}

// ---- login through the browser ----

#[test]
fn the_connect_window_offers_the_login_through_the_browser() {
    let mut rig = Rig::new();
    rig.msg(Message::Online(OnlineUiMessage::Open(
        OnlineDialog::Connect,
    )));
    assert!(has(&rig, "Log in with the browser"));
    assert!(sent(
        &click(&rig, "Log in with the browser"),
        &OnlineUiMessage::OAuthLogin
    ));
}

// ---- the piece of a premove that promotes ----

fn promoting_game(rig: &mut Rig) {
    online_common::start_game(
        rig,
        "alice",
        "4k3/P7/8/8/8/8/8/4K3 b - - 0 1",
        "",
        Instant::now(),
    );
    // The editor shows the position of the game, as it does in the program.
    let position = online_common::game_mut(rig).position().clone();
    rig.set_game(mychess_core::Game::from_position(position));
}

#[test]
fn a_premove_that_promotes_asks_for_the_piece_and_keeps_the_choice() {
    let mut rig = Rig::new();
    promoting_game(&mut rig);
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Set {
            from: sq("a7"),
            to: sq("a8"),
        },
    )));
    assert!(
        !rig.shell.app.online.premove.is_set(),
        "waits for the piece"
    );
    assert_eq!(
        rig.shell.ui.dialogs.online.premove_promotion,
        Some((sq("a7"), sq("a8")))
    );
    assert!(has(&rig, "Promote the pawn to"));
    assert!(has(&rig, "Knight"));
    assert!(matches!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::OnlinePromotion)
    ));
    rig.msg(Message::Online(OnlineUiMessage::PromotionChoice(Some(
        Role::Knight,
    ))));
    let kept = rig.shell.app.online.premove.get().expect("premove kept");
    assert_eq!(kept.promotion, Some(Role::Knight));
    assert!(rig.shell.ui.dialogs.online.premove_promotion.is_none());
}

#[test]
fn cancelling_the_chooser_keeps_no_premove() {
    let mut rig = Rig::new();
    promoting_game(&mut rig);
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Set {
            from: sq("a7"),
            to: sq("a8"),
        },
    )));
    rig.msg(Message::Online(OnlineUiMessage::PromotionChoice(None)));
    assert!(!rig.shell.app.online.premove.is_set());
    assert!(rig.shell.ui.dialogs.online.premove_promotion.is_none());
}
