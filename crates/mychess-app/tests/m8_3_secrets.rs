//! M8-3b contract tests of the preferences `[online]`, the token that never
//! leaks, and the purity of `update` (spec D1307, D1338, D1357, R3, and the
//! criteria "preferences with round-trip; token absent" and "`update` pure").
#![allow(
    clippy::large_futures, // the rig owns the whole `AppState` across awaits
    clippy::assert_is_empty,
    clippy::manual_assert_eq,
    clippy::unwrap_used,
    clippy::expect_used
)]

mod m8_3_common;

use m8_3_common::*;
use mychess_app::online::{
    ConnectRequest, DEFAULT_BASE_URL, OnlineCommand, OnlineHub, OnlineMessage, OnlineNotice,
    OnlinePrefs, OnlineState, SaveTo, StoreChoice, TokenSource, TokenStorePref,
    base_url_is_acceptable, login_is_valid,
};
use mychess_app::{
    AppNotice, AppState, Effect, Message, Preferences, load_preferences, render_preferences, update,
};
use std::fmt::Write as _;

use rstest::rstest;

// ---- preferences (D1338) ----

#[test]
fn the_defaults_are_the_ones_of_the_spec() {
    let prefs = OnlinePrefs::default();
    assert_eq!(prefs.login, "");
    assert!(!prefs.rated, "casual by default");
    assert!(prefs.premove, "premove on by default (P8)");
    assert!(prefs.confirm_resign);
    assert!(prefs.show_ping);
    assert_eq!(prefs.save_to, SaveTo::Clipbase, "P7");
    assert_eq!(prefs.base_url, "https://lichess.org");
    assert_eq!(DEFAULT_BASE_URL, "https://lichess.org");
    // A preferences file without the table gives the defaults.
    let loaded = load_preferences("version = 1\n");
    assert_eq!(*loaded.prefs.online, OnlinePrefs::default());
}

#[test]
fn the_online_table_round_trips_without_a_token() {
    let mut prefs = Preferences::default();
    *prefs.online = OnlinePrefs {
        login: "Bob_1".to_owned(),
        rated: true,
        time: 15,
        increment: 10,
        premove: false,
        confirm_resign: false,
        show_ping: false,
        save_to: SaveTo::Nowhere,
        token_store: TokenStorePref::File,
        base_url: "http://127.0.0.1:9".to_owned(),
    };
    let text = render_preferences(&prefs);
    assert!(text.contains("[online]"), "{text}");
    assert!(
        !text.to_lowercase().contains("token ="),
        "no token key: {text}"
    );
    assert!(!text.contains("tok_"));
    let loaded = load_preferences(&text);
    assert_eq!(*loaded.prefs.online, *prefs.online);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    // Render again: stable.
    assert_eq!(render_preferences(&loaded.prefs), text);
}

#[rstest]
#[case::path_in_login("login = \"../x\"", "")]
#[case::spaces_in_login("login = \"a b\"", "")]
#[case::long_login("login = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"", "")]
fn a_wild_login_in_the_file_is_dropped(#[case] line: &str, #[case] want: &str) {
    let loaded = load_preferences(&format!("version = 1\n[online]\n{line}\n"));
    assert_eq!(loaded.prefs.online.login, want);
}

#[test]
fn numbers_are_clamped_and_unsafe_addresses_are_replaced_when_read() {
    let text = "version = 1\n[online]\ntime = 99999\nincrement = 99999\nbase_url = \"http://evil.example\"\nsave_to = \"somewhere\"\n";
    let loaded = load_preferences(text);
    let online = &loaded.prefs.online;
    assert_eq!((online.time, online.increment), (180, 180));
    assert_eq!(
        online.base_url, DEFAULT_BASE_URL,
        "the token never goes in clear"
    );
    assert_eq!(online.save_to, SaveTo::Clipbase);
}

#[test]
fn a_garbage_online_table_does_not_break_the_other_preferences() {
    for text in [
        "version = 1\n[online]\ntime = \"ten\"\n",
        "version = 1\nonline = 3\n",
        "version = 1\n[online]\npremove = 7\nrated = \"yes\"\n",
    ] {
        let loaded = load_preferences(text);
        // Whatever it makes of them, the result is usable.
        let online = loaded.prefs.online.clone().sanitized();
        assert!(online.time <= 180, "{text}");
    }
}

