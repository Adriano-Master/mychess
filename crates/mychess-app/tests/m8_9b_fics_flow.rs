//! M8-9b contract tests of the play on FICS (spec `m8-9-fics.md`, D2601,
//! D2613, D2622, D2624, D2628): the application against the fake server of
//! `mychess-online` (no real network). The state machine itself is covered
//! by the unit tests of `fics::state_tests`.
#![allow(
    clippy::large_futures,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty
)]

use std::collections::VecDeque;
use std::time::Duration;

use mychess_app::fics::{
    AutoPromote, ConnectRequest, FicsEvents, FicsMessage, FicsPrefs, Phase, SeekSpec,
};
use mychess_app::{
    AnnotateMessage, AppState, BaseSlot, ColorChoice, Effect, Message, SlotId, load_preferences,
    perform, render_preferences, update,
};
use mychess_core::Square;
use mychess_online::fics::testing::{FakeFics, FakeOptions};

const START: &str = "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 7 me bob 1 5 0 39 39 300 300 1 none (0:00) none 0";
const E4: &str = "<12> rnbqkbnr pppppppp -------- -------- ----P--- -------- PPPP-PPP RNBQKBNR B 4 1 1 1 1 0 7 me bob -1 5 0 39 39 297 300 1 P/e2-e4 (0:03) e4 0";
const E5: &str = "<12> rnbqkbnr pppp-ppp -------- ----p--- ----P--- -------- PPPP-PPP RNBQKBNR W 4 1 1 1 1 0 7 me bob 1 5 0 39 39 297 296 2 P/e7-e5 (0:04) e5 0";

fn sq(text: &str) -> Square {
    text.parse().expect("square")
}

struct Rig {
    state: AppState,
    events: FicsEvents,
    fake: FakeFics,
}

impl Rig {
    async fn new() -> Rig {
        let fake = FakeFics::start(FakeOptions::default()).await;
        let mut state = AppState::default();
        state.fics.hub.set_port_override(Some(fake.port()));
        let prefs = FicsPrefs {
            host: fake.host(),
            ..FicsPrefs::default()
        };
        update(
            &mut state,
            Message::Fics(FicsMessage::SetPrefs(Box::new(prefs))),
        );
        let events = state.fics.hub.events().expect("receiver");
        Rig {
            state,
            events,
            fake,
        }
    }

    async fn run(&mut self, effects: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            if effect.runs_on_host() {
                continue;
            }
            let answer = perform(effect).await;
            queue.extend(update(&mut self.state, answer));
        }
    }

    async fn send(&mut self, message: Message) {
        let effects = update(&mut self.state, message);
        self.run(effects).await;
    }

    async fn fics(&mut self, message: FicsMessage) {
        self.send(Message::Fics(message)).await;
    }

    async fn until(&mut self, what: &str, done: impl Fn(&AppState) -> bool) {
        for _ in 0..400 {
            if done(&self.state) {
                return;
            }
            if let Ok(Some(message)) =
                tokio::time::timeout(Duration::from_millis(25), self.events.next()).await
            {
                self.send(Message::Fics(message)).await;
            }
        }
        panic!("timed out waiting for: {what}");
    }

    async fn login(&mut self) {
        self.fics(FicsMessage::Connect(ConnectRequest { name: None }))
            .await;
        self.until("login", |s| s.fics().phase == Phase::LoggedIn)
            .await;
    }

    async fn start_game(&mut self) {
        self.login().await;
        self.fake.send_line(START);
        self.until("game", |s| s.fics().playing()).await;
    }

    fn clipbase_games(&self) -> u32 {
        self.state
            .bases
            .slot(SlotId::CLIPBASE)
            .map_or(0, BaseSlot::total)
    }
}

#[tokio::test]
async fn a_guest_logs_in_and_the_console_shows_the_server() {
    let mut rig = Rig::new().await;
    rig.login().await;
    assert_eq!(rig.state.fics().me.as_deref(), Some("GuestTEST"));
    assert!(!rig.state.assist_locked(), "no game, no lock");
    rig.fake.send_line("Welcome to the server");
    rig.until("console line", |s| {
        s.fics()
            .console
            .lines()
            .any(|l| l.text.contains("Welcome to the server"))
    })
    .await;
}

#[tokio::test]
async fn a_game_of_the_user_locks_the_assistance_and_the_result_frees_and_stores_it() {
    let mut rig = Rig::new().await;
    rig.start_game().await;
    assert!(rig.state.assist_locked(), "D2622");
    // The families of help are dropped by `update`.
    rig.send(Message::Annotate(AnnotateMessage::Open)).await;
    assert!(rig.state.annotate_dialog.is_none(), "D1330 family dropped");
    // The move of the user goes to the server as coordinates.
    rig.fics(FicsMessage::PlayerMove {
        from: sq("e2"),
        to: sq("e4"),
        promotion: None,
    })
    .await;
    assert!(
        rig.fake
            .wait_line(|l| l == "e2e4", Duration::from_secs(2))
            .await
            .is_some(),
        "{:?}",
        rig.fake.received()
    );
    rig.fake.send_line(E4);
    rig.fake.send_line(E5);
    rig.until("two plies", |s| {
        s.fics().game.as_ref().is_some_and(|g| g.plies() == 2)
    })
    .await;
    rig.fake.send_line("{Game 7 (me vs. bob) bob resigns} 1-0");
    rig.until("unlocked", |s| !s.assist_locked()).await;
    assert_eq!(rig.clipbase_games(), 1, "D2624: stored in the Clipbase");
}

