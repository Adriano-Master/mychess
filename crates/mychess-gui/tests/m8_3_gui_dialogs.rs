//! M8-3c (GUI): the windows of the online play on Lichess: connect (the
//! token), find a game, challenge, challenges received and the
//! confirmations. Headless: no network, no stream; the effects of the
//! application are never run (spec `docs/specs/m8-3-lichess-online.md`,
//! D1470-D1499).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

#[path = "m2_4_common/mod.rs"]
mod common;
#[path = "m8_3_gui_common/mod.rs"]
mod online_common;

use std::time::Instant;

use common::Rig;
use iced::Size;
use mychess_app::online::{
    IncomingChallenge, OnlineColor, OnlineError, OnlineNotice, OnlinePhase, StoreChoice,
};
use mychess_app::{Action, NamedKey};
use mychess_gui::windows::dialog_ui::ActiveDialog;
use mychess_gui::windows::online_ui::{OnlineConfirm, OnlineDialog, OnlineUiMessage, Secret};
use mychess_gui::{Message, UiMessage};

const SIZE: Size = Size::new(1200.0, 800.0);

fn active(rig: &Rig) -> Option<ActiveDialog> {
    ActiveDialog::of(&rig.shell.app, &rig.shell.ui.dialogs)
}

fn online(rig: &mut Rig, message: OnlineUiMessage) {
    rig.msg(Message::Online(message));
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

fn escape(rig: &mut Rig) {
    rig.press(common::named(NamedKey::Escape));
}

fn enter(rig: &mut Rig) {
    rig.press(common::named(NamedKey::Enter));
}

const TOKEN: &str = "lip_SecretTokenText123";

// ---- connect ----

#[test]
fn the_shortcut_opens_the_connect_window_and_escape_closes_it() {
    let mut rig = Rig::new();
    rig.msg(Message::Ui(UiMessage::Action(Action::OnlineConnect)));
    assert_eq!(active(&rig), Some(ActiveDialog::OnlineConnect));
    assert!(rig.shell.modal_layer_open(), "the window owns the keyboard");
    assert!(has(&rig, "Connect to Lichess"));
    escape(&mut rig);
    assert_eq!(active(&rig), None);
    assert!(!rig.shell.modal_layer_open());
}

#[test]
fn asking_for_a_game_before_signing_in_leads_to_the_connect_window() {
    let mut rig = Rig::new();
    rig.msg(Message::Ui(UiMessage::Action(Action::OnlineSeek)));
    assert_eq!(active(&rig), Some(ActiveDialog::OnlineConnect));
}

#[test]
fn enter_without_a_token_does_nothing() {
    let mut rig = Rig::new();
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    enter(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Disconnected);
    assert_eq!(active(&rig), Some(ActiveDialog::OnlineConnect));
    // The button is disabled too.
    assert!(
        click_opt(&rig, "Connect").is_empty(),
        "a disabled button sends nothing"
    );
}

fn click_opt(rig: &Rig, label: &str) -> Vec<Message> {
    let mut sim = rig.sim(SIZE);
    if sim.click(label).is_err() {
        return Vec::new();
    }
    sim.into_messages().collect()
}

#[test]
fn the_token_is_staged_and_never_kept_in_a_message_or_in_debug() {
    let mut rig = Rig::new();
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    online(&mut rig, OnlineUiMessage::Store(StoreChoice::Session));
    let typed = Message::Online(OnlineUiMessage::Token(Secret::new(TOKEN)));
    assert!(
        !format!("{typed:?}").contains(TOKEN),
        "the message hides the token"
    );
    rig.msg(typed);
    assert!(
        !format!("{:?}", rig.shell.ui.dialogs.online).contains(TOKEN),
        "the state hides the token"
    );
    enter(&mut rig);
    // The attempt started: the field was emptied and the token is in the
    // hub only.
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Connecting);
    assert!(rig.shell.ui.dialogs.online.token.is_blank());
    assert!(rig.shell.ui.dialogs.online.staged);
    assert!(!format!("{:?}", rig.shell.app.online).contains(TOKEN));
    assert!(has(&rig, "Connecting..."));
}

