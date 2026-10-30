//! What the online play asks the outside world to do, and the hub that does
//! it (spec M8-3, D1310).
//!
//! [`OnlineCommand`] is data (an [`Effect`](crate::Effect) carries it);
//! [`perform_online`] runs it against an [`OnlineHub`], which owns the
//! client, the tasks that read the streams and the stores of the token. The
//! token is **never** in a command, a message, a state or a `Debug`: it is
//! handed to [`OnlineHub::stage_token`] once and stays inside the hub
//! (D1307).
//!
//! Everything the streams say reaches the front-end through
//! [`OnlineHub::events`], the way the engine events do.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mychess_online::models::{ChallengeParams, ColorChoice, SeekParams};
use mychess_online::oauth::OAuthFlow;
use mychess_online::{
    Client, ClientConfig, FileStore, GameAction, KeyringStore, StreamHandle, StreamMessage, Token,
    TokenStore,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::assist;
use super::message::{
    AccountView, ActionDone, ActionFailure, ActionKind, ChallengeRequest, OnlineColor, OnlineError,
    OnlineMessage, OnlineVariant, RetryKind, SeekRequest, StoreChoice, StreamKind, StreamUpdate,
    TokenSource,
};
use super::watch::ParsedExport;
use crate::message::Message;

/// How many events wait for the front-end before the tasks that read the
/// streams wait too (D1310, D1371): a slow consumer slows the reading, it
/// does not grow the memory.
const EVENT_QUEUE: usize = 256;

/// One thing for the online hub to do.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OnlineCommand {
    /// Check the token, keep it if asked, and open the event stream.
    Connect {
        /// The epoch of the attempt.
        epoch: u64,
        /// The server.
        base_url: String,
        /// Where the token comes from.
        source: TokenSource,
        /// Where to keep a staged token.
        store: StoreChoice,
    },
    /// Close everything and delete the token of `login` from the stores.
    Disconnect {
        /// The account.
        login: String,
    },
    /// Close everything without touching the stores (the program quits).
    Shutdown,
    /// Open the stream of a game (closing a seek or challenge stream).
    OpenGame {
        /// The epoch of the stream.
        epoch: u64,
        /// The game.
        id: String,
    },
    /// Close the stream of the game.
    CloseGame,
    /// Open a seek.
    Seek {
        /// The epoch of the seek.
        epoch: u64,
        /// What to look for.
        request: SeekRequest,
    },
    /// Close the seek.
    CancelSeek,
    /// Send a challenge and keep it open.
    Challenge {
        /// The epoch of the stream.
        epoch: u64,
        /// The challenge.
        request: ChallengeRequest,
    },
    /// Close the stream of the challenge sent and, if its id is known,
    /// cancel it at the server.
    CancelChallenge {
        /// The id of the challenge.
        id: Option<String>,
    },
    /// Accept a challenge received.
    AcceptChallenge {
        /// The challenge.
        id: String,
    },
    /// Decline a challenge received.
    DeclineChallenge {
        /// The challenge.
        id: String,
        /// The reason key (`generic`, `variant`, `noBot`...).
        reason: String,
    },
    /// Play a move.
    Move {
        /// The game.
        id: String,
        /// The move in UCI.
        uci: String,
        /// Offer a draw with it.
        offer_draw: bool,
        /// The ply it reaches.
        ply: usize,
    },
    /// Resign, abort, draw, takeback or claim (everything but a move).
    Game {
        /// The game.
        id: String,
        /// What to do.
        action: ActionKind,
    },
    /// Read the marker of a game in progress.
    ReadMarker {
        /// The directory of the marker.
        dir: PathBuf,
    },
    /// Write the marker of a game in progress.
    WriteMarker {
        /// The number of the marker command: a command older than one
        /// already done is dropped (D1372).
        seq: u64,
        /// The directory of the marker.
        dir: PathBuf,
        /// The game.
        game_id: String,
    },
    /// Remove the marker.
    RemoveMarker {
        /// The number of the marker command (see `WriteMarker`).
        seq: u64,
        /// The directory of the marker.
        dir: PathBuf,
    },
    /// Log in through the browser: opens the authorization page, waits for
    /// the callback on a loopback port and stages the token (D3005).
    OAuthLogin {
        /// The epoch of the attempt.
        epoch: u64,
        /// The server.
        base_url: String,
        /// Where the token will be kept once it works.
        store: StoreChoice,
    },
    /// Give up the login in the browser (closes the callback port).
    CancelOAuth,
    /// Open the public stream of a game of other players (D3000).
    OpenWatch {
        /// The epoch of the watch.
        epoch: u64,
        /// The server.
        base_url: String,
        /// The game.
        id: String,
    },
    /// Download the exported PGN of the watched game (D3003).
    FetchPgn {
        /// The epoch of the watch.
        epoch: u64,
        /// The server.
        base_url: String,
        /// The game.
        id: String,
    },
    /// Close the stream of the watched game.
    CloseWatch,
    /// Wait, then say the grace period of the marker ended.
    Grace {
        /// The number of the timer.
        generation: u64,
        /// How long to wait.
        delay: Duration,
    },
    /// Wait, then say a retry timer ended (D1363, D1366).
    Retry {
        /// What waits.
        kind: RetryKind,
        /// The number of the timer.
        generation: u64,
        /// How long to wait.
        delay: Duration,
    },
}