#[tokio::test]
async fn an_unfinished_result_is_not_stored() {
    let mut rig = Rig::new().await;
    rig.start_game().await;
    rig.fake
        .send_line("{Game 7 (me vs. bob) Game aborted by mutual agreement} *");
    rig.until("unlocked", |s| !s.assist_locked()).await;
    assert_eq!(rig.clipbase_games(), 0, "D2607");
}

#[tokio::test]
async fn a_seek_goes_out_and_is_refused_during_a_game() {
    let mut rig = Rig::new().await;
    rig.login().await;
    let spec = SeekSpec {
        minutes: 3,
        increment: 2,
        rated: false,
        color: mychess_app::fics::ColorChoice::Any,
        manual: false,
        range: None,
    };
    rig.fics(FicsMessage::Seek(spec.clone())).await;
    assert!(
        rig.fake
            .wait_line(
                |l| l.starts_with("seek 3 2 unrated"),
                Duration::from_secs(2)
            )
            .await
            .is_some(),
        "{:?}",
        rig.fake.received()
    );
    // The settings follow the last seek (D2628).
    assert_eq!(rig.state.prefs.fics.minutes, 3);
    rig.fake.send_line(START);
    rig.until("game", |s| s.fics().playing()).await;
    let before = rig.fake.received().len();
    rig.fics(FicsMessage::Seek(spec)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(rig.fake.received().len(), before, "no seek during a game");
    let _ = ColorChoice::Random;
}

#[tokio::test]
async fn a_game_is_refused_while_a_game_against_the_engine_or_lichess_is_on() {
    // The Lichess phase is set directly: the rule is the same for every
    // other play (D2613, exclusion).
    let mut state = AppState::default();
    state
        .fics
        .apply(FicsMessage::Connect(ConnectRequest { name: None }));
    state.online.phase =
        mychess_app::online::OnlinePhase::Playing(mychess_app::online::PlayingState::Live);
    let effects = update(
        &mut state,
        Message::Fics(FicsMessage::Seek(SeekSpec {
            minutes: 5,
            increment: 0,
            rated: false,
            color: mychess_app::fics::ColorChoice::Any,
            manual: false,
            range: None,
        })),
    );
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Fics { .. })),
        "nothing is sent"
    );
}

#[test]
fn update_stays_pure_when_asked_to_connect() {
    let mut state = AppState::default();
    let effects = update(
        &mut state,
        Message::Fics(FicsMessage::Connect(ConnectRequest { name: None })),
    );
    assert!(matches!(effects.as_slice(), [Effect::Fics { .. }]));
    assert_eq!(state.fics().phase, Phase::Connecting);
}

#[test]
fn the_fics_preferences_round_trip_and_never_hold_a_password() {
    let mut prefs = mychess_app::Preferences::default();
    *prefs.fics = FicsPrefs {
        host: "fics.example.org".to_owned(),
        port: 5001,
        login: "alice".to_owned(),
        rated: true,
        minutes: 15,
        increment: 10,
        auto_promote: AutoPromote::Knight,
        premove: false,
        store_emt: false,
        store_clk: false,
        shouts: true,
        init_commands: vec!["set seek 1".to_owned()],
    };
    let state = AppState::default();
    state.fics.hub.stage_password("hunter2").expect("password");
    let text = render_preferences(&prefs);
    assert!(text.contains("[fics]"), "{text}");
    assert!(!text.contains("hunter2"), "D2602");
    assert!(!text.to_lowercase().contains("password"), "{text}");
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.fics, prefs.fics);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
    assert!(!format!("{state:?}{:?}", state.fics.hub).contains("hunter2"));
}

#[test]
fn a_wild_fics_table_is_sanitised_when_read() {
    let text = "version = 1\n[fics]\nhost = \"a b/c\"\nport = 99999\nlogin = \"../x\"\nminutes = 9999\nauto_promote = \"king\"\ninit_commands = [\"ok\\nquit\", \"set seek 1\"]\n";
    let loaded = load_preferences(text);
    let fics = &loaded.prefs.fics;
    assert_eq!(fics.host, "freechess.org");
    assert_eq!(fics.port, 65535);
    assert_eq!(fics.login, "");
    assert_eq!(fics.minutes, 600);
    assert_eq!(fics.auto_promote, AutoPromote::Ask);
    assert_eq!(fics.init_commands, vec!["set seek 1".to_owned()]);
}

#[test]
fn the_window_flag_follows_the_messages() {
    let mut state = AppState::default();
    update(&mut state, Message::Fics(FicsMessage::OpenWindow));
    assert!(state.fics().window_open);
    update(&mut state, Message::Fics(FicsMessage::CloseWindow));
    assert!(!state.fics().window_open);
}

#[tokio::test]
async fn a_game_on_fics_can_be_resigned_but_never_replayed() {
    // M8-12, D3101 and D3103: the FICS client has no `rematch`.
    let mut rig = Rig::new().await;
    assert_eq!(rig.state.resign_mode(), None);
    rig.start_game().await;
    assert!(rig.state.fics().playing());
    assert_eq!(rig.state.resign_mode(), Some(mychess_app::GameMode::Fics));
    assert_eq!(rig.state.rematch_mode(), None);
}