#[rstest]
#[case("https://lichess.org", true)]
#[case("https://lichess.org/", true)]
#[case("https://example.org:8443", true)]
#[case("http://127.0.0.1:8080", true)]
#[case("http://localhost:3000", true)]
#[case("http://[::1]:9", true)]
#[case("http://lichess.org", false)]
#[case("http://example.com:80", false)]
#[case("http://127.0.0.1.evil.com", false)]
#[case("http://localhost.evil.com", false)]
#[case("http://127.0.0.1@evil.com", false)]
#[case("https://user:pw@lichess.org", false)]
#[case("https://lichess.org/?token=x", false)]
#[case("https://lichess.org/#x", false)]
#[case("ftp://127.0.0.1", false)]
#[case("file:///etc/passwd", false)]
#[case("lichess.org", false)]
#[case("https://", false)]
#[case("", false)]
#[case("https://a b", false)]
fn only_https_or_loopback_may_carry_the_token(#[case] url: &str, #[case] ok: bool) {
    assert_eq!(base_url_is_acceptable(url), ok, "{url:?}");
}

#[rstest]
#[case("Bob", true)]
#[case("a_b-c", true)]
#[case("ab", true)]
#[case("a", false)]
#[case("", false)]
#[case("../x", false)]
#[case("a b", false)]
#[case("é", false)]
#[case("a/b", false)]
#[case("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", false)]
fn user_names_have_the_shape_of_the_lichess(#[case] login: &str, #[case] ok: bool) {
    assert_eq!(login_is_valid(login), ok, "{login:?}");
}

#[tokio::test]
async fn the_last_login_is_remembered_in_the_preferences_file_without_the_token() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    rig.send(Message::FlushPreferences).await;
    let text = std::fs::read_to_string(rig.dir.path().join("preferences.toml")).expect("prefs");
    assert!(text.contains("Bob"), "{text}");
    assert!(!text.contains(SECRET), "{text}");
    assert!(!text.contains("tok_"), "{text}");
}

// ---- the token never leaks (D1307, D1357, R3) ----

#[test]
fn a_token_that_is_not_a_token_is_refused_without_echoing_it() {
    let hub = OnlineHub::default();
    for bad in [
        "",
        "has space",
        "with-dash",
        "ç",
        "tok\nen",
        &"a".repeat(5000),
    ] {
        let err = hub.stage_token(bad).expect_err("refused");
        let shown = format!("{err:?} {err:?}");
        assert!(
            bad.len() < 3 || !shown.contains(bad),
            "the error echoes the text: {shown}"
        );
    }
    assert!(hub.stage_token("abc_DEF_123").is_ok());
    assert!(
        hub.stage_token(&"a".repeat(512)).is_ok(),
        "512 characters are accepted"
    );
}

#[tokio::test]
async fn the_token_is_in_no_message_effect_state_or_file_of_a_whole_game() {
    let mut rig = Rig::new(1).await;
    rig.seek_and_start().await;
    rig.games[0].line(&full(&Full::default(), &state("e2e4")));
    rig.until("described", |s| s.online.game.is_some()).await;
    rig.move_ok("e7e5");
    rig.play("e7", "e5").await;
    rig.games[0].line(&finished("e2e4 e7e5", "resign", Some("white")));
    rig.until("unlocked", |s| !s.assist_locked()).await;
    rig.send(Message::FlushPreferences).await;
    let mut everything = rig.log.join("\n");
    for effect in &rig.seen {
        writeln!(everything, "{effect:?}").unwrap();
    }
    writeln!(everything, "{:?}", rig.state.online).unwrap();
    writeln!(everything, "{:?}", rig.state.online.hub).unwrap();
    writeln!(everything, "{:?}", rig.state.online.account).unwrap();
    writeln!(everything, "{:?}", rig.state.online.game).unwrap();
    writeln!(everything, "{:?}", rig.state.notice).unwrap();
    writeln!(everything, "{:?}", rig.state.prefs).unwrap();
    assert!(
        !everything.contains(SECRET),
        "the token appears in a Debug output"
    );
    assert!(!everything.contains("tok_SECRET"), "not even a piece of it");
    // Not on disk next to the preferences either.
    for entry in std::fs::read_dir(rig.dir.path()).expect("dir") {
        let path = entry.expect("entry").path();
        if path.is_file() {
            let bytes = std::fs::read(&path).expect("read");
            assert!(
                !String::from_utf8_lossy(&bytes).contains(SECRET),
                "{} holds the token",
                path.display()
            );
        }
    }
    // The server saw it only in the header: never in a path, a query or a body.
    for request in rig.fake.requests() {
        assert!(!request.path.contains(SECRET), "{}", request.path);
        assert!(!request.query.contains(SECRET), "{}", request.query);
        assert!(!request.body.contains(SECRET), "{}", request.path);
    }
    let with_header = rig
        .fake
        .requests()
        .iter()
        .filter(|r| r.header("authorization") == Some(format!("Bearer {SECRET}").as_str()))
        .count();
    assert!(with_header >= 3, "authenticated requests carry the header");
}

