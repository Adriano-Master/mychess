//! HTTP client for the Lichess Board API (spec m8-3, D1301, D1305, D1308).
//!
//! * `Authorization: Bearer <token>` only in the header, never in the URL, in
//!   `Debug` or in errors.
//! * `https` only (plain `http` is accepted for loopback, for the tests); no
//!   redirects are followed.
//! * Non-stream requests go through the [`RequestGate`]; game actions are
//!   urgent and retried (D1308).
//! * Bodies are bounded; JSON errors map to [`Error`] variants.

use std::{
    net::IpAddr,
    sync::{Arc, Once},
    time::Duration,
};

use reqwest::{Method, RequestBuilder, Response, StatusCode, Url, redirect::Policy};

pub use crate::gate::{GateConfig, GatePermit, Priority, RequestGate};
use crate::{
    error::{Error, Result},
    models::{Account, ChallengeParams, SeekParams},
    streams::{
        ChallengeHandle, EventStream, GameStream, SeekHandle, StreamConfig, WatchStream,
        spawn_challenge, spawn_event_stream, spawn_game_stream, spawn_seek, spawn_watch_stream,
    },
    token::Token,
};

/// Default server.
pub const DEFAULT_BASE_URL: &str = "https://lichess.org";
/// Limit for ordinary response bodies (1 MiB).
pub const MAX_BODY: usize = 1 << 20;
/// Limit for the exported PGN of a game (4 MiB, D1308).
pub const MAX_PGN_BODY: usize = 4 << 20;

/// Configuration of a [`Client`].
pub struct ClientConfig {
    /// Server root. `https` only, or `http` on loopback.
    pub base_url: String,
    /// `User-Agent` header (`MyChess/<version>`).
    pub user_agent: String,
    /// Personal access token; `None` for the public endpoints.
    pub token: Option<Token>,
    /// Connection timeout (10 s).
    pub connect_timeout: Duration,
    /// Total timeout of a non-stream request (30 s).
    pub request_timeout: Duration,
    /// Body size limit of a non-stream request.
    pub max_body: usize,
    /// 429 policy.
    pub gate: GateConfig,
    /// Attempts for a game action in 429, 5xx or network errors (3).
    pub action_attempts: u32,
    /// Pause between those attempts (500 ms).
    pub action_retry_delay: Duration,
    /// Streams: silence watch, reconnection.
    pub streams: StreamConfig,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: format!("MyChess/{}", env!("CARGO_PKG_VERSION")),
            token: None,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            max_body: MAX_BODY,
            gate: GateConfig::default(),
            action_attempts: 3,
            action_retry_delay: Duration::from_millis(500),
            streams: StreamConfig::default(),
        }
    }
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field("base_url", &self.base_url)
            .field("user_agent", &self.user_agent)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

/// What `GET /api/account` returned plus the granted OAuth scopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountInfo {
    /// The account.
    pub account: Account,
    /// Scopes from `X-OAuth-Scopes`; `None` when the header is absent.
    pub scopes: Option<Vec<String>>,
}

impl AccountInfo {
    fn has(&self, scope: &str) -> bool {
        self.scopes
            .as_ref()
            .is_none_or(|s| s.iter().any(|x| x == scope))
    }

    /// Whether the token can play (`board:play`; unknown scopes count as yes).
    #[must_use]
    pub fn can_play(&self) -> bool {
        self.has("board:play")
    }

    /// Whether the token can send and answer challenges (`challenge:write`).
    #[must_use]
    pub fn can_challenge(&self) -> bool {
        self.has("challenge:write")
    }
}

/// A game action (all urgent, D1308).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GameAction<'a> {
    /// A move in UCI, optionally offering a draw.
    Move {
        /// UCI text (`e2e4`, `e7e8q`).
        uci: &'a str,
        /// Offer a draw with the move.
        offering_draw: bool,
    },
    /// Resign.
    Resign,
    /// Abort.
    Abort,
    /// Offer or accept a draw (`true`), or decline (`false`).
    Draw(bool),
    /// Accept (`true`) or decline (`false`) a takeback proposal.
    Takeback(bool),
    /// Claim victory over an absent opponent.
    ClaimVictory,
    /// Claim a draw over an absent opponent.
    ClaimDraw,
}

