//! The online play inside `AppState`, against the fake Lichess (spec M8-3,
//! phase b): the whole path from `update` through `perform` and the streams
//! and back, with no real network.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use mychess_engine::testing::ScriptedEngine;
use mychess_online::testing::{FakeLichess, LiveStream, Response, Step};
use mychess_online::{ClientConfig, MemoryStore};

use crate::bases::SlotId;
use crate::effect::{Effect, perform};
use crate::engines::{EngineDraft, EngineHub, EngineMessage, StartMode};
use crate::flow::AppNotice;
use crate::message::Message;
use crate::online::{
    ConnectRequest, OnlineColor, OnlineCommand, OnlineEvents, OnlineHub, OnlineMessage,
    OnlineNotice, OnlinePhase, OnlineState, PlayingState, SeekRequest, StoreChoice, TokenSource,
    marker_path,
};
use crate::session::{GameMessage, Nav, PlayOptions};
use crate::shortcuts::Action;
use crate::state::{AppState, update};

const ID: &str = "abcd1234";
const ACCOUNT: &str =
    r#"{"id":"bob","username":"Bob","perfs":{"rapid":{"rating":1500,"games":10}}}"#;

struct Rig {
    state: AppState,
    events: OnlineEvents,
    fake: FakeLichess,
    event_stream: LiveStream,
    game_streams: Vec<LiveStream>,
    dir: tempfile::TempDir,
    seen: Vec<Effect>,
}

fn hub() -> OnlineHub {
    let tuning: Arc<dyn Fn(&mut ClientConfig) + Send + Sync> = Arc::new(|config| {
        config.streams.backoff_unit = Duration::from_millis(5);
    });
    OnlineHub::with_stores(Arc::new(MemoryStore::new()), None, Some(tuning))
}