#[test]
fn text_that_is_not_a_token_is_refused_in_the_window() {
    let mut rig = Rig::new();
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    online(
        &mut rig,
        OnlineUiMessage::Token(Secret::new("not a token!")),
    );
    enter(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Disconnected);
    assert!(rig.shell.ui.dialogs.online.token_rejected);
    assert!(has(&rig, "That does not look like a Lichess token."));
}

#[test]
fn a_failure_of_the_connection_is_shown_in_the_window() {
    let mut rig = Rig::new();
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    rig.shell.app.online.notice = Some(OnlineNotice::Error(OnlineError::Unauthorized));
    rig.sync();
    assert!(has(
        &rig,
        "Lichess refused the token. It may have expired or been revoked."
    ));
}

#[test]
fn the_store_choice_defaults_to_the_keychain_and_follows_the_user() {
    let mut rig = Rig::new();
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    assert_eq!(rig.shell.ui.dialogs.online.store, StoreChoice::Keyring);
    online(&mut rig, OnlineUiMessage::Store(StoreChoice::File));
    assert_eq!(rig.shell.ui.dialogs.online.store, StoreChoice::File);
    // Reopening the window forgets a one-off choice of the session.
    online(&mut rig, OnlineUiMessage::Store(StoreChoice::Session));
    escape(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    assert_eq!(rig.shell.ui.dialogs.online.store, StoreChoice::Keyring);
}

#[test]
fn the_connect_window_closes_by_itself_once_connected() {
    let mut rig = Rig::new();
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    online_common::connect(&mut rig);
    assert_eq!(active(&rig), None);
}

// ---- find a game ----

#[test]
fn the_seek_window_takes_presets_and_starts_the_search() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    rig.msg(Message::Ui(UiMessage::Action(Action::OnlineSeek)));
    assert_eq!(active(&rig), Some(ActiveDialog::OnlineSeek));
    let sent = click(&rig, "10+5");
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::Online(OnlineUiMessage::Preset(10, 5))))
    );
    online(&mut rig, OnlineUiMessage::Preset(10, 5));
    assert_eq!(rig.shell.ui.dialogs.online.seek.minutes, "10");
    assert_eq!(rig.shell.ui.dialogs.online.seek.increment, "5");
    enter(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Seeking);
    // The window turns into the wait, and warns about the assistance.
    assert_eq!(active(&rig), Some(ActiveDialog::OnlineSeek));
    assert!(has(&rig, "Looking for an opponent..."));
    assert!(has(
        &rig,
        "When a game starts, engines and queries are disabled until it ends."
    ));
}

#[test]
fn escape_while_looking_withdraws_the_search() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Seek));
    enter(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Seeking);
    escape(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Idle);
    assert_eq!(active(&rig), None);
}

#[test]
fn the_cancel_button_of_the_wait_withdraws_the_search() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Seek));
    enter(&mut rig);
    let sent = click(&rig, "Stop looking");
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::Online(OnlineUiMessage::CancelWait)))
    );
}

#[test]
fn a_time_control_lichess_refuses_disables_the_button_and_says_why() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Seek));
    online(&mut rig, OnlineUiMessage::Preset(1, 0));
    assert!(has(
        &rig,
        "Lichess does not accept this time control for a search (rapid or slower)."
    ));
    assert!(click_opt(&rig, "Search").is_empty());
    enter(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Idle);
    // Letters are not numbers.
    online(&mut rig, OnlineUiMessage::Minutes("1a5".to_owned()));
    assert_eq!(rig.shell.ui.dialogs.online.seek.minutes, "15");
}

#[test]
fn the_seek_is_rated_only_when_the_user_asks() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Seek));
    assert!(!rig.shell.ui.dialogs.online.seek.rated, "casual by default");
    online(&mut rig, OnlineUiMessage::Rated(true));
    assert!(rig.shell.ui.dialogs.online.seek.rated);
}

// ---- challenge ----