pub(crate) struct Inner {
    pub(crate) http: reqwest::Client,
    pub(crate) base: Url,
    token: Option<Token>,
    pub(crate) gate: RequestGate,
    cfg_request_timeout: Duration,
    cfg_max_body: usize,
    action_attempts: u32,
    action_retry_delay: Duration,
    pub(crate) streams: StreamConfig,
}

/// Lichess client. Cheap to clone (shares the connection pool and the gate).
#[derive(Clone)]
pub struct Client {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.inner.base.as_str())
            .field("authenticated", &self.inner.token.is_some())
            .finish_non_exhaustive()
    }
}

static CRYPTO: Once = Once::new();

/// Installs the `ring` provider for rustls (the workspace avoids `aws-lc-rs`,
/// see the root `Cargo.toml`). An already installed provider is kept.
fn install_crypto_provider() {
    CRYPTO.call_once(|| {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
    });
}

pub(crate) fn validate_base(text: &str) -> Result<Url> {
    let url = Url::parse(text).map_err(|_| Error::InsecureUrl)?;
    if url.cannot_be_a_base()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host().is_none()
        || !matches!(url.path(), "" | "/")
    {
        return Err(Error::InsecureUrl);
    }
    let host = url.host_str().unwrap_or_default();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    match url.scheme() {
        "https" => Ok(url),
        "http" if loopback => Ok(url),
        _ => Err(Error::InsecureUrl),
    }
}