#[tokio::test]
async fn a_failed_connection_leaks_no_token_either() {
    let mut rig = Rig::build(Opts {
        account_status: 401,
        ..Opts::default()
    })
    .await;
    rig.online(OnlineMessage::Connect(ConnectRequest {
        source: TokenSource::Staged,
        store: StoreChoice::Keyring,
    }))
    .await;
    rig.until("failure", |s| s.notice.is_some()).await;
    let everything = format!("{:?} {:?} {:?}", rig.log, rig.seen, rig.state.notice);
    assert!(!everything.contains(SECRET));
    assert!(!matches!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::Connected))
    ));
}

#[tokio::test]
async fn a_staged_token_is_used_once_and_only_a_kept_one_survives() {
    // Session-only: nothing in the store afterwards.
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    assert!(
        mychess_online::TokenStore::load(rig.store.as_ref(), "Bob")
            .unwrap()
            .is_none(),
        "StoreChoice::Session keeps nothing"
    );
    // A kept one is in the store under the name of the account.
    let mut kept = Rig::new(1).await;
    kept.online(OnlineMessage::Connect(ConnectRequest {
        source: TokenSource::Staged,
        store: StoreChoice::Keyring,
    }))
    .await;
    kept.until("connected", |s| s.online.account.is_some())
        .await;
    assert!(
        mychess_online::TokenStore::load(kept.store.as_ref(), "Bob")
            .unwrap()
            .is_some()
    );
}

// ---- purity of update ----

fn plain_state_with_staged_token() -> AppState {
    let mut state = AppState::default();
    state.online = Box::new(OnlineState::new(OnlineHub::default()));
    state.online.hub.stage_token(SECRET).expect("token");
    state
}

#[test]
fn update_only_describes_the_work_without_a_runtime_or_a_server() {
    // A plain `#[test]`: no Tokio runtime, no server. Anything that touched
    // the network or the disk inside `update` would fail or panic here.
    let mut state = plain_state_with_staged_token();
    let effects = update(
        &mut state,
        Message::Online(OnlineMessage::Connect(ConnectRequest {
            source: TokenSource::Staged,
            store: StoreChoice::Keyring,
        })),
    );
    assert!(matches!(
        effects[..],
        [Effect::Online {
            command: OnlineCommand::Connect { .. },
            ..
        }]
    ));
    assert!(!format!("{effects:?}").contains(SECRET));
    // Every command of the user in this state is answered without effects of
    // the network (the session is "connecting", not connected).
    for message in [
        OnlineMessage::Seek(Rig::seek_request()),
        OnlineMessage::Resign,
        OnlineMessage::Abort,
        OnlineMessage::CancelSeek,
        OnlineMessage::CancelPremove,
        OnlineMessage::DismissResult,
        OnlineMessage::AcceptChallenge("chal0001".into()),
        OnlineMessage::DeclineChallenge("chal0001".into()),
    ] {
        let effects = update(&mut state, Message::Online(message.clone()));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::Online { .. })),
            "{message:?} -> {effects:?}"
        );
    }
}

#[test]
fn update_writes_nothing_to_the_disk() {
    let dir = tempfile::tempdir().expect("dir");
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    drop(update(
        &mut state,
        Message::Online(OnlineMessage::LoadMarker),
    ));
    drop(update(
        &mut state,
        Message::Online(OnlineMessage::RemoveLock { confirmed: true }),
    ));
    let entries: Vec<_> = std::fs::read_dir(dir.path()).expect("dir").collect();
    assert!(entries.is_empty(), "update created files: {entries:?}");
}

#[test]
fn quitting_with_a_session_asks_to_close_the_streams_and_nothing_else() {
    let mut state = plain_state_with_staged_token();
    // Disconnected: nothing to close.
    let effects = update(&mut state, Message::Quit);
    assert!(
        !effects.iter().any(|e| matches!(
            e,
            Effect::Online {
                command: OnlineCommand::Shutdown,
                ..
            }
        )),
        "a program that never connected has nothing to shut down"
    );
}

#[tokio::test]
async fn quitting_while_connected_shuts_the_streams_down() {
    let mut rig = Rig::new(1).await;
    rig.connect().await;
    let effects = rig.update_only(Message::Quit);
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::Online {
                command: OnlineCommand::Shutdown,
                ..
            }
        )),
        "{effects:?}"
    );
}