#[test]
fn a_challenge_needs_a_valid_user_and_waits_for_the_answer() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Challenge));
    assert_eq!(active(&rig), Some(ActiveDialog::OnlineChallenge));
    online(&mut rig, OnlineUiMessage::ChallengeUser("a".to_owned()));
    enter(&mut rig);
    assert_eq!(
        rig.shell.app.online.phase,
        OnlinePhase::Idle,
        "one letter is not a user name"
    );
    online(&mut rig, OnlineUiMessage::ChallengeUser("bob".to_owned()));
    online(&mut rig, OnlineUiMessage::Color(OnlineColor::White));
    online(&mut rig, OnlineUiMessage::Preset(10, 5));
    enter(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Challenging);
    assert!(has(&rig, "Waiting for bob to answer..."));
    escape(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Idle);
}

#[test]
fn the_challenge_menu_item_is_off_without_the_scope() {
    use mychess_gui::ViewOptions;
    use mychess_gui::windows::menu_bar::{Label, MenuEntry, menu_specs};
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    rig.shell.app.online.account.as_mut().unwrap().can_challenge = false;
    rig.sync();
    let specs = menu_specs(&rig.shell.app, &rig.shell.ui, ViewOptions::default());
    let online = specs
        .iter()
        .find(|s| s.label_key == "menu-online")
        .expect("online menu");
    let enabled = online.entries.iter().find_map(|e| match e {
        MenuEntry::Item {
            label: Label::Key("menu-online-challenge"),
            action,
            ..
        } => Some(action.is_some()),
        _ => None,
    });
    assert_eq!(enabled, Some(false));
}

// ---- challenges received ----

fn challenge(id: &str, playable: bool) -> IncomingChallenge {
    IncomingChallenge {
        chess960: false,
        id: id.to_owned(),
        from: "carol".to_owned(),
        title: Some("IM".to_owned()),
        rating: Some(2100),
        rated: true,
        time: "10+5".to_owned(),
        playable,
        decline_reason: "generic",
    }
}

#[test]
fn the_list_of_challenges_accepts_and_declines() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    rig.shell.app.online.incoming = vec![challenge("c1", true)];
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Incoming));
    assert_eq!(active(&rig), Some(ActiveDialog::OnlineIncoming));
    assert!(has(&rig, "IM carol (2100): 10+5, rated"));
    let sent = click(&rig, "Accept");
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::Online(OnlineUiMessage::Accept(id)) if id == "c1"))
    );
    let sent = click(&rig, "Decline");
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::Online(OnlineUiMessage::Decline(id)) if id == "c1"))
    );
}

#[test]
fn a_challenge_the_app_cannot_play_only_offers_decline() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    rig.shell.app.online.incoming = vec![challenge("c2", false)];
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Incoming));
    assert!(has(
        &rig,
        "MyChess cannot play this one (variant or time control)."
    ));
    assert!(click_opt(&rig, "Accept").is_empty());
    assert!(!click_opt(&rig, "Decline").is_empty());
}

#[test]
fn an_empty_list_says_so_and_enter_closes_it() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Incoming));
    assert!(has(&rig, "No challenges."));
    enter(&mut rig);
    assert_eq!(active(&rig), None);
}

// ---- confirmations ----

#[test]
fn resigning_asks_first_and_enter_or_escape_keep_the_game() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    online(&mut rig, OnlineUiMessage::Resign);
    assert_eq!(active(&rig), Some(ActiveDialog::OnlineConfirm));
    assert_eq!(
        rig.shell.ui.dialogs.online.confirm,
        Some(OnlineConfirm::Resign)
    );
    assert!(has(&rig, "Do you want to resign this game?"));
    enter(&mut rig);
    assert_eq!(active(&rig), None, "Enter is the safe answer");
    online(&mut rig, OnlineUiMessage::Resign);
    escape(&mut rig);
    assert_eq!(active(&rig), None);
    assert!(rig.shell.app.online.phase.is_live());
}