async fn rig_with_games(game_streams: usize) -> Rig {
    let fake = FakeLichess::start().await.expect("fake server");
    fake.fallback(
        "GET",
        "/api/account",
        Response::json(200, ACCOUNT).header("X-OAuth-Scopes", "board:play, challenge:write"),
    );
    let (response, event_stream) = Response::live();
    fake.enqueue("GET", "/api/stream/event", response);
    let mut streams = Vec::new();
    for _ in 0..game_streams {
        let (response, live) = Response::live();
        fake.enqueue("GET", &format!("/api/board/game/stream/{ID}"), response);
        streams.push(live);
    }
    fake.fallback(
        "POST",
        "/api/board/seek",
        Response::stream(vec![Step::Hold]),
    );
    fake.fallback(
        "POST",
        &format!("/api/board/game/{ID}/move/e7e5"),
        Response::json(200, r#"{"ok":true}"#),
    );
    let dir = tempfile::tempdir().expect("dir");
    let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
    state.prefs.online.base_url = fake.base_url();
    state.online = Box::new(OnlineState::new(hub()));
    let events = state.online.hub.events().expect("events");
    state
        .online
        .hub
        .stage_token("tok_123")
        .expect("a valid token");
    Rig {
        state,
        events,
        fake,
        event_stream,
        game_streams: streams,
        dir,
        seen: Vec::new(),
    }
}

impl Rig {
    async fn run(&mut self, effects: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            if effect.runs_on_host() {
                continue;
            }
            self.seen.push(effect.clone());
            // The timer of the marker is driven by hand.
            if matches!(
                effect,
                Effect::Online {
                    command: OnlineCommand::Grace { .. },
                    ..
                }
            ) {
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

    async fn online(&mut self, message: OnlineMessage) {
        self.send(Message::Online(message)).await;
    }

    async fn until(&mut self, what: &str, done: impl Fn(&AppState) -> bool) {
        for _ in 0..400 {
            if done(&self.state) {
                return;
            }
            if let Ok(Some(message)) =
                tokio::time::timeout(Duration::from_millis(25), self.events.next()).await
            {
                self.send(message).await;
            }
        }
        panic!("timed out waiting for: {what}");
    }

    async fn connect(&mut self) {
        self.online(OnlineMessage::Connect(ConnectRequest {
            source: TokenSource::Staged,
            store: StoreChoice::Session,
        }))
        .await;
        self.until("connected", |s| {
            s.online.phase == OnlinePhase::Idle && s.online.link == crate::online::Link::Up
        })
        .await;
    }

    /// Seek, `gameStart` and `gameFull` of a game as Black.
    async fn start_game(&mut self) {
        self.connect().await;
        self.online(OnlineMessage::Seek(SeekRequest {
            variant: crate::online::OnlineVariant::Standard,
            minutes: 10,
            increment: 0,
            rated: false,
            color: OnlineColor::Random,
            rating_range: None,
        }))
        .await;
        assert!(self.fake.wait_requests("POST", "/api/board/seek", 1).await);
        self.event_stream.line(&format!(
            r#"{{"type":"gameStart","game":{{"gameId":"{ID}","color":"black"}}}}"#
        ));
        self.until("game started", rig_is_live_again).await;
        self.game_streams[0].line(&full(""));
        self.until("game described", |s| s.online.game.is_some())
            .await;
    }

    fn effects_of_kind(&self, f: impl Fn(&Effect) -> bool) -> usize {
        self.seen.iter().filter(|e| f(e)).count()
    }
}

fn state_json(moves: &str, status: &str, extra: &str) -> String {
    format!(
        r#"{{"type":"gameState","moves":"{moves}","wtime":598000,"btime":600000,"winc":0,"binc":0,"status":"{status}"{extra}}}"#
    )
}

fn full(moves: &str) -> String {
    format!(
        r#"{{"type":"gameFull","id":"{ID}","variant":{{"key":"standard","name":"Standard"}},"speed":"rapid","perf":{{"name":"Rapid"}},"rated":false,"createdAt":1700000000000,"white":{{"id":"alice","name":"alice","rating":1800}},"black":{{"id":"bob","name":"Bob","rating":1500}},"initialFen":"startpos","clock":{{"initial":600000,"increment":0}},"state":{}}}"#,
        state_json(moves, "started", "")
    )
}

fn sq(text: &str) -> mychess_core::Square {
    text.parse().expect("square")
}

fn clipbase_games(state: &AppState) -> u32 {
    state
        .bases
        .slot(SlotId::CLIPBASE)
        .map_or(0, crate::bases::BaseSlot::total)
}

// ---- the whole game ----

#[tokio::test]
async fn a_game_from_the_seek_to_the_import() {
    let mut rig = rig_with_games(1).await;
    rig.start_game().await;
    assert!(rig.state.assist_locked());
    assert!(
        rig.state.board_flipped,
        "Black sees the board from his side"
    );
    assert!(
        marker_path(rig.dir.path()).exists(),
        "the marker of the game in progress is written"
    );
    assert_eq!(rig.state.session.game().mainline().count(), 0);

    // A premove made on White's turn goes out when White has moved.
    rig.online(OnlineMessage::PlayerMove {
        from: sq("e7"),
        to: sq("e5"),
        promotion: None,
        offer_draw: false,
        at: std::time::Instant::now(),
    })
    .await;
    assert!(rig.state.online.premove.is_set());
    rig.game_streams[0].line(&state_json("e2e4", "started", ""));
    rig.until("move shown", |s| s.session.game().mainline().count() == 2)
        .await;
    let path = format!("/api/board/game/{ID}/move/e7e5");
    assert!(rig.fake.wait_requests("POST", &path, 1).await);

    // The server confirms; the game ends by resignation.
    rig.game_streams[0].line(&state_json("e2e4 e7e5", "started", ""));
    rig.until("latency measured", |s| s.online.ping.is_some())
        .await;
    rig.game_streams[0].line(&state_json("e2e4 e7e5", "resign", r#","winner":"black""#));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    assert!(
        !rig.state.assist_locked(),
        "the lock is released at the end"
    );
    let marker = marker_path(rig.dir.path());
    rig.until("marker removed", |_| !marker.exists()).await;
    assert_eq!(clipbase_games(&rig.state), 1, "the game is stored");
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::Imported))
    );
    let tags = rig.state.session.game().tags().clone();
    assert_eq!(tags.site, format!("https://lichess.org/{ID}"));
    assert_eq!(tags.black, "Bob");
}

#[tokio::test]
async fn an_aborted_game_is_not_stored() {
    let mut rig = rig_with_games(1).await;
    rig.start_game().await;
    rig.game_streams[0].line(&state_json("", "aborted", ""));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    assert_eq!(clipbase_games(&rig.state), 0);
    assert_eq!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::NotImported))
    );
}

#[tokio::test]
async fn the_game_survives_a_stream_that_drops_and_does_not_duplicate_a_move() {
    let mut rig = rig_with_games(2).await;
    rig.start_game().await;
    rig.game_streams[0].line(&state_json("e2e4", "started", ""));
    rig.until("e4 shown", |s| s.session.game().mainline().count() == 1)
        .await;
    rig.game_streams[0].drop_connection();
    rig.until("reconnected", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Reconnecting) || rig_is_live_again(s)
    })
    .await;
    assert!(
        rig.state.assist_locked(),
        "the lock holds while reconnecting"
    );
    // The new stream starts with a full description: the state reconciles.
    assert!(
        rig.fake
            .wait_requests("GET", &format!("/api/board/game/stream/{ID}"), 2)
            .await
    );
    rig.game_streams[1].line(&full("e2e4 e7e5"));
    rig.until("reconciled", |s| {
        s.session.game().mainline().count() == 2
            && s.online.phase == OnlinePhase::Playing(PlayingState::Live)
    })
    .await;
    assert_eq!(
        rig.fake
            .requests_for("POST", &format!("/api/board/game/{ID}/move/e7e5"))
            .len(),
        0,
        "no move was sent by the reconnection"
    );
}

