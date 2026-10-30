//! Regression tests of the review of the M8-9 (spec `m8-9-fics.md`, D2690-D2698):
//! the application against the fake server of `mychess-online` (no real
//! network). The state machine itself is covered by `fics::state_tests`.
#![allow(
    clippy::large_futures,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty
)]

use std::collections::VecDeque;
use std::time::Duration;

use mychess_app::engines::{EngineDraft, EngineId, EngineMessage, StartMode};
use mychess_app::fics::{ConnectRequest, FicsEvents, FicsMessage, FicsPrefs, Phase};
use mychess_app::{
    AppState, BaseSlot, ColorChoice, Effect, Message, PlayConfig, PlayMessage, SlotId, TimeMode,
    perform, update,
};
use mychess_core::Square;
use mychess_engine::{Capabilities, EngineIdentity};
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
    seen: Vec<Effect>,
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
            seen: Vec::new(),
        }
    }

    async fn run(&mut self, effects: Vec<Effect>) {
        self.seen.extend(effects.iter().cloned());
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            if effect.runs_on_host() {
                continue;
            }
            let answer = perform(effect).await;
            let more = update(&mut self.state, answer);
            self.seen.extend(more.iter().cloned());
            queue.extend(more);
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

fn ready_engine(state: &mut AppState) -> EngineId {
    let engine = state
        .engines
        .add(EngineDraft::new("Fake", "/fake"))
        .expect("add engine");
    let effects = update(
        state,
        Message::Engine(EngineMessage::Start(engine, StartMode::Analyze)),
    );
    let run = effects
        .iter()
        .find_map(|e| match e {
            Effect::StartEngine { run, .. } => Some(*run),
            _ => None,
        })
        .expect("StartEngine");
    update(
        state,
        Message::Engine(EngineMessage::Started {
            run,
            identity: Box::new(EngineIdentity {
                name: "Fake".to_owned(),
                author: None,
                options: Vec::new(),
                capabilities: Capabilities::default(),
            }),
            at: 1_700_000_000,
        }),
    );
    engine
}

const OBSERVED: &str = "<12> rnbqkbnr pppppppp -------- -------- -------- -------- PPPPPPPP RNBQKBNR W -1 1 1 1 1 0 4 ann zed 0 5 0 39 39 300 300 1 none (0:00) none 0";

#[tokio::test]
async fn a_new_game_with_the_number_of_a_finished_one_locks_stops_and_stores_again() {
    let mut rig = Rig::new().await;
    rig.start_game().await;
    rig.fake.send_line("{Game 7 (me vs. bob) bob resigns} 1-0");
    rig.until("unlocked", |s| !s.assist_locked()).await;
    assert_eq!(rig.clipbase_games(), 1);
    // An analysis is running between the games.
    ready_engine(&mut rig.state);
    assert!(!rig.state.engine_runs().is_empty());

    // The same number, the same players: a new game.
    rig.fake.send_line(START);
    rig.until("locked again", AppState::assist_locked).await;
    assert!(rig.state.engine_runs().is_empty(), "the engines stopped");
    assert!(
        rig.seen
            .iter()
            .any(|e| matches!(e, Effect::StopEngine { .. })),
        "the stop was asked"
    );
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
            .is_some()
    );
    rig.fake.send_line(E4);
    rig.fake.send_line(E5);
    rig.until("two plies", |s| {
        s.fics().game.as_ref().is_some_and(|g| g.plies() == 2)
    })
    .await;
    rig.fake.send_line("{Game 7 (me vs. bob) me resigns} 0-1");
    rig.until("unlocked", |s| !s.assist_locked()).await;
    assert_eq!(rig.clipbase_games(), 2, "the second game is the one stored");
}

#[tokio::test]
async fn show_is_refused_while_another_play_holds_the_editor() {
    let mut rig = Rig::new().await;
    rig.login().await;
    rig.fake.send_line(OBSERVED);
    rig.until("observed", |s| s.fics().observed.len() == 1)
        .await;
    let engine = ready_engine(&mut rig.state);
    let mut config = PlayConfig::new(engine);
    config.color = ColorChoice::White;
    config.mode = TimeMode::Depth(1);
    update(
        &mut rig.state,
        Message::Play(PlayMessage::Start(Box::new(config))),
    );
    assert!(rig.state.play_live());
    let before = rig.state.session.generation();
    rig.fics(FicsMessage::Show(4)).await;
    assert_eq!(rig.state.fics().shown, None, "refused");
    assert_eq!(rig.state.session.generation(), before, "editor untouched");
    assert_eq!(
        rig.state.notice,
        Some(mychess_app::AppNotice::Fics(
            mychess_app::fics::FicsNotice::WrongState
        ))
    );
}

#[tokio::test]
async fn show_works_without_another_play() {
    let mut rig = Rig::new().await;
    rig.login().await;
    rig.fake.send_line(OBSERVED);
    rig.until("observed", |s| s.fics().observed.len() == 1)
        .await;
    rig.fics(FicsMessage::Show(4)).await;
    assert_eq!(rig.state.fics().shown, Some(4));
}

#[tokio::test]
async fn a_refused_connect_does_not_keep_the_password() {
    let mut rig = Rig::new().await;
    rig.state.fics.hub.stage_password("secret").expect("stage");
    assert!(rig.state.fics.hub.has_staged_password());
    rig.fics(FicsMessage::Connect(ConnectRequest {
        name: Some("a".into()),
    }))
    .await;
    assert!(!rig.state.fics.hub.has_staged_password());
    assert_eq!(rig.state.fics().phase, Phase::Disconnected);
}

#[tokio::test]
async fn disconnect_then_connect_at_once_keeps_the_new_connection() {
    let mut rig = Rig::new().await;
    rig.login().await;
    let disconnect = update(&mut rig.state, Message::Fics(FicsMessage::Disconnect));
    let connect = update(
        &mut rig.state,
        Message::Fics(FicsMessage::Connect(ConnectRequest { name: None })),
    );
    let pick = |effects: Vec<Effect>| {
        effects
            .into_iter()
            .find(|e| matches!(e, Effect::Fics { .. }))
            .expect("a FICS effect")
    };
    let (a, b) = tokio::join!(perform(pick(disconnect)), perform(pick(connect)));
    for answer in [a, b] {
        let more = update(&mut rig.state, answer);
        rig.run(more).await;
    }
    rig.until("logged in again", |s| s.fics().phase == Phase::LoggedIn)
        .await;
    // The old closing command must not take the new connection down.
    tokio::time::sleep(Duration::from_millis(400)).await;
    for _ in 0..8 {
        if let Ok(Some(message)) =
            tokio::time::timeout(Duration::from_millis(25), rig.events.next()).await
        {
            rig.send(Message::Fics(message)).await;
        }
    }
    assert_eq!(rig.state.fics().phase, Phase::LoggedIn);
}
