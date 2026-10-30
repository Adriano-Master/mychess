//! M8-3c (GUI): the panel under the board during an online game, the
//! clocks in the strips, the notice of the lock of the assistance, the
//! online menu and the premove on the board (spec
//! `docs/specs/m8-3-lichess-online.md`, D1321, D1390-D1394, D1470-D1499).
//! Headless: no network; the session is put in the state the streams leave.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

#[path = "m2_4_common/mod.rs"]
mod common;
#[path = "m8_3_gui_common/mod.rs"]
mod online_common;

use std::time::{Duration, Instant};

use common::Rig;
use iced::Size;
use mychess_app::online::{
    EndKind, OnlineOutcome, OnlinePhase, OpponentGone, PlayingState, Premove,
};
use mychess_app::{Action, Message as AppMessage};
use mychess_core::{Color, GameResult, Role, Square};
use mychess_gui::ViewOptions;
use mychess_gui::widgets::board::interaction::BoardEvent;
use mychess_gui::widgets::board::premove::PremoveEvent;
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
use mychess_gui::windows::online_ui::{OnlineConfirm, OnlineUiMessage};
use mychess_gui::{Message, UiMessage};

const SIZE: Size = Size::new(1400.0, 900.0);

fn sq(name: &str) -> Square {
    name.parse().expect("square")
}

fn has(rig: &Rig, label: &str) -> bool {
    rig.sim(SIZE).find(label).is_ok()
}

fn click_opt(rig: &Rig, label: &str) -> Vec<Message> {
    let mut sim = rig.sim(SIZE);
    if sim.click(label).is_err() {
        return Vec::new();
    }
    sim.into_messages().collect()
}

fn click(rig: &Rig, label: &str) -> Vec<Message> {
    let mut sim = rig.sim(SIZE);
    sim.click(label)
        .unwrap_or_else(|e| panic!("no button {label:?}: {e:?}"));
    sim.into_messages().collect()
}

fn id_present(rig: &Rig, id: &'static str) -> bool {
    rig.sim(SIZE).find(iced::widget::Id::new(id)).is_ok()
}

fn sent(messages: &[Message], wanted: &OnlineUiMessage) -> bool {
    messages
        .iter()
        .any(|m| matches!(m, Message::Online(got) if format!("{got:?}") == format!("{wanted:?}")))
}

// ---- the buttons follow the state of the game ----

#[test]
fn before_two_plies_the_panel_offers_abort_and_afterwards_resign() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert!(id_present(&rig, "online-bar"));
    assert!(has(&rig, "Your turn"));
    assert!(has(&rig, "Abort"));
    assert!(!has(&rig, "Resign"));
    assert!(has(&rig, "Offer draw"));
    assert!(sent(&click(&rig, "Abort"), &OnlineUiMessage::Abort));

    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    assert!(has(&rig, "Resign"));
    assert!(!has(&rig, "Abort"));
    assert!(sent(&click(&rig, "Resign"), &OnlineUiMessage::Resign));
}

#[test]
fn the_panel_names_the_opponent_and_the_game() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert!(has(&rig, "vs Bob (1500?)  ·  Casual 10+5"));
    online_common::start_game(&mut rig, "bob", "startpos", "", Instant::now());
    assert!(has(&rig, "Opponent's turn"));
    assert!(has(&rig, "vs NM alice (1800)  ·  Casual 10+5"));
}

#[test]
fn a_draw_offered_by_the_opponent_is_answered_in_the_panel() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    assert!(has(&rig, "Offer draw"));
    online_common::game_mut(&mut rig).bdraw = true;
    rig.sync();
    assert!(!has(&rig, "Offer draw"));
    assert!(sent(
        &click(&rig, "Accept draw"),
        &OnlineUiMessage::AnswerDraw(true)
    ));
    assert!(sent(
        &click(&rig, "Decline"),
        &OnlineUiMessage::AnswerDraw(false)
    ));
}

#[test]
fn the_own_offer_waits_and_the_button_is_off() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    online_common::game_mut(&mut rig).wdraw = true;
    rig.sync();
    assert!(has(&rig, "Offer draw"));
    assert!(click_opt(&rig, "Offer draw").is_empty());
}

#[test]
fn a_takeback_proposed_is_answered_in_the_panel() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    online_common::game_mut(&mut rig).btakeback = true;
    rig.sync();
    assert!(sent(
        &click(&rig, "Accept takeback"),
        &OnlineUiMessage::AnswerTakeback(true)
    ));
    assert!(sent(
        &click(&rig, "Decline takeback"),
        &OnlineUiMessage::AnswerTakeback(false)
    ));
}