fn is_play(effect: &Effect) -> bool {
    matches!(effect, Effect::Sound(crate::sound::SoundEffect::Play(_)))
}

// ---- the sounds (spec M8-11, D4400-D4401) ----

#[tokio::test]
async fn an_online_game_plays_the_start_each_new_move_and_the_end_but_not_a_resync() {
    let mut rig = rig_with_games(2).await;
    rig.state.prefs.sound.enabled = true;
    rig.start_game().await;
    assert_eq!(rig.effects_of_kind(is_play), 1, "the start of the game");

    // The opponent moves: one sound.
    rig.game_streams[0].line(&state_json("e2e4", "started", ""));
    rig.until("e4 shown", |s| s.session.game().mainline().count() == 1)
        .await;
    assert_eq!(rig.effects_of_kind(is_play), 2, "the new move");

    // The same state again (a confirmation) is silent.
    rig.game_streams[0].line(&state_json("e2e4", "started", ""));
    rig.game_streams[0].drop_connection();
    assert!(
        rig.fake
            .wait_requests("GET", &format!("/api/board/game/stream/{ID}"), 2)
            .await
    );
    // The resynchronised `gameFull` has the same moves: silent.
    rig.game_streams[1].line(&full("e2e4"));
    rig.until("resynced", rig_is_live_again).await;
    assert_eq!(rig.effects_of_kind(is_play), 2, "no sound on a resync");

    // The end by resignation, no move with it: the end sound.
    rig.game_streams[1].line(&state_json("e2e4", "resign", r#","winner":"black""#));
    rig.until("finished", |s| {
        s.online.phase == OnlinePhase::Playing(PlayingState::Finished)
    })
    .await;
    assert_eq!(rig.effects_of_kind(is_play), 3, "the end of the game");
}

#[tokio::test]
async fn an_online_game_is_silent_with_the_sounds_off() {
    let mut rig = rig_with_games(1).await;
    rig.start_game().await;
    rig.game_streams[0].line(&state_json("e2e4", "started", ""));
    rig.until("e4 shown", |s| s.session.game().mainline().count() == 1)
        .await;
    assert_eq!(rig.effects_of_kind(is_play), 0);
}

fn rig_is_live_again(state: &AppState) -> bool {
    state.online.phase == OnlinePhase::Playing(PlayingState::Live)
}

// ---- the lock ----

#[tokio::test]
async fn a_game_start_stops_the_engines_and_the_lock_drops_what_asks_for_help() {
    let mut rig = rig_with_games(1).await;
    let fake_engine = ScriptedEngine::builder().build();
    rig.state.engine_hub = EngineHub::with_launcher(Arc::new(move |config| {
        let fake = fake_engine.clone();
        Box::pin(async move { fake.start(config).await })
    }));
    let id = rig
        .state
        .engines
        .add(EngineDraft::new("Fake", "/fake/engine"))
        .expect("engine");
    rig.send(Message::Engine(EngineMessage::Start(
        id,
        StartMode::Analyze,
    )))
    .await;
    assert!(!rig.state.engine_runs().is_empty());
    rig.start_game().await;
    assert!(
        rig.effects_of_kind(|e| matches!(e, Effect::StopEngine { .. })) >= 1,
        "the engine was stopped at the game start"
    );

    // Everything that asks for help is dropped, with a remark, in `update`.
    let moves_before = rig.state.session.game().mainline().count();
    for message in [
        Message::Engine(EngineMessage::Start(id, StartMode::Analyze)),
        Message::Action(Action::ToggleEngineAnalysis),
        Message::Action(Action::ToggleTreePane),
        Message::Action(Action::ToggleEvalBar),
        Message::Game(GameMessage::PlaySquares {
            from: sq("e2"),
            to: sq("e4"),
            promotion: None,
            options: PlayOptions::default(),
        }),
    ] {
        rig.state.notice = None;
        let effects = update(&mut rig.state, message.clone());
        assert!(effects.is_empty(), "{message:?}");
        assert_eq!(
            rig.state.notice,
            Some(AppNotice::Online(OnlineNotice::AssistanceDisabled)),
            "{message:?}"
        );
    }
    assert_eq!(rig.state.session.game().mainline().count(), moves_before);
    // Navigating the moves already played stays allowed.
    rig.state.notice = None;
    drop(update(
        &mut rig.state,
        Message::Game(GameMessage::Navigate(Nav::Start)),
    ));
    assert_eq!(rig.state.notice, None);
    // The name of the opening is hidden while locked.
    assert!(rig.state.opening().is_none());
}

#[tokio::test]
async fn the_lock_is_released_when_the_game_ends_and_asks_work_again() {
    let mut rig = rig_with_games(1).await;
    rig.start_game().await;
    rig.game_streams[0].line(&state_json("e2e4", "draw", ""));
    rig.until("finished", |s| !s.assist_locked()).await;
    let effects = update(&mut rig.state, Message::Action(Action::ToggleEvalBar));
    // The message is no longer dropped by the lock (it may do nothing else).
    assert_ne!(
        rig.state.notice,
        Some(AppNotice::Online(OnlineNotice::AssistanceDisabled)),
        "{effects:?}"
    );
}

#[tokio::test]
async fn a_marker_on_disk_starts_the_program_locked_and_the_grace_period_clears_a_stale_one() {
    let mut rig = rig_with_games(1).await;
    std::fs::write(
        marker_path(rig.dir.path()),
        format!("game = \"{ID}\"\ndate = \"2026.10.24\"\n"),
    )
    .expect("marker");
    rig.online(OnlineMessage::LoadMarker).await;
    assert!(rig.state.assist_locked(), "locked before anything connects");
    let before = update(
        &mut rig.state,
        Message::Action(Action::ToggleEngineAnalysis),
    );
    assert!(before.is_empty());
    // Connecting arms the grace timer; the event stream shows no game.
    rig.connect().await;
    let generation = rig
        .seen
        .iter()
        .rev()
        .find_map(|e| match e {
            Effect::Online {
                command: OnlineCommand::Grace { generation, .. },
                ..
            } => Some(*generation),
            _ => None,
        })
        .expect("a grace timer was asked for");
    rig.online(OnlineMessage::MarkerGrace(generation)).await;
    assert!(!rig.state.assist_locked());
    let marker = marker_path(rig.dir.path());
    rig.until("marker file removed", |_| !marker.exists()).await;
}

// ---- the previous game and the preferences ----

#[tokio::test]
async fn the_game_of_the_editor_is_parked_and_comes_back_only_when_asked() {
    let mut rig = rig_with_games(1).await;
    // A game of the user in the editor.
    drop(update(
        &mut rig.state,
        Message::Game(GameMessage::PlaySquares {
            from: sq("d2"),
            to: sq("d4"),
            promotion: None,
            options: PlayOptions::default(),
        }),
    ));
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    rig.start_game().await;
    assert_eq!(rig.state.session.game().mainline().count(), 0);
    assert!(!rig.state.online_has_parked_game(), "not while it is on");
    rig.game_streams[0].line(&state_json("e2e4", "draw", ""));
    rig.until("finished", |s| !s.assist_locked()).await;
    assert!(rig.state.online_has_parked_game());
    rig.online(OnlineMessage::RestorePrevious).await;
    assert_eq!(rig.state.session.game().mainline().count(), 1);
    assert!(!rig.state.board_flipped);
}

#[test]
fn the_online_preferences_round_trip_and_never_hold_a_token() {
    use crate::online::{SaveTo, TokenStorePref};
    use crate::{load_preferences, render_preferences};
    let mut prefs = crate::Preferences::default();
    *prefs.online = crate::online::OnlinePrefs {
        login: "Bob".to_owned(),
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
    assert!(!text.contains("tok_"), "no token is ever written");
    let loaded = load_preferences(&text);
    assert_eq!(loaded.prefs.online, prefs.online);
    assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
}

#[test]
fn a_wild_online_table_is_sanitised_when_read() {
    let text = "version = 1\n[online]\nlogin = \"../x\"\ntime = 99999\nbase_url = \"http://evil.example\"\nsave_to = \"somewhere\"\n";
    let loaded = crate::load_preferences(text);
    let online = &loaded.prefs.online;
    assert_eq!(online.login, "");
    assert_eq!(online.time, 180);
    assert_eq!(online.base_url, crate::online::DEFAULT_BASE_URL);
    assert_eq!(online.save_to, crate::online::SaveTo::Clipbase);
}

#[test]
fn update_stays_pure_when_the_online_play_is_asked_to_do_something() {
    // A connection request only describes work: no network, no disk.
    let mut state = AppState::default();
    state.online.hub.stage_token("tok_123").expect("token");
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
    let text = format!("{effects:?}");
    assert!(!text.contains("tok_123"), "the token is in no effect");
}