#[test]
fn the_yes_button_sends_the_resignation() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    online(&mut rig, OnlineUiMessage::Resign);
    let sent = click(&rig, "Yes");
    assert!(
        sent.iter()
            .any(|m| matches!(m, Message::Online(OnlineUiMessage::Confirm(true))))
    );
    let out = rig
        .shell
        .ui
        .dialogs
        .online
        .update(OnlineUiMessage::Confirm(true), &rig.shell.app);
    assert!(out.messages.iter().any(|m| matches!(
        m,
        mychess_app::Message::Online(mychess_app::online::OnlineMessage::Resign)
    )));
}

#[test]
fn without_the_confirmation_preference_the_resignation_goes_at_once() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "e2e4 e7e5", Instant::now());
    let mut prefs = rig.shell.app.prefs.online.as_ref().clone();
    prefs.confirm_resign = false;
    *rig.shell.app.prefs.online = prefs;
    online(&mut rig, OnlineUiMessage::Resign);
    assert_eq!(active(&rig), None, "no question was asked");
}

#[test]
fn closing_the_window_during_a_game_asks_and_no_keeps_playing() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    rig.msg(Message::Ui(UiMessage::CloseRequested));
    assert_eq!(
        rig.shell.ui.dialogs.online.confirm,
        Some(OnlineConfirm::Quit)
    );
    assert!(has(&rig, "A game is in progress"));
    enter(&mut rig);
    assert_eq!(rig.shell.ui.dialogs.online.confirm, None);
    assert!(rig.shell.app.online.phase.is_live());
}

#[test]
fn removing_the_lock_asks_for_the_declaration() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    rig.shell.app.online.link = mychess_app::online::Link::Down;
    rig.sync();
    assert!(rig.shell.app.assist_locked());
    online(&mut rig, OnlineUiMessage::AskRemoveLock);
    assert!(has(
        &rig,
        "I understand that the game may continue on the server and that I must not use assistance in it."
    ));
    let sent = click(&rig, "Yes");
    assert!(!sent.is_empty());
}

#[test]
fn the_lock_cannot_be_removed_while_the_event_stream_is_up() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    assert!(!rig.shell.app.can_remove_lock());
    online(&mut rig, OnlineUiMessage::AskRemoveLock);
    // Even if the window were open, the connection coming back closes it.
    rig.shell.app.online.link = mychess_app::online::Link::Down;
    rig.sync();
    online(&mut rig, OnlineUiMessage::AskRemoveLock);
    assert_eq!(
        rig.shell.ui.dialogs.online.confirm,
        Some(OnlineConfirm::RemoveLock)
    );
    rig.shell.app.online.link = mychess_app::online::Link::Up;
    rig.sync();
    assert_eq!(rig.shell.ui.dialogs.online.confirm, None);
}

#[test]
fn keys_typed_during_an_online_game_are_ignored_without_a_remark() {
    let mut rig = Rig::new();
    online_common::start_game(&mut rig, "alice", "startpos", "", Instant::now());
    rig.shell.app.notice = None;
    let revision = rig.shell.app.session.revision();
    for c in ['e', '4', 'N', '!'] {
        rig.press(common::ch(c));
    }
    rig.press(common::named(NamedKey::Enter));
    rig.press(common::named(NamedKey::Backspace));
    assert_eq!(rig.shell.app.session.revision(), revision);
    assert_eq!(rig.shell.app.notice, None, "no 'locked' remark either");
}

#[test]
fn a_refused_connection_forgets_the_token_the_window_kept_for_a_retry() {
    let mut rig = Rig::new();
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    rig.msg(Message::Online(OnlineUiMessage::Token(Secret::new(TOKEN))));
    enter(&mut rig);
    assert!(rig.shell.ui.dialogs.online.staged);
    // The server refused the token: nothing is kept (D1485).
    rig.shell.app.online.phase = OnlinePhase::Disconnected;
    rig.shell.app.online.notice = Some(OnlineNotice::Error(OnlineError::Unauthorized));
    rig.sync();
    assert!(!rig.shell.ui.dialogs.online.staged);
}

