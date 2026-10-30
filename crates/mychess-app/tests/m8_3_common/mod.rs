//! Shared helpers of the `m8_3_*` contract tests of the online play (spec
//! `docs/specs/m8-3-lichess-online.md`, phase M8-3b): a fake Lichess server on
//! `127.0.0.1`, an `AppState` wired to it and a driver that runs the effects
//! and feeds the hub events back to `update`, the way the front-end does.
//!
//! Only the public API of `mychess-app` (and the `testing` feature of
//! `mychess-online`) is used. No test touches the real network.
#![allow(
    dead_code,
    unused_imports,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mychess_app::SlotId;
use mychess_app::online::{
    ConnectRequest, Link, OnlineColor, OnlineCommand, OnlineEvents, OnlineHub, OnlineMessage,
    OnlinePhase, OnlineState, PlayingState, SeekRequest, StoreChoice, TokenSource,
};
use mychess_app::{AppNotice, AppState, Effect, Message, perform, update};
use mychess_core::{Color, Role, Square};
use mychess_online::testing::{FakeLichess, LiveStream, Response, Step};
use mychess_online::{ClientConfig, MemoryStore};

/// The id of the game of the fake server.
pub const ID: &str = "abcd1234";
/// A token that must never show up anywhere but in the `Authorization`
/// header.
pub const SECRET: &str = "tok_SECRET_9f3a";

pub const ACCOUNT: &str = r#"{"id":"bob","username":"Bob","perfs":{"rapid":{"rating":1500,"games":10},"blitz":{"rating":1400,"games":3,"prov":true}}}"#;

pub fn sq(text: &str) -> Square {
    text.parse().expect("square")
}

/// What the rig is built with.
pub struct Opts {
    /// Game streams the fake serves for `ID`, in order.
    pub games: usize,
    /// `X-OAuth-Scopes` of the account.
    pub scopes: &'static str,
    /// Body of `GET /api/account`.
    pub account: &'static str,
    /// Status of `GET /api/account`.
    pub account_status: u16,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            games: 1,
            scopes: "board:play, challenge:write",
            account: ACCOUNT,
            account_status: 200,
        }
    }
}

pub struct Rig {
    pub state: AppState,
    pub events: OnlineEvents,
    pub fake: FakeLichess,
    pub event_stream: LiveStream,
    pub games: Vec<LiveStream>,
    pub dir: tempfile::TempDir,
    pub store: Arc<MemoryStore>,
    /// Every effect that was run.
    pub seen: Vec<Effect>,
    /// `Debug` of every message given to `update` and every effect it
    /// returned.
    pub log: Vec<String>,
}

pub fn hub_with(store: Arc<MemoryStore>) -> OnlineHub {
    let tuning: Arc<dyn Fn(&mut ClientConfig) + Send + Sync> = Arc::new(|config| {
        config.streams.backoff_unit = Duration::from_millis(5);
    });
    OnlineHub::with_stores(store, None, Some(tuning))
}

impl Rig {
    pub async fn new(games: usize) -> Rig {
        Rig::build(Opts {
            games,
            ..Opts::default()
        })
        .await
    }