#[test]
fn the_claim_of_victory_opens_when_the_wait_is_over() {
    let mut rig = Rig::new();
    let t0 = Instant::now();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", t0);
    rig.shell.app.online.opponent_gone = Some(OpponentGone {
        claim_secs: Some(30),
        since: t0,
    });
    rig.msg(Message::Online(OnlineUiMessage::Tick(
        t0 + Duration::from_secs(5),
    )));
    rig.sync();
    assert!(has(&rig, "Claim victory"));
    assert!(click_opt(&rig, "Claim victory").is_empty(), "too early");
    // Later the same button works.
    rig.shell.app.online.opponent_gone = Some(OpponentGone {
        claim_secs: Some(0),
        since: t0,
    });
    rig.sync();
    assert!(has(
        &rig,
        "The opponent left. You can claim the victory now."
    ));
    assert!(sent(&click(&rig, "Claim victory"), &OnlineUiMessage::Claim));
}

#[test]
fn a_lost_connection_shows_in_the_panel() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert!(has(&rig, "Connected"));
    rig.shell.app.online.phase = OnlinePhase::Playing(PlayingState::Reconnecting);
    rig.shell.app.online.link = mychess_app::online::Link::Reconnecting;
    rig.sync();
    assert!(has(&rig, "Reconnecting..."));
    assert!(has(&rig, "Reconnecting"));
}

#[test]
fn the_ping_shows_unless_the_preference_hides_it() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    rig.shell.app.online.ping = Some(Duration::from_millis(87));
    rig.sync();
    assert!(has(&rig, "Connected  ·  87 ms"));
    let mut prefs = rig.shell.app.prefs.online.as_ref().clone();
    prefs.show_ping = false;
    *rig.shell.app.prefs.online = prefs;
    rig.sync();
    assert!(!has(&rig, "Connected  ·  87 ms"));
}

#[test]
fn a_finished_game_shows_the_result_and_close_leaves_it() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    rig.shell.app.online.phase = OnlinePhase::Playing(PlayingState::Finished);
    rig.shell.app.online.finished = Some(OnlineOutcome {
        result: GameResult::White,
        kind: EndKind::Resign,
        icon: None,
    });
    rig.sync();
    assert!(has(&rig, "You won. By resignation."));
    assert!(!has(&rig, "Resign"));
    assert!(sent(&click(&rig, "Close"), &OnlineUiMessage::Dismiss));
    assert!(
        !rig.shell.app.assist_locked(),
        "the lock is gone with the game"
    );
}

// ---- the strips and the clocks ----

#[test]
fn the_strips_carry_the_clocks_of_the_server_and_follow_the_ticks() {
    let mut rig = Rig::new();
    let t0 = Instant::now();
    online_common::start_game(&mut rig, "alice", "startpos", "", t0);
    let strips = rig.shell.ui.dialogs.online.strips.clone().expect("strips");
    assert!(strips.clock_slot);
    assert_eq!(strips.white.clock, Some(600));
    assert_eq!(strips.black.clock, Some(600));
    // White is to move: only its clock runs.
    rig.msg(Message::Online(OnlineUiMessage::Tick(
        t0 + Duration::from_secs(12),
    )));
    let strips = rig.shell.ui.dialogs.online.strips.clone().expect("strips");
    assert_eq!(strips.white.clock, Some(588));
    assert_eq!(strips.black.clock, Some(600));
}

#[test]
fn the_ticks_run_only_while_a_game_is_live() {
    let mut rig = Rig::new();
    assert!(!mychess_gui::windows::online_ui::wants_ticks(
        &rig.shell.app
    ));
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert!(mychess_gui::windows::online_ui::wants_ticks(&rig.shell.app));
    rig.shell.app.online.phase = OnlinePhase::Playing(PlayingState::Finished);
    assert!(!mychess_gui::windows::online_ui::wants_ticks(
        &rig.shell.app
    ));
}

// ---- the lock of the assistance ----

#[test]
fn the_notice_of_the_lock_is_permanent_while_the_game_is_on() {
    let mut rig = Rig::new();
    assert!(!id_present(&rig, "online-lock-banner"));
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert!(id_present(&rig, "online-lock-banner"));
    assert!(has(
        &rig,
        "Online game: assistance disabled (engines, opening tree, tablebases and searches)"
    ));
    // Online, the way out is not offered; offline it is.
    assert!(!has(&rig, "Remove lock..."));
    rig.shell.app.online.link = mychess_app::online::Link::Down;
    rig.sync();
    assert!(has(&rig, "Remove lock..."));
}