/// The events of the hub, in the order they happened. The front-end wraps
/// each in [`Message::Online`].
#[derive(Debug)]
pub struct OnlineEvents {
    rx: mpsc::Receiver<OnlineMessage>,
}

impl OnlineEvents {
    /// The next event; `None` when the hub is gone.
    pub async fn next(&mut self) -> Option<Message> {
        self.rx.recv().await.map(Message::Online)
    }

    /// The next event if one is queued.
    pub fn try_next(&mut self) -> Option<Message> {
        self.rx.try_recv().ok().map(Message::Online)
    }
}

#[derive(Default)]
struct Tasks {
    events: Option<JoinHandle<()>>,
    game: Option<JoinHandle<()>>,
    seek: Option<JoinHandle<()>>,
    challenge: Option<JoinHandle<()>>,
    oauth: Option<JoinHandle<()>>,
    watch: Option<JoinHandle<()>>,
}

/// Changes the configuration of the client before it is built (tests use it
/// to shorten the waits).
type Tuning = Arc<dyn Fn(&mut ClientConfig) + Send + Sync>;

struct Inner {
    tx: mpsc::Sender<OnlineMessage>,
    rx: Mutex<Option<mpsc::Receiver<OnlineMessage>>>,
    client: Mutex<Option<Client>>,
    staged: Mutex<Option<Token>>,
    /// The number of the last marker command done (D1372).
    marker_seq: Arc<Mutex<u64>>,
    tasks: Mutex<Tasks>,
    keyring: Arc<dyn TokenStore>,
    file: Option<Arc<dyn TokenStore>>,
    tuning: Option<Tuning>,
    /// A client without token for the public endpoints (D3000).
    public: Mutex<Option<Client>>,
    /// Opens a page in the browser (tests replace it).
    opener: Mutex<Opener>,
}

/// Opens an address in the browser; `false` when it could not.
type Opener = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// The client, its streams and the token stores. Cheap to clone; clones
/// share everything.
#[derive(Clone)]
pub struct OnlineHub {
    inner: Arc<Inner>,
}

impl fmt::Debug for OnlineHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Nothing about the token or the client is printed.
        f.debug_struct("OnlineHub").finish_non_exhaustive()
    }
}

impl PartialEq for OnlineHub {
    fn eq(&self, other: &OnlineHub) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for OnlineHub {}

impl Hash for OnlineHub {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.inner).hash(state);
    }
}