/// `^[A-Za-z0-9]{8,12}$` (game and challenge ids).
fn valid_game_id(id: &str) -> bool {
    (8..=12).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The 8-character game id in what the user typed: a bare id (8 or 12
/// characters, the extra 4 are the player id) or a link such as
/// `https://lichess.org/abcd1234`, `lichess.org/abcd1234/black` or
/// `https://www.lichess.org/abcd1234wxyz#12`. Anything else is `None`.
#[must_use]
pub fn parse_game_ref(input: &str) -> Option<String> {
    let text = input.trim();
    let path = if text.contains('/') || text.contains('.') {
        let no_scheme = text
            .strip_prefix("https://")
            .or_else(|| text.strip_prefix("http://"))
            .unwrap_or(text);
        let (host, rest) = no_scheme.split_once('/')?;
        let host = host.to_ascii_lowercase();
        if host != "lichess.org" && !host.ends_with(".lichess.org") {
            return None;
        }
        rest
    } else {
        text
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    let first = segments.next()?;
    // A game id is exactly 8 characters, or 12 with the player id: the
    // 9..=11 of the challenge ids are not games, and the pages of the site
    // (`training`, `tournament`, `analysis`, ...) are not either.
    if !valid_game_id(first) || !matches!(first.len(), 8 | 12) || is_site_route(first) {
        return None;
    }
    // After the id only the colour of the point of view may follow.
    if segments.any(|s| !matches!(s, "white" | "black")) {
        return None;
    }
    Some(first.chars().take(8).collect())
}

/// The first path segments of lichess.org that are 8 or 12 letters long and
/// could pass for a game id.
fn is_site_route(segment: &str) -> bool {
    matches!(
        segment,
        "training"
            | "analysis"
            | "practice"
            | "settings"
            | "streamer"
            | "password"
            | "features"
            | "variants"
            | "racer"
            | "coordinate"
            | "broadcast"
            | "tournament"
            | "openings"
            | "insights"
    )
}

/// `^[A-Za-z0-9_-]{2,30}$`.
fn valid_username(name: &str) -> bool {
    (2..=30).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `e2e4` or `e7e8q`, lowercase.
fn valid_uci(uci: &str) -> bool {
    let b = uci.as_bytes();
    matches!(b.len(), 4 | 5)
        && matches!(b[0], b'a'..=b'h')
        && matches!(b[1], b'1'..=b'8')
        && matches!(b[2], b'a'..=b'h')
        && matches!(b[3], b'1'..=b'8')
        && (b.len() == 4 || matches!(b[4], b'q' | b'r' | b'b' | b'n'))
}

pub(crate) fn map_reqwest(e: &reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Timeout
    } else {
        Error::Network(describe(e))
    }
}

fn describe(e: &reqwest::Error) -> String {
    if e.is_connect() {
        "connection failed".to_owned()
    } else if e.is_body() || e.is_decode() {
        "stream interrupted".to_owned()
    } else if e.is_request() {
        "request failed".to_owned()
    } else {
        "i/o error".to_owned()
    }
}

fn retry_after(resp: &Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Extracts the `error` field of a JSON body, clipped.
fn server_reason(body: &[u8]) -> String {
    #[derive(serde::Deserialize)]
    struct E {
        #[serde(default)]
        error: serde_json::Value,
    }
    let Ok(e) = serde_json::from_slice::<E>(body) else {
        return String::new();
    };
    let text = match e.error {
        serde_json::Value::String(s) => s,
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    };
    text.chars().take(200).collect()
}

impl Client {
    /// Builds the client.
    ///
    /// # Errors
    /// [`Error::InsecureUrl`] for a base URL that is not `https` (or loopback
    /// `http`); [`Error::Network`] if the TLS stack cannot start.
    pub fn new(config: ClientConfig) -> Result<Self> {
        install_crypto_provider();
        let base = validate_base(&config.base_url)?;
        let http = reqwest::Client::builder()
            .user_agent(config.user_agent)
            .redirect(Policy::none())
            .connect_timeout(config.connect_timeout)
            .build()
            .map_err(|e| Error::Network(describe(&e)))?;
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                base,
                token: config.token,
                gate: RequestGate::new(config.gate),
                cfg_request_timeout: config.request_timeout,
                cfg_max_body: config.max_body,
                action_attempts: config.action_attempts.max(1),
                action_retry_delay: config.action_retry_delay,
                streams: config.streams,
            }),
        })
    }

    /// The request gate (shared with the streams).
    #[must_use]
    pub fn gate(&self) -> &RequestGate {
        &self.inner.gate
    }

    /// The server root this client talks to (with a trailing `/`).
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base.as_str()
    }

    /// Whether the client carries a token.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.inner.token.is_some()
    }

    /// `base` plus the path segments (percent-encoded by `url`). The
    /// configured base URL must not have a path (anything but `/` is refused
    /// by `Client::new`).
    pub(crate) fn url(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.inner.base.clone();
        url.path_segments_mut()
            .map_err(|()| Error::InsecureUrl)?
            .clear()
            .extend(segments);
        Ok(url)
    }

    pub(crate) fn request(&self, method: Method, url: Url) -> RequestBuilder {
        let rb = self.inner.http.request(method, url);
        match &self.inner.token {
            Some(token) => rb.bearer_auth(token.expose()),
            None => rb,
        }
    }

    /// Sends a non-stream request through the gate and reads the bounded body.
    async fn call(&self, rb: RequestBuilder, priority: Priority) -> Result<Reply> {
        self.call_limited(rb, priority, self.inner.cfg_max_body)
            .await
    }

    /// [`Client::call`] with its own body limit (the exported PGN, D3000).
    async fn call_limited(
        &self,
        rb: RequestBuilder,
        priority: Priority,
        max_body: usize,
    ) -> Result<Reply> {
        let _slot = self.inner.gate.acquire(priority).await?;
        let resp = rb
            .timeout(self.inner.cfg_request_timeout)
            .send()
            .await
            .map_err(|e| map_reqwest(&e))?;
        let status = resp.status();
        let ra = retry_after(&resp);
        let block = self.inner.gate.record_status(status.as_u16(), ra);
        let scopes = resp
            .headers()
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = read_body(resp, max_body).await?;
        if status.is_success() {
            Ok(Reply { body, scopes })
        } else {
            Err(status_error(status, block, &body))
        }
    }

    /// `GET /api/account`: the account and the granted scopes.
    ///
    /// # Errors
    /// [`Error::Unauthorized`] for a bad token, [`Error::BotAccount`] for a Bot
    /// account (D1301), plus the usual network errors.
    pub async fn account(&self) -> Result<AccountInfo> {
        let rb = self.request(Method::GET, self.url(&["api", "account"])?);
        let reply = self.call(rb, Priority::Normal).await?;
        let account: Account = serde_json::from_slice(&reply.body)
            .map_err(|_| Error::Protocol("account is not valid JSON".to_owned()))?;
        if account.is_bot() {
            return Err(Error::BotAccount);
        }
        let scopes = reply.scopes.map(|h| {
            h.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        });
        Ok(AccountInfo { account, scopes })
    }

    /// [`Client::account`] plus the scope check of D1305: without `board:play`
    /// the login is refused.
    ///
    /// # Errors
    /// [`Error::ScopeMissing`] when the token cannot play.
    pub async fn validate_login(&self) -> Result<AccountInfo> {
        let info = self.account().await?;
        if info.can_play() {
            Ok(info)
        } else {
            Err(Error::ScopeMissing {
                scope: "board:play".to_owned(),
            })
        }
    }

    /// Whether a user exists (`GET /api/user/{name}`).
    ///
    /// # Errors
    /// Network errors; a 404 is `Ok(false)`.
    pub async fn user_exists(&self, username: &str) -> Result<bool> {
        if !valid_username(username) {
            return Err(Error::InvalidArgument { what: "user name" });
        }
        let mut url = self.url(&["api", "user", username])?;
        url.query_pairs_mut().append_pair("profile", "false");
        match self
            .call(self.request(Method::GET, url), Priority::Normal)
            .await
        {
            Ok(_) => Ok(true),
            Err(Error::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// `GET` of a public resource, **without the token** even when the client
    /// has one: `base + segments + query`, through the request gate (one at a
    /// time, 429 block), with the request timeout and the body limit of the
    /// configuration. Used by `mychess-tablebase` (spec m8-8, ADR 0012) with
    /// a client of its own whose base URL is the tablebase host.
    ///
    /// # Errors
    /// The usual status and network errors ([`Error::NotFound`] for 404,
    /// [`Error::RateLimited`] for 429, [`Error::LineTooLong`] for a body over
    /// the limit, ...).
    pub async fn get_public(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Vec<u8>> {
        let mut url = self.url(segments)?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let rb = self.inner.http.request(Method::GET, url);
        Ok(self.call(rb, Priority::Normal).await?.body)
    }

    /// Performs a game action with up to `action_attempts` tries (D1308).
    /// Drop the future to stop retrying (for instance when the game stream
    /// already shows the effect).
    ///
    /// Retries are not idempotent: a timeout or a dropped connection does not
    /// tell whether the server applied the action. Hence a 400 on the *first*
    /// attempt is a plain [`Error::Rejected`] (the action was refused), but a
    /// 400 after a retry becomes [`Error::AmbiguousAfterRetry`]: the earlier
    /// attempt may have been applied (the retry then fails with "Not your
    /// turn"), so the app must reconcile with `gameState`/`gameFull` and keep
    /// the local move. A 429 is also resent after `action_retry_delay`
    /// (500 ms) even though it opens the gate block, because urgent actions
    /// ignore that block (D1308).
    ///
    /// # Errors
    /// The last error; a 400 ([`Error::Rejected`]) is never retried.
    pub async fn game_action(&self, game_id: &str, action: GameAction<'_>) -> Result<()> {
        if !valid_game_id(game_id) {
            return Err(Error::InvalidArgument { what: "game id" });
        }
        let mut url;
        match action {
            GameAction::Move { uci, offering_draw } => {
                if !valid_uci(uci) {
                    return Err(Error::InvalidArgument { what: "move" });
                }
                url = self.url(&["api", "board", "game", game_id, "move", uci])?;
                if offering_draw {
                    url.query_pairs_mut().append_pair("offeringDraw", "true");
                }
            }
            GameAction::Resign => url = self.url(&["api", "board", "game", game_id, "resign"])?,
            GameAction::Abort => url = self.url(&["api", "board", "game", game_id, "abort"])?,
            GameAction::Draw(yes) => {
                let a = if yes { "yes" } else { "no" };
                url = self.url(&["api", "board", "game", game_id, "draw", a])?;
            }
            GameAction::Takeback(yes) => {
                let a = if yes { "yes" } else { "no" };
                url = self.url(&["api", "board", "game", game_id, "takeback", a])?;
            }
            GameAction::ClaimVictory => {
                url = self.url(&["api", "board", "game", game_id, "claim-victory"])?;
            }
            GameAction::ClaimDraw => {
                url = self.url(&["api", "board", "game", game_id, "claim-draw"])?;
            }
        }
        let mut attempt = 1;
        loop {
            let rb = self.request(Method::POST, url.clone());
            match self.call(rb, Priority::Urgent).await {
                Ok(_) => return Ok(()),
                Err(e) if e.is_transient() && attempt < self.inner.action_attempts => {
                    attempt += 1;
                    tokio::time::sleep(self.inner.action_retry_delay).await;
                }
                Err(Error::Rejected {
                    status: 400,
                    reason,
                }) if attempt > 1 => {
                    return Err(Error::AmbiguousAfterRetry {
                        status: 400,
                        reason,
                    });
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Shortcut for a move.
    ///
    /// # Errors
    /// See [`Client::game_action`].
    pub async fn play_move(&self, game_id: &str, uci: &str, offering_draw: bool) -> Result<()> {
        self.game_action(game_id, GameAction::Move { uci, offering_draw })
            .await
    }

    async fn challenge_action(&self, id: &str, action: &str, reason: Option<&str>) -> Result<()> {
        if !valid_game_id(id) {
            return Err(Error::InvalidArgument {
                what: "challenge id",
            });
        }
        let url = self.url(&["api", "challenge", id, action])?;
        let mut rb = self.request(Method::POST, url);
        if let Some(reason) = reason {
            rb = rb.form(&[("reason", reason)]);
        }
        self.call(rb, Priority::Normal).await.map(|_| ())
    }

    /// Accepts a received challenge.
    ///
    /// # Errors
    /// Usual request errors.
    pub async fn accept_challenge(&self, id: &str) -> Result<()> {
        self.challenge_action(id, "accept", None).await
    }

    /// Declines a received challenge; `reason` is a Lichess key such as
    /// `generic`, `later`, `variant` or `noBot` (D1313).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a malformed reason, plus usual errors.
    pub async fn decline_challenge(&self, id: &str, reason: &str) -> Result<()> {
        if reason.is_empty() || !reason.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(Error::InvalidArgument { what: "reason" });
        }
        self.challenge_action(id, "decline", Some(reason)).await
    }

    /// Cancels a challenge the user sent.
    ///
    /// # Errors
    /// Usual request errors.
    pub async fn cancel_challenge(&self, id: &str) -> Result<()> {
        self.challenge_action(id, "cancel", None).await
    }

    /// Opens the account event stream (one per token, D1308).
    ///
    /// # Errors
    /// [`Error::Closed`] outside a Tokio runtime.
    pub fn open_event_stream(&self) -> Result<EventStream> {
        spawn_event_stream(self)
    }

    /// Opens the Board API stream of a game.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad id; [`Error::Closed`] outside a runtime.
    pub fn open_game_stream(&self, game_id: &str) -> Result<GameStream> {
        if !valid_game_id(game_id) {
            return Err(Error::InvalidArgument { what: "game id" });
        }
        spawn_game_stream(self, game_id)
    }

    /// Opens the public stream of a game being played by others
    /// (`/api/stream/game/{id}`), **without the token** (D3000). Ongoing games
    /// arrive delayed by 3 moves.
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad id; [`Error::Closed`] outside a runtime.
    pub fn open_watch_stream(&self, game_id: &str) -> Result<WatchStream> {
        if !valid_game_id(game_id) {
            return Err(Error::InvalidArgument { what: "game id" });
        }
        spawn_watch_stream(self, game_id)
    }

    /// Downloads the official PGN of a game (`/game/export/{id}`, public, no
    /// token) with clocks, for completing an observed game (D3000). Bounded
    /// to [`MAX_PGN_BODY`].
    ///
    /// # Errors
    /// [`Error::InvalidArgument`], [`Error::NotFound`] and the usual request
    /// errors; [`Error::LineTooLong`] over the limit.
    pub async fn export_pgn(&self, game_id: &str) -> Result<Vec<u8>> {
        if !valid_game_id(game_id) {
            return Err(Error::InvalidArgument { what: "game id" });
        }
        let mut url = self.url(&["game", "export", game_id])?;
        url.query_pairs_mut()
            .append_pair("clocks", "true")
            .append_pair("evals", "false")
            .append_pair("literate", "false");
        let rb = self.inner.http.request(Method::GET, url);
        Ok(self
            .call_limited(rb, Priority::Normal, MAX_PGN_BODY)
            .await?
            .body)
    }

    /// Exchanges an OAuth authorization code for an access token
    /// (`POST /api/token`, PKCE, no client secret; D3005). The client must be
    /// token-less. Neither the code, the verifier nor the answer are logged
    /// or put in errors.
    ///
    /// # Errors
    /// [`Error::Rejected`] when the server refuses the code,
    /// [`Error::Protocol`] for an answer without a usable token, plus the
    /// usual request errors.
    pub async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
        client_id: &str,
    ) -> Result<Token> {
        let rb = self
            .request(Method::POST, self.url(&["api", "token"])?)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", verifier),
                ("redirect_uri", redirect_uri),
                ("client_id", client_id),
            ]);
        let reply = self.call(rb, Priority::Normal).await?;
        let value: serde_json::Value = serde_json::from_slice(&reply.body)
            .map_err(|_| Error::Protocol("token answer is not valid JSON".to_owned()))?;
        let text = value
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::Protocol("token answer has no access token".to_owned()))?;
        Token::new(text).map_err(|_| Error::Protocol("token answer has a bad token".to_owned()))
    }

    /// Opens a real-time seek (the connection stays open, D1315).
    ///
    /// # Errors
    /// [`Error::TimeControl`] for a time control the API refuses;
    /// [`Error::RateLimited`] during a 429 block.
    pub fn seek(&self, params: &SeekParams) -> Result<SeekHandle> {
        let fields = params.form_fields()?;
        self.inner.gate.check_not_blocked()?;
        spawn_seek(self, &fields, self.inner.streams.header_timeout)
    }

    /// Sends a challenge and keeps its stream open (D1316).
    ///
    /// # Errors
    /// [`Error::InvalidArgument`] for a bad user name; [`Error::TimeControl`];
    /// [`Error::RateLimited`] during a 429 block.
    pub fn challenge(&self, username: &str, params: &ChallengeParams) -> Result<ChallengeHandle> {
        if !valid_username(username) {
            return Err(Error::InvalidArgument { what: "user name" });
        }
        let fields = params.form_fields()?;
        self.inner.gate.check_not_blocked()?;
        spawn_challenge(self, username, fields)
    }

    /// Sends a request that opens a stream: no semaphore, no total timeout,
    /// and the status is mapped to an error. With `respect_block` the 429
    /// block of the gate is honored first; the game stream passes `false`
    /// because it is urgent (D1308) and only obeys its own 429.
    pub(crate) async fn open_stream(
        &self,
        rb: RequestBuilder,
        header_timeout: Option<Duration>,
        respect_block: bool,
    ) -> Result<Response> {
        if respect_block {
            self.inner.gate.check_not_blocked()?;
        }
        let send = rb.send();
        let resp = match header_timeout {
            Some(t) => tokio::time::timeout(t, send)
                .await
                .map_err(|_| Error::Timeout)?,
            None => send.await,
        }
        .map_err(|e| map_reqwest(&e))?;
        let status = resp.status();
        let block = self
            .inner
            .gate
            .record_status(status.as_u16(), retry_after(&resp));
        if status.is_success() {
            Ok(resp)
        } else {
            let body = read_body(resp, 64 * 1024).await.unwrap_or_default();
            Err(status_error(status, block, &body))
        }
    }
}

struct Reply {
    body: Vec<u8>,
    scopes: Option<String>,
}

/// Reads a body up to `max` bytes.
async fn read_body(mut resp: Response, max: usize) -> Result<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err(Error::LineTooLong);
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| map_reqwest(&e))? {
        if out.len() + chunk.len() > max {
            return Err(Error::LineTooLong);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn status_error(status: StatusCode, block: Option<Duration>, body: &[u8]) -> Error {
    match status.as_u16() {
        401 => Error::Unauthorized,
        403 => Error::ScopeMissing {
            scope: "unknown".to_owned(),
        },
        404 => Error::NotFound,
        429 => Error::RateLimited {
            retry_after: block.unwrap_or(Duration::from_secs(60)),
        },
        s @ 300..=399 => Error::Redirect(s),
        s @ 500..=599 => Error::Server(s),
        s => Error::Rejected {
            status: s,
            reason: server_reason(body),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rules() {
        assert!(validate_base("https://lichess.org").is_ok());
        assert!(validate_base("http://127.0.0.1:8080").is_ok());
        assert!(validate_base("http://localhost:1").is_ok());
        assert!(validate_base("http://[::1]:9").is_ok());
        for bad in [
            "http://lichess.org",
            "http://192.168.0.2:80",
            "ftp://127.0.0.1",
            "https://user:pw@lichess.org",
            "https://lichess.org/?token=x",
            "https://lichess.org/prefix",
            "not a url",
            "",
        ] {
            assert_eq!(validate_base(bad).err(), Some(Error::InsecureUrl), "{bad}");
        }
    }

    #[test]
    fn id_user_and_uci_validation() {
        assert!(valid_game_id("abcd1234"));
        assert!(valid_game_id("abcd1234wxyz"));
        assert!(!valid_game_id("abc"));
        assert!(!valid_game_id("abcd/234"));
        assert!(!valid_game_id("../../etc"));
        assert!(valid_username("Some_User-1"));
        assert!(!valid_username("a"));
        assert!(!valid_username("bad name"));
        assert!(!valid_username("x/../y"));
        assert!(valid_uci("e2e4"));
        assert!(valid_uci("e7e8q"));
        assert!(!valid_uci("E2E4"));
        assert!(!valid_uci("e2e9"));
        assert!(!valid_uci("e7e8k"));
        assert!(!valid_uci("e2"));
    }

    #[test]
    fn status_mapping() {
        let e = |s: u16, b: &[u8]| status_error(StatusCode::from_u16(s).expect("status"), None, b);
        assert_eq!(e(401, b""), Error::Unauthorized);
        assert_eq!(e(404, b""), Error::NotFound);
        assert!(matches!(e(403, b""), Error::ScopeMissing { .. }));
        assert_eq!(e(502, b""), Error::Server(502));
        assert_eq!(e(302, b""), Error::Redirect(302));
        assert_eq!(
            e(400, br#"{"error":"Not your turn"}"#),
            Error::Rejected {
                status: 400,
                reason: "Not your turn".to_owned()
            }
        );
        assert_eq!(
            e(400, b"<html>"),
            Error::Rejected {
                status: 400,
                reason: String::new()
            }
        );
        assert_eq!(
            status_error(
                StatusCode::TOO_MANY_REQUESTS,
                Some(Duration::from_secs(60)),
                b""
            ),
            Error::RateLimited {
                retry_after: Duration::from_secs(60)
            }
        );
    }

    #[test]
    fn config_debug_hides_the_token() {
        let cfg = ClientConfig {
            token: Token::new("lip_TOPSECRET").ok(),
            ..ClientConfig::default()
        };
        assert!(!format!("{cfg:?}").contains("TOPSECRET"));
        let client = Client::new(cfg).expect("client");
        assert!(!format!("{client:?}").contains("TOPSECRET"));
        assert!(client.is_authenticated());
    }

    #[test]
    fn insecure_base_is_refused() {
        let cfg = ClientConfig {
            base_url: "http://example.com".to_owned(),
            ..ClientConfig::default()
        };
        assert_eq!(Client::new(cfg).err(), Some(Error::InsecureUrl));
    }

    #[test]
    fn urls_encode_segments_and_keep_token_out() {
        let client = Client::new(ClientConfig {
            token: Token::new("lip_TOPSECRET").ok(),
            ..ClientConfig::default()
        })
        .expect("client");
        let url = client
            .url(&["api", "board", "game", "abcd1234", "move", "e2e4"])
            .expect("url");
        assert_eq!(
            url.as_str(),
            "https://lichess.org/api/board/game/abcd1234/move/e2e4"
        );
        let weird = client.url(&["api", "x/y"]).expect("url");
        assert_eq!(weird.path(), "/api/x%2Fy");
        assert!(!url.as_str().contains("TOPSECRET"));
    }

    #[test]
    fn game_refs_from_links_and_ids() {
        for (input, want) in [
            ("abcd1234", Some("abcd1234")),
            ("  abcd1234wxyz ", Some("abcd1234")),
            ("https://lichess.org/abcd1234", Some("abcd1234")),
            ("http://lichess.org/abcd1234/black", Some("abcd1234")),
            ("lichess.org/abcd1234wxyz?x=1#12", Some("abcd1234")),
            ("https://www.lichess.org/AbCd1234", Some("AbCd1234")),
            ("https://evil.example/abcd1234", None),
            ("https://notlichess.org/abcd1234", None),
            ("https://lichess.org/", None),
            ("https://lichess.org/tv", None),
            ("https://lichess.org/tournament/xyz", None),
            ("https://lichess.org/training/daily", None),
            ("https://lichess.org/analysis", None),
            ("lichess.org/abcd1234/white", Some("abcd1234")),
            ("lichess.org/abcd1234/other", None),
            ("abcd12345", None),
            ("abcd1234wxy", None),
            ("abc", None),
            ("abcd 1234", None),
            ("../../abcd1234", None),
            ("", None),
        ] {
            assert_eq!(parse_game_ref(input).as_deref(), want, "{input}");
        }
    }
}