    pub async fn build(opts: Opts) -> Rig {
        let fake = FakeLichess::start().await.expect("fake server");
        let mut account =
            Response::json(opts.account_status, opts.account).header("X-OAuth-Scopes", opts.scopes);
        if opts.account_status != 200 {
            account = Response::json(opts.account_status, r#"{"error":"nope"}"#);
        }
        fake.fallback("GET", "/api/account", account);
        let (response, event_stream) = Response::live();
        fake.enqueue("GET", "/api/stream/event", response);
        let mut games = Vec::new();
        for _ in 0..opts.games {
            let (response, live) = Response::live();
            fake.enqueue("GET", &format!("/api/board/game/stream/{ID}"), response);
            games.push(live);
        }
        fake.fallback(
            "POST",
            "/api/board/seek",
            Response::stream(vec![Step::Hold]),
        );
        for action in [
            "resign",
            "abort",
            "draw/yes",
            "draw/no",
            "takeback/yes",
            "takeback/no",
            "claim-victory",
            "claim-draw",
        ] {
            fake.fallback(
                "POST",
                &format!("/api/board/game/{ID}/{action}"),
                Response::json(200, r#"{"ok":true}"#),
            );
        }
        let dir = tempfile::tempdir().expect("dir");
        let store = Arc::new(MemoryStore::new());
        let mut state = AppState::with_preferences_path(Some(dir.path().join("preferences.toml")));
        state.prefs.online.base_url = fake.base_url();
        state.online = Box::new(OnlineState::new(hub_with(store.clone())));
        let events = state.online.hub.events().expect("events");
        state.online.hub.stage_token(SECRET).expect("a valid token");
        Rig {
            state,
            events,
            fake,
            event_stream,
            games,
            dir,
            store,
            seen: Vec::new(),
            log: Vec::new(),
        }
    }

    /// Makes the server accept `uci` as a move of the game.
    pub fn move_ok(&self, uci: &str) {
        self.fake.fallback(
            "POST",
            &format!("/api/board/game/{ID}/move/{uci}"),
            Response::json(200, r#"{"ok":true}"#),
        );
    }

    /// Makes the server refuse `uci` (400).
    pub fn move_400(&self, uci: &str) {
        self.fake.fallback(
            "POST",
            &format!("/api/board/game/{ID}/move/{uci}"),
            Response::json(400, r#"{"error":"Not your turn"}"#),
        );
    }

    pub fn posts(&self, path: &str) -> usize {
        self.fake.requests_for("POST", path).len()
    }

    pub fn move_posts(&self, uci: &str) -> usize {
        self.posts(&format!("/api/board/game/{ID}/move/{uci}"))
    }

    pub fn action_posts(&self, action: &str) -> usize {
        self.posts(&format!("/api/board/game/{ID}/{action}"))
    }

    pub async fn run(&mut self, effects: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            if effect.runs_on_host() {
                continue;
            }
            self.seen.push(effect.clone());
            // The timers of the marker and of the retries (D1365, D1366)
            // are driven by hand.
            if matches!(
                effect,
                Effect::Online {
                    command: OnlineCommand::Grace { .. } | OnlineCommand::Retry { .. },
                    ..
                }
            ) {
                continue;
            }
            let answer = perform(effect).await;
            self.log.push(format!("{answer:?}"));
            let more = update(&mut self.state, answer);
            self.log.push(format!("{more:?}"));
            queue.extend(more);
        }
    }

    /// `update` only: the effects are returned, not run.
    pub fn update_only(&mut self, message: Message) -> Vec<Effect> {
        self.log.push(format!("{message:?}"));
        let effects = update(&mut self.state, message);
        self.log.push(format!("{effects:?}"));
        effects
    }

    pub async fn send(&mut self, message: Message) {
        let effects = self.update_only(message);
        self.run(effects).await;
    }

    pub async fn online(&mut self, message: OnlineMessage) {
        self.send(Message::Online(message)).await;
    }

    pub async fn until(&mut self, what: &str, done: impl Fn(&AppState) -> bool) {
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
        panic!(
            "timed out waiting for: {what} (notice {:?}, phase {:?})",
            self.state.notice, self.state.online.phase
        );
    }

    /// Handles whatever the hub says for `ms` milliseconds (to see that
    /// something does NOT happen).
    pub async fn pump(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if let Ok(Some(message)) =
                tokio::time::timeout(Duration::from_millis(20), self.events.next()).await
            {
                self.send(message).await;
            }
        }
    }

    /// Handles events until the fake saw `n` requests for the stream of the
    /// game (the reopening is decided by `update`, so the events must be
    /// pumped while waiting).
    pub async fn wait_game_streams(&mut self, n: usize) {
        let path = format!("/api/board/game/stream/{ID}");
        for _ in 0..400 {
            if self.fake.requests_for("GET", &path).len() >= n {
                return;
            }
            self.pump(25).await;
        }
        panic!("the stream of the game was not requested {n} times");
    }

    /// Handles events until the fake saw `n` POSTs to `path`.
    pub async fn wait_post(&mut self, path: &str, n: usize) {
        for _ in 0..400 {
            if self.posts(path) >= n {
                return;
            }
            self.pump(25).await;
        }
        panic!("{path} was not posted {n} times");
    }

    pub async fn connect(&mut self) {
        self.online(OnlineMessage::Connect(ConnectRequest {
            source: TokenSource::Staged,
            store: StoreChoice::Session,
        }))
        .await;
        self.until("connected", |s| {
            s.online.phase == OnlinePhase::Idle && s.online.link == Link::Up
        })
        .await;
    }

    pub fn seek_request() -> SeekRequest {
        SeekRequest {
            variant: mychess_app::online::OnlineVariant::Standard,
            minutes: 10,
            increment: 0,
            rated: false,
            color: OnlineColor::Random,
            rating_range: None,
        }
    }

    /// Connects, seeks and answers with `gameStart`; stops when the app
    /// has opened the stream of the game.
    pub async fn seek_and_start(&mut self) {
        self.connect().await;
        self.online(OnlineMessage::Seek(Rig::seek_request())).await;
        assert!(self.fake.wait_requests("POST", "/api/board/seek", 1).await);
        self.game_start();
        self.until("game started", |s| {
            s.online.phase == OnlinePhase::Playing(PlayingState::Live)
        })
        .await;
        assert!(
            self.fake
                .wait_requests("GET", &format!("/api/board/game/stream/{ID}"), 1)
                .await
        );
    }

    pub fn game_start(&self) {
        self.event_stream.line(&format!(
            r#"{{"type":"gameStart","game":{{"gameId":"{ID}","fullId":"{ID}abcd","color":"black"}}}}"#
        ));
    }

    /// Seek, `gameStart` and the `gameFull` `full` on the first game stream.
    pub async fn start_with(&mut self, full: &str) {
        self.seek_and_start().await;
        self.games[0].line(full);
        self.until("game described", |s| s.online.game.is_some())
            .await;
    }

    /// A game as Black with no moves.
    pub async fn start_game(&mut self) {
        let text = full(&Full::default(), &state(""));
        self.start_with(&text).await;
    }

    pub fn mainline_count(&self) -> usize {
        self.state.session.game().mainline().count()
    }

    pub async fn play(&mut self, from: &str, to: &str) {
        self.online(player_move(from, to, None, false)).await;
    }

    pub fn lock(&self) -> bool {
        self.state.assist_locked()
    }

    pub fn clipbase_games(&self) -> u32 {
        clipbase_games(&self.state)
    }

    pub fn notice(&self) -> Option<&AppNotice> {
        self.state.notice.as_ref()
    }
}

pub fn clipbase_games(state: &AppState) -> u32 {
    state
        .bases
        .slot(SlotId::CLIPBASE)
        .map_or(0, mychess_app::BaseSlot::total)
}

pub fn player_move(
    from: &str,
    to: &str,
    promotion: Option<Role>,
    offer_draw: bool,
) -> OnlineMessage {
    OnlineMessage::PlayerMove {
        from: sq(from),
        to: sq(to),
        promotion,
        offer_draw,
        at: Instant::now(),
    }
}

// ---- JSON of the fake server ----

/// A `gameState` line. Times in milliseconds.
pub fn state_with(moves: &str, wtime: u64, btime: u64, status: &str, extra: &str) -> String {
    format!(
        r#"{{"type":"gameState","moves":"{moves}","wtime":{wtime},"btime":{btime},"winc":0,"binc":0,"status":"{status}"{extra}}}"#
    )
}

/// A `gameState` of a running game with plenty of time.
pub fn state(moves: &str) -> String {
    state_with(moves, 598_000, 600_000, "started", "")
}

pub fn finished(moves: &str, status: &str, winner: Option<&str>) -> String {
    let extra = winner.map_or(String::new(), |w| format!(r#","winner":"{w}""#));
    state_with(moves, 598_000, 597_000, status, &extra)
}

/// The inner object of a `gameState` (no `type`), for `gameFull.state`.
fn state_object(line: &str) -> String {
    line.replacen(r#""type":"gameState","#, "", 1)
}

/// How the `gameFull` describes the game.
#[derive(Clone)]
pub struct Full {
    pub me_white: bool,
    pub fen: Option<String>,
    pub variant: String,
    pub rated: bool,
    pub speed: String,
    pub clock: Option<(u64, u64)>,
    pub white_title: Option<String>,
    pub created_at: u64,
}

impl Default for Full {
    fn default() -> Full {
        Full {
            me_white: false,
            fen: None,
            variant: "standard".to_owned(),
            rated: false,
            speed: "rapid".to_owned(),
            clock: Some((600_000, 0)),
            white_title: None,
            created_at: 1_700_000_000_000,
        }
    }
}

impl Full {
    pub fn white() -> Full {
        Full {
            me_white: true,
            ..Full::default()
        }
    }

    pub fn from_fen(fen: &str) -> Full {
        Full {
            fen: Some(fen.to_owned()),
            ..Full::default()
        }
    }
}

/// A `gameFull` line with `state_line` (a `gameState` line) inside.
pub fn full(opts: &Full, state_line: &str) -> String {
    let (white, black) = if opts.me_white {
        (("bob", "Bob", 1500), ("alice", "alice", 1800))
    } else {
        (("alice", "alice", 1800), ("bob", "Bob", 1500))
    };
    let title = |is_white: bool| {
        if is_white {
            opts.white_title
                .as_ref()
                .map_or(String::new(), |t| format!(r#","title":"{t}""#))
        } else {
            String::new()
        }
    };
    let clock = opts.clock.map_or(String::new(), |(i, inc)| {
        format!(r#","clock":{{"initial":{i},"increment":{inc}}}"#)
    });
    let fen = opts.fen.as_deref().unwrap_or("startpos");
    format!(
        r#"{{"type":"gameFull","id":"{ID}","variant":{{"key":"{variant}","name":"{variant}"}},"speed":"{speed}","perf":{{"name":"{speed}"}},"rated":{rated},"createdAt":{created}{clock},"white":{{"id":"{wid}","name":"{wname}","rating":{wr}{wt}}},"black":{{"id":"{bid}","name":"{bname}","rating":{br}{bt}}},"initialFen":"{fen}","state":{state}}}"#,
        variant = opts.variant,
        speed = opts.speed,
        rated = opts.rated,
        created = opts.created_at,
        wid = white.0,
        wname = white.1,
        wr = white.2,
        wt = title(true),
        bid = black.0,
        bname = black.1,
        br = black.2,
        bt = title(false),
        state = state_object(state_line),
    )
}

/// An incoming challenge event.
pub fn challenge_event(
    id: &str,
    challenger: &str,
    title: Option<&str>,
    variant: &str,
    limit: u32,
    inc: u32,
) -> String {
    let title = title.map_or(String::new(), |t| format!(r#","title":"{t}""#));
    let show = format!("{}+{}", limit / 60, inc);
    format!(
        r#"{{"type":"challenge","challenge":{{"id":"{id}","status":"created","challenger":{{"id":"{lower}","name":"{challenger}","rating":1800{title}}},"destUser":{{"id":"bob","name":"Bob","rating":1500}},"variant":{{"key":"{variant}","name":"{variant}"}},"rated":false,"speed":"rapid","timeControl":{{"type":"clock","limit":{limit},"increment":{inc},"show":"{show}"}},"color":"random","finalColor":"white","direction":"in"}}}}"#,
        lower = challenger.to_lowercase(),
        show = show,
    )
}