#[test]
fn a_message_of_the_engine_family_is_refused_with_a_notice_during_the_game() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    let effects = mychess_app::update(
        &mut rig.shell.app,
        AppMessage::Action(Action::ToggleEngineAnalysis),
    );
    assert_eq!(effects.len(), 0);
    assert!(matches!(
        rig.shell.app.notice,
        Some(mychess_app::AppNotice::Online(
            mychess_app::online::OnlineNotice::AssistanceDisabled
        ))
    ));
}

// ---- the menu ----

fn online_items(rig: &Rig) -> Vec<(&'static str, bool)> {
    let specs = menu_specs(&rig.shell.app, &rig.shell.ui, ViewOptions::default());
    let menu = specs
        .iter()
        .find(|s| s.label_key == "menu-online")
        .expect("the Online menu");
    menu.entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item {
                label: Label::Key(key) | Label::Numbered(key, _),
                action,
                ..
            } => Some((*key, action.is_some())),
            _ => None,
        })
        .collect()
}

fn enabled(items: &[(&'static str, bool)], key: &str) -> bool {
    items
        .iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("no item {key}"))
        .1
}

#[test]
fn the_online_menu_follows_the_session() {
    let mut rig = Rig::new();
    let items = online_items(&rig);
    assert!(enabled(&items, "menu-online-connect"));
    assert!(!enabled(&items, "menu-online-disconnect"));
    assert!(!enabled(&items, "menu-online-seek"));
    assert!(!enabled(&items, "menu-online-remove-lock"));

    online_common::connect(&mut rig);
    let items = online_items(&rig);
    assert!(!enabled(&items, "menu-online-connect"));
    assert!(enabled(&items, "menu-online-seek"));
    assert!(enabled(&items, "menu-online-challenge"));
    assert!(enabled(&items, "menu-online-incoming"));
    assert!(enabled(&items, "menu-online-disconnect"));

    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    let items = online_items(&rig);
    assert!(!enabled(&items, "menu-online-seek"));
    assert!(!enabled(&items, "menu-online-disconnect"), "not in a game");
    // The lock goes by hand only without the network (D1332, D1373).
    assert!(!enabled(&items, "menu-online-remove-lock"));
    rig.shell.app.online.link = mychess_app::online::Link::Down;
    rig.sync();
    assert!(enabled(&online_items(&rig), "menu-online-remove-lock"));
}

#[test]
fn the_shortcuts_use_the_command_key() {
    let rig = Rig::new();
    for action in [Action::OnlineConnect, Action::OnlineSeek] {
        let binding = rig
            .shell
            .app
            .shortcuts
            .bindings(action)
            .first()
            .copied()
            .expect("bound");
        assert!(binding.modifiers.command, "{action:?} uses Command/Ctrl");
        assert!(binding.modifiers.shift);
    }
}

// ---- the moves and the premove ----

#[test]
fn a_move_of_the_board_goes_to_the_game_of_the_server() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    let mv = rig
        .shell
        .app
        .online
        .game
        .as_ref()
        .unwrap()
        .position()
        .find_move(sq("e2"), sq("e4"), None)
        .expect("legal");
    rig.msg(Message::Ui(UiMessage::Board(BoardEvent::PlayMove {
        mv,
        dragged: false,
    })));
    let game = rig.shell.app.online.game.as_ref().unwrap();
    assert_eq!(game.plies(), 1);
    assert!(game.has_pending_move(), "waits for the server");
}

#[test]
fn the_board_takes_a_premove_only_on_the_turn_of_the_opponent() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert!(
        rig.shell.ui.dialogs.online.premove_rule.get().is_none(),
        "my turn: a move, not a premove"
    );
    online_common::start_game(&mut rig, "bob", "startpos", "", Instant::now());
    let rule = rig
        .shell
        .ui
        .dialogs
        .online
        .premove_rule
        .get()
        .expect("rule");
    assert!(rule(sq("e7"), sq("e5")), "a pawn can do that");
    assert!(!rule(sq("e7"), sq("e4")), "no pawn can do that");
    assert!(!rule(sq("e2"), sq("e4")), "not his piece");
    // The board is drawn with the layer of the premove.
    assert!(rig.sim(SIZE).find("Opponent's turn").is_ok());
}