impl Default for OnlineHub {
    fn default() -> OnlineHub {
        OnlineHub::new()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn abort(slot: &mut Option<JoinHandle<()>>) {
    if let Some(task) = slot.take() {
        task.abort();
    }
}

impl OnlineHub {
    /// A hub that keeps tokens in the system keychain and, when the user
    /// consented, in the 0600 file of the data directory.
    pub fn new() -> OnlineHub {
        let file = FileStore::in_data_dir().map(|s| Arc::new(s) as Arc<dyn TokenStore>);
        OnlineHub::with_stores(Arc::new(KeyringStore::new()), file, None)
    }

    /// A hub with the given stores (tests use memory stores) and, optionally,
    /// a change to apply to the configuration of the client.
    pub fn with_stores(
        keyring: Arc<dyn TokenStore>,
        file: Option<Arc<dyn TokenStore>>,
        tuning: Option<Tuning>,
    ) -> OnlineHub {
        let (tx, rx) = mpsc::channel(EVENT_QUEUE);
        OnlineHub {
            inner: Arc::new(Inner {
                tx,
                rx: Mutex::new(Some(rx)),
                client: Mutex::new(None),
                staged: Mutex::new(None),
                marker_seq: Arc::new(Mutex::new(0)),
                tasks: Mutex::new(Tasks::default()),
                keyring,
                file,
                tuning,
                public: Mutex::new(None),
                opener: Mutex::new(Arc::new(super::browser::open_url)),
            }),
        }
    }

    /// Replaces the function that opens the browser (the tests play the
    /// browser with it).
    pub fn set_opener(&self, opener: impl Fn(&str) -> bool + Send + Sync + 'static) {
        *lock(&self.inner.opener) = Arc::new(opener);
    }

    /// A token-less client for `base_url`, reused while the address is the same.
    fn public_client(&self, base_url: &str) -> Result<Client, OnlineError> {
        if !super::prefs::base_url_is_acceptable(base_url) {
            return Err(OnlineError::InsecureUrl);
        }
        let mut slot = lock(&self.inner.public);
        if let Some(client) = slot
            .as_ref()
            .filter(|c| c.base_url().trim_end_matches('/') == base_url.trim_end_matches('/'))
        {
            return Ok(client.clone());
        }
        let mut config = ClientConfig {
            base_url: base_url.to_owned(),
            token: None,
            ..ClientConfig::default()
        };
        if let Some(tuning) = &self.inner.tuning {
            tuning(&mut config);
        }
        let client = Client::new(config).map_err(|e| OnlineError::from(&e))?;
        *slot = Some(client.clone());
        Ok(client)
    }

    /// Takes the receiver of the events: the first call gets it.
    pub fn events(&self) -> Option<OnlineEvents> {
        lock(&self.inner.rx).take().map(|rx| OnlineEvents { rx })
    }

    /// Keeps the text of a token for the next [`OnlineCommand::Connect`]
    /// with [`TokenSource::Staged`]. The text never leaves the hub.
    ///
    /// # Errors
    ///
    /// [`OnlineError::BadToken`] when the text is not shaped like a token.
    pub fn stage_token(&self, text: &str) -> Result<(), OnlineError> {
        let token = Token::new(text).map_err(|_| OnlineError::BadToken)?;
        *lock(&self.inner.staged) = Some(token);
        Ok(())
    }

    /// Forgets a staged token.
    pub fn clear_staged(&self) {
        *lock(&self.inner.staged) = None;
    }

    async fn send(&self, message: OnlineMessage) {
        // The receiver is gone when the program ends: nothing to tell.
        self.inner.tx.send(message).await.ok();
    }

    fn client(&self) -> Option<Client> {
        lock(&self.inner.client).clone()
    }

    fn close_all(&self) {
        let mut tasks = lock(&self.inner.tasks);
        abort(&mut tasks.events);
        abort(&mut tasks.game);
        abort(&mut tasks.seek);
        abort(&mut tasks.challenge);
        abort(&mut tasks.oauth);
        drop(tasks);
        *lock(&self.inner.client) = None;
    }

    /// Forwards the messages of a stream as updates stamped with `epoch`.
    fn forward<T: Send + 'static>(
        &self,
        mut handle: StreamHandle<T>,
        epoch: u64,
        wrap: impl Fn(StreamMessage<T>) -> StreamKind + Send + 'static,
    ) -> Option<JoinHandle<()>> {
        let runtime = tokio::runtime::Handle::try_current().ok()?;
        let tx = self.inner.tx.clone();
        Some(runtime.spawn(async move {
            while let Some(message) = handle.next().await {
                let update = StreamUpdate {
                    epoch,
                    at: Instant::now(),
                    kind: wrap(message),
                };
                if tx.send(OnlineMessage::Stream(update)).await.is_err() {
                    break;
                }
            }
        }))
    }

    async fn update(&self, epoch: u64, kind: StreamKind) {
        self.send(OnlineMessage::Stream(StreamUpdate {
            epoch,
            at: Instant::now(),
            kind,
        }))
        .await;
    }
}

fn account_view(info: &mychess_online::AccountInfo) -> AccountView {
    let account = &info.account;
    let mut ratings: Vec<(String, u32, bool)> = account
        .perfs
        .iter()
        .map(|(name, perf)| (name.clone(), perf.rating, perf.prov))
        .collect();
    ratings.sort();
    AccountView {
        login: account.username.clone(),
        id: account.id.to_lowercase(),
        title: account.title.clone().filter(|t| !t.is_empty()),
        ratings,
        can_challenge: info.can_challenge(),
    }
}

/// Longest wait for a token store (a locked keychain can ask the user and
/// never answer, D1374).
const STORE_TIMEOUT: Duration = Duration::from_secs(30);

/// Like [`blocking`], giving up after [`STORE_TIMEOUT`].
async fn blocking_store<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::time::timeout(STORE_TIMEOUT, blocking(work))
        .await
        .ok()
        .flatten()
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

async fn stored_token(hub: &OnlineHub, login: String) -> Result<Option<Token>, OnlineError> {
    let (keyring, file) = (hub.inner.keyring.clone(), hub.inner.file.clone());
    let found = blocking_store(move || {
        let from_keyring = keyring.load(&login);
        match from_keyring {
            Ok(Some(token)) => Ok(Some(token)),
            other => match file {
                Some(file) => match file.load(&login) {
                    Ok(Some(token)) => Ok(Some(token)),
                    Ok(None) => other,
                    Err(e) => Err(e),
                },
                None => other,
            },
        }
    })
    .await;
    match found {
        Some(Ok(token)) => Ok(token),
        _ => Err(OnlineError::TokenStore),
    }
}

async fn keep_token(
    hub: &OnlineHub,
    login: String,
    token: Token,
    choice: StoreChoice,
) -> Result<(), OnlineError> {
    let store = match choice {
        StoreChoice::Keyring => Some(hub.inner.keyring.clone()),
        StoreChoice::File => Some(hub.inner.file.clone().ok_or(OnlineError::TokenStore)?),
        StoreChoice::Session => None,
    };
    let Some(store) = store else {
        return Ok(());
    };
    match blocking_store(move || store.save(&login, &token)).await {
        Some(Ok(())) => Ok(()),
        _ => Err(OnlineError::TokenStore),
    }
}

async fn connect(
    hub: &OnlineHub,
    base_url: String,
    source: TokenSource,
    store: StoreChoice,
) -> Result<(AccountView, Client), OnlineError> {
    if !super::prefs::base_url_is_acceptable(&base_url) {
        return Err(OnlineError::InsecureUrl);
    }
    let (token, staged) = match source {
        TokenSource::Staged => (lock(&hub.inner.staged).take(), true),
        TokenSource::Stored { login } => {
            if !super::prefs::login_is_valid(&login) {
                return Err(OnlineError::NoToken);
            }
            (stored_token(hub, login).await?, false)
        }
    };
    let token = token.ok_or(OnlineError::NoToken)?;
    let keep = token.duplicate();
    let mut config = ClientConfig {
        base_url,
        token: Some(token),
        ..ClientConfig::default()
    };
    if let Some(tuning) = &hub.inner.tuning {
        tuning(&mut config);
    }
    let client = Client::new(config).map_err(|e| OnlineError::from(&e))?;
    let info = match client.validate_login().await {
        Ok(info) => info,
        Err(e) => {
            // Wrong shape or refused: a staged token is not worth keeping.
            return Err(OnlineError::from(&e));
        }
    };
    let view = account_view(&info);
    if staged && let Err(error) = keep_token(hub, view.login.clone(), keep.duplicate(), store).await
    {
        // The user may pick another place without typing it again.
        *lock(&hub.inner.staged) = Some(keep);
        return Err(error);
    }
    Ok((view, client))
}

fn game_action(kind: ActionKind) -> Option<GameAction<'static>> {
    Some(match kind {
        ActionKind::Resign => GameAction::Resign,
        ActionKind::Abort => GameAction::Abort,
        ActionKind::DrawYes => GameAction::Draw(true),
        ActionKind::DrawNo => GameAction::Draw(false),
        ActionKind::TakebackYes | ActionKind::TakebackPropose => GameAction::Takeback(true),
        ActionKind::TakebackNo => GameAction::Takeback(false),
        ActionKind::ClaimDraw => GameAction::ClaimDraw,
        ActionKind::ClaimVictory => GameAction::ClaimVictory,
        _ => return None,
    })
}