#[test]
fn only_a_refused_place_keeps_the_token_and_closing_the_window_forgets_it() {
    let mut rig = Rig::new();
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    rig.msg(Message::Online(OnlineUiMessage::Token(Secret::new(TOKEN))));
    enter(&mut rig);
    rig.shell.app.online.phase = OnlinePhase::Disconnected;
    rig.shell.app.online.notice = Some(OnlineNotice::Error(OnlineError::TokenStore));
    rig.sync();
    assert!(
        rig.shell.ui.dialogs.online.staged,
        "the user may pick another place"
    );
    escape(&mut rig);
    assert!(!rig.shell.ui.dialogs.online.staged);
}

#[test]
fn the_token_store_preference_does_not_preselect_the_file() {
    let mut rig = Rig::new();
    rig.shell.app.prefs.online.token_store = mychess_app::online::TokenStorePref::File;
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Connect));
    assert_eq!(rig.shell.ui.dialogs.online.store, StoreChoice::Keyring);
}

// ---- Chess960 (M9-6) ----

#[test]
fn m9_6_the_seek_window_has_a_standard_chess960_selector() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Seek));
    assert!(has(&rig, "Variant"), "the selector is labelled");
    assert!(
        !rig.shell.ui.dialogs.online.seek.chess960,
        "standard by default"
    );
    online(&mut rig, OnlineUiMessage::Chess960(true));
    assert!(rig.shell.ui.dialogs.online.seek.chess960);
    enter(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Seeking);
    assert!(rig.shell.modal_layer_open());
}

#[test]
fn m9_6_the_challenge_window_takes_a_chess960_position_only_when_casual_and_in_range() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Challenge));
    assert!(!has(&rig, "Position (0-959)"), "hidden for standard chess");
    online(&mut rig, OnlineUiMessage::Chess960(true));
    assert!(has(&rig, "Position (0-959)"));
    online(&mut rig, OnlineUiMessage::ChallengeUser("bob".to_owned()));
    online(&mut rig, OnlineUiMessage::ChallengeStart("960".to_owned()));
    assert!(has(
        &rig,
        "Enter a position from 0 to 959 (empty: Lichess draws one). A fixed position needs a casual game."
    ));
    enter(&mut rig);
    assert_eq!(
        rig.shell.app.online.phase,
        OnlinePhase::Idle,
        "960 is out of range"
    );
    online(&mut rig, OnlineUiMessage::ChallengeStart("518".to_owned()));
    online(&mut rig, OnlineUiMessage::Rated(true));
    enter(&mut rig);
    assert_eq!(
        rig.shell.app.online.phase,
        OnlinePhase::Idle,
        "fixed and rated"
    );
    online(&mut rig, OnlineUiMessage::Rated(false));
    enter(&mut rig);
    assert_eq!(rig.shell.app.online.phase, OnlinePhase::Challenging);
}

#[test]
fn m9_6_an_incoming_chess960_challenge_is_tagged_and_can_be_accepted() {
    let mut rig = Rig::new();
    online_common::connect(&mut rig);
    let mut c = challenge("c9", true);
    c.chess960 = true;
    rig.shell.app.online.incoming = vec![c];
    online(&mut rig, OnlineUiMessage::Open(OnlineDialog::Incoming));
    assert!(has(&rig, "Chess960 variant."));
    assert!(!click_opt(&rig, "Accept").is_empty());
}

#[test]
fn m9_6_the_online_variant_texts_exist_in_both_languages() {
    for lang in ["en-US", "pt-BR"] {
        let text = std::fs::read_to_string(format!(
            "{}/../../assets/i18n/{lang}/online.ftl",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("ftl");
        for key in [
            "online-variant",
            "online-variant-standard",
            "online-variant-chess960",
            "online-challenge-start",
            "online-challenge-start-invalid",
            "online-incoming-chess960",
            "online-notice-rejected-bad-variant",
        ] {
            assert!(
                text.lines().any(|l| l.starts_with(&format!("{key} ="))),
                "{lang}: {key}"
            );
        }
    }
}