#[test]
fn the_preference_turns_the_premove_off() {
    let mut rig = Rig::new();
    let mut prefs = rig.shell.app.prefs.online.as_ref().clone();
    prefs.premove = false;
    *rig.shell.app.prefs.online = prefs;
    online_common::start_game(&mut rig, "bob", "startpos", "", Instant::now());
    assert!(rig.shell.ui.dialogs.online.premove_rule.get().is_none());
}

#[test]
fn a_premove_is_kept_and_cancelled_by_the_events_of_the_board() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "bob", "startpos", "", Instant::now());
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Set {
            from: sq("e7"),
            to: sq("e5"),
        },
    )));
    let kept = rig.shell.app.online.premove.get().expect("premove kept");
    assert_eq!(
        (kept.from, kept.to, kept.promotion),
        (sq("e7"), sq("e5"), None)
    );
    assert!(has(&rig, "Premove set (Esc cancels it)"));
    // A second one replaces it: one at a time.
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Set {
            from: sq("g8"),
            to: sq("f6"),
        },
    )));
    assert_eq!(rig.shell.app.online.premove.get().unwrap().from, sq("g8"));
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Cancel,
    )));
    assert!(!rig.shell.app.online.premove.is_set());
    assert!(!has(&rig, "Premove set (Esc cancels it)"));
}

#[test]
fn a_premove_that_no_piece_could_make_is_not_kept() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "bob", "startpos", "", Instant::now());
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Set {
            from: sq("e7"),
            to: sq("e3"),
        },
    )));
    assert!(!rig.shell.app.online.premove.is_set());
}

#[test]
fn a_pawn_premoved_to_the_last_rank_asks_for_the_piece_and_a_queen_is_the_default_of_the_rule() {
    let mut rig = Rig::new();
    // White to move... it is Black's turn here, so White (alice) premoves.
    online_common::start_game(
        &mut rig,
        "alice",
        "4k3/P7/8/8/8/8/8/4K3 b - - 0 1",
        "",
        Instant::now(),
    );
    let position = online_common::game_mut(&mut rig).position().clone();
    rig.set_game(mychess_core::Game::from_position(position));
    rig.msg(Message::Online(OnlineUiMessage::Premove(
        PremoveEvent::Set {
            from: sq("a7"),
            to: sq("a8"),
        },
    )));
    // D3013: the chooser opens; the premove waits for the piece.
    assert!(!rig.shell.app.online.premove.is_set());
    assert_eq!(
        rig.shell.ui.dialogs.online.premove_promotion,
        Some((sq("a7"), sq("a8")))
    );
    rig.msg(Message::Online(OnlineUiMessage::PromotionChoice(Some(
        Role::Queen,
    ))));
    let kept = rig.shell.app.online.premove.get().expect("premove kept");
    assert_eq!(kept.promotion, Some(Role::Queen));
    let position = rig
        .shell
        .app
        .online
        .game
        .as_ref()
        .unwrap()
        .position()
        .clone();
    let again = Premove::enter(&position, Color::White, sq("a7"), sq("a8"), None);
    assert_eq!(again.map(|p| p.promotion), Some(Some(Role::Queen)));
}

#[test]
fn a_move_made_while_looking_at_an_earlier_position_is_not_sent() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    // The editor shows a position that is not the one of the game: the
    // board of the editor has one move played.
    let mut other = mychess_core::Game::new();
    let root = other.root();
    let node = common::play(&mut other, root, "d2d4");
    rig.set_game(other);
    rig.goto(node);
    assert_ne!(
        rig.shell.app.session.position().zobrist(),
        rig.shell
            .app
            .online
            .game
            .as_ref()
            .unwrap()
            .position()
            .zobrist()
    );
    let before = rig.shell.app.online.game.as_ref().unwrap().plies();
    let mv2 = rig
        .shell
        .app
        .online
        .game
        .as_ref()
        .unwrap()
        .position()
        .find_move(sq("e2"), sq("e4"), None)
        .expect("legal");
    rig.msg(Message::Ui(UiMessage::Board(BoardEvent::PlayMove {
        mv: mv2,
        dragged: false,
    })));
    assert_eq!(
        rig.shell.app.online.game.as_ref().unwrap().plies(),
        before,
        "nothing was sent"
    );
}

#[test]
fn the_confirmation_of_quitting_can_be_answered_yes() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    rig.msg(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(
        ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs),
        Some(ActiveDialog::OnlineConfirm)
    );
    assert_eq!(
        rig.shell.ui.dialogs.online.confirm,
        Some(OnlineConfirm::Quit)
    );
    rig.msg(Message::Online(OnlineUiMessage::Confirm(true)));
    assert_eq!(rig.shell.ui.dialogs.online.confirm, None);
}