/// The X-FEN of Chess960 start `n` (none outside 0..=959).
fn chess960_fen(n: u16) -> Option<String> {
    mychess_core::Position::chess960_start(n)
        .ok()
        .map(|p| p.fen())
}

fn color_choice(color: OnlineColor) -> ColorChoice {
    match color {
        OnlineColor::Random => ColorChoice::Random,
        OnlineColor::White => ColorChoice::White,
        OnlineColor::Black => ColorChoice::Black,
    }
}

fn done(
    action: ActionKind,
    id: Option<String>,
    ply: Option<usize>,
    result: Result<(), mychess_online::Error>,
) -> OnlineMessage {
    OnlineMessage::Done(ActionDone {
        action,
        id,
        ply,
        result: result.map_err(|e| ActionFailure::from_error(&e)),
        at: Instant::now(),
    })
}

fn no_client() -> mychess_online::Error {
    mychess_online::Error::Closed
}

/// Runs one command; the answer is the message to feed back to `update`.
/// Most answers are [`OnlineMessage::Ack`]: what the streams say comes
/// through [`OnlineHub::events`].
///
/// Must run inside a tokio runtime.
// One arm per command: a table, not logic to split.
#[allow(clippy::too_many_lines)]
pub(crate) async fn perform_online(command: OnlineCommand, hub: &OnlineHub) -> OnlineMessage {
    match command {
        OnlineCommand::Connect {
            epoch,
            base_url,
            source,
            store,
        } => {
            match connect(hub, base_url, source, store).await {
                Ok((view, client)) => {
                    // Through the channel, before the stream is read, so
                    // `update` sees `Connected` before anything the stream
                    // says.
                    hub.send(OnlineMessage::Connected {
                        epoch,
                        result: Ok(view),
                    })
                    .await;
                    match client.open_event_stream() {
                        Ok(handle) => {
                            let task = hub.forward(handle, epoch, StreamKind::Account);
                            *lock(&hub.inner.client) = Some(client);
                            let mut tasks = lock(&hub.inner.tasks);
                            abort(&mut tasks.events);
                            tasks.events = task;
                        }
                        Err(error) => {
                            hub.update(epoch, StreamKind::Account(StreamMessage::Failed(error)))
                                .await;
                        }
                    }
                }
                Err(error) => {
                    hub.send(OnlineMessage::Connected {
                        epoch,
                        result: Err(error),
                    })
                    .await;
                }
            }
            OnlineMessage::Ack
        }
        OnlineCommand::Disconnect { login } => {
            hub.close_all();
            hub.clear_staged();
            let (keyring, file) = (hub.inner.keyring.clone(), hub.inner.file.clone());
            let result = blocking(move || {
                let first = keyring.delete(&login);
                let second = file.map_or(Ok(()), |f| f.delete(&login));
                first.and(second)
            })
            .await
            .unwrap_or(Err(mychess_online::Error::Closed));
            done(ActionKind::Disconnect, None, None, result)
        }
        OnlineCommand::Shutdown => {
            hub.close_all();
            OnlineMessage::Ack
        }
        OnlineCommand::OpenGame { epoch, id } => {
            let Some(client) = hub.client() else {
                hub.update(
                    epoch,
                    StreamKind::Game {
                        id,
                        message: StreamMessage::Failed(no_client()),
                    },
                )
                .await;
                return OnlineMessage::Ack;
            };
            // The guard is gone before anything is awaited.
            let failed = {
                let mut tasks = lock(&hub.inner.tasks);
                abort(&mut tasks.seek);
                abort(&mut tasks.challenge);
                abort(&mut tasks.game);
                match client.open_game_stream(&id) {
                    Ok(handle) => {
                        let game_id = id.clone();
                        tasks.game = hub.forward(handle, epoch, move |message| StreamKind::Game {
                            id: game_id.clone(),
                            message,
                        });
                        None
                    }
                    Err(error) => Some(error),
                }
            };
            if let Some(error) = failed {
                hub.update(
                    epoch,
                    StreamKind::Game {
                        id,
                        message: StreamMessage::Failed(error),
                    },
                )
                .await;
            }
            OnlineMessage::Ack
        }
        OnlineCommand::CloseGame => {
            abort(&mut lock(&hub.inner.tasks).game);
            OnlineMessage::Ack
        }
        OnlineCommand::Seek { epoch, request } => {
            let Some(client) = hub.client() else {
                hub.update(
                    epoch,
                    StreamKind::Seek(mychess_online::SeekOutcome::Failed(no_client())),
                )
                .await;
                return OnlineMessage::Ack;
            };
            let params = SeekParams {
                minutes: request.minutes,
                increment: request.increment,
                rated: request.rated,
                color: color_choice(request.color),
                rating_range: request.rating_range,
                chess960: request.variant.is_chess960(),
            };
            match client.seek(&params) {
                Ok(mut handle) => {
                    let tx = hub.inner.tx.clone();
                    let task = tokio::runtime::Handle::try_current().ok().map(|rt| {
                        rt.spawn(async move {
                            let outcome = handle.outcome().await;
                            tx.send(OnlineMessage::Stream(StreamUpdate {
                                epoch,
                                at: Instant::now(),
                                kind: StreamKind::Seek(outcome),
                            }))
                            .await
                            .ok();
                        })
                    });
                    let mut tasks = lock(&hub.inner.tasks);
                    abort(&mut tasks.seek);
                    tasks.seek = task;
                }
                Err(error) => {
                    hub.update(
                        epoch,
                        StreamKind::Seek(mychess_online::SeekOutcome::Failed(error)),
                    )
                    .await;
                }
            }
            OnlineMessage::Ack
        }
        OnlineCommand::CancelSeek => {
            abort(&mut lock(&hub.inner.tasks).seek);
            OnlineMessage::Ack
        }
        OnlineCommand::Challenge { epoch, request } => {
            let Some(client) = hub.client() else {
                hub.update(
                    epoch,
                    StreamKind::Challenge(StreamMessage::Failed(no_client())),
                )
                .await;
                return OnlineMessage::Ack;
            };
            let params = ChallengeParams {
                limit_secs: request.limit_secs,
                increment: request.increment,
                rated: request.rated,
                color: color_choice(request.color),
                chess960: request.variant.is_chess960(),
                initial_fen: match request.variant {
                    OnlineVariant::Chess960 { start: Some(n) } => chess960_fen(n),
                    _ => None,
                },
            };
            match client.challenge(&request.username, &params) {
                Ok(handle) => {
                    let task = hub.forward(handle, epoch, StreamKind::Challenge);
                    let mut tasks = lock(&hub.inner.tasks);
                    abort(&mut tasks.challenge);
                    tasks.challenge = task;
                }
                Err(error) => {
                    hub.update(epoch, StreamKind::Challenge(StreamMessage::Failed(error)))
                        .await;
                }
            }
            OnlineMessage::Ack
        }
        OnlineCommand::CancelChallenge { id } => {
            abort(&mut lock(&hub.inner.tasks).challenge);
            match (id, hub.client()) {
                (Some(id), Some(client)) => {
                    let result = client.cancel_challenge(&id).await;
                    done(ActionKind::CancelChallenge, Some(id), None, result)
                }
                _ => OnlineMessage::Ack,
            }
        }
        OnlineCommand::AcceptChallenge { id } => {
            let result = match hub.client() {
                Some(client) => client.accept_challenge(&id).await,
                None => Err(no_client()),
            };
            done(ActionKind::AcceptChallenge, Some(id), None, result)
        }
        OnlineCommand::DeclineChallenge { id, reason } => {
            let result = match hub.client() {
                Some(client) => client.decline_challenge(&id, &reason).await,
                None => Err(no_client()),
            };
            done(ActionKind::DeclineChallenge, Some(id), None, result)
        }
        OnlineCommand::Move {
            id,
            uci,
            offer_draw,
            ply,
        } => {
            let result = match hub.client() {
                Some(client) => client.play_move(&id, &uci, offer_draw).await,
                None => Err(no_client()),
            };
            done(ActionKind::Move, Some(id), Some(ply), result)
        }
        OnlineCommand::Game { id, action } => {
            let result = match (hub.client(), game_action(action)) {
                (Some(client), Some(act)) => client.game_action(&id, act).await,
                _ => Err(no_client()),
            };
            done(action, Some(id), None, result)
        }
        OnlineCommand::ReadMarker { dir } => {
            let info = blocking(move || assist::read_marker(&dir)).await.flatten();
            OnlineMessage::MarkerRead(info)
        }
        OnlineCommand::WriteMarker { seq, dir, game_id } => {
            let order = hub.inner.marker_seq.clone();
            let written = blocking(move || {
                // Held while the file changes, so two commands never mix.
                let mut last = lock(&order);
                if seq < *last {
                    return Ok(());
                }
                *last = seq;
                let (y, m, d, ..) = super::game_builder::utc_parts(unix_seconds());
                assist::write_marker(&dir, &game_id, &format!("{y:04}.{m:02}.{d:02}"))
            })
            .await;
            if !matches!(written, Some(Ok(()))) {
                tracing::warn!("could not write the marker of the online game");
            }
            OnlineMessage::Ack
        }
        OnlineCommand::RemoveMarker { seq, dir } => {
            let order = hub.inner.marker_seq.clone();
            let removed = blocking(move || {
                let mut last = lock(&order);
                if seq < *last {
                    return Ok(());
                }
                *last = seq;
                assist::remove_marker(&dir)
            })
            .await;
            if !matches!(removed, Some(Ok(()))) {
                tracing::warn!("could not remove the marker of the online game");
            }
            OnlineMessage::Ack
        }
        OnlineCommand::OAuthLogin {
            epoch,
            base_url,
            store,
        } => {
            let Ok(runtime) = tokio::runtime::Handle::try_current() else {
                hub.send(OnlineMessage::OAuthDone {
                    epoch,
                    result: Err(OnlineError::Other),
                    store,
                })
                .await;
                return OnlineMessage::Ack;
            };
            let task_hub = hub.clone();
            let task = runtime.spawn(async move {
                oauth_login(&task_hub, epoch, &base_url, store).await;
            });
            let mut tasks = lock(&hub.inner.tasks);
            abort(&mut tasks.oauth);
            tasks.oauth = Some(task);
            OnlineMessage::Ack
        }
        OnlineCommand::CancelOAuth => {
            abort(&mut lock(&hub.inner.tasks).oauth);
            hub.clear_staged();
            OnlineMessage::Ack
        }
        OnlineCommand::OpenWatch {
            epoch,
            base_url,
            id,
        } => {
            let opened = hub.public_client(&base_url).and_then(|client| {
                client
                    .open_watch_stream(&id)
                    .map_err(|e| OnlineError::from(&e))
            });
            match opened {
                Ok(handle) => {
                    let game_id = id.clone();
                    let task = hub.forward(handle, epoch, move |message| StreamKind::Watch {
                        id: game_id.clone(),
                        message,
                    });
                    let mut tasks = lock(&hub.inner.tasks);
                    abort(&mut tasks.watch);
                    tasks.watch = task;
                }
                Err(_) => {
                    hub.update(
                        epoch,
                        StreamKind::Watch {
                            id,
                            message: StreamMessage::Failed(mychess_online::Error::Closed),
                        },
                    )
                    .await;
                }
            }
            OnlineMessage::Ack
        }
        OnlineCommand::FetchPgn {
            epoch,
            base_url,
            id,
        } => {
            let result = match hub.public_client(&base_url) {
                Ok(client) => match client.export_pgn(&id).await {
                    // The reading of up to 4 MiB of PGN leaves the async
                    // worker and `update` (D3117).
                    Ok(bytes) => tokio::task::spawn_blocking(move || {
                        ParsedExport::parse(&String::from_utf8_lossy(&bytes))
                    })
                    .await
                    .ok()
                    .flatten()
                    .ok_or(OnlineError::Protocol),
                    Err(e) => Err(OnlineError::from(&e)),
                },
                Err(e) => Err(e),
            };
            OnlineMessage::WatchPgn { epoch, result }
        }
        OnlineCommand::CloseWatch => {
            abort(&mut lock(&hub.inner.tasks).watch);
            OnlineMessage::Ack
        }
        OnlineCommand::Grace { generation, delay } => {
            tokio::time::sleep(delay).await;
            OnlineMessage::MarkerGrace(generation)
        }
        OnlineCommand::Retry {
            kind,
            generation,
            delay,
        } => {
            tokio::time::sleep(delay).await;
            OnlineMessage::Retry { kind, generation }
        }
    }
}

/// The login through the browser: authorization page, wait for the
/// callback, code exchange (D3005). The token stays in the hub.
async fn oauth_login(hub: &OnlineHub, epoch: u64, base_url: &str, store: StoreChoice) {
    let fail = |error: OnlineError| OnlineMessage::OAuthDone {
        epoch,
        result: Err(error),
        store,
    };
    let flow = match OAuthFlow::start(base_url).await {
        Ok(flow) => flow,
        Err(e) => return hub.send(fail(OnlineError::from(&e))).await,
    };
    let url = match flow.authorize_url() {
        Ok(url) => url,
        Err(e) => return hub.send(fail(OnlineError::from(&e))).await,
    };
    let opener = lock(&hub.inner.opener).clone();
    let page = url.clone();
    let opened = blocking(move || opener(&page)).await.unwrap_or(false);
    hub.send(OnlineMessage::OAuthUrl { epoch, url, opened })
        .await;
    match flow.finish(mychess_online::oauth::DEFAULT_WAIT).await {
        Ok(token) => {
            *lock(&hub.inner.staged) = Some(token);
            hub.send(OnlineMessage::OAuthDone {
                epoch,
                result: Ok(()),
                store,
            })
            .await;
        }
        Err(e) => hub.send(fail(OnlineError::from(&e))).await,
    }
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
